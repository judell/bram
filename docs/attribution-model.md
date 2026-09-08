# The attribution model: replay-positional vs. HEAD-diff membership

Bram attributes uncommitted changes to worklist items so that commits land
each item's work under its own id — the misattribution-prevention charter of
issue #327. The mechanism that shipped is **replay-positional**: capture a
tree snapshot at every claim boundary, diff adjacent boundaries into per-item
edit scripts, and replay those scripts forward into per-line ownership of the
current working tree. It works — interval staging is field-proven hunk-exact
for claim-era work — and it is accumulating epicycles: ghost rules, joint-run
refusals, already-applied skips, offset tripwires, and a residue computation
that produced three differently wrong numbers in one evening before being
withdrawn ([4158cc1](https://github.com/judell/bram/commit/4158cc17b9687ca044a43cdc3792abea1d79aa72)).

The candidate replacement, named at the drawing board during the 2026-09-07
burndown session, is **HEAD-diff membership**: the attribution universe is
only the current `git diff HEAD`, and ownership is decided by membership of
changed regions in items' evidence rather than by positional replay over
interval history. This document works both models through the recorded
failures and the known hard cases, so the Ptolemy-vs-Copernicus question —
keep patching the current model, or re-ground it — is decidable.

The architectural substrate (boundary trees, `refs/bram/claims/*`, temporary
indexes, selective commits) is mapped in
[`git-as-infrastructure.md`](git-as-infrastructure.md); trace ops cited below
are cataloged in [`trace-vocabulary.md`](trace-vocabulary.md) under the
`claim-interval` and `worklist-commit` kinds. Line numbers are as of
`7dcd058` and will drift; function names are the durable pointers.

## 1. Inventory: who consumes attribution, and which ownership definition each uses

Three definitions of "whose change is this" coexist today. **D** = declared
files (`item.files`, no attribution at all — coverage of a path is read as
authorship of its diff). **R** = replay-positional interval attribution
(boundary-chain replay into per-line owner runs). **H** = HEAD-diff shapes
(judgments made directly against the current `git diff HEAD`).

| surface | where | definition |
|---|---|---|
| Boundary capture | `capture_claim_tree` (`src-tauri/src/lib.rs:39712`), `record_claim_interval` (`lib.rs:39757`) | substrate for R. Temp-index tree snapshots at claim transitions, refs at `refs/bram/claims/<atMs>`, ordered records in `resources/.claim-intervals.json`. Since [4158cc1](https://github.com/judell/bram/commit/4158cc17b9687ca044a43cdc3792abea1d79aa72), clear/shrink of the inflight claim also cut boundaries (kinds `cleared`/`shrunk`, outgoing id-set; a full clear records empty ids — the honest "nobody owns what follows" marker) |
| Replay engine | `parse_attr_diff` (`lib.rs:39907`), `apply_attr_hunks` (`lib.rs:39981`), `attribution_runs_for_path` (`lib.rs:40039`), `attr_runs` (`lib.rs:40059`), driven by `claim_attribution_runs` (`lib.rs:40311`) | R. Positional by design, never content-keyed — a content lookup misattributes duplicate lines (`lib.rs:39896`). Unowned lines are first-class (`lib.rs:40056`) |
| Joint-interval disambiguation | `JOINT_RUN_SEP` / `IntervalPathOwner` / `resolve_interval_path_owner` (`lib.rs:40101`–`40213`) | R + D hybrid. One-click plural approvals write ONE claim; per path, declared by exactly one member id → that id; several → a JOINT run; none → unowned ([e4c408f](https://github.com/judell/bram/commit/e4c408fe8534133a9a357e6fec36d21a37a904d0)) |
| Ghost rule | `GhostOwnerIndex` (`lib.rs:40257`) | R. An interval whose owner left the board strips to unowned in every consumer; traced `op=ghost-owner-reassigned` |
| Board attribution payload (`attribution`, `attributionTotals`, Ownership tab, diff run-marking) | `lib.rs:54242`–`54291` | R |
| `willCommit` headline numbers | `lib.rs:54292`–`54381` | D decides the mode (declared-overlap entanglement, `owners_by_path` from runs), R supplies the entangled paths' interval take, whole-file `git diff HEAD` stat supplies the exclusive paths — three definitions in one number |
| `jointWith` pre-warning | `joint_owners_from_runs` (`lib.rs:40622`), `item_joint_with` (`lib.rs:40649`), served at `lib.rs:54383` | R |
| Pane exclusivity / Commit offer | `__bramSelectionAllCommittable` (`app/__shell/helpers.js:2693`), `__bramItemChangedSplit` (`helpers.js:2517`) | D + H. `changedFiles` is the current diff scoped to *declared* paths; `sharedWith` is declared-path intersection among begun items. No hunk attribution anywhere in the pane (`helpers.js:2510`: "Authorship is not recoverable") |
| Changed-of-planned counts and strip/tooltip | `helpers.js:2502`, `helpers.js:2588` ("Files: N of M planned") | D. Every item declaring a path reports that path's whole uncommitted diff |
| Entanglement warning convention | `app/__shell/conventions.md`, *Warn when a new item would entangle a committable item* | D. "intersect the candidate item's `files` list with the union of `files` across begun items" |
| Commit-gate entanglement scan | `handle_worklist_commit`, `lib.rs:56614`–`56748` | R. Interval attribution of staged paths decides interval-vs-whole-file staging (`op=entangled-interval-stage`); deliberately attribution-based, not declared-files-based (`lib.rs:56619`) |
| Joint refusal (#356) | `lib.rs:56668`–`56722`, `op=refuse-joint-interval` | R. A joint run with a begun outside-request member → 409, claim released; the message names the only honest outs |
| Scoped per-item patches | `claim_interval_diff` (`lib.rs:40681`) | R. Concatenated tree-to-tree diffs of the intervals a scope solely owns; ghost intervals deliberately NOT reassigned into patches (`lib.rs:40713`: stale trees can never apply) |
| Interval staging | `interval_stage_commit` (`lib.rs:40795`) | R patch, H gate. Stages the item's interval patch into a HEAD-seeded scratch index; `git apply --check` (no `--3way`) is the order-independence gate (`lib.rs:40878`); chunks already in HEAD are skipped, `op=interval-already-applied` (`lib.rs:40842`, added for #364's resume case); real index refreshed for committed paths (#357, `lib.rs:40930`) |
| Independence panel | `claim_interval_independence` (`lib.rs:41220`) | H over R patches: does each item's patch apply to HEAD alone |
| Residual disclosure (#364) | gate pass at `lib.rs:56995`–`57059`; `diff_residual_lines` (`lib.rs:40986`), `residual_owner_label` (`lib.rs:41061`) | H classified by R. Post-interval-stage `git diff HEAD` residue on declared paths; any residual line not accounted to a begun outside-request run classifies `unowned` and withholds the prune (`residualPaths` / `retained` in the response, `op=residual-disclosed`, `op=prune-withheld`) |
| Reserved residue plumbing | `unowned_by_path` tuple slot (`lib.rs:40424`) | reserved and deliberately never populated — the withdrawn counting feature's slot, kept so a redesign can refill it without re-plumbing every consumer |

The load-bearing observation: **the warning layer and the staging layer
disagree about what "entangled" means.** The pane's exclusivity, the strip's
counts, and the conventions' warning all key on declared files (D); the
commit gate's staging decision keys on interval attribution (R). Work that
neither definition covers — edited in unclaimed time, attributed to nothing,
declared by an item the staging layer never consults — falls through the gap
between them. That gap is where
[bcb2a7e](https://github.com/judell/bram/commit/bcb2a7e3d4942afdd5cef862a71475cc2ba9b52e)
happened (§2).

## 2. Failure catalog, with receipts

Each failure is mapped to the model property that failed, because the choice
between patching and replacing turns on whether the properties are fixable
in-model.

### 2.1 Exclusive credit without work — #273

The filed shape (2026-08-23): `notice-banner-component`, started for hours,
having written nothing, read `✓ Will commit +16 −13 in 1 of 7 planned` — the
+16 −13 was a `skip-worklist:` edit to a declared path no other started item
claimed. Exclusivity proved no *other item* made the edits; nothing proved
*this item* did. The 2026-08-28 specimen (#273 comment, the #294/#295 run):
a changed-of-planned count keyed on declared paths read "Changes in
progress" on `issue-294-home-scaffold-guard-capture` with zero of its work
on disk — the neighbour's hunks in a shared declared `lib.rs` satisfied its
1-of-1. #327's body carries the deliberate reproduction: a fixture item
declaring two files and having written nothing reported
`files: 2 of 2 planned · lines: +104, −57` — exactly another item's work.

**Property failed:** definition D has no concept of authorship at all.
Declared-path coverage of the current diff is read as credit. This is not a
bug in D's implementation; it is what D is.

### 2.2 The open-interval credit leak — root of #273, fixed in-model

The reproduction run recorded in
[4158cc1](https://github.com/judell/bram/commit/4158cc17b9687ca044a43cdc3792abea1d79aa72)'s
message found the mechanism behind R's half of the filed shape: **clears
never cut an attribution boundary**, so the last owner's interval stayed
open forever, and boundary-less foreign edits (a `skip-worklist:` turn, a
hand edit) landed inside it and were credited to that owner in every R
consumer. The surviving fix: clear/shrink now record boundaries (kinds
`cleared`/`shrunk`; a full clear records empty ids). An in-model fix, and a
real one — but note its shape: the model's correctness depends on the
boundary chain *tiling the timeline*, and every gap in the tiling is a
credit leak until someone finds it.

### 2.3 Whole-file absorption — #336, #356

#336 (Bram 0.6.3, budget project): `worklist-commit` for one of two
entangled begun items staged the shared file wholesale and committed the
neighbour's hunks under the requested id — the entanglement scan never
engaged. (The receipt commit `a739396` lives in the budget project's local
repo and cannot be verified from this checkout; the issue thread is the
record.) #356: a same-click plural approval writes one claim and one
capture boundary, so both items' edits to a shared declared path land in a
JOINT interval that per-item staging has nothing to stage from; before
[107cc3d](https://github.com/judell/bram/commit/107cc3d292948b13642a9a9dd61667d583b6ee77)
the scan read only `itemId` runs and skipped joint runs silently — silent
absorption again, by a different route. The fix is the honest 409
(`op=refuse-joint-interval`), with a remedy message that had to be corrected
once more when field testing showed "separate the hunks by hand and retry"
was unsatisfiable — attribution keys on the *recorded* interval, not the
instantaneous diff, so no amount of hand-editing the worktree produces a
per-item interval to stage from (`lib.rs:56697`, refused three times on
2026-09-07 regardless).

**Property failed:** in R, the safety of whole-file staging depends on the
scan enumerating every attribution state correctly (single, joint, ghost,
unowned), and each missed state is silent absorption. The joint case is
additionally *unfixable in-model*: one boundary for N items destroys
per-item evidence at capture time, and no later computation can recover it.
The refusal is the correct in-model answer — but note that the
unsatisfiable-remedy correction was an epicycle on the epicycle.

### 2.4 Half an item committed, the orphan absorbed — #364, the session receipts

The 2026-09-07 burndown session, in sequence:

1. `bold-chip-static-theme-values` was approved, applied under a claim,
   advanced (claim retired). A later redo — expanding scope into
   `app/__shell/helpers.js` — was edited in **unclaimed time**: no boundary
   captured it. At the commit gate the path was entangled with
   `closes-decision-belongs-to-the-commit-gate`, so interval staging engaged
   and did its job *correctly at the line level*:
   [10fdd12](https://github.com/judell/bram/commit/10fdd128d47efd4dc46afcef97a473985e8b2777)
   contains exactly the claim-era `Main.xmlui` work (1 file, +29 −11) and
   nothing of the unclaimed `helpers.js` work. Then the route reported plain
   success and pruned the item — orphaning the other half with its owner
   erased.
2. The attempted recovery — a fresh item, `rehome-session-line-bolding`,
   begun and *declaring* `app/__shell/helpers.js` — provided no protection,
   because the staging layer's entanglement test is R, not D: orphaned lines
   are attributed to nothing, so the neighbour's commit saw no entanglement
   and whole-file staged the path.
   [bcb2a7e](https://github.com/judell/bram/commit/bcb2a7e3d4942afdd5cef862a71475cc2ba9b52e)
   (`helpers.js` +79 in its stat) absorbed the orphaned
   `__bramFooterSessionLineParts` refactor under closes-decision's id and
   message. Permanent in `git log`; #364's correction comment is the record.
3. The recovery that *did* work, per #364's correction comment: physically
   parking the orphaned hunks out of the worktree before the neighbour's
   commit and restoring them after. **Attribution follows current content** —
   the human executed, by hand, exactly the computation the membership model
   proposes (§4).

The disclosure fix landed in `7dcd058`: the residual pass (`lib.rs:56995`)
diffs declared paths against the fresh HEAD, classifies residue, and
withholds the prune on unowned residue.

**Properties failed:** R's universe excludes unclaimed-time work, so
"staged everything attributable" and "staged everything" are different
claims that the response conflated; and the D/R definition split (§1) meant
the one arrangement the conventions offer for protecting work — declare it
on a begun item — was invisible to the layer that needed to see it.

### 2.5 The three wrong residue counts — the withdrawal receipt

The same evening (recorded in
[4158cc1](https://github.com/judell/bram/commit/4158cc17b9687ca044a43cdc3792abea1d79aa72)'s
message and the `trace-vocabulary.md` `claim-interval` row), a
residue-display feature — show the user how many changed lines belong to
nobody — shipped and was withdrawn after three renders produced three
different wrong numbers:

- **74,978** — approximately the whole unchanged body of `lib.rs`: the
  computation subtracted run spans from the full replay length, counting
  base lines (unowned-by-birth) as residue.
- **329** — counted committed files, via replay-base drift: the replay's
  base is the *first boundary tree*, not HEAD, so content that had since
  been committed still read as attributed-but-unaccounted.
- **427** — the entire working diff, once the count was clamped to HEAD:
  the clamp fixed the universe but the replay could not say which of the
  diff's lines were owned.

What survived: the `unowned_by_path` tuple slot (`lib.rs:40424`), reserved
and empty, and #364's narrow consumer — `residual_owner_label` makes an
**existence** claim ("some residual line is unowned"), a much weaker claim
than a count, and even it needed a same-run correction (the first cut
classified hunk-header context lines as unowned; `lib.rs:40974`).

**Property failed:** R has no conservation law. "Unowned" in R is not a
measurable bucket of a bounded universe; it is *everything the replay fails
to explain* — an error term. Error terms absorb every bug in the pipeline
(base drift, boundary gaps, offset shifts), which is why three attempts
produced three different numbers and none was checkable against anything.

### 2.6 The standing epicycle inventory

Individually justified patches whose accumulation is the strain signal:

- **Ghost rules** (`lib.rs:40243`): two cleverer resolutions each lied in
  production on 2026-09-04 — reassigning ghost intervals to a covering live
  item broke staging ("patch does not apply" ×3, stale trees), and
  display-only reassignment inflated a survivor's totals with
  already-committed content ("Will commit +802" against a true ~+290).
- **`op=interval-already-applied`** (`lib.rs:40842`): a retained item's
  recorded intervals include already-committed chunks, which would fail the
  independence gate with a misleading message — so staging reverse-applies
  each chunk against HEAD to detect and skip them. A patch compensating for
  the universe including committed history.
- **`op=run-line-mismatch`** (`attr_replay_line_offset`, `lib.rs:40022`): a
  tripwire for the replay's reconstructed file length diverging from the
  real file — the #324 offset class, where every run shifts by a constant
  and the Ownership view maps lines to the wrong run or none.
- **OID-keyed caches** (`lib.rs:40344`): claim ref *names* can be reused
  when `atMs` collides, so a name-keyed cache served a stale tree's diff —
  a 30-line offset in the field.
- **Deleted-file header parsing** (`lib.rs:39912`): `+++ /dev/null` left
  the path unset and a deletion's hunks bled onto the alphabetically
  preceding file (phantom −29, 2026-09-05).
- **`willCommit` mode selection** (`lib.rs:54292`): whole-file where
  exclusive, interval take where entangled, because either alone
  undercounted or overcounted ("Will commit +185 −1" while one exclusive
  file alone carried +32 −15).

Every one of these is downstream of the same structural choice: computing
present-tense facts by integrating history, then correcting the integral's
drift term by term.

## 3. The status-quo model, stated precisely

**Universe:** the ordered chain of boundary trees `refs/bram/claims/*` (per
`resources/.claim-intervals.json` record order), closed intervals as fixed
tree-to-tree diffs, plus the open interval — last boundary against the
working tree (a plain `git diff <ref>`, which cannot represent untracked
files exactly; a known diagnostic boundary, per `git-as-infrastructure.md`).

**Owner assignment:** each interval's owner is its claim's id-set, resolved
per path by `resolve_interval_path_owner` (single / joint / unowned), with
ghosts stripped to unowned. Per-line ownership is the positional replay of
every interval's edit script (`apply_attr_hunks`), oldest first, over an
ownership vector seeded from the base tree's line count.

**Hard cases in R:**

- *Duplicate lines:* handled correctly — attribution is positional, never
  content-keyed (`lib.rs:39896`), so identical text in two places cannot be
  confused. This is R's genuine strength and a constraint on any successor.
- *Deletions:* recorded in interval patches and counted in totals; but the
  per-line runs annotate *surviving* lines, so deleted content has no run,
  and the #364 residue classifier must special-case pure-deletion hunks to
  unowned (`lib.rs:41066`).
- *Unclaimed-time work:* invisible to the universe. Before 4158cc1 it was
  credited to the last open interval's owner (§2.2); after, it is unowned —
  but "unowned" in R cannot be *counted* (§2.5), only existence-tested, and
  unowned content is still silently absorbable by whole-file staging (§2.4).
- *One-click joint approvals:* per-item evidence is destroyed at capture;
  declaration disambiguates 71% of joint declared-file slots per the
  2026-09-05 mining (comment at `lib.rs:40088`; figure not independently
  re-verified here); the remainder is a first-class joint state and an
  honest refusal at the gate.
- *Supersession:* an interval records what an item did *then*; later work
  may overwrite it, so per-item interval diffs do not sum to the working
  diff (measured 2026-09-01, `trace-vocabulary.md`: A's interval contains
  `beta EDITED-BY-A`, which exists nowhere on disk).

**What patching R further would cost:** each remaining failure class needs
its own mechanism. The unowned-count problem needs a replay whose error
term is provably zero — which is the tiling assumption again, now load-
bearing for arithmetic. The D/R entanglement split needs either the pane
surfaces rebuilt on R (importing R's drift problems into every count the
user sees) or the gate taught D (which #336's own comment rejects for
breaking the sanctioned serialize-entangled flow, `lib.rs:56619`). The
absorbed-orphan class needs whole-file staging to consult unowned
attribution — a count R cannot currently produce trustworthily.

## 4. The candidate model: HEAD-diff membership

**Universe:** the current `git diff HEAD` for tracked content, plus
untracked non-ignored files — exactly what a commit could take. Committed
content is not in the universe; neither is any intermediate state.

**Owner assignment:** the boundary store survives as the **evidence base**.
For each *live, begun* item, derive its candidate patch from its claim
intervals (as `claim_interval_diff` does today). A changed region of the
current diff belongs to item X iff X's evidence *accounts for it in current
content* — operationally, X's candidate patch reverse-applies from the
current state (`git apply --reverse --check` against a scratch capture of
the worktree, the same temp-index machinery as `capture_claim_tree`), and
the region lies in the reverse-application's footprint. Joint evidence
(same-click boundaries) yields joint membership, disambiguated per path by
declaration exactly as `resolve_interval_path_owner` does now. Ghost
intervals are not candidates (only live begun items are consulted — the
ghost rule's answer, without the strip machinery). Everything in the
universe that no candidate's evidence accounts for is **unowned — by
subtraction, not by error term**.

The conservation law this buys: *members + joint + unowned = `git diff
HEAD`, per path, checkable on every render.* Residue stops being the
replay's unexplained remainder and becomes a measured bucket. The three
wrong counts of §2.5 are each structurally excluded: base lines are not in
the universe (kills 74,978), committed content is not in the universe
(kills 329), and owned membership is subtracted before anything is called
unowned (kills 427).

The field proof that the semantics are right is §2.4's working recovery:
parking a neighbour's hunks and restoring them after succeeds where no
interval arrangement does, because staging truth *already* follows current
content. And three shipped mechanisms are membership computations in
embryo: the order-independence gate (`git apply --check` against HEAD —
"does X's requested state exist in a HEAD-rooted universe?"), the
independence panel, and #364's residual pass (classifying `git diff HEAD`
residue — currently against replay runs, the model's two halves bolted
together).

**Hard cases in H:**

- *Duplicate lines:* the danger the replay's comment warns about
  (`lib.rs:39896`) constrains the mechanism: membership must never be
  decided by bare content lookup. `git apply` matches hunks by context, not
  by content-keyed search, which disambiguates most duplicates; when two
  placements of an identical hunk are both viable (identical context —
  e.g., the same one-line change in two identical blocks), reverse-apply
  picks one. For *staging* this is sound — the committed content is
  identical either way — but for *display* the line-run placement can be
  wrong by position. The honest handling, per the never-guess rule: when a
  candidate hunk admits multiple placements (detectable by probing the
  reverse-apply at both), mark the region's membership **ambiguous** — a
  fourth first-class state alongside single/joint/unowned, rendered as
  such, staged only with both placements' items together. This is H's
  analog of the joint refusal: honest, rare, and shaped like #356's.
- *Deletions:* first-class, and more natural than in R. A deletion is a
  region of `git diff HEAD` like any other; membership annotates *edit
  operations*, not surviving worktree lines, so `residual_owner_label`'s
  pure-deletion-hunk special case (`lib.rs:41066`) dissolves. An item whose
  evidence deletes lines owns that deletion.
- *Unclaimed-time work:* in the universe (it is in the diff), accounted to
  no candidate's evidence, therefore unowned — visible, countable, and
  consultable by *every* surface including whole-file staging. The bcb2a7e
  class closes structurally: the gate can see that a whole-file stage would
  absorb unowned regions and disclose or refuse, and a re-home item's
  declaration can be honored as a membership tie-breaker for unowned
  regions (declaration + being the sole declarer is a fact, the same
  standard e4c408f applies to joint slots) — or not, but either policy is
  now *expressible*, which in R it is not.
- *One-click joint approvals:* unchanged in substance. One boundary for N
  items still destroys per-item evidence; the joint candidate patch yields
  joint membership; the #356 refusal survives verbatim. H does not fix
  this and must not pretend to.
- *Supersession and drift:* an item whose evidence no longer
  reverse-applies (later work rewrote its lines, in-place, in unclaimed
  time or under another claim) partially or wholly loses membership. This
  is the model's honest degradation and must be handled as such:
  partially-applying evidence contributes the chunks that still apply;
  chunks that no longer apply mean the surviving content is *not* the
  item's recorded work, and crediting it anyway would be the ghost-
  reassignment lie of 2026-09-04 in new clothes. The consequence — an
  item's number can *shrink* when a neighbour rewrites its lines — is
  true, and R's number (which keeps counting the overwritten interval) is
  the one that lies (the +802-vs-~290 receipt).
- *What boundaries and intervals become:* **evidence, retained.** Capture
  keeps running exactly as today — boundary capture at claim transitions
  and clears is what makes unclaimed time *distinguishable* at all, and
  interval patches are the only per-item evidence in existence. What
  changes is authority: the intervals stop being replayed forward as the
  truth about the present and start being tested against the present. The
  no-evidence refusal (today's `no-interval` 409, `lib.rs:56772`) survives
  with a sharper meaning: no evidence, or evidence that no longer applies.

**What H costs:**

- Per-render `git apply` probes per begun item per contested path, against
  a scratch worktree capture (untracked files force the temp-index form —
  the same reason `git stash create` was rejected for boundaries). Begun
  items are few and the probes are local plumbing, but the cost must be
  *measured, not asserted* (#323 discipline; the replay's spawn-counting
  precedent at `lib.rs:40331`).
- Line-run rendering (the Ownership tab's colored runs) needs the
  reverse-apply footprint mapped to current line numbers — the same
  arithmetic `diff_residual_lines` already does for `+` lines, generalized.
- The historical question "what did this item do at the time?" is no
  longer the model's primary output. It stays answerable — the interval
  store is retained — as a diagnostic view, clearly labeled as history
  rather than present-tense fact.

## 5. What must survive any migration

- **Boundary capture, unchanged.** The temp-index, pathspec-scoped,
  untracked-inclusive tree capture (`capture_claim_tree`) and the
  boundary-on-clear kinds from 4158cc1 are field-proven and are the
  evidence base both models need. The adjacency-aware retirement rule
  (`claim_intervals_partition`, `lib.rs:39835`) and the ref-before-sidecar
  durability rule survive with it.
- **Hunk-exactness for claim-era work.** Interval staging's committed
  patches matched isolated per-item diffs to the line in the #336
  follow-up field verification, and 10fdd12's *committed half* was exactly
  right. Any successor staging path must preserve this, and the scratch-
  index noninterference guarantee (worktree and real index never touched;
  post-commit real-index refresh, #357).
- **The never-guess principle.** Unowned stays unowned; ghost and joint
  sets are never promoted to survivors; ambiguous membership is declared,
  not resolved by coin flip. Both 2026-09-04 "clever" resolutions lied;
  the principle is load-bearing, not stylistic.
- **The honest refusals.** #356's joint refusal (one click, one boundary,
  no per-item evidence) and the no-evidence 409, with remedies that are
  actually satisfiable (`lib.rs:56697`'s lesson).
- **Positional-not-content discipline.** The successor must not regress
  into content-keyed matching (`lib.rs:39896`); context-anchored apply is
  the floor, with ambiguity surfaced.
- **Observe-only first.** Per `developing-bram.md` § Log-first
  development: the membership engine ships as trace lines beside the
  replay before any surface flips, with falsifiable graduation criteria.

## 6. Recommendation

**Adopt HEAD-diff membership as the authoritative model for every
present-tense consumer — counts, exclusivity, entanglement, staging
decisions, residue — retaining boundary capture and the interval store
unchanged as its evidence base.** This is the Copernican move in the exact
sense the metaphor demands: R computes the present by integrating history
and then corrects the integral's drift epicycle by epicycle (§2.6); H
measures the present directly and uses history only to label it. The three
failure classes that are *unfixable* in R — no conservation law for
unowned content (§2.5), the D/R entanglement split (§2.4), and credit
leaking through any gap in the boundary tiling (§2.2) — are structural
non-problems in H, while everything R does well (hunk-exact staging, joint
honesty, duplicate-line discipline) transfers because the evidence and the
staging machinery are retained. The one genuinely new obligation —
declared ambiguity for identical-context duplicate hunks — is the same
shape as the joint refusal Bram already ships.

### Acceptance criteria (falsifiable)

1. **Conservation tripwire.** Per path, per board build: member + joint +
   ambiguous + unowned line counts sum exactly to the path's `git diff
   HEAD` counts. Divergence emits a trace op (successor to
   `op=run-line-mismatch`); the criterion is zero fires over a two-week
   dogfood soak *with* a provenance check (a deliberate-violation unit
   test), per the soak-vs-tripwire rule.
2. **The #273 fixture reads zero.** #327's `dummy-a` reproduction (item
   declares two files, writes nothing, neighbour writes +104 −57) reports
   own-work 0/0, unowned or neighbour-owned as appropriate — in the strip,
   the tooltip, and `willCommit`.
3. **The bcb2a7e shape cannot recur silently.** Fixture: orphan unowned
   lines on a path, begin a neighbour with claim-era work on the same
   path, commit the neighbour. Required outcome: the whole-file stage is
   refused or the absorption is disclosed in the response and trace —
   asserted by test, not by convention.
4. **The three wrong counts are regression tests.** Fixtures reproducing
   each §2.5 shape (large unchanged file; post-commit board; unowned-only
   diff) each render the true number.
5. **Staging exactness preserved.** The #336 follow-up's two-item
   entangled fixture still produces per-item commits matching isolated
   diffs to the line; 10fdd12's shape now returns `residualPaths` +
   `retained` (already true since `7dcd058` — must not regress).
6. **Refusals preserved.** The #356 same-click joint fixture still 409s
   with `op=refuse-joint-interval`; pre-capture-only work still hits the
   no-evidence 409.
7. **Cost within budget.** Counted spawns (the `lib.rs:40331` pattern) on
   a realistic board; the budget set from the current replay's measured
   baseline before the flip, not after.

### Migration sketch, per consuming surface

Sized so implementation items can be cut directly; ordering is the
observe → display → gate → retire sequence.

1. **Membership engine** (new, beside `claim_attribution_runs`): per-path
   partition of the current diff into single / joint / ambiguous / unowned
   regions, built from per-item interval evidence via reverse-apply
   probes; emits the conservation tripwire and an `op=membership-diverges`
   comparison line against the replay's runs. Observe-only; no consumer
   flips. (Criteria 1, 7.)
2. **Board payload flip**: `attribution`, `attributionTotals`,
   `totals_by_path`, `willCommit` (`lib.rs:54242`–`54381`) and `jointWith`
   (`lib.rs:54383`) source from membership; the reserved `unowned_by_path`
   slot (`lib.rs:40424`) is finally populated; `willCommit`'s three-way
   mode selection collapses to "the item's membership take". (Criteria 2, 4.)
3. **Pane flip**: `__bramItemChangedSplit` / `__bramSelectionAllCommittable`
   (`helpers.js:2517`, `2693`) consume per-item membership numbers instead
   of declared-path `changedFiles` arithmetic; "exclusive" stops being a
   proxy for "mine". Strip and tooltip copy re-verified against renders
   (the rendered-artifact rule). (Criterion 2.)
4. **Commit-gate flip** (`lib.rs:56614`): entanglement = outside-begun
   membership in staged paths; whole-file staging consults unowned
   membership and discloses/refuses absorption; the joint refusal keys on
   joint membership; the residual pass (`lib.rs:56995`) becomes the same
   computation's unowned bucket rather than a bolt-on, retiring
   `diff_residual_lines` / `residual_owner_label`. (Criteria 3, 5, 6.)
5. **Staging flip** (`interval_stage_commit`): stage the item's membership
   patch (equal to today's interval patch when nothing drifted); keep
   `apply --check`, no `--3way`, and the real-index refresh; retire
   `op=interval-already-applied` (committed content is out of the
   universe by construction). (Criterion 5.)
6. **Retirement pass**: `GhostOwnerIndex` shrinks to the live-candidates
   rule; `attr_replay_line_offset` / `op=run-line-mismatch` retire in
   favor of the conservation tripwire; the replay engine itself is
   retained only behind the historical "what did this item do" diagnostic
   view, or removed if that view is deemed not worth its weight.
7. **Docs, same change**: `trace-vocabulary.md` rows for the new ops in
   the introducing commit; `git-as-infrastructure.md`'s claim-attribution
   section re-grounded; `conventions.md`'s two entanglement definitions
   unified into one.

### Unverified claims

Marked here rather than silently asserted: the #336 receipt commit
`a739396` exists only in the budget project's local repository; the "71% of
joint declared-file slots have exactly one declaring id" figure is quoted
from the mining note at `lib.rs:40088` and was not re-derived for this
document; and the residue-display feature's three wrong renders are cited
from 4158cc1's commit message and the trace-vocabulary row — the withdrawn
code itself is recoverable from that commit's history but was not re-read.
