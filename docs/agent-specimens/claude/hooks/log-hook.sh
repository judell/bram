#!/bin/bash
# Specimen logger: append the stdin payload to $HOOKLOG_DIR/<Event>.jsonl; no output = no decision.
# Deny mode (2026-10-28): a PreToolUse payload containing DENYME is denied. $DENY_MODE=json (default)
# answers permissionDecision deny with the marker reason on stdout; $DENY_MODE=exit2 writes the marker to stderr and exits 2.
ev="$1"; d="${HOOKLOG_DIR:-$(dirname "$0")/../../hooklog}"; mkdir -p "$d"
p=$(cat); printf '%s\n' "$p" >> "$d/$ev.jsonl"
if [ "$ev" = PreToolUse ] && printf '%s' "$p" | grep -q DENYME; then
  if [ "${DENY_MODE:-json}" = exit2 ]; then echo "MARKER-REASON-7731 (exit2 stderr)" >&2; exit 2; fi
  echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"MARKER-REASON-7731"}}'
fi
exit 0
