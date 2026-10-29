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
| Launch and resume by id | `--session-id <uuid>` reserves, `--resume <id>` | `codex resume <id>` ([resume](codex/resume-by-id.txt)) | `--session <id>` ([resume](opencode/resume-by-id.txt)) | `create-chat`, `--resume <id>` ([create-chat](cursor/create-chat.txt)) |
| When the session id exists | at launch ([SessionStart](claude/hooks/SessionStart.json)) | allocated before any message ([idle exit](codex/exit-ctrl-c.txt)); rollout and index observed after the first message | after the first message ([session.created](opencode/hooks/session.created.json)) | at launch ([sessionStart](cursor/hooks/sessionStart.json)) |
| Session store | JSONL per session ([record](claude/transcript-record.jsonl)) | JSONL rollouts + `session_index.jsonl` ([record](codex/transcript-record.jsonl)) | SQLite `opencode.db` ([record](opencode/transcript-record.json)) | JSONL per chat ([record](cursor/transcript-record.jsonl)) |
| Subagent transcripts (a Transcript tab per subagent) | `<session-id>/subagents/agent-<id>.jsonl` + `.meta.json`; one Transcript chip each | none seen yet | none seen yet | not checked |
| Naming | `--name` at launch ([composer](claude/idle-composer.txt)) | auto `thread_name`; Bram re-asserts its own | auto title; Bram updates the DB | not captured |
| Guard hook | `PreToolUse` in `.claude/settings.json` ([payload](claude/hooks/PreToolUse.json)) | `PreToolUse` in `~/.codex/config.toml`, each hook trusted ([payload](codex/hooks/PreToolUse.json), [trust](codex/hooks-trust.txt)) | plugin `tool.execute.before`, throw to deny ([payload](opencode/hooks/tool.execute.before.shell.json)) | `preToolUse` in `.cursor/hooks.json`, fails open ([payload](cursor/hooks/preToolUse.json)) |
| Turn state | `UserPromptSubmit` … `Stop` ([Stop](claude/hooks/Stop.json)) | `UserPromptSubmit` … `Stop`, no `Stop` on Esc ([Stop](codex/hooks/Stop.json)) | `session.execution.*` events ([succeeded](opencode/hooks/session.execution.succeeded.json)) | `stop` (not captured) |
| Agent → Bram channel | loopback HTTP | filesystem intent/result files | loopback HTTP (plugin POST) | not tested (sandbox `--network` defaults off) |
| Bram → agent delivery | clear, bracketed paste, then CR | bracketed paste, then CR | typed into the PTY | paste and Enter as separate writes |
| Pasted images | `[Image: source: …]` marker (not captured) | not captured | not captured | `attachments` in `beforeSubmitPrompt` (untested) |
| Menus | [`pty-menu-specimens`](../pty-menu-specimens/README.md), [permission](claude/turn-with-shell.txt) | [`pty-menu-specimens`](../pty-menu-specimens/README.md), [approval](codex/turn-with-shell-approval.txt) | [palette](opencode/dialog-command-palette.txt), [sessions](opencode/dialog-session-list.txt), [models](opencode/dialog-model-list.txt) | [Run this command?](cursor/turn-with-shell.txt) |
| Working status | spinner line, changing verb ([turn](claude/turn-with-shell-readonly.txt)) | `• Working (Ns • esc to interrupt)` ([turn](codex/turn-with-shell-working.txt)) | spinner + `Build · <model> · Ns` ([turn](opencode/turn-with-shell.txt)) | `⠰⠰ Running  N tokens` ([turn](cursor/turn-with-shell.txt)) |
| Interrupt and exit | Esc; exit banner not captured | Esc; Ctrl-C exits, prints `codex resume <id>` ([exit](codex/exit-ctrl-c.txt)) | Esc twice within 5 s (not captured) | Ctrl-C (not captured) |
| Logged-out screen | none: footer `Not logged in` ([screen](claude/logged-out.txt)) | full sign-in chooser ([screen](codex/logged-out.txt)) | none ([screen](opencode/no-config-exit.txt)) | splash, any key logs in ([screen](cursor/logged-out.txt)) |
| First-run screen | folder trust, cursor on "No, exit" ([screen](claude/first-run-trust.txt)) | folder trust ([screen](codex/first-run.txt)) | none ([screen](opencode/first-run.txt)) | workspace trust, Enter trusts ([screen](cursor/first-run.txt)) |
| Instructions file | `CLAUDE.md` | `AGENTS.md` | `AGENTS.md`, `opencode.json` | not checked |
| Model and version | header, `SessionStart.model` | footer, `session_meta.cli_version` | composer row, footer | footer, `sessionStart.model` |

## Open threads from these captures

- **Claude's working status.** The check at `src-tauri/src/lib.rs:11955`
  (`pty_chunk_has_codex_working_status`) matches Codex's
  `Working (… esc to interrupt)` line. Whether Claude's spinner line is
  read elsewhere hasn't been checked against [this capture](claude/turn-with-shell-readonly.txt).
- **Codex hooks-trust drift.** On 0.165.0 the screen reads "Hooks need review
  / 6 hooks are new or changed" with "Trust all and continue"
  ([capture](codex/hooks-trust.txt)). Bram's marker is pinned to 0.148.0 and
  0.162.1 specimens.
- **OpenCode with no config.** `docs/developing-bram.md` says 2.1.4 prints a
  `cli.server-connection.resolve` stack trace and exits. With `--standalone`
  it drew a normal composer instead; the non-standalone path wasn't retried.
- **Codex text screens.** A plain ANSI strip runs a cursor-addressed Codex
  screen into one line, so `codex/*.txt` were rendered with a small VT
  emulator. The `.raw` files are redacted terminal streams, not untouched
  bytes. Path substitutions change widths and can affect cursor-addressed
  replay. The recovered [capture tools](codex/capture/README.md) document
  the renderer, retained schedules, and limits on reproducing these files.
- **Not captured anywhere yet:** pasted images, a guard deny (and whether
  the model sees the reason), the interrupt on Claude, OpenCode and Cursor,
  and a loopback `curl` from inside a turn.

## Review revisions

Signed: Codex (main thread), 2026-10-28. Corrected the Codex ID-allocation
claim and the raw-stream provenance; recovered the Codex capture tools and
documented their reproduction limits. Original captures are Claude's.

Signed: OpenCode (main thread), 2026-10-28. Corrected the OpenCode README's
description of its `.raw` files as byte-exact: they are redacted streams with
the scratch path replaced by `<workspace>`, matching Codex's correction.
