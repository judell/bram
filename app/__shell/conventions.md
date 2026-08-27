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
> project via a `@`-import in `CLAUDE.md`, and Setup seeds it into
> every managed project as `.claude/bram-conventions.md`. **Don't save
> project-related memories** — preferring the worklist, helper APIs,
> release quirks, conventions you discover, etc. Per-user memory is
> private to one agent on one machine; shared files reach everyone
> running Bram (Claude and Codex alike). Route by audience: would the
> knowledge change an agent's behavior in a project that is NOT the
> Bram source repo? Yes → here. No (it only matters when editing
> Bram's own source) → `docs/developing-bram.md`, which loads only in
> the source repo. XMLUI-framework-generic findings → upstream xmlui
> docs, reachable through the xmlui-mcp server. Memory stays reserved
> for things that genuinely can't live in any repo (cross-project
> user preferences, provider-specific tool quirks, etc.).

Bram's own UI is XMLUI. When developing Bram, expect the
XMLUI MCP server to be available, read the xmlui_rules,
and follow them. The same holds if the app under development
is XMLUI.

### Guard source of truth

Provider hook adapters live under `app/provider-hooks/` with
provider-prefixed names (#217). When editing Bram's Claude worklist
guard in this source repo, `app/provider-hooks/claude-worklist-guard.py`
is canonical. The runtime copy at
`.claude/hooks/claude-worklist-guard.py` is an installed artifact that
Setup and `src-tauri/build.rs` refresh from that canonical source. Do
not make functional edits in the installed copy; it will either be
reported as setup drift or overwritten by the next sync. The Claude
permission-menu hook follows the same split:
`app/provider-hooks/claude-permission-menu-hook.py` canonical,
`.claude/hooks/claude-permission-menu-hook.py` installed.

In **this source repo** the installed copies are tracked, and `build.rs`
regenerates them on every build — so a worklist item that edits a
canonical hook must list its installed twin in `files` as well
(`app/provider-hooks/claude-worklist-guard.py` →
`.claude/hooks/claude-worklist-guard.py`). Omit it and the commit lands
canonical-only; the next build then regenerates the installed copy into
a dirty working tree that belongs to no item, and a checkout of that
commit carries a guard whose installed and canonical forms disagree.
The `.gitignore` entries near `.claude/hooks/` cover only the retired
generic names, not the provider-prefixed installed copies.

The Codex guard has the same source/installed split:
`app/provider-hooks/codex-worklist-guard.py` is canonical, while
`~/.bram/codex-worklist-guard.py` is the installed runtime copy (the
Codex permission-menu hook mirrors it:
`app/provider-hooks/codex-permission-menu-hook.py` →
`~/.bram/codex-permission-menu-hook.py`).

`app/shell/` holds shell-launch support only (`claude-code-shellrc`,
`claude-code-profile.ps1` configure the shell that launches the agent
CLI; `codex-startup-instructions.md` is startup text injected into Codex
sessions) — no hook adapters. Legacy generic installed names
(`.claude/hooks/worklist-guard.py`, `.claude/hooks/permission-menu-hook.py`)
survive transiently after upgrade so live sessions' hook snapshots keep
working; Bram startup prunes them once **no settings source** references
them — project `settings.json`, project `settings.local.json`, or the
user-global `~/.claude/settings.json` (Claude Code merges hook config
from all three; a stale reference in any of them keeps the shim
load-bearing — #227). Bram never rewrites the global or local file: a
stale reference there holds the prune back and surfaces as a named
drift warning in the Setup result instead.

**Bram-bundled skills** follow the same canonical/installed split:
`app/skills/<name>/SKILL.md` is canonical; Setup seeds it into each
managed project's `.claude/skills/<name>/SKILL.md`, and `build.rs`
syncs the source repo's own installed copy (`loose-ends` is the first
member; new Bram-blessed skills drop into `app/skills/` and ride the
same path). Ownership is marker-based: Setup refreshes only files
carrying the `<!-- bram-managed -->` marker line — a same-named skill
without it is user-owned, never clobbered, and reported as skipped in
the Setup result. Skill staleness is deliberately NOT part of the
needs-setup banner; refresh is best-effort at Setup time. This is a
Claude-only surface (Codex has no skills concept). Do not make
functional edits in an installed `.claude/skills/` copy of a bundled
skill; edit `app/skills/<name>/SKILL.md`.

### XMLUI lookup order

When you are figuring out how to do a thing in XMLUI, ask the XMLUI
MCP server for how-to documents first (`xmlui_search_howto`). The
how-to corpus usually carries the complete pattern and tradeoffs.
After that, use `xmlui_component_docs` for exact component props,
events, and exposed methods. Use examples as a fallback or to confirm
local style, not as the first source of truth.

When a non-obvious markup choice depends on documentation, cite the
relevant how-to or component URL in the response.


## Code organization (developing Bram only)

The helpers.js / Globals.xs / window code-organization discipline, the
delegator and `__bram*` naming rules, the xs-engine failure modes, and
the post-edit verification ritual moved to `docs/developing-bram.md`
in the Bram source repo — they apply only when editing Bram's own
source, so they are not seeded into managed projects.


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

- **Explicit user opt-out in this turn.** The user ends their message with
 the single phrase "just do it" (case-insensitive). The opt-out must be in
 the same turn as the change request — don't carry it forward across turns
 or infer it from past patterns. Retired phrases ("skip the worklist",
 "commit this directly", "inline the fix", "no worklist for this", "don't
 bother with the worklist") no longer opt out — narrowed
 (opt-out-single-phrase-and-audit) from a seven-regex list to one explicit
 phrase, to cut the risk of an accidental match in ordinary prose. Both
 Claude and Codex honor the same phrase, but along different paths:
 Claude's guard matches `_OPT_OUT_PATTERNS` against `transcript_path` on every
 `PreToolUse` and allows inline; for Codex, Bram's host-side `toTurn` path
 matches the same phrase and writes a one-turn `direct-edit` record
 (`kind:"direct-edit"`, `paths:["*"]`, 1h TTL) to
 `resources/.worklist-authorization.json`, which the single Codex
 `PreToolUse` hook reads via `fresh_bypass()`. The phrase itself is
 identical, so the user-facing contract is the same regardless of agent.
 Codex prose opt-outs record a `direct-edit` line in the audit ledger via
 that same host `toTurn` path; Claude prose opt-outs have no equivalent
 host chokepoint on their allow path, so the Claude guard instead POSTs a
 best-effort breadcrumb to `POST /__audit/direct-edit` right before
 allowing — one `direct-edit` audit-ledger record per opted-out turn,
 deduped host-side so the guard's per-tool-call firing doesn't produce
 duplicates.

- **`skip-worklist:` structured prefix on this turn.** The user's
  turn begins literally with `skip-worklist: ` followed by the
  request text. Same family as `approved:` / `drop:` / `iterate:`,
  but for authorizing a direct edit rather than a lifecycle
  transition. The user-facing affordance is the **skip worklist**
  button beside the message input in Bram's footer — it prepends
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

- **Issue-only forge work with no repo diff.** If the user asks you to
  create, edit, comment on, close, or reopen a forge issue, and the
  task will not modify tracked files in the repo and will not produce a
  commit, skip the worklist and do it directly — using the project's
  detected forge CLI (`gh` on GitHub, `glab` on GitLab; see *Updating
  forge issues via gh / glab*). If the issue request is paired with
  repo changes, the repo changes still go through the worklist.

### What worklist items represent (and when to drop)

**Worklist items represent repository changes.** A `proposed` item
names a `file` (or `files`) plus `before` / `after` prose in
`resources/worklist-drafts/<id>.md`, describing what would change
on disk. An `applied` item has those changes on disk
waiting for the user to approve a commit — and so, now, can a
`proposed` item that has been begun (approved and started, but not
advanced): once its changes are on disk and exclusive to it, the pane
offers Commit on it directly, `status` unchanged. See *Field notes*
below and *Transports → Apply-and-commit gate*. Items exist to give
the user explicit veto power over what lands in their repo.

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
  as `applied` produces a row with nothing to commit (the legacy tab's
  TO COMMIT badge on an empty diff), which is exactly the user-visible
  failure mode of #88.
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

**Drop removes the item, not the bytes — and orphaned changes are
misattributed, not merely unattributed.** Every surface in the pane reasons
about changed files *through items*: the overlap index walks `item.files`,
`changeSummary` is computed per item, and exclusivity asks whether another
**begun** item claims the path. A changed file that no item declares is not
shown as belonging to nobody; it is credited to whichever begun item happens
to declare it, and is committable under that item's id and message with
nothing recording the mistake afterwards.

Two live cases, hours apart on 2026-08-24:

- `notice-banner-component` was green-lit two days earlier and produced
  nothing — its component file was never created and five of its seven declared
  files were untouched. The row nevertheless read
  `✓ Will commit +10 −35 in 1 of 7 planned` and offered **Commit**, because it
  was the only *begun* claimant of a file that edits made outside any
  authorized item had landed in. Exclusivity passed honestly.
- `issue-278-overlap-explorer` was dropped after its React Flow view lost to
  the table it was meant to improve on. The drop left 326 changed lines across
  three files plus 230 KB of vendored extension belonging to nothing.

So, when dropping an item that has **begun**:

- Say what remains on disk before the drop completes, and propose one of:
  revert it, re-home it under another item's `files`, or leave it
  deliberately — then say which was chosen. Both cases above had a defensible
  resolution and they were different ones.
- Prefer `git stash push -m "<item id> (dropped): <what>"` over discarding.
  The judgement that work is worthless is usually made minutes after making
  it; a stash costs nothing and keeps it recoverable.

This is also the strongest argument for not editing outside an authorized
item: unauthorized edits do not merely skip an audit trail, they are credited
to someone else.

### Placeholder items (droppable reminders)

One shape of item carries no diff yet and is still legitimate: a
**placeholder** for an action that is already decided but gated on an
external condition — an upstream merge, a release being cut, another
agent's verdict — that will resolve after this session ends. Chat
context dies with the session; the placeholder is what carries the
reminder across. Live precedents:
`file-upstream-null-expr-crash-after-3763` and
`revendor-after-xmlui-release` (bram), `watch-for-3764-merge` (xmlui).

- **Shape.** `Before` states the awaited condition plus enough
  self-contained context that no conversation history is needed to act
  on it. `After` states the action Approve green-lights, and says
  explicitly what condition would make Drop the right verdict. `files`
  lists what the eventual action will touch (empty for issue-only
  actions).
- **Lifecycle.** Approve = the condition is met; do the action, and the
  item behaves like any other approved item from there. Drop = the
  condition was mooted or the action superseded — an expected, honorable
  ending for this kind, not a failure.
- **Boundary.** This does not reopen the door to investigation items. A
  placeholder records a *decided future action*; an open question is
  still chat's job.

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
  is the older single-file form. Once an item is committable, its
  inline diff concatenates all listed files.
- `closesIssues` declares which GitHub issues the commit resolves
  (drives the close-on-commit dialog — see *Commit & git etiquette*).
  Set conservatively: only when the commit truly closes the issue, not
  when it merely cross-references (`see #N`, `related to #N`, partial
  multi-step work). Omit or use `[]` to skip the dialog.
- `begunAtMs` is **host-written, never authored by an agent**. The host
  stamps it when it first records an `approved` authorization covering
  the item, and never clears or moves it while the item lives — a
  re-approval (iterate, second gate) leaves the original stamp. It is
  the durable answer to "has work on this item begun?", which the
  Worklist strip and the overlap banner both need. The other two signals
  for that question — the authorization record and the inflight claim —
  are single-slot and displaceable, so on their own they let an item
  with real work on disk report "No changes yet" as soon as another item
  was approved. Don't set it by hand; don't rely on its absence meaning
  anything except "never approved".
- `status` tracks the item's stage, but as of 0.5.1 it no longer gates
  committability by itself (see *Transports → Apply-and-commit gate*):
  - `"proposed"` (default if omitted): user is approving you to make
    the change. The Worklist row's strip reads "No changes yet" until
    work begins; the legacy tab badges this **TO APPLY**. The current
    pane instead reasons about three states keyed on what the user can
    do — not started, nothing to commit, has changes you can commit —
    and a `proposed` item can reach that third state on its own, once
    begun with exclusive changes, without ever becoming `applied`.
  - `"applied"`: change is on disk, user is approving `git commit`
    (legacy badge **TO COMMIT**). Push decided separately via the Push
    button. `applied` still means committable; it is just no longer
    the only status that does.

Default to the two-stage flow: approved `proposed` → advance to
`applied` → user approves a separate commit → prune. Skip the
`applied` stage only when the user says "apply and commit" up front.
Drops prune directly with no `applied` stage. Don't pre-mark new
items `"applied"` unless the change is genuinely already on disk. This
default governs how an agent *authors and completes* an item's status;
it is independent of the pane's own committability judgment, which the
user exercises through the Commit button regardless of which status
the item currently carries.

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

2. **User triages** — ticks the rows to act on, optionally types one
   message in the box beside the buttons, and clicks one of them. The
   message fans out to every selected item, so a plural payload's items
   may carry identical feedback text; treat each item's feedback on its
   own terms, but answer identical fanned-out feedback **once** in chat,
   never repeated per item — the per-item copies belong to the items'
   histories, not the transcript. All action buttons emit the same payload
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
     - **`proposed`, not yet begun** (no `begunAtMs`, nothing on disk
       yet): revise the draft file's `before` / `after` prose; update
       `files` only if scope shifts. Item stays `proposed`, no
       project file edits.
     - **`proposed` but already begun** (real edits already on disk,
       even though `status` is still `proposed` — see *Field notes*),
       **or `applied`:** edit on-disk files per the feedback. Update
       the draft only if scope materially expanded. Item stays at its
       current status either way; iterate never advances or commits
       on its own.

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
     An item may instead carry inline `{id, feedback}` — that is the
     **degradation fallback only**, taken per item when its draft write
     failed, so an iterate still lands rather than blocking the click
     (its text has ridden the paste channel and is subject to the
     collapse above). For a while after the worklist2 rewrite the gate's
     Iterate emitted inline unconditionally while the Queue tab drafted —
     an accidental fork, repaired under #285; both emitters draft first
     now, and both opt-out matchers (guard-side and host-side) read the
     drafts (#171, #284).
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

**The third outcome: approved, investigated, nothing to apply.** An approve
gate has three endings, not two. Besides "work applied" and "work applied and
committed", there is the case where the first step of the approved item
falsifies its own premise — the investigation shows the change is unnecessary,
or the hypothesis it rested on is wrong. This is a normal ending, not a
failure, and it has its own handling:

- **Do not `mutate op:"advance"`.** Advancing asserts the work is on disk. It
  is not, and marking the item `applied` produces a row with nothing to
  commit.
- **Retire the claim explicitly.** The host set the inflight sentinel at
  approval time and nothing on this path clears it, so the spinner runs and —
  because row selection is locked while a claim is live — the user cannot even
  click Drop to resolve it. Call
  `POST /__worklist/end` with `{"ids": [...]}` naming the approved ids.
  Both the method and the body are required; a `GET` returns `POST only` and
  an empty body returns `{"error":"ids[] required"}`.
- **Report the finding in chat and recommend Drop**, exactly as the
  *investigation reveals nothing to commit* guidance above prescribes for the
  `advance` case. This extends that rule to cover the claim.

Live case, 2026-08-24: `issue-275-transcript-row-remount-churn` was approved to
apply, its first step disproved the item's own hypothesis, no files changed,
and no lifecycle call was correct to make. The claim stayed live and locked the
row until it was unwound by hand.

The same rule covers a subtler ending: **a turn that ends by asking the user
a decision must not leave a claim live.** Holding the claim "while you
decide" locks the row, so the two buttons that ARE the answer (Approve /
Drop) are unavailable, the spinner implies work that is not happening, and
the only remaining channel — a verbal reply in chat — is advertised nowhere
on the surface. Call `POST /__worklist/end` before ending the turn; the
row unlocks and the decision gets ordinary affordances. (Live case,
2026-08-26: `issue-262-cross-project-direct-edit-auth`'s apply surfaced a
premise conflict with the issue's recorded disposition; the orchestrator
held the claim across the question and the user was left asking "nobody is
working but we are still spinning" with no visible way to respond.)

**Apply-and-commit gate: skip `advance` — edit if needed, then
`worklist-commit`.** `gate: "apply-and-commit"` is no longer only the
pre-approval one-click **Approve & commit** button's payload. As of
0.5.1 the pane also puts a plain **Commit** on a `proposed` item that
has already begun and whose changes are EXCLUSIVE — every changed path
free of any other begun item's claim (`window.__bramSelectionAllCommittable`
in `helpers.js`). Clicking either control submits the identical
`gate: "apply-and-commit"` shape, and the host side is unchanged between
them — the widening is entirely in when the pane *offers* the button, not
in what the host accepts. So the agent handles both triggers the same
way: whatever produced the payload, do **not** `mutate op:"advance"`
first. Make any remaining proposed file edits, then call
`worklist-commit { ids, message }` directly — the host commits the
still-`proposed` item's files (authorized by the `commitToo` auth record
the click wrote, the `allow_proposed` path) and prunes, exactly as the
commit gate does. `closesIssues` / close-on-push behave identically to a
normal commit. The host sets the sentinel at approval time and
`worklist-commit` clears it. Both triggers — the one-click Approve &
commit button and the widened plain-Commit offer — are always available.
(A `worklist.oneClickApproveCommit` setting once gated them; it was
retired in the 0.5.3 run after its config-off path produced a dead-end
row — the offer was only ever visibility, never authorization, so
removing the flag removed a bug class and no capability.)

Exclusivity is what makes the widened offer safe, and the host does
**not** enforce it — only the pane does. `worklist_commit_files_for_ids`
(`lib.rs`) stages whatever files the approved ids declare, whoever
changed them; it has no notion of which hunk belongs to which item.
So an agent driving `worklist-commit` directly for a still-`proposed`
item — the Codex intent-file transport, or a hand-built curl, neither
of which goes through the pane's own gate check — must apply the same
exclusivity rule the pane applies before it would have offered Commit:
every one of that item's changed paths must be free of any *other*
begun item's claim (`applied`, or `proposed` with `begunAtMs` set).
An item whose changed paths are entirely shared with another begun
item is not committable that way — committing it would take the
neighbour's uncommitted work and land it under this item's id and
message, with no record afterward of which item wrote which hunk
(`worklist.json` carries no per-hunk authorship). Treat that case like
any other non-committable item: report it in chat and wait, per *Warn
when a new item would entangle a committable item* below.

**Commit gate: call `worklist-commit`.** This is the traditional
two-stage path: every id in the request is already `applied`, so
`gate` is plain `approved`, not `apply-and-commit`. Send one request
with `{ ids, message }` when the selected items land together. For
`split-shared-files`, isolate one item's hunks on disk, call
`worklist-commit` for that id, restore the next item's isolated hunks,
and repeat. Each successful subset call retires only those ids from the
authorization and inflight claim; the remaining ids and embedded item
bodies stay live for the later commits, and the final call consumes the
record. The host verifies approved auth, requires every requested id to
be `applied` (relaxed to also accept `proposed` only when `commitToo` is
set — see *Apply-and-commit gate* above), stages only those items' files,
refuses unrelated staged files, commits, and prunes the requested
items. **Issue close is
automatic — the agent does nothing.** When the approved feedback
carries `close-issue:` selections (from the close-on-commit
dialog), the host records only the requested ids' selections at commit
time bound to that commit's SHA, then
closes each issue automatically after the user's next explicit **Push** (only
once its commit reaches the default branch; in a squash-merge repo, a merged
PR containing the commit completes the close instead, and a queued close
whose issue is already closed by other means retires — #282). There is no
agent-reachable close
route — `/__issue/close` was removed — so there is nothing for the agent to
call and no close step to instruct the user to run; closing follows Push with
no further action (close-on-push-automatic, security H5). Closing never pushes.

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

The POST routes (`worklist-mutate`, `worklist-commit`) have their
own allowlist entries, but the match is narrow — keep the call in
this exact shape or it will prompt:

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

   `route` is one of `worklist-resolve`, `worklist-mutate`, or
   `worklist-commit`. `body` matches the HTTP route:
   - `worklist-resolve` — omit, or `{ "ids": [...] }` to filter.
   - `worklist-mutate` — `{ "op": "advance", "ids": [...], "status": "applied" }`
     or `{ "op": "prune", "ids": [...] }`.
   - `worklist-commit` — `{ "ids": [...], "message": "..." }`.

   There is no `issue-close` route: closing is fully automatic on the
   user's next Push (close-on-push-automatic, security H5). The agent
   never writes a close intent.

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
both apply.

Item ids are **immutable**. Renaming is not supported: removing an
existing id from `worklist.json` reads as an unauthorized prune and the
host reverts the write, while any draft file you already moved stays
moved — leaving the row with `_draftMissing` and no prose until someone
restores the filename by hand. The rollback is silent; your write
returns success, so nothing tells you it failed. If an id turns out to
under-name its item, keep the id and say so in the draft. Tracked in
judell/bram#276.

#### Keep the `files` list current as understanding evolves

An item's `files` is the agent's *prediction* of what the change will
touch, and the pane's change-activity count uses it as the denominator
(`files: 2 of 3 planned`). When a listed file proves unneeded mid-work,
**update the item's `files` to match** — an ordinary `worklist.json`
edit (version-bumped, guard-allowed) — so the count converges to
`N of N planned` before the commit gate. The draft prose keeps the
original prediction as the audit trail. The clean expression of "the
plan was wrong, not the work" is a corrected plan, not a caveat on the
count; a committed item whose count still reads `2 of 3` invites the
misreading that work went missing (live case: 2026-08-20,
`rethink-activity-indicators`, where an unneeded `helpers.js` guess
made a complete commit read as a partial one).

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

### Narrate as you reach for tools

Before a tool call (or a batch of them), say in a sentence what you
are about to do and why — "rebuilding the engine to pick up the
tooltip fix", "rerunning the one failing spec before the full suite".
When a result changes your plan, say so before acting on the new plan.
Long-running work (builds, test suites, background tasks) gets a
status line when it starts and when it lands, not silence until a
final summary.

This does not conflict with *Match terseness to the question* — the
audit trail in worklist drafts and commit messages still carries the
full story; narration is the live, one-line version so the user can
follow (and redirect) work in flight.

### Cross-project pivots

When the user pivots the conversation to work on a different project
("let's look at ~/other-app"), flag the boundary before proceeding and
offer the choice: a quick read-only look from here, or a handoff to that
project's own session. Sustained investigation, issue filing, and
follow-up work belong in the target project's session, because project
memory — session transcripts, the `/__search` index, worklist history —
is scoped by working directory and records work where it *runs*, not
where it belongs: the home project's index fills with foreign content
while the target project keeps no record the work happened. If the user
chooses to proceed from the current session anyway, make any artifact
left in the target project (issue, doc, commit message) self-contained —
carry the evidence inline rather than pointing at the wrong project's
transcript.

That subsection is about pivoting *this* session's attention. When two
sessions both stay put on opposite sides of a boundary and coordinate,
see *Working across project boundaries* below.


## Signing agent-authored forge artifacts

**Every agent-authored forge artifact opens with a signature** — issue
bodies, issue comments, PR descriptions, PR comments, reviews. Every
repo, boundary or not.

The reason is not "which project is this from" but **who is speaking**.
Agents post through the human's account, so an unsigned agent comment is
indistinguishable from one the human wrote, and that is equally true in
the project you are sitting in. An earlier version of this convention
scoped the requirement to artifacts that cross a project boundary; the
result was judell/bram#253, where unsigned agent comments sat beside
Jon's own replies while a cross-boundary issue filed in the same period
was correctly signed. The record was inconsistent along an axis no
reader cares about. The cross-boundary case is a subset of the problem,
and it was mistaken for the whole of it.

**The form.** One canonical opener, three slots, all load-bearing —
*whose* agent, *which* agent, *which* project:

    <owner>'s <Agent> speaking from the <Project> project:

This project's two instances:

    Jon's Claude speaking from the Bram project:
    Jon's Codex speaking from the XMLUI project:

A form that names only the project ("from the xmlui side") leaves "who
is speaking" unanswered, which is the half that matters when two agents
work the same thread. Across a boundary the third slot also answers
*which side the evidence comes from*.

**The scope.** **Every artifact, not just the first in a thread.** The
observed failure is decay: the opening comment is signed, and by the
fourth it has worn down to "Short addendum —" because by then it feels
redundant. It isn't. The reader the signature exists for — someone
opening the thread months later, or a third agent joining mid-way — has
no memory of comment #1.

Commit messages are deliberately excluded. Conventions ask for a
signature on any commit message another session will read, and that
stays as-is: commit subjects are short, `git log` is dense, and commits
already carry an author field that the comment box does not.

**The retrofit rule.** If you notice a missing signature after posting,
add it in a *new* comment rather than only editing the body. A body edit
fixes the page; it does not reach anyone who already got the
notification, and a silently-corrected record reads as a discipline that
was not there.

**Enforcement.** Both provider guards check this on `PreToolUse` and
**deny** an unsigned forge write rather than warning. You should never
meet that check: sign by default and it never fires. A denial means you
forgot, and costs one turn to fix. The guards fail open by design — a
body the parser cannot read (`--body-file -`, an unreadable path, no
body flag) is allowed through, so the check never blocks work it cannot
judge.

**Write the body file in its own step.** Build the body in one call,
post it in the next — never both in a single command. `PreToolUse` runs
*before* the command executes, so a body written by that same command
does not exist yet when the guard looks for it, and the check fails open
against the very artifact it exists to verify. The `--body-file`
indirection itself is still right (it keeps apostrophes and quotes out of
the allowlisted command line); only the ordering matters.

Two things follow from getting it right: the signature is genuinely
verified rather than nominally required, and an issue-only post is
allowed on its own merits rather than depending on whatever worklist
items happen to be in flight — a heredoc redirect in the same command is
another write pattern, which correctly disqualifies the issue-only
exemption. The trace names which happened:

- `crossboundary-unparsed:body-file-unreadable` — the body was not
  readable; the check passed without verifying anything.
- `crossboundary-signed:body-file` — the body was read and the signature
  confirmed.


## Working across project boundaries

Some of the best work happens between two sessions that each hold
evidence the other cannot reach, coordinating through issue threads.
Three shapes recur, and the practices are the same in all three — only
the asymmetry differs:

- **Downstream ↔ upstream.** Your project consumes a library; a bug or
  gap here is a change there.
- **Machine ↔ machine.** The same repo running on two platforms, where
  a failure reproduces on only one of them.
- **Agent ↔ agent.** The same repo, two agents, each able to reach
  things the other can't.

### Name the boundary

Say which side of the boundary you are on, and scope every claim to it.
The *signature* that carries this is not boundary-scoped — it is required
on every agent-authored forge artifact, in every repo. See *Signing
agent-authored forge artifacts* above for the form and the rule.

### Scope claims to what your side can observe

Say "from this side" and mean it. State what you verified and how; name
what you *cannot* check from here rather than letting silence imply
it's fine. *Local absence is not disproof* (in *Log-first development*)
is the special case of this rule for machine-specific artifacts; this
is the general one.

### The thread is the design document

Chat dies with the session; the thread is what the other side reads,
and months later it's the only record of why. So file at mechanism
depth — symptom, the mechanism cited at the *other* side's
`file:line`, blast radius, and the trace or measurement receipts — not
"this seems broken."

One issue, one mechanism. When a second mechanism surfaces mid-thread,
split it into its own issue and say in both places what moved and what
remains.

### Make the ask specific, and report gates by name

Ask for something answerable: a litmus test to run, a build to
validate, a specific gate to clear. When work is gated, enumerate the
gates and report their status per side ("gate 2 is green; from this
side, remaining: ..."), so neither session has to guess what the other
is waiting on.

### Green-light before irreversible or expensive steps

Vendoring a candidate build, merging, restarting something shared —
request the go-ahead across the boundary explicitly, and grant it
explicitly. An assumed green light is how two sessions end up half-way
through incompatible states.

### Reproduce before fixing; correct the record in public

On the other side of a boundary the currency is a reproduction — a
failing test pins the decision so prose doesn't have to. Build it
before proposing a fix, because it frequently contradicts the filing.

When it does, post a **correction comment** carrying the measurements.
Do not quietly edit the original body: the other side may have already
acted on it, and the correction is the most useful thing in the thread.

### Recompute rather than defer

Being upstream, or being the side where the bug reproduces, is not
authority over arithmetic. When the other session's claim conflicts
with yours, re-run the numbers and read the source before conceding —
then concede once, to the evidence, and move on. Deference and
digging in are the same failure.

### Verify the artifact you run, not the source diff

A merged fix, a green CI run, and a source diff are not the build in
your hands. Verify the artifact you actually execute (checksum,
marker string, behavioral probe), and when you report results, label
which of your instruments are authoritative and which are only
corroborating.

### Render what the reader will see

The rule above has a second half for changes whose deliverable is
something a human reads or sees — a docs page, a pane surface, a
rendered table: the artifact-you-run is the **rendered output**, and
the commit gate includes looking at it. A passing spec verifies
behavior, not communication. Receipt (xmlui wave 3, 2026-08-27,
judell/bram#291): five real defects surfaced only when the committed
how-to pages were opened in a docs server — clipped playgrounds, a
bold run that swallowed its lead clause, a demo whose central claim
was invisible because the spec drove the selection itself — every one
invisible to green tests and to reading the markdown. When the work
is delegated, this must be an explicit instruction in the subagent's
prompt, not an assumed judgment: a delegated agent that cannot verify
its own work will report success (wave 1's lesson), and "verify"
for a rendered artifact means render it.

### Close every hard stretch with two questions

This is the engine that turns local pain into shared improvement.
When a struggle ends, ask:

- *What documentation would have short-circuited this?* → name the
  question you couldn't answer and where you looked.
- *What feature would have obviated this workaround?* → carry the
  workaround itself as the evidence.

Then file each one on the other side. If that boundary doesn't take
issues from you, write the ask up locally anyway, fully formed and
evidence-backed, so it exists the day a channel opens.

Any workaround you land carries the issue number it's waiting on, and
its retirement is its own worklist item. A workaround with no filed
issue is a decision to keep the pain.

Where the other side publishes a searchable doc corpus, the
documentation half has a stricter form — validate the gap against the
current corpus before filing, since the fastest way to lose standing
is to report a gap that closed last week. See *Coordinating MCP demand
with search*.

### Carry gated follow-ups, and don't edit across the boundary

Actions gated on the other side (a merge, a release, a verdict) become
**placeholder items** — see *Placeholder items (droppable reminders)*.

Act only in the repo whose session you're in. The thread is the
transport, not a shortcut for reaching across and editing the other
project directly.


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
  and clears the sentinel. If the approved feedback includes `close-issue:`
  lines, the host itself records the pending close bound to the new SHA;
  the agent does nothing further — closing fires automatically on the
  user's next Push. There is no `issue-close` route to call.
- **`approved:` (apply-and-commit gate)** → `worklist-commit` with
  `{ ids, message }` after editing, with **no** `mutate op:"advance"` step.
  Set by either the one-click **Approve & commit** button or, as of
  0.5.1, a plain **Commit** on a `proposed` item that has begun with
  exclusive changes; the host's `commitToo` auth lets `worklist-commit`
  stage and commit the still-`proposed` files, then prune either way.
  See *Transports → Apply-and-commit gate*.
- **`drop:`** → `resolve` → `mutate op:"prune"`. Drops aren't set at
  approval time, so `resolve` is what raises the spinner.
- **`iterate:`** → no agent-side bracket needed. The host detects the
  `iterate:` prefix on the `toTurn` write path and sets the sentinel
  automatically (parallel to how `resolve` sets it for the commit gate and
  drops); the same turn-finished detectors that clear approve/drop
  sentinels clear iterate's too. (The legacy `/__iterate/begin` and
  `/__iterate/end` routes were removed in the #214 delete phase.)

Several of the bullets above say a mutate/commit call "clears the
sentinel" — true in the common one-id case, but see *Incremental claim
and authorization retirement* for what actually happens when the claim
covers more than one id.

### Incremental claim and authorization retirement

A claim can cover more than one id — approving several unentangled
items in one click writes one claim listing all of them. Resolving
just one of those ids is normal now, not refused: `mutate
op:"advance"`, `op:"prune"`, and `worklist-commit` (which delegates a
prune to the same path) each retire exactly the ids they resolved and
rewrite the claim with whatever is left, tracing
`[inflight-sentinel] op=clear-shrink resolved=[...] remaining=[...]`.
The claim file only disappears once the last id resolves. This is what
lets two disjoint items be started together in one click and then
completed — applied, committed, or dropped — in separate turns, each
clearing its own slice of the spinner instead of leaving it stuck until
every id is accounted for in a single call.

The authorization record retires by the same named subset. Until the
last id resolves it keeps `consumedAtMs` empty, removes the completed ids
and their embedded bodies, and traces
`[auth-record] op=consume-shrink resolved=[...] remaining=[...]`. This
is load-bearing for `split-shared-files`: one plural Commit approval can
produce several sequential `worklist-commit` calls without the first
commit consuming authority for the rest. The original `issuedAtMs` and
interrupt flag remain unchanged, so TTL and cancel fail-closed behavior
still cover the entire sequence.

This is distinct from `op=clear-partial`, which is still a refusal: the
blunt clears — turn-end detectors, cancel paths, startup cleanup, the
drop policy validator — cannot name which ids they're resolving, so
they still require full coverage of whatever claim is live, and log
`op=clear-partial` when a request covers only part of it. That shape
signals something colliding (a second claimant overwrote or partly
overlapped the live claim), not ordinary progress — do not read
`clear-partial` as "working as intended" the way `clear-shrink` is.
Only routes that resolve specific, named ids may shrink.

### Failure modes

A stuck spinner is the convention enforcing itself; there is no
arbitrary live-session timeout. Bram does have host-side completion
detectors that can clear a lingering claim without a cooperative agent
tail call: Claude session JSONL `stop_reason:"end_turn"`, Codex session
JSONL `task_complete`, PTY silence, and explicit cancellation paths. Most
commonly:

- **Approved/drop stuck:** `mutate` was never called, or errored
  before the clear — or the turn ended in the third outcome above, where
  no `mutate` was ever *correct* to call. Recovery, in order:
  `POST /__worklist/end` with `{"ids": [...]}` naming the claimed ids (the
  route is not iterate-specific, despite appearing only under *Iterate stuck*
  in earlier revisions of this file); or call `mutate` manually if the work
  really is on disk; or restart Bram (`cleanup_stale_inflight_claim` runs at
  startup), which is the heaviest option and ends any agent session running
  inside it.
- **Iterate stuck:** rare now that the host auto-detects the
  `iterate:` prefix and the turn-finished clearer fires for all
  sentinel kinds. If it does stick, host-side completion detectors
  will clear it on the next normal turn end; `/__worklist/end` remains
  available as an explicit manual unwind. It now returns
  `{"ok":true,"cleared":<bool>,"remaining":[...]}` instead of a bare
  `{"ok":true}` — `cleared:false` with a non-empty `remaining` means
  the call only resolved part of a multi-id claim (see *Incremental
  claim retirement* above), not that the call failed.
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

A committable item — `applied`, or a begun `proposed` item with
exclusive changes — sits indefinitely until an `approved:` payload
covers it. Describe the state factually ("relay has changes ready to
commit — confidence high on happy path, untested edges noted above")
and stop. The user clicks Approve (or Commit) when ready, or doesn't.
The exception is a *minor* change the user explicitly asks you to
commit directly.

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

### Hold the commit while a related item is still being started

When a committable item and a not-yet-begun `proposed` item touch the
same surface (feature + tuning adjustment, fix + follow-up regression
patch), don't process the commit if the user's `approved:` covers
both. Apply the proposed item only; leave the prior one committable
and uncommitted. The user verifies the combined behavior, then
approves a single commit covering both. This avoids intermediate
"kinda-works" commits where a feature is split from its companion
fix — bad for git history and bisect.

### Warn when a new item would entangle a committable item

Whenever you're about to **propose** or **apply** an item whose
`files` overlaps the `files` of an existing committable item —
`applied`, or a `proposed` item that has begun (`begunAtMs` set — see
*Field notes*) — surface that fact in chat *before* writing the
proposal or applying the edits:

> "issue-X has changes ready to commit and touches the same file(s) —
> recommend committing it first; otherwise this item's edits will mix
> into X's on-disk diff and need manual separation later."

Don't auto-block — the user may have a reason to proceed (the two
items are genuinely meant to ship together, X is about to be
dropped, etc.). The warning is so the user can decide *order*
intentionally rather than discovering the entanglement at commit
time. The check is mechanical: intersect the candidate item's
`files` list with the union of `files` across begun items (`applied`
status, or `proposed` with `begunAtMs` set) in
`resources/worklist.json`; non-empty intersection triggers the
warning.

### Delegating worklist items to subagents

Parallelize the *work*; serialize the *gates*.

- Subagents receive file paths and instructions only. They do **not**
  call `/__worklist/resolve`, `/__worklist/mutate`, or
  `worklist-commit`. Every lifecycle call stays in the orchestrator's
  own turn, after the subagents return.
- The reason is structural: the inflight sentinel holds one claim
  (writing a second overwrites the first), and an authorization
  record carries one whole-record consumed flag (the first `mutate`
  consumes it, and the next subagent is told
  `no_active_authorization` for work that was in fact approved).
  Neither surface can represent two concurrent claimants.
- Before delegating in parallel, intersect the `files` lists of the
  candidate items. **Non-empty intersection → do not parallelize
  those two**; run them in sequence.
- Attribution: `worklist.json` has no agent field, so a committable
  item's diff produced by several subagents carries no record of
  which one made which edit. If that matters for a batch, commit the
  items separately.
- **Hook-enforced on Claude, and verified by deliberate violation.** A
  lifecycle call from a delegated subagent is denied at `PreToolUse`
  (`decision=deny reason=subagent-lifecycle-call`), and the call never
  reaches the host. The check keys on the payload's `agent_id`, which is
  populated only for subagent-originated tool calls.

  It has not always worked. It originally tested for a `/subagents/`
  segment in `transcript_path` — a field present on every payload but
  never carrying a subagent path — so it was inert from the day it
  shipped, and no amount of quiet running would have revealed that. It
  was found by deliberately breaking the rule
  (xmlui-org/xmlui-mcp#33): the call reached the host and was stopped
  only by the authorization layer, which happened to refuse an id it did
  not cover. Had the id been covered, it would have succeeded.

  Two lessons worth keeping attached to this rule. Enforcement claims
  need a **fire** behind them rather than an inspection — see
  *Distinguish soak observers from tripwires*. And a check keyed on the
  *shape* of a path supplied by someone else's payload asserts something
  that payload never promised.

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

### Don't rewrite a commit the worklist history has recorded

Being unpushed is not sufficient license to rebase. `resources/worklist-history/`
stores each entry's commit URL **by SHA**, and the History tab renders those
links — so rewriting a recorded commit orphans them permanently. That is a
different failure from an unpushed link, which 404s only until you push and
then heals; an orphaned SHA never resolves again.

So the usual "it's unpushed, amend freely" reasoning does not apply to any
commit that produced a history entry, which is every commit made through the
commit gate. Prefer a follow-up commit. (Found 2026-08-22 while deciding whether
to amend `626e73d`: the rewrite would have broken the very file-link feature
that commit introduced, in Bram's own history.)

Since #277 the tooling holds this line with you rather than against you:
`scripts/bump.sh` preflights the current release window's history entries
and names any whose SHA is no longer an ancestor of HEAD before the
behind-origin error steers you into a rebase; the History tab marks an
orphaned entry ("orphaned by a rebase") instead of rendering a dead forge
link; and both provider guards deny a forge write whose body contains a
full 40-hex SHA that does not resolve locally — which catches fabricated
hashes and rebase-orphaned citations with the same test.

### Don't quote unpushed-commit counts in chat

After a commit lands, confirm with its short SHA and subject and stop.
Don't say "N unpushed commits now" or list unpushed SHAs in prose — the
Commits tab has the exact count and list; any number you'd state is
guesswork.

The same goes for recommending Push: don't advise it from a remembered
state — the user pushes without narrating it, so a session-long tally
of "commits made" says nothing about what's still unpushed (live
pattern, 2026-08-27: repeated "push the stack" advice while
`@{u}..HEAD` was empty). If push state matters to the point being
made, check `git log @{u}..HEAD` first; otherwise say nothing — the
Push button already carries the true count and the queued-close banner
already says what a push will do.

### Commit-then-push: the post-commit grace

`worklist-commit` prunes its items, so an emptied board would deny the very
`git push` the user just asked to follow the commit ("commit this, then push
and raise a PR" — #283, where the denial even advised proposing an item for
a change that no longer exists). Both guards therefore allow a push-shaped
Bash command for **10 minutes after a gate commit**, keyed on the consumed
`approved` authorization already on disk (trace reason
`post-commit-push-grace`). The grace covers only `git push`; it is a window,
not a standing permission. Outside it, the Push path is the user's: the
denial names the **Push** button in the Commits tab. Agent-driven push
within the grace is legitimate exactly when the user asked for it in the
approval; unprompted, prefer reporting the committable state and stopping,
per *Don't nudge toward commit approval* above.

### Push button auto-rebases on non-fast-forward

The Commits-tab Push button does `git push`; if rejected as
non-fast-forward, it fetches `origin` and rebases on `origin/<branch>`
before retrying (linear history, no merge commits). Don't manually
`git pull --rebase` — that's the button's job. Only intervene when
the button reports rebase conflicts (working tree left clean); then
start a manual rebase, resolve, and push.

### Commit messages

Summarize the worklist item that drove the commit. Use
multiline. Reference the driving issue if there is one — in
**non-closing phrasing**: "Refs #N", "see #N", or bare "#N" in prose.
Never use forge closing keywords (`close/closes/closed`,
`fix/fixes/fixed`, `resolve/resolves/resolved` followed by `#N`): both
GitHub and GitLab auto-close the referenced issue when such a commit
reaches the default branch, bypassing the close-on-commit dialog's
explicit user consent (first live occurrence: gitlab-demo 2026-07-21,
where `Closes #1` closed the issue before Bram's close-on-push ran).
Closing is the dialog's exclusive authority on every forge; the
`worklist-commit` gate rejects messages containing closing keywords so
they can be rephrased before the commit exists.

### Close-on-commit confirm dialog

When an item's `applied` commit would resolve a GitHub issue, set
`closesIssues: [{number: N, title: "..."}, ...]` on the item (title
from `gh issue view N --json title`; refresh if you iterate).
Approving a committable item (`applied`, or a begun `proposed` item
offered Commit) with non-empty `closesIssues` opens a
confirm dialog — one row per issue plus an optional close-comment
textbox. Ticking issues records them for automatic close-on-push (see
below); "commit only" commits without queuing any close. There is no
push-from-close path: closing follows the user's separate, explicit
Push. (A residual `push-before-close:` toggle in the dialog is inert —
the backend ignores it; removing it from the dialog UI is a small
follow-up.)

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
`approved:` payload as `close-issue:` lines appended after any free-text
feedback:

```
close-issue: 52
close-issue: 50 comment: "shipped, see commit message"
```

**Closing is fully automatic — the agent does nothing (close-on-push-
automatic, security H5).** At the commit gate the host itself parses these
verified `close-issue:` selections and records a pending close bound to the
new commit SHA. Nothing closes or pushes at commit time. On the user's next
explicit **Push**, once each commit reaches the default branch, the host
closes its issue automatically with the `Closed by <commit-url>` comment
(prefixed with the user's comment when one was given). Two refinements
(#282, found where squash-merge made the original predicate unsatisfiable):
a record whose issue is already closed by other means retires quietly
(`op=retired-already-closed`), and on GitHub a **merged PR** containing the
bound commit completes the close (`op=closed-via-pr`, `Closed by <pr-url>`)
even though the squashed SHA itself never lands on the default branch.

So after `worklist-commit` returns its `sha`, you are **done** — do not
resolve the SHA and do not tell the user to run a close step. There is no
close route or `issue-close` intent to write (both were removed); the host
does everything. Report the commit and stop; closing follows their Push with
no further action. **Closing never pushes** — the user's explicit
Push is the only thing that publishes commits, so closing one issue can
never silently push others stacked behind it.

**Approve without closing** arrives as feedback with no `close-issue:`
lines — commit only, nothing queued.


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

### Build vs. hot-reload boundary (developing Bram only)

The hot-reload table, launch discipline, and debug-build rules moved
to `docs/developing-bram.md` in the Bram source repo.

### Updating forge issues via gh / glab

Use the project's forge CLI directly — the Issues tab refetches on the
indexer's `issues-changed` signal (no polling), so updates surface without
a restart. There is no manual refresh; `/__issues?fresh=1` remains a
curl-only diagnostic that live-builds and rewrites the cached list. The forge is detected from the
`origin` remote (`.bram.json` `"forge"` override for ambiguous
self-hosted remotes; `GET /__app-info` reports the detection — see
`docs/forge-adapter.md`). On GitHub projects:

- `gh issue edit <n> --title "…" --body "…"`
- `gh issue comment <n> --body "…"`
- `gh issue close <n>` / `gh issue reopen <n>`

On GitLab projects the parallel `glab` commands apply (`glab issue
note <n> -m "…"` for comments). The worklist contract around issues
(`closesIssues`, `issue-<N>-` ids, close-on-push-automatic) is
forge-agnostic and identical on both.


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
- **Distinguish soak observers from tripwires — they graduate
  differently, and applying the wrong shape retires a working
  instrument.** A *soak observer* fires during normal operation, so a
  soak accumulates positive instances and the criteria above apply as
  written. A *tripwire* fires only when a rule is violated, and the
  rule usually exists precisely to prevent the condition — so correct
  operation reads zero indefinitely, and zero is **success, not absent
  evidence**. Its graduation question is not "did it fire" but "is the
  condition reachable, and would a fire be actionable?", settled by
  reasoning about the mechanism rather than by waiting.
  The inflight-claim collision instruments are the type case — a concept
  name, not a grep target: they emit under `[inflight-sentinel]` and
  `[auth-record]` (see the trace-vocabulary table).

  The trap: **a tripwire's zero and a dead instrument's zero are
  identical in a grep.** So a tripwire needs a *provenance* check in
  place of a soak — a deliberate violation in a test, or a review
  confirming the emit is wired and the path reachable. A THIRD zero is
  possible and was met on 2026-08-22: an instrument documented under a
  name it never emitted, whose grep therefore matched nothing while it
  was firing correctly. A name that cannot be grepped is a provenance
  check that silently fails. Receipt: the `inflight-collision` item
  draft borrowed the soak shape and asserted that
  a two-subagent run producing no lines would falsify it. The first
  two-track exercise (2026-08-19, xmlui-org/xmlui-mcp#33) ran two full
  gate cycles with zero fires — the convention worked — and by its own
  criterion that argued for dropping the tripwire that had just
  demonstrated the convention holding.
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
- **Exhaust on-disk evidence before declaring anything unverifiable.**
  Before writing "can't test from here" / "needs separate
  investigation", enumerate what already exists: rotated
  `bram-traces/bram-trace-*.log` archives (days of history, not just
  the live log), `git log`/`git blame`, Inspector exports, persisted
  tool results. Issue #69's hook regression was "unverifiable" until
  one grep across the rotated archives found 243 records that flipped
  the conclusion.
- **Local absence is not disproof.** For a bug reported from another
  machine or user, "not in my repo/history/disk" is expected for
  machine-specific artifacts and proves nothing about the remote
  case (issue #123). Verify the mechanism locally; frame the
  specifics as "can't be checked from here", never as discrediting
  the report.
- **Register new subkinds.** Every new trace op or subkind lands in
  the trace-vocabulary table (below) in the same change that
  introduces it, so the reading half keeps pace with the writing
  half.


## Search-first

Bram indexes the whole project history in embedded SQLite FTS5 (#230) —
Claude + Codex session transcripts, commits, issues, worklist-history — and
serves it at `GET /__search`. That turns the past into one bounded, ranked,
snippet-first query instead of per-source greps. Session JSONLs are 20–30 MB
and ungreppable without wrecking context, so before this the transcript
history was effectively write-only to the agent.

**The drill.** Before diagnosing a reported bug, proposing a worklist item,
filing an issue, or asserting a fact about the project's past, **query
`/__search` and cite what it returns.** The two highest-value triggers are bug
reports ("has this recurred / been fixed?") and "have we done X / did we
decide Y" questions. This is the log-first drill widened from the trace to the
whole history.

**Proactive triggers.** Search-first is not only for explicit questions about
the past. Fire a `/__search` *before* acting, and cite what it returns,
whenever:

- the user asks to "search the project" in any phrasing — `/__search` leads,
  grep follows for current-tree state (the FTS index covers history, not the
  working tree; the two are complements, in that order);
- you are about to reconstruct environment or infrastructure state — remote
  pod/VM setup, which corpora/indexes/models exist and where artifacts live,
  connection or proxy patterns ("where does X live" questions you are about
  to answer by directory listing). Infra state is exactly the knowledge that
  lives only in session history: it's not in the repo, not in CLAUDE.md, and
  each agent session starts blind to it;
- you are about to re-derive an operational recipe the project has plausibly
  executed before — setup scripts, launch/detach patterns, copy-back flows.

The test: if you find yourself groveling through the tree or asking the user
for a fact that a prior session likely established, the query was already
overdue.

**The call** (Claude loopback curl; literal port from `resources/.bram-port`,
already in the `.claude/settings.json` allowlist, so no prompt):

```
curl -4 -sS "http://127.0.0.1:61455/__search?q=<urlencoded>&limit=20&types=commit,issue"
```

(replace `61455` with whatever `Read resources/.bram-port` returned.)

- `q` defaults to **AND** across terms (every term must appear); wrap it in
  double quotes for an exact phrase. `mode=` overrides: `and` (default),
  `phrase`, `prefix`, `raw` (raw FTS5 boolean — `OR` / `NEAR` / `NOT`); invalid
  syntax falls back to a phrase match.
- `types=` filters buckets (`session` / `commit` / `issue` /
  `worklist-history`); omit for all. `limit` defaults 50, clamps 1–500.
- Ranked snippets are enough to judge relevance; for a hit's full stored
  content, `GET /__search/doc`.
- Compact read: `… | jq -r '.[] | "\(.type)\t\(.key)\t\(.snippet[0:140])"'`.

**Caveats.** FTS5 is keyword/phrase, not semantic — a miss means "nothing
matched those terms," not proof of absence (the same trap as event-shaped
logs). Scope is the current project. The issues bucket refreshes ~every 45s;
a just-created issue may not be indexed for a beat.

**Commit diffs are indexed.** Each `commit:` doc carries the commit's patch
text alongside its message (search-index-commit-diffs), so code-string
questions over commit history ("when did this identifier change") work in
`/__search` too — one query spans the discourse half (issues, sessions,
messages) and the code half of an investigation. Bounds, traced never
silent: diff lines over 2,000 chars are elided (`[long line elided]` —
neutralizes one-line minified vendor bumps while their file headers stay
searchable) and patches cap at 256 KB per commit (`[patch truncated]`);
truncations emit `[search-index] op=diff-truncated` with counts only.
`git log -S` / `git grep` remain the precision tools — uncapped,
regex-capable, any depth beyond the indexed `-n2000`.

### Coordinating MCP demand with search

When the question is "which how-to is missing, weak, or wrong?" for an
MCP-served project (XMLUI is the live one), treat MCP analytics as a
**nomination source**, never as proof. The analytics log is global across
projects and outcome-blind: a search always returns something, `result_count`
does not measure usefulness, and clock proximity cannot establish that a
project transcript caused or followed a query.

Validate a nominated topic with the project's `GET /__search` index across
sessions, commits, issues, and worklist history. Look for what happened after
the search: a how-to read that resolved it, repeated reformulation followed by
component or source fallback, a workaround committed, an issue filed, or a
later conclusion that reversed the first one. Then check the **current**
`xmlui_list_howto` / `xmlui_search_howto` corpus before calling anything a gap.

Classify the result instead of forcing every weak search into "missing
how-to": it may be a missing recipe, a discoverability problem, an inaccurate
reference page, an engine bug that needs a reproducer, a contradiction, or an
already-fixed gap. Operator test phrases and unrelated global MCP activity are
noise unless indexed project memory independently corroborates them. File an
upstream issue only when that evidence chain survives the current-corpus
check, and include the followable search, session, commit, and documentation
receipts.

Use `scripts/xmlui-howto-gap-miner.py` for a repeatable first pass. It groups
nearby `xmlui_search_howto` calls only when at least two meaningful terms
overlap, consolidates high-similarity recurrences across dates, attaches only
locally-near component/example/source fallbacks, and nominates a compact query
using recurrence plus corpus-wide term rarity. It asks `GET /__search` for
downstream project evidence and reconciles the result against the current
how-to directory. Its classification is a review hint, not a verdict; read the
returned snippets and current-doc matches before filing anything. Typical use
from the Bram repo:

```
python3 scripts/xmlui-howto-gap-miner.py --since 2026-06-01 --top 20
python3 scripts/xmlui-howto-gap-miner.py --since 2026-06-01 --json
```

The tool deliberately does not score `result_count`, parse provider-specific
transcripts, or correlate frustration by clock time. `--analytics`,
`--howto-dir`, `--search-url`, and `--port-file` make each input explicit;
defaults target the local XMLUI analytics/cache, `~/xmlui` how-tos, and this
project's `resources/.bram-port`.

### Citing evidence in issues and the search-wins ledger (issue #233)

When an issue comment, ledger entry, or postmortem cites evidence, make the
references followable:

- **Commits**: link the full-SHA forge URL
  (`https://<forge>/<owner>/<repo>/commit/<full-sha>`), displayed as the
  short SHA — bare short SHAs in backticks do **not** autolink on GitHub.
  Resolve with `git rev-parse <short>`.
- **Issues**: bare `#N` autolinks; use it.
- **Local-only sources** (session transcripts, worklist-history records)
  have no web URL. Name them by path or key, and where a runnable query
  helps the reader, include the `/__search` query (`q` / `mode` / `types`)
  that finds them — a distinctive phrase in `phrase` mode pinpoints; broad
  AND queries only retrieve.

**The search-wins ledger.** Keep a ledger issue in the project that collects
receipts for the claim that agent + indexed project memory changes the work
(this repo's is #233, which defines the entry format). Capture rule: record
an entry **at the moment** a live question is answered by the index and
something materially changes (a plan killed or redirected, prior art
recovered, duplicate work prevented). Routine lookups don't qualify; entries
are never reconstructed later. Ledgers are **project-local by design** —
receipts carry project specifics, so they belong in the project's own
tracker. Sharing standout entries upstream (to the flagship collection at
judell/bram#233) is a per-entry choice by the project's humans, never an
automatic behavior.

Worklist drafts follow the same rule: **plans cite inline in their prose**
(typed, followable refs — `code:path:line`, commit SHAs, issue numbers, doc
URLs, pinpointing `/__search` queries). Do not author
`resources/worklist-citations/<id>.json` files; that plumbing is dormant
(#232's postmortem has the rationale). For handing a user a runnable query
from the pane, use Search deep-linking:
`navigate('/search', { queryParams: { q, mode, types } })`.


## Debugging Bram itself

Three forensics surfaces, used together. The first two are raw
streams; the third is a dashboard that derives signals from them.

**`resources/bram-traces/bram-trace.log`** — host-side rolling log of HTTP
routes, iframe events, and inflight-sentinel writes / clears.
**On by default**; switch it off per project through **Settings → Traces**,
and `BRAM_TRACE` in the environment overrides the project setting either
way. Grep it directly when enabled. PTY previews and serialized
iframe payloads use Bram's `loomweave-scanner`-backed credential redactor before
persistence; Bram adds narrow structural expansion for complete PEM blocks and
Authorization/assignment values. Redaction is defense in depth, not a guarantee
for arbitrary content. At startup the prior active log is archived. A
background pass sanitizes and gzips raw archives older than
`traces.archiveAfterDays` (default 14 days, configurable from 1–3650), removing
each raw source only after its `.log.gz` replacement has been fully written,
synced, and atomically installed. Compressed history is retained indefinitely
with no byte cap, so trace storage is intentionally unbounded. The active log
is never an archive candidate during its session. Best for plumbing: stuck spinner,
sentinel anomalies, route errors, agent-turn-end detection,
heartbeat drift, close-cycle verification (`grep
"[issue-close-queue] op=closed" resources/bram-traces/bram-trace.log` —
one line per issue the host auto-closed after a Push; absence around a
known close timestamp means the commit hadn't reached the default branch
yet, or no close was queued at the commit gate; see also
`op=retired-already-closed` and `op=closed-via-pr`, #282).

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
| `describe-patch` | `__bramFlushDescribePatches` in `helpers.js` | `stage` (`begin` \| `end` \| `settle` \| `subagent-refetch`), `patches` (applied), `queued`, `provider`, `turns`, `ms`; on `settle` also `settleMs`; `stage=subagent-refetch` (`patches` = matched subagent ids) marks a flush whose completions belong to the chip-selected subagent view — delivery is a `subagentTurns` refetch through the host overlay, so no main-projection begin/end bracket fires (broadcast to second-next-paint, the fan-out render cost the sync `ms:0` flush conceals) and `sinceBeginMs` | Brackets the full-projection rebroadcast that splices Haiku "Tool Descriptions" results (`ai.describeCommands`) into the transcript. Since describe-rebroadcast-coalesce (2026-07-22 perf audit: 524 per-result rebroadcasts degraded heartbeat drift to 4.1s max and a tab-switch subscribe refetch to 3.1s), completions ENQUEUE and one flush per ~400ms window broadcasts them all — the bracket measures the flush, its `patches` field the batch size. Emitted synchronously before/after `__bramBroadcastProjectedTurns` (via `logToHost` → `invoke`, whose IPC dispatch survives an iframe main-thread freeze), so a hard freeze in the re-render is self-diagnosing: a `stage:begin` with **no matching `stage:end`** names the broadcast as the freeze and quantifies it (`turns`, `resultChars`). Added for the 2026-07-11 describe-freeze recurrence on a large Codex session (82 turns / 1 MB); unlike `long-task`, which logs at recovery and goes silent on a terminal freeze. |
| `describe-load` | describe issuance counters in `helpers.js` (`__bramRequestCommandDescription`) | `issued_1s`, `inflight`, `requested_total`, `held` (requests queued by the typing hold), `window_ms` (active flush coalesce window) | describe-backfill-observability: one coalesced line per second while Tool-Description requests are issued or in flight — the backfill pressure curve with a denominator. Boot of a large session fires a burst (2026-07-30: ~375 calls in 4 min, peak ~150/min, saturating the main thread during typing); this subkind makes that storm and its drain directly visible instead of reconstructible-by-correlation. Host-side twin: `[ai-describe] op=call` carries `concurrent=` (active requests at call entry). Pacing (describe-backfill-pacing, graduated from the 22.7%-settle-churn storm): new requests hold while the user typed within 2s (`held`), and the patch-flush window widens 400ms→2s while backfill is active (`window_ms`). |
| `projection-broadcast` | `__bramBroadcastProjectedTurns` in `helpers.js` | `reason` (`describe-flush` \| `refetch:<trigger>` \| `unknown`), `route` (active hash), `subscribers`, `turns`, `ms` (sync fan-out), `tail_emits`/`tail_skips` (cumulative decisions of the tail-scoped last-exchange source — workspace-tail-subscription; skips are broadcasts whose exchange content was unchanged, i.e. storm broadcasts the Worklist page no longer pays for); a second `stage:settle` line carries `settleMs` (double-rAF render settle) | projection-broadcast-attribution (round 2 of the 2026-07-30 boot-latency work): every projection broadcast names its trigger, the active tab, and both halves of its cost. Motivating finding: ~195ms settles on `#/worklist` — a page rendering no transcript rows — because Workspace subscribes to the projection (`Workspace.xmlui` PushSource). The graduation grep: which routes/reasons account for the non-describe 389–562ms long-tasks. |
| `follow-state` | `__bramFollowTransition` / `__bramFollowVerify` in `helpers.js`; call sites across `Transcript.xmlui` | `op=transition` (`to` bool, `cause`: `user-scroll-bottom` \| `user-scroll-up` \| `find-step` \| `tool-expand` \| `footer-arrow-up`/`-down` \| `mount-restore-reading` \| `agent-chip-switch` \| `unseen-jump` (chip-recruited arrival from another tab), `route`, `agentId`); `op=verify` \| `op=violation` (`cause`: `footer-arrow-down` \| `content-append-repin` \| `settle-repin` \| `mount-pin` \| `subagent-switch-repin`, `landed`, `endIndex`); `op=echo-suppressed` (`to`, `cause` = the echo-window opener (a transition cause or `mount`) or `uncorroborated`, `inputAgeMs`, `agentId`); `op=unseen-clear` (`count` of unseen turns cleared, `cause` = the FOLLOWING-entering transition, `route` — transcript-new-below-badge: the footer status-line chip's recruitment log); `op=repin-blocked` (`varAtBottom`, `sinceReadingMs`, `agentId` — follow-state-source-of-truth: a repin the stale xs var would have misfired, blocked by the synchronous window truth; the 2026-08-01 240ms-into-READING yank class made countable); `op=echo-repin` (`gap`, `attempt`, `maxRenderedIndex`, `total`) / `op=echo-repin-capped` — transcript-follow-echo-repair: a suppressed machine scroll took the view off the bottom AFTER an explicit jump verified `landed=true`, and the final row's bottom is now measurably below the fold, so the jump is re-issued (bounded 3 per rolling 3s). Fires on the MEASUREMENT, never on the suppression alone, so zero lines means the class stopped rather than the instrument going quiet. `op=echo-suppressed` carries `lastRowRendered`/`lastRowGap` for the same reason — it separates the ~10% of suppressions that are this failure from the jitter that is not | transcript-follow-contract layer 1: the Transcript's follow/reading contract (authoritative text in Transcript.xmlui's header) made self-reporting. Every state flip logs its cause; every bottom-promise is measured against the List's own visibleRange after a double-rAF, and a miss logs a violation — the six-fix whack-a-mole lineage (9daa693/f846258/6a892f6/652d9b3/410d069/c4c14ed) becomes a named, greppable inventory. Violations are leads, not convictions: an append landing inside the verify window can log a violation the next repin heals — corroborate with surrounding lines. Layer 2 (transcript-follow-echo-guard) graduated from this log's soak (1,056 bottom-promises, zero violations; phantom user-scroll flips within 200-400ms of every tool-expand): deliberate transitions and mount open a one-shot echo window (`__bramFollowEchoOpen`, 700ms / 1500ms mount), and `__bramFollowClassify` attributes a scroll event to the user only when no window is open AND user input corroborates it (wheel / keydown / held-or-recent pointer within 400ms; passive capture listeners). Input corroboration was added after the first live soak caught an expand echo re-arming FOLLOWING 2.7s post-click — machine scrolls have no input beside them at any latency, so no fixed window suffices. Suppressions log `op=echo-suppressed` with `inputAgeMs`, no state flip. A suppression of a genuine user scroll would surface as a missing expected transition next to an `echo-suppressed` line. Replaces the old `transcript-follow` onScroll trace. |
| `describe-scan` | `__bramEagerDescribe` / `__bramEagerDescribeSubagent` / `__bramPumpDescribeQueue` in `helpers.js` | `op=scan` (`ms`, `turns`, `entries` scanned, `queued`, `route`); `op=scan-subagent` (`agentId`, `ms`, `turns`, `entries`, `queued` — subagent-transcript-describe: the chip view's eager scan over the subagent stream); `op=pump` (`ms`, `via` `range` \| `dom-scrape`, `visible`, `queue`, `subQueue`, `route`; emitted only when either queue had work) | Round 3 of the boot-latency work (eager-describe-scan-instrumentation): the eager-describe queue rebuild walks all turns × entries inside every broadcast, and the pump's visibility re-partition falls back to a `[data-index]` DOM scrape on non-Transcript pages (also riding the 1.5s drain interval). Suspected for the 17.5s zero-subscriber settle (2026-07-30 22:49 worklist boot). Graduation candidates pre-agreed: incremental queue maintenance from deltas; gate the visibility scrape on `__bramTranscriptMounted`. |
| `projection-subscriber` | `notify()` in `bramSubscribeProjectedTurns`, `helpers.js` | `idx`, `ms`, `total` | Companion to `projection-broadcast`: times each PushSource emit's sync slice, logging any ≥50ms — a consumer named directly. React may defer the real render to the settle half; a silent subscriber log with a fat `settleMs` means the cost lives in the deferred render, not the emit. |
| `input-latency` | input responsiveness probe in `helpers.js` (keydown/pointerdown, rAF-measured, ≥100ms — floor dropped from 200ms for the describe-backfill-pacing soak; the felt band lives at 50-200ms) | `event`, `latencyMs`, `hadFocus`, `target`, `route` (active tab hash — the per-tab felt map); `describeInflight`, `lastLongTaskMs`, `lastLongTaskName`, `lastLongTaskAgoMs` (blockedBy attribution, describe-backfill-observability) | One line per slow input event: rAF delta ≥200ms from dispatch. The blockedBy fields are written at emit time — how many describe fetches were in flight and the most recent `long-task` within 1.5s — so a slow keystroke names its suspect directly (the 2026-07-30 storm needed three-subkind timestamp correlation to convict). |
| `refetch-called` | Workspace.xmlui debounce after `talk-session-changed` | `context`, `correlation_id`, `at_host_ms`, `delta_to_emit_ms` (host emit minus refetch-fire time, so it includes the 400 ms debounce coalesce) | Post-debounce refetch tick. A `delta_to_emit_ms` far above 400 ms means the iframe main thread was busy between emit and refetch. |
| `inspector-tap-tick` | `__inspectorTapTick` in `helpers.js` | `batch` (entries forwarded this tick), `available` (entries ready), `ms` (loop wall time) | Per-non-empty tick of the Inspector tap poller. Empty ticks are silent so this is a slow-tick alarm: a tick with `ms` ≫ 200 (the tick interval) means the IPC channel is backed up while the poller serializes entries through `logToHost`. Pairs with `inspector-event` / `inspector-overflow`. |
| `click` | UI Button onClick handlers (Worklist gate row and legacy Workspace) | Worklist: `target` (`worklist2-batch-approve` \| `worklist2-batch-approve-commit` \| `worklist2-batch-commit` \| `worklist2-batch-iterate` \| `worklist2-batch-drop` \| `worklist2-new-item`), `count` (or `issue` for new-item); legacy: `target` (`approve` \| `drop` \| `iterate`), `item` | Worklist user actions. |
| `queue` | queue mutation helpers in `helpers.js` (`__bramQueueAdd` / `__bramQueueUpdate` / `__bramQueueRemove` / `__bramQueueReorder` / `__bramQueueSend`) | `op` (`add` \| `update` \| `delete` \| `reorder` \| `send`), `id`, `chars`; `count` on `reorder`; `mode` (`message` \| `iterate`) on `send` | Queue-tab (`AgentMessageQueue`) mutation audit (queue-mutation-trace). `chars` is the note's text **length, never its content** (queue prose is user-authored, kept secret-safe like the describe redaction). A `send` logs `op=send` only — the internal removal suppresses its `op=delete` — so `delete` marks a user Delete, the recoverability signal for a mistaken removal. |
| `skill-invoke` | `__bramRunSkill` in `helpers.js` (Skills launcher) | `name`, `args_len` | One line per project-skill launch from the agent-pane Skills control (issue-221-skill-launcher). The turn itself rides `toTurn` (`/name args`) / `toShell` for Edit-first; this records which skill ran and its argument length. |
| `inflight-set` / `inflight-clear` | Workspace selectors + `inflightClaim` DataSource | `item`, `via`, `target`, `reason` | Inflight sentinel transitions; complements the host-side `[inflight-sentinel]` log entries. |
| `inflight-sentinel` / `auth-record` | `write_inflight_claim_sentinel` / `clear_inflight_claim_sentinel` / `shrink_inflight_claim_sentinel` / `consume_worklist_authorization` in `lib.rs` | `[inflight-sentinel] op=write` (+`prior_ids`, `prior_kind`, `prior_age_ms` when displacing a live claim); `[inflight-sentinel] op=clear-partial` (`claimed`, `requested`, `remaining`); `[inflight-sentinel] op=clear-shrink` (`resolved`, `remaining`); `[auth-record] op=consume-already-consumed` (`kind`, `consumed_at_ms`, `ids`) | subagent-worklist-collision-observability: observe-only detection of the ways concurrent subagent delegation collides on single-claimant host state — a claim overwritten while live, a clear covering only part of the active claim, and a second consumer finding the authorization record already spent. Steady-state single-agent sessions emit none of these; any occurrence is a collision. **This row is two emitted categories, not one subkind.** It read `inflight-collision` for a long time — a concept name in a column where every other entry is a literal greppable prefix, and that string appears nowhere in `lib.rs`. Live 2026-08-22: a partial clear happened, was correctly traced, and was reported as *"the tripwire didn't fire"*, because the grep used the documented name and matched nothing. A working instrument was declared dead on the authority of its own documentation. The conventions warn that a tripwire's zero and a dead instrument's zero look identical in a grep; this is a third case, where a misnamed instrument manufactures a zero that is neither. Greps that work: `grep -E '\[inflight-sentinel\] op=(write|clear-partial)'` and `grep '\[auth-record\] op=consume-already-consumed'`. **`op=clear-shrink` is NOT a collision** — it is a lifecycle route retiring the ids it resolved from a multi-id claim (incremental-claim-retirement). It is listed here because it shares the sentinel's category and because keeping it distinct from `clear-partial` is the whole point: folding the two together would make `clear-partial` mean "collision OR normal progress" and cost it its edge. |
| `voice-input` | Worklist voice input path in `Globals.xs` | `stage` (`start` \| `recording-started` \| `stop` \| `append`), `target`, `requestId`, `stopAtMs`, `stopToResultMs`, `stopToAppendMs`, `parentStopToDeliverMs` | End-to-end voice latency for iframe-driven dictation. `stopToAppendMs` on `stage:append` measures Stop Record click to text insertion in the XMLUI input, useful for Mac/Windows comparisons. |
| `paste-target-mirror` | `bramSetActiveVoiceTargetMirror` / `bramSetActiveFocusedFeedbackItemIdMirror` in `helpers.js` | `kind` (`voice` \| `focused-feedback-item`), `value`, `prev` | One line per change to the two mirrors that route a pasted screenshot or dictation to the right input (the Worklist feedback box sets a full `feedback:<id>` voice target on focus). A paste landing in the wrong box starts here: which mirror was stale when. |
| `paste-current-target` | `bramCurrentPasteTarget` in `helpers.js` | `voice`, `focusedFeedback`, `placeholder`, `activeLooksLikeFeedback`, `activeLooksLikeMessage`, `result` | The routing decision itself, logged with its inputs at paste time: the focused element's placeholder, both mirrors, and the chosen `result` target. Pairs with `paste-target-mirror` to distinguish a stale mirror from a wrong decision. |
| `inspector-event` | `__inspectorTapTick` in `helpers.js` | `entry` (sanitized `window._xsLogs` record) | Per-entry forwarding of the XMLUI Inspector log into `bram-trace.log` so Inspector events interleave with host traces live (#181). Opt-in via the **Traces → Inspector trace tap** switch in Settings (persisted as `traces.inspectorTap` in `.bram.json`). `__bramTraceSafeValue` truncates deep/large values and masks secret-shaped keys and known credential patterns before IPC; the host redacts the serialized payload again before persistence. Inspector traces remain intentionally complete — every keystroke, render, state change — so volume is high and heuristic redaction cannot prove arbitrary values safe. |
| `inspector-overflow` | `__inspectorTapTick` in `helpers.js` | `dropped`, `totalSeen` | Per-tick (200 ms) cap of 50 forwarded entries was exceeded; high-water mark advanced to current length and the listed count was dropped. Persistent overflow means cadence or cap needs tuning. |
| `turns-projection` (host) | `read_projected_turns` / `try_incremental_projected_turns` in `lib.rs` | `op=rebuild` (`src_bytes`, phase ms `read/parse/project/serialize`, `turns`, `window`, `body_bytes`, `total_ms`); `op=incremental` (`suffix_bytes`, `merged_turns`, `ms`) | Projection cost accounting on long sessions: the rebuild-vs-tail-merge ratio and which phase dominates (post-#214 measurement: parsing is ~10% of a rebuild; project/serialize dominate). |
| `reveal-floor` (host) | quiescence observer in the pty-throughput ticker, `lib.rs` | `op=would-reveal` / `op=reveal-suppressed reason=menu-displayed` / `op=reset reason=activity\|turn-changed\|turn-closed`, with `silence_ms`, `gap_p95_ms`, `gaps_n` | Phase-0 observe-only soak for the auto-reveal-terminal predicate ("turn open + byte-silent + no pane menu"). The graduation review greps these: every `would-reveal` must map to a corroborated terminal-needing moment. |
| `terminal-attention` | detector in the pty-throughput ticker, `lib.rs` (host); `bramSubscribeTerminalAttention` join bridge, `helpers.js` (iframe) | Host: `op=fire shape=hooks-trust silence_ms=<n>` / `op=clear reason=activity` / `op=candidate preview=<redacted tail, last 160 chars>`. Iframe: `op=warn` / `op=cleared` (via `__bramIframeTrace`), fired only on a derived active-state flip. | issue-234 unattended-boot-prompt banner. Turn-agnostic sibling to `reveal-floor` on the same ticker: fires on no-open-turn + PTY byte-silence ≥10s + a prompt-shaped ANSI-stripped tail, independent of terminal-pane visibility (the host can't see that — only the parent shell can). One classified shape today (`hooks-trust`, the Codex hooks-re-trust prompt); `op=candidate` is the observe-only inventory non-matching silent-and-unopened tails are logged into, from which future shapes graduate (never from speculation — see the item's "Scope: one classified shape + a candidate inventory"). The iframe bridge joins the host's `active` with the parent's `bram-terminal-visibility` state — displayed banner active = `host.active && terminalHidden === true` — so the footer notice only ever shows for a prompt the user genuinely can't see. |
| `compaction` | `CompactionTracker` + `CodexCompactionObserver` in the pty-throughput ticker, `lib.rs` (host); `bramSubscribeCompaction` join bridge, `helpers.js` (iframe) | Host: `op=fire reason=shape-present` (progress shape observed — banner on); `op=clear reason=progress-gone` (shape gone — banner off, hold continues); `op=complete reason=jsonl-compacted\|progress-fallback progress_seen=<bool> window=<id\|none> hold_ms=<n> record_lag_ms=<n>` (the one lifecycle edge; `record_lag_ms=-1` on the fallback path); `op=attach window=<id>` (late record after a fallback release — identity only, no second re-arm); `op=stale window=<id>` (live record predating the episode in flight — history only); `op=gap expected=<id\|none> got=<id>` (`previous_window_id` did not chain); `op=reread reason=Shrank\|SessionChanged\|OrdinalRegressed` (offset invalidated, full re-read; dedup makes the replay inert). Iframe: `op=warn` / `op=cleared` (via `__bramIframeTrace`), fired only on a derived active-state flip. | compaction-in-progress-banner, then issue #268 (episode-aware). **Progress and completion come from different sources, and conflating them was the original defect.** Progress is a PTY fact: a text-PRESENCE sibling to `terminal-attention` on the same ticker, structurally different from it — compaction actively prints a spinner, so `bytes > 0` on nearly every tick, and the detector fires when the live ANSI-stripped PTY tail CONTAINS the provider's progress line (`Compacting conversation`, matched case-sensitively by `compaction_progress_shape`, pinned from both Claude's and Codex's archived output) rather than on byte-silence. Completion is **not** a PTY fact: commit `9867a72` latched the terminal text `Context compacted`, but Codex repaints historical rows, so one real compaction consumed that latch 16 times at ~920 ms cadence (2026-08-21 04:43:00–04:43:44Z), each consumption re-arming the turn detector. Elapsed-time de-duplication cannot repair it — the same historical marker repainted again 31 s later — so completion now comes from the rollout JSONL's one top-level `type:"compacted"` record per episode, identified by `(session_id, window_id)` and chained by `previous_window_id`. The PTY completion latch is deleted; repaints mutate nothing. `Fire` still means Bram actually observed the progress shape, so a completion discovered only from JSONL emits no `Fire` and no `active:true` (a same-tick true→false pair would only flash a banner for an episode already over). Banner lifetime and hold lifetime are decoupled: the banner clears on shape disappearance while the hold persists until a matching record or `COMPACTION_PENDING_MS`. That bound is a safety ceiling, not a tuned value — pin it from the `hold_ms` / `record_lag_ms` distribution once a soak has real Progress→Pending→JSONL episodes (the only correlated pair on record is ~138 ms). The iframe bridge joins the host's `active` with `bram-terminal-visibility` the same way terminal-attention does, and additionally layers an episode-keyed dismiss (modeled on the suspicious-silence bridge, not present on terminal-attention): `window.__bramDismissCompaction()` suppresses only the current episode (keyed off the payload's `atMs`, stable for the whole active span), and the next compaction fire re-shows the banner. |
| `state-mirror` (host) | `worklist_state_apply` / `worklist_state_open` in `lib.rs`, calling into `worklist_state.rs`'s `mirror_*` fns from the auth/claim choke points (`write_worklist_authorization_record`, `retire_worklist_authorization`, `write_inflight_claim_sentinel`, `shrink_inflight_claim_sentinel`, `clear_inflight_claim_sentinel`); `op=divergence` from `observe_state_mirror_divergence` in the `/__worklist` route, `lib.rs` | `op=open path=<p> state=created\|existing` (once per process, first successful open); `op=apply kind=auth-record\|auth-consume\|auth-consume-shrink\|claim-write\|claim-shrink\|claim-clear ms=<n>`; `op=error stage=open\|apply\|divergence detail=<err>`; `op=divergence field=items\|claim\|auth item=<id\|-> ` plus a compact `file={...} db={...}` both-values detail | state-mirror-store-and-ledger, phase A of a mirror-then-migrate strategy: a SQLite shadow of worklist lifecycle state at `<app_cache_dir>/worklist-state/<project-key>.db`, mirroring `resources/.worklist-authorization.json` and `resources/.inflight-claim.json` writes into `auth_records` / `claims` tables plus an append-only `transitions` ledger. Files remain the sole truth this phase — every mirror call happens AFTER its file write succeeds, and any mirror error is trace-only, never blocking or altering the file path. Counts and durations only, never item prose. The transitions ledger makes lifecycle history ("what happened to item X") a SELECT instead of a trace-archaeology session. `scripts/state-mirror-check.py` is an on-demand consistency checker between the files and this db (its `--items` flag additionally compares worklist.json items against the mirror; records/rows predating the db file's own creation time report `PRE-MIRROR (informational)` instead of `MISMATCH`, excluded from the failing exit code — a fresh db necessarily lags any file history written before it existed). state-mirror-divergence-tripwire (2026-08-26) added the `op=divergence` line: every `/__worklist` build derives the state view twice — from files and from the db — and `worklist_state::compare_divergence` (a pure function over a file-derived snapshot + the db `Connection`, so it is unit-testable without an `AppHandle`) diffs items (id/status/begunAtMs/files), the live claim, and the active auth record (ids/kind/consumed state). This is a **tripwire, not a soak observer**: zero `op=divergence` lines is the success condition, and per the tripwire-provenance rule its deny-path reachability is proven by deliberate-fire tests in `worklist_state.rs` (`compare_divergence_catches_*`, each writing the db a lie and asserting the mismatch is caught) rather than by waiting. Lines dedupe per `(field, item, detail)` per process via a `static OnceLock<Mutex<HashSet<String>>>` (same pattern as the `worklist-attribution` row's `REPORTED` set) and increment `STATE_MIRROR_DIVERGENCE_COUNT`, an in-process counter the Status tab's new **State Mirror** section reads. That section (db present/size/path tail, last-apply time, transitions row count, divergences this process) is the soak's dashboard — mirror health readable at a glance instead of a grep. The same change converted three existing Status rows to dual-source: **Current claim** (Inflight Sentinel section) now derives its state/detail from the mirror's live (uncleared) `claims` row and only falls back to the file when the mirror is unavailable, elevating to `warn` on db/file disagreement; **Claim pairs** (renamed from **Trace pairs**) replaced the `bram-trace.log` grep with always-on `claim-write` vs `claim-clear` transition counts from the ledger (`claim-clear` already subsumes a shrink-to-empty termination, so no separate shrink count is needed); and **Auth history (mirror)** is a new row alongside the existing file-derived Authorization rows, showing the last 5 `auth_records` (kind, id count, consume latency) that the single-slot authorization file cannot show. The Worklist section stays file-backed pending a later `state-mirror-items-shadow` reader flip. `Status.xmlui` needed no markup change — its `Items`/`Table` binding over `status.value.sections` / `$item.rows` is fully generic on `signal`/`level`/`state`/`detail`/`seen`, so a new section title and new signal names render with no template change. |
| `worklist-attribution` (host) | changed-file annotation in the `/__worklist` payload builder, `lib.rs` | `op=predates-greenlight item=<id> path=<p> mtime_ms=<m> begun_ms=<b>` (deduped per item\|path\|mtime per process) | issue-273-preexisting-change-tripwire, observe only: exclusivity proves no *other started item* made a change on a path — it cannot prove *this item* did, and third-party work (direct edits, pre-item edits, build regeneration) lands on whichever item declares the path (live 2026-08-23: `notice-banner-component` offered Commit on +16 −13 it never wrote). This fires when a path counted as an item's own was last modified **before** the item's `begunAtMs` — the change predates the green light, so the attribution is provably wrong. A **tripwire**: zero is success; provenance is a deliberate fire (touch a declared file before green-lighting a probe item), never a soak. Blind spots recorded so it is not oversold: it cannot see third-party edits made *after* the green light, and any same-file agent edit advances mtime past the stamp — a fire proves pre-existing work; silence proves nothing. The fire-side evidence is what picks between #273's candidate fixes. |
| `worklist-advance` | `observe_pending_advances` in the `/__worklist` route, `lib.rs` | `op=pending item=<id> pending_ms=<n> carried=<bool> age_ms=<n> claim=live\|none files_changed=<n> files_total=<n> last_change_ago_ms=<n>` (rate-limited to one line per item per 60s while the state holds); `op=cleared item=<id> pending_ms=<n> carried=<bool>` when the item leaves it — advanced, pruned, or its changes went away | reconcile-dropped-advance, **phase 1, observe only**. Applying an approved item is two agent-driven steps — edit the files, then `mutate op:"advance"` — and nothing reconciles them, so a turn that ends between leaves the item `proposed` with its work on disk indefinitely: no detector notices, nothing ages out or retries, and the only signal is a person reading a row and thinking the button looks wrong (three occurrences 2026-08-22 — twice from mid-apply interrupts, once from an agent that resumed and dropped the outstanding advance). The state is cheap to recognise — `proposed`, begun, carrying changes — but *mid-apply right now* and *abandoned* are the **same observation in a single sample**, differing only in what happens next; a stuck spinner that same day turned out to be correct behaviour (approved claims outlive turns by design, `c00b386`) yet was observationally identical to a drop. Hence an observer, not a fix. `claim=live` is the LOAD-BEARING discriminator — `age_ms` and `last_change_ago_ms` cannot separate a verifying apply from a dropped one, falsified by the #286 specimen (2026-08-25: a legitimate three-item apply logged `age_ms` ≈ 17.7 min with a stale `last_change_ago_ms` and every planned file already moved, spanning several turn boundaries while its orchestrator verified before calling `mutate op:"advance"` — `claim=live` was the only field that read honestly the whole time). Read it as: `claim=live` + all planned files moved → mid-apply (possibly a long verification, not just a short one), expect an eventual `op=cleared`; `claim=none` is necessary for a drop but NOT sufficient — the second #286 specimen (2026-08-26: three items pending 81 min with `claim=none` and changes ~64 min stale, every one in a legitimate state — two were apply-and-commit items, which by design never advance and sit `proposed`+begun+complete for exactly as long as the human takes to decide, and the third was deliberately parked after its verification found a false positive) showed that shape is also the normal awaiting-a-human-decision state. This is a **soak observer**, not a tripwire — it fires during normal operation, so instances accumulate and the criteria apply as written, restated post-#286: a fire persisting past a turn boundary while `claim=live` is EXPECTED during verify-before-advance and is not itself evidence of a drop; `claim=none` narrows the reading to {dropped OR awaiting a human decision}, and no emitted field separates those two — elapsed time is what a deliberate hold and an abandonment have in common, because the difference is intent, not repository state. The two specimens together settle phase 2 as surfacing only: an auto-reconciler on these fields would have advanced items mid-verification (specimen 1) and mid-decision (specimen 2, one of which carried a real defect). Keying on `begunAtMs` (`1b74dc3`) is load-bearing: before that stamp the check would have rested on a displaceable authorization and silently stopped noticing as soon as another item was approved. Phase 2 — surface or reconcile — is deliberately undecided until the soak exists, because auto-advancing work that is merely *incomplete* would assert a completeness nobody checked; #286 chose surfacing over an auto-reconciler for the same reason. The pending map is **persisted** (app cache dir, hydrated once per process) because it began as in-memory state, and that made the log unreadable for the one thing it exists to show: a restart between an `op=pending` and the advance dropped the `op=cleared` half, and *fired pending, never cleared* is precisely the dropped-advance signature — so every relaunch mid-apply minted a permanent false positive (both of the first two fires, 2026-08-22, were exactly this). `carried=true` marks an entry that outlived a process: its `pending_ms` spans downtime, so the *resolved within one turn* criterion does not apply to it as written. Two surfacing changes came out of #286 in place of a reconciler: the Worklist strip reads "Changes complete, not yet advanced" for a begun item under a live claim whose changed-file count equals its planned total (`__bramWorklist2Strip`, `helpers.js`), taking priority over the generic Approving…/Starting… verb; and `/__worklist` carries a top-level `claimedIds` array naming the ids the currently live claim covers, so an agent reading the board mid-turn can see what it still holds without a separate `/__inflight` round trip. |
| `worklist-strip` | `__bramWorklistStripAnomaly` in `helpers.js`, called from `__bramWorklist2Strip` | `op=anomaly reason=no-changes-yet-with-changed-files` with `item`, `status`, `claimed`, `auth`, `auth_age_ms`, `cs_changed`, `cs_total`, `files_moved`, `files_total` | strip-vs-diff-disagreement-instrument: a Worklist row printing "No changes yet" while its own payload carries files with non-zero add/remove counts — observed 2026-08-21 ~07:12Z on 0.5.0, with the Diff section directly below rendering the very edits the strip denied, self-correcting at the next refetch about a minute later. The halves have different sources of truth, which is why they can disagree at all: `diff` / `changedFiles` derive from disk state (`997017a`), while the strip is gated on `__bramWorklist2Begun`, a status/authorization fact. Three inputs could each produce it — a payload predating the approval, an authorization consumed earlier than assumed, or `changeSummary.changed === 0` beside a non-empty `changedFiles` — and the `/__worklist` route lines carry none of them (method, status, body size, duration only). The fields above are chosen to discriminate, so one fire names the cause instead of nominating a fix. **This is a tripwire, not a soak observer**: it fires only while the row is actively lying, so zero lines is the success condition, and a tripwire's zero reads identically to a dead instrument's zero in a grep — its provenance check is a deliberate fire, never a wait. Deduped per `(item, status, auth, cs_changed, files_moved)` so a re-render storm cannot bury the first occurrence. Retire it once a fire has named the cause and the fix has landed. |
| `w2-selection` | selection `ChangeListener`s in `Worklist.xmlui`; the `gateSel` `ChangeListener` in `WorklistGateBar.xmlui` | `op=prune-input` (`items`, `selected`); `op=publish` (`count`, `items`, `store`); `op=receive` (`count`, `render`, `store`) | instrument-w2-selection-handoff: the Worklist -> footer selection handoff, made self-reporting. Selection crosses a window store (`__bramW2SetSelection` / `bramSubscribeW2Selection`) that is written **only on change** - the setter early-returns on an equal array - so nothing ever republishes the current value. Observed 2026-08-24: the footer gate bar rendered **no buttons** while the row's checkbox was still ticked. **Convicted same day, second capture**: the gate-bar submit path clears the store directly (`__bramW2SetSelection([])` in `helpers.js`), which traced as a publish-less `op=receive count=0 store=0` at the Approve click, followed by `op=prune-input items=3 selected=1` persisting indefinitely - sync was one-way (page -> store), so the page var and its `initialValue`-bound checkboxes never learned of the clear. The original refetch-wipe hypothesis (`op=prune-input items=0` then `op=publish count=0 items=0`) was falsified by the same capture: `items=3` throughout. Fixed by w2-selection-store-to-page-sync: a `PushSource` on the same store subscription bridges store -> page, making the store authoritative in both directions. The instrument stays through the fix's soak - steady state emits publish/receive PAIRS and the anomaly shapes are unchanged (a publish with no matching receive, or a `receive count=0` beside a live selection) - then retires per its charter. |
| `interrupt` | `resolve_cancel` + the interrupt observer in the pty-throughput ticker, `lib.rs` | `op=interrupt reason=jsonl-turn-aborted\|jsonl-request-interrupted id=<record id> record_lag_ms=<n> pty_marker_seen=<bool>` (a declarative record decided); `op=interrupt reason=pty-fallback deferred_ms=<n> provider=<p>` (no record arrived within `CANCEL_DEFER_MS`, so the PTY marker carried the case) | structured-interrupt-edge-both-providers: the user-cancel edge, moved off repaintable terminal text. `pty_output_clears_inflight` substring-matches `Conversation interrupted` / `You canceled the request` in PTY bytes, and because Codex repaints historical rows `c962ccc` had to guard it with a 5 s post-Esc window — the same elapsed-time-as-identity instrument #268 rejected for compaction. Both providers write a declarative record instead: Codex `event_msg` / `payload.type == "turn_aborted"` (with `turn_id`, `reason`, `duration_ms`), and Claude a `type:"user"` record whose text block starts with `[Request interrupted`. The Claude discriminator reads through `send_ledger_record_text`, which keeps only `type == "text"` blocks — load-bearing, since a `tool_result` echoing the sentinel and assistant prose discussing it both occur in real transcripts and a naive substring search matches all three. Precedence, not replacement: a PTY marker no longer acts immediately but arms a deferred marker that a record supersedes; only if none arrives does `pty-fallback` fire with the previous effects. The PTY path is deliberately kept (unlike the compaction latch, whose coverage the `task_started`/`task_complete`/`turn_aborted` accounting proved) because presence data cannot prove these records cover *every* cancellation. Retirement criterion, answerable by grep: if `reason=pty-fallback` never fires for a provider over a soak, delete that provider's PTY path. `record_lag_ms` is also what pins `CANCEL_DEFER_MS`, which is a bound and not a tuned value. |
| `esc-scan` (host) | send-ledger escape sweep and soft turn-end poller, `lib.rs` | `op=sweep` (`read_ms`, `total_ms`, `bytes`); `op=soft-turn-end` (`ms`, `bytes`, `waited_ms`) | Times the per-Esc full-session scans. Exonerated the host in the 2026-07-08 wedge hunt (5 ms over a 26 MB session). |
| `pty-menu` `op=surface-gap` (host) | `pty_menu_update` surface point, `lib.rs` | `tool`, `ms` (grid-first-sighting → pane-surface, `-1` if no matching sighting), `suppressor_armed`, `suppressor_age_ms`, `suppressor_tool`, `fp` (option-label fingerprint) | Observe-only instrument for menu-redetect-storm-after-completion facet B: measures the "menu on terminal but not pane" blindness window. A large `ms` with `suppressor_armed=true` (or `suppressor_tool` ≠ `tool`) implicates the unbounded post-dismiss suppressor / byte-pattern tool misinference holding back a genuinely-new menu. Graduation to a fix needs ≥2 captures naming a consistent culprit. |
| `pty-menu` `op=label-norm-divergence` (host) | post-dismiss suppressor in `pty_menu_update`, `lib.rs` | `raw` (`exact`\|`prefix`\|`none`), `norm`, `tool` | Observe-only (suppressor-label-normalization-divergence-observe): the fingerprint suppressor matches RAW option labels while the grid-rescue wrapper (`grid_menu_dismissed_label_match`) normalizes both sides (`normalized_menu_label`: collapse whitespace, straighten U+2019, lowercase). Emitted only when the two verdicts disagree on the same inputs — steady-state noise is zero. `raw=none norm=some` = the raw gate leaked a ghost the normalized gate catches (→ normalize the suppressor); `raw=some norm=none` = it over-suppressed a distinct menu (the worse fault); zero over a soak = the asymmetry is benign. |
| `xterm-liveness` (parent shell) | heartbeat watchdog in `app/main.js`, arrives with `context:parent` | `op=stall`, `gap_ms` | Measures freezes ≥500 ms of the parent main thread xterm renders on; one line per stall, logged at recovery. Stalls bracketed by a slow named op implicate it; absence during a felt wedge relocates the problem below the webview (child process / PTY). |
| `pane-visibility` | visibility/focus listeners in `helpers.js` (backgrounded-pane-menu-paint-observer) | `state` (`hidden` \| `visible` \| `blur` \| `focus`), `via` (`visibilitychange` \| `window`) | One line per pane visibility/focus transition. Correlation substrate for `menu-paint`: a `visible`/`focus` line stamps `__bramPaneLastVisibleMs`, the refocus instant a starved paint lands after. Observe-only. |
| `menu-paint` | double-rAF probe in `__bramApplyAgentMenu`, `helpers.js` (backgrounded-pane-menu-paint-observer) | `tool`, `menuId`, `hidden_at_receive`, `focused_at_receive`, `receive_to_paint_ms`, `painted_after_refocus` | Receive-vs-paint marker for the backgrounded-menu miss (2026-07-19: Write menu sat 28.8s, answered only after focus-in). rAF stalls while the webview is hidden, so a menu received hidden that paints only on refocus shows `receive_to_paint_ms` spanning the hidden period with `painted_after_refocus=true`, pairing with host `[prompt-lifecycle] op=shown`. Graduation (a render nudge on visibility→visible) needs the soak to show every long emit-to-answer gap on a backgrounded window matches a late paint, and zero late paints while visible. Observe-only. |
| `long-task` | `PerformanceObserver('longtask')` in `helpers.js` | `ms`, `name` | Iframe analog of `xterm-liveness`: one line per iframe main-thread task ≥200 ms, logged at recovery. Added for the 2026-07-09 describe-freeze (trace went silent at the freeze instant with nothing attributing the block); a hard freeze now names its duration instead of leaving a gap. |
| `resizeobserver-flood` | `installResizeObserverFloodDetector` in `helpers.js` | `firesPerSec`, `top` (className=count pairs) | Once per second while global ResizeObserver fire rate exceeds 50/sec, names WHICH elements are looping. The wrapped constructor counts every callback fire; `div._row_…` is XMLUI List's per-item wrapper, observed by virtua's item resizer. Diagnostic for the transcript RO-loop freezes (#150 lineage). |
| `resizeobserver-flood-detail` | `installResizeObserverFloodDetector` in `helpers.js` | `via` (`interval` \| `sync`), `newElements`, `repeatFires`, `ring1`..`ring4` | Companion to `resizeobserver-flood`: dumps the last ≤60 fires as compact strings — `+dt key#idx WxH*` (ms since prior fire, short element key `row`/`main`/`html`, `data-index` when present, contentRect to 0.1px, `*` = first-ever observation of that element). Discriminates the three flood mechanisms: same `#idx` alternating two heights = CSS oscillation (delta ≈15px → scrollbar, ≤1px → fractional rounding vs virtua's cache); streams of `*` across many indexes = remount loop (heights innocent); `main`/`html` entries interleaved with row re-measures = container size churn driving row re-wraps. `via:interval` is the per-second tick (its `newElements`/`repeatFires` are per-second counters); `via:sync` is emitted from INSIDE the RO callback when ≥120 fires accumulate without an intervening tick — i.e. the main thread stopped yielding — so a terminal freeze testifies instead of dying silent (counts derived from the ring; throttled to one per 2s; rides `logToHost` → `invoke`, whose IPC dispatch the host logs even if the iframe never yields again). Chunked strings because the trace serializer summarizes arrays and truncates strings at 500 chars. |
| `tool-format` | `__bramFormatToolResult` in `helpers.js` | `stage` (`begin` \| `end`), `tool`, `chars`, `longestLine`; on `end` also `ms`, `outChars` | Synchronous bracket around the tool-result formatter, emitted only for inputs >8KB (steady-state noise is zero). Built for the variant-B expansion freeze (2026-07-11 22:48Z: click → describe route entry → iframe dead, RO-quiet): with the click handler exonerated host-side, a freeze showing `begin` with no `end` names the formatter's string work; `begin`+`end` then silence names Markdown parse / WebKit layout by elimination. `longestLine` quantifies the long-line layout suspect that the formatter's 16KB total-size cap does not bound. Rides `logToHost` → `invoke`, so the host logs both stages even if the iframe never yields again (describe-patch precedent). |
| `xmlui-probe` | instrumented vendored engine (`~/xmlui` `script-runner/eval-trace.ts` — engine-neutral `evalTrace`, armed via `window.__xmluiEvalTraceUntil`, emitting through the host-registered `window.__xmluiEvalTraceSink`, which Bram forwards to this subkind; hooks in `evalBinding` before the compiled-bindings branch, the statement-queue loop, and the container reducer) | `op` (`eval` \| `stmt` \| `action`), `d` (binding source / statement / action+uid, ≤80 chars) | Freeze-probe for the transcript-expansion hang: emits synchronously (`logToHost` → `invoke`, survives a frozen main thread) but ONLY while `window.__xmluiEvalTraceUntil` is armed — `__bramExpandTool` arms 1.5s on each tool-row expansion click; inert otherwise and in every other xmlui embedding. A hang inside one evaluation/statement never returns, so the stream after a fatal `dom-click` ends AT the hanging site: `op=stmt` names a handler statement, `op=eval` a binding (with source text), `op=action` a state cascade. Expect a few hundred lines per armed click; that volume is the diagnostic, not noise. Remove the vendored probe once the hang is attributed upstream. |
| `send-ledger` (host) | ledger transitions and guards, `lib.rs` | `op=inject/transition/restore/auto-resend/aborted-no-restore/aborted-skip/stale-input-clear/stale-input-clear-skip` with entry ids, causes, byte counts; `op=submit-nudge` (`id`, `dwell_ms`; split-paste-cr-and-submit-nudge — a lone CR written when an unlanded entry's payload sits visibly in the composer past 5s, one nudge per entry, mirrored to always-on strand-forensics); `op=cross-session-land` / `op=cross-session-strand reason=not-found`, each `id=<id> old=<basename> new=<basename>` | Outbound-send lifecycle forensics: landing vs strand vs abort classification, restores, and the stale-terminal-input clear decisions. `cross-session-land` / `cross-session-strand` are the graduated form of send-ledger-reanchor-cross-session-strand (superseding the observe-only `would-suppress-strand`): when an in-flight send's active session differs from its inject session, landing detection re-anchors to the whole current session. A send found there resolves landed with `cross-session-land` (the pa11 false-strand, now fixed); one found nowhere still strands mechanical and warns, tagged `cross-session-strand` as a genuine cross-session-loss suspect. |
| `send-forensics` (host, **always-on**) | `append_strand_forensics_line` in `lib.rs`, writing `resources/bram-traces/strand-forensics.log` — NOT gated by `traces.enabled` | `op=inject` (`id`, `kind`, `mode`, `bytes`, `provider`, `menu_at_inject`, `turn_open_at_inject`, `ms_since_pty_out`, `stale_input`, `preview`); `op=landed` (`via_queue`, `elapsed_ms`, `cross_session`, `turn_open_at_inject`, `queue_wait_ms` — how much of `elapsed_ms` was queue wait behind the prior open turn; a slow landing with `queue_wait_ms=-1` is a genuine delivery suspect, per pa11 #7's 8 queue-wait false alarms); `op=stranded` (scene: `payload_in_tail`, `silence_ms`, `menu_now`, `menu_at_inject`, `ms_since_pty_out_at_inject`, `provider`, `stale_input`, `via_queue`, `retried`, `cross_session`, `tail="<400 ANSI-stripped chars>"`); `op=auto-resend` | strand-scene-forensics: every pane send writes an inject breadcrumb paired with a resolution line, so an inject with **no** resolution is itself the strand signal — across restarts (the in-memory ledger dies with the process) and on default-settings installs where `bram-trace.log` is off. On a strand, the scene names the cause: `payload_in_tail=true` = text sitting in the CLI composer, submitting CR never took (the observed live stranding shape; root cause confirmed 2026-07-22 and fixed by split-paste-cr-and-submit-nudge — the CR now rides its own PTY write after the paste, and a stuck entry gets one self-healing CR nudge, `op=submit-nudge`); `false` = injection swallowed entirely. Remote diagnosis: ask the reporter for this one small file. |
| `send-gate` (host) | `drain_pty_intents` + pty-throughput ticker in `lib.rs` (send-gate-hold-while-menu-open) | `op=hold` (`count`, `reason=menu-present`, `tool`); `op=flush` (`wrote`, `held_ms`); `op=hold-stale` (`held_ms`, `tool`; mirrored to always-on strand-forensics as `op=send-gate-hold-stale`) | Pane sends (`toShell`/`toTurn` intents) held while a permission menu or picker is displayed, instead of pasting into it and stranding (Eric 2026-07-19 21:04:44, `menu_at_inject=true`). `sendKeys` always passes — pane menu answers are what release the hold. AskUserQuestion never holds (typing over an open question is a legitimate answer path). Release is **evidence-based only**: the pty-throughput ticker flushes held intents when `pending_menu` clears. There is no operational timeout — force-flushing into a still-open menu would recreate the strand and could keystroke-answer a permission prompt. `op=hold-stale` fires once per hold at 120s as a diagnostic: a real menu sitting unanswered that long is user-visible; a ghost menu holding sends is a menu-eviction bug to fix at the source. `op=esc-suppressed reason=idle` (guard-double-escape-agent-exit) is a pane-origin bare Esc dropped because it had no job — no open turn to interrupt and no menu to dismiss — the state where a stray Esc falls through to the TUI as a gesture (double-Esc rewind / CLI exit); Esc typed in the terminal never rides this path, and any open turn or displayed menu passes the Esc untouched. (First live ghost caught 2026-08-01 — a user-answered dismissal's state clear was deferred behind hook ownership and an interrupt swallowed the hook's own clear, holding sends 7 minutes; fixed by ghost-menu-send-gate-eviction: user-input and outcome dismissals clear unconditionally, and an outcome clear that finds the raw cache empty sweeps a lingering pending flag, logging `[send-gate] op=ghost-cleared`.) |
| `hook-menu` (host) | permission-hook handlers and grid-defer decisions, `lib.rs` | `op=permission/payload/hook-diff/clear/retire-suppressor/grid-deferred/grid-emit-deferred/grid-emit-allowed`; parallel-menu-claim-queue: `op=claim-queue-add` (`id_key`, `resolved_id=<toolu_…|from-event|none|ambiguous|no-session|no-signature>`, `depth`; claim-id-resolution adopts the transcript's tool_use_id when the signature matches exactly one unresolved call), `op=claim-queue-remove` (`reason=hook-clear\|jsonl-resolved`, `id`, `removed`, `displayed_removed`, `depth`; `jsonl-resolved` is the synthesized terminal event for calls whose tool_result landed without a PostToolUse — e.g. failed commands), `op=claim-queue-select` (`joined=labels\|fallback\|signature\|tool-join\|unjoined\|grid-rescue\|none`, `cause`, `depth`; `tool-join` = the unambiguous candidate claim promoted onto the grid's live menu when no label/signature join succeeded — full claim identity (tool, tool_use_id, keyed clears), grid options; `unjoined` = the lone queued claim shown though it didn't join, `grid-rescue` = the grid menu surfaced directly because no claim matched — all display the grid's own options as ground truth since `menus.parseAndDisplay` is off by default and `pty_menu_update` won't surface them); `op=claim-labels-adopted` (`joined`, `tool`; adopt-grid-labels-on-join — the grid's own option phrasing replaced the hook-synthesized labels on a corroborated display, since the terminal text is what the keystroke actually grants), `op=grid-menu-without-claim` (`tool`, `sig_present` — a solved, benign race, not a mystery: the grid report was scored 14-80ms before the hook claim's enqueue landed (2026-08-01 measurements); the post-add coalesce/select pass re-joins within ~100ms); `op=grid-rescue-bare` (`reason=no-claim\|ambiguous\|answered-claim\|picker`, `depth`) — no claim could be promoted to a tool-join, the rescue rendered options-only (`answered-claim` = the lone candidate was the just-answered claim, ghost guard; the transient `op=grid-rescue-enriched` borrow from f46e8e2 was superseded by tool-join before ever firing enriched) | Hook-primary menu coordination: hook claims and their payloads, diff enrichment (`hook-diff cluster=N`), fence-suppressor retirement, and whether the grid deferred or emitted for a hook-owned prompt. The menu-miss retrospective greps these. Since parallel-menu-claim-queue, claims are a queue keyed by `tool_use_id` and the pane displays the claim whose option labels JOIN the grid's current display (the terminal arbitrates; a pane answer keystrokes the prompt it shows, by construction). `claim-queue-select joined=none` with depth>0 = queued claims none of which match the terminal — the grid path surfaces instead. `grid-menu-without-claim` marks the benign grid-before-claim enqueue race; the post-add selection pass self-heals it. Keyed clears (`claim-queue-remove`) never blank a different prompt's display; unkeyed clears (Codex PTY cancel, legacy hooks) drain the queue. |
| `subagents` (host) | `st_report_orphan_subagents` in `lib.rs` (subagent-discovery-workflow-dirs) | `op=orphan` (`agent_id`, `tool_use_id`, `cc_version`) | Coverage-gap instrument for subagent-transcript discovery. The main transcript names every dispatched agent (task-notification turns); an agent referenced there whose transcript the bounded-depth walk of `<sid>/subagents/**` cannot find means a Claude Code layout change or cleanup Bram can't see through. One line per (session, agent), stamped with the session's CC version so the layout shift correlates with the release that introduced it in one grep — the 2026-07-20 Workflow-dirs miss (`subagents/workflows/<wf_id>/`, CC 2.1.205) rendered a silently empty pane that read as "tracking is gone"; the next one names itself. |
| `session-new` (host) | `create_new_session` / `emit_talk_session_changed_for_provider` / `apply_pending_session_title` in `lib.rs` | `op=create provider=<p>`; `op=bootstrap-detected provider=codex sid=<sid>`; `op=name-pending provider=<p> sid=<sid> error=<detail>`; `op=named provider=<p> sid=<sid> title=<title>` | Named-session lifecycle from button click through metadata application. `create` should immediately put the queued title in the Sessions list as a non-interactive `[current]` pending row; Codex may not create any rollout file until its first real user turn. If Codex does write a bootstrap rollout, `bootstrap-detected` proves Bram selected it; `named` replaces the pending row with the real titled session. `name-pending` means the UUID was claimed but pinning or the provider metadata write failed, so later JSONL writes retry the same UUID rather than naming a different session. |
| `session-rotation` (host) | `emit_talk_session_changed_for_provider` in `lib.rs` | `op=detected provider=<p> old=<sid> new=<sid> silence_ms=<ms> tail="<pty snippet>"` | One line per genuine session rotation (the active provider's sid changed — provider-keyed, so a Claude↔Codex switch is excluded). `tail` is the last ~400 ANSI-stripped chars of the PTY tail at rotation time, so the rotation names its own cause: a usage-limit banner, a fresh Claude launch banner, a shell prompt (the CLI exited), or a `/clear`. `silence_ms` is the gap since the last PTY output (a long gap points at an idle/limit wait before a fresh relaunch). Diagnose-only; the child PID is unavailable (the PTY child handle is dropped, and the agent CLI relaunches inside the same PTY shell). Added to find why Claude Code keeps restarting into fresh sessions (session-rotation-self-diagnose). |
| `jsonl-turn-end` (host) | Claude/Codex JSONL completion detector, `lib.rs` | `op=scan/enter/skip/poll-handoff` with `tailTypes`, `decision`, `detected`; `op=would-end reason=user-after-assistant path=<basename>` | Turn-completion forensics: whether the JSONL tail marks the turn final. `would-end` is the observe-only phase of jsonl-turn-end-user-after-permission: emitted when a genuine user message (not a tool_result) trails a non-final assistant record — the interrupt/takeover signature where a pending permission menu should clear but the detector reports `non-final-assistant` and pins it. No behavior change yet; graduation to ending the turn needs the soak to confirm every fire is a real interrupt and zero fire on `assistant(tool_use) → user(tool_result)` mid-turn tails. The 2026-07-20 audit of the first soak (33 fires) failed 12: 9 were `isMeta:true` image-companion user records (string content beside a Read-tool image result) and 3 were subagent transcripts (`agent-*.jsonl`). The detector now excludes both classes (graduate-user-after-assistant-clear tuning); the soak baseline resets at that commit — only fires after it count toward graduation. |
| `prompt-lifecycle` (host) | `prompt_shown` / `prompt_resolved` / `record_menu_answer` in `lib.rs` | `op=shown` (`id`, `tool`, `source=hook\|grid`, `tool_use_id`, `labels`); `op=resolved` (`id`, `tool`, `outcome=answered\|resolved\|interrupted\|superseded\|session-ended`, `detail`, `open_ms`); `op=answer` (`id`=tool_use id, `label`, `via=click\|hook-clear\|claim-clear\|jsonl-clear`) — the chosen menu option, recorded immediately when the id is known (`click`), from the singleton fallback used only by claimless/pending menus (`hook-clear`), or from the exact answered claim when its id arrives via PostToolUse/JSONL cleanup (`claim-clear` / `jsonl-clear`); `op=answer-pending` (`reason`, `tool`, `label`) — a claimless/pending-menu label whose id is unknown, stashed for clear-time binding (60s TTL, one slot); `op=answer-miss` (`reason=no-displayed-claim\|no-key-match\|no-tool-use-id`) — an answer that could not be recorded or stashed, with the failing stage named (both grid and hook capture branches trace this); `op=answer-at-click` (`tool`, `id_source=pending-call\|resolved\|none`, `call_present`, `lookup_reason`, `displayed_source`, `shown_age_ms`, `supersede_age_ms`) and `op=answer-deferred-bind` (`gap_ms`, `tool` for the singleton path or `via=claim-clear\|jsonl-clear` for a per-claim record) — diagnostics for perceived "stuck" menus and delayed id binding. Claim-backed parallel answers retain distinct labels and click timestamps until their respective keyed clears; they never compete for the singleton stash. | upstream-prompt-lifecycle-events: Bram's own PromptShown/PromptResolved pair (the upstream-shaped API implemented locally — see `docs/upstream-asks.md` #3), emitted from the existing transition points: hook claim display and grid emit (shown), user-input dismissal (answered), hook/jsonl clears (resolved), Codex PTY cancel (interrupted), labels-join switch or joined=none (superseded), session rotation (session-ended). Exactly one open prompt at a time; one resolved per shown. Bounded history served on `/__prompt-lifecycle`. "What prompts appeared and how did each end" is now one grep. |
| `menu-echo-turn` (host) | `menu_echo_turn_observe` in `pty_write_internal`, `lib.rs` | `op=would-suppress` (`shape` `digit` \| `digit-cr`, `caller_hint`, `elapsed_ms`, `prompt_id`, `tool`, `outcome`, `labels`) | Observe-only (menu-echo-numeral-turn-observer): a menu-answer-shaped PTY write ("1".."9", pane clicks add `\r`) arriving with NO menu displayed, within 10s of a prompt resolution — the stray-turn candidate where a keystroke aimed at a just-answered/superseded prompt falls through to the composer and submits as a bare numeral turn (seventeen specimens 2026-08-08, clustered in subagent prompt churn). `shape` separates pane clicks from typed keys; `elapsed_ms` + the resolved prompt's identity say what the input was probably aimed at. Graduation to a hold/discard needs the soak to show every stray-numeral turn correlates within a small window AND zero fires for legitimate numeral answers to agent questions in chat. |
| `search-index` (host) | `start_search_indexer` / `run_search_index_pass` and the `__search` route in `lib.rs` | `op=scan bucket=<b> files=<n> indexed=<n> skipped=<n> rows=<n> ms=<n>` per incremental pass; Claude/Codex scans additionally carry `indexed_bytes`, `open_ms`, `discover_ms`, `gate_ms`, `extract_ms`, `write_ms`, `count_ms`; `op=scan-error bucket=<b> detail=<…>`; `op=scan-progress bucket=<commits\|issues> done=<n> total=<n>` (+ `batch_ms` for commits — one line per bounded backfill batch during cold-rebuild-shaped work, issue #250; the same progress publishes to `/__search-index-status` as `progress` and lights the footer/Status row even on quiet startup runs); `op=issues-list-rebuild-error detail=<…>` / `op=issues-list-rebuild-retry` (the issues:list cache rebuild failed and set the staleness marker / a marker-driven retry succeeded — #235); `op=issues-list-reconcile-stale probe_newest=<n> list_newest=<n>` (issue-252 post-rebuild reconciliation: the rebuild's gh snapshot raced a just-created issue — its newest number trails the probe's — so the staleness marker is set and the #235 retry rebuilds next pass instead of the cache staying stale forever) and `op=issues-list-fresh-error detail=<…>` (a fresh=1 diagnostic live build failed, cache served instead); `op=query chars=<n> facets=<allowlisted names> hits=<n> open_ms=<n> query_ms=<n> enrich_ms=<n> serialize_ms=<n> body_bytes=<n> ms=<n>` per `/__search` request | issue-230 unified full-text search (embedded SQLite FTS5, `search_index.rs`). The background indexer keeps the current project's content in an FTS5 index across buckets — `bucket=claude` and `bucket=codex` (session transcripts, every pass), `bucket=commits` (`git log`, every pass), `bucket=worklist-history` (`resources/worklist-history/<ts>.json`+`.md`, every pass), `bucket=issues` (`gh` via the forge adapter, every ~6th pass for rate limits) — stored as a rebuildable cache at `<app_cache_dir>/search-index/<project-key>.db`. Each row keys on a generic doc key (session path, `commit:<sha>`, `issue:<number>`, `history:<ts>`) with a change token (mtime / immutable / `updatedAt`) so unchanged docs skip. `op=scan` separates filesystem discovery and extraction from SQLite gating/writes; `op=query` separates SQLite open/query from result enrichment and JSON serialization. Trace payloads contain only counts, durations, byte sizes, query character count, and allowlisted facet names — never query text, snippets, paths, titles, or indexed content. |
| `search-render` | `__bramMeasureTurnsRender` in `helpers.js` (called from `SessionDetail`'s `/__turns` onLoaded) | `op=turns turns=<n> ms=<paint-delta>` | issue-230 session-transcript render cost. A double-`requestAnimationFrame` after the turns load measures data-ready → next-paint (captures through a synchronous render freeze, like `menu-paint`), logging render-to-paint ms against turn count. One line per session expand. Used to size the `Items`→virtualized-`List` win (host projection is ~0.5s for 1514 turns via `turns-projection`; this is the client-render half). |
| `commit-find-scroll` / `diff-find-scroll` | `__bramCommitScrollToActive` / `__bramDiffScrollToActive` in `helpers.js` | outer: `row`, `cursor`, `total`, `hasRef`; inner: `rows`, `active`, `row`, `hasRef` | Soak trace for the find-in-diff nav (search-index-commit-diffs iterate): one line per ▲▼ step at each scroll stage — outer (CommitDetail's block list) and inner (DiffView's line list). A step that doesn't land names its failure: no line = the ChangeListener never fired; `row=-1` = the match walker missed; `hasRef=false` = the List ref wasn't up when the step ran. Remove once the nav mechanics are trusted. |
| `search-date` | ChangeListeners and the date `Slider`'s `onDidChange` in `Search.xmlui` | `op=drag` (`lo`, `hi`), `op=filter-reset` (`settled`, `lo`, `hi`), `op=buckets` (`s`, `c`, `i`, `h`, `settled` — per-bucket `inProgress` flap), `op=displayed` (`count`, `filtered`, `lo`, `hi`), `op=pin-top` (`via=buckets` — bucket-toggle pin, `SearchResults.xmlui`; `via=filter-or-query` — debounced pin after a date-filter or query change, `Search.xmlui`; both via `window.scrollAllToTop`, not `List.scrollToTop`, whose outside-scroll `startMargin` goes stale and lands short). Epoch seconds and counts only — never query text. | search-date-filter-forensics (observe-only): built for the 2026-08-14 iterate on compact-search-header-and-hit-badges ("max thumb does not constrain"). The soak's verdict, same day: the drag → filter → displayed pipeline was correct (58→6 rows, zero resets); the visible falsehood was the endpoint labels, frozen at a transient first-settle domain by an inline `dateFilter ? … : dateRange[…]` ternary that defeated binding dependency tracking (fixed by extracting to `dateLoLabel`/`dateHiLabel` vars). A companion `search-expand op=auto` subkind existed for one soak cycle and convicted auto-expand of firing on every settle transition; auto-expand was then removed outright, and the subkind with it. The grep pattern stays useful: `drag` followed by `filter-reset` + a `buckets` flap = a refetch clearing the filter; `drag` with `displayed filtered=false` = the didChange path; `drag` with `filtered=true` but unchanged `count` = the comparison. Remove once the filter mechanics are trusted. |
| `ai-describe` (host) | `handle_describe_command` in `lib.rs` | `op=call` (`ms`, `model`, `input_tokens`, `output_tokens`, `upgraded`, `ctx`, `result`, `redactions`, `id`); `op=hit` (`id`); `op=skip` (`reason=disabled\|no-key`); `op=error` (`status`, `ms`, `detail`) | One line per `/__describe-command` request — Haiku intent-header synthesis for tool expansions. Default off: requires explicit project `ai.describeCommands: true` plus `ANTHROPIC_API_KEY`. Command/diff/write/access material, context, result excerpts, and existing descriptions are redacted before request construction. `op=call` carries latency, token counts, and the count of masked spans, never prompt content. Explicit opt-in is the security boundary because heuristic redaction cannot guarantee arbitrary content is secret-free. |
