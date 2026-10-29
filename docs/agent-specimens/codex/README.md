# Codex CLI specimens

What Codex's terminal agent actually draws and emits, captured alongside the
Cursor, Claude Code and OpenCode packs for the provider comparison in
[#462](https://github.com/judell/bram/issues/462). Markers Bram anchors on come
from captures like these, never from guesses (`docs/developing-bram.md`
§Capturing terminal specimens).

- **CLI:** `codex`, `codex-cli 0.165.0`; model shown in the footer as
  `GPT-6-Astra high` (`model = "gpt-6-astra"`)
- **Platform:** macOS (arm64), 2026-10-28
- **Capture:** `scripts/capture-terminal-specimen.py` for `logged-out`,
  `first-run` and `help`; a small multi-step variant of it (same pty, 120x40,
  scheduled keystrokes and snapshots) for everything that needs a keypress
  sequence. Both ran in scratch git repos outside the Bram repo. The recovered
  driver, renderer, and retained schedules are in [`capture/`](capture/README.md).
- **Redacted:** the scratch workspace path (`<workspace>`) and the home
  directory (`~`). Codex's TUI uses cursor addressing, so the plain ANSI strip
  `capture-terminal-specimen.py` does runs a whole screen together on one
  line. Each `.txt` here is instead the **final screen rendered through a
  minimal VT emulator** (one line per row, 120 columns). The `.raw` files are
  redacted terminal streams: escape sequences remain, but path bytes have
  been replaced. They are not byte-exact originals. Replacing paths changes
  widths and can change cursor-addressed replay; rendering a checked-in
  `.raw` need not reproduce its separately redacted `.txt` exactly. See the
  [capture notes](capture/README.md) for the renderer's limitations.
- **Account:** logged-out was captured under a throwaway `CODEX_HOME` with
  `OPENAI_API_KEY` unset. Everything that needed a model used the real
  `~/.codex` login (a throwaway home cannot log in non-interactively); see
  "Residue" below.

## Screens

| File | How | What it shows |
|---|---|---|
| `help.txt` | `codex --help` | Subcommands (`resume`, `fork`, `queue`, `exec`, `agents`, ...) and flags. `-a/--ask-for-approval` accepts only `on-request` and `never` in this version. |
| `logged-out` | `codex` under an empty `CODEX_HOME` | Full-screen chooser: `Welcome to Codex, OpenAI's command-line coding agent` / `Choose how you want to use Codex.` / `1. Sign in with ChatGPT`, `2. Sign in with Device Code`, `3. Use an OpenAI API key`, `4. Use Amazon Bedrock` / `Press enter to continue`. **Enter starts sign-in**, so pane sends must be held. |
| `first-run` | `codex` in a never-seen folder | The splash with an overlay: `Folder access` / `<path>` / `Trust this folder? ... Your trust decision will be saved.` / `› 1. Trust and continue` / `2. Back to Agent Command Center` / `enter continue · esc back`. **Enter trusts the folder** and writes `[projects."<path>"] trust_level = "trusted"` to `~/.codex/config.toml`. (In an earlier run the same overlay read `2. Quit`.) |
| `hooks-trust` | first launch after hooks appear in config | `Hooks need review` / `6 hooks are new or changed.` / `Hooks can run outside the sandbox after you trust them.` / `› 1. Review hooks`, `2. Trust all and continue`, `3. Continue without trusting (hooks won't run)` / `enter confirm · esc skip`. This is the surface Bram classifies as `hooks-trust` (`terminal_attention_prompt_shape`, `src-tauri/src/lib.rs`, pinned at v0.148.0/0.162.1). The shape differs from those specimens (it now offers "Trust all" directly), so the marker deserves a re-check against 0.165.0. |
| `hooks-review` | Enter on "Review hooks" | The `Hooks` table (Event / Installed / Active / Review) with `⚠ 6 hooks need review before they can run.` and `t trust all · enter review · esc close`. Pressing `t` writes `trusted_hash` entries to `~/.codex/config.toml` and leaves this menu open, so the next keystrokes go to the menu, not the composer. Send `esc`. |
| `idle-composer` | `codex` in a trusted folder | Splash logo, then `› Ask Codex to do anything`, the footer `GPT-6-Astra high · <path>`, and `? for shortcuts`. The right of the footer shows `⚠ 1 warning · f2 to view` (appeared with the hook flags; cause not isolated). |
| `turn-with-shell-working` | prompt `Run the shell command: echo hello-specimen > specimen.txt`, Enter | The prompt echoed after `›`, then `• Working (1s • esc to interrupt)` above a fresh `› Ask Codex to do anything` composer. |
| `turn-with-shell-approval` | same, with `-a on-request -s read-only` | `• Running echo hello-specimen > specimen.txt` / `Would you like to run the following command?` / `Environment: local` / `Reason: ...` / `$ <command>` / `› 1. Yes, proceed (y)`, `2. Yes, and don't ask again for commands that start with ... (p)`, `3. No, and tell Codex what to do differently (esc)` / `Press enter to confirm or esc to cancel`. **Enter or `y` runs the command.** |
| `turn-with-shell-declined` | `esc` at that prompt | `✗ You canceled the request to run ...` / `■ Conversation interrupted - use /feedback if something went wrong`, composer back. |
| `turn-with-shell-completed` | default sandbox (no approval needed) | `• Ran echo hello-specimen` / `└ hello-specimen` / `• hello-specimen` / `Worked for 7s • 8:12 PM`. No approval prompt: the default workspace-write sandbox allows it, and `PermissionRequest` never fired. |
| `dialog-slash-popup` | type `/` in an idle composer | A list that opens under the composer as you type: `/model  choose what model and reasoning effort to use`, `/fast`, `/presets`, `/daybreak`, `/ide`, `/permissions`, `/keymap`, `/vim`, ... The composer row shows `› /`; typing narrows the list (`/model` leaves one row). Esc closes it. It is not a modal: Enter on `/model` (or any row) runs the command. |
| `dialog-model-picker` | `/model`, Enter | `Select Model and Effort` / eight numbered rows (`1. GPT-6.1-Sol` ... `8. GPT-5.6-Luna`) each with a one-line description; the `›` cursor starts on the current model and that row ends `(current)` / footer `enter select · esc back`. Selecting a row leads to a reasoning-effort step (not captured; Esc was used). Differs from the 0.153.3 specimen in `pty-menu-specimens` (that footer was `Press enter to confirm or esc to go back`). |
| `dialog-resume-picker` | `codex resume` (no id) | `Resume a previous session` / `Filter:  Cwd  All    Status:  Active  Archived    Sort:  Updated  Created` / `Type to search` / rows `› 38s ago     <thread_name>` (relative time, then the `session_index.jsonl` name) / a `1 / 3 · 100%` rule / footer `enter resume   ⌃a archive   esc start new   ⌃c quit   tab focus sort/filter   ←/→ change option` and `⌃o comfortable view   ⌃t transcript   ⌃e expand   ↑/↓ browse`. Defaults to the **Cwd** filter, so it lists only sessions from the folder it was launched in. Typing `/` enters search (`Search: /`), and Esc then clears the search; a second Esc **starts a new session** rather than exiting. **Enter resumes the highlighted row.** |
| `agents-overview` | `codex agents` | `Agent command center  Group: Project` / status tabs `All 10  Needs you 0  Working 0  Ready 0  Inactive 10` / a task list grouped by project (`○ <thread_name>  Inactive  56s ago`) beside a `Task details` pane (model, reasoning, project path, branch, last message, prompt). Unlike the resume picker it spans **every** project's sessions on the shared app-server daemon, and uses `enter open`, `n new`, `esc back`. The only text file here (no `.raw`): this screen listed the user's other projects, whose rows are replaced with `<other project>` / `<other-project task>`. |
| `turn-with-apply-patch-completed` | prompt `Use the apply_patch tool to create a file named patch-specimen.txt ...` | Default sandbox, no approval. The edit is drawn as `• Added patch-specimen.txt (+1 -0)` / `1 +hello-patch` / `+ Show details`, not as a shell command, then `• Created patch-specimen.txt using apply_patch ...` / `Worked for 23s`. |
| `turn-with-apply-patch-approval` | same family, `-a on-request -s read-only`, prompt appends a line to `README.md` | `• Edited README.md (+1 -0)` / `2 +hello-update`, then **`Would you like to make the following edits?`** / `Description: Apply proposed file edits` / `Destination: <path>` / `› 1. Yes, proceed (y)`, `2. Yes, and don't ask again for these files (a)`, `3. No, and tell Codex what to do differently (esc)` / `Press enter to confirm or esc to cancel`. Different wording from the shell prompt (`Would you like to run the following command?`); Bram's marker for the shell prompt will not match it. `PermissionRequest` fired here. |
| `turn-with-subagent` | prompt `Spawn one subagent and ask it to reply with only the word pong ...` | ``• Started `/root/pong` `` / ``• Completed `/root/pong` `` / `• The subagent said: pong.` / `Worked for 11s`. Subagent activity is two status lines, no separate pane. |
| `resume-then-clear` | `codex resume <id>`, a message, then `/clear` and another message | Final screen after `/clear`: a fresh splash and one new turn (`ok2`). The resumed transcript is gone from the screen. `/clear` allocates a **new session id** (see `SessionStart-clear.json`). |
| `resume-by-id` | `codex resume <id>` | The session replays its transcript (`› Run the shell command: ...`, `• Ran ...`, `Worked for 7s`) above a fresh composer. |
| `exit-ctrl-c` | Ctrl-C at an idle, empty composer | **One Ctrl-C exits.** The shell gets `Session ID: <id>`; after a real turn it also prints `Token usage: ...` and `To continue this session, run: codex resume <id>`. |
| `turn-with-deny` | prompt asks for `echo DENYME > denyme.txt` and to quote any reason; the logger denies | `• Blocked by hook` / `└ MARKER-REASON-7731: blocked by specimen guard`, then the model's reply quoting the same text. No approval prompt, no `Working` line left behind. |
| `turn-with-image-mention-popup` | bracketed paste of `What colour is this image? @<abs png>` | A `Mentions` popup (`Files, directories, plugins, skills and tasks`; `All Results` / `Filesystem Only` / `Plugins`; `no matches`; `enter/tab insert · esc close`) opens over the composer because of the `@`. The Enter that followed still submitted the turn as typed (the popup did not swallow it). |
| `turn-with-image` | same, submitted | The composer shows the text with the path **verbatim**; then `• Viewed image red.png` / `+ Show details`, `• The image is red.` No `[Image #1]` chip: `@path` is not an attachment. |
| `turn-with-image-flag` | `codex "What colour is this image?" -i red.png` | `[Image #1]` chip above the prompt; `• Red.` The CLI-flag route attaches pixels. |
| `turn-with-loopback` | prompt `Run this exact shell command ... curl -sS http://127.0.0.1:<port>/` | `• Exited (status 7) curl -sS ...` / `curl: (7) Failed to connect to 127.0.0.1 port <port> after 0 ms: Couldn't connect to server`. No approval prompt. |

Delivery notes that aren't files: the working-line glyph is `◦`/`•` plus
`Working (Ns • esc to interrupt)`; typed text and the Enter that submits it
were sent as **separate writes** and submitted correctly. A pane send that
arrives while the `Hooks` menu is open types into the menu (it did not toggle
any hook here, but `n`/`o` are shortcuts there).

## Hooks (`hooks/`)

`hooks-config.toml` registers a logger (`log-hook.sh <Event>`, which appends
stdin to `$HOOKLOG_DIR/<Event>.jsonl` and prints nothing, i.e. no decision) on
the three events Bram's own Codex guard registers (`PreToolUse`,
`PermissionRequest`, `PostToolUse`, same matchers as `codex_hook_toml_block`,
`src-tauri/src/lib.rs:53379`) plus `SessionStart`, `UserPromptSubmit` and
`Stop`. `run.sh` shows how it was injected: as `-c hooks.<Event>=[...]`
command-line overrides, so no hook block was written to any config file.
(Codex still recorded `trusted_hash` entries after "Trust all"; see Residue.)

| Payload | Notes |
|---|---|
| `SessionStart.json` | `source: "startup"`. Fires **with the first message**, not at launch: a launch-and-exit run produced no `SessionStart` and no rollout file. Already carries `session_id`, `transcript_path`, `cwd`, `model`, `permission_mode`. |
| `UserPromptSubmit.json` | Adds `turn_id` and `prompt` (the text). |
| `PreToolUse.json` | `tool_name: "Bash"`, `tool_input: {command}`, `tool_use_id: "exec-..."`. Bram's guard enforcement point. Fired twice in the approval run: once for the sandboxed try, once after escalation. |
| `PermissionRequest.json` | Fires only when the approval prompt is raised. `tool_input` carries `command` and `description` (the prompt's "Reason"). Bram's `codex-permission-menu` hook uses it. |
| `PostToolUse.json` | `tool_response` is a plain string (`"hello-specimen\n"`). |
| `PostToolUse-sandbox-denied.json` | Same event for the sandboxed attempt that failed: `"/bin/bash: specimen.txt: Operation not permitted\n"`. A failed command still fires `PostToolUse`. |
| `PreToolUse-apply_patch.json` | `tool_name: "apply_patch"`; `tool_input` is `{"command": "*** Begin Patch\n*** Add File: <abs path>\n+hello-patch\n*** End Patch"}`: the patch text sits under the key `command`, with **absolute paths** in the file headers. Bram's guard matcher covers it. |
| `PostToolUse-apply_patch.json` | Same `tool_input`; `tool_response` is a plain string `"Exit code: 0\nWall time: 0.1 seconds\nOutput:\nSuccess. Updated the following files:\nA <abs path>\n"` (`A`/`M`/`D` per file). `tool_use_id` is `exec-...`, like Bash. |
| `PermissionRequest-apply_patch.json` | Raised only under `-a on-request -s read-only`. `tool_input` is the same `{command: <patch>}`; unlike the Bash request it has **no `description`**. |
| `SessionStart-resume.json` | `source: "resume"`, `session_id` is the **same id** as the resumed session. Fired with the first message after resume, not when the resumed transcript is shown. |
| `SessionStart-clear.json` | `source: "clear"` after `/clear`: a new `session_id` and a new rollout file. |
| `SessionEnd.json` | `{session_id, transcript_path, cwd, hook_event_name: "SessionEnd", reason: "other"}`; fired on Ctrl-C exit. Not in Bram's registered set. |
| `SubagentStart.json` / `SubagentStop.json` | `session_id` and `turn_id` are the **parent's**; `agent_id` is the subagent's thread id and `agent_type: "default"`. `SubagentStart.transcript_path` already names the **subagent's own rollout**; `SubagentStop` adds `agent_transcript_path`, `stop_hook_active` and `last_assistant_message` (`"pong"`). |
| `notify-agent-turn-complete.json` | Not a hook: the `notify = [program, ...]` config key runs the program with one JSON argument per completed turn (`type: "agent-turn-complete"`, `thread-id`, `turn-id`, `cwd`, `client`, `input-messages`, `last-assistant-message`). Codex runs it **twice** per first turn: once for the hidden title-generation turn (whose `input-messages` is the title prompt and whose last message is `{"title": ...}`), once for the real turn. This file is the real turn. |
| `Stop.json` | `stop_hook_active`, `last_assistant_message`. Did **not** fire when the turn was interrupted with Esc (declined run). |
| `PreToolUse-denied.json` | The payload the deny hook answered (`tool_name: "Bash"`, `tool_input.command` contains `DENYME`). `hooks/deny-hook.sh` printed `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"MARKER-REASON-7731: blocked by specimen guard"}}` and exited 0, the shape Bram's guard returns to Codex (`guard_selftest_run_case`, `src-tauri/src/lib.rs`). No `PostToolUse` fired for the denied call; `Stop` fired normally. |
| `UserPromptSubmit-image.json` | For the `@<png>` turn: `prompt` is the typed text with the path, and there is **no** `images`/`attachments` field. |
| `UserPromptSubmit-image-flag.json` | For the `-i` turn: `prompt` is only `What colour is this image?`; the image is not mentioned in the payload at all. |
| `PreToolUse-loopback.json`, `PostToolUse-loopback.json` | The `curl` call; `tool_response` is the plain string `"curl: (7) Failed to connect to 127.0.0.1 port <port> after 0 ms: Couldn't connect to server\n"`. A failed command still fires `PostToolUse`. |

**Events 0.165.0 recognizes** (from the binary's config schema, not all
captured): `PreModelRequest`, `PostModelRequest`, `PreToolUse`,
`PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`,
`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `SubagentStart`,
`SubagentStop`, `Stop`, plus an `Interrupt` entry. **There is no `Notification`
event**; the nearest things are the `notify` program above and
`[tui] notifications` / `notification_method` / `notification_condition`
(terminal bell or desktop notices, no payload to a script).

**Bram's own hooks were live during these runs.** After the first pass, the real
`~/.codex/config.toml` gained Bram's `# bram:start` block (guard on
`PreToolUse`/`PermissionRequest`/`PostToolUse`, already trusted). The logger
therefore ran **beside** Bram's guard (hook index 1 next to Bram's index 0), and the
agent, seeing Bram's conventions through the user's global instructions,
sometimes began a turn with read-only `Bash` calls for `AGENTS.md`/worklist
files (visible in `PreToolUse.jsonl` of `turn-with-apply-patch-completed`).
Adding an extra hook (e.g. `SubagentStart`) re-raises the `Hooks need review`
overlay; on the first Enter the schedule sends Down then Enter ("Trust all").

Every payload includes `transcript_path` (the rollout file) from the first
event on, which is why Bram can find the transcript before the turn ends.

## Guard deny, pasted images, loopback (2026-10-28)

Captured with `capture/schedules/s11`-`s14` (see [capture notes](capture/README.md)).

**A deny and what the model sees.** The hook answered with
`hookSpecificOutput.permissionDecision: "deny"` and a
`permissionDecisionReason`, exit 0 (`hooks/deny-hook.sh`). Codex drew
`Blocked by hook` with the reason, and **the reason reached the model**: the
rollout shows the tool call returning
`Script error:\nCommand blocked by PreToolUse hook: MARKER-REASON-7731: blocked by specimen guard. Command: echo DENYME > denyme.txt`,
and the model, asked to quote any reason, quoted it
(`transcript-record-deny.jsonl`). Unlike Cursor's `agent_message`, the
`permissionDecisionReason` is delivered whole and nothing was needed beyond it.
Codex's shell tool is a JS-wrapper `exec` `custom_tool_call`
(`tools.exec_command({cmd})`), but the hook still saw `tool_name: "Bash"`.

**Pasted images.** Bram's `@<abs png>` in the typed text does **not** attach
the image. The `UserPromptSubmit` prompt and the rollout's user message are
the literal text, with no `input_image` and no `images`/`local_images`
(`transcript-record-image.jsonl`, record 11). The model nevertheless answered
`The image is red.` because it chose to call its own `view_image` tool on the
path (`exec` call, an `ImageView` item, then a `custom_tool_call_output`
carrying an `input_image` data URL); the screen shows `Viewed image red.png`.
So the answer depends on the model reading the file from disk, which needs the
path to be readable inside the sandbox. The `-i/--image` flag route does attach:
the user message is `<image name=[Image #1] path="...">`, an `input_image`
data URL, `</image>`, then the text, and the item is a `local_image` content
entry (`transcript-record-image-flag.jsonl`). A bracketed paste containing `@`
opens the `Mentions` popup (`turn-with-image-mention-popup`); the following
Enter submitted the text as typed. `view_image` is not covered by the logger's
matcher (`^(Bash|apply_patch|Write|Edit|mcp__.*)$`), so no `PreToolUse` fired
for it.

**Loopback from inside a turn.** Under Bram's launch (plain `codex`, no
flags; `lib.rs` passes none) the default sandbox here is `approval_policy:
on-request`, `sandbox_policy: workspace-write`, `network_access: false`
(`transcript-record-loopback.jsonl`). `curl -sS http://127.0.0.1:<port>/`
against a `python3 -m http.server` the scratch dir started **was refused**
(`curl: (7) Failed to connect ... Couldn't connect to server`); the listener's
log shows only the check made from outside beforehand. There was no approval
prompt and no escalation, so the filesystem intent/result channel is required.
The model did not retry. The failure arrives as a normal `PostToolUse`.


## Session id, store, resume

- **An ID is allocated before the first message; persistence comes later.**
  Launch plus Ctrl-C prints a `Session ID: <uuid>` (see `exit-ctrl-c.txt`).
  That run was observed to leave no new file under `~/.codex/sessions/` and
  no `session_index.jsonl` line. This establishes an ID before any message,
  not a persisted, resumable conversation; the exact allocation instant
  within launch/exit was not captured.
- **After the first message:** `~/.codex/sessions/YYYY/MM/DD/rollout-<local-timestamp>-<session-id>.jsonl`
  appears, a `thread_name` line is appended to `~/.codex/session_index.jsonl`
  (auto-generated, e.g. `"Run echo hello-specimen"`; append-only, last wins),
  and `SessionStart` fires. The id is a UUIDv7; `session_id` in hook payloads is
  the same id.
- **Resume by id:** `codex resume <id>` (`resume-by-id`). Also `codex resume
  --last`, `codex fork`. Resume replays the transcript on screen.
- **Naming evidence:** `session-index-record.jsonl` is the six lines Codex itself
  appended for the scratch sessions of this pass (`{"id", "thread_name",
  "updated_at"}`), e.g. `"Create patch specimen"`, `"Reply with ok2"`. Names
  are generated by a hidden title-generation turn (visible as the first of two
  `notify` calls) from the first prompt, about five words, imperative.
  A `/clear` session and each fresh `codex` get their own line; a **resumed**
  session did not add a line (the earlier name stays). Only the main thread is
  named: **no line was written for the subagent**.
- **Resume and `/clear`:** `codex resume <id>` keeps the id (`SessionStart-resume.json`);
  `/clear` starts a new id and rollout in the same process (`SessionStart-clear.json`).
- **Subagents** (`multi_agent` is a stable feature, on by default): a spawned
  agent gets its **own rollout file** in the same date directory, written as soon
  as it starts. `subagent-session-meta-record.jsonl` is its first line:
  `parent_thread_id`, `source: {"subagent": {"thread_spawn": {parent_thread_id,
  depth: 1, agent_path: "/root/pong", agent_nickname, agent_role}}}`,
  `thread_source: "subagent"`, `multi_agent_version: "v2"`, and `session_id`
  equal to the **parent's** id (its own id is `id`). The parent's rollout holds
  a `function_call` `spawn_agent` (namespace `collaboration`; arguments
  `task_name`, `fork_turns`, and a `message` that is encrypted at rest).
  `codex agents` (`agents-overview`) is the screen for them across projects.
  The subagent has no `session_index.jsonl` line; Bram's session scanner
  should key on `source.subagent` / `thread_source` to skip them.
- `session-meta-record.jsonl`: the rollout's first line, trimmed to
  identifying fields. The real one also carries the account/user ids and the
  full base instructions text, left out here.

## Transcript (`transcript-record.jsonl`)

One rollout line per event, `{"timestamp", "ordinal", "type", "payload", ...}`.
The user's text is a `response_item` with
`payload: {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<prompt>"}]}`
and `provenance.type == "user"`. Beware: the harness injects **other**
`role: "user"` messages (`<environment_context>`, AGENTS.md payloads); the real
prompt is the one with `content_item_kinds: ["user.text"]`. Bram skips the
synthetic ones (`src-tauri/src/lib.rs:41922`).

## Residue (outside the scratchpad)

Real `~/.codex/config.toml` was used for the model turns and was modified by
Codex itself: a `[projects."<scratch>/repo"] trust_level = "trusted"` entry
(from pressing Enter on the folder trust prompt) and six `[hooks.state."..."]`
`trusted_hash` entries (from "Trust all"). Both were removed afterwards by
restoring the file from a copy taken before the run. The scratch session
rollouts (about five, plus their `session_index.jsonl` lines) remain under
`~/.codex/sessions/2026/10/28/`. No credential file was read or copied.

**Alignment pass (2026-10-28, second).** `~/.codex/config.toml` was backed up
before the first run and restored after the last (confirmed identical with
`diff`); Codex again added a folder-trust entry and `hooks.state` hashes for
the injected hooks, and one early probe typed text into the hook review menu
(it toggled one Bram hook to `enabled = false`, also undone by the restore).
Seven more scratch rollout files and their `session_index.jsonl` lines remain
under `~/.codex/sessions/2026/10/28/`. No credential file was read or copied.

## Not yet captured

- Pasted images: terminal clipboard paste of image bytes (as opposed to an `@path`) was not tried.
- Whether a loopback `curl` is allowed after the user approves an escalation: the default run raised no prompt to approve.
- `SessionStart` with `source` `fork`/`compact`: `codex fork` and compaction were not driven. (`resume` and `clear` are captured.)
- The reasoning-effort step after choosing a model in `/model` (Esc was used), and the picker's `⌃t transcript` / `⌃o` / `⌃a archive` views.
- `PreModelRequest`/`PostModelRequest`/`PreCompact`/`PostCompact`/`Interrupt` payloads: events exist in 0.165.0 but were not registered.
- A `PermissionRequest` for an MCP/`Write`/`Edit` tool: Codex's edit tool here is `apply_patch` only; no `Write`/`Edit` tool name was seen.
- `Notification` hook: Codex 0.165.0 has no such event (see Hooks); a `[tui] notifications` desktop/bell notice produces no capturable payload on the PTY.
- A separate sessions *list* distinct from the picker: `agents-overview` (`codex agents`) is the one; there is no other.
- Logged-out Codex after the sign-in choice (ChatGPT browser flow, device code,
  API key): the flows need a browser or credentials.
- Esc-Esc rewind menu: see Bram's existing
  [`pty-menu-specimens`](../../pty-menu-specimens/). (The `/` popup and `/model` picker are now captured.)
- Esc exit behaviour with a queued message, and `/exit` / `/quit`.

## Seams

| Seam | Codex | Evidence |
|---|---|---|
| launch and resume by id | `codex` / `codex resume <id>` / `codex resume` (picker) / `codex resume --last` / `codex fork`; `SessionStart` reports `source: resume` with the same id | `resume-by-id.txt`, `help.txt`; Bram: `src-tauri/src/lib.rs:384` |
| when the session id exists | Allocated before any message: idle exit prints a `Session ID`; rollout/index persistence and `SessionStart` were observed after the first message | `exit-ctrl-c.txt`; `hooks/SessionStart.json` |
| session store | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl` plus append-only `~/.codex/session_index.jsonl` | `session-meta-record.jsonl`; Bram: `src-tauri/src/lib.rs:25191`, `:31252` |
| naming | Codex auto-names a thread into `session_index.jsonl` (`thread_name`, last wins; main threads only, none for subagents or resumes); Bram appends `{"by":"bram","reassert":true}` entries to restore its names | Bram code: `src-tauri/src/lib.rs:31199` (`codex_titles_to_reassert`); captured Codex lines: `session-index-record.jsonl` |
| guard hook | `PreToolUse` (+ `PermissionRequest`, `PostToolUse`) in `~/.codex/config.toml` `[[hooks.*]]`, matcher `^(apply_patch\|Bash\|Write\|Edit\|mcp__.*)$`, user must trust each hook. The edit tool is `apply_patch`, input key `command` holds the patch text with absolute paths; `Write`/`Edit` names never appear. A deny is `hookSpecificOutput.permissionDecision: "deny"` + `permissionDecisionReason` on stdout, exit 0; the reason reaches the model verbatim in the tool error (`Command blocked by PreToolUse hook: <reason>. Command: <cmd>`) and no `PostToolUse` fires | `hooks/PreToolUse.json`, `hooks/PreToolUse-apply_patch.json`, `hooks/PermissionRequest-apply_patch.json`, `hooks/PreToolUse-denied.json`, `hooks/deny-hook.sh`, `turn-with-deny.txt`, `transcript-record-deny.jsonl`, `hooks-trust.txt`; Bram code: `src-tauri/src/lib.rs:53379` |
| subagents | Yes (`multi_agent`): own rollout per subagent, `SubagentStart`/`SubagentStop` with `agent_id` and `agent_transcript_path`, `session_meta.source.subagent`; not in the index; `Started`/`Completed` status lines | `hooks/SubagentStart.json`, `hooks/SubagentStop.json`, `subagent-session-meta-record.jsonl`, `turn-with-subagent.txt` |
| notification | No `Notification` hook event. A `notify = [...]` config program receives `agent-turn-complete` JSON per turn (twice on a first turn) | `hooks/notify-agent-turn-complete.json` |
| turn state | `UserPromptSubmit` starts a turn, `Stop` ends it (not on Esc interrupt); each carries `turn_id`. `SessionEnd` (`reason`) on exit | `hooks/UserPromptSubmit.json`, `hooks/Stop.json`, `turn-with-shell-declined.txt` |
| Agent→Bram channel | Loopback `curl` from a turn is **refused** under the default sandbox (`workspace-write`, `network_access: false`, `on-request`), with no approval prompt (`curl: (7) Failed to connect`), so the filesystem channel is needed. Filesystem: write `resources/.worklist-intent.json`, read `resources/.worklist-result.json` by nonce (the workspace-write sandbox refuses loopback) | Bram code: `src-tauri/src/lib.rs:51292`, `:51308`; `.claude/bram-reference/worklist-mechanics.md` §Codex transport; specimen: `turn-with-loopback.txt`, `hooks/PostToolUse-loopback.json`, `transcript-record-loopback.jsonl` |
| Bram→agent delivery | Bracketed paste into the PTY, Enter as a separate write | Bram code: `src-tauri/src/lib.rs:22089` (`bracketed_paste_turn_payload`); not captured as a screen |
| pasted images | `@<abs png>` typed or pasted is **not** an attachment: the prompt and rollout carry the literal text. The model answered `red` only by calling its own `view_image` on the path. The `-i/--image` flag attaches (`[Image #1]`, `input_image`, `local_image`). `@` opens a `Mentions` popup | `turn-with-image.txt`, `turn-with-image-mention-popup.txt`, `turn-with-image-flag.txt`, `transcript-record-image.jsonl`, `transcript-record-image-flag.jsonl`, `hooks/UserPromptSubmit-image*.json` |
| menus | `/` popup, `/model` picker, `codex resume` picker, `codex agents` overview; action-required prompts for shell and for edits | `dialog-slash-popup.txt`, `dialog-model-picker.txt`, `dialog-resume-picker.txt`, `agents-overview.txt`, `turn-with-apply-patch-approval.txt`; older specimens: [`../../pty-menu-specimens/2026-09-09-codex-model-picker.md`](../../pty-menu-specimens/2026-09-09-codex-model-picker.md), [`../../pty-menu-specimens/2026-06-18-codex-action-required-sudo-chown.md`](../../pty-menu-specimens/2026-06-18-codex-action-required-sudo-chown.md) |
| working status | `• Working (Ns • esc to interrupt)` | `turn-with-shell-working.txt` |
| interrupt and exit | Esc interrupts (`■ Conversation interrupted`); one Ctrl-C at an idle composer exits and prints `codex resume <id>` | `turn-with-shell-declined.txt`, `exit-ctrl-c.txt` |
| logged-out screen | Full-screen sign-in chooser, Enter starts sign-in | `logged-out.txt` |
| instructions file | `AGENTS.md` (Codex injects it as a synthetic user message) | Bram code: `src-tauri/src/lib.rs:36425`, `:41922` |
| screen shapes | Boxless, cursor-addressed TUI: footer `GPT-6-Astra high · <path>`; approval is a numbered list ending `Press enter to confirm or esc to cancel`; hooks-trust and folder-trust are overlays | `idle-composer.txt`, `turn-with-shell-approval.txt`, `hooks-trust.txt`, `first-run.txt` |
| model and version | `codex-cli 0.165.0`; `model = "gpt-6-astra"`, reasoning `high`; in every hook payload `model` and in `session_meta.cli_version` | `hooks/SessionStart.json`, `session-meta-record.jsonl`, `idle-composer.txt` |

## Review revisions

Signed: Codex (main thread), 2026-10-28. Distinguished ID allocation from
transcript persistence, corrected the description of redacted raw streams,
and recovered the capture tooling and schedules. The original captures and
observations are Claude's; these revisions did not run new model turns.

## Alignment — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
Closed the gaps listed in `resources/worklist-drafts/agent-specimens-align-to-codex.md`
using the same `capture/` driver, with new schedules `s4`-`s10`. Real model turns
ran against the real `~/.codex` login with injected logger hooks.

- **New screens:** `dialog-slash-popup`, `dialog-model-picker`,
  `dialog-resume-picker`, `agents-overview` (the only session list besides the
  picker), `turn-with-apply-patch-completed`, `turn-with-apply-patch-approval`,
  `turn-with-subagent`, `resume-then-clear`.
- **New payloads:** `apply_patch` `PreToolUse`/`PostToolUse`/`PermissionRequest`;
  `SessionStart` for `resume` and `clear`; `SessionEnd`; `SubagentStart`/`SubagentStop`;
  the `notify` program's JSON (the closest thing to a `Notification` payload).
- **New records:** `session-index-record.jsonl` (Codex's own `thread_name` lines),
  `subagent-session-meta-record.jsonl`.
- **Answers:** no `Notification` hook event in 0.165.0; subagents exist and are
  recorded as separate rollouts with `source.subagent`, not in the index.
- `.raw` files are redacted streams. The scratch path is `<workspace>`, but
  footer repaints split it with cursor-addressing escapes, so a few fragments of
  the scratch directory name remain in `.raw` (and `<scratch-root>` in the
  `.txt` footers). `agents-overview.txt` has no `.raw` because it listed other projects.

## Open items — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
Closed three holes listed in `resources/worklist-drafts/issue-462-specimen-open-items.md`
for Codex (codex-cli 0.165.0), real `~/.codex` login, injected hooks, scratch
repo outside the Bram repo.

- **Guard deny:** `hooks/deny-hook.sh` (selected through `PRETOOL_SCRIPT` in
  `hooks/run.sh`, schedule `s11`) denies a Bash call containing `DENYME` with
  reason `MARKER-REASON-7731`. The reason **reached the model** and it quoted
  it. Files: `turn-with-deny`, `hooks/PreToolUse-denied.json`,
  `transcript-record-deny.jsonl`.
- **Pasted images:** `@<abs png>` is **not** an attachment (no `images`/
  `local_images`, no `[Image #1]`); the model said red only by calling its own
  `view_image` on the path. `-i` does attach and the answer was red. Files:
  `turn-with-image`, `turn-with-image-mention-popup`, `turn-with-image-flag`,
  `hooks/UserPromptSubmit-image*.json`, `transcript-record-image*.jsonl`
  (`s12`, `s13`). The 64x64 red PNG was made with Python `zlib`+`struct`.
- **Loopback:** refused under the default sandbox, no prompt (`s14`). Files:
  `turn-with-loopback`, `hooks/*-loopback.json`, `transcript-record-loopback.jsonl`.
- **Surprises:** changing only the `PreToolUse` command re-raises `Hooks need
  review` (the first image run, pasted before the schedule was fixed, typed
  prose into that menu; the config restore undid any toggle). Codex's shell
  tool is a JS `exec` wrapper but hooks still report `tool_name: "Bash"`. The
  `.raw` files had the scratch path split by cursor addressing; fragments were
  scrubbed at byte level.
- **Residue:** `~/.codex/config.toml` backed up first and restored, confirmed
  identical with `diff` (Codex had added a folder-trust entry and `hooks.state`
  hashes). Four more scratch rollouts and their index lines remain under
  `~/.codex/sessions/2026/10/28/`. The loopback listener was killed by PID.
  No credential file was read or copied.
