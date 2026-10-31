#!/bin/bash
# usage: run.sh NAME MODE TOOL(Write|Shell|none) [failClosed] [timeout]
B=<scratch>/cursor-hooks
n=$1; mode=$2; tool=$3; fc=$4; to=$5
R=$B/repos/$n; rm -rf $R; mkdir -p $R/.cursor; cd $R; git init -q; echo x>README; git add .; git -c user.email=a@b -c user.name=t commit -qm i
extra=""; [ "$fc" = fc ] && extra=',"failClosed":true'; [ -n "$to" ] && extra="$extra,\"timeout\":$to"
if [ "$tool" != none ]; then
cat > .cursor/hooks.json <<J
{"version":1,"hooks":{"preToolUse":[{"command":"$B/hook.sh $mode $tool"$extra}]}}
J
fi
if [ "$tool" = Shell ]; then
 P="Run this exact shell command and nothing else: echo SHELLMARK > $R/shell-$n.txt . Then tell me if it ran or what you were told."
 F=shell-$n.txt
else
 P="Create a file at the exact absolute path $R/$n.txt (copy it character for character) containing exactly the text FILEMARK using your file edit/write tool (not shell). Then tell me whether it succeeded, and quote any message you received about it."
 F=$n.txt
fi
s=$(date +%s)
perl -e 'alarm 150; exec @ARGV' cursor-agent -p --force --trust --output-format text "$P" > $B/logs/$n.out 2>&1; ec=$?
e=$(( $(date +%s)-s ))
[ -f $R/$F ] && ex=YES || ex=NO
echo "$n mode=$mode tool=$tool fc=$fc to=$to exit=$ec secs=$e FILE=$ex" | tee -a $B/summary.log
