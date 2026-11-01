# Antigravity capture tools

The 120×40 PTY driver and minimal VT renderer are copied from the Cursor
specimen pack. `drive.py OUTDIR SCHEDULE.json -- agy [args...]` writes each
named snapshot as a cumulative `.raw` stream and `_all.raw`. Run the parent
pack's `redact.py` after capture; it removes `_all.raw`, redacts the current
home, email addresses and `ANTIGRAVITY_SPECIMEN_WORKSPACE`, and renders the
remaining streams to `.txt`.

These schedules were run with `agy` 1.4.12 on macOS at 120×40. Prompts and
Return are separate writes. Hook schedules use `.agents/hooks.json` in a
throwaway workspace; see [`../README.md`](../README.md) for the logger
configurations and path-resolution finding.

| Schedule | Capture |
|---|---|
| `schedules/idle-composer.json` | Idle composer |
| `schedules/trust-idle-exit.json` | First-run trust, idle and Ctrl-C |
| `schedules/simple-turn.json` | Plain response and working screen |
| `schedules/lifecycle-turn.json` | `SessionStart`, `PreInvocation`, `PostInvocation`, `Stop` |
| `schedules/logged-out.json` | Login chooser under an empty `HOME` |
| `schedules/hooks-view.json` | `/hooks` view |
| `schedules/hooks-tools.json`, `schedules/hooks-tools-fresh.json` | Initial file/write/edit/shell hook attempts |
| `schedules/tool-payloads.json`, `schedules/tool-payloads-approved.json`, `schedules/tool-payloads-full.json` | File creation/edit and shell tool payloads and prompts |
| `schedules/resume-shell-permission.json` | Resume and approve the harmless shell command |
| `schedules/hook-deny.json` | Denied file creation and model-visible reason |
| `schedules/permission-prompt.json` | Approval prompt and Escape interruption |
| `schedules/permission-decline.json`, `schedules/permission-decline-number.json` | Escape interruption versus explicit option 4 decline |
| `schedules/resume-then-exit.json`, `schedules/exit-idle.json`, `schedules/exit-idle-settled.json`, `schedules/exit-double-ctrl-c.json` | Resume and exit-key timing attempts |
| `schedules/rename-and-resume.json`, `schedules/rename-by-id.json`, `schedules/rename-then-resume.json` | Rename result and conversation list |
| `schedules/permission-full-screen-44x67.json` | Two sequential command approvals after filling a 44×67 screen (`ANTIGRAVITY_CAPTURE_COLS=44 ANTIGRAVITY_CAPTURE_ROWS=67 AGY_CLI_OSC7501=on`); `"esc to cancel"` bottom row omitted on both dialogs |
| `schedules/permission-short-46x26.json` | Wrapped `curl` command approval on a 46×26 screen (`ANTIGRAVITY_CAPTURE_COLS=46 ANTIGRAVITY_CAPTURE_ROWS=26 AGY_CLI_OSC7501=on`); `"esc to cancel"` omitted |
| `schedules/permission-scrolled-title-46x12.json` | Same `curl` command approval on a 46×12 screen (`ANTIGRAVITY_CAPTURE_COLS=46 ANTIGRAVITY_CAPTURE_ROWS=12 AGY_CLI_OSC7501=on`); dialog title and option 1 clipped off top |
| `permission-wrapped-options-44x67.raw` (from `scripts/capture-terminal-specimen.py --cols 44 --rows 67`, `AGY_CLI_OSC7501=on`, not a schedule) | Approval of `gh api … \| jq -s add \| tail -5`: a wrapped option label makes agy draw backspaces before option 3 |
| `schedules/permission-escape.json` | Esc at a `Run this command?` dialog with `AGY_CLI_OSC7501=on` (`permission-escape.{raw,txt}`); `blocked:kind=permission` goes straight to `idle` |

`logged-out.json` was replayed after capture. Rendering that stream produced
the committed `../logged-out.txt` exactly.
