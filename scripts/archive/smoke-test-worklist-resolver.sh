#!/usr/bin/env bash
# Smoke test for /__worklist/resolve consume-on-read behavior (issue #63).
#
# Run from within a Bram PTY shell, where BRAM_PORT (or legacy
# XMLUI_DESKTOP_PORT) is exported into the environment.
#
# Backs up .worklist-authorization.json in the state folder (.bram/ or
# resources/), writes synthetic records, curls the resolver, and asserts
# the response shapes. Restores the backup on exit (success, failure, or interrupt) via a trap.
#
# Scenario 1 — approved consume-on-read:
#   - first GET returns kind=approved
#   - second GET returns kind=no_active_authorization with consumedAtMs set
#
# Scenario 2 — drop is NOT consumed by the resolver:
#   - repeated GETs keep returning kind=drop
#   - (the enforcement path in maybe_enforce_worklist_policy is what consumes
#     drop, which is intentionally not exercised here)

set -euo pipefail

PORT="${BRAM_PORT:-${XMLUI_DESKTOP_PORT:-}}"
if [[ -z "$PORT" ]]; then
  echo "BRAM_PORT not set; run this script from a Bram PTY shell." >&2
  exit 2
fi

. "$(dirname "${BASH_SOURCE[0]}")/bram-state.sh"
AUTH="$(bram_state_dir .)/.worklist-authorization.json"
BACKUP=""

cleanup() {
  if [[ -n "$BACKUP" && -f "$BACKUP" ]]; then
    mv "$BACKUP" "$AUTH"
  fi
}
trap cleanup EXIT

if [[ -f "$AUTH" ]]; then
  BACKUP="$(mktemp)"
  cp "$AUTH" "$BACKUP"
fi
mkdir -p "$(dirname "$AUTH")"

fail=0

assert_kind() {
  local got="$1" want="$2" label="$3"
  if [[ "$got" == "$want" ]]; then
    echo "ok   [$label]: kind=$got"
  else
    echo "FAIL [$label]: expected kind=$want, got kind=$got" >&2
    fail=1
  fi
}

resolve() {
  curl -fsS "http://localhost:$PORT/__worklist/resolve"
}

write_record() {
  local kind="$1"
  local ts
  ts="$(date +%s)000"
  cat > "$AUTH" <<EOF
{
  "kind": "$kind",
  "ids": ["smoke-test-item"],
  "issuedAtMs": $ts,
  "source": "smoke-test",
  "consumedAtMs": null
}
EOF
}

# --- Scenario 1: approved consume-on-read ------------------------------------
write_record approved
r1="$(resolve)"
assert_kind "$(jq -r .kind <<<"$r1")" "approved" "approved-first"

r2="$(resolve)"
assert_kind "$(jq -r .kind <<<"$r2")" "no_active_authorization" "approved-second"

consumed="$(jq -r .consumedAtMs <<<"$r2")"
if [[ "$consumed" == "null" || -z "$consumed" ]]; then
  echo "FAIL [approved-second]: consumedAtMs missing from no_active_authorization" >&2
  fail=1
else
  echo "ok   [approved-second]: consumedAtMs=$consumed"
fi

# --- Scenario 2: drop is not consumed by the resolver ------------------------
write_record drop
r3="$(resolve)"
assert_kind "$(jq -r .kind <<<"$r3")" "drop" "drop-first"

r4="$(resolve)"
assert_kind "$(jq -r .kind <<<"$r4")" "drop" "drop-second"

if [[ "$fail" -ne 0 ]]; then
  echo "FAIL" >&2
  exit 1
fi
echo "PASS"
