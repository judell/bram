# Bram finds subagent transcripts from each provider's own parent link

Status: decided 2026-10-30 · revisit when a provider's child record changes shape

**Question:** Bram shows subagent transcripts for Claude only, by walking `<sid>/subagents/`. How should it find Codex and OpenCode children?

**Decision:** each provider's durable store is the source of truth, and the child's own record names its parent.

| Provider | Child is | Find it by | Child's own id | Transcript |
|---|---|---|---|---|
| Claude (shipped) | a sidecar file | walking `<sid>/subagents/agent-<id>.jsonl` + `.meta.json` | `agent-<id>` | the sidecar JSONL |
| Codex | a separate rollout | `parent_thread_id` in `session_meta` | `payload.id` | the child's rollout |
| OpenCode | a `session_v2` row | `parent_id` | the row's `id` | the child session's own records |

Three rules follow from the specimens:

- **Codex: never identify a child by `session_id`.** A child's `session_id` is its parent's. Its own id is `id`.
- **Codex: confirm a child with `thread_source == "subagent"`,** not with the presence of `forked_from_id`. A child's `source.subagent.thread_spawn` also carries `depth`, `agent_path` and `agent_nickname`, which give the chip its label.
- **OpenCode: keep the `parent_id IS NULL` filter on the session list.** Child lookup is a separate query, so children reach the roster without becoming candidates for the main session.

This note settles discovery only. The route changes, completion tracking and failure classes belong to `docs/subagents.md` and later items.

**Evidence:**

- Codex, in the wild: parent `01a18d7d…` and child `01a18ddf…` (2026-10-30, CLI 0.166.0). The child has `parent_thread_id` and `forked_from_id` set to the parent, `thread_source: "subagent"`, `depth: 1`, `agent_path: "/root/feedback_labels"` and `agent_nickname: "Bohr"`.
- Codex, captured: [`subagent-session-meta-record.jsonl`](../agent-specimens/codex/subagent-session-meta-record.jsonl) (0.165.0) shows the same fields. [`SubagentStart.json`](../agent-specimens/codex/hooks/SubagentStart.json) and [`SubagentStop.json`](../agent-specimens/codex/hooks/SubagentStop.json) show the hooks firing with the parent's `session_id` and the child's `agent_id`. `SubagentStop` adds `agent_transcript_path`.
- OpenCode, captured (2.1.4): [`session-meta-record-subagent.json`](../agent-specimens/opencode/session-meta-record-subagent.json) is a child row with `parent_id`, `agent: "explore"`, `time_idle` and `idle_outcome: "succeeded"`. [`transcript-record-subagent-child.json`](../agent-specimens/opencode/transcript-record-subagent-child.json) is its transcript, and [`subagent-event-order.txt`](../agent-specimens/opencode/subagent-event-order.txt) shows the interleaved events.
- OpenCode schema: `session_v2.parent_id` exists and has an index, `session_v2_parent_idx`. Today, `opencode_project_session_rows_in` filters `parent_id IS NULL` (`src-tauri/src/lib.rs`), and `forwardTurns` in `app/__shell/opencode/bram-plugin.js` skips child sessions. So children are hidden by Bram, not missing from the data.
- The live OpenCode DB on this machine had 24 sessions and no children on 2026-10-30, so the OpenCode contract rests on the captured specimen rather than on real work.
- Bram's Codex scanner has nothing to go on yet: `codex_session_meta_read` reads only `id` and `cwd`, and `lib.rs` never reads `thread_source`, `parent_thread_id` or `source.subagent`. The Codex specimen README already says the scanner should key on `source.subagent` or `thread_source`.

**Alternatives:**

- *Codex: reconstruct the child from the parent's inline records.* Rejected: `agent_message` records are the collaboration channel, not the child's work, and the `spawn_agent` message is encrypted at rest.
- *Codex: discover through `SubagentStart`/`SubagentStop` hooks.* Rejected as the source of truth: a hook only sees children spawned while Bram is listening, so past sessions would have none, and adding a hook re-raises Codex's "Hooks need review" overlay. They remain usable as a live signal if one is needed.
- *OpenCode: reconstruct from the plugin's event stream.* Rejected: same gap as hooks, and Bram already reads `session_v2`.
- *Copy children into a Claude-style sidecar layout.* Rejected: a second copy of a transcript the provider already stores durably.

**Revisit when:** a Codex or OpenCode release changes these fields (the evidence covers Codex 0.165.0–0.166.0 and OpenCode 2.1.4), or the first real OpenCode child disagrees with the captured one.
