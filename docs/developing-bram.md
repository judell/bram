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

## Reference: read when the situation arises

These sections live in [`developing-bram-reference.md`](developing-bram-reference.md),
which isn't loaded into every session. Read the one that matches what you're
doing:

- **Editing the orientation site's engine (`arch.js`, `Main.xmlui`) or its template** → read [`docs/developing-bram-reference.md` §The orientation site's engine lives in the template](developing-bram-reference.md#the-orientation-sites-engine-lives-in-the-template).
- **Changing the PTY write path, or Codex screens tearing** → read [`docs/developing-bram-reference.md` §Codex synchronized terminal redraws](developing-bram-reference.md#codex-synchronized-terminal-redraws).
- **The pane feels slow, or you want its responsiveness numbers (`scripts/ui-health.py`)** → read [`docs/developing-bram-reference.md` §Standing health check: `scripts/ui-health.py`](developing-bram-reference.md#standing-health-check-scriptsui-healthpy).
- **Working on the Log tab or its narrator** → read [`docs/developing-bram-reference.md` §The story of a stretch: the Log](developing-bram-reference.md#the-story-of-a-stretch-the-log).
- **Whether a slow pane needs a reload or a new session (`scripts/pane-age-health.py`)** → read [`docs/developing-bram-reference.md` §Does the pane slow with age, or with session size?](developing-bram-reference.md#does-the-pane-slow-with-age-or-with-session-size).
- **Working on the Tasks report (#483)** → read [`docs/developing-bram-reference.md` §What the agents keep running: the Tasks report](developing-bram-reference.md#what-the-agents-keep-running-the-tasks-report).
- **Working on token accounting (#486), or reading its numbers** → read [`docs/developing-bram-reference.md` §What the agents spend: token accounting](developing-bram-reference.md#what-the-agents-spend-token-accounting).
- **Making a claim about build time (`scripts/build-timings.sh`)** → read [`docs/developing-bram-reference.md` §Measuring build time, not guessing at it](developing-bram-reference.md#measuring-build-time-not-guessing-at-it).
- **Builds are slow or fail for disk space** → read [`docs/developing-bram-reference.md` §When builds get slow, check `target/debug/deps` first](developing-bram-reference.md#when-builds-get-slow-check-targetdebugdeps-first).
- **Hand-testing the Worklist gate** → read [`docs/developing-bram-reference.md` §Hand-testing the Worklist gate](developing-bram-reference.md#hand-testing-the-worklist-gate).
- **Changing Setup, hooks or startup checks, or running `scripts/setup-harness.sh`** → read [`docs/developing-bram-reference.md` §Developing and testing the startup dance](developing-bram-reference.md#developing-and-testing-the-startup-dance).
- **Developing a pane or interaction change against a demo instance (`scripts/demo-instance.sh`, starters, the switch matrix, the membership check)** → read [`docs/developing-bram-reference.md` §The synthetic-testbed method: drive a real instance, read its trace](developing-bram-reference.md#the-synthetic-testbed-method-drive-a-real-instance-read-its-trace).
- **Writing markers for an agent CLI's screen (terminal specimens)** → read [`docs/developing-bram-reference.md` §Capturing terminal specimens](developing-bram-reference.md#capturing-terminal-specimens).

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
