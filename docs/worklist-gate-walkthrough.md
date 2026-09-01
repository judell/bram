# Worklist gate walkthrough — hand-testing the selection matrix

A recipe for exercising the Worklist gate by hand: the button matrix, the
granularity choice, entanglement handling, and the lifecycle's stuck-prone
paths. It uses **real worklist items doing real but trivial work**, because
the failures worth catching are in the lifecycle and the markup, and a
fixture that fakes item state stops testing what ships.

Born as the 0.5.3 release gate (run of 2026-08-24, scorecard at the bottom —
three findings, all fixed in-run). Run it before a release, or after any
change to the gate surfaces (`WorklistGateBar.xmlui`, `Worklist.xmlui`, the
selection/committability predicates in `helpers.js`, the lifecycle routes in
`lib.rs`).

**Graduation path.** This wants to become an automated test once spawning
transient Bram instances is routine. The pieces exist: `bram <path>` runs a
full instance against a scratch project with no single-instance guard, and
`scripts/setup-harness.sh` already automates exactly that shape for Setup
(see *Developing and testing the startup dance* in `developing-bram.md`,
including the `BRAM_TRACE=1` and kill-by-PID disciplines). The unsolved kink
is driving the *pane*: Setup's assertions are all host-route observable,
while most of this walkthrough's expectations are rendered UI (which buttons
are lit, which line shows). Until that's worked out, the host-observable
subset (claim shrinkage, auth retirement, commit splitting, the teardown
trace greps) could graduate first, with the button matrix staying manual.

## What "pass" means

Three claims, all of which must hold:

1. **The buttons are right.** At every step, exactly the actions that are
   legal are offered, and none that aren't.
2. **The granularity choice and the gate explainer appear when needed, say
   the right thing, and stay away otherwise.**
3. **I never get stuck.** No spinner outlives its turn; row selection is
   never left locked; no step requires a `curl` to escape.

Claim 3 is the one that matters most. In the 0.5.2 development sessions the
UI locked three separate times, each because a turn ended holding an
inflight claim, and each needed a manual unwind. Row selection is disabled
while a claim is live, so a stuck claim removes the ability to resolve the
item that is stuck. A wrong button is a papercut; this is a wedge.

## Before you start

- Working tree clean apart from the usual untracked entries.
- Board empty, or note what is on it and leave it alone.
- Devtools console open and **cleared**.
- Note the current commit: `git log --oneline -1`.
- Set `worklist.gateExplainer: true` in `.bram.json` — the explainer line is
  a reserve debugging instrument, default off, and Phases 3, 4 and 6 assert
  its wordings. Remove the key again at teardown.

**Known-acceptable console noise** — do not treat these as failures (list as
of 2026-08-24; re-triage before trusting it):

| message | status |
|---|---|
| `ResizeObserver loop completed with undelivered notifications` | known, #275 A2, unfixed |
| `Failed to load resource … 404` for `components/*.xmlui` | known, upstream xmlui#3822 |

(The `Lifecycle violation on '': async 'unmount' handler…` row retired
2026-08-25: fixed upstream by xmlui#3825, vendored, and verified absent on a
fresh boot where it previously fired within minutes. If it reappears, that
is a finding, not noise.)

**Anything else in the console is a finding.** That is the whole value of
clearing it first.

## The standing check

**After every single action below — every Start, Commit, Drop, Refine —
confirm before doing anything else:**

- the spinner is gone
- row checkboxes are clickable again
- selection is consistent: no row left ticked while the footer offers
  nothing, and no footer count disagreeing with the ticks (the `w2-selection`
  one-way-store bug shape; fixed by store→page sync, 57b0063 — this check is
  its standing soak)

If any fails, stop and record it. That is a claim-3 failure and it outranks
whatever cell you were testing. Recovery, in order: `POST /__worklist/end`
with `{"ids":[...]}`; or `mutate` manually if work really is on disk; or
restart.

## Setup: the probe items

Ask the agent for these six items. The work each does is **append one line
to a scratch file** — nothing more. All files live under `docs/_probe/` so
teardown is one `rm -rf`.

| item | files | why it exists |
|---|---|---|
| `p1` | `docs/_probe/a.md` | isolated |
| `p2` | `docs/_probe/b.md` | isolated; disjoint partner for p1 |
| `p3` | `docs/_probe/a.md` | shares **a** with p1 |
| `p4` | `docs/_probe/b.md`, `docs/_probe/c.md` | shares **b** with p2 |
| `p5` | `docs/_probe/c.md` | shares **c** with p4 |
| `p6` | `docs/_probe/b.md`, `docs/_probe/c.md` | shares **two** files with p4 |

That graph gives disjoint pairs, single-file overlaps, a two-file overlap
(for plural wording), and a chain (p2–p4–p5) where an item overlaps two
different neighbours.

For declared-only entanglement (testing overlap display without starting
anything — e.g. the Review-overlaps hover emphasis), a lighter variant
works: two or three proposed-only items sharing paths, never started,
dropped after. The overlap index counts declared claims from unbegun items.

---

## Phase 1 — one item, nothing begun

**Do:** tick `p1` only.

**Expect:**
- `Start 1` enabled
- `Start & commit 1` enabled
- `Commit` **not** offered
- `Refine` **disabled** (composer empty)
- `Drop 1` enabled
- **No** radio group (needs 2+)
- **No** explainer line (single unbegun item is an unsurprising combo)

**Then:** type anything in the composer. `Refine 1` should enable. Clear it
again; it should disable.

**Then:** expand `p1`'s row. **No Diff surface** — an unbegun item shows
"No changes yet" and nothing else (Diff is gated on the begun predicate,
7458c57).

---

## Phase 2 — start one, and check Start goes away

**Do:** with `p1` ticked, click **Start**. Let the agent append its line and
advance.

**Expect:**
- Spinner clears when the agent finishes *(standing check)*
- Row now reads a "will commit" strip with a real diff count
- Expanding the row: the **Diff surface now renders** the appended line
- Ticking `p1`: **`Commit` enabled, `Start` NOT lit**

The last one is a regression that has happened before — Start lighting
beside Commit on a row that is ready to commit.

---

## Phase 3 — the gate explainer

The line above the buttons is derived from the same predicates that light
them (`__bramStartConsequence`), so it can never narrate an off-screen
action. It renders only with `worklist.gateExplainer: true`.

**Do:** tick `p1` (applied) **and** `p3` (not begun, shares `a.md`).

**Expect** the mixed-selection wording, naming only reachable actions:

> p1 already has changes and p3 has not started, so this selection has no
> joint Start or Commit. Select the started item alone to commit, or the
> proposed one to start it (their edits would then mix).

**Then:** untick and tick `p4` + `p6` (share **two** files, both unbegun —
the Start-available case, so the Start-tense wording):

> These share 2 files. Started together, their edits mix. You can later
> choose to commit together or ask the agent to separate them.

**Then:** tick `p1` + `p2` (mixed, disjoint):

> p1 already has changes and p2 has not started, so this selection has no
> joint Start or Commit. Select the started item alone to commit, or the
> proposed one to start it.

(Not silence: the explainer fires on any begun+unbegun mix, and the buttons
agree — only Refine and Drop are joint actions there.)

*History:* the original line keyed on shared files alone and narrated
unavailable actions twice over ("Started together…" and then "Commit p1
first" beside footers offering neither) — findings #1 of the 2026-08-24 run,
fixed by deriving the line from the button predicates (`2d4530d`). Two
earlier surfaces for the same job (per-id colour badges, a React Flow graph)
failed and were removed; the claimant→row tie is carried by hover emphasis
in the Review-overlaps disclosure instead.

---

## Phase 4 — Commit withheld on an entangled item

**Do:** tick `p3` and click **Start**. Agent appends to `a.md`. Now `p1`
(applied) and `p3` (begun) both claim `a.md`.

**Expect:**
- Tick `p3` alone → **no Commit offered**, and the explainer says why:
  *"p3's changes share a file with p1's, so nothing is exclusively its own
  and Commit is withheld. Select p1 alone to commit first, or ask the agent
  to separate their edits."*
- Tick `p1` alone → **Commit still offered**, no explainer line. `p1` is
  `applied`, and applied items skip the exclusivity check.
- Tick both → still no combined Commit, same explainer line as `p3` alone.

This asymmetry is deliberate but surprising. If `p3` *is* offered Commit,
that is a real finding: committing it would stage the whole file and take
`p1`'s work under `p3`'s id and message.

---

## Phase 5 — the granularity choice, disjoint

**Do:** Start `p2`, let it work. Now `p1` and `p2` are both committable and
disjoint. Tick both.

**Expect:**
- The radio group appears **in the footer above the buttons** (it moved
  there so it cannot sit below the fold)
- Split option reads the plain wording: *"A commit each — selected items
  land separately"*
- No explainer line (disjoint committables are the unsurprising combo)

This is the case that did not work before 0.5.2: the control used to be
gated on entanglement, so the free choice was never offered — two unrelated
items silently went into one commit. *History:* the 2026-08-24 run also
found the split label keyed board-scoped (any selected item sharing with
anyone begun anywhere) instead of selection-internal — finding #2, rekeyed
to `__bramSelectionSharesFiles`.

---

## Phase 6 — the granularity choice, entangled

**Do:** get `p4` and `p6` both to `applied` (Start each, one at a time —
they overlap, so do not start them together). Tick both.

**Expect:**
- Radio group appears (both applied, so exclusivity is skipped)
- Split option reads: *"A commit each — the agent will separate them"*
- Explainer line in its all-begun wording: *"These share 2 files. Both
  already have changes on disk; their edits in shared files mix. You can
  commit together or ask the agent to separate them."*

---

## Phase 7 — granularity actually does what it says

**Do:** with `p1` + `p2` ticked (disjoint), choose **"A commit each"** and
Commit.

**Expect:** **two** commits, one per item, each naming only its own file.
`git log --oneline -3` to confirm.

**Then:** with `p4` + `p6` ticked, choose **"One commit"** and Commit.

**Expect:** **one** commit covering both.

**Worth adding when time allows:** the fourth cell — *entangled* pair with
**"A commit each"** — exercises the split-shared-files pass (the agent
isolates each item's shared-file hunks per commit). First exercised
2026-08-25 on real items (`36c78ee`/`2a240f8`, an entangled pair sharing
`conventions.md`, work delegated to parallel subagents on the disjoint
halves): two clean commits, collision tripwires silent.

If the radio choice is ignored, that is a finding — the control existing is
not the same as it being honoured.

---

## Phase 8 — the three stuck-prone paths

Each of these is a *deliberate* attempt to wedge the UI.

**8a — approved, but nothing to do.** Ask the agent to propose `p7` whose
stated work is already true on disk (e.g. "ensure `docs/_probe/a.md` ends
with a newline" when it already does). Approve it to apply.

**Expect:** the agent reports there was nothing to apply, **retires the
claim** (`POST /__worklist/end` — the designed third outcome, not a
recovery), and recommends Drop. Spinner clears without you doing anything.
*This is the exact case that wedged the UI three times in 0.5.2
development.*

**8b — refine.** Tick any item, type feedback, click **Refine**.

**Expect:** spinner clears when the agent's turn ends. No manual unwind.

**8c — multi-item approve, staggered completion.** Tick two **disjoint**
not-begun items. Start both in one click.

**Expect:** one claim covering both; as each completes, only its own id
retires and the other stays live (`op=clear-shrink` in the trace); spinner
clears fully only when both are done. Neither gets stuck waiting on the
other.

---

## Phase 9 — config off

Set `worklist.gateExplainer` to `false` (the gate's only remaining flag) and
confirm the explainer line disappears while every button behaves unchanged.

*History:* this phase originally tested `worklist.oneClickApproveCommit:
false` and found finding #3 of the 2026-08-24 run — a begun proposed item
went Drop-only because one predicate judged committability with the flag
hardcoded true while the Commit button used the real config. The resolution
was to retire the setting entirely (`1cd9315`): it only ever hid an offer,
never changed authorization, so removing it removed the bug class. The
legacy Workspace tab and its flag went with it. The lesson generalizes: a
config flag consulted by some gate predicates and not others is a dead-end
row waiting to happen.

---

## Phase 10 (optional) — sessions pending row

**Do:** in the Sessions tab, click **+ New session** with a title.

**Expect:**
- A pending row: `[current] <title> — starting, will switch on next
  conversation turn`, size **Pending**
- Rename/delete disabled on the pending row; rename tooltip reads the
  provider-generic "Available after the next message creates the session"
- Send any message → the pending row becomes the real, named, `[current]`
  session (trace: `[session-rotation] op=detected` then
  `[session-new] op=named`)

---

## Teardown

1. Drop any remaining probe items. **For each, the agent must say what
   remains on disk before the drop completes** and propose revert / re-home
   / leave. Silence there is itself a finding.
2. `rm -rf docs/_probe/` — and if probe commits landed (Phase 7), one
   cleanup commit removing the tracked probe files.
3. Remove the `worklist.gateExplainer` key from `.bram.json`.
4. `git status --porcelain` → back to the usual entries.
5. `git log --oneline` → only the commits you intended, nothing else.
6. **Trace greps** — the tripwires turn claim 3 from eyeball into evidence.
   Against `resources/bram-traces/bram-trace.log` (mind log rotation if the
   run spanned a relaunch), scoped to the run's time window:
   - `grep '\[worklist-strip\] op=anomaly'` → **zero** (strip lying about changes)
   - `grep '\[inflight-sentinel\] op=clear-partial'` → **zero** (claim collision)
   - `grep '\[inflight-sentinel\] op=write' | grep prior_ids` → **zero**
     (live claim displaced; a same-ids same-kind rewrite on the drop path is
     benign — read the line before scoring it)
   - `grep '\[auth-record\] op=consume-already-consumed'` → **zero** (double consume)
   - `grep '\[worklist-advance\] op=pending'` → every fire has a matching
     `op=cleared` by teardown
   - Phase 8c corroboration: `grep '\[inflight-sentinel\] op=clear-shrink'`
     → one line per staggered retirement, naming each id

---

## Run record: 2026-08-24 (the 0.5.3 gate)

| phase | pass | notes |
|---|---|---|
| 1 buttons, single unbegun | ✓ | incl. no-Diff-before-begun check |
| 2 Start off when committable | ✓ | Commit lit, Start dim; Diff appeared after Start |
| 3 consequence line + plural | ✓* | finding #1: explainer narrated off-screen actions; rebuilt as predicate-derived (`2d4530d`), held in reserve behind `worklist.gateExplainer` |
| 4 Commit withheld, entangled | ✓ | withheld explainer names the unlock; `p1`-alone asymmetry as documented |
| 5 radio group, disjoint | ✓* | finding #2: split label keyed board-scoped (`SweepsShared`); rekeyed selection-internal, reworded, radio moved to footer |
| 6 radio group, entangled | ✓ | all-begun wording + "agent will separate them" label |
| 7 granularity honoured | ✓ | split: `ebcdece` + `ad3ede7` (1 file each, held-back hunks restored); together: `75d2d37` (2 files) |
| 8a nothing-to-apply unwind | ✓ | claim retired via `/__worklist/end`, Drop clean — the 0.5.2 wedge shape, no wedge |
| 8b iterate clears | ✓ | no-op iterate; turn-end detector cleared |
| 8c staggered multi-claim | ✓ | `/__inflight` shrank `[p5,p8]`→`[p8]`→`{}`; `clear-shrink` traced |
| 9 config off | ✓* | finding #3: one-click-off made a begun proposed item Drop-only; resolved by retiring the setting (`1cd9315`), plus legacy Worklist |
| 10 sessions pending row (opt) | ✓ | Claude side verified live; Codex click deferred |
| — teardown trace greps clean | ✓ | all tripwires zero; lone `prior_ids` write = same-claim drop-sentinel rewrite, not a collision |
| — never stuck, throughout | ✓ | zero manual unwinds all run (8a's `/__worklist/end` was the designed path, not a recovery) |

✓* = passed after an in-run fix; three findings, all fixed and committed
during the run. The last row is the release gate: a failure anywhere above
is a bug to fix; a failure in that row is a reason not to ship.
