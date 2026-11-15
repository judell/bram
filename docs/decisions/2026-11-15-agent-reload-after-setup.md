# After Setup, the pane tells each agent's user what they need to do, in Bram's words

Status: decided 2026-11-15 · revisit when an agent CLI changes how it reloads, or when Antigravity's behavior is confirmed live

**Question:** After a manual Setup, what should the "Setup complete" banner say? It used to say one thing for every agent: "Restart the agent in the terminal … to pick up the new coordination files", justified by "hooks are captured at agent-session startup."

**Decision:** The banner's sentence (`window.__bramSetupCompleteMessage(provider, result)` in `app/__shell/helpers.js`) depends on the active agent and on what Setup changed, read from the `/__enhance/run` response (`wrote`). It is worded as Bram actions, since the user lives in the pane. "Restart Bram" is the way to reload instructions, because a relaunch restarts or resumes the agent, and a restarted agent re-reads its instructions.

- **Claude, instructions changed** (`CLAUDE.md`, `.claude/bram-conventions.md`, `.claude/bram-reference/**`): "Restart Bram to load the updated instructions."
- **Claude, otherwise:** "Claude picks it up as it runs." Hooks and skills reload live.
- **Codex, instructions changed** (`AGENTS.md`): "Restart Bram to load the updated instructions." When `~/.codex/config.toml` also changed, add as fact: "Bram's hooks changed: open the terminal and use `/hooks` to trust them." When hooks didn't change, the message says nothing about hooks.
- **Codex, hooks only:** "Setup complete. Bram's hooks changed: open the terminal and use `/hooks` to trust them."
- **OpenCode:** "OpenCode picks it up as it runs."
- **Codex, nothing in `wrote`:** "Restart Bram to load the updated instructions." Setup's `developer_instructions` write isn't listed, so an empty list can still hide a change, and restarting is the safe ask.
- **Antigravity, rules changed** (`AGENTS.md`, `GEMINI.md`, `.agents/**`): "Restart Bram to load the updated rules." Otherwise: "Setup complete."
- **Unknown provider or no response:** "Setup complete. Restart Bram to load the updated files." No message sends the user to the terminal except the Codex `/hooks` trust step, which only the CLI can do.

**Evidence:** researched 2026-11-15 against the CLI versions shown.

| Agent | Hook / plugin entries | Instructions | Source |
|---|---|---|---|
| Claude Code 2.1.32x | Automatic (settings file watcher) | Read only at session start; `/compact` re-reads the root `CLAUDE.md` | code.claude.com/docs/en/settings.md "When edits take effect"; hooks.md; memory.md; skills.md |
| Codex 0.167.0 | New or changed hooks don't run until trusted in `/hooks` | Read at thread start; `/new` or resuming the thread re-reads `AGENTS.md` and config | `core/src/agents_md_manager.rs:87-135`, `core/src/session/session.rs:1523-1552`, `tui/src/app/resume_config.rs:44`, learn.chatgpt.com/docs/hooks |
| OpenCode 2.2.6 | Plugin reloaded automatically (~100 ms debounce) | Watched; a running session gets a diff notice on its next step | `packages/core/src/config.ts:376-420`, `config/plugin/instruction.ts:59-223`, `plugin/supervisor.ts:205-224` |
| Antigravity `agy` 1.5.7 | n/a | Unconfirmed: the docs conflict ("on every turn" vs "on startup") | antigravity.google/docs/rules.md, …/cli/best-practices |

The guards are `bram-guard` (`~/.bram/bram-guard`, a symlink to the build), so their behavior never needs an agent restart.

What `wrote` can tell the pane (`run_enhance`, `src-tauri/src/lib.rs`): it lists absolute paths of files Setup changed. Templates and marker-block merges list a path only when the bytes changed. Two exceptions: the Claude settings files (`.claude/settings.local.json` and the hook settings file) are always listed, so they can't signal a change; and the `developer_instructions` write in `~/.codex/config.toml` is not reported at all. `codexHookNeedsTrust` is always true, so it can't signal either; the pane keys Codex hooks on `config.toml` appearing in `wrote`.

Relaunch behavior (what "Restart Bram" does for the agent): under the default startup policy (`agentRecent`), Bram launches the configured agent's resume form: `claude --continue`, `codex resume --last`, `opencode --continue`, `agy --continue`. `lastActive` resumes the last-active provider's session by id; `newSession` starts fresh; `none` launches nothing, so the user starts the agent themselves. The first launch in an unmanaged repo starts fresh. Every launch starts a new agent process, which re-reads instructions.

**Alternatives:**

- One generic "restart the agent" message (the old text): wrong for Claude about hooks (picked up live), needlessly broad for Codex, and wrong for OpenCode (nothing to do).
- Terminal-command wording ("run `/new`", "exit and restart `claude`", "restart `agy`"): a Bram user rarely looks at the terminal, and "Restart Bram" does the same job. The one exception is Codex's `/hooks` trust, which has no pane equivalent.

**Revisit when:** a new CLI version changes its reload behavior (especially Claude's settings watcher, Codex's trust flow, OpenCode's instruction watcher), or a live `agy` check settles whether Antigravity re-reads its rules per turn (the message would then match Claude's). Also if `run_enhance` starts reporting `developer_instructions` changes, so the Codex "nothing detected" case can say something more specific.
