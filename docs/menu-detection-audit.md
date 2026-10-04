# Menu detection: hook vs grid (dependency audit)

_2026-07-13. Establishes which detection path surfaces each permission-menu
family, why, and which timing machinery is load-bearing vs redundant. Basis
for scoping the post-dismiss suppressor out of the hook-covered common case._

> **Status (2026-10-03):** a dated audit, kept as written. Citations were
> refreshed to function names and current `src-tauri/src/lib.rs` lines, and
> the Python hook scripts it named were replaced by the Rust `bram-guard`
> binary in `1378e3e`. For current behavior, see the menu rows of
> `docs/trace-vocabulary.md` and `docs/xterm-grid-screen-reading.md`.

## Premise correction: byte-pattern scraping is already retired

Raw byte-pattern scraping of PTY output no longer exists.
`pty_menu_update` starts `detected: Option<PtyMenu> = None` and only the
grid build/override sets it (`pty_menu_update`, `lib.rs:11086` — _"Byte detection retired: the
grid (read clean from xterm.js) is now the sole menu detector"_). The
`reason=byte-pattern` label in `[pty-menu] state=shown` traces is a
**legacy misnomer** — it means *grid-detected*; the real detection source
is the `signature_source` field (`grid` | `jsonl` | `hook`).

So there is no byte-pattern layer to delete. There are two paths:

- **Hook** — the `bram-guard` binary (`menu_would_action`,
  `src-tauri/src/guard.rs:521`; the Python hooks this audit originally named
  were deleted in `1378e3e`) POSTs to `/__menu/permission[/clear]`; host
  `handle_permission_menu` (`lib.rs:8800`). Authoritative: claims the slot via
  `set_menu_hook_owner` (`lib.rs:977`), and is causal on **both** show
  (PermissionRequest / PreToolUse) **and** clear (PostToolUse /
  PermissionDenied).
- **Grid** — xterm's clean rendered grid, reported via `report_grid_menu`
  (`lib.rs:10575`) and applied in `pty_menu_update`. Defers while the hook
  owns the slot (`op=grid-deferred`, `lib.rs:7905`); acts as fallback
  otherwise.

## Family × path coverage

| Family | Recognized at | Hook | Grid | Authoritative |
|---|---|---|---|---|
| Claude Family A (Bash, Edit, Write, MultiEdit, NotebookEdit, apply_patch, `mcp__*`) | `guard.rs:524`; `permission_request_to_menu` (`lib.rs:8540`) | ✓ PermissionRequest | ✓ fallback + signature-less shape classify (`grid_menu_is_bash_command_box` / `grid_menu_edit_write_tool`, `lib.rs:5311`, `:5352`) | Hook |
| Claude Family B (AskUserQuestion) | `guard.rs:527`; `askuserquestion_to_menu` (`lib.rs:8045`) | ✓ PreToolUse | ✓ fallback (title extract) | Hook |
| ExitPlanMode | `lib.rs:8552` carve-out | ✗ (denylisted) | ✓ **sole path** | **Grid** |
| Codex Shell / File-Patch / Generic | `guard.rs:533`; `codex_permission_request_to_menu` (`lib.rs:8419`) | ✓ PermissionRequest / PostToolUse (`guard.rs:534-535`) | ✓ fallback (`grid_menu_is_codex_permission_box`, `lib.rs:5384`) | Hook |

Codex fires **real** PermissionRequest/PostToolUse events — it is
hook-covered, not grid-dependent. The Codex grid path is defensive
fallback for hook timeout/absence, not the primary.

## Where the timing machinery lives, and for whom

Every duration-dependent piece — `fence_decision` (`lib.rs:4515`),
`pty_menu_suppressed_cell` (`lib.rs:10759`), the `pre-absence-redetect` /
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

- **ExitPlanMode** — never reaches the hook (carve-out, `lib.rs:8552`); grid
  is the sole detector, including its own dismiss handling.
- **Hook-slow / absent fallback window** — the ~1.5 s before the hook
  claims (the grid-snapshot apply in `pty_menu_update`), and the hook-timeout reclaim path.
- **Signature-less shape classification** — Edit/Write/Bash before the
  JSONL carries the tool_use signature (`lib.rs:5311`, `:5352`).

## Decision this yields

**For hook-covered menus, the hook's clear is authoritative: the grid must
defer to it and NOT run `fence_decision` at all.** The suppressor's timing
heuristics remain only for grid-only menus (ExitPlanMode) and the bounded
hook-absent window.

This is the causal, duration-free target: gate the grid re-surface on hook
ownership / hook-cleared state — which `retire_dismissed_menu_on_hook_claim`
(`lib.rs:10763`) already tracks — not on absence-fence timing. Nearly every
menu becomes hook-authoritative with no suppressor in the loop; the timing
machinery survives only where nothing else can clear the menu.

The code change (scope `fence_decision` to grid-only menus / make
hook-clear authoritative for hook-covered menus) is a **separate item**,
gated on this doc so the boundary is written down before the suppressor is
touched.
