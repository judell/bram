#!/bin/bash
# usage: run.sh NAME MODE TOOL(apply_patch|Bash|none) [timeoutSeconds]
# codex-cli 0.166.0 `codex exec`. Isolated CODEX_HOME per case (auth.json copied, minimal config,
# deleted afterwards) so the user's global hooks never fire. Hook trust bypassed with
# --dangerously-bypass-hook-trust; sandbox workspace-write, exec never prompts.
B=$(cd "$(dirname "$0")" && pwd)
n=$1; mode=$2; tool=$3; to=$4
R=$B/repos/$n; rm -rf $R; mkdir -p $R; cd $R; git init -q; echo x>README; git add .; git -c user.email=a@b -c user.name=t commit -qm i
H=$B/home-$n; rm -rf $H; mkdir -p $H; cp ~/.codex/auth.json $H/auth.json; chmod 600 $H/auth.json
printf 'model = "gpt-6-astra"\nmodel_reasoning_effort = "low"\n[projects."%s"]\ntrust_level = "trusted"\n' "$R" > $H/config.toml
if [ "$tool" != none ]; then
 t=10; [ -n "$to" ] && t=$to
 cat >> $H/config.toml <<J
[[hooks.PreToolUse]]
matcher = "^($tool)\$"
[[hooks.PreToolUse.hooks]]
type = "command"
command = "$B/hook.sh $mode $tool"
timeout = $t
J
fi
if [ "$tool" = Bash ]; then
 P="Run this exact shell command and nothing else: echo SHELLMARK > $R/shell-$n.txt . Then tell me if it ran, and quote any message you received about it."
 F=shell-$n.txt
else
 P="Create a file at the exact absolute path $R/$n.txt (copy it character for character) containing exactly the text FILEMARK using your apply_patch file edit tool (not shell). Then tell me whether it succeeded, and quote any message you received about it."
 F=$n.txt
fi
s=$(date +%s)
CODEX_HOME=$H perl -e 'alarm 150; exec @ARGV' env -u OPENAI_API_KEY codex exec --dangerously-bypass-hook-trust -s workspace-write -C $R "$P" < /dev/null > $B/logs/$n.out 2>&1; ec=$?
e=$(( $(date +%s)-s ))
rm -rf $H
[ -f $R/$F ] && ex=YES || ex=NO
echo "$n mode=$mode tool=$tool to=$to exit=$ec secs=$e FILE=$ex" | tee -a $B/summary.log
