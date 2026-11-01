#!/bin/bash
# usage: run.sh NAME MODE TOOL(write|shell|none)
# OpenCode 2.1.4 `opencode run --standalone` (private server; the user's background service
# and global config are not touched). Guard is a project plugin .opencode/plugins/guard.js.
# opencode.json sets permission "*":"allow" so OpenCode's own permission system never decides.
B=$(cd "$(dirname "$0")" && pwd)
n=$1; mode=$2; tool=$3
R=$B/repos/$n; rm -rf $R; mkdir -p $R/.opencode/plugins; cd $R; git init -q; echo x>README; git add .; git -c user.email=a@b -c user.name=t commit -qm i
echo '{"$schema":"https://opencode.ai/config.json","permission":{"*":"allow"}}' > opencode.json
if [ "$mode" = load-syntax ]; then echo 'export default {{{ this is not valid js' > .opencode/plugins/guard.js
elif [ "$tool" != none ]; then cp $B/guard.js .opencode/plugins/guard.js; fi
if [ "$tool" = shell ]; then
 P="Run this exact shell command and nothing else: echo SHELLMARK > $R/shell-$n.txt . Then tell me if it ran, and quote any message you received about it."
 F=shell-$n.txt
else
 P="Create a file at the exact absolute path $R/$n.txt (copy it character for character) containing exactly the text FILEMARK using your write tool (not shell). Then tell me whether it succeeded, and quote any message you received about it."
 F=$n.txt
fi
s=$(date +%s)
GUARD_HANG_MS=${GUARD_HANG_MS:-40000} GUARD_MODE=$mode GUARD_TOOL=$tool GUARD_LOG=$B/hook-fired-$n.log env -u CLAUDE_CODE_CHILD_SESSION perl -e 'alarm 150; exec @ARGV' opencode run --standalone -m anthropic/claude-haiku-4-5 "$P" < /dev/null > $B/logs/$n.out 2>&1; ec=$?
e=$(( $(date +%s)-s ))
[ -f $R/$F ] && ex=YES || ex=NO
echo "$n mode=$mode tool=$tool exit=$ec secs=$e FILE=$ex" | tee -a $B/summary.log
