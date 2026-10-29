#!/bin/bash
# Portable launcher revision signed: Codex (main thread), 2026-10-28.
# usage: bash run.sh REPO OUTDIR SCHEDULE [codex args...]
set -euo pipefail
if [ "$#" -lt 3 ]; then
  echo 'usage: bash run.sh REPO OUTDIR SCHEDULE [codex args...]' >&2
  exit 2
fi
HOOK_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
L="$HOOK_DIR/log-hook.sh"
R=$(cd -- "$1" && pwd -P)
mkdir -p -- "$2"
OUT=$(cd -- "$2" && pwd -P)
SCH=$(python3 -I -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$3")
shift 3
test -f "$SCH"
# JSON/TOML string escaping also handles spaces, quotes and backslashes in paths.
LOGGER=$(python3 -I -c 'import shlex,sys; print("bash " + shlex.quote(sys.argv[1]))' "$L")
toml_string() { python3 -I -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"; }
hook_override() {
  local event="$1" matcher="${2:-}" match_field=''
  if [ -n "$matcher" ]; then
    match_field="matcher=$(toml_string "$matcher"),"
  fi
  printf 'hooks.%s=[{%shooks=[{type="command",command=%s}]}]' \
    "$event" "$match_field" "$(toml_string "$LOGGER $event")"
}
M='^(Bash|apply_patch|Write|Edit|mcp__.*)$'
mkdir -p -- "$OUT/hooklog"
cd -- "$R"
env -u OPENAI_API_KEY HOOKLOG_DIR="$OUT/hooklog" python3 -I "$HOOK_DIR/../capture/drive.py" "$OUT" "$SCH" -- codex \
 -c "projects.$(toml_string "$R").trust_level=\"trusted\"" \
 -c "$(hook_override SessionStart)" \
 -c "$(hook_override UserPromptSubmit)" \
 -c "$(hook_override PreToolUse "$M")" \
 -c "$(hook_override PermissionRequest "$M")" \
 -c "$(hook_override PostToolUse "$M")" \
 -c "$(hook_override Stop)" "$@"
