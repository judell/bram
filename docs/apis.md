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
providing Worklist, Commits, Issues, Sessions, History, Context, Status, and
Settings tabs).
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

The pending-worklist surface plus the verified-authorization endpoint that
agents read after an `approved:` / `drop:` payload arrives. Per-item
`hash` is computed server-side (SipHash via `DefaultHasher` over the
canonical JSON serialization) and travels with each item — the UI
propagates it back into the structured payload so the watcher can verify
without re-shipping content.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__worklist` | HTTP GET | — | `{ description, items: [{ id, status, file(s), before, after, hash, diff? }], exists, resourcesExists, path }` | agent pane iframe |
| `/__worklist/init` | HTTP GET | — | same shape as `/__worklist` (file created if missing) | agent pane iframe |
| `/__worklist/resolve` | HTTP GET | `ids=foo,bar` | active: `{ kind, ids, items, mismatchedIds, issuedAtMs, source, consumedAtMs }` · consumed: `{ kind: "no_active_authorization", consumedAtMs }` | agent (curl) |
| `/__worklist/mutate` | HTTP POST | body `{ op: "prune" \| "advance", ids: [...], status?: "applied" }` | `{ ok: true, pruned: [...] }` / `{ ok: true, advanced: [...] }`, or 400 `{ error: "…" }` on auth-kind mismatch | agent (curl) |
| `/__worklist/commit` | HTTP POST | body `{ ids: [...], message: "..." }` | `{ ok: true, sha }`, or 400/500 `{ error: "..." }` | agent (curl) |
| `/__worklist-config` | HTTP GET | — | `{ batchCommitActions: bool }` from `.bram.json` `worklist` block; defaults to `false` | agent pane iframe (`Workspace.xmlui` gates batch-commit UI on this) |

- `/__worklist` injects a `diff` field on each `applied` item (the output
  of `git diff -- <file>`) so the TO COMMIT rows can preview their pending
  change inline.
- `/__worklist/resolve` returns the most recent verified authorization
  record. Active-record `kind` is one of `approved`, `drop`, `rejected_stale`.
  When `rejected_stale`, the supplied hashes did not match the on-disk
  file at receive time — the agent must surface staleness and refuse to
  edit. The optional `ids=` query filters `items[]` and `ids[]` to the
  named subset.
- **Consume-on-read for `approved`.** A successful resolve of an `approved`
  record consumes it (sets `consumedAtMs` on the file). Subsequent reads
  return `{ kind: "no_active_authorization", consumedAtMs }` — agents
  must NOT treat that as authorization. This is the architectural
  backstop for the `iterate:` / `talk:` / any-non-authorization turn that
  reflexively curls the resolver: it gets an unambiguous "nothing here"
  instead of stale approval data. `drop` records are **not** consumed by
  the resolver — `maybe_enforce_worklist_policy` (in `lib.rs`) consumes
  drop after observing the prune so authorized prunes survive the
  watcher round-trip.
- Authorization payloads the agent sees in chat carry only `{id, hash}`
  pairs. To fetch the full verified content the agent calls
  `/__worklist/resolve` rather than parsing the `approved:` line.
- `/__worklist/mutate` is the symmetric mechanical-mutations counterpart
  to `/__worklist/resolve`. `prune` requires `kind: "drop"` (or
  `kind: "approved"` for the post-commit prune case) covering every
  requested id; `advance` requires `kind: "approved"`. This is the
  canonical path for mechanical worklist state changes; direct edits to
  `resources/worklist.json` are for proposal authoring and iterate-time
  prose refinement. The chat doesn't render a diff and the server-side
  auth check is uniform.
- `/__worklist/commit` is the server-side commit gate for approved TO
  COMMIT items. It requires an `approved` auth record covering every id,
  requires every id to be `status:"applied"`, stages only those items'
  listed `file`/`files`, refuses when unrelated files are already staged,
  commits with the supplied message, then prunes through the same mutation
  machinery that clears the inflight sentinel and records history.
- **Authorization state-machine enforcement.** `record_worklist_authorization_from_input`
  parses the structured turn and calls `build_worklist_authorization_record`
  to verify each supplied item hash against the resolved on-disk worklist
  item. Hash drift produces `kind: "rejected_stale"` with no verified
  item bodies. `handle_worklist_mutate` delegates auth-kind, id-coverage,
  and post-commit prune checks to pure helpers before it edits
  `worklist.json`: `advance` requires an `approved` record; `prune`
  requires `drop`, except the post-commit prune path accepts `approved`
  only when every requested item is already `status: "applied"`.
  Provider hooks (`worklist-guard.py`) reject direct `worklist.json`
  status changes or item removals so mechanical state changes stay on
  the host route.
- A same-turn `resolve → edit files → mutate` flow is supported. An
  `approved` record becomes `no_active_authorization` for subsequent
  `/__worklist/resolve` reads after the first GET, but `/__worklist/mutate`
  still uses the stored auth record from that turn.
- **Side effect: inflight sentinel.** `/__worklist/resolve` writes
  `resources/.inflight-claim.json` (with `kind` matching the auth
  record) as part of serving an `approved` or `drop` record;
  `/__worklist/mutate` clears the file as part of a successful
  advance or prune. Both writes emit the `inflight-claim-changed`
  Tauri event so iframe subscribers re-fetch `/__inflight`. See
  section 11 for the full mechanism.

### 3a. Codex filesystem lifecycle channel (#130)

Codex's sandbox refuses loopback connections (`curl: (7)` even when Bram
listens), so Codex drives the lifecycle through files instead of the HTTP
routes above. The host watches the intent file and dispatches it through the
*same* handlers (`handle_worklist_resolve`, `handle_worklist_mutate`,
`handle_worklist_commit`, `handle_worklist_end`, `handle_issue_close`), so all
side effects and auth checks are identical.

| Surface | Type | Shape |
|---|---|---|
| `resources/.worklist-intent.json` | file (agent writes) | `{ nonce, route, body? }` — `route` ∈ `worklist-resolve` \| `worklist-mutate` \| `worklist-commit` \| `worklist-end` \| `issue-close`; `body` is the matching HTTP route's request body |
| `resources/.worklist-result.json` | file (host writes) | `{ nonce, ok, status, result? , error?, completedAtMs }` — `result` is the HTTP route's response body verbatim; `error` present when `ok:false` |

- The watcher drain reads-then-deletes the intent file (so duplicate notify
  events in one burst no-op), writes the result atomically (`.tmp` + rename),
  and traces `[worklist-intent] route=… nonce=… ok=… status=…`.
- Startup deletes any stale intent/result files
  (`cleanup_stale_worklist_intent`) so a leftover result can't be misread as a
  reply to a fresh intent — the agent must match `nonce`.
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
| `/__sessions/search` | HTTP GET | `provider=`, `q=`, `scope=recent\|all` | `[{ id, title, hits: [{ line, snippet }] }, …]` | agent pane iframe |
| `/__sessions/delete` | HTTP GET | `provider=`, `id=` | `{ ok: true }` | agent pane iframe |
| `/__sessions/rename` | HTTP GET | `provider=`, `id=`, `title=` | `{ ok: true }` | agent pane iframe |
| `/__turns` | HTTP GET | `provider=claude\|codex` (optional), `id=<session-id>` (optional; by-id fetch), `agent=<agentId>` (optional; subagent transcript), `latest=N` (optional; tail window for live refreshes) | `{ sid, provider, total, windowStart, turns: [...] }` — `total` is the full turn count; `windowStart` is the index of the first returned turn (0 for full fetches); with `latest=N` only the last N turns are returned so the iframe can splice the window onto accumulated history and detect rotation/shrink. | agent pane iframe (Transcript, Sessions) |
| `/__send-ledger` | HTTP GET | — | `{ entries: [...], nowMs, staleTerminalInput, awaitingTurn }` — snapshot of the outbound-send ledger. `entries` carry `id/kind/state/cause/viaQueue/mode/preview/injectedAtMs/resolvedAtMs/retried`. `staleTerminalInput` = an Esc-aborted send's copy is known to sit in the terminal input; `awaitingTurn` = a pane-submitted turn is in flight (drives the composer gate). | agent pane iframe |
| `/__current-turn-edits` | HTTP GET | — | `{ added, removed, filePath, kind, lastToolId }` for the current turn's edit aggregates. Same derive-at-the-boundary pattern: host parses the 64 KB tail once per request, iframe binds via DataSource instead of running `currentTurnEdits(lastJsonl)` on every fanout. | agent pane iframe (Workspace turn-edits hint) |
| `/__waiting-for-assistant` | HTTP GET | — | `{ waiting: bool }` — true when the most recent meaningful JSONL record is a user message (`tool_result`-only records skipped). Mirror of the iframe `isWaitingForAssistant` helper. | agent pane iframe |
| `/__tool-detail` | HTTP GET | `id=<toolId>` | `{ input, result }` or `null` for a single tool by id. Mirrors `getToolDetail(jsonlText, toolId)`. | agent pane iframe (Workspace in-flight edit detail / compatibility consumers) |
| `/__describe-command` | HTTP POST | body `{ id, command, description? }` | `{ ok: true, description, cached? }` on success; `{ ok: false, reason: "disabled"\|"no-key"\|... }` when the opt-in gates are closed or the API call fails. Synthesizes (or upgrades) a one-line intent header for a tool expansion via the Anthropic Messages API (Haiku). On by default (`ai.describeCommands`, the Settings tab's "Tool Descriptions" switch — explicit `false` disables); the effective gate is `ANTHROPIC_API_KEY` in the host environment — no key, no calls. Results are cached in memory (by tool_use id and by command) and overlaid onto `/__turns` tool entries at serve time. Trace family: `[ai-describe]`. | agent pane iframe (Transcript tool expansion) |

- Provider directories: `~/.claude/projects/<encoded-cwd>/` for Claude
  Code (`claude_sessions_dir` at `lib.rs:1942`),
  `~/.codex/sessions/...` for Codex (`discover_codex_sessions` at
  `lib.rs:2224`). The encoding is the absolute project path with `/`
  → `-`.

## 6. Git & repo

Read-only browsing of git state plus the lone IPC mutation (`git_push`).
The HTTP routes shell out to `git`; the IPC command shells out to
`git push` and surfaces the result via a notification channel.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__commits` | HTTP GET | — | `[{ sha, summary, body, author, time }, …]` (HEAD ↓) | agent pane iframe |
| `/__commits/search` | HTTP GET | `q=` | filtered commit list | agent pane iframe |
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
JSON; write endpoints (`/__issue/comment`, `/__issue/close`) shell out
to `gh issue comment` / `gh issue close` on the host. `/__issue/close`
also has a close-on-commit mode (`commit=`/`push=`) that verifies a
commit is visible on GitHub — pushing first when asked — before closing
with a generated commit-URL comment; see the table note below. Issue
*creation* is still user-driven via the agent's own shell — there's no
`/__issue/create` endpoint.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__issues` | HTTP GET | — | `[{ number, title, state, … }, …]` | agent pane iframe |
| `/__issues/search` | HTTP GET | `q=` | filtered issue list | agent pane iframe |
| `/__issue` | HTTP GET | `n=<number>` | `{ number, title, body, state, comments: [...] }` | agent pane iframe |
| `/__issue/comment` | HTTP GET | `number=<n>&body=<urlencoded>` | `gh issue comment` JSON on success, 400 if `number` missing | agent pane iframe |
| `/__issue/close` | HTTP GET | plain: `number=<n>&comment=<urlencoded>`; close-on-commit: `number=<n>&commit=<sha>[&push=true]` | plain: `gh issue close` JSON. Close-on-commit: verifies the commit is visible on GitHub (pushing first when `push=true`), then closes with a generated commit-link comment that includes `Closed by https://github.com/<owner>/<repo>/commit/<full-sha>` and, when available, `Commit: <subject>`; on refusal returns `{ok:false,code,...}` where `code` ∈ `commit-not-visible` \| `focused-push-failed` \| `no-github-remote` \| `invalid-commit` \| `commit-visibility-check-failed`. 400 if `number` missing | agent pane iframe |

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
- **Lifecycle by `kind`.**
  - `approved` — written as a side effect of `/__worklist/resolve`
    serving a `kind:"approved"` record; cleared by
    `/__worklist/mutate advance` covering every claimed id, with the
    host PTY / turn-end fallback able to clear a lingering claim if the
    cycle still needs to drain.
  - `drop` — written as a side effect of `/__worklist/resolve` serving a
    `kind:"drop"` record; cleared by `/__worklist/mutate prune`
    covering every claimed id, with the same host fallback.
  - `iterate` — written by the host on the `toTurn` write path when an
    `iterate:` prefix is detected; cleared by the same turn-finished
    detectors that clear `approved` and `drop` sentinels.
    `POST /__worklist/end` remains as the explicit manual unwind.
- **Coverage rule for clears.** `clear` operations are no-ops unless
  every id currently claimed is in the supplied ids. Partial coverage
  intentionally leaves the file in place — a stuck sentinel is the
  diagnostic for an incomplete agent contract.
- **No live-session timeout.** Stuck claims stay claimed until the
  matching end / mutate call arrives, or until Bram restart (the
  startup helper `cleanup_stale_inflight_claim` deletes any leftover
  sentinel and emits one final `inflight-claim-changed`). This is by
  design: a stuck spinner surfaces the failure case instead of hiding
  it.
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

## Drift policy

Code under `src-tauri/src/lib.rs` is authoritative. This catalog is the
announcement surface for backend APIs — update it whenever a route or
IPC command is added, renamed, removed, or has its response shape
changed. Approximate line ranges for orientation:

- HTTP routes: `lib.rs:4800–5600` (the `route_request` function).
- IPC commands: `lib.rs:1279–1880` (individual `#[tauri::command]`
  functions) and `lib.rs:5654` (the `tauri::generate_handler!`
  registration).
- Worklist authorization plumbing: `lib.rs:85–95` (record struct),
  `:4247–4400` (parser, recorder, reader).
