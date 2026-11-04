#!/usr/bin/env bash
# Build Bram (debug) and log the build, so slow builds explain themselves.
#
#   scripts/build.sh [extra cargo args]
#
# Same as `cargo build --manifest-path src-tauri/Cargo.toml`: output and exit
# status pass through unchanged. Each build appends one JSON line to
# build-log.jsonl in the state folder (.bram/ or resources/; gitignored):
#
#   started, seconds, exit   when, how long, cargo's status
#   head, head_moved         HEAD, and whether it changed since the last logged
#                            build (a pull, or partner commits)
#   lines_changed            src-tauri/ lines added+removed since the last logged
#                            build's working tree; cargo_lock_changed likewise
#   other_cargo              cargo/rustc processes already running at start
#   load                     1-minute load average at start
#   caller                   the parent process's command name
#   deps_files               files in target/debug/deps after the build
#   target_bytes             bytes under the target dir after the build
#   free_bytes               free bytes on the target dir's volume after it
#   stale_debug_tree_days    days since a sibling profile tree (a target-triple
#                            <triple>/debug beside the debug/ this builds) was
#                            last written; null when there is none
#   platform                 sys.platform
#
# Past BRAM_DEPS_WARN files (default 100000) in deps, it prints one warning
# line to stderr. Before building, it also warns when free space is under
# BRAM_DISK_WARN_GB (default 5) or the target dir is over BRAM_TARGET_WARN_GB
# (default 20) (build-warns-on-disk-and-target-size: on 2026-11-03 a Windows
# build died of a full disk while deps held only 1,713 files). It never
# cleans: a clean forces a full rebuild, which is someone's choice to make
# (build-deps-growth-guard, #452). scripts/build.ps1 is the Windows twin and
# logs the same fields; it alone renames a running exe aside, since Unix
# replaces a running binary's file without trouble.
#
# BRAM_BUILD_MEASURE_ONLY=1 runs only the measurements and warnings, with no
# build and no log line.
#
# Why (build-log-real-builds, #452): scripts/build-timings.sh times synthetic
# edits on a quiet tree. On 2026-10-27 it measured 24.68 s (appended line) and
# 33.75 s (shifted lines) while real builds that day took 2m55s and 1m38s. The
# difference was something the synthetic run can't see. This records it.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/src-tauri/Cargo.toml"
. "$ROOT/scripts/bram-state.sh"
STATE="$(bram_state_dir "$ROOT")"
LOG="$STATE/build-log.jsonl"
mkdir -p "$STATE"

command -v python3 > /dev/null 2>&1 || { exec cargo build --manifest-path "$MANIFEST" "$@"; }

# Target-dir measurements, shared by the pre-build warnings and the log line.
# Prints one JSON object; with "warn", also prints the disk and size warnings
# to stderr.
read -r -d '' MEASURE_PY <<'PY'
import json, os, shutil, sys, time
root, mode = sys.argv[1], sys.argv[2]
target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(root, "src-tauri", "target")

def tree_bytes(path):
    total, stack = 0, [path]
    while stack:
        try:
            it = os.scandir(stack.pop())
        except OSError:
            continue
        with it:
            for e in it:
                try:
                    # Don't follow links or junctions (the Windows debug dir
                    # holds an app junction into the checkout).
                    if e.is_symlink() or getattr(e, "is_junction", lambda: False)():
                        continue
                    if e.is_dir(follow_symlinks=False):
                        stack.append(e.path)
                    else:
                        total += e.stat(follow_symlinks=False).st_size
                except OSError:
                    pass
    return total

def newest_write(path):
    newest = None
    try:
        with os.scandir(path) as it:
            for e in it:
                if e.name == ".cargo-lock":
                    continue
                m = e.stat(follow_symlinks=False).st_mtime
                newest = m if newest is None or m > newest else newest
    except OSError:
        pass
    return newest

target_bytes = tree_bytes(target) if os.path.isdir(target) else None
try:
    free_bytes = shutil.disk_usage(target if os.path.isdir(target) else root).free
except OSError:
    free_bytes = None
# This script builds target/debug; a <triple>/debug tree beside it is a sibling
# profile tree that only grows stale here (and vice versa on Windows).
stale = None
try:
    for name in os.listdir(target):
        sib = os.path.join(target, name, "debug")
        if name not in ("debug", "release") and os.path.isdir(sib):
            m = newest_write(sib)
            if m is not None and (stale is None or m > stale):
                stale = m
except OSError:
    pass
stale_days = round((time.time() - stale) / 86400, 1) if stale is not None else None
print(json.dumps({"target_bytes": target_bytes, "free_bytes": free_bytes,
                  "stale_debug_tree_days": stale_days}))

if mode == "warn":
    def env_gb(name, default):
        try:
            return float(os.environ.get(name, default))
        except ValueError:
            return float(default)
    disk_gb, target_gb = env_gb("BRAM_DISK_WARN_GB", 5), env_gb("BRAM_TARGET_WARN_GB", 20)
    remedy = "Remedies: cargo clean --manifest-path src-tauri/Cargo.toml --profile dev"
    if stale_days is not None:
        remedy += "; or delete the sibling profile tree beside target/debug (last written %s days ago)" % stale_days
    if free_bytes is not None and free_bytes < disk_gb * 2**30:
        sys.stderr.write("[build.sh] only %.1f GB free on the target's volume (warn under %g GB, "
                         "BRAM_DISK_WARN_GB). A full disk fails the link and breaks traces and "
                         "worklist writes. %s\n" % (free_bytes / 2**30, disk_gb, remedy))
    if target_bytes is not None and target_bytes > target_gb * 2**30:
        sys.stderr.write("[build.sh] the target dir holds %.1f GB (warn over %g GB, "
                         "BRAM_TARGET_WARN_GB). %s\n" % (target_bytes / 2**30, target_gb, remedy))
PY

python3 -I -c "$MEASURE_PY" "$ROOT" warn > /dev/null
[ "${BRAM_BUILD_MEASURE_ONLY:-}" = 1 ] && exit 0

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
measured="$(python3 -I -c "$MEASURE_PY" "$ROOT" log)"
python3 -I - "$ROOT" "$LOG" "$t0" "$t1" "$status" "$head" "$tree_ref" "$other" "$caller" "$measured" <<'PY'
import json, os, subprocess, sys, time
root, log, t0, t1, status, head, tree, other, caller, measured = sys.argv[1:]
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

# Cargo never deletes old artifacts, and rustc lists this directory on every
# invocation; at 982,330 files it cost a library rebuild ~8 s of waiting (#452).
target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(root, "src-tauri", "target")
deps = os.path.join(target, "debug", "deps")
deps_files = None
try:
    with os.scandir(deps) as it:
        deps_files = sum(1 for _ in it)
except OSError:
    pass

rec = {
    "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(float(t0))),
    "seconds": round(float(t1) - float(t0), 2),
    "exit": int(status),
    "head": head[:12],
    "head_moved": (prev.get("head") != head[:12]) if prev else None,
    "lines_changed": lines_changed,
    "cargo_lock_changed": lock_changed,
    "other_cargo": int(other or 0),
    "load": round(os.getloadavg()[0], 2) if hasattr(os, "getloadavg") else None,
    "caller": caller,
    "tree": tree,
    "deps_files": deps_files,
}
try:
    rec.update(json.loads(measured))
except ValueError:
    pass
rec["platform"] = sys.platform
with open(log, "a") as f:
    f.write(json.dumps(rec) + "\n")

try:
    limit = int(os.environ.get("BRAM_DEPS_WARN", "100000"))
except ValueError:
    limit = 100000
if deps_files is not None and deps_files > limit:
    sys.stderr.write(
        "[build.sh] target/debug/deps has %d files; builds slow down as it grows (#452). "
        "Run: cargo clean --manifest-path src-tauri/Cargo.toml --profile dev\n" % deps_files
    )
PY

# The documented launch is the ./bram symlink at the repo root, and nothing
# else creates it (docs/developing-bram.md, Build vs. reload boundary).
binary="$ROOT/src-tauri/target/debug/bram"
if [ "$status" -eq 0 ] && [ -x "$binary" ]; then
  ln -sfn "$binary" "$ROOT/bram"
fi
exit "$status"
