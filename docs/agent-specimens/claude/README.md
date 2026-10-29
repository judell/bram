# Claude Code specimens

What Claude Code's terminal agent actually draws and emits, captured to sit
beside the Cursor, Codex and OpenCode packs for the provider comparison in
[#462](https://github.com/judell/bram/issues/462). Markers Bram anchors on
come from captures like these, never from guesses
(`docs/developing-bram.md` §Capturing terminal specimens).

- **CLI:** `claude`, version `2.1.312` (Claude Code)
- **Platform:** macOS (arm64), 2026-10-28
- **Capture:** `scripts/capture-terminal-specimen.py` for the first pass; the
  scheduled-keystroke driver in [`capture/`](capture/README.md) (adapted from
  Codex's) for the 2026-10-28 alignment pass. Both: 120×40 pseudo-terminal,
  run in a scratch git repo, `CLAUDE_CODE_*` markers and `ANTHROPIC_API_KEY`
  unset. Logged-out and first-run screens used throwaway `CLAUDE_CONFIG_DIR`s;
  the turns used the real account (a throwaway dir has no login).
- **Redacted:** the scratch workspace path (`<workspace>`), its munged form
  under `~/.claude/projects` (`<munged-workspace>`), the per-session scratch
  dir root (`<scratchpad-root>`), and the home directory (`~`). The account
  email never appeared in these captures. Redacting the path changes line
  widths, so a few borders in the `.txt` screens may not line up. Session ids
  are throwaway and kept.
- The stripper drops cursor-forward moves, so some `.txt` lines read as words
  run together (`Doyouwanttoproceed?`). The `.raw` files are the truth.

## Screens

| File | Command | What it shows |
|---|---|---|
| `first-run` | `claude` with an empty config dir | `Welcome to Claude Code v2.1.312`, then the theme picker (`Choose the text style that looks best with your terminal`, Auto / Dark / Light / colorblind / ANSI variants, with a syntax-highlight diff preview). |
| `first-run-trust` | `claude` in a folder not yet trusted | `Accessing workspace:` / path / `Quick safety check: Is this a project you created or one you trust?` / `1. Yes, I trust this folder`, `2. No, exit` / `Enter to confirm · Esc to cancel`. In the capture the cursor (`❯`) sits on `No, exit`, so a bare Enter exits; trusting took Down then Enter. Held sends matter here. |
| `logged-out` | `claude` with onboarding done, no login | **No login screen.** The normal composer draws, with `Not logged in · Run /login` in the footer, and an "Auto mode is now Claude Code's default permission mode" notice. Header reads `Opus 5.5 · API Usage Billing` (not a login signal). |
| `idle-composer` | `claude --name specimen-idle --session-id <uuid>` | Header (`Claude Code v2.1.312`, model, plan `Claude Max`, path, `◐ medium · /effort`), the name on the input rule (`── specimen-idle ─`), `❯ Try "fix typecheck errors"`, footer `⏵⏵ auto mode on (shift+tab to cycle) · ← for agents`. |
| `turn-with-shell-readonly` | `claude --permission-mode default "Run the shell command: echo hello-specimen"` | The spinner line (`✢ Stewing… (running PreToolUse hooks… 0/2 · 2s)`, later `(2s · ↓25 tokens)`), `⏺ Bash(echo hello-specimen)`, `⎿ Waiting…`, result, `✻ Worked for 3s`. **No permission prompt**: `echo` is read-only and runs unprompted even in `default` mode. |
| `turn-with-shell` | same, with `echo hello-specimen > specimen.txt` | The permission prompt: `Bash command` / the command / `Do you want to proceed?` / `❯ 1. Yes` / `2. Yes, and always allow access to <workspace> from this project` / `3. Yes, and switch to auto mode` / `4. No` / `Esc to cancel · Tab to amend`. **Enter runs the command** (option 1 is preselected). Not approved; the process was ended at the capture deadline. |
| `turn-with-shell-working` | prompt `Run the shell command: echo hello-specimen > specimen.txt`, Enter | Before the dialog: `❯ <prompt>`, a spinner line (`✢ Bloviating…`, verb varies), and the footer `⏸ manual mode on · esc to interrupt · ← for agents`. |
| `turn-with-shell-approval` | same, `--permission-mode default`, snapshot ~7 s in | `⏺ Bash(echo hello-specimen > specimen.txt)` / `⎿ Waiting…` / `Bash command` / a `Tip:` line / the model's one-line description / the command between dashed rules / `Do you want to proceed?` / `❯ 1. Yes`, `2. Yes, and always allow access to <workspace> from this project`, `3. Yes, and switch to auto mode`, `4. No` / `Esc to cancel · Tab to amend`. **Enter runs the command.** This is `turn-with-shell` re-captured on the same schedule as the other phases. |
| `turn-with-shell-declined` | Esc at that dialog | `⎿ Interrupted · What should Claude do instead?` / `✻ Churned for 1s · done 9:05 PM`, and an empty composer. The turn ends on screen, but **no `Stop` hook fired** (nor `PostToolUse`, `PostToolUseFailure`, `PermissionDenied`). |
| `turn-with-shell-completed` | Enter at the dialog | `⎿ (No output)` / the assistant's reply / `✻ Sautéed for 2s · done 9:06 PM`; the composer then shows a dim suggested next prompt (`cat specimen.txt`). |
| `resume-by-id` | `claude --resume <id>` (the `-completed` session) | The whole conversation replays above a fresh composer: `❯ <prompt>`, `⏺ Bash(...)`, the reply, `✻ Sautéed for 2s`, the name on the rule (`── specimen-align-2 ─`). `SessionStart` fires with `source: "resume"`. |
| `exit-ctrl-c` | Ctrl-C twice at an idle composer | The first press only shows `Press Ctrl-C again to exit` (it reverts after about a second; presses 2 s apart did not exit). The second exits and the shell gets `Resume this session with:` / `claude --resume <session-id>`. This is the full stream (`_all.raw`), since the child exited. |
| `interrupt-approval` | prompt asking for `python3 -c 'import time; time.sleep(30)'` | The dialog again, with `This command requires approval` and option 2 reading `Yes, and don’t ask again for: python3 *`. (A bare `sleep 20` is not run in the foreground: Claude says foreground sleep is blocked and backgrounds it, `Running in the background (↓ to manage)`.) |
| `interrupt-working` | Enter at the dialog, snapshot 5 s later | `⏺ Bash(...)` / `⎿ Running… (4s)` / `(ctrl+b to run in background)` / `✢ Meandering… (7s · ↓ 99 tokens)`, footer `esc to interrupt`. |
| `interrupt-esc` | Esc mid-run | `⎿ Interrupted · What should Claude do instead?` and an empty composer. No `Stop`, `PostToolUse` or `SessionEnd` followed. |
| `edit-write-approval` | prompt: use the Write tool to create `edit-specimen.txt` containing `hi` | `⏺ Write(edit-specimen.txt)` / `Create file` / path / a line-numbered preview (`1 hi`) / `Do you want to create edit-specimen.txt?` / `1. Yes`, `2. Yes, and switch to accept edits (auto-approve file edits and common file commands) for this session (shift+tab)`, `3. No`. Three options, not four. |
| `edit-edit-approval` | prompt: use the Edit tool to change `hi` to `hello` | `⏺ Update(edit-specimen.txt)` / `Edit file` / a `-`/`+` diff / `Do you want to make this edit to edit-specimen.txt?` and the same three options. The tool is `Edit` in hooks, `Update` on screen. |
| `edit-completed` | Enter at the Edit dialog | `⎿ Added 1 line, removed 1 line` with the diff, the reply, `✻ Brewed for 4s`. |
| `failed-command` | `ls /nonexistent-specimen-dir`, approved | A command that fails: `Listed 1 directory`, the reply quoting `Exit code 1` and the `ls` error. (A read-only `ls` of a path **outside** the workspace still raised the dialog, with only Yes / Yes-and-auto / No.) |
| `turn-with-deny` | prompt `Run this exact shell command and nothing else: echo DENYME`, hook answers deny; then a second prompt asking the model to quote the reason | `⏺ Bash(echo DENYME)` / `⎿  Error: PreToolUse:Bash hook error: MARKER-REASON-7731` (red error line, **no permission dialog**), and the model says it was blocked and quotes the marker. `turn-with-deny-working` is the same screen a moment earlier (the turn had already finished by then). |
| `turn-with-deny-exit2` | same, hook exits 2 with the marker on stderr | `⎿  Error: PreToolUse:Bash hook error: [$CLAUDE_PROJECT_DIR/.claude/log-hook.sh PreToolUse]: MARKER-REASON-7731 (exit2 stderr)`. The exit-2 form prefixes the hook command in brackets; the JSON form does not. |
| `turn-with-image-popup` | bracketed paste of `What colour is this image? @<workspace-sibling>/red.png`, snapshot before Enter | The composer wraps the text onto two lines and a file-suggestion row (`❯ <path>`) appears under the footer rule. `@path` is **not** drawn as an attachment chip; it stays literal text. |
| `turn-with-image-enter-swallowed` | same, one Enter, snapshot 20 s later | **The first Enter did not submit.** The text sits unsent in the composer: the Enter was consumed by the `@` suggestion. A second Enter submits (`turn-with-image-after-enter` is the screen between the two Enters). |
| `turn-with-image-working` / `turn-with-image` | second Enter | `⎿  Read ../../open-claude/red.png (168 bytes)` (a transcript attachment, not a tool call), then `⏺ The image is solid red, which looks like pure red (#FF0000).` / `✻ Baked for 1s`. |
| `turn-with-loopback-approval` | prompt `Run this exact shell command and nothing else: curl -sS http://127.0.0.1:<port>/` | The ordinary Bash dialog, `This command requires approval`, option 2 `Yes, and don’t ask again for: curl *`. A loopback `curl` is **not** auto-allowed, and nothing mentions a sandbox. |
| `turn-with-loopback` | Enter at the dialog | `⎿  loopback-ok-7731` (the body of the scratch `python3 -m http.server`), `✻ Brewed for 9s`. The listener logged the request. |
| `help.txt` | `claude --help` | Every flag in this version (`--session-id`, `--name`, `--resume`, `--continue`, `--permission-mode`, `--settings`, …). |

Related menu screens (permission variants, AskUserQuestion, model picker,
session resume picker) are already specimens:
[`docs/pty-menu-specimens/`](../../pty-menu-specimens/) (the `*claude*` files).

## Session id (`--session-id`, `--name`, `--resume`)

- `claude --session-id <uuid>` pre-allocates the id. `SessionStart` fired with
  that exact `session_id` before any message (`hooks/SessionStart.json`).
- **The transcript file does not exist until the first message.** The
  `idle-composer` run fired `SessionStart` but wrote no
  `~/.claude/projects/<munged-workspace>/<id>.jsonl`; the two turn sessions did.
- `--name <title>` is a display name: it appears on the composer rule, in
  `SessionStart.session_title` and `UserPromptSubmit.session_title`, and as
  `custom-title` / `agent-name` records at the top of the JSONL.
- Resume is `claude --resume <id>` (or `--continue` for the latest); see
  `resume-by-id`. It appends to the same JSONL, and `SessionStart` fires again
  with `source: "resume"` (`hooks/SessionStart-resume.json`).
- **Allocation and persistence differ.** With `--session-id`, the id exists
  at launch (`SessionStart` carries it); the JSONL appears only at the first
  message (see above). `exit-ctrl-c` printed a resume line for a session that
  had a transcript; an idle launch-and-exit with no message was not tried, so
  whether the printed id is then resumable is unknown.

## Hooks (`hooks/`)

`settings.json` (the scratch repo's `.claude/settings.json`) registers
`log-hook.sh <Event>` on twelve events (the first pass had seven); the script appends the stdin payload to a
per-event file and exits 0 (allow). Every payload shares `session_id`,
`transcript_path`, `cwd`, `scratchpad_dir`, and (after the first prompt)
`prompt_id`, `permission_mode`, `effort`.

| Payload | Notes |
|---|---|
| `SessionStart.json` | At launch, **before any message**: `source: "startup"`, `model`, `session_title`. The transcript path is already given. |
| `UserPromptSubmit.json` | `prompt` text, `prompt_id`, `session_title`. |
| `PreToolUse.json` | `tool_name: "Bash"`, `tool_input: {command, description}`, `tool_use_id`. The guard's enforcement point. Fires for every tool call, including auto-allowed ones. |
| `PermissionRequest.json` | Fires when the dialog is shown (non-read-only command): `tool_input`, `permission_suggestions`. Bram's menu hook uses this event. |
| `Notification.json` | The "Claude needs your permission" message, fired alongside the dialog. |
| `PostToolUse.json` | After the tool ran, with its response. |
| `PreToolUse-Write.json` / `PreToolUse-Edit.json` | The edit tools. Write: `tool_input {file_path, content}`. Edit: `tool_input {file_path, old_string, new_string, replace_all}`. Same envelope and `tool_use_id` as Bash. A guard matching `Bash` only would miss these. |
| `PermissionRequest-Write.json` / `-Edit.json` | Same `tool_input`, plus `permission_suggestions: [{type: "setMode", mode: "acceptEdits", destination: "session"}]` (the dialog's option 2). |
| `PostToolUse-Write.json` / `-Edit.json` | `tool_response` is an object here, not a string: Write `{type: "create", filePath, content, structuredPatch: [], originalFile: null, userModified}`; Edit `{filePath, oldString, newString, originalFile, structuredPatch: [{oldStart, ..., lines}], userModified, replaceAll}`. Plus `duration_ms`. |
| `PreToolUse-denied.json` | The `PreToolUse` payload of the denied call (`tool_input.command: "echo DENYME"`). It is the **only** hook that fired for that tool call: no `PermissionRequest`, `PostToolUse`, `PostToolUseFailure` or `PermissionDenied` followed. `Stop` and `SubagentStop` fired at the end of the turn as usual. |
| `UserPromptSubmit-image.json` | `prompt` is the literal typed text with the absolute `@<path>`; no `attachments` field, nothing image-shaped. |
| `PreToolUse-loopback.json`, `PermissionRequest-loopback.json`, `PostToolUse-loopback.json` | The `curl` turn. `PermissionRequest.permission_suggestions` is an `addRules` allow for the exact command (`destination: "localSettings"`). `PostToolUse.tool_response` is `{stdout, stderr, interrupted, isImage, noOutputExpected}`; there is no sandbox field. |
| `PostToolUseFailure.json` | A Bash command that exited non-zero: `error` (`"Exit code 1\nls: ..."`), `is_interrupt: false`, `duration_ms`. The analogue of Codex's sandbox-denied `PostToolUse`. |
| `PreToolUse-interrupted.json` | The `PreToolUse` of the Esc-interrupted run. It is the last tool event: **no `PostToolUse`, `PostToolUseFailure` or `Stop` followed**. |
| `SessionStart-resume.json` | `source: "resume"`, plus `seconds_since_last_response`, `context_tokens`, `prompt_cache_likely_expired`, `estimated_cache_write_usd`. `session_title` is kept from the first run. |
| `SessionEnd.json` | Fired by the double Ctrl-C exit: `reason: "prompt_input_exit"`. Not fired when the driver killed the child. |
| `SubagentStop.json` | Fires **at the end of an ordinary completed turn**, alongside `Stop`, with `agent_type: ""` and `last_assistant_message` equal to the suggested next prompt (`cat specimen.txt`), so it appears to be the prompt-suggestion helper, not a user-visible subagent. Not fired on declined or interrupted turns. A consumer keyed on `SubagentStop` would see phantom subagents. |
| `Stop.json` | End of turn: `stop_hook_active`, `last_assistant_message`, `background_tasks`. |

`SessionStart.json` comes from the idle run, `PostToolUse.json`, `Stop.json`,
`PreToolUse.json` and `UserPromptSubmit.json` from the read-only turn, and
`PermissionRequest.json` and `Notification.json` from the permission turn.
In the 2026-10-28 runs `Notification` did **not** fire in the first five seconds of the dialog (the earlier pass held it longer), and no `PermissionDenied` fired after declining. No payload for either of those events is new.

## Transcript (`transcript-record.jsonl`)

Stored at `~/.claude/projects/<munged-workspace>/<session-id>.jsonl`; the
munged name is the cwd with `/` (and `.`/`_`) turned into `-`, matching
`encode_path_for_filename` (`src-tauri/src/lib.rs:553`). One file per session,
and `--resume` appends to the same file. A user record is
`{"type": "user", "message": {"role": "user", "content": "<text>"}, "promptId", "uuid", "sessionId", "cwd", "version", "origin": {"kind": "human"}, …}`,
plain string content, no wrapper tags. Other record types seen in a one-turn
session: `custom-title`, `agent-name`, `mode`, `permission-mode`,
`file-history-snapshot`, `attachment` (environment, model, skill listing…),
`assistant`, `last-prompt`, `tools-used`, `api-request*`.

## Guard deny and what the model sees (2026-10-28)

`hooks/log-hook.sh` now answers a `PreToolUse` payload containing `DENYME`
(`DENY_MODE=json`, the default):
`{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"MARKER-REASON-7731"}}`
on stdout, exit 0. Alternatively (`DENY_MODE=exit2`) it writes the marker to
stderr and exits 2. Both blocked the call.

- **The reason reaches the model, in both forms.** The transcript
  (`transcript-record-deny.jsonl`) holds a `tool_result` with
  `is_error: true` and `content: "PreToolUse:Bash hook error: MARKER-REASON-7731"`;
  asked to quote what it was told, the model quoted exactly that string. With exit 2
  (`transcript-record-deny-exit2.jsonl`) the content is
  `PreToolUse:Bash hook error: [<hook command>]: MARKER-REASON-7731 (exit2 stderr)`
  and the model even reported the exit status.
- **The wording is "hook error", not "denied".** Even a clean JSON deny is
  presented to the model and on screen as `PreToolUse:Bash hook error: <reason>`.
  A guard's reason text should therefore be written as an instruction to the model.
- The user-record envelope carries `toolDenialKind: "permission-rule"` and
  `permissionDecision: {decision: "reject", source: "hook", reasonType: "hook", reasonCode: "hook:PreToolUse"}`,
  and `toolUseResult` is the same error string.
- **No dialog, no later hooks.** Nothing follows `PreToolUse` for the call
  (see `hooks/PreToolUse-denied.json`); the turn continues and ends with `Stop`.
  `PermissionDenied` did not fire.

## Pasted images (2026-10-28)

Bram's frame is `@<absolute path to png>` inside bracketed-paste text.
A 64x64 solid red PNG (zlib+struct) was sent as
`What colour is this image? @<path>/red.png`.

- **The image arrives and is understood.** The reply was "The image is solid
  red, which looks like pure red (#FF0000)."
- **Mechanism:** the typed text stays literal in the user record
  (`message.content` is the plain string, `promptSource: "typed"`); the CLI adds a separate
  `attachment` record `{type: "file", filename, content: {type: "image", file: {base64, type: "image/png", originalSize: 168, dimensions: {originalWidth: 64, ...}}}, displayPath}`
  right after it (`transcript-record-image.jsonl`; a `[Image: ...]` marker was **not** seen).
  The screen shows `⎿  Read <relative path> (168 bytes)`. No tool call, so no
  `PreToolUse`/`PostToolUse` fired, and `UserPromptSubmit` carried only the text.
- **Delivery caveat:** the `@` token opens a file-suggestion popup, and the
  **first Enter after the paste was swallowed by it**; the text stayed
  unsent until a second Enter. A paste followed by one carriage return, which
  is Bram's delivery frame, would therefore not submit a turn that contains an
  `@path`. Typing the same text key by key behaved the same (an earlier run).
  Whether a trailing space before the Enter avoids it was not tried.

## Loopback `curl` from inside a turn (2026-10-28)

`curl -sS http://127.0.0.1:<port>/` against a `python3 -m http.server` in the
scratch dir (default permission mode) raised the normal approval dialog, not
a refusal; after Enter it ran and printed the page. There was no sandbox
notice. The listener logged the request. So Bram's
loopback channel works from a Claude turn, gated only by the approval prompt
(and by an allowlist entry in a real project, per `conventions.md`).

## Session metadata (`session-meta-record.jsonl`)

The non-conversation records of one JSONL, in file order: `custom-title`
(`customTitle`), `agent-name` (`agentName`), `mode`, `permission-mode`,
`last-prompt` (`lastPrompt`, `leafUuid`), `tools-used`. The set is **written
again at each resume** (three copies here: the first run, the `--resume`, and
the closing exit), so the name on a session is the last `custom-title` /
`agent-name` record, not the first. There is no summary record in these
sessions (none appeared with `--name`).

## Not yet captured

- A `[Image: …]` marker in a transcript (`lib.rs:39229` handles them): the
  `@path` form produces an `attachment` record instead; the marker's source
  (likely a true clipboard paste of image bytes) was not captured.
- Whether a trailing space avoids the `@path` Enter swallowing.
- A `PermissionDenied` payload: it fired neither when the dialog was declined with
  Esc nor on a hook deny; what triggers it (perhaps only auto-mode denials)
  was not isolated.
- A `Notification` payload from this pass (the first pass has one).
- An idle launch-and-exit with no message (is the printed resume id usable?).
- The `/exit` slash command and a single-Ctrl-C exit at a non-idle prompt.
- Loopback `curl` under a sandboxed or auto mode (only default mode, approved by hand, was run).
- Subagent transcripts of a user-visible subagent (`SubagentStop` fired only
  for the prompt-suggestion helper; its `agent_transcript_path` was not read).
- Residue: the trust entry for the scratch repo in the real `~/.claude.json`
  (from the first pass), and the sessions of both passes under
  `~/.claude/projects/<munged-workspace>/`. The second pass added the
  transcripts for `specimen-align-1`, `-2`, `-4`, `-5`, an unnamed
  failing-command session, and subagent transcript directories. No credential
  file was read or copied.

## Seams

| Seam | Claude Code | Evidence |
|---|---|---|
| launch and resume by id | `claude --name '<title>' --session-id <uuid>` for a fresh named session; `claude --resume <id>` (replays the conversation, `SessionStart` `source: "resume"`); `claude --continue` when no id | Bram code: `src-tauri/src/lib.rs:26805-26840`; `help.txt`, `resume-by-id.txt`, `hooks/SessionStart-resume.json` |
| when the session id exists | Before the first message: `SessionStart` carries it, and with `--session-id` it is the id Bram chose | `hooks/SessionStart.json` |
| session store | `~/.claude/projects/<munged-cwd>/<id>.jsonl`, created at the first message, appended on resume | `transcript-record.jsonl`; Bram code: `lib.rs:553`, `lib.rs:30822` |
| naming | `--name` at launch (display title, also `custom-title` / `agent-name` records, rewritten on each resume; last wins); no rename needed | `idle-composer.txt`; `hooks/SessionStart.json` (`session_title`); `session-meta-record.jsonl` |
| guard hook | `PreToolUse` in `.claude/settings.json`, command hook reading JSON on stdin; Bram registers `bram-guard guard claude-worklist` there, plus a menu hook on `PermissionRequest`, `PostToolUse`, `PermissionDenied`, `PreToolUse` (AskUserQuestion) | `hooks/PreToolUse.json`, `hooks/PreToolUse-denied.json`, `transcript-record-deny.jsonl`, `turn-with-deny.txt`, `hooks/settings.json`; A deny is `permissionDecision: "deny"` + `permissionDecisionReason` (JSON on stdout) or exit 2 with stderr; both block the call with no dialog, and the reason reaches the model as `PreToolUse:Bash hook error: <reason>`. Bram code: `lib.rs:51533`, `lib.rs:51559` |
| turn state | `UserPromptSubmit` (start) and `Stop` (end) hooks (`Stop` is absent when the turn is declined or interrupted, so a hook-only tracker never sees those turns end); on screen, the spinner line with `esc to interrupt` and `✻ Worked for Ns` | `hooks/UserPromptSubmit.json`, `hooks/Stop.json`, `turn-with-shell-readonly.txt` |
| Agent→Bram channel | Loopback `curl` to the port in `resources/.bram-port` from the agent's Bash tool (per `conventions.md`), plus hooks calling out. Verified: `curl -sS http://127.0.0.1:<port>/` from a turn raised the normal approval dialog (no sandbox) and returned the page | `turn-with-loopback-approval.txt`, `turn-with-loopback.txt`, `hooks/PostToolUse-loopback.json`; Bram code: `app/__shell/conventions.md` (Transports) |
| Bram→agent delivery | Bracketed paste (`ESC[200~ … ESC[201~`) after a composer clear (`ESC ^U`), then a carriage return | Bram code: `lib.rs:22072`, `lib.rs:22089` |
| pasted images | `@<abs path>.png` in pasted text works: the CLI attaches the pixels (transcript `attachment` record with base64 + dimensions; screen `⎿ Read <path> (N bytes)`; the model answered red). The text stays literal; no `[Image: …]` marker seen. **The first Enter after the paste is swallowed by the `@` file popup**; a second submits | `turn-with-image-*.txt`, `transcript-record-image.jsonl`, `hooks/UserPromptSubmit-image.json`; Bram code: `lib.rs:39229` (marker form, not reproduced) |
| edit-tool hook payload | `PreToolUse` / `PermissionRequest` / `PostToolUse` with `tool_name` `Write` or `Edit` (screen label `Update`), `tool_input` `{file_path, content}` or `{file_path, old_string, new_string, replace_all}`, and an object `tool_response` | `hooks/PreToolUse-Write.json`, `PreToolUse-Edit.json`, `PostToolUse-Write.json`, `PostToolUse-Edit.json`, `PermissionRequest-Edit.json` |
| blocked or failed tool | A failing command fires `PostToolUseFailure` (`error`, `is_interrupt`); declining the dialog or Esc fires nothing after `PermissionRequest`; `PermissionDenied` was not observed | `hooks/PostToolUseFailure.json`, `turn-with-shell-declined.txt` |
| approval dialog shapes | Bash: 4 options (Yes / always allow / switch to auto mode / No); outside-workspace read: 3 options; Write/Edit: 3 options (Yes / switch to accept edits / No). Enter picks the preselected `1. Yes` | `turn-with-shell-approval.txt`, `failed-command` approval (`interrupt-approval.txt`), `edit-write-approval.txt` |
| menus | AskUserQuestion, permission prompts, model picker, resume picker | [`docs/pty-menu-specimens/`](../../pty-menu-specimens/) `*claude*` files, and `turn-with-shell.txt` |
| working status | `✢ Stewing… (running PreToolUse hooks… 0/2 · 2s)` then `(Ns · ↓N tokens)` with `esc to interrupt`; a verb that changes (Stewing, Decipher…) | `turn-with-shell-readonly.txt`; Bram code: `lib.rs:11955` matches the `Working (…esc to interrupt)` form used by Codex, so check it against this one |
| interrupt and exit | Esc mid-turn or at a dialog prints `⎿ Interrupted · What should Claude do instead?` and ends the turn **without a `Stop` hook**; Ctrl-C twice (second within about a second) exits, fires `SessionEnd` (`prompt_input_exit`) and prints `Resume this session with:` / `claude --resume <id>`; the first Ctrl-C only shows a hint | `interrupt-esc.txt`, `turn-with-shell-declined.txt`, `exit-ctrl-c.txt`, `hooks/SessionEnd.json`, `hooks/PreToolUse-interrupted.json`; Bram test quote of the banner: `lib.rs:47009` |
| logged-out screen | None: the normal composer, footer `Not logged in · Run /login` | `logged-out.txt` |
| instructions file | `CLAUDE.md` (project root and parents), `@`-imports | Bram code: `lib.rs:52390` |
| screen shapes | Boxed trust prompt with numbered options; composer rule with the session name; numbered permission dialog with `Esc to cancel · Tab to amend` | `first-run-trust.txt`, `idle-composer.txt`, `turn-with-shell.txt` |
| model and version | Header line `Claude Code v2.1.312`, `Opus 5.5 · Claude Max`; `SessionStart.model` is `claude-opus-5-5`; transcript records carry `version` | `idle-composer.txt`, `hooks/SessionStart.json`, `transcript-record.jsonl` |

## Alignment — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
Captured with `claude` 2.1.312 in the same scratch repo as the first pass,
with hooks logging to a directory outside the repo. Added:

- Screens: `turn-with-shell-working`, `-approval`, `-declined`, `-completed`;
  `resume-by-id`; `exit-ctrl-c`; `interrupt-approval`, `-working`, `-esc`;
  `edit-write-approval`, `edit-edit-approval`, `edit-completed`;
  `failed-command`. (`turn-with-shell` and `turn-with-shell-readonly` are kept.)
  Each has a `.txt` rendered through `capture/render.py`; the `.raw` files are
  redacted streams, not byte-exact.
- Hooks: `PreToolUse-Write/Edit`, `PermissionRequest-Write/Edit`,
  `PostToolUse-Write/Edit`, `PostToolUseFailure`, `PreToolUse-interrupted`,
  `SessionStart-resume`, `SessionEnd`, `SubagentStop`; `settings.json` now
  registers twelve events and `log-hook.sh` honours `$HOOKLOG_DIR`.
- `session-meta-record.jsonl`.
- `capture/`: `drive.py`, `render.py`, `run.sh`, six schedules and a README.
- Seams cells: interrupt and exit, turn state, launch and resume, naming; new
  rows for the edit-tool payload, blocked or failed tools, and dialog shapes.
- Findings worth a second look: `Stop` does not fire on a declined or
  interrupted turn; `SubagentStop` fires on every completed turn; a bare
  `sleep 20` is backgrounded rather than run, so interrupt tests need another
  long command; the first Ctrl-C only shows a hint and expires within about a
  second.

## Open items — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
Captured with `claude` 2.1.312 for the open items in #462, in the same scratch
repo, through the same driver. Added:

- Screens: `turn-with-deny`, `-deny-working`, `-deny-exit2`;
  `turn-with-image-popup`, `-enter-swallowed`, `-after-enter`, `-working`,
  `turn-with-image`; `turn-with-loopback-approval`, `turn-with-loopback`
  (`.txt` rendered through `capture/render.py`, `.raw` redacted).
- Hooks: `PreToolUse-denied`, `UserPromptSubmit-image`,
  `PreToolUse-loopback`, `PermissionRequest-loopback`, `PostToolUse-loopback`;
  `log-hook.sh` now denies a `PreToolUse` containing `DENYME` (`DENY_MODE=json`
  or `exit2`).
- Transcripts: `transcript-record-deny.jsonl`, `-deny-exit2.jsonl`,
  `transcript-record-image.jsonl` (the image record keeps the 168-byte PNG as base64).
- Schedules: `s7-guard-deny`, `s7b-guard-deny-exit2`, `s8-pasted-image`
  (bracketed paste, Enter, then a second Enter), `s9-loopback`. The scratch
  paths in them are redacted, so adjust before reuse.
- Findings: the deny reason reaches the model in both forms, worded as
  `hook error`; a pasted `@png` arrives as an image and was read as red; the
  first Enter after a paste containing `@path` is swallowed by the file popup;
  a loopback `curl` runs after the normal approval dialog.
- Residue outside the scratchpad: more session transcripts under
  `~/.claude/projects/<munged-workspace>/` (`specimen-deny`, `-deny2`,
  `-image`, `-loopback`, plus one aborted image run). No settings file or
  credential was touched; the scratch `python3 -m http.server` was stopped.
