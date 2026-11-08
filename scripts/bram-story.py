#!/usr/bin/env python3
"""Bram's story: the trace log told as short first-person episodes (read-only).

An experiment for judell/bram#473. The trace is a firehose; the lines that
mark something happening are a small minority. This reads them, groups them
into EPISODES, and prints each one as a sentence or two when it ends, in
Bram's voice. Fixed templates over trace fields: no model, no network, and
nothing is said that a line doesn't back (--cite shows which lines).

    python3 -I scripts/bram-story.py                     # last 60 minutes
    python3 -I scripts/bram-story.py --since 14:30       # HH:MM local today, or ISO
    python3 -I scripts/bram-story.py --since 2026-11-04T09:00 --until 2026-11-04T11:00
    python3 -I scripts/bram-story.py --follow --cite     # live, with sources
    python3 -I scripts/bram-story.py --root ~/other-project

Episodes (source lines in parentheses):
  launch   I started up                  [bram-trace] event=session-start
  turn     a turn: why it began, which   [turn-state] from/to, [turn-context]
           items it was claimed for,       op=set, [inflight-sentinel]
           how long, pauses for you,       write / clear / clear-at-turn-end
           claim released
  gate     Start / Start & commit /      [iframe] subkind=click target=gatebar-*,
           Commit / Drop, approval,        [auth-record] op=write, [turn-send],
           commit sha, unowned lines       [worklist-commit] staging/committed/
                                           commit-took-unowned
  stall    pane froze or a keystroke     [iframe] subkind=long-task (>=1000 ms
           lagged, bursts coalesced        while focused), subkind=input-latency
                                           (>=200 ms); optional "during" field
  refusal  guard denies (hook-events.log),  decided=deny reason=...,
           expired approvals, refused      [auth-record] op=refuse,
           queue saves                     [queue] op=save-refused
  push     a git push: branch and range,  [git-push] op=push-result
           or why it failed
  search   what you searched for on the  [iframe] subkind=search-query
           Search tab, and the hit count
  demo     a demo instance started or    [demo-instance] op=launch|stop
           stopped (scripts/demo_instance.py)
  quiet    --follow only: one line after 20 minutes with nothing to tell

Not available in the trace today (so not told): a 409 from the commit gate,
a reverted worklist write ([worklist-enforce] never reaches the trace), the
button behind a Drop's item ids.

Flags: --follow, --since, --until, --cite, --root (see above). Times are local.
"""
from __future__ import annotations

import argparse
import collections
import glob
import gzip
import heapq
import json
import os
import re
import sys
import textwrap
import time
from datetime import datetime, timedelta

LINE = re.compile(r"^\[(?P<ts>[^\]]+)\] \[(?P<cat>[^\]]+)\] (?P<rest>.*)$")
HOOK = re.compile(r"^(?P<ts>\S+) (?P<agent>\S+) (?P<event>\S+) (?P<tool>\S+) (?P<rest>.*)$")
WANT = {"turn-state", "turn-context", "inflight-sentinel", "auth-record", "turn-send",
        "worklist-commit", "iframe", "queue", "bram-trace", "hook-events", "git-push",
        "demo-instance"}
QUIET_SECS = 20 * 60
STALL_GAP = 30
LATENCY_MS, LONGTASK_MS = 200, 1000
START_COMMIT_SECS = 10 * 60
DURING = {"transcript-render": "rendering the Transcript",
          "worklist-refetch": "refetching the Worklist",
          "worklist-render": "rendering the Worklist"}


def epoch(ts):
    return datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()


def hhmm(t):
    return datetime.fromtimestamp(t).strftime("%H:%M")


def dur(s):
    s = int(round(s))
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{s // 60}m{s % 60:02d}s"
    return f"{s // 3600}h{s % 3600 // 60:02d}m"


def kv(rest, key):
    m = re.search(r"(?:^|\s)" + key + r"=(\S+)", rest)
    return m.group(1) if m else None


def ids_of(rest):
    m = re.search(r"ids=(\[[^\]]*\])", rest)
    try:
        return json.loads(m.group(1)) if m else []
    except ValueError:
        return []


def names(ids):
    if not ids:
        return "no item"
    if len(ids) <= 2:
        return " and ".join(ids)
    return f"{ids[0]}, {ids[1]} and {len(ids) - 2} more"


def parse_json_tail(rest):
    i = rest.find("{")
    try:
        return json.loads(rest[i:]) if i >= 0 else {}
    except ValueError:
        return {}


def parse(line, hook=False):
    """-> (t, cat, rest) for a line worth looking at, else None."""
    if hook:
        m = HOOK.match(line)
        if not m or "decided=deny" not in m["rest"]:
            return None
        cat, rest = "hook-events", f"tool={m['tool']} {m['rest']}"
    else:
        m = LINE.match(line)
        if not m or m["cat"] not in WANT:
            return None
        cat, rest = m["cat"], m["rest"]
        if cat == "iframe" and not re.match(r"subkind=(click|long-task|input-latency|heartbeat-drift|search-query) ", rest):
            return None
    try:
        return epoch(m["ts"]), cat, rest
    except ValueError:
        return None


class Ep:
    """An episode in progress: cites + a deadline after which it is told."""

    def __init__(self, kind, t):
        self.kind, self.t, self.deadline, self.cites, self.d = kind, t, t + 5, [], {}

    def cite(self, t, what):
        self.cites.append((t, what))


class Narrator:
    def __init__(self, out):
        self.out = out
        self.open: list[Ep] = []
        self.turn = None
        self.recent = collections.deque(maxlen=60)  # (t, "ctx"|"claim", kind, ids)
        self.drift = (0, None)
        self.last_hook = {}
        self.flush_now = None

    # ---- plumbing -------------------------------------------------------
    def say(self, ep, text):
        self.out(ep.t, text, ep.cites)

    def tick(self, now):
        for ep in [e for e in self.open if e.deadline <= now]:
            self.open.remove(ep)
            getattr(self, "end_" + ep.kind)(ep)

    def flush(self):
        # Episodes still open at the end are told now; end_gate reads
        # flush_now to tell "not yet" from "never did".
        self.flush_now = time.time()
        self.tick(float("inf"))
        self.flush_now = None

    def find(self, kind):
        for ep in reversed(self.open):
            if ep.kind == kind:
                return ep

    def feed(self, t, cat, rest):
        self.tick(t)
        sub = kv(rest, "subkind") if cat == "iframe" else None
        if cat == "turn-state":
            self.on_turn_state(t, rest)
        elif cat == "turn-context" and "op=set" in rest:
            self.recent.append((t, "ctx", kv(rest, "kind"), kv(rest, "ids")))
        elif cat == "inflight-sentinel":
            self.on_sentinel(t, rest)
        elif cat == "iframe" and sub == "click":
            self.on_click(t, rest)
        elif cat == "iframe" and sub == "heartbeat-drift":
            self.drift = (t, parse_json_tail(rest).get("focused"))
        elif cat == "iframe" and sub in ("long-task", "input-latency"):
            self.on_stall(t, sub, parse_json_tail(rest))
        elif cat == "iframe" and sub == "search-query":
            self.on_search(t, parse_json_tail(rest))
        elif cat == "auth-record":
            self.on_auth(t, rest)
        elif cat == "turn-send":
            self.on_send(t, rest)
        elif cat == "worklist-commit":
            self.on_commit(t, rest)
        elif cat == "git-push":
            self.on_push(t, rest)
        elif cat == "demo-instance":
            # log-records-demo-instances: scripts/demo_instance.py's launch
            # and stop, appended to this trace.
            # log-demo-restart-and-expanders: a stop is held 10 s; a launch
            # inside it makes one restart line. No scenario name in the
            # sentence (it's a session-made label).
            op, pid = kv(rest, "op"), kv(rest, "pid")
            ptxt = f" (PID {pid})" if pid else ""
            stop = getattr(self, "demo_stop", None)
            if op == "stop":
                ep = self.simple(t, "demo-instance stop", f"The demo instance stopped{ptxt}.")
                ep.deadline = t + 10
                self.demo_stop = (t, pid, ep)
            elif op == "launch":
                if stop and t - stop[0] <= 10 and stop[2] in self.open:
                    both = f" (PID {stop[1]} \u2192 {pid})" if stop[1] and pid else ptxt
                    stop[2].d["text"] = f"The demo instance restarted{both}."
                    stop[2].cite(t, "demo-instance launch")
                    self.demo_stop = None
                else:
                    self.simple(t, "demo-instance launch", f"A demo instance started{ptxt}.")
        elif cat == "hook-events":
            self.on_hook(t, rest)
        elif cat == "queue" and "op=save-refused" in rest:
            bm = re.search(r"base=Some\((\d+)", rest)
            self.simple(t, "queue", "I refused a queue save: it was written against version "
                        f"{bm[1] if bm else '?'} but the file was at {kv(rest, 'on_disk')}. "
                        "The writer has to re-read and try again.")
        elif cat == "bram-trace" and "event=session-start" in rest:
            self.simple(t, "bram-trace", f"I started up (pid {kv(rest, 'pid')}).")

    def simple(self, t, what, text):
        ep = Ep("simple", t)
        ep.cite(t, what)
        ep.d["text"] = text
        ep.deadline = t
        self.open.append(ep)
        return ep

    def end_simple(self, ep):
        self.say(ep, ep.d["text"])

    # ---- push (log-shows-start-commit-push-search) ----------------------
    # One [git-push] op=push-result line per `git push` the host runs
    # (issue-383); the range is read from git's own stderr.
    def on_push(self, t, rest):
        if kv(rest, "op") != "push-result":
            return
        branch, stage = kv(rest, "branch") or "the branch", kv(rest, "stage") or "initial"
        em = re.search(r'stderr="((?:[^"\\]|\\.)*)"', rest)
        err = em[1].replace("\\n", "\n").replace('\\"', '"') if em else ""
        rm = re.search(r"([0-9a-f]{7,40})\.\.([0-9a-f]{7,40})", err)
        if kv(rest, "ok") == "true":
            s = f"You pushed {branch}"
            if rm:
                s += f" ({rm[1][:8]}..{rm[2][:8]})"
            elif "up-to-date" in err.lower():
                s += "; there was nothing new to send"
            s += ", after a rebase onto what had been pushed meanwhile." if stage == "rebase" else "."
            self.simple(t, "git-push push-result", s)
        else:
            lines = [l.strip() for l in err.split("\n") if l.strip()]
            why = next((l for l in lines if re.match(r"^(error|fatal):", l)), lines[-1] if lines else "git gave no error text")
            self.simple(t, "git-push push-result", f"Your push of {branch} failed at the {stage} stage: {why}")

    # ---- search ---------------------------------------------------------
    # The Search tab's [iframe] subkind=search-query, one per bucket per
    # query. Buckets of one query land within a second; a query typed in
    # stages replaces the last within 4 s, so the final query is told.
    LABEL = {"session": "sessions", "commit": "commits", "issue": "issues", "worklist-history": "history"}

    def on_search(self, t, j):
        if j.get("op") != "results" or not j.get("q"):
            return
        ep = self.find("search")
        if ep and t - ep.t > 4:
            ep = None
        if not ep:
            ep = Ep("search", t)
            ep.d.update(q=j["q"], mode=j.get("mode"), hits={})
            self.open.append(ep)
        elif ep.d["q"] != j["q"]:
            ep.d.update(q=j["q"], mode=j.get("mode"), hits={})
            ep.t, ep.cites = t, []
        ep.d["hits"][j.get("bucket") or "?"] = int(j.get("hits") or 0)
        ep.cite(t, f"search-query {j.get('bucket')}")
        ep.deadline = t + 4

    def end_search(self, ep):
        d = ep.d
        total = sum(d["hits"].values())
        parts = [f"{n} {self.LABEL.get(b, b)}" for b, n in d["hits"].items() if n]
        q = d["q"] if len(d["q"]) <= 180 else d["q"][:177] + "…"
        s = f"You searched for “{q}”" + (f" ({d['mode']} mode)" if d.get("mode") not in (None, "and") else "")
        if total:
            s += f" and got {total} hit{'s' * (total != 1)}" + (f" ({', '.join(parts)})" if len(parts) > 1 else "") + "."
        else:
            s += " and found nothing."
        self.say(ep, s)

    # ---- turn -----------------------------------------------------------
    def on_sentinel(self, t, rest):
        op = kv(rest, "op")
        tr = self.turn
        if op == "write" and kv(rest, "kind"):
            self.recent.append((t, "claim", kv(rest, "kind"), ids_of(rest)))
            g = self.find("gate")
            if g and ids_of(rest):
                g.d["claimed"] = True
                g.cite(t, "inflight-sentinel write")
        if tr and op in ("clear-at-turn-end", "clear"):
            tr.d["released"] = ids_of(rest) or tr.d.get("released") or True
            tr.cite(t, f"inflight-sentinel {op}")

    def on_turn_state(self, t, rest):
        frm, to, menu = kv(rest, "from"), kv(rest, "to"), kv(rest, "menu")
        if to == "working" and frm != "working" and self.turn is None:
            ep = self.turn = Ep("turn", t)
            ep.deadline = float("inf")
            ep.cite(t, f"turn-state {frm}->working")
            ep.d["pauses"] = []
        elif self.turn is not None:
            tr = self.turn
            if to == "waiting-for-permission" and frm != to:
                tr.d["pauses"].append(menu or "a prompt")
                tr.cite(t, "turn-state waiting")
            elif frm in ("working", "waiting-for-permission") and to in ("finished", "idle"):
                tr.d["end"] = t
                tr.cite(t, f"turn-state ->{to}")
                tr.deadline = t + 3  # wait briefly for the claim release
                self.open.append(tr)
                self.turn = None

    def end_turn(self, ep):
        t0, t1 = ep.t, ep.d["end"]
        near = [r for r in self.recent if t0 - 15 <= r[0] <= t0 + 5]
        ctx = [r for r in near if r[1] == "ctx" and r[2] != "turn-end"]
        claims = [r for r in near if r[1] == "claim"]
        kind = ctx[-1][2] if ctx else (claims[-1][2] if claims else None)
        ids = claims[-1][3] if claims else []
        n = ctx[-1][3] if ctx else None
        why = {"general": "your chat message", "approved": "your approval",
               "iterate": "your feedback"}.get(kind)
        if why is None:
            why = "something I can't attribute (no turn-context line)"
        on = f" on {names(ids)}" if ids else (f" on {n} item(s)" if n not in (None, "0") else "")
        if t1 - t0 < 2 and not ids:
            return
        s = f"Claude took a turn for {why}{on} and finished in {dur(t1 - t0)}."
        if ep.d["pauses"]:
            k = len(ep.d["pauses"])
            s += f" It stopped {k} time{'s' * (k != 1)} to wait for you ({', '.join(sorted(set(ep.d['pauses'])))})."
        rel = ep.d.get("released")
        if rel:
            s += " I released the claim."
        elif ids:
            s += " I did not see the claim released, so the item may still read as running."
        if t1 - t0 > 900:
            s = s.replace("took a turn", "took a long turn")
        self.say(ep, s)

    # ---- gate -----------------------------------------------------------
    def on_click(self, t, rest):
        j = parse_json_tail(rest)
        tg = j.get("target", "")
        if j.get("op") != "act" or not tg.startswith("gatebar-"):
            return
        g = Ep("gate", t)
        g.d.update(target=tg[8:], count=j.get("count") or 0)
        # A Commit lands within a minute; a Start & commit first does the
        # work, so its commit can be many minutes away.
        g.deadline = t + (START_COMMIT_SECS if tg == "gatebar-start-commit" else 60 if tg == "gatebar-commit" else 8)
        g.cite(t, f"click {tg}")
        self.open.append(g)

    def gate(self, t):
        g = self.find("gate")
        return g if g and t - g.t < 60 else None

    def on_auth(self, t, rest):
        op, kind = kv(rest, "op"), kv(rest, "kind")
        if op == "write" and kind == "approved":
            g = self.gate(t)
            if g is None or g.d.get("ids"):
                g = Ep("gate", t)
                g.d.update(target=None, count=0)
                g.deadline = t + 8
                self.open.append(g)
            g.d.update(ids=ids_of(rest), recorded=True)
            g.deadline = max(g.deadline, t + 8)
            g.cite(t, "auth-record write")
        elif op == "write" and kind == "direct-edit":
            self.simple(t, "auth-record direct-edit", "You sent a turn that skips the Worklist "
                        "(skip-worklist: prefix), so I allowed direct edits with no item.")
        elif op == "refuse":
            m = re.search(r"expired \((\d+) ms old, TTL (\d+) ms\)", rest)
            what = kv(rest, "mutate_op") or "request"
            ids = ids_of(rest)
            if m:
                self.simple(t, "auth-record refuse",
                            f"I refused to {what} {names(ids)}: the approval was {dur(int(m[1]) / 1000)} old "
                            f"and they expire after {dur(int(m[2]) / 1000)}. That's the rule working; "
                            "it needs a fresh Start.")
            else:
                dm = re.search(r"detail=(.*?) ids=", rest)
                self.simple(t, "auth-record refuse", f"I refused to {what} {names(ids)} and gave this reason: "
                            f"{dm[1] if dm else 'none given'}. "
                            "I have no standard sentence for that, so it may be new.")

    def on_send(self, t, rest):
        g = self.gate(t)
        if g and re.search(r'head="(approved|drop):', rest) and t - g.t < 30:
            g.d["handed"] = True
            g.cite(t, "turn-send")

    def on_commit(self, t, rest):
        op = kv(rest, "op")
        g = self.find("gate")
        if op == "committed":
            if g and g.d.get("target") in ("commit", "start-commit", None):
                g.d["sha"] = kv(rest, "sha")
                g.deadline = t + 4
                g.cite(t, "worklist-commit committed")
            else:
                self.simple(t, "worklist-commit committed", f"A commit landed ({kv(rest, 'sha')}), "
                            "but I saw no gate click for it.")
        elif op == "staging" and g:
            g.d["files"] = kv(rest, "files")
            g.cite(t, "worklist-commit staging")
        elif op == "commit-took-unowned" and g and g.d.get("sha"):
            g.d.setdefault("unowned", []).append((kv(rest, "path"), kv(rest, "added"), kv(rest, "removed")))
            g.cite(t, "commit-took-unowned")

    def end_gate(self, ep):
        d = ep.d
        tg, ids = d.get("target"), d.get("ids") or []
        who = names(ids) if ids else (f"{d['count']} item{'s' * (d['count'] != 1)}" if d.get("count") else "an item")
        if tg is None:
            if not d.get("recorded"):
                return
            s = (f"An approval for {who} was recorded, but I saw no click on the gate bar for it"
                 f"{'; the turn was handed to Claude' if d.get('handed') else ''}.")
        elif tg == "drop":
            s = f"You clicked Drop on {who}. Feedback-less drops finish in the host, so there's no turn to tell about."
        else:
            label = {"start": "Start", "commit": "Commit", "start-commit": "Start & commit"}.get(tg, tg)
            # The click and, for a commit, what landed; the approval, claim
            # and hand-off are no longer recited (Jon, 2026-11-07).
            s = f"You clicked {label} on {who}."
            if tg in ("commit", "start-commit"):
                if d.get("sha"):
                    f = d.get("files")
                    s += f" I staged {f} file{'s' * (f != '1')} and committed {d['sha']}." if f else f" I committed {d['sha']}."
                    for p, a, r in d.get("unowned", []):
                        s += (f" That commit also took lines no item owned in {p} (+{a}/-{r}); "
                              "that's disclosed, not hidden, but worth a look.")
                elif self.flush_now is not None and self.flush_now < ep.deadline:
                    s += " The commit hasn't landed yet."
                elif tg == "commit":
                    s += " I did not see a commit land within a minute."
                else:
                    s += f" No commit had landed {dur(START_COMMIT_SECS)} later."
        self.say(ep, s)

    # ---- stall ----------------------------------------------------------
    def on_stall(self, t, sub, j):
        if sub == "input-latency":
            if (j.get("latencyMs") or 0) < LATENCY_MS or not j.get("hadFocus"):
                return
        else:
            focused = self.drift[1] if t - self.drift[0] < 3 else None
            if (j.get("ms") or 0) < LONGTASK_MS or focused is not True:
                return
        ep = self.find("stall")
        if ep is None:
            ep = Ep("stall", t)
            ep.d["ev"] = []
            self.open.append(ep)
        ep.d["ev"].append((t, sub, j))
        ep.deadline = t + STALL_GAP
        ep.cite(t, sub)

    def end_stall(self, ep):
        ev = ep.d["ev"]
        lt = [j for _, s, j in ev if s == "long-task"]
        il = [j for _, s, j in ev if s == "input-latency"]
        span = ev[-1][0] - ev[0][0]
        during = collections.Counter(DURING.get(j["during"], j["during"]) for _, _, j in ev if j.get("during"))
        if len(ev) == 1:
            j = ev[0][2]
            if il:
                s = (f"A {j.get('event', 'keystroke')} took {j['latencyMs']}ms to land on "
                     f"{j.get('route', 'the pane')}")
            else:
                s = f"The pane froze for {j['ms'] / 1000:.1f}s while you had it focused"
        else:
            parts = []
            if lt:
                parts.append(f"froze {len(lt)} time{'s' * (len(lt) != 1)}, worst "
                             f"{max(j['ms'] for j in lt) / 1000:.1f}s")
            if il:
                routes = collections.Counter(j.get("route") or "the pane" for j in il)
                parts.append(f"{len(il)} slow input{'s' * (len(il) != 1)}, worst "
                             f"{max(j['latencyMs'] for j in il)}ms, mostly on {routes.most_common(1)[0][0]}")
            s = f"The pane had trouble over {dur(span) if span >= 1 else 'a moment'}: " + " and ".join(parts)
        if during:
            s += f", while I was {during.most_common(1)[0][0]}."
        else:
            s += ". I can't say what I was busy with; the trace doesn't name the work."
        self.say(ep, s)

    # ---- refusal (guard) ------------------------------------------------
    def on_hook(self, t, rest):
        reason, tool, target = kv(rest, "reason") or "?", kv(rest, "tool"), kv(rest, "target")
        key = (reason, tool, target)
        ep = self.find("refusal")
        if ep and ep.d["key"] == key:
            ep.d["n"] += 1
            ep.deadline = t + 20
            ep.cite(t, "hook deny")
            return
        ep = Ep("refusal", t)
        ep.d.update(key=key, n=1)
        ep.deadline = t + 20
        ep.cite(t, "hook deny")
        self.open.append(ep)

    def end_refusal(self, ep):
        reason, tool, target = ep.d["key"]
        at = f" {target}" if target not in (None, "-") else ""
        base, _, arg = reason.partition(":")
        works = " That's the rule working."
        if base == "stale-worklist-version":
            s = f"I refused Claude's {tool} of{at}: it was written against an old worklist version. It has to re-read and retry." + works
        elif base == "no-coverage-no-opt-out":
            s = (f"I refused Claude's {tool}{at}: no approved item covers it and you hadn't said 'just do it'." + works)
        elif base == "crossboundary-unsigned":
            s = "I refused a forge post because it lacked the agent signature line." + works
        elif base == "crossboundary-stale-version":
            s = f"I refused a forge post: its signature named Bram {arg}, which isn't the running version." + works
        elif base == "crossboundary-unparsed":
            s = f"I refused a forge post because I couldn't check it ({arg or 'unparsed'})." + works
        elif base == "subagent-lifecycle-call":
            s = "I refused a worklist lifecycle call that came from a subagent." + works
        else:
            s = (f"I refused Claude's {tool}{at} with reason '{reason}'. "
                 "I have no standard sentence for that one, so it may be new.")
        if ep.d["n"] > 1:
            s += f" (It happened {ep.d['n']} times in a row.)"
        self.say(ep, s)


# ---- reading ------------------------------------------------------------
def open_log(p):
    return gzip.open(p, "rt", errors="replace") if p.endswith(".gz") else open(p, errors="replace")


def state_dir(root):
    for d in (".bram", "resources"):
        if os.path.isdir(os.path.join(root, d, "bram-traces")):
            return os.path.join(root, d, "bram-traces")
    sys.exit(f"no bram-traces folder under {root}/.bram or {root}/resources")


def stream(paths, hook, lo, hi):
    for p in paths:
        try:
            with open_log(p) as f:
                for line in f:
                    r = parse(line, hook)
                    if r and lo <= r[0] <= hi:
                        yield r
        except OSError:
            pass


def replay_paths(d, lo):
    live = [os.path.join(d, "bram-trace.log")]
    old = [p for p in glob.glob(os.path.join(d, "bram-trace-*.log*")) if os.path.getmtime(p) >= lo]
    return sorted(old, key=os.path.getmtime) + live


def when(s):
    if re.fullmatch(r"\d{1,2}:\d\d", s):
        h, m = map(int, s.split(":"))
        return datetime.now().replace(hour=h, minute=m, second=0, microsecond=0).timestamp()
    dt = datetime.fromisoformat(s.replace("Z", "+00:00"))
    return dt.timestamp()


def follow(narr, d, printer):
    files = {"trace": (os.path.join(d, "bram-trace.log"), False), "hook": (os.path.join(d, "hook-events.log"), True)}
    st = {}
    for k, (p, _) in files.items():
        st[k] = {"f": None, "ino": None, "buf": ""}
    last_told, quiet = time.time(), False
    first = True
    while True:
        for k, (p, hook) in files.items():
            s = st[k]
            try:
                ino, size = os.stat(p).st_ino, os.stat(p).st_size
            except OSError:
                continue
            if s["f"] is None or ino != s["ino"] or size < s["f"].tell():
                if s["f"]:
                    s["f"].close()
                s["f"], s["ino"], s["buf"] = open(p, errors="replace"), ino, ""
                if first or s.get("seen"):
                    s["f"].seek(0, os.SEEK_END) if first else None
                s["seen"] = True
            chunk = s["f"].read()
            if not chunk:
                continue
            s["buf"] += chunk
            *lines, s["buf"] = s["buf"].split("\n")
            for line in lines:
                r = parse(line, hook)
                if r:
                    narr.feed(*r)
        first = False
        narr.tick(time.time())
        told = printer.count
        if told != getattr(follow, "n", 0):
            follow.n, last_told, quiet = told, time.time(), False
        if not quiet and time.time() - last_told >= QUIET_SECS:
            print(f"{datetime.now():%H:%M}  ··· quiet for {QUIET_SECS // 60} minutes ···", flush=True)
            quiet = True
        time.sleep(0.5)


class Printer:
    def __init__(self, cite):
        self.cite, self.count = cite, 0

    def __call__(self, t, text, cites):
        self.count += 1
        if self.cite:
            seen, bits = set(), []
            for ct, what in cites:
                key = (int(ct), what)
                if key not in seen:
                    seen.add(key)
                    bits.append(f"{datetime.fromtimestamp(ct):%H:%M:%S} {what}")
            text += "  [" + "; ".join(bits[:8]) + ("; ..." if len(bits) > 8 else "") + "]"
        print(textwrap.fill(text, width=79, initial_indent=f"{hhmm(t)}  ", subsequent_indent="       "), flush=True)


def main():
    ap = argparse.ArgumentParser(description="Tell Bram's trace as a story.")
    ap.add_argument("--follow", action="store_true")
    ap.add_argument("--since")
    ap.add_argument("--until")
    ap.add_argument("--cite", action="store_true")
    ap.add_argument("--root", default=os.getcwd())
    a = ap.parse_args()
    d = state_dir(os.path.abspath(a.root))
    printer = Printer(a.cite)
    narr = Narrator(printer)
    if a.follow:
        try:
            follow(narr, d, printer)
        except KeyboardInterrupt:
            pass
        return
    lo = when(a.since) if a.since else time.time() - 3600
    hi = when(a.until) if a.until else float("inf")
    streams = [stream(replay_paths(d, lo), False, lo, hi),
               stream([os.path.join(d, "hook-events.log")], True, lo, hi)]
    for r in heapq.merge(*streams, key=lambda r: r[0]):
        narr.feed(*r)
    narr.flush()
    if printer.count == 0:
        print("Nothing to tell for that stretch.")


if __name__ == "__main__":
    try:
        main()
    except BrokenPipeError:
        sys.stderr.close()
