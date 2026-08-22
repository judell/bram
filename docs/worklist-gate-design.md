# The Worklist gate

The design of the surface where work is approved. This is the **maintained**
copy: the vocabulary originated in an external tutorial gist (v4), which the
markup still cites (`app/tools/components/Worklist.xmlui`), but a gist cannot be
diffed against the code it governs, cited by line, or corrected in the same
commit as a behaviour change. The gist stays as provenance. Corrections land
here.

Scope: the gate itself. The worklist *protocol* — propose → approve → apply →
commit, the payload shapes, the authorization flow — is in
`app/__shell/conventions.md` and is not restated here.

---

## What ships today

The redesigned gate became **the** Worklist on 2026-08-20 (`8336553`,
`98d655e`). The legacy tab survives behind **Settings → Show legacy Worklist**.

### Rows are evidence-first

A row leads with what is true on disk, not with an item's bookkeeping status.
`proposed` and `applied` are host bookkeeping and deliberately absent from the
user-facing vocabulary; the row instead says whether work has begun and what it
moved.

### The strip

`__bramWorklist2Strip` (`app/__shell/helpers.js`) resolves the line beside the
row title, in priority order:

1. **A claim verb**, when an action is in flight for this item — `Approving…`,
   `Committing…`, `Iterating…`, `Dropping…` (`__bramClaimVerb`). The verb reads
   the claim's `statusLabel`, the gate fixed at approval time, never the moving
   changed-count: an earlier version inferred it and flipped `Approving…` →
   `Committing…` mid-apply as the first edits landed.
2. **Activity counts**, once the item has begun and its `changeSummary` shows
   changes: `files: N of M planned · lines: +A, −R · edits: N · last: <time>`.
   The denominator is the item's own `files` list, so a count that reads
   `2 of 3` means the plan named a file the work did not need — correct the
   plan rather than caveat the count.
3. **`No changes yet`**, when the item has not begun.

"Begun" is `__bramWorklist2Begun`: status `applied`, **or** a live claim covers
the row, **or** the item carries an approved authorization. All three are host
facts. None is agent-state inference.

The third clause exists because the first two have shorter lifetimes than the
question. During an apply the item is still `proposed`, and host turn-completion
detectors clear the claim on ordinary end-turn signals — so a row demonstrably
mid-apply regressed to `No changes yet` while its measured changes sat unused in
the payload (`a1e92ba`).

**Plan versus activity is load-bearing, not cosmetic.** A proposed, unclaimed
item is a *plan*: sibling work on a shared path is not this item's activity, and
showing counts for it would let one item claim another's work. That is not
hypothetical — file overlap between items is common enough to have its own
column.

A row printing `No changes yet` while its own payload carries files with
non-zero counts is a contradiction, and one that has occurred; it is now
recorded as a tripwire (`worklist-strip op=anomaly`, `e5f745d`).

### The file table

Per row: **file**, **changes**, **shared with**. It renders for plan rows too —
only the activity numbers are begun-gated — because the path list and the
overlap are what a reader needs in order to decide what to do next.

`sharedWith` comes from the host and names the other items touching that path.
Since the commit gate stages whole files, an entangled item cannot be committed
alone without carrying its neighbour's changes. A banner at the foot of the list
says so and states the remedy (`__bramWorklistOverlapText`, `4d0c114`); it is
dismissed per *overlap signature*, so a later, different entanglement raises a
fresh one rather than being silenced by an earlier dismissal.

### The selection gate

Rows carry tickboxes; **one** action bar acts on the selection. The selection is
the sentence and the verb is the action, which is why a batch mode is not a
separate thing. A message typed once fans out to every selected item.

Which verbs appear is derived from the selection, not from a mode:
`__bramSelectionAllStatus` and `__bramSelectionAllPlans` gate **Approve N**,
**Approve & commit N** (subject to the `worklist.oneClickApproveCommit`
setting), **Commit N**, **Iterate N**, **Drop N**.

A row's tickbox stays ticked and frozen while its action is in flight, because
the tick is part of the sentence just submitted.

Row expansion is keyed by item id. It was positional, and pruning the list
transferred an expansion opened on row 3 to whatever landed at row 3.

### Serialization, and how it is shown

`__bramInflightBlocker` is **the single reason gate buttons ever disable**: an
unconsumed authorization is being carried out. All five gate buttons bind it, so
while a claim is live the entire action bar is inert and the claimed row shows
its verb.

The helper returns the first claimed id rather than a boolean, deliberately:

> when parallel agent work later brings multi-claim host state, the evolution
> happens here and call sites stay put.

Between the click and the host's sentinel write there is a gap, filled by a
bounded local echo in `__bramItemInflightKind` — host state wins whenever the
claim covers the item, and the echo expires after ~30 s so a completion callback
that never fires cannot leave an indicator running against a clean sentinel.

---

## Open case 1 — independent parallel begin

**Two items cannot be independently in flight.** Two structural reasons, both
single-slot:

- The inflight sentinel is one claim. `write_inflight_claim_sentinel`
  (`src-tauri/src/lib.rs`) writes one `.inflight-claim.json` with one `ids`
  array and one `kind`; a second write overwrites the first.
- The authorization record carries one whole-record consumed flag. The first
  `mutate` consumes it; a second consumer is told `no_active_authorization` for
  work that was in fact approved.

Both are instrumented rather than merely believed: `inflight-collision` traces a
write that displaces a live claim, a clear covering only part of one, and a
double consume.

Note the distinction the UI can blur: **N items begun in one action** already
works — batch selection covers N rows under one claim. What is unrepresentable
is two items begun *independently*, at different times, resolving at different
times.

**This case is currently visible, not silent.** While a claim is live every gate
button is disabled and the claimed row names its verb, so a user who tries to
begin a second item meets a disabled control rather than a mystery. The design
question is therefore not "warn about it" but whether the serialization should
be lifted at all.

Before lifting it, answer the turn boundary. Both surfaces are single-slot
partly because a PTY agent session is serial: a claim is approximately "the turn
that is running". Two independent claims mean two approvals resolving inside one
serial turn stream, and the honest options are that the agent interleaves them
within a turn, that the second queues behind the first, or that they genuinely
parallelise across delegated subagents. That answer decides whether plural
claims are an affordance or a concurrency model — and it should precede any
change to the file formats.

Whatever lands, `inflight-collision` must be re-aimed or retired deliberately:
if plural claims become legal, a displacing write stops being an anomaly, and an
instrument left asserting a retired rule is worse than none.

Worth checking first whether the constraint has ever actually bound: the
collision traces are the evidence, and a limit that has never cost anything in
practice may not be worth the concurrency work.

## Open case 2 — the agent is busy on unlisted work

**The gate cannot distinguish "idle" from "busy on something that is not a
listed item."**

Observed 2026-08-22: three items on screen, none in flight, every strip
correctly reading `No changes yet`, every gate button enabled — while the agent
was working on a conversational turn. The only indication anywhere in the
product was the footer verb, `Beaming… (3m 9s)`.

This is the inverse of case 1's visibility. A claim-backed action is well
reported; work with no claim is reported nowhere in this surface, because every
indicator the gate owns is keyed to a claim.

The vocabulary already exists **one tab over**. `__bramQueueReadyLabel`
(`app/__shell/helpers.js`) answers exactly this question for the Queue, reading
the same host state the Worklist has access to:

```js
if (menu) return "menu pending — hold";
if (status && status.state === "working") return "agent working — hold";
return "ready to send";
```

Two gates in the same pane, reading the same state, and only one tells the user
what it is waiting for.

**Proposal.** Report agent-busy in the gate, following the Queue's three-way
label rather than inventing a second vocabulary for the same condition. Two
sub-questions to settle before implementing:

- **Placement** — beside the gate buttons, where it explains their behaviour, or
  on the heading, where the Queue puts it.
- **Wording** — whether to distinguish "busy on a listed item" (which the
  per-row claim verbs already cover) from "busy on something else" (which
  nothing covers). Only the second is missing, and a label that does not
  separate them would be redundant with the row verbs.

## Why the two compound

Each case is individually mild. Case 1 is a constraint that is currently shown.
Case 2 is a state that is currently hidden.

Together they describe a gate that is well behaved when it is claiming work and
silent when it is not — so the one condition it never reports is the one where
the user has no other explanation for what they are seeing. A future reader
should not fix one and consider the pair closed.

**Ordering.** Case 2 should be resolved first, and case 1 re-examined
afterwards. A declared busy state may be a sufficient account of why the gate is
waiting, in which case case 1's user-facing half needs no change at all and only
the concurrency question remains — which is a decision about the substrate, not
about this surface.
