# Cursor CLI specimens

What Cursor's terminal agent actually draws and emits, captured for the
provider feasibility discussion in
[#462](https://github.com/judell/bram/issues/462). Markers Bram anchors on
come from captures like these, never from guesses
(`docs/developing-bram.md` §Capturing terminal specimens).

- **CLI:** `agent` (also installed as `cursor-agent`), version
  `2026.10.22-27dcf5f`; model `Auto` (`model: "default"` in every hook payload)
- **Platform:** macOS (arm64), 2026-10-28
- **Capture:** `scripts/capture-terminal-specimen.py` for the first pass
  (`logged-out`, `first-run`, `idle-composer`, `turn-with-shell`, `create-chat*`);
  the 2026-10-28 alignment pass used the recovered Codex-style driver (same
  pty, 120x40, scheduled keystrokes and snapshots), now in
  [`capture/`](capture/README.md). Everything ran in scratch git repos
  outside the Bram repo, with hooks in the scratch repo's `.cursor/hooks.json`.
- **Redacted:** the account email (`<user-email>`), the scratch workspace path
  (`<workspace>`), its munged form under `~/.cursor/projects`
  (`<munged-workspace>`), and the home directory (`~`). Conversation ids are
  throwaway and kept. Replacing a path changes line widths, so box borders in
  some `.txt` screens may not line up. The `.raw` files are **redacted
  terminal streams**: escape sequences remain but path bytes are replaced, so
  they are not byte-exact originals and replaying one need not reproduce its
  `.txt`. The `.txt` files are the final screen rendered through a minimal VT
  emulator (Cursor redraws with relative cursor moves) and then redacted.
- **Account:** the real, logged-in Cursor account for every model turn.
  `logged-out` came from the first pass.

## Screens

| File | How | What it shows |
|---|---|---|
| `logged-out` | `agent` while logged out | Full-screen splash: `Cursor Agent` / version / `Press any key to log in...`. **Any key starts login**, so pane sends must be held. |
| `first-run` | `agent` in a new folder | Boxed `⚠ Workspace Trust Required`, `▶ [a] Trust this workspace` / `[q] Quit`, "Enter to select". **Enter trusts the folder**, so sends must be held. `agent --trust` skips it and writes `.workspace-trusted` (`trustMethod: "cli-flag"`) under `~/.cursor/projects/<munged-workspace>/`. |
| `idle-composer` | `agent`, then `a` | The composer is ready: `→ Plan, search, build anything`, with the model (`Auto`) and `<path> · <branch>` in the footer. |
| `turn-with-shell` | `agent "Run the shell command: echo hello-specimen"` | The earlier combined stream: working line, then the permission prompt. Kept as it was. |
| `turn-with-shell-working` | prompt, Enter, `y` | Working line `⠠⠛ Running  81 tokens` (before the tool starts: `Thinking  1 token`; with no tool, `Working`), the tool line `$ <command> 5.1s`, the composer as `→ Add a follow-up … ctrl+c to stop`, footer `1 task` / `Auto · 4.7%`. |
| `turn-with-shell-approval` | same prompt, hook answering `ask` | Tool line ends `Waiting for approval...`, then a rule and `$ <command> in .` / `Run this command?` / `Hook requested approval • Not in allowlist: echo` / `→ Run (once) (y)`, `Run Everything (shift+tab)`, `Skip & tell the agent what to do instead (esc or n)`. **Enter or `y` runs the command.** The allowlist is `Shell(ls)` in `~/.cursor/cli-config.json`; the prompt without the hook was not captured. |
| `turn-with-shell-approval-sandbox` | same, `--sandbox enabled`, hook `ask` | `Run this command outside the sandbox?` / `→ Run outside sandbox (once) (y)`. |
| `turn-with-shell-declined-input` | `esc` (or `n`) at the prompt | Esc does not decline yet: it opens a boxed command and `→ Tell the agent what to do instead (Enter to send, empty to skip, Esc to cancel)`. |
| `turn-with-shell-declined` | `esc`, then empty `Enter` | The tool line ends `Rejected` and the model replies that the command was declined and not run. Composer returns; `stop` status `completed`. |
| `turn-with-shell-completed` | `y` at the prompt | `$ <command> 5.1s`, then the model's reply `Done. specimen.txt now contains hello-specimen.`, composer `→ Add a follow-up`, footer `Auto · 4.7%`. The `Running` line and `ctrl+c to stop` are gone. |
| `interrupt-esc` | `esc` while `sleep 20` runs | Tool line ends `Cancelled • 5.2s`; composer returns at once. No `Conversation interrupted` banner. |
| `interrupt-ctrl-c` | Ctrl-C while `sleep 20` runs | Same `Cancelled`; Ctrl-C mid-turn **stops the turn but does not exit**, and a second Ctrl-C then shows `Press Ctrl+C again to exit` as at idle. |
| `resume-by-id` | `agent --resume <id>` | The conversation replays above a fresh `→ Add a follow-up` composer (the user turn, `I'll run that command now.`, the tool line, the reply). |
| `exit-ctrl-c` | two Ctrl-C within about 1 s, after a real turn | First Ctrl-C prints `Press Ctrl+C again to exit` below the footer (erased after about 2 s); the second exits and prints `To resume this session: agent --resume=<id>`. |
| `exit-ctrl-c-idle` | two Ctrl-C at an idle composer, no message sent | Same exit, but **no resume hint** (the chat has no conversation). |
| `slash-popup` | `/` at the composer | A list opens under the prompt: `/model`, `/goal`, `/add-dir`, `/save-workspace`, `/load-workspace`, `/run-everything`, `/auto-review`, `/plan`, `/ask`, `↓ more below`. Typing `re` narrows it (`/rewind`, `/review`, `/rename <name>`, `/resume`, ...). |
| `rename` | `/rename specimen-title-8841` | Confirmation line `Renamed chat to specimen-title-8841`. |
| `session-list` | `/resume` | A full-width list `All chats • This workspace` / `Last updated`, newest first, titles are the chat name (`New Agent` until renamed), footer `↑/↓ to navigate • ←/→ to switch scope • Enter to select • Backspace to delete • Esc to close`. |
| `turn-with-edit-completed` | prompt asks for a file via the edit tool | `Edited notes.txt +1` / `▎+ edit-specimen`, footer `Auto · 4.7% · 1 file edited`. No permission prompt for the edit. |
| `turn-with-edit-denied` | same, `preToolUse` returns deny | `Edited DENYME.txt Edit rejected`; the model quotes `Blocked by specimen hook (user_message).`. |
| `turn-with-shell-denied` | shell call, `preToolUse` returns deny (`agent_message` only) | `Rejected` / `Shell blocked by preToolUse hook` / `Agent note: Do not suggest workarounds to the blocked tool.` |
| `help.txt` | `agent --help` | Every flag in this version, including some the docs page doesn't list (`--auto-review`, `--add-dir`, `-e/--endpoint`). Commands include `ls`, `resume`, `create-chat`, `generate-rule`. |
| `create-chat*.txt` | `agent create-chat` | A new chat id printed before anything launches: under a PTY, with no TTY, and inside a repo. |

Delivery note that isn't a file: text and a carriage return written in **one**
write were typed into the composer but not submitted. Send the paste and the
Enter as separate writes. A pending prompt that is still `Waiting for approval...`
takes `y`, `n` and Esc, so a pane send arriving then types a choice, not text.

## Hooks (`hooks/`)

`hooks.json` (the flat `{"version": 1, …}` format, placed at
`<repo>/.cursor/hooks.json`) and `log-hook.sh` are the logger that produced
these payloads. It allows `preToolUse`, answers `ask` for
`beforeShellExecution` (which is what raises the permission prompt), and
answers deny to a `preToolUse` whose payload contains `DENYME`. `run.sh`
launches `agent` through the driver. Cursor asked for no hook trust: the
project hooks file was honored on first launch.

| Payload | Notes |
|---|---|
| `sessionStart.json` | Fires at launch of a **new** session, **before any message**: `conversation_id` (= `session_id`), `model`, `cursor_version`, `workspace_roots`, `user_email`. `transcript_path` is `null`. **Does not fire on `--resume`** (neither interactive nor `-p`). |
| `beforeSubmitPrompt.json` | The prompt text and `attachments: []`, where a pasted image would appear (untested). |
| `preToolUse.json` | `tool_name: "Shell"`, `tool_input: {command, cwd, timeout}`. This is the guard's enforcement point. |
| `beforeShellExecution.json` | `command`, `sandbox: false`, and the first non-null `transcript_path`. |
| `afterShellExecution.json` | `command`, `output`, `duration`, `sandbox`. |
| `afterShellExecution-declined.json` | A **declined** command still fires this, with `output: ""`: indistinguishable from a successful no-output command. |
| `postToolUse.json` | `tool_output` is a JSON string: `{"output":"","exitCode":0}`. |
| `postToolUse-declined.json` | Fires for a declined command too, with `exitCode: 0`. Only the transcript, the screen and the absence of the file show it was rejected. |
| `postToolUseFailure-interrupted.json` | Esc or Ctrl-C mid-command: `error_message: "Command was aborted"`, `failure_type: "error"`, `is_interrupt: true`. |
| `postToolUseFailure-denied.json`, `-denied-read.json` | A `preToolUse` deny: `failure_type: "permission_denied"`, `is_interrupt: false`. `error_message` is the hook's `user_message` when given (`-denied-read`), else `Shell blocked by preToolUse hook` (`-denied`). |
| `stop-completed.json` | `status: "completed"`, `loop_count`, token counts (`input_tokens`, `output_tokens`, `cache_*`), `transcript_path`. |
| `stop-aborted.json`, `stop-error.json` | Esc or Ctrl-C mid-turn fires **two** `stop` payloads: `aborted`, then `error`. (Seen in every interrupt run, including Esc alone, so the second is not a Ctrl-C effect.) A reader treating `error` as failure will misread an interrupt. |
| `afterAgentResponse.json` | `text` of the whole reply, plus token counts. |
| `sessionEnd-completed.json`, `-completed-after-resume.json`, `sessionEnd-aborted.json` | `reason`, `final_status`, `duration_ms`. Two Ctrl-C at the composer: `completed`. The process killed by SIGTERM: `aborted`. |
| `preToolUse-read-before-write.json` | The **edit** tool fires a `Read` `preToolUse` first, with the **same `tool_use_id`**, then the write. |
| `preToolUse-write.json` | The edit tool is `Write`: `tool_input: {file_path, content}`, full content. Edits to an existing file are also `Write` with the new full text. |
| `postToolUse-write.json` | `tool_output`: `{"file_path":…,"success":true}`. |
| `afterFileEdit.json`, `afterFileEdit-existing-file.json` | `file_path` and `edits: [{old_string, new_string}]` (empty `old_string` for a new file; the changed span for an existing one). |
| `preToolUse-denied-shell.json`, `-denied-read.json` | The payloads the deny hook answered. |

**A deny and what the model sees.** The hook answered
`{"permission":"deny","user_message":…,"agent_message":"SPECIMEN-DENY-REASON-7731…"}`.
Told to quote any reason it was given, the model quoted the `user_message`
(`transcript-record-edit-denied.jsonl`); with `agent_message` alone it quoted
only `Rejected: Shell blocked by preToolUse hook` (`turn-with-shell-denied.txt`), and the `agent_message` marker
appeared nowhere on screen or in the transcript. So in this run **only
`user_message` was observed to reach the model**; `agent_message` was not.
Also, a deny of the edit tool's pre-read `Read` call blocked the write.

## Session id, store, resume

- **An ID is allocated at launch; persistence comes with the message.** A new
  `agent` fires `sessionStart` at once and creates
  `~/.cursor/chats/<md5(workspace path)>/<id>/meta.json` with
  `hasConversation: false` (`session-meta-record-idle.json`); no transcript
  file and no `store.db` exist yet. After the first message `store.db` and
  `prompt_history.json` appear and `hasConversation` is `true`. The
  transcript appears only once the first user turn is written. `agent create-chat`
  prints an id before anything launches (see `create-chat.txt`).
- **Resume by id:** `agent --resume <id>` (`resume-by-id`), also `agent resume`
  (latest), `--continue`, `agent ls` and the `/resume` list (`session-list`).
  `sessionStart` does not fire on resume.
- **Exit prints the resume command** only when the chat has a conversation:
  `To resume this session: agent --resume=<id>` (`exit-ctrl-c`).
- **Stores:** `~/.cursor/chats/<md5(workspace)>/<id>/{meta.json,store.db,prompt_history.json}`
  (the SQLite `store.db` holds `blobs` and a `meta` row), and the transcript
  at `~/.cursor/projects/<munged-workspace>/agent-transcripts/<id>/<id>.jsonl`.
- `session-meta-record.json` is `meta.json` after a rename (`title` appears);
  `session-meta-record-idle.json` is the launch-time one.
  `session-store-meta-record.json` is the decoded `store.db` `meta` row with
  `name`, `mode`, `lastUsedModel`; its encryption key and root blob id are left
  out (the real row carries a `blobEncryptionKey`).

## Naming

`/rename <name>` (and `/rename-chat`, a model-driven rename) is a slash
command. It writes `title` into `meta.json` and `name` into `store.db`'s
`meta` row, and `/resume` shows it; the transcript is unchanged. The default
name is `New Agent`. There is no CLI flag to name a chat at launch in this
version's `--help`; `agent create-chat` takes no name.

## Transcript (`transcript-record.jsonl`)

One file per conversation. A user record is
`{"role": "user", "message": {"content": [{"type": "text", "text": …}]}}`, with
the text wrapped in `<timestamp>` and `<user_query>` tags. Assistant records
carry `text` and `tool_use` items (`{"name":"Shell","input":{command,description}}`
or `Write` with `path`/`contents`). **The last line of a finished turn is
`{"type":"turn_ended","status":"success"}`** (`aborted` with
`"error":"User aborted/interrupted manually."` after Esc/Ctrl-C;
`transcript-turn-ended.jsonl`). A declined command shows as a normal
`tool_use` followed by a reply, with no tool-result record
(`transcript-record-declined.jsonl`). See also `transcript-record-edit.jsonl`
and `transcript-record-edit-denied.jsonl`.

## Instructions file

Cursor honored all three of these in a scratch repo, using a distinctive
word-marker instruction and a question with `agent -p`: `AGENTS.md`
(`AGENTSMD-7742` led the answer), `.cursor/rules/*.mdc` with `alwaysApply: true`
(`RULES-5513` ended it), and `CLAUDE.md` (`CLAUDEMD-9914` led the answer, alone
and beside the rule). Not tested: which wins on a conflict, and whether
Cursor needs `alwaysApply` for a rule to load.

## Loopback `curl`

A `curl -4 http://127.0.0.1:<port>/` to a local `python3 -m http.server` got
its response in every configuration: default (`sandbox: false`), `--sandbox
enabled` with the hook answering `allow` (payload `sandbox: true`, no prompt,
the listener logged the request), and `--sandbox enabled` with the hook
answering `ask` (prompt `Run this command outside the sandbox?`, then
success). No `--network` flag was needed. So Bram's loopback `curl` channel
is not blocked by Cursor's sandbox here; the only gate is the approval prompt.

## Residue (outside the scratchpad)

Under `~/.cursor`: about 20 scratch chats under `chats/<md5(workspace)>/`, a
`projects/<munged-workspace>/` directory with transcripts and a
`.workspace-trusted` file, and `cli-config.json` rewritten by the CLI
(content seen unchanged where checked: allowlist `Shell(ls)`, sandbox
`disabled`). No credential file was read or copied, and the `blobEncryptionKey`
in `store.db` was left out. The scratch repo's `.cursor/` (hooks) lives in the
scratchpad only.

## Not yet captured

- Whether `@<png>` attaches an image (`attachments` in `beforeSubmitPrompt`
  stays untested).
- Why `stop` fires twice (`aborted` then `error`) on an interrupt; not isolated
  beyond "every Esc/Ctrl-C run".
- `sessionStart` with a source other than a new launch (there is no `source`
  field; resume fires nothing).
- A `Notification`-style hook: none of the registered events looked like one.
- Subagent transcripts: not checked for this version.
- Which of `AGENTS.md`, `CLAUDE.md` and rules wins when they conflict; hook
  `agent_message` visibility with a model that is not asked to quote it.
- Logged-out flow after the keypress (browser login).

## Seams

| Seam | Cursor | Evidence |
|---|---|---|
| launch and resume by id | `agent` / `agent --resume <id>` / `agent resume` (latest) / `--continue` / `agent ls` | `resume-by-id.txt`, `help.txt`; Bram: no Cursor provider code |
| when the session id exists | Allocated at launch: `sessionStart` fires with `conversation_id` and a `meta.json` (`hasConversation:false`) is written; `store.db` and the transcript appear with the first message | `hooks/sessionStart.json`, `session-meta-record-idle.json` |
| session store | `~/.cursor/chats/<md5(workspace)>/<id>/{meta.json,store.db,prompt_history.json}` plus transcripts under `~/.cursor/projects/<munged-workspace>/agent-transcripts/<id>/<id>.jsonl` | `session-meta-record.json`, `session-store-meta-record.json`, `transcript-record.jsonl` |
| naming | `/rename <name>` writes `meta.json` `title` and `store.db` `name`; default `New Agent`; shown by `/resume` | `rename.txt`, `session-list.txt`, `session-meta-record.json` |
| guard hook | `preToolUse` (and `beforeShellExecution`) in project `.cursor/hooks.json`, no trust prompt; a deny returns `permission:"deny"`; the model saw `user_message`, not `agent_message` | `hooks/preToolUse.json`, `hooks/hooks.json`, `transcript-record-edit-denied.jsonl` |
| turn state | `beforeSubmitPrompt` starts a turn, `stop` ends it with `status` `completed`/`aborted`/`error` (an interrupt fires `aborted` then `error`); transcript ends with `turn_ended` | `hooks/stop-completed.json`, `hooks/stop-aborted.json`, `transcript-turn-ended.jsonl` |
| Agent→Bram channel | Loopback `curl` works, including under `--sandbox enabled` (no `--network` needed) | `hooks/afterShellExecution.json` (`sandbox:true` run), section Loopback `curl` |
| Bram→agent delivery | Paste and Enter must be separate writes; a one-write paste is typed but not submitted | `README.md` delivery note |
| pasted images | not captured | n/a |
| menus | `/` popup, `/resume` list, approval prompt (`Run this command?`) | `slash-popup.txt`, `session-list.txt`, `turn-with-shell-approval.txt` |
| working status | `⠠⠛ Running  N tokens` / `Thinking` / `Working`, composer `→ Add a follow-up … ctrl+c to stop`, footer `N task` | `turn-with-shell-working.txt` |
| interrupt and exit | Esc or Ctrl-C stops a turn (`Cancelled`); at the composer Ctrl-C once shows `Press Ctrl+C again to exit`, twice exits and prints `agent --resume=<id>` (only for a chat with a conversation) | `interrupt-esc.txt`, `interrupt-ctrl-c.txt`, `exit-ctrl-c.txt` |
| logged-out screen | Full-screen splash, any key starts login | `logged-out.txt` |
| instructions file | `AGENTS.md`, `.cursor/rules/*.mdc` (`alwaysApply`) and `CLAUDE.md` all honored | section Instructions file |
| screen shapes | Boxless, left-indented; prompt `→`, footer `<model> · <pct>` over `<path> · <branch>`; approval is an inline `Run this command?` list with `(y)`/`(esc or n)` hints; Esc at it opens a feedback box | `idle-composer.txt`, `turn-with-shell-approval.txt`, `turn-with-shell-declined-input.txt` |
| model and version | `agent` `2026.10.22-27dcf5f`; `model: "default"` (displayed `Auto`); `cursor_version` in every hook payload | `hooks/sessionStart.json`, `idle-composer.txt` |

## Alignment — 2026-10-28

Added for the Codex-aligned pack ([worklist item
`agent-specimens-align-to-codex`](../../../resources/worklist-drafts/agent-specimens-align-to-codex.md)):
the `resume-by-id`, `exit-ctrl-c` and `turn-with-shell-*` phase screens,
interrupt, slash/rename/list screens and edit/deny screens; `stop`,
`afterShellExecution`, `postToolUse*`, `afterFileEdit`, `sessionEnd` and deny
payloads; session metadata records; the instruction-file and loopback checks;
and `capture/`. Pre-existing files were left as they were, except the hook
logger and `hooks.json`, which gained events and the deny switches. Signed:
Claude subagent (Sonnet), delegated by Jon's Claude main thread.
