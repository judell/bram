# Backend APIs

Inventory of the backend surfaces produced by `src-tauri/src/lib.rs`.
Bram hosts two iframes inside the parent shell — the **right
pane** (the project under development; any web app served by the
project's HTTP server) and the **agent-tools drawer** (Bram's
own XMLUI control surface at `app/tools/`, providing Transcript,
Worklist, Commits, Issues, Sessions, Context, README tabs). Two
transport channels carry traffic between the Rust host and everything
else (those two iframes, the parent shell, and the agent running in the
terminal):

- **HTTP loopback.** A `tiny_http` server bound to `127.0.0.1:<random-port>`
  serves the agent directly (via `curl`) and serves both iframes
  *indirectly* through the `tauri://localhost` custom-scheme handler.
  Every route lives under the `__<name>` prefix (and a small set of
  static fallbacks). When an iframe fetches
  `tauri://localhost/__worklist`, the scheme handler's loopback tier
  proxies the request to `http://127.0.0.1:<port>/__worklist` and
  returns the body. The two call sites (`curl` from the agent, `fetch`
  from an iframe) hit the same handlers with different base URLs.
- **Tauri IPC.** `#[tauri::command]` functions registered in
  `tauri::generate_handler!` at the bottom of `lib.rs`. Reachable from
  any window owned by this app process. Because both iframes are
  same-origin with the parent shell at `tauri://localhost`, they call
  IPC directly via `window.parent.__TAURI__.core.invoke(...)`;
  `app/__shell/helpers.js:getTauriInvoke` formalizes a `window.__TAURI__`
  → `window.parent.__TAURI__` → `window.top.__TAURI__` fallback chain.
  The `postMessage` bridge to `app/main.js` has been retired except for
  voice (`voice-start` / `voice-stop`), which still routes through the
  parent because the parent shell owns the MediaRecorder pipeline. The
  agent itself cannot call IPC — it has no Tauri runtime.

The right-pane iframe URL is provisioned via the IPC command
`get_right_pane_url`, which returns the `tauri://localhost/__project/...`
form (the scheme handler routes `/__project/*` to the project's HTTP
server). The loopback port is not exposed to the iframes — they only
see the `tauri://` origin. There is no auth on either channel beyond
loopback / process scope.

When a route or command is added or removed, update this catalog. Code is
the source of truth; this is the announcement surface.

## Sections

| # | Section | What it covers | Primary consumers |
| --- | --- | --- | --- |
| 1 | [App & shell meta](#1-app--shell-meta) | Version banner, right-pane info, restart, error reporting, PTY views and control | parent shell, both iframes |
| 2 | [Setup (agent coordination)](#2-setup-agent-coordination) | Per-repo installer of the shared worklist core + per-agent adapters | agent-tools iframe |
| 3 | [Worklist & authorization](#3-worklist--authorization) | Pending items + verified `approved:` / `drop:` records | agent-tools iframe, agent (curl) |
| 4 | [Worklist history](#4-worklist-history) | Reverse-chronological archive of worklist transitions | agent-tools iframe |
| 5 | [Sessions](#5-sessions) | Claude / Codex JSONL session enumeration, content, search | agent-tools iframe |
| 6 | [Git & repo](#6-git--repo) | Commits, diffs, file reads, origin, push | agent-tools iframe, parent shell |
| 7 | [Issues](#7-issues) | GitHub passthrough via `gh` | agent-tools iframe |
| 8 | [Context](#8-context) | `CLAUDE.md` / `AGENTS.md` import chain + memory + hooks + settings | agent-tools iframe |
| 9 | [Voice / transcription](#9-voice--transcription) | Whisper subprocess lifecycle | parent shell |
| 10 | [Static & hot-reload](#10-static--hot-reload) | Files served from disk or embedded; iframe reload coupling | both iframes |

## 1. App & shell meta

App-wide version, screen, and process information; PTY echo views and
write/resize control. The parent shell uses these to render the
update-available banner and to drive the terminal; the agent-tools iframe
uses them for the right-pane info dialog.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__app-info` | HTTP GET | — | `{ current, latest, has_update, release_url }` | parent shell, agent-tools iframe |
| `/__right-pane-info` | HTTP GET | — | `{ url, default_right_pane, spawned? }` | agent-tools iframe |
| `/__restart-server` | HTTP GET | — | empty / 200 on success | agent-tools iframe |
| `/__error` | HTTP GET | — | reported error context | agent-tools iframe |
| `/__pty-tail` | HTTP GET | `lines=` | last N lines of PTY output, `text/plain` | agent-tools iframe |
| `/__pty-stripped` | HTTP GET | — | PTY output with ANSI escapes removed, `text/plain` | agent-tools iframe |
| `/__pty-menu` | HTTP GET | — | current permission menu (if any), JSON | agent-tools iframe |
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
| `/__enhance/status` | HTTP GET | — | `{ enhanced, claudeMd, sidecarExists, hookScriptExists, hookRegistered, … }` | agent-tools iframe |
| `/__enhance/run` | HTTP GET | — | `{ enhanced: true, wrote: [<path>, …] }` | agent-tools iframe |
| `/__enhance/codex-trust-ack` | HTTP GET | — | `{ ok: true }` (emits `enhance-status-changed` Tauri event) | agent-tools iframe |

## 3. Worklist & authorization

The pending-worklist surface plus the verified-authorization endpoint that
agents read after an `approved:` / `drop:` payload arrives. Per-item
`hash` is computed server-side (SipHash via `DefaultHasher` over the
canonical JSON serialization) and travels with each item — the UI
propagates it back into the structured payload so the watcher can verify
without re-shipping content.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__worklist` | HTTP GET | — | `{ description, items: [{ id, status, file(s), before, after, hash, diff? }], exists, resourcesExists, path }` | agent-tools iframe |
| `/__worklist/init` | HTTP GET | — | same shape as `/__worklist` (file created if missing) | agent-tools iframe |
| `/__worklist/resolve` | HTTP GET | `ids=foo,bar` | active: `{ kind, ids, items, mismatchedIds, issuedAtMs, source, consumedAtMs }` · consumed: `{ kind: "no_active_authorization", consumedAtMs }` | agent (curl) |
| `/__worklist/mutate` | HTTP POST | body `{ op: "prune" \| "advance", ids: [...], status?: "applied" }` | `{ ok: true, pruned: [...] }` / `{ ok: true, advanced: [...] }`, or 400 `{ error: "…" }` on auth-kind mismatch | agent (curl) |

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
- A same-turn `resolve → edit files → mutate` flow is supported. An
  `approved` record becomes `no_active_authorization` for subsequent
  `/__worklist/resolve` reads after the first GET, but `/__worklist/mutate`
  still uses the stored auth record from that turn.

## 4. Worklist history

Reverse-chronological archive of every worklist transition. Snapshots
live under `resources/worklist-history/<ts_ms>.{json,md}` — JSON is the
worklist state at that moment; Markdown is the changelog narrative
(`Items proposed`, `Items applied`, `Items committed`, `Items dropped`,
`Description changed`).

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__worklist-history/list` | HTTP GET | — | `[{ ts, iso, summary, ids, changelog }, …]` (newest first) | agent-tools iframe |
| `/__worklist-history/changelog` | HTTP GET | `ts=<ms>` | raw `.md` body, `text/markdown` | agent-tools iframe |
| `/__worklist-history/snapshot` | HTTP GET | `ts=<ms>` | raw `.json` body | agent-tools iframe |

- The list endpoint parses item ids out of changelog bullet lines
  (`` - `<id>` (was …) ``, `` - `<id>` (proposed, …) ``,
  `` - `<id>`: proposed → applied ``) for the `ids` field. When a snapshot
  records no item transitions (e.g. a description-only edit), the
  endpoint falls back to reading the `.json` sibling and surfacing the
  ids present at that moment, and the summary becomes
  `"description changed"` instead of the generic `"change"`.

## 5. Sessions

Provider-aware enumeration of Claude Code / Codex JSONL session files
plus content / search / delete / rename. Same route shape for both
providers, switched by the `provider=` query.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__sessions/meta` | HTTP GET | `provider=` | `{ count, latest_mtime, … }` | agent-tools iframe |
| `/__sessions/list` | HTTP GET | `provider=` | `[{ id, mtime, title, … }, …]` | agent-tools iframe |
| `/__sessions/latest` | HTTP GET | `provider=` | full JSONL body, `text/plain` | agent-tools iframe |
| `/__sessions/latest-meta` | HTTP GET | `provider=` | `{ size, mtime, id }` | agent-tools iframe |
| `/__sessions/latest-pending` | HTTP GET | `provider=` | pending tool-use record, JSON | agent-tools iframe |
| `/__sessions/latest-tail` | HTTP GET | `provider=`, `lines=N\|all` | last N JSONL records (default 200), `text/plain` | agent-tools iframe |
| `/__sessions/content` | HTTP GET | `provider=`, `id=` | full JSONL body for that session, `text/plain` | agent-tools iframe |
| `/__sessions/search` | HTTP GET | `provider=`, `q=`, `scope=recent\|all` | `[{ id, title, hits: [{ line, snippet }] }, …]` | agent-tools iframe |
| `/__sessions/delete` | HTTP GET | `provider=`, `id=` | `{ ok: true }` | agent-tools iframe |
| `/__sessions/rename` | HTTP GET | `provider=`, `id=`, `title=` | `{ ok: true }` | agent-tools iframe |

- Provider directories: `~/.claude/projects/<encoded-cwd>/` for Claude
  Code (`claude_sessions_dir` at `lib.rs:1942`),
  `~/.codex/sessions/...` for Codex (`discover_codex_sessions` at
  `lib.rs:2224`). The encoding is the absolute project path with `/`
  → `-`.
- `latest-tail`'s `lines=` default is 200 to avoid accidental multi-MB
  fetches when the query parameter is missed; `lines=all` is the only
  way to ask for the full file.

## 6. Git & repo

Read-only browsing of git state plus the lone IPC mutation (`git_push`).
The HTTP routes shell out to `git`; the IPC command shells out to
`git push` and surfaces the result via a notification channel.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__commits` | HTTP GET | — | `[{ sha, summary, body, author, time }, …]` (HEAD ↓) | agent-tools iframe |
| `/__commits/search` | HTTP GET | `q=` | filtered commit list | agent-tools iframe |
| `/__commit` | HTTP GET | `sha=` | `{ sha, summary, body, diff }` | agent-tools iframe |
| `/__repo/origin` | HTTP GET | — | `{ remote, owner, name }` | agent-tools iframe |
| `/__git-diff` | HTTP GET | `path=` | `git diff -- <path>`, `text/plain` | agent-tools iframe |
| `/__file` | HTTP GET | `path=` | file body, `text/plain` | agent-tools iframe |
| `git_push` | IPC | — | `Result<(), String>` | iframe helpers (direct) |

## 7. Issues

GitHub issue passthrough via the local `gh` CLI. Read endpoints fetch
JSON; write endpoints (`/__issue/comment`, `/__issue/close`) shell out
to `gh issue comment` / `gh issue close` on the host. Issue *creation*
is still user-driven via the agent's own shell — there's no
`/__issue/create` endpoint.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__issues` | HTTP GET | — | `[{ number, title, state, … }, …]` | agent-tools iframe |
| `/__issues/search` | HTTP GET | `q=` | filtered issue list | agent-tools iframe |
| `/__issue` | HTTP GET | `n=<number>` | `{ number, title, body, state, comments: [...] }` | agent-tools iframe |
| `/__issue/comment` | HTTP GET | `number=<n>&body=<urlencoded>` | `gh issue comment` JSON on success, 400 if `number` missing | agent-tools iframe |
| `/__issue/close` | HTTP GET | `number=<n>&comment=<urlencoded>` | `gh issue close` JSON on success, 400 if `number` missing | agent-tools iframe |

## 8. Context

Per-provider catalog of agent-coordination files: `CLAUDE.md` / `AGENTS.md`
import chain, agent-managed memory, hooks, and settings. Drives the
Context tab in the agent-tools drawer.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `/__context/list` | HTTP GET | `provider=` | `{ provider, summary, sections: [{ key, label, items: [{ path, display, kind }] }] }` | agent-tools iframe |
| `/__context/search` | HTTP GET | `provider=`, `q=` | `{ results: [{ path, display, category, hits: [{ line, snippet }] }] }` (≤ 50 hits) | agent-tools iframe |
| `/__context/file` | HTTP GET | `path=` | file body, `text/plain` | agent-tools iframe |

## 9. Voice / transcription

Whisper subprocess lifecycle. The parent shell auto-starts the server
on first record click; the IPC commands are also the only way to stop
or query state. No HTTP surface — voice is parent-shell-only.

| Surface | Kind | Query / params | Response | Consumer |
| --- | --- | --- | --- | --- |
| `whisper_start` | IPC | `{ modelPath }` | `Result<(), String>` | parent shell |
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
| `/__tools/<path>` | HTTP GET | — | agent-tools drawer XMLUI sources | agent-tools iframe |
| `/resources/worklist.json` | HTTP GET | — | file body, or `{description:"", items:[]}` if missing | agent-tools iframe |

- `app/__shell/` and `app/vendor/` changes trigger reload in both
  iframes; `app/tools/` changes reload only the agent-tools iframe; the
  user's project directory triggers a right-pane reload only. The
  parent shell (`app/index.html`, `app/main.js`) is not hot-reloaded —
  changes there require `cargo build` and a restart.
- `/resources/worklist.json` returns the empty-worklist JSON instead of
  `404` when the file doesn't exist yet, so the Workspace tab's polling
  loop doesn't flood devtools with 404s in guest projects that haven't
  opted in to the worklist flow.

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
