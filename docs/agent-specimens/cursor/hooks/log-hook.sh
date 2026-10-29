#!/bin/bash
# Specimen logger: append the payload, allow everything.
ev="$1"
cat >> "$(dirname "$0")/../../hooklog/$ev.jsonl"
echo >> "$(dirname "$0")/../../hooklog/$ev.jsonl"
case "$ev" in
  preToolUse) echo '{"permission":"allow"}' ;;
  beforeShellExecution) echo '{"permission":"ask"}' ;;
  *) echo '{}' ;;
esac
