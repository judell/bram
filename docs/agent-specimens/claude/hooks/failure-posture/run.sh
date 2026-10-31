#!/bin/bash
# usage: run.sh NAME MODE TOOL(Write|Bash|none) [timeoutSeconds]
# Claude Code 2.1.316. User-level settings (which carry real hooks) are excluded with
# --setting-sources project,local; login is kept. Permissions bypassed so Claude's own
# prompt system does not decide the result; hooks still run.
B=$(cd "$(dirname "$0")" && pwd)
n=$1; mode=$2; tool=$3; to=$4
R=$B/repos/$n; rm -rf $R; mkdir -p $R/.claude; cd $R; git init -q; echo x>README; git add .; git -c user.email=a@b -c user.name=t commit -qm i
if [ "$tool" != none ]; then
 t=""; [ -n "$to" ] && t=",\"timeout\":$to"
 cat > .claude/settings.json <<J
{"hooks":{"PreToolUse":[{"matcher":"$tool","hooks":[{"type":"command","command":"$B/hook.sh $mode $tool"$t}]}]}}
J
fi
if [ "$tool" = Bash ]; then
 P="Run this exact shell command and nothing else: echo SHELLMARK > $R/shell-$n.txt . Then tell me if it ran, and quote any message you received about it."
 F=shell-$n.txt
else
 P="Create a file at the exact absolute path $R/$n.txt (copy it character for character) containing exactly the text FILEMARK using your Write tool (not shell). Then tell me whether it succeeded, and quote any message you received about it."
 F=$n.txt
fi
s=$(date +%s)
perl -e 'alarm 150; exec @ARGV' claude -p --model haiku --setting-sources project,local --permission-mode bypassPermissions --output-format text "$P" > $B/logs/$n.out 2>&1; ec=$?
e=$(( $(date +%s)-s ))
[ -f $R/$F ] && ex=YES || ex=NO
echo "$n mode=$mode tool=$tool to=$to exit=$ec secs=$e FILE=$ex" | tee -a $B/summary.log
