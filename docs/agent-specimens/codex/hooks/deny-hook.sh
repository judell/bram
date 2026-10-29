#!/bin/bash
# Specimen guard: log the payload, then deny any call whose payload contains
# DENYME, in the shape Bram's own Codex guard returns (lib.rs
# guard_selftest_run_case: hookSpecificOutput.permissionDecision "deny" plus
# permissionDecisionReason, exit 0). Otherwise print nothing (= allow).
ev="$1"
p=$(cat)
printf '%s\n' "$p" >> "$HOOKLOG_DIR/$ev.jsonl"
case "$p" in
  *DENYME*)
    printf '%s\n' "$p" >> "$HOOKLOG_DIR/$ev-denied.jsonl"
    printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"MARKER-REASON-7731: blocked by specimen guard"}}'
    ;;
esac
