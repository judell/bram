#!/bin/bash
# usage: run.sh REPO OUTDIR SCHEDULE   (REPO: scratch git repo; never the Bram checkout)
# Installs the logger plugin and an opencode.json into REPO, then drives `opencode --standalone`.
# Does not touch the background `opencode serve --service` or global config.
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$1" && pwd)"; mkdir -p "$2"; OUT="$(cd "$2" && pwd)"; SCHED="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"
mkdir -p "$REPO/.opencode/plugins" "$OUT/events"
cp "$HERE/../hooks/logger.js" "$REPO/.opencode/plugins/logger.js"
[ -f "$REPO/opencode.json" ] || cp "$HERE/config/ask.opencode.json" "$REPO/opencode.json"
export SPECIMEN_EVENTS_DIR="$OUT/events"
cd "$REPO"
exec env -u CLAUDE_CODE_CHILD_SESSION python3 -I "$HERE/drive.py" "$OUT" "$SCHED" -- opencode --standalone
