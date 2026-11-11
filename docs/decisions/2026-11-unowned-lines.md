# Why commits take lines no item owned

**Status:** findings, 2026-11-11. Recommendations at the end; each becomes
its own Worklist item.

A whole-file commit through the Worklist gate takes any lines in the file
that no item owns, and says so (`unownedTaken`, the trace's
`[worklist-commit] op=commit-took-unowned`). That was decided on
2026-10-02 ("disclose, don't refuse", #425). Until 2026-11-10 the Log
showed every such commit in the warning color, though nobody could act on
it, which is what prompted this note. The question here is where the
lines come from, and what to do about each source.

## The census

Every `op=commit-took-unowned` line in this checkout's trace archives
(about 1,100 files, 2026-07-20 to 2026-11-11):

- **93 rows across 45 commits.** The first is 2026-10-08, when the line
  rule shipped. October has 38, November 55.
- **Paths:** `src-tauri/src/lib.rs` 19, `app/__shell/helpers.js` 9,
  `docs/trace-vocabulary.md` 8, then `docs/developing-bram.md`,
  `app/__shell/conventions.md`, `Main.xmlui`, `Transcript.xmlui`,
  `guard.rs` at 3–4 each.
- **Method:** the commit's sha and items come from `.bram/audit-ledger.jsonl`.
  The source of each row comes from the trace before the commit: claim
  boundaries (`[claim-interval] op=capture … tree=`), claim writes and
  clears (`[inflight-sentinel]`), the main thread's Edit and Write hook
  lines, subagent binding lines, and the host's own membership
  diagnostics. Where the boundary trees survive (about 370 of 1,258, so
  roughly the last week), consecutive trees were diffed to see which
  window wrote each line.

| cause | rows | confidence |
|---|---|---|
| The claim cleared while the agent kept working | 30 (28 in November) | high where trees survive |
| Chat edits with no claim | 26 | medium |
| Lines the host itself couldn't explain | 11 | high: the host's own line |
| Edits inside the item's own window that still read unowned | 6 | low; probably the same family as the row above |
| The claim cleared mid-turn by a clear request | 5 | medium |
| Unknown (no visible edit) | 13 | — |
| An orchestrator edit outside a subagent's claim | 1 | low |
| A tool write | 1 | low |

The calibration case, `08cd835b` (2026-11-10), was first described in
chat as "an orchestrator edit between subagent runs". The census
corrects that: every window from 19:54 to 20:19 was held by the item, and
the host's gate compute logged both paths as `membership-unexplained`
with exactly the counts the commit later took. It belongs to the third
row, not the first.

## The causes

### 1. The claim clears while the agent is still working (30 rows)

The largest source, and a real loss of ownership. But in the rows checked,
the turn really had ended: the edits came from **background subagents that
outlived the turn's claim**.

- **`b54f19d`** (window 2026-11-02 23:40–23:55Z). The main thread launched
  three background agents at 23:40:24, and its turn ended at 23:40:27. The
  `lib.rs` edits are subagent `ae85765e`'s (97 Edits). Each
  `rearm-at-turn-start` lands within a second of one subagent reporting back.
- **By date:** 2 rows in October (`d785e69` is ledger F, fixed by
  `ab3cae8`), 19 from the 2026-11-03 `issue-469` run, and 8 on 2026-11-04
  while the retention fix (`622252f1`, `4f0fde38`) was being built, before a
  relaunch put it in effect. One row comes after it.
- **That one, `2f2ec7c`** (2026-11-09 15:51Z), has its own route. The main
  thread resumed a background subagent with SendMessage. The host logged
  `[delegated] op=bind-skipped reason=held-claim-only` (`lib.rs`
  `bind_locked`), which misread the continuation as a chat turn's new
  subagent. The turn's own `turn_duration` record said
  `pendingBackgroundAgentCount: 1`, and the host doesn't read that field.
  The subagent then wrote `lib.rs` with Bash heredocs, not Edit.

**Remedy class: hand the claim on to the work that continues it** (#478
ask 2).

### 2. Chat edits with no claim (26 rows)

An edit made in a turn addressed to no item belongs to no item, even when
a begun item declares the file. This is the documented rule
(`worklist-mechanics.md` §Edits made in chat turns), with conventions
against it: route the change as feedback, and **Resend as feedback** when a
chat message was plainly about an item. The line rule (`93830b1`) already
stops such an edit from erasing the item's own credit.

**Remedy class: leave it, and keep disclosing it.** The rule is right. These
lines really were written for no item, and the commit message and Log
needn't treat them as alarming.

### 3. Lines the host couldn't explain (11 rows, plus probably 6)

Before each commit the host runs the line rule and logs
`op=membership-line-rule … unexplained=+N-M`, then
`op=membership-unexplained path=…` for any line no step accounts for. In 11
rows those lines match the later `commit-took-unowned` exactly. These aren't
edits outside a claim. They're the diff-alignment effect #436 described on
2026-10-13: the diff from HEAD to the final file pairs repeated identical
lines (a lone `}`) differently from the chain of step diffs, so a line the
steps never moved reads as removed and re-added. The commit's contents are
correct; only the accounting is wrong. The six "inside the item's own window"
rows probably share the cause; their trees are gone, so it can't be shown.

#436 proposed the fix: cancel a removed HEAD line and an added final line
with identical text when the step chain carries that HEAD line unchanged to
that position. No commit shows it landed.

**Remedy class: stop reporting it.**

### 4. The claim cleared mid-turn by a clear request (5 rows)

The claim was cleared by a `clear-request` during a working turn, and the
edits that followed had none. Fewer rows, same effect as cause 1.

**Remedy class: claim the edit.**

### 5. Unknown (13 rows), subagents, and tools

- **Subagent edits are logged, but not attributed.** `hook-events.log` has
  every subagent Edit and Write with its path (96 `lib.rs` lines against
  `ae85765e`'s 97 Edits), and no `agent_id`. What it can't show is a Bash
  write's target (`target=-`). A row with no visible edit is therefore
  most likely a Bash write, as in `2f2ec7c`, or a formatter.
- **Formatter and Setup writes leave no trace line.** One row fits a Setup
  run; `cargo fmt` writes can't be detected at all. The record has never
  analyzed them (the prior-art review found no issue or commit naming them).
- **Subagents unbound from a claim:** no clear rows. The 2026-11-04 decision
  (`622252f1`, `4f0fde38`, `ae2c2eb3`) holds the claim while a Claude subagent
  runs. The large 2026-11-04 rows (`lib.rs` +1,078, +982 −734) came while that
  feature was being built and have no surviving trees.
- **Skip-worklist direct edits:** no clear rows. Unowned by construction when
  they happen.

## Recommendations, in order

#478 asks for three things, and they line up with the causes:

1. **Hand the claim on to delegated work** (#478 ask 2; cause 1). Start with
   the one route seen since 2026-11-04: a SendMessage continuation skipped as
   `held-claim-only`. Trace `pendingBackgroundAgentCount` at each turn end,
   so the next case names itself.
2. **Offer Commit when unowned lines are the only blocker** (#478 ask 1).
   Take them and disclose them as `unownedTaken`, the way whole-file staging
   already does ("disclose, don't refuse").
3. **Don't retain a row whose own lines all landed** (#478 ask 3). Unowned
   residue in a shared file isn't the item's.
4. **Cancel the alignment phantom (cause 3).** #436's pairing rule. Its
   `78248b1` repro needs Tuck's `refs/bram/repro/436-unexplained/*`; the
   middle tree isn't on the Air.
5. **Attribute hook lines to an agent, and record Bash write targets**
   (cause 5), so the next census can split the unknown row.
6. **Leave chat edits as they are (cause 2).**

Not recommended: refusing commits that take unowned lines (the 2026-10-02
decision stands), and asking the user per commit (most of these lines are
cause 1 or cause 3, which a person can't judge).

**Revisit** after recommendations 1, 4 and 5 land: rerun the census over
the following two weeks. Cause 1 already runs at about one row a week since
2026-11-04, so measure against that, not against October–November's 30. Read
`pendingAgents=` on `op=clear-at-turn-end` at the same time (from
`delegated-continuation-keeps-claim`): a clear with agents still pending is
the next route.

**Census shas:** four were rebased at push after the audit ledger recorded
them: `93349fe` is `622252f1`, `58d711d` is `4f0fde38`, and `3b8bd4c` is
`977e4722`. `7b23e5f` is unmapped.
