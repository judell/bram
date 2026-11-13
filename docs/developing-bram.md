# Developing Bram

Guidance that applies **only when editing Bram's own source** — the
agent pane's XMLUI, `app/__shell/helpers.js`, `app/tools/Globals.xs`,
and the Rust shell. This file is `@`-imported by the source repo's
`CLAUDE.md` and referenced from its `AGENTS.md`; it is **never seeded
into managed projects** (their sessions get `.claude/bram-conventions.md`
only). The audience test for what belongs here vs. there: *would this
text change an agent's behavior in a project that is not the bram
repo?* If yes, it belongs in `app/__shell/conventions.md`.

The rules below are agent-neutral: they bind Claude, Codex, and any
future provider editing this repo.

For the authorization policy, shipped checks, intentional exceptions,
and known gaps, start with [authorization.md](authorization.md). A change
to authorization must update its action matrix, the affected API entry,
and the seeded operational instructions together, naming the enforcement
point and evidence. Do not leave a managed project's instructions
dependent on this repo's unseeded `docs/` directory.

For the architectural map of Bram's Git-backed mechanisms—temporary
indexes, claim and safety refs, selective commits, linked worktrees,
scenario branches, and remote-tracking state—see
[`git-as-infrastructure.md`](git-as-infrastructure.md).

When a choice between alternatives gets settled (with evidence) and
would otherwise be re-argued later, record it as a short note in
[`decisions/`](decisions/README.md) instead of an issue: one file per
decision, with its evidence, the alternatives rejected, and when to
revisit.

## Code organization (helpers.js / Globals.xs / window)

Iframe-side code spans four surfaces. The rules below describe where
each kind of code should live, and how XMLUI markup calls into it.

### The surfaces

- **`app/__shell/helpers.js`** — real JavaScript. Async, `fetch`,
  `setTimeout`, `postMessage`, tauri event listeners — anything the
  XMLUI expression engine can't host directly. Functions live on
  `window` (see naming below) and are reached from XMLUI markup as
  `window.foo(...)`. Rebuild-required; see *Build vs. hot-reload
  boundary*.
- **`app/tools/Globals.xs`** — XMLUI's expression engine context.
  Holds xs-scope module state (vars whose readers/writers all live in
  xs) and the few helpers whose proximity to that state earns them a
  place here. Engine restrictions: no async/await, no setTimeout, no
  fetch, no Promise chaining outside DataSource. Top-level
  `function foo(...)` declarations auto-hoist onto `window.foo` —
  but that binding is engine-scoped: it makes `foo` bare-callable
  from XMLUI attribute expressions and lets an xs name shadow a
  helpers.js export, yet the function is NOT reliably reachable as
  `window.foo(...)` from real-JS contexts in helpers.js. Live
  receipt (2026-08-20): a helpers.js click path calling
  `window.initCloseIssueState(...)` failed with "is not a function"
  though the xs declaration existed; the fix was a self-contained
  helpers.js replica. Code that must be callable from both sides
  lives in helpers.js, never in Globals.xs.
- **`window.*`** — the shared namespace. helpers.js writes here
  explicitly; `Globals.xs` writes here implicitly via hoisting. The
  `__bram*` prefix exists to give helpers.js a collision-safe space
  when an xs-side counterpart of the same name would otherwise hoist
  over it.
- **`.xmlui` files** — markup. Attribute handlers (`onClick`,
  `onDidChange`, `onLoaded`, etc.) and binding expressions
  (`value="{...}"`, `when="{...}"`) are tiny expressions, not
  hosting environments for code.

### Where each kind of code goes

- **Pure functions** (sync, no XMLUI component state, no
  engine-hostile primitives) → `window.foo` in `helpers.js`. XMLUI
  markup calls them as `window.foo(...)`.
- **Shims for outside-sandbox operations** (async, fetch, setTimeout,
  postMessage, tauri events) → also `window.foo` in `helpers.js`,
  because the engine can't host them. Markup calls them as
  `window.foo(...)`.
- **xs-only code** → `Globals.xs`, but only when the function
  genuinely needs xs (touches xs-scope module state directly, or is a
  very hot binding-string callee where the `window.` prefix is
  measurably annoying enough to justify the cost).
- **XMLUI attribute handlers** → a single function call:
  `onClick="window.foo(...)"` (or `onClick="foo(...)"` if `foo` is an
  xs function). Never multi-statement bodies, never multi-line arrow
  bodies, never object-literal blobs. See *Failure modes* below.

### When and why do we need delegators?

A *delegator* is `function foo(...) { return window.__bramFoo(...); }`
in `Globals.xs`. Its only purpose is to let XMLUI markup write the
bare name `foo(...)` instead of `window.__bramFoo(...)`.

**Default: don't add one.** Call helpers as `window.foo(...)` from
XMLUI markup. This includes inside arrow-function bodies passed to
`subscribeTauriEvent` / `onDidChange` / `onLoaded` etc. — the engine
analyzes the *qualified* `window.foo` member access without trouble.

**Add a delegator only when** (a) the function is called many times
in attribute expressions where the seven-character `window.` prefix
is genuinely annoying, and (b) the name doesn't already exist on the
bare `window` surface. Each delegator we add hoists `function foo`
onto `window.foo`, expanding the collision-prone surface — the
exchange rate has to be worth it.

The `Globals.xs` of today has zero delegators — the fossil set from a
prior model was pared away during the host-route migrations. The rule
above governs whether any new one earns its place.

### The `__bram*` namespace prefix

`__bramFoo` on `window` defends a helpers.js export against being
clobbered by a `function foo` declaration in `Globals.xs` (which
would auto-hoist onto `window.foo`). It is **not** a blanket rule
for every helpers.js name — bare-name window helpers
(`toShell`, `toTurn`, `logToHost`, `openExternal`, `sendKeys`,
`captureScreenshot`, etc.) are fine as long as no `Globals.xs`
declaration shadows them.

The discipline:

- If a name has a matching `Globals.xs` delegator → name the helper
  `window.__bramFoo`. The delegator body is
  `return window.__bramFoo(...)`; no collision.
- If a name lives only in `helpers.js` → bare `window.foo` is fine.
  No prefix required.

### Failure modes that informed these rules

Learned from real incidents; each is a hard rule, not a preference:

- **Attribute expressions stay a single function call.**
  Multi-statement / multi-arrow-body / object-literal blobs in
  handler attributes are the anti-pattern that produced the
  hour-long "parser quirk" hunts. When an XMLUI surface throws a
  weird error and the markup has an inline ternary / `&&` chain /
  multi-statement arrow — the bug is the inline expression, not the
  parser. Extract to a `window.foo` helper first.
- **Bare names inside arrow bodies silently abort analysis.** In an
  arrow body passed to `subscribeTauriEvent` / `onDidChange` /
  `onLoaded`, a bare `foo` with no xs declaration silently kills the
  registration AND every statement after it in the same handler —
  the symptom surfaces as an unrelated component failing to mount.
  Call qualified: `window.foo()`. (Top-level attribute positions
  tolerate bare names; arrow bodies are the trap.)
- **xs `function foo` hoists over `window.foo`.** helpers.js loads
  first; a same-named xs declaration then clobbers the helper with
  the xs-bound version. If the xs function was a delegator calling
  `window.foo(...)`, it now calls itself — infinite recursion,
  swallowed silently by trace try/catch, presenting as hung
  handlers. Fix: name the helper `window.__bramFoo` (when an xs
  delegator exists) or remove the xs declaration entirely.
- **helpers.js top-level *calls* must follow their definitions.**
  The file runs top-to-bottom; a load-time throw
  (`window.X is not a function`) aborts everything after it,
  breaking features unrelated to the edit (menu, voice,
  talk-session). Function *definitions* referencing later names are
  fine; top-level *invocations* are not.
- **`ExpandableItem` expansion state is uncontrolled and positional.**
  Inside an `Items` loop, when the list shrinks (a Worklist prune),
  the component instances are reused by position, so an expansion
  opened on row 3 silently transfers to whatever item now occupies
  row 3. Fix: controlled expansion keyed by item id — a `when`-gated
  body plus an explicit chevron/header toggle writing an id-keyed
  map (the Worklist rows are the live pattern). Filed upstream as an
  ask; until then, never rely on `ExpandableItem`'s own state in
  loops whose membership changes.

### Post-edit verification ritual

After ANY iframe-side change (`.xmlui`, `Globals.xs`, `helpers.js`):
grep `console-error|console-unhandledrejection` in
`resources/bram-traces/bram-trace.log` once the pane has reloaded.
Zero matches is the pass condition; any match is triaged before
reading anything else from the trace.

#### Known noise: the ResizeObserver loop

One message is exempt:
`ResizeObserver loop completed with undelivered notifications.` Exclude
it from the grep (`| grep -v ResizeObserver`) and don't triage it.
Everything else still counts.

It is a known XMLUI-side warning that Bram can't fix. It comes from
[virtua](https://github.com/inokawa/virtua), the virtualizer under XMLUI's
`List`: its per-row item resizers measure in the same frame they observe.
Virtualized scrolling alone produces it, with no Bram markup involved.
The record:

- #275 §A2 measured it at about 204 a day, all from virtualized `List`s,
  and traced it to Bram's initial scaffold (`b4abf0f`).
- [xmlui-org/xmlui#3830](https://github.com/xmlui-org/xmlui/issues/3830)
  was closed as wontfix: a bare React + virtua fixture with no XMLUI code
  reproduces it, on virtua 0.48.8 and 0.52.7 alike.
- We have not filed it on virtua. Jon doesn't know their policy on
  AI-assisted contributions and won't present this work as his own.

This keeps coming back. It reappeared during the `issue-425-retire-replay`
demo check on 2026-10-12: 20 lines in the demo's trace, and 142 in the
working instance's trace that day, starting before any pane edit.

**Removing this exemption.** XMLUI's
`xmlui/tests/isolation/virtua-ro/` specs
([`f6369d9`](https://github.com/xmlui-org/xmlui/commit/f6369d98cc1917edc6acd13c2f970f300a1c9184))
are marked `test.fail()`, so they turn red when a virtua release fixes the
loop. When that lands and Bram's vendored XMLUI picks it up, delete this
subsection and the pointer to it in `worklist-gate-walkthrough.md`.

For the registered trace categories, subkinds, fields, and their diagnostic
purpose, consult the on-demand
[trace vocabulary](trace-vocabulary.md) while reading the log.

Why this is non-negotiable: the xs engine **silently rejects
assignments to member expressions** from inside function bodies it
evaluates — `window.X = value` in a Globals.xs function fails with a
scope error that only the trace sees, while the calling pipeline
just stops. (Top-level `window.X = ...` statements in Globals.xs are
fine — the file loader parses those, not the expression engine.)
Workaround: define the setter in helpers.js (real JS) and call it as
a function from xs.

### Peer-pattern check before designing

Before introducing a new mechanism in the pane, grep 2–3 peer
components for how they handle the same shape of problem, and run
`xmlui_search_howto` for the operative concept. If every other
reactive surface uses the same pattern and the misbehaving component
hand-rolls something different, the outlier is almost certainly
where the bug lives — refit to the canonical pattern before adding
instrumentation.

## Warning color is earned

Use the warning color only when a nontechnical user can clearly
understand the problem and can easily act on it (Jon, 2026-11-10). A
warning also says the action in plain words ("Push again from the
Commits tab", "Try the switch again from Sessions"); a line that can't
name one isn't a warning. Detail only an agent or a developer can use
stays in the trace, not on the surface.

The receipt: the Log once colored a commit line orange for "lines no
item owned … that's disclosed, not hidden, but worth a look". Nobody
could tell what it meant or do anything about it. The clause is gone,
and the Log's eight warning sites were checked against this rule: two
kept the color (a failed push, a switch that didn't start), and the
rest went plain.

**The same rule governs the Status tab** (Jon, 2026-11-12: "There's no point
in warning me without offering an action"):

- A `warn` or `error` row carries an `action` in plain words ("Run Setup",
  "Relaunch Bram", "Rebuild Bram (make build-debug)"); the pane shows it under
  the detail.
- A signal with no action for the user reports `none`, shown as **N/A**. Its
  detail stays in the row's text and the trace. N/A means "no verdict", not
  "fine".
- `ok` means a known good state, never "nothing wrong that we noticed".
- The host enforces it: `enforce_status_actions` demotes any warn/error row
  without an action to `none` (and traces `[status] op=warn-without-action`),
  and `status_rows_missing_action` is what the unit tests assert on. Add the
  `action` where the row is built.
- Compare like with like: the `.claude/bram-reference` check in the source
  repo compares against the unstamped source bytes, because `build.rs` writes
  them unstamped; comparing against Setup's stamped seed made it warn forever
  about a condition that wasn't real.

## Rust: typed data, not hand-read JSON

Data inside the host is typed (Jon, 2026-11-11, a standing rule):

- **Structs for shapes.** `#[derive(Serialize, Deserialize)]`, with
  `#[serde(rename_all = "camelCase")]` to keep the wire shape and
  `#[serde(default)]` for optional fields. Not a `serde_json::Value` read
  one field at a time (`v.get("kind").and_then(|v| v.as_str())…`,
  `v["offer"].as_bool()`).
- **Enums for closed sets of codes** (outcomes, obstacles, kinds), so the
  compiler rejects an unknown value and every `match` is checked.
- **`Value` only where the shape isn't ours,** such as a provider's
  transcript record or a CLI's output. Build JSON only where a route
  answers.
- **A Tauri command takes the typed struct as its parameter.**
- **When a struct replaces a `Value`, keep the wire shape byte-identical,**
  and pin it with a test, because the pane and other tests read it. Serde's
  error text differs from hand-written errors, so check whether anything
  matches the old text.

New code follows the rule. Old code converts when an item touches it, not
in sweeps. The receipt: `queue_pty_intent_inner` spends about 40 lines
extracting five fields that one derived struct and an enum replace. On
2026-11-11, `lib.rs` had about 960 such field reads and 101 files parsed to
an untyped `Value`, against 16 parsed to a struct. The audit is on
[#487](https://github.com/judell/bram/issues/487).

## Client storage: decision vs. focus

The pane's browser storage splits into two classes, and picking the wrong
one is a recurring bug shape, not a style nit — file each new persisted key
under the right one before writing it.

- **decision** (persists across sessions) → `localStorage`. A choice the
  user would be annoyed to re-make: sort order, a dismissed-forever notice,
  an opt-in/opt-out toggle.
- **focus** (dies with the session) → `sessionStorage`. Where the user
  *was*: scroll position, which row was expanded, an in-progress draft, a
  pending operation.

**Default to focus when unsure.** The failure modes are asymmetric. A
wrongly-session-scoped decision costs the user one re-click next launch —
annoying, self-correcting, over in a second. A wrongly-persisted focus key
resurrects stale state long after its context is gone, and it does not
present as a storage mistake — it presents as a bug in whatever surface it
resurfaces in (a pane that opens pre-expanded to rows nobody asked about, a
banner that can never be seen again, an operation that appears to silently
replay). The reader debugging that surface has no reason to suspect
`localStorage` first.

**The test:** ask whether the value describes what the user is *looking at
right now* (focus — session) or what the user *decided, once, going
forward* (decision — persisted). If the answer would still make sense
read aloud a week later without the session that wrote it ("I want the
newest-first sort", "I've dismissed this notice") it's a decision. If it
would sound stale or confusing read aloud later ("row 3 is expanded",
"there's a pending delete") it's focus.

**Worked example, and why this is written down rather than left to
intuition:** commit `30033d8` moved `bram.updateBannerDismissedVersion`
from `localStorage` to `sessionStorage` — the reasoning comment sits at
`app/__shell/helpers.js` around lines 12047–12061. The X on the update
banner reads like a dismissal, which is the decision shape, and issue
#271's own audit table had guessed exactly that. The considered answer was
the opposite: the X means "not now, this session," and persisting it made
a single click a permanent, cross-instance forfeiture of the update
prompt with no undo short of editing storage by hand. A key that looks
like a decision at a glance can still be focus once you ask what the user
meant by the click — that's the whole reason to run the test explicitly
instead of pattern-matching the key's name.

When moving a key between stores, check whether the old value needs
clearing: `localStorage` and `sessionStorage` are separate stores under
the same origin, so a stale value left in the old store is never read
again once every read/write site points at the new one — it is inert
clutter, not a shadowing hazard. Commits `6fa9fd8` and `30033d8` both
relied on exactly this and did no migration; do the same unless a
specific read site still reaches into the old store (grep for it — don't
assume).

### Scope persisted keys by project

`localStorage` is **one store shared by every Bram window** on the machine.
It's one app (`org.xmlui.bram`) with one WebKit store, whichever project each
window serves. So every persisted key needs a second question after decision
vs. focus: **is it about this project, or about the user?**

- **About the user** (a preference that holds in any project, such as
  `bram.turnCompleteBeep` or a dismissed notice): an unscoped key is right.
- **About this project** (its saved tab, the unsent message-box draft, and
  anything naming its items, sessions or files): suffix the key with the
  project. Use `window.__bramProjectScopedKey(base)` in `helpers.js`, which
  returns `base + ":" + projectKey`, or null when the project isn't known. On
  null, neither read nor write: never fall back to the shared slot. `main.js`
  sets `window.__bramProjectKey` (from `/__app-info`) before the pane loads,
  so the helper works synchronously.
- **Don't migrate an unscoped project key's old value** unless you know which
  project wrote it. A one-shot migration hands the value to whichever project
  reads first, and that is the leak itself.

The receipts:

- #279: the saved tab. Two windows fought over one slot.
- `message-draft-bleeds-across-bram-windows`, 2026-09-29: a draft dictated
  and sent in `~/bram-studio` reappeared unsent in `~/bram`'s box after a
  relaunch, and went to the wrong agent.
- The draft is also the case #271's audit singled out as "must survive"
  (unsent prose the user can't redo). That's why it stays in `localStorage`,
  scoped, rather than moving to `sessionStorage`. That move was tried first,
  and it would have dropped drafts on every relaunch.

The unscoped keys were audited against this rule on 2026-10-31. Two project
keys turned up, `bram.worklistSubmittedMessage` and `…Kind`, but nothing read
them, so they were deleted, not scoped. When a key looks misfiled, check first
that something reads it. `bram.dismissedAgentIds` stays unscoped: agent ids are
unique per dispatch, so windows only share its cap of 500.

## The orientation site's engine lives in the template

Bram's own orientation site (`orientation/`) is a copy of the template in
`app/__shell/reference/orientation-template/`, like every other repo's site.
The template is the source; nothing flows from a site back to it
(settled 2026-11-03, `orientation-scaffolding-managed-by-setup`).

- **Engine files:** `arch.js` and `Main.xmlui` carry a
  `bram-managed: orientation engine` marker. Setup finds every directory in a
  project whose `arch.js` carries it, wherever it lives (code-exam's is
  `docs/orientation/`), and refreshes both files from the template, keeping
  the site's `SITE` / `ARCHITECTURE` block and its `PageMetaTitle`. Unmarked
  files are never touched. Setup traces one
  `op=orientation-scaffolding path=… outcome=…` line per file it finds.
- **Here too:** the refresh runs on the source-repo path as well, which
  otherwise skips seeded docs and skills, so `orientation/` here is refreshed
  like any site.
- **The loop for an engine change:** edit the template, run Setup
  (`GET /__enhance/run?force=true` on the loopback port, or the Setup
  button), reload `orientation/` on the preview server, and commit the
  template and the refreshed `orientation/` files in one item. Editing
  `orientation/arch.js` or `Main.xmlui` directly is lost on the next Setup.
- **Content** (`OVERVIEW.md`, `architecture/*`, `screenshots/`, the `SITE`
  block, every `index.html`) is the site's own, here and everywhere.

## Push over polling

Do NOT add `pollIntervalInSeconds` to XMLUI DataSources for
freshness. Drive refetch from events or actions, by tier:

- **Local action** (e.g. posting a comment): bump a reactive var the
  DataSource depends on (a `refreshTick` in queryParams), or call
  `.refetch()` in the action's `onSuccess` — refresh exactly when it
  changed, not on a timer.
- **Cross-component / host state**: subscribe with a `PushSource` on
  `window.bramSubscribeTauriEvent('<event>')` (`git-status-changed`,
  `worklist-changed`, `talk-session-changed`, …) and refetch on its
  tick — the established pattern across the pane.
- **Remote state with no filesystem trigger** (e.g. forge issue edits
  by others): the poll does NOT move to the client and is NOT
  replaced by a manual Refresh button. Relocate it to the Rust host —
  a background thread polls, computes a result signature, and
  synthesizes a Tauri event (`issues-changed` is the precedent) only
  on change; the client subscribes and refetches. The host often
  piggybacks on work it already does (the search indexer fetches
  issues anyway).

## Codex synchronized terminal redraws

Codex brackets full-screen paints with DEC synchronized-output mode 2026
(`CSI ? 2026 h` / `CSI ? 2026 l`). Bram's vendored xterm.js 5.x predates
support for that mode, so `app/main.js` coalesces PTY frames and delivers a
complete synchronized paint to xterm in one write. The hold is bounded at
100 ms: a missing closing marker fails open instead of wedging the terminal.

When changing the PTY write path, replay these three shapes against the
coalescer before hand-testing a real Codex resume: begin/content/end in one
frame, markers split across frames, and a begin with no end. The first two
must flush atomically; the last must flush on the deadline. Then check
`resources/bram-traces/bram-trace.log` for `terminal-write-batch` slow lines
and `xterm-liveness` stalls. Do not replace this with xterm.js 6.0 solely for
mode-2026 support: upstream's current implementation still has an open
ED2-inside-sync viewport-yank bug affecting Codex and Claude.

## Perf diagnosis ordering

**Timeline first, semantic probes second.** When the pane feels slow,
record a browser Timeline (Safari Web Inspector → Timelines, Frames
view) before reaching for Bram's semantic instruments. The Timeline's
Script / Layout / Paint split is the cheap first partition and names
the fix class directly: Layout-dominated frames point at DOM size and
forced reflow (virtualize, bound the region), Script-dominated frames
point at evaluation work — and only then is the eval-trace probe
(which binding? which handler?) the right drill.

Receipt: the 2026-07-31 search-typing-lag hunt fixed three script-side
layers (describe pacing, projection tail-scoping, eval confinement)
before a Timeline recording named Layout as the felt cost —
virtualization (69be17a) was the fix a day-one recording would have
named immediately. The probe still earned its keep by falsifying the
eval theory decisively, and it remains irreplaceable where Timelines
can't go (hard freezes, remote machines, semantic attribution) — but
the ordering lesson stands: category attribution before semantic
attribution.

### Standing health check: `scripts/ui-health.py`

Run it periodically, and whenever the window feels slow. It's read-only and
needs no instance:

```sh
python3 -I scripts/ui-health.py                          # the live trace
python3 -I scripts/ui-health.py --trend 2026-10-27 2026-10-28   # one row per day
```

It reports the trace's responsiveness gauges for a window:

- the pane: heartbeat drift by focus state, long tasks, input latency by route
- the parent: `xterm-liveness` stalls and slow `terminal-write-batch` writes
- transcript projection times
- the slowest host routes

It also splits spikes by whether an agent turn was running. For each idle
spike, it lists the host emits and routes in the 2.5 seconds before it. That
correlation is the cheap first step: it names *what else was happening*.
When it names nothing obvious, record a Timeline (above) to find out *where*
the main thread's time went.

The baseline on Jon's machine:

- **After the 2026-07-22 fix** (`250503cf`): heartbeat avg 6–14 ms, max
  81–141 ms, no spikes.
- **July to October daily trend:** 0.3–3 focused spikes over 1 s per active
  hour, input-latency p50 104–185 ms.

A run well above that is a finding. Record the before and after with this
script, so a fix's claim has numbers.

The first standing run (2026-10-29) found 89 spikes in 23 minutes, against
4 and 0 on either side. The idle-spike column named `subagents-changed` →
`/__agents` for all 31 idle spikes. The writes came from another project's
subagents: the watcher took any `subagents/` path under
`~/.claude/projects/` as this project's (`foreign-subagent-writes-refetch-roster`).
A real subagent run in this project still costs up to a second per roster
refetch (the 2026-10-28 04:00 hour had 141 spikes). That's open, and it
starts with a Timeline.

### The story of a stretch: the Log

The **Log** tab (`app/tools/components/Log.xmlui`) narrates what Bram did
around the work. Rust selects lines from the trace and `hook-events.log`
through `/__story/lines`; JavaScript (`__bramStoryTell` in
`app/__shell/helpers.js`) turns them into episodes; XMLUI displays them.
Every sentence comes from a fixed template over trace fields, with no model
call. When a signal is missing, the narrator says so rather than guessing.
A wording change needs a pane reload.

The original Python narrator and its standalone replay CLI were retired on
2026-11-07 (`retire-python-narrator`). Its `--follow`, `--since`, `--until`,
`--cite`, and `--root` commands are no longer available. The Log is the
narration surface, and JavaScript is the sole maintained narrator; it has no
Python runtime dependency.

It shows the last five hours, across relaunches: each launch rotates the
trace, so the host reads the rotated archives as well as the live file. It
keeps what it selected in memory, reads each archive once, and reads the
live file only from where it stopped, so a refetch costs what the trace
gained since the last one. The pane names the kinds it shows (`kinds=`) and
gets only the lines those are told from. It refreshes when a turn starts or
ends, and when the host sees a shown kind happen (`story-changed`, debounced:
a commit, push, search, queue change, selection restore, gate click, or a
session change or failed agent launch).
`[story] op=lines` records each request's cost. Before this
(log-reads-across-trace-rotation, 2026-11-07), a relaunch blanked the Log,
and every refetch re-read 32 MB and sent 2.6 MB to show 13 entries. Every
entry expands, on a click anywhere on its sentence, to the agent and session
it happened in and the board as it stood then (`/__worklist-history/at`).
An entry's time links into the Transcript at that moment
(`/transcript?at=<ms>`, landing on the first turn that began at or after it,
from each turn's `ts`) when it happened in the session the Transcript shows. A pushed commit's sha links to the
forge, and a push's range to its compare view; a commit no remote has yet
links to the Commits tab's local view (`/commits?sha=`), since the forge would
404 on it.
What reaches it is opt-in (`STORY_SHOW` in `helpers.js`, Jon's 2026-11-05 rule:
show nothing until asked): queue messages, selection changes, gate clicks,
pushes, your searches, and demo instances starting and stopping
(`scripts/demo_instance.py` appends `[demo-instance]` lines to this checkout's
trace), as of 2026-11-07. Session switches joined them the same day
(log-tells-session-switches). A switch is told when the session actually
changes (an `[agent-switch] op=live-session-set` naming a different session),
and the ops just before it say why: `op=start` (another agent, with its
`resume_reason`), `op=fresh-partner`, `op=autostart` (the resume at launch),
or `[session-new] op=create`. A set with none of those is the Sessions tab or
the agent itself, and is told as that. A switch expands to from → to instead
of the single context line. Session names aren't linked: the Sessions tab has
no per-session deep link. Every other kind (turns, stalls,
refusals, the launch) is told but held back until someone asks for it; adding
one is a one-word change there, plus its lines in `story_kind_wants`
(`lib.rs`); a kind the host doesn't know yet gets every line, so it still
tells correctly, only less cheaply. Item names in a line are links to the
item's History entry (`/history?entry=<id>`), so the sentence is rendered as
Markdown. A gate line names the click and what landed; it doesn't recite the
approval, the claim and the hand-off, which every click does and the row's
spinner shows live. A Start & commit waits ten minutes for its commit, a
Commit one; a line told before its deadline says the commit hasn't landed
yet rather than that it never did. The Log also includes
quoted openings of what you sent, and what each turn did ("ran 1 search, made
3 edits"), counted from the Transcript projection by order, and left out
when the order doesn't line up. Change the voice in `helpers.js`.

### Does the pane slow with age, or with session size?

`scripts/pane-age-health.py` answers that from the trace and the agent's
session files. It reports projection p50/p90 and focused spikes over 1 s per
active hour, by session size, by pane age, by active minutes since load
(minutes with a focused drift line or a working turn, so a pane left idle
overnight stops looking old), and as a size × age cross-tab. A pane load is
the first `GET __app-info` after a 10 s gap. The session file's size at each
tick is rebuilt from its own line timestamps, since the file is append-only.

```sh
python3 -I scripts/pane-age-health.py                                  # archives since 2026-10-15
python3 -I scripts/pane-age-health.py --since 2026-11-01
python3 -I scripts/pane-age-health.py --build-since 2026-11-04T04:06:00Z   # every table, before and after
```

The baseline, from this script on 2026-11-04 (archives since 2026-10-15: 21
sessions, 373 pane loads), runs 19, 63, 112 and 121 spikes per active hour
at wall-clock pane ages of 0–0.5 h, 0.5–1 h, 1–2 h and 2–4 h. By active
hours since load it runs 19, 94 and 127 over the first two hours, then
falls to 33 for 2–4 h, so the climb comes from use, not elapsed time.
Session size barely moves it: 19–82 per hour across the size bins, with no
trend, and projection p50 stays within 93–139 ms. (The scratch run of
2026-11-03 that prompted the script read 20, 74, 121 and 123.) So a
sluggish pane is fixed by a reload, not a new session. Start a new
session for the agent's reasons. Use `--build-since` to score a pane change
against the age curve.

### What the agents keep running: the Tasks report

judell/bram#483 asks for a Tasks feature: a repeatable command, once it
appears in a session, saved to a file the project owns (a Makefile, a
`dodo.py`) so it runs the same way every time and CI can run it without
Bram. Before building any of that we shipped the measurement
(`issue-483-task-candidates-report`, 2026-11-09): **More › Tasks** in the
pane, `GET /__tasks/candidates`, and `bram --task-candidates <root>` for a
headless run. It reads every provider's sessions for the project, takes
each shell tool call, sets aside Bram's own protocol traffic (loopback
curls, state-folder paths), git, forge CLI calls and shell plumbing, and
clusters what's left by recipe. The route documents the shape in
`apis.md`; the exclusion counts are shown, not hidden, so the filter can
be judged from any project's numbers.

The pane's words are for a reader new to repos (Jon: "joyce will not
have any idea what a runner is"): it says *commands the agents keep
running*, *ways typed*, and *a file of saved commands*, never "runner",
"target" or "recipe". Each repeated spelling (the unit a saved command
would name: `ffmpeg … -af ebur128 …` is a task, `ffmpeg` isn't) carries
a placeholder name from the host (`ffmpeg-ebur128`, `cargo-test-lib`),
shown read-only under a note saying so. The real name is the agent's to
propose when a task is saved, from the context the command ran in, the
way it names Worklist items (Jon, 2026-11-09), and is editable then. The
report names single commands: a compound line is split into steps before
clustering, so a repeated *sequence* of commands (build, stop, launch) is
not seen as one thing. That gap is the part of #483 Make's dependencies
address, and it is open.

**Bram's own baseline (2026-11-09, 424 sessions, 43,585 shell commands,
96,281 steps):** 15,592 steps talked to Bram, 10,654 were git, 2,255
forge, 62,533 shell plumbing. 159 commands recur three times or more;
the top of the list:

| runs | sessions | ways typed | command |
|---|---|---|---|
| 826 | 89 | 8+ | `cargo test` |
| 687 | 35 | 8+ | `scripts/demo-instance.sh` |
| 562 | 84 | 8+ | `cargo fmt` |
| 473 | 57 | 8+ | `node --check` |
| 350 | 49 | 8+ | `cargo build` |
| 309 | 57 | 4 | `cargo check` |
| 93 | 17 | 8+ | `scripts/build.sh` |

Two readings. `cargo build` outruns `scripts/build.sh` almost four to
one, though this file has said "build through `scripts/build.sh`" since
#452: the documented recipe loses to the one the agent already knows, and
every one of those plain builds left no record in `build-log.jsonl`. And
the test command has at least eight spellings (bare, `--manifest-path`,
`--lib`, shell-variable paths, an absolute worktree path). Zero runs of
`cargo build --release`: a simple rule holds, a recipe doesn't. This repo
has 46 scripts and no file of saved commands; the recipes in this document
are the task list, in prose. Bram is the first case for #483.

The same CLI on `~/bram-studio` (13 sessions, 4,138 commands, 57
candidates) put `ffmpeg` first (349 runs, 5 sessions, 8+ spellings), then
`ffprobe` (84), `serve_media.py` (77) and `whisper-cli` (26): a project's
own compositions, with Bram protocol filtered out. Bare words such as
`post` and `band` there are shell functions the agent defined in-session;
the report can't see their bodies and leaves them as they are. That is what the report is for in a project
that isn't Bram.

**The page prioritizes; the JSON keeps everything** (Jon, 2026-11-09,
after reading the xmlui and bram-studio pages: "the output is pretty
overwhelming. how can we help people prioritize?"). Three kinds of row
crowded out the ones that matter: commands run many times in a single
session (half or more of every list: one afternoon's loop, which a saved
command doesn't fix), one-word shell functions the agent defined
in-session (27 of bram-studio's 57 rows), and helpers left in scratch or
temp directories (7 of xmlui's 40). So the host ranks candidates by
sessions, then ways typed, then runs, and files scratch and temp paths,
including the script an interpreter runs, under `shell`. The page opens
with one sentence naming the five commands that hold most of the repeated
runs (59–65% in all three projects), then the rows seen in three or more
sessions (falling back to two, then all, when a small project has fewer
than five such rows), at most ten, with the rest behind "N more recur in
fewer sessions" and a **Show all** button. That took xmlui from 33 rows to
8 and bram-studio from 54 to 8 (40 and 57 before the scratch rule); Bram's 62 widely recurring rows hit the
cap. The ranking and fold are in
`helpers.js` (`__bramTaskRows`, `__bramTaskHeadline`), repeated client-side
so a pane served by an older host reads the same.

**A row is a command only if its first word is something you can run**
(issue-483-tasks-runnable-first-word, 2026-11-12). The first Windows page
was mostly not commands: Codex's multi-line PowerShell split into lines
(`body = @{…}`, `route = 'worklist-mutate'`, `try {`, `foreach (…) {`,
`Write-Error`) and prose split out of messages (`Windows`, `So`, `the`).
Word lists can't keep up with that (Jon: "rather than whack-a-mole what is
the rule"), so the host asks one question of every step the lists let
through: does its first word name a file in the project, or a program on
PATH spelled in the case it was typed (PATHEXT on Windows, the on-disk name
from the folder listing, so `OpenCode` doesn't match `opencode.cmd`)? And
is it not data (`name = …`, even when `route` is a Windows program)? A step
that fails is counted as `notACommand`. That also drops the one-word shell
functions an agent defines in a session (bram-studio's `post` and `band`),
which used to sit below the widely recurring rows. On this repo's Windows
sessions it took the list from 49 rows to 28 and removed every non-command.
The answer depends on the machine running the report, which is the machine
whose sessions it reads.

**Calls to Bram's own routes are candidates too**
(task-protocol-filter-sees-url-variables, 2026-11-12). They had been set
aside whole as `bramProtocol`, so the commands agents repeat most never
appeared, and the filter only saw a literal `127.0.0.1:` or `localhost:`,
missing `curl "$U/__worklist/plan"`. Now a `curl` whose URL is loopback or a
shell variable before `/__<route>` is clustered by route (and `op`, inline or
from a body file the same session wrote earlier) into `bramRoutes`, shown on
the Tasks page under "Bram's own routes". State-folder reads and request-body
building stay `bramProtocol`. On this repo's Claude and Codex sessions
(2026-11-12, 431 sessions, 95,629 steps) 56 routes recur: `/__search` 481
runs in 60 sessions, `/__worklist/mutate op=advance` 309 in 52,
`/__worklist` 323 in 43, `/__worklist/commit` 411 in 42, `/__app-info` 165 in
41, and `bramProtocol` fell to 12,855 steps. Saving one as a Makefile target
raises the permission, Codex-transport and port questions in the item's draft.

The run costs about 13 s here (`[tasks] op=candidates … ms=`), all in
parsing session files, so the pane fetches on open and Refresh only.
What the feature itself should be (which runner, a save flow, whether
the agent or the host runs a saved command) is to be decided from these
numbers and Greg's; the notes are in the item's draft.

### What the agents spend: token accounting

judell/bram#486 asks what the agents' model calls cost, and #477 and #487
wait on the answer. `issue-486-token-accounting-instrument` shipped the
instrument in its own module (`src-tauri/src/token_accounting.rs`), so that
measuring `lib.rs` does not grow it. It reads the usage every provider already
records per call (Claude's `message.usage`, Codex's `token_count` events,
OpenCode's `message.data.tokens`), joins each call to the claim ledger (which
item's work it was, or chat), and answers three questions: what each session
and item spent, how much context Bram's real work needs, and which files the
agents read into context.

```sh
src-tauri/target/debug/bram --token-accounting ~/bram > /tmp/ta.json
src-tauri/target/debug/bram --token-accounting ~/bram --since 2026-11-08T00:00:00Z --calls
curl -4 -sS "http://127.0.0.1:<port>/__token-accounting?session=claude:<id>"
```

The first is the whole history for Claude, Codex and OpenCode; the second is
one window with a row per call; the third is one live session and its
subagents. Each turn end also traces `[session-context] op=sample … tokens=`
(the last call's context), so the log carries the curve as it happens. The
shapes and definitions are in `apis.md`. Two matter most: context is
`input + cacheRead + cacheCreation` for Claude and OpenCode, but plain
`input_tokens` for Codex (whose `input_tokens` already includes the cached
part); and attribution only reaches back as far as the state-mirror db, which
on this machine begins 2026-11-07.

**The constrained-environment reading** (Jon's goal of knowing whether Bram
could run on a self-hosted model): `constrained.byModel` reports, per
provider and model, how much context main-thread calls needed (nearest-rank
p50, p90, p99 and the peak), the share of calls and of sessions that never
exceeded 32k, 64k, 128k, 200k and 256k, the floor (context at each session's
first call, which is Bram's own fixed cost before any work), the uncached
tokens a cache-less server would process per call, and the pace. It does not
say whether a smaller model could do the work, and it does not run one.

**The ledger** (`issue-486`, `token-accounting-ledger-in-sqlite`) keeps every
call and read once seen, so a call stays counted after its session file is
gone. It lives at `<app_local_data_dir>/token-ledger/<project key>.db` (on
macOS `~/Library/Application Support/org.xmlui.bram/token-ledger/`). Deleting
it is safe: the next request backfills from whatever session files still
exist, and calls whose files are gone are lost. A call's item is stamped once
it is 10 minutes old (`ATTRIBUTION_SETTLE_MS`), with one exception: a call at
or after the start of the oldest claim still open is left unstamped until that
claim clears, because an open claim reads as 1 ms wide and the call would be
stamped `chat` for good. A Codex call is keyed `<ordinal>:<time>`, so a rollout
rewritten in place can't overwrite an old row. If the ledger can't be opened
or was written by a newer Bram, the report is built from the files and the
trace says `[token-accounting] op=ledger-fallback reason=open-failed|newer-schema`.
`op=ledger` is traced only when a pass stored something.

**First baseline (2026-11-12, this repo: 247 Claude sessions with 27,545
calls, 286 Codex rollouts with 24,530):**

- Claude's floor is 74-104k (median 81k on `claude-opus-5-5`, 94k on
  `claude-fable-5-1`), so a 32k or 64k window is out before any work starts.
  Of `claude-opus-5-5` calls, 8% fit 128k, 23% fit 200k and 34% fit 256k;
  its peak is 975k.
- Codex's floor is 13-25k (median 18k on `gpt-5.5`), and 56% of its `gpt-5.5` calls fit 128k, 85% fit
  200k and all fit 256k. Its fixed cost is a fifth of Claude's here.
- Cache reads are 94-99% of context. The median uncached tokens per call are
  892 for `claude-opus-5-5` and 1,739 for `gpt-5.5`; a server with no prompt
  cache would instead process the whole context each call (median 363k and
  117k).
- `src-tauri/src/lib.rs`: 29,125 of 114,658 lines are inside test modules
  (25%), counted by an independent script at 29,085. Agents read it mostly
  through the shell: 15.6 MB via shell against 1.8 MB through `Read`, which
  is why `viaShellBytes` is kept apart.

The 2026-10-31 session `ebf9386c`, which the earlier scratch survey put at 786
main-thread calls, reads 793 calls and 382.8M cache-read tokens now: the
transcript kept growing after that survey, and an independent count of its
unique non-synthetic message ids gives the same 793. Its 550 subagent calls
and 73.2M cache-read tokens match exactly.

## Build vs. reload boundary

**Settled 2026-08-26 (the helpers.js-needs-rebuild mantra was wrong,
and launch mode is the variable.)** Bram registers its own `tauri`
scheme handler, which serves every `app/**` asset through
`serve_app_file` (`lib.rs`) — and that function prefers an **on-disk
`app/` root**, falling back to the `include_dir!` embedded tree only
when no candidate directory exists. The candidate that fires under the
documented launch is `exe_dir/app`: launching via the **`./bram`
symlink at the repo root** keeps the executable's parent at the repo
root, where the live source `app/` sits. The symlink is load-bearing.

So, **when launched via `./bram` at the repo root**:

| path | rule |
|---|---|
| `app/tools/**` | Served from disk per request. Pane reload picks up edits AND new files. (Auto-reload only when `ui.toolsPaneHotReload` is on; otherwise reload the pane manually.) |
| `app/__shell/**` (incl. `helpers.js`) | Served from disk per request. Pane reload re-fetches and re-executes it. No rebuild. Proven three ways on 2026-08-26: source chain, a live pane-reload observation, and a loopback probe showing a pre-edit process serving post-edit bytes. |
| `app/vendor/**` | Served from disk per request. `cp` the new build, reload the pane. No rebuild. |
| `app/main.js`, `app/index.html`, `app/styles.css` | Served from disk, but the parent window doesn't reload with the tools iframe — **relaunch the app** to re-execute them. Still no rebuild. |
| `src-tauri/**` | **Rebuild + relaunch.** The only genuinely rebuild-gated path. |

**When launched any other way** — the raw `target/debug/bram` binary,
or an installed bundle with no adjacent `app/` directory — no disk
candidate exists and everything serves from the **embedded** tree
baked at the last `cargo build` of the binary actually running. There
the old mantra is true: rebuild + relaunch for any `app/**` change,
and a running process keeps its launch-time embedded tree regardless
of later builds. This is also why `cargo build` still matters even
for pure-JS changes you validated via reload: it refreshes the
embedded fallback that installed binaries will serve.

**And one sharper fact inside that mode (#332):** `include_dir!`
embeds all of `app/` while `build.rs` declares `rerun-if-changed` for
exactly one file under it (the loose-ends skill). So a markup-only
edit followed by `cargo build` recompiles nothing, and a raw-binary
launch silently serves markup from an older vintage. This is
deliberate — watching `app/` wholesale would recompile a ~55k-line
crate on every `.xmlui` edit, for a path the documented `./bram`
launch serves from disk anyway, and release builds run from clean
checkouts so shipped binaries always embed fresh. The staleness is
made legible instead: `bram --embedded-app-hash` prints the baked
tree's content hash, `bram --hash-app-dir <path>` hashes a checkout
with the same algorithm, `/__app-info` reports `embeddedAppHash` and
`servingEmbedded`, and `tb.ps1` compares the two hashes in its
preflight and warns `STALE EMBEDDED app/` on mismatch. When testing
`app/**` behavior through a raw-binary launch, trust that preflight,
not the build's exit status.

Residual caution, kept on purpose: a pane reload re-executes
`helpers.js` in the iframe, but long-lived listeners, pre-XMLUI
globals, and parent-window state can carry stale behavior across a
reload. For subtle shell-runtime changes, a full relaunch remains the
clean-room validation even though the bytes were already fresh.

**Always `cargo build` (debug), never `cargo build --release`, when
validating changes** — debug builds are seconds, release builds are
minutes, and release is for shipping. Don't suggest `cargo run`
either. And always launch the `./bram` symlink at the repo root, not
an installed/older app — both for fresh code and because the symlink
is what makes disk serving resolve at all.

**Build through `scripts/build.sh`**, the same debug build with a
record: output and exit status pass through, and each build appends a
line to `resources/build-log.jsonl` (gitignored). The line has its
duration, HEAD and whether it moved, `src-tauri/` lines changed since
the last logged build, other `cargo`/`rustc` processes running, load,
and the caller. `scripts/build-timings.sh` times synthetic edits on a
quiet tree; this log catches the slow real builds it can't (#452:
2m55s and 1m38s builds the same day the synthetic rows read 25–34 s).
Read it by sorting on `seconds` and looking at the slow rows' context.
On Windows, build through `scripts/build.ps1`
(`powershell -NoProfile -File scripts\build.ps1`): it runs VsDevCmd
itself, builds the `x86_64-pc-windows-msvc` target the Windows launch
uses, and logs the same line with `platform: "windows"`.

### Formatting is enforced, so run `cargo fmt`

`src-tauri/` was normalized in `d94a3b9`, and `.github/workflows/fmt.yml`
runs `cargo fmt --check` on every push and pull request. Formatting is
no longer something to remember: run `cargo fmt` before you commit and
the check passes. `src-tauri/rustfmt.toml` pins
`style_edition = "2021"`, so your rustfmt and CI's agree even across
toolchain updates.

And now it is enforced at the gate, not just in CI
(worklist-commit-refuses-unformatted-rust). When a `worklist-commit`
in the source repo stages any `.rs`, the route runs `cargo fmt --check`
on the worktree first and refuses (409 `op=refuse-unformatted-rust`,
claim released) if it is dirty — so an unformatted Rust commit cannot
land through the gate whether or not anyone ran `cargo fmt`, and whether
it goes through whole-file or interval staging. A git pre-commit hook
would NOT cover the interval path: it commits via `git commit-tree`,
which runs no hooks. The worktree check is inductively sufficient — a
clean worktree keeps the reconstructed interval tree clean because
prior gated commits kept HEAD clean. The recurrence that motivated this
(`c2ff3e6a` committed without `cargo fmt`; CI failed on that tip; the
earlier one-shot fix `811ceaa` cleaned the drift but did not stop the
next one) is why the discipline moved from "remember" to "cannot".

That check is deliberately its own workflow. `build.yml` is
`workflow_dispatch`-only because the Tauri matrix builds are expensive;
a fmt check needs no system dependencies and no compilation, so it can
afford to run per push — which is the only place it prevents drift
rather than discovering it during a release.

The sweep is listed in `.git-blame-ignore-revs`, and you can register
it locally:

```sh
git config blame.ignoreRevsFile .git-blame-ignore-revs
```

GitHub's blame view honours the file already, with no configuration.

Do not expect it to do much for `d94a3b9` specifically. Measured after
the fact: of 53,930 lines in `lib.rs`, only **8** blame to the sweep at
all — git's own diff pairing already carries reformatted lines back to
the commit that wrote the code — and those 8 do not move even under an
explicit `--ignore-rev`, because the sweep left them with no predecessor
line to reassign to. The blame-churn objection to reformatting a large
file turned out to be much smaller than it is usually assumed to be.
The file is here as standing hygiene for the next sweep, not because
this one needed rescuing.

Before the sweep this section said the opposite — *never* run the
formatter, because on an unnormalized tree it rewrote 247 regions of
code nobody had touched, and the commit gate stages whole declared
files, so a 12-line fix would ship as a 900-line diff under your item's
id. That is what normalizing bought: the rule stopped depending on
anyone remembering it. The cross-machine coordination that landing it
required is on judell/bram#322.

### Measuring build time, not guessing at it

`scripts/build-timings.sh` prints the four numbers a build-time claim
needs, as a markdown table:

| measurement | what it isolates |
|---|---|
| no-op | cargo's freshness scan. If this already costs seconds, the complaint is not compilation |
| incremental (one line appended to `lib.rs`) | the cheapest edit there is: nothing below it moves |
| incremental, lines shifted (one line inserted at the top) | a mid-file edit, which moves every line below it. Full debuginfo records line numbers, so this may cost much more than the appended row; the gap between the two is the measurement (2026-10-27's mid-file edits built in 34 s to 2m55s) |
| relink only | link time in isolation, which matters because the debug binary is ~70 MB |
| check only (same one-line probe) | type-checking and metadata, with no codegen and no link |
| clean (`--clean`) | everything, dependencies included — the only number comparable across machines and toolchains |

**The rows do not add up, and the script does not pretend they do.** The
obvious derived figure — `incremental − check − relink` as "codegen" — was
tried and came out **negative**, twice: cold (`260.59s` of check against a
`25.48s` build) and warm (`19.66s` check plus `9.91s` relink against a
`23.95s` build). The legs share work: relink recompiles the bin's trivial
`main.rs`, and check runs its own analysis pass. So read `check` and `relink`
as independent scales for type-checking and for linking, and don't subtract
one from another.

The `check` leg needs warming to be a scale at all: `cargo check` keeps
separate fingerprints, so a check issued right after a build type-checks the
whole crate from nothing. The script warms that cache first, which is why the
cold `260.59s` above is not the number it reports.

**What these rows settle, and what they don't.** `check` is the row that
decides whether crate size is worth attacking, because it is the cost that
does **not** shrink when files are split *within* one crate: `lib.rs` →
`lib/*.rs` stays a single compilation unit, and rustc type-checks and
codegens the whole of it either way. A module split buys readability and
nothing in build time. Only separate crates — a workspace — shrink the unit,
which is a much larger change and should be taken on a number, not a hunch.
The debug profile (`codegen-units`, `debug = "line-tables-only"`) is the other
lever, and it wants the same treatment: measured, not guessed.

Restoring a probe is itself an edit, so after each restore the script runs one
untimed build before the next timed row. Before 2026-10-27 the relink row ran
straight after the incremental probe's restore, so its number may include a
recompile; compare it with earlier runs with that in mind.

The incremental leg restores `lib.rs`'s exact bytes from a saved copy via
a trap, so an interrupted run still leaves the tree as it found it, and a
dirty tree's edits survive rather than being discarded. `--clean` builds
into a scratch `CARGO_TARGET_DIR` and removes it afterward
(`--keep-scratch` keeps it), so it cannot disturb the target dir you
actually build into.

**A before/after claim about build time is made with this script, not
from memory.** That is the same rule §Perf diagnosis ordering states for
the pane — the baseline is a commit, recorded with the instrument that
will verify the "after" — applied to the one performance question that
has no trace to read, because builds happen on the host before the app
exists. Before this script, every slow-build report (#451) was a feeling.

Run it on demand, not in CI: like `scripts/setup-harness.sh` and the
membership starter check, it needs a machine that can build, which is
not what the workflows have.

### When builds get slow, check `target/debug/deps` first

Cargo never deletes old build artifacts, and `rustc`, the linker and
`cargo check` list `target/debug/deps` on every invocation. On
2026-10-27 that directory held 982,330 files (119 GB), and it, not the
code, was why builds had crept from seconds to half a minute (#452). The
library rebuild used 4.3 s of CPU in 10.8 s of wall time. Neither
`-Z time-passes` nor `-Z self-profile` sees that gap, because both time
work, not waiting. macOS `sample` on the running `rustc` found it in
`__getdirentries64` under `rustc_session::search_paths`.

`cargo clean --manifest-path src-tauri/Cargo.toml --profile dev` fixed it.
The full rebuild afterwards took 40 s, and then:

| | before | after |
|---|---|---|
| library rebuild | 10.6–10.9 s | 2.4 s |
| incremental (appended) | 24–34 s | 3–8 s |
| relink only | 7–8.5 s | 0.5 s |
| check only | 16–20 s | 1.5 s |

The directory grows back by about 360 files per build. `scripts/build.sh`
logs the count as `deps_files` and prints a warning past 100,000 files
(set `BRAM_DEPS_WARN` to change that). It never cleans on its own,
because a clean forces a full rebuild. Earlier build-time numbers in this
section, and any profile or crate-split argument resting on them, were
measured on the polluted directory: re-measure on a clean one before
relying on them.

That warning missed the next incident. On 2026-11-03 a Windows rebuild
failed with `There is not enough space on the disk. (os error 112)`, at
0.2 GB free of 476 GB, while `deps` held 1,713 files. The space was in
bytes, not files: lib-test PDBs of 92 to 167 MB from earlier tips, seven
renamed exes, and 10.5 GB in a stale `target/debug` tree. And the check
was bash-only, so the Windows build never ran it, and it looked only at
`deps`. Both `scripts/build.sh` and `scripts/build.ps1` now log
`target_bytes`, `free_bytes` and `stale_debug_tree_days`, and warn before
building when free space is under 5 GB (`BRAM_DISK_WARN_GB`) or the
target dir is over 20 GB (`BRAM_TARGET_WARN_GB`).

On Windows the stale tree is `src-tauri\target\debug`. The Windows launch
builds and runs `target\x86_64-pc-windows-msvc\debug`, so nothing uses
the plain `debug` tree and it is safe to delete outright.

`build.ps1` also moves a running Bram's exe aside, and it has to move
both names. Cargo links `deps\bram.exe` and uplifts it to
`debug\bram.exe`. On 2026-11-03, with only the `debug` name moved, the
`deps` name still mapped the running image and the link failed twice
with `LNK1104: cannot open file '…\deps\bram.exe'`. Moving both let it
link in 10 s. The moved copies are `bram.exe.old-<timestamp>`; each run
deletes the ones no process holds. It refuses to move anything when free
space is under 2 GB, or under the largest `free_bytes_used` a logged
build recorded, because a build that then fails for space would leave no
exe to relaunch. If a build fails anyway with the exe gone, it moves the
copies back.

## Hand-testing the Worklist gate

The gate's selection matrix — which buttons light for which combination of
items, entanglement, and lifecycle state — is exercised by the walkthrough
in `docs/worklist-gate-walkthrough.md`: a demo-instance board built from
starters, phase-by-phase expectations, deliberate wedge attempts, and
teardown trace greps. Born as the 0.5.3 release gate (three findings, all
fixed in-run).

**Run it on demand, not before every release** — when a field report or a
change to the gate surfaces calls for it, and only the phases that report
points at. Field reports have been finding the gate's bugs (#432, #433,
#434); the reasoning is in
[`decisions/2026-10-13-gate-walkthrough-on-demand.md`](decisions/2026-10-13-gate-walkthrough-on-demand.md).
Its graduation target is still an automated test on transient instances,
per the harness pattern below; the host-observable half could graduate
first.

## Developing and testing the startup dance

Startup — Setup seeding a project, hook registration, currency and
staleness checks, the needs-setup and first-run banners — was for a
long time the least testable part of Bram, and it shows in the issue
record: #99, #102, #173, #211, #247 and #249 were all found by a
person noticing, several of them after shipping. #249's gate 1 is the
representative artifact, a hand-run expectation table scored "3 of 4
pass, 1 real finding".

The reason it stayed manual is that verifying startup appeared to
require restarting *your own* Bram — which kills the agent session
doing the verifying. It does not.

### Run a second instance against a throwaway project

`bram <path>` takes a project root (`determine_project_root`,
`src-tauri/src/lib.rs`), and there is no single-instance guard. Each
instance binds its own loopback port and writes its own
`resources/.bram-port`. So a full Bram can run against a scratch
directory while your working session keeps going, untouched:

```sh
git init /tmp/scratch-project   # issue-448: Bram starts only at a repo's root
BRAM_TRACE=1 ./bram /tmp/scratch-project
```

Two details that are easy to get wrong:

- **`BRAM_TRACE=1` is belt-and-braces, no longer load-bearing.** Traces
  default ON (they were opt-in until 2026-08-26), so a scratch project
  with no `.bram.json` traces without it. Keeping the variable pins the
  intent against the project setting and costs nothing — and nearly
  every startup assertion worth making reads the trace.
- **Kill by PID, never `pkill bram`.** Your own session is a `bram`
  process too.

### The stale-binary trap

`cargo build` replaces the binary file, but a running process keeps
executing the inode it started with. A Bram launched before your build
will happily keep running the old code, and its behavior looks like a
real result. Before trusting any startup finding, check the binary's
mtime against the process start time. This is the concrete form of the
rebuild-and-relaunch rule above; the rule says to relaunch, this says
why a stale run is so easy to mistake for a genuine one.

### Startup is observable without the UI

Setup's effects are entirely inspectable over the loopback port, which
is what makes them assertable:

| what | how |
|---|---|
| installed / needs-setup / currency flags | `GET /__enhance/status` |
| run Setup headlessly | `GET /__enhance/run?force=true` |
| what Setup seeded | the file tree (`.claude/`, `AGENTS.md`, `CLAUDE.md`, `resources/.worklist-authorization.json`) |
| what startup did | `resources/bram-traces/bram-trace.log` |

The status says *which* check failed, not only that one did (#465):

| field | meaning |
|---|---|
| `claudeDocsCurrent`, `menuHookRegistered` | the two halves of `claudeNeedsSetup` that were once invisible (`hookRegistered` and `coreInstalled` are the others) |
| `setupReasons` | stable codes, computed beside the verdict for the providers `providerNeedsSetup` consults: `not-installed`, `claude-hook-missing`, `claude-menu-hook-missing`, `claude-instructions-stale`, `codex-hook-missing`, `codex-developer-instructions-missing`, `codex-instructions-stale`, `opencode-plugin-missing`, `opencode-instructions-missing`, `antigravity-hook-missing`, `antigravity-rules-stale`. The banner and an agent read this one list |
| `runningVersion`, `checkoutVersion`, `processOlderThanCheckout` | the process's `CARGO_PKG_VERSION` against `src-tauri/Cargo.toml` beside the on-disk `app/` root; `checkoutVersion` is null when serving the embedded tree. Compared numerically |
| `providerSetupKind` | `repo-setup`, `codex-hook-refresh`, or `restart-older-process` (the banner offers no Run Setup button there) |

A process older than its checkout can't be produced by the harness's
pristine scenarios; raise the worktree's `src-tauri/Cargo.toml` version
under a running `./bram` launch and read the status.

`/__enhance/status` fields answer *different questions* and can
legitimately disagree — a fact that was itself a bug for a while.
`claudeNeedsSetup` / `codexNeedsSetup` are about installation currency
and key on `core_installed` (the worklist authorization file plus
per-provider hook registration). `firstRun` asks whether this project
has ever been managed at all. Before 2026-08-20 `firstRun` keyed only
on `.bram.json` / `.xmlui-desktop.json` — the *settings* files, written
when a setting is first saved and never by Setup — so a successful
Setup left the "Bram is starting for the first time in this repo"
banner up, with the banner's own text claiming Setup writes
`.bram.json`. Same shape as #211.

### The harness

`scripts/setup-harness.sh` automates the above: per scenario it creates
a pristine temp project, launches the locally built binary against it,
drives Setup over the port, asserts, checks that a second Setup is
byte-idempotent, and tears down (keeping the directory on failure).

```sh
scripts/setup-harness.sh                 # all scenarios
scripts/setup-harness.sh pristine_git    # one
BRAM_BIN=/path/to/bram scripts/setup-harness.sh
```

Exit status is the number of failed assertions. Scenarios vary the
*starting state* rather than the steps, because that is where the
historical failures were: `pristine_nogit` (not a git repo, so refused:
nothing written, autostart held, #448), `subfolder` (inside a repo but not
its root, refused and named as such, #448), `pristine_git`,
`already_setup` (the cross-machine re-run from #249), `legacy_hooks`
(retired generic hook names, #173), and `nested` (a managed parent). The
refused scenarios find the port from Bram's stderr (`[bram] loopback
port=`), since a refused launch writes no port file.

Every assertion traces to a bug the record actually caught, which is
the standard for adding another one. Notably `already_setup` asserts
that re-running Setup leaves **tracked** files unmodified — #249's real
failure — while untracked seeded files are expected on a fresh project.

Two boundaries worth keeping:

- **The source repo is deliberately not a scenario.** `is_source_repo`
  takes a different path (#102) and pointing the harness at it would
  dirty your working tree.
- **It is not wired into CI**, because launching the app needs a
  windowing session. It is a command you or an agent runs on demand.

When a startup assertion fails, prefer fixing the *fixture* over the
product until you have shown the failure reproduces outside the
harness — the first `already_setup` failure was the harness committing
its own live trace log, not Setup churn.

## The synthetic-testbed method: drive a real instance, read its trace

**This is the default way to develop a pane or interaction change — not
an occasional technique.** For anything a user sees or clicks, stand up
a throwaway Bram showing the *whole range* of the feature's states at
once, drive it by hand, and refine against its trace. The startup-dance
section above is one *scripted* application of the second-instance
pattern; this is the general, *exploratory* loop it belongs to. Jon,
after a first sitting that reshaped a surface in minutes: "just
absolutely the way to develop."

### The loop

- **Stand up a second full Bram** with the standard synthetic fixture:

  ```sh
  scripts/demo-instance.sh new full-board --from all
  scripts/demo-instance.sh launch
  ```

  This is the startup-dance second-instance pattern generalized past
  startup. The generated fixture's `resources/worklist.json`,
  `resources/.claim-intervals.json`, and `refs/bram/claims/*` display
  disjoint entanglement, a true dependency, supersession, unattributed
  work, many simultaneous claimants, and an expired authorization **on
  one board** — the range a scripted test would cover one case per
  assertion, laid out for the eye simultaneously.
- **The human drives it as a clueless user** — clicking around,
  misclicking, hitting real states. This is the fuzzer. It finds what
  scripted expectations miss because it moves the way hands actually
  move, not the way a test author predicted they would.
- **The agent reads the trace, not the screen.** The orchestrator
  watches the demo instance's
  `resources/bram-traces/bram-trace.log` and diagnoses from
  host-observable evidence — the log-first drill, run live instead of
  post-mortem.
- **Edits land in seconds.** Under the `./bram` symlink launch the demo
  serves `app/**` from the *source* repo's disk (`helpers.js`,
  `*.xmlui`, `vendor/**`) per request, so an edit shows on a pane reload
  with no rebuild and no relaunch — the reload boundary this doc already
  establishes, turned into a development cadence. Edit → reload → look →
  refine, tightened to the length of a reload. (Rust still
  rebuilds+relaunches; the pane does not.)
- **Save or reset on one word.** `scripts/demo-instance.sh capture
  <name>` parks a stumbled-into state as `scenario/<name>`;
  `scripts/demo-instance.sh reset` rehydrates the active scenario byte
  for byte, including its claim refs and ignored scenario-owned
  `resources/` sidecars, while preserving the running instance's port
  and trace files. Each probe can start clean without throwing away a
  finding.

### Why it's the way, not a technique

- It makes the *render-what-the-reader-sees* rule (in
  `app/__shell/conventions.md`) continuous. A green test verifies
  behavior; this verifies communication, live — the half a passing spec
  cannot reach.
- It puts the human on realistic input and the agent on tireless
  observation — each doing what it is best at.
- It turns the trace from a forensic artifact into a live conversation.

### Receipts (proof, not decoration)

In its first sittings the method surfaced, in minutes each:

- the terse-strip + `tooltipMarkdown` redesign — five iterations
  against live renders, each visible on reload;
- an accidental **Drop that could not be canceled** — destructive, a
  ~90s agent turn, and Escape never routed to the agent's PTY (#340);
- an **env-inheritance bug** that silently disabled the demo agent's
  transcript.

None were reachable by reading code; all were obvious the moment a
human drove an instrumented instance.

### Two launch-hygiene requirements the method itself surfaced

- **Scrub `CLAUDE_CODE_*` on launch.** An agent-launched instance
  inherits the parent's `CLAUDE_CODE_CHILD_SESSION` and its siblings;
  the demo's own agent then reads the marker, concludes it is a child
  session, and disables transcript saving. Launch with
  `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_SESSION_ID`,
  `CLAUDE_CODE_BRIDGE_SESSION_ID`, `CLAUDE_CODE_MESSAGING_SOCKET`,
  `CLAUDE_CODE_MESSAGING_TOKEN`, `CLAUDE_CODE_ENTRYPOINT`, and
  `CLAUDE_CODE_EXECPATH` unset (`env -u …`). (#339)
- **Kill by PID, never `pkill bram`** — your working session is a
  `bram` process too (the same trap the startup-dance section names).

### Institutionalization (#339)

`scripts/demo_instance.py` is the platform-neutral fixture engine;
`scripts/demo-instance.sh` and `scripts/demo-instance.ps1` are the thin
platform launchers. The default fixture lives beside this checkout at
`../bram-demo-instance`; put `--repo <path>` before the verb to use a
different location. The useful verbs are:

| verb | effect |
|---|---|
| `new <name> --from <starter>` | create and hydrate a scenario; repeat `--from`, or use `--from all` |
| `use <name>` | discard the live probe and hydrate `scenario/<name>` exactly |
| `add <starter>` | add another synthetic state to the live board; it is temporary until captured |
| `capture <name>` | park the current worktree, worklist sidecars, and claim boundaries as a scenario |
| `reset` | rehydrate the active scenario |
| `status [--json]` | report scenarios, active state, item/ref counts, PID, and log path |
| `launch` / `stop` | start the real binary or stop only its recorded PID |
| `prune [--match GLOB] [--keep N] [--dry-run] [--gc]` | remove scenario branches matching the glob (default `check-*`, the starter check's boards), keeping the newest N by name and never the active scenario; captured scenarios are untouched unless named. `--gc` then reclaims the space |

The starter names are `disjoint-entanglement`, `dependency`,
`supersession`, `unattributed`, `many-claimants`,
`expired-authorization`, `ambiguous-duplicate` (identical-context
duplicate placements — the one membership shape no wild board reliably
produces; see `docs/attribution-model.md` §4), `reedit-own-lines`
(one item's file created then re-edited across two claim windows —
judell/bram#367's double-count shape, kept as its regression fixture), `multi-file`
(one begun item over five files: two modified, one new, one large diff,
one listed but untouched, so its files table shows every row state),
`new-file-left-out` (judell/bram#419: one started item with a new file and
a tracked file, a second started item that edited that tracked file without
declaring it; committing the first alone reaches hunk-by-hunk staging with a
never-tracked file),
`chat-edit-in-item-file` (judell/bram#273 — one item edits a file in two
claim windows with a chat-turn edit to the same file in between, so the
item's lines and the unowned chat lines sit side by side), and
`committed-outside` (judell/bram#406: one row whose work landed through a
plain `git commit`, and one begun-then-reverted row that must not read as
landed), and `joint-owner-committed` (judell/bram#432: two items approved in
one click whose owner of the shared file has since committed and left the
board, a partner with no changes, and a later, separately approved item on
the shared file whose commit must not be refused), and `entangled-with-unowned`
(two begun owners of one file plus a third hunk written with no claim open:
each row alone predicts only its own hunk, while committing both together
takes the whole file; Codex's acceptance case for issue-425-retire-replay),
and `membership-unanswerable` (judell/bram#437: one begun item declaring a
file it hasn't created yet, another declaring a new file an ignore rule
hides; both once failed every membership compute, so the gate refused all
commits as "no membership answer"), and `foreign-dot-bram` (judell/bram#469:
a `.bram/` holding another tool's files, with no Bram marker, beside a live
board in `resources/`; Bram must keep `resources/` and write nothing into
`.bram/`. It's left out of `--from all`, since it forces the old layout).
For example, a focused board is:

```sh
scripts/demo-instance.sh new ordering \
  --from disjoint-entanglement \
  --from dependency
scripts/demo-instance.sh launch
tail -f ../bram-demo-instance/resources/bram-traces/bram-trace.log
# Human explores; agent reads the trace.
scripts/demo-instance.sh capture ordering-found-cancel-gap
scripts/demo-instance.sh reset
scripts/demo-instance.sh stop
```

On Windows, use the same verbs through
`scripts\demo-instance.ps1`. Build first with `scripts\build.ps1`, which
sets up the VS developer environment itself. The launcher refuses a debug binary older
than its Rust sources (see below) or a half-build where `bram-guard.exe`
is newer than a locked `bram.exe`; it names existing Bram PIDs rather than killing them. On
every launch it verifies a directory junction named `app` beside the
target-triple debug executable, pointing at this checkout's `app/`.
That is the Windows disk-serving equivalent of Unix's repo-root
`./bram` symlink: no privileged symlink, and no hardlink left stale when
Cargo replaces the executable. If this workflow changes on macOS/Linux,
its automated launcher smoke runs here; if it changes on Windows, run
the corresponding PowerShell launch smoke on Windows before calling it
done.

Both launchers delegate process creation to the shared engine, which
also refuses a stale binary: one older than the newest git-tracked file
under `src-tauri/`, committed or not, since under the symlink (or
junction) launch `app/**` is served from disk and only the Rust can make
the binary stale. The refusal names that file. Until 2026-10-13 the test
was "older than HEAD's commit time", which refused after scripts- and
docs-only commits and missed uncommitted Rust edits. It
captures stdout/stderr under the demo repo's git directory, records the
exact child PID, enables tracing, and removes
`CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_SESSION_ID`,
`CLAUDE_CODE_BRIDGE_SESSION_ID`, `CLAUDE_CODE_MESSAGING_SOCKET`,
`CLAUDE_CODE_MESSAGING_TOKEN`, `CLAUDE_CODE_ENTRYPOINT`, and
`CLAUDE_CODE_EXECPATH`. A nested demo agent therefore saves its own
transcript instead of mistaking itself for the parent session.

The durable scenario library is **git branches** (`scenario/<name>`),
strictly more powerful than regenerate-from-script because a branch
captures real *stumbled-into* states, not only synthesized ones — the
externalized-context principle applied to fixtures. A scenario branch
retains the clean boundary, every claim tree, and the pending-work tree
in one commit ancestry. `use` checks out its tip detached, performs
`git reset --mixed <boundary>` so Bram sees pending work, and recreates
only that scenario's `refs/bram/claims/*`. The generated repository has
a conspicuous marker and no remote; every destructive verb verifies
both facts and the exact git toplevel before acting. Transfer a scenario
branch with ordinary Git plumbing (for example, a bundle), but never add
an upstream to the disposable instance.

**Layout.** A scenario branch stores the board under `resources/`, the
0.8.3 layout, so every capture stays valid for both binaries. Hydration
(`new`, `use`, `reset`, `capture`) moves it into `.bram/` the way the
mover does: `.gitignore` = `*` first, then the entries, then
`.migrated-from`, so the demo's binary never runs the mover on it. The
live port and traces survive in either folder. `capture` reads the board
from whichever folder is live and writes it back at the `resources/`
paths, so a board captured in either layout gives the same tree. Pass
`--legacy-layout` to hydrate the 0.8.3 way and exercise the real mover.
`reset` and `capture` repeat the active layout; `new` and `use` default
to `.bram/` (judell/bram#469).

### Switch matrix

`scripts/switch-matrix.py` drives the session-switching matrix from
[`decisions/2026-10-27-bram-chooses-the-session.md`](decisions/2026-10-27-bram-chooses-the-session.md)
against a demo instance with the real agent CLIs, and checks the trace after
each step. It launches the demo with `BRAM_TEST_DRIVE=1`, which turns on
`POST /__test/{switch,reload,new-session,turn}`. These call the same functions
the pane's clicks do, and are off otherwise (see `authorization.md`).

```sh
python3 -I scripts/switch-matrix.py --no-turns --agents claude,opencode   # no model turns
python3 -I scripts/switch-matrix.py --agents claude,opencode              # adds New session, fresh partners, delete
```

The cases: a plain switch; New session (named on its reserved id, brief
released when the composer is drawn); fresh partners (started, grouped with and
named after the Claude session); resuming a group member by id, including
Codex → OpenCode in a group of three; OpenCode marked current at launch;
no-group fallback by id, with the offer up; a reload clearing the offer;
deleting the running session refused; deleting a grouped one taking it out of
its group; the offer's Add and the Sessions row's Leave. Across the whole run, no `launch-timeout`, `launch-exited`,
`boot-held-restore` or `rebound` may appear.

Exit status is the number of failed checks; on failure the demo stays up. Cases
that deliver a brief or catch-up start a real model turn; `--no-turns` skips
them. The script waits for each launch to settle before the next step, as a
person would: driving faster produced failures that weren't Bram's. It needs a
windowing session, so it runs on demand, not in CI. Record new failures in the
decision note, not as new items, unless one blocks a release.

### Membership starter checks

`scripts/membership-starter-check.py` turns the starters into a regression
check for line ownership. It hydrates one board with every starter (or the
ones named with `--starter`), launches the locally built Bram on it, waits for
a fresh membership compute, and checks `/__worklist` against
`EXPECTATIONS` in `scripts/demo_instance.py`: each path's owners and its
single, joint and unowned line counts, paths that must have no entry, and
selected items' `willCommit`. Any give-up (`op=membership … reason=`),
unexplained line or conservation breach in the run's trace fails it too.

```sh
python3 -I scripts/membership-starter-check.py
python3 -I scripts/membership-starter-check.py --starter new-file-left-out
```

The exit status is the number of failed checks. On failure the demo is left
running on the failing board. After a passing run it prunes its own boards to
the newest three (`prune --match 'check-*' --keep 3`); a failing run prunes
nothing.

**When to run it:** after a change to the membership engine, claim capture or
the board serve, and before a release that touches them. The Rust tests cover
the line rule's pure functions; this covers the integration they can't reach,
which is where judell/bram#437 lived (the engine's scratch-index seed failed
on two ordinary board shapes, and the field found it first).

**One board is a canary.** All starters share one compute, so one broken
starter can fail them all, as `membership-unanswerable` would have before
#437. Rerun with `--starter` to narrow it down.

**Adding a starter means adding its expectation** in the same change, written
from what the starter is meant to show. When the engine and the table
disagree, read the starter first: the table is a claim about intended
behaviour, not a recording of current output. It can't check anything the pane
computes (the board header, strip wording); those stay rendered checks.

It needs a windowing session, so it runs on demand, not in CI, like
`setup-harness.sh`.

### Forcing the suspicious-silence banner

The footer's "terminal is suspiciously silent" banner normally needs a real
silence with the terminal hidden. To raise it on demand, **Shift-click the
button that hides the terminal**, while the terminal is showing. A plain click
hides it with no banner, and a Shift-click that shows it does nothing extra.

This is a deliberate self-test (`app/main.js`, `postSuspiciousSilenceSelfTest`).
It posts the banner's message from the parent shell to the pane, and doesn't
touch the live turn or the host's detector. It logs `op=self-test-dispatched`
and an `op=warn` with `provider=self-test`. **Open terminal** clears it, the
same way it clears a real one.

For a footer layout check, start a slow turn first (`sleep 120` in the agent's
shell), so the banner sits beside "working…", then resize the window. The
trace's pass condition is no `flip:true` and no `wide` on
`transcript-split-height`. An overlap inside the footer doesn't overflow the
page, so the trace can't see it: a person has to look. On 2026-10-31 this test
showed the jitter fixed while **Open terminal** still sat on top of the status
label (footer-silence-banner-overflow-jitter).

### Relationship to the scripted sections

The Worklist-gate walkthrough and the startup-dance harness are
*scripted* checks with fixed expectation tables — good for regression,
weak at discovery. This is their *exploratory* complement: unscripted
human driving against the full state range, for finding what you didn't
know to script. The two feed each other — graduating a finding here
into a `scenario/` branch or a harness assertion is how an exploratory
discovery becomes a durable regression guard.

## Capturing terminal specimens

Bram reads agents' screens: terminal attention (`terminal_attention_prompt_shape`
in `lib.rs`), the launch gate, the boot-prompt holds. Its markers must come
from **captured specimens, never guesses** (the rule the trace vocabulary's
`terminal-attention` row states). A specimen is what a CLI actually draws, at
a known version, ANSI-stripped.

There are two sources:

- **The candidate inventory.** Unmatched idle screens are logged as
  `[terminal-attention] op=candidate preview=…`. Grep every project's trace
  archives for the screen you're after. Claude's "⚠ Your login expires in 3
  days · run /login to renew" turned up this way (2.1.296).
- **A deliberate capture.** `scripts/capture-terminal-specimen.py` runs a
  command under a pseudo-terminal of fixed size (120×40 by default) and
  writes the raw bytes and an ANSI-stripped `.txt`:

  ```sh
  python3 -I scripts/capture-terminal-specimen.py /tmp/spec/claude 15 -- claude
  python3 -I scripts/capture-terminal-specimen.py --keys $'\r' --after 4 \
      /tmp/spec/codex 12 -- codex
  ```

**Reach a state without touching a real account.** Point the CLI at a
throwaway config directory and unset the API keys. Most states are a few
fields of config away:

| State | Setup |
|---|---|
| Claude logged out | `CLAUDE_CONFIG_DIR=$(mktemp -d)`, unset `ANTHROPIC_API_KEY`, and pre-set onboarding and folder trust in its `.claude.json` (`hasCompletedOnboarding`, `projects."<dir>".hasTrustDialogAccepted`). Otherwise the first-run theme picker and the folder-trust prompt come first. |
| Codex logged out | `CODEX_HOME=$(mktemp -d)`, unset `OPENAI_API_KEY`, and pre-trust the folder in its `config.toml` (`[projects."<dir>"] trust_level = "trusted"`). |
| Antigravity logged out | Use the whole-screen specimen in `docs/agent-specimens/antigravity/logged-out.txt` (Antigravity CLI 1.4.12); it offers Google OAuth or a Google Cloud project. |
| Antigravity folder trust | Use a fresh workspace with no trusted-workspace entry; see `docs/agent-specimens/antigravity/first-run-trust.txt` (CLI 1.4.12). |
| Antigravity resume picker | Open the conversation picker at an idle composer; see `docs/agent-specimens/antigravity/resume-picker.txt` (CLI 1.4.12). |
| Antigravity approval prompts | The `permission-prompt.txt` snapshot is an ordinary working screen, not an approval dialog. Use the captured file approvals (`approval.txt`, `tool-write-approval.txt`, `tool-edit-approval.txt`, `tool-shell-approval.txt`) or command approval (`command-approval.txt`), all Antigravity CLI 1.4.12. |
| A first-run prompt | the same throwaway directory with nothing pre-set |
| An OpenCode dialog | At an idle composer, Ctrl+P opens the command palette, and Ctrl+X then `m`, `l` or `a` opens the model, session or agent list. Bram drains its tail on every keystroke, so capture what is drawn after the last key, not the whole stream. |
| OpenCode logged out | No such screen in 2.1.4. With empty `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` and `XDG_CACHE_HOME`, it prints a stack trace (`cli.server-connection.resolve`) and exits before drawing its composer. With a real config, it runs its free models without signing in. |

Capture from an empty working directory, so the Bram repo's own trust state
doesn't interfere. The same environment works for a demo instance:
`env -u ANTHROPIC_API_KEY CLAUDE_CONFIG_DIR=<dir> scripts/demo-instance.sh launch`.
Set `shell.startupPolicy` to `newSession` in its `.bram.json`, because Bram
resumes from the real session history, which the throwaway config doesn't
have ("No conversation found to continue").

**What the specimens taught the first time** (terminal-attention-login-screen,
#448): a logged-out Claude 2.1.310 draws **no** login screen. It opens its
normal composer, and only the footer says "Not logged in · Run /login", which
is why the launch check reported success. Codex 0.163.0 draws a full sign-in
screen whose Enter starts the sign-in. Guessing would have got Claude wrong.

**Then pin it.** Write the markers against the stripped text, anchored to the
end of the screen where a live prompt puts its footer. Pin the specimen
whole in a test, along with a near-miss: a reply that merely quotes the text,
followed by the composer's chrome, must not match. Record the CLI version in
the comment beside the markers.

## Moved from the seeded conventions

These sections lived in `app/__shell/conventions.md` until the
core/reference split (`conventions-core-and-reference-split`,
judell/bram#396). They matter only when developing Bram itself, so they
live here, unseeded. The history behind the seeded rules is in
[`conventions-rationale.md`](conventions-rationale.md).

### XMLUI lookup order

When you are figuring out how to do a thing in XMLUI, ask the XMLUI
MCP server for how-to documents first (`xmlui_search_howto`). The
how-to corpus usually carries the complete pattern and tradeoffs.
After that, use `xmlui_component_docs` for exact component props,
events, and exposed methods. Use examples as a fallback or to confirm
local style, not as the first source of truth.

When a non-obvious markup choice depends on documentation, cite the
relevant how-to or component URL in the response.

### Test Worklist UX through the worklist itself

When a change touches the Worklist UX (button states, gray-out,
feedback flow, pruning), surface it as a pending item even when the
diff is already on disk. Approving the item exercises the new
behavior end-to-end — file rewrites, pruning, Talk-page update — as
the actual test.

### Fold optional companion input into existing actions

When a surface already has clear primary actions (Approve / Drop /
Submit) and a new optional input is added (free-text feedback, notes,
override flag), fold the input value into the existing actions'
onClick payloads rather than adding a separate Submit / Send button.
Render the input above or beside the primary buttons; clear it after
submission. A separate submit button creates a third decision point
("which button do I click for what?") and forces the user to send
two messages when one would do. Only add a separate submit button if
the auxiliary input is genuinely independent of the primary actions.
