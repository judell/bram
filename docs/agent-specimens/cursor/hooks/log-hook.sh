#!/bin/bash
# Specimen logger: append the payload to <hooklog>/<event>.jsonl, then answer.
# Default: allow preToolUse, ask for beforeShellExecution (raises the prompt),
# empty object for everything else (SHELL_PERMISSION=allow|ask|deny overrides the ask). If the payload contains the literal
# DENYME, preToolUse answers deny with a user_message and an agent_message,
# (DENY_STYLE=agent-only|user-only|both selects the fields), to test whether the model sees the reason.
ev="$1"
dir="${HOOKLOG_DIR:-$(dirname "$0")/../../hooklog}"
payload=$(cat)
printf '%s\n' "$payload" >> "$dir/$ev.jsonl"
# One timestamped line per payload (epoch seconds, event, payload), for ordering questions.
printf '%s\t%s\t%s\n' "$(python3 -c 'import time;print("%.3f"%time.time())')" "$ev" "$(printf '%s' "$payload" | tr -d '\n')" >> "$dir/_timeline.tsv"
case "$ev" in
  preToolUse)
    if printf '%s' "$payload" | grep -q DENYME; then
      case "${DENY_STYLE:-both}" in
        agent-only) echo '{"permission":"deny","agent_message":"SPECIMEN-DENY-REASON-7731: this call was blocked by the guard hook."}' ;;
        user-only) echo '{"permission":"deny","user_message":"Blocked by specimen hook (user_message)."}' ;;
        *) echo '{"permission":"deny","user_message":"Blocked by specimen hook (user_message).","agent_message":"SPECIMEN-DENY-REASON-7731: this call was blocked by the guard hook."}' ;;
      esac
    else
      echo '{"permission":"allow"}'
    fi ;;
  beforeShellExecution) echo "{\"permission\":\"${SHELL_PERMISSION:-ask}\"}" ;;
  *) echo '{}' ;;
esac
