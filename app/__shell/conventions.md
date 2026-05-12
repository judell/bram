# Working with xmlui-desktop

xmlui-desktop is a **workspace for XMLUI development with AI agents**.
The shell puts a real terminal alongside an XMLUI surface, plus an
"agent tools" drawer that includes a Worklist (pending items + commits),
a Sessions browser, and a Context viewer (CLAUDE.md + memory + hooks +
settings, searchable). The user sees the right pane while talking to
you — use it.

> Note on memory: this file is loaded into every session in this
> project via a `@`-import in `CLAUDE.md`. **Don't save project-related
> memories** — preferring the worklist, helper APIs, release quirks,
> conventions you discover, etc. Per-user memory is private to one
> agent on one machine; this file is shared with everyone running
> xmlui-desktop. When you learn something worth keeping for future
> sessions, add it here so the whole community gets it. Memory stays
> reserved for things that genuinely can't live in the project repo
> (cross-project user preferences, etc.).

## Naming and user-facing copy

- **Don't call xmlui-desktop an IDE** in user-facing copy (README, UI
  strings, manual.md). Frame it as a workspace, desktop shell, or
  describe what it does. Don't recommend external IDE tooling
  (rust-analyzer, VS Code extensions) in this project's docs —
  xmlui-desktop is the workspace.
- **Don't call this repo a "dogfood project"** or use similar
  internal-team jargon in committed text. It's the xmlui-desktop
  project; users developing their own XMLUI app launch xmlui-desktop
  in their own project directory.

## Render structured output in the right pane

When the user asks for something that benefits from structured output
(tables, lists, charts) or structured input (selectors, forms,
multi-step flows), edit `Main.xmlui` (or a file under `components/`)
so the right pane renders it. A filesystem watcher reloads the iframe
automatically — you don't need to ask the user to refresh.

## Coordinate via worklist.json

`resources/worklist.json` is the canonical surface for multi-step
coordination between you and the user. The Worklist tab in the agent
tools drawer renders it as a checklist under "Worklist".

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

The `status` field controls the badge in the Worklist tab and what
the user is being asked to approve:

- `"proposed"` (the default if omitted) → badge **TO APPLY**. The user
  is approving you to *make* the change. After they approve, apply
  the edits, then **re-add the same item with `status: "applied"`** —
  do not prune yet.
- `"applied"` → badge **TO COMMIT**. The change is on disk and you're
  asking the user to approve a `git commit`. After they approve,
  create the commit and prune the item from `worklist.json`. Push is
  decided separately via the "Push N unpushed commits" button.

Default to the two-stage flow: every approved `proposed` item should
transition to `applied` before being pruned, so the user explicitly
approves both the edit and the commit. Skip the `applied` stage only
if the user says "apply and commit" (or similar) up front. Dropped
items are pruned directly with no `applied` stage.

When you first add items, default to omitting the status (or setting
`"proposed"`). Don't pre-mark things as `"applied"` unless the change
is genuinely already on disk.

You do not need to create `resources/worklist.json` in advance — when
the file is missing, xmlui-desktop serves an empty default and the
Worklist tab shows *(none)*. Just write to the file the first time
you actually have items to propose; xmlui-desktop will create it.

Lifecycle:

1. **Propose** — write items to `resources/worklist.json`. Each item
   should be small, discrete, and independently rejectable. Writing
   items to the file does **not** mean they are approved — it means
   you are *asking* the user to approve them.
2. **User triages** — unchecks anything they don't want, then clicks
   one of three buttons:
   - *Talk to agent* (with a comment typed above it) → you receive
     `talk: <text>` as a fresh user turn. The user is asking a
     question or giving feedback with **no items approved and none
     dropped**. Respond to the message; do not edit files, do not
     touch `worklist.json`.
   - *Approve selected (N)* — only enabled when ≥1 item is checked.
     You receive `approved: {"items":[...], "feedback":"..."}`.
     **Execute ONLY the items in that JSON array — do NOT re-read
     `resources/worklist.json` to figure out what to do.** The user has
     already triaged; items they unchecked are deliberately absent from
     the array even though they're still in the file. Treat the array
     as authoritative; treat `worklist.json` at this moment as stale.
     Respond to the optional feedback.
   - *Drop selected (N)* — only enabled when ≥1 item is checked.
     You receive `drop: {"ids":[...], "feedback":"..."}`. Remove the
     listed ids from `worklist.json` without acting; respond to the
     optional feedback.
3. **Prune** — after either action, rewrite `resources/worklist.json`
   with only the still-pending items. The worklist is *pending* work,
   not history; completed items belong in commit messages.
4. **Empty state is fine** — leave it as `{ "description": "", "items": [] }`.

If you ever do receive `approved: {"items":[]}` or
`drop: {"ids":[]}` (shouldn't happen — the buttons are disabled when
nothing is checked — but be defensive), treat it the same as
`talk:` — feedback only, take no action.

**Don't infer commit / drop / advance from feedback.** When the user
says things like "looks good", "seems pretty good", "it works", or
sends a voice-dictated test phrase that begins with `voice: ...`, do
**not** read that as authorization to commit applied items, drop
proposed items, or otherwise advance worklist state. Wait for the
user to *explicitly* ask (e.g., "commit it", a structured `approved:`
payload listing the items). Voice content arriving as `voice: ...` is
user speech, treated the same as typed talk — informational, not an
instruction to act on the worklist. If a verbal phrase is ambiguous,
ask one focused question instead of acting.

**Hold the commit while a related TO APPLY item is in flight.** When
the worklist contains both a TO COMMIT item and a TO APPLY item that
touch the same surface (e.g., a feature plus a tuning adjustment, a
fix plus a follow-up regression patch), do **not** process the commit
when the user's `approved:` payload happens to cover both. Apply the
proposed item only; leave the prior item in TO COMMIT. The user
verifies the combined behavior, then approves a single commit covering
both. This avoids landing intermediate "kinda-works" commits where
the feature is split from its companion fix — those make git history
hard to read and bisect against.

When *not* to use this: one-or-two-item decisions, free-text input, or
anything where typing in chat is faster than rendering UI.

### When to route through the worklist

Default to proposing items in `resources/worklist.json` whenever a
change spans more than one file, or has more than ~2 discrete
sub-edits in a single file, even after the user has verbally said
"do it". The two-stage proposed→applied flow lets the user uncheck
individual sub-edits before any code is touched. Single-file tweaks,
typo fixes, and direct corrections to in-progress code can still be
edited directly.

### Test Worklist UX through the worklist itself

When a change touches the Worklist UX itself (Approve/Drop button
states, gray-out behavior, feedback flow, worklist-pruning), prefer
to surface it as a pending item even when the diff is already on
disk. Approving the item then exercises the new behavior end-to-end
— your file rewrites, the worklist pruning, the Talk-page update —
which is the actual test.

**Hook enforcement.** xmlui-desktop installs a PreToolUse hook at
`.claude/hooks/worklist-guard.py` that validates Write/Edit operations
on `resources/worklist.json`. If you remove an item without an explicit
`drop:` authorization in the user's last message, the harness rejects
the write with a stderr message explaining the violation. Read it; the
hook is the convention's enforcement mechanism, not a bug to work
around.

## Right-pane helpers (opt-in, only needed for project-side hooks)

The Worklist and Sessions tabs in the agent tools drawer already use
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

## UI patterns

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

### Keep empty scaffolding files

When emptying a file that's part of an XMLUI app's expected structure
(`Globals.xs`, components referenced from `Main.xmlui`, etc.), keep
the empty file rather than deleting it — the slot signals where
future code can land. Distinguish between files whose existence is
incidental (orphan components, dead scripts: delete them) and files
whose existence is structural (expected entry points, conventional
configs: empty them out and leave the file in place). When in doubt,
ask.

## Citing XMLUI docs

When citing an XMLUI component or howto, the canonical URL form is:

- Components: `https://www.xmlui.org/docs/reference/components/<Name>`
- Howtos: `https://www.xmlui.org/docs/howto/<slug>`

The `xmlui-mcp` server's `Source:` lines and "Documentation URLs:"
footers print the `docs.xmlui.org/...` form, which 404s on the live
site. Rewrite before citing: `docs.xmlui.org/<path>` →
`www.xmlui.org/docs/reference/<path>` (the `reference/` segment is on
the working URLs).

## Build vs. runtime-served files

The xmlui-desktop binary ships with the `app/` tree embedded at build
time (Tauri's `frontendDist: "../app"`). At runtime, xmlui-desktop
prefers an on-disk `app/` next to the binary if present, otherwise
falls back to the embedded copy.

A filesystem watcher (in `src-tauri/src/lib.rs`) watches three
directories under the on-disk `app/` and reloads iframes when they
change:

| path | reloads |
|---|---|
| `app/__shell/` | both iframes (right pane and agent-tools drawer) |
| `app/vendor/` | both iframes |
| `app/tools/` | the agent-tools drawer iframe only |
| user's project directory | the right-pane iframe only (drawer stays put) |

Files under those paths are hot-reloaded — no rebuild, no restart.

The **parent shell** (`app/index.html`, `app/main.js`, `app/styles.css`,
plus anything else loaded once at WebView startup) is **not**
hot-reloaded. After editing those, run `cargo build` and have the
user restart. Don't suggest `cargo run` as an alternative — the user
prefers rebuild + restart, and the incremental build is fast.

## Building and releasing

Debug builds are the shipping format. Don't propose `cargo build
--release`, code signing, notarization, or installer pipelines. The
Rust side is thin glue (PTY relay, loopback HTTP file server, small
git/sessions queries); the heavy lifting is XMLUI's TypeScript
runtime in the WebView, which is identical between debug and release.
The audience is XMLUI developers, who benefit from devtools being
accessible.

Cutting a new release:

1. Bump `version` in `src-tauri/Cargo.toml` and
   `src-tauri/tauri.conf.json`.
2. Run `cargo build` to refresh `Cargo.lock`.
3. Commit, then create the `vX.Y.Z` tag locally
   (`git tag vX.Y.Z <release-commit>`).
4. Push the commits via the agent-tools drawer's "Push N unpushed
   commits" button, then push the tag separately with
   `git push origin vX.Y.Z`. **The push button does not follow
   tags** — `git ls-remote --tags origin vX.Y.Z` after clicking it
   will return empty until you push the tag explicitly. The workflow
   in step 5 needs the tag to exist on origin.
5. Manually dispatch `.github/workflows/build.yml` from the GitHub
   Actions UI with the tag string. The workflow is `workflow_dispatch`
   only — it builds debug binaries for linux-amd64, macos-arm64,
   macos-intel, and windows-amd64, generates SHA256SUMS, and attaches
   `install.sh` / `install.ps1`.

It's fine to leave `#[cfg(debug_assertions)]` gates in code (e.g.,
`open_devtools`) — they work in the only build we ship.

## Compressing screencasts for the README

GitHub README videos are capped at ~10 MiB. Screencasts of
xmlui-desktop are mostly static UI with the occasional cursor or text
update — that's a profile h264 handles extremely well with the right
flags. The recipe:

```
ffmpeg -i INPUT.mp4 -vf "fps=8" \
  -c:v libx264 -preset slow -tune stillimage -crf 37 -pix_fmt yuv420p \
  -c:a aac -ac 1 -b:a 48k -movflags +faststart OUTPUT.mp4
```

Why each flag earns its place:

- `-tune stillimage` is the big win. It tells x264 the source is
  near-static, so it spends bits on text sharpness instead of motion.
  Without it, CRF this aggressive smears UI text.
- `fps=8` is fine for screencasts — most frames are identical anyway.
- `-crf 37` is the starting point. Lower for sharper text, higher for
  smaller file. CRF 40 is the floor before text starts to suffer
  visibly. Each step of CRF roughly changes file size by ~12%.
- `-ac 1 -b:a 48k` mono AAC — voiceover is intelligible at this
  setting. Drop `-c:a aac -ac 1 -b:a 48k` and add `-an` to strip
  audio entirely (buys ~3 MiB of headroom for video quality).
- `-movflags +faststart` puts the moov atom up front so the file
  starts playing before fully downloaded.

Tuning loop: encode once, check size, adjust CRF up or down by 1-2.
Native resolution is what makes text legible, so don't downscale
unless you've already pushed CRF to 42-44 and are still over budget.

GitHub has accepted slightly over the nominal 10 MiB limit in
practice (10.4 MiB landed fine), so a result of 10.0-10.3 MiB is
usually safe — but aim for 9-10 MiB to stay clear of the line.

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
