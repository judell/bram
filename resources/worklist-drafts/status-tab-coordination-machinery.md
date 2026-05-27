# Before

Issue #103 enumerates eight coordination signals Bram should make visible when the agent/worklist flow feels off: worklist transitions, inflight sentinel state, latest-tail polling, JSONL fanout, guard decisions, stale approvals, interrupts, and Inspector trace exports. Today those signals are scattered across `resources/worklist.json`, `resources/.worklist-authorization.json`, `resources/.inflight-claim.json`, `resources/bram-trace.log`, hook output, and `~/Downloads/xs-trace-*.json`.

The practical result is that debugging a stuck spinner, stale worklist item, suspicious fresh-tail churn, missing fanout subscriber, or stale approval requires remembering which file or trace category to inspect and then grepping manually. The Worklist tab shows item state, but it does not expose the surrounding coordination machinery that explains why state may not be moving.

# After

Add a first-cut Status tab to the agent-tools drawer that surfaces Bram's coordination machinery in one place, starting with the highest-pain signals from #103 and leaving the remaining signals shaped for follow-up expansion.

Scope the first cut to signals that already have local state or trace records and can be rendered without inventing a large new subsystem:

1. Worklist transitions and current counts from `resources/worklist.json`, plus enough recent transition history from the existing worklist mutate/authorization records to explain proposed/applied/committed/pruned movement.
2. Inflight sentinel health from `resources/.inflight-claim.json` and `[inflight-sentinel]` trace lines, including current kind, ids, age, and recent write/clear/silence-clear-skip events.
3. Latest-tail / JSONL fanout freshness from existing trace subkinds (`latest-tail`, `jsonl-fanout`, `jsonl-broadcast`, `jsonl-cap-trim`) so the tab can show whether polling is diff-heavy, whether subscribers exist, and whether cap trims or repeated fresh fetches are happening.
4. Guard/staleness/interrupt/trace readiness as lighter summary rows: recent hook blocks or allow decisions where available, rejected-stale resolve events, silence/interrupt-related trace records, and latest `xs-trace-*.json` export metadata from `~/Downloads` when available.

Implementation shape:

- Add `/__coordination-status` as a Rust-backed host route that gathers a compact JSON payload for the Status tab from existing files, `resources/bram-trace.log`, worklist history snapshots, the inflight sentinel, and latest `~/Downloads/xs-trace-*.json` metadata. Do not make the XMLUI surface grep logs directly.
- Add a Status tab entry in the drawer UI alongside the existing Worklist/Transcript/Workspace surfaces, backed by `app/tools/components/Status.xmlui`. The component should render from the DataSource's `status.value` payload, not the DataSource wrapper itself, and should show explicit loading/error/empty states so a bad binding or failed route does not look like an empty pane.
- Render dense operational rows rather than a marketing-style dashboard: section headers, status badges, counts, timestamps/ages, and short concerning-state hints. The tab should answer, at a glance, whether the worklist, sentinel, fanout/latest-tail, guards, staleness, interrupts, and trace exports look healthy.
- Keep thresholds explicit and conservative: e.g. sentinel age over roughly two minutes is concerning, repeated `latest-tail mode=fresh` during steady polling is concerning, zero fanout subscribers while a tab is visible is concerning, stale approvals should be near-zero, and TO COMMIT/applied items should not sit unexplained for days.
- Mine the stashed `post-v33 codex status-page exercise` only as reference material if useful; do not apply it wholesale. The landed code should match current Bram conventions and go through this worklist item.

Done test:

1. With existing worklist items present, open the Status tab and confirm worklist counts match the Worklist tab.
2. Trigger an iterate or approved/drop flow and confirm the Status tab shows the current inflight claim while active and recent write/clear events after it resolves.
3. Confirm latest-tail/fanout sections populate from current trace lines and distinguish diff vs fresh polling plus subscriber counts when those records exist.
4. Confirm missing optional data degrades cleanly: no recent guard blocks, no stale approvals, no interrupts, missing `bram-trace.log`, no worklist-history directory, or no `xs-trace` export should show as neutral/empty rather than an error.
5. Confirm the tab refreshes on a reasonable interval or explicit reload without requiring an app restart.
6. Confirm this proposal itself exercises the draft mechanism: `resources/worklist.json` contains only metadata for `status-tab-coordination-machinery`, `/__worklist` resolves `before` / `after` from `resources/worklist-drafts/status-tab-coordination-machinery.md`, and an iterate edit to this draft updates the Worklist modal without touching `resources/worklist.json`.
7. Confirm the Inspector trace for the Status tab shows `/__coordination-status` returning `sections`, and the rendered table consumes `status.value.sections` rather than rendering blank while the route is healthy.

Out of scope for this first cut:

- A complete design spec for every signal in #103.
- New enforcement behavior or changes to worklist/sentinel semantics.
- Deep trace analytics beyond compact recent summaries.
- Reworking the existing Worklist tab.
