# Post-2.17 assessment — consolidate before 2.18

**Date:** 2026-07-06. **Question:** before releasing v0.2.18, is another
consolidation warranted, and can 2.18 reduce rather than grow the line
count? **Method:** three research passes (redesign-doc review, git
line-count archaeology, caller-verified legacy inventory), run as
subagents during the 2.18 burn-in of esc handling, menus, and subagent
transcripts.

## 1. What the prior consolidations did — and didn't

| Redesign | Consolidated | Delete phase status |
| --- | --- | --- |
| Turn-transport | transport / persistence / projection split; clients became pure consumers of `/__turns` | **Complete** (steps 1–7; client JSONL parsers deleted in `b773098`) |
| Esc-resend | outcome-anchored send ledger replaced bounce heuristics, landing state, Resend button | **Phase 4 mostly executed** in #214 tranches 3a/3b: bounce heuristics, write-only landing state, four iframe end-detectors, and localStorage awaiting keys deleted; capture scrapers intentionally kept as diagnostics |
| Menus (#182 lineage, four generations) | hooks primary (Gen-4), grid sole detector (Gen-3), byte detection retired | `dc74e78` delivered the Gen-2 detector deletion (−1,852); **post-burn-in demotion of Gen-3 classifiers and Gen-2 scan diagnostics not done; "the prize" (ExitPlanMode hook → Gens 1–3 become trivial oracle) deferred** |
| xterm-grid reading | grid replaces strip_ansi parsing | `e77ed36` deleted parsers (−499); **step 4 "retire raw-PTY fallback" pending trace-coverage validation** |
| Turn-state spine (#182) | one arbitration point for turn state | Complete as designed (overlay, not replacement — nothing to delete) |
| Session-context architecture | session-global state out of routed Pages | Menu hoist **done 2026-07-05** (footer canonical menu surface); voice wedge still open; shell-cache move not done |

The pattern is unambiguous: **build phases ship; delete phases starve.**
Twelve explicitly promised retirements across these docs remain undone.

## 2. Line-count reality

Current core: `lib.rs` 29,499 · `helpers.js` 5,453 · `main.js` 2,523 ·
`Globals.xs` 732 · `.xmlui` total 4,478 ≈ **42,700 lines**, from 226 on
2026-05-03. Still accelerating: +5,500 in `lib.rs` over the last 17 days.

Only ~12 commits in the repo's history ever shrank `lib.rs`. The top
two, by an order of magnitude, were executed delete phases:

- `dc74e78` retire byte-based menu detection: **−1,852**
- `e77ed36` delete strip_ansi parsers + fixtures: **−499**

Plus `Globals.xs` halved (1,581 → 741) in the v0.2.10-era host-route
migration. Shrinking is not hypothetical here — it happens exactly and
only when a delete phase is treated as the deliverable.

## 3. The hole-digging question, honestly

The 2026-07-05/06 menu chain was five commits in ~14 hours, three of
them correcting the commit immediately before (`4f46382` ghost
suppression → `f55cd2a` causal fence → `c5161ab` three-gate fence).
Each iteration was evidence-driven, narrowed the failure class
(multi-second clickable ghosts → sub-second flicker admits → none
known), and terminated in a pure, unit-tested decision function. That
is convergence by the numbers.

It is also digging, by the shape. Every hole was in the same place:
the **inference layer** — deriving menu state from pixels plus JSONL
timing, neither of which Bram controls. The fence is now as good as
that layer can be, and the correct reading of the spiral is not "keep
hardening" but "stop investing here." `docs/menu.md` already names the
endgame — hook-primary detection with the grid demoted to oracle — and
the burn-in now running is precisely the evidence-gathering that plan
requires. **Decision rule going forward: a change that adds lines to
the inference layer must state why the hook layer cannot own the
behavior.**

## 4. Verdict and 2.18 candidates

A consolidation is warranted, and it is not a new redesign: **2.18
should be the delete-phase release** — executing retirements the
existing redesign docs already promised, plus dead surface found by
caller verification. Ranked by deletable lines per unit risk:

| # | Candidate | ~Lines | Risk | Evidence |
| --- | --- | --- | --- | --- |
| 1 | Dead routes, zero app/ callers: `__last-assistant-text` (+fn), `__last-exchange` (+fn), `__session-turns` (+fn +dedicated cache), `subscribeLatestJsonl` | ~120 | Trivial | caller sweep: zero consumers |
| 2 | Legacy `__iterate/begin` / `__iterate/end` routes | ~110 | Low — conventions already say "no longer required"; grep agent guidance first | conventions.md §sentinel |
| 3 | Gen-2 byte-scan diagnostic machinery (`pty_scan_*`, `pty_menu_scan_diagnostic`) — runs on every PTY chunk purely to trace | ~580 | Low-medium — it is the forensic tool for menu misses; delete after the fence soak is quiet, keep `record-trace.sh` capture path | menu.md §Cleanup "now, low risk" |
| 4 | Esc-resend phase 4: bounce heuristics, iframe landing state, capture scrapers (helpers.js + lib.rs) | est. 300–600 | Medium — inventory the exact surface first; ledger has soaked since v0.2.17 | esc-resend-redesign.md §What gets deleted |
| 5 | Latest-tail push pipeline (`helpers.js` ~860 lines) — now only a change signal for `/__turns` refetch; replace with a slim tick | ~600 net | Medium — one consumer chain, but it is the Transcript's heartbeat; do behind a soak | inventory: no raw-JSONL consumers remain |
| 6 | "The prize": PreToolUse hook for ExitPlanMode → demote Gen-3 classifiers, simplify Gen-1 signature cross-ref | large (secondary deletions across the menu path) | Higher — needs hook-coverage evidence (`op=gap` silence over the burn-in) | menu.md §Cleanup |

Items 1–3 alone are ~800 lines of negative diff at low risk; with 4–5
the pool is ~1,500–2,000. Feature-freeze aside, **a net-negative 2.18
is feasible** if the release's headline deliverable is this table.

Also collected, cheap doc-drift fixes: `conventions.md` still claims 86
`Globals.xs` delegators and cites `docs/code-organization-audit.md` —
today's count is **zero delegators** and the audit file does not exist;
`docs/mirror-e2e-scratch.md` is self-declared deletable.

## 5. What burns in meanwhile

The soak criteria for the pieces shipped this cycle, all trace-checkable:

- Fence: zero `post-absence-reappear` without a corresponding real
  menu; `unproven-frame-redetect` present under fast commands; no
  stray option-key turns.
- Menus on subagent viewports: footer emit rate ≈ 2 per real menu
  (show + user answer); if remount churn persists post-gate-three,
  the scoped fix is identity-on-decision-surface (tool + option
  labels), not another fence change.
- Subagent transcripts: chips/roster/notifications steady through
  multi-agent research fan-outs (this document's research was the
  first such soak).
- Esc handling: send-ledger notices remain accurate through the
  workout (no strand/restore surprises).

When these are quiet, execute the table top-down.

## 6. Execution log

- **2026-07-06 — tranche 1 executed** (`8dea22f`, −192 net): table
  candidates #1–2 plus the doc-drift fixes. Caller checks re-verified
  at apply time; the `/__worklist/end` alias was kept (out of scope).
  The commit gate itself surfaced a staging gap on nothing-matching
  paths — fixed as `worklist-commit-stage-deletions` so tranches 2+
  can delete real files through the gate. Candidates #3–6 remain,
  gated as tabled.
- **2026-07-06 — tranche 2 executed** (`b9bc27a`, −631 net): candidate
  #3, the Gen-2 byte-scan diagnostics. Gate evidence: zero
  `[pty-menu-scan]` lines across the heaviest menu-forensics stretch to
  date (2026-07-05/06, six menu failure modes diagnosed entirely from
  the grid/fence/hook trace layers). The excision cascaded — rustc
  flagged four more byte-scan-era helpers orphaned, taking the net from
  the priced ~580 to −631. `pty-menu-scan-report.py` marked retired
  against its now-dead data source.
- **2026-07-06 — tranche 3a executed** (`d8ad622`, −64 net): the
  dead-only half of candidate #4 — bounce heuristics end to end and
  write-only iframe landing state. Kept, with evidence recorded in the
  esc-resend phase-4 execution ledger (`docs/esc-resend-redesign.md`,
  the authoritative per-item record for candidate #4): capture scrapers
  (their `send-capture` traces did live forensic work twice that same
  day), `awaitingResponse` (the live submit-button gate; its
  ledger-driven rewire is tranche 3b, filed as
  `issue-214-tranche-3b-ledger-submit-gate`), and
  `submittedWorklistMessage` (read on mount). Candidate #4's remainder
  moved to tranche 3b.
- **2026-07-06 — tranche 3b executed** (`f2125e3`, +17 net):
  candidate #4's live submit-button gate moved onto the host send
  ledger. `awaitingTurn` is derived from `/__send-ledger`; the iframe's
  four end-detectors, `__bramMarkTurnEnded`, and awaiting localStorage
  keys were deleted. The small positive diff is intentional: it trades
  scattered iframe state for one host-owned outcome source.
- **2026-07-06 — candidate #5 executed** (worklist
  `issue-214-latest-tail-slim-tick`): the latest-tail push pipeline
  retired end to end — host cursor/diff/cap machinery and the
  `/__sessions/latest-tail` route (zero fetchers), the iframe raw-JSONL
  cache with its startup gating and replay path, the four `jsonl-*`
  trace subkinds, and the Status tab's latest-tail/fanout/backpressure
  counters. `talk-session-changed` now carries only `{sid, provider}`
  and the iframe tick calls `__bramRefetchProjectedTurns` directly; the
  `__projectedTurns*` broadcast Transcript consumes is unchanged.
  Apply-time verification also caught two stale conventions.md trace
  rows (`sessionTurns-parse`, `helper-call`) whose emitters were
  deleted in an earlier phase — pruned with the four retired rows.

## 7. Current status after issue #214 cleanup

As of `9d5fff9` plus the follow-on Esc hardening (`b77868f`,
`a7fda3f`), the planned delete-phase release has largely done what this
assessment asked for. Candidates #1–5 are executed or intentionally
bounded:

| Candidate | Status | Result |
| --- | --- | --- |
| #1 Dead routes / client JSONL remnants | Done in `8dea22f` | Removed zero-caller routes and helper surface; no app callers left behind |
| #2 Legacy iterate endpoints | Done in `8dea22f` | Removed legacy begin/end routes; host-managed iterate sentinel remains canonical |
| #3 Gen-2 byte-scan diagnostics | Done in `b9bc27a` | Deleted the PTY byte-scan diagnostic layer and orphaned helpers after grid/fence/hook traces proved sufficient |
| #4 Esc-resend superseded layers | Mostly done in `d8ad622` + `f2125e3` | Bounce heuristics, iframe match/baseline state, four turn-end detectors, `__bramMarkTurnEnded`, and awaiting localStorage keys are gone; capture scrapers remain diagnostic by explicit evidence |
| #5 Latest-tail push pipeline | Done in `9d5fff9` | Replaced the raw-JSONL push/cache/startup pipeline with a slim session-change tick that refetches `/__turns`; Transcript's projected-turns consumer stayed stable |
| #6 ExitPlanMode hook / menu classifier simplification | Remaining major item | Still higher-risk and still depends on hook-coverage burn-in (`op=gap` silence) before demoting Gen-3 classifiers and simplifying Gen-1 cross-reference |

The cleanup is therefore close to the original target through the
medium-risk items: the table's concrete, priced candidates #1–5 have
landed, and the largest unresolved cleanup is the deliberately deferred
"prize" (#6). The remaining work is no longer a broad post-2.17
debt pile; it is a narrower menu-architecture decision gated on evidence.

Net simplification from the main #214 commits:

- `8dea22f`: 46 insertions / 238 deletions.
- `b9bc27a`: 28 insertions / 659 deletions.
- `d8ad622`: 27 insertions / 91 deletions.
- `f2125e3`: 141 insertions / 124 deletions, trading iframe state for
  host-owned ledger-derived submit state.
- `9d5fff9`: 112 insertions / 853 deletions.

That totals 354 insertions / 1,965 deletions across the five cleanup
commits, before counting smaller follow-on hardening. The release did
become net-negative in the intended places: less client parsing, fewer
legacy routes, less raw JSONL transport machinery, and a single
host-owned send ledger doing work previously split across heuristics,
localStorage, and iframe listeners.

Residual follow-ons to track separately:

- Candidate #6: decide whether the PermissionRequest / ExitPlanMode hook
  evidence is strong enough to demote the remaining menu inference layers.
- Capture scrapers: keep for one release where they are unconsulted, then
  revisit deletion.
- Small dead-state sweeps such as `submittedKind` persistence, if caller
  verification still shows no turn-end readers.
