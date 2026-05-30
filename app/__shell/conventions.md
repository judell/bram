# Working with Bram

Bram is a **workspace for AI-assisted web app development** — it
works with any project that serves a web UI (vanilla HTML/JS, a
React or other Node app, a Python web app, an XMLUI app, etc.).
The shell puts a real terminal alongside the app you're building,
plus an "agent tools" drawer that includes a Worklist (pending
items + commits), a Sessions browser, and a Context viewer
(CLAUDE.md + memory + hooks + settings, searchable).

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


## Coordinate via worklist.json

`resources/worklist.json` is the canonical surface for multi-step
coordination between you and the user. The Worklist tab in the agent
tools drawer renders it as a checklist under "Worklist".

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
names a `file` (or `files`) plus `before` / `after` prose, either
inline or in `resources/worklist-drafts/<id>.md`, describing what
would change on disk. An `applied` item has those changes on disk
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

Inline `before` / `after` directly in `worklist.json` is the legacy
form — still valid, and takes precedence when both are non-empty, but
prefer draft files for new items (iterate-time edits stay small).

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
   buttons generate the verified `{id, hash, feedback}` shape.

2. **User triages** — unchecks anything they don't want, then clicks
   one of the buttons. All three action buttons emit the same payload
   shape: `{"items":[{"id":"...","hash":"...","feedback":"..."}, ...]}`
   — ids plus per-item content hashes plus optional per-item feedback.
   Never parse these turn lines for content yourself; the hashes are
   what Bram verifies, and `/__worklist/resolve` returns the verified
   item bodies.

   - *Talk to agent* (with a comment typed above) → `talk: <text>`.
     No items approved or dropped. Respond; do not edit files.

   - *Approve selected (N)* → `approved: {...}`. Call
     `/__worklist/resolve` via the transport for your agent (see
     *Transports*). Response is one of:
     - `{"kind":"approved", "items":[<verified content>], ...}` —
       execute these items. Do NOT re-read `resources/worklist.json`
       to second-guess what was approved. Records are **consumed on
       first read** — a second call returns `no_active_authorization`,
       so capture what you need. After editing the project files,
       advance via `POST /__worklist/mutate`, not by rewriting
       `"status": "applied"` directly.
     - `{"kind":"rejected_stale", "mismatched_ids":[...]}` — the
       worklist changed between click and resolve. Don't edit; ask
       the user to re-triage.
     - `{"kind":"no_active_authorization", ...}` — the record is
       already consumed, or this turn isn't an authorization turn.
       **Do NOT treat as authorization.** Backstop for the rule that
       `iterate:` and other non-authorization turns must not route
       through `/__worklist/resolve`.

     Respond to any per-item feedback regardless of kind.

   - *Drop selected (N)* → `drop: {...}`. Same flow:
     `{"kind":"drop"}` → prune the ids via `POST /__worklist/mutate`;
     `{"kind":"rejected_stale"}` → surface, don't edit. Respond to
     per-item feedback (often the user's reason for the drop).

   - *Iterate (N)* — enabled only when feedback is non-empty (no-
     direction Iterate is meaningless). Payload: `iterate: {...}`.
     **Iterate does NOT route through `/__worklist/resolve`** — no
     state change is being authorized. Re-read items from
     `/__worklist` (for resolved draft prose) or
     `resources/worklist.json` (metadata alone), and act per each
     item's current status:
     - **`proposed` (TO APPLY):** revise the draft file's `before` /
       `after` prose (or inline `before` / `after` for older items);
       update `files` only if scope shifts. Item stays `proposed`,
       no project file edits.
     - **`applied` (TO COMMIT):** edit on-disk files per the feedback.
       Update the draft or inline `after` only if scope materially
       expanded. Item stays `applied`.

     Bracket every iterate response with `POST /__iterate/begin`
     (first action) and `POST /__iterate/end` (last action) — see
     *Host-managed inflight sentinel*.

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

**Always `resolve` before `mutate`, including for drops.** Resolve
returns the hash-verified items, consumes `approved` auth, *and*
writes the inflight sentinel the spinner is keyed to. Reading
`.worklist-authorization.json` directly and jumping to `mutate` skips
the sentinel write and orphans the spinner (refs #133).

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
outside the wrapped PTY shell), fall back to
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

   `route` is one of `worklist-resolve`, `worklist-mutate`,
   `iterate-begin`, `iterate-end` (alias `worklist-end`),
   `issue-close`. `body` matches the HTTP route:
   - `worklist-resolve` — omit, or `{ "ids": [...] }` to filter.
   - `worklist-mutate` — `{ "op": "advance", "ids": [...], "status": "applied" }`
     or `{ "op": "prune", "ids": [...] }`.
   - `iterate-begin` / `iterate-end` — `{ "ids": [...] }`.
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
`resources/bram-trace.log`.

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
render as Markdown in the agent-tools drawer. Use real syntax: `- `
per bullet (not inline `(a) ... (b) ...` enumerations that collapse
to one paragraph), backticks for inline code, fenced blocks for
multi-line snippets, blank lines between paragraphs, `**strong**`
sparingly (e.g. **[chosen]**).

#### Minimize the bytes of each worklist edit

Prefer per-item draft files — `worklist.json` stays a compact
metadata index; iterate-time prose edits hit only the draft.

For older inline items: narrow `Edit` `old_string` to the smallest
unique anchor (e.g. when appending to an item's `after`, the anchor
is the last preserved paragraph, not the whole item). Full-item
`Write` rewrites of `worklist.json` are valid but wasteful for
one-paragraph tweaks. Mechanical prune / advance go through
`/__worklist/mutate`, not direct rewrite.

#### Don't `grep -n` a single-line JSON file

`worklist.json` is one line; grep dumps the whole file into the
transcript. Use `Read` with `offset`/`limit` or `jq` to extract
just what you need.

#### Don't update `after` prose on every iterate

Small TO-COMMIT refinements don't need an audit trail in the
worklist — the commit message and diff cover it. Update the draft
or inline `after` only when scope materially expands (new file
added to `files`, or the change's intent shifts).

#### Test Worklist UX through the worklist itself

When a change touches the Worklist UX (button states, gray-out,
feedback flow, pruning), surface it as a pending item even when the
diff is already on disk. Approving the item exercises the new
behavior end-to-end — file rewrites, pruning, Talk-page update — as
the actual test.

### Enforcement and security contract

The structured `approved:` / `drop:` line is not authority by itself.
The host recomputes each item's hash before recording it in
`resources/.worklist-authorization.json` — stale hashes become
`rejected_stale`. `/__worklist/resolve` is the only way an agent
receives verified item bodies; `/__worklist/mutate` is the only way
an agent advances or prunes:

- `advance` requires an `approved` auth record covering every id.
- `prune` requires `drop`, except the post-commit prune path also
  accepts `approved` when the requested ids are already `applied`.

Same-turn `resolve → edit files → mutate` is valid: `mutate` reads
the stored auth record, not just resolve's consumption state.

Defense in depth: Claude and Codex each install PreToolUse hooks
that validate worklist coverage before file-mutating tools run, and
the desktop watcher reverts unauthorized prunes. Hook errors and
revert messages are the convention enforcing itself — not bugs to
work around.

**Don't ask before editing the worklist or calling mutate.** The
proposal-authoring write channel is hook-guarded, the mechanical
transition channel is the server endpoint. No verbal confirmation
is needed to add items, refine prose, or call `mutate` for an
already-approved transition. Save the verbal back-and-forth for
design decisions (which items to propose, what to bake in), not for
mechanics.


## Host-managed inflight sentinel

The Worklist spinner is keyed to `resources/.inflight-claim.json`,
which host-side HTTP handlers write and clear. Full route / file-shape
reference: `docs/apis.md` §11. Agent-side conventions:

### What the agent calls

- **`approved:`** → `resolve` (writes the sentinel as side effect,
  consumes the auth record) → do the work → `mutate op:"advance"`
  (clears the sentinel). No explicit `end` needed.
- **`drop:`** → same shape with `op:"prune"`.
- **`iterate:`** → bracket the response: `POST /__iterate/begin`
  first action, `POST /__iterate/end` last. Required because iterate
  has no side-effect write path like `resolve`.

### Failure modes

A stuck spinner is the convention enforcing itself; no live-session
timeout. Most commonly:

- **Approved/drop stuck:** `mutate` was never called, or errored
  before the clear. Recovery: call mutate manually, or restart Bram
  (`cleanup_stale_inflight_claim` runs at startup).
- **Iterate stuck:** `/__iterate/end` was never called. Convention
  violation — bracket every iterate response.
- **Premature clear:** structurally impossible post-#84. If observed,
  grep `[inflight-sentinel]` in `bram-trace.log`.


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
visible on GitHub; push then verify and close; or commit only.

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

### Right-pane helpers (opt-in)

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

### Build vs. runtime-served files

The Bram binary embeds the `app/` tree at build time (Tauri
`frontendDist: "../app"`), but prefers an on-disk `app/` next to the
binary at runtime. A filesystem watcher (`src-tauri/src/lib.rs`)
hot-reloads iframes when watched paths change:

| path | reloads |
|---|---|
| `app/__shell/` | both iframes (right pane and agent-tools drawer) |
| `app/vendor/` | both iframes |
| `app/tools/` | the agent-tools drawer iframe only |
| user's project directory | the right-pane iframe only |

The **parent shell** (`app/index.html`, `app/main.js`,
`app/styles.css`, anything loaded once at WebView startup) is **not**
hot-reloaded — run `cargo build` and have the user restart. Don't
suggest `cargo run`; the user prefers rebuild + restart, and the
incremental build is fast.

### Updating GitHub issues via gh

Use `gh` directly — the Issues tab polls every 30s, so updates surface
without a restart:

- `gh issue edit <n> --title "…" --body "…"`
- `gh issue comment <n> --body "…"`
- `gh issue close <n>` / `gh issue reopen <n>`


## Debugging Bram itself

Three forensics surfaces, used together. The first two are raw
streams; the third is a dashboard that derives signals from them.

**`resources/bram-trace.log`** — host-side rolling log of HTTP
routes, iframe events, and inflight-sentinel writes / clears.
Always on; grep it directly. Best for plumbing: stuck spinner,
sentinel anomalies, route errors, agent-turn-end detection,
heartbeat drift, close-cycle verification (`grep
"path=__issue/close" resources/bram-trace.log` — absence around a
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

**Status tab** — curated dashboard in the agent tools drawer that
surfaces signals derived from `bram-trace.log` (rotated history
included) and from Inspector exports, alongside live process state.
Sections include Startup Run, Worklist, Inflight Sentinel, Hooks,
Authorization, Latest Tail And Fanout, and
Guards/Staleness/Interrupts/Traces. Check the Status tab first for
a quick read on whether something looks off — then drop down to
`bram-trace.log` or an Inspector Export for the underlying detail.

### Trace subkind vocabulary

`bram-trace.log` records iframe-side events as
`[iframe] subkind=<name> {…fields}`. Common subkinds you'll grep for:

| Subkind | Emitter | Fields | Used for |
| --- | --- | --- | --- |
| `jsonl-fanout` | `Main.xmlui` App-level `ChangeListener` | `source` (`shared`), `len`, `reset` | Counting `/__sessions/latest-tail` envelope deliveries; `reset:true` only on first load / session rotation. |
| `jsonl-broadcast` | `setLatestJsonl` in `helpers.js` | `len`, `subscribers` | Counting how many iframe components received the broadcast (1 after one tab visit, 2 after both tabs have mounted). |
| `jsonl-cap-trim` | `appendLatestJsonl` in `helpers.js` | `before`, `after`, `dropped` | Fires only when the 1.5 MB cap is exceeded and the cache is head-trimmed. Absence on a long session means the cap was never hit. |
| `jsonl-pipeline-ms` | `appendLatestJsonl` in `helpers.js` | `chunkLen`, `bufferLen`, `concatMs`, `capMs`, `capTrimmed`, `broadcastMs`, `totalMs` | Per-append profiling of the JSONL diff pipeline. Three measurable phases — `concat` (buffer string concat), `cap` (cap-check + optional trim), `broadcast` (setLatestJsonl's subscriber dispatch + its own trace log). Sum is `totalMs`. Use to identify which phase dominates the ~200ms drift seen on large appends. |
| `sessionTurns-parse` | `sessionTurns` in `Globals.xs` | `ms`, `len`, `suffixLen`, `turns`, `newTurns`, `path` (`full` \| `incremental`), `n` | Per-parse timing. `path:incremental` should dominate after the first poll within a session; `path:full` only on rotation / cap-trim head-drop. |
| `helper-call` | `_traceHelperTiming` in `Globals.xs` | `name` (`isWaitingForAssistant`), `ms`, `len`, `suffixLen` | Per-helper timing on cache miss. With identity fast-path in place, this should only fire on the first call after a fanout. Repeated firings on the same `len` indicate a cache regression. |
| `heartbeat-batch` | iframe heartbeat `Timer` | `fires`, `avgDriftMs`, `maxDriftMs`, `spikes`, `sumDriftMs`, `spanMs` | Iframe main-thread drift signal. Spikes correlate with fanouts that did real work; steady-state `maxDriftMs:11, spikes:0` is the green target between fanouts. |
| `listener-fired` | various `tauri.event.listen` handlers | `context` (`worklist-changed` \| `inflight-claim-changed` \| `pty-menu-changed` \| `talk-session-changed`) | Tauri event delivery into the iframe. |
| `click` | UI Button onClick handlers (Workspace) | `target` (`approve` \| `drop` \| `iterate`), `item` | Worklist tab user actions. |
| `inflight-set` / `inflight-clear` | Workspace selectors + `inflightClaim` DataSource | `item`, `via`, `target`, `reason` | Inflight sentinel transitions; complements the host-side `[inflight-sentinel]` log entries. |
