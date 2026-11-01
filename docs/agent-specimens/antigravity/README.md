# Antigravity CLI specimens

Captured from `agy` 1.4.12 on macOS at 120×40 on 2026-10-30, using a
throwaway git workspace and the signed-in Antigravity account. The simple
turn used Gemini 3.8 Flash (High). The same version can update itself, so
these observations apply to 1.4.12 only.

The terminal driver and renderer are copied from the Codex-aligned Cursor
capture tools. The `.raw` files are redacted terminal streams, not byte-exact
originals; path and email replacements can shift cursor positions. The `.txt`
files are rendered from those redacted streams.

## Files

| File | Capture / contents |
|---|---|
| `version.txt`, `help.txt` | `agy --version` and `agy --help` (captured 2026-10-30) |
| `first-run-trust.raw`, `first-run-trust.txt` | First launch in a new scratch workspace; workspace trust prompt |
| `idle-composer.raw`, `idle-composer.txt` | Idle composer after accepting workspace trust |
| `working.raw`, `working.txt` | A short plain-text turn while generating |
| `completed.raw`, `completed.txt` | Hook experiment; first file tool followed by hook failure reported in the UI |
| `resume-by-id.raw`, `resume-by-id.txt` | Resumed the captured conversation with `--conversation <id>` |
| `exit-first-ctrl-c.raw`, `exit-first-ctrl-c.txt` | First idle Ctrl-C shows `press ctrl+c again to exit` |
| `exit-ctrl-c.raw`, `exit-ctrl-c.txt` | Second Ctrl-C did not exit in the captured attempt; screen returned to the composer |
| `logged-out.raw`, `logged-out.txt` | Login-method chooser from a throwaway `HOME` with no account state |
| `hooks-view.raw`, `hooks-view.txt` | `/hooks` lists five hook types and one handler for each |
| `startup.raw`, `startup.txt`, `first-tool.raw`, `first-tool.txt`, `second-tool.raw`, `second-tool.txt`, `approval.raw`, `approval.txt` | Intermediate snapshots from the hook experiment; the requested tool sequence did not complete |
| `permission-prompt.raw`, `permission-prompt.txt` | Snapshot of an ordinary working screen (`⢿ Working...`); the approval dialog had not appeared before capture ended |
| `approval.raw`, `approval.txt`, `tool-write-approval.raw`, `tool-write-approval.txt`, `tool-edit-approval.raw`, `tool-edit-approval.txt`, `tool-shell-approval.raw`, `tool-shell-approval.txt` | File creation/edit approval dialogs with numbered choices |
| `command-approval.raw`, `command-approval.txt` | `force_ask` shell-command approval with four numbered choices (captured 2026-10-31) |
| `capture/schedules/command-approval.json` | Schedule that requests a command and captures its forced approval dialog (2026-10-31) |
| `permission-declined.raw`, `permission-declined.txt` | Choosing option 4 reports `User declined the tool call` |
| `hook-deny.raw`, `hook-deny.txt` | `PreToolUse` deny reason displayed to the model |
| `hook-deny-result.raw`, `hook-deny-result.txt` | Replay of the denied-write response |
| `tool-write-approval.txt`, `tool-edit-approval.txt`, `tool-shell-approval.txt`, `tool-complete.txt`, `shell-command-result.txt` | File creation and edit approval prompts, then the harmless shell command |
| `transcript-record.jsonl`, `transcript-full-record.jsonl` | The plain-text prompt and response from the completed simple turn; ids are throwaway |
| `transcript-record-denied.jsonl` | Transcript record for the denied file write and the model's response |
| `transcript-record-lifecycle.jsonl` | Transcript record for the completed no-tool lifecycle turn |
| `transcript-record-tools.jsonl` | Transcript with file write, file edit, and shell tool calls |
| `rules-and-transport.md` | Which instruction files agy loads, the 24,000-byte rule truncation, the shared always-on budget and its largest-first demotion, `@` mentions in rules, the three convention questions, and loopback curl with and without a command grant (2026-10-31, `antigravity-signature-and-conventions`) |
| `capture/rule-probes.py` | Lays out the instruction-file probes of `rules-and-transport.md` in a scratch repo |
| `hooks/hooks.json` | Workspace logger configuration from the relative-path failure probe |
| `hooks/lifecycle-hooks.json` | Same event logger with absolute pack paths; replace `<pack>` before replay |
| `hooks/permission-hooks.json` | Hook configuration that forced the captured approval prompt |
| `hooks/deny-hooks.json`, `hooks/deny-hook.py` | Hook configuration and recorder that produced the captured deny payload |
| `hooks/log-hook.sh` | Logger used by the probe; it records input JSON and returns `allow` for `PreToolUse` |
| `hooks/payloads/SessionStart.jsonl` | Session id, workspace, transcript and model fields from the first prompted session |
| `hooks/payloads/PreToolUse.jsonl` | Observed `write_file` call and its `Content`, `Overwrite`, and `Path` arguments |
| `hooks/payloads/PreToolUse-denied.jsonl` | Denied `write_file` input, including the same argument fields |
| `hooks/payloads/PreInvocation.jsonl` | Invocation number and trajectory step count |
| `hooks/payloads/PostInvocation.jsonl` | Completed model invocation |
| `hooks/payloads/Stop.jsonl` | `NO_TOOL_CALL`, `fullyIdle: true` on a completed no-tool turn |
| `hooks/payloads/PostToolUse.jsonl` | Successful and failed file tool results |
| `osc7501-turns.md` | How a turn ends: hook sequences vs OSC 7501 states for a completed turn, two interrupts and a background command, with `AGY_CLI_OSC7501=on` (2026-10-31, `antigravity-turn-lifecycle`) |
| `hooks/payloads/Stop-background.jsonl` | The two `Stop` payloads of a background command: `fullyIdle: false` while it ran, then `true` after agy's own follow-up |
| `capture/drive.py`, `capture/render.py` | PTY driver and minimal terminal renderer, copied from the Cursor pack |
| `capture/redact.py` | Redacts the home directory, workspace, and email, renders `.txt`, and removes `_all.raw` |
| `capture/schedules/idle-composer.json` | Idle screen schedule |
| `capture/schedules/trust-idle-exit.json` | Trust prompt, idle and first Ctrl-C schedule |
| `capture/schedules/simple-turn.json` | Prompt, generation, completion and first Ctrl-C schedule |
| `capture/schedules/hooks-tools.json`, `capture/schedules/hooks-tools-fresh.json` | Attempted file creation, edit and shell-tool probe |
| `capture/schedules/hooks-view.json` | `/hooks` screen schedule |
| `capture/schedules/resume-then-exit.json`, `capture/schedules/exit-idle.json`, `capture/schedules/exit-idle-settled.json`, `capture/schedules/exit-double-ctrl-c.json` | Resume and Ctrl-C timing attempts |
| `capture/schedules/logged-out.json` | Login screen in the throwaway `HOME` |
| `capture/schedules/permission-prompt.json`, `capture/schedules/permission-decline.json`, `capture/schedules/permission-decline-number.json` | Approval, Escape interruption, and explicit cancel option |
| `capture/schedules/hook-deny.json` | Denied file-write request |
| `capture/schedules/lifecycle-turn.json` | No-tool turn with lifecycle hooks |
| `capture/schedules/tool-payloads.json`, `capture/schedules/tool-payloads-approved.json`, `capture/schedules/tool-payloads-full.json` | File write/edit and shell tool payload probe at different approval timings |
| `capture/schedules/resume-shell-permission.json` | Resume the pending harmless shell approval and run it |
| `capture/schedules/permission-prompt.json` | Force a shell approval prompt; Escape interrupts the turn |
| `capture/schedules/command-approval.json` | Force a shell-command approval prompt with a `PreToolUse` `force_ask` hook (2026-10-31) |
| `capture/schedules/permission-decline-number.json` | Select option 4 to explicitly decline |
| `capture/schedules/exit-double-ctrl-c.json`, `capture/schedules/exit-idle-settled.json` | Attempts to exit with a second Ctrl-C |
| `capture/schedules/resume-then-exit.json` | Resume by conversation id, then interrupt the replayed turn |
| `capture/schedules/logged-out.json` | Start under an isolated, empty `HOME` |
| `capture/schedules/turn-completed.json`, `capture/schedules/turn-interrupt.json`, `capture/schedules/turn-background.json` | The three turn-ending runs in `osc7501-turns.md`; run with `AGY_CLI_OSC7501=on` and a session-only `command(sleep)` allow rule |

## How to replay

From this folder, create an empty scratch git repo and run:

```sh
mkdir -p scratch
git -C scratch init
cd scratch
python3 -I ../capture/drive.py .. ../capture/schedules/simple-turn.json -- agy
```

The schedule pastes the prompt and sends Return in separate writes. After
capturing, run `capture/redact.py` from the pack directory. It reads the
current home directory and accepts `ANTIGRAVITY_SPECIMEN_WORKSPACE` for the
scratch workspace to redact. The email is replaced by a general email-address
pattern. The driver itself does not redact and writes `_all.raw`; do not keep
that file.

For hooks, copy the appropriate `hooks/*.json` to the scratch repo's
`.agents/hooks.json`. The simple logger config uses a relative path to document
the observed working-directory failure. For lifecycle and tool payload
captures, the command path was absolute; replace `<pack>` in
`hooks/lifecycle-hooks.json` with the pack's absolute path before replaying.

Replay check: `capture/schedules/logged-out.json` was rerun with an empty,
throwaway `HOME` on 2026-10-30. Rendering its terminal stream reproduced
`logged-out.txt` exactly.

## Findings and limits

- `agy --help` lists `--conversation`, `--project`, `--new-project`, `--settings`,
  `--mode`, `--sandbox` and one-shot print/stream-json flags. It lists no ACP
  subcommand. The only structured mode advertised is print mode; ACP was not
  independently probed.
- A successful first prompted turn produced a `SessionStart` payload with the
  conversation id, workspace path, transcript path and `modelName`. Under
  `~/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/` the observed
  transcript files were `transcript.jsonl` and `transcript_full.jsonl`. The
  transcript record is a `USER_INPUT` and a `PLANNER_RESPONSE`.
- The idle screen shows the account, version, model, workspace and `>` prompt.
  The `/hooks` view showed `PreToolUse`, `PostToolUse`, `PreInvocation`,
  `PostInvocation` and `Stop`, with one handler each.
- The hook probe observed `write_file` with arguments `Content`, `Overwrite`
  and `Path`. A relative logger command failed from the hook runner's working
  directory and blocked later calls; the model saw that failure. A separate
  deterministic `deny` hook showed its `reason` to the model and recorded the
  incoming tool call. Hook crash, timeout, invalid JSON, and shell-command
  `PostToolUse` remain uncaptured.
- A `PreToolUse` hook returning `force_ask` produced a four-choice command
  approval menu. Pressing Escape interrupted the turn; selecting option 4
  produced `User declined the tool call`. These are separate captures.
- `--conversation <id>` resumed the recorded conversation. `/resume` listed
  conversations and the renamed title appeared in that list. `/rename`
  reported success; the durable title store was not identified. The
  `SessionStart.workspacePaths` field links a conversation to its workspace
  at launch, but a persisted project mapping was not found under `brain/<id>`.
  The first idle
  Ctrl-C displayed “press ctrl+c again to exit”; a second press did not exit
  in these captures. Mid-turn interrupt behavior remains uncaptured.
- A completed no-tool turn emitted `PreInvocation`, `PostInvocation` and
  `Stop` (`terminationReason: NO_TOOL_CALL`, `fullyIdle: true`). Interrupt and
  background-task lifecycle behavior remain unknown.
- The logged-out profile showed Google OAuth and Google Cloud project choices.
  Image delivery, subagents and background task state remain uncaptured.
  Instructions-file loading and loopback were captured on 2026-10-31; see
  `rules-and-transport.md`. `agy --help` lists no ACP subcommand, but ACP
  behavior was not independently probed.

The official [Antigravity hooks reference](https://antigravity.google/docs/hooks)
describes workspace `.agents/hooks.json`, hook event schemas and the
`PreToolUse` decisions. Those docs informed the probe configuration; the
findings above are limited to the artifacts actually captured.

## Seams

| Seam | Antigravity CLI | Evidence |
|---|---|---|
| Launch and resume | `agy --conversation <id>` resumed the recorded session; `/resume` lists conversations | `help.txt`, `resume-by-id.txt`, `renamed-session-list.txt` |
| Session id | Present in `SessionStart` on the first prompted turn | `hooks/payloads/SessionStart.jsonl` |
| Session store | Transcript JSONL under `brain/<id>/.system_generated/logs/`; `SessionStart` carries `workspacePaths`, durable project mapping not identified | `transcript-record.jsonl`, `hooks/payloads/SessionStart.jsonl` |
| Session metadata and naming | `/rename` changes the title shown by `/resume`; durable title store not identified | `rename-result.txt`, `renamed-session-list.txt` |
| Guard hook | Workspace `.agents/hooks.json`; failed command stopped later calls, and `deny.reason` reached the model | `hooks/hooks.json`, `hooks/deny-hooks.json`, `hook-deny.txt` |
| Tool payload | `write_file` (`Content`, `Overwrite`, `Path`), `edit_file` (`Path`, `Replacement`, `Target`), and `run_command` (`CommandLine`, `WaitMsBeforeAsync`, `toolSummary`) | `hooks/payloads/PreToolUse.jsonl`, `hooks/payloads/PreToolUse-denied.jsonl` |
| Turn hooks | `PreInvocation`, `PostInvocation` and `Stop` on a completed turn. An interrupted turn fires **no** `Stop` or `PostInvocation`. A background command gets `Stop` with `fullyIdle: false`, then `true` after agy's own follow-up | `hooks/payloads/Stop.jsonl`, `hooks/payloads/Stop-background.jsonl`, `osc7501-turns.md` |
| Turn state (OSC 7501) | With `AGY_CLI_OSC7501=on`: `state=working\|done\|idle:app=antigravity`. A completed turn ends at `done`, an interrupted one at `idle`. Launch reports `blocked:kind=auth` first, and the folder-trust dialog `blocked:kind=permission`. Without the variable, only the `7501;?` query | `osc7501-turns.md` |
| Menus | `/hooks`, `/resume` conversation list, file approvals, four-choice command approval | `hooks-view.txt`, `renamed-session-list.txt`, `approval.txt`, `tool-write-approval.txt`, `tool-edit-approval.txt`, `tool-shell-approval.txt`, `command-approval.txt` (`permission-prompt.txt` is a working screen) |
| Screen states | Trust prompt, idle composer, generating, first Ctrl-C hint | corresponding `.txt` screens |
| Model and version | Gemini 3.8 Flash (High), agy 1.4.12 | `idle-composer.txt`, `version.txt`, `hooks/payloads/SessionStart.jsonl` |
| ACP | No ACP subcommand in help; not independently probed | `help.txt` |
| Instructions | `AGENTS.md`, `GEMINI.md`, and `.agents/rules/*.md` with `trigger: always_on` frontmatter; a rule without it is rejected. Each rule is cut silently past 24,000 bytes after its frontmatter. Always-on text shares about 60,000 bytes, and past it the largest rule goes first. No `@` mention in a rule was expanded | `rules-and-transport.md` |
| Agent → Bram channel | Loopback curl works (no network sandbox as Bram launches agy), behind agy's own "command" permission; a prefix grant doesn't cover a chained command | `rules-and-transport.md` |

Signed: Codex, 2026-10-30. The captures were made in a throwaway workspace;
the scratch tree was removed after the specimens and transcripts were
redacted.

## Complete inventory

All terminal screens, transcripts, hook payloads, and schedules below are from `agy` 1.4.12 (2026-10-30). The copied PTY driver and renderer are from the Cursor pack; `redact.py` is this pack’s redaction utility.

| File | Version / role |
|---|---|
| `approval.raw` | agy 1.4.12 |
| `approval.txt` | agy 1.4.12 |
| `command-approval.raw` | agy 1.4.12 (2026-10-31) |
| `command-approval.txt` | agy 1.4.12 (2026-10-31) |
| `capture/README.md` | capture instructions and schedules |
| `capture/drive.py` | copied capture utility |
| `capture/redact.py` | pack redaction utility |
| `capture/render.py` | copied capture utility |
| `capture/rule-probes.py` | probe layout utility (2026-10-31) |
| `capture/schedules/command-approval.json` | agy 1.4.12 (2026-10-31) |
| `capture/schedules/exit-double-ctrl-c.json` | agy 1.4.12 |
| `capture/schedules/exit-idle-settled.json` | agy 1.4.12 |
| `capture/schedules/exit-idle.json` | agy 1.4.12 |
| `capture/schedules/hook-deny.json` | agy 1.4.12 |
| `capture/schedules/hooks-tools-fresh.json` | agy 1.4.12 |
| `capture/schedules/hooks-tools.json` | agy 1.4.12 |
| `capture/schedules/hooks-view.json` | agy 1.4.12 |
| `capture/schedules/idle-composer.json` | agy 1.4.12 |
| `capture/schedules/lifecycle-turn.json` | agy 1.4.12 |
| `capture/schedules/logged-out.json` | agy 1.4.12 |
| `capture/schedules/permission-decline-number.json` | agy 1.4.12 |
| `capture/schedules/permission-decline.json` | agy 1.4.12 |
| `capture/schedules/permission-prompt.json` | agy 1.4.12 |
| `capture/schedules/rename-and-resume.json` | agy 1.4.12 |
| `capture/schedules/rename-by-id.json` | agy 1.4.12 |
| `capture/schedules/rename-then-resume.json` | agy 1.4.12 |
| `capture/schedules/resume-shell-permission.json` | agy 1.4.12 |
| `capture/schedules/resume-then-exit.json` | agy 1.4.12 |
| `capture/schedules/simple-turn.json` | agy 1.4.12 |
| `capture/schedules/tool-payloads-approved.json` | agy 1.4.12 |
| `capture/schedules/tool-payloads-full.json` | agy 1.4.12 |
| `capture/schedules/tool-payloads.json` | agy 1.4.12 |
| `capture/schedules/trust-idle-exit.json` | agy 1.4.12 |
| `capture/schedules/turn-background.json` | agy 1.4.12 (2026-10-31) |
| `capture/schedules/turn-completed.json` | agy 1.4.12 (2026-10-31) |
| `capture/schedules/turn-interrupt.json` | agy 1.4.12 (2026-10-31) |
| `completed.raw` | agy 1.4.12 |
| `completed.txt` | agy 1.4.12 |
| `exit-ctrl-c.raw` | agy 1.4.12 |
| `exit-ctrl-c.txt` | agy 1.4.12 |
| `exit-first-ctrl-c.raw` | agy 1.4.12 |
| `exit-first-ctrl-c.txt` | agy 1.4.12 |
| `first-run-trust.raw` | agy 1.4.12 |
| `first-run-trust.txt` | agy 1.4.12 |
| `first-tool.raw` | agy 1.4.12 |
| `first-tool.txt` | agy 1.4.12 |
| `help.txt` | agy 1.4.12 |
| `hook-deny-result.raw` | agy 1.4.12 |
| `hook-deny-result.txt` | agy 1.4.12 |
| `hook-deny.raw` | agy 1.4.12 |
| `hook-deny.txt` | agy 1.4.12 |
| `hooks-view.raw` | agy 1.4.12 |
| `hooks-view.txt` | agy 1.4.12 |
| `hooks/deny-hook.py` | agy 1.4.12 |
| `hooks/deny-hooks.json` | agy 1.4.12 |
| `hooks/hooks.json` | agy 1.4.12 |
| `hooks/lifecycle-hooks.json` | agy 1.4.12 |
| `hooks/log-hook.sh` | agy 1.4.12 |
| `hooks/payloads/PostInvocation.jsonl` | agy 1.4.12 |
| `hooks/payloads/PostToolUse.jsonl` | agy 1.4.12 |
| `hooks/payloads/PreInvocation.jsonl` | agy 1.4.12 |
| `hooks/payloads/PreToolUse-denied.jsonl` | agy 1.4.12 |
| `hooks/payloads/PreToolUse.jsonl` | agy 1.4.12 |
| `hooks/payloads/SessionStart.jsonl` | agy 1.4.12 |
| `hooks/payloads/Stop-background.jsonl` | agy 1.4.12 (2026-10-31) |
| `hooks/payloads/Stop.jsonl` | agy 1.4.12 |
| `hooks/permission-hooks.json` | agy 1.4.12 |
| `idle-composer.raw` | agy 1.4.12 |
| `idle-composer.txt` | agy 1.4.12 |
| `lifecycle-complete.raw` | agy 1.4.12 |
| `lifecycle-complete.txt` | agy 1.4.12 |
| `lifecycle-idle-exit.raw` | agy 1.4.12 |
| `lifecycle-idle-exit.txt` | agy 1.4.12 |
| `lifecycle-idle.raw` | agy 1.4.12 |
| `lifecycle-idle.txt` | agy 1.4.12 |
| `lifecycle-working.raw` | agy 1.4.12 |
| `lifecycle-working.txt` | agy 1.4.12 |
| `logged-out.raw` | agy 1.4.12 |
| `logged-out.txt` | agy 1.4.12 |
| `osc7501-turns.md` | agy 1.4.12 (2026-10-31) |
| `permission-declined.raw` | agy 1.4.12 |
| `permission-declined.txt` | agy 1.4.12 |
| `permission-prompt.raw` | agy 1.4.12 (working-screen snapshot; approval dialog not captured) |
| `permission-prompt.txt` | agy 1.4.12 (working-screen snapshot; approval dialog not captured) |
| `rename-idle.raw` | agy 1.4.12 |
| `rename-idle.txt` | agy 1.4.12 |
| `rename-result.raw` | agy 1.4.12 |
| `rename-result.txt` | agy 1.4.12 |
| `renamed-session-list.raw` | agy 1.4.12 |
| `renamed-session-list.txt` | agy 1.4.12 |
| `resume-by-id.raw` | agy 1.4.12 |
| `resume-by-id.txt` | agy 1.4.12 |
| `resume-picker.raw` | agy 1.4.12 |
| `resume-picker.txt` | agy 1.4.12 |
| `rules-and-transport.md` | agy 1.4.12 (2026-10-31) |
| `second-tool.raw` | agy 1.4.12 |
| `second-tool.txt` | agy 1.4.12 |
| `shell-approval-resume.raw` | agy 1.4.12 |
| `shell-approval-resume.txt` | agy 1.4.12 |
| `shell-command-result.raw` | agy 1.4.12 |
| `shell-command-result.txt` | agy 1.4.12 |
| `startup.raw` | agy 1.4.12 |
| `startup.txt` | agy 1.4.12 |
| `tool-approval.raw` | agy 1.4.12 |
| `tool-approval.txt` | agy 1.4.12 |
| `tool-complete.raw` | agy 1.4.12 |
| `tool-complete.txt` | agy 1.4.12 |
| `tool-edit-approval.raw` | agy 1.4.12 |
| `tool-edit-approval.txt` | agy 1.4.12 |
| `tool-edit-result.raw` | agy 1.4.12 |
| `tool-edit-result.txt` | agy 1.4.12 |
| `tool-first-result.raw` | agy 1.4.12 |
| `tool-first-result.txt` | agy 1.4.12 |
| `tool-idle.raw` | agy 1.4.12 |
| `tool-idle.txt` | agy 1.4.12 |
| `tool-shell-approval.raw` | agy 1.4.12 |
| `tool-shell-approval.txt` | agy 1.4.12 |
| `tool-write-approval.raw` | agy 1.4.12 |
| `tool-write-approval.txt` | agy 1.4.12 |
| `transcript-full-record.jsonl` | agy 1.4.12 |
| `transcript-record-denied.jsonl` | agy 1.4.12 |
| `transcript-record-lifecycle.jsonl` | agy 1.4.12 |
| `transcript-record-tools.jsonl` | agy 1.4.12 |
| `transcript-record.jsonl` | agy 1.4.12 |
| `version.txt` | agy 1.4.12 |
| `working.raw` | agy 1.4.12 |
| `working.txt` | agy 1.4.12 |
