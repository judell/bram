#!/usr/bin/env python3
"""UI responsiveness health check, read from Bram's trace (read-only).

The same gauges the 2026-07-22 perf audit used (250503cf), plus the ones added
since: iframe heartbeat drift, long tasks, keystroke/click latency, parent
xterm stalls, slow terminal-write batches, transcript projection times, and
the slowest host routes. It reads logs; it never touches a running Bram.

    python3 -I scripts/ui-health.py                    # this checkout's live trace
    python3 -I scripts/ui-health.py LOG [LOG ...]      # specific logs or archives
    python3 -I scripts/ui-health.py --trend 2026-10-01 2026-10-27 2026-10-28

The default report also attributes idle-time spikes: for every heartbeat spike
over 1 s while no agent turn was running, it counts which host emits and routes
appeared in the 2.5 s before it. That column found the 2026-10-29 regression
(another project's subagent writes refetching this window's roster). --trend
reads the rotated archives under resources/bram-traces/ and prints one row per
day, for comparing against the long-run baseline.

What "healthy" looked like on this machine, for reading the numbers: the
2026-07-22 fix left heartbeat avg 6-14 ms, max 81-141 ms, 0 spikes; the
2026-07..10 daily trend ran 0.3-3 focused spikes >1 s per active hour with
input-latency p50 104-185 ms. Heartbeat drift lines are logged only when a
tick is late, so their counts are events, not samples.

The trace tells you WHEN and WHAT ELSE was happening, not where main-thread
time went. Per docs/developing-bram.md §Perf diagnosis ordering, a felt
slowdown without an obvious correlate gets a Safari Timeline recording first.
"""
from __future__ import annotations

import argparse
import collections
import glob
import gzip
import json
import os
import re
import sys
from datetime import datetime
from pathlib import Path

LINE = re.compile(r"^\[(?P<ts>[^\]]+)\] \[(?P<cat>[^\]]+)\] (?P<rest>.*)$")
GAUGES = (
    "heartbeat-drift",
    "xterm-liveness",
    "long-task",
    "input-latency",
    "terminal-write-batch",
    "projected-turns",
)


def pct(xs, p):
    if not xs:
        return 0
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100 * (len(xs) - 1))))]


def row(name, xs, spike=None):
    if not xs:
        return f"{name:36} n=0"
    s = f"{name:36} n={len(xs):5} p50={pct(xs, 50):6}ms p95={pct(xs, 95):6}ms max={max(xs):6}ms"
    if spike is not None:
        s += f"  >{spike}ms={sum(1 for x in xs if x > spike)}"
    return s


def open_log(path):
    return gzip.open(path, "rt", errors="replace") if path.endswith(".gz") else open(path, errors="replace")


def epoch(ts):
    return datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()


def gauge(cat, rest):
    """(kind, payload) for a real iframe gauge line, else None. PTY-content
    lines can contain the same text, so match the category and prefix."""
    if cat != "iframe" or not rest.startswith("subkind="):
        return None
    kind, _, payload = rest.partition(" ")
    kind = kind[len("subkind="):]
    if kind not in GAUGES:
        return None
    try:
        return kind, json.loads(payload)
    except ValueError:
        return None


def report(paths):
    hb = collections.defaultdict(list)
    stalls, longtasks, inputs, writes, subscribe, tick = [], [], [], [], [], []
    input_routes = collections.defaultdict(list)
    routes = collections.defaultdict(list)
    spikes_by_hour = collections.Counter()
    phase, last, phase_secs = "finished", None, collections.Counter()
    phase_spikes = collections.Counter()
    recent = collections.deque()
    before_idle = collections.Counter()
    idle_spikes = 0
    first = end = None

    for path in paths:
        with open_log(path) as f:
            for line in f:
                m = LINE.match(line)
                if not m:
                    continue
                ts, cat, rest = m["ts"], m["cat"], m["rest"]
                try:
                    now = epoch(ts)
                except ValueError:
                    continue
                first = first or ts
                end = ts
                if last is not None and now >= last:
                    phase_secs[phase] += now - last
                last = now
                if cat == "turn-state":
                    pm = re.search(r" to=(\w+)", rest)
                    if pm:
                        phase = "working" if pm.group(1) == "working" else "idle"
                    continue
                g = gauge(cat, rest)
                if g is None:
                    if cat == "route":
                        pm = re.search(r"path=(\S+)", rest)
                        dm = re.search(r"duration_ms=(\d+)", rest)
                        if pm and dm and "phase=exit" in rest:
                            routes[pm.group(1)].append(int(dm.group(1)))
                            recent.append((now, "route " + pm.group(1)))
                    elif cat == "emit":
                        km = re.search(r"kind=(\S+)", rest)
                        if km:
                            recent.append((now, "emit " + km.group(1)))
                    while recent and now - recent[0][0] > 2.5:
                        recent.popleft()
                    continue
                kind, d = g
                if kind == "heartbeat-drift":
                    state = "hidden" if d.get("hidden") else ("focused" if d.get("focused") else "visible-unfocused")
                    drift = d.get("drift_ms", 0)
                    hb[state].append(drift)
                    if state == "focused" and drift > 1000:
                        spikes_by_hour[ts[:13]] += 1
                        phase_spikes[phase] += 1
                        if phase == "idle":
                            idle_spikes += 1
                            for k in {k for _, k in recent}:
                                before_idle[k] += 1
                elif kind == "xterm-liveness" and d.get("op") == "stall":
                    stalls.append(d.get("gap_ms", 0))
                elif kind == "long-task":
                    longtasks.append(d.get("ms", 0))
                elif kind == "input-latency":
                    inputs.append(d.get("latencyMs", 0))
                    input_routes[d.get("route", "?")].append(d.get("latencyMs", 0))
                elif kind == "terminal-write-batch" and d.get("op") == "slow":
                    writes.append(d.get("parseMs", 0))
                elif kind == "projected-turns":
                    (subscribe if d.get("reason") == "subscribe" else tick).append(d.get("ms", 0))

    print(f"window: {first} .. {end}")
    print("-- pane main thread --")
    for state in ("focused", "visible-unfocused", "hidden"):
        print(row(f"heartbeat-drift [{state}]", hb[state], 1000))
    print("   focused spikes >1s by hour:", dict(sorted(spikes_by_hour.items())))
    for p in ("working", "idle"):
        hours = phase_secs[p] / 3600
        rate = phase_spikes[p] / hours if hours else 0
        print(f"   during {p:8} {hours:5.2f} h  spikes>1s={phase_spikes[p]:4}  ({rate:5.1f}/h)")
    print(row("long-task", longtasks, 500))
    print(row("input-latency", inputs, 200))
    for r, xs in sorted(input_routes.items(), key=lambda kv: -len(kv[1]))[:4]:
        print(row(f"   input-latency {r}", xs, 200))
    print("-- parent window / terminal --")
    print(row("xterm-liveness stall", stalls, 1000))
    print(row("terminal-write-batch slow parse", writes, 250))
    print("-- transcript projection --")
    print(row("projected-turns subscribe", subscribe, 500))
    print(row("projected-turns tick", tick, 500))
    print("-- host routes, slowest p95 (n>=5) --")
    for _, p, xs in sorted(((pct(xs, 95), p, xs) for p, xs in routes.items() if len(xs) >= 5), reverse=True)[:8]:
        print(row(f"   {p[:32]}", xs))
    print(f"-- what preceded idle spikes >1s (n={idle_spikes}, window 2.5s) --")
    for k, n in before_idle.most_common(10):
        print(f"   {n:4}  {k}")


def trend(trace_dir, days):
    print(f"{'day':10} {'hours':>5} {'spikes/h':>8} {'drift p50':>9} {'input p50':>9} {'input p95':>9} {'inputs':>6}")
    for day in days:
        files = [
            f
            for f in sorted(glob.glob(os.path.join(trace_dir, f"bram-trace-{day}*.log*")))
            if not os.path.basename(f).startswith(".")
        ]
        hours, spikes, drift, inputs = set(), 0, [], []
        for path in files:
            try:
                with open_log(path) as f:
                    for line in f:
                        if "subkind=heartbeat-drift" not in line and "subkind=input-latency" not in line:
                            continue
                        m = LINE.match(line)
                        g = m and gauge(m["cat"], m["rest"])
                        if not g:
                            continue
                        kind, d = g
                        if kind == "heartbeat-drift":
                            hours.add(m["ts"][:13])
                            if d.get("focused") and not d.get("hidden"):
                                drift.append(d.get("drift_ms", 0))
                                spikes += d.get("drift_ms", 0) > 1000
                        elif kind == "input-latency":
                            inputs.append(d.get("latencyMs", 0))
            except (OSError, EOFError):
                continue
        n = max(1, len(hours))
        print(
            f"{day:10} {len(hours):5} {spikes / n:8.1f} {pct(drift, 50):9} "
            f"{pct(inputs, 50):9} {pct(inputs, 95):9} {len(inputs):6}"
        )


def main():
    root = Path(__file__).resolve().parent.parent
    traces = root / "resources" / "bram-traces"
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("logs", nargs="*", help="trace logs or .gz archives (default: the live trace)")
    ap.add_argument("--trend", nargs="+", metavar="DAY", help="per-day rows from the archives, YYYY-MM-DD")
    ap.add_argument("--trace-dir", default=str(traces), help="archive directory for --trend")
    args = ap.parse_args()
    if args.trend:
        trend(args.trace_dir, args.trend)
        return 0
    logs = args.logs or [str(traces / "bram-trace.log")]
    missing = [p for p in logs if not os.path.exists(p)]
    if missing:
        print("no such log: " + ", ".join(missing), file=sys.stderr)
        return 2
    report(logs)
    return 0


if __name__ == "__main__":
    sys.exit(main())
