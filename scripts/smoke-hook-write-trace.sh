#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUARD="$ROOT/app/__shell/worklist-guard.py"
. "$ROOT/scripts/bram-state.sh"
TRACE="$(bram_state_dir "$ROOT")/bram-traces/bram-trace.log"

if [[ ! -f "$GUARD" ]]; then
  echo "missing guard file: $GUARD" >&2
  exit 2
fi

if [[ ! -f "$TRACE" ]]; then
  echo "missing trace log: $TRACE" >&2
  echo "Start Bram with tracing enabled before running this smoke." >&2
  exit 2
fi

before_lines="$(wc -l < "$TRACE" | tr -d ' ')"
tmp="$(mktemp "${TMPDIR:-/tmp}/bram-hook-guard.XXXXXX")"
restore() {
  if [[ -f "$tmp" ]]; then
    cp "$tmp" "$GUARD"
    rm -f "$tmp"
  fi
}
trap restore EXIT

cp "$GUARD" "$tmp"
guard_mode="$(stat -f '%Lp' "$GUARD" 2>/dev/null || stat -c '%a' "$GUARD")"

sentinel="$(mktemp "${TMPDIR:-/tmp}/bram-hook-guard-sentinel.XXXXXX")"
{
  printf '# bram hook-write smoke sentinel %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  cat "$tmp"
} > "$sentinel"
mv "$sentinel" "$GUARD"
chmod "$guard_mode" "$GUARD"

sleep 0.4
restore_copy="$(mktemp "${TMPDIR:-/tmp}/bram-hook-guard-restore.XXXXXX")"
cp "$tmp" "$restore_copy"
mv "$restore_copy" "$GUARD"
chmod "$guard_mode" "$GUARD"
sleep 0.8

new_lines="$(tail -n +"$((before_lines + 1))" "$TRACE" | grep '\[hook-write\].*target=app/__shell/worklist-guard.py' || true)"

if [[ -z "$new_lines" ]]; then
  echo "no [hook-write] trace observed for app/__shell/worklist-guard.py" >&2
  echo "Confirm the current Bram process includes hook-write instrumentation and tracing is enabled." >&2
  exit 1
fi

if ! printf '%s\n' "$new_lines" | awk '
  /pre=/ && /post=/ {
    pre = $0
    post = $0
    sub(/^.* pre=/, "", pre)
    sub(/ post=.*$/, "", pre)
    sub(/^.* post=/, "", post)
    sub(/ post_size=.*$/, "", post)
    if (pre != post) found = 1
  }
  END { exit found ? 0 : 1 }
'; then
  echo "[hook-write] trace appeared, but no changed pre/post fingerprint was found." >&2
  printf '%s\n' "$new_lines" >&2
  exit 1
fi

echo "hook-write trace smoke passed"
printf '%s\n' "$new_lines"
