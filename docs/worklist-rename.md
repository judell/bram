# Renaming a worklist item — design plan

Status: **design, pre-implementation.** This document is the deliverable of
the `issue-276-worklist-rename-design` worklist item. It decomposes
judell/bram#276 into an inventory, three candidate semantics, and a
recommendation. **No product code ships from this item** — implementation
is proposed from this plan afterward, each piece with its own gate, the
same design-first treatment `docs/needs-you-inbox.md` gave #338.

## The two defects, restated precisely

#276 names two things:

- **(A)** no supported way to rename a worklist item id, atomic across
  `resources/worklist.json` and its draft file.
- **(B)** the unsupported attempt is not refused — it is silently reverted,
  and the revert can leave the draft file moved with nothing on the board
  pointing at it.

`worklist_mutate_required_kind` (`src-tauri/src/lib.rs:58499-58504`) is the
complete op vocabulary of `/__worklist/mutate`:

```rust
fn worklist_mutate_required_kind(op: &str) -> Result<&'static str, String> {
    match op {
        "prune" => Ok("drop"),
        "advance" => Ok("approved"),
        _ => Err(format!("unknown op: {}", op)),
    }
}
```

No `rename`. The only `"rename"` match anywhere in `lib.rs` is session
renaming — unrelated, confirmed by grep, not touched by this design.

**Sharper finding on B than the issue itself states.** The issue calls the
revert path's `eprintln!` a "trace [an agent] has no reason to grep." It
undersells it: that line is not in `resources/bram-traces/bram-trace.log`
at all. `maybe_enforce_worklist_policy` (`lib.rs:53092-53161`) calls plain
`eprintln!` (`lib.rs:53149`), not `append_bram_trace_line`
(`lib.rs:2600`) — the function that actually writes to `bram-trace.log`
and is what every "grep the trace" convention in this repo means. Compare
`generate_worklist_changelog`'s neighboring comment
(`lib.rs:53174-53176`, "rename_from was retired... unauthorized prunes are
already blocked... before they can reach the watcher") — the block is real,
but its only observable evidence lands on Bram's own process stderr, a
stream the coding agent doing the rename cannot read at all: it is not the
PTY it runs in, not a file it can open, not a route it can curl. **An
agent that reverts silently and an agent that succeeds are, from the
agent's side, in the exact same epistemic position: a 200-shaped write
that returned nothing describing failure.** This is worse than "a trace
that requires knowing to look" — it is a trace with no agent-reachable
address at all. Log-first development says "does the trace already
capture what happened?" — here the honest answer is no, not "yes, but
subtle."

## 1. Where an id is load-bearing

This inventory *is* the design: every row below is something a rename
must rewrite, must correctly leave alone, or must refuse to run near.
Verified against current source, not the issue's citations (which the
issue itself already found stale once, at #303's five-thousand-line drift).

| # | id-keyed state | file:line | what a rename must do |
|---|---|---|---|
| 1 | `resources/worklist.json` item `.id` | the metadata array item itself | **rewrite** — this is the rename |
| 2 | `resources/worklist-drafts/<id>.md` | path built by `draft_markdown_path` (`lib.rs:40822-40828`), read by `resolve_worklist_item_draft` (`lib.rs:46025-46095`) via `worklist_draft_path` (`lib.rs:45991-45996`) | **move atomically with #1** — the half-move that produced #276's `_draftMissing` symptom |
| 3 | `resources/worklist-citations/<id>.json` | `worklist_citation_path` (`lib.rs:45998-46003`), merged into the resolved item by `resolve_worklist_item_draft` | **move if present** — dormant per `app/__shell/conventions.md` ("Do not author... that plumbing is dormant"), but the merge code runs unconditionally, so an orphaned citation file after a rename would silently stop showing up exactly the way an orphaned draft does |
| 4 | `resources/.worklist-authorization.json` `.ids[]` and `.items[].id` | `WorklistAuthorizationRecord` (`lib.rs:98-126`); read by `validate_worklist_mutate_authorization` (`lib.rs:58562-58590`) | **refuse the rename** if a live (unconsumed, unexpired) record covers the id — see precondition set below |
| 5 | `resources/.inflight-claim.json` `.ids[]` | written by `write_inflight_claim_sentinel` (`lib.rs:43970-…`); checked wherever a claim gate blocks concurrent work | **refuse the rename** if a live claim covers the id — same reasoning as #4: a claim mid-flight has a turn actively reasoning about the old name |
| 6 | `resources/feedback-drafts/<unix-ms>-<item-id>.md`, `resources/feedback-history/<unix-ms>-<item-id>.md` | filename suffix match, `feedback_draft_belongs_to_item` (`lib.rs:40833-40835`): `file_name == "<id>.md" \|\| file_name.ends_with("-<id>.md")` | **rewrite the id suffix** on every matching file in both directories, or the refine/iterate audit trail silently stops resolving for the item post-rename |
| 7 | `item.closesIssues` | plain field in the `worklist.json` item, no separate file | **travels for free** with #1 while the item is still `proposed`/`applied` — but see the decoupling below |
| 8 | `resources/.worklist-pending-closes.json` (queued closes) | `PendingIssueClose` (`lib.rs:155-169`): keyed by `{issue: u64, commit_sha, patch_id}` | **not id-keyed at all** — once a close is queued at the commit gate it is anchored to the issue number and commit SHA, never the item id. **This means closesIssues bookkeeping is only a rename concern pre-commit**; a queued or already-closed issue has already forgotten the item id entirely. Worth stating explicitly because the issue's own table implies `closesIssues` is a live rename risk throughout the item's life — it stops being one the moment a commit exists. |
| 9 | the `reminder-` prefix contract | `needs_you_reminder_row` (`lib.rs:51144-51174`): `id.starts_with("reminder-")` gates whether the Awaiting You inbox's mechanical lane sees the item at all | **the id IS the routing key** — renaming a reminder in or out of the prefix changes which surface finds it, which is exactly the case #276's own comment thread already worked around by hand (see §4) |
| 10 | `resources/worklist-history/<ts>.json` and `<ts>.md` | e.g. `resources/worklist-history/1789403155271.json`, item bodies embedded with their `id` field as authored at proposal time | **deliberately NOT rewritten** — history is what happened under the name it happened under; rewriting it would falsify a historical record for a cosmetic gain, and `generate_worklist_changelog` (`lib.rs:53177-…`) already treats id removal from `worklist.json` as `dropped`/`committed`, never `renamed` (that bucket was removed in `67eff19` and its removal comment, `lib.rs:53170-53176`, is still accurate) |
| 11 | commit messages and chat transcript | free text, e.g. "Name the reminders so a surface can find them" (`13a0fd3`) | **not rewritten, not rewritable** — the id lives on as prose in `git log` and session history forever; this is the same "immutable id" fact stated as a cost rather than a rule |
| 12 | `app/__shell/conventions.md` — "Choosing an id" / "Item ids are immutable" | the convention text itself, not a data file | **the convention must change** if op:"rename" ships, since it currently states flatly "Item ids are immutable. Renaming is not supported" and names Drop-plus-re-propose as "the only conversion" (judell/bram#276 cited inline) |

Rows 1–3 and 6 are the mechanical core (files that must move together).
Rows 4–5 are the precondition set (state that must be *absent*, not moved).
Rows 7–9 are semantic edge cases the issue's table under-specified — 7/8
because the issue implies `closesIssues` is a rename risk for the item's
whole life when it is only one for part of it, and 9 because it postdates
the issue (`13a0fd3`/`3ad832e` shipped 2026-09-13, after #276 was filed).
Rows 10–12 are the "leave alone or update prose, never data-rewrite" set.

## 2. Three candidate semantics

### A. Rewrite everything (rows 1–3, 6 above), refuse near 4–5, leave 10–11 alone

The issue's own proposal: `POST /__worklist/mutate {"op":"rename","id":"<old>","to":"<new>"}`,
host-performed so it is atomic, scoped to items with `status:"proposed"`
and no `begunAtMs` (row 4/5's precondition — see below for why that scope
matters more now than when filed).

**Cost:** the host must touch up to five directories in one call (drafts,
citations, feedback-drafts, feedback-history, plus worklist.json itself)
and roll back cleanly if any individual move fails partway — the exact
atomicity bug #276 exists to fix, now with more files in scope than the
issue counted (it named 2, 6, 4, 5; it missed row 3, the dormant citations
sidecar, because that plumbing didn't exist as a documented "don't touch"
path until later). Getting this right is real work, but it is the only
candidate that produces a board where the id genuinely means one thing
before and after.

### B. Alias — new id is primary, the old id resolves to it forever

Keep a `resources/.worklist-aliases.json` (or an `aliases` array on the
item itself) mapping old → new. `/__worklist`, `/__worklist/resolve`, and
the draft/feedback file lookups all check the alias table before treating
an id as unknown.

**Cost:** every one of rows 2, 3, 6 needs an alias-aware read path added
permanently, not just at rename time — `worklist_draft_path`,
`feedback_draft_belongs_to_item`, and the citation merge would each need
to consult the alias table on every read, forever, for every item, not
just renamed ones. That is a permanent tax on the hot path (every
`/__worklist` GET) to serve a rare rename event. Worse: it means the id is
no longer a unique key — two spellings resolve to one item — which
contradicts row 9's routing use (`needs_you_reminder_row`'s
`starts_with("reminder-")` check would need to resolve the alias *before*
testing the prefix, or a reminder renamed away from the prefix keeps
routing to the inbox lane under its old alias forever, which is precisely
the kind of stale-signal bug Requirement 0 in `docs/needs-you-inbox.md`
was written to prevent). Aliasing solves the "back-references break" fear
the current conventions.md line names ("renaming breaks back-references
for marginal benefit") at the cost of making every future id lookup a
two-step resolution instead of one.

### C. Rename only before "begun" (no authorization/claim/history exists yet)

Narrower version of A: permit rename only when nothing outside
`worklist.json` + its draft references the id yet — i.e., the item has
never had an `approved:`/`drop:` click, `begunAtMs` is unset, no
`feedback-drafts/*-<id>.md` exists, and it never appeared in
`worklist-history/`.

**Cost:** cheap and structurally safe — rows 4, 5, 6, 10 are all
guaranteed absent by definition, so the only real move is rows 1–3. But
it **declines the case where the need is most acute**. The #276 filing
incident itself was `issue-275-target-pane-boots-while-hidden`, an item
whose scope had *widened during work* — which means it had almost
certainly already begun (authorization records, likely feedback drafts
from iterate cycles) by the time its name stopped fitting. An id "under-
naming its work" is discovered by working the item, not by reading the
proposal — so a rename that only fires pre-begun mostly serves items that
haven't been touched enough to need a better name yet, and refuses the
one this issue was filed to fix.

## 3. Recommendation

**Ship A, but split its precondition scope from what #276 originally
proposed:** allow rename for any item **not currently referenced by a
live authorization record or a live inflight claim** — rows 4 and 5 only
— rather than #276's stricter "must be `proposed`, `begunAtMs` absent."

Reasoning: `begunAtMs` (per `app/__shell/conventions.md`'s field notes) is
"host-written... and never clears... while the item lives" — it is a
durable historical marker, not a live-state indicator. Refusing rename on
*any* item with `begunAtMs` set reproduces candidate C's core defect
(declines the case where scope drift is discovered) for no safety benefit,
because `begunAtMs` being set says nothing about whether an authorization
or claim is *currently* live. The actual hazard — a rename mid-flight
while a turn is actively reasoning about the old id, or while a claim is
locking the board on it — is fully captured by checking rows 4/5 directly:
`validate_worklist_mutate_authorization`'s existing `auth_ids.contains`
pattern (`lib.rs:58580-58585`) and the inflight-claim file's `ids[]` are
both already-read structures; a rename precondition is "neither contains
this id right now," which is strictly narrower and strictly more honest
than gating on `begunAtMs`.

`status` still matters, but differently than #276 proposed: refuse rename
on `applied` too (not just non-`proposed`), because an `applied` item is
one commit-click away from staging files under its `id`-derived commit
message context and from potentially having `closesIssues` bookkeeping
mid-flight (row 7) — renaming under it is a race with the commit gate,
not merely a cosmetic risk. `proposed`-with-`begunAtMs`-set is exactly the
case worth unlocking; `applied` is not.

So the full precondition set for `op:"rename"`:

- `status` is `"proposed"` (not `"applied"`) — `begunAtMs` may be set or
  unset, this is the change from #276's draft
- no live (unconsumed, unexpired, `interruptedAtMs` unset) authorization
  record's `ids[]` contains the old id
- no live inflight-claim `ids[]` contains the old id
- target id doesn't already exist, matches kebab-case id shape, and — if
  it is entering or leaving the `reminder-` namespace — the caller is told
  explicitly that this changes which Awaiting You lane finds it (row 9),
  since that is now a routing change, not a cosmetic one
- source draft exists; target draft path doesn't
- host performs rows 1, 2, 3 (if present), 6 (all matches) as one atomic
  operation — write worklist.json last, after every file move succeeds,
  so a failure partway leaves the *old* id fully intact rather than
  producing a fresh version of #276's own bug

**Rejected:** B (alias) for the permanent-tax and stale-routing reasons
above; C (pre-begun only) for declining the motivating case, twice now —
once at filing and again at the `13a0fd3`/`3ad832e` conversions, both of
which needed to rename items that were unambiguously "begun" by any
reasonable definition (drafts with real content, in one case already
approved-and-dropped once).

**No separate authorization gate**, agreeing with #276's own instinct:
renaming an item whose status stays `proposed` and whose files list is
unchanged commits nothing and stages nothing — it is prose-adjacent
metadata editing, which agents already do freely under "Refine". The
precondition set above (not a fresh `approved:`/`drop:` round-trip) is
what keeps it safe.

## 4. Defect B, specified independently of A

**Ship this first, regardless of whether op:"rename" ever lands.** It is
small, self-contained, and valuable on its own — it converts a class of
silent failures (any unauthorized removal from `worklist.json`, not just
rename attempts) into loud ones.

### 4a. Make the revert observable to the agent that caused it

`maybe_enforce_worklist_policy` (`lib.rs:53092-53161`) already computes
everything needed: the offending id(s), their status, and the last
authorization kind on file (`lib.rs:53147-53153`). Two changes:

1. **Route the existing message through `append_bram_trace_line`, not
   `eprintln!`**, category `worklist-enforce` (a new token added to the
   closed vocabulary the comment at `lib.rs:2596-2599` names). This alone
   closes the "invisible to the agent" gap identified in the sharper
   finding above — the message already exists and already names the
   right things, it is just writing to the wrong stream.
2. **Make the *next write attempt* discoverable the way `stale-worklist-
   version` already is.** Today a stale-version write gets a structured
   deny with `reason=stale-worklist-version` that names the correct
   current version (per `app/__shell/conventions.md`'s worklist-version
   flow). A reverted removal should do the same: record the revert event
   (id, prior status, timestamp) somewhere the *next* `worklist.json`
   write can check, and have that check surface
   `reason=worklist-write-reverted:<id>` on the next PreToolUse hook
   evaluation or the next `/__worklist` GET, whichever the agent hits
   first. This is the general form the issue itself named as "probably
   the more valuable" fix: a silently reverted write becomes
   distinguishable from a successful one at the API surface the agent
   actually reads.

### 4b. Should the revert also restore a moved draft, or only report it?

**Recommendation: report, don't auto-restore the draft.** Reasoning:

- The revert already fully restores `worklist.json` to `prior_str`
  (`lib.rs:53150`) — that half is genuinely a rollback.
- The draft file is a plain filesystem move the agent made with its own
  tool call (`mv` or an Edit-tool rewrite at a new path), entirely outside
  `maybe_enforce_worklist_policy`'s view — it only sees `worklist.json`
  diffs, never touches `resources/worklist-drafts/`. Having the revert
  path reach into a *different* directory and move a file back is a much
  larger blast radius than restoring the one file it's already
  authoritative over, and it can go wrong in its own new way (what if the
  agent had already started editing prose at the new path before the
  revert fires asynchronously? auto-restoring now silently discards that
  edit).
- The coherence check `GET /__worklist` already performs
  (`_draftMissing`, `lib.rs:46057`) is the right place to *detect* the
  mismatch — it already does. What's missing is surfacing it loudly
  rather than rendering a quiet placeholder. Add a `_draftOrphaned`
  companion signal: when a draft file exists at a path that doesn't match
  any current `worklist.json` id **and** its name matches a status-quo id
  from the id just reverted-away-from (row 4's revert record from §4a
  gives the host exactly this fact for a bounded window), report it
  explicitly rather than leaving the reader to notice a `_draftMissing`
  row and reason about it from scratch.
- This keeps the fix within the file `maybe_enforce_worklist_policy`
  already owns, makes the failure *legible* (which is the actual ask —
  "stop the silent failure," not "make renaming work by accident through
  the revert path"), and leaves the human or agent to do the one-line
  `mv` back once they see the report — which they can now do confidently
  because the report names both paths.

## 5. The Drop-plus-re-propose workaround, and its begun-item gap

`app/__shell/conventions.md`'s current answer (added alongside the
`reminder-` prefix work, `13a0fd3`/`3ad832e`) is: the user Drops the item,
the agent re-proposes it verbatim under the new id, and the history reads
drop → re-propose honestly. Exercised twice on 2026-09-13
(`reminder-revendor-xmlui-after-table-sort-fix-releases` and
`reminder-rerun-notification-census-after-2026-09-19`, both visible in
`resources/worklist-history/1789403155271.json` with their "Lineage"
sections explaining the conversion inline).

**This workaround only ever serves items with nothing on disk.** Both
2026-09-13 conversions were pure-prose reminder items — `files: []`,
nothing to touch until an external condition resolves. That is why
Drop-plus-re-propose cost nothing there: the "before" and "after" the
Drop discarded and the propose recreated were text, not a working tree.

For a **begun** item — real edits already on disk, per `app/__shell/
conventions.md`'s "Field notes" (`begunAtMs` set, changes exclusive to the
item) — Drop is documented to leave those changes stranded under a
dropped id: "**Drop removes the item, not the bytes** — and orphaned
changes are misattributed, not merely unattributed... every surface...
credited to whichever begun item happens to declare it." A drop-then-
re-propose of a begun item would either (a) leave its on-disk diff
credited to nothing until the fresh item's `files` happens to re-cover the
same paths (accidental re-adoption, not intentional), or (b) require the
`park/drop/re-propose` full patch-parking dance that
`app/__shell/conventions.md`'s "Serializing entangled items" section
describes for a completely different problem (joint-interval commit
refusals) — repurposing that heavier mechanism just to rename a begun
item is exactly the friction #276 was filed to remove.

So the workaround has zero coverage for the case §3's recommendation
targets (a `proposed`, `begunAtMs`-set item whose name no longer fits its
widened scope) — which is the *exact* incident that opened #276. The gap
is not a corner case; it is the whole motivating scenario.

## 6. Decomposition into independently gateable items

In landing order, each with its own worklist item and gate:

1. **`worklist-rename-revert-observable`** (defect B, §4a) — route the
   `maybe_enforce_worklist_policy` revert eprintln through
   `append_bram_trace_line` under a new `worklist-enforce` trace category
   registered in `docs/trace-vocabulary.md`; add the next-write
   `reason=worklist-write-reverted:<id>` surfacing. Small, self-contained,
   ships regardless of items 3–4 below. Update `docs/trace-vocabulary.md`
   in the same change per the "register new subkinds" rule in
   `docs/developing-bram.md`.
2. **`worklist-rename-draft-coherence-check`** (defect B, §4b) — add
   `_draftOrphaned` detection to `GET /__worklist`'s resolution path
   (`resolve_worklist_item_draft`, `lib.rs:46025`), reporting rather than
   auto-restoring. Depends on item 1's revert record existing to bound the
   detection window; otherwise a stray draft file with no matching id is
   ambiguous (orphaned by a revert vs. orphaned by an old manual mistake).
3. **`worklist-rename-op-mutate`** (candidate A core, §3) — add
   `op:"rename"` to `worklist_mutate_required_kind` and
   `/__worklist/mutate`'s dispatch, host-atomic across rows 1, 2, 3, 6 of
   the inventory, gated on the precondition set in §3 (status `proposed`,
   no live auth/claim covering the id, target id free and well-shaped).
   This is the item with the real atomicity engineering — five
   directories, ordered so `worklist.json` writes last.
4. **`worklist-rename-conventions-update`** — once item 3 ships, update
   `app/__shell/conventions.md`'s "Choosing an id" section (currently:
   "Item ids are immutable. Renaming is not supported... judell/bram#276")
   to describe the supported path and its preconditions, and note that
   Drop-plus-re-propose remains the only option for `applied` items or
   items with a live claim/authorization.

Items 1–2 are worth shipping even if 3–4 never do — they fix the part of
#276 everyone agrees is a bug (a write that lies about succeeding) without
committing to a rename UI or wire contract at all.

## What I could not determine

- Whether `/__worklist/resolve`'s consume-on-read semantics interact with
  a rename that lands between a `resolve` and the matching `mutate` in the
  same turn — I traced the same-turn `resolve → edit → mutate` path
  (`app/__shell/conventions.md`, "Enforcement and security contract") but
  did not find a scenario where a rename would be issued mid-sequence by
  the *same* agent turn that also holds a resolve; flagging as an
  open question for whoever implements item 3, since the precondition
  set in §3 checks live auth/claim state but not "is this id currently
  the subject of an in-progress resolve→edit→mutate sequence within this
  same call."
- Whether the citations sidecar (row 3, `resources/worklist-citations/`)
  has any live callers beyond the dormant-per-convention state described
  in `app/__shell/conventions.md`'s search section — the merge code
  (`resolve_worklist_item_draft`) runs unconditionally regardless, so I
  scoped it into the atomic-move set defensively rather than confirming
  zero production use.
