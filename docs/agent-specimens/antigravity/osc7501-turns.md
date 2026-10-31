# How agy 1.4.12 ends a turn: hooks vs OSC 7501

Captured 2026-10-31 for `antigravity-turn-lifecycle`, on macOS, agy 1.4.12.

## Setup

- 120×40 PTY driven by `capture/drive.py` with `capture/schedules/turn-completed.json`, `turn-interrupt.json` and `turn-background.json`.
- A scratch git repo. Its `.agents/hooks.json` logged every `PreInvocation`, `PostInvocation`, `Stop`, `PreToolUse` and `PostToolUse` payload with a timestamp (the logger answered `PreToolUse` with `allow` and printed nothing for the others).
- `AGY_CLI_OSC7501=on` in agy's environment.
- `--settings '{"trustedWorkspaces":["<workspace>"],"permissions":{"allow":["command(sleep)"]}}'`, session-only, so the folder-trust dialog and the approval prompt for `sleep` stayed out of the way. No permission-bypass flag.

## Results

| Run | Hooks, in order (seconds from the turn's `PreInvocation`) | OSC 7501 states, in order |
|---|---|---|
| completed (`Reply with only TURN-OK-7731`) | `PreInvocation` 0.0, `PostInvocation` 1.8, `Stop` 1.9 (`terminationReason: NO_TOOL_CALL`, `fullyIdle: true`) | `blocked:kind=auth`, `idle`, `working`, `done`, `idle`, `idle` |
| Esc while `sleep 25` ran, then Esc while a 60-line reply streamed | `PreInvocation` 0.0, `PreToolUse` (`run_command`) 6.1, then nothing for that turn; `PreInvocation` 18.1 for the second, then nothing | `blocked:kind=auth`, `idle`, `working`, `idle`, `working`, `idle`, `idle` |
| `sleep 40` started in the background | `PreInvocation` 0.0, `PreToolUse` 5.2, `PostInvocation` 5.7, `PreInvocation` 5.7, `PostInvocation` 7.8, `Stop` 7.8 (`fullyIdle: false`); when `sleep` ended, `PostToolUse` 45.2, then a `PreInvocation` agy started on its own at 45.2, `PostInvocation` 47.6, `Stop` 47.6 (`fullyIdle: true`) | `blocked:kind=auth`, `idle`, `working`, `done`, `working`, `done`, `idle`, `idle` |

Every marker carries `app=antigravity`, for example `ESC ] 7501 ; state=working:app=antigravity BEL`. `done` adds `msg=` with the reply text in base64 (`VFVSTi1PSy03NzMx` is `TURN-OK-7731`). The launch marker is `state=blocked:kind=auth:app=antigravity:msg=U2lnbiBpbiByZXF1aXJlZA==` ("Sign in required").

The two `Stop` payloads of the background run, redacted: [`hooks/payloads/Stop-background.jsonl`](hooks/payloads/Stop-background.jsonl).

**Where the trailing `idle`s come from.** In the completed and background runs they came from each schedule's closing Ctrl-C, not from the turn. The snapshot taken 14 s after the completed reply (`done`) ends at `done`, and so does the background run's snapshot after `sleep` finished (`working`, `done`, `working`, `done`). A first reading of the table took them as turn ends. A demo-instance turn through Bram's own delivery (2026-10-31) confirmed it: `working`, then `done` with the reply in `msg=`, and no `idle` afterwards.

## What follows

- **An interrupted turn fires no turn-end hook:** no `Stop`, and no `PostInvocation`.
- **A completed turn ends at `done`, an interrupted one at `idle`.** `idle` also comes at launch and at exit. Bram ends a turn on either (`note_antigravity_state_markers` in `src-tauri/src/lib.rs`).
- **A background command's turn ends at `done` too.** When the command finishes, agy starts a new turn on its own (`working`, then `done`), so Bram's spinner goes off while the command runs and comes back for the follow-up.
- **agy only reports when asked.** Without `AGY_CLI_OSC7501=on`, agy sends the query `ESC ] 7501 ; ?` at launch and emits no states when the terminal doesn't answer, which is what every earlier capture in this pack shows. The variable is documented in agy's 1.4.0 release note (in the binary): "Set `AGY_CLI_OSC7501=on` for a terminal that supports it but doesn't answer the check." Bram sets it when it spawns its PTY.
- **Every launch reports `blocked:kind=auth` first,** then `idle` once signed in. It's not a blocked turn and not a login screen.
- **In a folder agy doesn't trust, launch reports `blocked:kind=permission`** with `msg=` "Trust this folder…" (seen in the demo instance, 2026-10-31). Bram logged `launch-confirmed … via=no-prompt` with the dialog up, so a send at that moment would be typed into it. That's for `antigravity-login-and-startup-choice`, and this marker is a way to detect the dialog without a screen specimen.
