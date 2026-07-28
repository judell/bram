#!/usr/bin/env python3
"""XMLUI MCP struggle correlator — time as the join axis.

The demand miner (scripts/xmlui-mcp-demand-miner.py) nominates topics. This convicts
them: it puts two timestamped streams on ONE clock —

  * the MCP query log  — what I reached for, and how thin/off-target the
    returns were (result_count);
  * our transcripts    — the intent (in the query text itself) and the
    frustration (in your turns).

Aligned on time, a burst of rephrased same-topic queries returning 60-120
"matches" that resolve nothing, landing next to a "going around in circles"
turn, is a provable "we were ill-served here" moment — with receipts. The
canonical example is the 2026-07-26 scroll spiral that the pin-a-toolbar
how-to then closed.

Read-only, stdlib only. Never writes.

  --discover            scan all history, rank struggle windows
  --window START END    interleaved timeline for an explicit ISO range
  --topic "a b c"       windows whose MCP queries hit these terms

Timestamps: MCP analytics are ISO with a tz offset; Claude transcripts are
ISO-Z. Both are parsed to epoch seconds for merging, then rendered in the
analytics' local offset so a single wall clock reads naturally.

Scope note: transcripts default to this project's Claude sessions. Codex
sessions live elsewhere and are a follow-up (--transcripts takes any dir of
*.jsonl with {type,timestamp,message.content}). The analytics is GLOBAL
(every project on this machine), so a window may show MCP queries from work
unrelated to the transcript in view — the time overlap is the filter.
"""

import argparse
import collections
import glob
import json
import os
import re
import sys

DEFAULT_ANALYTICS = os.path.expanduser(
    "~/Library/Caches/xmlui/xmlui-mcp/xmlui-mcp-analytics.json"
)
DEFAULT_TRANSCRIPTS = os.path.expanduser(
    "~/.claude/projects/-Users-jonudell-bram"
)

# Tools that emit a paired `search_query` record (with result_count); we take
# the search_query for those and the tool_invocation for everything else, so a
# single search shows once.
SEARCH_TOOLS = {"xmlui_search", "xmlui_search_howto", "xmlui_examples",
                "xmlui_pattern"}

# Struggle markers — DERIVED from mining 1,190 authored turns, not intuition.
# Finding: explicit emotional venting is ~0% here (zero god's-sake / ugh / wtf);
# being ill-served shows up as dry struggle-STATE language. Rates below are of
# the authored corpus. The signature is "still" + a negative ("still no joy"),
# not an expletive. See docs / the conventions "Correlating…" note.
#   still+neg 8.8% · broken/stuck 3.6% · negation-open 2.1% · why-still 1.4%
# retry-ops ("relaunched/restarted") is 14% but almost all NEUTRAL dev cadence,
# so it is deliberately EXCLUDED — it only signals struggle when it repeats, and
# repetition is caught structurally (MCP thrash), not lexically.
STRUGGLE = re.compile(r"""
    \bno\s+joy\b
  | \bstill\s+(no\b|nothing|not\b|the\s+same|same\b|broken|failing|fails?\b
              |doesn'?t|won'?t|can'?t|isn'?t|hangs?\b|stuck|frozen|wedged?)
  | (doesn'?t|does\s+not|won'?t)\s+work | not\s+working
  | \b(broke|broken|busted|fails?\b|failing|hangs?\b|frozen|stuck|spinning
      |wedged?|going\s+(around\s+)?in\s+circles|round\s+and\s+round)\b
  | ^(no[.!,\s]|nope\b|not\s|wrong\b|nah\b|that'?s\s+(wrong|not\s+right))
  | \bwhy\s+(are\s+we\s+|is\s+it\s+|does\s+it\s+|do\s+we\s+)?still\b
  | for\s+god'?s\s+sake
""", re.I | re.X | re.M)

# Machine / injected turns that are NOT the user reacting. The transcript's
# `user` stream is heavily polluted by these; mining them tanks precision.
DROP_PREFIX = ("this session is being continued", "approved:", "drop:",
    "iterate:", "talk:", "skip-worklist:", "read this screenshot",
    "read and follow this bram turn", "caveat:", "<", "[request interrupted")
DROP_CONTAINS = ("[image", "<system-reminder>", "<command-name>",
    "outbound-turns", "end of bram turn", "<local-command-stdout>")

# Bram writes image/voice turns as a relay whose transcript text is just a path;
# the authored words live in these sidecars. Recover them so those turns (the
# "here's a screenshot of the broken thing" ones) aren't invisible.
SIDECAR_DIR = "resources/outbound-turns"

# Gap (seconds) that starts a new MCP cluster during --discover.
CLUSTER_GAP = 30 * 60
# Pad (seconds) around a cluster when looking for co-located chat.
CHAT_PAD = 20 * 60

STOP = frozenset("""a an the this that and or not to of in on with for use how i
my me we our you your it its component components example examples docs
documentation xmlui get set add show do want need can list""".split())


def demachine(text):
    """Collapse whitespace; return authored prose or None for machine turns."""
    t = " ".join(text.split())
    low = t.lower()
    if low.startswith("voice:"):  # Jon dictated; strip the transport marker
        t = t[6:].strip()
        low = t.lower()
    if not low.strip() or len(t) < 2:
        return None
    if low.startswith(DROP_PREFIX):
        return None
    if any(k in low for k in DROP_CONTAINS):
        return None
    return t


def parse_iso(s):
    """ISO-8601 (with 'Z' or +hh:mm offset) -> (epoch_seconds, tz_offset_sec).

    Stdlib only, no dateutil. Returns (None, 0) on anything unparseable.
    """
    if not s:
        return None, 0
    m = re.match(r"(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})"
                 r"(?:\.\d+)?(Z|[+-]\d{2}:?\d{2})?", s)
    if not m:
        return None, 0
    import datetime
    y, mo, d, hh, mm, ss = (int(x) for x in m.group(1, 2, 3, 4, 5, 6))
    dt = datetime.datetime(y, mo, d, hh, mm, ss)
    off = m.group(7)
    if off and off != "Z":
        sign = 1 if off[0] == "+" else -1
        offsec = sign * (int(off[1:3]) * 3600 + int(off[-2:]) * 60)
    else:
        offsec = 0
    epoch = int(dt.replace(tzinfo=datetime.timezone.utc).timestamp()) - offsec
    return epoch, offsec


def fmt(epoch, offsec):
    import datetime
    dt = datetime.datetime.fromtimestamp(epoch + offsec, datetime.timezone.utc)
    return dt.strftime("%Y-%m-%d %H:%M")


def terms(text):
    out = set()
    for raw in text.lower().replace("/", " ").replace("-", " ").split():
        tok = "".join(c for c in raw if c.isalnum())
        if len(tok) >= 3 and tok not in STOP:
            out.add(tok)
    return out


# ---------------------------------------------------------------------------
# Loaders

def load_mcp(path):
    """MCP events: dicts {epoch, off, tool, text, rc, component}."""
    out = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                d = json.loads(line)
            except ValueError:
                continue
            epoch, off = parse_iso(d.get("timestamp"))
            if epoch is None:
                continue
            typ = d.get("type")
            tool = d.get("tool_name", "?")
            if typ == "search_query":
                out.append(dict(epoch=epoch, off=off, tool=tool,
                                text=d.get("query", ""),
                                rc=d.get("result_count"), component=None))
            elif typ == "tool_invocation" and tool not in SEARCH_TOOLS:
                args = d.get("arguments") or {}
                comp = args.get("component")
                text = comp or args.get("path") or args.get("query") or ""
                if not text:
                    continue
                out.append(dict(epoch=epoch, off=off, tool=tool,
                                text=text, rc=None, component=comp))
    out.sort(key=lambda e: e["epoch"])
    return out


def load_chat(transcripts):
    """Authored user turns: dicts {epoch, off, text, struggle}.

    De-machined (see demachine) and augmented with the outbound-turn sidecars,
    which carry the authored text of image/voice turns the transcript hides
    behind a path reference. Sidecars are read relative to the CWD (the repo).
    """
    out = []
    for fp in glob.glob(os.path.join(transcripts, "*.jsonl")):
        try:
            fh = open(fp, encoding="utf-8")
        except OSError:
            continue
        with fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    d = json.loads(line)
                except ValueError:
                    continue
                if d.get("type") != "user" or d.get("isSidechain"):
                    continue
                msg = d.get("message")
                content = msg.get("content") if isinstance(msg, dict) else None
                if not isinstance(content, str):
                    continue  # tool_result turns are lists; skip
                text = demachine(content)
                if text is None:
                    continue
                epoch, off = parse_iso(d.get("timestamp"))
                if epoch is None:
                    continue
                out.append(dict(epoch=epoch, off=off, text=text,
                                struggle=bool(STRUGGLE.search(text))))
    # Recover the hidden image/voice turns from their sidecars.
    for fp in glob.glob(os.path.join(SIDECAR_DIR, "*.json")):
        try:
            d = json.load(open(fp, encoding="utf-8"))
        except (OSError, ValueError):
            continue
        text = demachine(str(d.get("text") or ""))
        if text is None:
            continue
        ms = d.get("createdAtMs")
        if not isinstance(ms, (int, float)):
            continue
        epoch = int(ms // 1000)
        # local offset unknown for sidecars; borrow the analytics offset at
        # render time — store 0 and let callers pass an offset for display.
        out.append(dict(epoch=epoch, off=0, text=text,
                        struggle=bool(STRUGGLE.search(text))))
    out.sort(key=lambda e: e["epoch"])
    return out


# ---------------------------------------------------------------------------
# Rendering

def render_window(mcp, chat, lo, hi, offsec, max_chat=400):
    events = []
    for e in mcp:
        if lo <= e["epoch"] <= hi:
            rc = f"rc={e['rc']}" if e["rc"] is not None else ""
            tag = "DOC " if e["component"] else "MCP "
            label = e["component"] or e["text"]
            events.append((e["epoch"], tag,
                           f"{e['tool']:22} {rc:8} {label[:74]}"))
    for c in chat:
        if lo <= c["epoch"] <= hi:
            mark = "CHAT*" if c["struggle"] else "CHAT "
            events.append((c["epoch"], mark, c["text"][:max_chat]))
    events.sort(key=lambda x: x[0])
    for ep, tag, body in events:
        print(f"{fmt(ep, offsec)}  {tag} {body}")


# ---------------------------------------------------------------------------
# Discover

def sessionize(mcp):
    clusters, cur = [], []
    for e in mcp:
        if cur and e["epoch"] - cur[-1]["epoch"] > CLUSTER_GAP:
            clusters.append(cur)
            cur = []
        cur.append(e)
    if cur:
        clusters.append(cur)
    return clusters


def score_cluster(cl, chat):
    lo, hi = cl[0]["epoch"], cl[-1]["epoch"]
    # repeated component docs (reaching for the same component >1x)
    comps = collections.Counter(e["component"] for e in cl if e["component"])
    repeated_docs = sum(n for n in comps.values() if n >= 2)
    # rephrased same-topic queries: queries sharing a hot term (asked >=3x)
    qs = [terms(e["text"]) for e in cl if e["rc"] is not None and e["text"]]
    topic = collections.Counter()
    for t in qs:
        topic.update(t)
    hot = {w for w, n in topic.items() if n >= 3}
    rephrase = sum(1 for t in qs if t & hot)
    # avg result_count (outcome-blind tell: high yet unresolved)
    rcs = [e["rc"] for e in cl if e["rc"] is not None]
    avg_rc = sum(rcs) / len(rcs) if rcs else 0
    # co-located struggle turns
    frusts = [c for c in chat
              if lo - CHAT_PAD <= c["epoch"] <= hi + CHAT_PAD and c["struggle"]]
    # Struggle co-location is the conviction signal; MCP thrash (same topic
    # rephrased) corroborates. repeated_docs is mostly broad-survey browsing —
    # kept as a faint tiebreaker, capped so a wide survey can't outscore a real
    # struggle window (the v1 failure: top windows were surveys, zero struggle).
    score = len(frusts) * 10 + min(rephrase, 8) * 0.5 + min(repeated_docs, 6) * 0.5
    return dict(lo=lo, hi=hi, off=cl[0]["off"], n=len(cl),
                repeated_docs=repeated_docs, rephrase=rephrase, avg_rc=avg_rc,
                frusts=frusts, score=score,
                hot=[w for w, _ in topic.most_common(6)], comps=comps)


def main():
    ap = argparse.ArgumentParser(
        prog="xmlui-mcp-correlate",
        description="Correlate MCP queries with transcript frustration on a "
                    "shared clock.")
    ap.add_argument("--analytics", default=DEFAULT_ANALYTICS)
    ap.add_argument("--transcripts", default=DEFAULT_TRANSCRIPTS)
    ap.add_argument("--discover", action="store_true",
                    help="rank struggle windows across all history.")
    ap.add_argument("--window", nargs=2, metavar=("START", "END"),
                    help="interleaved timeline for an ISO time range.")
    ap.add_argument("--topic", default=None,
                    help="show windows whose MCP queries hit these terms.")
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument("--min-score", type=float, default=10,
                    help="--discover: minimum score (default 10 = >=1 "
                         "co-located struggle turn).")
    args = ap.parse_args()

    if not os.path.exists(args.analytics):
        sys.exit(f"analytics not found: {args.analytics}")
    mcp = load_mcp(args.analytics)
    chat = load_chat(args.transcripts)
    print(f"loaded {len(mcp)} MCP events, {len(chat)} user turns "
          f"({sum(c['struggle'] for c in chat)} struggle-marked)\n")

    if args.window:
        lo, off = parse_iso(args.window[0])
        hi, _ = parse_iso(args.window[1])
        if lo is None or hi is None:
            sys.exit("could not parse --window bounds (ISO, e.g. "
                     "2026-07-26T17:00).")
        render_window(mcp, chat, lo, hi, off or (mcp[0]["off"] if mcp else 0))
        return

    if args.topic:
        want = terms(args.topic)
        hits = [e for e in mcp if terms(e["text"]) & want]
        if not hits:
            print("no MCP queries matched those terms.")
            return
        clusters = sessionize(hits)
        clusters.sort(key=lambda cl: -len(cl))
        for cl in clusters[:args.top]:
            lo, hi, off = cl[0]["epoch"], cl[-1]["epoch"], cl[0]["off"]
            print(f"=== window {fmt(lo, off)} .. {fmt(hi, off)}  "
                  f"({len(cl)} matching MCP queries) ===")
            render_window(mcp, chat, lo - CHAT_PAD, hi + CHAT_PAD, off)
            print()
        return

    # default / --discover
    clusters = sessionize(mcp)
    scored = [score_cluster(cl, chat) for cl in clusters]
    scored = [s for s in scored if s["score"] >= args.min_score]
    scored.sort(key=lambda s: -s["score"])
    print(f"{len(scored)} struggle windows at score >= {args.min_score} "
          f"(of {len(clusters)} MCP clusters)\n")
    for s in scored[:args.top]:
        fr = f", {len(s['frusts'])} struggle" if s["frusts"] else ""
        docs = ("; repeated docs: "
                + ", ".join(f"{c}x{n}" for c, n in s["comps"].items()
                            if n >= 2)) if s["repeated_docs"] else ""
        print(f"score {s['score']:5.1f}  {fmt(s['lo'], s['off'])} .. "
              f"{fmt(s['hi'], s['off'])}  "
              f"[{s['n']} q, avg rc {s['avg_rc']:.0f}{fr}]")
        print(f"           topic: {' '.join(s['hot'])}{docs}")
        for c in s["frusts"][:2]:
            print(f"           CHAT* {c['text'][:120]}")
    print("\nDrill into any window with:  "
          "python3 scripts/xmlui-mcp-correlate.py --window 'START' 'END'")


if __name__ == "__main__":
    main()
