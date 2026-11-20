# One started item per file at a time; same-tree hunk separation is retired, not frozen

Status: decided 2026-11-19 · revisit on field demand for same-tree parallel work backed by real commits

**Question:** Several started items may declare the same file, and the host infers afterwards which lines were whose (claim intervals, joint claims, ghost rules, interval staging, re-anchor). [`2026-09-24-freeze-same-tree-hunk-separation.md`](2026-09-24-freeze-same-tree-hunk-separation.md) froze new investment in that machinery as guidance. Should the policy instead be enforced, so the machinery can go?

**Decision:** Enforce it. **Start**, **Start & commit** and a plan's `start` step are refused for an item whose `files` overlap those of any started item still on the board, naming the row in the way and the shared file. The unblock is Commit or Drop on that item, or sending the change as feedback on it. With one started item per file, every changed line in a file belongs to the one started item declaring it, so staging is whole files and the hunk-separation machinery (problem 2 of the September note) is removed in follow-up items, not kept as a dormant fallback. The Start snapshots and the per-line rule (problem 1) stay: they are how a chat edit or a post-turn subagent edit in the same file is told from the item's own work. Worktrees are meant to remain the route for real parallel work on one file, but as decided the refusal has no worktree exemption, so the #404 protocol (one subagent per item in its own worktree, items dropped at the end) can't start two overlapping items. A worktree-aware Start is a follow-up; the gated worktree design itself is still deferred ([#310](https://github.com/judell/bram/issues/310)).

**Evidence** ([#504](https://github.com/judell/bram/issues/504)):

- The freeze did not hold: `issue-495-credit-undeclared-path-to-sole-declarer` spent three Codex and Antigravity review rounds inside the frozen machinery, each round's fix producing the next round's findings.
- Replaying every `.bram/worklist-history/` snapshot: 79 of 668 starts since 2026-08-22 (12%) were on a file an already-started item declared; 43 of 424 in the last 60 days (10%), 24 of them against one row left started for a week. The shared file was `src-tauri/src/lib.rs` in 49 of 79.
- The machinery fired in 6 of 157 gate commits between 2026-11-06 and 2026-11-20 (`op=interval-staged`), with 1 joint refusal: under 4%, the September rate.
- The removal: about 2,000 lines of Rust, 350 of pane JS and 200 lines of seeded conventions, against about 200 new lines for the refusal (#504, folds 2 and 4).

**Alternatives:**

- **Keep the freeze as guidance and finish #495.** Rejected: each fix round found new edge cases inside the fix (deleted folders, renames, re-anchors), for a path under 4% of commits take.
- **A worktree per started item.** Rejected for now: it isolates authoring but not integration, needs a cold Rust build per checkout, and its design is deferred in #310. It stays the escape hatch for overlap.
- **One active item at a time.** Rejected: the replay shows about 90% of starts don't overlap, so this gives up parallel work the simple rule allows.

**Revisit when:** real commits (not Bram's own test scenarios) show demand for two started items editing one file in one tree that worktrees don't serve; or a stale started row blocks starts often enough that the refusal, rather than clear-landed ([#497](https://github.com/judell/bram/issues/497)), is what users notice.
