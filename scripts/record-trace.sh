#!/usr/bin/env bash
# Record a normalized trace snapshot for a documented UI gesture.
#
# Usage (from repo root):
#   scripts/record-trace.sh [--with-timing] <gesture-name>
#
# 1. Reads the current byte size of bram-traces/bram-trace.log in the state
#    folder (.bram/ or resources/) as a start marker.
# 2. Prints the gesture instructions from tests/trace-gestures.md.
# 3. Waits for the operator to perform the gesture and press Enter.
# 4. Captures all bytes appended to bram-trace.log between the markers.
# 5. Pipes through scripts/normalize-trace.py for the structural
#    snapshot, and (if --with-timing) again through
#    scripts/normalize-trace.py --keep-timing for a timing-preserved
#    snapshot.
# 6. Writes:
#      tests/trace-snapshots/<gesture>__<unix-ms>.jsonl         (always)
#      tests/trace-snapshots/<gesture>__<unix-ms>__timing.jsonl (--with-timing)
#
# The structural snapshot is what `scripts/diff-trace.sh` compares —
# behavioral equivalence across a refactor. The timing snapshot
# preserves timestamps + delta_to_emit_ms + elapsed_ms etc. and is
# what you grep for lag distributions across N runs of the same
# gesture: compute median / p95 of any timing field before and after
# a perf-targeted change.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

with_timing=0
gesture=""
while [ $# -gt 0 ]; do
  case "$1" in
    --with-timing)
      with_timing=1
      shift
      ;;
    -h|--help)
      sed -n '2,28p' "$0"
      exit 0
      ;;
    --)
      shift
      gesture="${1:-}"
      shift || true
      ;;
    -*)
      echo "$0: unknown option: $1" >&2
      exit 64
      ;;
    *)
      if [ -z "$gesture" ]; then
        gesture="$1"
      else
        echo "$0: unexpected positional arg: $1" >&2
        exit 64
      fi
      shift
      ;;
  esac
done

if [ -z "$gesture" ]; then
  echo "usage: $0 [--with-timing] <gesture-name>" >&2
  exit 64
fi

. scripts/bram-state.sh
trace_log="$(bram_state_dir .)/bram-traces/bram-trace.log"
gestures_doc="tests/trace-gestures.md"
snapshot_dir="tests/trace-snapshots"

if [ ! -f "$trace_log" ]; then
  echo "$0: trace log not found at $trace_log" >&2
  echo "  Is Bram running? Is BRAM_TRACE=1 enabled?" >&2
  exit 1
fi
if [ ! -f "$gestures_doc" ]; then
  echo "$0: gestures doc not found at $gestures_doc" >&2
  exit 1
fi
mkdir -p "$snapshot_dir"

# Show the gesture's documented instructions, if found.
echo "============================================================"
echo "Gesture: $gesture"
if [ "$with_timing" -eq 1 ]; then
  echo "Mode: structural + timing"
else
  echo "Mode: structural"
fi
echo "============================================================"
if grep -q "^## \`$gesture\`" "$gestures_doc"; then
  awk -v g="^## \`$gesture\`" '
    $0 ~ g { inside = 1; print; next }
    inside && /^## / { exit }
    inside { print }
  ' "$gestures_doc"
else
  echo "(no entry for '$gesture' in $gestures_doc — record anyway)"
fi
echo "============================================================"

start_size=$(wc -c < "$trace_log" | tr -d ' ')
echo "Start marker: byte offset $start_size of $trace_log"
echo ""
read -p "Perform the gesture, then press Enter to capture..." _

# Give the trace a brief grace window to flush any in-flight writes.
sleep 1

end_size=$(wc -c < "$trace_log" | tr -d ' ')
captured_bytes=$((end_size - start_size))
if [ "$captured_bytes" -le 0 ]; then
  echo "$0: no bytes appended to $trace_log during the window." >&2
  echo "  Is BRAM_TRACE=1 enabled? Did the gesture actually emit traces?" >&2
  exit 2
fi

unix_ms=$(python3 -c 'import time; print(int(time.time() * 1000))')
out_struct="$snapshot_dir/${gesture}__${unix_ms}.jsonl"
out_timing="$snapshot_dir/${gesture}__${unix_ms}__timing.jsonl"

# Capture the raw slice once, normalize it one or two ways.
tmp_raw=$(mktemp)
trap 'rm -f "$tmp_raw"' EXIT
dd if="$trace_log" bs=1 skip="$start_size" count="$captured_bytes" \
  2>/dev/null > "$tmp_raw"

python3 "$(dirname "$0")/normalize-trace.py" < "$tmp_raw" > "$out_struct"
struct_lines=$(wc -l < "$out_struct" | tr -d ' ')
echo ""
echo "Captured $captured_bytes bytes."
echo "Wrote $out_struct ($struct_lines lines, structural)"

if [ "$with_timing" -eq 1 ]; then
  python3 "$(dirname "$0")/normalize-trace.py" --keep-timing < "$tmp_raw" > "$out_timing"
  timing_lines=$(wc -l < "$out_timing" | tr -d ' ')
  echo "Wrote $out_timing ($timing_lines lines, timing-preserved)"
fi
