# Bram

You are running in the **terminal** of a Tauri desktop shell. The shell
puts a real terminal (where you run) next to an agent pane the user can
SEE while talking to you. It can *optionally* also show a right-pane
target-app iframe, but that pane is **off by default** and often absent —
most users run their app in their own browser. Detect before you assume an
iframe is there; when one is present, use it.

Keep two distinct surfaces straight — they are not the same, and the rules
differ:

- **The agent pane** — Bram's own UI (the Worklist / Transcript / Sessions /
  Context / Status tabs). **Always XMLUI** — that's how Bram
  is built. Editing it means `app/tools/Main.xmlui`,
  `app/tools/components/*.xmlui`, `app/__shell/helpers.js`,
  `app/tools/Globals.xs`.
- **The target app** — whatever project the user is developing with Bram's
  help, shown in the **optional** target-app iframe *when that pane is
  enabled* (off by default, often absent). Per `app/__shell/conventions.md`,
  Bram "works with any project that serves a web UI (vanilla HTML/JS, a
  React or other Node app, a Python web app, an XMLUI app, etc.)." It **may
  or may not be XMLUI** — detect before you assume, and don't assume the
  pane is even present.

The XMLUI-specific guidance below is **unconditional when working on Bram
itself**, and applies to the target app **only when the target is XMLUI**.

## Working on Bram itself (the agent pane is XMLUI)

Most edits to Bram land in `.xmlui` files. Rules the xmlui-standalone
evaluator enforces hard:

- **No raw browser JS in event handlers** — `setTimeout`, `setInterval`,
  `fetch` outside DataSource, `async` / `await`, etc. are rejected at
  evaluation time with an unhandled rejection. Stay within App
  abstractions: `delay(ms)`, `debounce(ms, fn, ...args)`, the `Timer`
  component, `DataSource` for HTTP, `ChangeListener` for derived
  reactivity.
- **Lead with the xmlui-mcp tools** before reaching for a JS solution.
  The `xmlui_search_howto` tool is the fastest way to find the
  XMLUI-native pattern for a feature (e.g. "delay function", "debounce
  input", "wrap text in table cell"); `xmlui_component_docs` is for
  component-prop lookups; `xmlui_get_prompt` re-injects the server's
  framing guidance mid-session when you suspect you've drifted.
- **Cite a doc URL** for any non-obvious markup decision —
  `https://www.xmlui.org/docs/reference/components/<Name>` or
  `https://www.xmlui.org/docs/howto/<slug>`. If you can't cite one,
  search again.

The `xmlui-mcp` server is loaded for this conversation. Use it.

The helpers.js / Globals.xs / window code-organization discipline (where
each kind of code lives, when delegators are warranted, the `__bram*`
prefix, the xs failure modes, and the post-edit error grep) is in
`@docs/developing-bram.md`, `@`-imported below. That file is
source-repo-only; `@app/__shell/conventions.md` (also imported below) is
the cross-target half that Setup seeds into every managed project.

### Files you'll edit most (Bram)

- `app/tools/Main.xmlui` — the agent-pane surface
- `app/tools/components/*.xmlui` — Worklist (the primary gate, route
  `/worklist2`), Sessions, Toolbar, Architecture, etc. (the legacy
  `Workspace.xmlui` tab was retired in the 0.5.3 run)
- `app/tools/config.json` — XMLUI app config (resources, appGlobals)
- `app/tools/resources/*.svg` — custom icons; register in `config.json`
  under `resources` with the `icon.<name>` prefix
- `app/__shell/helpers.js` — window helpers loaded by `index.html` via
  `xmlui://localhost/__shell/helpers.js`

Reload boundary: only `app/tools/**` is hot-reloadable; everything else
(including `helpers.js`) is rebuild-from-`src-tauri/` + relaunch `./bram`
territory — full table and launch discipline in `@docs/developing-bram.md`.

## Working on the target app

The embedded target app is **optional and off by default** — most sessions
won't have one (the user previews their app in their own browser). This
section applies only when the user has enabled the target-app pane and asks
for something in it.

When the user asks for something in the target app, **first detect what the
target is**, then render output its native way:

- **Vanilla HTML/JS** — `index.html` + plain `.js`, no framework manifest.
  Edit the HTML/JS directly.
- **React / other Node** — `package.json` (look for `react`, `vue`, `next`,
  etc.). Edit components in the project's own framework.
- **Python web app** — `requirements.txt` / `pyproject.toml` / `*.py`
  serving templates. Edit templates / handlers.
- **XMLUI** — `config.json` + `.xmlui` files. See *When the target app is
  XMLUI* below.

When the target-app pane is enabled, a filesystem watcher reloads that
iframe automatically when you save — you do not need to ask the user to
reload. This auto-reload is purely for the embedded pane; it is irrelevant
when the user views their app in their own browser.

## When the target app is XMLUI

Only when the target is an XMLUI app: when the user asks for something that
benefits from structured output (tables, lists, charts, multi-line text) or
structured input (selectors, forms, multi-step flows), **edit the target's
`Main.xmlui`** (or one of its `components/` files) so it renders. The XMLUI
rules from "Working on Bram itself" apply here too.

Examples:

- *"Show me my recent commits"* → write a `<Table>` or `<List>` bound to
  data you fetched, then continue the conversation pointing at it.
- *"Pick a target branch"* → render a `<Select>` whose `onDidChange`
  calls `toShell('selected: ' + value)`. The user clicks; their pick
  arrives as user input on your next turn.
- *"Walk me through this in steps"* → render a `<Stepper>` or tabbed
  `<Pages>` and let the user navigate.

## The host helpers (cross-target)

The shell exposes helpers as window globals that post messages to the parent
shell. They are available in the **agent pane** unconditionally. To use them
from the **target app** (XMLUI or not), the target's `index.html` must
include `<script src="/__shell/helpers.js"></script>`. If it doesn't, drive
these from the agent pane instead.

> **Since C1 (target-pane origin isolation).** The target pane is served at a
> distinct `bramapp://localhost` origin, so cross-origin `getTauriInvoke()`
> returns `null` and the PTY-driving helpers (`toShell` / `toTurn` /
> `sendKeys` / `openExternal`) **no-op inside an embedded target app** — the
> pane is **display-only**. `helpers.js` is still served there (so XMLUI apps
> boot) but its host-driving functions are inert. Render agent-facing controls
> (`Select` / `Button` → `toTurn`) in the **agent pane**, which stays
> same-origin and where the helpers work.

| intent | call | what the host does |
|---|---|---|
| inject text the user can edit | `toShell(text)` | text + `\n` appears in your stdin; user must press Enter |
| submit a complete user turn | `toTurn(text)` | bracketed-paste + carriage return; auto-submits as a fresh turn |
| send raw key bytes to PTY (no newline) | `sendKeys(text)` | bytes go straight to PTY stdin — for Esc, arrows, single-key menus |
| open an external URL | `openExternal(url)` | host opens the URL in the system browser |
| capture screenshot of right pane | `captureScreenshot()` | host captures + injects the file path as a `toTurn` |
| log without bothering you | `logToHost(payload)` | recorded in cargo run stderr only — invisible to you |

Use `toTurn` for one-shot form submissions (Approve buttons, Confirm
buttons, single-pick selectors). Use `toShell` only when you want to
inject text the user can edit before sending.

```xml
<Select onDidChange="(v) => toTurn('branch: ' + v)">
  <Option value="main" label="main" />
  <Option value="dev"  label="dev" />
</Select>

<Button label="Confirm" onClick="toTurn('confirmed')" />
```

The user types or clicks; you receive `branch: dev` (or whatever you
chose) as a fresh user message.

## Coordinating via worklist.json (canonical worklist)

`resources/worklist.json` is the canonical surface for coordinating
multi-step work between you and the user. The Worklist tab in the
agent pane renders it under the heading "Worklist". Use it
whenever you'd otherwise enumerate small, independently-approvable
changes in prose. This is framework-agnostic — it applies whatever the
target is.

The full lifecycle (proposed → applied → committed), payload shapes,
authorization flow (`/__worklist/resolve`, `/__worklist/mutate`), and
edge cases live in `@app/__shell/conventions.md`, which is `@`-imported
below — read that for the authoritative description. Don't duplicate
conventions guidance here; this file points at the source of truth.

## Files to leave alone unless asked

- `src-tauri/src/lib.rs` — Rust backend (PTY, custom URI scheme,
  filesystem watcher, IPC command handlers)
- `app/main.js`, `app/index.html` — parent shell wiring
- `app/vendor/*` — vendored libraries (xmlui-standalone, xterm.js, etc.)

## Inspector

When editing Bram or an XMLUI target: `<Inspector />` is mounted in the
AppHeader's profile menu slot — the magnifying-glass icon top-right. It
shows semantic traces of XMLUI events. Open it when you're debugging
interactions before assuming the markup is wrong.

## Architectural background

The deeper narrative — why Tauri, why a static frontend, the gotchas
we hit (Tauri's SPA fallback, XMLUI's hidden `config.json` requirement,
cross-origin iframe reload) — lives at
`~/.agents/scout/projects/claude-code-desktop.md`. Read it if a
mechanism here surprises you.

<!-- bram:start -->
@docs/developing-bram.md
@app/__shell/conventions.md
<!-- bram:end -->
