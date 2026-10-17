# Bram vendors xmlui#3941 candidate 4 before it is released, and keeps measuring

Status: decided 2026-10-17 · revisit when XMLUI releases #3941

**Question:** Bram Studio vendored two engine fixes from xmlui-org/xmlui#3941: candidate 3 (NavPanel no longer redraws every container below it) and candidate 4 (a refetch sets its loading flags in one write; it also contains candidates 1–3). Do they make Bram's tools pane faster, and if so, should Bram ship an unreleased engine build? judell/bram#444.

**Decision:** vendor candidate 4 now (`app/vendor/xmlui-standalone.umd.js`, sha256 `e4ec32fc9dff05793d7d2ad12ecb68a9d9046d3c7c4305e90825d49f95a85aaf`). Keep measuring with Bram and Bram Studio together on #3941, to take out as much of XMLUI's slack as we can (Jon, 2026-10-17).

**Evidence:** three alternating runs per build, median (min–max).

| | current 0.14.27 | baseline (trace build) | candidate 4 |
|---|---|---|---|
| navigating: busy ms | 4,851 (4,667–5,008) | 5,095 (4,847–5,625) | **2,894** (2,882–3,392) |
| navigating: long tasks | 38 | 41 | **16** |
| navigating: render passes (top-level / all) | no trace | 550 / 1,521 | **259 / 769** |
| idle 75 s: busy ms | 1,556 (1,553–1,676) | 1,660 (1,637–1,671) | **1,278** (1,271–1,314) |
| idle: render passes (top-level / all) | no trace | 45 / 130 | **16 / 56** |

- Navigation busy time is ~40% lower than what Bram shipped, and render passes halve. In the baseline, `ModalDialog` took 136 passes and ~3.8 s per navigation run; on candidate 4 it's no longer among the top components. That fits candidate 3: the pane's dialogs sit in containers under the NavPanel.
- Idle busy time is ~18% lower and idle passes fall from 130 to 56, which fits candidate 4's one-write loading flags. `Worklist` still takes ~1 s of passes per idle stretch on both trace builds.
- Unattributed `App` passes are unchanged (21 navigating, 1 idle); neither candidate targets them.
- No range overlaps between candidate 4 and the other builds.

**Method:** headless Chrome 155 loading a scratch copy of `app/tools` and `app/__shell` from a local server. The server served the build under test as `/__vendor/xmlui-standalone.umd.js` and passed every other route to a demo instance (`scripts/demo-instance.sh new perf444 --from all`, 28 items). Tracing on, cap 50,000; driven over the DevTools protocol after Studio's #3941 runs.

- Script: 7 nav links × 3 rounds at 1.5 s, then 75 s idle on Worklist.
- Busy time is the sum of long tasks (>50 ms). Top-level passes are `render:pass` entries whose parent isn't in the same window.
- Builds, each checked by sha256: current `9d271b4f…0d62`; baseline Studio `2c11457` `63000cb7…2436` (the xmlui#3942 render-trace build from just before any #3941 candidate, needed because 0.14.27 emits no `render:pass`); candidate 4 Studio `1e6e2e5`.
- Current's busy time matches the baseline's, so tracing doesn't skew the comparison.

**Limits:**

- Chrome has no Tauri bridge, so push-event refreshes weren't exercised; navigation, polling and refetches were.
- The baseline (XMLUI `main` `f32497ecb` + trace events) and candidate 4 (`a3429e8` + István's local commits) may differ by other upstream changes.
- One machine, one board, Chrome not WebKit: the relative change is the finding.

**Provenance and risk:** candidate 4 is built from `a3429e8` plus `47c7d3a` and `3c100b0` (local to István) and the candidate 2 and 3 patches attached to #3941 (Studio `1e6e2e5`'s message has their hashes). It is in no XMLUI release. It includes xmlui#3942's render-trace events, which run only while tracing is on (Bram's `xsVerbose`, capped at 200 entries). `app/vendor/xmlui-parser.es.js` is unchanged.

**Alternatives:**

- Wait for an XMLUI release containing #3941. Rejected: the gain is large, and running it in Bram as well as Studio is how the fixes get exercised before release.

**WebKit follow-up (2026-10-17, after vendoring):** measured in Bram's real webview, with push events live, covering the two things headless Chrome couldn't. A demo instance launched from a git worktree, through its own `./bram` symlink, served its own `app/` (checked by planting a marker only in the worktree's copy), so engine swaps never touched the working pane. A driver loaded only there read XMLUI's in-page `render:pass` log; Safari reports no long tasks, so render time stands in for busy time. Baseline (`2c11457`) and candidate 4 alternated, three runs each, each run on a freshly launched demo (`perf444`, 28 items). The commit scenario has two baseline runs; the third timed out before it.

| real webview, median (range) | baseline | candidate 4 |
|---|---|---|
| first visit, 7 tabs: render passes | 596 (584–614) | **303** (293–309) |
| first visit: top-level render ms | 10,285 (7,092–11,197) | **6,341** (3,266–6,463) |
| first visit: time to settle, ms | 7,714 (6,269–8,268) | 6,457 (3,706–6,610) |
| 10 `worklist.json` changes (`worklist-changed`): passes | 2,289 (2,102–2,407) | **1,124** (1,024–1,172) |
| same: top-level render ms | 45,873 (33,005–51,423) | **19,702** (19,467–20,076) |
| 5 empty commits (git events): passes | 1,557 (1,545–1,569) | **790** (658–790) |
| same: top-level render ms | 17,427 (9,837–25,017) | 10,163 (8,709–14,194) |

- Render passes halve in every scenario, with no overlap. Render time drops ~40% on first visits and ~57% per Worklist change, also without overlap. Settle time and commit render time improve, but their ranges overlap.
- **A Worklist change is expensive on both builds:** ~4.6 s of top-level render time per `worklist.json` change on the baseline, ~2 s on candidate 4. Render time can include overlapping or waiting passes, so this isn't a 2 s freeze, but it's the heaviest path measured, and the next target.
- **By feel:** a blind paired test in Jon's own pane (worklist item `blind-test-xmlui-candidate-4`, dropped) scored 2 of 3, which is chance. The gain is real by measurement but not obvious in ordinary use.

**Revisit when:** XMLUI releases #3941 (vendor the release and rerun this measurement), or a regression is traced to this build (go back to `9d271b4f…0d62`, which is 0.14.27).
