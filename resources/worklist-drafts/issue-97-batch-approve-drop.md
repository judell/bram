# Before

The Worklist tab is single-selection only: `var.selected` holds one
item id, and the per-item **Approve** / **Iterate** / **Drop** buttons
act on that one item via `buildApprovePayload` / `buildIteratePayload`
/ `buildDropPayload` in `Globals.xs`. Committing a coordinated bundle
of N TO COMMIT items — five trims of one file, a multi-section
refactor proposed as N items — costs N separate Approve clicks, and
cleaning up post-bundle residue costs N more Drops (issue #97, friction
points 1 and 2).

The `approved:` / `drop:` payload shape (`{items:[{id,hash,feedback}]}`)
and the host parser (`parse_worklist_authorization_message` in
`lib.rs`, which loops the full `items` array and verifies each hash
independently) already support N-item batches. The gap is purely UI:
no button submits more than one item.

# After

Add an **Approve all** / **Drop all** batch bar to the Worklist tab,
scoped to **TO COMMIT (applied) items only** (the scope the issue
names; both friction points are TO COMMIT).

`Globals.xs` — three new helpers after `buildSingleItemApprovePayload`:

- `countByStatus(items, status)` — count of items in a status group,
  for the bar's `when` guard and button labels.
- `buildBatchApprovePayload(items, feedback)` — `{items:[...]}` over
  every `applied` item, each `{id, hash, feedback}`.
- `buildBatchDropPayload(items, feedback)` — symmetric.

`Workspace.xmlui` — a batch bar rendered above the `<Items>` list,
shown only when there are **≥2** TO COMMIT items (with 1, the per-item
Approve already suffices):

- "`N items ready to commit`" label, **Approve all (N)** and
  **Drop all (N)** buttons, plus an ⓘ help dialog.
- Buttons disabled while `submitting`. On click: clear selection
  state, set `submitting = true` and `submittedItemId` to the first
  TO COMMIT id (one spinner, consistent with the sentinel path), then
  `toTurn('approved: ' + buildBatchApprovePayload(...))` /
  `'drop: ' + buildBatchDropPayload(...)` with empty per-item feedback.
- The agent resolves the batch via `/__worklist/resolve` and prunes
  via `/__worklist/mutate` exactly as today — N ids per call, no
  server change.
- ⓘ help dialog (`actionHelpFor === 'batch'`) explains the actions and
  surfaces the close-on-commit caveat below.

**Close-on-commit:** batch Approve sends **no** `close-issue:` lines,
so issues flagged via `closesIssues` are **not** auto-closed in a
batch — the ⓘ dialog says to close them via single-item Approve or in
chat. (Chosen over aggregating into one combined dialog.)

Alternatives considered:

- Multiselect checkboxes + "Approve selected" — **rejected:** larger
  change; the issue explicitly defers multiselect ("or every selected
  item, *if a multiselect mechanism lands first*").
- Per-status group bars (also batch TO APPLY) — **rejected:** user
  chose TO COMMIT-only scope; the flat, non-grouped list makes paired
  per-group bars busy.
- Aggregate close-issue dialog for the batch — **rejected:** user
  chose to skip; safe default never closes an issue without a
  per-item confirm.
- **[chosen]** Single batch bar over TO COMMIT items, ≥2 threshold,
  one `approved:` / `drop:` payload, empty feedback, no close-issue
  dialog.
