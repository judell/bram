# Bram reference: diagnostics

Seeded by Bram Setup as `.claude/bram-reference/diagnostics.md`. Read
the relevant section when a spinner sticks, something misbehaves, you
are designing a mechanism, or you are citing evidence.

Bram internals, for diagnosing Bram itself (source-repo docs, not
seeded):

- Route and file-shape reference for the inflight sentinel:
  https://github.com/judell/bram/blob/main/docs/apis.md (§11)
- Registered trace categories, subkinds and fields:
  https://github.com/judell/bram/blob/main/docs/trace-vocabulary.md

## Inflight sentinel failure modes

The Worklist spinner is keyed to `resources/.inflight-claim.json`,
which host-side HTTP handlers write and clear. A stuck spinner is the
convention enforcing itself; there is no arbitrary live-session
timeout. Bram does have host-side completion detectors that can clear a
lingering claim without a cooperative agent tail call: Claude session
JSONL `stop_reason:"end_turn"`, Codex session JSONL `task_complete`, PTY
silence, and explicit cancellation paths. Most commonly:

- **Approved/drop stuck:** `mutate` was never called, or errored before
  the clear — or the turn ended with nothing to apply, where no `mutate`
  was ever correct to call. Recovery, in order: `POST /__worklist/end`
  with `{"ids": [...]}` naming the claimed ids (the route is not
  iterate-specific); or call `mutate` manually if the work really is on
  disk; or restart Bram (`cleanup_stale_inflight_claim` runs at startup),
  which is the heaviest option and ends any agent session running inside
  it.
- **Item-feedback (`iterate:`) stuck:** rare now that the host auto-detects the `iterate:`
  prefix and the turn-finished clearer fires for all sentinel kinds. If
  it does stick, host-side completion detectors clear it on the next
  normal turn end; `/__worklist/end` remains available as an explicit
  manual unwind. It returns `{"ok":true,"cleared":<bool>,"remaining":[...]}`
  — `cleared:false` with a non-empty `remaining` means the call resolved
  only part of a multi-id claim, not that it failed (see
  `worklist-mechanics.md` §Incremental claim and authorization
  retirement).
- **Premature clear:** silence alone is not authoritative. PTY silence
  can request a sentinel clear, but the host first checks the latest
  provider JSONL completion detector. If JSONL says the assistant turn
  is still non-final, the host logs
  `[agent-status] op=skip-sentinel-clear ... reason=jsonl-non-final` and
  leaves the sentinel intact. If a premature clear is suspected, inspect
  `[agent-status] op=skip-sentinel-clear`, `[jsonl-turn-end]`, and
  `[inflight-sentinel]` in `bram-trace.log`. Missing/unreadable JSONL
  falls back to the legacy silence-clear behavior.

The Status tab's Inflight Sentinel section includes a `Turn completion`
row. Use it first when diagnosing a stuck spinner: it reports the last
detector source, provider, skip/detect reason, timestamp, and whether
the observed completion happened after the active claim.

Do not conflate this with XMLUI component-local busy states. APICall
spinners/buttons are driven by the APICall component's `inProgress`
state and lifecycle handlers; Worklist spinners are driven by Bram's
host-managed inflight sentinel. XMLUI fixes to delayed APICall
`onSuccess` cleanup (such as xmlui-org/xmlui#3540) don't replace the host
turn-completion detector needed for approved/drop/iterate worklist
cycles, which are sent through `toTurn` and cleared through
`/__inflight` plus host lifecycle events.

## Log-first development

Agents default to writing and reading code; in Bram the higher-value
habit is writing and reading logs. Behavior here arises from the
interplay of Rust, the parent shell, XMLUI, two agent CLIs, and
Markdown/Python-governed workflow — runtime questions ("was the right
message sent at the right time? did the transition fire? did it
render?") are answered by evidence, not inspection. The norms:

- **The drill.** When behavior goes wrong — or a new mechanism is being
  designed — the first question is: does the trace already capture what
  happened? If no, add the instrumentation (as its own worklist item
  when scope warrants) and keep dogfooding until the problem recurs; the
  next occurrence should be self-diagnosing. If yes, use it before
  theorizing. A fix proposed without trace evidence should say so
  explicitly.
- **Observe-only first for behavior changes.** Mechanisms that will act
  on inferred conditions (auto-clears, auto-reveals, suppressors) ship
  first as trace lines only, with graduation criteria written into the
  worklist draft as falsifiable checks against the soak ("every would-X
  corresponds to a corroborated moment; zero fire during Y"). The design
  review is a grep.
- **Distinguish soak observers from tripwires — they graduate
  differently, and applying the wrong shape retires a working
  instrument.** A *soak observer* fires during normal operation, so a
  soak accumulates positive instances and the criteria above apply as
  written. A *tripwire* fires only when a rule is violated, and the rule
  usually exists precisely to prevent the condition — so correct
  operation reads zero indefinitely, and zero is **success, not absent
  evidence**. Its graduation question is not "did it fire" but "is the
  condition reachable, and would a fire be actionable?", settled by
  reasoning about the mechanism rather than by waiting. The
  inflight-claim collision instruments are the type case (a concept
  name, not a grep target: they emit under `[inflight-sentinel]` and
  `[auth-record]`).

  The trap: **a tripwire's zero and a dead instrument's zero are
  identical in a grep.** So a tripwire needs a *provenance* check in
  place of a soak — a deliberate violation in a test, or a review
  confirming the emit is wired and the path reachable. A third zero is
  possible: an instrument documented under a name it never emits, whose
  grep matches nothing while it fires correctly. A name that can't be
  grepped is a provenance check that silently fails.
- **Baselines are commits.** Perf work starts with an instrumentation
  commit that records the before, and the same trace line verifies the
  after. Numbers in commit messages come from the trace, not from
  estimates.
- **Logs cannot prove absence.** Event-shaped logging proves presence
  only: a missing line means "nothing flushed", not "nothing happened"
  (the `[pty-in]` small-read accumulator is the canonical trap). Any
  claim of the form "X never happens" requires an instrument that
  affirmatively records zeros with a denominator — per-turn gap
  distributions are the pattern.
- **Exhaust on-disk evidence before declaring anything unverifiable.**
  Before writing "can't test from here" / "needs separate
  investigation", enumerate what already exists: rotated
  `bram-traces/bram-trace-*.log` archives (days of history, not just the
  live log), `git log`/`git blame`, Inspector exports, persisted tool
  results.
- **Local absence is not disproof.** For a bug reported from another
  machine or user, "not in my repo/history/disk" is expected for
  machine-specific artifacts and proves nothing about the remote case.
  Verify the mechanism locally; frame the specifics as "can't be checked
  from here", never as discrediting the report.
- **Register new subkinds.** Every new trace op or subkind lands in
  Bram's trace vocabulary (link above) in the same change that
  introduces it, so the reading half keeps pace with the writing half.

## Debugging Bram itself

Three forensics surfaces, used together. The first two are raw
streams; the third is a dashboard that derives signals from them.

**`resources/bram-traces/bram-trace.log`** — host-side rolling log of
HTTP routes, iframe events, and inflight-sentinel writes / clears.
**On by default**; switch it off per project through **Settings →
Traces**, and `BRAM_TRACE` in the environment overrides the project
setting either way. Grep it directly when enabled. PTY previews and
serialized iframe payloads use Bram's `loomweave-scanner`-backed
credential redactor before persistence; Bram adds narrow structural
expansion for complete PEM blocks and Authorization/assignment values.
Redaction is defense in depth, not a guarantee for arbitrary content. At
startup the prior active log is archived. A background pass sanitizes
and gzips raw archives older than `traces.archiveAfterDays` (default 14
days, configurable from 1–3650), removing each raw source only after its
`.log.gz` replacement has been fully written, synced, and atomically
installed. Compressed history is retained indefinitely with no byte
cap, so trace storage is intentionally unbounded. The active log is
never an archive candidate during its session. Best for plumbing: stuck
spinner, sentinel anomalies, route errors, agent-turn-end detection,
heartbeat drift, close-cycle verification (`grep "[issue-close-queue]
op=closed" resources/bram-traces/bram-trace.log` — one line per issue
the host auto-closed after a Push; absence around a known close
timestamp means the commit hadn't reached the default branch yet, or no
close was queued at the commit gate; see also
`op=retired-already-closed` and `op=closed-via-pr`).

**Inspector Export** — XMLUI runtime trace (events, state changes,
handler invocations) for Bram's own XMLUI UI, captured on demand. Best
for in-pane misbehavior: a button doesn't fire, a DataSource shows wrong
data, a state change doesn't propagate, a component renders wrong. Ask
the user to open the Inspector (magnifying-glass icon), reproduce, then
click **Export** — writes `~/Downloads/xs-trace-<timestamp>.json`.
Analyze with the xmlui MCP tools:

- **`xmlui_find_trace`** — locate the export by timestamp or content.
- **`xmlui_distill_trace`** — reduce to interactions / state changes /
  handler boundaries relevant to a specific question.

Don't read the raw JSON initially, it's huge; only grep as necessary.

**Status tab** — curated dashboard in the agent pane that surfaces
signals derived from `bram-trace.log` (rotated history included) and
from Inspector exports, alongside live process state. Sections include
Startup Run, Worklist, Inflight Sentinel, Hooks, Authorization, Latest
Tail And Fanout, and Guards/Staleness/Interrupts/Traces. Check the
Status tab first for a quick read on whether something looks off — then
drop down to `bram-trace.log` or an Inspector Export for the underlying
detail.

## Coordinating MCP demand with search

When the question is "which how-to is missing, weak, or wrong?" for an
MCP-served project (XMLUI is the live one), treat MCP analytics as a
**nomination source**, never as proof. The analytics log is global
across projects and outcome-blind: a search always returns something,
`result_count` does not measure usefulness, and clock proximity cannot
establish that a project transcript caused or followed a query.

Validate a nominated topic with the project's `GET /__search` index
across sessions, commits, issues, and worklist history. Look for what
happened after the search: a how-to read that resolved it, repeated
reformulation followed by component or source fallback, a workaround
committed, an issue filed, or a later conclusion that reversed the
first one. Then check the **current** `xmlui_list_howto` /
`xmlui_search_howto` corpus before calling anything a gap.

Classify the result instead of forcing every weak search into "missing
how-to": it may be a missing recipe, a discoverability problem, an
inaccurate reference page, an engine bug that needs a reproducer, a
contradiction, or an already-fixed gap. Operator test phrases and
unrelated global MCP activity are noise unless indexed project memory
independently corroborates them. File an upstream issue only when that
evidence chain survives the current-corpus check, and include the
followable search, session, commit, and documentation receipts.

Use `scripts/xmlui-howto-gap-miner.py` (in the Bram source repo) for a
repeatable first pass. It groups nearby `xmlui_search_howto` calls only
when at least two meaningful terms overlap, consolidates high-similarity
recurrences across dates, attaches only locally-near
component/example/source fallbacks, and nominates a compact query using
recurrence plus corpus-wide term rarity. It asks `GET /__search` for
downstream project evidence and reconciles the result against the
current how-to directory. Its classification is a review hint, not a
verdict; read the returned snippets and current-doc matches before
filing anything. Typical use from the Bram repo:

```
python3 scripts/xmlui-howto-gap-miner.py --since 2026-06-01 --top 20
python3 scripts/xmlui-howto-gap-miner.py --since 2026-06-01 --json
```

The tool deliberately does not score `result_count`, parse
provider-specific transcripts, or correlate frustration by clock time.
`--analytics`, `--howto-dir`, `--search-url`, and `--port-file` make each
input explicit; defaults target the local XMLUI analytics/cache,
`~/xmlui` how-tos, and this project's `resources/.bram-port`.

## Citing evidence and the search-wins ledger

When an issue comment, ledger entry, or postmortem cites evidence, make
the references followable:

- **Commits**: link the full-SHA forge URL
  (`https://<forge>/<owner>/<repo>/commit/<full-sha>`), displayed as the
  short SHA — bare short SHAs in backticks do **not** autolink on
  GitHub. Resolve with `git rev-parse <short>`.
- **Issues**: bare `#N` autolinks; use it.
- **Local-only sources** (session transcripts, worklist-history
  records) have no web URL. Name them by path or key, and where a
  runnable query helps the reader, include the `/__search` query (`q` /
  `mode` / `types`) that finds them — a distinctive phrase in `phrase`
  mode pinpoints; broad AND queries only retrieve.

**The search-wins ledger.** Keep a ledger issue in the project that
collects receipts for the claim that agent + indexed project memory
changes the work (Bram's own is judell/bram#233, which defines the entry
format). Capture rule: record an entry **at the moment** a live question
is answered by the index and something materially changes (a plan
killed or redirected, prior art recovered, duplicate work prevented).
Routine lookups don't qualify; entries are never reconstructed later.
Ledgers are **project-local by design** — receipts carry project
specifics, so they belong in the project's own tracker. Sharing
standout entries upstream (to judell/bram#233) is a per-entry choice by
the project's humans, never an automatic behavior.

Worklist drafts follow the same rule: **plans cite inline in their
prose** (typed, followable refs — `code:path:line`, commit SHAs, issue
numbers, doc URLs, pinpointing `/__search` queries). Do not author
`resources/worklist-citations/<id>.json` files; that plumbing is
dormant. For handing a user a runnable query from the pane, use Search
deep-linking: `navigate('/search', { queryParams: { q, mode, types } })`.

## Guards, retired hooks, and bundled skills

Bram internals, for diagnosing Bram itself.

The guards are Rust, compiled into Bram itself: the policy lives in
`src-tauri/src/guard_policy.rs` and the hook plumbing (dispatch, menu
POSTs, fail-closed fault handling, breadcrumbs) in
`src-tauri/src/guard.rs`. Hook registrations invoke `~/.bram/bram-guard`
(`bram-guard.exe` on Windows), a link Bram installs at startup and
re-ensures on a ticker, pointing at the dedicated `bram-guard` binary
built beside the app (GUI subsystem on Windows in every profile, so hook
spawns never flash a conhost). There are no script files to edit or
keep in sync — editing a guard is editing the Rust source, and
validating it is `cargo build` + relaunch, which refreshes the link's
target. Registration strings carry a bare `guard <hook>` subcommand; a
`--authority` token from the transition era is still accepted and
ignored.

The retired Python hook scripts (`.claude/hooks/claude-*.py` in managed
projects, `~/.bram/codex-*.py` user-globally) are deleted by Setup once
**no settings source** references them — project `settings.json`,
project `settings.local.json`, or the user-global
`~/.claude/settings.json` for the Claude pair, `~/.codex/config.toml`
for the Codex pair (the same reference-gated prune that covers the
older generic names). Bram never rewrites the global files: a stale
reference there holds the deletion back and is named in the Setup
result instead. A lingering script is surfaced by the Status tab's
"Retired Python hooks" rows.

In the Bram source repo, `app/shell/` holds shell-launch support only
(`claude-code-shellrc`, `claude-code-profile.ps1` configure the shell
that launches the agent CLI; `codex-startup-instructions.md` is startup
text injected into Codex sessions) — no hook adapters.
`app/provider-hooks/` is gone with the Python guards it held.

**Bram-bundled skills** follow a canonical/installed split:
`app/skills/<name>/SKILL.md` in the Bram source is canonical; Setup
seeds it into each managed project's `.claude/skills/<name>/SKILL.md`,
and `build.rs` syncs the source repo's own installed copy (`loose-ends`
is the first member; new Bram-blessed skills drop into `app/skills/` and
ride the same path). Ownership is marker-based: Setup refreshes only
files carrying the `<!-- bram-managed -->` marker line — a same-named
skill without it is user-owned, never clobbered, and reported as
skipped in the Setup result. Skill staleness is deliberately NOT part
of the needs-setup banner; refresh is best-effort at Setup time. This
is a Claude-only surface (Codex has no skills concept). Do not make
functional edits in an installed `.claude/skills/` copy of a bundled
skill; edit `app/skills/<name>/SKILL.md` in the Bram source.
