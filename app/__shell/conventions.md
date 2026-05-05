# Working with xmlui-desktop

This project runs inside the **xmlui-desktop** Tauri shell. The shell
puts a real terminal alongside an XMLUI surface, plus an "agent tools"
drawer that includes a Workspace (worklist + commits) and a Sessions
browser. The user sees the right pane while talking to you — use it.

> Note on memory: this file is loaded into every session in this
> project via a `@`-import in `CLAUDE.md`. Don't write memory entries
> capturing what you read here — preferring the worklist, knowing the
> helper APIs, etc. Memory is for cross-session context that wouldn't
> otherwise be available; project conventions are already available
> by virtue of being in this file.

## Render structured output in the right pane

When the user asks for something that benefits from structured output
(tables, lists, charts) or structured input (selectors, forms,
multi-step flows), edit `Main.xmlui` (or a file under `components/`)
so the right pane renders it. A filesystem watcher reloads the iframe
automatically — you don't need to ask the user to refresh.

## Coordinate via proposal.json

`resources/proposal.json` is the canonical surface for multi-step
coordination between you and the user. The Workspace tab in the agent
tools drawer renders it as a checklist under "Pending items".

Schema:

```json
{
  "description": "one-line context for this batch",
  "items": [
    {
      "id": "kebab-case-id",
      "status": "proposed",
      "file": "path/to/file.xmlui",
      "before": "what's there now (or context for new content)",
      "after": "what you'll change it to"
    }
  ]
}
```

The `status` field controls the badge in the Workspace tab and what
the user is being asked to approve:

- `"proposed"` (the default if omitted) → badge **TO APPLY**. The user
  is approving you to *make* the change. After they approve, you apply
  the edits to the file(s), then prune the items from
  `proposal.json`.
- `"applied"` → badge **TO COMMIT**. Use this only when the change is
  already on disk and you're asking the user to approve a commit. The
  Approve button then means "yes, commit these". Most flows skip this
  state — apply, prune, and decide about commits separately.

Default to omitting the status (or setting `"proposed"`) when you
first add items. Don't pre-mark things as `"applied"` unless they
truly are.

You do not need to create `resources/proposal.json` in advance — when
the file is missing, xmlui-desktop serves an empty default and the
Workspace tab shows *(none)*. Just write to the file the first time
you actually have items to propose; xmlui-desktop will create it.

Lifecycle:

1. **Propose** — write items to `resources/proposal.json`. Each item
   should be small, discrete, and independently rejectable. Writing
   items to the file does **not** mean they are approved — it means
   you are *asking* the user to approve them.
2. **User triages** — unchecks anything they don't want, then clicks
   one of three buttons:
   - *Talk to agent* (with a comment typed above it) → you receive
     `talk: <text>` as a fresh user turn. The user is asking a
     question or giving feedback with **no items approved and none
     dropped**. Respond to the message; do not edit files, do not
     touch `proposal.json`.
   - *Approve selected (N)* — only enabled when ≥1 item is checked.
     You receive `approved: {"items":[...], "feedback":"..."}`.
     Execute the items in the array; respond to the optional feedback.
   - *Drop selected (N)* — only enabled when ≥1 item is checked.
     You receive `drop: {"ids":[...], "feedback":"..."}`. Remove the
     listed ids from `proposal.json` without acting; respond to the
     optional feedback.
3. **Prune** — after either action, rewrite `resources/proposal.json`
   with only the still-pending items. The worklist is *pending* work,
   not history; completed items belong in commit messages.
4. **Empty state is fine** — leave it as `{ "description": "", "items": [] }`.

If you ever do receive `approved: {"items":[]}` or
`drop: {"ids":[]}` (shouldn't happen — the buttons are disabled when
nothing is checked — but be defensive), treat it the same as
`talk:` — feedback only, take no action.

When *not* to use this: one-or-two-item decisions, free-text input, or
anything where typing in chat is faster than rendering UI.

## Right-pane helpers (opt-in, only needed for project-side hooks)

The Workspace and Sessions tabs in the agent tools drawer already use
these helpers internally — you get the worklist Approve/Drop flow with
no extra setup. The script tag below is only needed if **your own**
xmlui markup wants to talk back to the running agent (e.g., a custom
Approve button, an in-page form that submits to a fresh user turn).

If your project's `index.html` includes
`<script src="/__shell/helpers.js"></script>`, these globals become
available inside xmlui markup:

| helper | usage |
|---|---|
| `toShell(text)` | inject text into stdin; user must press Enter |
| `toTurn(text)` | submit text as a complete user turn (auto-Enter) |
| `openExternal(url)` | open URL in the system browser |
| `logToHost(payload)` | log to xmlui-desktop stderr without bothering you |

Use `toTurn` for one-shot form submissions (Approve buttons, Confirm
buttons). Use `toShell` to inject text the user can edit before sending.

## Charting

`<EChart>` is available — Apache ECharts under the hood, accepts any
ECharts `option` configuration. References:
https://www.xmlui.org/docs/howto/use-echarts-for-advanced-charting and
https://echarts.apache.org/en/option.html

## Debugging xmlui apps with traces

When something in the right pane misbehaves — a button doesn't fire,
a DataSource shows wrong data, a state change doesn't propagate, a
component renders the wrong way — don't guess from the markup. Ask
the user to reproduce the problem with the Inspector open, then
click its **Export** button. The button writes a JSON trace to
`~/Downloads/xs-trace-<timestamp>.json` (the button briefly flashes
green on success).

Once the export lands, use the xmlui MCP tools to analyze it:

- **`xmlui_find_trace`** — locate the export file by timestamp or
  by content (component name, event kind, etc.).
- **`xmlui_distill_trace`** — reduce a raw trace to the relevant
  interactions, state changes, API calls, and handler boundaries
  for a specific question. Don't try to read the raw JSON yourself;
  it's huge and noisy.

A typical loop:

1. User reports the problem.
2. You: "Open the Inspector (magnifying-glass icon), reproduce the
   problem, then click Export."
3. User clicks Export → file appears in Downloads, button flashes.
4. You: `xmlui_find_trace` to get the path, `xmlui_distill_trace`
   with a question framed around the symptom.
5. Read the distilled output, propose a fix (via the worklist if
   it's multi-step).
