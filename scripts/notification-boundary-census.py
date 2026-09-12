#!/usr/bin/env python3
"""Census background-work notifications against inflight-sentinel vetoes.

A background command or subagent completion is reported to the agent as a
`<task-notification>` envelope. It reaches the session JSONL in two shapes, and
which one it takes is decided by DELIVERY TIMING, not CLI version:

  agent idle     (previous assistant record is `end_turn`)  -> plain `user`
                 record with string content
  agent mid-turn (previous assistant record is `tool_use`)  -> `queue-operation`
                 plus an `attachment` with commandMode "task-notification"

Only the idle shape reaches `compute_claude_turn_stats`, which before b72a29b
adopted it as a turn boundary. That moved the boundary forward mid-turn, made
the turn's own `end_turn` look stale to `claude_jsonl_end_is_stale`, and vetoed
the inflight-sentinel clear -- visible in the trace as
`[jsonl-turn-end] op=skip-stale-jsonl-end`.

This is the instrument behind the before-measurement published in the
correction comment on judell/bram#201, kept as a script so the AFTER is
produced by the same method rather than by a second hand-rolled version (method
drift would otherwise be indistinguishable from result change).

Correlation is not causation, and a stale-end also fires legitimately when a
real user message starts a turn. The `attributable` figure is the tightened
form: a stale-end following a notification with NO genuine user message in
between, so nothing but the notification could have moved the boundary. That is
the number to compare across runs.

A stale-end is a vetoed clear, not a visible stall. N attributable fires means
N vetoes, not N stuck spinners anyone watched.

Boundary convention, stated because it moves the loose numbers: trace lines
carry one-second resolution, so a fire recorded in the SAME second as a
notification may in truth precede it. Both correlation and attribution therefore
count only fires strictly after that second. The attributable figure published
on #201 (29 of 97) already used this conservative rule and reproduces exactly.
The looser correlation figures in that comment (43 of 97 at 60s) counted
same-second fires as hits and run about fourteen higher; this tool reports the
conservative form at every window, so successive runs compare like with like.

Read-only, standard library only. Prints text or JSON; never writes.
"""

from __future__ import annotations

import argparse
import bisect
import datetime as dt
import gzip
import json
import random
import re
import sys
from pathlib import Path

TRACE_MARKER = "skip-stale-jsonl-end"
TRACE_TS = re.compile(r"^\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})")
ENVELOPE = "<task-notification>"
# Windows (seconds) at which correlation is reported. The attributable figure
# uses ATTRIBUTION_WINDOW.
WINDOWS = (60, 120, 300)
ATTRIBUTION_WINDOW = 120
BASELINE_SAMPLES = 2000
BASELINE_SEED = 7  # fixed so two runs of the same data agree


def session_dir_for(project_root: Path) -> Path:
    """Claude's per-project session directory, named after the project path."""
    slug = str(project_root.resolve()).replace("/", "-")
    return Path("~/.claude/projects").expanduser() / slug


def parse_ts(text: str):
    """Parse an ISO timestamp, or a bare `YYYY-MM-DD` as that day's midnight.

    The bare-date form is not a convenience: the placeholder item that schedules
    the after-run says `--since 2026-09-12`, and a tool that rejects its own
    documented invocation fails at the one moment it is needed.
    """
    if not isinstance(text, str):
        return None
    text = text.strip()
    if len(text) == 10:
        text += "T00:00:00"
    try:
        return dt.datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S").replace(
            tzinfo=dt.timezone.utc
        )
    except ValueError:
        return None


def open_log(path: Path):
    if path.suffix == ".gz":
        return gzip.open(path, "rt", errors="replace")
    return path.open("r", errors="replace")


def read_stale_fires(trace_dir: Path):
    """Timestamps of every skip-stale-jsonl-end across live and rotated logs.

    Returns (fires, scanned, damaged). Truncated `.log.gz` archives exist in the
    wild -- rotation can be interrupted -- and a gzip stream that ends before its
    end-of-stream marker raises partway through. Whatever was read before the
    break is kept, and the file is COUNTED as damaged rather than skipped in
    silence: a census that quietly drops archives misreports its own denominator,
    which is exactly the failure the throwaway `zgrep` behind the original
    measurement had (it exits nonzero on a truncated member and says nothing
    about which one).
    """
    fires, scanned, damaged = [], 0, []
    for path in sorted(trace_dir.glob("*.log*")):
        if path.name.endswith(".tmp") or ".tmp-" in path.name:
            continue
        scanned += 1
        try:
            with open_log(path) as fh:
                for line in fh:
                    if TRACE_MARKER not in line:
                        continue
                    m = TRACE_TS.match(line)
                    if m:
                        ts = parse_ts(m.group(1))
                        if ts:
                            fires.append(ts)
        except (OSError, EOFError, gzip.BadGzipFile):
            damaged.append(path.name)
    fires.sort()
    return fires, scanned, damaged


def record_text(rec: dict):
    """The text a user-role record carries, string or content-block array."""
    content = rec.get("message", {}).get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        if any(
            isinstance(b, dict) and b.get("type") == "tool_result" for b in content
        ):
            return None
        return "\n\n".join(
            b.get("text", "")
            for b in content
            if isinstance(b, dict) and b.get("type") == "text"
        )
    return None


def scan_sessions(session_dir: Path, since):
    """Notifications by shape, plus genuine user messages, per session."""
    idle, busy, users_by_session = [], [], {}
    for path in sorted(session_dir.glob("*.jsonl")):
        recs = []
        try:
            with path.open("r", errors="replace") as fh:
                for line in fh:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        recs.append(json.loads(line))
                    except json.JSONDecodeError:
                        continue
        except OSError:
            continue
        users = []
        for i, rec in enumerate(recs):
            ts = parse_ts(rec.get("timestamp") or "")
            if ts is None or (since and ts < since):
                continue
            kind = None
            if rec.get("type") == "user":
                text = record_text(rec)
                if text is None:
                    continue
                if text.lstrip().startswith(ENVELOPE):
                    kind = "idle"
                elif not rec.get("isMeta"):
                    users.append(ts)
            elif rec.get("type") == "attachment":
                att = rec.get("attachment") or {}
                if att.get("commandMode") == "task-notification":
                    kind = "busy"
            if kind is None:
                continue
            # The preceding assistant record's stop_reason is what decides the
            # shape; recorded so an upstream change shows as a table change.
            stop = "none"
            for j in range(i - 1, -1, -1):
                if recs[j].get("type") == "assistant":
                    stop = recs[j].get("message", {}).get("stop_reason") or "none"
                    break
            entry = {
                "ts": ts,
                "stop": stop,
                "version": rec.get("version") or "?",
                "session": path.stem,
            }
            (idle if kind == "idle" else busy).append(entry)
        users.sort()
        users_by_session[path.stem] = users
    idle.sort(key=lambda e: e["ts"])
    busy.sort(key=lambda e: e["ts"])
    return idle, busy, users_by_session


def followed_within(fires, when, window) -> bool:
    lo = bisect.bisect_right(fires, when)
    hi = bisect.bisect_right(fires, when + dt.timedelta(seconds=window))
    return hi > lo


def first_fire_after(fires, when, window):
    lo = bisect.bisect_right(fires, when)
    if lo < len(fires) and fires[lo] <= when + dt.timedelta(seconds=window):
        return fires[lo]
    return None


def attribute(notifications, fires, users_by_session, window):
    """Fires with no genuine user message between notification and fire."""
    attributable, intervened, pairs = 0, 0, []
    for note in notifications:
        fire = first_fire_after(fires, note["ts"], window)
        if fire is None:
            continue
        users = users_by_session.get(note["session"], [])
        between = bisect.bisect_right(users, fire) - bisect.bisect_right(
            users, note["ts"]
        )
        if between > 0:
            intervened += 1
        else:
            attributable += 1
            pairs.append((note["ts"], fire))
    return attributable, intervened, pairs


def baseline(fires, notifications, window) -> float:
    """Hit rate for random timestamps drawn from the notifications' own span."""
    if len(notifications) < 2:
        return 0.0
    lo, hi = notifications[0]["ts"], notifications[-1]["ts"]
    span = (hi - lo).total_seconds()
    if span <= 0:
        return 0.0
    rng = random.Random(BASELINE_SEED)
    hits = sum(
        1
        for _ in range(BASELINE_SAMPLES)
        if followed_within(fires, lo + dt.timedelta(seconds=rng.random() * span), window)
    )
    return 100.0 * hits / BASELINE_SAMPLES


def summarize(entries) -> dict:
    versions, stops = {}, {}
    for e in entries:
        versions[e["version"]] = versions.get(e["version"], 0) + 1
        stops[e["stop"]] = stops.get(e["stop"], 0) + 1
    return {
        "count": len(entries),
        "versions": dict(sorted(versions.items())),
        "prev_stop_reason": dict(sorted(stops.items(), key=lambda kv: -kv[1])),
        "first": entries[0]["ts"].isoformat() if entries else None,
        "last": entries[-1]["ts"].isoformat() if entries else None,
    }


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument(
        "--project-root",
        type=Path,
        default=Path.cwd(),
        help="project whose sessions and traces to read (default: cwd)",
    )
    ap.add_argument("--session-dir", type=Path, help="override the session directory")
    ap.add_argument("--trace-dir", type=Path, help="override the trace directory")
    ap.add_argument(
        "--since", help="ISO date or timestamp; ignore notifications before it"
    )
    ap.add_argument("--json", action="store_true", help="emit JSON instead of text")
    args = ap.parse_args(argv)

    root = args.project_root.expanduser().resolve()
    session_dir = (args.session_dir or session_dir_for(root)).expanduser()
    trace_dir = (args.trace_dir or root / "resources" / "bram-traces").expanduser()
    since = parse_ts(args.since) if args.since else None
    if args.since and since is None:
        print(f"could not parse --since {args.since!r}", file=sys.stderr)
        return 2
    for label, path in (("session", session_dir), ("trace", trace_dir)):
        if not path.is_dir():
            print(f"{label} directory not found: {path}", file=sys.stderr)
            return 2

    fires, scanned, damaged = read_stale_fires(trace_dir)
    idle, busy, users_by_session = scan_sessions(session_dir, since)
    attributable, intervened, pairs = attribute(
        idle, fires, users_by_session, ATTRIBUTION_WINDOW
    )

    result = {
        "project_root": str(root),
        "since": since.isoformat() if since else None,
        "stale_end_fires": len(fires),
        "trace_logs_scanned": scanned,
        "trace_logs_damaged": damaged,
        "idle_shape": summarize(idle),
        "busy_shape": summarize(busy),
        "correlation": {
            str(w): {
                "idle": sum(1 for e in idle if followed_within(fires, e["ts"], w)),
                "busy": sum(1 for e in busy if followed_within(fires, e["ts"], w)),
                "baseline_pct": round(baseline(fires, idle, w), 1),
            }
            for w in WINDOWS
        },
        "attributable": attributable,
        "intervened": intervened,
        "attribution_window_s": ATTRIBUTION_WINDOW,
        "sample_pairs": [
            {"notification": a.isoformat(), "stale_end": b.isoformat()}
            for a, b in pairs[:5]
        ],
    }

    if args.json:
        print(json.dumps(result, indent=2))
        return 0

    print(f"project        {root}")
    if since:
        print(f"since          {since.isoformat()}")
    print(
        f"stale-end fires {len(fires)}"
        f" (from {scanned} trace logs, live and rotated)"
    )
    if damaged:
        print(
            f"    WARNING {len(damaged)} log(s) truncated; partial reads counted:"
            f" {', '.join(damaged[:3])}{' ...' if len(damaged) > 3 else ''}"
        )
    print()
    for label, entries in (("idle  (user record)", idle), ("busy  (attachment)", busy)):
        s = summarize(entries)
        print(f"{label}: {s['count']}")
        if s["count"]:
            print(f"    span      {s['first'][:16]} .. {s['last'][:16]}")
            print(f"    versions  {s['versions']}")
            print(f"    prev stop {s['prev_stop_reason']}")
    print()
    print("correlation (notification followed by a stale-end within N seconds)")
    for w in WINDOWS:
        c = result["correlation"][str(w)]
        pct = 100 * c["idle"] // len(idle) if idle else 0
        print(
            f"    {w:>3}s   idle {c['idle']}/{len(idle)} ({pct}%)"
            f"   busy {c['busy']}/{len(busy)}"
            f"   random baseline {c['baseline_pct']}%"
        )
    print()
    print(
        f"attributable   {attributable}/{len(idle)}"
        f"  (stale-end within {ATTRIBUTION_WINDOW}s, no user message between)"
    )
    print(f"intervened     {intervened}  (excluded: a real user message came first)")
    for pair in result["sample_pairs"]:
        print(f"    {pair['notification'][:19]} -> {pair['stale_end'][:19]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
