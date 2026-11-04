# Delegated work binds to the claim it was spawned under, and is released only on evidence

Status: decided 2026-11-04 · revisit when one turn routinely sends different subagents to different items, or when another provider offers subagent start and stop evidence

**Question:** when the orchestrator hands an item's work to a subagent, how does the host know which items the subagent is working on, and when the subagent is done? Today it knows neither. The main claim retires when the orchestrator's turn ends, so the rows' Commit and Drop re-enable while the subagent is still editing, and the subagent's lines are credited to no item.

**Decision:**

1. **Claude only, for now.** Claude Code gives the evidence this needs. Other providers keep today's behavior.
2. **A subagent inherits the claim it was spawned under.** On `SubagentStart`, if the main turn holds a claim, the host binds the subagent's `agent_id` to that claim's item ids. A subagent spawned with no claim (a chat turn) binds to nothing. While the binding is live, its items stay locked for Commit, Start & commit and Drop, and the subagent's writes are credited to them.
3. **A binding is released only on evidence, never on a timeout.** The events that release it:
   - `SubagentStop` for that `agent_id`;
   - the end of the session that spawned it;
   - a Bram restart, which starts with no bindings;
   - a traced **Release** on the row, for a subagent whose stop never arrived.

**Evidence:**

- **The gap, measured on 2026-11-04.**
  - Two delegated items' rows re-enabled at every orchestrator turn end while their subagent was still editing.
  - The commits carried the subagent's lines as `unownedTaken`: `b74ae59` (`lib.rs` +235 −21 plus two docs) and `3b8bd4c` (19 lines of `developing-bram.md`).
  - Committing delegated items together was the workaround, on macOS and on Windows (#471).
- **What the evidence rests on.** Claude Code's [hooks reference](https://code.claude.com/docs/en/hooks):
  - `SubagentStart` and `SubagentStop` carry `agent_id`;
  - every tool call a subagent makes carries the same `agent_id`, which Bram's guard already reads (`guard_policy.rs`, `subagent-lifecycle-call`).

**Alternatives:**

- **The orchestrator declares each binding through a host route.** This is exact when one turn sends subagent A to item 1 and subagent B to items 2 and 3. Rejected for now: it's one more call every agent must remember, and forgetting it silently restores today's behavior. Inheriting errs safe instead: with two subagents on one claim, every item stays locked until both finish.
- **Release after a quiet period.** Rejected. A subagent can be quiet through a long build or test run, and releasing it then re-enables Commit on half-finished work, which is the failure this decision exists to prevent.
- **A session-wide lock while any subagent runs.** Rejected. It blocks unrelated items, the long-running-work problem from #348 and #368.

**Revisit when:** over-locking from inheritance costs real time (then add per-dispatch declaration alongside it), or Codex, OpenCode or Antigravity expose equivalent start and stop evidence.
