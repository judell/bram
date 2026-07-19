# Menu interventions: catalog and strategic assessment

A retrospective across the ~195 menu-related commits (through 2026-07-18),
prompted by the question "is this Sisyphean?" The companion architecture
doc is `docs/menu.md` (mechanism map, current to its own date); this one
catalogs what went wrong, what was done, why recent fixes hold, and what
remains structurally open. Shape docs: `docs/pty-menu-shapes.md`,
`docs/pty-menu-hook-catalog.md`, `docs/menu-detection-audit.md`.

## The generations

Successive architectures, each subsuming (not merely patching) its
predecessor:

| Gen | Approach | Representative commits | Fate |
| --- | --- | --- | --- |
| 0 | Poll the session JSONL for pending `tool_use` | `545ba14`, `9be1bea`, `3904c84` | Too laggy; blind to non-transcript prompts. Survives as the signature/enrichment oracle. |
| 1 | Scan raw PTY bytes for menu-shaped text | `591ab1a`, `3816a16`, `e5faacf` | Flappy; Gen-2 deletions retired its diagnostics (`b9bc27a`). Byte-pattern detect survives as a fast hint. |
| 2 | Parse the rendered xterm grid (on-screen cells) | `65882f1`, `0442e95`, `6693f5e` | Ground-truth channel to this day; the labels/scene joins anchor on it. |
| 3 | Hook-primary: PermissionRequest/PostToolUse POST structured menus | `78d5702`, `250b69a`, `b7d0386`, `1f17e3d` | Semantic richness (previews, diffs); introduced the coordination bug family. |
| 4 | Causal discipline: clocks and tuned windows replaced by invariants | `f55cd2a` (absence fence), `c5161ab`, `3c41db4` (tool_use identity), `d407a14` (outcome anchoring) | The turning point: fixes stopped regressing. |
| 5 | Claim queue + terminal arbitration + always-on forensics | `40855ce`, `f5c9342`, `22f2e36`, `a5558ae`, `6279213`, `9beb77b` | Parallel prompts (subagent fanouts) made honest; misses self-attribute. |

## The failure buckets

1. **Seeing** — the grid parse misses a rendered menu. Wrapped labels,
   stale-cell garbling ("2.Yes" with no space), buffer eviction,
   trailing-run poisoning by menu-shaped prose (`a5558ae`). The defense is
   shape tolerance plus specimen capture (`xterm-grid-miss`).
2. **Identity** — same menu re-read, or genuinely new? The deepest bucket.
   Lineage: 600 ms window → 10 s cap → causal absence fence (`f55cd2a`) →
   resolution + frame-provenance corroboration (`c5161ab`) → identity
   keyed on `tool_use` id (`3c41db4`) → labels primary over guessed
   signatures (`f5c9342`).
3. **Channel coordination** — hook, grid, and JSONL disagreeing about the
   display. Ownership and defers (`4f4e725`, `aefbe9d`), stale-claim
   release (`22f2e36`), signature misattribution under parallel pending
   calls, and finally the claim queue with the terminal as arbiter
   (`40855ce`): the pane shows the claim that joins the grid's display,
   so a pane answer keystrokes the prompt it shows, by construction.
4. **Lifecycle** — taking menus down correctly. Hook clears voiding held
   menus (`a29c0d1`), stranded-menu reclaim (`aefbe9d`), keyed clears that
   cannot blank another prompt (`40855ce`), and orphan tolerance when the
   clear never comes (`6279213`).
5. **Upstream gaps** — Claude Code behaviors that force inference, with
   reproducible 2026-07-18 specimens: `PermissionRequest` carries no
   `tool_use_id`; **no `PostToolUse` fires for a command that runs and
   fails** (two exit-1 reproductions, breadcrumb-logged); occasional
   prompts with no PermissionRequest at all (open, instrumented). And
   fundamentally: a TUI renders menus and prose in the same characters.
6. **Instrumentation** — the meta-bucket that makes the others converge:
   trace categories (`4cb0d90`), surface-gap with honest per-menu stamps
   (`8c07fc1`, `3b60950`), the claim-queue ledger, hook-event breadcrumbs,
   always-on strand forensics (`9beb77b`). A miss in 2026-05 meant hours
   of guessing; a miss in 2026-07 is a five-minute ledger read.

## The ratchet argument

The effort is asymptotic, not Sisyphean, for a structural reason: since
Gen 4, fixes are causal invariants rather than tuned heuristics. A clock
window regresses when load changes; "suppress until the grid has observed
the dismissed menu absent" cannot. "Display the claim whose labels join
the terminal's display" cannot show the wrong menu, at any timing. The
2026-07-18 provocation battery attacked four previously-fixed mechanisms
(trailing-run poison, identical-fingerprint succession, parallel prompts,
command-substitution shape) and provoked zero misses; the same day's soak
then surfaced a genuinely new class (orphaned claims from failed
commands), which was attributed entirely from ledgers and closed causally
(`6279213`) within the hour. New stress finds new classes; closed classes
stay closed.

## Structurally open

- **The limit**: Bram reconstructs a closed program's interactive state
  from outside, over three lossy channels. Perfect fidelity requires the
  program's cooperation.
- **The upstream ask**: hook-lifecycle completeness — `tool_use_id` on
  `PermissionRequest`, a guaranteed terminal event on every tool outcome,
  and ideally an explicit `PromptShown`/`PromptResolved{outcome}` pair —
  would collapse buckets 3–5 into event-following. Recorded locally in
  `docs/upstream-asks.md` (deliberately not filed externally).
- **Parked observations** (instrumented, awaiting specimens): intermittent
  missing PermissionRequest (`grid-menu-without-claim` +
  `hook-events.log` classify it passively); menu-answer keystrokes landing
  in the pane composer as chat turns (two occurrences, uncaptured).
