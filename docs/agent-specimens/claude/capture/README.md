# Claude Code capture tooling

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread,
2026-10-28. Adapted from [`../../codex/capture/`](../../codex/capture/README.md).

## What is here

- `drive.py`: the 120x40 PTY driver. A schedule is a JSON array of
  `[seconds_since_start, keys_or_null, snapshot_name_or_null]`; at each entry
  it saves the cumulative bytes **before** sending that entry's keys. Keys use
  JSON escapes (`\u001b` Esc, `\u0003` Ctrl-C, `\r` Enter). It stops at the
  last entry or PTY EOF, terminates the child, and writes `_all.raw`.
  **A child that exits early (Ctrl-C twice) skips later snapshots**; use
  `_all.raw` as the capture (that is how `exit-ctrl-c` was made).
- `render.py`: the Codex renderer, unchanged (a minimal VT emulator, not a
  full one). Claude Code does not use the alternate screen, so a final-screen
  render also shows the exit lines printed after the TUI tears down.
- `run.sh`: the launcher. `bash run.sh REPO OUTDIR SCHEDULE [claude args...]`.
  It runs `claude` inside the scratch `REPO` with the six `CLAUDE_CODE_*`
  markers and `ANTHROPIC_API_KEY` unset, and `HOOKLOG_DIR=OUTDIR/hooklog` so the
  logger writes payloads outside the repo. It adds no `-c` overrides: Claude
  reads hooks from the repo's `.claude/settings.json`.
- `schedules/`: the schedules used (names below).

## What changed from the Codex copy

- `drive.py`: same algorithm and the same timing semantics; comments only
  (the dead "wait for EOF" loop is a no-op kept for reference).
- `run.sh` is new (Codex's is `../hooks/run.sh`, which injects hooks with
  `-c` overrides; Claude has no such flag, so `../hooks/settings.json` is
  copied to `REPO/.claude/settings.json` and `../hooks/log-hook.sh` to
  `REPO/.claude/log-hook.sh` instead).
- `../hooks/log-hook.sh` now writes to `$HOOKLOG_DIR` when set (falling back
  to the old relative path). `../hooks/settings.json` now registers 12 events.

## Scratch setup

```text
git init <scratch>/ws                       # a throwaway git repo, outside Bram
mkdir <scratch>/ws/.claude
cp hooks/settings.json hooks/log-hook.sh <scratch>/ws/.claude/
# trust the folder once by hand (the trust prompt is the first-run-trust screen)
bash capture/run.sh <scratch>/ws <scratch>/out1 capture/schedules/s1-working-approval-declined.json \
     --permission-mode default --name specimen-align-1 --session-id "$(uuidgen | tr A-Z a-z)"
python3 -I capture/render.py <scratch>/out1/turn-with-shell-approval.raw
```

Turns use the real account and cost a model turn each; keep prompts tiny.

## Schedules and the files they made

| Schedule | Extra args | Produced |
|---|---|---|
| `s1-working-approval-declined.json` | `--permission-mode default --name ... --session-id ...` | `turn-with-shell-working`, `-approval`, `-declined` (prompt, snapshot at 10 s and 16 s, Esc at 16.5 s) |
| `s2-completed-exit.json` | same | `turn-with-shell-completed` (Enter at 16 s approves). Its Ctrl-C pair is **2 s apart and did not exit**: the hint `Press Ctrl-C again to exit` expires first. Kept as evidence; `exit-ctrl-c` comes from `s3`. |
| `s3-resume-exit.json` | `--permission-mode default --resume <id>` | `resume-by-id` (snapshot at 9 s), `exit-ctrl-c` (Ctrl-C at 12 s and 12.3 s; the stream ends with the resume line) |
| `s4-interrupt.json` | `--name ... --session-id ...` | `interrupt-approval`, `interrupt-working`, `interrupt-esc` (Enter approves at 15 s, Esc at 20.5 s) |
| `s5-edit-tools.json` | same | `edit-write-approval`, `edit-edit-approval`, `edit-completed`, and the Write/Edit hook payloads |
| `s6-failing-command.json` | `--permission-mode default --session-id ...` | `failed-command` and `PostToolUseFailure.json` |

The schedules approve dialogs by sending a bare Enter on a timer; they are not
screen-aware. If a dialog is absent, the Enter lands in an empty composer and
is harmless; if the model is slow, the Enter can arrive before the dialog.

## Redaction

None is done by the tools. Published `.raw` files are **redacted streams**:
`<workspace>`, `<munged-workspace>`, `<scratchpad-root>` and `~` replace the
real paths, so they are not byte-exact and replaying one need not reproduce
its `.txt`. The `.txt` files were rendered from the unredacted stream and then
redacted. Session ids are throwaway and kept.
