# Finish crediting each item's own lines; freeze same-tree hunk separation

Status: decided 2026-09-24 · one exception taken 2026-10-12 (`f225f1a`) · revisit on field demand

**Question:** #273 bundled two problems. Is the git accounting needed for both worth it?

1. **Crediting an item with what it actually wrote**, when it is the only thing working on its files. This affects every user.
2. **Several begun items sharing one working tree, with changes separated hunk by hunk.** This covers interval staging, joint claims and their refusals (#356), the independence check, and `split-shared-files`.

**Decision:**

- **Problem 1: finish it.** Move the pane and the gate to membership, and retire the replay. The Start snapshots are problem 1's evidence (they are how Bram tells an item's own edits from other edits in the same file), so they stay.
- **Problem 2: freeze it.** Make no new investment in same-tree hunk separation. The existing machinery stays as a dormant fallback. On entangled paths the guidance is to **commit the items together**, or to use **worktrees** for real parallelism.

**Evidence:** Every gate commit in the trace archives ([#273 comment](https://github.com/judell/bram/issues/273#issuecomment-5819390857)):

| Project | Gate commits | Hunk-separated | Entanglement detected | Refusals |
|---|---|---|---|---|
| bram | 247 | 8 | 14 | 6 |
| tau-extractor-sandbox | 43 | 4 | 6 | 1 |
| xmlui | 16 | 0 | 0 | 0 |
| budget | 15 | 0 | 0 | 0 |

12 of 321 commits (under 4%) needed hunk separation, all in Bram's own repo or a test sandbox, and many of them deliberate scenarios. Problem 2 had cost roughly 1,500–1,800 lines of the most intricate Rust in the app and 60+ commits. Several of those commits fixed the machinery itself: #356 (silent absorption), #364 (half an item committed), #367 (double count).

**Alternatives:**

- **Finish both.** Rejected: steps 4b and 5 of #425 (joint refusal and staging from the membership patch) serve the under-4% case, at the highest complexity.
- **Remove problem 2's machinery now.** Rejected: it is dormant at runtime and switches on only when a commit meets entangled paths. Its carrying cost is maintenance, not field exposure.
- **Drop the snapshots too.** Rejected after a correction: without them, attribution collapses to "the whole diff of the declared files belongs to the item", which is #273's bug.

**Exceptions taken:**

- **2026-10-06, `8c0d7d5` (#425 step 4a):** the gate decides entanglement by membership. This was in scope: it serves problem 1, because the replay was routing ordinary commits into problem 2's machinery (#419).
- **2026-10-12, `f225f1a` (#432):** the same-click joint refusal reads membership instead of the replay. This was a narrow exception to the 4b freeze, taken because a stale replay record was blocking unrelated commits, and because it lets the replay be deleted.

**Revisit when:** field demand for parallel work in one tree that worktrees don't serve, backed by real commits and not by Bram's own test scenarios.
