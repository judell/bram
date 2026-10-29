#!/bin/bash
# Specimen logger: append the payload, allow everything (no stdout = no decision).
ev="$1"
cat >> "$HOOKLOG_DIR/$ev.jsonl"
echo >> "$HOOKLOG_DIR/$ev.jsonl"
