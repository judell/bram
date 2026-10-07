# No wider automatic switching to the Transcript

Status: decided 2026-10-07 · revisit after the Transcript's Worklist split pane has a week of use

**Question:** Only item actions (gate buttons, feedback) switch the agent pane to the Transcript, through `__bramGateGoTranscript` in `app/__shell/helpers.js`. A plain chat send stays on whatever tab it came from. Should Bram switch to the Transcript more aggressively, for example on every chat send, or from the tabs where users usually go there next?

Jon, 2026-09-24: "There are a lot of other situations where chat results in activity that I want to see in the transcript and I'm not in the transcript… Maybe we can just be more aggressive about auto switching." `trace-send-followup-for-transcript-autoswitch` (`030a23b`) added observe-only `send-followup` instrumentation to decide this, and said a week of use should settle it.

**Decision:** No. Plain chat sends keep their tab. The footer's "▼ N new" chip is the way back to the Transcript.

**Evidence:** `python3 scripts/send-switch-report.py .`, run 2026-10-07.

- **Where sends come from.** Reconstruction from `tools-route-save` / `to-turn`, 2026-07-20 → 10-07: 4,065 plain chat sends, 2,339 (58%) of them from the Transcript. `send-followup` trace, 2026-09-24 → 10-07: 277 pane sends, **242 (87%) from the Transcript**.
- **What follows a send from elsewhere.** Of the 35 off-Transcript sends, 20 were followed by a trip to the Transcript: **15 via the chip**, 5 by navigation, **0 automatic**. One bounced straight back.
- **Turns ending while the user is away.** 870 turn ends; 118 while the user was off the Transcript, 85 of those with unseen messages; 24 with no pane send in the previous ten minutes (typed in the terminal, or started by the agent). The chip already flags these.
- The trend runs the same way. Since the Transcript's Worklist split pane (`14fe5bb`, `9bbdaaa`, 2026-10-07), Worklist actions can be taken without leaving the Transcript, which removes the main reason to be elsewhere.

**Alternatives:**

- **Switch on every chat send.** Rejected: 87% of sends already come from the Transcript. For the rest, a forced switch would also hit the cases where the result belongs on the current tab, which the history can't tell apart from "didn't want to look".
- **Switch from the high-rate tabs only.** The reconstruction shows Queue 60% and Search 56% followed by a trip within 60s, against Commits 13% and Settings 13%. Rejected: those trips are one click on the chip, the volume is small (Queue 110 and Search 69 sends over eleven weeks), and a per-tab policy is behavior users would have to learn.

**Revisit when:** the split pane has a week of use. If Worklist actions are mostly taken from the split pane, already on the Transcript, the remaining post-action switch (`__bramGateGoTranscript`) may be retirable too. Rerun the report and compare off-Transcript gate actions.

**Known gap:** the report shows kind `?` for every `send-followup` row; it doesn't read the `kind` field. So it can't yet split chat from feedback from gate actions. Fix it before the revisit if that split matters.
