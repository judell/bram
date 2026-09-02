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
    if [ -f "$dir/resources/.bram-port" ]; then
      BRAM_PORT="$(cat "$dir/resources/.bram-port" 2>/dev/null)"
      [ -n "$BRAM_PORT" ] && sleep 1 && return 0
    fi
    kill -0 "$BRAM_PID" 2>/dev/null || { bad "launch" "process exited; see $WORK_ROOT/$SCENARIO.log"; return 1; }
    sleep 0.5
  done
  bad "launch" "no port file after 40s; see $WORK_ROOT/$SCENARIO.log"
  return 1
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

# Fingerprint everything Setup owns. resources/ is excluded: the port file,
# port sidecar and traces legitimately change between runs.
fingerprint() {
  local dir="$1"
  (cd "$dir" && find . -type f \
      -not -path './resources/*' -not -path './.git/*' \
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
    if grep -q "$pattern" "$dir/resources/bram-traces/bram-trace.log" 2>/dev/null; then
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
  assert_file "$dir" "resources/.worklist-authorization.json"
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
  local dir="$WORK_ROOT/pristine_nogit"; mkdir -p "$dir"
  printf '# scratch\n' > "$dir/README.md"
  launch "$dir" || return
  # Auto-setup replaced the old post-launch "firstRun true" observation: by
  # the time the API answers, Setup has already run. The trace records what
  # firstRun was at the moment of decision.
  assert_trace "$dir" "\[auto-setup\] op=ran firstRun=true" "auto-setup ran on first launch"
  sleep 1
  assert_setup_landed "$dir"
  assert_idempotent "$dir"
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
  printf 'resources/bram-traces/\nresources/.bram-port*\n' > "$dir/.gitignore"
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

scenario_legacy_hooks() {
  # #173: a project carrying the retired generic hook names.
  local dir="$WORK_ROOT/legacy_hooks"; mkdir -p "$dir/.claude/hooks"
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
  if [ -e "$dir/resources/.worklist-authorization.json" ]; then
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

SCENARIOS=(pristine_nogit pristine_git already_setup legacy_hooks nested)
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
