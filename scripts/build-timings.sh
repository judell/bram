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

cargo_run() {  # cargo_run <logfile> <subcommand> [extra cargo args...]
  local log="$1" sub="$2"; shift 2
  cargo "$sub" --manifest-path "$MANIFEST" "$@" > "$log" 2>&1 || {
    echo "cargo $sub failed; last 20 lines of $log:" >&2
    tail -20 "$log" >&2
    exit 1
  }
}

timed_cargo() {  # timed_cargo <logfile> <subcommand> -> prints seconds
  local t0 t1
  t0="$(now)"
  cargo_run "$1" "$2"
  t1="$(now)"
  elapsed "$t0" "$t1"
}

# The probe: append one line to lib.rs, measure, restore the original bytes.
# Used twice (incremental and check), so it is a pair rather than inline code;
# the exit trap restores from $BACKUP if a run dies between them.
probe_edit() {
  cp -p "$LIB" "$WORK/lib.rs.orig"
  BACKUP="$WORK/lib.rs.orig"
  printf '\n// build-timings probe\n' >> "$LIB"
}
probe_restore() {
  [ -n "$BACKUP" ] || return 0
  cp -p "$BACKUP" "$LIB"
  if ! cmp -s "$BACKUP" "$LIB"; then
    echo "warning: lib.rs differs from its saved copy; check git status" >&2
  fi
  BACKUP=""   # restored; the exit trap need not repeat it
}

# build-timings-line-shift-row: the same probe, inserted as line 1 instead of
# appended. Appending shifts no other line, the cheapest possible edit; real
# edits land mid-file and move every line below them, which full debuginfo
# records. Three mid-file edits on 2026-10-27 built in 2m55s, 1m38s and 34s
# against the appended probe's 22.76s (#451), hence the second row.
probe_edit_top() {
  cp -p "$LIB" "$WORK/lib.rs.orig"
  BACKUP="$WORK/lib.rs.orig"
  { printf '// build-timings probe\n'; cat "$WORK/lib.rs.orig"; } > "$LIB"
}

# Restoring a probe is itself an edit (undoing a top insert shifts every line
# back), so the next leg would pay for it. An untimed build after each restore
# lets every timed row start from a settled tree.
settle() {
  cargo_run "$WORK/settle-$1.log" build
}

rows=()
row() { rows+=("$1|$2|$3"); }

# --- warm-up ---------------------------------------------------------------
# Untimed, and required: a cold or half-built tree would make the "no-op"
# measurement below a lie.
printf 'warming up …\n' >&2
cargo_run "$WORK/warmup.log" build

# --- 1. no-op --------------------------------------------------------------
# The floor. Every other number is read against it: if a no-op already costs
# seconds, cargo's own freshness scan is part of the complaint.
row "no-op build (nothing to do)" "$(timed_cargo "$WORK/noop.log" build)" "the floor"

# --- 2. incremental --------------------------------------------------------
# One line appended to lib.rs: the shape of an ordinary edit while working.
# Measured before the check leg, so this row stays comparable with the #451
# baseline recorded in 84ab36e.
probe_edit
inc_s="$(timed_cargo "$WORK/incremental.log" build)"
probe_restore
settle incremental
row "incremental (one line added to lib.rs)" "$inc_s" "$(wc -l < "$LIB" | tr -d ' ') lines rechecked"

# --- 2b. incremental, lines shifted -----------------------------------------
# One line inserted at the top: every line below moves, as a mid-file edit
# does. Compare with the row above; the gap is what line shifts cost.
probe_edit_top
shift_s="$(timed_cargo "$WORK/incremental-top.log" build)"
probe_restore
settle incremental-top
row "incremental, lines shifted (one line inserted at top of lib.rs)" "$shift_s" "every line below moves"

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
relink_s=""
if [ "${#relink_victims[@]}" -gt 0 ]; then
  rm -f "${relink_victims[@]}"
  relink_s="$(timed_cargo "$WORK/relink.log" build)"
  row "relink only (bin artifacts removed)" "$relink_s" "link cost in isolation"
else
  row "relink only (bin artifacts removed)" "n/a" "no bram-* artifact in target/debug/deps"
fi

# --- 4. check --------------------------------------------------------------
# cargo check type-checks and emits metadata but runs no codegen and no link.
# This is the row that decides whether crate size is worth attacking: it does
# NOT shrink when files are split inside one crate, because modules are still
# one compilation unit. Only separate crates shrink it.
#
# It must be warmed first. check keeps its own fingerprints, so a check issued
# after a build runs COLD -- it type-checks the whole crate from nothing, and
# that number is not a share of anything. Measured on this repo 2026-10-26:
# 260.59s cold against 25.48s for the full incremental build, which made the
# subtraction below clamp to 0.00 and mean nothing. The untimed warm-up makes
# the timed row one edit against a warm cache, comparable with the row above.
printf 'warming the check cache … (minutes on a cold cache)\n' >&2
cargo_run "$WORK/check-warm.log" check
probe_edit
check_s="$(timed_cargo "$WORK/check.log" check)"
probe_restore
settle check
row "check only (one line added to lib.rs)" "$check_s" "type-check, no codegen, no link"

# --- 5. no derived row -----------------------------------------------------
# There is deliberately no "codegen ≈" row. The obvious one,
# incremental − check − relink, was measured on this repo (2026-10-26) and
# came out NEGATIVE twice: once cold (260.59s check against a 25.48s build),
# once warm (19.66s check + 9.91s relink against a 23.95s build). The legs are
# not additive -- relink recompiles the bin's trivial main.rs and check runs
# its own analysis pass, so each includes work the incremental leg shares.
# Read check and relink as independent scales for type-checking and linking;
# do not subtract them.

# --- 6. clean --------------------------------------------------------------
# Opt-in: minutes, and it needs its own target dir so it cannot disturb the
# one you actually build into. This is the number to compare across machines
# and toolchains.
if [ "$CLEAN" = 1 ]; then
  SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/bram-clean-build.XXXXXX")"
  printf 'clean build into %s … (minutes)\n' "$SCRATCH" >&2
  t0="$(now)"
  CARGO_TARGET_DIR="$SCRATCH" cargo_run "$WORK/clean.log" build
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
