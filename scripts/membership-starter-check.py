#!/usr/bin/env python3
"""Check membership's answer for every demo starter against a live Bram.

The membership engine's pure functions have Rust tests; this checks the
integration they don't reach: the engine run against a real repo, its memo, the
claim records and refs, and the board serve. That is where judell/bram#437
lived (a scratch-index seed that failed on two ordinary board shapes, found in
the field after five refusals).

It hydrates one demo board with the chosen starters (all by default), launches
the locally built Bram on it, waits for a fresh membership compute, reads
/__worklist over the loopback port, and checks each starter's entry in
demo_instance.EXPECTATIONS. It also fails on any membership give-up
(`op=membership … reason=`), unexplained line, or conservation breach the run
traced. Exit status is the number of failed checks. On failure the demo is
left running on the failing board so it can be looked at.

    python3 -I scripts/membership-starter-check.py              # every starter
    python3 -I scripts/membership-starter-check.py --starter dependency
    python3 -I scripts/membership-starter-check.py --keep       # leave it running

One board for all starters means one launch, but one broken starter can spoil
the shared compute (pre-#437, `membership-unanswerable` would have failed every
starter). When that happens, rerun with --starter to narrow it.

Needs a windowing session (it launches the app), so it runs on demand, not in
CI, like scripts/setup-harness.sh.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))
import demo_instance  # noqa: E402

LAUNCHER = SCRIPTS / "demo-instance.sh"
KEEP_BOARDS = 3  # passing check-* boards kept after a run
FRESH = re.compile(r"\[claim-interval\] op=membership paths=\d+ .*cached=false fresh=true")
# A compute that gave up: `paths=0 … cached=false fresh=false`, with a
# `reason=` since #437 and without one on older builds. (Since #437 an empty
# universe is an answer, `fresh=true`, so this shape is always a give-up.)
BAD = re.compile(
    r"\[claim-interval\] op=(membership paths=0 \S+ \S+ \S+ cached=false fresh=false[^\n]*"
    r"|membership-unexplained \S+"
    r"|membership-conservation-broken[^\n]*)"
)
MEMBERSHIP = re.compile(r"\[claim-interval\] op=membership paths=")


def run(repo: Path, *args: str, tolerate: str = "") -> None:
    proc = subprocess.run(
        [str(LAUNCHER), "--repo", str(repo), *args], capture_output=True, text=True
    )
    if proc.returncode != 0 and not (tolerate and tolerate in proc.stdout + proc.stderr):
        sys.exit(f"demo-instance {' '.join(args)} failed:\n{proc.stdout}{proc.stderr}")


def trace_lines(repo: Path) -> list[str]:
    path = repo / "resources/bram-traces/bram-trace.log"
    try:
        return path.read_text(errors="replace").splitlines()
    except FileNotFoundError:
        return []


def wait_for_fresh(repo: Path, start: int, timeout: float) -> list[str]:
    deadline = time.time() + timeout
    while time.time() < deadline:
        lines = trace_lines(repo)[start:]
        if any(FRESH.search(l) for l in lines):
            time.sleep(2)  # let the serve after the compute land too
            return trace_lines(repo)[start:]
        time.sleep(1)
    last = [l for l in trace_lines(repo)[start:] if MEMBERSHIP.search(l)][-3:]
    print(f"FAIL  no fresh membership compute within {timeout:.0f}s. Last membership lines:")
    for l in last or ["(none)"]:
        print(f"      {l}")
    print("demo left running; stop with scripts/demo-instance.sh stop")
    sys.exit(1)


def board(repo: Path) -> dict:
    port = (repo / "resources/.bram-port").read_text().strip()
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/__worklist", timeout=30) as r:
        return json.load(r)


def owners_of(entry: dict) -> list[str]:
    return sorted(entry.get("owners") or [])


def pair(entry: dict, key: str) -> tuple[int, int]:
    b = entry.get(key) or {}
    return (int(b.get("added", 0)), int(b.get("removed", 0)))


def check(starters: list[str], data: dict, failures: list[str]) -> dict[str, list[str]]:
    by_path = (data.get("membership") or {}).get("byPath") or {}
    items = {it.get("id"): it for it in data.get("items") or []}
    results: dict[str, list[str]] = {}
    for s in starters:
        exp = demo_instance.EXPECTATIONS[s]
        sub = lambda t: t.replace("{p}", s)  # noqa: E731
        problems: list[str] = []
        for path, want in (exp.get("paths") or {}).items():
            path = sub(path)
            got = by_path.get(path)
            if got is None:
                problems.append(f"{path}: no membership entry")
                continue
            want_owners = sorted(sub(o) for o in want.get("owners", []))
            if owners_of(got) != want_owners:
                problems.append(f"{path}: owners {owners_of(got)}, expected {want_owners}")
            for key in ("single", "joint", "unowned"):
                w = tuple(want.get(key, (0, 0)))
                g = pair(got, key)
                if g != w:
                    problems.append(f"{path}: {key} {g}, expected {w}")
        for path in exp.get("absent") or []:
            path = sub(path)
            if path in by_path:
                problems.append(f"{path}: expected no membership entry, got owners {owners_of(by_path[path])}")
        for item_id, w in (exp.get("will_commit") or {}).items():
            item_id = sub(item_id)
            wc = (items.get(item_id) or {}).get("willCommit") or {}
            g = (wc.get("added"), wc.get("removed"))
            if g != tuple(w):
                problems.append(f"{item_id}: willCommit {g}, expected {tuple(w)}")
        results[s] = problems
        failures.extend(f"{s}: {p}" for p in problems)
    return results


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--repo", default=str(SCRIPTS.parent.parent / "bram-demo-instance"))
    ap.add_argument("--starter", action="append", choices=demo_instance.STARTERS)
    ap.add_argument("--timeout", type=float, default=90)
    ap.add_argument("--keep", action="store_true", help="leave the demo running even on success")
    args = ap.parse_args()
    repo = Path(args.repo).resolve()
    starters = args.starter or list(demo_instance.STARTERS)

    name = "check-" + time.strftime("%Y%m%d-%H%M%S")
    run(repo, "stop", tolerate="no recorded demo process")
    froms =[a for s in starters for a in ("--from", s)]
    run(repo, "new", name, *froms)
    start = len(trace_lines(repo))
    run(repo, "launch")
    lines = wait_for_fresh(repo, start, args.timeout)
    data = board(repo)

    failures: list[str] = []
    if data.get("membershipStale"):
        failures.append("board: membershipStale is true after a fresh compute")
    bad = sorted({m.group(0) for l in lines for m in [BAD.search(l)] if m})
    failures.extend(f"trace: {b}" for b in bad)
    results = check(starters, data, failures)

    width = max(len(s) for s in starters)
    for s in starters:
        mark = "ok  " if not results[s] else "FAIL"
        print(f"{mark}  {s.ljust(width)}  {'; '.join(results[s])}")
    for f in failures:
        if not f.startswith(tuple(f"{s}:" for s in starters)):
            print(f"FAIL  {f}")
    print(f"\n{len(failures)} failed check(s); scenario {name}")
    if failures or args.keep:
        print(f"demo left running on scenario/{name}; stop with scripts/demo-instance.sh stop")
    else:
        run(repo, "stop")
        # One board per run piles up; a passing board is never looked at again.
        # A failing run prunes nothing, so its board survives for inspection.
        run(repo, "prune", "--match", "check-*", "--keep", str(KEEP_BOARDS))
    return len(failures)


if __name__ == "__main__":
    sys.exit(main())
