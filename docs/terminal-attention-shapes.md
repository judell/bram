# Terminal attention shapes — a trigger × affordance registry

Status: **design, pre-implementation.** This document is the deliverable of
the `issue-270-attention-shape-registry-design` worklist item
(judell/bram#270). It defines the two axes a shape registry needs so
membership is decidable, places today's detectors in the resulting table,
decides the mid-turn question the issue left open, and proposes a v1
decomposition. No product code ships from it.

Issue #270 had three asks. The first — deduplicating the bridge scaffolding
in `helpers.js` — shipped in `c5e71f4` as
`window.__bramMakeTerminalVisibilityBridge` (`app/__shell/helpers.js:7739`,
call sites `:7857` and `:7905`), corrected mid-thread from "one shared
bridge" to "one factory, many isolated instances" — see the issue's first
comment. This document is the other two: the shape registry, and the two
uncovered cells (self-upgrade, mid-turn hooks-trust).

## The one question

*Something is happening in a terminal the user is not looking at, and they
need to know.* Every mechanism below is an answer to that one question, for
one specific "something." There turn out to be **three** existing answers,
not two — the issue's own background section names two ("the compaction
detector and the terminal-attention detector"); reading the code past what
the issue cites surfaces a third, and getting the affordance axis right
depends on not missing it:

1. **`terminal-attention` banner** (`TerminalAttentionTracker`, issue #234,
   `src-tauri/src/lib.rs:5630-5716`) — silence-triggered, one named shape
   (`hooks-trust`). Fires the "The terminal is waiting at a prompt Bram
   can't answer" banner with an **Open terminal** button
   (`app/tools/Main.xmlui:1111-1132`).
2. **The boot-prompt menu** (`codex_trust_prompt_menu_from_chunk` +
   `boot_prompt_commit_decision`, `src-tauri/src/lib.rs:10554-10577` and
   `:10633-10675`) — presence-triggered, scans every PTY chunk for the byte
   string `need review` unconditionally (it runs *before* the
   `bram_menus_parse_enabled()` gate at `lib.rs:10707-10709`, so the
   `menus.parseAndDisplay`-off default does not suppress it). When it
   classifies a full menu (title **and** trust option together — see
   `codex_trust_prompt_menu_from_chunk`'s own test,
   `one_marker_alone_is_not_the_prompt`, `lib.rs:~10624`) it arms a pending
   menu; on the next tick, if the provider is Codex, no turn is `working`,
   and no menu is already displayed (`boot_prompt_commit_decision`,
   `lib.rs:10563-10577`), it commits into `TurnState.pending_menu`, which
   the pane renders as a clickable option list wired to `sendKeys`.
3. **`compaction` banner** (`CompactionTracker`, issue #268,
   `src-tauri/src/lib.rs:6343-6420`) — presence-triggered on
   `stripped_tail.contains("Compacting conversation")`
   (`compaction_progress_shape`, `lib.rs:5908-5910`), rendered as an
   informational banner with a **✕ dismiss**
   (`app/tools/Main.xmlui:1138-1163`). Its *lifecycle* completion is not
   read from the PTY at all — Codex repaints the compaction line, so raw
   text presence fired 16 times for one real compaction — it is read from a
   structured `compacted` record in the session JSONL instead, with a
   30-second fallback bound for CLI versions that write none
   (`COMPACTION_PENDING_MS`, `lib.rs:5901`).

**Correction to the issue's own table.** The issue's trigger×affordance grid
puts "hooks-trust at launch" in the silence-triggered × answerable cell.
That conflates (1) and (2): the *answerable* affordance — click resolves it
without leaving the pane — belongs entirely to the presence-triggered
boot-prompt menu (2). The silence-triggered banner (1) is strictly
notify-only; its only affordance is **Open terminal**, and
`app/tools/Main.xmlui:1105-1108` says why explicitly: "No dismiss button by
design." The two mechanisms happen to answer the same real-world event (a
Codex hooks-trust prompt at session boot) through different sources, and
the richer affordance is not a property of the *shape* "hooks-trust" — it's
a property of *which detector reached it first*. That is precisely the
ambiguity a registry has to resolve, not paper over.

## The two axes

A registry whose membership rule is "the shapes we already had" is a
rename. The two axes below are the actual contribution: each is defined so
that a person proposing a *new* candidate condition can answer, without
guessing, which cell it lands in.

### Axis 1 — Trigger

A trigger has two parts: the **condition** observed, and the **source** it
is observed from. Three sources exist in the codebase today, each with a
different reliability/latency profile:

| source | what it is | reliability | latency |
|---|---|---|---|
| **PTY text presence** | the live, rolling PTY tail (`pty_tail_cell()`) contains a marker string *right now* | high precision when the marker requires multiple co-located fragments (the boot menu's title+option pair); low precision on a single loose marker — text an agent merely *discusses* can match (see `HOOKS_TRUST_MARKERS`, `lib.rs:5602-5608`, which is an OR over four independent single markers) | near-instant — evaluated on every chunk that has `bytes > 0` |
| **PTY text absence (silence)** | no PTY bytes for ≥ a threshold, then the *stale* tail is classified once per silence episode | depends entirely on the presence classifier it hands off to at the end of the wait; adds a genuine "is anything moving" signal presence-alone lacks | bounded by the threshold (`TERMINAL_ATTENTION_SILENT_MS = 10_000`, `lib.rs:5581`) plus the fact that it only evaluates once per episode (`episode_evaluated`, `lib.rs:5641`) |
| **JSONL structured record** | a parsed entry from the provider's session transcript file (e.g. Codex's `compacted` record) | highest precision — it's the provider's own authoritative lifecycle marker, not a screen-scrape | can lag the PTY by real seconds, and does not exist on every provider version (`compaction`'s own comment: "an old Codex CLI that writes no `compacted` record", `lib.rs:5895-5897`) — this is *why* compaction still needs the 30s fallback bound rather than waiting on JSONL alone |

Every existing detector also carries a **gate** — a host turn-state
precondition that can suppress the trigger from committing regardless of
what the source shows. This is a separate concept from the trigger itself,
and conflating them is what made the mid-turn gap easy to miss:

- (1) gates on `!authoritative_open` (`lib.rs:5678`) — `phase_open
  (working|waiting-for-permission) && turn_stamp non-empty &&
  last_jsonl_activity_at_ms > last_completion_at_ms`
  (`lib.rs:16326-16332`).
- (2) gates on `!working && !menu_present`
  (`boot_prompt_commit_decision`, `lib.rs:10563-10577`) — a differently
  *named* but functionally equivalent "no turn is open" precondition.
- (3) has no gate — it fires regardless of turn state, because compaction
  is something Codex does mid-turn by definition.

**Decidability test for Trigger:** a candidate shape names (a) which of the
three sources it reads, (b) the exact condition on that source, and (c)
whether any host turn-state gate suppresses it and under what predicate. If
a candidate cannot name a source with a stated reliability/latency
profile — "the agent seems stuck" is not a source — it does not belong in
the registry yet; it belongs in the `op=candidate` observe-only inventory
first (the pattern `terminal_attention_prompt_shape`'s own `None` branch
already uses, `lib.rs:5615, 5711-5714`), per the Log-first development
discipline (`docs/developing-bram.md`).

### Axis 2 — Affordance

What the user can actually do about the fired condition, ordered by how
much it does for them:

| affordance | what it means | who has it today |
|---|---|---|
| **nothing** | informational only, no control offered — the condition self-clears | *(no current shape; self-upgrade is a candidate — see below)* |
| **dismiss** | a ✕ that suppresses the current episode; a later episode re-fires | compaction (`window.__bramDismissCompaction`, `helpers.js:7910`) |
| **switch to terminal** | a button that reveals the terminal pane so the user can act there themselves | terminal-attention's `hooks-trust` banner (`window.__bramOpenTerminalForAttention`, `helpers.js:7863`) |
| **answer inline** | the pane renders the actual choice (a menu, a text field) and resolves it without the user leaving the agent pane | the boot-prompt menu (`pendingMenu` → click → `sendKeys`) |

**Decidability test for Affordance:** ask what the user's *next physical
action* would be if the banner fired right now. If it's "read this and do
nothing," → nothing. If it's "I don't need to see this again this
episode," → dismiss. If it's "I need to go type something into the
terminal," → switch to terminal. If the answer set is small, enumerable,
and safe to render as a menu, → answer inline is *available*, but see the
security caveat below before assigning it.

**The affordance ceiling is a safety judgment, not a UI convenience.**
Answer-inline is strictly more capable than switch-to-terminal, so it is
tempting to treat it as the target every shape should eventually reach.
The issue's own "Open question" section pushes back on that for
`hooks-trust` specifically: "Trust all and continue" grants hooks the
ability to run code outside the sandbox, so a mis-click there has real
blast radius, unlike a mis-click on compaction's dismiss. That tradeoff is
per-shape, which is exactly why Affordance is a registry *attribute* and
not a fixed pipeline stage every shape climbs.

## The table

| shape | trigger source | condition | gate | affordance | status |
|---|---|---|---|---|---|
| `hooks-trust` (banner) | PTY silence → presence classify | ≥10s byte-silence, then stale tail matches any of `HOOKS_TRUST_MARKERS` | `!authoritative_open` | switch to terminal | **shipped** (#234) |
| `hooks-trust` (boot menu) | PTY presence (chunk-level) | chunk contains `need review`; title+option pair required | `!working && !menu_present` | answer inline | **shipped** (undated, predates #270) |
| `compaction` | PTY presence (banner) + JSONL record (lifecycle) | tail contains `Compacting conversation`; `compacted` record or 30s fallback ends the episode | none | dismiss | **shipped** (#268) |
| `self-upgrade` | *(none wired)* | candidate text captured live 2026-08-22 18:17:29: `Updating Codex via npm install -g @openai/codex... ⠙ ⠹ ⠸` | *(undecided)* | nothing / dismiss | **GAP** |
| `hooks-trust` (mid-turn) | *(both existing sources blocked)* | the prompt Codex shows precisely when a hook is about to run — i.e. precisely mid-turn | blocked twice: (1)'s `!authoritative_open` **and** (2)'s `!working` | *(none reachable today)* | **GAP** |

### Filling `self-upgrade`

What would have to be true: a marker string(s) over PTY text presence,
same shape as `compaction_progress_shape`. The one live specimen
(`Updating Codex via npm install`) only reached the candidate inventory
because it went quiet with spinner frames still in the tail — confirming
it is presence-triggered like compaction, not silence-triggered like
hooks-trust: `terminal-attention`'s silence gate *structurally cannot see
it while it runs*, because byte-silence never holds during an active
spinner. Two things are still open and neither is answered by code
already in the tree:

- **Affordance.** There is nothing for the user to do about a self-upgrade
  in progress — it is autonomous, like compaction. That argues for
  `dismiss` (matching compaction's palette and behavior) over `nothing`
  (no control at all) mostly as a UX-consistency call, not a technical
  one; either is defensible and this document does not decide it.
- **Completion signal.** Compaction learned the hard way that PTY text
  presence is not a reliable *off* edge (the repaint problem, above).
  Nothing in the codebase suggests what a reliable self-upgrade
  completion signal would be — is there a JSONL record, an exit code, a
  version-string change in a later prompt? This needs its own specimen
  capture (the `op=candidate` observe-only phase, run until at least one
  full episode is caught start-to-finish) before a `Complete` transition
  can be designed at all. Filling this cell is therefore **two-phase**:
  observe-only first, banner second — same shape as compaction's own
  history.

### Filling `hooks-trust` (mid-turn)

See the decision below — this is the question the task asked to be
settled, not left as a gap description.

## The mid-turn question, decided

**Decision: make the silence-tracker's early return (`lib.rs:5678`)
conditional, scoped narrowly — and explicitly do NOT touch the boot-menu's
separate `!working` guard (`boot_prompt_commit_decision`,
`lib.rs:10563-10577`) in this pass.**

### Why the guard exists, stated precisely

`TerminalAttentionTracker::step`'s early return isn't there to protect
against false *positives* on its own — the prompt-shape classifier already
requires marker text, not just silence. Its job, per its own comment
(`lib.rs:5674-5677`), is to stop silence from **accruing** across an open
turn: normal turns contain long stretches where the agent is thinking and
prints nothing, and none of that silence should count toward "look, a
stuck prompt." Left unconditional, an ordinary 12-second thinking pause
mid-turn would reach the 10s threshold and hand the stale tail to the
classifier — and the classifier is the weak link that makes this
dangerous to just unblock:

### The concrete risk the single-marker classifier creates

`terminal_attention_prompt_shape` fires on **any one** of four independent
markers (`lib.rs:5602-5608`) — `"hooks need review"`, `"hook needs
review"`, `"trust all and continue"`, `"press enter to confirm or esc to
go back"` — matched as a simple substring, case-insensitively, with no
requirement that two markers co-occur. The boot-menu detector, by
contrast, explicitly guards against exactly this failure mode: its own
test is named `one_marker_alone_is_not_the_prompt`
(`lib.rs:~10624-10628`) and asserts that quoting a single phrase
("we saw hooks need review yesterday") must NOT classify.

That distinction matters concretely here: **this very document** contains
the strings "Trust all and continue," "Press enter to confirm or esc to go
back," and "hooks need review," multiple times each, because it is a
design doc *about* the hooks-trust prompt. Any agent turn that discusses
this mechanism in its own PTY output — explaining the prompt, quoting the
marker list, walking through this issue — prints one of these markers,
then very plausibly goes quiet for 10+ seconds while formulating the next
sentence. Unblocking the guard with the *existing* single-marker
classifier would make Bram's own PTY chatter about this issue a live
trigger for a false "the terminal is waiting at a prompt Bram can't
answer" banner, mid-turn, on the very issue that describes the bug.

### The condition, specified precisely

Do not remove the guard. Replace the blanket `if authoritative_open {
return }` with a per-shape predicate: **a shape may accrue silence toward
a fire during `authoritative_open` only if it declares itself
`mid_turn_eligible: true` AND its classifier, when invoked under
`authoritative_open`, uses the stricter co-occurrence rule the boot-menu
already implements** — title marker and trust-option marker present
together in the same tail window, not any single marker. Concretely:

- Add a second classification path,
  `terminal_attention_prompt_shape_strict`, requiring both a title-class
  marker (`"hooks need review"` / `"hook needs review"`) **and** an
  option-class marker (`"trust all and continue"` /
  `"press enter to confirm or esc to go back"`) to be present in the same
  stale tail — mirroring `codex_trust_prompt_menu_from_chunk`'s own
  precedent rather than inventing a new rule.
- In `TerminalAttentionTracker::step`, when `authoritative_open` is true,
  still zero `silent_ms` (turns still shouldn't accrue silence from
  ordinary mid-turn pauses) — **except** skip the early return and run the
  strict classifier once per tick specifically to check "does the current
  tail already look like a live hooks-trust prompt right now," rather than
  waiting out a silence episode. This is a presence check layered onto
  the silence tracker for exactly one shape, not a general unblocking of
  silence accrual — the risk above is about silence accruing across
  narrated chatter, and a same-tick presence check with the strict
  two-marker rule does not have that failure mode, because the two
  markers must be co-located the way they only are in the real Codex
  prompt paint (`lib.rs:10614`'s live specimen), not in prose describing
  it across separate sentences.
- Affordance for this cell stays **switch to terminal**, matching the
  existing banner (`hooks-trust`'s non-boot instance), not answer-inline.
  Reaching answer-inline mid-turn would additionally require relaxing
  `boot_prompt_commit_decision`'s `!working` guard, which is the security
  question the issue's "Open question" section raised and explicitly
  declined to settle ("Recording the question, not deciding it"). This
  document inherits that non-decision rather than overturning it: a wrong
  click on **switch to terminal** costs the user a pane switch; a wrong
  click on **answer inline** for a hooks-trust prompt grants sandbox
  escape. The two guards look identical in code (`!authoritative_open` vs
  `!working`) but they gate affordances with different blast radii, and
  only one of the two is being loosened here.

### Consequence if this is left unimplemented

Recorded so the question is not re-litigated from scratch: the guard as
it stands means the one prompt class that **must** be answered for Codex
to make any further progress (hooks-trust) is the one class that cannot
raise attention while a turn is open — precisely the gap #270 exists to
close. A user whose Codex session is silently blocked on a mid-turn
re-trust prompt gets no banner, no candidate trace line (the tail is never
examined — `authoritative_open` returns before `tail_fn()` is ever
called), and no `op=candidate` evidence that anything happened. That's the
"any negative result... is weaker than it looks" caveat from the issue
body, restated: it is not just weak, it is currently unfalsifiable from
the trace alone.

## v1 decomposition

Each item below is independently gateable and small enough to reject on
its own without blocking the others.

1. **Registry data shape (mechanical, no behavior change).** Introduce a
   `AttentionShape` struct — `id`, `trigger: {Presence, Silence}`,
   `source: {PtyText, JsonlRecord}`, `gate: Option<fn(&TurnState) -> bool>`,
   `affordance: {Nothing, Dismiss, SwitchToTerminal, AnswerInline}` — and
   migrate `terminal_attention_prompt_shape`'s single `hooks-trust` return
   into one static table entry. Behavior-identical; the win is that the
   *next* shape is a table row, not a new function. Small, reviewable in
   one sitting, no trace changes needed since nothing observable moves.

2. **Self-upgrade, observe-only.** Add `self_upgrade_progress_shape`
   (presence, PTY text, same shape as `compaction_progress_shape`) wired
   only to an `op=candidate`-style trace line — no banner, no `Fire`
   transition, per the Log-first "observe-only first for behavior
   changes" discipline. Graduation criterion: at least one full
   self-upgrade episode captured start-to-finish in trace, so item 3 below
   has a real completion signal to design against instead of guessing.

3. **Self-upgrade banner.** Gated on item 2's soak producing a usable
   completion signal. Wires the `Fire`/`Complete` transitions and the
   `dismiss`-affordance banner (reusing the existing bridge factory — one
   more `__bramMakeTerminalVisibilityBridge` spec, one more Main.xmlui
   `VStack`). Rejectable independently of items 4–5 if the completion
   signal never stabilizes; ships as observe-only indefinitely in that
   case, which is itself an acceptable outcome per the tripwire-vs-soak
   distinction in `docs/developing-bram.md`.

4. **Strict mid-turn classifier.** Add
   `terminal_attention_prompt_shape_strict` (title marker AND option
   marker co-located) as its own function with its own test — modeled
   directly on `codex_trust_prompt_menu_from_chunk`'s
   `one_marker_alone_is_not_the_prompt` precedent — with **no** wiring
   into `step` yet. Reviewable purely as a text-classification unit,
   verifiable against the same live specimens already pinned in
   `terminal_attention_tests` (`lib.rs:5799-5825`) plus at least one
   negative specimen built from this very document's own prose.

5. **Wire the mid-turn presence check.** Change
   `TerminalAttentionTracker::step` to run item 4's strict classifier on
   the current tail when `authoritative_open` is true, per the precise
   condition specified above, firing the same `switch to terminal`
   banner. Depends on item 4; independent of items 2–3. This is the one
   item that changes production behavior for the `hooks-trust` shape and
   should be soaked (Log-first: does the trace show `op=fire` only on
   genuine mid-turn prompts, zero fires during ordinary narrated-chatter
   turns) before being called done — the synthetic-testbed method
   (`docs/developing-bram.md`) is the natural way to drive this: script a
   Codex turn that discusses hooks-trust in its own output, confirm no
   fire, then reproduce a real mid-turn prompt per the issue's documented
   repro (delete `trusted_hash` from `~/.codex/config.toml`) and confirm a
   fire.

6. **Not in this v1 — flagged, not scheduled.** Relaxing
   `boot_prompt_commit_decision`'s `!working` guard to reach
   answer-inline mid-turn. This is the issue's own open security question,
   restated in the Affordance section above; it is a bigger decision than
   the registry mechanics and should be its own worklist item with its
   own explicit security review, not a rider on this one.

## What this document could not determine

- Whether `self-upgrade` should render as `nothing` or `dismiss` — no
  code or trace precedent settles this; it's a UX-consistency call for
  whoever picks up item 3.
- What a reliable self-upgrade *completion* signal is (JSONL record, exit
  code, or something else) — genuinely unknown until item 2's soak
  produces a captured episode. Nothing in `lib.rs` or the issue thread
  names one.
- Whether `authoritative_open`'s `waiting-for-permission` phase value ever
  itself gets set for a hooks-trust boot-menu commit (i.e., whether
  committing the boot menu could retroactively start blocking the silence
  banner on the very episode it just handled). Tracing the exact
  interaction between `pending_menu` and `phase` transitions further was
  out of scope for this pass; worth a trace check (`grep
  '\[terminal-attention\]\|\[pty-menu\]'`) before implementing item 5.
