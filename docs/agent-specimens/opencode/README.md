# OpenCode specimens

What OpenCode's terminal agent actually draws and emits, captured to sit
beside the Cursor, Claude Code and Codex packs
(`docs/agent-specimens/`). Markers Bram anchors on come from captures like
these, never from guesses (`docs/developing-bram.md` §Capturing terminal
specimens).

- **CLI:** `opencode`, version `v2.1.4` (`2.1.4` in the footer)
- **Platform:** macOS (arm64), 2026-10-28
- **Capture:** `scripts/capture-terminal-specimen.py`, 120×40 pseudo-terminal,
  run in a scratch git repo, always with `--standalone` (a private server), so
  the user's background `opencode serve --service` and its plugins were not
  involved. Turns used the real config's free/Go models; no sign-in.
- **Redacted:** the scratch workspace path (`<workspace>`), the home
  directory (`~`), the user name inside munged paths. Throwaway session ids
  are kept. Redaction changes line widths, so alignment may be off. The
  substitution lands in both `.txt` and `.raw`; `.raw` is a redacted stream,
  not the byte-exact original.

There are two kinds of `.txt`. The first-pass files (`idle-composer`,
`first-run`, `dialog-*`, `resume-by-id`, `turn-with-shell`,
`write-outside-project`, `no-config-exit`) are ANSI-stripped **streams**, not rendered screens:
OpenCode's TUI repaints with cursor addressing, so a `.txt` holds every
repaint concatenated (and whitespace-collapsed fragments of earlier frames).
Read the end of the file for the final frame. The files added on 2026-10-28
(marked **rendered** below) were produced by `capture/render.py` from a named
snapshot and show the **final screen** at that moment; the two `exit-ctrl-c*`
files hold the plain text printed after the TUI exits. Each `.txt` has its
redacted stream in `.raw` (escape sequences intact, paths already
`<workspace>`); `.raw` is a redacted stream, not the byte-exact original.
`capture/` reproduces the new captures (see `capture/README.md`). The header
tab bar in the new screens shows earlier sessions of the same scratch folder as
truncated tabs.

## Screens

| File | Command | What it shows |
|---|---|---|
| `help.txt` | `opencode --help` | Flags: `--standalone`, `--server`, `--auto`, `--continue/-c`, `--session/-s <id>` ("or to create if it does not exist"), `--prompt`; subcommands incl. `run`, `session`, `service`, `serve`, `api`. |
| `no-config-exit.txt` | `opencode --standalone` with empty `XDG_CONFIG/DATA/STATE/CACHE_HOME` | **No stack trace and no exit in 2.1.4 `--standalone`**: it draws the normal composer (`Ask anything… "Fix a TODO in the codebase"`, `Build · GPT-6.1 Terra  OpenAI`, footer `shift+tab agents  ctrl+p commands  2.1.4`) plus `/update to install v2.2.0`. The stack trace in `docs/developing-bram.md` was seen without `--standalone` (background service path); not re-tried here to avoid touching the user's service. So there is no logged-out screen either way. |
| `first-run.txt` | `opencode --standalone` in a never-seen folder, real config | Same composer; no trust dialog, no onboarding. Footer path is the folder. |
| `idle-composer.txt` | `opencode --standalone` in the scratch repo | `┃ Ask anything… "Fix a TODO in the codebase"` / `Build · <model> <provider>` inside a `┃ … ╹▀▀▀` box; footer `<path>:main  shift+tab agents  ctrl+p commands  2.1.4`. |
| `turn-with-shell.txt` | `opencode --standalone --prompt "Run the shell command: echo hello-specimen"` | The working indicator: a `⠹` spinner with the tool line `• Shell echo command`, a `⬝⬝⬝⬝⬝■■■` bar, and `Build · <model> · 4.9s · 11.1 tok/s`. **No permission prompt** in the default config: a plain `echo` is allowed. See the `permission: {"*":"ask"}` rows for the prompt. |
| `write-outside-project.txt` | `--prompt "Create the file <outside the project> … using the write tool"` | Attempt to raise a permission prompt. **It did not prompt**: the write outside the repo succeeded (`Wrote …`). See Not yet captured. |
| `dialog-command-palette.txt` | `Ctrl+P` | `Commands  esc  Search`, groups `Suggested` (`Switch model ctrl+x m`, `Open settings`) and `Session` (`Switch session ctrl+x l`, `New session ctrl+x n`, `Clear session`, `Open session or project ctrl+o`, `Close tab ctrl+x w`, `Reopen closed tab`, `Open editor ctrl+x e`, `Manage workspaces`, `Ask a side question`). |
| `dialog-session-list.txt` | `Ctrl+X l` | `Sessions for repo  esc  Search`, `delete ctrl+d  rename ctrl+r  all projects ctrl+a` (empty on the first capture: `No sessions available`; the final capture ran after sessions existed). |
| `dialog-model-list.txt` | `Ctrl+X m` | The model picker. |
| `turn-with-shell-working.txt` (**rendered**) | `permission: {"*":"ask"}` in `opencode.json`; prompt `Run the shell command: echo hello-specimen` | Turn start, before any tool: title tab `New session`, `⬝⬝⬝⬝⬝⬝⬝⬝ esc interrupt` under the composer. |
| `turn-with-shell-approval.txt` (**rendered**) | same, ~5 s later | The prompt: `△ Permission required` / `$ echo hello-specimen` / `Allow once   Always allow   Reject    ctrl+f fullscreen  ⇆ select  enter confirm`, with the tab marked `!`. Allow once is preselected. A tool outside shell, like `question`, offers only `Allow once   Reject` (`form-permission.txt`). |
| `turn-with-shell-declined.txt` (**rendered**) | Esc at the prompt | `The user declined this tool call` under `$ echo hello-specimen`, then `Build · <model> · 10.0s · … · interrupted`. One Esc at the prompt rejects. |
| `turn-with-shell-declined-selected.txt` (**rendered**) | Right, Right, Enter (Reject) | Same end state as Esc. |
| `turn-with-shell-completed.txt` (**rendered**) | Enter (Allow once) | `$ echo hello-specimen` / `hello-specimen`, then `Build · <model> · 8.3s · 12.8 tok/s` with no `interrupted`. |
| `interrupt-esc-once.txt`, `interrupt-esc-twice.txt` (**rendered**) | `sleep 20`, allowed, then Esc, Esc within 5 s | After the first Esc the bar reads `esc again to interrupt`; after the second: `Tool execution interrupted` and `Build · … · interrupted`. |
| `permission-denied-by-config.txt` (**rendered**) | config denies `shell` on `echo *` | No prompt; `Permission denied: shell` under `$ echo hello-specimen`. |
| `form-permission.txt`, `form-question.txt`, `form-answered.txt` (**rendered**) | prompt asking for the `question` tool | The `question` tool first asks permission (`Allow once   Reject`); then the form `Questions` / `1. Red  2. Blue  3. Type your own answer` / `↑↓ select  enter submit  esc dismiss`; after Enter the answer `Red` is shown. |
| `exit-ctrl-c.txt`, `exit-ctrl-c-idle.txt` | Ctrl-C | A single Ctrl-C exits at once, from the composer, with no confirmation. After a turn it prints `Session   <title>` and `Continue  opencode -s ses_…` (the resume hint). With no session yet (idle composer) it prints nothing. |
| `turn-with-shell-denied-by-guard.txt` (**rendered**) | scratch plugin throws `MARKER-REASON-7731` from `tool.execute.before` for a shell call containing `DENYME`; prompt asks the model to quote the refusal | `$ echo DENYME` with the thrown message as the tool output, in the tool box, and no command run; the model: `The refusal message I received, verbatim, is: MARKER-REASON-7731`. No permission prompt, no "blocked" decoration: the thrown message is the whole text. |
| `pasted-image-typed.txt`, `pasted-image-answer.txt` (**rendered**) | `What colour is this image? @<abs path>/red64.png ` typed, then Enter | The composer holds the typed `@/…/red64.png` as plain text (no chip, no attachment indicator). The model (`DeepSeek V4 Pro`, `OpenCode Go`) said it cannot view images, so it decoded the PNG with Python and answered `The image is pure red`. |
| `turn-with-loopback-curl.txt` (**rendered**) | `Run the shell command: curl -sS http://127.0.0.1:<port>/` against a scratch `python3 -m http.server` | Allowed with no prompt (default config): `$ curl -sS http://127.0.0.1:53775/` / `hello-from-listener`. The listener logged the request (`loopback-listener.log`). |
| `subagent-working.txt`, `subagent-done.txt` (**rendered**) | `Use the task tool to start a subagent that lists the files in this directory…` | A row `⠋ Explore Subagent — List files in directory` (`✓` when done), footer `↓ 1 subagent · $0.00`. No extra tab opens; the child's tool calls are not drawn in the parent's transcript. |
| `resume-by-id.txt` | `opencode --standalone --session ses_e7a5b72c3ffeMcdT467AVCgZDo` | Replays the prior conversation (`• Shell echo command`, `• Create …`) and lands on the composer. |
| `session-list.txt` | `opencode session list` | `<ses_id>\t<title>\t<date>`, newest first, current project only. |

## When the session id appears, and resume

There is **no session until the first message**. `session.created` fires
when the prompt is submitted (`hooks/session.created.json`: `data.sessionID`
`ses_…`, `slug` like `quick-squid`, `agent`, `model`, `version`), then
`session.execution.started`. The title is generated afterwards (`session.renamed`
in `hooks/` is not saved; it appears in the all-events log as "other").

Resume by id: `opencode --session <id>` (or `-s`); it creates the session if
the id does not exist. `--continue`/`-c` resumes the last one. Both
`resume-by-id.txt` and `session-list.txt` show it working.

## Session store (`transcript-record.json`)

SQLite at `~/.local/share/opencode/opencode.db` (not files). Tables of
interest: `session` / `session_v2` (id, `directory`, `title`, `agent`,
`model` JSON, tokens), `session_message` (`id`, `session_id`, `type`
`user|assistant`, `seq`, `time_created`, `data` JSON), `part`, `event`.
`session-meta-record.json` is a `session_v2` row (the `session` table had no rows for these sessions; this version writes
`session_v2`): `slug`, `directory`, `title`,
`agent`, `model` (a JSON string), `cost`, token counts, and `idle_outcome`
(`succeeded` after a normal turn, `interrupted` after a double Esc, empty for a
rejected prompt, a form, and a session that never finished), with `time_idle`.
`project_id` and `directory` are redacted.
`transcript-record.json` is the user row for `echo hello-specimen`; its
`data` holds `{time, text, files: [], agents: []}`. Assistant rows carry
`content[]` parts (`reasoning`, `tool`, `text`), `finish`, `cost`, `tokens`.
Sessions made with `--standalone` land in the same database as the
background service's.

## Plugin events (`hooks/`)

Bram's integration is the project plugin `.opencode/plugins/bram.js`
(source `app/__shell/opencode/bram-plugin.js`): `api.tool.hook("execute.before")`
for the guard, `execute.after` for the edit advisory, and
`api.event.subscribe()` forwarding the set below to Bram. `hooks/logger.js`
is a stand-in written for this pack (a scratch-repo project plugin, not
Bram's) that writes the first event of each type. Payloads:

| Payload | Notes |
|---|---|
| `session.created.json` | Top-level event: `id`, `created`, `type`, `durable.aggregateID`, `location.directory`, `data.sessionID`. A `parentID` marks a subagent. |
| `session.execution.started.json` | Turn start. Carries only `data.sessionID` (no directory; Bram learns it from `session.created`/`inbox`). |
| `session.inbox.delivered.json` | The user message was taken in (`data.inboxID` = `msg_…`). |
| `session.step.ended.json` | Per step: `finish` (`tool-calls`, `stop`), `cost`, `tokens`. |
| `session.execution.succeeded.json` | Turn end. |
| `session.execution.interrupted.user.json` | Double Esc mid-turn: `data.reason: "user"`. Preceded by `session.tool.failed` and `session.step.failed`, ~15 ms apart. |
| `session.execution.interrupted.after-reject.json` | Rejecting a permission prompt also ends in `session.execution.interrupted`, but with `data.reason: "shutdown"` (not `"user"`). Ctrl-C with a form open gave `"shutdown"` too. |
| `session.tool.failed.interrupted.json`, `.declined.json`, `.denied.json` | `data.error`: `{type:"aborted", message:"Tool execution interrupted"}` (double Esc), `{type:"aborted", message:"The user declined this tool call"}` (Reject), `{type:"permission.rejected", message:"Permission denied: shell"}` (config deny). `executed: false`. Not in Bram's forwarded set. |
| `session.step.failed.json` | `error: {type:"aborted", message:"Step interrupted"}`, plus `rawFinish`, `cost`, `tokens`. Not forwarded. |
| `permission.asked.shell.json` | `data`: `id` (`per_…`), `sessionID`, `action: "shell"`, `resources: ["echo hello-specimen"]`, `save: ["echo *"]` (the pattern "Always allow" would store), `source: {type:"tool", messageID, id}`. |
| `permission.asked.question.json` | `action: "question"`, `resources: ["*"]`, no `save` (no "Always allow"). |
| `permission.replied.once.json`, `permission.replied.reject.json` | `data: {sessionID, requestID, reply}`; `reply` is `"once"` or `"reject"` (`"always"` not captured). |
| `tool.execute.after.shell.denied.json` | A tool blocked by a configured rule: `status: "error"`, `error.error._tag: "Permission.BlockedError"`, the matching `rules`. `tool.execute.before` still fired and no `permission.asked` was emitted. |
| `form.created.json` | `data.form`: `id` (`frm_…`), `sessionID`, `title: "Questions"`, `metadata.kind: "question"`, `fields[]` (`key`, `title`, `description`, `type`, `options[]`, `custom`). |
| `form.replied.json`, `form.cancelled.json` | `form.replied`: `data: {id, sessionID, answer: {q0: "Red"}}`; `form.cancelled`: `data: {id, sessionID}` (here from Ctrl-C). |
| `tool.execute.before.shell.guard-denied.json`, `session.tool.failed.guard-denied.json` | The call the scratch plugin threw on, and the failure that follows: `data.error` is `{type:"unknown", message:"MARKER-REASON-7731"}` (the thrown `Error` message, verbatim), `executed: false`. **No `tool.execute.after` fired** for the denied call, and no `permission.asked`. The stored tool state is `status: "error"`, `error.message` the same (`transcript-record-guard-deny.json`), and the model's next step is a normal one; the turn ends `succeeded`. Event order: `guard-deny-event-order.txt`. |
| `tool.execute.after.read-image.json` | The `read` tool on a PNG returns `result.output` `{type:"file", uri:"file://…", mime:"image/png", encoding:"base64", content}` and `result.content` `[{type:"text","Image read successfully"},{type:"file", uri:"data:image/png;base64,…", mime, name}]`. |
| `session.created.child.json` | A subagent's session: `data.parentID` is the parent `ses_…`, `agent: "explore"`, `title` from the task description, `slug`. Its own `session.execution.started` / `succeeded` fire (the child's `succeeded` first). |
| `tool.execute.before.subagent.json` | The parent's call: `tool: "subagent"` (not `task`), `agent: "build"`, `input: {agent: "explore", description, prompt}`. Child tool calls (`read`, `shell`) pass the same plugin hooks with the child's `sessionID` and `agent: "explore"`. |
| `tool.execute.before.shell.json` | The guard's enforcement point: `tool: "shell"`, `input.command`, `sessionID`, `agent`, `messageID`, `id`. |
| `tool.execute.before.write.json` / `tool.execute.after.write.json` | `tool: "write"`, `input.path`, `input.content`; `after` adds `status` and `result.output` / `result.content`, which the edit advisory appends to. |
| `tool.execute.after.shell.json` | The shell result. |

## Not yet captured

- `session.execution.failed`: not reachable. A `model` set in `opencode.json`
  was ignored by the TUI (the stored model wins), a denied tool ends in
  `session.tool.failed` plus a normal `succeeded`, and no bad-credential or
  network failure was provoked. Bram's code handles it (lib.rs ~10795).
- `permission.replied` with `reply: "always"`.
- Subagent sessions are captured (see Seams); not tried: a subagent whose tool call the guard denies, or one asking for a permission.
- A permission-denied `write` (only `shell` denial was captured).
- The stack-trace exit with empty XDG dirs (see `no-config-exit.txt`).
- Naming through the TUI. Pasted images: only the typed `@<path>` form was captured; the picker-selected `@` (which may fill the user row's `files` array) and an image-capable model were not tried, and the model in use cannot see images.
- `opencode agent list`: there is no such subcommand in 2.1.4 (`opencode --help` lists none); the agent names (`build`, `explore`) were read from events.
- Loopback under the `permission: {"*":"ask"}` config (only the default config was run).
- Bram's transport is not captured, only cited: the plugin POSTs events to
  `http://127.0.0.1:<port>/__opencode/event` (`app/__shell/opencode/bram-plugin.js`
  `postEvent`; handler `src-tauri/src/lib.rs` ~86060), and `conventions.md`
  §Transports covers the curl routes the agent uses.

## Seams

| Seam | OpenCode fact | Specimen |
|---|---|---|
| launch and resume by id | `opencode [dir]`; `--session/-s <id>` (creates if absent), `--continue/-c` | `help.txt`, `resume-by-id.txt` |
| when the session id exists | Not until the first message is submitted: `session.created` carries `ses_…` | `hooks/session.created.json` |
| session store | SQLite `~/.local/share/opencode/opencode.db`, `session_message` rows with JSON `data` | `transcript-record.json` |
| naming | Auto title after the first turn; Bram renames by an SQL `UPDATE` on the DB; TUI rename is `ctrl+r` in the session list | Bram code: `src-tauri/src/lib.rs:32009`; `dialog-session-list.txt` |
| guard hook | Plugin `api.tool.hook("execute.before")`; throw to deny. The thrown `Error` message reaches the model verbatim as the tool's error (the model quoted `MARKER-REASON-7731`); `session.tool.failed` carries it as `error.message`, `type:"unknown"`; no `tool.execute.after`, no permission prompt | `turn-with-shell-denied-by-guard.txt`, `hooks/session.tool.failed.guard-denied.json`, `transcript-record-guard-deny.json`; Bram code: `app/__shell/opencode/bram-plugin.js` |
| turn state | Bus events `session.execution.started/succeeded/failed/interrupted`, forwarded by the plugin | `hooks/session.execution.started.json`, `hooks/session.execution.succeeded.json` |
| Agent→Bram channel | Plugin `fetch` POST to Bram loopback `/__opencode/event`. A loopback `curl` from the agent's shell tool succeeds with no prompt in the default config (and the listener sees it) | `turn-with-loopback-curl.txt`, `loopback-listener.log`; Bram code: `app/__shell/opencode/bram-plugin.js` (`postEvent`), `src-tauri/src/lib.rs:86060` |
| Bram→agent delivery | Typed into the PTY (no inbound API used) | not captured; Bram code: `src-tauri/src/lib.rs:44208` |
| pasted images | `@<abs path>` typed into the composer stays literal text in the user row (`files: []`). The model then calls `read` on the path; the tool returns a `file` part (`data:image/png;base64,…`). Whether pixels reach the model depends on the model: DeepSeek V4 Pro said it could not view images and inspected the bytes instead | `pasted-image-answer.txt`, `transcript-record-pasted-image.json`, `hooks/tool.execute.after.read-image.json` |
| subagents | The model calls a tool named `subagent` (`input: {agent, description, prompt}`); a child session is created in `session_v2` with `parent_id` set and `agent: "explore"`; the result text is `<subagent sessionID="ses_…" state="completed"…>`. The child's tool calls hit the plugin hooks; the TUI shows one row and a `↓ N subagent` footer, no tab | `subagent-working.txt`, `subagent-done.txt`, `session-meta-record-subagent.json`, `transcript-record-subagent-child.json`, `transcript-record-subagent-parent-tool.json`, `hooks/session.created.child.json`, `subagent-event-order.txt` |
| menus | Ctrl+P palette; `Ctrl+X m/l/a` lists; `esc` closes | `dialog-command-palette.txt`, `dialog-session-list.txt`, `dialog-model-list.txt` |
| working status | Spinner + `• Shell echo command` + `Build · <model> · <secs> · <tok/s>`; `⬝⬝⬝⬝⬝⬝⬝⬝ esc interrupt` while running; `session.execution.started` | `turn-with-shell.txt`, `turn-with-shell-working.txt` |
| interrupt and exit | First Esc: `esc again to interrupt`; second within 5 s stops the turn: `session.execution.interrupted` `reason: "user"`, after `session.tool.failed`/`session.step.failed`. Ctrl-C exits at once and prints `Continue  opencode -s <id>` (after a turn only). | `interrupt-esc-once.txt`, `interrupt-esc-twice.txt`, `exit-ctrl-c.txt`, `exit-ctrl-c-idle.txt`, `hooks/session.execution.interrupted.user.json` |
| approval prompt | `△ Permission required` with `Allow once / Always allow / Reject`; Esc rejects. Only with `permission: {"*":"ask"}` (default config runs `echo` unasked). `permission.asked` → `permission.replied` | `turn-with-shell-approval.txt`, `hooks/permission.asked.shell.json`, `hooks/permission.replied.once.json` |
| blocked tool | Reject: `session.tool.failed` (`aborted`, "The user declined this tool call") then `interrupted` (`reason: "shutdown"`). Config deny on a resource: `permission.rejected`, no prompt, turn still `succeeded` | `turn-with-shell-declined.txt`, `hooks/session.tool.failed.declined.json`, `hooks/session.tool.failed.denied.json`, `hooks/tool.execute.after.shell.denied.json` |
| question form | `question` tool: permission prompt, then `form.created` with `fields[]`; answered with `form.replied` | `form-question.txt`, `hooks/form.created.json`, `hooks/form.replied.json` |
| session metadata | `session_v2` row incl. `idle_outcome`, `slug`, `model` JSON | `session-meta-record.json` |
| reproducible capture | driver, renderer, schedules, configs, logger plugin | `capture/` |
| logged-out screen | None; with empty XDG dirs `--standalone` draws the normal composer | `no-config-exit.txt` |
| instructions file | `AGENTS.md`, `opencode.json(c)`, project plugin `.opencode/plugins/` | Bram code: `src-tauri/src/lib.rs:52615` |
| screen shapes | Boxed `┃ Ask anything…` composer, footer `<path>:<branch>  shift+tab agents  ctrl+p commands  <version>`; no trust dialog | `idle-composer.txt`, `first-run.txt` |
| model and version | `Build · <model> <provider>` row in the composer; version at footer right; `session.created` has `model`, `version` | `idle-composer.txt`, `hooks/session.created.json` |

## Review revisions

Signed: OpenCode (main thread), 2026-10-28. Corrected the description of the
`.raw` files: they are redacted streams (the scratch path is `<workspace>`),
not byte-exact originals — the same correction Codex made to its own pack.
Also noted that `strip_ansi` removes OSC/CSI but keeps a short kitty/iTerm2
graphics preamble (`ESC P+q… ESC \` / `ESC _G…`) at the head of several
`.txt` files. Original captures are Claude's; these revisions did not run new
model turns.

## Alignment — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
Aligned this pack to Codex's set (`resources/worklist-drafts/agent-specimens-align-to-codex.md`).
Added the exit, turn-phase, interrupt, denied, form screens; the permission,
form, `tool.failed`/`step.failed` and interrupted payloads; `session-meta-record.json`;
and `capture/`. Captured with a scratch repo and `--standalone` (config:
`capture/config/`); the scratch sessions were deleted from the global
database afterwards. `hooks/logger.js` gained `session.tool.failed` and
`session.step.failed`. Findings: the config key is `permission` (legacy
shape, `"*": "ask"` works) or `permissions` (array of `{action, resource,
effect}`); denying the whole `shell` action hides the tool from the model
instead of blocking a call. ID allocation versus persistence: `ses_…` is
allocated at `session.created` and written to `session_v2` then, but the
title arrives later (`session.renamed`) and `idle_outcome` only at turn end.
Older first-pass files (`resume-by-id`, `turn-with-shell`,
`write-outside-project`) still contain a mangled scratch path; the new files
are redacted.

## Open items — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
Closed four of the open items in `resources/worklist-drafts/issue-462-specimen-open-items.md`
for OpenCode 2.1.4, model DeepSeek V4 Pro (`opencode-go`), `--standalone`, scratch repo, default permissions.

- **Guard deny.** The scratch logger (`hooks/logger.js`, now with
  `SPECIMEN_DENY_MARKER` / `SPECIMEN_DENY_MESSAGE`) throws from
  `tool.execute.before`, as Bram's plugin does. The model got the message
  verbatim and quoted it. Evidence: screen, `session.tool.failed`, stored
  tool `error`, `guard-deny-event-order.txt`.
- **Pasted image.** `@<path>` stays literal text; the model chose to `read`
  it; the `read` result is a `file` part with a base64 data URI. The model
  in use does not support images (it said so and decoded the PNG by hand), so
  "answers red" shows the file arrived, not that vision worked. Two runs were
  made; the first was cut off by the schedule before the answer.
- **Loopback.** Allowed, no prompt, listener logged `GET /`. Run only under
  the default config.
- **Subagents.** Exist (tool `subagent`, agent `explore`). Child session has
  `parent_id`; its `session.created` carries `parentID` (Bram's note in this
  README was right). Child `session.execution.succeeded` fires before the
  parent's. The child's tool calls fire the plugin hooks, so Bram's guard
  would judge them too, with `agent: "explore"` and the child's `sessionID`;
  the user row of the child starts `You are a subagent spawned by another
  session.`
- Not checked: how Bram's event forwarder treats a child session's
  `session.execution.*` events (this pack's logger does not run Bram's code).
