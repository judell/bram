# Claude Code specimens

What Claude Code's terminal agent actually draws and emits, captured to sit
beside the Cursor, Codex and OpenCode packs for the provider comparison in
[#462](https://github.com/judell/bram/issues/462). Markers Bram anchors on
come from captures like these, never from guesses
(`docs/developing-bram.md` §Capturing terminal specimens).

- **CLI:** `claude`, version `2.1.312` (Claude Code)
- **Platform:** macOS (arm64), 2026-10-28
- **Capture:** `scripts/capture-terminal-specimen.py`, 120×40 pseudo-terminal,
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
- Resume is `claude --resume <id>` (or `--continue` for the latest). Not run
  here, because it would cost a turn; Bram's launch lines are cited in Seams.

## Hooks (`hooks/`)

`settings.json` (the scratch repo's `.claude/settings.json`) registers
`log-hook.sh <Event>` on each event; the script appends the stdin payload to a
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
| `Stop.json` | End of turn: `stop_hook_active`, `last_assistant_message`, `background_tasks`. |

`SessionStart.json` comes from the idle run, `PostToolUse.json`, `Stop.json`,
`PreToolUse.json` and `UserPromptSubmit.json` from the read-only turn, and
`PermissionRequest.json` and `Notification.json` from the permission turn.

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

## Not yet captured

- Esc and Ctrl-C behaviour mid-turn, `/exit` and the "Resume this session with"
  exit banner (Bram's tests quote one, `lib.rs:47009`). Nothing was sent after
  the prompt, to avoid approving anything.
- `--resume <id>` itself, and a pasted image (`[Image: …]` markers are handled
  at `lib.rs:39229`; no capture here).
- A `PreToolUse` deny and what the model sees of it.
- A loopback `curl` from inside a turn.
- Residue note: the turn sessions wrote a trust entry for the scratch repo into
  the real `~/.claude.json` and their transcripts into `~/.claude/projects/`.

## Seams

| Seam | Claude Code | Evidence |
|---|---|---|
| launch and resume by id | `claude --name '<title>' --session-id <uuid>` for a fresh named session; `claude --resume <id>`; `claude --continue` when no id | Bram code: `src-tauri/src/lib.rs:26805-26840`; `help.txt` |
| when the session id exists | Before the first message: `SessionStart` carries it, and with `--session-id` it is the id Bram chose | `hooks/SessionStart.json` |
| session store | `~/.claude/projects/<munged-cwd>/<id>.jsonl`, created at the first message, appended on resume | `transcript-record.jsonl`; Bram code: `lib.rs:553`, `lib.rs:30822` |
| naming | `--name` at launch (display title, also `custom-title` record); no rename needed | `idle-composer.txt`; `hooks/SessionStart.json` (`session_title`) |
| guard hook | `PreToolUse` in `.claude/settings.json`, command hook reading JSON on stdin; Bram registers `bram-guard guard claude-worklist` there, plus a menu hook on `PermissionRequest`, `PostToolUse`, `PermissionDenied`, `PreToolUse` (AskUserQuestion) | `hooks/PreToolUse.json`, `hooks/settings.json`; Bram code: `lib.rs:51533`, `lib.rs:51559` |
| turn state | `UserPromptSubmit` (start) and `Stop` (end) hooks; on screen, the spinner line with `esc to interrupt` and `✻ Worked for Ns` | `hooks/UserPromptSubmit.json`, `hooks/Stop.json`, `turn-with-shell-readonly.txt` |
| Agent→Bram channel | Loopback `curl` to the port in `resources/.bram-port` from the agent's Bash tool (per `conventions.md`), plus hooks calling out | Bram code: `app/__shell/conventions.md` (Transports); loopback-from-turn not captured |
| Bram→agent delivery | Bracketed paste (`ESC[200~ … ESC[201~`) after a composer clear (`ESC ^U`), then a carriage return | Bram code: `lib.rs:22072`, `lib.rs:22089` |
| pasted images | `[Image: source: /abs/path.png]` marker in the composer text; transcript records show `[Image: original WxH, …]` | not captured; Bram code: `lib.rs:39229` |
| menus | AskUserQuestion, permission prompts, model picker, resume picker | [`docs/pty-menu-specimens/`](../../pty-menu-specimens/) `*claude*` files, and `turn-with-shell.txt` |
| working status | `✢ Stewing… (running PreToolUse hooks… 0/2 · 2s)` then `(Ns · ↓N tokens)` with `esc to interrupt`; a verb that changes (Stewing, Decipher…) | `turn-with-shell-readonly.txt`; Bram code: `lib.rs:11955` matches the `Working (…esc to interrupt)` form used by Codex, so check it against this one |
| interrupt and exit | Esc interrupts (footer `esc to interrupt`); Ctrl-C/`/exit` end the session and print a resume line | `turn-with-shell-readonly.txt` (footer); exit banner not captured (Bram test quote: `lib.rs:47009`) |
| logged-out screen | None: the normal composer, footer `Not logged in · Run /login` | `logged-out.txt` |
| instructions file | `CLAUDE.md` (project root and parents), `@`-imports | Bram code: `lib.rs:52390` |
| screen shapes | Boxed trust prompt with numbered options; composer rule with the session name; numbered permission dialog with `Esc to cancel · Tab to amend` | `first-run-trust.txt`, `idle-composer.txt`, `turn-with-shell.txt` |
| model and version | Header line `Claude Code v2.1.312`, `Opus 5.5 · Claude Max`; `SessionStart.model` is `claude-opus-5-5`; transcript records carry `version` | `idle-composer.txt`, `hooks/SessionStart.json`, `transcript-record.jsonl` |
