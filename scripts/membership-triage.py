#!/usr/bin/env python3
"""Sweep bram traces for membership-engine evidence and queue adjudications.

The flip decision (docs/attribution-model.md §6) needs adjudicated divergence
evidence with a denominator. The membership engine's three op families —
``op=membership`` (cost, every board serve), ``op=membership-diverges`` (the
two models disagree on a path's owners), and
``op=membership-conservation-broken`` (the tripwire) — scroll past in
``resources/bram-traces/bram-trace.log`` and its rotated archives across every
enrolled repo. This tool makes that ambient evidence visible and countable:

- collects the three families from each repo's live log plus rotated
  ``bram-trace-*.log`` / ``.log.gz`` archives;
- dedups divergences by (path, replay-set, membership-set) and emits each as a
  ready-to-paste markdown adjudication block, with a shape guess that is a
  hint, never a verdict;
- tallies the denominator: total serves, serves with zero divergence,
  conservation fires with timestamps, and the current conservation streak;
- summarizes against the flip criteria (attribution-model.md §6): streak,
  shapes sampled, cost percentiles beside the replay's own ``op=attribute``.

Adjudication itself stays human+agent judgment posted to judell/bram#273; this
only builds the queue. Read-only, no network, stdlib only (the gap-miner's
discipline). ``--json`` for machine use.
"""

from __future__ import annotations

import argparse
import gzip
import io
import json
import os
import re
import sys
from pathlib import Path
from typing import Any, Iterable, Iterator

SOURCE_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REPOS = (SOURCE_ROOT, Path.home() / "xmlui", Path.home() / "budget")
TRACE_DIR = "resources/bram-traces"

LINE_RE = re.compile(
    r"^\[(?P<ts>[0-9T:.Z+-]+)\]\s+\[claim-interval\]\s+(?P<payload>op=\S.*)$"
)


def parse_kv(payload: str) -> dict[str, str]:
    """Tokenize ``k=v`` pairs; later duplicate keys win (there are none today)."""
    out: dict[str, str] = {}
    for token in payload.split():
        if "=" in token:
            key, _, value = token.partition("=")
            out[key] = value
    return out


def id_set(raw: str) -> frozenset[str]:
    if raw in ("", "-"):
        return frozenset()
    return frozenset(part for part in raw.split(",") if part)


def classify(replay: frozenset[str], membership: frozenset[str]) -> str:
    """A triage hint, never a verdict (the adjudicator decides).

    - membership ⊃ replay: the replay misses work whose evidence accounts for
      current content (the created-files specimen on #273) → under-credit.
    - replay ⊃ membership: the replay credits ids whose evidence does not
      account for current content. From the line alone, phantom credit
      (over-credit, #273's filed shape) and drifted evidence (supersession)
      are indistinguishable → the compound label.
    - mixed multi-id sets: joint attribution disagreement → joint.
    - anything else → ambiguous (including membership's fourth first-class
      state, which the divergence line cannot currently express directly).
    """
    if replay < membership:
        return "under-credit"
    if membership < replay:
        return "over-credit|supersession"
    if len(replay) > 1 or len(membership) > 1:
        return "joint"
    return "ambiguous"


def trace_files(repo: Path) -> list[Path]:
    trace_dir = repo / TRACE_DIR
    if not trace_dir.is_dir():
        return []
    files = sorted(
        p
        for p in trace_dir.iterdir()
        if p.name.startswith("bram-trace") and p.suffix in (".log", ".gz")
    )
    return files


def read_lines(path: Path) -> Iterator[str]:
    """Yield lines, salvaging what decompresses from a truncated archive.

    Rotated ``.log.gz`` files are written by a background pass; a crash can
    leave one without its end-of-stream marker (gzip raises EOFError, not
    OSError, mid-iteration), and one bad archive must not abort the sweep.
    """
    try:
        if path.suffix == ".gz":
            fh = gzip.open(path, "rt", encoding="utf-8", errors="replace")
        else:
            fh = io.open(path, "r", encoding="utf-8", errors="replace")
    except OSError as exc:
        print(f"membership-triage: skipping {path}: {exc}", file=sys.stderr)
        return
    with fh:
        while True:
            try:
                line = fh.readline()
            except (OSError, EOFError) as exc:
                print(f"membership-triage: truncated {path}: {exc}", file=sys.stderr)
                return
            if not line:
                return
            yield line


def collect_events(repo: Path, since: str | None) -> list[dict[str, Any]]:
    """Every membership-family event in the repo's traces, sorted by timestamp.

    ISO-8601 Z timestamps sort lexically, so a global sort restores
    chronology across the live log and however the archives were rotated.
    """
    wanted = ("op=membership ", "op=membership-diverges ", "op=membership-conservation-broken ")
    events: list[dict[str, Any]] = []
    for path in trace_files(repo):
        for raw in read_lines(path):
            if "[claim-interval]" not in raw or "op=membership" not in raw:
                continue
            match = LINE_RE.match(raw.strip())
            if not match:
                continue
            payload = match.group("payload")
            if not payload.startswith(wanted):
                continue
            ts = match.group("ts")
            if since and ts < since:
                continue
            kv = parse_kv(payload)
            events.append(
                {"ts": ts, "op": kv.get("op", ""), "kv": kv, "line": raw.strip()}
            )
    events.sort(key=lambda e: e["ts"])
    return events


def collect_replay_costs(repo: Path, since: str | None) -> list[int]:
    """The replay's own op=attribute ms figures — the flip's cost baseline
    (§6 criterion 7: the budget is set from the replay's measured baseline)."""
    costs: list[int] = []
    for path in trace_files(repo):
        for raw in read_lines(path):
            if "op=attribute " not in raw or "[claim-interval]" not in raw:
                continue
            match = LINE_RE.match(raw.strip())
            if not match or not match.group("payload").startswith("op=attribute "):
                continue
            if since and match.group("ts") < since:
                continue
            ms = parse_kv(match.group("payload")).get("ms", "")
            if ms.isdigit():
                costs.append(int(ms))
    return costs


def percentile(sorted_values: list[int], fraction: float) -> int:
    if not sorted_values:
        return 0
    index = min(len(sorted_values) - 1, int(round(fraction * (len(sorted_values) - 1))))
    return sorted_values[index]


def triage_repo(repo: Path, since: str | None) -> dict[str, Any]:
    events = collect_events(repo, since)
    serves: list[dict[str, Any]] = []
    divergences: dict[tuple[str, frozenset[str], frozenset[str]], dict[str, Any]] = {}
    conservation: list[dict[str, Any]] = []
    pending_divergences = 0
    pending_conservation = 0
    clean_serves = 0
    serves_since_breach = 0

    for event in events:
        op = event["op"]
        kv = event["kv"]
        if op == "membership-diverges":
            pending_divergences += 1
            replay = id_set(kv.get("replay", "-"))
            membership = id_set(kv.get("membership", "-"))
            key = (kv.get("path", "?"), replay, membership)
            record = divergences.get(key)
            if record is None:
                divergences[key] = {
                    "path": kv.get("path", "?"),
                    "replay": sorted(replay),
                    "membership": sorted(membership),
                    "shape": classify(replay, membership),
                    "count": 1,
                    "firstSeen": event["ts"],
                    "lastSeen": event["ts"],
                    "sampleLine": event["line"],
                }
            else:
                record["count"] += 1
                record["lastSeen"] = event["ts"]
        elif op == "membership-conservation-broken":
            pending_conservation += 1
            conservation.append({"ts": event["ts"], "line": event["line"], "kv": kv})
        elif op == "membership":
            # The engine emits divergence/breach lines before its cost line,
            # so pending events belong to THIS serve.
            if pending_divergences == 0:
                clean_serves += 1
            if pending_conservation:
                serves_since_breach = 0
            else:
                serves_since_breach += 1
            serves.append(
                {
                    "ts": event["ts"],
                    "paths": int(kv.get("paths", "0") or 0),
                    "ms": int(kv.get("ms", "0") or 0),
                    "spawns": int(kv.get("spawns", "0") or 0),
                    # Absent on lines predating membership-ambiguous-trace-channel;
                    # 0 keeps old logs contributing nothing rather than skewing.
                    "ambiguous": int(kv.get("ambiguous", "0") or 0),
                    "divergences": pending_divergences,
                }
            )
            pending_divergences = 0
            pending_conservation = 0

    membership_ms = sorted(s["ms"] for s in serves)
    replay_ms = sorted(collect_replay_costs(repo, since))
    ambiguous_serves = sum(1 for s in serves if s["ambiguous"] > 0)
    ambiguous_paths_total = sum(s["ambiguous"] for s in serves)
    shapes = sorted({d["shape"] for d in divergences.values()})
    if ambiguous_serves:
        # The ambiguous STATE has its own positive channel on the cost line;
        # without this it would be invisible to "shapes sampled" whenever
        # conservation holds and the owner sets agree.
        shapes = sorted({*shapes, "ambiguous-state"})
    return {
        "repo": str(repo),
        "traceFiles": len(trace_files(repo)),
        "serves": len(serves),
        "cleanServes": clean_serves,
        "divergenceLines": sum(d["count"] for d in divergences.values()),
        "uniqueDivergences": len(divergences),
        "conservationFires": len(conservation),
        "conservationFireTimestamps": [c["ts"] for c in conservation],
        "conservationStreak": serves_since_breach,
        "ambiguousServes": ambiguous_serves,
        "ambiguousPathsTotal": ambiguous_paths_total,
        "shapesSampled": shapes,
        "membershipMs": {
            "p50": percentile(membership_ms, 0.50),
            "p90": percentile(membership_ms, 0.90),
            "max": membership_ms[-1] if membership_ms else 0,
        },
        "replayMs": {
            "p50": percentile(replay_ms, 0.50),
            "p90": percentile(replay_ms, 0.90),
            "max": replay_ms[-1] if replay_ms else 0,
        },
        "divergences": sorted(
            divergences.values(), key=lambda d: (d["path"], d["firstSeen"])
        ),
        "conservation": conservation,
    }


def render_markdown(reports: list[dict[str, Any]], since: str | None) -> str:
    out: list[str] = []
    scope = f" since {since}" if since else ""
    out.append(f"## Membership triage sweep{scope}")
    out.append("")
    out.append(
        "| repo | serves | clean serves | divergence lines | unique | "
        "conservation fires | streak | ambiguous serves/paths | "
        "membership ms p50/p90/max | replay ms p50/p90/max | shapes |"
    )
    out.append("|---|---|---|---|---|---|---|---|---|---|---|")
    for rep in reports:
        m, r = rep["membershipMs"], rep["replayMs"]
        out.append(
            "| {repo} | {serves} | {clean} | {lines} | {uniq} | {fires} | {streak} | "
            "{aserves}/{apaths} | "
            "{mp50}/{mp90}/{mmax} | {rp50}/{rp90}/{rmax} | {shapes} |".format(
                repo=Path(rep["repo"]).name,
                serves=rep["serves"],
                clean=rep["cleanServes"],
                lines=rep["divergenceLines"],
                uniq=rep["uniqueDivergences"],
                fires=rep["conservationFires"],
                streak=rep["conservationStreak"],
                aserves=rep["ambiguousServes"],
                apaths=rep["ambiguousPathsTotal"],
                mp50=m["p50"], mp90=m["p90"], mmax=m["max"],
                rp50=r["p50"], rp90=r["p90"], rmax=r["max"],
                shapes=", ".join(rep["shapesSampled"]) or "-",
            )
        )
    out.append("")
    out.append(
        "Streak = consecutive serves since the last conservation fire "
        "(equals total serves when the history is clean). Shape guesses are "
        "hints, not verdicts — adjudication is posted to judell/bram#273."
    )
    for rep in reports:
        name = Path(rep["repo"]).name
        for fire in rep["conservation"]:
            out.append("")
            out.append(f"### CONSERVATION FIRE — {name} `{fire['kv'].get('path', '?')}`")
            out.append("")
            out.append("```")
            out.append(fire["line"])
            out.append("```")
        for div in rep["divergences"]:
            out.append("")
            out.append(f"### {name}: `{div['path']}`")
            out.append("")
            out.append(f"- shape guess: **{div['shape']}** (hint, not a verdict)")
            out.append(
                "- replay: `{}` · membership: `{}`".format(
                    ", ".join(div["replay"]) or "-",
                    ", ".join(div["membership"]) or "-",
                )
            )
            out.append(
                f"- seen {div['count']}× · first `{div['firstSeen']}` · last `{div['lastSeen']}`"
            )
            out.append("")
            out.append("```")
            out.append(div["sampleLine"])
            out.append("```")
    out.append("")
    return "\n".join(out)


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--repo",
        action="append",
        type=Path,
        help="repo root to sweep (repeatable; default: this checkout, ~/xmlui, ~/budget)",
    )
    parser.add_argument(
        "--since", help="ignore events before this ISO date/timestamp (lexical compare)"
    )
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    args = parser.parse_args(list(argv) if argv is not None else None)

    repos = args.repo or [p for p in DEFAULT_REPOS if (p / TRACE_DIR).is_dir()]
    if not repos:
        print("membership-triage: no repos with trace directories found", file=sys.stderr)
        return 2
    resolved = [Path(os.path.expanduser(str(r))).resolve() for r in repos]
    reports = [triage_repo(repo, args.since) for repo in resolved]
    if args.json:
        for rep in reports:
            rep.pop("conservation", None)  # duplicated by timestamps + lines above
        print(json.dumps(reports, indent=2, sort_keys=True))
    else:
        print(render_markdown(reports, args.since))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
