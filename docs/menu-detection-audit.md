# Menu detection: hook vs grid (dependency audit)

_2026-07-13. Establishes which detection path surfaces each permission-menu
family, why, and which timing machinery is load-bearing vs redundant. Basis
for scoping the post-dismiss suppressor out of the hook-covered common case._

## Premise correction: byte-pattern scraping is already retired

Raw byte-pattern scraping of PTY output no longer exists.
`pty_menu_update` starts `detected: Option<PtyMenu> = None` and only the
grid build/override sets it (`lib.rs:4952` — _"Byte detection retired: the
grid (read clean from xterm.js) is now the sole menu detector"_). The
`reason=byte-pattern` label in `[pty-menu] state=shown` traces is a
**legacy misnomer** — it means *grid-detected*; the real detection source
is the `signature_source` field (`grid` | `jsonl` | `hook`).

So there is no byte-pattern layer to delete. There are two paths:

- **Hook** — `app/__shell/permission-menu-hook.py` (Claude) /
  `app/shell/codex-permission-menu-hook.py` (Codex) POST to
  `/__menu/permission[/clear]`; host `handle_permission_menu`
  (`lib.rs:3119`). Authoritative: claims the slot via
  `set_menu_hook_owner` (`lib.rs:2528`), and is causal on **both** show
  (PermissionRequest / PreToolUse) **and** clear (PostToolUse /
  PermissionDenied).
- **Grid** — xterm's clean rendered grid, reported via `report_grid_menu`
  (`lib.rs:4717`) and applied in `pty_menu_update`. Defers while the hook
  owns the slot (`op=grid-deferred`, `lib.rs:2529`); acts as fallback
  otherwise.

## Family × path coverage

| Family | Recognized at | Hook | Grid | Authoritative |
|---|---|---|---|---|
| Claude Family A (Bash, Edit, Write, MultiEdit, NotebookEdit, apply_patch, `mcp__*`) | `permission-menu-hook.py:76`; `lib.rs:2890` | ✓ PermissionRequest | ✓ fallback + signature-less shape classify (`lib.rs:5170`) | Hook |
| Claude Family B (AskUserQuestion) | `permission-menu-hook.py:83`; `lib.rs:2657` | ✓ PreToolUse | ✓ fallback (title extract) | Hook |
| ExitPlanMode | `lib.rs:2906` denylist | ✗ (denylisted) | ✓ **sole path** | **Grid** |
| Codex Shell / File-Patch / Generic | `codex-permission-menu-hook.py:129`; `lib.rs:2776` | ✓ PermissionRequest / PostToolUse (`lib.rs:18695`) | ✓ fallback (`grid_menu_is_codex_permission_box`, `lib.rs:2234`) | Hook |

Codex fires **real** PermissionRequest/PostToolUse events — it is
hook-covered, not grid-dependent. The Codex grid path is defensive
fallback for hook timeout/absence, not the primary.

## Where the timing machinery lives, and for whom

Every duration-dependent piece — `fence_decision` (`lib.rs:1706`),
`pty_menu_suppressed_cell` (`lib.rs:4880`), the `pre-absence-redetect` /
`absence-fence` / `unproven-frame-redetect` states, and the observe-only
`op=surface-gap` instrument — lives in the **grid** path and does one job:
**post-dismiss stale-reread suppression** (the grid keeps re-reading the
terminal and can re-detect a just-dismissed menu's cells).

**Evidence-backed core finding:** the grid suppressor fires on
**hook-covered** menus. The 2026-07-13 misses — a 39 s blindness
(`op=surface-gap ms=39024`) and a re-show storm — both had `op=permission`
from the hook yet still hit `pre-absence-redetect`. For a hook-covered
menu the hook's PostToolUse→clear is already the authoritative "menu gone,"
so the grid's absence-fence timing there is **redundant**. Redundant-but-
active is precisely what generates the misses.

Grid detection is **genuinely load-bearing** only for:

- **ExitPlanMode** — never reaches the hook (denylist `lib.rs:2906`); grid
  is the sole detector, including its own dismiss handling.
- **Hook-slow / absent fallback window** — the ~1.5 s before the hook
  claims (`lib.rs:5017`), and the hook-timeout reclaim path.
- **Signature-less shape classification** — Edit/Write/Bash before the
  JSONL carries the tool_use signature (`lib.rs:5170`).

## Decision this yields

**For hook-covered menus, the hook's clear is authoritative: the grid must
defer to it and NOT run `fence_decision` at all.** The suppressor's timing
heuristics remain only for grid-only menus (ExitPlanMode) and the bounded
hook-absent window.

This is the causal, duration-free target: gate the grid re-surface on hook
ownership / hook-cleared state — which `retire_dismissed_menu_on_hook_claim`
(`lib.rs:4884`) already tracks — not on absence-fence timing. Nearly every
menu becomes hook-authoritative with no suppressor in the loop; the timing
machinery survives only where nothing else can clear the menu.

The code change (scope `fence_decision` to grid-only menus / make
hook-clear authoritative for hook-covered menus) is a **separate item**,
gated on this doc so the boundary is written down before the suppressor is
touched.
