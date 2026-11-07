# Item, agent, session: detail

Click-through detail for 07-relationships.mmd. Facts are as of commit `f5f40783` (2026-11-07); the rest of the site is pinned to `e8a38ac0`. Five things can bear on a Worklist item: an agent, a session of that agent, a subagent inside one of the session's turns, and the Transcript where the session or the subagent is read. The picture shows the relationships Bram keeps among them. An item is linked to the agent that last worked on it and the session that work landed in; a session is one agent's; a subagent is an agent's child and can work on the item its parent was working on; the Transcript shows a session or a subagent, one at a time. The next diagram, "Item, agent, session, in detail", expands this one with the message, the selection, the claim, the group and the ledger, and with what is and isn't recorded. The open issue is [judell/bram#481](https://github.com/judell/bram/issues/481).

## Worklist item

<!-- read: app/__shell/conventions.md#What worklist items represent (and when to drop) -->

"Worklist items represent repository changes." An item "names a `file` (or `files`) plus `before` / `after` prose" in its draft. [`app/__shell/conventions.md:110-116`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L110-L116) The metadata item has `id`, `status`, `files` and `closesIssues`. [`app/__shell/conventions.md:196-206`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L196-L206) Its agent and session are attached to the board payload at serve time as `lastSession`, "never written to the file". [`src-tauri/src/lib.rs:23124-23131`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23124-L23131)

## Claude

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

One of the four harnesses `switch_agent` accepts. [`src-tauri/src/lib.rs:24775-24786`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24775-L24786) It names its own session through `SessionStart` and `UserPromptSubmit` in the menu hook. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:17-19`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L17-L19) It is the only harness whose subagents are bound to items: "Claude only, for now. Claude Code gives the evidence this needs." [`docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md:9-9`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md?plain=1#L9-L9) The Providers diagram on this site shows how it is wrapped.

## Codex

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

Accepted by `switch_agent`. [`src-tauri/src/lib.rs:24775-24786`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24775-L24786) Its session is named by "the existing `PreToolUse` guard", which "reports `session_id`", so a change shows at the next tool call. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:17-20`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L17-L20) Its subagents are shown in the Transcript; their work is not bound to items. [`docs/subagents.md:20-33`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/subagents.md?plain=1#L20-L33)

## OpenCode

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

Accepted by `switch_agent`. [`src-tauri/src/lib.rs:24775-24786`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24775-L24786) "Bram launches `opencode --standalone`, so the plugin runs in a private server under this terminal", and its plugin events name the session. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:17-22`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L17-L22)

## Antigravity

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

Accepted by `switch_agent`. [`src-tauri/src/lib.rs:24775-24786`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24775-L24786) Its `PreInvocation` hook "posts `conversationId` to `/__agent/session`". [`docs/decisions/2026-10-27-bram-chooses-the-session.md:17-21`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L17-L21)

## Session

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

One agent's conversation. "Bram chooses the session, launches it by id, marks it current at launch, and groups sessions only through an explicit act tied to that id." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:1-7`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L1-L7) "A line of work is a *group*: at most one session per agent, any number of agents, and a session is in at most one group." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:50-50`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L50-L50)

## Subagent

<!-- read: docs/subagents.md#What the feature is -->

"A subagent is a child agent working inside its parent's turn. Bram shows each one as a chip on the footer's session line, and clicking the chip shows that child's turns in the Transcript." "Bram doesn't spawn subagents. It only reads what the provider recorded." [`docs/subagents.md:9-18`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/subagents.md?plain=1#L9-L18)

## Transcript

<!-- read: docs/subagents.md#What the feature is -->

The pane reads a session's turns from `/__turns`, which takes a provider, a session id, or `agent=<agentId>` for a subagent's transcript. [`docs/apis.md:334-334`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/apis.md?plain=1#L334-L334) The Transcript's `subagentTurns` DataSource fetches with `agent: $props.agentId`. [`app/tools/components/Transcript.xmlui:85-101`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/tools/components/Transcript.xmlui?plain=1#L85-L101)

## last worked on by

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

When a Start, Commit, Drop or feedback turn is delivered, the host records the current provider and its live session on each addressed item. [`src-tauri/src/lib.rs:23072-23092`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23072-L23092) The newest record is served as `lastSession` with its `provider`. [`src-tauri/src/lib.rs:23124-23150`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23124-L23150) The gate bar names the agent when it differs from the current one: "Last worked on by <agent> in '<title>'." [`app/__shell/helpers.js:1973-1998`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L1973-L1998)

## last touched in

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

The same record carries the session id; a row is written "only when the session differs from the item's latest one". [`src-tauri/src/worklist_state.rs:165-176`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L165-L176) When the session differs from the current one, the gate bar reads "Last worked on in another session: '<title>'." and offers **Switch there**, which reloads that session and lands on the Transcript. [`app/__shell/helpers.js:1973-1998`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L1973-L1998) [`app/__shell/helpers.js:2000-2029`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L2000-L2029)

## runs in

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

One agent runs in the terminal at a time; the current one is read from the active-agent hint file. [`src-tauri/src/lib.rs:33431-33436`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L33431-L33436) The host's `live_session_id` answers one session per provider. [`src-tauri/src/lib.rs:41751-41754`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L41751-L41754) In `switch_agent`, "a switch never lands in another group's session. From inside a group, the group decides: its member for the target agent, or a new session made for the group." [`src-tauri/src/lib.rs:24878-24892`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24878-L24892)

## spawns, inside a turn

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

"A subagent isn't the session." Claude's and Codex's subagent hooks "carry the parent's `session_id` plus an `agent_id`". [`docs/decisions/2026-10-27-bram-chooses-the-session.md:28-28`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L28-L28) Where each provider keeps a child and how it links to the parent is the provider seam table. [`docs/subagents.md:20-33`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/subagents.md?plain=1#L20-L33)

## works on, under its parent's claim

<!-- read: docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md -->

"Claude only, for now." "A subagent inherits the claim it was spawned under. On `SubagentStart`, if the main turn holds a claim, the host binds the subagent's `agent_id` to that claim's item ids." "While the binding is live, its items stay locked for Commit, Start & commit and Drop, and the subagent's writes are credited to them." [`docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md:9-16`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md?plain=1#L9-L16) The route is `handle_agent_subagent`. [`src-tauri/src/lib.rs:72849-72893`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L72849-L72893)

## shown in

<!-- read: docs/subagents.md#What the feature is -->

Both arrows: the Transcript shows a session's turns, or one subagent's. The Providers diagram on this site names where each provider writes its turns. The pane reads them through `/__turns`, which takes a session id, or `agent=<agentId>` for a subagent. [`docs/apis.md:334-334`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/apis.md?plain=1#L334-L334) `/__agents` serves "the roster of the active session's subagents for the footer chips". [`docs/apis.md:340-340`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/apis.md?plain=1#L340-L340) The Transcript refetches the chip-selected child on `subagents-changed`. [`app/tools/components/Transcript.xmlui:85-101`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/tools/components/Transcript.xmlui?plain=1#L85-L101)
