# Codex capture tooling

Signed: Codex (main thread), 2026-10-28. Recovered from Claude's scratch
directory for the original captures. Claude authored `drive.py`, `render.py`
and the three schedules; Codex removed an unused machine-specific import
path from the driver and made [`../hooks/run.sh`](../hooks/run.sh) portable.
The capture and rendering algorithms are otherwise preserved.

## What is retained

- `drive.py`: a 120×40 PTY driver. A schedule is a JSON array of
  `[seconds_since_start, keys_or_null, snapshot_name_or_null]`. At each entry
  it saves the cumulative bytes **before** sending that entry's keys. It
  stops at the last entry or PTY EOF, terminates the child, and saves
  `_all.raw`. A child that exits early can prevent later named snapshots.
- `render.py`: the minimal renderer used to turn a cumulative stream into
  final-screen text. It handles the cursor moves and erases used in these
  captures; it is not a full VT emulator (for example, it does not model
  wide/combining character widths, all terminal modes, or scrollback).
- `schedules/s1.json`: Escape at 7 s, prompt text at 9 s, Enter at 11 s,
  snapshot `x` and **`y` at 20 s**, snapshot `y-done` at 28 s, end at 30 s.
  The `y` can approve a pending command. This is a historical schedule,
  not a screen-aware automation; inspect the intended state before use.
- `schedules/s2.json`: snapshots around Escape and two Ctrl-C keystrokes,
  for the idle-exit probe.
- `schedules/s3.json`: a snapshot at 8 s, Ctrl-C at 9 s, end at 11 s,
  used with `resume <id>`.

The schedules retain their original names and contents. The complete
per-file command/schedule mapping and earlier schedules were not retained
here, so these are the recovered tools, not a claim that every published
screen can be recreated exactly. Timing, hook trust, account/config state,
and model output also affect what a new run draws.

## Running the recovered tools

From the repository root, the general invocation is:

```text
bash docs/agent-specimens/codex/hooks/run.sh REPO OUTDIR SCHEDULE [codex args...]
```

`REPO` must be an existing scratch repository. The launcher creates `OUTDIR`
and `OUTDIR/hooklog`, resolves relative paths before changing directory, and
locates its driver and logger relative to itself. It passes the six logger
hooks as command-line overrides and unsets `OPENAI_API_KEY`. It uses the
caller's Codex configuration/login; folder and hook trust can still affect
the run, as described in the [capture account/residue notes](../README.md).
Choose the schedule and extra Codex arguments explicitly; the launcher
does not select a scenario or approve hooks automatically.

The driver can also capture any command without the Codex wrapper:

```text
python3 -I docs/agent-specimens/codex/capture/drive.py OUTDIR SCHEDULE -- COMMAND [args...]
```

For this direct invocation, create `OUTDIR` first. Render a captured stream
with:

```text
python3 -I docs/agent-specimens/codex/capture/render.py CAPTURE.raw > CAPTURE.txt
```

## Redaction and replay

The published `.raw` files contain path replacements such as `<workspace>`
and `~`; they are **redacted terminal streams**, not byte-exact originals.
The `.txt` snapshots were also redacted. Replacing text with shorter strings
changes column positions while leaving cursor-addressing escapes in place,
so replaying a published `.raw` may differ from its `.txt`. Use these files
as evidence of text and prompt structure, not pixel/column-exact fixtures.

For a new capture, render the original stream before redacting the output,
and document replacements in both artifacts. The recovered tools perform
no redaction themselves.

## Verification of the recovery

Codex checked shell/Python syntax, JSON/JSONL parsing, and documentation
links. A fake Codex executable exercised the launcher through a real PTY,
parsed its TOML overrides, invoked all six hook loggers, and produced a
snapshot that the recovered renderer read correctly. The check covered
paths with spaces and macOS path aliases. No real model turn was run.

## Alignment additions — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
`drive.py`, `render.py` and `../hooks/run.sh` are unchanged. New schedules
(all run through `run.sh REPO OUTDIR SCHEDULE [codex args]` in a scratch repo,
real `~/.codex` login; back up and restore `~/.codex/config.toml` around them):

| Schedule | Extra codex args | Produces |
|---|---|---|
| `s4-apply-patch.json` | none | `turn-with-apply-patch-completed`, `PreToolUse`/`PostToolUse-apply_patch` |
| `s5-apply-patch-approval.json` | `-a on-request -s read-only` | `turn-with-apply-patch-approval`, `PermissionRequest-apply_patch` |
| `s6-dialogs.json` | none | `dialog-slash-popup` (snapshot `slash2`), `dialog-model-picker` (`model2`) |
| `s7-picker-and-agents.json` | `resume` or `agents` | `dialog-resume-picker`, `agents-overview` (snapshot `a`) |
| `s8-resume-and-clear.json` | `resume <id>` | `SessionStart` `resume` and `clear`, `resume-then-clear` |
| `s9-subagent.json` | `-c hooks.SubagentStart=[...] -c hooks.SubagentStop=[...]` pointing at `log-hook.sh` | `turn-with-subagent`, `SubagentStart`/`SubagentStop` |
| `s10-notify-and-session-end.json` | `-c notify=["bash", <script>] -c hooks.SessionEnd=[...]` | `notify-agent-turn-complete`, `SessionEnd` |

Notes learned:

- **A snapshot is saved before the same entry's keys are sent**, so a snapshot
  named for the screen you want must be scheduled *after* the key that opens it
  (the `slash` vs `slash2` and `model` vs `model2` pairs: only the second of each is the dialog).
- **Hook trust:** the first launch with any new hook shows `Hooks need review`;
  schedules `s9`/`s10` open with `Down`, `Enter` (choose "Trust all and continue")
  before typing. With the folder already trusted in the config, no folder
  prompt appears. Do **not** type prose into that screen: spaces and Enter toggle
  hooks in the review menu (one Bram hook was toggled off by an early probe;
  restoring the config undid it).
- **Latency:** an approval prompt for `apply_patch` took 25-35 s to appear in the
  Bram-hooks-enabled setup; `s5` waits 38 s before pressing Enter.
- The schedules hold Enter on `Trust all` only because the logger hooks must run;
  they are not screen-aware.
- Redaction (`<workspace>` etc.) was applied afterwards by a throwaway script
  that is not retained; `.raw` redaction is by plain byte replacement.

## Open-items additions — 2026-10-28

Signed: Claude subagent (Sonnet), delegated by Jon's Claude main thread.
`hooks/run.sh` gained an optional `PRETOOL_SCRIPT=<hook script>` environment
variable that replaces the `PreToolUse` logger; `hooks/deny-hook.sh` is such a
script. Schedules assume the scratch folder and hooks are already trusted (run
once with `Down`, `Enter` at the `Hooks need review` overlay first; swapping
the `PreToolUse` script re-raises it, and never paste prose while it is up).
The image schedule contains the scratch path, redacted to `<workspace>`.

| Schedule | Extra args / env | Produces |
|---|---|---|
| `s11-deny.json` | `PRETOOL_SCRIPT=.../hooks/deny-hook.sh` | `turn-with-deny`, `PreToolUse-denied` |
| `s12-image.json` | none (prompt is a bracketed paste of `@<png>`) | `turn-with-image`, `turn-with-image-mention-popup` |
| `s13-image-flag.json` | `"What colour is this image?" -i <png>` | `turn-with-image-flag` |
| `s14-loopback.json` | a listener from `python3 -m http.server <port> --bind 127.0.0.1` (its port is in the schedule) | `turn-with-loopback` |

