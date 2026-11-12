# Backend APIs

This document inventories Bram's route-like surfaces. The `__*` HTTP endpoints
are only one subset; the app also uses project-content routing, bundled asset
routes, Tauri IPC, filesystem coordination files, watcher events, and one
external local service.

## Key terms

**Route**
: A URL-addressable path such as `/__worklist` or `/__project/index.html`.
  Routes are convenient for browser navigation, XMLUI `DataSource` reads,
  image/file previews, and compatibility calls from tools such as `curl`.

**Loopback**
: A real TCP network call from this machine back to this machine, usually to
  `127.0.0.1` or `localhost`. In Bram, loopback usually means an agent or helper
  calls the tiny HTTP server at `http://127.0.0.1:<bram-port>/...`. A URL that
  merely contains the word `localhost`, such as `tauri://localhost/...`, is not
  necessarily loopback.

**Tauri scheme**
: Browser URL handling inside the WebView, using `tauri://localhost/...` (or
  `http://tauri.localhost/...` on some platforms). It looks URL-like and can
  serve the same internal route shapes, but it is handled inside Tauri rather
  than by opening a TCP connection to `127.0.0.1`.

**Tauri IPC**
: Direct WebView-to-host commands and events through `window.__TAURI__`, such as
  `invoke(...)` calls and events like `worklist-changed`. IPC is not URL routing
  and is not loopback.

**Filesystem coordination**
: Communication through watched files under `resources/`, such as
  `.worklist-intent.json`, `.worklist-result.json`, `.worklist-authorization.json`,
  `.inflight-claim.json`, `worklist.json`, and history files. This is not
  loopback; it is file I/O plus host watcher dispatch.

Bram hosts two iframes inside the parent shell: the **target app** (the project
under development; any web app served by the project's HTTP server) and the
**agent pane** (Bram's own agent pane at `app/tools/`,
providing Worklist, Commits, Issues, Queue, Sessions, History, Context, Status,
and Settings tabs).
These surfaces use several transports:

- **Tauri scheme routing** loads shell pages, project content, bundled assets,
  and internal route-shaped APIs for the browser-facing iframes.
- **Internal HTTP-style routes** are request/response handlers in
  `src-tauri/src/lib.rs`. The browser usually reaches them through the Tauri
  scheme; agents historically reached some of them through loopback `curl`.
- **Loopback HTTP** is the real `127.0.0.1:<bram-port>` tiny HTTP server path.
  It remains useful as a compatibility path, but Codex cannot reliably use it
  under the current sandbox.
- **Tauri IPC commands and events** carry direct WebView-to-host commands and
  invalidation signals. `app/__shell/helpers.js:getTauriInvoke` formalizes a
  `window.__TAURI__` -> `window.parent.__TAURI__` -> `window.top.__TAURI__`
  fallback chain.
- **Filesystem coordination** is now the preferred direction for agent
  lifecycle work: intent/result files, authorization records, inflight claims,
  worklist state, and history.
- **External local services** are separate loopback services outside Bram's
  route handler, currently represented by the Whisper voice endpoint at
  `http://127.0.0.1:18080`.

The target app iframe URL is provisioned via the IPC command
`get_right_pane_url`, which returns the `tauri://localhost/__project/...`
form. The scheme handler routes `/__project/*` to the project's HTTP server or
serves files directly. The loopback port is not exposed to the iframes; they see
the `tauri://` origin. The agent itself cannot call IPC because it has no Tauri
runtime.

When a route or command is added or removed, update this catalog. Code is
the source of truth; this is the announcement surface.

## Sections

| # | Section | What it covers | Primary consumers |
| --- | --- | --- | --- |
| 1 | [App & shell meta](#1-app--shell-meta) | Version banner, target app info, restart, error reporting, PTY views and control | parent shell, both iframes |
| 2 | [Setup (agent coordination)](#2-setup-agent-coordination) | Per-repo installer of the shared worklist core + per-agent adapters | agent pane iframe |
| 3 | [Worklist & authorization](#3-worklist--authorization) | Pending items + verified `approved:` / `drop:` records | agent pane iframe, agent (curl) |
| 4 | [Worklist history](#4-worklist-history) | Reverse-chronological archive of worklist transitions | agent pane iframe |
| 5 | [Sessions](#5-sessions) | Claude / Codex JSONL session enumeration, content, search | agent pane iframe |
| 6 | [Git & repo](#6-git--repo) | Commits, diffs, file reads, origin, push | agent pane iframe, parent shell |
| 7 | [Issues](#7-issues) | GitHub passthrough via `gh` | agent pane iframe |
| 8 | [Context](#8-context) | `CLAUDE.md` / `AGENTS.md` import chain + memory + hooks + settings | agent pane iframe |
| 9 | [Voice / transcription](#9-voice--transcription) | Whisper subprocess lifecycle | parent shell |
| 10 | [Static & hot-reload](#10-static--hot-reload) | Files served from disk or embedded; iframe reload coupling | both iframes |
| 11 | [Inflight sentinel](#11-inflight-sentinel) | Host-managed claim file driving the Worklist tab's spinner state | agent pane iframe, agent (curl) |
| 12 | [Agent message queue](#12-agent-message-queue) | Durable, manually sent messages parked while an agent works | agent pane iframe |
| 13 | [Prompt lifecycle](#13-prompt-lifecycle) | Locally implemented prompt shown/resolved ledger; hook and plugin callbacks | agent pane iframe |
| 14 | [Search index](#14-search-index) | Full-text search over sessions, commits, issues, worklist history | agent pane iframe, agent (curl) |
| 15 | [Needs You inbox](#15-needs-you-inbox) | The Inbox tab's lanes and dismissals | agent pane iframe |

## 1. App & shell meta

App-wide version, screen, and process information; PTY echo views and
write/resize control. The parent shell uses these to render the
update-available banner and to drive the terminal; the agent pane iframe
uses them for the target app info dialog.

**A held start (0.8.7, judell/bram#469).** When Bram's files are still in `resources/` and the startup move couldn't run, the start is held: `/__enhance/status` reports `projectFolder.kind: "state-held"` with `obstacle`, `retry` and `trackedFiles`. Every state-changing request (any POST, plus GET `__worklist/resolve`, `__worklist/init`, `__enhance/run`, `__restart-server`, `__issue/comment`) answers 409 `{ ok: false, held: <obstacle> }` and logs `[state-hold] op=refuse-write` to stderr (`held_write_refusal`). Allowed through: `__state-dir/migrate-relaunch` and `__state-dir/decision` (the way out), the self-update routes, `__trace/pane`, `__pane/flush` and `__startup-ready` (`HELD_ALLOWED_PATHS`). Under a hold, migrate-relaunch is the banner's Retry: it relaunches without an offer, and records `move` for a mixed folder.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__app-info` | HTTP GET | — | `{ current, latest, has_update, release_url, forge, embeddedAppHash, servingEmbedded, project?, projectKey?, repo?, targetAppUrl }`. `embeddedAppHash` / `servingEmbedded` say which `app/` tree this process serves; `project` / `projectKey` carry the window's project identity; `targetAppUrl` is where a person opens the project's app, or `null`. | parent shell, agent pane iframe, agents (version for forge signatures) |
| `/__right-pane-info` | HTTP GET | — | `{ url, default_right_pane, spawned?, browserUrl, hasTargetApp }`. `browserUrl` is the browser-safe address of the target app (or `null`); `hasTargetApp` says whether the project has one. | agent pane iframe |
| `/__restart-server` | HTTP GET | — | empty / 200 on success | agent pane iframe |
| `/__startup-ready` | HTTP GET | `event=<event-name>` (required; 400 without) | `{ ok, exists, name, replayed, … }` — asks the host to re-emit a named Tauri event for a pane that attached late (`talk-session-changed` re-emits live and answers `seeded: true`). Sibling of `/__event/latest`, which only reads the remembered payload. | agent pane iframe |
| `/__agent-status` | HTTP GET | — | the live agent status row `{ provider, state, verb?, elapsedText?, substate?, … }` parsed from the terminal's status line; a stale Codex `working` reads as `finished`. | agent pane iframe |
| `/__turn-state` | HTTP GET | — | the host's turn-phase record `{ provider, phase, turnStamp?, lastPtyActivityAtMs, lastJsonlActivityAtMs, lastNonFinalAssistantAtMs, lastCompletionAtMs, … }` that drives turn-end detection. | agent pane iframe, diagnostics |
| `/__guard/selftest` | HTTP GET | — | `{ ok, failed: [...], cases: [{ id, pass, got, reason }], claude, codex, opencode }` — runs the installed guard chain (hook scripts and plugin) against a throwaway fixture project and reports allow/deny per case (issue-415). Read-only against the real project. | agent pane iframe (Status tab), diagnostics |
| `/__self-update/status` | HTTP GET | — | `{ state: idle\|running\|done\|failed, phase, tail: [...], error?, installed_to?, target_version?, started_at_ms, finished_at_ms? }` | parent shell (update banner) |
| `/__self-update` | HTTP POST | — (no body) | `{ ok: true, state: "running" }`; 409 while an update runs or when no release info is known; 405 on other methods. Starts the install flow: the host fetches the release's installer script and runs it on a background thread; progress rides `/__self-update/status` and the `self-update-changed` event. **Authorization:** none beyond the common loopback gate; it needs no grant and ignores the body and query, so a caller cannot choose the URL or version. The script comes from `BRAM_BASE_URL` / `XMLUI_DESKTOP_BASE_URL` when Bram's own environment sets one, else `https://github.com/judell/bram/releases/download/<latest tag>/install.sh` (`self_update_base_url`, `run_self_update`). The host does not verify the script; `install.sh` checks the artifact against a `SHA256SUMS` fetched from the same base, which catches corruption, not a compromised release. The 409 guard is only a double-start check on the in-process status (`state == "running"`), not a caller check. See [authorization.md](authorization.md#action-matrix) and its known gaps. | parent shell (update banner) |
| `/__self-update/relaunch` | HTTP POST | — (no body) | `{ ok: true }` then the process exits; 409 `no relaunch target` if the installed binary cannot be found. Spawns the freshly installed binary against the current project root, detached, and quits this instance. **Authorization:** none beyond the common loopback gate, and no 'update finished' precondition: it relaunches `installed_to` if one was recorded, else the running executable (`handle_self_update_relaunch`). The caller supplies nothing, so it picks no target. It ends the running instance, so an unwanted call is a disruption. | parent shell (update banner) |
| `/__state-dir/migrate-relaunch` | HTTP POST | — (no body) | `{ ok: true, relaunching: true, choiceRecorded }` then the process exits; 409 `{ ok: false, obstacle }` when `retry.offer` is false (nothing is written, nothing relaunches); 500 if the `move` answer can't be written (no relaunch). Status's **Migrate and relaunch** (issue-469-leftover-folder-reported). Recomputes `retry` (below); when `needsChoice`, records `move` in `resources/.state-move` first, then spawns the running executable against the project root, detached, and quits, so the next startup's mover does the move. **Authorization:** none beyond the common loopback gate; it picks no target (the running exe, never the self-update path) and the move itself still checks tracked files, live claims and the inventory at startup (`handle_state_migrate_relaunch`). It ends the running instance, so an unwanted call is a disruption. Emits `[state-dir] op=migrate-relaunch`. | agent pane iframe (Status tab, State folder section) |
| `/__trace-archive-status` | HTTP GET | — | the last trace-archival pass: `{ state, completedAtMs?, archiveAfterDays, candidateFiles, archivedFiles, rawBytes, compressedBytes, redactions, failures, skippedExisting, elapsedMs }` (see Trace persistence in section 5). | agent pane iframe (Status tab) |
| `/__pty-intent` | HTTP GET | — | `{ queue: [<parsed intent line>, …] }` — read-only view of the right-pane intent queue file; `{ queue: [] }` when empty or missing (#86). | diagnostics |
| `/__trace/pane` | HTTP POST | JSON trace payload (the same shape as `log_from_right_pane`) | `{ ok: true }`; 400 on bad JSON. Fetch fallback for the pane's trace transport when the Tauri invoke bridge is unavailable (#343); same sink and redaction as the IPC command. | agent pane iframe (`helpers.js`) |
| `/__pane/flush` | HTTP POST | none | `{ ok: true }`, or 409 `{ ok: false, error: "ui.paneIdleFlush is off" }`. Requests a forced reload of the agent pane through `emit_or_defer_tools_pane_reload`, so a turn in flight holds it until the claim clears; traces `[tools-pane-reload] op=requested source=pane-flush`. Called by the pane's own idle flush (`__bramPaneFlushTick`) only when it has degraded and every guard is clear. A workaround for judell/bram#482. pane-idle-flush-when-degraded. | agent pane iframe |
| `/__paste-image` | HTTP POST | `type=<image/png\|jpeg\|gif\|webp>`; body is the raw image bytes | `{ ok: true, path }` — the image is staged under `~/.cache/bram/paste/` so the message text can reference it as `[Image: source: <path>]`; 400 on an empty body or unsupported type. **Authorization:** none beyond the common loopback gate. The caller picks neither directory nor file name: the directory is fixed, the name is `<unix-ms>-<hash>.<ext>`, and `ext` comes from a four-entry allowlist keyed on `type`. Contents are not checked to be an image, and there is no size cap or cleanup in the handler (`handle_paste_image`), so repeated calls can fill disk. | agent pane iframe (composer paste) |
| `/__test/switch`, `/__test/reload`, `/__test/new-session`, `/__test/turn` | HTTP POST | JSON body per route (`provider`, `id`, `title`, …) | the result of the same command function the pane's IPC calls. **404 unless Bram was started with `BRAM_TEST_DRIVE=1`** (never set by `./bram`); 405 on non-POST. For `scripts/switch-matrix.py` on a disposable demo instance. See the Test-drive row in [authorization.md](authorization.md#action-matrix). | `scripts/switch-matrix.py` |
| `/__error` | HTTP GET | — | reported error context | agent pane iframe |
| `/__pty-tail` | HTTP GET | `lines=` | last N lines of PTY output, `text/plain` | agent pane iframe |
| `/__pty-stripped` | HTTP GET | — | PTY output with ANSI escapes removed, `text/plain` | agent pane iframe |
| `/__pty-menu` | HTTP GET | — | current permission menu (if any), JSON | agent pane iframe |
| `/__settings` | HTTP GET | — | user-facing slice of `.bram.json`: `{ shell: { agent }, worklist: { batchCommitActions }, ui: { targetAppMinimized } }` (always populated, defaults filled) | agent pane iframe (Settings tab), parent shell |
| `/__settings` | HTTP POST | body matches GET shape | merged response after writing `.bram.json` atomically; preserves unknown top-level keys; 400 on JSON parse error / 500 on IO error | agent pane iframe (Settings tab) |
| `settings-changed` | Tauri event | — | same payload `GET /__settings` returns | agent pane iframe (Settings DataSource refetch), parent shell (`app/main.js` `targetAppMinimized` driver) |
| `/__story/lines` | HTTP GET | `since` (ISO-8601 UTC; lines at or after it); `kinds` (optional, comma-separated Log kinds: `gate`, `push`, `search`, `queue-add`, `queue-delete`, `selection`, `turn`, `stall`, `refusal`, `launch`; returns only the lines those kinds are told from, plus session sets; an unknown kind gets every line; omitted, every selected line) | `{ lines: [{ ts, src: "trace"\|"hook", cat, text }], queueDrafts: { [queueId]: entry } }`; drafts are resolved only for queue IDs in the returned trace window, from retained `queue-drafts/<id>.json` or the active queue (legacy fallback). Lines sorted by `ts`: the trace lines that mark episodes (`turn-state` transitions, `turn-context`, `inflight-sentinel`, `auth-record`, `turn-send`, `worklist-commit`, `queue`, `bram-trace`, `git-push`, `demo-instance`, `agent-switch` sets only, and iframe `click` / `queue` / `long-task` / `input-latency` / `heartbeat-drift` / `w2-selection-restore` / `search-query`) plus `hook-events.log` denies. Reads across trace rotation (log-reads-across-trace-rotation): the live `bram-trace.log` and `hook-events.log`, plus every plain `bram-trace-*.log` and `hook-events-*.log` archive modified at or after `since` (gzipped archives are past `archiveAfterDays` and out of any window). The host keeps what it selected in memory: archives are read once, the live files only from the offset last read. The last session set before `since` leads the lines, so the window's first entries can be named. Emits `[story] op=lines` (`kinds`, `selected`, `lines`, `bytes`, `read_bytes`, `files`, `archives_read`, `ms`). The host also emits the signal `story-changed`, debounced 1.5 s, when the trace gains a line that can end a shown kind's episode (`worklist-commit op=committed`, `git-push op=push-result`, iframe `search-query` / `queue` / `w2-selection-restore`, a gate-bar `click`); the Log refetches on it. Since log-shows-start-commit-push-search the body also carries `currentSession` (`{ provider, sessionId, title }` or null), `sessionTitles` (`{ [sessionId]: title }` for the sessions the window's set lines name) `commitUrlBase` (the repo's web address plus `/commit/`, or null without a remote) and `unpushedShas` (full shas on HEAD that no remote has, `git rev-list HEAD --not --remotes`, at most 500), for the entries' expanded detail and the commit links: a pushed sha links to the forge, an unpushed one to `/commits?sha=` (the Commits tab's local view), and a push's range to the forge's compare view. Read-only. The pane tells them (`__bramStoryTell` in `helpers.js`); issue-473-readable-launch-log. | agent pane iframe (the Log tab, under More) |
| `/__tasks/candidates` | HTTP GET | `all=1` (optional; also returns the excluded steps, top 200) | judell/bram#483, issue-483-task-candidates-report: read-only measurement of what a Tasks feature would have saved. Walks every provider's sessions for the project (Claude and Codex by root; OpenCode and Antigravity through their discoverers), takes each shell tool call, splits compound commands on `&&`, `;` and newlines (quote-aware; a pipeline is one step), strips `env`/`VAR=`/`sudo`/`time` wrappers, judges a wrapped command by what it wraps (issue-483-tasks-unwrap-shell-wrappers: the quoted script of `powershell`/`pwsh -Command`/`-c`, the rest after `cmd /c`, the command after `call` and after GNU `timeout <N>`, each split into steps again; `call` of the VS environment script and Windows' `timeout /t` are `shell`), normalizes paths, and clusters the rest by recipe key (a script path when the step runs one; tool + first argument for subcommand tools such as `cargo`, `npm`, `make`; `python3 -m <module>`; otherwise the tool's first word). `{ root, sessions, commands, steps, excluded: { bramProtocol, forge, git, shell, notACommand }, runner: { files, targets: [{ name, file, body, runs, sessions }] }, candidates: [{ key, runs, sessions, agents, firstSeenMs, lastSeenMs, spellings: [{ text, runs, name, now }], savedAs }], excludedSteps?, ms }`. `name` is a kebab-case suggestion for the spelling as a saved command (`ffmpeg-ebur128`, `curl-record-status`, `demo-instance-launch`), from the cluster key plus up to two distinctive tokens of the command (a script's first argument, a filter word inside `-af`/`-vf`, a URL's last path words), deduped within the cluster. A placeholder: the pane shows it read-only and says so; the agent proposes the real name, from session context, when a task is saved. `excluded` counts the steps set aside before clustering: `bramProtocol` (loopback `curl` to `/__…`, any state-folder path, body files built for them), `forge` (`gh`, `glab`), `git` (all of it), `shell` (reads, edits, process checks, plumbing, inline `-c`/`-e` scripts, local probes, scratchpad helpers). `notACommand` (issue-483-tasks-runnable-first-word): a step the lists let through whose first word isn't something you can run (a file in the project, or a program on PATH spelled in the case typed, PATHEXT on Windows) or that is data (`name = …`): lines of a multi-line PowerShell script, text split out of a message, a shell function defined in-session. Clusters need 3 runs; spellings are the top 8 by count. `candidates` are ranked by `sessions`, then by number of spellings, then by `runs` (breadth before volume: the command typed many ways in many sessions is what a saved command fixes; a one-session loop isn't). A command or interpreter script under a temp or scratch path (`/private/tmp/…`, `/tmp/…`, `…/scratchpad/…`, `$S/…`) is `shell`. `runner` reports which of `Makefile`, `justfile`, `dodo.py`, `Taskfile.yml`, `package.json` (scripts) exist (`files`) and their targets (parsed, never executed). issue-483-save-as-task: for the root `Makefile` only, each target carries `body` (its recipe lines, tab and `@`/`-`/`+` prefixes removed) and `runs`/`sessions`, counted over the same session walk from steps whose head is `make <name>` (also `make -C . <name>`, `make <name> VAR=value`; a bare `make` counts for the first target, the default goal; `-C`/`-f` naming another directory or file is not counted). Targets of the other files have `file` set, an empty `body`, and null `runs`/`sessions` (not counted, as opposed to never run). Candidates are matched to the Makefile through the comment block a save writes above its target: a ``# Saved by Bram from the agents' `<key>` runs. Replaces:`` header, then `#   <spelling>  ->  make <name> [VAR=value]` lines (two or more spaces each side of `->`) and `# Not a task: <spelling> (<why>)` lines; other comment lines are ignored. A spelling whose text (normalized the same way as `spellings[].text`) appears on a `->` line gets `now` = that invocation; on a `Not a task` line, `now` = `"not a task"`; otherwise `now` is null. `savedAs` lists the defined targets a candidate is saved as: those its spellings map to, those any block whose header key equals the candidate key maps to, and `<name>` itself for a `make <name>` candidate; when that finds none, every target whose recipe has a step (with `$(…)`/`${…}` removed) whose recipe key equals the candidate key (the fallback for a Makefile without Bram comments). Empty when unsaved. Emits `[tasks] op=candidates sessions= commands= clusters= excluded= targets= saved= ms=` (`targets` counts every runner target, `saved` the candidates with a non-empty `savedAs`). The same JSON without a window: `bram --task-candidates <root>` (Claude and Codex sessions only; no trace line). | agent pane iframe (the Tasks tab, under More); the CLI flag |
| `/__token-accounting` | HTTP GET | `since=<ISO>` (calls at or after), `session=<provider>:<id>` (one session and its subagents), `calls=1` (adds per-call rows) | judell/bram#486, issue-486-token-accounting-instrument: per-call token readings from the agents' own transcripts, joined to the claim ledger. Reads Claude (`<id>.jsonl` as `thread: main`, `<id>/subagents/agent-*.jsonl` as `thread: subagent` with `agentId` from the file name), Codex rollouts (a subagent rollout, `thread_source: subagent`, is `thread: subagent` under its `parent_thread_id`; its records before `subagent_history_start_ordinal`, which counts the rollout's own top-level `ordinal` field, are the parent's copied history and are not counted) and OpenCode (`session_message` rows of type `assistant`; child sessions as subagents). Antigravity is `unavailable` with a reason. A streamed Claude message repeats its usage under one `message.id`: one call, the last record's numbers; `<synthetic>` and zero-usage events are not calls; a Codex `token_count` repeating the previous running total is skipped. A line that fails to parse is counted in the session's `unreadable`, not dropped. **Context** is what the model was shown: Claude `input + cacheRead + cacheCreation` (its `input_tokens` excludes cache); Codex `input_tokens` (it already includes `cached_input_tokens`; the reported `input` is the remainder); OpenCode `input + cache.read + cache.write` (its `input` excludes cache; observed input 4 beside cache.read 30633). Uncached tokens per call are `context - cacheRead` for every provider. `{ root, providers: { claude, codex, opencode: { status: "read", sessions, calls }, antigravity: { status: "unavailable", reason } }, attribution: { source: "state-mirror"|"unavailable", windows }, sessions: [{ provider, id, thread, agentId?, label?, models, calls, context: { total, peak, last, first }, input, cacheRead, cacheCreation, output, firstMs, lastMs, items: [{ id, calls, context, output, peakContext }], chat: { calls, context, output }, quota?, unreadable, compactions }], constrained: { byModel: [{ provider, model, calls, sessions, context: { p50, p90, p99, peak }, fits, sessionsFit, floor: { p50, min, max, sessions }, uncachedPerCall: { p50, p90 }, cacheHitShare, gapMs: { p50 }, outputPerCall: { p50 } }], instructionBytes: [{ path, bytes }] }, files: [{ path, reads, bytes, viaShellBytes, sizeBytes (size on disk now, null if the file is gone), linesRead?, testLines?, testBytes? }], filesTotal, libRsNow?: { lines, testLines, testSpans }, calls?: [{ provider, session, thread, agentId?, model, atMs, context, input, cacheRead, cacheCreation, output, reasoningOutput?, attribution: { items: [ids] }|"chat" }], ms }`. `constrained` covers main-thread calls only, per provider and model: percentiles are nearest-rank (the value at rank ceil(p/100 * n) of the ascending list); `fits` is the share of calls, and `sessionsFit` the share of sessions whose peak, that stayed within 32768, 65536, 131072, 200000 and 262144 tokens (keys `32k` to `256k`); `floor` is the context of each session's first main-thread call (what Bram's instructions cost before any work), counted under the model of that first call, beside `instructionBytes` (byte sizes of `CLAUDE.md`, `AGENTS.md` and the files they `@`-import, resolved from the root; bytes, never estimated tokens); `gapMs` is the median gap between consecutive calls within a turn (a gap over two minutes starts a new turn). **Attribution**: a call inside a claim window (`claims_overlapping` in the state-mirror db) with non-empty ids is `items`, split evenly when several ids share the window (an item's `calls` is then fractional); anything else is `chat`. Limits: the ledger is per machine and per project, so sessions made elsewhere, or before the db begins (this machine's begins 2026-11-07), read as all chat; the db's claim `ids` shrink as items resolve, so a window names the items still claimed at its end; work after a claim retires falls to chat. With no readable ledger `attribution.source` is `unavailable` and everything is chat. **Bytes read into context** (`files`): a Claude `Read` result is credited to its `file_path`; a content-mode `Grep` result is split on its `path:line:` prefixes (a files-only result credits its path lines); shell reads (`Bash` commands using `sed`, `cat`, `head`, `tail`, `grep`, `rg`, `nl`, `awk`, `less` or `bat` on a named file, and every Codex `exec`/`exec_command`/`shell` call that names a file) go to `viaShellBytes`, never mixed into `bytes`; a command naming several files splits its output evenly between them, which is approximate, and a bare file name (`lib.rs` after a `cd`) is its own row. For `src-tauri/src/lib.rs`, lines of a Read or content Grep that fall inside a `#[cfg(test)] mod` span (brace-counted from `git show <sha>:src-tauri/src/lib.rs` at the last commit at or before the call, cached per commit; uncommitted edits are not seen) are `testLines`/`testBytes` beside `linesRead`; `libRsNow` is the same count on the working tree. No provider reports tokens per tool result, so these are bytes. `files` is capped at 300 rows by bytes (`filesTotal` is the full count). Parsed files are cached in memory by (path, mtime, size). Emits `[token-accounting] op=report sessions= calls= files= ms=`. The same JSON without a window: `bram --token-accounting <root> [--since <ISO>] [--calls]` (reads the claim ledger read-only; no trace line). | the CLI flag; no pane surface yet |
| `/__coordination-status` | HTTP GET | — | derived status payload for the Status tab: `{ generatedAt, sections: [{ title, rows: [{ signal, level, state, detail, seen }] }] }`. Aggregates worklist counts by status, inflight sentinel state, turn-completion monitor state, authorization record freshness, watcher / hook health, recent trace tails. | agent pane iframe (Status tab) |
| `/__state-dir/plan` | HTTP GET | — | judell/bram#469 dry run, read-only (nothing moves in 0.8.3): `{ stateDir, stateDirExists, wouldMove: [{ path, kind, isDir }], foreign: [{ name, isDir }], tracked: { count, paths, truncated }, liveClaim, liveClaimDetail: { claimIds, authorization }, nested: [{ path, stateDir }], note, startupMove, retry }`. `startupMove` is the structured outcome of this launch's startup mover (`{ outcome: skipped\|moved\|blocked\|abandoned\|moved-selective\|selective-failed, reason, osError, attempts, restored, rolledBack, entry, moved, entries, atMs }`, null where not applicable; the whole value is null for the CLI). `retry` is `{ leftover, offer, needsChoice, obstacle }`: `leftover` is true when the folder still resolves to `resources/` and holds Bram-owned entries; `offer` is true when a relaunch can finish the move (`needsChoice`: the `move` answer for a mixed folder is written first); otherwise `obstacle` is `live-claim`, `tracked-unknown`, `tracked-bram-files` (the selective move refuses Bram-owned tracked files), `staging-left` (`.bram-moving/` exists) or `rollback-failed`. `wouldMove` is the machine-state and shown-only `BRAM_OWNED_ENTRIES` present at the state folder's top level (`kind` = `file` \| `dir` \| `dotfile`, the last for dotfiles only the dotfiles rule covers); `foreign` is every other top-level entry there; `tracked` is `git ls-files -- <stateDir>` (list capped at 200; empty outside a repo); `liveClaim` is a non-empty inflight claim or an unexpired, non-`none` authorization record; `nested` is subfolders to depth 3 (skipping `.git`, `node_modules`, `target`, the state folder) holding their own `.bram-port` or `worklist.json`, which migrate themselves. Emits `[state-dir] op=plan` (with `leftover`, `retry_offer`, `obstacle`). The same JSON without a window: `bram --state-dir-plan <root>` (no trace line). | agent pane iframe (Status tab, State folder section); the CLI flag |
| `/__event/latest` | HTTP GET | `name=<event-name>` | `{ exists, payload, tsMs }` — the most recently remembered Tauri event of the given name; subscriber replay for late-attaching listeners. | agent pane iframe, diagnostics |
| `pty_spawn` | IPC | `{ shell, cwd, env, agentAutostart? }` | `Result<(), String>` | parent shell |
| `pty_write` | IPC | `{ data: String }` | `Result<(), String>` | parent shell, iframe helpers (direct) |
| `pty_resize` | IPC | `{ cols, rows }` | `Result<(), String>` | parent shell |
| `open_devtools` | IPC | — | `()` (debug builds only) | parent shell |
| `open_url` | IPC | `{ url }` | `Result<(), String>` | iframe helpers (direct) |
| `save_trace_export` | IPC | `{ json }` | `Result<String, String>` (path) | iframe helpers (direct) |
| `capture_screenshot` | IPC | — | `Result<String, String>` (path) | iframe helpers (direct) |
| `get_right_pane_url` | IPC | — | `String` | parent shell |
| `get_tools_pane_url` | IPC | — | `String` | parent shell |
| `log_from_right_pane` | IPC | `{ payload }` | `()` | parent shell, iframe helpers (direct) |

`pty_write` runs every byte through `record_worklist_authorization_from_input`,
which detects `approved:` / `drop:` prefixes and writes the verified
authorization record to `resources/.worklist-authorization.json`.

## 2. Setup (agent coordination)

The per-repo installer that lays down the shared worklist-enforcement core
plus per-agent adapters (Claude `CLAUDE.md` @-import + `.claude/hooks/`,
Codex `AGENTS.md` block + `~/.codex/config.toml` PreToolUse hook). Skipped
when running in the Bram source repo itself (detected via
`ENHANCE_SOURCE_BUNDLE_REL`).

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__enhance/status` | HTTP GET | — | `{ enhanced, claudeMd, sidecarExists, hookScriptExists, hookRegistered, … }` | agent pane iframe |
| `/__enhance/run` | HTTP GET | — | `{ enhanced: true, wrote: [<path>, …] }` | agent pane iframe |

## 3. Worklist & authorization

For policy, enforcement limits, and decision history, see
[authorization.md](authorization.md). This section describes the wire and
handler behavior. The pane records gate actions through Tauri
`record_worklist_action_authorization` before delivering an agent turn;
turn text alone does not create that grant.

Served item data can include a `hash`, but approval no longer compares
content hashes or produces `rejected_stale` for a mismatch. Worklist
`version` checks guard file-write races, not human consent.

| Surface | Kind | Query / params | Response / behavior | Consumer |
| --- | --- | --- | --- | --- |
| `/__worklist` | HTTP GET | — | `{ description, version, items: [{ id, status, file(s), before, after, hash, diff?, ... }], exists, resourcesExists, path, ... }`; augmented board state | agent pane / agent |
| `/__worklist/init` | HTTP GET | — | board payload; creates the file if missing | agent pane |
| `/__worklist/resolve` | HTTP GET | optional `ids=foo,bar` | recorded `{ kind, ids, items, issuedAtMs, source, consumedAtMs, interruptedAtMs, commitToo }`, or `{ kind: "no_active_authorization", consumedAtMs }` | agent |
| `/__worklist/mutate` | HTTP POST | `{ op: "prune" \| "advance", ids: [...], status?: "applied" }` | `{ ok: true, pruned: [...] }` / `{ ok: true, advanced: [...] }`, or refusal | agent |
| `/__worklist/commit` | HTTP POST | `{ ids: [...], message: "..." }` | `{ ok: true, sha, queuedCloses: [n, ...], ... }`; may include `retained` / `residualPaths`; commits and performs its own prune | agent |
| `/__worklist/drop` | HTTP POST | `{ ids: [...] }` | `{ ok: true, pruned: [...] }`; records its own drop grant, resolves, prunes | feedback-less pane Drop; handler does not establish caller identity |
| `/__worklist/clear-landed` | HTTP POST | `{ ids: [...], via?: "pane" \| "agent" }` | `{ ok: true, cleared: [{id, sha}], refused: [...] }` when at least one clears; refuses if none qualify | pane / agent; Git evidence replaces a Drop grant |
| `/__worklist/reanchor` | HTTP POST | `{ ids: [...] }` | `{ ok, head, reanchored: [{id, path}], refused: [{id, path, reason}] }`; 409 when nothing was re-anchored (every refusal named: `already-applies`, `not-begun`, `unknown-item`, `no-failing-path`, `other-owners:<ids>`, `no-window-patch`, `not-only-this-items-work`, `untracked`), 503 while membership has no partition. For a begun item whose interval patch no longer applies to HEAD (a commit outside the gate changed its file after it began; the row's `commitStrandedBy` names it), records under `reanchors` in the claim-intervals sidecar that, at this HEAD and file content, `git diff HEAD -- path` is the item's patch — allowed only when the item's own window patch, applied to HEAD's copy of the file (with normal, then one-line, then no context), reproduces the worktree file byte for byte, which proves the worktree differs from HEAD on that path by the item's work alone. Membership is consulted only to name another begun owner (`other-owners`), since its removed-line attribution is unreliable in exactly the stranded case. The precheck and the commit then build that path's chunk from that diff while HEAD and the file's bytes still match; otherwise the record is stale and ignored. Worktree, index and history untouched. **Authorization:** none beyond the common loopback gate; it grants nothing and commits nothing. Trace `[worklist-commit] op=reanchor`. | agent (asked in chat: "re-anchor the item") |
| `/__worklist/plan` | HTTP POST | `{ summary, steps: [{ verb: "drop" \| "commit" \| "feedback", ids, text? }] }` | validates and stores a version-bound recommendation; feedback requires text and cannot share a plan with commit | agent proposes; pane executes on user click |
| `/__worklist/will-commit` | HTTP GET | `ids=a,b` | `{ ready, stale, mode: "whole-file" \| "interval", added, removed, unowned?: [{ path, added, removed }], entangledWith?: [...], dependentPaths?: [...], ids, stateKey }` (`ready: false` when membership has no partition yet). The gate's routing for that selection, run without committing: what committing those items together would take. The pane shows an answer only when it is fresh and its `stateKey` matches the board's `membershipStateKey` (issue-425-retire-replay) | gate bar (selection line); agent before a multi-id `worklist-commit` |
| `/__worklist/end` | HTTP POST | `{ ids: [...] }` | releases the named ids' inflight claim early (the shrink helper of section 11); 400 on bad JSON; GET or empty ids are refused. Also dispatched from the Codex intent channel as `worklist-end`. **Authorization:** none beyond the common loopback gate; it needs no grant (`handle_iterate_end`). It only shrinks or clears the inflight claim, never a grant, and retires the named ids only if they are in the claim. A caller who knows the ids can release a running turn's claim early, which unlocks the board and ends the claim window used for attribution; it cannot approve, commit or drop. See authorization.md. | agent |
| `/__worklist/misaddressed` | HTTP GET / POST | GET: none. POST: `{ op: "flag", feedbackRef }` (agent), `{ op: "resend", feedbackRef }` (pane's **Resend as chat**), `{ op: "dismiss" }` (pane, on the next send); and the other direction, `{ op: "flag-chat", turnRef, id }` (agent) and `{ op: "resend-as-feedback", turnRef }` (pane's **Resend as feedback**); and a third, `{ op: "flag-item", feedbackRef, id }` (agent) and `{ op: "resend-to-item", feedbackRef }` (pane's **Resend to <id>**) | GET: `{ pending: { kind: "feedback", feedbackRef, itemId, atMs } \| { kind: "chat", turnRef, itemId, atMs } \| { kind: "item", feedbackRef, itemId, fromId, atMs } \| null }`. `flag`: `{ ok, itemId }`, or 400 for a malformed ref and 409 when the ref has no live draft in `feedback-drafts/` or the running turn (`.turn-context.json`) isn't an `iterate` turn addressed to that item. `resend`: 409 unless the ref is the pending flag; appends a "Resent as chat" note to the draft (original text kept above it) and returns `{ ok, itemId, text }` for the pane to send as chat. `flag-chat` (misaddressed-chat-resends-as-feedback): a chat message that was feedback on an item; 400 for a `turnRef` not shaped `<ms>-turn`, 409 unless it is the newest file in `outbound-turns/`, sent as chat (`kind: "message"`, empty `mode`), while the running turn's context is `general`, and `id` is on the board. `resend-as-feedback`: 409 unless the ref is the pending chat flag; returns `{ ok, itemId, text, images, note }` from the turn's record, which the pane sends to the item as feedback (images as screenshot markers, the note saying it was first sent as chat). `flag-item` (footer-selection-label-follows-selection): feedback ticked on X that is about item Y; same checks as `flag` (well-formed ref, live draft, `iterate` turn addressed to the ref's item X; 400 bad ref, 409 otherwise) plus `id` on the board and different from X (409 `not-on-board` / `same-item`); returns `{ ok, itemId, fromId }`. `resend-to-item`: 409 unless the ref is the pending item flag; returns `{ ok, itemId, fromId, text, note }` (the note says it was first sent to X), which the pane sends to Y alone as feedback; the draft is not rewritten. `dismiss`: clears any flag. Every change emits `feedback-misaddressed-changed`. One pending flag, in memory. misaddressed-feedback-resends-as-chat. | agent (flag); agent pane composer (GET, resend, dismiss) |
| `/__worklist/reorder` | HTTP POST | `{ ids: [...], baseVersion: <n> }` | `{ ok: true, changed: bool }`; 409 when `baseVersion` is stale or `ids` is not exactly the current item set (the pane refetches). The pane's drag-to-reorder: changes order only, bumps `version`. **Authorization:** none beyond the common loopback gate; the handler requires no grant because it changes no item's content or lifecycle. Its checks are consistency, not identity: `baseVersion` must equal the version on disk and `ids` must be exactly the current item set, so it cannot add, remove or alter an item (`handle_worklist_reorder`). Those checks mean a caller must already know the board's version and ids. | agent pane iframe |
| `/__worklist/interval-diff` | HTTP GET | `id=<item id or __unattributed>`, `path=<file>` (both required; 400 otherwise) | `{ patch, intervals, … }` — one scope's own patch for one path, from the recorded claim intervals; fetched when the Diff tab's scope selector leaves "All changes". Read-only. | agent pane iframe |
| `/__worklist/independence` | HTTP GET | `path=<file>` (required; 400 otherwise) | `{ items: { <id>: … } }` — per-claimant separability for one path, measured with `git apply --check` in a throwaway index (worktree untouched). Fetched when a file expander renders its ownership summary. Read-only. | agent pane iframe |
| `/__audit/direct-edit` | HTTP POST | `{ provider, source?, turnKey? }` | `{ ok: true }` (`deduped: true` for a repeated `turnKey`); 400 on bad JSON. Audit-only breadcrumb recording that the Claude guard already allowed a direct edit. It never writes `.worklist-authorization.json` and authorizes nothing; see the Direct edit row in [authorization.md](authorization.md#action-matrix). | Claude guard (hook) |
| `/__worklist-config` | HTTP GET | — | `{ batchCommitActions: bool }` from `.bram.json` `worklist` block; defaults to `false` | agent pane iframe (`Workspace.xmlui` gates batch-commit UI on this) |
| `/query` | HTTP POST | body `{ sql, params? }` (or a bare SQL string) — the de-facto XMLUI `dataType="sql"` wire contract | 200 JSON array of column-keyed row objects; 409 when `.bram.json` has no `db`; 400 on bad body / SQL error; read-only enforced at the engine (read-only open + `PRAGMA query_only`); the loopback gate exempts it from the state-change origin rule as read-only, so a Bram-served page's own origin is accepted, and the Host check still applies | target app / pane markup (`DataSource dataType="sql"`), #355 |

- **Recorded bodies and consumption.** `handle_worklist_resolve` filters
  ids if requested and resolves draft bodies. It establishes a claim for
  approved/drop items. It consumes an **approved** record on read;
  subsequent resolver reads return `no_active_authorization`. A **drop**
  record is not consumed on read. Resolve itself is not the final freshness
  check, and a successful resolve is not a reusable grant for unrelated work.
- **Payloads.** Approved/drop turns carry `items: [{id, feedback?, gate?}]`;
  feedback turns use `{id, feedbackRef}` with inline feedback as fallback.
  Hash pairs are not required. `gate:"apply-and-commit"` sets `commitToo`.
  The stored `approved` kind does not otherwise distinguish Start from
  Commit. The ordinary Start path already has a claim; provider instructions
  may still require resolve before application.
- **Mutations.** `validate_worklist_mutate_authorization` checks kind, ids,
  and reconciliation freshness. Advance requires `approved` and can only
  set `applied`. Prune requires `drop`, or `approved` with the post-commit
  status check: applied ids, unless `commitToo` permits proposed ones.
  That check does not independently prove a commit happened. Agents let
  the commit route prune after committing. Direct Worklist writes are for
  proposal metadata; review prose lives in separate drafts.
- **Commit.** `ensure_worklist_commit_authorized` checks approved ids and
  publish freshness. The route separately checks item existence, work on
  disk, declared paths, membership, and message restrictions. Proposed
  items with changes can qualify without an advance. It uses whole-file
  staging for exclusive paths and selected membership hunks for entangled
  paths; see [the mechanics reference](../app/__shell/reference/worklist-mechanics.md).
  Report any retained ids/residual paths. `queuedCloses` is the authority
  for saying an issue will close after push, not the item's `closesIssues`.
- **Freshness.** Both mutation and commit validators deliberately ignore
  `consumedAtMs` so resolve → edit → mutate remains valid. The normal grant
  TTL is 30 minutes from issuance. A recorded interrupt prevents publication
  but permits reconciliation within TTL. The invalidator skips already-
  consumed records; consumption is not comprehensive revocation.
- **Retirement.** Advance/prune retire affected claim ids. Unconsumed grants
  shrink by resolved ids until final consumption; already-consumed grants
  do not shrink. Turn-end retires the execution claim; a still-live approval
  can rearm it on the next turn. See section 11 and
  [authorization lifecycle](authorization.md#records-transport-and-lifecycle).
- **Enforcement limits.** Rust hooks (`guard.rs` / `guard_policy.rs`) reject
  direct lifecycle rewrites and check tool coverage, with the watcher as a
  further check on unauthorized removal. Coverage is not equivalent to
  approval and Bash checks are coarse. Direct Drop assumes a pane caller;
  clear-landed is a deliberate evidence-checked exception. Neither transport
  availability nor an agent-authored recommendation is human consent.

### 3a. Codex filesystem lifecycle channel (#130)

Codex's sandbox refuses loopback connections (`curl: (7)` even when Bram
listens), so Codex drives the lifecycle through files instead of the HTTP
routes above. The host watches the intent file and dispatches it through the
*same* handlers (`handle_worklist_resolve`, `handle_worklist_mutate`,
`handle_worklist_commit`, `handle_iterate_end`), so all
side effects and auth checks are identical.

| Surface | Type | Shape |
|---|---|---|
| `resources/.worklist-intent.json` | file (agent writes) | `{ nonce, route, body? }` — `route` ∈ `worklist-resolve` \| `worklist-mutate` \| `worklist-commit` \| `worklist-end`; `body` is the matching HTTP route's request body |
| `resources/.worklist-result.json` | file (host writes) | `{ nonce, ok, status, result? , error?, completedAtMs }` — `result` is the HTTP route's response body verbatim; `error` present when `ok:false` |

- The watcher drain reads the intent, dispatches it, writes the result
  atomically (`.tmp` + rename), then deletes the intent,
  and traces `[worklist-intent] route=… nonce=… ok=… status=…`.
- Startup deletes any stale intent/result files
  (`cleanup_stale_worklist_intent`) so a leftover result can't be misread as a
  reply to a fresh intent — the agent must match `nonce`.
- Match the response nonce; surface missing/mismatched responses and
  `ok:false`. Unknown routes fail. The dispatcher has no direct-Drop,
  clear-landed, plan, issue-close, or iterate-bracket route.
- The Codex PreToolUse guard exempts `resources/.worklist-intent.json` from
  worklist coverage. Claude is unaffected and keeps using the HTTP routes.

## 4. Worklist history

Reverse-chronological archive of every worklist transition. Snapshots
live under `resources/worklist-history/<ts_ms>.{json,md}` — JSON is the
worklist state at that moment; Markdown is the changelog narrative
(`Items proposed`, `Items applied`, `Items committed`, `Items dropped`,
`Description changed`).

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__worklist-history/list` | HTTP GET | — | `[{ ts, iso, summary, ids, changelog }, …]` (newest first) | agent pane iframe |
| `/__worklist-history/changelog` | HTTP GET | `ts=<ms>` | raw `.md` body, `text/markdown` | agent pane iframe |
| `/__worklist-history/snapshot` | HTTP GET | `ts=<ms>` | raw `.json` body | agent pane iframe |
| `/__worklist-history/at` | HTTP GET | `ts=<ms>` | `{ ts, live, items: [{ id, status, begun }] }`: the board as it stood at `ts`. Each snapshot holds the board as it was just before the change stamped in its name, so this is the earliest snapshot stamped at or after `ts` (`live:false`, its stamp in `ts`), or the live board when nothing has changed since (`live:true`, `ts:null`). One directory listing, no file reads but the one. log-shows-start-commit-push-search: a Log entry's expanded detail | agent pane iframe (the Log tab) |
| `/__history-index` | HTTP GET | `limit=<n>` (default 200, clamped 1–2000) | `[{ id, title, date, extra }, …]` newest first, read from the search index rather than by scanning files; `extra` is the full history group, so an expander needs no refetch. Lags the live files by up to one indexer pass. | agent pane iframe (History tab) |

- The list endpoint parses item ids out of changelog bullet lines
  (`` - `<id>` (was …) ``, `` - `<id>` (proposed, …) ``,
  `` - `<id>`: proposed → applied ``) for the `ids` field. When a snapshot
  records no item transitions (e.g. a description-only edit), the
  endpoint falls back to reading the `.json` sibling and surfacing the
  ids present at that moment, and the summary becomes
  `"description changed"` instead of the generic `"change"`.

### 4a. Feedback history

Reverse-chronological archive of per-iterate user feedback. Each
`iterate:` cycle writes a draft under `resources/feedback-drafts/<feedback_id>.md`;
when the cycle's `advance` / `prune` mutation lands,
`promote_feedback_drafts_for_items` moves the file to
`resources/feedback-history/<unix_ms>-<itemId>.md` so the draft
directory stays small and the history dir accumulates a permanent
record. The standalone Feedback tab and its `/__feedback-history/{list,content,search}`
routes were retired when feedback was folded into the History page
(`444bcd5e`); `/__feedback/content` and `/__feedback/map` remain because the
iterate flow and the Transcript resolve a `feedbackRef` through them.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__feedback/content` | HTTP GET | `ref=<feedbackRef>` | raw `.md` body, `text/markdown`, from `feedback-drafts/` (live) then `feedback-history/` (promoted); 404 if neither has it | agent pane iframe (iterate flow, Transcript) |
| `/__feedback/map` | HTTP GET | `limit=<n>` (default 200) | `{ "<feedbackRef>": "<text>", … }` across drafts and history, most recent first; the Transcript fetches it once to resolve each iterate turn's `feedbackRef` | agent pane iframe (Transcript) |
| `feedback-history-changed` | Tauri event | — | empty payload | agent pane iframe DataSource refetch |

- Filename schema is `<unix_ms>-<itemId>.md`. Uniqueness collisions
  (rare) get a `-<n>` suffix before `.md` via
  `unique_feedback_history_path`.
- `/__feedback/content` reconstructs the filename from the `ref` rather
  than trusting a client-supplied path — no traversal.
- `feedback-history-changed` is emitted by the filesystem watcher
  whenever anything under `resources/feedback-history/` changes. No
  debounce — promotion fires at most once per iterate cycle.

## 5. Sessions

Provider-aware enumeration of Claude Code / Codex JSONL session files
plus content / search / delete / rename. Same route shape for both
providers, switched by the `provider=` query.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__sessions/meta` | HTTP GET | `provider=` | `{ count, latest_mtime, … }` | agent pane iframe |
| `/__sessions/list` | HTTP GET | `provider=` | `[{ id, mtime, title, … }, …]` | agent pane iframe |
| `/__agent/session` | HTTP POST | JSON body `{ provider, session, bram_token }`; `bram_token` must be this instance's menu token (`authenticated_menu_payload`), else 403; `provider` is `claude`, `codex` or `antigravity` and `session` must pass `session_id_is_safe`, else 400 | `{ ok, changed }`. The running CLI naming its own session (session spine rule 1, `docs/decisions/2026-10-27-bram-chooses-the-session.md`): when `provider` is the current provider, the host records `session` as that provider's named session (Antigravity: the live-session cell), remembers it as last active, traces `[agent-switch] op=live-session-set … source=agent-event`, claims a queued title for Antigravity, and emits `sessions-list-changed`. A different current provider, or the same session again, answers `changed:false` without side effects | `bram-guard` in its session-hook modes (`session_report`, `guard.rs`): Claude's and Codex's session hooks post `session_id` (skipped when an `agent_id` marks a subagent), Antigravity's posts `conversationId`; the port and token come from the terminal's `BRAM_PORT` / `BRAM_MENU_TOKEN`, never from the port file. OpenCode's plugin uses `/__opencode/event` instead |
| `/__sessions/latest` | HTTP GET | `provider=` | full JSONL body, `text/plain` | agent pane iframe |
| `/__sessions/latest-meta` | HTTP GET | `provider=` | `{ size, mtime, id }` | agent pane iframe |
| `/__sessions/latest-pending` | HTTP GET | `provider=` | pending tool-use record, JSON | agent pane iframe |
| `/__sessions/content` | HTTP GET | `provider=`, `id=` | full JSONL body for that session, `text/plain` | agent pane iframe |
| `/__sessions/delete` | HTTP GET | `provider=`, `id=` | `{ ok: true }` | agent pane iframe |
| `/__sessions/rename` | HTTP GET | `provider=`, `id=`, `title=` | `{ ok: true }` | agent pane iframe |
| `/__sessions/pair` | HTTP GET | `from=<provider>` (optional), plus one `<provider>=<session id>` per side (exactly two sessions of different providers; placeholder ids refused with 409) | `{ ok: true }` — records the pair/group (the Sessions pair offer's Keep); with `from` also offers a catch-up. Emits `sessions-list-changed`. | agent pane iframe |
| `/__sessions/unpair` | HTTP GET | `provider=`, `id=` | `{ ok: true }` — takes the session out of its group (the Sessions tab's Leave); 500 `provider required` without `provider`. | agent pane iframe |
| `/__sessions/suggest` | HTTP POST | body `{ name, whatsNext? }` (`name` required, else 400) | `{ ok: true }` — asks the pane to open its New session dialog pre-filled; creates nothing. Emits a non-replayable `session-suggest` event. | agent (curl), agent pane iframe |
| `/__sessions/<id>` | HTTP GET | `provider=` | any other `__sessions/` suffix is read as a session id and answers like `/__sessions/content` (full JSONL, `text/plain`). | agent pane iframe |
| `/__turns` | HTTP GET | `provider=claude\|codex` (optional), `id=<session-id>` (optional; by-id fetch), `agent=<agentId>` (optional; subagent transcript), `latest=N` (optional; tail window for live refreshes) | `{ sid, provider, total, windowStart, turns: [...] }` — `total` is the full turn count; `windowStart` is the index of the first returned turn (0 for full fetches); with `latest=N` only the last N turns are returned so the iframe can splice the window onto accumulated history and detect rotation/shrink. Each turn carries `ts` (the ISO timestamp of the record it was built from) when the provider stamps one, as Claude and Codex do; the Transcript uses it to land a Log entry's `/transcript?at=<ms>` link (log-rows-baseline-and-spacing). | agent pane iframe (Transcript, Sessions) |
| `/__send-ledger` | HTTP GET | — | `{ entries: [...], nowMs, staleTerminalInput, awaitingTurn }` — snapshot of the outbound-send ledger. `entries` carry `id/kind/state/cause/viaQueue/mode/preview/injectedAtMs/resolvedAtMs/retried`. `staleTerminalInput` = an Esc-aborted send's copy is known to sit in the terminal input; `awaitingTurn` = a pane-submitted turn is in flight (drives the composer gate). | agent pane iframe |
| `/__current-turn-edits` | HTTP GET | — | `{ added, removed, filePath, kind, lastToolId }` for the current turn's edit aggregates. Same derive-at-the-boundary pattern: host parses the 64 KB tail once per request, iframe binds via DataSource instead of running `currentTurnEdits(lastJsonl)` on every fanout. | agent pane iframe (Workspace turn-edits hint) |
| `/__waiting-for-assistant` | HTTP GET | — | `{ waiting: bool }` — true when the most recent meaningful JSONL record is a user message (`tool_result`-only records skipped). Mirror of the iframe `isWaitingForAssistant` helper. | agent pane iframe |
| `/__conversation-state` | HTTP GET | — | `{ lastAssistantText, lastExchange, currentTurnEdits }` derived from the active session's JSONL, behind a `(path, mtime, len)` cache. | agent pane iframe (Workspace) |
| `/__session-size` | HTTP GET | — | `{ bytes, humanBytes, state: green\|amber\|red, provider, guidance, … }` for the active session file (green ≤ 5 MB, amber ≤ 15 MB); `{}` with no active session. Informational, not a warning. | agent pane iframe (Status tab) |
| `/__agents` | HTTP GET | — | `{ sid, mainModel, agents: [{ agentId, agentType, description, toolUseId, model, state: running\|waiting\|finished, resumedCount, … }] }` — roster of the active session's subagents for the footer chips. `finished` requires the latest completion notice in the parent transcript to be newer than the latest `SubagentStart` the host recorded for that id (`<state>/.subagent-starts.json`), so a resumed subagent reads running again; `resumedCount` is starts seen minus one. | agent pane iframe |
| `/__describe-command` | HTTP POST | body `{ id, command, description?, name?, context?, result? }` | `{ ok: true, description, cached? }` on success; `{ ok: false, reason: "disabled"\|"no-key"\|... }` when the opt-in gates are closed or the API call fails. Synthesizes (or upgrades) a one-line intent header for a tool expansion via the Anthropic Messages API (Haiku). Native agent-authored descriptions (e.g. Claude's Bash `description` param) render on tool rows without this route; it is the enhancement tier — upgrades for Claude, coverage for Codex, whose tool calls carry no native description. Default off: the project must explicitly set `ai.describeCommands: true`, and `ANTHROPIC_API_KEY` must also be present in Bram's environment. Depending on the tool, `command` can carry command text, a diff or written content, or file/search material; `context` and `result` carry bounded excerpts. Every textual prompt field passes through the host credential redactor, backed by the pinned `loomweave-scanner` Rust crate, before request construction. Redaction is heuristic defense in depth, so explicit opt-in—not redaction—is the external-processing boundary. Results are cached in memory by redacted material and overlaid onto `/__turns` tool entries at serve time. `[ai-describe] op=call` records latency, token counts, presence flags, and `redactions=N`, never prompt content. | agent pane iframe (Transcript tool expansion) |

- Provider directories: `~/.claude/projects/<encoded-cwd>/` for Claude
  Code (`claude_sessions_dir` at `lib.rs:1942`),
  `~/.codex/sessions/...` for Codex (`discover_codex_sessions` at
  `lib.rs:2224`). The encoding is the absolute project path with `/`
  → `-`.

Trace persistence: when tracing is enabled, `resources/bram-traces/bram-trace.log`
is the active file. A restart archives the prior active file. In a background
startup pass, raw `bram-trace-*.log` archives older than
`traces.archiveAfterDays` (default 14, allowed range 1–3650) are streamed
through the host credential redactor into sibling `.log.gz` files. The raw
source is removed only after the compressed archive has been fully written,
synced, and atomically installed without overwriting an existing destination.
Failures preserve the raw source. Compressed history is retained indefinitely;
there is no byte cap, so trace storage is intentionally unbounded. The active
file is never an archive candidate during its session.

PTY previews, serialized iframe trace payloads, and the archival pass use the
same host credential redactor; credential detection is supplied by the pinned
`loomweave-scanner` Rust crate, with narrow structural expansion for complete
PEM blocks and Authorization/assignment values. Inspector entries are also
sanitized before IPC. These are high-confidence defenses, not proof that
arbitrary diagnostic content is secret-free.

## 6. Git & repo

Read-only browsing of git state plus the lone IPC mutation (`git_push`).
The HTTP routes shell out to `git`; the IPC command shells out to
`git push` and surfaces the result via a notification channel.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__commits` | HTTP GET | — | `[{ sha, summary, body, author, time }, …]` (HEAD ↓) | agent pane iframe |
| `/__commit` | HTTP GET | `sha=` | `{ sha, summary, body, diff }` | agent pane iframe |
| `/__repo/origin` | HTTP GET | — | `{ remote, owner, name }` | agent pane iframe |
| `/__git-diff` | HTTP GET | `path=` | `git diff -- <path>`, `text/plain` | agent pane iframe |
| `/__file` | HTTP GET | `path=` | file body, `text/plain` | agent pane iframe |
| `/__local-file-preview` | HTTP GET | `path=`, optional `line=`, `sha=`, `maxBytes=` (default 200 KB, ceiling 8 MB) | `{ ok, path, displayPath, repoPath, name, language, renderMode, content, truncated, size, line }`, or `{ ok: false, error, … }` (still 200). The path must resolve inside the project root (symlinks and `..` collapsed); binary files are refused. With `sha=` it serves the file as that commit had it (`git show`, local). | agent pane iframe (file previews, history links) |
| `/__repo/tree` | HTTP GET | — | `{ rows, files }` — tracked plus untracked-not-ignored files as flat Tree rows, with Bram's own state files added; `{ rows: [], error: "not-a-git-repo" }` outside a repo. | agent pane iframe (Files page) |
| `/__repo/tree/dir` | HTTP GET | `path=<dir>` | `{ rows }` for one of Bram's state folders (or a folder beneath one); **403** for any other directory, so it cannot browse arbitrary folders. | agent pane iframe (Files page lazy expand) |
| `/__repo/grep` | HTTP GET | `q=<literal text>` | `{ hits, total, capped }` — literal, case-insensitive `git grep` over tracked and untracked-not-ignored text files plus Bram's state paths, per-file and total caps; empty `q` returns no hits. | agent pane iframe (Files page search) |
| `/__git/status` | HTTP GET | — | `{ ahead, behind, dirty, branch, upstream, hasOrigin, pushWithheld, fetchError }`. Runs `git fetch origin` first so `behind` reflects current remote (without it the Pull button can be dimmed while origin has new commits). The fetch is bounded to 10 s (the child is killed on expiry), runs with `GIT_TERMINAL_PROMPT=0`, and does not hold the Pull/Push rebase lock. A failed or timed-out fetch does not fail the route: counts come from the last good fetch and `fetchError` carries git's first non-empty stderr line or `"timed out"` (`null` when the fetch succeeded or there is no origin). Pull and Push keep their own blocking fetches. | agent pane iframe (Commits tab Pull-button gating, Worklist tab dirty-tree banner) |
| `/__git/pull-rebase` | HTTP POST | — | `{ ok: true }` on success / 500 plain text on error. Runs the equivalent of `git pull --rebase --autostash`. | agent pane iframe (Commits tab Pull button) |
| `/__diff/annotate` | HTTP POST | body `{ diff: "<unified-diff-text>" }` | `[{ kind: "context" \| "hunk" \| "fileheader" \| "add" \| "del", text, segments? }, …]`. `segments` is set on paired 1:1 (-,+) lines and carries per-segment `{ text, bg? }` runs from `similar::TextDiff::from_words` for intra-line word emphasis. `bg` is a theme-variable string (`$color-danger-200` / `$color-success-200`). Diffs over `DIFF_ANNOTATE_LINE_CAP` (1500 lines) skip word-diffing and return plain per-line rows. | agent pane iframe (`<DiffView>` consumers: Workspace TO COMMIT, Commits per-file patch) |
| `git_push` | IPC | — | `Result<(), String>` | iframe helpers (direct) |

## 7. Issues

GitHub issue passthrough via the local `gh` CLI. Read endpoints fetch
JSON; the one write endpoint (`/__issue/comment`) shells out to
`gh issue comment` on the host. There is **no** agent-reachable close
route: `/__issue/close` was removed (close-on-push-automatic, security
H5). Closing is now a host consequence of the user's explicit Push — the
commit gate records ticked issues into `.worklist-issue-close.json`, and
`flush_pending_issue_closes` closes each one whose commit is visible on
origin after the next push. Issue *creation* is still user-driven via the
agent's own shell — there's no `/__issue/create` endpoint.

`/__issue-close-queue` (GET) serves the pending records from that same
file as `{ pending: [{ issue, commitSha, createdAtMs }, …] }` — the
post-push half of the publish story (#329), read by the Commits tab's
`closeQueue` DataSource. `/__issue-close-queue/withdraw` (POST, below,
issue-382-withdraw-queued-close) is the one write route in this section
that is **not** origin-gated the way `/__issue/comment` is. `/__issue/comment`
refuses a foreign `Origin` (M7, docs/security.md); the withdraw route has
no such gate, and adding one would not have made it agent-blocked anyway —
the Origin check at the POST dispatch table (`lib.rs`, near the
`__issue/comment` branch) refuses only a FOREIGN Origin, while its own
comment records that no-Origin callers — the pane's own fetches, and
curl — pass unchecked. So the withdraw route's actual barrier is layered,
not absolute: it is simply unlisted in the agent's `settings.json`
allowlist (an agent's curl call prompts the user rather than running
silently), and every removal is traced and audit-recorded with its
source (`via=pane` for the route, `via=file-edit` for a hand-edit of the
queue file caught by the file watcher — see `docs/trace-vocabulary.md`,
`issue-close-queue` `op=withdrawn`). Symmetric with the close side: the
agent has no way to queue a close either, and must not have a silent way
to unqueue one.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__issues` | HTTP GET | `limit=<n>` (optional, clamped to the project's issue limit); `fresh=1` bypasses the cache | `[{ number, title, state, commentSummary, latestCommentAuthor, latestCommentAt, activityAt, bramState, … }, …]` — served from the cached `issues:list` row. Raw `comments` arrays are stripped at serve time (issues-status-lightweight: they were ~90% of a 3 MB response no list consumer read); the derived comment fields remain, and full comments come from `/__issue`. | agent pane iframe (Issues tab, Worklist cite-an-issue picker) |
| `/__issues/status` | HTTP GET | `numbers=<n>,<n>,…` (optional; omit for every issue) | `[{ number, state, updatedAt }, …]`; unknown numbers are absent | The lightweight "is #N open?" check: same cached list, no forge call, current to the last re-index (`refresh_issue_now` updates it at once for closes Bram makes). Agents can use it instead of `gh issue view N --json state`, which is a network round trip. |
| `/__issue` | HTTP GET | `n=<number>` | `{ number, title, body, state, comments: [...] }` | agent pane iframe |
| `/__issue/comment` | HTTP GET | `number=<n>&body=<urlencoded>` | `gh issue comment` JSON on success, 400 if `number` missing | agent pane iframe |
| `/__issue-close-queue` | HTTP GET | — | `{ pending: [{ issue, commitSha, createdAtMs }, …] }` | agent pane iframe (Commits tab `closeQueue` DataSource) |
| `/__issue-close-queue/withdraw` | HTTP POST | body `{ issue: <n>, commitSha: "<sha>" }` | `{ ok: true }` on removal; `{ ok: false, reason: "no matching record" }` (200, not an error) when nothing matched; 400 `{ error: "issue and commitSha required" }`; 500 `{ error: "..." }` on a write failure | agent pane iframe (Commits tab Withdraw button); not in the agent's allowlist — see prose above |

## 8. Context

Per-provider catalog of agent-coordination files: `CLAUDE.md` / `AGENTS.md`
import chain, agent-managed memory, hooks, and settings. Drives the
Context tab in the agent pane.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__context/list` | HTTP GET | `provider=` | `{ provider, summary, sections: [{ key, label, items: [{ path, display, kind }] }] }` | agent pane iframe |
| `/__context/search` | HTTP GET | `provider=`, `q=` | `{ results: [{ path, display, category, hits: [{ line, snippet }] }] }` (≤ 50 hits) | agent pane iframe |
| `/__context/file` | HTTP GET | `path=` | file body, `text/plain` | agent pane iframe |
| `/__context/skills` | HTTP GET | — | `{ provider, skills: [...] }` — the project's skills from `.claude/skills/*/SKILL.md` (issue-221-skill-launcher). | agent pane iframe |

## 9. Voice / transcription

Whisper subprocess lifecycle. The parent shell auto-starts the server
on first record click; the IPC commands are also the only way to stop
or query state. The transcription HTTP server listens at `http://127.0.0.1:18080`; on Windows the IPC launcher starts it inside WSL with `wsl.exe`.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `whisper_start` | IPC | `{ modelPath }` | `Result<u32, String>` | parent shell |
| `whisper_stop` | IPC | — | `Result<(), String>` | parent shell |
| `whisper_status` | IPC | — | `WhisperStatusReport` | parent shell |
| `whisper_preflight` | IPC | `{ modelPath }` | `WhisperPreflightReport` | parent shell |

`whisper_preflight` runs on a 🎤 click when the engine isn't already
answering, before `whisper_start`. It reports, for `whisper-server`, the
model file and `ffmpeg`, whether each was found and where (`{ found, path }`),
plus the directories searched, whether the user's login shell was used, and
Bram's PATH. It searches Bram's PATH, then `/opt/homebrew/bin` and
`/usr/local/bin`, then the login shell (`command -v`, 2 s limit), so a Bram
opened from the Dock still finds Homebrew installs. It traces one line,
`[whisper] preflight binary=… model=… ffmpeg=… searched=… login_shell=… path=…`.
On Windows it returns `checked: false` (the engine runs inside WSL).

`whisper_start` spawns the binary where the check found it and passes
`--convert` only when `ffmpeg` was found, putting `ffmpeg`'s directory on
the engine's PATH if Bram's lacks it. `--convert` serves only the safety
pass (the whole compressed recording, transcribed when live dictation
fails); without `ffmpeg` live dictation works and the safety pass is off.
It traces `[whisper] start convert=<bool> …`.

When something is missing, the shell posts `{ type: "bram-voice-setup",
missing, blocking, report }` to the agent pane, which opens its setup
dialog. `blocking` is true when `whisper-server` or the model is missing,
and then the engine is not started.

### The dictation script (`/__shell/dictation.js`)

Live dictation itself (capture, the sliding-window loop, pause repair, and
writing into a field) is a host-neutral script, served to any page at
`/__shell/dictation.js` with its capture worklet beside it at
`/__shell/dictation-capture-worklet.js` (judell/bram#417). It defines
`window.BramDictation` and uses no Tauri call, no `postMessage` and no
`localStorage`. Bram's shell and pane are its first two callers; the
`voice-start` / `voice-stop` / `voice-into-partial` / `voice-into-result` /
`voice-edit-boundary` messages between them are Bram's own transport, not
part of the script.

| Entry point | Takes | Gives | Caller |
| --- | --- | --- | --- |
| `BramDictation.createLoop(options)` | `engine`, `trace(stage, fields)`, `requestId`, `pauseMs`, `workletUrl`, `onPartial(partial)` | `start(mediaStream)`, `finish()` → `{ text, errors, parts }`, `abort()`, `setEditBoundary(fromSeq)` | parent shell (`app/main.js`) |
| `BramDictation.createField(options)` | `get()`, `set(text)`, `trace`, `onEditBoundary(fromSeq)` | `showLive(partial)`, `finalize(transcript, parts)`, `markStopped()` | agent pane (`helpers.js`) |
| `BramDictation.attach(options)` | `get()`, `set(text)`, optional `onState(state, extra)`, `trace`, `pauseMs`, `audio`, `engine` | `start()`, `stop()` → transcript, `recording` | a project page |
| `BramDictation.engines` | — | the engine adapters; `whisper` is the only one | all |

A project page uses `attach`:

```html
<script src="/__shell/dictation.js"></script>
<script>
  const box = document.querySelector("textarea");
  const dictation = BramDictation.attach({
    get: () => box.value,
    set: (text) => { box.value = text; },
  });
  // dictation.start() on a 🎤 click, dictation.stop() on the next.
</script>
```

**`get` must return the field's text at the moment it is called.** A stale
`get` fails silently and looks like a dictation that erases itself: every
window reads as an edit back to the old text, and the final write empties
the field. The script traces `live-get-stale-suspected` when it sees the
pattern. In an XMLUI page the trap is specific: a component handed from
markup to page script is a snapshot whose methods keep working but whose
`value` stays frozen, so `get: () => box.value` is stale. Push the value
from the field's `onDidChange` instead (reported by Bram Studio on
judell/bram#417, where this bridge ran):

```xml
<TextArea id="box" onDidChange="(v) => window.noteValue(v)" />
```

```js
var latest = "";
window.noteValue = (v) => { latest = String(v || ""); };
BramDictation.attach({
  get: () => latest,
  set: (text) => { latest = text; box.setValue(text); },
});
```

The function the markup calls must exist before the XMLUI engine loads.
Bram's own pane avoids the trap another way: its markup passes a fresh box
on every partial.

Two options a page may want:

- `audio`: `getUserMedia` audio constraints, to choose a microphone
  (`{ deviceId: { exact: id } }`). The default is the browser's default
  input.
- `engine`: an engine adapter. For a Whisper server at another address,
  copy the built-in one: `Object.assign({}, BramDictation.engines.whisper,
  { host: "http://127.0.0.1:8767", url: "http://127.0.0.1:8767/inference" })`.

`start()` rejects with an error whose `code` is `engine-unavailable`,
`no-microphone` or `capture-unavailable`. The script cannot start the
speech engine: `whisper_start` is IPC-only, so a page gets dictation only
while the engine on port 18080 is already running (after a first 🎤 click
in Bram's pane). Each frame captures its own microphone; Bram's
one-recording-at-a-time guard covers Bram's pane only.

A working example is served by any running Bram at
`/__shell/dictation-example.html` (`app/__shell/dictation-example.html`).

Tests, both run with `node` and needing no microphone or Bram window:

- `scripts/tests/dictation-logic.test.js`: the loop and the field, driven
  by hand with a stubbed engine.
- `scripts/tests/dictation-pane.test.js`: the real `helpers.js` loaded
  under a stub window, with the pane's helper functions driven by the
  messages `main.js` sends.

## 10. Static & hot-reload

Static files served from the binary's on-disk `app/` (preferred) or
embedded copy (fallback). The filesystem watcher in `lib.rs` reloads
iframes when files under `app/__shell/`, `app/vendor/`, or `app/tools/`
change.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__shell/<path>` | HTTP GET | — | file body, content-typed | both iframes |
| `/__vendor/<path>` | HTTP GET | — | vendor JS/CSS, content-typed | both iframes |
| `/__tools/<path>` | HTTP GET | — | agent pane XMLUI sources | agent pane iframe |
| `/__bram/<path>` | HTTP GET | — | a file in Bram's state folder (`.bram/`), content-typed; `worklist.json` answers `{description:"", items:[]}` if missing, anything else missing is 404. Refuses `..`, `.`, empty segments, backslashes and drive colons (`state_url_rel`). The `/resources/` alias that also served this was removed in 0.8.7: `/resources/<path>` is now an ordinary project path. | agent pane iframe |
| `/__project/<path>` | Tauri scheme only | — | proxied to the target app's upstream with the prefix stripped (loopback default or an external dev server). Not handled by the loopback server. The isolated `bramapp://` target scheme serves only this project content and refuses every `__*` route and shell asset. | target-app iframe |
| `/<project path>` | HTTP GET | — | project file served from the project root (the loopback fallback after every reserved route); extensionless misses fall back to the root `index.html` when one exists (SPA fallback, issue-358), other misses 404. | target app, browser |

- `app/__shell/` and `app/vendor/` changes trigger reload in both
  iframes; `app/tools/` changes reload only the agent pane iframe; the
  user's project directory triggers a target app reload only. The
  parent shell (`app/index.html`, `app/main.js`) is not hot-reloaded —
  changes there require `cargo build` and a restart.
- `/__bram/worklist.json` returns the empty-worklist JSON instead of
  `404` when the file doesn't exist yet, so the Workspace tab's polling
  loop doesn't flood devtools with 404s in guest projects that haven't
  opted in to the worklist flow.

## 11. Inflight sentinel

The Worklist tab's spinner state derives from a single on-disk file,
`resources/.inflight-claim.json`, written and cleared by host-side
HTTP handlers. Replaces an earlier iframe-side heuristic chain that
accumulated false-clears, premature clears, and silent
inconsistencies. See `app/__shell/conventions.md` for the agent-side
convention prose and failure-mode guide; this section is the HTTP /
file / event reference.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__inflight` | HTTP GET | — | sentinel JSON or `{}` if no claim | agent pane iframe |
| `resources/.inflight-claim.json` | file | — | `{ ids: [...], claimedAt: <ms>, kind: "approved" \| "drop" \| "iterate" }` or absent | host write, iframe via `/__inflight` |
| `inflight-claim-changed` | Tauri event | — | empty payload | agent pane iframe |

- **File invariants.** Either absent or contains valid JSON with all
  three fields (`ids`, `claimedAt`, `kind`). Writes are atomic via
  `.tmp` + rename. The host serializes writes (single-process).
- **Lifecycle by `kind`.** Approval recording establishes an `approved`
  claim before delivery; resolve can also establish approved/drop claims.
  The host establishes `iterate` claims on the addressed-turn path.
  Turn-end clears execution claims; a still-live approved grant can rearm
  its claim on the next turn. Claim lifetime is not authorization lifetime.
- **Per-id retirement.** Advance/prune/commit retire the resolved ids and
  preserve any remaining claim. `POST /__worklist/end {"ids":[...]}` is an
  explicit early release and uses the same shrink helper; GET or empty ids
  are refused. A full-clear helper, in contrast, refuses partial coverage.
- **No arbitrary live-session timeout.** Cooperative lifecycle calls,
  matching turn-end detection, explicit release, and startup cleanup retire
  claims. An expired mutation grant releases the refused ids as well. A
  lingering spinner should be diagnosed from those records, not treated as
  proof that approval remains valid.
- **Independent turn completion detection.** In addition to cooperative
  agent calls, the host watches session JSONL for durable completion
  records. Claude clears on the most recent assistant record with
  `message.stop_reason:"end_turn"`. Codex clears on `event_msg` /
  `payload.type:"task_complete"`. Both providers use the sentinel's
  `claimedAt` as a stale-line guard so a completion record from a prior
  turn cannot clear a newly-started claim. PTY silence and explicit
  cancellation remain fallback clear paths.
- **Turn completion monitor.** `/__coordination-status` includes
  `raw.turnCompletion` and the Status tab's Inflight Sentinel section
  includes a `Turn completion` row. The row reports the last detector
  source (`jsonl`, `pty-silence`, `mutate`, `iterate-end`, `cancel`),
  provider, reason, timestamp, and whether the observed completion was
  after the active claim. This is the first place to look when a spinner
  is stuck.
- **Scope boundary.** XMLUI component busy states and Bram Worklist
  busy states have different sources of truth. APICall-driven controls
  clear from the component's `inProgress` state and lifecycle handlers;
  Worklist controls clear from the host-managed inflight sentinel served
  by `/__inflight`. A scheduler fix in XMLUI APICall handling can
  remove delayed component cleanup, but it does not remove the need for
  this host-side turn-completion monitor for approved/drop/iterate
  cycles.
- **`inflight-claim-changed`** is emitted from inside the host helpers
  after the file write / delete completes. Iframe
  subscribers refetch `/__inflight` on receipt; the `Workspace.xmlui`
  `inflightClaim` DataSource is the primary consumer.
- **Trace categories.** `[inflight-sentinel] op=write kind=<…> ids=[…]`
  on writes, `[inflight-sentinel] op=clear ids=[…]` on clears,
  `[inflight-sentinel] op=stale-startup-clear` on startup-time cleanup.
  `[jsonl-turn-end] op=enter|detect|skip provider=<claude|codex>
  reason=<...>` records durable turn-completion detector decisions.
  Paired with `[emit] kind=inflight-claim-changed` and
  `[iframe] subkind=listener-fired context=inflight-claim-changed`
  downstream.

## 12. Agent message queue

The Queue tab persists unsent messages in
`<state>/.bram-queue.json`. Each accepted save also retains message records in
`<state>/queue-drafts/<id>.json`; sending or deleting removes them from the active
queue but leaves these saved drafts for the Log. Older already-removed messages
may have no draft. Message bodies are not written to the trace. Entries remain user-controlled: saving
does not inject them into the terminal; the iframe calls the existing
`toTurn` path only when the user clicks an item's **Send** button.
The message box's unsent draft is persisted the same way, in
`resources/.bram-message-draft.json` behind `/__draft` and `/__draft/save`.
Each queue edit also writes a synchronous `sessionStorage` recovery snapshot
before the debounced host save. An iframe refresh restores that snapshot
and retries `/__queue/save`. The recovery snapshot remains until replaced by
a later mutation or the browser session ends. Saves are serialized using the
last acknowledged version. Removed entries carry their final editor values
through the debounce window as `retainedEntries`; the host preserves those
drafts before replacing the active queue. Old clients may omit this field;
the host still retains their previously saved entries.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__queue` | HTTP GET | — | `{ entries: [{ id, text, sendMode, targetItemId, updatedAtMs }], version }`; `sendMode` is `message` or `iterate`; missing file defaults to `{ entries: [] }` | agent pane iframe |
| `/__queue/save` | HTTP POST | body `{ entries: [...], version, retainedEntries?: [...] }` | `{ ok: true, version }`; rejects a stale/missing base version with 409 before writing. Retains per-message drafts, then atomically replaces `<state>/.bram-queue.json`. `retainedEntries` is not kept in the active queue file. | agent pane iframe |
| `/__queue/draft` | HTTP GET | `id` (a queue id, `q-…`) | `{ active: true, entry }` while the message is queued, `{ active: false, entry }` from its retained draft in `<state>/queue-drafts/` after a send or delete, or `{ missing: true }` (older message, unknown or path-shaped id). The entry carries `agent` (recorded when it was written), `sentToAgent` / `sentAtMs` or `deletedAtMs` when known. Read-only; never recreates an entry. Backs the Log's Open in Queue link (`log-queue-entry-opens-message`). | agent pane iframe (Queue tab) |
| `/__draft` | HTTP GET | — | `{ text }` — the message box's unsent draft from `resources/.bram-message-draft.json` (empty string when none); 500 without a project root | agent pane iframe |
| `/__draft/save` | HTTP POST | body `{ text }` | `{ ok: true }`; an empty `text` removes the file; 400 on bad JSON or missing `text`. Last writer wins, one message box per window. | agent pane iframe |

`sendMode:"message"` uses the footer-equivalent `toTurn` path.
`sendMode:"iterate"` requires a current `targetItemId` and delegates to
`sendIterateWithFeedbackDraft`, preserving the same feedback-draft and
structured lifecycle path as item feedback: a message sent from the
composer with Worklist items selected.

## 13. Prompt lifecycle

- **`POST /__menu/permission`**, **`POST /__menu/permission/clear`** — hook
  callbacks for presenting and retiring permission menus. Their JSON bodies
  must include `bram_token` equal to the current instance's inherited
  `BRAM_MENU_TOKEN`; missing or foreign credentials receive 403. The existing
  event-specific tool fields are built by `guard.rs::menu_post_body`. These
  routes do not grant tool or Worklist authorization. Local Codex launches use
  `--no-daemon` when supported so the hook inherits the correct credential.
  Explicit remote launches retain their requested transport; an unhandled
  Codex permission block can instead reveal the terminal. Never copy tokens
  into project files, command arguments, traces, or issue reports.

- **`GET /__prompt-lifecycle`** — Bram's locally-implemented
  PromptShown/PromptResolved ledger (upstream-prompt-lifecycle-events;
  rationale in `docs/upstream-asks.md` #3). Returns
  `{ open, history, nowMs }`: `open` is the currently-displayed prompt
  (`promptId`, `tool`, `toolUseId`, `labels`, `source: hook|grid`,
  `shownAtMs`) or null; `history` is the bounded (200) event stream of
  `prompt-shown` / `prompt-resolved` records, the latter carrying
  `outcome: answered|resolved|interrupted|superseded|session-ended`,
  `detail`, and `openMs`. Read-only; producers are the existing menu
  transition points in `lib.rs`. Trace mirror: the `[prompt-lifecycle]`
  category.

- **`POST /__hook-trace`** — `{ script, event, tool, target, cwd,
  decision, reason }` from the hook scripts; the host writes one `[hook]`
  trace line (newlines flattened) and answers `{ ok: true }`; 400 on bad
  JSON. Trace-only: it decides and grants nothing.

- **`POST /__opencode/event`** — turn boundaries from the OpenCode plugin
  (`type`, `sessionID`, `at`). The host traces the event and, while OpenCode
  is the active provider, moves the claim and refreshes the Transcript;
  answers `{ ok: true, handled: bool }`, 400 on bad JSON. It grants no
  authorization. The plugin sends `bram_token` (`BRAM_MENU_TOKEN`) when it
  runs in Bram's terminal. A wrong token is dropped, as is a missing one once
  this Bram has seen a valid one (`op=foreign-dropped`, still 200). Until then
  a missing one is accepted, for a plugin older than the token.

## 14. Search index

Unified full-text search over the project's history (sessions, commits,
issues, worklist history), served from the SQLite FTS5 index. The agent-side
drill is described in `app/__shell/conventions.md` (Search-first).

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__search` | HTTP GET | `q=`, `limit=` (default 50, clamped 1–500), `types=session,commit,issue,worklist-history` (optional), `mode=and\|phrase\|prefix\|raw` (default `and`) | `[{ type, source, date, link, snippet, rank, key, id, provider, extra }, …]`; **503** `{ error: "search index not ready", reason: holding\|initial-scan, hint, … }` until a first index cycle completes (never read it as "no history") | agent pane iframe (Search page), agents |
| `/__search/doc` | HTTP GET | `key=<hit key>` | `{ content }` — the stored full content of one hit (empty when unknown); same 503 as `/__search` | agent pane iframe (inline expanders), agents |
| `/__search-index-status` | HTTP GET | — | `{ active_buckets, total, issuesThrough, lastCycleMs, progress?, last_added }` — indexer state for the footer indicator and the Search page | agent pane iframe |

## 15. Needs You inbox

The Inbox tab's data: what is waiting on the user, in four lanes.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__needs-you` | HTTP GET | — | `{ verifiedAtMs, checking, lanes: { needsYouNow, yourDecision, othersHands, looseEnds }, notes }`. Forge rows carry `excerpt`: the first line of real content in the thread's latest comment (a Bram forge signature line, blank lines and leading `#`/`>` marks skipped; whitespace collapsed; capped at 160 chars with an ellipsis; empty when the comment is only a signature). It comes from the existing latest-comment fetch, with no extra `gh` call. Untrusted text: render as plain text, never Markdown. | agent pane iframe (Inbox tab) |
| `/__needs-you/dismiss` | HTTP POST | `{ id, activityMarker }` (both required; 400 otherwise) | `{ ok: true }` — hides a forge item until new activity changes its marker; emits `needs-you-changed`. | agent pane iframe |

## Drift policy

Code under `src-tauri/src/lib.rs` is authoritative. This catalog is the
announcement surface for backend APIs — update it whenever a route or
IPC command is added, renamed, removed, or has its response shape
changed. Approximate line ranges for orientation:

- HTTP routes: the `route_request` function (GET-style routes) and the
  `handle_http` function (POST-only routes, the Origin gate and the `__test/`
  prefix), both in `lib.rs`.
- IPC commands: `lib.rs:1279–1880` (individual `#[tauri::command]`
  functions) and `lib.rs:5654` (the `tauri::generate_handler!`
  registration).
- Worklist authorization plumbing: use the current symbol map in
  [authorization.md](authorization.md#where-the-implementation-lives);
  historical line offsets no longer locate these handlers.
