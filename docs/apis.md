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
| 13 | [Prompt lifecycle](#13-prompt-lifecycle) | Locally implemented prompt shown/resolved ledger | agent pane iframe |

## 1. App & shell meta

App-wide version, screen, and process information; PTY echo views and
write/resize control. The parent shell uses these to render the
update-available banner and to drive the terminal; the agent pane iframe
uses them for the target app info dialog.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__app-info` | HTTP GET | — | `{ current, latest, has_update, release_url }` | parent shell, agent pane iframe |
| `/__target app-info` | HTTP GET | — | `{ url, default_right_pane, spawned? }` | agent pane iframe |
| `/__restart-server` | HTTP GET | — | empty / 200 on success | agent pane iframe |
| `/__error` | HTTP GET | — | reported error context | agent pane iframe |
| `/__pty-tail` | HTTP GET | `lines=` | last N lines of PTY output, `text/plain` | agent pane iframe |
| `/__pty-stripped` | HTTP GET | — | PTY output with ANSI escapes removed, `text/plain` | agent pane iframe |
| `/__pty-menu` | HTTP GET | — | current permission menu (if any), JSON | agent pane iframe |
| `/__settings` | HTTP GET | — | user-facing slice of `.bram.json`: `{ shell: { agent }, worklist: { batchCommitActions }, ui: { targetAppMinimized } }` (always populated, defaults filled) | agent pane iframe (Settings tab), parent shell |
| `/__settings` | HTTP POST | body matches GET shape | merged response after writing `.bram.json` atomically; preserves unknown top-level keys; 400 on JSON parse error / 500 on IO error | agent pane iframe (Settings tab) |
| `settings-changed` | Tauri event | — | same payload `GET /__settings` returns | agent pane iframe (Settings DataSource refetch), parent shell (`app/main.js` `targetAppMinimized` driver) |
| `/__coordination-status` | HTTP GET | — | derived status payload for the Status tab: `{ generatedAt, sections: [{ title, rows: [{ signal, level, state, detail, seen }] }] }`. Aggregates worklist counts by status, inflight sentinel state, turn-completion monitor state, authorization record freshness, watcher / hook health, recent trace tails. | agent pane iframe (Status tab) |
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
| `/__worklist/plan` | HTTP POST | `{ summary, steps: [{ verb: "drop" \| "commit" \| "feedback", ids, text? }] }` | validates and stores a version-bound recommendation; feedback requires text and cannot share a plan with commit | agent proposes; pane executes on user click |
| `/__worklist/will-commit` | HTTP GET | `ids=a,b` | `{ ready, stale, mode: "whole-file" \| "interval", added, removed, unowned?: [{ path, added, removed }], entangledWith?: [...], dependentPaths?: [...], ids, stateKey }` (`ready: false` when membership has no partition yet). The gate's routing for that selection, run without committing: what committing those items together would take. The pane shows an answer only when it is fresh and its `stateKey` matches the board's `membershipStateKey` (issue-425-retire-replay) | gate bar (selection line); agent before a multi-id `worklist-commit` |
| `/__worklist-config` | HTTP GET | — | `{ batchCommitActions: bool }` from `.bram.json` `worklist` block; defaults to `false` | agent pane iframe (`Workspace.xmlui` gates batch-commit UI on this) |
| `/query` | HTTP POST | body `{ sql, params? }` (or a bare SQL string) — the de-facto XMLUI `dataType="sql"` wire contract | 200 JSON array of column-keyed row objects; 409 when `.bram.json` has no `db`; 400 on bad body / SQL error; read-only enforced at the engine (read-only open + `PRAGMA query_only`) | target app / pane markup (`DataSource dataType="sql"`), #355 |

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
| `/__worklist-history/search` | HTTP GET | `q=<query>` (min 2 chars; shorter returns `{results:[]}`) | `{ results: [<WorklistHistoryGroup augmented with `hitBody`>, …] }`. `hitBody` is concatenated title + subtitle + prose summary + before / after, centered on the first match, capped at 4 KB. | agent pane iframe (History tab `<SearchHitModal>`) |

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
record. The backend keeps these routes for audit/history tooling; the primary
user-facing feedback flow now lives on each Worklist item.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__feedback-history/list` | HTTP GET | `limit=<n>` (default 120) | `[{ ts, itemId, fileName }, …]` (newest first) | agent pane iframe / history tooling |
| `/__feedback-history/content` | HTTP GET | `ts=<unix_ms>`, `itemId=<id>` | raw `.md` body, `text/markdown`; 400 on missing / unsafe params; 404 if no matching file | agent pane iframe / history tooling |
| `/__feedback-history/search` | HTTP GET | `q=<query>` (min 2 chars) | `{ results: [{ ts, itemId, fileName, snippet, body }, …] }`. `snippet` is a ~200-char window centered on the first match; `body` is the full `.md` body for the `<SearchHitModal>` 500-char centered window. | agent pane iframe / history tooling |
| `feedback-history-changed` | Tauri event | — | empty payload | agent pane iframe DataSource refetch |

- Filename schema is `<unix_ms>-<itemId>.md`. Uniqueness collisions
  (rare) get a `-<n>` suffix before `.md` via
  `unique_feedback_history_path`.
- The content route reconstructs the filename from `ts` + `itemId`
  rather than trusting a client-supplied path — no traversal.
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
| `/__sessions/latest` | HTTP GET | `provider=` | full JSONL body, `text/plain` | agent pane iframe |
| `/__sessions/latest-meta` | HTTP GET | `provider=` | `{ size, mtime, id }` | agent pane iframe |
| `/__sessions/latest-pending` | HTTP GET | `provider=` | pending tool-use record, JSON | agent pane iframe |
| `/__sessions/content` | HTTP GET | `provider=`, `id=` | full JSONL body for that session, `text/plain` | agent pane iframe |
| `/__sessions/delete` | HTTP GET | `provider=`, `id=` | `{ ok: true }` | agent pane iframe |
| `/__sessions/rename` | HTTP GET | `provider=`, `id=`, `title=` | `{ ok: true }` | agent pane iframe |
| `/__turns` | HTTP GET | `provider=claude\|codex` (optional), `id=<session-id>` (optional; by-id fetch), `agent=<agentId>` (optional; subagent transcript), `latest=N` (optional; tail window for live refreshes) | `{ sid, provider, total, windowStart, turns: [...] }` — `total` is the full turn count; `windowStart` is the index of the first returned turn (0 for full fetches); with `latest=N` only the last N turns are returned so the iframe can splice the window onto accumulated history and detect rotation/shrink. | agent pane iframe (Transcript, Sessions) |
| `/__send-ledger` | HTTP GET | — | `{ entries: [...], nowMs, staleTerminalInput, awaitingTurn }` — snapshot of the outbound-send ledger. `entries` carry `id/kind/state/cause/viaQueue/mode/preview/injectedAtMs/resolvedAtMs/retried`. `staleTerminalInput` = an Esc-aborted send's copy is known to sit in the terminal input; `awaitingTurn` = a pane-submitted turn is in flight (drives the composer gate). | agent pane iframe |
| `/__current-turn-edits` | HTTP GET | — | `{ added, removed, filePath, kind, lastToolId }` for the current turn's edit aggregates. Same derive-at-the-boundary pattern: host parses the 64 KB tail once per request, iframe binds via DataSource instead of running `currentTurnEdits(lastJsonl)` on every fanout. | agent pane iframe (Workspace turn-edits hint) |
| `/__waiting-for-assistant` | HTTP GET | — | `{ waiting: bool }` — true when the most recent meaningful JSONL record is a user message (`tool_result`-only records skipped). Mirror of the iframe `isWaitingForAssistant` helper. | agent pane iframe |
| `/__tool-detail` | HTTP GET | `id=<toolId>` | `{ input, result }` or `null` for a single tool by id. Mirrors `getToolDetail(jsonlText, toolId)`. | agent pane iframe (Workspace in-flight edit detail / compatibility consumers) |
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
| `/__git/status` | HTTP GET | — | `{ ahead, behind, dirty }`. Runs `git fetch origin` first so `behind` reflects current remote (without it the Pull button can be dimmed while origin has new commits). | agent pane iframe (Commits tab Pull-button gating, Worklist tab dirty-tree banner) |
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
| `/resources/worklist.json` | HTTP GET | — | file body, or `{description:"", items:[]}` if missing | agent pane iframe |

- `app/__shell/` and `app/vendor/` changes trigger reload in both
  iframes; `app/tools/` changes reload only the agent pane iframe; the
  user's project directory triggers a target app reload only. The
  parent shell (`app/index.html`, `app/main.js`) is not hot-reloaded —
  changes there require `cargo build` and a restart.
- `/resources/worklist.json` returns the empty-worklist JSON instead of
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
`resources/.bram-queue.json`. Entries remain user-controlled: saving
does not inject them into the terminal; the iframe calls the existing
`toTurn` path only when the user clicks an item's **Send** button.
Each edit also writes a synchronous `sessionStorage` recovery snapshot
before the debounced host save. An iframe refresh restores that snapshot
and retries `/__queue/save`; only acknowledgement of the identical
payload clears it.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__queue` | HTTP GET | — | `{ entries: [{ id, text, sendMode, targetItemId, updatedAtMs }] }`; `sendMode` is `message` or `iterate`; missing file defaults to `{ entries: [] }` | agent pane iframe |
| `/__queue/save` | HTTP POST | body `{ entries: [...] }` | `{ ok: true }`; validates that `entries` is an array, then replaces `resources/.bram-queue.json` | agent pane iframe |

`sendMode:"message"` uses the footer-equivalent `toTurn` path.
`sendMode:"iterate"` requires a current `targetItemId` and delegates to
`sendIterateWithFeedbackDraft`, preserving the same feedback-draft and
structured lifecycle path as item feedback: a message sent from the
composer with Worklist items selected.

## 13. Prompt lifecycle

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

## Drift policy

Code under `src-tauri/src/lib.rs` is authoritative. This catalog is the
announcement surface for backend APIs — update it whenever a route or
IPC command is added, renamed, removed, or has its response shape
changed. Approximate line ranges for orientation:

- HTTP routes: `lib.rs:4800–5600` (the `route_request` function).
- IPC commands: `lib.rs:1279–1880` (individual `#[tauri::command]`
  functions) and `lib.rs:5654` (the `tauri::generate_handler!`
  registration).
- Worklist authorization plumbing: use the current symbol map in
  [authorization.md](authorization.md#where-the-implementation-lives);
  historical line offsets no longer locate these handlers.
