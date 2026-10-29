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

All were captured on macOS at 120×40 in a scratch git repo. Each folder's
README lists its files, the commands that produced them, what was redacted,
and a full Seams table with evidence. The table below is the short form;
follow the links for the specimen or the Bram code behind each cell.

Menus captured earlier live in [`../pty-menu-specimens/`](../pty-menu-specimens/README.md)
and are linked from the provider READMEs, not copied.

## Seams

| Seam | Claude Code | Codex | OpenCode | Cursor |
|---|---|---|---|---|
| Launch and resume by id | `--session-id <uuid>` reserves, `--resume <id>` ([resume](claude/resume-by-id.txt)) | `codex resume <id>`, or a picker ([resume](codex/resume-by-id.txt), [picker](codex/dialog-resume-picker.txt)) | `--session <id>` ([resume](opencode/resume-by-id.txt)) | `--resume <id>`, `agent ls` ([resume](cursor/resume-by-id.txt), [create-chat](cursor/create-chat.txt)) |
| When the session id exists | at launch ([SessionStart](claude/hooks/SessionStart.json)) | allocated before any message ([idle exit](codex/exit-ctrl-c.txt)); rollout and index after the first message | after the first message ([session.created](opencode/hooks/session.created.json)) | at launch, `hasConversation:false` until the first message ([sessionStart](cursor/hooks/sessionStart.json), [meta](cursor/session-meta-record-idle.json)) |
| Session store | JSONL per session ([record](claude/transcript-record.jsonl)) | JSONL rollouts + `session_index.jsonl` ([record](codex/transcript-record.jsonl)) | SQLite `opencode.db` ([record](opencode/transcript-record.json)) | `meta.json` + `store.db` per chat, JSONL transcript ([record](cursor/transcript-record.jsonl)) |
| Session metadata | `custom-title`, `agent-name`… rewritten on resume ([records](claude/session-meta-record.jsonl)) | `session_meta` ([record](codex/session-meta-record.jsonl)) | `session_v2` row, `idle_outcome` ([record](opencode/session-meta-record.json)) | `meta.json` ([record](cursor/session-meta-record.json)) |
| Subagent transcripts (a Transcript tab per subagent) | `<session-id>/subagents/agent-<id>.jsonl` + `.meta.json`; one Transcript chip each. `SubagentStop` also fires per completed turn with an empty `agent_type` ([payload](claude/hooks/SubagentStop.json)) | own rollout per subagent; `SubagentStart`/`SubagentStop` carry `agent_transcript_path` ([start](codex/hooks/SubagentStart.json), [meta](codex/subagent-session-meta-record.jsonl), [screen](codex/turn-with-subagent.txt)) | not captured | not checked |
| Naming | `--name` at launch ([composer](claude/idle-composer.txt)) | auto `thread_name` from a hidden title turn; Bram re-asserts its own ([index](codex/session-index-record.jsonl)) | auto title; Bram updates the DB ([sessions](opencode/dialog-session-list.txt)) | `/rename` writes `title` ([screen](cursor/rename.txt)) |
| Guard hook | `PreToolUse` in `.claude/settings.json` ([payload](claude/hooks/PreToolUse.json)) | `PreToolUse` in `~/.codex/config.toml`, each hook trusted ([payload](codex/hooks/PreToolUse.json), [trust](codex/hooks-trust.txt)) | plugin `tool.execute.before`, throw to deny ([payload](opencode/hooks/tool.execute.before.shell.json)) | `preToolUse` in `.cursor/hooks.json`, no trust prompt; deny shows the model `user_message` ([payload](cursor/hooks/preToolUse.json), [deny](cursor/hooks/preToolUse-denied-shell.json)) |
| Edit-tool payload | `Write` / `Edit` ([Write](claude/hooks/PreToolUse-Write.json), [Edit](claude/hooks/PreToolUse-Edit.json)) | `apply_patch`, patch text in `command` ([payload](codex/hooks/PreToolUse-apply_patch.json)) | `write` ([payload](opencode/hooks/tool.execute.before.write.json)) | `Write`, after a `Read` with the same `tool_use_id` ([payload](cursor/hooks/preToolUse-write.json)) |
| Approval prompt | numbered dialog, options vary by tool ([shell](claude/turn-with-shell-approval.txt), [write](claude/edit-write-approval.txt)) | numbered list, enter/esc ([shell](codex/turn-with-shell-approval.txt), [patch](codex/turn-with-apply-patch-approval.txt)) | `△ Permission required`, only with `permission: ask` ([screen](opencode/turn-with-shell-approval.txt), [asked](opencode/hooks/permission.asked.shell.json)) | `Run this command?`; Esc opens a feedback box ([screen](cursor/turn-with-shell-approval.txt)) |
| Declined or blocked tool | nothing fires after `PermissionRequest`; a failing command fires `PostToolUseFailure` ([declined](claude/turn-with-shell-declined.txt), [failure](claude/hooks/PostToolUseFailure.json)) | sandbox denial in `PostToolUse` ([payload](codex/hooks/PostToolUse-sandbox-denied.json), [declined](codex/turn-with-shell-declined.txt)) | `session.tool.failed` then `interrupted` ([declined](opencode/hooks/session.tool.failed.declined.json), [denied](opencode/hooks/session.tool.failed.denied.json)) | a declined shell still fires `afterShellExecution` with exit 0 ([payload](cursor/hooks/afterShellExecution-declined.json)) |
| Turn state | `UserPromptSubmit` … `Stop`; no `Stop` when declined or interrupted ([Stop](claude/hooks/Stop.json)) | `UserPromptSubmit` … `Stop`, no `Stop` on Esc ([Stop](codex/hooks/Stop.json)); `notify` program per turn ([payload](codex/hooks/notify-agent-turn-complete.json)) | `session.execution.*` events ([succeeded](opencode/hooks/session.execution.succeeded.json)) | `beforeSubmitPrompt` … `stop` with `completed`/`aborted`/`error`; an interrupt fires `aborted` then `error` ([completed](cursor/hooks/stop-completed.json), [aborted](cursor/hooks/stop-aborted.json)) |
| Agent → Bram channel | loopback HTTP | filesystem intent/result files | loopback HTTP (plugin POST) | loopback `curl` works, even under `--sandbox enabled` ([payload](cursor/hooks/afterShellExecution.json)) |
| Bram → agent delivery | clear, bracketed paste, then CR | bracketed paste, then CR | typed into the PTY | paste and Enter as separate writes |
| Pasted images | `[Image: source: …]` marker (not captured) | not captured | not captured | not captured |
| Menus | [`pty-menu-specimens`](../pty-menu-specimens/README.md) | [`/` popup](codex/dialog-slash-popup.txt), [model](codex/dialog-model-picker.txt), [agents](codex/agents-overview.txt) | [palette](opencode/dialog-command-palette.txt), [sessions](opencode/dialog-session-list.txt), [models](opencode/dialog-model-list.txt), [question form](opencode/form-question.txt) | [`/` popup](cursor/slash-popup.txt), [sessions](cursor/session-list.txt) |
| Working status | spinner line, changing verb ([turn](claude/turn-with-shell-working.txt)) | `• Working (Ns • esc to interrupt)` ([turn](codex/turn-with-shell-working.txt)) | spinner + `Build · <model> · Ns` ([turn](opencode/turn-with-shell-working.txt)) | `Running  N tokens` ([turn](cursor/turn-with-shell-working.txt)) |
| Interrupt | Esc, no `Stop` ([screen](claude/interrupt-esc.txt)) | Esc ([screen](codex/turn-with-shell-declined.txt)) | Esc twice within 5 s ([screen](opencode/interrupt-esc-twice.txt)) | Esc or Ctrl-C ([screen](cursor/interrupt-esc.txt)) |
| Exit and resume hint | Ctrl-C twice within ~1 s, prints `claude --resume <id>` ([screen](claude/exit-ctrl-c.txt)) | Ctrl-C once, prints `codex resume <id>` ([screen](codex/exit-ctrl-c.txt)) | Ctrl-C once, prints `opencode -s <id>` after a turn ([screen](opencode/exit-ctrl-c.txt)) | Ctrl-C twice, prints `agent --resume=<id>` if the chat has a conversation ([screen](cursor/exit-ctrl-c.txt)) |
| Logged-out screen | none: footer `Not logged in` ([screen](claude/logged-out.txt)) | full sign-in chooser ([screen](codex/logged-out.txt)) | none ([screen](opencode/no-config-exit.txt)) | splash, any key logs in ([screen](cursor/logged-out.txt)) |
| First-run screen | folder trust, cursor on "No, exit" ([screen](claude/first-run-trust.txt)) | folder trust ([screen](codex/first-run.txt)) | none ([screen](opencode/first-run.txt)) | workspace trust, Enter trusts ([screen](cursor/first-run.txt)) |
| Instructions file | `CLAUDE.md` | `AGENTS.md` | `AGENTS.md`, `opencode.json` | `AGENTS.md`, `.cursor/rules/*.mdc`, `CLAUDE.md` all honored |
| Model and version | header, `SessionStart.model` | footer, `session_meta.cli_version` | composer row, footer | footer, `cursor_version` in every hook |
| Reproducible capture | [`capture/`](claude/capture/README.md) | [`capture/`](codex/capture/README.md) | [`capture/`](opencode/capture/README.md) | [`capture/`](cursor/capture/README.md) |

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
- **Not captured anywhere yet:** pasted images; OpenCode subagents and
  Cursor subagents; a guard deny on Claude, Codex and OpenCode; a loopback
  `curl` from inside a Claude or OpenCode turn.

## Review revisions

Signed: Codex (main thread), 2026-10-28. Corrected the Codex ID-allocation
claim and the raw-stream provenance; recovered the Codex capture tools and
documented their reproduction limits. Original captures are Claude's.

Signed: OpenCode (main thread), 2026-10-28. Corrected the OpenCode README's
description of its `.raw` files as byte-exact: they are redacted streams with
the scratch path replaced by `<workspace>`, matching Codex's correction.
