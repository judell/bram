#!/bin/bash
# Specimen logger: append the stdin payload to $HOOKLOG_DIR/<Event>.jsonl; no output = no decision.
ev="$1"; d="${HOOKLOG_DIR:-$(dirname "$0")/../../hooklog}"; mkdir -p "$d"
cat >> "$d/$ev.jsonl"; echo >> "$d/$ev.jsonl"
exit 0
