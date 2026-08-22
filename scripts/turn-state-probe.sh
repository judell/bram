#!/usr/bin/env bash
# Sample every source of "is the agent busy" at once, and show what each
# consumer derives from it.
#
# Why together: each source is trivially easy to fetch on its own, and the
# question is never what one of them says — it is whether they agree. Four
# surfaces gate or display on three different derivations of turn completion
# (docs/turn-state-consumers.md), so a disagreement between two of them is the
# first thing worth seeing.
#
# Read-only. Usage:
#   scripts/turn-state-probe.sh              # one sample
#   scripts/turn-state-probe.sh --watch      # sample every 2s until Ctrl-C
#   scripts/turn-state-probe.sh --watch 5    # ...every 5s
#
# The interesting observation is usually a TRANSITION, or its absence: interrupt
# a turn, watch, and see whether the gates release.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT_FILE="${BRAM_PORT_FILE:-$REPO_ROOT/resources/.bram-port}"

WATCH=0
INTERVAL=2
if [ "${1:-}" = "--watch" ]; then
  WATCH=1
  [ -n "${2:-}" ] && INTERVAL="$2"
fi

if [ ! -f "$PORT_FILE" ]; then
  echo "no port file at $PORT_FILE — is Bram running against this project?"
  exit 1
fi
PORT="$(cat "$PORT_FILE")"
command -v jq >/dev/null || { echo "jq required"; exit 127; }

api() {
  curl -4 -sS --max-time 5 "http://127.0.0.1:$PORT/$1" 2>/dev/null
}

sample() {
  local status inflight ledger menu
  status="$(api "__agent-status")"
  inflight="$(api "__inflight")"
  ledger="$(api "__send-ledger")"
  menu="$(api "__pty-menu")"

  local state source claim_ids claim_kind awaiting menu_present
  state="$(printf '%s' "$status"   | jq -r '.state // "?"' 2>/dev/null)"
  source="$(printf '%s' "$status"  | jq -r '.source // "-"' 2>/dev/null)"
  claim_ids="$(printf '%s' "$inflight" | jq -r '(.ids // []) | join(",")' 2>/dev/null)"
  claim_kind="$(printf '%s' "$inflight" | jq -r '.kind // "-"' 2>/dev/null)"
  awaiting="$(printf '%s' "$ledger" | jq -r '.awaitingTurn // false' 2>/dev/null)"
  menu_present="$(printf '%s' "$menu" | jq -r 'if (.tool // "") == "" then "no" else "yes" end' 2>/dev/null)"
  [ -z "$claim_ids" ] && claim_ids="(none)"

  printf '\n%s\n' "$(date '+%H:%M:%S')"
  printf '  %-28s %s\n' "agent-status.state"      "$state (source=$source)"
  printf '  %-28s %s\n' "inflight claim"          "$claim_ids (kind=$claim_kind)"
  printf '  %-28s %s\n' "send-ledger.awaitingTurn" "$awaiting"
  printf '  %-28s %s\n' "pty menu displayed"      "$menu_present"

  # What each consumer derives. Mirrors the classification in
  # docs/turn-state-consumers.md; keep the two in step.
  local queue worklist workspace
  if [ "$menu_present" = "yes" ]; then
    queue="HELD (menu pending — hold)"
  elif [ "$state" = "working" ]; then
    queue="HELD (agent working — hold)"
  else
    queue="ready to send"
  fi
  if [ "$claim_ids" = "(none)" ]; then
    worklist="buttons enabled (no claim)"
  else
    worklist="HELD (claim live: $claim_ids)"
  fi
  if [ "$awaiting" = "true" ]; then
    workspace="HELD (awaitingResponse)"
  else
    workspace="buttons enabled"
  fi

  printf '  --\n'
  printf '  %-28s %s\n' "Footer (display)"        "$state"
  printf '  %-28s %s\n' "Queue Send (gate)"       "$queue"
  printf '  %-28s %s\n' "Worklist gate"           "$worklist"
  printf '  %-28s %s\n' "Workspace legacy (gate)" "$workspace"
}

if [ "$WATCH" = "1" ]; then
  printf 'watching every %ss on port %s — Ctrl-C to stop\n' "$INTERVAL" "$PORT"
  while true; do
    sample
    sleep "$INTERVAL"
  done
else
  sample
  printf '\n'
fi
