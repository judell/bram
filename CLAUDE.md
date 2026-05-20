# Bram

You are running in the **left pane** of a Tauri desktop shell that puts
a real terminal next to an XMLUI surface. The user can SEE the right
pane while talking to you. Use it.

## What to do with the right pane

When the user asks for something that benefits from structured output
(tables, lists, charts, multi-line text) or structured input (selectors,
forms, multi-step flows), **edit `Main.xmlui`** (or one of the
`components/` files) so the right pane renders it. A filesystem watcher reloads the iframe automatically
when you save — you do not need to ask the user to reload.

Examples:

- *"Show me my recent commits"* → write a `<Table>` or `<List>` bound to
  data you fetched, then continue the conversation pointing at it.
- *"Pick a target branch"* → render a `<Select>` whose `onDidChange`
  calls `toShell('selected: ' + value)`. The user clicks; their pick
  arrives as user input on your next turn.
- *"Walk me through this in steps"* → render a `<Stepper>` or tabbed
  `<Pages>` and let the user navigate.

## Working in XMLUI surfaces

Both panes here are XMLUI, so most edits land in `.xmlui` files. Some
rules the xmlui-standalone evaluator enforces hard:

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

## How the right pane talks back to you

The right pane's `index.html` exposes helpers as window globals that
post messages to the parent shell:

| intent | from XMLUI | what the host does |
|---|---|---|
| inject text the user can edit | `toShell(text)` | text + `\n` appears in your stdin; user must press Enter |
| submit a complete user turn | `toTurn(text)` | bracketed-paste + carriage return; auto-submits as a fresh turn |
| open an external URL | `openExternal(url)` | host opens the URL in the system browser |
| log without bothering you | `logToHost(payload)` | recorded in cargo run stderr only — invisible to you |
| open devtools | (wrench icon does it) | n/a |

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

`resources/worklist.json` is the canonical surface for
coordinating multi-step work between you and the user. The Worklist
tab in the agent-tools drawer renders it as a checklist under the
heading "Worklist". Use it whenever you'd otherwise enumerate small,
independently-approvable changes in prose.

Schema:

```json
{
  "description": "one-line context for this batch",
  "items": [
    { "id": "...", "file": "...", "before": "...", "after": "..." }
  ]
}
```

The Worklist tab UI:

- "Worklist" heading is hard-coded; don't try to override it via JSON.
- `description` renders only when `items` is non-empty.
- Each item shows: checkbox (default checked) | filename (mono) | `before → after`.
- Two action buttons (only shown when items exist):
  - **Approve selected (N)**: sends `approved: <JSON array of full items>` via `toTurn`.
  - **Drop selected (N)**: sends `drop: <JSON array of ids>` via `toTurn`.
- When `items` is empty, the section shows just the heading + `(none)`.

Lifecycle:

1. **Propose** — write items to `worklist.json`. Each item should be
   small, discrete, and independently rejectable. Surface dependencies
   in `description`; don't bake them into ordering.
2. **User triages** — unchecks anything they don't want in this round,
   then clicks one of:
   - *Approve selected* → you receive `approved: [...]` and execute those items.
   - *Drop selected* → you receive `drop: [ids]` and remove them from the list without acting.
3. **Prune** — after either action, rewrite `worklist.json` with only
   the still-pending items, plus any newly-surfaced consequences
   (e.g., orphans revealed by a deletion). The worklist represents
   pending work, not history — completed items belong in commit
   messages.
4. **Empty state is fine** — when there's no pending work, leave
   `worklist.json` as `{ "description": "", "items": [] }`. The
   Worklist tab will render the heading and a `(none)` placeholder.
5. **Commit** when an executed batch is a meaningful unit.

When *not* to use this: one-or-two-item decisions, free-text input, or
anything where typing in chat is faster than rendering UI. The
worklist earns its keep when prose enumeration would be tedious or the
response ambiguous.

## Charting

The `xmlui-echart` extension is loaded — `<EChart>` is available
out of the box. It wraps Apache ECharts and accepts any valid ECharts
`option` configuration. Use it whenever the user asks for a chart
(line, bar, pie, scatter, heatmap, etc.). XMLUI theme colors are
applied automatically.

Reference: https://docs.xmlui.org/howto/use-echarts-for-advanced-charting
and https://echarts.apache.org/en/option.html for the full option API.

## Files you'll edit most

- `Main.xmlui` — the XMLUI surface (the one)
- `components/*.xmlui` — Workspace, Sessions, Toolbar, Architecture, etc.
- `config.json` — XMLUI app config (resources, appGlobals)
- `resources/*.svg` — custom icons; register in
  `config.json` under `resources` with the `icon.<name>` prefix
- `app/__shell/helpers.js` — window helpers loaded by `index.html` via
  `xmlui://localhost/__shell/helpers.js`

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

<!-- xmlui-desktop:start -->
@app/__shell/conventions.md
<!-- xmlui-desktop:end -->
