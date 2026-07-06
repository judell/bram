# PTY menu tools — analysis & cataloging toolkit

**What this is:** the catalog of scripts we use to capture, classify, and
analyze interactive-menu evidence from Bram's PTY traces. It is the
companion to the *data* docs:

- `pty-menu-shapes.md` — the curated shape catalog (one row per shape, by detection axis).
- `pty-menu-specimens/` — the raw specimen corpus (one file per observation).
- `pty-menu-tunables.md` — the detector knobs.

This file catalogs the **tools**, not the data. It will grow as we build
more; add a row + a section when you add a script. Keep tools in
`scripts/`; describe them here.

## Source of evidence

All tools read the `[pty-menu-scan]` trace lines in
`resources/bram-traces/bram-trace.log` (and rotated `bram-trace-*.log`).
Each scan line carries the detection axes (`cursor`, `header`,
`numbered`, `needle2_after_anchor`, `anchor_distance_ok`, …), `op=fire` |
`op=skip`, `menu_bearing=`, and — on fires and menu-bearing skips — a
stripped `excerpt='…'`.

## Catalog

| Tool | Question it answers | Granularity | Output |
| --- | --- | --- | --- |
| `pty-menu-scan-report.py` | **RETIRED** — its data source (`[pty-menu-scan]` lines) was deleted in delete-phase tranche 2 (#214, 2026-07-06); useful only against pre-deletion rotated logs. *Original question: which menus did we cover / miss / not recognize?* | per-excerpt | covered/missed/unknown classification; `--write-specimens` drafts specimen files |
| `pty-menu-timeline.py` | *Do menus interfere across time (succession hazards)?* | per-menu **episode** (temporal) | episode timeline + DISTINCT-SHAPE-MISS / SAME-SHAPE-SKIP-TAIL flags |

Supporting trace infrastructure (general, not menu-specific):

| Tool | Role |
| --- | --- |
| `record-trace.sh` | capture a trace run |
| `normalize-trace.py` | canonicalize a trace for stable diffing |
| `diff-trace.sh` | compare two traces |

---

## `pty-menu-scan-report.py`

Classifies individual menu observations against the current catalog
anchors. The **machine intake** half of the specimen pipeline (see
`pty-menu-specimens/README.md`).

- **Reads:** `[pty-menu-scan]` excerpts (default: live `bram-trace.log`).
- **Emits:** counts of *covered* catalog shapes (`op=fire`), *missed*
  shapes (`op=skip menu_bearing=true`), and *unknown* menu-bearing skips.
- **`--write-specimens`:** writes missed/unknown excerpts as draft
  specimen files under `docs/pty-menu-specimens/`.
- **Use when:** triaging a session for new or regressed shapes; feeding
  the specimen corpus.
- **Caveat:** it can't tell a real menu from agent text *about* menus —
  prose/commands containing menu literals show up as false missed/unknown
  (see the *self-reference collision* note in `pty-menu-shapes.md`).

## `pty-menu-timeline.py`

Reconstructs the temporal sequence the per-excerpt report can't see.
Groups scan lines into per-menu **episodes** (new episode on identity
change or a >2.5 s gap), marks each FIRED vs skip-only, and flags two
succession hazards:

- **DISTINCT-SHAPE MISS** — a real menu that never fired, appearing ≤12 s
  after a different menu that did. (None observed across all logs as of
  2026-06-22.)
- **SAME-SHAPE SKIP-TAIL** — a fired episode with a sustained
  menu-bearing skip tail after its last fire — the case where two
  same-shape menus back-to-back would otherwise merge and hide the
  second.

- **Usage:** `scripts/pty-menu-timeline.py` (live), `--all` (every
  rotated log), or explicit paths.
- **Use when:** investigating whether menus interfere with each other in
  time (e.g. "is a second menu dropped when it follows the first
  quickly?").
- **Hard limit (important):** the scanner samples at ~200-300 ms. A menu
  shown and dismissed faster than one scan interval leaves **no frame**
  (neither fire nor skip) and is invisible to this — and to any
  log-mining tool. **Zero candidates ≠ no sub-cadence miss.** To probe
  that regime, capture a **live deliberate repro**, not history.

---

## Conventions for adding a tool

1. Put the script in `scripts/`, with a module docstring stating the
   question it answers and its known limits.
2. Add a row to the **Catalog** table and a short section here.
3. If the tool feeds the specimen corpus, cross-reference
   `pty-menu-specimens/README.md`.
4. State the instrument's blind spots explicitly — a trace tool's
   resolution and its false-positive sources are part of its contract.
