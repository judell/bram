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

For items that span 2+ files, use `files: ["path/a", "path/b"]`
instead of `file`. The TO COMMIT inline diff renders all listed
files concatenated, so the reviewer sees the full scope of the
change. `file` (singular) stays valid for single-file items.

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

**The user is the only one who commits features.** A TO COMMIT item
sits in the working tree indefinitely until an `approved:` payload
covering it arrives — there is no "I should go ahead and commit
this" path short of explicit user authorization. Avoid framing in
chat that makes a feature-level commit sound like a unilateral next
step: don't write "Let me commit X first", "I'll commit X then
propose Y", or "going to land this now". Frame the proposal
instead — "Approve X to commit" or "Once you approve, I'll commit
X" — and leave the trigger to the user. The danger of "Let me
commit X" phrasing is that it nudges the user toward approving a
commit they hadn't yet thought through, defeating the purpose of
the two-stage flow.

The exception is a *minor* change the user explicitly asks you to
commit directly — a typo fix, a one-line doc tweak, a small
correction surfaced in chat ("just commit it", "commit this
directly, no worklist"). In that case the worklist isn't needed
and you can stage + commit immediately. The shape of the request
matters: an explicit "commit this" from the user is authorization;
inferring it from "looks good" or similar feedback is not (see
the *Don't infer commit / drop / advance from feedback* guidance
above).

When you first add items, default to omitting the status (or setting
`"proposed"`). Don't pre-mark things as `"applied"` unless the change
is genuinely already on disk.

You do not need to create `resources/worklist.json` in advance — when
the file is missing, xmlui-desktop serves an empty default. The
Worklist tab can create `resources/worklist.json` (and the enclosing
`resources/` folder if needed) the first time you opt into the
worklist flow.

Lifecycle:

1. **Propose** — write items to `resources/worklist.json`. Each item
   should be small, discrete, and independently rejectable. Writing
   items to the file does **not** mean they are approved — it means
   you are *asking* the user to approve them.
2. **User triages** — unchecks anything they don't want, then clicks
   one of these buttons:
   - *Talk to agent* (with a comment typed above it) → you receive
     `talk: <text>` as a fresh user turn. The user is asking a
     question or giving feedback with **no items approved and none
     dropped**. Respond to the message; do not edit files, do not
     touch `worklist.json`.
   - *Approve selected (N)* — only enabled when ≥1 item is checked.
     You receive `approved: {"items":[{"id":"...","hash":"...","feedback":"..."}, ...]}`.
     The payload is intentionally minimal: ids plus per-item content
     hashes plus optional per-item feedback text (empty string when
     the user didn't expand that item's feedback input), no `before`
     / `after` prose. Per-item feedback is the user's note attached
     specifically to that item — different items can have different
     notes, or none. The PTY watcher verifies each hash against
     `resources/worklist.json` at the moment the line arrives and
     writes the verified item content into
     `resources/.worklist-authorization.json`.
     **To act on the approval, GET `/__worklist/resolve` from the
     loopback HTTP server.** xmlui-desktop injects `XMLUI_DESKTOP_PORT`
     into the PTY child's environment at spawn time, so the agent can
     reach the endpoint without rediscovering the random loopback port:

     ```
     curl -s "http://localhost:$XMLUI_DESKTOP_PORT/__worklist/resolve"
     ```

     If the env var is unset (the rare case where the agent was launched
     outside the wrapped PTY shell), fall back to discovering the port
     via `lsof -nP -iTCP -sTCP:LISTEN | grep xmlui-desktop`. The
     response is one of:
     - `{"kind":"approved", "items":[<full verified content>], ...}` —
       execute these items. The user has already triaged; do NOT
       re-read `resources/worklist.json` to second-guess what was
       approved.
     - `{"kind":"rejected_stale", "mismatched_ids":[...]}` — the
       worklist file changed between the user's click and the watcher
       reading it. Do not edit files; surface the staleness to the
       user and ask them to re-triage.
     Respond to the optional feedback in either case. Never parse the
     `approved:` turn line yourself for content — the line carries
     only ids and hashes.
   - *Drop selected (N)* — only enabled when ≥1 item is checked.
     You receive `drop: {"items":[{"id":"...","hash":"...","feedback":"..."}, ...]}`.
     Same shape, same `/__worklist/resolve` flow:
     `{"kind":"drop"}` → remove those ids from `worklist.json`
     without acting on them; `{"kind":"rejected_stale"}` → surface
     the staleness, do not edit. Respond to any per-item feedback
     (often the user's reason for dropping that specific item).
   - *Iterate (N)* — enabled when an item is selected AND its
     per-item feedback box has non-empty content (no-direction
     Iterate is meaningless and the button reflects that). You
     receive `iterate: {"items":[{"id":"...","hash":"...","feedback":"..."}, ...]}`.
     **Unlike approved/drop, iterate does NOT route through
     `/__worklist/resolve`** — no worklist state change is being
     authorized, so the watcher doesn't write an auth record for it.
     Re-read items directly from `resources/worklist.json` and act
     per each item's current status, scoped by that item's own
     feedback:
     - **`proposed` (TO APPLY):** revise the item's `before` /
       `after` / `files` in `resources/worklist.json` per the
       feedback. Item stays `proposed`. No file edits on disk.
     - **`applied` (TO COMMIT):** edit the on-disk files per the
       feedback AND update the item's `after` (and `files` if scope
       expanded) to reflect the new scope. Item stays `applied`.
     Iterate is the channel for "refine in place" — the user wants
     to keep working these items without yet approving or dropping.
     After iterating, the items are ready for the user to re-triage
     on the next click.

     The Worklist tab's spinner clears when your turn ends with
     `stop_reason: end_turn` — the agent-idle ChangeListener on the
     session JSONL is the primary clear path. There's also a 60s
     stale-timeout backstop for cases where that chain breaks. End
     your turn cleanly with a text response so the user's spinner
     clears promptly.
3. **Prune** — after either action, rewrite `resources/worklist.json`
   with only the still-pending items. The worklist is *pending* work,
   not history; completed items belong in commit messages.
4. **Empty state is fine** — leave it as `{ "description": "", "items": [] }`.

If you ever do receive `approved: {"items":[]}`, `drop: {"items":[]}`,
or `iterate: {"items":[]}` (shouldn't happen — the buttons are
disabled when nothing is checked — but be defensive), treat it the
same as `talk:` — feedback only, take no action.

**Don't infer commit / drop / advance from feedback.** When the user
says things like "looks good", "seems pretty good", "it works", or
sends a voice-dictated test phrase that begins with `voice: ...`, do
**not** read that as authorization to commit applied items, drop
proposed items, or otherwise advance worklist state. Wait for the
user to *explicitly* ask (e.g., "commit it", a structured `approved:`
payload listing the items). Voice content arriving as `voice: ...` is
user speech, treated the same as typed talk — informational with
respect to *worklist state advancement only*. Direct task requests
delivered by voice — `voice: create foo.txt`, `voice: fix the bug in
X`, `voice: explain Y` — are acted on the same as if typed. The
prefix is a transport marker (the user dictated instead of typed); it
is not a refusal trigger. If a verbal phrase is ambiguous, ask one
focused question instead of acting.

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

**Notice when sibling commits should be squashed (post-hoc).** The
"hold the commit" rule above prevents the split proactively. When it
*doesn't* fire — usually because the user said "commit directly" on
one half before approving the other through the worklist — you end up
with two consecutive unpushed commits that together form one feature.
Watch for this signal: the most recent two unpushed commits are a
mechanism + the config that exercises it (or a backend route + the
frontend that calls it, or a struct + the only code that constructs
it). Either commit alone is dead weight; together they're the feature.

When you spot this, flag it to the user before they push: "`<sha1>`
and `<sha2>` are two halves of the same feature — want to squash them
into one commit?" If they say yes, and **both commits are unpushed**:

```
git reset --soft HEAD~2     # keeps both diffs staged
git commit -F <new-msg>     # one combined commit
```

Verify with `git log --oneline -3` and `git log --oneline @{u}..HEAD`
that the combined commit is unpushed and replaces the prior two. Never
squash already-pushed commits without explicit force-push consent — the
soft-reset approach is only safe on unpushed history.

When *not* to use this: one-or-two-item decisions, free-text input, or
anything where typing in chat is faster than rendering UI.

### Refer to items by id, not by ordinal

When you mention worklist items in chat, name them by their `id`
verbatim (e.g. `codex-launcher-require-hook`), never by ordinal
position ("item 3", "items 3 and 5", "the second one"). Numbers are
unstable — they shift as items are approved, applied, dropped, or
pruned, and they don't match what the user sees in the Worklist tab
UI which is keyed by id. Ids stay stable across the item's lifetime
and are what the structured `approved:` / `drop:` payloads
reference, so naming them keeps chat aligned with both the UI and
the authorization channel.

### When to route through the worklist

**Default: every change request goes through `resources/worklist.json`.**
Single-file, single-line, single-attribute — size doesn't matter.
Propose first, wait for the user's `approved:` payload, then apply.
The two-stage proposed → applied flow lets the user redirect or veto
before any code is touched, and the worklist history serves as the
audit trail for what landed and why.

Skip the worklist only in these specific contexts, never because the
change is "small":

- **Explicit user opt-out in this turn.** The user types something
  like "just do it", "commit directly, no worklist", "inline the
  fix", "no worklist for this", "skip the worklist". The opt-out
  must be in the same turn as the change request — don't carry it
  forward across turns or infer it from past patterns. "Looks good"
  is not opt-out (see *Don't infer commit / drop / advance from
  feedback*).
- **Correcting code you just wrote in the current iteration.**
  If you wrote a typo or off-by-one in the last assistant turn and
  you're fixing it on the next turn, that's iteration on
  in-progress work, not a fresh change request. Direct fix is
  fine.
- **Iterating on an uncommitted draft.** If the user and you are
  bouncing edits on a file that hasn't been committed yet — e.g.,
  shaping a new component during the same conversation — direct
  edits keep the loop tight. Once the draft is committed, fresh
  edits become change requests and route through the worklist.

Worked examples:

- "let's fix the top row layout" → propose (fresh change request;
  size is irrelevant).
- "center it to match" as a follow-up to a clarifying question
  about the top-row layout → propose (clarifying didn't authorize
  direct edit; it just resolved ambiguity).
- "oh wait, you wrote `intialValue` — typo on line 12" → direct fix
  (correcting in-progress code from the previous turn).
- "fix the off-by-one in the loop you just added" → direct fix
  (same).
- "fix the top row layout, just do it, no worklist" → direct edit
  (explicit opt-out in the same turn).
- "add a chart" + "let's fix the layout too" → propose (multi-step,
  but this was always the case).

### Match prose verbosity to change complexity

Match `before` / `after` prose to the size and judgment-load of the
change. Two regimes:

**Small, mechanical changes** — single-file tweak, typo fix, one-line
CSS adjustment, rename, clear bug with one obvious fix. A short
paragraph or two for `before` and `after` is enough. Don't pad with
alternatives-considered when there was effectively one path; the
commit message + diff already carry the audit trail.

**Complex or judgment-load changes** — anything where several
reasonable approaches existed and you had to choose, anything
touching multiple files in non-mechanical ways, anything whose
*why* will fade from memory in a month. Name the alternatives you
considered and why you rejected them; mark `[chosen]` on the
picked path. For example:

> Alternatives considered:
>
> - Embedded diff via DataSource — rejected: each row would fire its own request.
> - Full-tree diff at the top of the worklist — rejected: hides per-item attribution.
> - **[chosen]** Server augmentation via `/__worklist` — single payload, per-item diffs travel with each row.

This is the audit trail future-agent grep will hit (especially when
the repo commits `docs/worklist-history.md`) — earn it.

If in doubt: would a reader six months from now reconstruct the
decision from the current code + git log alone? Yes → short.
No → fulsome.

### Use Markdown in item prose

Worklist `before` / `after` fields and worklist-history `changelog`
entries render as Markdown in the agent-tools drawer, so use real
Markdown syntax instead of inline ad-hoc formatting:

- Bullet lists need `- ` (or `* `) at the start of each line; inline
  enumerations like `(a) ... (b) ... (c) ...` collapse into a
  single run-on paragraph and lose the scanning affordance.
- Numbered steps use `1.` / `2.` per line.
- Inline code references (file paths, identifiers, attribute names)
  belong in single backticks so they render monospace and stay
  greppable.
- Multi-line code or markup snippets belong in fenced code blocks.
- Blank lines separate paragraphs; a lone newline inside a paragraph
  is just a soft wrap.
- `*emphasis*` and `**strong**` work for the rare term that needs
  to stand out (e.g., **[chosen]** to mark the picked alternative).

### Test Worklist UX through the worklist itself

When a change touches the Worklist UX itself (Approve/Drop button
states, gray-out behavior, feedback flow, worklist-pruning), prefer
to surface it as a pending item even when the diff is already on
disk. Approving the item then exercises the new behavior end-to-end
— your file rewrites, the worklist pruning, the Talk-page update —
which is the actual test.

**Enforcement layers.** xmlui-desktop records structured `approved:` /
`drop:` payloads in `resources/.worklist-authorization.json`. That is
the provider-neutral authorization record for worklist state changes.
Claude installs a PreToolUse hook at `.claude/hooks/worklist-guard.py`
for `Write` / `Edit`, and Codex installs its own native PreToolUse hook
through `~/.codex/config.toml` to cover `apply_patch`, mutation-shaped
Bash, and filesystem/MCP writes. Both hooks validate worklist coverage
before the tool runs. The desktop watcher remains as fallback coverage:
it compares the old/new worklist snapshots and rewrites the old file
back if the prune was not authorized or if a hook failed to launch. If
you hit either path, read the error or revert message; it is the
convention's enforcement mechanism, not a bug to work around.

**Don't ask before editing the worklist.** The write channel is already
approved and guarded. Claude's allow-listed `Write(./resources/worklist.json)`
and `Edit(./resources/worklist.json)` calls are validated by its hook,
and Codex mutations are validated by its matching PreToolUse hook. The
shared authorization record plus watcher fallback remain behind both.
Either way, there is no need to verbally confirm with the user before
adding, advancing, or pruning worklist items — unsafe removals will be
rejected or reverted.
Save the verbal back-and-forth for design decisions (which items to
propose, what choices to bake in), not for the mechanical write.

**Minimize the bytes of each worklist edit.** Worklist items often have
multi-paragraph `before` / `after` prose. A naive `Edit` with the
whole item as `old_string` and the slightly-changed item as
`new_string` doubles the per-edit token cost and floods the user's
transcript with redundant text. Prefer:

- Narrow `Edit` targets for the smallest possible string that uniquely
  identifies the change. Flipping `"status": "proposed"` → `"status":
  "applied"` only needs those two strings, not the whole item.
- When you're appending to an item's `after` (e.g., adding a new
  sub-section after an iterate), `old_string` is the last paragraph
  you want to preserve and `new_string` is that same paragraph plus
  the appended content — not the entire `after`.
- Full-item rewrites (`Write` over `worklist.json` from scratch) are
  fine for batch operations like pruning multiple items at once, but
  avoid them for single-status flips or one-paragraph tweaks.

The hook validates the resulting file regardless of edit shape, so the
choice is purely about token economy and transcript noise.

**Don't `grep -n` a single-line JSON file** (like `worklist.json`) to
find an anchor for an `Edit`. The matching "line" *is* the whole
file, and the grep tool result dumps it into the transcript. Find
your anchor by recalling the structure from prior turns, using
`Read` with `offset`/`limit`, or `jq` to extract just what you need.

**Don't update an item's `after` field on every iterate.** Small
refinements during TO-COMMIT iteration don't need an audit trail in
the worklist — the commit message captures the final state and the
file diff is reviewable in git. Only update `after` when scope
materially expands (new file added to `files`, or the change's
intent shifts). Otherwise leave it; the iteration history doesn't
need to live in the item.

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

## Updating GitHub issues via gh

For changes to an existing issue, use the `gh` CLI directly — no
need to explore `gh issue --help`:

- Edit title/body: `gh issue edit <n> --title "…" --body "…"`
- Add a comment: `gh issue comment <n> --body "…"`
- Change state: `gh issue close <n>` / `gh issue reopen <n>`

The Issues tab polls every 30s and refetches the expanded issue's
body + comments, so updates surface in xmlui-desktop without a
restart.

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
