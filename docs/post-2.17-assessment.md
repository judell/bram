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
| Esc-resend | outcome-anchored send ledger replaced bounce heuristics, landing state, Resend button | Phases 1–3 done for v0.2.17; **phase 4 "delete the superseded layers" is TODO** |
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
