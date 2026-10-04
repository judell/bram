# PTY menu survival tunables (host-side)

> **Status (2026-10-03): historical.** This describes the byte-scanning
> detector of June 2026. `MENU_EVICTION_GRACE_MS`, the `hold-start` /
> `holding` / `hold-expired` / `buffer-evicted` states and the 10 s
> post-click suppression no longer exist: byte detection was retired in
> favor of the xterm.js grid, and dismissal is now decided by the absence
> fence (see `docs/menu-detection-audit.md`). Only the 64 KB tail cap
> survives. The thresholds that govern behavior today are in
> [Current thresholds](#current-thresholds-2026-10-03) at the end.

Two constants in `src-tauri/src/lib.rs` govern how long the host
keeps a detected permission menu visible to the agent pane after
the menu's bytes scroll past the detector's view in the PTY tail.
Both were tuned in response to incident #182 #17, where a Bash
permission menu was dismissed host-side 37 seconds after detection
while Claude's TUI was still presenting it on the terminal —
leaving the user with a prompt in the terminal and nothing
actionable in the agent pane.

## The constants

### `MENU_EVICTION_GRACE_MS` (60_000 — 60 s)

How long `pty_menu_update` defers the
`state=dismissed reason=buffer-evicted` emit after
`pty_menu_detect` stops finding the menu signature in the current
PTY tail. Any TUI redraw that brings the menu bytes back into the
tail within the grace clears the grace via the
`detected.is_some()` branch and keeps the menu visible. If no
redraw arrives within the grace window, the host emits the
dismiss legitimately.

Previously 1500 ms, which dismissed the menu host-side whenever a
user paused for more than ~1.5 s of TUI silence — far too short
for real "walk back to the keyboard" delays.

### PTY tail cap (65_536 bytes — 64 KB)

The rolling byte history `pty_menu_update` retains and scans
(`tail.len() > 65536` trim in `pty_menu_update`). Larger tail
means more chances for the menu signature to stay in scan range
across partial TUI redraws (spinner animation, status-line
updates) that don't re-emit the menu region.

Previously 8 KB. Byte-pattern scan at 64 KB is still sub-millisecond,
so the cost is the extra memory (~56 KB per Bram process).

## Companion: post-click suppression

`pty_menu_suppressed_cell` blocks re-detection of a tool for a
window after the user dismisses its menu by keystroke — without
this, the just-dismissed menu's bytes still sitting in the (now
larger) tail would re-trip detection.

The suppression duration is paired with the tail cap: bumped from
2 s to 10 s when the tail grew from 8 KB to 64 KB. Roughly, the
suppression should cover the time it takes for the dismissed-menu
bytes to scroll off the new tail under typical PTY chunk rates.

## Diagnosing menu-survival issues

If a permission menu disappears from the agent pane prematurely
despite the terminal still presenting it, grep the trace:

```bash
LOG=resources/bram-traces/bram-trace.log
grep -E 'state=shown|state=hold-start|state=holding|state=hold-expired|state=dismissed' "$LOG"
```

Read the sequence around the dismissal:

- `state=shown … reason=byte-pattern` — detector found the menu;
  this is the moment the agent pane should render it.
- `state=hold-start … reason=buffer-evicted grace_ms=60000` — the
  current chunk doesn't contain the menu; deferring dismiss.
- `state=holding … elapsed_ms=… grace_ms=60000` — a subsequent
  chunk still doesn't contain it; still deferring.
- `state=hold-expired … reason=buffer-evicted elapsed_ms=…` —
  grace ran out; the next line will be `state=dismissed`.
- `state=dismissed … reason=user-input` — user pressed a key, host
  cleaned up. **Distinct from `reason=buffer-evicted`** — the
  former is a real dismissal, the latter is the host giving up on
  a still-presented menu.

If `state=dismissed reason=buffer-evicted` fires in a scenario
where the user is still looking at the menu on the terminal, that
is the failure mode incident #182 #17 documented. Consider whether
to raise `MENU_EVICTION_GRACE_MS` further or whether the menu
bytes truly are no longer in the tail.

## Where the constants live

- `MENU_EVICTION_GRACE_MS`: `src-tauri/src/lib.rs` near line 1034.
- PTY tail cap (literal `65536`): `src-tauri/src/lib.rs` inside
  `pty_menu_update`, in the chunk-append trim branch.
- Post-click suppression `Duration::from_secs(10)`:
  `src-tauri/src/lib.rs` inside `pty_menu_update`, after the
  `pty_menu_suppressed_cell` lookup.

## See also

- [`docs/apis.md`](apis.md) — the HTTP-route reference, including
  `/__turn-state` which carries the live `pendingMenu` payload to
  the agent pane.
- Issue #182 incident #17:
  https://github.com/judell/bram/issues/182#issuecomment-4674505038

## Current thresholds (2026-10-03)

Verified against the code on this date. All in `src-tauri/src/lib.rs`
unless noted.

| What | Value | Where |
|---|---|---|
| PTY tail kept for raw-byte scans | 64 KB | `tail.len() > 65536` in `pty_menu_update` (`:11059`) |
| Spinner silence that counts as turn activity stopping | 800 ms | `AGENT_TURN_IDLE_THRESHOLD_MS` (`:9867`) |
| Silence before the busy spinner is cleared | 3 s | `MIN_SILENCE_FOR_SENTINEL_CLEAR_MS` (`:9874`) |
| Re-emit cooldown for turn-activity events | 5 s | `AGENT_TURN_EMIT_COOLDOWN_MS` (`:9883`) |
| Silence before the status row is cleared (never while a menu is pending) | 10 s | `ROW_HARD_KILL_MIN_SILENCE_MS` (`:9895`) |
| Grid menu report counts as fresh for building a menu | 1.5 s | literal in `pty_menu_update` (`:11152`) |
| Grid snapshot counts as fresh for joining a hook claim | 2.5 s | `CLAIM_JOIN_GRID_FRESH_MS` (`:1209`) |
| Hook claim coalesce window | 50 ms | `MENU_HOOK_CLAIM_COALESCE_MS` (`:1215`) |
| Hook ownership timeout | 300 s | `MENU_HOOK_OWNER_TIMEOUT_MS` (`:975`) |
| Hook claim TTL | 300 s | `MENU_HOOK_CLAIM_TTL_MS` (`:1016`) |
| Pending menu answer TTL | 60 s | `PENDING_MENU_ANSWER_TTL_MS` (`:40404`) |
| Send-gate stale warning | 120 s | `SEND_GATE_STALE_WARN_MS` (`:37165`) |
| Grid menu keep-alive re-report while a menu is up | 1 s | `setInterval` in `app/main.js:1611-1625` |

Whether turn-finished is final comes from the provider's transcript
(Claude `end_turn`, Codex `task_complete`), not from these silence
windows.
