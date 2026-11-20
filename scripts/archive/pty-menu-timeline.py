#!/usr/bin/env python3
"""Timeline / succession analysis of Bram PTY menu detection traces.

Complements `pty-menu-scan-report.py` (which classifies *individual*
excerpts as covered/missed/unknown). This tool reconstructs the *temporal
sequence* of menus: it groups `[pty-menu-scan]` lines into per-menu
**episodes** (split on identity change and on a time gap), marks each
episode FIRED vs skip-only, and flags two succession hazards:

  (a) DISTINCT-SHAPE MISS  — a real menu that never fired, appearing
      shortly after a different menu that did fire.
  (b) SAME-SHAPE SKIP-TAIL — a fired episode that then ran a sustained
      menu-bearing skip tail after its last fire (the case where two
      same-shape menus back-to-back would otherwise merge and hide the
      second).

Known limit: the scanner samples the PTY at ~200-300 ms; a menu shown and
dismissed faster than one scan interval leaves NO frame (neither fire nor
skip) and is invisible here. Absence of candidates ≠ absence of
sub-cadence misses — use a live repro for that regime.

Usage:
  scripts/pty-menu-timeline.py                 # live bram-trace.log
  scripts/pty-menu-timeline.py --all           # live + all rotated logs
  scripts/pty-menu-timeline.py path1 path2 …   # explicit logs
"""

from __future__ import annotations

import datetime as dt
import glob
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import bram_state  # noqa: E402

TRACE_DIR = bram_state.trace_dir(ROOT)
LIVE = TRACE_DIR / "bram-trace.log"

TS = re.compile(r"^\[(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+)Z\]")
HEADER = re.compile(
    r"Do you want to (make this edit to|create|overwrite|insert this cell into|"
    r"delete this cell from|proceed|allow this connection|use this API key|allow)"
    r"([^?\n]{0,40})"
)
DECOY_MARKERS = ["op=skip", "op=fire", "pty-menu-scan", "grep ", "excerpt=",
                 "Let me", "=== ", ".log", "header to the real", "--project", "npx "]
REALOPT = re.compile(r"❯ ?1\.|1\. Yes|1\. Allow|1\. Yes, proceed")

GAP_MS = 2500          # episode split when same-key scans are this far apart
QUICK_S = 12           # "succession" window for detector (a)
TAIL_MIN = 4           # min skip frames after last fire for detector (b)


def ts_ms(s: str) -> float:
    return dt.datetime.strptime(s, "%Y-%m-%dT%H:%M:%S.%f").timestamp() * 1000


def parse(path: str):
    rows = []
    for line in open(path, encoding="utf-8", errors="replace"):
        if "[pty-menu-scan]" not in line:
            continue
        m = TS.match(line)
        if not m:
            continue
        op = (re.search(r"op=(\w+)", line) or [None, None])[1]
        mb = (re.search(r"menu_bearing=(\w+)", line) or [None, None])[1]
        if op == "fire" or (op == "skip" and mb == "true"):
            ex = re.search(r"excerpt='(.*)'", line)
            ex = ex.group(1) if ex else ""
            hm = HEADER.search(ex)
            key = (hm.group(1) + hm.group(2)).strip()[:30] if hm else "(no-header)"
            decoy = any(d in ex for d in DECOY_MARKERS)
            real = bool(REALOPT.search(ex)) and not decoy
            rows.append((ts_ms(m.group(1)), m.group(1)[11:], op, key, real, decoy, ex[:90]))
    return rows


def episodes(rows):
    eps, cur = [], None
    for t, hh, op, key, real, decoy, ex in rows:
        if cur and (t - cur["end"] <= GAP_MS) and key == cur["key"]:
            cur["end"] = t
            cur["fired"] = cur["fired"] or op == "fire"
            cur["real"] = cur["real"] or real
            if op == "fire":
                cur["last_fire"], cur["tail"] = t, 0
            else:
                cur["tail"] += 1
        else:
            if cur:
                eps.append(cur)
            cur = dict(start=t, end=t, hh=hh, key=key, fired=(op == "fire"),
                       real=real, decoy=decoy, ex=ex,
                       last_fire=(t if op == "fire" else None),
                       tail=(0 if op == "fire" else 1))
    if cur:
        eps.append(cur)
    return eps


def report(path: str) -> int:
    eps = episodes(parse(path))
    cand, out, prev = 0, [], None
    for e in eps:
        gap = (e["start"] - prev["end"]) / 1000 if prev else None
        flag = ""
        if prev and prev["fired"] and not e["fired"] and e["real"] and gap is not None and gap <= QUICK_S:
            flag = " <<< DISTINCT-SHAPE MISS"
        if e["fired"] and e["real"] and e["tail"] >= TAIL_MIN:
            secs = (e["end"] - e["last_fire"]) / 1000 if e["last_fire"] else 0
            flag = f" <<< SAME-SHAPE SKIP-TAIL (tail={e['tail']}, {secs:.1f}s)"
        if flag:
            cand += 1
            out.append(f"  {e['hh']} FIRED={e['fired']} real={e['real']} [{e['key']}]{flag}")
            out.append(f"        excerpt: {e['ex']}")
        prev = e
    print(f"===== {Path(path).name}: {len(eps)} episodes, {cand} candidate(s) =====")
    print("\n".join(out) if out else "  (no succession-hazard candidates)")
    return cand


def main(argv):
    if argv and argv[0] == "--all":
        paths = [str(LIVE)] + sorted(glob.glob(str(TRACE_DIR / "bram-trace-*.log")))
    elif argv:
        paths = argv
    else:
        paths = [str(LIVE)]
    total = sum(report(p) for p in paths)
    print(f"\nTOTAL candidates: {total}")


if __name__ == "__main__":
    main(sys.argv[1:])
