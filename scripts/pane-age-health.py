#!/usr/bin/env python3
"""Does pane age or session size explain a sluggish pane? (read-only)

Reads Bram's trace and the agent's session transcripts, and reports pane
responsiveness by session size and by how long the pane has been loaded. It
answers "should I start a new session, or just reload the pane?". The
2026-11-03 baseline (18 sessions, 327 pane loads) said reload: focused
heartbeat spikes over 1 s per active hour ran 20, 74, 121, 123 across pane
ages of 0-0.5 h, 0.5-1 h, 1-2 h, 2-4 h, while session size barely moved them.

    python3 -I scripts/pane-age-health.py                       # rotated archives + live trace, since 2026-10-15
    python3 -I scripts/pane-age-health.py --since 2026-11-01
    python3 -I scripts/pane-age-health.py --build-since 2026-11-04T04:06:00Z
    python3 -I scripts/pane-age-health.py LOG [LOG ...]         # specific logs or archives

Inputs, all from the trace unless noted:
  - projected-turns ticks (sid, ms): the transcript projection time.
  - focused heartbeat-drift lines: a spike is drift over 1 s. Drift lines are
    logged only when a tick is late, so "active hours" counts the distinct
    minutes that hold at least one focused drift line, divided by 60.
  - pane loads: the first GET __app-info after a 10 s gap (the pane fetches it
    on boot; the route is logged as an entry and an exit line a moment apart).
  - turn-state lines (to=working vs anything else): working minutes count as
    active when measuring active minutes since load.
  - the session file, ~/.claude/projects/<project>/<sid>.jsonl, where
    <project> is the repo root with every non-alphanumeric character turned
    into "-". The file is append-only, so its size at a tick is the byte total
    of the lines whose own timestamp is at or before the tick.

Each spike or tick is attributed to the session of the latest projection tick
within 10 minutes before it, and to the pane load before it. Tables are by
session size, by wall-clock pane age, by ACTIVE minutes since load (a minute
holding a focused drift line or a working turn phase, so a pane left idle
overnight stops looking old), and size x age. --build-since splits every table
into before and after that moment, to score a pane change on the age curve.
"""
from __future__ import annotations

import argparse
import bisect
import glob
import gzip
import json
import os
import re
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bram_state  # noqa: E402

LINE = re.compile(r"^\[(?P<ts>[^\]]+)\] \[(?P<cat>[^\]]+)\] (?P<rest>.*)$")
LOAD_GAP = 10.0  # seconds of silence before a GET __app-info starts a new pane load
CTX_MAX = 600.0  # a spike belongs to the latest tick only if it is this recent
SIZE_BINS = [0, 2e6, 5e6, 10e6, 20e6, 40e6, 1e12]
AGE_BINS = [0, 0.5, 1, 2, 4, 8, 1e9]  # hours


def open_log(path):
    return gzip.open(path, "rt", errors="replace") if path.endswith(".gz") else open(path, errors="replace")


def epoch(ts):
    return datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100 * (len(xs) - 1))))] if xs else 0


def bin_of(v, bins):
    for k in range(len(bins) - 1):
        if bins[k] <= v < bins[k + 1]:
            return k
    return len(bins) - 2


def label(bins, k, unit, scale=1.0):
    a, b = bins[k] / scale, bins[k + 1] / scale
    return f"{a:g}-{b:g} {unit}" if bins[k + 1] < 1e9 else f"{a:g}+ {unit}"


def size_label(k):
    return label(SIZE_BINS, k, "MB", 1e6)


def age_label(k):
    return label(AGE_BINS, k, "h")


def default_logs(trace_dir, since):
    out = []
    for f in sorted(glob.glob(os.path.join(trace_dir, "bram-trace-*.log*"))):
        m = re.match(r"bram-trace-(\d{4}-\d\d-\d\d)", os.path.basename(f))
        if m and m.group(1) >= since:
            out.append(f)
    live = os.path.join(trace_dir, "bram-trace.log")
    if os.path.exists(live):
        out.append(live)
    return out


def session_dir(root, projects):
    return os.path.join(projects, re.sub(r"[^A-Za-z0-9]", "-", str(root)))


class Sizes:
    """Session file size over time, rebuilt from per-line timestamps."""

    def __init__(self, directory):
        self.dir = directory
        self.idx = {}

    def at(self, sid, t):
        if sid not in self.idx:
            p = os.path.join(self.dir, sid + ".jsonl")
            if not os.path.exists(p):
                self.idx[sid] = None
            else:
                times, sizes, total, last = [], [], 0, 0.0
                with open(p, "rb") as f:
                    for line in f:
                        total += len(line)
                        m = re.search(rb'"timestamp":"([^"]+)"', line)
                        if m:
                            try:
                                last = epoch(m.group(1).decode())
                            except ValueError:
                                pass
                        times.append(last)
                        sizes.append(total)
                self.idx[sid] = (times, sizes)
        idx = self.idx[sid]
        if idx is None:
            return None
        i = bisect.bisect_right(idx[0], t)
        return idx[1][i - 1] if i else 0


def read(paths, since_epoch):
    ticks, beats, loads, phases = [], [], [], []
    last_app_info = None
    for path in paths:
        with open_log(path) as f:
            for line in f:
                if "__app-info" not in line and "subkind=projected-turns" not in line \
                        and "subkind=heartbeat-drift" not in line and "[turn-state]" not in line:
                    continue
                m = LINE.match(line)
                if not m:
                    continue
                try:
                    t = epoch(m["ts"])
                except ValueError:
                    continue
                if t < since_epoch:
                    continue
                cat, rest = m["cat"], m["rest"]
                if cat == "route" and "path=__app-info" in rest:
                    if last_app_info is None or t - last_app_info > LOAD_GAP:
                        loads.append(t)
                    last_app_info = max(t, last_app_info or t)
                elif cat == "turn-state":
                    pm = re.search(r" to=(\w+)", rest)
                    if pm:
                        phases.append((t, pm.group(1) == "working"))
                elif cat == "iframe" and rest.startswith("subkind="):
                    kind, _, payload = rest.partition(" ")
                    try:
                        d = json.loads(payload)
                    except ValueError:
                        continue
                    if kind == "subkind=projected-turns" and d.get("reason") == "tick" and d.get("sid"):
                        ticks.append((t, d["sid"], d.get("ms", 0)))
                    elif kind == "subkind=heartbeat-drift" and d.get("focused") and not d.get("hidden"):
                        beats.append((t, d.get("drift_ms", 0)))
    for x in (ticks, beats, loads, phases):
        x.sort()
    return ticks, beats, loads, phases


def active_minutes(beats, phases):
    """Sorted list of minute numbers that were active: a focused drift line, or
    inside a working turn phase."""
    mins = {int(t // 60) for t, _ in beats}
    start = None
    for t, working in phases:
        if working and start is None:
            start = t
        elif not working and start is not None:
            mins.update(range(int(start // 60), int(t // 60) + 1))
            start = None
    return sorted(mins)


def print_table(header, rows):
    print(header)
    for r in rows:
        print(r)


def tables(title, rows, beats_ctx, nloads_note=""):
    """rows: (t, size, age_h, active_h, ms). beats_ctx: (t, size, age_h, active_h, drift)."""
    print(f"\n=== {title}: {len(rows)} ticks, {len(beats_ctx)} focused drift lines {nloads_note}")
    dims = (
        ("session size", 1, SIZE_BINS, size_label),
        ("wall-clock pane age", 2, AGE_BINS, age_label),
        ("active hours since load", 3, AGE_BINS, age_label),
    )
    for name, col, bins, lab in dims:
        print(f"\nby {name}")
        print(f"{'bin':>10} {'ticks':>6} {'p50 ms':>7} {'p90 ms':>7} {'active h':>8} {'spikes>1s':>9} {'per h':>6}")
        for k in range(len(bins) - 1):
            ms = [r[4] for r in rows if bin_of(r[col] if col == 1 else r[col], bins) == k]
            bs = [b for b in beats_ctx if bin_of(b[col], bins) == k]
            if not ms and not bs:
                continue
            hours = len({int(b[0] // 60) for b in bs}) / 60
            sp = sum(1 for b in bs if b[4] > 1000)
            rate = f"{sp / hours:6.1f}" if hours else f"{'-':>6}"
            print(f"{lab(k):>10} {len(ms):>6} {pct(ms, 50):>7.0f} {pct(ms, 90):>7.0f} {hours:>8.1f} {sp:>9} {rate}")
    for what, fn in (("projection p50 ms (n)", "p50"), ("spikes>1s per active hour (hours)", "rate")):
        print(f"\nsize (rows) x wall-clock pane age (cols): {what}; cells with too little data are '-'")
        print(f"{'':>10}" + "".join(f"{age_label(a):>13}" for a in range(len(AGE_BINS) - 1)))
        for s in range(len(SIZE_BINS) - 1):
            cells = []
            for a in range(len(AGE_BINS) - 1):
                if fn == "p50":
                    v = [r[4] for r in rows if bin_of(r[1], SIZE_BINS) == s and bin_of(r[2], AGE_BINS) == a]
                    cells.append(f"{pct(v, 50):.0f} ({len(v)})" if len(v) >= 5 else "-")
                else:
                    bs = [b for b in beats_ctx if bin_of(b[1], SIZE_BINS) == s and bin_of(b[2], AGE_BINS) == a]
                    h = len({int(b[0] // 60) for b in bs}) / 60
                    sp = sum(1 for b in bs if b[4] > 1000)
                    cells.append(f"{sp / h:.0f} ({h:.1f})" if h >= 0.25 else "-")
            print(f"{size_label(s):>10}" + "".join(f"{c:>13}" for c in cells))


def main():
    root = Path(__file__).resolve().parent.parent
    traces = bram_state.trace_dir(root)
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("logs", nargs="*", help="trace logs or .gz archives (default: archives since --since plus the live trace)")
    ap.add_argument("--since", default="2026-10-15", metavar="DAY", help="first day to read, YYYY-MM-DD (default 2026-10-15)")
    ap.add_argument("--build-since", metavar="ISO", help="split every table into before/after this moment, e.g. 2026-11-04T04:06:00Z")
    ap.add_argument("--trace-dir", default=str(traces), help="archive directory")
    ap.add_argument("--projects-dir", default=os.path.expanduser("~/.claude/projects"), help="where the agent keeps session jsonl")
    args = ap.parse_args()

    logs = args.logs or default_logs(args.trace_dir, args.since)
    missing = [p for p in logs if not os.path.exists(p)]
    if missing or not logs:
        print("no such log: " + ", ".join(missing) if missing else "no logs found", file=sys.stderr)
        return 2
    try:
        since = epoch(args.since + "T00:00:00Z")
        build = epoch(args.build_since) if args.build_since else None
    except ValueError:
        print("bad --since / --build-since", file=sys.stderr)
        return 2

    ticks, beats, loads, phases = read(logs, since)
    sizes = Sizes(session_dir(root, args.projects_dir))
    amins = active_minutes(beats, phases)

    def pane(t):
        i = bisect.bisect_right(loads, t)
        if not i:
            return None
        load = loads[i - 1]
        active = bisect.bisect_right(amins, int(t // 60)) - bisect.bisect_left(amins, int(load // 60))
        return (t - load) / 3600, max(0, active) / 60

    rows = []
    for t, sid, ms in ticks:
        sz, p = sizes.at(sid, t), pane(t)
        if sz is not None and p is not None:
            rows.append((t, sz, p[0], p[1], ms, sid))
    tick_t = [r[0] for r in rows]
    bc = []
    for t, drift in beats:
        i = bisect.bisect_right(tick_t, t)
        p = pane(t)
        if i and t - tick_t[i - 1] <= CTX_MAX and p is not None:
            bc.append((t, rows[i - 1][1], p[0], p[1], drift))

    print(f"logs: {len(logs)}; since {args.since}; ticks used {len(rows)} of {len(ticks)}; "
          f"sessions {len({r[5] for r in rows})}; pane loads {len(loads)}; focused drift lines {len(beats)}")
    if build is None:
        tables("all data", rows, bc)
    else:
        tables(f"before {args.build_since}", [r for r in rows if r[0] < build], [b for b in bc if b[0] < build])
        tables(f"after {args.build_since}", [r for r in rows if r[0] >= build], [b for b in bc if b[0] >= build])
    return 0


if __name__ == "__main__":
    sys.exit(main())
