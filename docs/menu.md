# Reading the agent's terminal

How Bram works out what the CLI agent (Claude Code or Codex) is doing from
outside it: whether a permission or choice menu is up and what its options
are, whether the agent is working, and when its turn is finished. This is
the entry point; it describes the mechanism as of 2026-10-03, with code
citations, and links to the deeper and historical docs in the
[reading map](#reading-map).

Line numbers are `src-tauri/src/lib.rs` unless another file is named. They
drift; the function names don't, so search by name when a line is off.

## Four sources, each authoritative for something

| Source | How Bram gets it | Authoritative for |
|---|---|---|
| **Provider hooks** | Claude/Codex hook events, relayed by the `bram-guard` binary to `/__menu/permission[/clear]` (`src-tauri/src/guard.rs:521`) | That a menu was posed, with its structured payload (tool, command, diff, suggestions) and, for some events, its `tool_use_id` |
| **xterm.js grid** | The terminal pane's rendered cells, read in the webview (`__gridReadLiveRows`, `app/main.js:1146`) and reported to the host (`report_grid_menu`, `:10575`; `report_grid_status`, `:10523`) | Which menu is *actually on screen* and its exact option text; the status row (verb, elapsed, substate) |
| **Raw PTY bytes** | The PTY reader thread (`pty_spawn`, `:16588`), which feeds every chunk to `pty_menu_update`, `pty_agent_turn_update`, `pty_agent_status_update` and `pty_output_clears_inflight` (`:17175-17178`) before xterm.js renders it | Activity timing (spinner glyphs), cancel markers, and a few prompts the other sources can't see (below) |
| **Provider transcripts (JSONL)** | `~/.claude/projects/<cwd>/*.jsonl`, `~/.codex/sessions`, keyed on the project path (`check_jsonl_for_turn_end`, `:52014`) | That a turn is truly finished; tool-call signatures that make a grid-seen menu actionable |

There is **no terminal emulator in Rust**. The only terminal crate is
`portable-pty` (`src-tauri/Cargo.toml`). The screen model is xterm.js in the
webview; the host keeps only a 64 KB byte tail and a hand-rolled
`strip_ansi` (`:12409`). Why the grid replaced parsing stripped bytes is
told in [`xterm-grid-screen-reading.md`](xterm-grid-screen-reading.md).

## Menus

### Hooks pose, the terminal arbitrates

1. **A hook posts a claim.** `bram-guard` relays Claude's
   `PermissionRequest` and `PreToolUse(AskUserQuestion)` to
   `/__menu/permission`, and `PostToolUse` / `PermissionDenied` to
   `/__menu/permission/clear` (`guard.rs:523-531`). Codex has
   `PermissionRequest` and `PostToolUse` only (`guard.rs:533-536`); a Codex
   denial is cleared by PTY cancel detection. The host
   (`handle_permission_menu`, `:8800`) builds a menu from the payload
   (`permission_request_to_menu` `:8540`, `askuserquestion_to_menu` `:8045`,
   `codex_permission_request_to_menu` `:8419`) and adds it to a **claim
   queue** keyed by `tool_use_id`, falling back to a signature. Claims
   expire after 300 s (`MENU_HOOK_CLAIM_TTL_MS`, `:1016`).
2. **The grid reports what's on screen.** On each xterm write-parsed frame,
   `__gridDetectMenu` (`app/main.js:1169`) scans the last 200 rows for runs
   of numbered options, bottom-up, tolerating stale cells (`2.Yes`) and
   rejoining wrapped labels. A run counts as a menu when:
   - its first option is `Yes`, or `Allow` with a rendered cursor
     (`:1269-1279`), **and** there is a header (`Do you want to|requires
     approval|Would you like to run|wants to`, `:1230`), a footer below
     (`Esc to cancel|Press enter to confirm`, `:1256`) or a Codex signal
     (`(y)`, `(p)`, `(esc)`, `tell Codex`, `Yes, proceed`, `:1258`); or
   - it is a **picker**: both `Enter to confirm` and `Esc to cancel` below
     the block plus a rendered cursor (`pickerSignal`, `:1288`).

   A menu-shaped block that fails the gate is logged as
   `iframe-trace subkind=xterm-grid-miss` with its rows (`:1511`). While a
   menu stays up the PTY goes quiet, so the grid re-reports every second
   to stay fresh (`:1611-1625`).
3. **The pane shows the claim that joins the screen.**
   `select_hook_claim_display` (`:1768`) picks, in order:
   - a queued claim whose option labels **join** the grid's current menu;
   - else a lone claim that survived the 50 ms coalescing window
     (`MENU_HOOK_CLAIM_COALESCE_MS`, `:1215`);
   - else **grid-rescue**: the grid's own options, shown directly, when the
     terminal shows a menu no claim matches (`:1850-1858`).

   On a join the grid's phrasing replaces the hook-synthesized labels,
   because the terminal text is what the keystroke actually grants. So a
   pane answer always keystrokes the prompt the terminal shows.
4. **Dismissal is outcome-driven.** A keyed hook clear removes its own
   claim; a transcript `tool_result` without a `PostToolUse` (a failed
   command) synthesizes one (`claim-queue-remove reason=jsonl-resolved`).
   When the grid stops seeing the menu, the **absence fence** decides
   whether that's a real dismissal or a mid-repaint frame
   (`fence_decision`, `:4515`); grid absence alone holds the menu rather
   than clearing it (`PTY_MENU_HELD`, `:11741-11746`).

While any menu is open, a **send-gate** holds pane sends so a typed message
can't land in the menu as an answer (`:37152`). The current menu is
`pendingMenu` on `/__turn-state` (`:64071`); `/__prompt-lifecycle`
(`:64590`) keeps the ledger of prompts shown and answered.

### `menus.parseAndDisplay` is off by default

`BRAM_MENUS_PARSE_ENABLED` defaults to off: "menu parsing is unreliable,
so it is opt-in" (`:862-865`). With it off, `pty_menu_update` returns
before any menu building (`:11082`). The grid still reports, and grid
menus reach the pane only through the claim queue's join and grid-rescue
above. Turning it on adds the older grid-build path in `pty_menu_update`
(signature-less Bash/Edit/Write builds, `grid_menu_is_bash_command_box`
`:5311`, `grid_menu_edit_write_tool` `:5352`). `menus.hookDriven`
defaults on (`:867-869`); setting it false is the kill switch that hands
menus back to the grid.

### Menus with no hook

- **ExitPlanMode.** The hook payload lacks its four options, so the host
  declines it (`:8552`) and the grid is the only detector.
- **Codex hooks-trust prompt.** Classified straight from raw bytes at boot,
  before the parse gate (`codex_trust_prompt_menu_from_chunk`,
  `:11025-11044`).
- **Pickers** (session resume and its structural twins), via the picker
  gate above. The Claude and Codex **model pickers** fail it (their
  footers differ) and go undetected; see
  [`pty-menu-shapes.md`](pty-menu-shapes.md) Family C.

## What still scans raw bytes

Byte-pattern menu detection is retired: `pty_menu_update` starts from
`None` and only the grid sets it (`:11084-11087`). Raw bytes are still
scanned for:

- **Spinner glyphs**, the U+27xx/U+28xx range, as turn activity
  (`pty_chunk_has_turn_activity_glyph`, `:9930`);
- **Codex `Working (… esc to interrupt`** (`pty_chunk_has_codex_working_status`,
  `:9939`);
- **The Codex "Action Required" title**, a heartbeat that keeps a Codex menu
  held (`pty_codex_action_required_pos`, `:12540`);
- **Cancel markers** that clear the busy state (`pty_output_clears_inflight`,
  `:12569`);
- **The Codex hooks-trust prompt** (above);
- **Agent boot evidence**, to release sends held for a fresh CLI
  (`agent_boot_evidence_scan`, `:37700`);
- **Terminal-modal mode bytes**, observe-only (`term_modal_scan`, `:37913`).

## Busy, idle and turn end

- **Working.** For Claude, the status row is driven entirely by the grid's
  read of the status line (`:9738-9745`). Spinner-glyph activity from raw
  bytes marks the turn as active.
- **Activity stopped.** 800 ms without spinner output is the eager edge
  (`AGENT_TURN_IDLE_THRESHOLD_MS`, `:9867`). The busy spinner clears only
  after 3 s (`MIN_SILENCE_FOR_SENTINEL_CLEAR_MS`, `:9874`) and the status row
  after 10 s, never while a menu is pending (`:9885-9901`).
- **Finished.** Final completion comes from the transcript: Claude's
  `stop_reason: "end_turn"`, Codex's `task_complete`
  (`codex_jsonl_completion_decision`, `:51831`; `check_jsonl_for_turn_end`,
  `:52014`). JSONL can lag, so the silence windows above only govern what
  is shown while waiting.

The full threshold table is at the end of
[`pty-menu-tunables.md`](pty-menu-tunables.md#current-thresholds-2026-10-03).

## Agents Bram didn't launch

Under `startupPolicy: "none"` the user runs their own command in Bram's
terminal; the motivating case is `herdr agent attach <pane>` (#389, #394).
Bram records which provider the user named (`7624541`), so transcript
turn-end detection works, and `bram-guard` still finds the host through
`resources/.bram-port`, so worklist denies work.

**Hook menus do not.** `pty_spawn` gives the agents it launches a
per-session `BRAM_MENU_TOKEN` (`:16650-16652`), and `/__menu/permission`
rejects POSTs without it as coming from a foreign agent (`:8812`;
`guard.rs:635-642`). An externally hosted agent never inherits it. So for
those agents the grid is Bram's only menu signal, and with
`parseAndDisplay` off it reaches the pane only through grid-rescue. What
the grid sees is also the host's re-rendering (herdr's) of the agent's
screen, which has not been tested against these gates.

## Known limits

The failure classes, with the history of each, are in
[`menu-interventions-catalog.md`](menu-interventions-catalog.md):

- **Prose looks like menus.** Agent text quoting menu phrases, or a stale
  numbered list above a real menu, can be mistaken for one; the gate's
  cursor and footer requirements exist for this.
- **Width.** Wrapped labels and command boxes corrupt extraction; out of
  scope by policy ([`pty-menu-shapes.md`](pty-menu-shapes.md), top).
- **Re-read or new?** Telling a stale re-read of a dismissed menu from a
  new one drove the fence, `tool_use` identity and the labels join.
- **Gaps in the providers.** `PermissionRequest` carries no `tool_use_id`;
  a command that runs and fails fires no `PostToolUse`; some prompts fire
  no `PermissionRequest`. Tracked in `upstream-asks.md`.
- **Timing.** A menu shown and dismissed between two grid reads leaves no
  trace. The grid report can arrive 14–80 ms before the hook claim
  (self-healing). A backgrounded webview stalls the grid read.
- **Version drift.** The shape catalog was last pinned to Claude Code
  2.1.179 and Codex 0.142.2 (2026-06-25).

## Open issues

- **#192**: provider-neutral deny-clear. Claude clears on
  `PermissionDenied`, Codex on PTY cancel; these could unify on PTY cancel.
- **Stranded hook-owned menu.** The release-only reclaim (`aefbe9d`) drops
  stale hook ownership once the grid keeps seeing a menu the hook cleared.
  But a hook clear doesn't clear `pty_menu_cell`, so when the grid cell
  already matches there is no transition and the menu stays stranded.
  Watch the `stranded-reclaim` trace: a line with no following
  `pty-menu-changed` surface is that residual case.
- **The ExitPlanMode prize.** A `PreToolUse` handler for ExitPlanMode, like
  the one for AskUserQuestion, would leave the grid as an oracle and kill
  switch only.

## Display

`app/tools/components/AgentMenuView.xmlui` renders the resolved menu on two
surfaces: inline in the Transcript and in the Worklist agent dock
(`surface` prop). The preview is open by default and single-scroller: the
inline body uses a compact `DiffView` and spills longer content to a
component-owned modal (`67bcc80`).

## How we got here

Four detection approaches were built in sequence, each subsuming the last:

1. **Transcript polling.** Too laggy, and blind to prompts that never land
   in the transcript. Survives as the signature oracle
   (`lookup_pending_tool_call`).
2. **Raw PTY byte scanning.** Flapped; the escape-sequence cleanup never
   ended. Its menu detection and forensic scaffolding were deleted
   (#214, 2026-07-06).
3. **The xterm.js grid.** Faithful to what is drawn; now the arbiter.
4. **Hooks.** Structured, and fire exactly when a menu is posed; primary.

Then the clocks and tuned windows were replaced by causal invariants
(absence fence, `tool_use` identity) and, last, the claim queue with
terminal arbitration. The catalog numbers these Gen 0–5.

## Reading map

| Doc | What it is | Status |
|---|---|---|
| [`xterm-grid-screen-reading.md`](xterm-grid-screen-reading.md) | Why bytes, grid and JSONL differ; why `strip_ansi` mangles | Current in substance; predates the claim queue |
| [`trace-vocabulary.md`](trace-vocabulary.md) (`hook-menu`, `pty-menu`, `send-gate`, `prompt-lifecycle` rows) | Every trace op the menu path emits | Current |
| [`menu-interventions-catalog.md`](menu-interventions-catalog.md) | Retrospective: generations, failure buckets, the ratchet argument | Historical, through 2026-07-18 |
| [`menu-detection-audit.md`](menu-detection-audit.md) | Hook vs grid coverage per menu family | Historical (2026-07-13), citations refreshed |
| [`pty-menu-shapes.md`](pty-menu-shapes.md) | The shape catalog: which menus must surface, by family | Current shapes; byte-era axis vocabulary |
| [`pty-menu-hook-catalog.md`](pty-menu-hook-catalog.md) | Hook payload shapes per tool | Current |
| [`pty-menu-specimens/`](pty-menu-specimens/README.md) | Raw observations, one per file | Current (human intake) |
| [`pty-menu-tunables.md`](pty-menu-tunables.md) | June 2026 byte-scanner knobs, plus the current threshold table | Historical, table current |
| [`pty-menu-tools.md`](pty-menu-tools.md) | Analysis scripts | Both scripts retired |
| [`claude-permissions.md`](claude-permissions.md), [`codex-permissions.md`](codex-permissions.md) | Provider prompt strings | Pinned to 2026-06 builds |
| [`terminal-attention-shapes.md`](terminal-attention-shapes.md) | Non-menu prompts that need the user | Separate topic |
| [`upstream-asks.md`](upstream-asks.md) | What we've asked the providers for | Current |
