# xmlui-claude-code-desktop

You are running in the **left pane** of a Tauri desktop shell that puts
a real terminal next to an XMLUI surface. The user can SEE the right
pane while talking to you. Use it.

## What to do with the right pane

When the user asks for something that benefits from structured output
(tables, lists, charts, multi-line text) or structured input (selectors,
forms, multi-step flows), **edit `app/right/Main.xmlui`** so the right
pane renders it. A filesystem watcher reloads the iframe automatically
when you save — you do not need to ask the user to reload.

Examples:

- *"Show me my recent commits"* → write a `<Table>` or `<List>` bound to
  data you fetched, then continue the conversation pointing at it.
- *"Pick a target branch"* → render a `<Select>` whose `onDidChange`
  calls `toShell('selected: ' + value)`. The user clicks; their pick
  arrives as user input on your next turn.
- *"Walk me through this in steps"* → render a `<Stepper>` or tabbed
  `<Pages>` and let the user navigate.

The user's existing `~/CLAUDE.md` already mandates that you use the
**`xmlui-mcp`** MCP server for component lookups and cite a doc URL for
any non-obvious markup. Follow that here too.

## How the right pane talks back to you

Three intents, all sent via `window.postMessage` to the parent shell.
The right pane's `index.html` exposes two helpers as window globals; the
third is a literal message:

| intent | from XMLUI | what the host does |
|---|---|---|
| inject text into your stdin | `toShell(text)` | text appears as user input on your next turn |
| log without bothering you | `logToHost(payload)` | recorded in cargo run stderr only — invisible to you |
| open devtools | (the wrench icon already does it) | n/a |

So input controls in `Main.xmlui` look like:

```xml
<Select onDidChange="(v) => toShell('branch: ' + v)">
  <Option value="main" label="main" />
  <Option value="dev"  label="dev" />
</Select>

<Button label="Confirm" onClick="toShell('confirmed')" />
```

The user types or clicks; you receive `[XMLUI] branch: dev` (or
whatever you chose) as a fresh user message.

## Charting

The `xmlui-echart` extension is loaded — `<EChart>` is available
out of the box. It wraps Apache ECharts and accepts any valid ECharts
`option` configuration. Use it whenever the user asks for a chart
(line, bar, pie, scatter, heatmap, etc.). XMLUI theme colors are
applied automatically.

Reference: https://docs.xmlui.org/howto/use-echarts-for-advanced-charting
and https://echarts.apache.org/en/option.html for the full option API.

## Files you'll edit most

- `app/right/Main.xmlui` — the XMLUI surface (the one)
- `app/right/manual.md` — user-facing manual (renders when the help
  icon is clicked)
- `app/right/config.json` — XMLUI app config (resources, appGlobals)
- `app/right/resources/*.svg` — custom icons; register in
  `config.json` under `resources` with the `icon.<name>` prefix

## Files to leave alone unless asked

- `src-tauri/src/lib.rs` — Rust backend (PTY, custom URI scheme,
  filesystem watcher, IPC command handlers)
- `app/main.js`, `app/index.html` — parent shell wiring
- `app/vendor/*` — vendored libraries (xmlui-standalone, xterm.js, etc.)

## Inspector

The right pane mounts `<Inspector />` in the AppHeader's profile menu
slot — it's the magnifying-glass icon top-right. It shows semantic
traces of XMLUI events. Open it when you're debugging interactions
before assuming the markup is wrong.

## Architectural background

The deeper narrative — why Tauri, why a static frontend, the gotchas
we hit (Tauri's SPA fallback, XMLUI's hidden `config.json` requirement,
cross-origin iframe reload) — lives at
`~/.agents/scout/projects/claude-code-desktop.md`. Read it if a
mechanism here surprises you.
