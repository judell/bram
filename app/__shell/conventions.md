# Working with Bram

Bram is a **workspace for AI-assisted web app development** — it
works with any project, whatever it serves. The shell puts a real
terminal alongside an "agent tools" pane that includes a Worklist
(pending items + commits), a Sessions browser, and a Context viewer
(CLAUDE.md + memory + hooks + settings, searchable).

Bram can *optionally* embed a **target app** — a project iframe that
previews a web UI (vanilla HTML/JS, a React or other Node app, a Python
web app, an XMLUI app, etc.). This pane is **off by default** and a
minority case: most users view their app in their own browser. Don't
assume an iframe is present — detect before you rely on it.

> Note on memory: Setup seeds this file into every managed project as
> `.claude/bram-conventions.md`, `@`-imported by `CLAUDE.md`. **Don't
> save project-related memories** — worklist preferences, helper APIs,
> release quirks, conventions you discover. Per-user memory is private
> to one agent on one machine; shared files reach everyone running Bram
> (Claude and Codex alike). Route by audience: knowledge that would
> change an agent's behavior in a project that is NOT the Bram source
> repo belongs in these conventions; knowledge that matters only when
> editing Bram's own source belongs in Bram's source-repo developer
> docs; XMLUI-framework-generic findings go to the upstream xmlui docs,
> reachable through the xmlui-mcp server. Memory stays reserved for
> what can't live in any repo (cross-project user preferences,
> provider-specific tool quirks).

Bram's own UI is XMLUI. When developing Bram, or when the app under
development is XMLUI, expect the XMLUI MCP server to be available, read
the xmlui_rules, and follow them.

**Reference files.** Detail you need only in a recognizable situation
lives beside this file in `.claude/bram-reference/`:
`worklist-mechanics.md` (transports, entangled commits, delegation,
enforcement), `diagnostics.md` (stuck spinners, traces, logs, evidence,
Bram's guards) and `environment.md` (target-app helpers, `/query`, forge
CLI, test suites, Windows, cross-project work). Each section below says
when to read which. Setup refreshes these files and Bram-bundled skills
(`.claude/skills/<name>/SKILL.md` carrying a `<!-- bram-managed` marker);
don't make functional edits to installed copies, since the next Setup
overwrites them.


## Coordinate via worklist.json

`resources/worklist.json` is the canonical surface for multi-step
coordination between you and the user. The Worklist tab in the agent
pane renders it as a checklist under "Worklist". It doesn't need to
exist in advance — Bram serves an empty default and the Worklist tab
creates the file (and `resources/`) on first use. Empty state is fine:
`{ "description": "", "items": [] }`.

### When to route through the worklist

**Default: every change request goes through `resources/worklist.json`.**
Single-file, single-line, single-attribute — size doesn't matter.
Propose first, wait for the user's `approved:` payload, then apply. The
two-stage proposed → applied flow lets the user redirect or veto before
any code is touched, and the worklist history is the audit trail for
what landed and why.

Skip the worklist only in these contexts, never because the change is
"small":

- **Explicit user opt-out in this turn.** The user ends their message
  with the single phrase "just do it" (case-insensitive). It must be in
  the same turn as the change request — don't carry it forward across
  turns or infer it from past patterns. Retired phrases ("skip the
  worklist", "commit this directly", "inline the fix", "no worklist for
  this", "don't bother with the worklist") no longer opt out. Claude
  and Codex honor the same phrase, so the user-facing contract is the
  same regardless of agent.
- **`skip-worklist:` structured prefix on this turn.** The turn begins
  literally with `skip-worklist: ` followed by the request. Same family
  as `approved:` / `drop:` / `iterate:`, but it authorizes a direct
  edit rather than a lifecycle transition. The **skip worklist** button
  that once prepended it is retired; today the user types the prefix,
  or uses the "just do it" phrase. Unlike the other prefixes, the host forwards the **entire
  turn text including the prefix** so you can see it. On seeing it,
  skip propose-first, act on the rest of the message as a direct edit,
  and do not write a new worklist item; the PreToolUse hook allows the
  edits.
- **Correcting code you just wrote in the current iteration.** Fixing a
  typo or off-by-one from your last turn is iteration on in-progress
  work, not a fresh change request.
- **Iterating on an uncommitted draft.** While you and the user bounce
  edits on a file not yet committed, direct edits keep the loop tight.
  Once the draft is committed, fresh edits are change requests again.
- **Issue-only forge work with no repo diff.** Creating, editing,
  commenting on, closing or reopening a forge issue, when the task
  modifies no tracked files and produces no commit: do it directly with
  the project's forge CLI (`gh` on GitHub, `glab` on GitLab). If paired
  with repo changes, the repo changes still go through the worklist.

- **How the opt-outs are enforced (sidecar, audit breadcrumbs)** → read `.claude/bram-reference/worklist-mechanics.md` (§Opt-out plumbing).
- **Using `gh` / `glab`** → read `.claude/bram-reference/environment.md` (§Updating forge issues via gh / glab).

### What worklist items represent (and when to drop)

**Worklist items represent repository changes.** A `proposed` item
names a `file` (or `files`) plus `before` / `after` prose in
`resources/worklist-drafts/<id>.md`, describing what would change on
disk. An `applied` item has those changes on disk awaiting commit
approval — and so can a `proposed` item that has **begun** (approved
and started, not advanced): once its changes are on disk the pane
offers Commit on it directly, `status` unchanged. Items exist to give
the user explicit veto power over what lands in their repo.

Investigation work does NOT belong in the worklist — checking whether
a port is open or a server is running, restarting a process or a
Docker container. That happens in chat: nothing to write, nothing to
land.

**If an investigation reveals nothing to commit, guide the user to
Drop.** When the item turns out to need no change (a runtime
configuration issue, a process restart, every check passed):

- Do NOT call `/__worklist/mutate op:"advance"` — it produces a row
  with nothing to commit.
- Summarize the finding in chat ("checked X, Y, Z; the issue is
  runtime-only, no code change needed") and recommend the user click
  **Drop** on that item in the Worklist tab. Drop works the same as any
  other drop and equally well on `proposed` and `applied` items.
- **Already advanced?** Same recovery: explain, recommend Drop on the
  resulting row. No special undo path.

**Write the finding into the draft before reporting it in chat.** Chat
does not outlive the turn; the draft does. "Nothing came of it"
collapses three situations with opposite right answers: never got to
it (**Start again**), worked it and found nothing to change (**Drop**),
or found the item's premise **false** (**Drop**, and the draft's own
claims must say they are wrong). Record in
`resources/worklist-drafts/<id>.md` which of the three it is: the
premise that failed, what was verified, and what would make the item
actionable again.

**Drop removes the item, not the bytes — and orphaned changes are
misattributed, not merely unattributed.** The pane reasons about
changed files *through items* (the overlap index walks `item.files`,
`changeSummary` is per item, exclusivity asks whether another begun
item claims the path), so a changed file no item declares is credited
to whichever begun item declares it, and is committable under that
item's id. When dropping an item that has **begun**:

- Say what remains on disk before the drop completes, and propose one
  of: revert it, re-home it under another item's `files`, or leave it
  deliberately — then say which was chosen.
- Prefer `git stash push -m "<item id> (dropped): <what>"` over
  discarding; a stash costs nothing and keeps the work recoverable.

This is also why you must not edit outside an authorized item:
unauthorized edits don't merely skip the audit trail, they are credited
to someone else.

### Reminder items (placeholders you can drop)

One item shape carries no diff yet and is still legitimate: a
**reminder** (placeholder) for an action already decided but gated on
an external condition — an upstream merge, a release being cut, another
agent's verdict — that resolves after this session ends. Chat dies with
the session; the reminder carries it across.

- **Name.** A reminder's id **begins with `reminder-`**
  (`reminder-revendor-after-xmlui-release`); after the prefix the
  ordinary id rules apply. When the gate is a **date**, put it at the
  end of the id (`reminder-rerun-census-after-2026-09-19`). The rule is
  mechanical on purpose: surfaces such as the Awaiting You inbox key on
  the prefix. Ids are immutable, so an older reminder keeps its name;
  the only conversion is the user Dropping it and you re-proposing it
  verbatim under a `reminder-` id. Don't push that, but say plainly
  that prefix-keyed surfaces won't see the old row.
- **Shape.** `Before` states the awaited condition plus enough
  self-contained context to act with no conversation history. `After`
  states the action Approve green-lights and what condition would make
  Drop the right verdict. `files` lists what the eventual action will
  touch (empty for issue-only actions).
- **Lifecycle.** Approve = condition met; do the action, then it
  behaves like any approved item. Drop = mooted or superseded — an
  expected, honorable ending, not a failure.
- **Boundary.** Not a door back to investigation items: a reminder
  records a *decided future action*; an open question is chat's job.

### Schema and draft layout

Metadata and review prose live in two files:

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
so the pane sees one combined item; hashes cover metadata + resolved
prose. A missing draft yields empty `before` / `after` plus
`"_draftMissing": true` and a placeholder in the UI.

**The `version` rule.** `worklist.json` carries a top-level `version`
integer. Every write MUST set `version: N+1`, where `N` is the value on
disk when you read it; both agents' PreToolUse hooks deny any other
bump (`/__worklist/mutate` bumps on its own path under a mutex). So:
read and capture `version`; write with `version: <captured + 1>`; on a
`reason=stale-worklist-version` denial, re-read, rebase your change on
the new contents, and retry. A file with no `version` is version 0, and
the first write introducing `version: 1` is allowed.

**Prose lives only in the draft file.** Both guards reject inline
`before` / `after` keys in `worklist.json`. Prose edits made in response to item
feedback go to the draft; `worklist.json` changes only when metadata (`files`,
`closesIssues`, etc.) shifts.

**Field notes:**

- `files: ["path/a", "path/b"]` for multi-file items; `file` (singular)
  is the older single-file form. A directory entry covers everything
  under it at the commit gate. Once committable, the item's inline diff
  concatenates all listed files.
- `closesIssues` declares which issues the commit resolves and drives
  the close-on-commit dialog. Set it conservatively — only when the
  commit truly closes the issue, not for cross-references (`see #N`,
  `related to #N`, partial multi-step work); omit or use `[]` to skip
  the dialog. Before the item is committable it is an *association*
  (the pane renders "for #N"); "closes #N" and the gate's ticks appear
  only once committable. **Before requesting commit approval,
  re-verify the claim against what the work became, and remove
  `closesIssues` when it no longer resolves the issue** — a documented
  duty. A one-click **Start & commit** of pure plans offers no close
  ticks and queues no closes; close afterwards from the forge or via a
  normal commit gate on a follow-up item.
- `begunAtMs` is **host-written, never authored by an agent.** The host
  stamps it at the first `approved` authorization covering the item and
  never moves it while the item lives. It is the durable answer to "has
  work on this item begun?" (the authorization record and inflight
  claim are single-slot and displaceable). Don't set it; its absence
  means only "never approved".
- `status`: `"proposed"` (default) — the user is approving you to make
  the change; the strip reads "No changes yet" until work begins
  (legacy badge **TO APPLY**). `"applied"` — the change is on disk and
  the user is approving `git commit` (legacy badge **TO COMMIT**); push
  is decided separately via the Push button. `applied` means
  committable, but a begun `proposed` item can be committable too; the
  pane reasons in three states: not started, nothing to commit, has
  changes you can commit.

Default to the two-stage flow: approved `proposed` → advance to
`applied` → user approves a separate commit → prune. Skip `applied`
only when the user says "apply and commit" up front. Drops prune
directly. Don't pre-mark new items `"applied"` unless the change is
genuinely already on disk. This governs how you author and complete
status; the user's Commit button judges committability independently.

### Lifecycle: propose → triage → mechanical transitions

1. **Propose** — write the draft to `resources/worklist-drafts/<id>.md`,
   then the metadata item to `resources/worklist.json`. Each item small,
   discrete, independently rejectable. Writing the item is *asking*, not
   approval. Don't show or instruct on raw `approved:` / `drop:` /
   `iterate:` payloads — the buttons generate them.
2. **User triages** — ticks rows, optionally types one message, clicks a
   button. The message fans out to every selected item: treat each
   item's feedback on its own terms, but answer identical fanned-out
   feedback **once** in chat, never per item. Payload shape:
   `{"items":[{"id":"...","feedback":"..."}, ...]}`. Never parse these
   turn lines for content yourself.
3. **Mechanical transitions** — `POST /__worklist/mutate` is the only
   channel for approval-driven state changes:
   `{"op":"advance","ids":[...],"status":"applied"}` after an approved
   apply; `{"op":"prune","ids":[...]}` after a drop, or after a commit of
   already-`applied` items. Never rewrite `"status": "applied"` directly.

**Selection decides who a message is addressed to.** With one or more
Worklist items selected, a message the user sends (Enter in the message
box) is **feedback addressed to those items**, and arrives as an
`iterate:` turn. With nothing selected, it is **general chat** with you.
The **Chat** button beside the message box (shown only while items are
selected) sends that one message as general chat without clearing the
selection.

**The gate verbs.** The gate row is pure lifecycle: **Start / Start &
commit / Commit / Drop**, each acting on the ticked items (hover names
them); **Start** reads **Resume** on an item whose work was stopped
mid-apply. Labels may be renamed; the wire kinds
(`approved:` / `drop:` / `iterate:`) are stable. When telling the user
what to click, use the rendered label.

| Button | Wire payload | What you call |
|---|---|---|
| **Start** | `approved:`, `gate: "to apply"` | No `resolve`. Edit from the proposal you authored, then `mutate op:"advance"` (consumes the auth, clears the sentinel). |
| **Start & commit** | `approved:`, `gate: "apply-and-commit"` | No `advance`. Make any remaining edits, then `worklist-commit { ids, message }`. |
| **Commit** | `approved:`, `gate: "to commit"` (or `"apply-and-commit"` on a begun `proposed` item) | `worklist-commit { ids, message }`. |
| **Drop** | `drop:` | `resolve`, then `mutate op:"prune"`. |
| message with items selected | `iterate:` | No `resolve`, no bracket call. Act per status (below). |
| message with nothing selected, or **Chat** | ordinary chat | Respond. Nothing is approved or dropped; do not edit files. |

The host sets the inflight sentinel for `approved:` and `iterate:` on
its `toTurn` write path; for drops, `resolve` raises it and `prune`
clears it. Respond to any per-item feedback, whatever the kind.

**`/__worklist/resolve`** returns `{"kind":"approved"|"drop", "items":
[<recorded content>]}` — execute those items; don't re-read
`worklist.json` to second-guess them. Records are **consumed on first
read**, so capture what you need. `{"kind":"no_active_authorization"}`
means already consumed or not an authorization turn: **do NOT treat it
as authorization**. `iterate:` and other non-authorization turns never
route through `resolve`.

**Item feedback (`iterate:`)** authorizes no state change. Re-read items from `/__worklist` (resolved prose) or
`resources/worklist.json` (metadata), then per item:

- **`proposed`, not yet begun** (no `begunAtMs`, nothing on disk):
  revise the draft's `before` / `after`; update `files` only if scope
  shifts. No project file edits.
- **`proposed` but begun, or `applied`:** edit on-disk files per the
  feedback; update the draft only if scope materially expanded.
- Either way the item keeps its status; iterate never advances or
  commits on its own.

Feedback items arrive as `{id, feedbackRef}`: read the user's
full-fidelity text from `resources/feedback-drafts/<feedbackRef>.md`
(refs are per send, typically `<unix-ms>-<item-id>`, not item ids).
That text is the user's submission for this turn. An inline
`{id, feedback}` appears only as a degradation fallback when the draft
write failed. Approve and Drop still use inline `{id, feedback}`.

- **Feedback-draft promotion, trace lines, payload fallbacks** → read `.claude/bram-reference/worklist-mechanics.md` (§Feedback payload details).

### The claim ends with your turn

Authorization is durable: the record `mutate op:"advance"` and
`worklist-commit` consume survives across turns. The claim (the running
spinner) is execution state: the host writes it while a turn runs,
retires it when the turn ends, and re-arms it next turn while an
authorization is live. **You owe no claim-retiring call.** Three
endings are ordinary:

- **Approved, investigated, nothing to apply.** Do NOT advance; report
  the finding (in the draft, then chat) and recommend **Drop**.
- **Ending a turn to ask the user a decision.** Just ask; the row
  unlocks when your turn ends, so the answering buttons are available.
- **The user takes over the commit** ("I'll commit"). End your turn;
  Commit is a button.

`POST /__worklist/end` with `{"ids": [...]}` is an optional early
release to unlock the board mid-turn.

- **Details of claim vs. authorization, `/__worklist/end` responses** → read `.claude/bram-reference/worklist-mechanics.md` (§The claim and the authorization).
- **A multi-id claim retires partially (`clear-shrink`, `consume-shrink`, `cleared:false`)** → read `.claude/bram-reference/worklist-mechanics.md` (§Incremental claim and authorization retirement).
- **A stuck spinner** → read `.claude/bram-reference/diagnostics.md` (§Inflight sentinel failure modes).

### Commit and apply-and-commit gates

**Apply-and-commit** (`gate: "apply-and-commit"`) comes from the
one-click **Start & commit** or from a plain **Commit** on a begun
`proposed` item. Handle both the same: do **not** `mutate
op:"advance"`; make remaining edits, then call `worklist-commit
{ ids, message }`. The host commits the still-`proposed` files under the
click's `commitToo` authorization and prunes.

**Commit gate** (items already `applied`; `approved:` with `gate: "to commit"`):
call `worklist-commit` with `{ ids, message }` — one request when the
items land together. The host verifies the approval, stages only those
items' files, refuses unrelated staged files, commits, prunes, and
records any ticked issue closes. **Closing is fully automatic — do
nothing**: there is no close route to call and no close step to tell
the user to run. Closes fire on the user's next **Push**.

The pane offers Commit on any begun item with changes; when a path is
shared with another begun item the host stages only the requested
items' own hunks. Committing entangled items **together** is safe;
committing **one of N** is the risky operation.

**`worklist-commit` is single-shot per approval.** On a timeout, never
re-POST: check `git log` first and treat what you find as the answer. A
response saying the commit "ALREADY RAN" is success.

- **Entangled commits, 409 refusals, `op=refuse-joint-interval`, `split-shared-files`** → read `.claude/bram-reference/worklist-mechanics.md` (§Interval staging and entangled commits).
- **A slow or timed-out commit** → read `.claude/bram-reference/worklist-mechanics.md` (§Commit timeouts: never re-POST).
- **One click approved several items that share files** → read `.claude/bram-reference/worklist-mechanics.md` (§Serializing entangled items approved together).
- **About to delegate worklist work to subagents** → read `.claude/bram-reference/worklist-mechanics.md` (§Delegating worklist items to subagents).

### Transports

**Claude: loopback curl.** Bram writes its port to `resources/.bram-port`
(plain decimal). Read it once and substitute the literal number
(`$BRAM_PORT` won't match the permission allowlist):

```
curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1 \
  "http://127.0.0.1:61455/__worklist/resolve"
```

POST routes (`/__worklist/mutate`, `/__worklist/commit`) must keep this
exact shape or they prompt:

```
curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1 -X POST \
  -H "Content-Type: application/json" --data @/tmp/body.json \
  "http://127.0.0.1:61455/__worklist/commit"
```

(replace `61455` with the value from `resources/.bram-port`). Two
pitfalls:

- **Include literal `-X POST`.** `--data` alone matches neither the
  POST nor the GET allowlist entry.
- **Keep the curl a standalone command.** Build the JSON body in a
  *separate* Bash call (`jq … > /tmp/body.json`), then `--data
  @/tmp/body.json`. A compound command starting with `cat` or a heredoc
  can't match. Apostrophes and quotes in commit messages belong in the
  body-building step.

- **Why these flags; a port that keeps refusing; repeated 5xx** → read `.claude/bram-reference/worklist-mechanics.md` (§Claude transport: flag rationale and stale ports).
- **You are Codex (filesystem intent/result files)** → read `.claude/bram-reference/worklist-mechanics.md` (§Codex transport: filesystem intent/result files).

### Notice when feedback drifts from its item

Feedback sent with a row ticked is filed to that item's audit trail
(`resources/feedback-drafts/`, then `feedback-history/`), so feedback
about something else makes the record **false**. When a turn's feedback
is plainly about something else — a different subsystem, bug or
project — open with the question, verbatim:

> Did you mean to address `<item-id>` just now?

then note that the **Chat** button (or sending with nothing selected)
is how to talk outside an item's context, and **stop there and wait for the answer**. It is not a
refusal — the answer is usually "that was meant as chat", after which
you do the work — but it *is* blocking: proceeding writes a false audit
record and does the work in the wrong context. Asking and then
proceeding regardless is the worst option. Fire on obvious drift only,
**once per drift**; a tangent that touches the item, or a follow-up
about work just done, is not drift. The test: would a reader six months
from now, seeing this feedback on this item, be misled about why it
changed?

### Authoring conventions

- **Ids.** Items from a single GitHub issue: `issue-<N>-<slug>`
  (`issue-86-pty-intent-relay`), paired with `closesIssues` when both
  apply. Exploratory, cross-cutting or multi-issue items: a bare
  descriptive slug (`worklist-drafts-separate-prose-from-metadata`). Reminders: the `reminder-` prefix. **Ids are
  immutable** — removing an id from `worklist.json` reads as an
  unauthorized prune and the host silently reverts the write (a draft
  you already moved stays moved, leaving `_draftMissing`). If an id
  under-names its item, keep it and say so in the draft.
- **Keep `files` current.** It is your prediction of what the change
  touches and the denominator of the pane's `N of M planned`. When a
  listed file proves unneeded, update `files` (an ordinary version-bumped
  edit) so the count converges to `N of N`; the draft keeps the original
  prediction.
- **Refer to items by id, never by ordinal** ("item 3", "the second
  one") — ordinals shift; ids match the pane and payloads.
- **Match prose to complexity.** Small mechanical changes: a short
  paragraph each. Judgment-load changes: name the alternatives and mark
  `[chosen]`:

  > - Embedded diff via DataSource — rejected: each row would fire its own request.
  > - **[chosen]** Server augmentation via `/__worklist` — single payload, per-item diffs travel with each row.

  Rule of thumb: could a reader six months on reconstruct the decision
  from code + git log alone? Yes → short. No → fulsome.
- **Use Markdown in item prose** — it renders: `- ` per bullet (not
  inline `(a) … (b) …`), backticks, fenced blocks, blank lines between
  paragraphs, `**strong**` sparingly.
- **Minimize the bytes of each edit.** Prose tweaks touch only the
  draft; prune/advance go through `/__worklist/mutate`, not rewrites.
- **Don't `grep -n` `worklist.json`** — it is one line; use `Read` with
  `offset`/`limit` or `jq`.
- **Don't update `after` on every iterate.** Only when scope materially
  expands (a new file in `files`, or the intent shifts).

### Enforcement

The `approved:` / `drop:` turn line is not authority by itself: the
host records each clicked id, `/__worklist/resolve` is the only way to
receive recorded bodies, `/__worklist/mutate` the only way to advance
or prune, and PreToolUse hooks (Claude and Codex) check worklist
coverage before file-mutating tools run while the desktop watcher
reverts unauthorized prunes. **Hook denials and reverts are the
convention enforcing itself — not bugs to work around;** never route
around one. **Don't ask before editing the worklist or calling
mutate**: adding items, refining prose and already-approved transitions
need no verbal confirmation. Save back-and-forth for design decisions.

- **What each route requires; the structural security account** → read `.claude/bram-reference/worklist-mechanics.md` (§Enforcement and security contract).


## Talking to users

### Name UI affordances, not protocols

When the user needs to act and a control exists, name it: "Click the
**Start** button" (Start & commit, Commit, Drop, Chat, Push, Trust
this hook, Setup). Never say "send `approved: {...}`", "paste the
structured approval payload", or describe the wire format — the button
generates the verified payload.

### Keep internal jargon out of user-facing chat

"Inflight sentinel", "resolve/mutate", "PreToolUse hook", "worklist
authorization record" are internals. Talk about what the user sees and
does: "the Worklist tab", "approve the item", "the spinner cleared".
Use jargon only when asked about internals or when pointing at a path
they'll grep.

### Cite, don't gesture

When referencing a file, route, or doc, name it
(`resources/worklist.json`, `/__worklist/resolve`,
`.claude/bram-reference/diagnostics.md`) so the user can verify in one
click. If you can't cite, say so.

### Match terseness to the question

No preamble ("Great question!"), no restating the question, no trailing
summary unless load-bearing. The Worklist tab shows the items, the diff
shows the code; chat is for what those can't show.

### Narrate as you reach for tools

Before a tool call (or batch), say in a sentence what you're about to
do and why. When a result changes your plan, say so before acting on
it. Long-running work (builds, test suites, background tasks) gets a
status line when it starts and when it lands. This doesn't conflict
with terseness: drafts and commit messages carry the full story;
narration is the live one-line version.

### Cross-project pivots

When the user pivots to a different project ("let's look at
~/other-app"), flag the boundary and offer the choice: a quick
read-only look from here, or a handoff to that project's own session.
Sustained investigation, issue filing and follow-up belong in the
target project's session, because session transcripts, the `/__search`
index and worklist history are scoped by working directory and record
work where it *runs*. If the user proceeds from here anyway, make any
artifact left in the target project (issue, doc, commit message)
self-contained, carrying the evidence inline.

- **Two sessions coordinating across a boundary (upstream, another machine, another agent)** → read `.claude/bram-reference/environment.md` (§Working across project boundaries).


## Signing agent-authored forge artifacts

**Every agent-authored forge artifact opens with a signature** — issue
bodies, issue comments, PR descriptions, PR comments, reviews,
gists/snippets — in every repo. Agents post through the human's
account, so an unsigned agent comment is indistinguishable from the
human's. The form:

    <owner>'s <Agent> (Bram <version>, <thread>, <model>, <os>, <machine>) speaking from the <Project> project (<forge-host>/<owner-or-group>/<repo>):

For example:

    Jon's Claude (Bram 0.6.5, main thread, Fable 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):
    Jon's Codex (Bram 0.6.5, main thread, gpt-5.2-codex, macOS, Tuck) speaking from the XMLUI project (github.com/xmlui-org/xmlui):

- `<version>` — Bram's version, from `GET /__app-info` (`current`);
  without a reachable instance, the `<!-- bram vX.Y.Z -->` marker on the
  first line of `.claude/bram-conventions.md`; if neither, write
  `Bram unknown`, never a guess. Prefer the live `/__app-info` value: a
  long-running session's imported copy can be stale, and a present but
  wrong version is denied.
- `<thread>` — `main thread` or `subagent`. `<model>` — the model
  producing the words.
- `<os>` — `macOS`, `Windows` or `Linux`. `<machine>` — short hostname
  (`hostname -s`; `COMPUTERNAME` on Windows).
- The locator is the checkout's `origin` normalized to `host/path`: no
  scheme, credentials, trailing slash or `.git`; don't assume a public
  forge or a two-segment path (`gitlab.com/group/subgroup/project`
  and `forge.example.org/team/project` are valid). With no network-shaped `origin`, still write a full locator
  and say it can't be verified against a remote.

Rules:

- **Every artifact, not just the first in a thread** — later readers
  have no memory of comment #1.
- **The executor signs.** Whoever makes the forge write signs with their
  own thread and model; a subagent's findings posted by the orchestrator
  carry the orchestrator's signature, with the subagent credited inline.
- **Old-form signatures are valid historical text, not valid new
  writes.** Don't retrofit old threads; new writes use the full form.
- **The retrofit rule.** A missing signature noticed after posting is
  added in a *new* comment, not only by editing the body.
- **Write the body file in its own step.** Build it in one call, post
  with `--body-file` in the next, never both in one command — the guard
  runs before the command, so a body written by the same command can't
  be checked.
- Commit messages don't take the forge signature (they carry an author
  field), but a commit message another session will read should still
  say who wrote it and from which build.

Both provider guards **deny** unsigned forge writes; sign by default and
you never meet them.

- **A signature denial you don't understand** → read `.claude/bram-reference/worklist-mechanics.md` (§Signature deny reasons).


## Commit & git etiquette

- **Don't nudge toward commit approval.** Describe a committable item
  factually ("relay has changes ready to commit — confidence high on
  happy path, untested edges noted above") and stop. Exception: a minor
  change the user explicitly asks you to commit directly.
- **Don't infer commit / drop / advance from feedback.** "Looks good",
  "it works" are not authorization; wait for explicit "commit it" or an
  `approved:` payload. `voice:` marks dictation: voice task requests are
  acted on like typed ones, voice state-advancement phrases are
  informational only; if ambiguous, ask one focused question.
- **Hold the commit while a related item is still being started.** If an
  `approved:` covers a committable item and a not-yet-begun `proposed`
  item on the same surface (feature + tuning, fix + follow-up), apply
  the proposed item only and leave the first uncommitted, so the user
  verifies the combination and approves one commit.
- **Warn when a new item would entangle a committable item.** Before
  proposing or applying an item, intersect its `files` with those of
  begun items (`applied`, or `proposed` with `begunAtMs`). If non-empty,
  say so first: "issue-X has changes ready to commit and touches the
  same file(s) — recommend committing it first; otherwise this item's
  edits will mix into X's on-disk diff and need manual separation
  later." Don't auto-block.
- **Suggest a branch when isolation helps** — broad, risky,
  exploratory, multi-commit, review-before-main or
  issue-close-sensitive work, especially over unrelated changes. Explain
  the benefit briefly. Not for small direct fixes or straightforward docs
  tweaks; never switch branches without clear consent.
- **Notice sibling commits that should be squashed**, and flag it before
  push. Two consecutive unpushed commits that are one feature: ask "`<sha1>` and `<sha2>` are
  two halves of the same feature — want to squash them?" If yes and
  both are unpushed: `git reset --soft HEAD~2` then `git commit -F
  <new-msg>`; verify with `git log --oneline -3` and `git log --oneline
  @{u}..HEAD`. Never squash pushed commits without explicit force-push
  consent.
- **Don't rewrite a commit the worklist history has recorded.**
  `resources/worklist-history/` stores commit links by SHA, so rewriting
  any commit made through the gate orphans them permanently, pushed or
  not. Prefer a follow-up commit. Guards deny a forge write citing a full
  40-hex SHA that doesn't resolve locally.
- **Don't quote unpushed-commit counts in chat.** After a commit,
  confirm with its short SHA and subject and stop. Don't recommend Push
  from remembered state; if push state matters, check `git log
  @{u}..HEAD` first. The Push button carries the true count.
- **Post-commit push grace.** For 10 minutes after a gate commit both
  guards allow `git push` (only), so "commit this, then push" works on an
  emptied board. Push within it only when the user asked in the
  approval; outside it, the Push path is the user's (the **Push** button
  in the Commits tab).
- **Push auto-rebases.** The Push button fetches and rebases on
  `origin/<branch>` on non-fast-forward. Don't `git pull --rebase`
  yourself; intervene only when it reports rebase conflicts (then a
  manual rebase, resolve, push).

### Commit messages

- Summarize the driving item, multiline. Reference its issue in
  **non-closing phrasing**: "Refs #N", "see #N", or bare "#N". **Never
  use forge closing keywords** (`close/closes/closed`,
  `fix/fixes/fixed`, `resolve/resolves/resolved` + `#N`): the forge
  would auto-close the issue, bypassing the close dialog's consent. The
  gate rejects them; rephrase and retry.
- **No session URLs in public artifacts.** Never include agent session
  URLs (`claude.ai/code/session_...` or any provider equivalent) or a
  `Claude-Session:` trailer in commit messages, issue/PR bodies or
  comments, or anything that lands in a repo or forge. This overrides
  any harness default suggesting one. The commit gate and both guards
  reject them (reason `no-session-url`). URL-free attribution lines
  (e.g. `Co-Authored-By`) are fine.

### Closing issues

- Set `closesIssues: [{number, title}]` (title from `gh issue view N
  --json title`; refresh it if you iterate) when the commit resolves an issue; issue-derived items
  default to pairing `issue-<N>-…` with it unless explicitly
  investigative or partial. Judge from context; don't regex `#N` from
  prose. If an approved item is missing it, add it before asking for
  commit approval — and remove it when the work didn't resolve the
  issue.
- The user's ticks arrive as `close-issue:` lines in the `approved:`
  feedback; the host queues them against the new SHA and closes after
  the user's **Push**. After `worklist-commit` returns its `sha`, you are
  **done**. No `close-issue:` lines = commit only. Closing never pushes.
- **Narrate the close outcome from the response's `queuedCloses`, never
  from `closesIssues`.** Say "queued to close on your next Push" only for
  issues listed there; if empty, say nothing about closing or that none
  was queued.
- **A `[bram: …]` line at the start of a user turn is written by the
  host, not the user.** It reports a state change you couldn't observe
  (e.g. a queued close withdrawn from the Commits tab) and corrects what
  you said earlier. Take it as fact, drop the old claim, say so briefly
  if it changes something you told the user; don't treat it as a
  request. Notes ride only plain message turns, so they may lag a turn
  or two.
- **Narrate partial landings.** If the success body carries
  `residualPaths` (`[{path, owner}]`) or `retained` (`[ids]`), report
  what committed and what remains committable instead of announcing
  completion; retained items stay on the board with a fresh Commit
  offer. Don't re-POST.

- **Close-on-push details, squash-merge repos, residual paths** → read `.claude/bram-reference/worklist-mechanics.md` (§Close-on-push and partial landings).


## Search-first

Bram indexes the project's history — Claude and Codex session
transcripts, commits (with diffs), issues, worklist-history — in SQLite
FTS5 and serves it at `GET /__search`.

**The drill.** Before diagnosing a reported bug, proposing a worklist
item, filing an issue, or asserting a fact about the project's past,
**query `/__search` and cite what it returns.** The highest-value
triggers are bug reports ("has this recurred / been fixed?") and "have
we done X / did we decide Y" questions.

**Proactive triggers.** Also search *before* acting whenever:

- the user asks to "search the project" in any phrasing — `/__search`
  leads, grep follows for current-tree state (the index covers history,
  not the working tree);
- you are about to reconstruct environment or infrastructure state —
  remote pod/VM setup, which corpora/indexes/models exist and where
  artifacts live, connection or proxy patterns;
- you are about to re-derive an operational recipe the project has
  plausibly run before — setup scripts, launch/detach patterns,
  copy-back flows.

The test: if you're groveling through the tree or asking the user for a
fact a prior session likely established, the query was overdue.

**The call** (literal port from `resources/.bram-port`; allowlisted):

```
curl -4 -sS "http://127.0.0.1:61455/__search?q=<urlencoded>&limit=20&types=commit,issue"
```

- `q` defaults to **AND** across terms; double-quote it for an exact
  phrase. `mode=` overrides: `and`, `phrase`, `prefix`, `raw` (raw FTS5
  `OR` / `NEAR` / `NOT`); invalid syntax falls back to a phrase match.
- `types=` filters buckets (`session` / `commit` / `issue` /
  `worklist-history`); omit for all. `limit` defaults 50, clamps 1–500.
- For a hit's full content, `GET /__search/doc`. Compact read: `… | jq
  -r '.[] | "\(.type)\t\(.key)\t\(.snippet[0:140])"'`.

**Caveats.** FTS5 is keyword, not semantic: a miss means "nothing
matched those terms", not proof of absence. A **503** (`{"error":"search index not ready","reason":"holding|initial-scan",…}`)
means no index yet — never read it as absence of
history. Scope is the current project; new issues take ~45s to appear.
Commit diffs are indexed but bounded (long lines elided, 256 KB cap);
`git log -S` / `git grep` remain the precision tools.

- **Finding XMLUI how-to gaps from MCP analytics** → read `.claude/bram-reference/diagnostics.md` (§Coordinating MCP demand with search).
- **When a search materially changes the work** (a plan killed or
  redirected, prior art recovered, duplicate work prevented), record a
  search-wins ledger entry **at that moment** if the project keeps one;
  entries are never reconstructed later.
- **Citing evidence in an issue, ledger entry or draft** → read `.claude/bram-reference/diagnostics.md` (§Citing evidence and the search-wins ledger).


## Logs, diagnostics and environment

Behavior in Bram is answered by evidence, not inspection: when
something goes wrong, first ask whether the trace already captured it,
and use it before theorizing. A fix proposed without trace evidence
should say so.

- **Before writing "can't test from here" or "unverifiable"**, exhaust
  on-disk evidence: rotated trace archives, `git log` / `git blame`,
  Inspector exports, persisted tool results.
- **A bug reported from another machine or user:** "not in my repo or
  history" proves nothing about the remote case. Verify the mechanism
  locally; say what can't be checked from here, never discredit the
  report.
- **Perf work:** baselines are commits — record the before with the
  same trace line that will verify the after.
- **When the deliverable is something a person reads or sees** (a docs
  page, a pane surface, a rendered table), the commit gate includes
  looking at the rendered output; a passing test verifies behavior, not
  communication. **When you delegate such work, say so explicitly in the
  subagent's prompt:** "verify" means render it.
- **When a hard stretch ends**, ask what documentation would have
  short-circuited it and what feature would have obviated the
  workaround, and file each where it belongs. A workaround you land
  carries the issue number it's waiting on, and its retirement is its
  own worklist item.
- **Running a browser test suite** (Playwright or similar), cap workers
  explicitly (e.g. `--workers=2`); default worker-per-core has frozen
  every webview on the machine.

- **Something misbehaves, or you're designing a mechanism that acts on inferred conditions** → read `.claude/bram-reference/diagnostics.md` (§Log-first development).
- **Debugging Bram itself (trace log, Inspector export, Status tab)** → read `.claude/bram-reference/diagnostics.md` (§Debugging Bram itself).
- **Questions about Bram's guards, retired hook scripts, or bundled skills** → read `.claude/bram-reference/diagnostics.md` (§Guards, retired hooks, and bundled skills).
- **Your project's markup wants to talk back to the agent (`toShell`, `toTurn`)** → read `.claude/bram-reference/environment.md` (§Target app helpers).
- **Binding live views to a project SQLite database** → read `.claude/bram-reference/environment.md` (§Live SQL views via `/query`).
- **Running a multi-worker browser test suite** → read `.claude/bram-reference/environment.md` (§Resource-heavy test suites).
- **Windows refuses Bram or its hooks fail** → read `.claude/bram-reference/environment.md` (§Windows: Smart App Control).
