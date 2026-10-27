#!/usr/bin/env bash
# Build Bram (debug) and log the build, so slow builds explain themselves.
#
#   scripts/build.sh [extra cargo args]
#
# Same as `cargo build --manifest-path src-tauri/Cargo.toml`: output and exit
# status pass through unchanged. Each build appends one JSON line to
# resources/build-log.jsonl (gitignored):
#
#   started, seconds, exit   when, how long, cargo's status
#   head, head_moved         HEAD, and whether it changed since the last logged
#                            build (a pull, or partner commits)
#   lines_changed            src-tauri/ lines added+removed since the last logged
#                            build's working tree; cargo_lock_changed likewise
#   other_cargo              cargo/rustc processes already running at start
#   load                     1-minute load average at start
#   caller                   the parent process's command name
#
# Why (build-log-real-builds, #452): scripts/build-timings.sh times synthetic
# edits on a quiet tree. On 2026-10-27 it measured 24.68 s (appended line) and
# 33.75 s (shifted lines) while real builds that day took 2m55s and 1m38s. The
# difference was something the synthetic run can't see. This records it.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/src-tauri/Cargo.toml"
LOG="$ROOT/resources/build-log.jsonl"
mkdir -p "$ROOT/resources"

command -v python3 > /dev/null 2>&1 || { exec cargo build --manifest-path "$MANIFEST" "$@"; }

# A snapshot of the working tree (git stash create makes a commit object
# without touching the index or the stash list; empty when the tree is clean).
tree_ref="$(git -C "$ROOT" stash create 2>/dev/null)"
[ -n "$tree_ref" ] || tree_ref="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)"
head="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)"
other="$( (pgrep -x cargo; pgrep -x rustc) 2>/dev/null | wc -l | tr -d ' ')"
caller="$(ps -o comm= -p "$PPID" 2>/dev/null | sed 's#.*/##')"
t0="$(python3 -c 'import time; print("%.3f" % time.time())')"

cargo build --manifest-path "$MANIFEST" "$@"
status=$?

t1="$(python3 -c 'import time; print("%.3f" % time.time())')"
python3 -I - "$ROOT" "$LOG" "$t0" "$t1" "$status" "$head" "$tree_ref" "$other" "$caller" <<'PY'
import json, os, subprocess, sys, time
root, log, t0, t1, status, head, tree, other, caller = sys.argv[1:]
prev = None
try:
    with open(log) as f:
        lines = [l for l in f if l.strip()]
    prev = json.loads(lines[-1]) if lines else None
except (OSError, ValueError):
    prev = None

def git(*args):
    return subprocess.run(["git", "-C", root, *args], capture_output=True, text=True).stdout

lines_changed = None
lock_changed = None
if prev and prev.get("tree") and tree:
    added = removed = 0
    lock_changed = False
    for row in git("diff", "--numstat", prev["tree"], tree, "--", "src-tauri").splitlines():
        a, r, path = (row.split("\t") + ["", "", ""])[:3]
        if a.isdigit():
            added += int(a)
        if r.isdigit():
            removed += int(r)
        if path.endswith("Cargo.lock"):
            lock_changed = True
    lines_changed = added + removed

rec = {
    "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(float(t0))),
    "seconds": round(float(t1) - float(t0), 2),
    "exit": int(status),
    "head": head[:12],
    "head_moved": (prev.get("head") != head[:12]) if prev else None,
    "lines_changed": lines_changed,
    "cargo_lock_changed": lock_changed,
    "other_cargo": int(other or 0),
    "load": round(os.getloadavg()[0], 2),
    "caller": caller,
    "tree": tree,
}
with open(log, "a") as f:
    f.write(json.dumps(rec) + "\n")
PY
exit "$status"
