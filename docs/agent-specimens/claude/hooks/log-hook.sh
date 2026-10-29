#!/bin/bash
# Specimen logger: append the payload (stdin) per event, allow nothing special.
ev="$1"
cat >> "$(dirname "$0")/../../hooklog/$ev.jsonl"
echo >> "$(dirname "$0")/../../hooklog/$ev.jsonl"
exit 0
