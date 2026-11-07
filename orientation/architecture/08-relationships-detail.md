# Item, agent, session, in detail: detail

Click-through detail for 08-relationships-detail.mmd. Facts are as of commit `f5f40783` (2026-11-07); the rest of the site is pinned to `e8a38ac0`. This expands the previous diagram, "Item, agent, session", with the parts that make its links: the selection that addresses a message, the message itself, the claim a turn holds, the group that ties sessions across agents, and the ledger where item–session stamps are kept. Each arrow says what links two of them and whether Bram writes the link down. What the expansion shows that the simple view hides: an item has no agent of its own. It has sessions, each session has an agent, and that is the whole item–agent relationship. Two links are never written: the session that proposed an item, and the session that committed it. The open issue is [judell/bram#481](https://github.com/judell/bram/issues/481).

## Worklist item

<!-- read: app/__shell/conventions.md#What worklist items represent (and when to drop) -->

"Worklist items represent repository changes." An item "names a `file` (or `files`) plus `before` / `after` prose" in its draft. [`app/__shell/conventions.md:110-116`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L110-L116) The metadata item has `id`, `status`, `files` and `closesIssues`, and nothing naming an agent or a session. [`app/__shell/conventions.md:196-206`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L196-L206) The item's sessions are attached to the board payload at serve time and "never written to the file". [`src-tauri/src/lib.rs:23124-23131`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23124-L23131)

## Selection

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

The ticked rows. The selection is focus state, "what the user is working on now", kept in `sessionStorage` so that "it survives a reload, dies with the session, and is project-scoped so another Bram window never sees it". [`app/__shell/helpers.js:13273-13283`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L13273-L13283) "After **Start** or feedback the items stay selected, so follow-ups arrive as `iterate:`; other gate verbs clear it." [`app/__shell/conventions.md:299-300`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L299-L300)

## A message you send

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

A gate click (Start, Start & commit, Commit, Drop) or Enter in the message box. The gate verbs' wire kinds are `approved:`, `drop:` and `iterate:`; a message with items selected is `iterate:`, and one with nothing selected is ordinary chat. [`app/__shell/conventions.md:302-317`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L302-L317) Before delivery the host runs its prefix records on the raw text, among them the item–session stamp. [`src-tauri/src/lib.rs:23674-23680`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23674-L23680)

## Agent

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

The harness running in the terminal. `switch_agent` accepts `codex`, `claude`, `opencode` and `antigravity`. [`src-tauri/src/lib.rs:24775-24786`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24775-L24786) The current one is read from the active-agent hint file. [`src-tauri/src/lib.rs:33431-33436`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L33431-L33436) The Providers diagram on this site shows how each one is wrapped.

## Session

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

One agent's conversation. The decision: "Bram chooses the session, launches it by id, marks it current at launch, and groups sessions only through an explicit act tied to that id." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:1-7`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L1-L7) After launch, "a session is current because something named it": the running CLI's own event first, Bram's launch second, newest-file last. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:9-16`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L9-L16) The host's accessor is `live_session_id`, per provider. [`src-tauri/src/lib.rs:41751-41754`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L41751-L41754)

## Session group

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

"A line of work is a *group*: at most one session per agent, any number of agents, and a session is in at most one group." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:50-50`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L50-L50) Groups are stored "as `kind='group'` transitions rows, append-only and replayed in order", with `join` and `leave` operations; "a group left with fewer than two members dissolves." [`src-tauri/src/worklist_state.rs:236-250`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L236-L250) The readers and writers are `session_group`, `join_group` and `leave_group`. [`src-tauri/src/worklist_state.rs:390-390`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L390-L390) [`src-tauri/src/worklist_state.rs:420-426`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L420-L426) [`src-tauri/src/worklist_state.rs:459-459`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L459-L459)

## Subagent

<!-- read: docs/subagents.md#What the feature is -->

"A subagent is a child agent working inside its parent's turn. Bram shows each one as a chip on the footer's session line, and clicking the chip shows that child's turns in the Transcript." "Bram doesn't spawn subagents. It only reads what the provider recorded. A child's activity is never the main session's activity." [`docs/subagents.md:9-18`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/subagents.md?plain=1#L9-L18) The session decision adds: "A subagent isn't the session." Claude's and Codex's subagent hooks "carry the parent's `session_id` plus an `agent_id`, and the guard skips any payload with one." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:28-28`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L28-L28)

## Execution claim

<!-- read: app/__shell/conventions.md#The claim ends with your turn -->

"The claim (the running spinner) is execution state: the host writes it while a turn runs, retires it when the turn ends, and re-arms it next turn while an authorization is live." [`app/__shell/conventions.md:363-369`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L363-L369) The writer is `write_inflight_claim_sentinel`. [`src-tauri/src/lib.rs:69921-69921`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L69921-L69921) The Worklist diagram on this site shows where the claim sits in the item's lifecycle.

## Item-session ledger

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

The state mirror's `transitions` table, `kind='session'`: "Bram records which session received an item's Start / Commit / Drop / feedback turn", "written only when the session differs from the item's latest one, so ordinary turns add no rows." The rows are per machine, "so the fact lives where it's true", and a failed write "is a missed stamp, healed by the item's next delivery". [`src-tauri/src/worklist_state.rs:165-176`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L165-L176) Each row's detail is `{provider, sessionId}`. [`src-tauri/src/worklist_state.rs:182-204`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L182-L204) The writer is `record_item_session`. [`src-tauri/src/worklist_state.rs:206-227`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L206-L227)

## Transcript

<!-- read: docs/subagents.md#What the feature is -->

The pane reads a session's turns from `/__turns`, which takes a provider, a session id, or `agent=<agentId>` for a subagent's transcript. [`docs/apis.md:334-334`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/apis.md?plain=1#L334-L334) The Transcript's `subagentTurns` DataSource fetches with `agent: $props.agentId` and refetches on `subagents-changed`. [`app/tools/components/Transcript.xmlui:85-101`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/tools/components/Transcript.xmlui?plain=1#L85-L101)

## ticked on the board

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

"User triages — ticks rows, optionally types one message, clicks a button. The message fans out to every selected item." [`app/__shell/conventions.md:279-283`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L279-L283)

## ticked: feedback on them; none: chat

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

"Selection decides who a message is addressed to. With one or more Worklist items selected, a message the user sends … is **feedback addressed to those items**, and arrives as an `iterate:` turn. With nothing selected, it is **general chat**." The placeholder "shows where Enter will send", and **Send as chat instead** "sends that one message as general chat without clearing the selection". [`app/__shell/conventions.md:291-301`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L291-L301)

## goes to the current agent

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

The stamp, like the delivery, takes the agent from `current_provider`, the hint file, and the session from `live_session_id` for that provider. [`src-tauri/src/lib.rs:23072-23082`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23072-L23082) Nothing on the item is consulted to choose the agent.

## lands in its live session

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

The session that is current for that provider. "The running CLI names its own session in an event, from this Bram's terminal. Authoritative: it replaces a pin. Bram names it at launch. It holds until (1) says otherwise. Newest wins only when neither has spoken." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:9-16`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L9-L16)

## stamps (agent, session) on each item

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

`record_item_session_stamp` runs on every delivered turn: it takes the ids from an `approved:`, `drop:` or `iterate:` prefix, the current provider and its live session, and writes one row per id. [`src-tauri/src/lib.rs:23052-23058`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23052-L23058) [`src-tauri/src/lib.rs:23072-23092`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23072-L23092) A row is written only when the session differs from the item's latest, and a write re-emits `worklist-changed`, since "a stamp never touches `worklist.json`". [`src-tauri/src/lib.rs:23114-23120`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23114-L23120)

## newest shown as Last worked on, with Switch there

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

`worklist_attach_last_sessions` reads `latest_item_session` for each item and attaches a single `lastSession` `{provider, sessionId, atMs, title, isCurrent}`; "a session that no longer resolves gets no field, so the pane offers nothing rather than a dead switch." [`src-tauri/src/lib.rs:23124-23150`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23124-L23150) The gate bar's line reads "Last worked on by <agent> in '<title>'." when the agent differs, or "Last worked on in another session: '<title>'." when only the session does. [`app/__shell/helpers.js:1973-1998`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L1973-L1998) **Switch there** reloads that session and lands on the Transcript. [`app/__shell/helpers.js:2000-2029`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L2000-L2029) Older rows are kept but nothing serves or shows them.

## proposing and committing record nothing

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

The stamp is called from one place, the delivery path, and only for turns with an `approved:`, `drop:` or `iterate:` prefix. [`src-tauri/src/lib.rs:23052-23058`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23052-L23058) [`src-tauri/src/lib.rs:23674-23680`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L23674-L23680) Writing an item is an agent's file write, with no turn prefix, so nothing is stamped. The commit route writes no stamp either. [`src-tauri/src/lib.rs:93956-93960`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L93956-L93960) The history snapshot "writes the *prior* contents to worklist-history/<unix_ms>.json plus a changelog .md", the board as it was, with no session. [`src-tauri/src/lib.rs:82343-82352`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L82343-L82352)

## no direct link: through its sessions

<!-- read: app/__shell/conventions.md#Schema and draft layout -->

The item's schema has no agent field. [`app/__shell/conventions.md:196-206`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L196-L206) Each ledger row carries the provider beside the session id, so "which agents worked on this item" is answered by reading the item's rows. [`src-tauri/src/worklist_state.rs:182-204`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L182-L204) The repo stores nothing else that ties an item to an agent.

## in at most one group, one session per agent

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

A `join` row "moves (p, id) out of any other group into g, replacing g's current member for p". [`src-tauri/src/worklist_state.rs:239-246`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/worklist_state.rs?plain=1#L239-L246) Joining is explicit: "the offer's Group these two, or a loose row's Add to '<group>'. A resume across agents no longer joins." Members share the group's name. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:50-50`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L50-L50)

## switching agents resumes its member, or starts one and adds it

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md -->

In `switch_agent`: "a switch never lands in another group's session. From inside a group, the group decides: its member for the target agent, or a new session made for the group. From a loose session, the target's most recent *loose* session, or a new one when every session it has is grouped." [`src-tauri/src/lib.rs:24878-24892`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24878-L24892) When the group has no member for the target, the fresh-partner reason is `group` and the pairing source `switch-group`, so the new session is added to the group. [`src-tauri/src/lib.rs:24926-24932`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24926-L24932) [`src-tauri/src/lib.rs:24958-24965`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L24958-L24965) The switch reloads the pane, and the selection survives that reload, so items ticked before the switch are still the address afterwards. [`app/__shell/helpers.js:1209-1211`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L1209-L1211) [`app/__shell/helpers.js:13273-13283`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/helpers.js?plain=1#L13273-L13283)

## claims the items while the turn runs

<!-- read: app/__shell/conventions.md#The claim ends with your turn -->

"The host records the approval and claim before delivering an approved turn; it also establishes claims for addressed feedback." [`app/__shell/conventions.md:318-320`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L318-L320) The host "writes it while a turn runs, retires it when the turn ends". [`app/__shell/conventions.md:363-369`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/app/__shell/conventions.md?plain=1#L363-L369)

## a subagent spawned under it inherits its ids

<!-- read: docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md -->

"Claude only, for now." "A subagent inherits the claim it was spawned under. On `SubagentStart`, if the main turn holds a claim, the host binds the subagent's `agent_id` to that claim's item ids. A subagent spawned with no claim (a chat turn) binds to nothing." [`docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md:9-10`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md?plain=1#L9-L10) The route is `handle_agent_subagent`; on `SubagentStart` it calls `bind_locked`, which reads the claim and returns `bind-skipped reason=no-claim` when there is none. [`src-tauri/src/lib.rs:72849-72893`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L72849-L72893) [`src-tauri/src/lib.rs:72519-72540`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L72519-L72540) Bindings live in `.delegated-bindings.json`. [`src-tauri/src/lib.rs:72185-72185`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L72185-L72185)

## edits credited; rows locked until its stop

<!-- read: docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md -->

"While the binding is live, its items stay locked for Commit, Start & commit and Drop, and the subagent's writes are credited to them." "A binding is released only on evidence, never on a timeout": `SubagentStop`, the end of the spawning session, a Bram restart, or a traced **Release** on the row. [`docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md:10-16`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/decisions/2026-11-04-delegated-work-binds-to-the-spawning-claim.md?plain=1#L10-L16) The pane's `heldByDelegation` says when every claimed id is held only by subagents. [`src-tauri/src/lib.rs:88152-88152`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/src-tauri/src/lib.rs?plain=1#L88152-L88152)

## its turns

<!-- read: docs/subagents.md#The provider seam -->

The Providers diagram on this site names where each provider writes its turns. The pane reads them through `/__turns`. [`docs/apis.md:334-334`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/apis.md?plain=1#L334-L334)

## a chip shows its own turns

<!-- read: docs/subagents.md#What the feature is -->

`/__agents` serves "the roster of the active session's subagents for the footer chips". [`docs/apis.md:340-340`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/apis.md?plain=1#L340-L340) Where each provider keeps a child, how it links to the parent, and what Bram can and cannot join up, is the provider seam table. [`docs/subagents.md:20-33`](https://github.com/judell/bram/blob/f5f4078303b57960c74ffd6cd6a6ccc9412b6f4a/docs/subagents.md?plain=1#L20-L33)
