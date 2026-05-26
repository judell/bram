#!/usr/bin/env bash
# Capture Bram lockup state for diagnosis.
#
# 1. Snapshots resources/bram-trace.log, .inflight-claim.json,
#    worklist.json, .worklist-authorization.json into a staging dir.
# 2. Filters bram-trace.log to the last DURATION (default 1h).
# 3. Prompts the user to export an xs-trace via the Inspector
#    (restarting Bram first if it's wedged).
# 4. After confirmation, picks up the newest xs-trace-*.json from
#    ~/Downloads.
# 5. Zips everything into ~/Downloads/bram-capture-<timestamp>.zip
#
# Usage:
#   scripts/capture-bram-state.sh [DURATION]
#
# DURATION: lookback for bram-trace.log filtering. Accepts Nm or Nh.
# Default 1h. Run from a Bram project directory (one with resources/).

set -euo pipefail

DURATION="${1:-1h}"

case "$DURATION" in
  *m) MINUTES="${DURATION%m}" ;;
  *h) MINUTES=$(( ${DURATION%h} * 60 )) ;;
  *)
    echo "Bad duration: $DURATION (use Nm or Nh, e.g. 30m or 2h)" >&2
    exit 1
    ;;
esac

if [[ ! -d resources ]]; then
  echo "No resources/ in $PWD — run from a Bram project directory" >&2
  exit 1
fi

STAMP=$(date +%Y%m%dT%H%M%S)
WORKDIR=$(mktemp -d -t bram-capture)
STAGE="$WORKDIR/bram-capture-$STAMP"
mkdir -p "$STAGE"
trap 'rm -rf "$WORKDIR"' EXIT

# Compute cutoff in ISO-8601 UTC (BSD date on macOS, GNU date elsewhere)
if date -v -1M +%s >/dev/null 2>&1; then
  CUTOFF=$(date -v -${MINUTES}M -u +%FT%TZ)
else
  CUTOFF=$(date -u -d "$MINUTES minutes ago" +%FT%TZ)
fi

echo "Capturing pre-restart state to $STAGE"
echo "  (trace cutoff: $CUTOFF, $MINUTES minutes back)"
echo

# Snapshot the JSON state files
for f in .inflight-claim.json worklist.json .worklist-authorization.json; do
  if [[ -f "resources/$f" ]]; then
    cp "resources/$f" "$STAGE/$f"
    echo "  + $f"
  else
    echo "  - $f (not present)"
  fi
done

# Filter and snapshot bram-trace.log
if [[ -f resources/bram-trace.log ]]; then
  awk -v cutoff="$CUTOFF" '
    /^\[[0-9]/ {
      end = index($0, "]")
      ts = substr($0, 2, end - 2)
      keep = (ts >= cutoff)
    }
    keep
  ' resources/bram-trace.log > "$STAGE/bram-trace.log"
  lines=$(wc -l < "$STAGE/bram-trace.log" | tr -d ' ')
  echo "  + bram-trace.log ($lines lines since $CUTOFF)"
else
  echo "  - bram-trace.log (not present)"
fi

# Capture environment
{
  echo "stamp: $STAMP"
  echo "pwd: $PWD"
  echo "duration: $DURATION (cutoff $CUTOFF)"
  echo "uname: $(uname -a)"
  echo "git: $(git -C . rev-parse --short HEAD 2>/dev/null || echo n/a) on $(git -C . rev-parse --abbrev-ref HEAD 2>/dev/null || echo n/a)"
} > "$STAGE/capture-meta.txt"

echo
echo "Pre-restart state captured. Now export the xs-trace:"
echo
echo "  1. Open the Inspector (magnifying-glass icon, top-right of right pane)"
echo "  2. Click Export — it writes xs-trace-<timestamp>.json to ~/Downloads"
echo
echo "  If Bram is wedged so badly the Inspector won't respond, restart Bram"
echo "  first — but note that exporting from a fresh session won't capture"
echo "  the locked-up state."
echo
read -r -p "Press Enter once the xs-trace is in ~/Downloads (or skip with Ctrl-D): " || true

# Pick up newest xs-trace
LATEST=$(ls -t ~/Downloads/xs-trace-*.json 2>/dev/null | head -1)
if [[ -z "$LATEST" ]]; then
  echo "  - no xs-trace-*.json found in ~/Downloads (continuing)"
else
  cp "$LATEST" "$STAGE/$(basename "$LATEST")"
  echo "  + $(basename "$LATEST")"
fi

OUT="$HOME/Downloads/bram-capture-$STAMP.zip"
( cd "$WORKDIR" && zip -qr "$OUT" "bram-capture-$STAMP" )

echo
echo "Wrote $OUT"
ls -lh "$OUT"
