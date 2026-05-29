# Working with Bram

Bram is a **workspace for AI-assisted web app development** — it
works with any project that serves a web UI (vanilla HTML/JS, a
React or other Node app, a Python web app, an XMLUI app, etc.).
The shell puts a real terminal alongside the app you're building,
plus an "agent tools" drawer that includes a Worklist (pending
items + commits), a Sessions browser, and a Context viewer
(CLAUDE.md + memory + hooks + settings, searchable). The user
sees the right pane while talking to you — use it.

> Note on memory: this file is loaded into every session in this
> project via a `@`-import in `CLAUDE.md`. **Don't save project-related
> memories** — preferring the worklist, helper APIs, release quirks,
> conventions you discover, etc. Per-user memory is private to one
> agent on one machine; this file is shared with everyone running
> Bram. When you learn something worth keeping for future
> sessions, add it here so the whole community gets it. Memory stays
> reserved for things that genuinely can't live in the project repo
> (cross-project user preferences, etc.).

## Naming and user-facing copy

- **Don't call Bram an IDE** in user-facing copy (README, UI
  strings, manual.md). Frame it as a workspace, desktop shell, or
  describe what it does. Don't recommend external IDE tooling
  (rust-analyzer, VS Code extensions) in this project's docs —
  Bram is the workspace.
- **Don't call this repo a "dogfood project"** or use similar
  internal-team jargon in committed text. It's the Bram
  project; users developing their own XMLUI app launch Bram
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

Recommended proposal layout separates review prose from metadata:

```text
resources/worklist.json
resources/worklist-drafts/<id>.md
```

For a new proposal, first write
`resources/worklist-drafts/<id>.md`:

```markdown
# Before

what's there now, relevant context, rejected alternatives

# After

what you'll change it to
```

Then add a metadata-only item to `resources/worklist.json`:

```json
{
  "id": "kebab-case-id",
  "status": "proposed",
  "files": ["path/to/file.xmlui"]
}
```

The server merges the draft prose into `/__worklist` and
`/__worklist/resolve`, so the Worklist tab and approval flow render the
same `before` / `after` content as inline items. Hashes are computed
from the combined metadata plus resolved prose. If the draft file is
missing, `/__worklist` returns empty `before` / `after` plus
`"_draftMissing": true` so the UI can show an explicit placeholder.
Inline `before` / `after` in `worklist.json` remains valid and takes
precedence when both are non-empty; use inline prose only when it is
more convenient or for compatibility with older items.

For items that span 2+ files, use `files: ["path/a", "path/b"]`
instead of `file`. The TO COMMIT inline diff renders all listed
files concatenated, so the reviewer sees the full scope of the
change. `file` (singular) stays valid for single-file items.

Optional `closesIssues: [{number: <int>, title: <string>}, ...]` on
an item declares that the commit will resolve those GitHub issues.
Each entry carries the issue number and its current title, which the
Worklist tab shows in the close-on-commit confirm dialog (see
*Close-on-commit confirm dialog* below). Set it conservatively — only
when the commit truly closes the issue, not when it merely
cross-references it (`see #N`, `related to #N`, partial work on a
multi-step issue). Omit or use an empty array to skip the dialog.

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

**Don't nudge the user toward commit approval.** A TO COMMIT item
sits in the working tree indefinitely until an `approved:` payload
covering it arrives. Both unilateral framing and chat solicitation
push the authorization onto something other than the deliberate
Approve click and cheapen it:

- Unilateral framing: "Let me commit X", "I'll commit X then propose
  Y", "going to land this now". Frame the proposal instead — "Approve
  X to commit" or "Once you approve, I'll commit X".
- Chat solicitation: "Want me to commit it now?", "Should I commit?",
  "Ready to commit?". The structured Approve button IS the channel.

Describe the TO COMMIT state factually ("relay is TO COMMIT —
confidence high on happy path, untested edges noted above") and stop
there. The user reads it and clicks Approve when ready, or doesn't.

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
the file is missing, Bram serves an empty default. The
Worklist tab can create `resources/worklist.json` (and the enclosing
`resources/` folder if needed) the first time you opt into the
worklist flow.

Lifecycle:

1. **Propose** — write draft prose to
   `resources/worklist-drafts/<id>.md`, then write a metadata item to
   `resources/worklist.json`. Each item should be small, discrete, and
   independently rejectable. Writing items to the file does **not**
   mean they are approved — it means you are *asking* the user to
   approve them. Inline `before` / `after` fields in
   `worklist.json` are still accepted, but draft files are the
   preferred path because iterate-time prose edits stay small.
   After proposing, tell the user to click **Approve** or **Drop** in
   the Worklist tab. Do **not** show or instruct on raw `approved:`,
   `drop:`, or `iterate:` payloads. The tab's buttons generate the
   verified `{id, hash, feedback}` shape; hand-typed payloads are easy
   to get wrong and may fail hash verification.
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
     loopback HTTP server.** Bram writes its bound port at startup to
     `resources/.bram-port` (plain decimal, no newline). Read that
     file once via your `Read` tool at the start of any
     worklist-handling work and substitute the literal number into
     your curl calls:

     ```
     curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1 \
       "http://127.0.0.1:61455/__worklist/resolve"
     ```

     (replace `61455` with whatever `Read resources/.bram-port`
     returned). The literal port matches the
     127.0.0.1 worklist allowlist in `.claude/settings.json` and runs
     without a prompt. Use `-4` and `127.0.0.1`, not `localhost`: Bram
     binds the loopback server on IPv4, while `localhost` may try IPv6
     `::1` first and fail with `curl: (7)` even when Bram is listening.
     Use `-sS` (silence progress, KEEP errors) rather than bare `-s` —
     `-s` swallows `Failed to connect` and other curl errors, so a
     restart-race against a stale port shows up as `(no output)`
     instead of `curl: (7)` and makes the next failure mode hard to
     diagnose.

     Bram publishes `resources/.bram-port` only after the loopback
     accept loop answers an HTTP readiness probe, and also writes
     `resources/.bram-port.json` with the same port plus pid, project
     root, and startup timestamp. If the port keeps refusing after
     repeated fresh reads of `.bram-port`, treat it as a stale-port or
     restarting-server diagnostic, not as a reason to continue without
     the lifecycle call. Check the Status tab's **Port file** row; it
     compares the running process, the plain port file, and the metadata
     sidecar.

     **Codex uses the filesystem channel, not loopback curl.** Codex's
     sandbox refuses loopback connections (`curl: (7)` even when Bram is
     listening on the exact IPv4 port — issue #130), so all the curl
     guidance above is the **Claude** path. Codex drives the identical
     lifecycle by writing `resources/.worklist-intent.json` and reading
     `resources/.worklist-result.json` — see *Codex filesystem lifecycle
     channel* below. Both transports dispatch through the same host-side
     handlers, so response kinds, consume-on-read, the inflight sentinel,
     and the auth checks are identical regardless of which an agent uses.

     Use the literal port number — `$BRAM_PORT` won't work because
     Claude Code's permission matcher doesn't expand variables before
     matching, so any `$` makes the allowlist fail (see
     https://code.claude.com/docs/en/permissions.md).

     If `resources/.bram-port` is missing (the rare case where the
     agent was launched outside the wrapped PTY shell), fall back to
     `lsof -nP -iTCP -sTCP:LISTEN | grep bram`. The response is one
     of:
     - `{"kind":"approved", "items":[<full verified content>], ...}` —
       execute these items. The user has already triaged; do NOT
       re-read `resources/worklist.json` to second-guess what was
       approved. Approved records are **consumed on the first read** —
       a second GET for the same turn returns `no_active_authorization`
       (see below), so capture what you need on the first call.
       After you edit the approved project files, mechanically advance
       those ids with `POST /__worklist/mutate` instead of rewriting
       `"status": "applied"` directly in `resources/worklist.json`.
     - `{"kind":"rejected_stale", "mismatched_ids":[...]}` — the
       worklist file changed between the user's click and the watcher
       reading it. Do not edit files; surface the staleness to the
       user and ask them to re-triage.
     - `{"kind":"no_active_authorization", "consumedAtMs":<ts>}` —
       the prior authorization record has already been consumed (or
       this turn isn't an authorization turn at all and the resolver
       is returning a previously-consumed record). **Do NOT treat
       this as authorization.** No items to act on, no file edits.
       This is the architectural backstop for the rule above that
       `iterate:` and other non-authorization turns must not route
       through `/__worklist/resolve` — if you forget and call it
       anyway, you'll land here instead of getting stale approval
       data back.
     Respond to the optional feedback in either case. Never parse the
     `approved:` turn line yourself for content — the line carries
     only ids and hashes.
   - *Drop selected (N)* — only enabled when ≥1 item is checked.
     You receive `drop: {"items":[{"id":"...","hash":"...","feedback":"..."}, ...]}`.
     Same shape, same `/__worklist/resolve` flow:
     `{"kind":"drop"}` → prune those ids via
     `POST /__worklist/mutate` without acting on them;
     `{"kind":"rejected_stale"}` → surface the staleness, do not
     edit. Respond to any per-item feedback (often the user's reason
     for dropping that specific item).
   - *Iterate (N)* — enabled when an item is selected AND its
     per-item feedback box has non-empty content (no-direction
     Iterate is meaningless and the button reflects that). You
     receive `iterate: {"items":[{"id":"...","hash":"...","feedback":"..."}, ...]}`.
     **Unlike approved/drop, iterate does NOT route through
     `/__worklist/resolve`** — no worklist state change is being
     authorized, so the watcher doesn't write an auth record for it.
     Re-read items from `/__worklist` when you need resolved draft
     prose, or from `resources/worklist.json` when metadata alone is
     enough, and act per each item's current status, scoped by that
     item's own feedback:
     - **`proposed` (TO APPLY):** revise the item's `before` /
       `after` prose in `resources/worklist-drafts/<id>.md` when the
       item uses a draft file; otherwise revise inline `before` /
       `after` in `resources/worklist.json`. Update `files` in
       `worklist.json` only when scope changes. Item stays
       `proposed`. No project file edits on disk.
     - **`applied` (TO COMMIT):** edit the on-disk files per the
       feedback AND update the draft file or inline `after` (and
       `files` if scope expanded) to reflect the new scope. Item stays
       `applied`.
     Iterate is the channel for "refine in place" — the user wants
     to keep working these items without yet approving or dropping.
     After iterating, the items are ready for the user to re-triage
     on the next click.

     **Iterate cycles must bracket with `POST /__iterate/begin`
     (first action of your response) and `POST /__iterate/end` (last
     action before your turn ends), both with body `{"ids":[...]}`
     carrying the iterate ids. The host also clears the sentinel at
     turn-end as a safety net, so a missed `end` won't strand the
     spinner. See *Host-managed inflight sentinel* below for the
     mechanism (refs #84, #91).
3. **Mechanical transitions** — use `POST /__worklist/mutate` for
   approval-driven state changes:
   - `{"op":"advance","ids":[...],"status":"applied"}` after an
     approved apply
   - `{"op":"prune","ids":[...]}` after a drop, or after a commit of
     already-`applied` items
   Direct edits to `resources/worklist.json` are for proposal
   metadata authoring and scope refinement; direct edits to
   `resources/worklist-drafts/<id>.md` are for proposal prose
   refinement. Neither direct path is for mechanical prune/advance.
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

### Codex filesystem lifecycle channel

Codex's `workspace-write` sandbox refuses loopback connections — its
`curl` to `http://127.0.0.1:<port>/...` returns `curl: (7)` even when Bram
is listening on that exact IPv4 port (issue #130). Codex has no
loopback-only network allowance, and the only sandbox knob that would fix
it (`network_access = true`) grants *all* outbound network, which we
consider too permissive. So Codex does **not** use the loopback HTTP routes.

Instead Codex drives the lifecycle through two coordination dot-files that
the host watches and drains:

1. **Write** `resources/.worklist-intent.json`:

   ```json
   { "nonce": "<unique-per-request>", "route": "<route>", "body": { ... } }
   ```

   `route` is one of `worklist-resolve`, `worklist-mutate`,
   `iterate-begin`, `iterate-end` (alias `worklist-end`). `body` is the
   exact JSON the matching HTTP route took:
   - `worklist-resolve` — no `body` needed (optional `{ "ids": [...] }` to
     filter, mirroring the `?ids=` query).
   - `worklist-mutate` — `{ "op": "advance", "ids": [...], "status": "applied" }`
     or `{ "op": "prune", "ids": [...] }`.
   - `iterate-begin` / `iterate-end` — `{ "ids": [...] }`.

2. **Read** `resources/.worklist-result.json` and act on the record whose
   `nonce` matches the one you just wrote (ignore any stale result from a
   prior request):

   ```json
   { "nonce": "<echoed>", "ok": true,  "status": 200, "result": { ... }, "completedAtMs": 0 }
   { "nonce": "<echoed>", "ok": false, "status": 400, "error":  { ... }, "completedAtMs": 0 }
   ```

   `result` is byte-for-byte what the HTTP route would have returned (e.g.
   `{ "kind": "approved", "items": [...] }` for resolve). The host writes
   the result within watcher latency (a few ms) and then deletes the intent
   file, so a brief read-retry covers the race. **Do not continue silently**
   on a missing result or `ok: false` — that's the same rule as a refused
   curl on the Claude path.

The host dispatches each intent through the *same* internal functions that
back the HTTP routes, so consume-on-read, the inflight sentinel, the
`kind`-match auth check, and the post-commit-prune safeguard are all
identical. The intent file is only a request envelope — it grants Codex no
authority beyond what the hash-verified `.worklist-authorization.json`
already encodes. The Codex PreToolUse guard exempts
`resources/.worklist-intent.json` from worklist coverage (it's a
coordination file, like the loopback curl was). Trace each drain by
grepping `[worklist-intent]` in `resources/bram-trace.log`.

Claude continues to use the loopback HTTP routes — it has no sandbox
restriction, and the curl allowlist runs without a prompt.

### Close-on-commit confirm dialog

When you propose or iterate a worklist item whose `applied` commit
would resolve a GitHub issue, set
`closesIssues: [{number: N, title: "..."}, ...]` on the item — the
title comes from `gh issue view N --json title` (or from the issue's
data in the Issues tab); keep it current if you iterate. When the user clicks **Approve** on a TO COMMIT item that
carries a non-empty `closesIssues`, the Worklist tab opens a confirm
dialog — one row per issue with a checkbox (default checked) and an
optional close-comment textbox.

For issue-derived items — for example "Propose a worklist item to
address #N (...)" from chat or the Issues tab — default to pairing the
`issue-<N>-...` id with `closesIssues` for that same issue. Omit it
only when the proposed change is explicitly investigative, partial, or
otherwise not intended to resolve the issue. If you later discover an
approved/applied issue-derived item is missing `closesIssues`, iterate
the item metadata before asking for commit approval; do not rely on the
ignored runtime `resources/worklist.json` change being commit-worthy on
its own.

The user's choices arrive back in the per-item `feedback` of the
`approved:` payload as one or more `close-issue:` lines, appended
after any free-text feedback the user typed. Two shapes:

```
close-issue: 52
close-issue: 50 comment: "shipped, see commit message"
```

Per item, after resolving `/__worklist/resolve` and committing as
usual:

1. Split the verified `feedback` on `\n` and pick out lines that
   start with `close-issue: `.
2. For each, run `gh issue close <N>` — with `-c "<comment>"` when
   a `comment: "..."` clause follows the number. The comment string
   is JSON-encoded by the dialog, so `JSON.parse` on the substring
   between the matching quotes gives the literal text.
3. The user's choice is authoritative — if they unchecked an issue
   the agent listed in `closesIssues`, that issue will not appear
   as a `close-issue:` line. Do **not** close issues the user
   didn't confirm, even if `closesIssues` originally listed them.
4. If the user clicks **Approve without closing** instead of
   **Confirm**, the payload arrives with the user's free-text
   feedback only (no `close-issue:` lines). Commit; do not close
   anything.

Detection by regex on `#N` in item prose is **not** part of this
flow — the agent has the conversational context to judge whether a
commit truly resolves an issue, and a regex over `before` / `after`
has false positives on cross-references. Set `closesIssues`
explicitly when warranted; leave it off otherwise.

### Choosing an id

When an item is clearly derived from a single GitHub issue, prefix
the id with `issue-<N>-` where `<N>` is the issue number, followed
by a short descriptive slug. Examples:

- `issue-86-pty-intent-relay`
- `issue-91-defer-sentinel-clear`
- `issue-92-feedback-panel`

Skip the prefix when there's no clean 1:1 issue mapping —
exploratory items, cross-cutting refactors, or items that touch
multiple issues. Use a bare descriptive slug as today
(`worklist-drafts-separate-prose-from-metadata`,
`issues-search-include-comments`).

The prefix complements `closesIssues: [{number: <N>, title: "..."}]`
rather than replacing it: the id signal is for human scanning of
the Worklist tab list / `git log` / chat references, the
`closesIssues` field is for the close-on-commit dialog and the
structured authorization payload. Pair them when both apply.

Existing items keep their names — not retroactive. Renaming would
break commit-message back-references and chat history for marginal
benefit.

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
- **Issue-only `gh` work with no repo diff.** If the user asks you to
  create, edit, comment on, close, or reopen a GitHub issue, and the
  task will not modify tracked files in the repo and will not produce a
  commit, skip the worklist and do it directly. If the issue request is
  paired with repo changes, the repo changes still go through the
  worklist.

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
- "comment on issue 51 that the external MCP config is fixed" → direct
  `gh issue comment` (issue-only work, no repo diff, no commit).
- "add a chart" + "let's fix the layout too" → propose (multi-step,
  but this was always the case).

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
- Verifying CORS headers, environment variables, or other runtime
  configuration.
- Tailing logs to diagnose a bug.
- Browsing GitHub issues without a planned repo change.

…all happen in chat, not as worklist items. They produce no
`before` / `after` because there's nothing to write. They produce
no commit because there's nothing to land. Routing them through
the worklist creates surprise-TO-COMMIT rows with empty diffs, and
the user can't tell whether something genuinely went wrong or the
agent just used the wrong channel.

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

Refs #88.

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

Worklist `before` / `after` prose (inline or in draft files) and
worklist-history `changelog` entries render as Markdown in the
agent-tools drawer, so use real Markdown syntax instead of inline
ad-hoc formatting:

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

**Enforcement layers.** Structured `approved:` / `drop:` payloads land
in `resources/.worklist-authorization.json` (provider-neutral). Claude
and Codex each install PreToolUse hooks that validate worklist coverage
before file-mutating tools run. The desktop watcher is fallback coverage
(reverts unauthorized prunes). If you hit a hook error or revert
message, that's the convention's enforcement mechanism, not a bug to
work around.

**Don't ask before editing the worklist or calling mutate.** The
proposal-authoring write channel is already approved and guarded, and
the mechanical transition channel is the shared server endpoint.
Claude's allow-listed `Write(./resources/worklist.json)` /
`Edit(./resources/worklist.json)` calls are validated by its hook, and
Codex mutations are validated by its matching PreToolUse hook. Either
way, there is no need to verbally confirm with the user before adding
items, refining their prose, or calling `/__worklist/mutate` for an
already-approved advance/prune.
Save the verbal back-and-forth for design decisions (which items to
propose, what choices to bake in), not for the mechanical transition.

**Minimize the bytes of each worklist edit.** Prefer per-item draft
files for new proposals. The first draft write still has to carry the
prose, but later iterate-time prose refinements edit only
`resources/worklist-drafts/<id>.md`; `worklist.json` stays a compact
metadata index. For older inline items:

- Narrow `Edit` targets for the smallest possible string that uniquely
  identifies the change. Appending a paragraph to an item's `after`
  only needs the tail paragraph as the anchor, not the whole item.
- When you're appending to an item's `after`, `old_string` is the last
  paragraph you want to preserve and `new_string` is that same
  paragraph plus the appended content — not the entire `after`.
- Full-item rewrites (`Write` over `worklist.json` from scratch) are
  acceptable for compatibility, but avoid them for one-paragraph
  tweaks. Mechanical prune / advance transitions go through
  `/__worklist/mutate`, not a direct rewrite.

The hook validates the resulting file regardless of edit shape, so the
choice is purely about token economy and transcript noise.

**Don't `grep -n` a single-line JSON file** (like `worklist.json`) to
find an anchor for an `Edit`. The matching "line" *is* the whole
file, and the grep tool result dumps it into the transcript. Find
your anchor by recalling the structure from prior turns, using
`Read` with `offset`/`limit`, or `jq` to extract just what you need.

**Don't update an item's `after` prose on every iterate.** Small
refinements during TO-COMMIT iteration don't need an audit trail in
the worklist — the commit message captures the final state and the
file diff is reviewable in git. Only update the draft or inline
`after` when scope materially expands (new file added to `files`, or
the change's intent shifts). Otherwise leave it; the iteration
history doesn't need to live in the item.

**Use `POST /__worklist/mutate` for mechanical prunes + status
advances.** Two ops: `{"op":"prune","ids":[...]}` after a drop,
`{"op":"advance","ids":[...],"status":"applied"}` after an approved
apply. Server-side auth check requires the matching `kind` in
`.worklist-authorization.json` from the current turn's resolve; same-turn
`resolve → edit files → mutate` is valid because mutate reads the stored
auth record, not just `resolve`'s consumption state. Direct `Edit`s to
`worklist.json` that change `status` or remove items are the wrong
channel — may be blocked by the provider hook. Item content edits
(proposal authoring, iterate prose revisions) still go through
`Write` / `Edit`; only the mechanical mutations route through `mutate`.

Security contract: the structured approval line is not the authority by
itself. The host records it only after recomputing each item's hash, and
stale hashes become `rejected_stale`. `/__worklist/resolve` is the only
way an agent receives verified item bodies; `/__worklist/mutate` is the
only way an agent performs mechanical `advance` / `prune`. `advance`
requires an `approved` auth record covering every id. `prune` requires
`drop`, except the post-commit prune path accepts `approved` only when
the requested ids are already `status: "applied"`. Provider hooks block
direct `worklist.json` status changes and removals as defense in depth.

## Host-managed inflight sentinel

The Worklist tab's spinner derives from `resources/.inflight-claim.json`,
written and cleared by host-side HTTP handlers. Spinner is up while the
file claims the targeted item, off when it doesn't. Full route /
file-shape / event reference lives in `docs/apis.md` §11; this section
is the agent-side convention only.

### What the agent calls

- **`approved:` payload** → `GET /__worklist/resolve` (writes the
  sentinel as a side effect; consumes the auth record), do the work,
  `POST /__worklist/mutate op:"advance"`. That successful mutate
  clears the sentinel for the approved ids immediately. The host's
  silence-detected turn-end remains a fallback if a cycle still needs
  to drain after the state transition, so no explicit `/__worklist/end`
  is required.
- **`drop:` payload** → same shape with `op:"prune"` instead.
- **`iterate:` payload** → bracket your response with
  `POST /__iterate/begin` (writes the sentinel) as the first action
  and `POST /__iterate/end` (clears it) as the last. Required because
  iterate has no side-effect write path equivalent to `resolve`.

### Failure modes

A stuck spinner is the convention's enforcement mechanism; there's no
live-session timeout. Most commonly:

- **Approved/drop stuck:** `/__worklist/mutate` was never called for
  those ids, or it errored before the clear landed. Recovery: call
  mutate manually, or restart Bram (startup `cleanup_stale_inflight_claim`
  deletes leftover sentinels).
- **Iterate stuck:** `/__iterate/end` was never called. Convention
  violation. Bracket every iterate response — `begin` first, `end`
  last — regardless of how trivial the body.
- **Premature clear:** should be structurally impossible post-#84.
  If observed, grep `[inflight-sentinel]` in `bram-trace.log` to
  trace the clear source.

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
| `logToHost(payload)` | log to Bram stderr without bothering you |

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

## Don't quote unpushed-commit counts in chat

After a commit lands, confirm with its short SHA and subject and stop.
Don't say "N unpushed commits now" or list unpushed SHAs in prose — the
Commits tab has the exact count and list; any number you'd state is
guesswork.

## Push button auto-rebases on non-fast-forward

The Commits-tab Push button does `git push`; if rejected as
non-fast-forward, it fetches `origin` and rebases on `origin/<branch>`
before retrying (linear history, no merge commits). Don't manually
`git pull --rebase` — that's the button's job. Only intervene when
the button reports rebase conflicts (working tree left clean); then
start a manual rebase, resolve, and push.

## Updating GitHub issues via gh

Use `gh` directly — the Issues tab polls every 30s, so updates surface
without a restart:

- `gh issue edit <n> --title "…" --body "…"`
- `gh issue comment <n> --body "…"`
- `gh issue close <n>` / `gh issue reopen <n>`

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

The Bram binary ships with the `app/` tree embedded at build
time (Tauri's `frontendDist: "../app"`). At runtime, Bram
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


## Commit messages

Summarize the worklist item that drove the commit. Use
multiline. Reference the driving issue if there is on.


## Debugging xmlui apps with traces

Two forensics sources, picked by symptom:

**`resources/bram-trace.log`** — host-side rolling log of HTTP
routes, iframe events, and inflight-sentinel writes / clears.
Always on; grep it directly. Best for plumbing issues: stuck
spinner, sentinel anomalies, route errors, agent-turn-end
detection, heartbeat drift.

**Inspector Export** — XMLUI runtime trace (events, state changes,
handler invocations), captured on demand. Best for in-pane
misbehavior: a button doesn't fire, a DataSource shows wrong data,
a state change doesn't propagate, a component renders wrong. Ask
the user to open the Inspector (magnifying-glass icon), reproduce,
and click **Export** — writes `~/Downloads/xs-trace-<timestamp>.json`.
Analyze with the xmlui MCP tools (don't read the raw JSON, it's
huge):

- **`xmlui_find_trace`** — locate the export by timestamp or content.
- **`xmlui_distill_trace`** — reduce to interactions / state changes
  / handler boundaries relevant to a specific question.

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
