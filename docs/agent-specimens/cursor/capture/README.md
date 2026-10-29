# Cursor capture tooling

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread,
2026-10-28.

## What changed from `../../codex/capture/`

- `drive.py` and `render.py` are copies of Codex's, with only the header comment
  changed. Schedule format, 120x40 PTY, and the "save the cumulative stream
  before sending this entry's keys" rule are the same. Fractional times
  (`6.6`) work, because the driver compares against wall-clock.
- The renderer is still minimal. It is enough for Cursor's differential
  redraws (relative `CSI A`/`K`), with one thing to know: Cursor draws
  transient lines, such as `Press Ctrl+C again to exit`, below the footer and
  erases them after about two seconds, so a snapshot taken a second late shows
  a screen without them. Take the snapshot within about a second of the key.
- `../hooks/run.sh` is a Cursor launcher: `bash run.sh REPO OUTDIR SCHEDULE
  [agent args...]`. Unlike Codex's it injects no hooks on the command line,
  because Cursor reads project hooks from `REPO/.cursor/hooks.json`. Copy
  `hooks/hooks.json` and `hooks/log-hook.sh` into `REPO/.cursor/` first. The
  launcher exports `HOOKLOG_DIR=OUTDIR/hooklog`, which `log-hook.sh` writes to.
  No `~/.cursor/hooks.json` is involved.
- Pass `--trust` so the folder-trust overlay does not need a keystroke
  (it records `.workspace-trusted` under `~/.cursor/projects/<munged-workspace>/`).
- `log-hook.sh` reads three environment variables the launcher passes through:
  `DENY_STYLE=both|agent-only|user-only` (what a `preToolUse` deny returns when the
  payload contains `DENYME`) and `SHELL_PERMISSION=ask|allow|deny` (the
  `beforeShellExecution` answer; default `ask` raises the permission prompt).

## Schedules (`schedules/`)

Each is `[[seconds, keys-or-null, snapshot-or-null], ...]`. The prompt text
and the Enter are separate entries, because a paste with a trailing carriage
return in one write is not submitted.

| Schedule | Command | Produced |
|---|---|---|
| `launch-prompt-approval` | `agent --trust` | `idle-composer`, the `Run this command?` prompt (`turn-with-shell-approval`) |
| `completed-then-ctrl-c` | `agent --trust` | `turn-with-shell-working` (snapshot `working2`), `-completed`, and one Ctrl-C |
| `idle-double-ctrl-c` | `agent --trust` | `exit-ctrl-c-idle` |
| `resume-then-exit` | `agent --trust --resume <id>` | `resume-by-id`, `exit-ctrl-c` |
| `esc-midturn` | `agent --trust` (`sleep 20`) | `interrupt-esc` |
| `ctrl-c-midturn` | `agent --trust` (`sleep 20`) | `interrupt-ctrl-c` |
| `esc-at-approval` | `agent --trust` | `turn-with-shell-declined-input` |
| `esc-then-enter-at-approval` | `agent --trust` | `turn-with-shell-declined` |
| `esc-midturn-then-end` | `agent --trust` | checks `stop` payloads when the driver ends the run |
| `edit-tool-write` | `agent --trust` | `turn-with-edit-completed`, the edit-tool payloads |
| `edit-denied` | `agent --trust` | `turn-with-edit-denied` (hook run with `DENY_STYLE` unset) |
| `shell-denied` | `agent --trust` | `turn-with-shell-denied` (run with `DENY_STYLE=agent-only`) |
| `loopback-curl` | `agent --trust` | loopback `curl` under the default configuration (sandbox disabled) |
| `loopback-curl-sandbox-allowed` | `agent --trust --sandbox enabled`, `SHELL_PERMISSION=allow` | loopback `curl` inside the sandbox |
| `slash-popup` | `agent --trust` | `slash-popup` |
| `rename-and-resume-list` | `agent --trust` | `rename`, `session-list` |

The port in the two `loopback-curl` schedules (`51251`) belonged to a throwaway
`python3 -m http.server` on 127.0.0.1; use any free port. Two of the
snapshots in the pack come from runs not scheduled here: `turn-with-shell-approval-sandbox`
(the `loopback-curl` schedule plus `--sandbox enabled`, with the default `ask`
answer) and the instructions-file and edit-an-existing-file checks, which
used `agent --trust -p "<prompt>"` (print mode) with no PTY.

## Redaction and replay

Same caveats as Codex's pack. The checked-in `.raw` files are **redacted
terminal streams**: escape sequences are kept and the account email,
workspace path, munged workspace path and home directory are replaced.
Replacement changes column positions, so replaying a `.raw` need not
reproduce its `.txt`. The `.txt` files were rendered from the unredacted stream and
then redacted. The recovered tools do no redaction. The pack's redaction was a
`sed`-style regex pass over the rendered text, the raw stream and the hook JSON.
