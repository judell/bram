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
indexes, claim and safety refs, linked worktrees,
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
- **The pane's memory, or a claim about it (`scripts/pane-memory-bisect.sh`)** → read [`docs/developing-bram-reference.md` §Where the pane's memory goes](developing-bram-reference.md#where-the-panes-memory-goes).
- **Working on the Tasks report (#483)** → read [`docs/developing-bram-reference.md` §What the agents keep running: the Tasks report](developing-bram-reference.md#what-the-agents-keep-running-the-tasks-report).
- **Working on token accounting (#486), or reading its numbers** → read [`docs/developing-bram-reference.md` §What the agents spend: token accounting](developing-bram-reference.md#what-the-agents-spend-token-accounting).
- **Working on the Context tab's stack (`/__context-stack`, `src/context_stack.rs`), or adding a provider's reader** → read [`docs/developing-bram-reference.md` §What the model was sent: the context stack](developing-bram-reference.md#what-the-model-was-sent-the-context-stack).
- **Making a claim about build time (`scripts/build-timings.sh`)** → read [`docs/developing-bram-reference.md` §Measuring build time, not guessing at it](developing-bram-reference.md#measuring-build-time-not-guessing-at-it).
- **Builds are slow or fail for disk space** → read [`docs/developing-bram-reference.md` §When builds get slow, check `target/debug/deps` first](developing-bram-reference.md#when-builds-get-slow-check-targetdebugdeps-first).
- **Hand-testing the Worklist gate** → read [`docs/developing-bram-reference.md` §Hand-testing the Worklist gate](developing-bram-reference.md#hand-testing-the-worklist-gate).
- **Changing Setup, hooks or startup checks, or running `scripts/setup-harness.sh`** → read [`docs/developing-bram-reference.md` §Developing and testing the startup dance](developing-bram-reference.md#developing-and-testing-the-startup-dance).
- **Developing a pane or interaction change against a demo instance (`scripts/demo-instance.sh`, starters, the switch matrix, the membership check)** → read [`docs/developing-bram-reference.md` §The synthetic-testbed method: drive a real instance, read its trace](developing-bram-reference.md#the-synthetic-testbed-method-drive-a-real-instance-read-its-trace).
- **Writing markers for an agent CLI's screen (terminal specimens)** → read [`docs/developing-bram-reference.md` §Capturing terminal specimens](developing-bram-reference.md#capturing-terminal-specimens).

## Code organization (helpers.js / Globals.xs / window)

Iframe-side code spans four surfaces; these rules say where each kind of code lives.

### The surfaces

- **`app/__shell/helpers.js`**: real JavaScript (async, `fetch`, timers, tauri events). Functions live on `window`; markup calls `window.foo(...)`.
- **`app/tools/Globals.xs`**: the expression engine's context. No async/await, setTimeout, fetch, or Promise chaining outside DataSource. Top-level `function foo` auto-hoists onto `window.foo`: bare-callable from attribute expressions and able to shadow a helpers.js export, but NOT reliably reachable as `window.foo(...)` from helpers.js. Code callable from both sides lives in helpers.js.
- **`window.*`**: the shared namespace; the `__bram*` prefix gives helpers.js a collision-safe space.
- **`.xmlui` files**: markup; handlers and bindings are tiny expressions.

### Where each kind of code goes

- **Pure functions** and **outside-sandbox shims** → `window.foo` in `helpers.js`.
- **xs-only code** → `Globals.xs`, only when it touches xs-scope module state directly or is a very hot binding callee.
- **XMLUI attribute handlers** → a single function call: `onClick="window.foo(...)"`. Never multi-statement bodies, multi-line arrow bodies, or object-literal blobs.

### When and why do we need delegators?

A *delegator* is `function foo(...) { return window.__bramFoo(...); }` in `Globals.xs`, letting markup write `foo(...)`.

**Default: don't add one.** Call `window.foo(...)` from markup, including inside arrow bodies. **Add one only when** the function is called many times in attribute expressions and the name isn't already on the bare `window` surface; each one widens the collision surface.

### The `__bram*` namespace prefix

`__bramFoo` defends a helpers.js export against a same-named `function foo` in `Globals.xs`. It is **not** a blanket rule: bare-name helpers (`toShell`, `toTurn`, etc.) are fine when no `Globals.xs` declaration shadows them.

- A name with a matching `Globals.xs` delegator → name the helper `window.__bramFoo`.
- A name living only in `helpers.js` → bare `window.foo`.

### Failure modes that informed these rules

Each is a hard rule, not a preference:

- **Attribute expressions stay a single function call.** A weird error from markup with an inline ternary / `&&` chain / multi-statement arrow is the inline expression, not the parser. Extract to a `window.foo` helper first.
- **Bare names inside arrow bodies silently abort analysis.** In an arrow body passed to `subscribeTauriEvent` / `onDidChange` / `onLoaded`, a bare `foo` with no xs declaration kills the registration and every statement after it. Call qualified: `window.foo()`.
- **xs `function foo` hoists over `window.foo`.** A same-named xs declaration clobbers the helper (a delegator then calls itself). Name the helper `window.__bramFoo` or remove the xs declaration.
- **helpers.js top-level *calls* must follow their definitions.** A load-time throw aborts everything after it; definitions referencing later names are fine.
- **Match pane elements by `automationId` or `id`, never `testId`.** XMLUI renders `data-testid` only while `xsVerbose` is on; turning it off (`e7df16d3`) silently broke every `[data-testid=…]` lookup, ⌘Enter among them (#506). `automationId` renders as `data-automation-id` and `id` as `data-xmlui-id`, whatever the config.
- **`ExpandableItem` state is uncontrolled and positional.** In an `Items` loop whose membership changes, use controlled expansion keyed by item id.

### Post-edit verification ritual

After ANY iframe-side change (`.xmlui`, `Globals.xs`, `helpers.js`): grep `console-error|console-unhandledrejection` in `.bram/bram-traces/bram-trace.log` once the pane has reloaded. Zero matches is the pass condition; any match is triaged before reading anything else from the trace. When the edit touched a binding or a helper it calls, also run `node scripts/check-bound-helpers.mjs`.

#### Known noise: the ResizeObserver loop

One message is exempt: `ResizeObserver loop completed with undelivered notifications.` Exclude it from the grep (`| grep -v ResizeObserver`) and don't triage it. Everything else still counts.

Trace vocabulary: [trace-vocabulary.md](trace-vocabulary.md).

The xs engine **silently rejects assignments to member expressions** inside function bodies (`window.X = value` in a Globals.xs function; only the trace sees the error). Top-level statements are fine. Define the setter in helpers.js and call it from xs.

### Displayed state lives in XMLUI, not on `window`

Keeping *displayed* state on `window` defeats XMLUI's tracking (Jon, 2026-11-16).

- **State the pane displays lives in XMLUI state:** a `var.` for one component and its children, a global (a top-level var in `Globals.xs`) across components, a DataSource or PushSource for anything from the host.
- **A helper called from a binding reads only its arguments.** No `window.__bram*` data, helpers.js module variables, storage, or clock. Pass the value in, or the tick that changes with it.

**The check:** `node scripts/check-bound-helpers.mjs` fails on any helper read not in its allowlist; `--list` prints the allowlist.

### Fixed tab headers and scrolling bodies

Use `PinnedHeader` for a tab heading and the controls that act on its body.
The App uses `scrollWholePage="false"`, **every Page uses `height="*"
minHeight="0"`**, and the tab root is a `VStack height="*"`. The shared
component keeps the header outside its `height="*" overflowY="auto"` body.
A splitter pane can bound a `height="100%"` column, whose inner columns
still use `height="*"`. Do not omit the Page sizing: its default
`min-height:100%` lets it grow with content, so the inner body never scrolls.
A component mounted both in a Page and in a max-height box (the Worklist in
the Footer's pane) takes the star only in the Page: under a maximum the star
resolves to a fraction of the box with overflow visible, and the siblings
after it pin to the box's edge over the spilled content
(`worklist-footer-pane-gate-overlap`).
See [the full-height layout how-to](https://www.xmlui.org/docs/howto/build-a-full-height-scroll-layout)
and the browser reproduction in the pinned-header worklist draft. Transcript
retains its measured List height; Search retains a sticky heading in its
bounded route scroller.

### Peer-pattern check before designing

Before introducing a new mechanism in the pane, grep 2–3 peer components for the same problem and run `xmlui_search_howto`. If peers share one pattern and the misbehaving component hand-rolls another, the outlier is where the bug lives: refit to the canonical pattern before adding instrumentation.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Code organization](developing-bram-reference.md#receipts-code-organization).

## Warning color is earned

Use the warning color only when a nontechnical user can clearly
understand the problem and can easily act on it (Jon, 2026-11-10). A
warning also says the action in plain words ("Push again from the
Commits tab", "Try the switch again from Sessions"); a line that can't
name one isn't a warning. Detail only an agent or a developer can use
stays in the trace, not on the surface.

**The same rule governs the Status tab** (Jon, 2026-11-12: "There's no point in warning me without offering an action"):

- A `warn` or `error` row carries an `action` in plain words ("Run Setup", "Relaunch Bram"); the pane shows it under the detail.
- A signal with no action reports `none`, shown as **N/A** ("no verdict", not "fine").
- `ok` means a known good state, never "nothing wrong that we noticed".
- The host enforces it: `enforce_status_actions` demotes any warn/error row without an action to `none`; add the `action` where the row is built.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Warning color](developing-bram-reference.md#receipts-warning-color).

## Define a term where it first appears

When pane text uses a glossary-like term (a word with a meaning
specific to Bram or to the agent, such as *block*, *call*, *compaction*,
*claim*), define it in the sentence right after its first mention on that
surface: "Every block in this session, since the last compaction, sized to
the measured total. A block is one piece of what the model is sent: a
message, a tool call or its result, an instruction file, a bout of
reasoning." (Jon, 2026-11-25). Use parentheses only for a short gloss that
doesn't interrupt the sentence. Later mentions on the same surface use the
bare term.
A tooltip or dialog counts as its own surface. Don't send the reader to a
separate glossary for a word the sentence needs.

## Rust: typed data, not hand-read JSON

Data inside the host is typed (Jon, 2026-11-11, a standing rule):

- **Structs for shapes** (`#[derive(Serialize, Deserialize)]`, `#[serde(rename_all = "camelCase")]`, `#[serde(default)]` for optional fields), not a `serde_json::Value` read one field at a time.
- **Enums for closed sets of codes.**
- **`Value` only where the shape isn't ours** (a provider's transcript record, a CLI's output). Build JSON only where a route answers.
- **A Tauri command takes the typed struct as its parameter.**
- **When a struct replaces a `Value`, keep the wire shape byte-identical** and pin it with a test; check whether anything matches serde's old error text.

New code follows the rule; old code converts when an item touches it, not in sweeps.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Rust typed data](developing-bram-reference.md#receipts-rust-typed-data).

## Provider parity

If a thing can be provider-neutral, it must be (Jon, 2026-11-25). Bram
supports Claude, Codex, OpenCode and Antigravity. A feature, check,
instruction or fix built for one of them is built for all, through one
code path that branches on the provider only where that provider's own
CLI, file format or config forces it.

- **When it can't be neutral, say why not.** Name the provider fact that
  forces it ("only Claude takes a session id and name at launch") in a
  comment at the branch, and in the item's draft and commit message.
  "Did Codex first" is not a reason.
- **A provider left out for later is a Worklist item now**, not a note.
- **Drafts say what each provider gets.** An item's `# After` names how it
  covers each provider, or which provider it can't cover and why.
- **Check the peers before you ship.** When a fix lands for one provider,
  look for the same gap in the other three, the way you look for the same
  pattern in peer components.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Provider parity](developing-bram-reference.md#receipts-provider-parity).

## Paths: write for Windows too

Path code that only the Mac's `cargo test` has seen fails on Windows.

- **Recognize an absolute path by `starts_with('/')` or `Path::is_absolute()`, never by a leading `/` alone;** keep the `/` test wherever Unix-shaped strings can appear (fixtures, ledgers).
- **Compare paths after folding `\` to `/`, treat a drive-letter root (`C:`) as case-insensitive,** and accept the Git Bash spelling `/c/Users/…` as `C:/Users/…`.
- **Build fixture paths and URIs the way the real producer writes them**.
- **Test path functions with Windows spellings on every platform:** plain strings, no `#[cfg(windows)]`.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Paths](developing-bram-reference.md#receipts-paths).

## Client storage: decision vs. focus

File each new persisted key under the right class before writing it.

- **decision** (persists across sessions) → `localStorage`: a choice the user would be annoyed to re-make (sort order, a dismissed-forever notice, an opt-in toggle).
- **focus** (dies with the session) → `sessionStorage`: where the user *was* (scroll position, expanded row, an in-progress draft, a pending operation).

**Default to focus when unsure:** a wrongly-persisted focus key resurrects stale state as a bug elsewhere.

**The test:** would the value still make sense read aloud a week later without the session that wrote it? Decision. Would it sound stale ("row 3 is expanded")? Focus.

When moving a key between stores, grep for any read site still reaching into the old store; no migration is needed otherwise.

### Scope persisted keys by project

`localStorage` is **one store shared by every Bram window**. After decision vs. focus, ask: **is it about this project, or about the user?**

- **About the user:** an unscoped key.
- **About this project** (its saved tab, the unsent draft, anything naming its items, sessions or files): scope it with `window.__bramProjectScopedKey(base)` in `helpers.js`. On null (project unknown), neither read nor write; never fall back to the shared slot.
- **Don't migrate an unscoped project key's old value** unless you know which project wrote it.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Client storage](developing-bram-reference.md#receipts-client-storage).

## Push over polling

Do NOT add `pollIntervalInSeconds` to XMLUI DataSources. Drive refetch from events or actions:

- **Local action:** bump a reactive var the DataSource depends on (a `refreshTick` in queryParams), or call `.refetch()` in `onSuccess`.
- **Cross-component / host state:** a `PushSource` on `window.bramSubscribeTauriEvent('<event>')`.
- **Remote state with no filesystem trigger:** the poll is NOT moved to the client or replaced by a Refresh button. A Rust host thread polls, computes a signature, and synthesizes a Tauri event (`issues-changed` is the precedent) only on change.

## Perf diagnosis ordering

**Timeline first, semantic probes second.** When the pane feels slow, record a browser Timeline (Safari Web Inspector → Timelines, Frames view) before Bram's semantic instruments. Layout-dominated frames mean DOM size and forced reflow (virtualize, bound the region); Script-dominated frames mean evaluation work, and only then is the eval-trace probe the right drill.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Perf diagnosis ordering](developing-bram-reference.md#receipts-perf-diagnosis-ordering).

## Build vs. reload boundary

**Launch mode is the variable.** `serve_app_file` (`lib.rs`) prefers an on-disk `app/` root, falling back to the embedded tree only when none exists. The **`./bram` symlink at the repo root** puts the live source `app/` beside the executable; it is load-bearing.

**When launched via `./bram`:**

| path | rule |
|---|---|
| `app/tools/**` | Served from disk. Pane reload picks up edits AND new files (auto-reload only when `ui.toolsPaneHotReload` is on; otherwise reload the pane). |
| `app/__shell/**` (incl. `helpers.js`) | Served from disk. Pane reload re-executes it. No rebuild. |
| `app/vendor/**` | Served from disk. `cp` the new build, reload the pane. |
| `app/main.js`, `app/index.html`, `app/styles.css` | Served from disk, but the parent window doesn't reload: **relaunch the app**. No rebuild. |
| `src-tauri/**` | **Rebuild + relaunch.** The only rebuild-gated path. |

**Launched any other way** (raw `target/debug/bram`, an installed bundle), everything serves from the **embedded** tree baked at the last `cargo build`: rebuild + relaunch for any `app/**` change. A markup-only edit followed by `cargo build` recompiles nothing, so trust the `tb.ps1` preflight (`bram --embedded-app-hash` vs `bram --hash-app-dir <path>`), not the build's exit status. For subtle shell-runtime changes, relaunch fully.

**Always `cargo build` (debug), never `cargo build --release`, when validating changes.** Don't suggest `cargo run` either. Always launch the `./bram` symlink, not an installed/older app.

**Build through `scripts/build.sh`** (debug build, logged to `.bram/build-log.jsonl`). On Windows, use `powershell -NoProfile -File scripts\build.ps1`.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Build vs. reload boundary](developing-bram-reference.md#receipts-build-vs-reload-boundary).

### Formatting is enforced, so run `cargo fmt`

`.github/workflows/fmt.yml` runs `cargo fmt --check` on every push and pull request; run `cargo fmt` before you commit. `src-tauri/rustfmt.toml` pins `style_edition = "2021"`. The gate enforces it too: a `worklist-commit` that stages any `.rs` runs `cargo fmt --check` on the worktree first and refuses (409 `op=refuse-unformatted-rust`, claim released) if it is dirty.

Receipts → [`docs/developing-bram-reference.md` §Receipts: Formatting](developing-bram-reference.md#receipts-formatting).

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
