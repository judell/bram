#!/usr/bin/env python3
"""Summarize what happens after a pane send, from Bram trace logs.

Evidence for the Transcript auto-switch policy
(worklist item trace-send-followup-for-transcript-autoswitch).

Usage (from the repo root):

    scripts/send-switch-report.py [PROJECT_ROOT ...]

Each root's resources/bram-traces/bram-trace*.log* (rotated .gz archives
included) is read. Two reports per root:

1. Reconstruction, which works on any history: plain chat sends
   (`to-turn` stage=source whose text is not a lifecycle payload), the tab
   the pane was on (from `tools-route-save`), whether the user reached the
   Transcript within WINDOW seconds, and whether they went straight back.
   It can't tell "didn't want to look" from "watched the terminal".

2. send-followup, only once the observe-only trace family exists: per-tab
   and per-kind arrival rates, `via` (auto/chip/nav), how arrivals landed
   (follow vs restore-reading, and how often a restore was corrected by
   scrolling to the bottom), bounce-backs, turns that ended while the user
   was elsewhere, and screenshot sends.
"""
import glob
import gzip
import json
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime

WINDOW = 60
BOUNCE = 20
LINE = re.compile(r"^\[([^\]]+)\] \[iframe\] subkind=([a-z0-9-]+) (\{.*\})\s*$")
LIFECYCLE = re.compile(r"^\s*(approved:|drop:|iterate:|skip-worklist:|voice:)")
WANTED = ("subkind=to-turn", "subkind=tools-route", "subkind=send-followup")


def ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def events(root):
    for f in sorted(glob.glob(f"{root}/resources/bram-traces/bram-trace*.log*")):
        opener = gzip.open if f.endswith(".gz") else open
        try:
            with opener(f, "rt", errors="replace") as fh:
                for line in fh:
                    if not any(w in line for w in WANTED):
                        continue
                    m = LINE.match(line)
                    if not m:
                        continue
                    try:
                        yield ts(m.group(1)), m.group(2), json.loads(m.group(3))
                    except ValueError:
                        continue
        except (OSError, EOFError) as e:
            print(f"  (skipped {f}: {e})", file=sys.stderr)


def tab(route):
    r = (route or "").lstrip("#/").split("/")[0].split("?")[0]
    return r or "root"


def pct(a, b):
    return f"{100 * a // b}%" if b else "-"


def median(xs):
    xs = sorted(xs)
    return xs[len(xs) // 2] if xs else "-"


def reconstruction(evs):
    route, sends, navs = None, [], []
    for t, kind, d in evs:
        if kind == "tools-route-save":
            navs.append((t, d.get("previous"), d.get("route")))
            route = d.get("route")
        elif kind == "tools-route-boot":
            route = d.get("route") or route
        elif kind == "to-turn" and d.get("stage") == "source":
            text = d.get("textPreview") or ""
            if not LIFECYCLE.match(text):
                sends.append((t, tab(route), text))
    uniq = []
    for s in sends:  # the same send is sometimes logged twice
        if uniq and abs(s[0] - uniq[-1][0]) < 1 and s[2] == uniq[-1][2]:
            continue
        uniq.append(s)
    stats, delays = defaultdict(Counter), defaultdict(list)
    for t, where, _ in uniq:
        stats[where]["sends"] += 1
        if where == "transcript":
            continue
        went = next((n for n in navs if t < n[0] <= t + WINDOW and tab(n[2]) == "transcript"), None)
        if not went:
            continue
        stats[where]["went"] += 1
        delays[where].append(round(went[0] - t, 1))
        back = next((n for n in navs
                     if went[0] < n[0] <= went[0] + BOUNCE and tab(n[1]) == "transcript"), None)
        if back and tab(back[2]) == where:
            stats[where]["back"] += 1
    return uniq, stats, delays


def followup(evs):
    sends, arrives, leaves, corrected, ends = {}, {}, {}, set(), []
    for t, kind, d in evs:
        if kind != "send-followup":
            continue
        op = d.get("op")
        if op == "send":
            sends[d.get("sendId")] = d
        elif op == "arrive":
            arrives[d.get("sendId")] = d
        elif op == "leave":
            leaves[d.get("sendId")] = d
        elif op == "arrive-corrected":
            corrected.add(d.get("sendId"))
        elif op == "turn-end":
            ends.append(d)
    return sends, arrives, leaves, corrected, ends


def report(root):
    evs = sorted(events(root), key=lambda e: e[0])
    print(f"== {root}")
    uniq, stats, delays = reconstruction(evs)
    if uniq:
        first = datetime.fromtimestamp(uniq[0][0]).date()
        last = datetime.fromtimestamp(uniq[-1][0]).date()
        print(f"-- reconstruction: {len(uniq)} plain chat sends, {first} .. {last}")
        print(f"   {'from':<14}{'sends':>7}{'to Transcript <' + str(WINDOW) + 's':>22}"
              f"{'back <' + str(BOUNCE) + 's':>12}{'median s':>10}")
        for where, c in sorted(stats.items(), key=lambda kv: -kv[1]["sends"]):
            went = "-" if where == "transcript" else f"{c['went']} ({pct(c['went'], c['sends'])})"
            print(f"   {where:<14}{c['sends']:>7}{went:>22}{c['back']:>12}{median(delays[where]):>10}")
    else:
        print("-- reconstruction: no plain chat sends found")

    sends, arrives, leaves, corrected, ends = followup(evs)
    if not sends:
        print("-- send-followup: no lines yet\n")
        return
    print(f"-- send-followup: {len(sends)} sends")
    by = defaultdict(Counter)
    for sid, d in sends.items():
        key = (d.get("tab"), d.get("kind"))
        by[key]["sends"] += 1
        by[key]["image"] += bool(d.get("hasImage"))
        by[key]["savedReading"] += bool(d.get("savedReading"))
        a = arrives.get(sid)
        if a:
            by[key]["arrive"] += 1
            by[key]["via_" + str(a.get("via"))] += 1
            by[key]["landed_" + str(a.get("landed"))] += 1
            by[key]["corrected"] += sid in corrected
            lv = leaves.get(sid)
            if lv and lv.get("tab") == d.get("tab") and lv.get("dwellMs", 1e9) < BOUNCE * 1000:
                by[key]["back"] += 1
    print(f"   {'tab/kind':<26}{'sends':>6}{'arrive':>8}{'auto':>6}{'chip':>6}{'nav':>6}"
          f"{'restore':>8}{'fixed':>7}{'back':>6}{'img':>5}{'saved':>7}")
    for (t, k), c in sorted(by.items(), key=lambda kv: -kv[1]["sends"]):
        print(f"   {(t or '?') + '/' + (k or '?'):<26}{c['sends']:>6}{c['arrive']:>8}{c['via_auto']:>6}"
              f"{c['via_chip']:>6}{c['via_nav']:>6}{c['landed_restore-reading']:>8}{c['corrected']:>7}"
              f"{c['back']:>6}{c['image']:>5}{c['savedReading']:>7}")
    away = [e for e in ends if e.get("tab") != "transcript"]
    stale = [e for e in ends if e.get("sinceSendMs", -1) < 0 or e.get("sinceSendMs", 0) > 600000]
    print(f"   turn-ends: {len(ends)}; off the Transcript: {len(away)} "
          f"(unseen>0: {sum(1 for e in away if e.get('unseen', 0) > 0)}); "
          f"no pane send in the last 10 min (terminal-typed or agent-started): {len(stale)}\n")


if __name__ == "__main__":
    for root in sys.argv[1:] or ["."]:
        report(root)
