# Agent specimens

What each agent CLI actually draws and emits, captured at a known version so
the providers can be compared seam by seam. This is the basis for mapping a
new provider onto Bram's existing seams, and for refactoring those seams
([#462](https://github.com/judell/bram/issues/462)). Markers Bram anchors on
come from captures like these, never from guesses
(`docs/developing-bram.md` §Capturing terminal specimens).

| Provider | Folder | Version | Captured |
|---|---|---|---|
| Claude Code | [`claude/`](claude/README.md) | 2.1.312 | 2026-10-28 |
| Codex | [`codex/`](codex/README.md) | codex-cli 0.165.0 | 2026-10-28 |
| OpenCode | [`opencode/`](opencode/README.md) | 2.1.4 | 2026-10-28 |
| Cursor | [`cursor/`](cursor/README.md) | agent 2026.10.22-27dcf5f | 2026-10-28 |
| Antigravity CLI | [`antigravity/`](antigravity/README.md) | agy 1.4.12 | 2026-10-30 |

All were captured on macOS at 120×40 in a scratch git repo. Each folder's
README lists its files, the commands that produced them, what was redacted,
and a full Seams table with evidence. The table below is the short form;
follow the links for the specimen or the Bram code behind each cell.

Menus captured earlier live in [`../pty-menu-specimens/`](../pty-menu-specimens/README.md)
and are linked from the provider READMEs, not copied.

## Seams

| Seam | Claude Code | Codex | OpenCode | Cursor | Antigravity |
|---|---|---|---|---|---|
| Launch and resume by id | `--session-id <uuid>` reserves, `--resume <id>` ([resume](claude/resume-by-id.txt)) | `codex resume <id>`, or a picker ([resume](codex/resume-by-id.txt), [picker](codex/dialog-resume-picker.txt)) | `--session <id>` ([resume](opencode/resume-by-id.txt)) | `--resume <id>`, `agent ls` ([resume](cursor/resume-by-id.txt), [create-chat](cursor/create-chat.txt)) | `--conversation <id>` resumes; `/resume` lists conversations ([resume](antigravity/resume-by-id.txt), [list](antigravity/renamed-session-list.txt)) |
| When the session id exists | at launch ([SessionStart](claude/hooks/SessionStart.json)) | allocated before any message ([idle exit](codex/exit-ctrl-c.txt)); rollout and index after the first message | after the first message ([session.created](opencode/hooks/session.created.json)) | at launch, `hasConversation:false` until the first message ([sessionStart](cursor/hooks/sessionStart.json), [meta](cursor/session-meta-record-idle.json)) | `SessionStart` payload carried an id on the first prompted turn ([payload](antigravity/hooks/payloads/SessionStart.jsonl)); idle allocation was not probed before the first prompt |
| Session store | JSONL per session ([record](claude/transcript-record.jsonl)) | JSONL rollouts + `session_index.jsonl` ([record](codex/transcript-record.jsonl)) | SQLite `opencode.db` ([record](opencode/transcript-record.json)) | `meta.json` + `store.db` per chat, JSONL transcript ([record](cursor/transcript-record.jsonl)) | `brain/<id>/.system_generated/logs/transcript*.jsonl`; `SessionStart.workspacePaths` gives the workspace at launch ([record](antigravity/transcript-record.jsonl), [hook](antigravity/hooks/payloads/SessionStart.jsonl)); persisted project mapping unknown |
| Session metadata | `custom-title`, `agent-name`… rewritten on resume ([records](claude/session-meta-record.jsonl)) | `session_meta` ([record](codex/session-meta-record.jsonl)) | `session_v2` row, `idle_outcome` ([record](opencode/session-meta-record.json)) | `meta.json` ([record](cursor/session-meta-record.json)) | `/rename` title appears in `/resume`; durable store not identified ([screen](antigravity/renamed-session-list.txt)) |
| Subagent transcripts (a Transcript tab per subagent) | `<session-id>/subagents/agent-<id>.jsonl` + `.meta.json`; one Transcript chip each. `SubagentStop` also fires per completed turn with an empty `agent_type` ([payload](claude/hooks/SubagentStop.json)) | own rollout per subagent; `SubagentStart`/`SubagentStop` carry `agent_transcript_path` ([start](codex/hooks/SubagentStart.json), [meta](codex/subagent-session-meta-record.jsonl), [screen](codex/turn-with-subagent.txt)) | `subagent` tool spawns a child session with `parent_id`; its events pass through the plugin ([child](opencode/transcript-record-subagent-child.json), [order](opencode/subagent-event-order.txt)) | `Task` tool; child transcript at `<parent>/subagents/<child>.jsonl`, child hooks carry only the child's id; `subagentStart`/`subagentStop` never fired ([child](cursor/transcript-record-subagent-child.jsonl), [task](cursor/hooks/preToolUse-task.json)) | not captured; no subagent turn was run |
| Naming | `--name` at launch ([composer](claude/idle-composer.txt)) | auto `thread_name` from a hidden title turn; Bram re-asserts its own ([index](codex/session-index-record.jsonl)) | auto title; Bram updates the DB ([sessions](opencode/dialog-session-list.txt)) | `/rename` writes `title` ([screen](cursor/rename.txt)) | `/rename` changes the visible title ([result](antigravity/rename-result.txt), [list](antigravity/renamed-session-list.txt)) |
| Guard hook (and does the deny reason reach the model?) | `PreToolUse` in `.claude/settings.json`; deny reason reaches the model, worded "hook error" ([payload](claude/hooks/PreToolUse.json), [deny](claude/turn-with-deny.txt), [record](claude/transcript-record-deny.jsonl)) | `PreToolUse` in `~/.codex/config.toml`, each hook trusted; deny reason reaches the model as "Command blocked by PreToolUse hook" ([payload](codex/hooks/PreToolUse.json), [deny](codex/turn-with-deny.txt), [trust](codex/hooks-trust.txt)) | plugin `tool.execute.before` throws; the message reaches the model verbatim ([payload](opencode/hooks/tool.execute.before.shell.guard-denied.json), [deny](opencode/turn-with-shell-denied-by-guard.txt)) | `preToolUse` in `.cursor/hooks.json`, no trust prompt; the model sees `user_message`, not `agent_message` ([payload](cursor/hooks/preToolUse.json), [deny](cursor/hooks/preToolUse-denied-shell.json)) | `.agents/hooks.json`; `PreToolUse` observed a `write_file` call and a `deny` reason reached the model ([input](antigravity/hooks/payloads/PreToolUse-denied.jsonl), [screen](antigravity/hook-deny.txt), [transcript](antigravity/transcript-record-denied.jsonl)) |
| Guard failure posture (does the call run when the guard breaks?) | Fails open except deny JSON or exit 2: exit 1/3, bad JSON, timeout, empty, `{}` and older-guard shape all run the call, model sees nothing. No fail-closed option ([posture](claude/hooks/failure-posture/posture.md)) | Fails open except deny JSON or exit 2: exit 1/3, timeout, empty, `{}`, bad JSON run the call. Explicit `allow` JSON is rejected (`Failed`) yet runs. No fail-closed option ([posture](codex/hooks/failure-posture/posture.md)) | Plugin: any throw denies (non-Error loses text); `return` of anything, `{}` or a deny-shaped object allows; plugin load/setup failure silently drops the guard; no timeout, a hang blocks ([posture](opencode/hooks/failure-posture/posture.md)) | Fails open on crash, non-2 exit, timeout, empty output unless `failClosed: true`; invalid JSON and exit 2 block regardless; `{}` allows even under failClosed ([posture](cursor/hooks/failure-posture/posture.md)) | Fails closed except exit 0 with empty output: exit 1/2/3, invalid JSON and a hang past `timeout` block; empty output (the older-guard shape) runs, which Bram's hook wrapper turns into a deny. `{}` not tried ([posture](antigravity/hooks/failure-posture/posture.md)) |
| Edit-tool payload | `Write` / `Edit` ([Write](claude/hooks/PreToolUse-Write.json), [Edit](claude/hooks/PreToolUse-Edit.json)) | `apply_patch`, patch text in `command` ([payload](codex/hooks/PreToolUse-apply_patch.json)) | `write` ([payload](opencode/hooks/tool.execute.before.write.json)) | `Write`, after a `Read` with the same `tool_use_id` ([payload](cursor/hooks/preToolUse-write.json)) | `write_file` (`Content`, `Overwrite`, `Path`), `edit_file` (`Path`, `Replacement`, `Target`), and `run_command` (`CommandLine`, `WaitMsBeforeAsync`, `toolSummary`) ([payloads](antigravity/hooks/payloads/PreToolUse.jsonl), [results](antigravity/hooks/payloads/PostToolUse.jsonl)) |
| Approval prompt | numbered dialog, options vary by tool ([shell](claude/turn-with-shell-approval.txt), [write](claude/edit-write-approval.txt)) | numbered list, enter/esc ([shell](codex/turn-with-shell-approval.txt), [patch](codex/turn-with-apply-patch-approval.txt)) | `△ Permission required`, only with `permission: ask` ([screen](opencode/turn-with-shell-approval.txt), [asked](opencode/hooks/permission.asked.shell.json)) | `Run this command?`; Esc opens a feedback box ([screen](cursor/turn-with-shell-approval.txt)) | four numbered choices, including once, conversation allow, persistent allow, and cancel ([specimen](../pty-menu-specimens/2026-10-30-antigravity-command-approval.md), [screen](antigravity/permission-prompt.txt)) |
| Declined or blocked tool | nothing fires after `PermissionRequest`; a failing command fires `PostToolUseFailure` ([declined](claude/turn-with-shell-declined.txt), [failure](claude/hooks/PostToolUseFailure.json)) | sandbox denial in `PostToolUse` ([payload](codex/hooks/PostToolUse-sandbox-denied.json), [declined](codex/turn-with-shell-declined.txt)) | `session.tool.failed` then `interrupted` ([declined](opencode/hooks/session.tool.failed.declined.json), [denied](opencode/hooks/session.tool.failed.denied.json)) | a declined shell still fires `afterShellExecution` with exit 0 ([payload](cursor/hooks/afterShellExecution-declined.json)) | explicit cancel reports “User declined the tool call”; hook `deny` reports its reason to the model ([screen](antigravity/permission-declined.txt), [deny](antigravity/hook-deny.txt)) |
| Turn state | `UserPromptSubmit` … `Stop`; no `Stop` when declined or interrupted ([Stop](claude/hooks/Stop.json)) | `UserPromptSubmit` … `Stop`, no `Stop` on Esc ([Stop](codex/hooks/Stop.json)); `notify` program per turn ([payload](codex/hooks/notify-agent-turn-complete.json)) | `session.execution.*` events ([succeeded](opencode/hooks/session.execution.succeeded.json)) | `beforeSubmitPrompt` … `stop`; every interrupt fires an `aborted`+`error` pair within 13 ms, same `generation_id` ([aborted](cursor/hooks/stop-aborted-esc-tool.json), [error](cursor/hooks/stop-error-esc-tool.json)) | `SessionStart`, `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, and a normal `Stop` (`NO_TOOL_CALL`, `fullyIdle: true`) captured; interrupt/background state not captured because those runs were not made ([hooks](antigravity/hooks/payloads/SessionStart.jsonl), [PreToolUse](antigravity/hooks/payloads/PreToolUse.jsonl), [PostToolUse](antigravity/hooks/payloads/PostToolUse.jsonl), [PreInvocation](antigravity/hooks/payloads/PreInvocation.jsonl), [PostInvocation](antigravity/hooks/payloads/PostInvocation.jsonl), [Stop](antigravity/hooks/payloads/Stop.jsonl)) |
| Agent → Bram channel (loopback `curl` from a turn) | loopback HTTP; `curl` works after the Bash prompt ([screen](claude/turn-with-loopback.txt)) | filesystem intent/result files; loopback refused by the default sandbox, `Failed to connect` ([screen](codex/turn-with-loopback.txt)) | loopback HTTP (plugin POST); `curl` allowed with no prompt ([screen](opencode/turn-with-loopback-curl.txt)) | loopback `curl` works, even under `--sandbox enabled` ([payload](cursor/hooks/afterShellExecution.json)) | not captured; no probe was run |
| Bram → agent delivery | clear, bracketed paste, then CR | bracketed paste, then CR | typed into the PTY | paste and Enter as separate writes | simple prompt delivered by separate paste and CR; combined write not compared ([schedule](antigravity/capture/schedules/simple-turn.json)) |
| Pasted images (`@<png>` as Bram sends it) | attached: a base64 `attachment` record; but the `@` file popup swallows the first Enter ([screen](claude/turn-with-image.txt), [popup](claude/turn-with-image-enter-swallowed.txt), [record](claude/transcript-record-image.jsonl)) | **not attached**: literal text; the model chose to open it with `view_image`, which the guard matcher doesn't cover. `-i` does attach ([screen](codex/turn-with-image.txt), [`-i`](codex/turn-with-image-flag.txt)) | not attached: literal text; the model read the file with its `read` tool (this model has no vision) ([record](opencode/transcript-record-pasted-image.json)) | not attached: the model reads the file with `Read`; `attachments` stays empty ([screen](cursor/turn-with-image.txt)) | not captured; no probe was run |
| Menus | [`pty-menu-specimens`](../pty-menu-specimens/README.md) | [`/` popup](codex/dialog-slash-popup.txt), [model](codex/dialog-model-picker.txt), [agents](codex/agents-overview.txt) | [palette](opencode/dialog-command-palette.txt), [sessions](opencode/dialog-session-list.txt), [models](opencode/dialog-model-list.txt), [question form](opencode/form-question.txt) | [`/` popup](cursor/slash-popup.txt), [sessions](cursor/session-list.txt) | [`/hooks`](antigravity/hooks-view.txt), [command approval](../pty-menu-specimens/2026-10-30-antigravity-command-approval.md); other menus not captured because they were not probed |
| Working status | spinner line, changing verb ([turn](claude/turn-with-shell-working.txt)) | `• Working (Ns • esc to interrupt)` ([turn](codex/turn-with-shell-working.txt)) | spinner + `Build · <model> · Ns` ([turn](opencode/turn-with-shell-working.txt)) | `Running  N tokens` ([turn](cursor/turn-with-shell-working.txt)) | `Generating...` ([screen](antigravity/working.txt)) |
| Interrupt | Esc, no `Stop` ([screen](claude/interrupt-esc.txt)) | Esc ([screen](codex/turn-with-shell-declined.txt)) | Esc twice within 5 s ([screen](opencode/interrupt-esc-twice.txt)) | Esc or Ctrl-C ([screen](cursor/interrupt-esc.txt)) | not captured mid-turn; only idle Ctrl-C was tested |
| Exit and resume hint | Ctrl-C twice within ~1 s, prints `claude --resume <id>` ([screen](claude/exit-ctrl-c.txt)) | Ctrl-C once, prints `codex resume <id>` ([screen](codex/exit-ctrl-c.txt)) | Ctrl-C once, prints `opencode -s <id>` after a turn ([screen](opencode/exit-ctrl-c.txt)) | Ctrl-C twice, prints `agent --resume=<id>` if the chat has a conversation ([screen](cursor/exit-ctrl-c.txt)) | `--conversation <id>` resumed the captured conversation; first idle Ctrl-C showed the second-press hint; the second returned to the composer, and process exit was not captured ([resume](antigravity/resume-by-id.txt), [hint](antigravity/exit-first-ctrl-c.txt), [second press](antigravity/exit-ctrl-c.txt)) |
| Logged-out screen | none: footer `Not logged in` ([screen](claude/logged-out.txt)) | full sign-in chooser ([screen](codex/logged-out.txt)) | none ([screen](opencode/no-config-exit.txt)) | splash, any key logs in ([screen](cursor/logged-out.txt)) | Google OAuth / Cloud project chooser in a throwaway `HOME` ([screen](antigravity/logged-out.txt)) |
| First-run screen | folder trust, cursor on "No, exit" ([screen](claude/first-run-trust.txt)) | folder trust ([screen](codex/first-run.txt)) | none ([screen](opencode/first-run.txt)) | workspace trust, Enter trusts ([screen](cursor/first-run.txt)) | workspace trust prompt ([screen](antigravity/first-run-trust.txt)) |
| Instructions file | `CLAUDE.md` | `AGENTS.md` | `AGENTS.md`, `opencode.json` | `AGENTS.md`, `.cursor/rules/*.mdc`, `CLAUDE.md` all honored | not captured; no probe was run |
| Model and version | header, `SessionStart.model` | footer, `session_meta.cli_version` | composer row, footer | footer, `cursor_version` in every hook | `Gemini 3.8 Flash (High)` on screen, `modelName` in hook payload; `agy 1.4.12` ([version](antigravity/version.txt), [payload](antigravity/hooks/payloads/SessionStart.jsonl)) |
| Reproducible capture | [`capture/`](claude/capture/README.md) | [`capture/`](codex/capture/README.md) | [`capture/`](opencode/capture/README.md) | [`capture/`](cursor/capture/README.md) | [`capture/`](antigravity/capture/README.md) |

## Open threads from these captures

- **Turns that end without a turn-end event.** Claude fires no `Stop` when a
  turn is declined or interrupted, and Codex fires none on Esc. A tracker
  keyed only on those hooks never sees such turns end. Cursor's `stop` fires
  twice on an interrupt (`aborted`, then `error`), cause unknown.
- **Claude's `SubagentStop` on ordinary turns.** It fires at the end of every
  completed turn with an empty `agent_type`, apparently from the
  prompt-suggestion helper. Anything that reads it as "a subagent ran" sees
  phantom subagents.
- **Codex has subagents.** They have their own rollouts and hook events, so
  Bram's Transcript could give them tabs as it does for Claude's.
- **Cursor's declined shell looks like a success** in `afterShellExecution`
  and `postToolUse` (exit 0, empty output). Only the screen and the
  transcript show the rejection.
- **Claude's working status.** The check at `src-tauri/src/lib.rs:11955`
  (`pty_chunk_has_codex_working_status`) matches Codex's
  `Working (… esc to interrupt)` line. Whether Claude's spinner line is
  read elsewhere hasn't been checked against [this capture](claude/turn-with-shell-working.txt).
- **Codex hooks-trust drift.** On 0.165.0 the screen reads "Hooks need review
  / 6 hooks are new or changed" with "Trust all and continue"
  ([capture](codex/hooks-trust.txt)). Bram's marker is pinned to 0.148.0 and
  0.162.1 specimens. Typing into that menu toggles hooks, so pane sends must
  be held while it is up.
- **OpenCode with no config.** `docs/developing-bram.md` says 2.1.4 prints a
  `cli.server-connection.resolve` stack trace and exits. With `--standalone`
  it drew a normal composer instead; the non-standalone path wasn't retried.
- **`.raw` files are redacted streams**, not untouched bytes. Path
  substitutions change widths and can affect cursor-addressed replay. Each
  pack's `capture/` README documents its renderer, schedules and limits.
- **Pasted images reach two of four providers as pixels.** Only Claude
  attaches `@<png>` itself. Codex, OpenCode and Cursor see a path and attach
  nothing; the image arrives only if the model decides to open the file, and
  on Codex that tool (`view_image`) bypasses the guard's matcher. Claude's
  `@` popup also swallows an Enter, which Bram's frame survives only because
  text follows the path.
- **Background subagents keep the claim armed.** Bram's turn-end watcher
  re-arms the main claim from a subagent transcript's activity, so an item
  delegated to background subagents stays locked after the main turn ends
  (worklist item `background-subagent-rearms-claim`).
- **Not captured anywhere yet:** a real clipboard image paste (Claude's
  `[Image: …]` form); Claude's `PermissionDenied`; OpenCode with an
  image-capable model; why Cursor's `subagentStart`/`subagentStop` never fire.
- **Antigravity CLI 1.4.12:** the captured `PreToolUse` deny reason reaches the
  model, and `force_ask` renders four choices with separate persistent and
  conversation-only allow options. Its logged-out OAuth chooser, transcript
  layout, `/resume` list, `--conversation` resume, and file/edit/shell tool
  payloads are captured. Hook timeout/crash/invalid-JSON behavior,
  interrupt/background lifecycle, shell `PostToolUse`, subagents, loopback,
  image delivery, instructions files, and persistent title/project metadata remain
  open in [`antigravity/`](antigravity/README.md).

## Authorship and review revisions

Signed: Jon's Claude (Bram 0.8.0, main thread, Opus 5.5, macOS), 2026-10-28.
Captured the Cursor pack (60ebeb0) and wrote this index. The Claude Code,
Codex and OpenCode packs (9772971) and the alignment of all four to one set
(aa4f0bc) were captured by Claude subagents (Sonnet), one per provider, which
I briefed and checked. I rebuilt the Seams table from their packs, and
re-redacted screen captures that still held scratch-path fragments split by
terminal escapes.

Signed: Codex (main thread), 2026-10-28. Corrected the Codex ID-allocation
claim and the raw-stream provenance; recovered the Codex capture tools and
documented their reproduction limits. Original captures are Claude's.

Signed: OpenCode (main thread), 2026-10-28. Corrected the OpenCode README's
description of its `.raw` files as byte-exact: they are redacted streams with
the scratch path replaced by `<workspace>`, matching Codex's correction.
