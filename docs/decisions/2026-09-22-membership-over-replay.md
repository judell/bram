# Line ownership comes from HEAD-diff membership, not the positional replay

Status: decided 2026-09-22 · the gate moved over in `8c0d7d5` (2026-10-06); the replay retires under `issue-425-retire-replay`

**Question:** Bram had two models of which Worklist item owns which changed line. The **replay** (R) replays every claim interval's edit script forward over the file. **Membership** (H) takes only the current `git diff HEAD` and credits each changed region to the item whose claim evidence accounts for it. Where they disagree, which is right? The answer decides which model the pane and the commit gate trust.

**Decision:** Membership. The pane switched in `7d19f27`, the gate's entangled-or-not routing in `8c0d7d5`, and the joint refusal in `f225f1a`. The replay is being deleted.

**Evidence:** Every `op=membership-diverges` line in the bram repo's traces, 23 in all, ruled one by one on 2026-09-22 ([#273 comment](https://github.com/judell/bram/issues/273#issuecomment-5782458288)). **23 for membership, 0 for the replay.** The evidence was of two strengths:

- **15 against independent evidence.**
  - 7 under-credits: the replay credited nobody on a new, untracked file. Each time, `git log` names the adding commit as the item membership credited.
  - 8 over-credits: the replay credited docs-only design items (for example `adaptive-work-management-design`) with lines in `helpers.js`, `WorklistGateBar.xmlui` and `lib.rs`, files they never declared or wrote. This is the open-interval credit leak of `docs/attribution-model.md` §2.2.
- **8 on the model's stated principle only:** supersession, where a later edit rewrote an item's lines. These were not independently verified.

A 2026-10-03 to 06 soak at the gate then saw 31 commits, 8 divergent path lines all leaning to membership, and 0 conservation breaches ([#425](https://github.com/judell/bram/issues/425)). The replay's routing had sent 18 of 50 ordinary commits down hunk-by-hunk staging, causing 3 refusals and 7 partial landings (#419).

**Alternatives:**

- **Keep the replay and patch it.** Rejected: `docs/attribution-model.md` §2.6 lists the epicycles (ghost rules, `op=interval-already-applied`, the line-offset tripwire, OID-keyed caches, `willCommit` mode selection). Each one corrects drift from computing present-tense facts by integrating history.
- **Run both, compare forever.** Rejected: the comparison is evidence for a switch, not a design. Two models of one fact are what produced pane/gate disagreements like #336 and #430.

**Revisit when:** a conservation breach is explained by membership's model rather than an implementation bug. #398, the one unexplained breach, is still open.

**Note:** the H that was adjudicated decided ownership by reverse-applying each item's whole patch. That rule was replaced on 2026-10-02 by the per-line rule (`93830b1`), because a chat-turn edit to an item's own lines broke the whole-patch test in 17 of 19 cases (`docs/attribution-model.md` §2.8). The ruling stands: both rules diff against HEAD, and they agree on the cases above.
