#!/usr/bin/env bash
# Deliberate violation of the worklist authorization rule, to prove the
# `would-deny` observer actually fires -- and, just as importantly, that it
# stays silent when it should.
#
# Why this exists: judell/bram#368's decision not to deny general-turn edits
# rests on a soak that read 3 fires. A tripwire's zero and a dead instrument's
# zero are identical in a grep (docs/developing-bram.md names this trap and
# prescribes a deliberate violation rather than a longer wait). The census
# (scripts/would-deny-census.py) answers "what did the predicate do over
# history"; this answers "does the instrument work right now".
#
# It has already earned its keep. Run by hand on 2026-09-21 it falsified
# judell/bram#387's central claim -- that Codex never emits `would-deny` -- in
# minutes, after that claim had been filed as an issue and repeated in a
# correction comment. It was rebuilt from scratch twice in one session and
# survived only as prose both times; this file is that recipe made runnable by
# someone who was not there.
#
# The technique: `bram-guard` is a hook binary that reads a JSON payload on
# stdin and appends to `<project>/resources/bram-traces/hook-events.log`. So a
# violation needs no live agent session and no running Bram -- just a scratch
# project and a crafted payload.
#
# Usage:
#   scripts/would-deny-probe.sh
#   GUARD_BIN=/path/to/bram-guard scripts/would-deny-probe.sh
#
# Exit status is the number of failed assertions (0 = all pass).

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUARD_BIN="${GUARD_BIN:-$REPO_ROOT/src-tauri/target/debug/bram-guard}"
KEEP_ON_FAIL="${KEEP_ON_FAIL:-1}"

PASS=0
FAIL=0
WORK=""

pass() { PASS=$((PASS + 1)); printf '  ok    %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$1"; }

# A scratch project the guard will recognize as managed: the authorization
# file is the marker `codex_dispatch` requires (`cwd.join(AUTH_REL).exists()`),
# and one begun item declares `app/covered.js`.
#
# `consumedAtMs` present = the authorization is spent, so nothing is live and a
# covered edit is "unaddressed". Omitting it with a fresh `issuedAtMs` is the
# live case below.
setup_project() {
  local auth_json="$1"
  WORK="$(mktemp -d -t bram-would-deny-probe)"
  mkdir -p "$WORK/resources" "$WORK/app"
  git init -q "$WORK"
  printf '%s' "$auth_json" > "$WORK/resources/.worklist-authorization.json"
  printf '{"description":"probe","items":[{"id":"probe-item","status":"proposed","begunAtMs":1,"files":["app/covered.js"]}],"version":1}' \
    > "$WORK/resources/worklist.json"
  echo "x" > "$WORK/app/covered.js"
}

teardown_project() {
  if [ -n "$WORK" ] && [ -d "$WORK" ]; then
    if [ "$FAIL" -gt 0 ] && [ "$KEEP_ON_FAIL" = "1" ]; then
      printf '  kept  %s\n' "$WORK"
    else
      rm -rf "$WORK"
    fi
  fi
  WORK=""
}

# Drive the guard with one Bash payload. `$1` is the command as a JSON string
# value (already escaped), `$2` the hook name.
drive() {
  printf '{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":%s},"cwd":"%s"}' \
    "$1" "$WORK" | "$GUARD_BIN" guard "$2" >/dev/null 2>&1
}

log_lines() {
  cat "$WORK/resources/bram-traces/hook-events.log" 2>/dev/null
}

# `$1` label, `$2` grep pattern, `$3` expected count
expect_count() {
  local label="$1" pattern="$2" want="$3" got
  got="$(log_lines | grep -cE "$pattern" 2>/dev/null || true)"
  got="${got:-0}"
  if [ "$got" = "$want" ]; then
    pass "$label"
  else
    fail "$label (wanted $want, got $got)"
    log_lines | sed 's/^/        /'
  fi
}

SPENT_AUTH='{"kind":"approved","ids":["probe-item"],"issuedAtMs":1,"consumedAtMs":1}'

# --- the violation fires -----------------------------------------------------
# A heredoc write to a path a BEGUN item declares, with no authorization live.
# This is the shape #368 is about, arriving through the channel that carries
# most agent writes (judell/bram#387).
probe_fires() {
  local provider="$1"
  printf '\n%s: covered path, authorization spent -> expect would-deny\n' "$provider"
  setup_project "$SPENT_AUTH"
  drive '"cat > app/covered.js <<EOF\nhello\nEOF"' "$provider"
  expect_count "would-deny on the covered path" \
    "would-deny .*target=app/covered\.js.*auth=none" 1
  teardown_project
}

# --- and stays silent when authorization IS live -----------------------------
# The half most likely to be left out, and the one that actually catches
# regressions: it proves the observer keys on AUTHORIZATION, not merely on
# coverage. The observer's first version keyed on turn-addressing and measured
# the wrong thing for weeks before b64d81f corrected it; a probe without this
# case passes clean against that bug.
probe_silent_when_authorized() {
  local provider="$1"
  printf '\n%s: covered path, authorization LIVE -> expect silence\n' "$provider"
  local now
  now="$(( $(date +%s) * 1000 ))"
  setup_project "{\"kind\":\"approved\",\"ids\":[\"probe-item\"],\"issuedAtMs\":${now}}"
  drive '"cat > app/covered.js <<EOF\nhello\nEOF"' "$provider"
  expect_count "no would-deny while authorized" "would-deny " 0
  teardown_project
}

# --- an uncovered path is not this observer's business -----------------------
probe_uncovered() {
  local provider="$1"
  printf '\n%s: uncovered path -> expect silence\n' "$provider"
  setup_project "$SPENT_AUTH"
  drive '"echo hi > app/uncovered.js"' "$provider"
  expect_count "no would-deny on an undeclared path" "would-deny " 0
  teardown_project
}

# --- an unreadable target is recorded, never silent --------------------------
# Without this line, "no fires" cannot be told from "could not look" -- the
# missing-denominator defect that turned out to be the real bug in the close
# queue (#380), the push path (#383), and this observer itself.
probe_unknown_target() {
  local provider="$1"
  printf '\n%s: unextractable target -> expect would-deny-unknown-target\n' "$provider"
  setup_project "$SPENT_AUTH"
  drive '"echo hi > \"$OUT\""' "$provider"
  expect_count "unknown-target line present" "would-deny-unknown-target" 1
  expect_count "and not reported as a resolved target" \
    "would-deny .*target=" 0
  teardown_project
}

main() {
  if [ ! -x "$GUARD_BIN" ]; then
    printf 'guard binary not found or not executable: %s\n' "$GUARD_BIN" >&2
    printf 'build it first: cargo build --manifest-path src-tauri/Cargo.toml\n' >&2
    return 1
  fi
  printf 'guard: %s\n' "$GUARD_BIN"

  # Both providers, because parity here is load-bearing and was wrong in a
  # filed issue until it was checked: Claude writes through Edit/Write/Bash,
  # Codex through apply_patch/Bash, and both branches must observe alike.
  for provider in claude-worklist codex-worklist; do
    probe_fires "$provider"
    probe_silent_when_authorized "$provider"
    probe_uncovered "$provider"
    probe_unknown_target "$provider"
  done

  printf '\npassed %d, failed %d\n' "$PASS" "$FAIL"
  return "$FAIL"
}

main "$@"
