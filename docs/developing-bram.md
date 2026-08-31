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

### Never run `cargo fmt`

`src-tauri/` has no `rustfmt.toml` and no CI fmt check, and its
formatting has never been normalized. On a clean tree at `8a08f4c`,
`cargo fmt --check` wanted **247 regions across 4 files — 237 in
`lib.rs` alone, spanning `lib.rs:783` to `lib.rs:52080`, replacing 657
lines with 915.** None of it is yours. All of it will land in your
commit, because the commit gate stages whole declared files rather than
the hunks you authored — so a 12-line fix ships as a 900-line diff under
your item's id, and the user's one veto surface stops working.

So: **match the surrounding style by hand for the lines you write, and
never run `cargo fmt`, `rustfmt`, or an editor's format-on-save over
these files.** If one runs anyway, `git checkout -p` the unrelated hunks
before the commit gate sees them — `cargo fmt --check` names every
region it would touch, which makes the sweep easy to spot and easy to
revert.

Normalizing the tree is a legitimate change, but it is its own worklist
item and its own commit, never a passenger on someone else's.

## Hand-testing the Worklist gate

The gate's selection matrix — which buttons light for which combination of
items, entanglement, and lifecycle state — is exercised by the walkthrough
in `docs/worklist-gate-walkthrough.md`: probe items doing trivial real work,
phase-by-phase expectations, deliberate wedge attempts, and teardown trace
greps. Born as the 0.5.3 release gate (three findings, all fixed in-run).
Run it before a release or after touching the gate surfaces. Its graduation
target is an automated test on transient instances, per the harness pattern
below; the host-observable half could graduate first.

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
historical failures were: `pristine_nogit`, `pristine_git`,
`already_setup` (the cross-machine re-run from #249), `legacy_hooks`
(retired generic hook names, #173), and `nested` (a managed parent).

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
