#!/bin/bash
# usage: bash run.sh REPO OUTDIR SCHEDULE [agent args...]
# REPO is a scratch git repo; its .cursor/ must hold hooks.json and
# log-hook.sh (copy them from this directory). Payloads land in OUTDIR/hooklog.
set -euo pipefail
if [ "$#" -lt 3 ]; then echo 'usage: bash run.sh REPO OUTDIR SCHEDULE [agent args...]' >&2; exit 2; fi
HOOK_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
R=$(cd -- "$1" && pwd -P)
mkdir -p -- "$2"; OUT=$(cd -- "$2" && pwd -P)
SCH=$(python3 -I -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$3")
shift 3
test -f "$SCH"
mkdir -p -- "$OUT/hooklog"
cd -- "$R"
env -u CURSOR_API_KEY HOOKLOG_DIR="$OUT/hooklog" python3 -I "$HOOK_DIR/../capture/drive.py" "$OUT" "$SCH" -- agent "$@"
