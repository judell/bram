# OpenCode capture tooling

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread,
2026-10-28. Adapted from `../../codex/capture/`.

## What changed from the Codex tooling

- `drive.py` and `render.py` are copied unchanged (a header comment added).
  Schedule format is the same: `[seconds, keys|null, snapshot|null]`; a
  snapshot is the cumulative stream **before** that entry's keys are sent.
- `run.sh` replaces Codex's `hooks/run.sh`. OpenCode has no hook-command
  overrides; the logger is a **project plugin**, so the launcher copies
  `../hooks/logger.js` into `REPO/.opencode/plugins/` and passes
  `SPECIMEN_EVENTS_DIR`. It runs `opencode --standalone` (a private server),
  never the user's background service.
- `config/ask.opencode.json` sets `permission: {"*": "ask"}`, which makes
  every tool ask. `config/deny-echo.opencode.json` uses the 2.1.4 schema's
  `permissions` array (`{action, resource, effect}`) to deny `echo *`. A
  deny on the whole `shell` action removes the tool from the model's list,
  so the model never calls it; a narrow `resource` rule is needed to see the
  blocked-tool payload.
- Schedules (typing the prompt is the first entry; model latency varies, so
  adjust the times):
  - `approve-complete-exit.json`: working, approval, Enter (Allow once),
    completed, then Ctrl-C (exits with the resume hint).
  - `decline-esc.json`: Esc at the permission prompt (rejects).
  - `decline-select-reject.json`: Right, Right, Enter (Reject selected).
  - `interrupt-double-esc.json`: `sleep 20`, Allow once, Esc, Esc within 5 s.
  - `denied-by-config.json`: run with `deny-echo.opencode.json`.
  - `form-ctrlc.json` / `form-answer.json`: ask for the `question` tool;
    the first Enter allows the tool, the second picks option 1.
  - `idle-ctrl-c.json`: Ctrl-C at an idle composer with no session.

## Rendering and redaction

Render a stream with `python3 -I render.py CAPTURE.raw > CAPTURE.txt`. The
pack's new `.txt` files (turn phases, interrupt, form, permission-denied)
are renderer output of a named snapshot, i.e. the **final screen** at that
moment. The two `exit-ctrl-c*` `.txt` files are the plain text printed after
the TUI leaves the alternate screen (the renderer cannot show it). The
older files (`idle-composer.txt` and the rest of the first pass) are
ANSI-stripped repaint streams. The renderer is minimal and shows a few
glitches (a repainted logo overlaying the footer after exit).

The published `.raw` files are **redacted streams**: scratch path ->
`<workspace>`, home -> `~`. The tools do no redaction; the `.txt` was
rendered from the unredacted stream and then redacted, so a replay of the
redacted `.raw` can differ in column positions. The header tab bar shows
earlier sessions of the same folder as truncated tabs; that is why a few
tabs read "Shell", "Red or", etc.

## Residue

Every run creates sessions in the global `~/.local/share/opencode/opencode.db`
(even `--standalone`). Delete them afterwards with `opencode session delete <id>`
for the ids in the `session.created` events.
