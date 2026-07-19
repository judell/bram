# PTY menu shapes — hook (PreToolUse / PermissionRequest) catalog

**Parallel to [`docs/pty-menu-shapes.md`](./pty-menu-shapes.md).** That doc
catalogs interactive menus by their *rendered screen shape* (the xterm grid
axes: `cursor` / `header` / `1./2. pair` / `footer` / `keyword guard`). This
doc catalogs the same menus by their *structured hook payload* — what a Claude
Code `PreToolUse` / `PermissionRequest` hook sees at pose-time, before the
prompt renders. The hook-driven permission-menu surfacing (`menus.hookDriven`,
`app/provider-hooks/claude-permission-menu-hook.py`, `/__menu/permission`) builds the
agent-pane menu from these fields instead of scraping the grid.

**Claude and Codex.** Claude contributes permission prompts plus
AskUserQuestion. Codex contributes approval prompts through
`PermissionRequest` and clear events through `PostToolUse` /
`PermissionDenied`; it has no AskUserQuestion equivalent.

## The identifying fields

A menu is identified not by screen literals but by structured fields on the
hook payload:

- **`hook_event_name`** — `PermissionRequest` (a permission dialog is about to
  show — Family A) or `PreToolUse` (a tool call; e.g. AskUserQuestion —
  Family B).
- **`tool_name`** — `Bash` / `Edit` / `Write` / `NotebookEdit` / `Skill` /
  `WebFetch` / `AskUserQuestion` / `mcp__*` …
- **`permission_mode`** — `default` / `acceptEdits` / `plan` /
  `bypassPermissions` / `dontAsk` / `auto`. Gates whether *any* menu fires:
  `auto` / `dontAsk` / `bypassPermissions` run silently (no
  `PermissionRequest`), so it's a per-payload axis, not a shape.
- **`permission_suggestions`** — a tagged union (by `type`) of the allow-all
  options. With one exception (`setMode`, below) the rendered option count =
  `Yes` + (non-`setMode` suggestions) + `No`.
- **`tool_use_id`** (`tuid`) — present on `PreToolUse` and `PostToolUse`, not
  on `PermissionRequest`. A `PostToolUse` with the same `tuid` as a
  `PreToolUse` proves the call ran (approved); absence proves it was denied.

The screen-axis vocabulary of the parallel doc has **no analog here** — it's
replaced by the fields above. Version-pinned header-fragment literals are not
needed.

## `permission_suggestions` union (observed 2026-06-28 → 06-29)

| `type` | fields | rendered as | seen on |
| --- | --- | --- | --- |
| `addRules` | `behavior:"allow"`, `rules:[{toolName, ruleContent?}]` | one option per entry; label from `ruleContent` (cmd / glob) or `toolName` | Bash non-file cmd (`ruleContent`=cmd); MCP (`toolName` only, **no** `ruleContent`); Skill (**two** entries: `ruleContent` exact + `:*` glob) |
| `addDirectories` | `directories:[…]`, `destination` | one option, *"…allow access to `<dir>/`"* | Write / Edit / NotebookEdit (file-edit family) |
| `setMode` | `mode:"acceptEdits"`, `destination` | **no own option** — folds into the option-2 `(shift+tab)` affordance | file-edit family in `default` mode (paired with `addDirectories`) |
| *(frontier)* | ? | ? | connection / API-key — unconfirmed |

**Suggestion type tracks the operation, not the tool.** `tool_name` does **not**
determine the suggestion `type`. A Bash command that reads a path can yield an
`addRules` with `toolName:"Read"` and `destination:"session"` (a read-scoped
cross-tool grant); a Bash non-file command yields `addRules` with the command
in `ruleContent`.

### Count rule and its one break (`setMode`)

Build **one option per suggestion entry, regardless of `type`** — even an
unrecognized `type` then yields the right option count at the right positions,
so the pane's number-keystroke stays aligned with the terminal (only the
synthesized *label* may be generic). **The sole exception is `setMode`:** the
CLI does not render it as its own numbered row — it collapses into the
`(shift+tab)` hint on the directory-grant option. So the rule is *one option
per non-`setMode` suggestion*. This is exactly why a raw `len(suggestions)` of
2 (`setMode` + `addDirectories`) renders as **3** options, not 4 — the
count-rule-breakage case the parallel doc flags for the file-edit family.

## Confirmed shapes (Claude)

| Family / shape | event | `tool_name` | `permission_mode` | suggestions | rendered opts | evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Bash non-file cmd | `PermissionRequest` | `Bash` | default | `addRules` (toolName + ruleContent=cmd) | 3 | sw_vers probe 06-28 |
| Bash reads a path | `PermissionRequest` | `Bash` | default | `addRules` (toolName=`Read`, ruleContent=glob, dest `session`) | 3 | grep `~/.claude/**` 06-29 |
| Bash 2-option safety | `PermissionRequest` | `Bash` | acceptEdits | `[]` | 2 | `[hook-menu] options=2` traces |
| File edit (default mode) | `PermissionRequest` | `Write`/`Edit`/`NotebookEdit` | default | `setMode` + `addDirectories` | **3** (setMode folds) | Desktop probe 06-29 |
| File edit (acceptEdits mode) | `PermissionRequest` | `Write`/`Edit`/`NotebookEdit` | acceptEdits | `addDirectories` only | 3 | Desktop write 06-29 |
| MCP tool | `PermissionRequest` | `mcp__*` | default | `addRules` (toolName only, no ruleContent) | 3 | `mcp__xmlui__xmlui_search` 06-29 |
| Skill | `PermissionRequest` | `Skill` | default | `addRules` ×2 (ruleContent exact + `:*` glob) | **4** | `Skill(claude-api)` 06-29 |
| AskUserQuestion (Family B) | **`PreToolUse`** | `AskUserQuestion` | any | n/a — options in `tool_input.questions[].options[].label` (+ `multiSelect`) | per payload | probe 06-28 |

**Skill is the only multi-suggestion shape** and the clearest proof that the
parser must count by suggestions, not assume 3: two `addRules`
(`claude-api`, `claude-api:*`) → 4 rendered options. **`NotebookEdit` is not a
distinct type** — it is the Write/Edit file-edit family
(`setMode` + `addDirectories`).

## Grid sees it, drops it; hook catches it

The headline case for the hook path. On 2026-06-29 a `NotebookEdit` confirmation
on `~/Desktop/probe.ipynb` was a **grid miss**: the scanner saw the menu
(`menu_bearing=true`, `header=true`, `cursor=true`, byte-perfect `excerpt=`)
but `op=skip`'d it on the lone `numbered=false` gate — because the notebook
diff's own gutter line-numbers (`1`, `2`, `3`) collided with the option
anchors, splitting them across rows (`opt_anchors=1:…@0:38, 2:…@857:390,
3:…@857:459` — option 1 anchored to a spurious early `1`). At the *same
instant*, the hook captured it cleanly:

```
NotebookEdit | PreToolUse + PermissionRequest + PostToolUse(ran=True)
suggestions: [ {type:setMode, mode:acceptEdits, dest:session},
               {type:addDirectories, directories:["…/Desktop"], dest:session} ]
```

No grid, no numbers to collide, no contiguity gate. **Every** Write/Edit/
NotebookEdit confirmation renders a numbered diff in its body, so this
collision is a latent miss for the whole file-edit family — which is the
concrete argument for making the **file-edit family hook-authoritative** when
`menus.hookDriven` is on. Grid detail:
[`docs/pty-menu-shapes.md`](./pty-menu-shapes.md) → *Known edge — option
numbers collide with a numbered body*.

## Frontier (still unconfirmed)

- **connection** ("allow this connection?") — tool + suggestion type unknown
  (a network grant — likely a new union `type`).
- **API key** — may not surface as a tool `PermissionRequest` at all.
- **`bypassPermissions` / `auto` / `dontAsk` modes** — a *negative* data point,
  not a shape: no `PermissionRequest` fires; the menu is silenced entirely.
  Confirms `permission_mode` gates emission.

## Exclusions are free

The screen catalog's *Exclusions* (onboarding/trust two-button confirms — "Yes,
I trust these settings / No, exit") and its known edges (self-reference
collision, wrap/blend corruption, **the file-edit gutter-number collision
above**) **do not exist from the hook perspective**: trust confirms are not
tool calls or permission requests, so no hook fires for them; the collision and
wrap edges are screen-reading artifacts with no analog in a structured payload.
The grid had to actively guard against the trust confirms (#187) and now misses
file-edit menus on gutter collisions; the hook simply never has either problem.

## How this doc is populated

A passive capture hook (`~/.claude/hook-catalog-capture.py`, registered for
`PreToolUse` + `PermissionRequest` + `PostToolUse` + `PermissionDenied` +
`Notification` on all tools) appends one compact JSONL record per event to
`~/.claude/hook-catalog-capture.jsonl`:
`{ts, event, tool, tuid, perm_mode, ti_keys, suggestions, cmd_head|file,
question/options/multiSelect, ran/is_error/out_keys}`. Running it through
normal work accumulates real payloads; periodically we grep the log for new
`tool_name` / suggestion-`type` combinations and promote confirmed shapes from
*Frontier* into *Confirmed*. Remove the hook (and this section) once the union
is settled.

Codex capture is built into `app/provider-hooks/codex-permission-menu-hook.py`: every
`PermissionRequest` appends a compact shape record to
`resources/codex-permission-hook-capture.jsonl` while also forwarding the menu
payload to Bram.

## Confirmed shapes (Codex)

Codex is hook-primary for permission menus when `menus.hookDriven` is on. The
Codex hook forwards `PermissionRequest` payloads to `/__menu/permission` and
uses `PostToolUse` / `PermissionDenied` as clear signals. The hook records
compact payload shapes in `resources/codex-permission-hook-capture.jsonl`;
the human-visible option labels are cataloged in
[`docs/codex-permissions.md`](./codex-permissions.md).

| Family / shape | event | `tool_name` | rendered opts | evidence |
| --- | --- | --- | --- | --- |
| Exec / command approval | `PermissionRequest` | `Bash` | 2+ | direct Bash probes, including denied commands |
| File change approval | `PermissionRequest` | `apply_patch` / file-edit tools | 3 | Desktop and project file edit probes |
| MCP tool approval | `PermissionRequest` | `mcp__*` | provider-defined | filesystem MCP probe |
| Long-tail / quoting approval | `PermissionRequest` | `Bash` | provider-defined | quoted command probe |

The xterm grid path remains as the fallback and as diagnostic coverage for
unknown prompt shapes. Unlike Claude AskUserQuestion, Codex has no question
tool payload for Bram to surface as a choice menu.
