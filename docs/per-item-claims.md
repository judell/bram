# Per-item claims: evidence from one gate click

Companion to [`attribution-model.md`](attribution-model.md). That document
decided *how attribution should be computed* (HEAD-diff membership over
retained interval evidence). This one designs the remaining structural gap
that no computation can close: **a one-click plural approval writes one claim
and one capture boundary, so same-click items' edits to a shared path land in
a joint interval with no per-item evidence to compute from** (#356, and the
"one-click joint approvals" hard case the model doc explicitly declines to
fix: "H does not fix this and must not pretend to").

This is #269's surviving ask — two items truly in flight, each with its own
evidence — after every subset of it that could ship shipped: incremental
claim/auth retirement by resolved ids
([a195225](https://github.com/judell/bram/commit/a195225e1c48efe8f37c732a60c054dd28dd754a)),
durable `begunAtMs`, interval staging, the joint refusal
([107cc3d](https://github.com/judell/bram/commit/107cc3d292948b13642a9a9dd61667d583b6ee77)).
Implementation is sequenced *after* the membership flip — the store surgery
below should meet one surviving reader, and the chain-order-dependent replay
is the reader it breaks (§2) — but sequencing gates the build, not the
drawing board. The cost of designing late is starting cold the day the #273
ledger clears; the cost of the missing capability is paid nightly as
ceremony: the Start-choice radio (`45e1f07`), the futile-joint refusal and
its park/drop/re-propose dance (`34973c4`), the withheld Commit on a
joint-encumbered selection (`d3bcd61`). Every one of those surfaces carries
its own sunset note pointing here.

## 1. What one gate click writes today, and the two candidate shapes

Today, approving N items in one click writes:

- **one claim** — `resources/.inflight-claim.json`, a single record with one
  `ids` array and one `kind`; a later Start *displaces* it
  (`[inflight-sentinel] op=write` with `prior_ids`);
- **one capture boundary** — `record_claim_interval` snapshots one tree into
  `refs/bram/claims/<atMs>` and appends `{atMs, ids, kind, ref, tree}` to the
  ordered `resources/.claim-intervals.json`;
- **one authorization record** — single-slot; any later gate click replaces
  it (#269's corrected inventory: claim and auth are both displaceable, only
  `begunAtMs` + disk are durable).

The interval that follows is owned by the whole id-set. When exactly one
member declares a changed path, declaration disambiguates
(`resolve_interval_path_owner`); when several do, the interval is JOINT and
per-item staging has nothing to stage from — the #356 shape, refused honestly
at the gate.

Two candidate record shapes for the successor:

- **One claim with per-item sub-boundaries.** Keep the single claim record;
  add interior boundaries as the agent moves between items. Rejected: it
  preserves the single-slot displacement problem (a second Start still
  overwrites), and it leaves the claim meaning two different things at once —
  "what was approved together" (an authorization fact) and "what is being
  worked now" (an execution fact) — the conflation #269's second comment
  identified as the root defect.
- **[chosen] N claims, keyed by item id.** `.inflight-claim.json` becomes a
  map `{claims: {<id>: {kind, issuedAtMs, ...}}}`. A gate click *adds* N
  entries; nothing is displaced. Each claim retires alone (the existing
  clear-shrink machinery generalizes from "shrink one array" to "remove one
  key"). The authorization store splits the same way: per-item records, each
  consumed by the `mutate`/`worklist-commit` that resolves its id — the
  "per-item authorization records" direction from the #269 thread, now a
  requirement rather than an option, because a keyed claim resumed tomorrow
  needs an authorization that yesterday's unrelated click did not silently
  destroy.

**What "boundary" means when claims interleave.** A boundary stays what it is
today — a global instant, one tree snapshot, refs and record shape unchanged
(`{atMs, ids, kind, ref, tree}`). What changes is the meaning of `ids`: it
names the owner of the **work window** the boundary opens — the item the
agent is working on *now* — not the set of everything currently claimed. The
chain still tiles the timeline (the §2.2 lesson from the model doc: every
gap is a credit leak); windows are just narrower than claims. This is the
load-bearing continuity: `capture_claim_tree`, the ref store, the
adjacency-aware retirement rule, and `claim_interval_diff`'s "concatenate the
intervals this id solely owns" all survive **byte-compatible** — old records
read exactly as before, new records simply carry sharper `ids`.

## 2. Where window boundaries come from, and the store's concurrency model

The click can't cut per-item boundaries because at click time no work exists
to separate — the joint interval's evidence is destroyed *by the capture
schedule*, not by the record shape. Per-item evidence requires a boundary at
each **work transition**: the instant the agent stops editing under item A
and starts editing under item B.

The host already observes exactly that instant. Every file-mutating tool call
passes the PreToolUse guard, and the guard's allow decision already computes
*which begun item's declared files cover the target path* — that computation
is the guard's job today. The design adds one breadcrumb: when the covering
item differs from the current window's owner, the guard notifies the host
(the same in-process plumbing that posts menu events and direct-edit
breadcrumbs today) and the host cuts a boundary — capture, ref, record with
`ids=[<covering id>]`, kind `switched` — *before* the write lands. Cost is
one capture per **transition**, not per write: an agent making thirty edits
to item A's files then moving to item B cuts one boundary, which is the same
economy that justified boundaries-over-writes in the first place
(`trace-vocabulary.md`, the claim-interval row). That rationale was derived
under single-claimant ("the timeline partitions into intervals each owned by
one claim"); this design re-derives it for keyed claims rather than
inheriting it silently.

Lifecycle calls keep cutting boundaries as they do since
[4158cc1](https://github.com/judell/bram/commit/4158cc17b9687ca044a43cdc3792abea1d79aa72):
`advance`, `prune`, `worklist-commit`, `/__worklist/end`, and turn-end
detectors close the open window (kinds `cleared`/`shrunk`/`turn-end`), so
unclaimed time stays distinguishable.

**What replaces chain order as the attribution substrate: nothing needs to.**
The chain remains ordered — boundaries are global instants under a
serializing mutex, exactly today's write discipline. What changes is what
consumers *do* with the order:

- The **replay** integrates the chain oldest-first into per-line ownership.
  Under interleaved windows it still functions mechanically, but its failure
  modes sharpen: a missed `switched` boundary doesn't just blur credit, it
  assigns one item's edits to another *by position*, and the drift
  corrections of model-doc §2.6 all get harder. The replay is the reader the
  surgery breaks.
- **Membership** treats each item's windows as *evidence to test against the
  present*: candidate patch = the concatenated tree-to-tree diffs of the
  item's windows (unchanged `claim_interval_diff`), probed by reverse-apply.
  Probes are order-independent — evidence either accounts for current content
  or it doesn't — so interleaved capture, a late-cut boundary, or two items'
  windows alternating on one file degrade *honestly* (a mis-windowed hunk
  fails its owner's probe and lands unowned or joint) rather than silently
  shifting credit. This is why implementation sequences after the flip: the
  probe semantics tolerate what positional replay cannot.

## 3. Hard cases, worked

**Two items' truly simultaneous edits to one file.** The parallel-dispatch
case single-slot claims cannot represent at all. Under keyed claims with
guard-observed transitions, simultaneity serializes at tool-call granularity:
captures run under the host mutex, so even two subagents' interleaved writes
produce an alternating window chain, each window solely owned. The residual
genuinely-simultaneous case — one write to a path that *two* active items'
declarations cover — produces a window whose `ids` is the covering set: a
joint window, shrunk from "the whole click-to-click span" (today) to "one
write-run on one contested path". The conventions' rule (non-empty `files`
intersection → don't parallelize) stays, but its violation now costs a small
honest joint region instead of wholesale evidence destruction. Worktree
dispatch composes: worktree paths already strip to real-tree coverage
(#309), and each worktree's captures snapshot the tree the write actually
landed in; evidence probes run against wherever the commit will be cut.

**A shared file, interleaved states.** The measured supersession fact
(`trace-vocabulary.md`, 2026-09-01: A's interval contains
`beta EDITED-BY-A`, which exists nowhere on disk) is unchanged — windows are
sequential states, and an item's window-diffs answer "what did this item do
then", not "what survives". Membership already handles the divergence: B's
rewrite of A's line makes A's evidence fail its probe there, and A's number
shrinks — the honest degradation §4 of the model doc defends, now applied at
window granularity. The fixture for the identical-content edge (two items
whose aligned edits leave duplicate placements) exists as the
`ambiguous-duplicate` demo starter.

**Claim displacement, and its retirement.** Today the second Start click
overwrites the live claim (`op=write prior_ids=...`), which is how an item
ends up permanently green-lit with nothing authorizing it (the
`notice-banner-component` receipt on #269: green-lit eleven hours, live
record covering three unrelated pruned items). Under keyed claims and keyed
auth records, a second Start *adds*. The displacement tripwire retires; its
successor instrument is the opposite assertion — `op=claim-add` tracing that
an existing key was **not** disturbed, plus a collision tripwire for a
second add of the *same* id (which should be impossible: the pane offers
Start only on unclaimed items).

**TTL, interrupt, cancel — per claim.** Each keyed claim carries its own
`issuedAtMs` and interrupt flag, so:

- TTL expiry is evaluated per item; one stale claim expiring cannot strand a
  fresh sibling (today a shared record ages as a unit).
- Esc/cancel fail-closed paths mark the *window's owner* — the item actually
  being worked — not every approved id.
- Turn-end detectors close the open window and clear claims by the same
  rules as today's full-coverage clears; `clear-partial` (the
  can't-name-ids refusal) survives for the blunt paths, but its trigger
  population shrinks to genuine collisions.
- Row-locking splits along the #269 comment's authorized-vs-executing line:
  the gate serializes on the *executing* window (spinner on the item being
  worked), while held-but-idle claims lock nothing. The "no button to commit
  with" deadlock class (#348, the issue-262/348 third- and fourth-outcome
  rules, the multi-id generalization in conventions §Serializing) dissolves
  structurally: ending a claim to unlock a decision stops being a required
  agent duty because idle claims never lock the decision in the first place.

## 4. What the ceremonies become

Each interim surface carries an evidence-keyed sunset, written into its own
introducing commit; per-item claims are the condition that empties them.

- **The Start-choice radio** (`d3bcd61`, `45e1f07`: *Start one at a time —
  separate commits* vs *Start N now — one combined commit*) exists only
  because starting together forecloses per-item commits. When one click
  writes per-item evidence, the foreclosure is gone and the radio collapses
  to a pure commit-granularity preference — or retires outright, since the
  commit gate can offer either granularity after the fact. Its own commit
  message says so: "when #269's per-item claims make one click write
  per-item evidence, the conditions empty and both legs retire themselves."
- **The joint refusal** (#356, `op=refuse-joint-interval`) keys on the
  *record*, not the click, so it sunsets by evidence vintage: post-#269
  records produce joint windows only for the shrunken contested-write case,
  where the refusal remains correct and rare (the same status the model doc
  gives ambiguous membership). Legacy joint records keep refusing exactly as
  today.
- **The park/drop/re-propose dance** (`34973c4`) retires for post-#269
  records — per-item staging just works — and remains the documented out for
  legacy joints until none remain on any board.
- **The withheld Commit** (selection missing a still-begun jointWith
  partner, `d3bcd61`) empties the same way: `jointWith` computed over
  post-#269 evidence names only genuine contested-write partners.
- **"A commit each" lights up as a consequence**: a same-click pair with
  disjoint hunks in a shared file becomes two offerable per-item commits with
  no ceremony — the exact thing refused three times on 2026-09-07 and danced
  around on 2026-09-08.

## 5. Acceptance criteria (falsifiable)

1. **The canonical fixture is the 2026-09-08 live reproduction**: one click,
   two items, one shared declared file, disjoint hunks (the xmlui same-click
   pair where each item drove its own PR, recorded on #356 and in
   `34973c4`'s message). Required outcome: two per-item commits, each
   hunk-exact against its item's isolated diff, with no radio choice, no
   409, no dance. Ship it as a demo starter (`same-click-pair`) beside
   `disjoint-entanglement`, whose separate-boundary version it deliberately
   mirrors.
2. **No displacement.** Start item B while A's claim is live: A's claim,
   auth record, and evidence are byte-identical before and after; both items
   then complete in either order. The #269 `notice-banner` shape (green-lit
   with nothing authorizing it) becomes unreachable — asserted by test, not
   by soak.
3. **Conservation survives interleaving.** The membership tripwire
   (criterion 1 of the model doc) stays zero across an interleaved two-item
   editing session on a shared file — the window chain must tile exactly.
4. **Refusals preserved where honest.** A legacy joint record still 409s;
   a contested simultaneous write to a doubly-declared path yields a joint
   window and either a joint-together commit or a refusal naming both ids.
   Pre-capture work still hits the no-evidence 409.
5. **Per-claim lifecycle isolation.** Expire/interrupt/cancel one item's
   claim; sibling claims, windows, and evidence unaffected. Turn-end closes
   the open window without clearing idle claims.
6. **The deadlock class retires.** With one item executing, every other
   row's Commit/Drop/Start affordances remain live — the #348 "user takes
   over the commit" sequence needs no `/__worklist/end` from the agent.
7. **Cost within budget.** Captures counted per transition (the
   `op=membership spawns=` discipline); budget set from the current
   boundary-capture baseline before the flip, not after.

## 6. Migration sketch, sized for item-cutting

Ordered observe → display → gate → retire, and gated behind the membership
flip (attribution-model.md §6's migration steps 2–5) having landed:

1. **Keyed authorization store.** Per-item auth records, consumed per id;
   the single-slot file becomes a map with a legacy-read shim. Independently
   shippable and independently valuable (it alone closes the displacement
   half of #269). Trace: `op=auth-add` / retirement per id.
2. **Keyed claim store.** `.inflight-claim.json` map; clear/shrink become
   key removal; spinner and row-locking rework onto the executing-window
   distinction (§3). The displacement tripwire retires, replaced per §3.
3. **Window boundaries, observe-only.** Guard→host covering-item breadcrumb;
   `switched` boundary kind; captures cut on transition beside today's
   click-time behavior, traced (`op=window-switch`) but not yet consumed —
   the log-first discipline, with graduation criteria on the soak: every
   switch corroborated by a write to the named item's files, zero switches
   during single-item sessions.
4. **Evidence flip.** `claim_interval_diff` consumes window-owned records
   (no code change if §1's byte-compatibility holds — the item's "solely
   owned intervals" simply become its windows); membership candidates
   sharpen automatically. The #356 refusal re-keys on record vintage.
5. **Gate and ceremony retirement.** Per-item staging on same-click pairs;
   radio, dance messaging, and withheld-Commit conditions empty per their
   recorded sunsets (§4); conventions and `trace-vocabulary.md` rows update
   in the same commits.

Design only; no code moves with this document. #269 closes when the
capability ships, not when this file lands.
