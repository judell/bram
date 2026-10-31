#!/bin/bash
# usage: hook.sh MODE TARGETTOOL ; logs to $LOG
mode="$1"; tgt="$2"
p=$(cat)
tool=$(printf '%s' "$p" | python3 -c 'import sys,json;print(json.load(sys.stdin).get("tool_name",""))' 2>/dev/null)
echo "$(date +%T) ran mode=$mode tool=$tool" >> "$(dirname "$0")/hook-fired.log"
if [ "$tool" != "$tgt" ]; then echo '{"permission":"allow"}'; exit 0; fi
echo "$(date +%T) APPLY mode=$mode tool=$tool" >> "$(dirname "$0")/hook-fired.log"
case "$mode" in
 allow) echo '{"permission":"allow"}';;
 deny) echo '{"permission":"deny","user_message":"USERMSG-DENY","agent_message":"AGENTMSG-DENY-REASON-5521: blocked by guard."}';;
 exit1) echo "STDERR-EXIT1-MARK" >&2; exit 1;;
 exit3) echo "STDERR-EXIT3-MARK" >&2; exit 3;;
 badjson) echo 'this is {not json';;
 hang) sleep 30; echo '{"permission":"deny"}';;
 exit2) echo "STDERR-EXIT2-MARK" >&2; exit 2;;
 empty) exit 0;;
 emptyobj) echo '{}';;
 oldguard) echo "bram-guard: unknown hook preToolUse" >&2; exit 0;;
esac
