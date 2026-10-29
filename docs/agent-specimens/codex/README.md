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
| `resume-by-id` | `codex resume <id>` | The session replays its transcript (`› Run the shell command: ...`, `• Ran ...`, `Worked for 7s`) above a fresh composer. |
| `exit-ctrl-c` | Ctrl-C at an idle, empty composer | **One Ctrl-C exits.** The shell gets `Session ID: <id>`; after a real turn it also prints `Token usage: ...` and `To continue this session, run: codex resume <id>`. |

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
| `Stop.json` | `stop_hook_active`, `last_assistant_message`. Did **not** fire when the turn was interrupted with Esc (declined run). |

Every payload includes `transcript_path` (the rollout file) from the first
event on, which is why Bram can find the transcript before the turn ends.

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

## Not yet captured

- Pasted images (`@<png>` / clipboard paste): not tried.
- A `PreToolUse` deny and whether the model sees the denial reason: Bram's guard
  does this, but the logger only allows.
- `SessionStart` with `source` other than `startup` (resume, clear).
- Logged-out Codex after the sign-in choice (ChatGPT browser flow, device code,
  API key): the flows need a browser or credentials.
- Esc-Esc / `/` command menus: see Bram's existing
  [`pty-menu-specimens`](../../pty-menu-specimens/).
- Esc exit behaviour with a queued message, and `/exit` / `/quit`.

## Seams

| Seam | Codex | Evidence |
|---|---|---|
| launch and resume by id | `codex` / `codex resume <id>` / `codex resume --last` / `codex fork` | `resume-by-id.txt`, `help.txt`; Bram: `src-tauri/src/lib.rs:384` |
| when the session id exists | Allocated before any message: idle exit prints a `Session ID`; rollout/index persistence and `SessionStart` were observed after the first message | `exit-ctrl-c.txt`; `hooks/SessionStart.json` |
| session store | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl` plus append-only `~/.codex/session_index.jsonl` | `session-meta-record.jsonl`; Bram: `src-tauri/src/lib.rs:25191`, `:31252` |
| naming | Codex auto-names a thread into `session_index.jsonl` (`thread_name`, last wins); Bram appends `{"by":"bram","reassert":true}` entries to restore its names | Bram code: `src-tauri/src/lib.rs:31199` (`codex_titles_to_reassert`) |
| guard hook | `PreToolUse` (+ `PermissionRequest`, `PostToolUse`) in `~/.codex/config.toml` `[[hooks.*]]`, matcher `^(apply_patch\|Bash\|Write\|Edit\|mcp__.*)$`, user must trust each hook | `hooks/PreToolUse.json`, `hooks-trust.txt`; Bram code: `src-tauri/src/lib.rs:53379` |
| turn state | `UserPromptSubmit` starts a turn, `Stop` ends it (not on Esc interrupt); each carries `turn_id` | `hooks/UserPromptSubmit.json`, `hooks/Stop.json`, `turn-with-shell-declined.txt` |
| Agent→Bram channel | Filesystem: write `resources/.worklist-intent.json`, read `resources/.worklist-result.json` by nonce (the workspace-write sandbox refuses loopback) | Bram code: `src-tauri/src/lib.rs:51292`, `:51308`; `.claude/bram-reference/worklist-mechanics.md` §Codex transport |
| Bram→agent delivery | Bracketed paste into the PTY, Enter as a separate write | Bram code: `src-tauri/src/lib.rs:22089` (`bracketed_paste_turn_payload`); not captured as a screen |
| pasted images | not captured | n/a |
| menus | Model picker, action-required prompts | [`../../pty-menu-specimens/2026-09-09-codex-model-picker.md`](../../pty-menu-specimens/2026-09-09-codex-model-picker.md), [`../../pty-menu-specimens/2026-06-18-codex-action-required-sudo-chown.md`](../../pty-menu-specimens/2026-06-18-codex-action-required-sudo-chown.md) |
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
