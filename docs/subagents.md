# Subagent transcripts

How Bram shows a subagent's transcript, and what Codex and OpenCode need to
get the same treatment. Today only Claude's subagents appear. The choice of
how to find Codex and OpenCode children is recorded in
[`decisions/2026-10-30-subagent-transcript-provider-discovery.md`](decisions/2026-10-30-subagent-transcript-provider-discovery.md);
this doc covers everything around it.

## What the feature is

A subagent is a child agent working inside its parent's turn. Bram shows each
one as a chip on the footer's session line, and clicking the chip shows that
child's turns in the Transcript. `SubagentPeek` shows them inline under the
tool call that dispatched the child.

Bram doesn't spawn subagents. It only reads what the provider recorded. A
child's activity is never the main session's activity.

## The provider seam

| | Claude (shipped) | Codex (gap) | OpenCode (gap) |
|---|---|---|---|
| Where the child is | `<sid>/subagents/agent-<id>.jsonl`, or under `workflows/wf_*/` | its own rollout in `~/.codex/sessions/<y>/<m>/<d>/` | a row in `session_v2` |
| Link to parent | the directory | `parent_thread_id` in `session_meta` | `parent_id` |
| Child's id | `agent-<id>` | `id` (its `session_id` is the **parent's**) | the row's `id` |
| Label | `.meta.json`: `agentType`, `description`, `toolUseId` | `agent_path`, `agent_nickname` | `agent`, `title` |
| Link to the dispatching call | `toolUseId` | the parent's `spawn_agent` call (join not worked out) | the `subagent` tool's `metadata.sessionID` |
| Explicit completion | `<task-notification>` in the parent | `SubagentStop` hook; Bram doesn't register it | child's `idle_outcome`, and the parent's `<subagent … state="completed">` result |
| Evidence | `st_subagent_files` | [`codex` specimens](agent-specimens/codex/README.md), rollouts `01a18d7d…` and `01a18ddf…` | [`opencode` specimens](agent-specimens/opencode/README.md) |

A specimen is not proof of behaviour on a real project: this machine's
OpenCode database had no child rows on 2026-10-30, so the OpenCode column rests
on one captured run (2.1.4). Cursor and Antigravity are out of scope here.

## Where the code assumes Claude

Everything goes through one helper, and two more checks share its assumption.

- **`st_subagent_files`** finds children by walking the directory from
  `st_session_subagents_dir`, which is the sidecar layout. For a Codex or
  OpenCode session that directory doesn't exist, so the answer is an empty
  list, not an error. Its callers:
  - `read_subagent_roster`, behind `/__agents`, which drives the chips
  - `read_subagent_turns`, behind `/__turns?agent=`
  - `st_tag_agent_tool_entries`, which gives tool rows their peek
  - `sweep_subagents_for_pending_call` and the pending-call lookups, which
    find a subagent's permission prompt
- **`is_subagent_transcript_path`** tests for a path component named
  `subagents`. A Codex child rollout has none, so this guard can't see one.
- **`is_own_subagent_transcript_path`** decides when the watcher emits
  `subagents-changed`, and it's anchored to the Claude sessions directory.

Both routes resolve the parent through `active_session_path`, and that is
provider-aware already. The coupling is below it.

To extend this, put the discovery behind one provider-aware function that
returns what `SubagentFile` holds today, and keep the routes' response shape.
The chips, the Transcript view and `SubagentPeek` then need no changes.
OpenCode exports are Claude-shaped, so its children should fit the same
projection pass. Whether a child gets exported at all is still open.

## Completion

`st_subagent_state_str` derives three states:

- **`finished`:** the parent recorded completion
  (`st_parent_agent_task_finished`). This beats recency.
- **`running`:** the transcript changed within
  `SUBAGENT_RECENT_ACTIVITY_MS` (2 minutes).
- **`waiting`:** neither. It's stopped or wedged, and Bram doesn't guess which.

Only explicit evidence sets `finished`. A failed read yields `waiting`, never a
false completion. Keep that rule for the new providers:

- **OpenCode** has explicit evidence on the child row (`idle_outcome`).
- **Codex** is the open case. `SubagentStop` is explicit, but relying on it
  means registering a hook, which re-raises Codex's "Hooks need review"
  overlay, and it can't speak for past sessions. The better candidate is the
  rollout's own `task_complete` event, which ends child `01a18ddf…`. That's one
  specimen, and it marks the end of a turn. Whether a child can take another
  turn afterwards hasn't been checked.
- **Claude** has a trap: its `SubagentStop` also fires on ordinary turns with
  an empty `agent_type` (see the
  [specimen index](agent-specimens/README.md)), so don't read it as "a
  subagent ran".

## Failure classes to design against

Each has already happened in Bram. Codex's are from the trace of session
`01a18d7d` on 2026-10-30; a Codex parent and child both ran in that session.

**A child is taken for the main session.** Observed for Codex: `/__agents`
returned an empty roster, named the child rollout as the active session, and
Bram switched between parent and child. The cause is visible:
`latest_codex_session_path` follows the most recent writer among rollouts with
a matching `cwd`, and a child has the same `cwd`, so a working child can
outrank its parent. The child has to be excluded from selection and
added to the roster; doing only one leaves the bug. OpenCode's
`parent_id IS NULL` filter already does the first half. For Claude the
directory layout prevents this.

**Child activity drives main-session state.** `bdbf4a36d` fixed this for
Claude: a background subagent writing after the main turn ended re-armed the
claim and locked the Worklist row. The fix gates main-session effects behind
`is_subagent_transcript_path`, which, as above, doesn't recognise Codex
children. Expect the same bug there until it does.

**Menu answers miss, and menus come back.** Observed for Codex:
`answer-miss reason=no-key-match` with repeated menu reappearances, alongside
the parent/child switching. The cause isn't established. The Claude history is
the place to start: `40855ce4` made hook menu claims a queue, because parallel
subagent prompts defeat a single slot, and `d0c2e80e` bound an answer to the
displayed prompt's own id instead of the oldest pending call.

**A subagent's edits land outside any claim.** The commit for #461 came out
entirely unowned. A subagent's timing is suspected but unconfirmed. Line
ownership is tracked in #436, and #454's grant redesign may change this class.
Don't count it as understood.

## Refresh

Follow [Push over polling](developing-bram.md#push-over-polling). Children
should arrive through `subagents-changed`, so the pane needs no new
subscription. For Codex, the watcher needs to recognise a child rollout. For
OpenCode, the data is in SQLite, so the signal has to come from the plugin or
the host. The plugin's `forwardTurns` drops child sessions from the turn
stream, and that must stay true for turn state: a child's signal needs a
separate path.

## Open

- Whether `task_complete` means a Codex child has finished, or only a turn.
- How to join a Codex child to its `spawn_agent` call, for the inline peek.
- How OpenCode child transcripts get exported, and how the host learns they
  changed.
- Whether prompt sweeps should cover Codex and OpenCode children. Unsettled
  until the `no-key-match` cause is known.
- Dimming Commit and Drop during delegated work
  (`delegated-work-action-gates`), which is separate from showing transcripts
  and touches #454's area.
