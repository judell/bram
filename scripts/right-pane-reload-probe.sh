#!/usr/bin/env bash
# Unattended reproduction + regression guard for judell/bram#385: the
# target-app preview pane reloaded on ANY watched filesystem change,
# including another project's Claude session transcript and writes under
# ~/.bram, because `right-pane-reload` used to be the watcher's `else`
# fallback -- "unclassified" silently meant "reload the user's app".
#
# Why this exists as a script rather than a transcript: the reproduction is
# fully unattended -- the trigger is a file touch, the assertion is a trace
# grep, no UI interaction -- so it belongs in git, not in a session that
# scrolls away. See docs/developing-bram.md, "Run a second instance against
# a throwaway project" for the second-instance technique this borrows, and
# scripts/would-deny-probe.sh for the shape this follows (pass/fail
# counters, exit status = failed-assertion count, KEEP_ON_FAIL, refuses to
# touch anything it did not create).
#
# What it drives, in one throwaway project with the target-app pane on:
#   1. Three appends to a FOREIGN .jsonl under a fake sibling of
#      ~/.claude/projects/<this-project's-slug> -- content belonging to no
#      real project at all, sharpening the filing: this is not one project
#      watching another, it is that anything appearing under
#      ~/.claude/projects reloads the preview. Must produce a traced SKIP
#      naming why, and zero new right-pane-reload dispatches/signals.
#   2. One edit to the project's OWN index.html. Must still produce
#      dispatch=right-pane and the reload signal, unchanged -- the direction
#      a later watcher refactor could silently drop, disabling the feature
#      entirely while direction 1 stays green.
#   3. One write under ~/.bram (the latent second case named in the fix:
#      the guard-link re-ensure ticker writes here too). Must also SKIP.
#
# Measured against the pre-fix build (2026-09-21, this repo's own debug
# binary): 3 appends to a foreign .jsonl produced 3 `dispatch=right-pane`
# lines naming the bare filename; 1 edit to the project's own index.html
# produced 1 more. Run this probe against an unfixed build first --
# direction 1 must fail, direction 2 must pass -- before trusting a fix.
#
# Usage:
#   scripts/right-pane-reload-probe.sh
#   BRAM_BIN=/path/to/bram scripts/right-pane-reload-probe.sh
#   KEEP_ON_FAIL=0 scripts/right-pane-reload-probe.sh
#
# Exit status is the number of failed assertions (0 = all pass).
#
# Safety, read before changing anything here:
#   - Never `pkill bram` -- the user's OWN working Bram is a `bram` process
#     too. This script records the exact child PID at launch and kills only
#     that PID on teardown.
#   - The foreign session directory and the ~/.bram marker file live OUTSIDE
#     the scratch project, so they are removed in teardown unconditionally
#     (not gated by KEEP_ON_FAIL, which only governs the scratch project
#     directory) -- leaving a fake project under the real
#     ~/.claude/projects/ would clutter Sessions/search indexing in every
#     Bram instance on the machine, not just this one.
#   - Every path this script deletes is checked against a pattern that only
#     matches something IT generated (embedding this process's PID) before
#     the delete runs, so a corrupted variable can't turn a cleanup into a
#     wider deletion.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRAM_BIN="${BRAM_BIN:-$REPO_ROOT/src-tauri/target/debug/bram}"
KEEP_ON_FAIL="${KEEP_ON_FAIL:-1}"
PID_TAG="right-pane-reload-probe-$$"

PASS=0
FAIL=0
WORK=""
FOREIGN_DIR=""
BRAM_MARKER=""
BRAM_PID=""
TRACE_LOG=""
LAUNCH_LOG=""

pass() { PASS=$((PASS + 1)); printf '  ok    %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$1"; }

# --- teardown -----------------------------------------------------------
# Kill only the PID this script spawned. Never pkill -- the caller's own
# Bram is a `bram` process too.
kill_bram() {
  if [ -n "$BRAM_PID" ] && kill -0 "$BRAM_PID" 2>/dev/null; then
    kill "$BRAM_PID" 2>/dev/null
    for _ in $(seq 1 20); do
      kill -0 "$BRAM_PID" 2>/dev/null || break
      sleep 0.2
    done
    kill -9 "$BRAM_PID" 2>/dev/null || true
  fi
  BRAM_PID=""
}

teardown() {
  kill_bram
  # These two live outside the scratch project and are never worth keeping
  # around, pass or fail -- only remove paths that carry this process's own
  # PID tag, so a stale variable can never widen the blast radius.
  if [ -n "$FOREIGN_DIR" ] && [[ "$FOREIGN_DIR" == *"$PID_TAG"* ]] && [ -d "$FOREIGN_DIR" ]; then
    rm -rf "$FOREIGN_DIR"
  fi
  if [ -n "$BRAM_MARKER" ] && [[ "$BRAM_MARKER" == *"$PID_TAG"* ]] && [ -e "$BRAM_MARKER" ]; then
    rm -f "$BRAM_MARKER"
  fi
  if [ -n "$WORK" ] && [ -d "$WORK" ]; then
    if [ "$FAIL" -gt 0 ] && [ "$KEEP_ON_FAIL" = "1" ]; then
      printf '  kept  %s\n' "$WORK"
    else
      rm -rf "$WORK"
    fi
  fi
}
trap teardown EXIT

# --- setup ----------------------------------------------------------------
setup_project() {
  WORK="$(mktemp -d -t bram-right-pane-reload-probe)"
  git init -q "$WORK"
  git -C "$WORK" -c user.email=probe@example.com -c user.name=probe \
    commit -q --allow-empty -m "right-pane-reload-probe scratch root"
  cat > "$WORK/index.html" <<'HTML'
<!doctype html>
<html><body>right-pane-reload-probe scratch target</body></html>
HTML
  # ui.showTargetApp: true -- the reported bug is specifically about the
  # target-app preview pane, and it exists whether or not the pane is
  # visually shown (the watcher classifies unconditionally), but this
  # keeps the fixture honest about the scenario in the filing.
  printf '{"ui":{"showTargetApp":true}}' > "$WORK/.bram.json"
  TRACE_LOG="$WORK/resources/bram-traces/bram-trace.log"
  LAUNCH_LOG="$WORK/bram-launch.log"
}

# CLAUDE_CODE_* scrub: an agent-launched instance otherwise inherits the
# parent's session markers and its own agent concludes it's a child session
# and disables transcript saving (docs/developing-bram.md, "Two launch-
# hygiene requirements the [demo-instance] method itself surfaced").
# BRAM_TRACE=1 is belt-and-braces (traces default on) but pins the intent.
launch_bram() {
  env -u CLAUDE_CODE_CHILD_SESSION -u CLAUDE_CODE_SESSION_ID \
      -u CLAUDE_CODE_BRIDGE_SESSION_ID -u CLAUDE_CODE_MESSAGING_SOCKET \
      -u CLAUDE_CODE_MESSAGING_TOKEN -u CLAUDE_CODE_ENTRYPOINT \
      -u CLAUDE_CODE_EXECPATH \
      BRAM_TRACE=1 "$BRAM_BIN" "$WORK" > "$LAUNCH_LOG" 2>&1 &
  BRAM_PID=$!
}

wait_for_ready() {
  local i
  for i in $(seq 1 150); do
    if ! kill -0 "$BRAM_PID" 2>/dev/null; then
      printf 'bram exited during launch; log tail:\n' >&2
      tail -n 40 "$LAUNCH_LOG" >&2 2>/dev/null
      return 1
    fi
    if [ -f "$WORK/resources/.bram-port" ] && grep -q '\[watcher\] watching' "$LAUNCH_LOG" 2>/dev/null; then
      # Give the watcher a beat past its first "watching" line to finish
      # registering every root (proj_root, tools-pane dirs, sessions
      # parent, ~/.bram, current-agent) before we start perturbing it.
      sleep 0.5
      return 0
    fi
    sleep 0.3
  done
  printf 'timed out waiting for bram watcher to come up; log tail:\n' >&2
  tail -n 40 "$LAUNCH_LOG" >&2 2>/dev/null
  return 1
}

# --- trace helpers ---------------------------------------------------------
trace_line_count() {
  wc -l < "$TRACE_LOG" 2>/dev/null | tr -d ' ' || echo 0
}

# `$1` count-before. Prints only the lines appended since.
new_trace_lines() {
  local since="$1"
  tail -n "+$((since + 1))" "$TRACE_LOG" 2>/dev/null
}

# `$1` label, `$2` lines (already the sliced new-lines text), `$3` grep
# pattern, `$4` expected count.
expect_count() {
  local label="$1" lines="$2" pattern="$3" want="$4" got
  got="$(printf '%s\n' "$lines" | grep -cE "$pattern" 2>/dev/null || true)"
  got="${got:-0}"
  if [ "$got" = "$want" ]; then
    pass "$label"
  else
    fail "$label (wanted $want, got $got)"
    printf '%s\n' "$lines" | grep -E "$pattern" | sed 's/^/        /' | head -10
  fi
}

# `$1` label, `$2` lines, `$3` pattern -- passes when at least one line matches.
expect_at_least_one() {
  local label="$1" lines="$2" pattern="$3" got
  got="$(printf '%s\n' "$lines" | grep -cE "$pattern" 2>/dev/null || true)"
  got="${got:-0}"
  if [ "$got" -ge 1 ]; then
    pass "$label ($got line(s))"
  else
    fail "$label (wanted >=1, got 0)"
  fi
}

settle() { sleep 1.5; }

# --- direction 1: a transcript belonging to NO project reloads nothing ----
probe_foreign_transcript() {
  printf '\ndirection 1: foreign session transcript under ~/.claude/projects\n'
  FOREIGN_DIR="$HOME/.claude/projects/-BRAM-PROBE-$PID_TAG"
  if [ -e "$FOREIGN_DIR" ]; then
    fail "refusing: $FOREIGN_DIR already exists (not created by this run)"
    return
  fi
  mkdir -p "$FOREIGN_DIR"
  local before
  before="$(trace_line_count)"
  local i
  for i in 1 2 3; do
    printf '{"probe":"append-%s"}\n' "$i" >> "$FOREIGN_DIR/probe.jsonl"
    sleep 0.4
  done
  settle
  local lines
  lines="$(new_trace_lines "$before")"
  expect_count "no right-pane-reload dispatch for the foreign transcript" \
    "$lines" '\[watcher\] dispatch=right-pane path=probe\.jsonl' 0
  expect_count "no right-pane-reload SIGNAL fired during the foreign-transcript window" \
    "$lines" '\[emit\] kind=right-pane-reload' 0
  expect_at_least_one "foreign transcript appends produce a named skip" \
    "$lines" '\[watcher\] dispatch=skip path=probe\.jsonl reason=foreign-claude-session'
}

# --- direction 2: the project's own content still reloads, unchanged -----
probe_own_file_edit() {
  printf '\ndirection 2: edit to the project'"'"'s own index.html\n'
  local before
  before="$(trace_line_count)"
  printf '<!-- probe edit -->\n' >> "$WORK/index.html"
  settle
  local lines
  lines="$(new_trace_lines "$before")"
  expect_at_least_one "own-file edit dispatches right-pane" \
    "$lines" '\[watcher\] dispatch=right-pane path=index\.html'
  expect_at_least_one "own-file edit fires the right-pane-reload signal" \
    "$lines" '\[emit\] kind=right-pane-reload'
}

# --- direction 3: writes under ~/.bram reload nothing (the latent case) --
probe_bram_dir_write() {
  printf '\ndirection 3: write under ~/.bram (guard-link re-ensure ticker'"'"'s directory)\n'
  BRAM_MARKER="$HOME/.bram/.probe-marker-$PID_TAG"
  if [ -e "$BRAM_MARKER" ]; then
    fail "refusing: $BRAM_MARKER already exists (not created by this run)"
    return
  fi
  local before
  before="$(trace_line_count)"
  printf 'probe\n' > "$BRAM_MARKER"
  settle
  local lines marker_name
  marker_name="$(basename "$BRAM_MARKER")"
  lines="$(new_trace_lines "$before")"
  expect_count "no right-pane-reload dispatch for a ~/.bram write" \
    "$lines" "\\[watcher\\] dispatch=right-pane path=${marker_name}" 0
  expect_count "no right-pane-reload SIGNAL fired during the ~/.bram window" \
    "$lines" '\[emit\] kind=right-pane-reload' 0
  expect_at_least_one "~/.bram write produces a named skip" \
    "$lines" "\\[watcher\\] dispatch=skip path=${marker_name} reason=bram-dir"
}

main() {
  if [ ! -x "$BRAM_BIN" ]; then
    printf 'bram binary not found or not executable: %s\n' "$BRAM_BIN" >&2
    printf 'build it first: cargo build --manifest-path src-tauri/Cargo.toml\n' >&2
    return 1
  fi
  printf 'bram binary: %s\n' "$BRAM_BIN"
  printf 'built:       %s\n' "$(date -r "$BRAM_BIN" 2>/dev/null || stat -c %y "$BRAM_BIN" 2>/dev/null)"

  setup_project
  printf 'scratch project: %s\n' "$WORK"
  launch_bram
  if ! wait_for_ready; then
    fail "bram watcher came up"
    printf '\nfailed %d\n' "$FAIL"
    return "$FAIL"
  fi
  pass "bram watcher came up"

  probe_foreign_transcript
  probe_own_file_edit
  probe_bram_dir_write

  printf '\npassed %d, failed %d\n' "$PASS" "$FAIL"
  return "$FAIL"
}

main "$@"
