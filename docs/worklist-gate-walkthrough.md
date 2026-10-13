# Worklist gate walkthrough: hand-testing the gate on demand

A recipe for exercising the Worklist gate by hand: which buttons light,
what the gate says when it holds something back, what a commit takes, and
the lifecycle paths that used to leave the board stuck.

**This is an on-demand tool, not a release gate.** Since 2026-10-13 the
gate's bugs are found by field reports first, and this walkthrough is run
when a report (or a change to the gate surfaces) calls for it. The
reasoning is in
[`decisions/2026-10-13-gate-walkthrough-on-demand.md`](decisions/2026-10-13-gate-walkthrough-on-demand.md).
Run the phases a report points at, not necessarily all of them.

**Written from the code, not yet run.** This version was rewritten on
2026-10-13 from the code at `468f8ea`, after about half of the 0.5.3
version (2026-08-24) had come to test behaviour the gate no longer has.
Its expectations have not been confirmed by a run yet. On a first run,
treat a mismatch as a question (is the doc wrong, or the code?) rather
than an automatic finding, and fix the doc either way.

The gate surfaces this covers: `WorklistGateBar.xmlui`, `Worklist.xmlui`,
the selection and committability predicates in `app/__shell/helpers.js`,
and the lifecycle routes in `src-tauri/src/lib.rs`
(`/__worklist/commit`, `/__worklist/mutate`, `/__worklist/drop`, the
`/__worklist` serve's precheck fields).

## What "pass" means

Three claims, all of which must hold:

1. **The buttons are right.** At every step, exactly the actions that are
   legal are offered, and none that aren't.
2. **When the gate holds something back, it says why, and what to do.**
   A missing button reads as guidance, never as a regression.
3. **You never get stuck.** No spinner outlives its turn; row selection is
   never left locked; no step needs a `curl` to escape.

Claim 3 matters most. Row selection is disabled while a claim is live, so
a stuck claim removes the ability to resolve the item that is stuck. A
wrong button is a papercut; this is a wedge.

## Setup: a demo instance

Run it on a demo instance (`docs/developing-bram.md`, "The
synthetic-testbed method"), never on the working repo: the probe commits
then land in the disposable demo repo, and teardown is a reset.

```sh
scripts/demo-instance.sh new gate-walk \
  --from dependency \
  --from joint-owner-committed \
  --from entangled-with-unowned \
  --from new-file-left-out \
  --from committed-outside
scripts/demo-instance.sh launch
```

What each starter is for:

| starter | exercises |
|---|---|
| `dependency` | a commit that only applies on top of another item's uncommitted lines (#430) |
| `joint-owner-committed` | a same-click pair whose owner has since committed, and a later item that must still commit (#432) |
| `entangled-with-unowned` | two owners of one file plus a hunk nobody owns: hunk-by-hunk commits, and `unownedTaken` |
| `new-file-left-out` | hunk-by-hunk staging with a never-tracked file (#419) |
| `committed-outside` | a row whose work landed through a plain `git commit` (#406) |

For the button basics (Phases 1, 2, 8), ask the demo's agent for two or
three plain items that each append one line to their own scratch file.

**Before you start:**

- Note the demo's commit: `git -C ../bram-demo-instance log --oneline -1`.
- Note where the demo's trace ends:
  `wc -l < ../bram-demo-instance/resources/bram-traces/bram-trace.log`.
- For Phase 3, set `worklist.gateExplainer: true` in the demo's
  `.bram.json` (the explainer line is off by default).
- Console noise to ignore: the ResizeObserver loop (see
  `developing-bram.md`, "Known noise: the ResizeObserver loop"). Anything
  else in the console is a finding.

## The standing check

**After every action below (every Start, Commit, Drop, item feedback),
confirm before doing anything else:**

- the spinner is gone;
- row checkboxes are clickable again;
- the selection is consistent: no row left ticked while the footer offers
  nothing, and no footer count disagreeing with the ticks.

If any of these fails, stop and record it: it's a claim-3 failure and it
outranks whatever you were testing. Recovery, in order:
`POST /__worklist/end` with `{"ids":[...]}`; `mutate` by hand if the work
really is on disk; restart the demo.

---

## Phase 1: one item, nothing begun

**Do:** tick one plain item.

**Expect:**
- **Start** and **Start & commit** enabled; hovering names the item.
- **Commit** not offered.
- **Drop** enabled.
- No explainer line.
- Expanding the row shows "No changes yet" and no Diff.

**Then, the composer:** the line above the message box reads "To: <item>
(feedback)" with **Send as chat instead**. Click it: the line reads "To:
Claude (chat)" with the item still ticked. Change the selection and it
switches back to feedback. Clear the selection: "To: Claude (chat)", with
no control.

## Phase 2: start one; Start goes away once it can commit

**Do:** Start the item and let the agent append its line and advance.

**Expect:**
- The spinner clears when the agent finishes *(standing check)*.
- The row's strip reads "Will commit +1 −0" (or similar), and the Diff
  shows the appended line.
- Ticking it: **Commit** enabled, **Start** not lit. Start lit beside
  Commit on a committable row is a regression seen before.

## Phase 3: the explainer (`worklist.gateExplainer: true`)

The line above the buttons comes from the same predicates that light them
(`__bramStartConsequence`), so it should never describe an action that
isn't on screen.

- **Started + not started**, ticked together:
  *"<a> already has changes and <b> has not started, so this selection has
  no joint Start or Commit. Select the started item alone to commit, or the
  proposed one to start it."* The clause "(their edits would then mix)" is
  added when they share a file.
- **Two not started that share a file**:
  *"These share <file>. Started together, they'll be committed together. To
  commit them separately, start and commit one before starting the
  other."*

Then set the key back to `false` and confirm the line goes away while
every button behaves the same.

## Phase 4: entanglement by owned lines (`entangled-with-unowned`)

"Entangled" means another begun item **owns lines** in a file the commit
takes, not merely declares the file (#425).

**Do:** tick one owner of the shared file alone.

**Expect:**
- **Commit** is offered. Its strip's "Will commit" counts only that item's
  own hunk.
- Commit it: the commit takes only that item's hunk (hunk-by-hunk
  staging), and the other owner's lines stay in the worktree.

**Then:** reset the scenario, tick **both** owners, and Commit.

**Expect:** with both owners ticked there is no outside owner, so the gate
takes the whole file, the unowned hunk included. The gate says what
Commit takes for the selection, and the commit response carries
`unownedTaken` naming the unowned lines (trace
`op=commit-took-unowned`). Taking unowned lines silently is a finding.

## Phase 5: a commit that depends on another (`dependency`, #430)

**Do:** tick the dependent item alone.

**Expect:** **Commit** withheld, with the reason on the row: *"It commits
only together with <other>; tick both and commit once."* Tick both and
Commit: one commit with both items' work.

If the host refuses a dependent commit anyway, the row reads *"Last
Commit was refused: …"* until HEAD moves, and the trace has
`op=refuse-interval-dependent`. Seeing that after the pane offered Commit
means the precheck and the gate disagree: a finding.

## Phase 6: same-click partners (`joint-owner-committed`, #432)

**Expect:**
- The later, separately approved item on the shared file **commits**. A
  refusal citing the committed owner or a partner with no changes is
  #432's bug back.
- For a live same-click pair (approve two items in one click, let both
  edit one file), ticking one alone withholds Commit with *"Committable
  together with <partner>"*, and ticking both commits them together. The
  host's refusal for the forced case traces `op=refuse-joint-interval`.

## Phase 7: other commit shapes

- **`new-file-left-out` (#419):** commit the first item alone. Its new
  file must be in the commit. A success that leaves a declared new file
  out is #419.
- **`committed-outside` (#406):** the landed row reads "Committed outside
  the Worklist in <sha>…" and clears in one click. The reverted row must
  *not* read as landed.
- **CRLF (#434), optional:** in the demo repo, commit a file with CRLF
  endings while `core.autocrlf=false`, set `core.autocrlf=true`, edit the
  file through an item, and Commit. It must commit; a
  `refuse-state-drift` here is #434 back.
- **Drop of an applied item (#433):** Drop an `applied` row with an empty
  message box. The newest `resources/worklist-history/*.md` must say
  "1 dropped" with no Commit line (trace
  `op=changelog-drop-of-applied`).

## Phase 8: the paths that used to get stuck

Each is a deliberate attempt to wedge the UI.

- **8a, approved with nothing to do.** Ask the agent for an item whose
  stated work is already true on disk, then Start it. **Expect:** the
  agent reports there was nothing to apply and recommends Drop; its turn
  ends and the spinner clears without you doing anything. This is the
  shape that wedged the UI three times in 0.5.2 development.
- **8b, item feedback.** Tick any item, type feedback, press **Enter**.
  **Expect:** the spinner clears when the agent's turn ends.
- **8c, a staggered multi-item Start.** Tick two disjoint, not-begun
  items and Start both in one click. **Expect:** one claim covering both;
  as each finishes, only its own id retires (`op=clear-shrink`); the
  spinner clears only when both are done.

---

## Teardown

1. Drop any remaining probe items. For each, the agent must say what
   remains on disk before the drop completes. Silence there is itself a
   finding.
2. `scripts/demo-instance.sh reset` (or `stop`). Nothing touches Bram's
   own repo.
3. **Trace checks**, on the demo's
   `resources/bram-traces/bram-trace.log` from the line you noted:
   - `grep '\[inflight-sentinel\] op=clear-partial'` → **zero** (claim
     collision);
   - `grep '\[inflight-sentinel\] op=write' | grep prior_ids` → **zero**
     (a live claim displaced; a same-ids, same-kind rewrite on the drop
     path is benign, so read the line before scoring it);
   - `grep '\[auth-record\] op=consume-already-consumed'` → **zero**
     (double consume);
   - `grep '\[worklist-advance\] op=pending'` → every one has a matching
     `op=cleared` by teardown;
   - refusals (`op=refuse-joint-interval`, `op=refuse-interval-dependent`,
     `op=refuse-state-drift`, `op=refuse-empty-commit`) → only the ones a
     phase deliberately provoked;
   - `grep -E 'console-error|console-unhandledrejection' | grep -v
     ResizeObserver` → **zero**.

---

## History

### Run record: 2026-08-24 (the 0.5.3 gate), historical

This was the release gate for 0.5.3, run in the working repo with
hand-made probe items. Several of the phases it scored no longer exist:
the "One commit / A commit each" choice was removed
(`gate-remove-commit-share-radio`), and "Commit withheld on an entangled
item" ended when entangled items gained hunk-by-hunk commits (#327). Kept
for its findings:

| phase | pass | notes |
|---|---|---|
| buttons, single unbegun | ✓ | incl. no Diff before begun |
| Start off when committable | ✓ | |
| explainer + plural | ✓* | finding #1: the explainer narrated off-screen actions; rebuilt from the button predicates (`2d4530d`) |
| Commit withheld, entangled | ✓ | (behaviour since replaced, #327) |
| commit-each choice, disjoint | ✓* | finding #2: the split label was keyed board-wide; rekeyed to the selection (choice since removed) |
| commit-each choice, entangled | ✓ | (choice since removed) |
| choice honoured | ✓ | (choice since removed) |
| nothing-to-apply unwind | ✓ | no wedge |
| feedback clears | ✓ | |
| staggered multi-claim | ✓ | `clear-shrink` traced |
| config off | ✓* | finding #3: a config flag consulted by some predicates and not others; resolved by retiring the setting (`1cd9315`) |
| teardown trace checks | ✓ | |
| never stuck, throughout | ✓ | |

✓* = passed after an in-run fix. The lesson from finding #3 still holds:
a config flag consulted by some gate predicates and not others is a
dead-end row waiting to happen.
