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

## Amendment, 2026-11-19: narrowing from the brief ([#478](https://github.com/judell/bram/issues/478))

**The revisit trigger fired.** One approval covering several items, with one subagent sent to each, bound every subagent to every item. Each row showed every subagent's label, and early release was refused for all of them. Two receipts:

- 2026-11-15 in `~/bram`: "Subagent working: Local file route for browser" on both rows (#478);
- 2026-11-19 in `~/xmlui`: "Ablation builds for per-instance memory" on `xmlui-churn-combined-state`, which wasn't that subagent's work.

**The amendment: narrow on evidence, never widen.**

1. `SubagentStart` still binds to the whole inheritable claim, so a subagent's items are never left unlocked.
2. Once the subagent's evidence is readable, the binding narrows to the bound ids its description (`meta.json`) names, or failing that the ids its brief (the transcript's first record) names, matching whole ids only. Naming none, or every bound id, keeps the whole binding. The check runs once per binding, at `SubagentStart`, on `/__inflight` reads, on roster reads, and before a commit, drop or mutate checks bindings. Traced `[delegated] op=narrow … source=description|brief` or `op=narrow-skipped reason=…`.
3. A narrowed agent that rebinds (a resume) keeps its narrowing.
4. Ids a narrowing frees behave as unbound: their rows unlock, and with no main turn running they retire from a claim that only bindings held.

**What it doesn't change:** line credit is by claim interval, and the claim holds the union of live bindings. While two narrowed subagents run at once, each one's lines still credit both items. Credit tightens only as bindings release.

**Still rejected:** the declare route. Naming the item in the description or brief is the same information without a call to forget: forgetting it falls back to the whole claim, which errs safe.

