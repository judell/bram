#!/bin/bash
# usage: bash run.sh REPO OUTDIR SCHEDULE [claude args...]
# REPO: scratch git repo whose .claude/settings.json registers hooks/log-hook.sh
# (see ../hooks/settings.json). Hook payloads land in OUTDIR/hooklog/<Event>.jsonl.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
R=$(cd -- "$1" && pwd -P); mkdir -p -- "$2"; OUT=$(cd -- "$2" && pwd -P)
SCH=$(python3 -I -c 'import os,sys;print(os.path.abspath(sys.argv[1]))' "$3"); shift 3
mkdir -p -- "$OUT/hooklog"; cd -- "$R"
exec env -u CLAUDE_CODE_CHILD_SESSION -u CLAUDE_CODE_SESSION_ID -u CLAUDE_CODE_BRIDGE_SESSION_ID \
  -u CLAUDE_CODE_MESSAGING_SOCKET -u CLAUDE_CODE_MESSAGING_TOKEN -u CLAUDE_CODE_ENTRYPOINT \
  -u CLAUDE_CODE_EXECPATH -u ANTHROPIC_API_KEY HOOKLOG_DIR="$OUT/hooklog" \
  python3 -I "$HERE/drive.py" "$OUT" "$SCH" -- claude "$@"
