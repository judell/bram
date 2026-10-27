#!/usr/bin/env bash
# Measure Bram's debug build times, so "builds feel slower" has a baseline.
#
# Why this exists: every slow-build report so far (#451) has been a feeling.
# docs/developing-bram.md already requires that a claim about performance be
# recorded with the same instrument that will verify the "after"; the pane has
# one, builds had none.
#
#   scripts/build-timings.sh                  the cheap numbers, in seconds
#   scripts/build-timings.sh --clean          also time a full build into a
#                                             scratch target dir (minutes)
#   scripts/build-timings.sh --keep-scratch   leave that scratch dir behind
#   scripts/build-timings.sh --out FILE       also write the table to FILE
#
# Prints a markdown table; redirect or --out to save it. Debug only, never
# --release: debug is the build validation actually uses (docs/developing-bram.md,
# "Build vs. reload boundary"). Building replaces target/debug/bram; a running
# Bram keeps its own inode and is unaffected.
#
# The incremental measurement appends one line to src-tauri/src/lib.rs to give
# cargo something to recompile, then restores the file's exact bytes from a
# saved copy via a trap, so an interrupted run still leaves the tree as it
# found it. Dirty-tree edits are preserved, not discarded.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/src-tauri/Cargo.toml"
LIB="$ROOT/src-tauri/src/lib.rs"
TARGET="$ROOT/src-tauri/target"

CLEAN=0
KEEP_SCRATCH=0
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --clean) CLEAN=1 ;;
    --keep-scratch) KEEP_SCRATCH=1 ;;
    --out) shift; OUT="${1:-}"; [ -n "$OUT" ] || { echo "--out needs a path" >&2; exit 2; } ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

[ -f "$MANIFEST" ] || { echo "no manifest at $MANIFEST" >&2; exit 1; }
[ -f "$LIB" ] || { echo "no lib.rs at $LIB" >&2; exit 1; }
command -v cargo > /dev/null 2>&1 || { echo "cargo not on PATH" >&2; exit 1; }
command -v python3 > /dev/null 2>&1 || { echo "python3 needed for sub-second timing" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bram-buildtimings.XXXXXX")"
BACKUP=""
SCRATCH=""

on_exit() {
  # Restore lib.rs first: a build probe must never outlive the run.
  if [ -n "$BACKUP" ] && [ -f "$BACKUP" ]; then
    cp -p "$BACKUP" "$LIB"
  fi
  if [ -n "$SCRATCH" ] && [ "$KEEP_SCRATCH" = 0 ]; then
    rm -rf "$SCRATCH"
  fi
  rm -rf "$WORK"
}
trap on_exit EXIT INT TERM

now() { python3 -c 'import time; print("%.3f" % time.time())'; }
elapsed() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.2f", b - a }'; }

build() {  # build <logfile> [extra cargo args...]
  local log="$1"; shift
  cargo build --manifest-path "$MANIFEST" "$@" > "$log" 2>&1 || {
    echo "build failed; last 20 lines of $log:" >&2
    tail -20 "$log" >&2
    exit 1
  }
}

timed_build() {  # timed_build <logfile> -> prints seconds
  local t0 t1
  t0="$(now)"
  build "$1"
  t1="$(now)"
  elapsed "$t0" "$t1"
}

rows=()
row() { rows+=("$1|$2|$3"); }

# --- warm-up ---------------------------------------------------------------
# Untimed, and required: a cold or half-built tree would make the "no-op"
# measurement below a lie.
printf 'warming up …\n' >&2
build "$WORK/warmup.log"

# --- 1. no-op --------------------------------------------------------------
# The floor. Every other number is read against it: if a no-op already costs
# seconds, cargo's own freshness scan is part of the complaint.
row "no-op build (nothing to do)" "$(timed_build "$WORK/noop.log")" "the floor"

# --- 2. incremental --------------------------------------------------------
# One line appended to lib.rs: the shape of an ordinary edit while working.
cp -p "$LIB" "$WORK/lib.rs.orig"
BACKUP="$WORK/lib.rs.orig"
printf '\n// build-timings probe\n' >> "$LIB"
inc_s="$(timed_build "$WORK/incremental.log")"
cp -p "$BACKUP" "$LIB"
if cmp -s "$BACKUP" "$LIB"; then
  BACKUP=""   # restored; the exit trap need not repeat it
else
  echo "warning: lib.rs differs from its saved copy; check git status" >&2
  BACKUP=""
fi
row "incremental (one line added to lib.rs)" "$inc_s" "$(wc -l < "$LIB" | tr -d ' ') lines rechecked"

# --- 3. relink -------------------------------------------------------------
# Removes the crate's own binary artifacts so cargo must relink from the rlibs
# it already has, isolating #451's lead 3 — that linking a ~70 MB debug binary
# dominates small edits. The bin's main.rs recompiles too, which is trivial.
relink_victims=()
if [ -d "$TARGET/debug/deps" ]; then
  for f in "$TARGET/debug/deps"/bram-*; do
    [ -e "$f" ] || continue
    case "${f##*/}" in
      *.*) continue ;;   # .d and .dSYM accompany the binary; the binary alone forces the relink
    esac
    relink_victims+=("$f")
  done
fi
if [ "${#relink_victims[@]}" -gt 0 ]; then
  rm -f "${relink_victims[@]}"
  row "relink only (bin artifacts removed)" "$(timed_build "$WORK/relink.log")" "link cost in isolation"
else
  row "relink only (bin artifacts removed)" "n/a" "no bram-* artifact in target/debug/deps"
fi

# --- 4. clean --------------------------------------------------------------
# Opt-in: minutes, and it needs its own target dir so it cannot disturb the
# one you actually build into. This is the number to compare across machines
# and toolchains.
if [ "$CLEAN" = 1 ]; then
  SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/bram-clean-build.XXXXXX")"
  printf 'clean build into %s … (minutes)\n' "$SCRATCH" >&2
  t0="$(now)"
  CARGO_TARGET_DIR="$SCRATCH" build "$WORK/clean.log"
  t1="$(now)"
  row "clean build (empty target dir)" "$(elapsed "$t0" "$t1")" "all dependencies included"
fi

# --- report ----------------------------------------------------------------
lines="$(wc -l < "$LIB" | tr -d ' ')"
bin_size="$(du -h "$TARGET/debug/bram" 2>/dev/null | awk '{print $1}')"
{
  printf '## Build timings\n\n'
  printf -- '- date: %s\n' "$(date '+%Y-%m-%d %H:%M')"
  printf -- '- host: %s\n' "$(hostname -s 2>/dev/null || echo unknown)"
  printf -- '- %s / %s\n' "$(rustc -V)" "$(cargo -V)"
  printf -- '- lib.rs: %s lines\n' "$lines"
  [ -n "$bin_size" ] && printf -- '- debug binary: %s\n' "$bin_size"
  printf '\n| measurement | seconds | note |\n|---|---|---|\n'
  for r in "${rows[@]}"; do
    IFS='|' read -r label secs note <<< "$r"
    printf '| %s | %s | %s |\n' "$label" "$secs" "$note"
  done
  printf "\nDebug build only (\`cargo build --manifest-path src-tauri/Cargo.toml\`).\n"
} | tee ${OUT:+"$OUT"}
