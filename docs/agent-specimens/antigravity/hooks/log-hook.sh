#!/bin/bash
set -euo pipefail
event="$1"
payload=$(cat)
hook_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
mkdir -p "$hook_dir/payloads"
printf '%s\n' "$payload" >> "$hook_dir/payloads/${event}.jsonl"
if [[ "$event" == "PreToolUse" ]]; then
  printf '{"decision":"allow"}\n'
else
  printf '{}\n'
fi
