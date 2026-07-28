#!/usr/bin/env python3
"""XMLUI MCP demand miner.

Mine an MCP server's analytics log for what people *ask it for* — the demand
signal — and where its corpus is thinnest. Named for the XMLUI MCP server
(its analytics is the default target and the reason this exists), but the
engine is server-agnostic: point --analytics at any MCP server that logs
the same JSONL shape and pass --server to relabel the output.

Read-only, stdlib only. Prints, never writes.

WHAT THIS IS (and isn't)
------------------------
The analytics is *global*: every use of the MCP server on this machine,
across every project — not scoped to this repo. So it is a population-scale
demand curve, not our own experience.

It is also *outcome-blind*. In this corpus `result_count` is NEVER 0 — a
search always returns *something*, so there is no hard "miss" to grep for.
Being ill-served is therefore qualitative: a high-demand topic whose best
matches are thin or off-target. This tool surfaces demand and weak coverage;
it cannot by itself prove a how-to is missing. Confirm each candidate against
`xmlui_list_howto` (does the doc exist?) and `/__search` over our own
sessions (did we actually struggle?). See the "Search-first" /
"Coordinating MCP demand with search" notes in app/__shell/conventions.md.

RECORD SHAPES (xmlui-mcp-analytics.json, JSONL)
-----------------------------------------------
  type=tool_invocation  tool_name, arguments{}, success, result_size_chars
  type=search_query     tool_name, query, result_count, success, search_paths[]

`search_query` records are the query log with outcomes; `tool_invocation`
records are the raw tool-usage tape. A search fires one of each.
"""

import argparse
import collections
import json
import os
import sys

DEFAULT_ANALYTICS = os.path.expanduser(
    "~/Library/Caches/xmlui/xmlui-mcp/xmlui-mcp-analytics.json"
)

# Domain + English stopwords: strip these before ranking query terms so the
# demand curve reflects topics, not scaffolding.
STOPWORDS = frozenset("""
a an the this that these those and or not but for to of in on with without
into from as at by is are be do does how i my me we our you your it its
use using used get set add make show do want need can could should would
xmlui component components example examples doc docs documentation
""".split())


def load_records(path):
    """Yield parsed JSON records, skipping blank/garbage lines."""
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except ValueError:
                continue


def query_text(rec):
    """The query string on a record, wherever it lives."""
    q = rec.get("query")
    if q:
        return q
    args = rec.get("arguments")
    if isinstance(args, dict):
        return args.get("query") or ""
    return ""


def terms(q):
    """Lowercase alnum tokens of q, minus stopwords and length-<2 noise."""
    out = []
    for raw in q.lower().replace("/", " ").replace("-", " ").split():
        tok = "".join(c for c in raw if c.isalnum())
        if len(tok) >= 2 and tok not in STOPWORDS:
            out.append(tok)
    return out


def is_howto(rec):
    """True when the query touched the how-to corpus."""
    if rec.get("tool_name") == "xmlui_search_howto":
        return True
    paths = rec.get("search_paths")
    if isinstance(paths, list):
        return any("howto" in p for p in paths)
    return False


def bar(n, width, scale):
    return "#" * min(width, int(round(n * width / scale))) if scale else ""


def section(title):
    print("\n" + title)
    print("-" * len(title))


def rank_terms(searches, top, title, note=None):
    counter = collections.Counter()
    for rec in searches:
        counter.update(set(terms(query_text(rec))))  # per-search, dedup within
    section(title)
    if note:
        print(note)
    if not counter:
        print("(none)")
        return
    scale = counter.most_common(1)[0][1]
    for term, n in counter.most_common(top):
        print(f"  {n:4d}  {bar(n, 24, scale):<24}  {term}")


def rank_full_queries(searches, top, title):
    counter = collections.Counter()
    for rec in searches:
        q = query_text(rec).strip().lower()
        if q:
            counter[q] += 1
    section(title)
    for q, n in counter.most_common(top):
        if n < 2:
            break
        print(f"  {n:4d}  {q}")


def weak_coverage(searches, top, title):
    """Topics whose searches return the fewest matches = thinnest corpus.

    Grouped by normalized query so a repeated thin query ranks by how often
    it was asked, not just once. result_count is never 0, so this is a
    relative 'thin', not an absolute miss.
    """
    groups = collections.defaultdict(list)
    for rec in searches:
        rc = rec.get("result_count")
        if rc is None:
            continue
        q = query_text(rec).strip().lower()
        if q:
            groups[q].append(rc)
    rows = []
    for q, counts in groups.items():
        rows.append((sum(counts) / len(counts), len(counts), min(counts), q))
    rows.sort(key=lambda r: (r[0], -r[1]))  # thinnest first, then most-asked
    section(title)
    print("  avg  min  asked  query")
    for avg, asked, mn, q in rows[:top]:
        print(f"  {avg:4.1f} {mn:4d} {asked:5d}  {q}")


def main():
    ap = argparse.ArgumentParser(
        prog="xmlui-mcp-demand-miner",
        description="XMLUI MCP demand miner: rank what an MCP server is asked "
        "for and where its corpus is thin.",
    )
    ap.add_argument("--analytics", default=DEFAULT_ANALYTICS,
                    help="path to the MCP analytics JSONL "
                         "(default: XMLUI MCP cache).")
    ap.add_argument("--server", default="xmlui",
                    help="label for the server in output (default: xmlui).")
    ap.add_argument("--tool", default=None,
                    help="restrict to one tool_name (e.g. xmlui_search_howto).")
    ap.add_argument("--since", default=None,
                    help="ISO date/time lower bound on timestamp (e.g. "
                         "2026-06-01).")
    ap.add_argument("--top", type=int, default=25,
                    help="rows per section (default 25).")
    ap.add_argument("--howto-only", action="store_true",
                    help="restrict every section to how-to-corpus queries.")
    args = ap.parse_args()

    if not os.path.exists(args.analytics):
        sys.exit(f"analytics log not found: {args.analytics}")

    invocations = []   # type=tool_invocation
    searches = []      # type=search_query
    failures = []
    for rec in load_records(args.analytics):
        if args.since and str(rec.get("timestamp", "")) < args.since:
            continue
        if args.tool and rec.get("tool_name") != args.tool:
            continue
        if rec.get("success") is False:
            failures.append(rec)
        t = rec.get("type")
        if t == "tool_invocation":
            invocations.append(rec)
        elif t == "search_query":
            if args.howto_only and not is_howto(rec):
                continue
            searches.append(rec)

    howto_searches = [r for r in searches if is_howto(r)]

    print(f"XMLUI MCP demand miner  —  server={args.server}")
    print(f"source: {args.analytics}")
    print(f"records: {len(invocations)} invocations, {len(searches)} searches"
          f"  ({len(howto_searches)} how-to)"
          + (f", since {args.since}" if args.since else ""))
    print("NOTE: analytics is GLOBAL (all use on this machine, every project), "
          "not this repo.\n"
          "      result_count is never 0 here, so 'weak coverage' is thin, "
          "not a hard miss.\n"
          "      Confirm each gap: xmlui_list_howto (exists?) + /__search "
          "(did we struggle?).")

    # Tool usage tape.
    section("Tool usage")
    usage = collections.Counter(r.get("tool_name", "?") for r in invocations)
    if usage:
        scale = usage.most_common(1)[0][1]
        for name, n in usage.most_common():
            print(f"  {n:4d}  {bar(n, 24, scale):<24}  {name}")
    else:
        print("(none)")

    rank_terms(searches, args.top, "Demand — top query terms (all search)")
    rank_terms(howto_searches, args.top,
               "How-to demand — top terms (howto corpus)",
               note="The signal for missing/weak how-tos. Cross-check each "
                    "against xmlui_list_howto + /__search.")
    rank_full_queries(searches, args.top,
                      "Repeated exact queries (asked >= 2x)")
    weak_coverage(howto_searches, args.top,
                  "Weak how-to coverage (thinnest result sets, most-asked)")

    section("Failures (success=false)")
    if failures:
        fc = collections.Counter(r.get("tool_name", "?") for r in failures)
        for name, n in fc.most_common():
            print(f"  {n:4d}  {name}")
    else:
        print("(none — no failed invocations in this log)")


if __name__ == "__main__":
    main()
