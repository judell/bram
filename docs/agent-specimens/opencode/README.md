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

The `.txt` files are ANSI-stripped **streams**, not rendered screens:
OpenCode's TUI repaints with cursor addressing, so a `.txt` holds every
repaint concatenated (and whitespace-collapsed fragments of earlier frames).
Read the end of the file for the final frame. Each has the redacted stream in
`.raw` (escape sequences intact, path already `<workspace>`).

## Screens

| File | Command | What it shows |
|---|---|---|
| `help.txt` | `opencode --help` | Flags: `--standalone`, `--server`, `--auto`, `--continue/-c`, `--session/-s <id>` ("or to create if it does not exist"), `--prompt`; subcommands incl. `run`, `session`, `service`, `serve`, `api`. |
| `no-config-exit.txt` | `opencode --standalone` with empty `XDG_CONFIG/DATA/STATE/CACHE_HOME` | **No stack trace and no exit in 2.1.4 `--standalone`**: it draws the normal composer (`Ask anything… "Fix a TODO in the codebase"`, `Build · GPT-6.1 Terra  OpenAI`, footer `shift+tab agents  ctrl+p commands  2.1.4`) plus `/update to install v2.2.0`. The stack trace in `docs/developing-bram.md` was seen without `--standalone` (background service path); not re-tried here to avoid touching the user's service. So there is no logged-out screen either way. |
| `first-run.txt` | `opencode --standalone` in a never-seen folder, real config | Same composer; no trust dialog, no onboarding. Footer path is the folder. |
| `idle-composer.txt` | `opencode --standalone` in the scratch repo | `┃ Ask anything… "Fix a TODO in the codebase"` / `Build · <model> <provider>` inside a `┃ … ╹▀▀▀` box; footer `<path>:main  shift+tab agents  ctrl+p commands  2.1.4`. |
| `turn-with-shell.txt` | `opencode --standalone --prompt "Run the shell command: echo hello-specimen"` | The working indicator: a `⠹` spinner with the tool line `• Shell echo command`, a `⬝⬝⬝⬝⬝■■■` bar, and `Build · <model> · 4.9s · 11.1 tok/s`. **No permission prompt**: a plain `echo` is allowed by default. |
| `write-outside-project.txt` | `--prompt "Create the file <outside the project> … using the write tool"` | Attempt to raise a permission prompt. **It did not prompt**: the write outside the repo succeeded (`Wrote …`). See Not yet captured. |
| `dialog-command-palette.txt` | `Ctrl+P` | `Commands  esc  Search`, groups `Suggested` (`Switch model ctrl+x m`, `Open settings`) and `Session` (`Switch session ctrl+x l`, `New session ctrl+x n`, `Clear session`, `Open session or project ctrl+o`, `Close tab ctrl+x w`, `Reopen closed tab`, `Open editor ctrl+x e`, `Manage workspaces`, `Ask a side question`). |
| `dialog-session-list.txt` | `Ctrl+X l` | `Sessions for repo  esc  Search`, `delete ctrl+d  rename ctrl+r  all projects ctrl+a` (empty on the first capture: `No sessions available`; the final capture ran after sessions existed). |
| `dialog-model-list.txt` | `Ctrl+X m` | The model picker. |
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
| `session.execution.succeeded.json` | Turn end. (`failed` and `interrupted` not captured.) |
| `tool.execute.before.shell.json` | The guard's enforcement point: `tool: "shell"`, `input.command`, `sessionID`, `agent`, `messageID`, `id`. |
| `tool.execute.before.write.json` / `tool.execute.after.write.json` | `tool: "write"`, `input.path`, `input.content`; `after` adds `status` and `result.output` / `result.content`, which the edit advisory appends to. |
| `tool.execute.after.shell.json` | The shell result. |

## Not yet captured

- `permission.asked` / `permission.replied`, `form.*`: nothing prompted in
  this config (a `write` outside the repo and a shell `echo` both ran
  without asking). Bram's verbatim `permission.asked` payload and screens are
  in `src-tauri/src/lib.rs` (`opencode_permission_menu_tests`, ~line 9559;
  `opencode_prompt_tests`, ~line 47250).
- `session.execution.failed` / `interrupted`, and the double-Esc interrupt
  (Bram code: lib.rs ~10795).
- The stack-trace exit with empty XDG dirs (see `no-config-exit.txt`).
- Pasted images, naming through the TUI.
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
| guard hook | Plugin `api.tool.hook("execute.before")`; throw to deny | `hooks/tool.execute.before.shell.json`; Bram code: `app/__shell/opencode/bram-plugin.js` |
| turn state | Bus events `session.execution.started/succeeded/failed/interrupted`, forwarded by the plugin | `hooks/session.execution.started.json`, `hooks/session.execution.succeeded.json` |
| Agent→Bram channel | Plugin `fetch` POST to Bram loopback `/__opencode/event` | Bram code: `app/__shell/opencode/bram-plugin.js` (`postEvent`), `src-tauri/src/lib.rs:86060` |
| Bram→agent delivery | Typed into the PTY (no inbound API used) | not captured; Bram code: `src-tauri/src/lib.rs:44208` |
| pasted images | not captured | not captured |
| menus | Ctrl+P palette; `Ctrl+X m/l/a` lists; `esc` closes | `dialog-command-palette.txt`, `dialog-session-list.txt`, `dialog-model-list.txt` |
| working status | Spinner + `• Shell echo command` + `Build · <model> · <secs> · <tok/s>`; `session.execution.started` | `turn-with-shell.txt` |
| interrupt and exit | Second Esc within 5 s stops a turn, reported as `session.execution.interrupted` | Bram code: `src-tauri/src/lib.rs:10795` |
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
