# Menu capture and display

How Bram surfaces a CLI agent's permission / choice menus from the terminal
into the agent pane. Four detection generations were built in sequence. The
newest (hooks) is primary; the older three survive as fallback, oracle, and
forensic scaffolding. This doc maps what each left behind and what cleanup is
open — it is the starting point for that conversation, not an exhaustive tour.

## The four generations (brief)

1. **JSONL session parsing.** Read the agent's session transcript (`.jsonl`) to
   infer when a menu was up. Mostly accurate, but blind to prompts that never
   land in the transcript (e.g. AskUserQuestion), and too slow to feel live.

2. **PTY-stream scanning.** Scan the raw PTY byte stream for menu-shaped text and
   clean up the terminal-escape garbage. Survived a long time but was never
   satisfactory — the byte patterns flap and the cleanup is endless.

3. **xterm.js grid.** Reach into the rendered xterm.js grid (the actual on-screen
   cells) instead of raw bytes — "the grid." More faithful to what's drawn, but
   still missed menus (gutter collisions, signature gaps).

4. **Hook menus.** The agent's own permission hooks POST the structured menu to
   Bram's loopback (`/__menu/permission`), which builds it from data — no
   scraping, and it fires exactly when a menu appears. Where we should have
   started.

## What each layer left in the tree

| Gen | Still present (`src-tauri/src/lib.rs` unless noted) | Current role |
|---|---|---|
| 1 — JSONL | `lookup_pending_tool_call` (reads the `⏺ Tool(…)` signature from the session jsonl) | Attaches a tool signature to grid/PTY-detected menus; redundant once the hook supplies the tool directly. |
| 2 — PTY scan | `pty_menu_update` (detector); forensic scaffolding `pty_scan_anchor_ranges`, `pty_scan_raw_anchor_ranges`, `pty_menu_scan_excerpt`, `pty_menu_scan_diagnostic`, `pty_menu_anchor_pos` — **deleted tranche 2, #214** | Fallback + oracle. The forensic scaffolding was deleted after gate-evidence confirmed zero use across the heaviest menu-forensics week. |
| 3 — grid | `report_grid_menu` (iframe→host bridge), classifiers `grid_menu_is_bash_command_box` / `grid_menu_is_codex_permission_box` / `grid_menu_command_preview`; plus the iframe-side xterm scrape that feeds it | Fallback + oracle; the deferring half of the coordination (`op=grid-deferred` while the hook owns the slot). |
| 4 — hook | `handle_permission_menu`, `permission_request_to_menu`, `codex_permission_request_to_menu`, `askuserquestion_to_menu` + the hook scripts (`app/__shell/permission-menu-hook.py`, `app/shell/codex-permission-menu-hook.py`) | **Primary.** `menus.hookDriven` default-on. |

## Cleanup

The hook path is primary and now hardened — ownership gate (`4f4e725`),
sustained-presence (`0442e95`), foreign-POST token guard (`34f9ff4`),
stranded-menu reclaim (`aefbe9d`, traced via `545a90f`). Gens 1–3
are no longer authoritative; they're fallback + oracle. Staged disposition:

- **Now, low risk — DONE (delete-phase tranche 2, #214, 2026-07-06):** the
  Gen-2 forensic scaffolding (`pty_scan_anchor_ranges` et al., ~520 lines
  including the `traces.gridScanVerbose` gate) was deleted after a
  gate-evidence check: zero `[pty-menu-scan]` lines across the heaviest
  menu-forensics week the codebase has had — every diagnosis came from the
  grid-menu / pty-menu / fence / hook trace layers. Future menu forensics:
  `record-trace.sh` plus those surviving layers.
- **Post-burn-in:** delete/demote the Gen-3 authoritative detection for
  hook-covered tools (the Bash/Edit/Write classifiers); drop the Gen-2 PTY
  detector to oracle-only.
- **Gen-1 simplification:** once the PTY/grid paths are oracle-only,
  `lookup_pending_tool_call`'s signature cross-ref is largely redundant for hook
  menus (the hook already carries the tool).

### The irreducible core (keep)

- **ExitPlanMode.** The hook *declines* it (4 options, empty
  `permission_suggestions`), so the grid stays authoritative for it. This is the
  one thing still forcing a real grid.
- **The `menus.hookDriven=false` kill switch.** The fallback path is the safety
  net if the hook regresses.
- **A slim oracle.** The grid *reads* the rendered terminal (ground truth); the
  hook *predicts* it. A thin oracle catches prediction drift — it caught the
  "similar commands" label and the Edit/Write count mismatch.

### The prize

ExitPlanMode is the last thing requiring an authoritative grid. A PreToolUse hook
handler for it (we already built one for AskUserQuestion) would let the grid
shrink to a near-trivial oracle + kill switch, collapsing Gens 1–3 to a handful
of lines.

## Display (agent pane)

Detection is only half of it — `app/tools/components/AgentMenuView.xmlui`
renders the resolved menu on two surfaces: inline in the Transcript event
stream and in the Worklist agent dock (`surface` prop). The preview is open
by default and single-scroller: the inline body hugs its content via a
compact `DiffView` mode (no 60vh virtualized `List`, no nested scrollbar) and
spills longer diffs/content to a component-owned modal, so nothing
double-scrolls on either surface (`67bcc80`).

## Open menu issues

- **#192** — provider-neutral guard / deny-clear. The deny-clear is currently
  split (Claude `PermissionDenied` hook vs. Codex PTY-cancel) and could unify on
  the PTY-cancel path.
- **Stranded hook-owned menu (force-surface follow-up).** The release-only
  reclaim (`aefbe9d`) drops stale hook ownership once the grid keeps seeing a
  menu the hook cleared, so the grid can surface it on its next transition.
  But a hook clear doesn't clear `pty_menu_cell`, so when the grid cell
  already matches there is no transition and the menu stays stranded. Watch
  the `stranded-reclaim` trace: a line with no following `pty-menu-changed`
  surface is that residual case, and a forced re-emit is the fix.
