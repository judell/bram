# Cursor CLI specimens

What Cursor's terminal agent actually draws and emits, captured for the
provider feasibility discussion in
[#462](https://github.com/judell/bram/issues/462). Markers Bram anchors on
come from captures like these, never from guesses
(`docs/developing-bram.md` §Capturing terminal specimens).

- **CLI:** `agent` (also installed as `cursor-agent`), version
  `2026.10.22-27dcf5f`
- **Platform:** macOS (arm64), 2026-10-28
- **Capture:** `scripts/capture-terminal-specimen.py`, 120×40 pseudo-terminal,
  run in a scratch git repo
- **Redacted:** the account email (`<user-email>`), the scratch workspace path
  (`<workspace>`), its munged form under `~/.cursor/projects`
  (`<munged-workspace>`), and the home directory (`~`). Redacting the path
  changes the width of a few lines, so box borders in the `.txt` screens may
  not line up. Conversation ids are throwaway and kept.

Each screen has an ANSI-stripped `.txt` and the raw bytes in `.raw`.

## Screens

| File | Command | What it shows |
|---|---|---|
| `logged-out` | `agent` while logged out | Full-screen splash: `Cursor Agent` / version / `Press any key to log in...`. **Any key starts login**, so pane sends must be held. |
| `first-run` | `agent` in a new folder | Boxed `⚠ Workspace Trust Required`, `▶ [a] Trust this workspace` / `[q] Quit`, "Enter to select". **Enter trusts the folder**, so sends must be held. |
| `idle-composer` | `agent`, then `a` | The composer is ready: `→ Plan, search, build anything`, with the model (`Auto`) and `<path> · <branch>` in the footer. |
| `turn-with-shell` | `agent "Run the shell command: echo hello-specimen"` | Working line `⠰⠰ Running  31 tokens`, the composer as `→ Add a follow-up … ctrl+c to stop`, and the footer `1 task` / `Auto · 4.6%`. Then the permission prompt: `Run this command?` / `→ Run (once) (y)` / `Run Everything (shift+tab)` / `Skip & tell the agent what to do instead (esc or n)`. **Enter or `y` runs the command.** |
| `help.txt` | `agent --help` | Every flag in this version, including some the docs page doesn't list (`--auto-review`, `--add-dir`, `-e/--endpoint`). |
| `create-chat*.txt` | `agent create-chat` | A new chat id printed before anything launches: under a PTY, with no TTY, and inside a repo. |

A delivery note that isn't a file: text and a carriage return written in
**one** write were typed into the composer but not submitted. Send the paste and
the Enter as separate writes.

## Hooks (`hooks/`)

`hooks.json` (the flat `{"version": 1, …}` format) and `log-hook.sh` are the
logger that produced these payloads. It allows `preToolUse` and answers `ask`
for `beforeShellExecution`, which is what raised the permission prompt above.

| Payload | Notes |
|---|---|
| `sessionStart.json` | Fires at launch, **before any message**: `conversation_id` (= `session_id`), `model`, `cursor_version`, `workspace_roots`, `user_email`. `transcript_path` is `null`. |
| `beforeSubmitPrompt.json` | The prompt text and `attachments: []`, where a pasted image would appear (untested). |
| `preToolUse.json` | `tool_name: "Shell"`, `tool_input: {command, cwd, timeout}`. This is the guard's enforcement point. |
| `beforeShellExecution.json` | `command`, `sandbox: false`, and the first non-null `transcript_path`. |

## Transcript (`transcript-record.jsonl`)

Stored at
`~/.cursor/projects/<munged-workspace>/agent-transcripts/<conversation-id>/<conversation-id>.jsonl`,
one file per conversation. A user record is
`{"role": "user", "message": {"content": [{"type": "text", "text": …}]}}`, with
the text wrapped in `<timestamp>` and `<user_query>` tags.

## Not yet captured

`stop` payloads (completed, aborted, error), Esc and Ctrl-C behaviour, a
loopback `curl` from inside a turn (does the sandbox allow it?), whether
`@<png>` attaches an image, naming a chat, and a `preToolUse` deny (does the
model see `agent_message`?).
