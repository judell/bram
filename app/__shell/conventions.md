# Working with Bram

Bram is a **workspace for AI-assisted web app development** — it
works with any project, whatever it serves. The shell puts a real
terminal alongside an "agent tools" pane that includes a Worklist
(pending items + commits), a Sessions browser, and a Context viewer
(CLAUDE.md + memory + hooks + settings, searchable).

Bram can *optionally* embed a **target app** — a project iframe that
previews a web UI inside Bram (vanilla HTML/JS, a React or other Node
app, a Python web app, an XMLUI app, etc.). This pane is **off by
default** and is a minority case: most users run their own app in
their own server and view it in their own browser, so the embedded
preview is reserved for a simple app or a quick check. Don't assume
an iframe is present — detect before you rely on it.

> Note on memory: this file is loaded into every session in this
> project via a `@`-import in `CLAUDE.md`. **Don't save project-related
> memories** — preferring the worklist, helper APIs, release quirks,
> conventions you discover, etc. Per-user memory is private to one
> agent on one machine; this file is shared with everyone running
> Bram. When you learn something worth keeping for future
> sessions, add it here so the whole community gets it. Memory stays
> reserved for things that genuinely can't live in the project repo
> (cross-project user preferences, etc.).

Bram's own UI is XMLUI. When developing Bram, expect the
XMLUI MCP server to be available, read the xmlui_rules,
and follow them. The same holds if the app under development
is XMLUI.

### Guard source of truth

When editing Bram's Claude worklist guard in this source repo,
`app/__shell/worklist-guard.py` is canonical. The runtime copy at
`.claude/hooks/worklist-guard.py` is an installed artifact that Setup
and `src-tauri/build.rs` refresh from that canonical source. Do not
make functional edits in `.claude/hooks/worklist-guard.py`; they will
either be reported as setup drift or overwritten by the next sync.

The Codex guard has a separate source/installed split:
`app/shell/worklist-guard-codex.py` is canonical, while
`~/.bram/codex-worklist-guard.py` is the installed runtime copy.

### XMLUI lookup order

When you are figuring out how to do a thing in XMLUI, ask the XMLUI
MCP server for how-to documents first (`xmlui_search_howto`). The
how-to corpus usually carries the complete pattern and tradeoffs.
After that, use `xmlui_component_docs` for exact component props,
events, and exposed methods. Use examples as a fallback or to confirm
local style, not as the first source of truth.

When a non-obvious markup choice depends on documentation, cite the
relevant how-to or component URL in the response.


## Code organization (helpers.js / Globals.xs / window)

Iframe-side code spans four surfaces. The rules below describe where
each kind of code should live, and how XMLUI markup calls into it.

### The surfaces

- **`app/__shell/helpers.js`** — real JavaScript. Async, `fetch`,
  `setTimeout`, `postMessage`, tauri event listeners — anything the
  XMLUI expression engine can't host directly. Functions live on
  `window` (see naming below) and are reached from XMLUI markup as
  `window.foo(...)`. Reload behavior depends on which Bram binary is
  running; see *Build vs. hot-reload boundary*.
- **`app/tools/Globals.xs`** — XMLUI's expression engine context.
  Holds xs-scope module state (vars whose readers/writers all live in
  xs) and the few helpers whose proximity to that state earns them a
  place here. Engine restrictions: no async/await, no setTimeout, no
  fetch, no Promise chaining outside DataSource. Top-level
  `function foo(...)` declarations auto-hoist onto `window.foo`.
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
  bodies, never object-literal blobs. The past failure modes are
  catalogued in *Failure modes that informed these rules* below.

### When and why do we need delegators?

A *delegator* is `function foo(...) { return window.__bramFoo(...); }`
in `Globals.xs`. Its only purpose is to let XMLUI markup write the
bare name `foo(...)` instead of `window.__bramFoo(...)`.

**Default: don't add one.** Call helpers as `window.foo(...)` from
XMLUI markup. This includes inside arrow-function bodies passed to
`subscribeTauriEvent` / `onDidChange` / `onLoaded` etc. — the engine
analyzes the *qualified* `window.foo` member access without trouble.
The historical bite (a bare `foo` inside an arrow body silently
aborts identifier analysis when no xs declaration exists) is avoided
by writing `window.foo` rather than by adding a delegator.

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

Captured in user memory (cross-project, not in this file):

- `feedback_xmlui_no_complex_expressions_in_attributes` — keep
  attribute expressions to a single function call.
- `feedback_xmlui_arrow_body_needs_xs_decl` — arrow-body identifier
  analysis. Refined fix: use `window.foo()` rather than bare `foo()`
  inside arrow bodies. Adding an xs declaration also works, but
  costs.
- `feedback_xs_to_window_migration_name_collision` — the `__bramFoo`
  prefix is for names that have an xs counterpart, not a blanket
  rule. Same evidence, sharper edge.
- `feedback_helpers_js_load_order` — new top-level *calls* in
  helpers.js must come after the function they invoke; load-time
  throws abort the whole file.


## Coordinate via worklist.json

`resources/worklist.json` is the canonical surface for multi-step
coordination between you and the user. The Worklist tab in the agent
agent pane renders it as a checklist under "Worklist".

### When to route through the worklist

**Default: every change request goes through `resources/worklist.json`.**
Single-file, single-line, single-attribute — size doesn't matter.
Propose first, wait for the user's `approved:` payload, then apply.
The two-stage proposed → applied flow lets the user redirect or veto
before any code is touched, and the worklist history serves as the
audit trail for what landed and why.

Skip the worklist only in these specific contexts, never because the
change is "small":

- **Explicit user opt-out in this turn.** The user ends with
"just do it" or "skip the worklist". The opt-out must be in the same turn
 as the change request — don't carry it forward across turns or infer it from past patterns.
 Both Claude and Codex honor the same phrase list, but along different paths:
 Claude's guard matches `_OPT_OUT_PATTERNS` against `transcript_path` on every
 `PreToolUse` and allows inline; for Codex, Bram's host-side `toTurn` path
 matches the same list and writes a one-turn `direct-edit` record
 (`kind:"direct-edit"`, `paths:["*"]`, 1h TTL) to
 `resources/.worklist-authorization.json`, which the single Codex
 `PreToolUse` hook reads via `fresh_bypass()`. The phrases themselves are
 identical, so the user-facing contract is the same regardless of agent.

- **`skip-worklist:` structured prefix on this turn.** The user's
  turn begins literally with `skip-worklist: ` followed by the
  request text. Same family as `approved:` / `drop:` / `iterate:`,
  but for authorizing a direct edit rather than a lifecycle
  transition. The user-facing affordance is the **Skip worklist**
  button next to the Worklist tab's message-agent input — it prepends
  the prefix and submits. Same convention as for Approve / Drop /
  Iterate: tell users to click the button, do not instruct them to
  type or paste the wire format. When the host's `toTurn` write path
  sees the prefix it writes the same one-turn `direct-edit` record to
  `resources/.worklist-authorization.json` that the prose opt-out
  writes, then forwards the **entire turn text including the prefix**
  to the agent (unlike `approved:` / `drop:` / `iterate:`, which the
  agent is told not to mention but the prefix is left in place so the
  agent can see it). Agents seeing a `skip-worklist:` prefix on their
  turn must skip the propose-first convention and act on the rest of
  the message as a direct edit; do not write a new worklist item.
  The PreToolUse hook will allow the edits via the existing
  `fresh_bypass()` path.

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

- **Issue-only `gh` work with no repo diff.** If the user asks you to
  create, edit, comment on, close, or reopen a GitHub issue, and the
  task will not modify tracked files in the repo and will not produce a
  commit, skip the worklist and do it directly. If the issue request is
  paired with repo changes, the repo changes still go through the
  worklist.

### What worklist items represent (and when to drop)

**Worklist items represent repository changes.** A `proposed` item
names a `file` (or `files`) plus `before` / `after` prose in
`resources/worklist-drafts/<id>.md`, describing what would change
on disk. An `applied` item has those changes on disk
waiting for the user to approve a commit. Items exist to give the
user explicit veto power over what lands in their repo.

Investigation work does NOT belong in the worklist. Things like:

- Checking whether a port is open or a server is running.
- Restarting a process or a Docker container.

…all happen in chat, not as worklist items. They produce no
`before` / `after` because there's nothing to write. They produce
no commit because there's nothing to land.

**If an investigation reveals nothing to commit, guide the user to
Drop.** Sometimes the agent proposes an item expecting code work
and the investigation turns up no actionable change — the bug was
a runtime configuration issue, the fix was a process restart,
every check passed. In that case:

- Do NOT call `/__worklist/mutate op:"advance"`. Marking the item
  as `applied` produces a TO COMMIT row with nothing to commit,
  which is exactly the user-visible failure mode of #88.
- Instead, summarize the finding in chat ("checked X, Y, Z; the
  issue is runtime-only, no code change needed") and explicitly
  recommend the user click **Drop** on that item in the Worklist
  tab.
- The user's Drop click works the same as any other drop —
  `/__worklist/resolve` with `kind: "drop"`, then
  `/__worklist/mutate op:"prune"`. Standard flow.

**Recovery if you've already advanced.** If you call `advance`
before realizing the apply was a no-op, the recovery is identical:
explain the finding in chat, recommend Drop on the resulting TO
COMMIT row. The user's Drop click works equally well on `proposed`
and `applied` items. No special undo path needed.

### Schema and draft layout

Proposals split metadata from review prose across two files:

```text
resources/worklist.json              # compact metadata index
resources/worklist-drafts/<id>.md    # before / after prose per item
```

The draft file:

```markdown
# Before

what's there now, relevant context, rejected alternatives

# After

what you'll change it to
```

The metadata item:

```json
{
  "id": "kebab-case-id",
  "status": "proposed",
  "files": ["path/to/file.xmlui"],
  "closesIssues": [{ "number": 42, "title": "..." }]
}
```

Bram merges draft prose into `/__worklist` and `/__worklist/resolve`,
so the Worklist tab and approval flow see one combined item. Hashes
cover metadata + resolved prose together. If a draft file is missing,
`/__worklist` returns empty `before` / `after` plus
`"_draftMissing": true` and the UI shows a placeholder.

`worklist.json` also carries a top-level `version` integer that guards
against concurrent-writer races between agents and the
`/__worklist/mutate` route. Every write to `worklist.json` MUST set
`version: N+1` where `N` is the value present on disk at the moment
you read it. The PreToolUse hooks (Claude and Codex both) compute the
current on-disk version and deny the write if the new content does
not bump it by exactly one. `/__worklist/mutate` does the same bump
on its own RMW path under a serializing mutex. The flow for an agent
proposing or refining items is:

1. Read `worklist.json` and capture its `version`.
2. Construct the new content with `version: <captured + 1>`.
3. Write. If the hook denies with `reason=stale-worklist-version`,
   re-read the file (another writer landed first), rebase your
   change on the new contents, and retry.

Files without a `version` field (legacy) are treated as version 0;
the first write that introduces the field at version 1 is the
natural migration path and the hooks allow it.

Prose lives only in the draft file. Inline `before` / `after` keys
in `worklist.json` are rejected by both guards — the proposal
authoring channel writes metadata to `worklist.json` and prose to
the matching `worklist-drafts/<id>.md`, never both. Iterate-time
prose edits go to the draft file; `worklist.json` only changes
when metadata (`files`, `closesIssues`, etc.) shifts.

**Field notes:**

- `files: ["path/a", "path/b"]` for multi-file items; `file` (singular)
  is the older single-file form. The TO COMMIT inline diff
  concatenates all listed files.
- `closesIssues` declares which GitHub issues the commit resolves
  (drives the close-on-commit dialog — see *Commit & git etiquette*).
  Set conservatively: only when the commit truly closes the issue, not
  when it merely cross-references (`see #N`, `related to #N`, partial
  multi-step work). Omit or use `[]` to skip the dialog.
- `status` controls the Worklist tab badge:
  - `"proposed"` (default if omitted) → **TO APPLY**. User is approving
    you to make the change.
  - `"applied"` → **TO COMMIT**. Change is on disk, user is approving
    `git commit`. Push decided separately via the Push button.

Default to the two-stage flow: approved `proposed` → advance to
`applied` → user approves a separate commit → prune. Skip the
`applied` stage only when the user says "apply and commit" up front.
Drops prune directly with no `applied` stage. Don't pre-mark new
items `"applied"` unless the change is genuinely already on disk.

`resources/worklist.json` doesn't need to exist in advance — Bram
serves an empty default; the Worklist tab creates the file (and
`resources/`) on first use.

### Lifecycle: propose → triage → mechanical transitions

1. **Propose** — write draft prose to
   `resources/worklist-drafts/<id>.md`, then write a metadata item to
   `resources/worklist.json`. Each item should be small, discrete, and
   independently rejectable. Writing the item is *asking* the user to
   approve, not approval itself. Don't show or instruct on raw
   `approved:` / `drop:` / `iterate:` payloads — the Worklist tab's
   buttons generate the `{id, feedback}` shape.

2. **User triages** — unchecks anything they don't want, then clicks
   one of the buttons. All three action buttons emit the same payload
   shape: `{"items":[{"id":"...","feedback":"..."}, ...]}`
   — ids plus optional per-item feedback. Never parse these turn lines
   for content yourself; `/__worklist/resolve` returns the recorded
   item bodies.

   - *Talk to agent* (with a comment typed above) → `talk: <text>`.
     No items approved or dropped. Respond; do not edit files.

   - *Approve selected (N)* → `approved: {...}`. Call
     `/__worklist/resolve` via the transport for your agent (see
     *Transports*). Response is one of:
     - `{"kind":"approved", "items":[<recorded content>], ...}` —
       execute these items. Do NOT re-read `resources/worklist.json`
       to second-guess what was approved. Records are **consumed on
       first read** — a second call returns `no_active_authorization`,
       so capture what you need. After editing the project files,
       advance via `POST /__worklist/mutate`, not by rewriting
       `"status": "applied"` directly.
     - `{"kind":"no_active_authorization", ...}` — the record is
       already consumed, or this turn isn't an authorization turn.
       **Do NOT treat as authorization.** Backstop for the rule that
       `iterate:` and other non-authorization turns must not route
       through `/__worklist/resolve`.

     Respond to any per-item feedback regardless of kind.

   - *Drop selected (N)* → `drop: {...}`. Same flow:
     `{"kind":"drop"}` → prune the ids via `POST /__worklist/mutate`.
     Respond to per-item feedback (often the user's reason for the drop).

   - *Iterate (N)* — enabled only when feedback is non-empty (no-
     direction Iterate is meaningless). Payload: `iterate: {...}`.
     **Iterate does NOT route through `/__worklist/resolve`** — no
     state change is being authorized. Re-read items from
     `/__worklist` (for resolved draft prose) or
     `resources/worklist.json` (metadata alone), and act per each
     item's current status:
     - **`proposed` (TO APPLY):** revise the draft file's `before` /
       `after` prose; update `files` only if scope shifts. Item
       stays `proposed`, no project file edits.
     - **`applied` (TO COMMIT):** edit on-disk files per the feedback.
       Update the draft only if scope materially expanded. Item
       stays `applied`.

     No agent-side bracket needed. The host detects the `iterate:`
     prefix on the `toTurn` write path and sets the inflight sentinel
     automatically; the same turn-finished detectors that clear
     approve/drop sentinels clear iterate's too. (The legacy
     `/__iterate/begin` and `/__iterate/end` routes were removed in
     the #214 delete phase.) See *Host-managed inflight sentinel*.

     The Iterate payload's per-item shape is `{id, feedbackRef}`
     where `feedbackRef` names a file at
     `resources/feedback-drafts/<feedbackRef>.md` containing the user's
     full-fidelity feedback text. Read that file directly to get the
     feedback content — `toTurn`'s `\s+ → " "` collapse and the
     receiving TUI's bracketed-paste limits don't apply because the
     text never rode the PTY paste channel. Feedback refs are allocated
     per click, typically `<unix-ms>-<item-id>`; they are not item ids.
     The feedback text is the new user-authored submission for this turn.
     Successful `/__worklist/mutate` advance/prune promotes matching
     drafts from `feedback-drafts/` to `feedback-history/` so drafts do
     not accumulate. Each draft write emits a `[feedback-draft] op=write`
     trace line with `feedback_id` and byte count. Approve and Drop
     still use the inline `{id, feedback}` shape (their feedback is
     usually short); their migration to `feedbackRef` is filed as
     follow-up. See #144.

3. **Mechanical transitions** — `POST /__worklist/mutate` is the only
   channel for approval-driven state changes:
   - `{"op":"advance","ids":[...],"status":"applied"}` after an
     approved apply.
   - `{"op":"prune","ids":[...]}` after a drop, or after a commit of
     already-`applied` items.

4. **Empty state is fine** — `{ "description": "", "items": [] }`.

### Transports

Both transports dispatch through the same host-side handlers, so
response kinds, consume-on-read, the inflight sentinel, and the auth
checks are identical. What differs is *how* the call is made.

**Apply gate: skip `resolve` — edit, then `mutate op:"advance"`.** The
host sets the inflight sentinel at approval time (on the `toTurn` write
path, the way `iterate:` does), and `mutate op:"advance"` consumes the
`approved` auth, so `resolve`'s two side effects are covered without a
round-trip. Its return value is dead weight for an apply — the bodies are
the proposal you authored. So an apply-approve is one call: edit from the
proposal, then `mutate op:"advance"`.

**Commit gate: call `worklist-commit`.** For approved TO COMMIT items,
send one request with `{ ids, message }`. The host verifies approved auth,
requires every id to be `applied`, stages only those items' files, refuses
unrelated staged files, commits, prunes the items, consumes auth, and clears
the sentinel. Close-on-commit (`close-issue:` / `push-before-close:`) stays
on the existing `/__issue/close` path after the commit returns its `sha`.

**Drops: still `resolve` before `mutate`.** Resolve returns the recorded
items and writes the drop sentinel (drops aren't set at approval time), then
`mutate op:"prune"` clears it.

#### Claude: loopback curl

Bram writes its bound port at startup to `resources/.bram-port` (plain
decimal, no newline). Read that file once and substitute the literal
number into curl:

```
curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1 \
  "http://127.0.0.1:61455/__worklist/resolve"
```

(replace `61455` with whatever `Read resources/.bram-port` returned).
The literal port matches the `.claude/settings.json` allowlist and
runs without a prompt. `$BRAM_PORT` won't work — Claude Code's
permission matcher doesn't expand variables, so `$` breaks the match
(see https://code.claude.com/docs/en/permissions.md).

The POST routes (`worklist-mutate`, `worklist-commit`,
`issue-close`) have their own allowlist entries, but the match is
narrow — keep the call in this exact shape or it will prompt:

```
curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1 -X POST \
  -H "Content-Type: application/json" --data @/tmp/body.json \
  "http://127.0.0.1:61455/__worklist/commit"
```

Two pitfalls, both of which prompted a real `worklist-commit` call:

- **Include literal `-X POST`.** The POST allowlist entries require
  it; relying on `--data` to imply POST matches neither the POST
  entries (which need `-X POST`) nor the GET entry (whose URL must
  follow `--retry-delay 1` with no flags between).
- **Keep the curl a standalone command.** Build the JSON body in a
  *separate* Bash call (`jq … > /tmp/body.json`), then `--data
  @/tmp/body.json`. A compound `cat <<EOF … && jq … && curl …` makes
  the command string start with `cat`, so no `curl …` prefix can
  match and the whole thing prompts. The body-building step is also
  where apostrophes/quotes in a commit message belong — out of the
  allowlisted curl line.

Flag rationale:
- `-4` + `127.0.0.1` (not `localhost`): Bram binds IPv4 only;
  `localhost` may try `::1` first and fail with `curl: (7)`.
- `-sS` (not `-s`): `-s` swallows `Failed to connect`, so a stale-port
  race surfaces as `(no output)` instead of `curl: (7)`.

If the port keeps refusing after fresh re-reads, treat it as a
stale-port / restarting-server diagnostic — don't continue without
the lifecycle call. Check the Status tab's **Port file** row, which
cross-checks the running process, `.bram-port`, and the
`.bram-port.json` sidecar (port + pid + project root + startup
timestamp). If `.bram-port` is missing entirely (agent launched
outside Bram's PTY shell), fall back to
`lsof -nP -iTCP -sTCP:LISTEN | grep bram`.

#### Codex: filesystem intent/result files

Codex's `workspace-write` sandbox refuses loopback connections (issue
#130); the only knob that would fix it (`network_access = true`)
grants all outbound network. So Codex drives the lifecycle through
two coordination dot-files instead:

1. **Write** `resources/.worklist-intent.json`:

   ```json
   { "nonce": "<unique-per-request>", "route": "<route>", "body": { ... } }
   ```

   `route` is one of `worklist-resolve`, `worklist-mutate`, `worklist-commit`, or
   `issue-close`. `body` matches the HTTP route:
   - `worklist-resolve` — omit, or `{ "ids": [...] }` to filter.
   - `worklist-mutate` — `{ "op": "advance", "ids": [...], "status": "applied" }`
     or `{ "op": "prune", "ids": [...] }`.
   - `worklist-commit` — `{ "ids": [...], "message": "..." }`.
   - `issue-close` — `{ "number": N, "commit": "<full-sha>", "push": <bool> }`
     for the generated-comment verified path, or
     `{ "number": N, "comment": "<user-supplied>" }` for the
     user-supplied-comment path. Same field semantics as
     `/__issue/close` — see the close-on-commit section below.

2. **Read** `resources/.worklist-result.json` for the record whose
   `nonce` matches (ignore stale results from prior requests):

   ```json
   { "nonce": "<echoed>", "ok": true,  "status": 200, "result": { ... }, "completedAtMs": 0 }
   { "nonce": "<echoed>", "ok": false, "status": 400, "error":  { ... }, "completedAtMs": 0 }
   ```

   `result` is byte-for-byte what the HTTP route would have returned.
   The host writes within watcher latency (a few ms) and then deletes
   the intent file; a brief read-retry covers the race. **Do not
   continue silently** on a missing result or `ok: false`.

The Codex PreToolUse guard exempts `.worklist-intent.json` from
worklist coverage — it's a coordination file, like the loopback curl
is for Claude. Trace each drain by grepping `[worklist-intent]` in
`resources/bram-traces/bram-trace.log`.

### Authoring conventions

#### Choosing an id

For items clearly derived from a single GitHub issue, prefix the id
with `issue-<N>-` followed by a short slug
(`issue-86-pty-intent-relay`, `issue-91-defer-sentinel-clear`). Skip
the prefix for exploratory items, cross-cutting refactors, or items
that touch multiple issues — use a bare descriptive slug
(`worklist-drafts-separate-prose-from-metadata`).

The prefix complements `closesIssues` rather than replacing it: the
id is for human scanning (Worklist tab, `git log`, chat),
`closesIssues` drives the close-on-commit dialog. Pair them when
both apply. Existing items keep their names — renaming breaks
back-references for marginal benefit.

#### Refer to items by id, not by ordinal

Name worklist items in chat by their `id` verbatim
(`codex-launcher-require-hook`), never by position ("item 3", "the
second one"). Ordinals shift as items move through approve / apply
/ drop / prune; ids are stable and match the Worklist tab UI and
the `approved:` / `drop:` payloads.

#### Match prose verbosity to change complexity

Match `before` / `after` prose to the size and judgment-load of the
change.

**Small, mechanical changes** (typo, one-line tweak, rename, clear
bug with one obvious fix): a short paragraph each is enough. Don't
pad with alternatives-considered when there was effectively one
path — the commit message + diff carry the audit trail.

**Complex or judgment-load changes** (multiple reasonable
approaches, multi-file non-mechanical, *why* will fade in a month):
name the alternatives, mark `[chosen]` on the picked path:

> Alternatives considered:
>
> - Embedded diff via DataSource — rejected: each row would fire its own request.
> - Full-tree diff at the top of the worklist — rejected: hides per-item attribution.
> - **[chosen]** Server augmentation via `/__worklist` — single payload, per-item diffs travel with each row.

Rule of thumb: would a reader six months from now reconstruct the
decision from current code + git log alone? Yes → short. No →
fulsome.

#### Use Markdown in item prose

Worklist `before` / `after` prose and worklist-history entries
render as Markdown in the agent pane. Use real syntax: `- `
per bullet (not inline `(a) ... (b) ...` enumerations that collapse
to one paragraph), backticks for inline code, fenced blocks for
multi-line snippets, blank lines between paragraphs, `**strong**`
sparingly (e.g. **[chosen]**).

#### Minimize the bytes of each worklist edit

`worklist.json` stays a compact metadata index; iterate-time prose
edits hit only the draft file. Full-item `Write` rewrites of
`worklist.json` are valid but wasteful for one-paragraph tweaks
that don't actually need to touch `worklist.json` — prose changes
go to the draft alone. Mechanical prune / advance go through
`/__worklist/mutate`, not direct rewrite.

#### Don't `grep -n` a single-line JSON file

`worklist.json` is one line; grep dumps the whole file into the
transcript. Use `Read` with `offset`/`limit` or `jq` to extract
just what you need.

#### Don't update `after` prose on every iterate

Small TO-COMMIT refinements don't need an audit trail in the
worklist — the commit message and diff cover it. Update the draft
file's `after` only when scope materially expands (new file added
to `files`, or the change's intent shifts).

#### Test Worklist UX through the worklist itself

When a change touches the Worklist UX (button states, gray-out,
feedback flow, pruning), surface it as a pending item even when the
diff is already on disk. Approving the item exercises the new
behavior end-to-end — file rewrites, pruning, Talk-page update — as
the actual test.

### Enforcement and security contract

The structured `approved:` / `drop:` line is not authority by itself.
The host records each clicked id into
`resources/.worklist-authorization.json` with its kind (`approved` /
`drop`); `/__worklist/resolve` is the only way an agent receives the
recorded item bodies; `/__worklist/mutate` is the only way an agent
advances or prunes:

- `advance` requires an `approved` auth record covering every id.
- `prune` requires `drop`, except the post-commit prune path also
  accepts `approved` when the requested ids are already `applied`.

Same-turn `resolve → edit files → mutate` is valid: `mutate` reads
the stored auth record, not just resolve's consumption state.

There is **no content-hash verification**. An earlier design recomputed
each item's content hash at record time and flipped mismatches to a
`rejected_stale` kind — an optimistic-concurrency guard against the
worklist changing between click and record. Bram only ever shares a
worklist between agents **serially, never concurrently**, so that guard
never fired and was removed. The remaining concurrency guard is the
`version` integer on `worklist.json` (file-write races, hook-enforced);
self-authorization is gated structurally — `resolve` / `mutate` are the
only channels and the auth record is consumed on read — not by a hash.

Defense in depth: Claude and Codex each install PreToolUse hooks
that validate worklist coverage before file-mutating tools run, and
the desktop watcher reverts unauthorized prunes. Both guards also
reject `worklist.json` writes that put non-empty `before` /
`after` on any proposed item — prose must live in
`resources/worklist-drafts/<id>.md`. Hook errors and revert
messages are the convention enforcing itself — not bugs to work
around.

**Don't ask before editing the worklist or calling mutate.** The
proposal-authoring write channel is hook-guarded, the mechanical
transition channel is the server endpoint. No verbal confirmation
is needed to add items, refine prose, or call `mutate` for an
already-approved transition. Save the verbal back-and-forth for
design decisions (which items to propose, what to bake in), not for
mechanics.


## Talking to users

### Name UI affordances, not protocols

When the user needs to take an action that has a UI control, name the
control. Say "Click the **Approve** button" (Drop, Iterate, Push, Trust
this hook, Setup). Never say "send `approved: {...}`", "paste the
structured approval payload", or describe the wire format — the button
generates the verified payload for them. This is what reopened #62:
Codex told the user to paste raw JSON instead of pointing at the
Worklist tab.

### Keep internal jargon out of user-facing chat

"Inflight sentinel", "resolve/mutate", "PreToolUse hook", "worklist
authorization record" describe internals. In chat, talk about what
the user sees and does: "the Worklist tab", "approve the item", "the
spinner cleared". Use the jargon only when the user has asked about
internals, or when you're pointing at a file path they'll need to grep.

### Cite, don't gesture

When referencing a file, route, or doc, name it (`resources/worklist.json`,
`/__worklist/resolve`, `docs/apis.md §11`) so the user can verify in
one click. Vague references ("the worklist system", "the relevant
config") force a follow-up question. Same rule as the CLAUDE.md
guidance: if you can't cite, say so.

### Match terseness to the question

No preamble ("Great question!", "Let me explain..."), no restating the
user's question, no trailing summary of what you just did unless it's
load-bearing. The Worklist tab shows the items, the diff shows the
code; chat is for what those surfaces can't show.


## Host-managed inflight sentinel

The Worklist spinner is keyed to `resources/.inflight-claim.json`,
which host-side HTTP handlers write and clear. Full route / file-shape
reference: `docs/apis.md` §11. Agent-side conventions:

### What the agent calls

- **`approved:` (apply gate)** → no `resolve`. The host detects the
  `approved:` prefix on the `toTurn` write path and sets the sentinel
  automatically (the way it does for `iterate:`). Edit from the proposal
  you authored, then `mutate op:"advance"`, which consumes the `approved`
  auth and clears the sentinel. One call.
- **`approved:` (commit gate)** → `worklist-commit` with `{ ids, message }`.
  The host stages only the approved files, commits, prunes, consumes auth,
  and clears the sentinel. If the approved feedback includes `close-issue:` /
  `push-before-close:` lines, call the existing `issue-close` route after
  the commit route returns `sha`.
- **`drop:`** → `resolve` → `mutate op:"prune"`. Drops aren't set at
  approval time, so `resolve` is what raises the spinner.
- **`iterate:`** → no agent-side bracket needed. The host detects the
  `iterate:` prefix on the `toTurn` write path and sets the sentinel
  automatically (parallel to how `resolve` sets it for the commit gate and
  drops); the same turn-finished detectors that clear approve/drop
  sentinels clear iterate's too. (The legacy `/__iterate/begin` and
  `/__iterate/end` routes were removed in the #214 delete phase.)

### Failure modes

A stuck spinner is the convention enforcing itself; there is no
arbitrary live-session timeout. Bram does have host-side completion
detectors that can clear a lingering claim without a cooperative agent
tail call: Claude session JSONL `stop_reason:"end_turn"`, Codex session
JSONL `task_complete`, PTY silence, and explicit cancellation paths. Most
commonly:

- **Approved/drop stuck:** `mutate` was never called, or errored
  before the clear. Recovery: call mutate manually, or restart Bram
  (`cleanup_stale_inflight_claim` runs at startup).
- **Iterate stuck:** rare now that the host auto-detects the
  `iterate:` prefix and the turn-finished clearer fires for all
  sentinel kinds. If it does stick, host-side completion detectors
  will clear it on the next normal turn end; `/__worklist/end` remains
  available as an explicit manual unwind.
- **Premature clear:** silence alone is not authoritative. PTY silence
  can request a sentinel clear, but the host first checks the latest
  provider JSONL completion detector. If JSONL says the assistant turn is
  still non-final, the host logs
  `[agent-status] op=skip-sentinel-clear ... reason=jsonl-non-final` and
  leaves the sentinel intact. If a premature clear is suspected, inspect
  `[agent-status] op=skip-sentinel-clear`, `[jsonl-turn-end]`, and
  `[inflight-sentinel]` in `bram-trace.log`. Missing/unreadable JSONL
  falls back to the legacy silence-clear behavior.

The Status tab's Inflight Sentinel section includes a `Turn completion`
row. Use it first when diagnosing a stuck spinner: it reports the last
detector source, provider, skip/detect reason, timestamp, and whether
the observed completion happened after the active claim.

Do not conflate this with XMLUI component-local busy states. APICall
spinners/buttons are driven by the APICall component's `inProgress`
state and lifecycle handlers; Worklist spinners are driven by Bram's
host-managed inflight sentinel. XMLUI fixes such as
xmlui-org/xmlui#3540 can resolve delayed APICall `onSuccess` cleanup,
but they do not replace the host turn-completion detector needed for
approved/drop/iterate worklist cycles, which are sent through `toTurn`
and cleared through `/__inflight` plus host lifecycle events.


## Commit & git etiquette

### Don't nudge toward commit approval

A TO COMMIT item sits indefinitely until an `approved:` payload
covers it. Describe the state factually ("relay is TO COMMIT —
confidence high on happy path, untested edges noted above") and
stop. The user clicks Approve when ready, or doesn't. The exception
is a *minor* change the user explicitly asks you to commit directly.

### Don't infer commit / drop / advance from feedback

"Looks good", "seems pretty good", "it works" — these are not
authorization to commit applied items, drop proposed items, or
otherwise advance worklist state. Wait for explicit "commit it" or a
structured `approved:` payload.

`voice: ...` is a transport marker (the user dictated instead of
typed), not a refusal trigger. Voice *state-advancement* phrases
("voice: looks good") behave like typed talk — informational only.
Voice *task requests* ("voice: create foo.txt", "voice: fix the bug
in X") are acted on the same as if typed. If a verbal phrase is
ambiguous, ask one focused question instead of acting.

### Hold the commit while a related TO APPLY is in flight

When a TO COMMIT item and a TO APPLY item touch the same surface
(feature + tuning adjustment, fix + follow-up regression patch),
don't process the commit if the user's `approved:` covers both.
Apply the proposed item only; leave the prior in TO COMMIT. The
user verifies the combined behavior, then approves a single commit
covering both. This avoids intermediate "kinda-works" commits where
a feature is split from its companion fix — bad for git history and
bisect.

### Warn when a new item would entangle a TO COMMIT

Whenever you're about to **propose** or **apply** an item whose
`files` overlaps the `files` of an existing TO COMMIT item, surface
that fact in chat *before* writing the proposal or applying the
edits:

> "issue-X is TO COMMIT and touches the same file(s) — recommend
> committing it first; otherwise this item's edits will mix into
> X's on-disk diff and need manual separation later."

Don't auto-block — the user may have a reason to proceed (the two
items are genuinely meant to ship together, X is about to be
dropped, etc.). The warning is so the user can decide *order*
intentionally rather than discovering the entanglement at commit
time. The check is mechanical: intersect the candidate item's
`files` list with the union of `files` across `applied`-status items
in `resources/worklist.json`; non-empty intersection triggers the
warning.

### Suggest a branch when isolation helps

Bram should guide users toward good git practice, not force ceremony.
Before broad, risky, exploratory, multi-commit, review-before-main, or
issue-close-sensitive work — especially when the current
branch/worktree already contains unrelated changes — suggest creating
or switching to a branch and explain the benefit briefly. Do not
branch for small direct fixes or straightforward docs tweaks, and do
not change branches without clear user consent.

### Notice when sibling commits should be squashed

If two consecutive unpushed commits are really one feature (mechanism
+ config, backend route + frontend caller, struct + only constructor),
flag it before push: "`<sha1>` and `<sha2>` are two halves of the same
feature — want to squash them?" If yes, and **both commits are
unpushed**:

```
git reset --soft HEAD~2     # keeps both diffs staged
git commit -F <new-msg>     # one combined commit
```

Verify with `git log --oneline -3` and `git log --oneline @{u}..HEAD`.
Never squash already-pushed commits without explicit force-push consent.

### Don't quote unpushed-commit counts in chat

After a commit lands, confirm with its short SHA and subject and stop.
Don't say "N unpushed commits now" or list unpushed SHAs in prose — the
Commits tab has the exact count and list; any number you'd state is
guesswork.

### Push button auto-rebases on non-fast-forward

The Commits-tab Push button does `git push`; if rejected as
non-fast-forward, it fetches `origin` and rebases on `origin/<branch>`
before retrying (linear history, no merge commits). Don't manually
`git pull --rebase` — that's the button's job. Only intervene when
the button reports rebase conflicts (working tree left clean); then
start a manual rebase, resolve, and push.

### Commit messages

Summarize the worklist item that drove the commit. Use
multiline. Reference the driving issue if there is one.

### Close-on-commit confirm dialog

When an item's `applied` commit would resolve a GitHub issue, set
`closesIssues: [{number: N, title: "..."}, ...]` on the item (title
from `gh issue view N --json title`; refresh if you iterate).
Approving a TO COMMIT item with non-empty `closesIssues` opens a
confirm dialog — one row per issue plus an optional close-comment
textbox, with three actions: close after verifying the commit is
visible on GitHub; push then verify and close; or commit only. The
push-before-close path is branch-scoped, not item-scoped: it pushes
the new worklist commit plus any unpublished commits already reachable
from the current branch tip. The dialog must show that scope before
confirmation, including a table of pending commits when any already
exist.

Issue-derived items (e.g. "Propose a worklist item to address #N
...") default to pairing the `issue-<N>-...` id with `closesIssues`
for that same issue. Omit only when the change is explicitly
investigative, partial, or not intended to resolve. If you discover
an approved/applied item is missing `closesIssues`, iterate the
metadata before asking for commit approval.

Don't regex `#N` from item prose — false positives on
cross-references. Use conversational context to judge whether the
commit truly resolves an issue; set `closesIssues` explicitly when
it does.

The user's choices arrive in the per-item `feedback` of the
`approved:` payload as lines appended after any free-text feedback:

```
close-issue: 52
close-issue: 50 comment: "shipped, see commit message"
push-before-close: true
```

After resolving and committing as usual:

1. Parse the verified `feedback`: lines starting with `close-issue: N`
   each name an issue to close; an exact `push-before-close: true`
   line toggles push-before-close.
2. Resolve the new commit's full SHA.
3. For each `close-issue: N` **without** a user-supplied comment,
   call Bram's backend route through your transport (don't `gh issue
   close` directly):

   - **Claude (loopback curl):**

     ```sh
     curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1 \
       "http://127.0.0.1:<bram-port>/__issue/close?number=N&commit=<full-sha>[&push=true]"
     ```

     Append `&push=true` if `push-before-close: true` was present.

   - **Codex (filesystem intent):** write `resources/.worklist-intent.json`
     with `{ "nonce": "...", "route": "issue-close", "body": { "number": N,
     "commit": "<full-sha>", "push": <bool> } }` and read
     `resources/.worklist-result.json` for the matching nonce. Same
     drain-and-retry rules as the worklist routes above.

   Either way, the backend pushes (if requested), verifies GitHub
   sees the commit, and on success closes with the generated comment
   `Closed by https://github.com/<owner>/<repo>/commit/<full-sha>`.

4. For `close-issue: N comment: "..."`, close with the same transport
   shapes — Claude:
   `/__issue/close?number=N&comment=<encoded-comment>`; Codex:
   `route: "issue-close", body: { "number": N, "comment": "..." }`.
   Don't rewrite the user's comment into the generated form.

5. On backend refusal (`{"ok":false,"code":"commit-not-visible"}` or
   `"push-failed"`), do **not** fall back to `gh issue close`. Report
   the message plainly, e.g.: "Committed `<short-sha>`, but did not
   close #N because GitHub cannot see the commit yet." The worklist
   item may still be pruned if the commit succeeded — issue closing
   is a post-commit side effect.

6. **Approve without closing** arrives as feedback with no
   `close-issue:` lines — commit only.


## Bram shell mechanics

### Target app helpers (opt-in)

Bram's own Worklist and Sessions tabs already use these helpers
internally — the worklist Approve/Drop flow works with no extra
setup. You only need these if **your own** project markup wants to
talk back to the agent (custom Approve buttons, in-page forms that
submit a fresh user turn).

Include `<script src="/__shell/helpers.js"></script>` in your
project's `index.html` to expose:

| helper | usage |
|---|---|
| `toShell(text)` | inject text into stdin; user must press Enter |
| `toTurn(text)` | submit text as a complete user turn (auto-Enter) |
| `openExternal(url)` | open URL in the system browser |
| `logToHost(payload)` | log to Bram stderr without bothering you |

Use `toTurn` for one-shot form submissions (Approve, Confirm). Use
`toShell` to inject text the user can edit before sending.

> **Since C1 (target-pane origin isolation).** The target pane is served at a
> distinct `bramapp://localhost` origin, so `getTauriInvoke()` returns `null`
> there and `toShell` / `toTurn` / `sendKeys` / `openExternal` **no-op** inside
> an embedded target app — the pane is display-only. `helpers.js` is still
> served (so XMLUI apps boot) but its host-driving functions are inert; only
> Bram's own agent pane (Worklist/Sessions), which stays same-origin, drives
> them. If an embedded app needs to talk back to the agent, render the control
> in the agent pane instead. The target scheme (`handle_target_scheme` in
> `lib.rs`) refuses the dynamic host routes (`__file`, `__worklist/*`,
> `__settings`, …) and serves only project content plus the static
> `__vendor/*` / `__shell/*` namespaces.

### UI patterns

#### Fold optional companion input into existing actions

When a surface already has clear primary actions (Approve / Drop /
Submit) and a new optional input is added (free-text feedback, notes,
override flag), fold the input value into the existing actions'
onClick payloads rather than adding a separate Submit / Send button.
Render the input above or beside the primary buttons; clear it after
submission. A separate submit button creates a third decision point
("which button do I click for what?") and forces the user to send
two messages when one would do. Only add a separate submit button if
the auxiliary input is genuinely independent of the primary actions.

### Build vs. hot-reload boundary

Use a hard boundary for Bram development:

| path | rule |
|---|---|
| `app/tools/**` | Hot-reloadable tools XMLUI app code: `Main.xmlui`, `components/**`, `Globals.xs`, `config.json`, `themes/**`, `resources/**`. |
| user's project directory | Target-pane reload / project dev-server reload, depending on the project setup. |
| `app/__shell/**` | Rebuild from `src-tauri/`, then relaunch `./bram`. This includes `helpers.js`. |
| `app/main.js`, `app/index.html`, `app/styles.css` | Rebuild from `src-tauri/`, then relaunch `./bram`. Parent-shell code is not hot-reloaded. |
| `app/vendor/**` | Rebuild from `src-tauri/`, then relaunch `./bram`. |
| `src-tauri/**` | Rebuild from `src-tauri/`, then relaunch `./bram`. |

Do not describe `app/__shell/helpers.js`, parent-shell assets, vendor
assets, or Rust as hot-reloadable. Even if the watcher reloads an iframe,
those paths are shell/runtime code and their behavior can depend on
pre-XMLUI globals, parent-window state, custom scheme handling, Tauri
commands, or long-lived listeners. Validate those edits only after a
fresh build and relaunch of the locally built binary.

Launch discipline for Bram development:

1. For `app/tools/**`, save the file and let the tools iframe reload.
2. For every other Bram runtime path, run `cargo build` from
   `src-tauri/`, then relaunch the locally built `./bram` symlink
   (`src-tauri/target/debug/bram`), not an installed/older app.

The Bram binary embeds the `app/` tree at build time
(`include_dir!("$CARGO_MANIFEST_DIR/../app")`, plus Tauri
`frontendDist: "../app"`). That embedding is the reason the rebuild rule
exists for shell/runtime assets: a plain restart of the wrong binary, or
a build followed by relaunching that wrong binary, still runs stale code.

Don't suggest `cargo run`; the user prefers rebuild + restart, and the
incremental build is fast.

### Updating GitHub issues via gh

Use `gh` directly — the Issues tab polls every 30s, so updates surface
without a restart:

- `gh issue edit <n> --title "…" --body "…"`
- `gh issue comment <n> --body "…"`
- `gh issue close <n>` / `gh issue reopen <n>`


## Log-first development

Agents default to writing and reading code; in Bram the higher-value
habit is writing and reading logs. Behavior here arises from the
interplay of Rust, the parent shell, XMLUI, two agent CLIs, and
Markdown/Python-governed workflow — runtime questions ("was the right
message sent at the right time? did the transition fire? did it
render?") are answered by evidence, not inspection. The norms:

- **The drill.** When behavior goes wrong — or a new mechanism is
  being designed — the first question is: does the trace already
  capture what happened? If no, add the instrumentation (as its own
  worklist item when scope warrants) and keep dogfooding until the
  problem recurs; the next occurrence should be self-diagnosing. If
  yes, use it before theorizing. A fix proposed without trace
  evidence should say so explicitly.
- **Observe-only first for behavior changes.** Mechanisms that will
  act on inferred conditions (auto-clears, auto-reveals, suppressors)
  ship first as trace lines only, with graduation criteria written
  into the worklist draft as falsifiable checks against the soak
  ("every would-X corresponds to a corroborated moment; zero fire
  during Y"). Precedents: the send-ledger's observe-only phase, the
  reveal-floor observer. The design review is a grep.
- **Baselines are commits.** Perf work starts with an instrumentation
  commit that records the before (see `a99c7d9`, "sets up the
  before/after": ~1.7 footer re-renders/sec while typing, 49 ms avg
  drift), and the same trace line verifies the after. Numbers in
  commit messages come from the trace, not from estimates.
- **Logs cannot prove absence.** Event-shaped logging proves presence
  only: a missing line means "nothing flushed", not "nothing
  happened" (the `[pty-in]` small-read accumulator is the canonical
  trap). Any claim of the form "X never happens" requires an
  instrument that affirmatively records zeros with a denominator —
  the reveal-floor's per-turn gap distributions are the pattern.
- **Register new subkinds.** Every new trace op or subkind lands in
  the trace-vocabulary table (below) in the same change that
  introduces it, so the reading half keeps pace with the writing
  half.


## Debugging Bram itself

Three forensics surfaces, used together. The first two are raw
streams; the third is a dashboard that derives signals from them.

**`resources/bram-traces/bram-trace.log`** — host-side rolling log of HTTP
routes, iframe events, and inflight-sentinel writes / clears.
Always on; grep it directly. Best for plumbing: stuck spinner,
sentinel anomalies, route errors, agent-turn-end detection,
heartbeat drift, close-cycle verification (`grep
"path=__issue/close" resources/bram-traces/bram-trace.log` — absence around a
known close timestamp means the agent bypassed
`gh_issue_close_with_commit` and shelled out to `gh issue close`
directly).

**Inspector Export** — XMLUI runtime trace (events, state changes,
handler invocations) for Bram's own XMLUI UI, captured on demand.
Best for in-pane misbehavior: a button doesn't fire, a DataSource
shows wrong data, a state change doesn't propagate, a component
renders wrong. Ask the user to open the Inspector (magnifying-glass
icon), reproduce, then click **Export** — writes
`~/Downloads/xs-trace-<timestamp>.json`. Analyze with the xmlui MCP
tools.

- **`xmlui_find_trace`** — locate the export by timestamp or content.

- **`xmlui_distill_trace`** — reduce to interactions / state changes
  / handler boundaries relevant to a specific question.

Don't read the raw JSON initially, it's huge, only grep as necessary.

**Status tab** — curated dashboard in the agent pane that
surfaces signals derived from `bram-trace.log` (rotated history
included) and from Inspector exports, alongside live process state.
Sections include Startup Run, Worklist, Inflight Sentinel, Hooks,
Authorization, Latest Tail And Fanout, and
Guards/Staleness/Interrupts/Traces. Check the Status tab first for
a quick read on whether something looks off — then drop down to
`bram-trace.log` or an Inspector Export for the underlying detail.

### Trace subkind vocabulary

`bram-trace.log` records iframe-side events as
`[iframe] subkind=<name> {…fields}` and host-side events as
`[<category>] op=<name> …` lines (parent-shell events arrive as
iframe-shaped subkinds with `context:parent`). Common entries you'll
grep for:

| Subkind | Emitter | Fields | Used for |
| --- | --- | --- | --- |
| `projected-turns` | `__bramRefetchProjectedTurns` in `helpers.js` | `reason`, `sid`, `turns`, `ms` | One line per coalesced `/__turns` refetch — the Transcript's heartbeat. `reason:tick` is the talk-session change signal (issue-214 candidate #5 replaced the latest-tail envelope pipeline with this tick). |
| `heartbeat-batch` | iframe heartbeat `Timer` | `fires`, `avgDriftMs`, `maxDriftMs`, `spikes`, `sumDriftMs`, `spanMs` | Iframe main-thread drift signal. Spikes correlate with fanouts that did real work; steady-state `maxDriftMs:11, spikes:0` is the green target between fanouts. |
| `listener-fired` | various `tauri.event.listen` handlers | `context` (`worklist-changed` \| `inflight-claim-changed` \| `pty-menu-changed` \| `talk-session-changed`); for `talk-session-changed` also `correlation_id`, `at_host_ms`, `delta_to_emit_ms` (iframe receive minus host emit, `-1` if the event predates `at_host_ms`) | Tauri event delivery into the iframe. |
| `event-received` | `talk-session-changed` listener in `helpers.js` | `correlation_id`, `subscribers`, `at_host_ms`, `delta_to_emit_ms` | Parent → iframe hand-off latency for `talk-session-changed`, logged once per host emit before subscriber fan-out. Pairs with the host `[emit] ... correlation_id=...` line to expose the Tauri event hop in isolation from subscriber dispatch. |
| `target-scheme` (host) | `handle_target_scheme` in `lib.rs` | `op=enter rel=<path>`, `op=refuse rel=<path>` | Per-request trace for the isolated target-pane origin (`bramapp://`, security C1). `op=enter` confirms the `bramapp` scheme is routed to the handler; `op=refuse` flags a dynamic host route (`__file`, `__worklist/*`, `__settings`, …) denied to target content. Static namespaces (`__project/*`, `__vendor/*`, `__shell/*`) proxy through with only an `op=enter` line. Used to confirm the isolation is live and to see what target content probes. |
| `describe-patch` | `__bramPatchProjectedToolDescription` in `helpers.js` | `stage` (`begin` \| `end`), `provider`, `name`, `turns`, `resultChars`, `descChars`, `ms` | Brackets the full-projection rebroadcast that splices a Haiku "Tool Descriptions" result (`ai.describeCommands`) into the transcript. Emitted synchronously before/after `__bramBroadcastProjectedTurns` (via `logToHost` → `invoke`, whose IPC dispatch survives an iframe main-thread freeze), so a hard freeze in the re-render is self-diagnosing: a `stage:begin` with **no matching `stage:end`** names the broadcast as the freeze and quantifies it (`turns`, `resultChars`). Added for the 2026-07-11 describe-freeze recurrence on a large Codex session (82 turns / 1 MB); unlike `long-task`, which logs at recovery and goes silent on a terminal freeze. |
| `refetch-called` | Workspace.xmlui debounce after `talk-session-changed` | `context`, `correlation_id`, `at_host_ms`, `delta_to_emit_ms` (host emit minus refetch-fire time, so it includes the 400 ms debounce coalesce) | Post-debounce refetch tick. A `delta_to_emit_ms` far above 400 ms means the iframe main thread was busy between emit and refetch. |
| `inspector-tap-tick` | `__inspectorTapTick` in `helpers.js` | `batch` (entries forwarded this tick), `available` (entries ready), `ms` (loop wall time) | Per-non-empty tick of the Inspector tap poller. Empty ticks are silent so this is a slow-tick alarm: a tick with `ms` ≫ 200 (the tick interval) means the IPC channel is backed up while the poller serializes entries through `logToHost`. Pairs with `inspector-event` / `inspector-overflow`. |
| `click` | UI Button onClick handlers (Workspace) | `target` (`approve` \| `drop` \| `iterate`), `item` | Worklist tab user actions. |
| `inflight-set` / `inflight-clear` | Workspace selectors + `inflightClaim` DataSource | `item`, `via`, `target`, `reason` | Inflight sentinel transitions; complements the host-side `[inflight-sentinel]` log entries. |
| `voice-input` | Worklist voice input path in `Globals.xs` | `stage` (`start` \| `recording-started` \| `stop` \| `append`), `target`, `requestId`, `stopAtMs`, `stopToResultMs`, `stopToAppendMs`, `parentStopToDeliverMs` | End-to-end voice latency for iframe-driven dictation. `stopToAppendMs` on `stage:append` measures Stop Record click to text insertion in the XMLUI input, useful for Mac/Windows comparisons. |
| `inspector-event` | `__inspectorTapTick` in `helpers.js` | `entry` (verbatim `window._xsLogs` record) | Per-entry forwarding of the XMLUI Inspector log into `bram-trace.log` so Inspector events interleave with host traces live (#181). Opt-in via the **Traces → Inspector trace tap** switch in Settings (persisted as `traces.inspectorTap` in `.bram.json`). Inspector traces are intentionally complete — every keystroke, render, state change — so volume is high; selectivity filters (drop categories, sample) are a follow-up. |
| `inspector-overflow` | `__inspectorTapTick` in `helpers.js` | `dropped`, `totalSeen` | Per-tick (200 ms) cap of 50 forwarded entries was exceeded; high-water mark advanced to current length and the listed count was dropped. Persistent overflow means cadence or cap needs tuning. |
| `turns-projection` (host) | `read_projected_turns` / `try_incremental_projected_turns` in `lib.rs` | `op=rebuild` (`src_bytes`, phase ms `read/parse/project/serialize`, `turns`, `window`, `body_bytes`, `total_ms`); `op=incremental` (`suffix_bytes`, `merged_turns`, `ms`) | Projection cost accounting on long sessions: the rebuild-vs-tail-merge ratio and which phase dominates (post-#214 measurement: parsing is ~10% of a rebuild; project/serialize dominate). |
| `reveal-floor` (host) | quiescence observer in the pty-throughput ticker, `lib.rs` | `op=would-reveal` / `op=reveal-suppressed reason=menu-displayed` / `op=reset reason=activity\|turn-changed\|turn-closed`, with `silence_ms`, `gap_p95_ms`, `gaps_n` | Phase-0 observe-only soak for the auto-reveal-terminal predicate ("turn open + byte-silent + no pane menu"). The graduation review greps these: every `would-reveal` must map to a corroborated terminal-needing moment. |
| `esc-scan` (host) | send-ledger escape sweep and soft turn-end poller, `lib.rs` | `op=sweep` (`read_ms`, `total_ms`, `bytes`); `op=soft-turn-end` (`ms`, `bytes`, `waited_ms`) | Times the per-Esc full-session scans. Exonerated the host in the 2026-07-08 wedge hunt (5 ms over a 26 MB session). |
| `xterm-liveness` (parent shell) | heartbeat watchdog in `app/main.js`, arrives with `context:parent` | `op=stall`, `gap_ms` | Measures freezes ≥500 ms of the parent main thread xterm renders on; one line per stall, logged at recovery. Stalls bracketed by a slow named op implicate it; absence during a felt wedge relocates the problem below the webview (child process / PTY). |
| `long-task` | `PerformanceObserver('longtask')` in `helpers.js` | `ms`, `name` | Iframe analog of `xterm-liveness`: one line per iframe main-thread task ≥200 ms, logged at recovery. Added for the 2026-07-09 describe-freeze (trace went silent at the freeze instant with nothing attributing the block); a hard freeze now names its duration instead of leaving a gap. |
| `resizeobserver-flood` | `installResizeObserverFloodDetector` in `helpers.js` | `firesPerSec`, `top` (className=count pairs) | Once per second while global ResizeObserver fire rate exceeds 50/sec, names WHICH elements are looping. The wrapped constructor counts every callback fire; `div._row_…` is XMLUI List's per-item wrapper, observed by virtua's item resizer. Diagnostic for the transcript RO-loop freezes (#150 lineage). |
| `resizeobserver-flood-detail` | `installResizeObserverFloodDetector` in `helpers.js` | `via` (`interval` \| `sync`), `newElements`, `repeatFires`, `ring1`..`ring4` | Companion to `resizeobserver-flood`: dumps the last ≤60 fires as compact strings — `+dt key#idx WxH*` (ms since prior fire, short element key `row`/`main`/`html`, `data-index` when present, contentRect to 0.1px, `*` = first-ever observation of that element). Discriminates the three flood mechanisms: same `#idx` alternating two heights = CSS oscillation (delta ≈15px → scrollbar, ≤1px → fractional rounding vs virtua's cache); streams of `*` across many indexes = remount loop (heights innocent); `main`/`html` entries interleaved with row re-measures = container size churn driving row re-wraps. `via:interval` is the per-second tick (its `newElements`/`repeatFires` are per-second counters); `via:sync` is emitted from INSIDE the RO callback when ≥120 fires accumulate without an intervening tick — i.e. the main thread stopped yielding — so a terminal freeze testifies instead of dying silent (counts derived from the ring; throttled to one per 2s; rides `logToHost` → `invoke`, whose IPC dispatch the host logs even if the iframe never yields again). Chunked strings because the trace serializer summarizes arrays and truncates strings at 500 chars. |
| `tool-format` | `__bramFormatToolResult` in `helpers.js` | `stage` (`begin` \| `end`), `tool`, `chars`, `longestLine`; on `end` also `ms`, `outChars` | Synchronous bracket around the tool-result formatter, emitted only for inputs >8KB (steady-state noise is zero). Built for the variant-B expansion freeze (2026-07-11 22:48Z: click → describe route entry → iframe dead, RO-quiet): with the click handler exonerated host-side, a freeze showing `begin` with no `end` names the formatter's string work; `begin`+`end` then silence names Markdown parse / WebKit layout by elimination. `longestLine` quantifies the long-line layout suspect that the formatter's 16KB total-size cap does not bound. Rides `logToHost` → `invoke`, so the host logs both stages even if the iframe never yields again (describe-patch precedent). |
| `xmlui-probe` | instrumented vendored engine (`~/xmlui` `script-runner/bram-probe.ts`; hooks in `evalBinding`, the statement-queue loop, and the container reducer) | `op` (`eval` \| `stmt` \| `action`), `d` (binding source / statement / action+uid, ≤80 chars) | Freeze-probe for the transcript-expansion hang: emits synchronously (`logToHost` → `invoke`, survives a frozen main thread) but ONLY while `window.__bramXmluiTraceUntil` is armed — `__bramExpandTool` arms 1.5s on each tool-row expansion click; inert otherwise and in every other xmlui embedding. A hang inside one evaluation/statement never returns, so the stream after a fatal `dom-click` ends AT the hanging site: `op=stmt` names a handler statement, `op=eval` a binding (with source text), `op=action` a state cascade. Expect a few hundred lines per armed click; that volume is the diagnostic, not noise. Remove the vendored probe once the hang is attributed upstream. |
| `send-ledger` (host) | ledger transitions and guards, `lib.rs` | `op=inject/transition/restore/auto-resend/aborted-no-restore/aborted-skip/stale-input-clear/stale-input-clear-skip` with entry ids, causes, byte counts | Outbound-send lifecycle forensics: landing vs strand vs abort classification, restores, and the stale-terminal-input clear decisions. |
| `hook-menu` (host) | permission-hook handlers and grid-defer decisions, `lib.rs` | `op=permission/payload/hook-diff/clear/retire-suppressor/grid-deferred/grid-emit-deferred/grid-emit-allowed` | Hook-primary menu coordination: hook claims and their payloads, diff enrichment (`hook-diff cluster=N`), fence-suppressor retirement, and whether the grid deferred or emitted for a hook-owned prompt. The menu-miss retrospective greps these. |
| `ai-describe` (host) | `handle_describe_command` in `lib.rs` | `op=call` (`ms`, `model`, `input_tokens`, `output_tokens`, `upgraded`, `ctx`, `result`, `id`); `op=hit` (`id`); `op=skip` (`reason=disabled\|no-key`); `op=error` (`status`, `ms`, `detail`) | One line per `/__describe-command` request — Haiku intent-header synthesis for tool expansions (haiku-command-descriptions). On by default (`ai.describeCommands`, the Settings "Tool Descriptions" switch); effective gate is `ANTHROPIC_API_KEY` in the host env. `op=call` carries latency + token counts so spend is grep-auditable. |
