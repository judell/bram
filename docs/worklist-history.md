# Worklist history

xmlui-desktop snapshots `resources/worklist.json` on every meaningful
change, so the prose of past items survives after they've been
committed or dropped. This document sketches how the pieces fit.

## Flow

A filesystem watcher in `src-tauri/src/lib.rs` notices writes to
`resources/worklist.json` and calls `maybe_snapshot_worklist`, which
compares the new contents to a cached prior state. If anything
meaningful changed — see *Phases* below — it writes:

- `resources/worklist-history/<unix_ms>.json` — the *current*
  (post-change) worklist contents
- `resources/worklist-history/<unix_ms>.md` — a human-readable
  changelog describing the transition from the prior snapshot

Trivial writes (re-wording an item's `before` or `after` prose
without changing its identity, status, or the worklist's
description) are suppressed. The cache is always updated regardless,
so the next change diffs against the latest state.

## Phases

The changelog tracks four named phases. They appear both in the
summary line —

```
**Summary:** {p} proposed, {a} advanced, {r} renamed, {x} pruned
```

— and as section headers in the body:

- **proposed** — an item newly written to `worklist.json` (TO APPLY).
  A worklist item appears here when the agent first asks the user to
  authorize a change.
- **advanced** — an item's `status` transitioned. Typically
  `proposed → applied` after the user approves an apply, but the
  mechanism is general.
- **renamed** — a new item adopted an old item's identity by
  declaring `rename_from: "<old-id>"`. The pair is reported once as a
  rename, not separately as `proposed + pruned`.
- **pruned** — an item disappeared from `worklist.json`. Either
  committed (after a TO COMMIT approval) or dropped (via the `drop:`
  UI action).

A snapshot fires when *any* phase has at least one entry, **or** when
the worklist's `description` field changes. Otherwise the write is
treated as a content edit and suppressed.

## HTTP routes

The right-pane loopback server (`lib.rs::route_request`) exposes:

| Route | Returns |
|-------|---------|
| `/__worklist` | `worklist.json` augmented with a `diff` field on each `applied` item (the `git diff -- <file>` output) |
| `/__worklist-history/list` | reverse-chronological snapshots with `ts`, `iso`, `summary`, and the full `changelog` text embedded |
| `/__worklist-history/changelog?ts=<ms>` | raw `.md` body for one snapshot |
| `/__worklist-history/snapshot?ts=<ms>` | raw `.json` body for one snapshot |

The list endpoint embeds the changelog text directly, so the UI
doesn't need a second fetch per row.

## Changelog format

Each `.md` opens with the summary line shown under *Phases*, followed
by sections for the phases that fired this round:
`## Description changed`, `## Items proposed`, `## Items advanced`,
`## Items renamed`, `## Items pruned`. Each item appears under its
phase with the full before/after prose carried forward.

## Renames

Renames are first-class. A new item declaring
`rename_from: "<old-id>"` adopts the old item's identity. Two pieces
keep this honest end-to-end:

1. `.claude/hooks/worklist-guard.py` recognizes the field and
   permits the old id's removal without an explicit `drop:` from
   the user.
2. `generate_worklist_changelog` pairs the rename and reports it as
   `1 renamed` rather than `1 proposed + 1 pruned`.

## UI

`app/tools/components/Workspace.xmlui` polls
`/__worklist-history/list` every 10 seconds and renders a
chronological list of `iso + summary` rows under the worklist
itself. Expanding any row renders the snapshot's `.md` via
`<Markdown content="{$item.changelog}" />` — phase headers plus
the prose of each item that moved through the transition.

## Per-project applicability

xmlui-desktop is launched against an arbitrary project, so the
history directory is created inside that project's `resources/`.
Each project decides independently whether to commit
`resources/worklist-history/` or keep it gitignored. xmlui-desktop's
own repo commits its history — see `resources/worklist-history/`.

The choice is expressed via the project's `.gitignore` — no XMLUI
config or runtime flag is involved. A project that wants to opt out
adds `resources/worklist-history/` to `.gitignore`; a project that
wants to track history simply omits the path and stages the snapshot
files alongside whatever feature work produced them.
