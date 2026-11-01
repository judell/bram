#!/bin/bash
# usage: hook.sh MODE TARGETTOOL-REGEX ; payload on stdin; logs to hook-fired.log beside this script.
# Codex PreToolUse guard stand-in. Outputs follow Claude's hook contract.
mode="$1"; tgt="$2"
p=$(cat)
tool=$(printf '%s' "$p" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("tool_name",""))' 2>/dev/null)
L="$(dirname "$0")/hook-fired.log"
echo "$(date +%T) ran mode=$mode tool=$tool" >> "$L"
if ! printf '%s' "$tool" | grep -Eq "^($tgt)$"; then exit 0; fi
echo "$(date +%T) APPLY mode=$mode tool=$tool" >> "$L"
case "$mode" in
 allow) echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}';;
 deny) echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"AGENTMSG-DENY-REASON-5521: blocked by guard."}}';;
 exit1) echo "STDERR-EXIT1-MARK" >&2; exit 1;;
 exit3) echo "STDERR-EXIT3-MARK" >&2; exit 3;;
 badjson) echo 'this is {not json';;
 hang) sleep 30; echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"HANG-LATE-DENY"}}';;
 exit2) echo "STDERR-EXIT2-MARK" >&2; exit 2;;
 empty) exit 0;;
 emptyobj) echo '{}';;
 oldguard) echo "bram-guard: unknown hook PreToolUse" >&2; exit 0;;
esac
