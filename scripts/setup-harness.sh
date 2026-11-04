#!/usr/bin/env bash
# Black-box check of Bram's first-time Setup, run against throwaway projects.
#
# Why this exists: every Setup verification in the issue record (#99, #102,
# #173, #211, #247, #249) was done by hand on a live instance, and several bugs
# shipped anyway. Setup's effects are entirely observable without the UI --
# files on disk plus GET /__enhance/status -- so they can be asserted.
#
# The technique that makes it safe: `bram <path>` takes a project root and
# there is no single-instance guard, so a full Bram can run against a temp
# directory while your working session keeps going. See
# docs/developing-bram.md, "Developing and testing the startup dance".
#
# Usage:
#   scripts/setup-harness.sh                 # all scenarios
#   scripts/setup-harness.sh pristine_git    # one scenario
#   BRAM_BIN=/path/to/bram scripts/setup-harness.sh
#
# Exit status is the number of failed assertions (0 = all pass).

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$REPO_ROOT/scripts/bram-state.sh"
# The state folder's name under a project (.bram or resources), re-resolved on
# each call because Setup may create it mid-scenario.
state_rel() { basename "$(bram_state_dir "$1")"; }
BRAM_BIN="${BRAM_BIN:-$REPO_ROOT/src-tauri/target/debug/bram}"
WORK_ROOT="${HARNESS_TMP:-$(mktemp -d -t bram-setup-harness)}"
KEEP_ON_FAIL="${KEEP_ON_FAIL:-1}"

PASS=0
FAIL=0
BRAM_PID=""
BRAM_PORT=""
SCENARIO=""

ok()  { PASS=$((PASS + 1)); printf '    ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '    FAIL %s\n' "$1"; [ $# -gt 1 ] && [ -n "$2" ] && printf '         %s\n' "$2"; }

# check <name> <command...>  -- passes when the command succeeds
check() {
  local name="$1"; shift
  local out
  if out="$("$@" 2>&1)"; then ok "$name"; else bad "$name" "$out"; fi
}

banner() { printf '\n--- %s ---\n' "$1"; }

require_bin() {
  if [ ! -x "$BRAM_BIN" ]; then
    echo "no binary at $BRAM_BIN"
    echo "build it first:  (cd src-tauri && cargo build)"
    exit 127
  fi
  # A running process keeps its original inode, so a stale build is easy to
  # mistake for a real result. Report what we are about to run.
  printf 'binary: %s\n' "$BRAM_BIN"
  printf 'built:  %s\n' "$(date -r "$BRAM_BIN" 2>/dev/null || stat -c %y "$BRAM_BIN" 2>/dev/null)"
}

launch() {
  local dir="$1"
  # BRAM_TRACE=1 matters: traces are opt-in per project and a scratch project
  # has no settings yet, so only the env var turns them on.
  BRAM_TRACE=1 "$BRAM_BIN" "$dir" > "$WORK_ROOT/$SCENARIO.log" 2>&1 &
  BRAM_PID=$!
  local i
  for i in $(seq 1 80); do
    if [ -f "$(bram_state_dir "$dir")/.bram-port" ]; then
      BRAM_PORT="$(cat "$(bram_state_dir "$dir")/.bram-port" 2>/dev/null)"
      [ -n "$BRAM_PORT" ] && sleep 1 && return 0
    fi
    kill -0 "$BRAM_PID" 2>/dev/null || { bad "launch" "process exited; see $WORK_ROOT/$SCENARIO.log"; return 1; }
    sleep 0.5
  done
  bad "launch" "no port file after 40s; see $WORK_ROOT/$SCENARIO.log"
  return 1
}

# issue-448: a launch outside a project folder (not a git repo's root) writes
# no port file, so its port comes from the "[bram] loopback port=" line Bram
# prints on stderr. Ready once /__enhance/status answers.
launch_refused() {
  local dir="$1"
  BRAM_TRACE=1 "$BRAM_BIN" "$dir" > "$WORK_ROOT/$SCENARIO.log" 2>&1 &
  BRAM_PID=$!
  local i
  for i in $(seq 1 80); do
    BRAM_PORT="$(sed -n 's/^\[bram\] loopback port=\([0-9]*\).*/\1/p' "$WORK_ROOT/$SCENARIO.log" | head -1)"
    if [ -n "$BRAM_PORT" ] && api "__enhance/status" > /dev/null 2>&1; then
      sleep 2
      return 0
    fi
    kill -0 "$BRAM_PID" 2>/dev/null || { bad "launch" "process exited; see $WORK_ROOT/$SCENARIO.log"; return 1; }
    sleep 0.5
  done
  bad "launch" "no loopback port after 40s; see $WORK_ROOT/$SCENARIO.log"
  return 1
}

# A refused launch starts and writes nothing: no Setup files, no startup
# scaffold, no agent.
assert_refused() {
  local dir="$1" kind="$2" got p written=""
  got="$(status_field projectFolder.kind)"
  if [ "$got" = "$kind" ]; then
    ok "projectFolder.kind is $kind"
  else
    bad "projectFolder.kind is $kind" "got: $got"
  fi
  if grep -q "op=autostart-held reason=$kind" "$WORK_ROOT/$SCENARIO.log"; then
    ok "autostart held ($kind)"
  else
    bad "autostart held ($kind)" "no op=autostart-held line in $WORK_ROOT/$SCENARIO.log"
  fi
  for p in resources .bram .claude AGENTS.md CLAUDE.md .bram.json; do
    [ -e "$dir/$p" ] && written="$written $p"
  done
  if [ -z "$written" ]; then
    ok "nothing written in $(basename "$dir")"
  else
    bad "nothing written in $(basename "$dir")" "found:$written"
  fi
}

# Kill only the PID we spawned -- never pkill, which would take down the
# developer's own Bram.
shutdown_bram() {
  [ -n "$BRAM_PID" ] || return 0
  kill "$BRAM_PID" 2>/dev/null
  wait "$BRAM_PID" 2>/dev/null
  BRAM_PID=""
  BRAM_PORT=""
}

api() { curl -4 -sS --max-time 20 "http://127.0.0.1:$BRAM_PORT/$1"; }
status_field() { api "__enhance/status" | jq -r ".$1"; }

assert_field() {
  local field="$1" want="$2" note="${3:-}"
  local got; got="$(status_field "$field")"
  if [ "$got" = "$want" ]; then ok "$field == $want"; else bad "$field == $want (got: $got)" "$note"; fi
}

assert_file() {
  local dir="$1" rel="$2"
  if [ -e "$dir/$rel" ]; then ok "seeded $rel"; else bad "seeded $rel" "missing"; fi
}

# Fingerprint everything Setup owns. The state folder (.bram/ or resources/)
# is excluded: the port file, port sidecar and traces legitimately change
# between runs.
fingerprint() {
  local dir="$1"
  (cd "$dir" && find . -type f \
      -not -path './resources/*' -not -path './.bram/*' -not -path './.git/*' \
      -print0 2>/dev/null | sort -z | xargs -0 shasum 2>/dev/null)
}

run_setup() { api "__enhance/run?force=true" > /dev/null; }

# first-run-sequence-greeting-vs-setup: Setup now runs automatically inside
# pty_spawn, before the agent autostart. The trace is the observable record of
# that decision; poll briefly because the PTY spawn can trail the port bind.
assert_trace() {
  local dir="$1" pattern="$2" name="$3"
  local i
  for i in $(seq 1 20); do
    if grep -q "$pattern" "$(bram_state_dir "$dir")/bram-traces/bram-trace.log" 2>/dev/null; then
      ok "$name"
      return 0
    fi
    sleep 0.5
  done
  bad "$name" "no line matching: $pattern"
  return 1
}

# ---------------------------------------------------------------------------
# Shared assertion table. Everything here traces to a bug the issue record
# actually caught; see the worklist draft setup-verification-harness.md.
# ---------------------------------------------------------------------------
assert_setup_landed() {
  local dir="$1"

  # core_installed keys on this file (lib.rs:33509).
  assert_file "$dir" "$(state_rel "$dir")/.worklist-authorization.json"
  assert_file "$dir" ".claude/bram-conventions.md"
  assert_file "$dir" ".claude/settings.json"
  assert_file "$dir" "AGENTS.md"
  assert_file "$dir" "CLAUDE.md"

  # retire-python-hooks-rust-only: Setup installs NO Python hook scripts,
  # and deletes previously installed ones once unreferenced. Presence after
  # a fresh Setup is the failure.
  for retired in ".claude/hooks/claude-worklist-guard.py" ".claude/hooks/claude-permission-menu-hook.py"; do
    if [ -e "$dir/$retired" ]; then
      bad "retired Python hook absent: $retired" "present after Setup"
    else
      ok "retired Python hook absent: $retired"
    fi
  done

  # The deciding registration is the bram-guard link, not python.
  if grep -q "bram-guard" "$dir/.claude/settings.local.json" 2>/dev/null; then
    ok "bram-guard registered in settings.local.json"
  else
    bad "bram-guard registered in settings.local.json" "no bram-guard command found"
  fi
  if grep -qE "python|\.py" "$dir/.claude/settings.local.json" 2>/dev/null; then
    bad "no python commands in settings.local.json" "$(grep -oE '"command":[^,]*' "$dir/.claude/settings.local.json" | head -3)"
  else
    ok "no python commands in settings.local.json"
  fi

  # #247: claude_installed was lenient, so the button never surfaced.
  assert_field enhanced true
  assert_field claudeNeedsSetup false
  assert_field codexNeedsSetup false

  # opencode-worklist-guard: the guard plugin is seeded where OpenCode is
  # installed (opencode_plugin_wanted keys on its CLI) and nowhere else, so
  # projects that never run OpenCode get no .opencode/ directory.
  assert_field opencodeNeedsSetup false
  if command -v opencode > /dev/null 2>&1; then
    assert_file "$dir" ".opencode/plugins/bram.js"
    if cmp -s "$dir/.opencode/plugins/bram.js" "$REPO_ROOT/app/__shell/opencode/bram-plugin.js"; then
      ok "OpenCode plugin matches the bundle"
    else
      bad "OpenCode plugin matches the bundle" "seeded copy differs from app/__shell/opencode/bram-plugin.js"
    fi
    # opencode-signature-and-conventions: OpenCode loads the conventions
    # through its `instructions` config, not the AGENTS.md pointer.
    if jq -e '.instructions | index(".claude/bram-conventions.md")' \
         "$dir/.opencode/opencode.json" > /dev/null 2>&1; then
      ok "OpenCode instructions list the conventions"
    else
      bad "OpenCode instructions list the conventions" "$(cat "$dir/.opencode/opencode.json" 2>/dev/null | head -5)"
    fi
  elif [ -e "$dir/.opencode" ]; then
    bad "no .opencode/ without OpenCode" "Setup created it with no opencode on PATH"
  else
    ok "no .opencode/ without OpenCode"
  fi

  # antigravity-worklist-guard: Bram's guard group is seeded into
  # .agents/hooks.json where agy is installed (antigravity_hooks_wanted keys
  # on its CLI) and nowhere else. assert_idempotent's fingerprint covers the
  # file, so a second Setup must leave it byte-identical.
  assert_field antigravityNeedsSetup false
  if command -v agy > /dev/null 2>&1; then
    assert_file "$dir" ".agents/hooks.json"
    # The command wraps the guard so that no decision (an older guard that
    # doesn't know the hook) becomes a deny; agy lets empty output through.
    if jq -e '.["bram-worklist-guard"].PreToolUse[0]
              | (.matcher == "^(write_file|edit_file|run_command|call_mcp_tool)$")
                and (.hooks[0].command | contains("guard antigravity-worklist"))
                and (.hooks[0].command | contains("\"decision\":\"deny\""))' \
         "$dir/.agents/hooks.json" > /dev/null 2>&1; then
      ok "Antigravity hooks register the guard"
    else
      bad "Antigravity hooks register the guard" "$(head -12 "$dir/.agents/hooks.json" 2>/dev/null)"
    fi
    # antigravity-signature-and-conventions: agy loads the conventions as
    # always-on rules, cut in two to fit its 24,000-byte limit, which it
    # enforces by truncating silently. Each slice must fit, and the slices
    # minus their six header lines must give back the seeded conventions.
    # assert_idempotent then covers a second Setup leaving them byte-identical.
    assert_field antigravityRulesCurrent true
    local slice slices_ok=1 slice_bad=""
    for slice in "$dir/.agents/rules/bram-conventions-1.md" "$dir/.agents/rules/bram-conventions-2.md"; do
      if [ ! -f "$slice" ]; then
        slices_ok=0; slice_bad="$slice_bad missing:${slice##*/}"
      elif [ "$(wc -c < "$slice")" -gt 24000 ]; then
        slices_ok=0; slice_bad="$slice_bad over-24000:${slice##*/}"
      elif [ "$(sed -n 2p "$slice")" != "trigger: always_on" ]; then
        slices_ok=0; slice_bad="$slice_bad no-trigger:${slice##*/}"
      fi
    done
    if [ "$slices_ok" = 1 ]; then
      ok "Antigravity rules: two always-on slices within 24,000 bytes"
    else
      bad "Antigravity rules: two always-on slices within 24,000 bytes" "$slice_bad"
    fi
    if [ -e "$dir/.agents/rules/bram-conventions-3.md" ]; then
      bad "Antigravity rules: no third slice" "bram-conventions-3.md present"
    fi
    if { tail -n +7 "$dir/.agents/rules/bram-conventions-1.md"; tail -n +7 "$dir/.agents/rules/bram-conventions-2.md"; } 2>/dev/null \
         | cmp -s - "$dir/.claude/bram-conventions.md"; then
      ok "Antigravity rules reassemble the seeded conventions"
    else
      bad "Antigravity rules reassemble the seeded conventions" "slices minus headers differ from .claude/bram-conventions.md"
    fi
  elif [ -e "$dir/.agents/hooks.json" ] || [ -e "$dir/.agents/rules" ]; then
    bad "no .agents/ seeding without agy" "Setup created .agents/ files with no agy on PATH"
  else
    ok "no .agents/ seeding without agy"
  fi

  # Tonight's bug: firstRun keys on .bram.json (lib.rs:33551 / 21336), which
  # Setup does not write, so the "first time in this repo" banner survives a
  # successful Setup. Same shape as #211.
  assert_field firstRun false "firstRun and needsSetup answer different questions and disagree here"

  # #249: machine-specific absolute interpreter paths must not reach the
  # tracked settings file.
  if grep -qE '"command"[^"]*"[A-Za-z]:\\\\|/Users/[^"]*/python|/usr/local/bin/python[0-9.]*"' \
       "$dir/.claude/settings.json" 2>/dev/null; then
    bad "no machine-specific interpreter in tracked settings.json" "$(grep -o '"command":[^,]*' "$dir/.claude/settings.json" | head -3)"
  else
    ok "no machine-specific interpreter in tracked settings.json"
  fi

  # #227: legacy generic names are pruned only when unreferenced.
  if [ -e "$dir/.claude/hooks/worklist-guard.py" ] || [ -e "$dir/.claude/hooks/permission-menu-hook.py" ]; then
    if grep -q "hooks/worklist-guard.py\|hooks/permission-menu-hook.py" \
         "$dir/.claude/settings.json" "$dir/.claude/settings.local.json" 2>/dev/null; then
      ok "legacy hook shim retained while still referenced"
    else
      bad "legacy hook shim pruned when unreferenced" "shim present with no settings reference"
    fi
  else
    ok "no stale legacy hook shims"
  fi
}

assert_idempotent() {
  local dir="$1"
  local before after
  before="$(fingerprint "$dir")"
  run_setup
  sleep 1
  after="$(fingerprint "$dir")"
  if [ "$before" = "$after" ]; then
    ok "second Setup is byte-idempotent"
  else
    bad "second Setup is byte-idempotent" "$(diff <(echo "$before") <(echo "$after") | head -8)"
  fi
}

# ---------------------------------------------------------------------------
# Scenarios. Most historical failures were about the STARTING state, not the
# happy path, which is why these vary the fixture rather than the steps.
# ---------------------------------------------------------------------------

scenario_pristine_nogit() {
  # issue-448: a folder that isn't in a git repository is not a project
  # folder (README prerequisite #1). Before, Setup seeded it on first launch.
  local dir="$WORK_ROOT/pristine_nogit"; mkdir -p "$dir"
  printf '# scratch\n' > "$dir/README.md"
  launch_refused "$dir" || return
  assert_refused "$dir" "not-git"
  shutdown_bram
}

scenario_subfolder() {
  # issue-448: a folder inside a repo is named as a subfolder, never silently
  # swapped for the repo root, and nothing is written in either.
  local repo="$WORK_ROOT/subfolder_repo"
  local dir="$repo/src"
  mkdir -p "$dir"
  git -C "$repo" init -q
  printf '# repo\n' > "$repo/README.md"
  launch_refused "$dir" || return
  assert_refused "$dir" "subfolder"
  local top; top="$(status_field projectFolder.toplevel)"
  if [ "$top" = "$(cd "$repo" && pwd -P)" ]; then
    ok "toplevel names the repo root"
  else
    bad "toplevel names the repo root" "got: $top"
  fi
  local p written=""
  for p in resources .bram .claude AGENTS.md .bram.json; do
    [ -e "$repo/$p" ] && written="$written $p"
  done
  if [ -z "$written" ]; then ok "nothing written at the repo root"; else bad "nothing written at the repo root" "found:$written"; fi
  shutdown_bram
}

scenario_pristine_git() {
  local dir="$WORK_ROOT/pristine_git"; mkdir -p "$dir"
  git -C "$dir" init -q
  printf '# scratch\n' > "$dir/README.md"
  git -C "$dir" add -A
  git -C "$dir" -c user.email=h@h -c user.name=harness commit -qm init
  launch "$dir" || return
  assert_trace "$dir" "\[auto-setup\] op=ran firstRun=true" "auto-setup ran on first launch"
  sleep 1
  assert_setup_landed "$dir"

  # #249's real failure: Setup must not leave TRACKED files modified. Untracked
  # seeded files are expected on a fresh project, so this checks tracked only.
  check "no tracked files modified by Setup" git -C "$dir" diff --quiet
  # new-repo-orientation: the playbook is seeded stamped; the template ships
  # in a subfolder, verbatim (a stamp would break arch.js).
  check "orientation playbook seeded" test -f "$dir/.claude/bram-reference/orientation.md"
  check "orientation template seeded" test -f "$dir/.claude/bram-reference/orientation-template/architecture/01-system.mmd"
  check "template copied unstamped" bash -c "! head -c 11 '$dir/.claude/bram-reference/orientation-template/arch.js' | grep -q 'bram v'"
  assert_idempotent "$dir"
  shutdown_bram
}

scenario_already_setup() {
  # The cross-machine case from #249: seeded files committed, Setup run again.
  local dir="$WORK_ROOT/already_setup"; mkdir -p "$dir"
  git -C "$dir" init -q
  printf '# scratch\n' > "$dir/README.md"
  # Traces and the port sidecar are runtime state, not Setup output, and a real
  # managed project does not track them (the bram repo keeps
  # resources/bram-traces/ untracked). Committing them would make this
  # scenario fail on its own live trace writes rather than on Setup churn.
  printf 'resources/bram-traces/\nresources/.bram-port*\n.bram/bram-traces/\n.bram/.bram-port*\n' > "$dir/.gitignore"
  git -C "$dir" add -A
  git -C "$dir" -c user.email=h@h -c user.name=harness commit -qm init
  launch "$dir" || return
  run_setup; sleep 1
  git -C "$dir" add -A
  git -C "$dir" -c user.email=h@h -c user.name=harness commit -qm "setup output"
  run_setup; sleep 1
  check "re-running Setup leaves the tree clean" git -C "$dir" diff --quiet
  assert_field firstRun false
  assert_field claudeNeedsSetup false
  shutdown_bram
}

scenario_stale_instructions() {
  # #465: a set-up project whose conventions copy no longer matches. The
  # status used to say only claudeNeedsSetup=true; setupReasons must name the
  # failed check, and a missing hook must read differently.
  local dir="$WORK_ROOT/stale_instructions"; mkdir -p "$dir"
  git -C "$dir" init -q
  printf '# scratch\n' > "$dir/README.md"
  launch "$dir" || return
  run_setup; sleep 1
  assert_field claudeNeedsSetup false
  printf '<!-- bram v0.0.1 -->\nold text\n' > "$dir/.claude/bram-conventions.md"
  sleep 1
  assert_field claudeNeedsSetup true
  assert_field claudeDocsCurrent false
  assert_field menuHookRegistered true
  local reasons; reasons="$(api __enhance/status | jq -c '.setupReasons')"
  if [ "$reasons" = '["claude-instructions-stale"]' ]; then
    ok "setupReasons names stale instructions only"
  else
    bad "setupReasons names stale instructions only" "got: $reasons"
  fi
  shutdown_bram
}

scenario_antigravity_reasons() {
  # agy-setup-reasons: with Antigravity active, removing Bram's group from
  # .agents/hooks.json or staling .agents/rules/ must report its own code in
  # setupReasons rather than leaving setupReasons empty.
  local dir="$WORK_ROOT/antigravity_reasons"; mkdir -p "$dir"
  git -C "$dir" init -q
  printf '# scratch\n' > "$dir/README.md"
  printf '{"shell":{"agent":"antigravity","startupPolicy":"none"}}\n' > "$dir/.bram.json"
  launch "$dir" || return
  run_setup; sleep 1
  assert_field activeProvider antigravity
  assert_field antigravityNeedsSetup false
  assert_field providerNeedsSetup false
  local reasons; reasons="$(api __enhance/status | jq -c '.setupReasons')"
  if [ "$reasons" = '[]' ]; then
    ok "setupReasons empty when Antigravity setup is current"
  else
    bad "setupReasons empty when Antigravity setup is current" "got: $reasons"
  fi
  jq 'del(.["bram-worklist-guard"])' "$dir/.agents/hooks.json" > "$dir/.agents/hooks.json.tmp" \
    && mv "$dir/.agents/hooks.json.tmp" "$dir/.agents/hooks.json"
  sleep 1
  assert_field antigravityNeedsSetup true
  assert_field providerNeedsSetup true
  assert_field antigravityHooks missing
  reasons="$(api __enhance/status | jq -c '.setupReasons')"
  if [ "$reasons" = '["antigravity-hook-missing"]' ]; then
    ok "setupReasons names missing Antigravity hook group"
  else
    bad "setupReasons names missing Antigravity hook group" "got: $reasons"
  fi
  rm -f "$dir/.agents/rules/bram-conventions-1.md"
  sleep 1
  assert_field antigravityRulesCurrent false
  reasons="$(api __enhance/status | jq -c '.setupReasons')"
  if [ "$reasons" = '["antigravity-hook-missing","antigravity-rules-stale"]' ]; then
    ok "setupReasons names missing hook group and stale rules"
  else
    bad "setupReasons names missing hook group and stale rules" "got: $reasons"
  fi
  run_setup; sleep 1
  assert_field antigravityNeedsSetup false
  assert_field providerNeedsSetup false
  shutdown_bram
}

scenario_legacy_hooks() {
  # #173: a project carrying the retired generic hook names.
  local dir="$WORK_ROOT/legacy_hooks"; mkdir -p "$dir/.claude/hooks"
  # issue-448: Setup runs only at a git repo's root.
  git -C "$dir" init -q
  printf '# scratch\n' > "$dir/README.md"
  printf '#!/usr/bin/env python3\n# legacy\n' > "$dir/.claude/hooks/worklist-guard.py"
  printf '#!/usr/bin/env python3\n# legacy\n' > "$dir/.claude/hooks/permission-menu-hook.py"
  launch "$dir" || return
  run_setup; sleep 1
  assert_setup_landed "$dir"
  shutdown_bram
}

scenario_nested() {
  # A managed parent means this launch is nested, which is near-certainly a
  # mistake (lib.rs:33555). Setup is NOT run here -- the point is the warning.
  local parent="$WORK_ROOT/nested_parent"
  local dir="$parent/child"
  mkdir -p "$dir"
  # issue-448: the child must be a project folder (a repo root) for nesting
  # to be the question; a non-repo child is refused before nesting is asked.
  git -C "$dir" init -q
  printf '{}\n' > "$parent/.bram.json"
  printf '# child\n' > "$dir/README.md"
  launch "$dir" || return
  local nested; nested="$(status_field nestedUnder)"
  if [ "$nested" != "null" ] && [ -n "$nested" ]; then
    ok "nestedUnder reported ($nested)"
  else
    bad "nestedUnder reported" "got: $nested"
  fi
  # The targeting check: a nested launch must not be auto-set-up.
  assert_trace "$dir" "\[auto-setup\] op=skip reason=nested" "auto-setup skipped on nested launch"
  if [ -e "$(bram_state_dir "$dir")/.worklist-authorization.json" ]; then
    bad "nested launch not seeded" "worklist authorization present"
  else
    ok "nested launch not seeded"
  fi
  shutdown_bram
}

# ---------------------------------------------------------------------------

trap 'shutdown_bram' EXIT INT TERM

require_bin
command -v jq >/dev/null || { echo "jq required"; exit 127; }
printf 'workdir: %s\n' "$WORK_ROOT"

SCENARIOS=(pristine_nogit subfolder pristine_git already_setup stale_instructions antigravity_reasons legacy_hooks nested)
[ $# -gt 0 ] && SCENARIOS=("$@")

for s in "${SCENARIOS[@]}"; do
  SCENARIO="$s"
  banner "$s"
  if ! declare -f "scenario_$s" > /dev/null; then
    bad "unknown scenario: $s"
    continue
  fi
  "scenario_$s"
  shutdown_bram
done

printf '\n=== %d passed, %d failed ===\n' "$PASS" "$FAIL"
if [ "$FAIL" -gt 0 ] && [ "$KEEP_ON_FAIL" = "1" ]; then
  printf 'projects kept for inspection: %s\n' "$WORK_ROOT"
else
  [ "$FAIL" -eq 0 ] && [ -z "${HARNESS_TMP:-}" ] && rm -rf "$WORK_ROOT"
fi
exit "$FAIL"
