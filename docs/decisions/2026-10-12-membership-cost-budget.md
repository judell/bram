# Membership's cost budget, set before the replay is deleted

Status: decided 2026-10-12 · revisit if `op=membership-over-budget` fires in ordinary use, or a field report names Worklist latency

**Question:** #425 criterion 7 (`docs/attribution-model.md`) requires a cost budget for membership, set from the replay's measured baseline before the replay goes. What are the bounds, and how are they kept?

**Decision:**

| What | Bound | Why this one |
|---|---|---|
| Board read: membership's share (memo read) | **200 ms** per read | The cost the pane feels on every refresh. The strictest bound. |
| Background compute (`membership_precompute`) | **5 s** per run | Off the request path, but it sets how stale the board is after an edit. |
| Commit gate: fresh membership check | **1 s** per commit | Adds directly to every commit's time. |
| Git spawns | memo read: constant (3); compute: one diff per claim step plus 2 | Criterion 7's own measure. A compute grows with claim history, never with tree size. |

Each bound is checked **per event**. A run over its bound writes `op=membership-over-budget kind=<serve|compute|gate> ms=<n> limit=<n>` to the trace, so a breach reports itself the way a conservation breach does. Percentiles come from the `op=membership`, `op=gate-membership` and `op=commit-precheck` lines when someone asks.

**Evidence:** Jon's working instance, `resources/bram-traces/bram-trace.log`, 2026-10-12 21:13 to 2026-10-13 03:25, with the replay still running beside membership (so this is the "before" baseline, measured side by side):

| What | n | p50 | p90 | max |
|---|---|---|---|---|
| Replay, every board read (`op=attribute`) | 80 | 135 ms | 351 ms | 1,165 ms |
| Membership, memo read (`cached=true`) | 155 | 23 ms | 114 ms | 170 ms |
| Membership, compute (`cached=false`) | 109 | 50 ms | 676 ms | 1,096 ms |
| Membership at the gate (`op=gate-membership`) | 3 | 461 ms | n/a | 526 ms |

#425's earlier replay baseline was p90 223 ms, max 6,852 ms. Gate-time membership was 47–117 ms in the 2026-10-03 to 06 soak. Today's 461–526 ms reflects a longer claim history (18 spawns per compute), still inside the bound, and the number to watch.

**Alternatives:**

- **Percentile bounds checked by a periodic report.** Rejected for now: nobody runs the report until something feels slow, and history is lost to trace rotation (the 2026-10-12 attempt to count post-4a interval commits found no traces older than that day). A per-event tripwire needs no one to remember.
- **Bounds equal to the replay's numbers.** Rejected: the replay's p90 (351 ms) is what is being retired, and membership already beats it by 3x on reads.

**Revisit when:** the tripwire fires in ordinary use (not a deliberately huge fixture), or the gate bound is approached as claim histories lengthen. The likely fix then is compacting claim steps whose items have left the board, not raising the bound.
