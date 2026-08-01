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
the sentinel. **Issue close is automatic — the agent does nothing.** When the
approved feedback carries `close-issue:` selections (from the close-on-commit
dialog), the host records them at commit time bound to the new SHA, then
closes each issue automatically after the user's next explicit **Push** (only
once its commit is visible on origin). There is no agent-reachable close
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
Approving a TO COMMIT item with non-empty `closesIssues` opens a
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
explicit **Push**, once each commit is visible on origin, the host closes
its issue automatically with the `Closed by <commit-url>` comment (prefixed
with the user's comment when one was given).

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
Opt-in through **Settings → Traces** unless `BRAM_TRACE` explicitly overrides
the project setting; grep it directly when enabled. PTY previews and serialized
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
known close timestamp means the commit wasn't visible on origin yet, or
no close was queued at the commit gate).

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
| `describe-patch` | `__bramFlushDescribePatches` in `helpers.js` | `stage` (`begin` \| `end` \| `settle`), `patches` (applied), `queued`, `provider`, `turns`, `ms`; on `settle` also `settleMs` (broadcast to second-next-paint, the fan-out render cost the sync `ms:0` flush conceals) and `sinceBeginMs` | Brackets the full-projection rebroadcast that splices Haiku "Tool Descriptions" results (`ai.describeCommands`) into the transcript. Since describe-rebroadcast-coalesce (2026-07-22 perf audit: 524 per-result rebroadcasts degraded heartbeat drift to 4.1s max and a tab-switch subscribe refetch to 3.1s), completions ENQUEUE and one flush per ~400ms window broadcasts them all — the bracket measures the flush, its `patches` field the batch size. Emitted synchronously before/after `__bramBroadcastProjectedTurns` (via `logToHost` → `invoke`, whose IPC dispatch survives an iframe main-thread freeze), so a hard freeze in the re-render is self-diagnosing: a `stage:begin` with **no matching `stage:end`** names the broadcast as the freeze and quantifies it (`turns`, `resultChars`). Added for the 2026-07-11 describe-freeze recurrence on a large Codex session (82 turns / 1 MB); unlike `long-task`, which logs at recovery and goes silent on a terminal freeze. |
| `describe-load` | describe issuance counters in `helpers.js` (`__bramRequestCommandDescription`) | `issued_1s`, `inflight`, `requested_total`, `held` (requests queued by the typing hold), `window_ms` (active flush coalesce window) | describe-backfill-observability: one coalesced line per second while Tool-Description requests are issued or in flight — the backfill pressure curve with a denominator. Boot of a large session fires a burst (2026-07-30: ~375 calls in 4 min, peak ~150/min, saturating the main thread during typing); this subkind makes that storm and its drain directly visible instead of reconstructible-by-correlation. Host-side twin: `[ai-describe] op=call` carries `concurrent=` (active requests at call entry). Pacing (describe-backfill-pacing, graduated from the 22.7%-settle-churn storm): new requests hold while the user typed within 2s (`held`), and the patch-flush window widens 400ms→2s while backfill is active (`window_ms`). |
| `projection-broadcast` | `__bramBroadcastProjectedTurns` in `helpers.js` | `reason` (`describe-flush` \| `refetch:<trigger>` \| `unknown`), `route` (active hash), `subscribers`, `turns`, `ms` (sync fan-out), `tail_emits`/`tail_skips` (cumulative decisions of the tail-scoped last-exchange source — workspace-tail-subscription; skips are broadcasts whose exchange content was unchanged, i.e. storm broadcasts the Worklist page no longer pays for); a second `stage:settle` line carries `settleMs` (double-rAF render settle) | projection-broadcast-attribution (round 2 of the 2026-07-30 boot-latency work): every projection broadcast names its trigger, the active tab, and both halves of its cost. Motivating finding: ~195ms settles on `#/worklist` — a page rendering no transcript rows — because Workspace subscribes to the projection (`Workspace.xmlui` PushSource). The graduation grep: which routes/reasons account for the non-describe 389–562ms long-tasks. |
| `follow-state` | `__bramFollowTransition` / `__bramFollowVerify` in `helpers.js`; call sites across `Transcript.xmlui` | `op=transition` (`to` bool, `cause`: `user-scroll-bottom` \| `user-scroll-up` \| `find-step` \| `tool-expand` \| `footer-arrow-up`/`-down` \| `mount-restore-reading` \| `agent-chip-switch` \| `unseen-jump` (chip-recruited arrival from another tab), `route`, `agentId`); `op=verify` \| `op=violation` (`cause`: `footer-arrow-down` \| `content-append-repin` \| `settle-repin` \| `mount-pin` \| `subagent-switch-repin`, `landed`, `endIndex`); `op=echo-suppressed` (`to`, `cause` = the echo-window opener (a transition cause or `mount`) or `uncorroborated`, `inputAgeMs`, `agentId`); `op=unseen-clear` (`count` of unseen turns cleared, `cause` = the FOLLOWING-entering transition, `route` — transcript-new-below-badge: the footer status-line chip's recruitment log); `op=repin-blocked` (`varAtBottom`, `sinceReadingMs`, `agentId` — follow-state-source-of-truth: a repin the stale xs var would have misfired, blocked by the synchronous window truth; the 2026-08-01 240ms-into-READING yank class made countable) | transcript-follow-contract layer 1: the Transcript's follow/reading contract (authoritative text in Transcript.xmlui's header) made self-reporting. Every state flip logs its cause; every bottom-promise is measured against the List's own visibleRange after a double-rAF, and a miss logs a violation — the six-fix whack-a-mole lineage (9daa693/f846258/6a892f6/652d9b3/410d069/c4c14ed) becomes a named, greppable inventory. Violations are leads, not convictions: an append landing inside the verify window can log a violation the next repin heals — corroborate with surrounding lines. Layer 2 (transcript-follow-echo-guard) graduated from this log's soak (1,056 bottom-promises, zero violations; phantom user-scroll flips within 200-400ms of every tool-expand): deliberate transitions and mount open a one-shot echo window (`__bramFollowEchoOpen`, 700ms / 1500ms mount), and `__bramFollowClassify` attributes a scroll event to the user only when no window is open AND user input corroborates it (wheel / keydown / held-or-recent pointer within 400ms; passive capture listeners). Input corroboration was added after the first live soak caught an expand echo re-arming FOLLOWING 2.7s post-click — machine scrolls have no input beside them at any latency, so no fixed window suffices. Suppressions log `op=echo-suppressed` with `inputAgeMs`, no state flip. A suppression of a genuine user scroll would surface as a missing expected transition next to an `echo-suppressed` line. Replaces the old `transcript-follow` onScroll trace. |
| `describe-scan` | `__bramEagerDescribe` / `__bramPumpDescribeQueue` in `helpers.js` | `op=scan` (`ms`, `turns`, `entries` scanned, `queued`, `route`); `op=pump` (`ms`, `via` `range` \| `dom-scrape`, `visible`, `queue`, `route`; emitted only when the queue had work) | Round 3 of the boot-latency work (eager-describe-scan-instrumentation): the eager-describe queue rebuild walks all turns × entries inside every broadcast, and the pump's visibility re-partition falls back to a `[data-index]` DOM scrape on non-Transcript pages (also riding the 1.5s drain interval). Suspected for the 17.5s zero-subscriber settle (2026-07-30 22:49 worklist boot). Graduation candidates pre-agreed: incremental queue maintenance from deltas; gate the visibility scrape on `__bramTranscriptMounted`. |
| `projection-subscriber` | `notify()` in `bramSubscribeProjectedTurns`, `helpers.js` | `idx`, `ms`, `total` | Companion to `projection-broadcast`: times each PushSource emit's sync slice, logging any ≥50ms — a consumer named directly. React may defer the real render to the settle half; a silent subscriber log with a fat `settleMs` means the cost lives in the deferred render, not the emit. |
| `input-latency` | input responsiveness probe in `helpers.js` (keydown/pointerdown, rAF-measured, ≥100ms — floor dropped from 200ms for the describe-backfill-pacing soak; the felt band lives at 50-200ms) | `event`, `latencyMs`, `hadFocus`, `target`, `route` (active tab hash — the per-tab felt map); `describeInflight`, `lastLongTaskMs`, `lastLongTaskName`, `lastLongTaskAgoMs` (blockedBy attribution, describe-backfill-observability) | One line per slow input event: rAF delta ≥200ms from dispatch. The blockedBy fields are written at emit time — how many describe fetches were in flight and the most recent `long-task` within 1.5s — so a slow keystroke names its suspect directly (the 2026-07-30 storm needed three-subkind timestamp correlation to convict). |
| `refetch-called` | Workspace.xmlui debounce after `talk-session-changed` | `context`, `correlation_id`, `at_host_ms`, `delta_to_emit_ms` (host emit minus refetch-fire time, so it includes the 400 ms debounce coalesce) | Post-debounce refetch tick. A `delta_to_emit_ms` far above 400 ms means the iframe main thread was busy between emit and refetch. |
| `inspector-tap-tick` | `__inspectorTapTick` in `helpers.js` | `batch` (entries forwarded this tick), `available` (entries ready), `ms` (loop wall time) | Per-non-empty tick of the Inspector tap poller. Empty ticks are silent so this is a slow-tick alarm: a tick with `ms` ≫ 200 (the tick interval) means the IPC channel is backed up while the poller serializes entries through `logToHost`. Pairs with `inspector-event` / `inspector-overflow`. |
| `click` | UI Button onClick handlers (Workspace) | `target` (`approve` \| `drop` \| `iterate`), `item` | Worklist tab user actions. |
| `queue` | queue mutation helpers in `helpers.js` (`__bramQueueAdd` / `__bramQueueUpdate` / `__bramQueueRemove` / `__bramQueueReorder` / `__bramQueueSend`) | `op` (`add` \| `update` \| `delete` \| `reorder` \| `send`), `id`, `chars`; `count` on `reorder`; `mode` (`message` \| `iterate`) on `send` | Queue-tab (`AgentMessageQueue`) mutation audit (queue-mutation-trace). `chars` is the note's text **length, never its content** (queue prose is user-authored, kept secret-safe like the describe redaction). A `send` logs `op=send` only — the internal removal suppresses its `op=delete` — so `delete` marks a user Delete, the recoverability signal for a mistaken removal. |
| `skill-invoke` | `__bramRunSkill` in `helpers.js` (Skills launcher) | `name`, `args_len` | One line per project-skill launch from the agent-pane Skills control (issue-221-skill-launcher). The turn itself rides `toTurn` (`/name args`) / `toShell` for Edit-first; this records which skill ran and its argument length. |
| `inflight-set` / `inflight-clear` | Workspace selectors + `inflightClaim` DataSource | `item`, `via`, `target`, `reason` | Inflight sentinel transitions; complements the host-side `[inflight-sentinel]` log entries. |
| `voice-input` | Worklist voice input path in `Globals.xs` | `stage` (`start` \| `recording-started` \| `stop` \| `append`), `target`, `requestId`, `stopAtMs`, `stopToResultMs`, `stopToAppendMs`, `parentStopToDeliverMs` | End-to-end voice latency for iframe-driven dictation. `stopToAppendMs` on `stage:append` measures Stop Record click to text insertion in the XMLUI input, useful for Mac/Windows comparisons. |
| `inspector-event` | `__inspectorTapTick` in `helpers.js` | `entry` (sanitized `window._xsLogs` record) | Per-entry forwarding of the XMLUI Inspector log into `bram-trace.log` so Inspector events interleave with host traces live (#181). Opt-in via the **Traces → Inspector trace tap** switch in Settings (persisted as `traces.inspectorTap` in `.bram.json`). `__bramTraceSafeValue` truncates deep/large values and masks secret-shaped keys and known credential patterns before IPC; the host redacts the serialized payload again before persistence. Inspector traces remain intentionally complete — every keystroke, render, state change — so volume is high and heuristic redaction cannot prove arbitrary values safe. |
| `inspector-overflow` | `__inspectorTapTick` in `helpers.js` | `dropped`, `totalSeen` | Per-tick (200 ms) cap of 50 forwarded entries was exceeded; high-water mark advanced to current length and the listed count was dropped. Persistent overflow means cadence or cap needs tuning. |
| `turns-projection` (host) | `read_projected_turns` / `try_incremental_projected_turns` in `lib.rs` | `op=rebuild` (`src_bytes`, phase ms `read/parse/project/serialize`, `turns`, `window`, `body_bytes`, `total_ms`); `op=incremental` (`suffix_bytes`, `merged_turns`, `ms`) | Projection cost accounting on long sessions: the rebuild-vs-tail-merge ratio and which phase dominates (post-#214 measurement: parsing is ~10% of a rebuild; project/serialize dominate). |
| `reveal-floor` (host) | quiescence observer in the pty-throughput ticker, `lib.rs` | `op=would-reveal` / `op=reveal-suppressed reason=menu-displayed` / `op=reset reason=activity\|turn-changed\|turn-closed`, with `silence_ms`, `gap_p95_ms`, `gaps_n` | Phase-0 observe-only soak for the auto-reveal-terminal predicate ("turn open + byte-silent + no pane menu"). The graduation review greps these: every `would-reveal` must map to a corroborated terminal-needing moment. |
| `esc-scan` (host) | send-ledger escape sweep and soft turn-end poller, `lib.rs` | `op=sweep` (`read_ms`, `total_ms`, `bytes`); `op=soft-turn-end` (`ms`, `bytes`, `waited_ms`) | Times the per-Esc full-session scans. Exonerated the host in the 2026-07-08 wedge hunt (5 ms over a 26 MB session). |
| `pty-menu` `op=surface-gap` (host) | `pty_menu_update` surface point, `lib.rs` | `tool`, `ms` (grid-first-sighting → pane-surface, `-1` if no matching sighting), `suppressor_armed`, `suppressor_age_ms`, `suppressor_tool`, `fp` (option-label fingerprint) | Observe-only instrument for menu-redetect-storm-after-completion facet B: measures the "menu on terminal but not pane" blindness window. A large `ms` with `suppressor_armed=true` (or `suppressor_tool` ≠ `tool`) implicates the unbounded post-dismiss suppressor / byte-pattern tool misinference holding back a genuinely-new menu. Graduation to a fix needs ≥2 captures naming a consistent culprit. |
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
| `send-gate` (host) | `drain_pty_intents` + pty-throughput ticker in `lib.rs` (send-gate-hold-while-menu-open) | `op=hold` (`count`, `reason=menu-present`, `tool`); `op=flush` (`wrote`, `held_ms`); `op=hold-stale` (`held_ms`, `tool`; mirrored to always-on strand-forensics as `op=send-gate-hold-stale`) | Pane sends (`toShell`/`toTurn` intents) held while a permission menu or picker is displayed, instead of pasting into it and stranding (Eric 2026-07-19 21:04:44, `menu_at_inject=true`). `sendKeys` always passes — pane menu answers are what release the hold. AskUserQuestion never holds (typing over an open question is a legitimate answer path). Release is **evidence-based only**: the pty-throughput ticker flushes held intents when `pending_menu` clears. There is no operational timeout — force-flushing into a still-open menu would recreate the strand and could keystroke-answer a permission prompt. `op=hold-stale` fires once per hold at 120s as a diagnostic: a real menu sitting unanswered that long is user-visible; a ghost menu holding sends is a menu-eviction bug to fix at the source. (First live ghost caught 2026-08-01 — a user-answered dismissal's state clear was deferred behind hook ownership and an interrupt swallowed the hook's own clear, holding sends 7 minutes; fixed by ghost-menu-send-gate-eviction: user-input and outcome dismissals clear unconditionally, and an outcome clear that finds the raw cache empty sweeps a lingering pending flag, logging `[send-gate] op=ghost-cleared`.) |
| `hook-menu` (host) | permission-hook handlers and grid-defer decisions, `lib.rs` | `op=permission/payload/hook-diff/clear/retire-suppressor/grid-deferred/grid-emit-deferred/grid-emit-allowed`; parallel-menu-claim-queue: `op=claim-queue-add` (`id_key`, `resolved_id=<toolu_…|from-event|none|ambiguous|no-session|no-signature>`, `depth`; claim-id-resolution adopts the transcript's tool_use_id when the signature matches exactly one unresolved call), `op=claim-queue-remove` (`reason=hook-clear\|jsonl-resolved`, `id`, `removed`, `displayed_removed`, `depth`; `jsonl-resolved` is the synthesized terminal event for calls whose tool_result landed without a PostToolUse — e.g. failed commands), `op=claim-queue-select` (`joined=labels\|fallback\|signature\|tool-join\|unjoined\|grid-rescue\|none`, `cause`, `depth`; `tool-join` = the unambiguous candidate claim promoted onto the grid's live menu when no label/signature join succeeded — full claim identity (tool, tool_use_id, keyed clears), grid options; `unjoined` = the lone queued claim shown though it didn't join, `grid-rescue` = the grid menu surfaced directly because no claim matched — all display the grid's own options as ground truth since `menus.parseAndDisplay` is off by default and `pty_menu_update` won't surface them); `op=claim-labels-adopted` (`joined`, `tool`; adopt-grid-labels-on-join — the grid's own option phrasing replaced the hook-synthesized labels on a corroborated display, since the terminal text is what the keystroke actually grants), `op=grid-menu-without-claim` (`tool`, `sig_present` — a solved, benign race, not a mystery: the grid report was scored 14-80ms before the hook claim's enqueue landed (2026-08-01 measurements); the post-add coalesce/select pass re-joins within ~100ms); `op=grid-rescue-bare` (`reason=no-claim\|ambiguous\|answered-claim\|picker`, `depth`) — no claim could be promoted to a tool-join, the rescue rendered options-only (`answered-claim` = the lone candidate was the just-answered claim, ghost guard; the transient `op=grid-rescue-enriched` borrow from f46e8e2 was superseded by tool-join before ever firing enriched) | Hook-primary menu coordination: hook claims and their payloads, diff enrichment (`hook-diff cluster=N`), fence-suppressor retirement, and whether the grid deferred or emitted for a hook-owned prompt. The menu-miss retrospective greps these. Since parallel-menu-claim-queue, claims are a queue keyed by `tool_use_id` and the pane displays the claim whose option labels JOIN the grid's current display (the terminal arbitrates; a pane answer keystrokes the prompt it shows, by construction). `claim-queue-select joined=none` with depth>0 = queued claims none of which match the terminal — the grid path surfaces instead. `grid-menu-without-claim` marks the benign grid-before-claim enqueue race; the post-add selection pass self-heals it. Keyed clears (`claim-queue-remove`) never blank a different prompt's display; unkeyed clears (Codex PTY cancel, legacy hooks) drain the queue. |
| `subagents` (host) | `st_report_orphan_subagents` in `lib.rs` (subagent-discovery-workflow-dirs) | `op=orphan` (`agent_id`, `tool_use_id`, `cc_version`) | Coverage-gap instrument for subagent-transcript discovery. The main transcript names every dispatched agent (task-notification turns); an agent referenced there whose transcript the bounded-depth walk of `<sid>/subagents/**` cannot find means a Claude Code layout change or cleanup Bram can't see through. One line per (session, agent), stamped with the session's CC version so the layout shift correlates with the release that introduced it in one grep — the 2026-07-20 Workflow-dirs miss (`subagents/workflows/<wf_id>/`, CC 2.1.205) rendered a silently empty pane that read as "tracking is gone"; the next one names itself. |
| `session-rotation` (host) | `emit_talk_session_changed_for_provider` in `lib.rs` | `op=detected provider=<p> old=<sid> new=<sid> silence_ms=<ms> tail="<pty snippet>"` | One line per genuine session rotation (the active provider's sid changed — provider-keyed, so a Claude↔Codex switch is excluded). `tail` is the last ~400 ANSI-stripped chars of the PTY tail at rotation time, so the rotation names its own cause: a usage-limit banner, a fresh Claude launch banner, a shell prompt (the CLI exited), or a `/clear`. `silence_ms` is the gap since the last PTY output (a long gap points at an idle/limit wait before a fresh relaunch). Diagnose-only; the child PID is unavailable (the PTY child handle is dropped, and the agent CLI relaunches inside the same PTY shell). Added to find why Claude Code keeps restarting into fresh sessions (session-rotation-self-diagnose). |
| `jsonl-turn-end` (host) | Claude/Codex JSONL completion detector, `lib.rs` | `op=scan/enter/skip/poll-handoff` with `tailTypes`, `decision`, `detected`; `op=would-end reason=user-after-assistant path=<basename>` | Turn-completion forensics: whether the JSONL tail marks the turn final. `would-end` is the observe-only phase of jsonl-turn-end-user-after-permission: emitted when a genuine user message (not a tool_result) trails a non-final assistant record — the interrupt/takeover signature where a pending permission menu should clear but the detector reports `non-final-assistant` and pins it. No behavior change yet; graduation to ending the turn needs the soak to confirm every fire is a real interrupt and zero fire on `assistant(tool_use) → user(tool_result)` mid-turn tails. The 2026-07-20 audit of the first soak (33 fires) failed 12: 9 were `isMeta:true` image-companion user records (string content beside a Read-tool image result) and 3 were subagent transcripts (`agent-*.jsonl`). The detector now excludes both classes (graduate-user-after-assistant-clear tuning); the soak baseline resets at that commit — only fires after it count toward graduation. |
| `prompt-lifecycle` (host) | `prompt_shown` / `prompt_resolved` / `record_menu_answer` in `lib.rs` | `op=shown` (`id`, `tool`, `source=hook\|grid`, `tool_use_id`, `labels`); `op=resolved` (`id`, `tool`, `outcome=answered\|resolved\|interrupted\|superseded\|session-ended`, `detail`, `open_ms`); `op=answer` (`id`=tool_use id, `label`, `via=click\|hook-clear\|claim-clear\|jsonl-clear`) — the chosen menu option, recorded immediately when the id is known (`click`), from the singleton fallback used only by claimless/pending menus (`hook-clear`), or from the exact answered claim when its id arrives via PostToolUse/JSONL cleanup (`claim-clear` / `jsonl-clear`); `op=answer-pending` (`reason`, `tool`, `label`) — a claimless/pending-menu label whose id is unknown, stashed for clear-time binding (60s TTL, one slot); `op=answer-miss` (`reason=no-displayed-claim\|no-key-match\|no-tool-use-id`) — an answer that could not be recorded or stashed, with the failing stage named (both grid and hook capture branches trace this); `op=answer-at-click` (`tool`, `id_source=pending-call\|resolved\|none`, `call_present`, `lookup_reason`, `displayed_source`, `shown_age_ms`, `supersede_age_ms`) and `op=answer-deferred-bind` (`gap_ms`, `tool` for the singleton path or `via=claim-clear\|jsonl-clear` for a per-claim record) — diagnostics for perceived "stuck" menus and delayed id binding. Claim-backed parallel answers retain distinct labels and click timestamps until their respective keyed clears; they never compete for the singleton stash. | upstream-prompt-lifecycle-events: Bram's own PromptShown/PromptResolved pair (the upstream-shaped API implemented locally — see `docs/upstream-asks.md` #3), emitted from the existing transition points: hook claim display and grid emit (shown), user-input dismissal (answered), hook/jsonl clears (resolved), Codex PTY cancel (interrupted), labels-join switch or joined=none (superseded), session rotation (session-ended). Exactly one open prompt at a time; one resolved per shown. Bounded history served on `/__prompt-lifecycle`. "What prompts appeared and how did each end" is now one grep. |
| `search-index` (host) | `start_search_indexer` / `run_search_index_pass` and the `__search` route in `lib.rs` | `op=scan bucket=<b> files=<n> indexed=<n> skipped=<n> rows=<n> ms=<n>` per incremental pass; Claude/Codex scans additionally carry `indexed_bytes`, `open_ms`, `discover_ms`, `gate_ms`, `extract_ms`, `write_ms`, `count_ms`; `op=scan-error bucket=<b> detail=<…>`; `op=issues-list-rebuild-error detail=<…>` / `op=issues-list-rebuild-retry` (the issues:list cache rebuild failed and set the staleness marker / a marker-driven retry succeeded — #235) and `op=issues-list-fresh-error detail=<…>` (a fresh=1 diagnostic live build failed, cache served instead); `op=query chars=<n> facets=<allowlisted names> hits=<n> open_ms=<n> query_ms=<n> enrich_ms=<n> serialize_ms=<n> body_bytes=<n> ms=<n>` per `/__search` request | issue-230 unified full-text search (embedded SQLite FTS5, `search_index.rs`). The background indexer keeps the current project's content in an FTS5 index across buckets — `bucket=claude` and `bucket=codex` (session transcripts, every pass), `bucket=commits` (`git log`, every pass), `bucket=worklist-history` (`resources/worklist-history/<ts>.json`+`.md`, every pass), `bucket=issues` (`gh` via the forge adapter, every ~6th pass for rate limits) — stored as a rebuildable cache at `<app_cache_dir>/search-index/<project-key>.db`. Each row keys on a generic doc key (session path, `commit:<sha>`, `issue:<number>`, `history:<ts>`) with a change token (mtime / immutable / `updatedAt`) so unchanged docs skip. `op=scan` separates filesystem discovery and extraction from SQLite gating/writes; `op=query` separates SQLite open/query from result enrichment and JSON serialization. Trace payloads contain only counts, durations, byte sizes, query character count, and allowlisted facet names — never query text, snippets, paths, titles, or indexed content. |
| `search-render` | `__bramMeasureTurnsRender` in `helpers.js` (called from `SessionDetail`'s `/__turns` onLoaded) | `op=turns turns=<n> ms=<paint-delta>` | issue-230 session-transcript render cost. A double-`requestAnimationFrame` after the turns load measures data-ready → next-paint (captures through a synchronous render freeze, like `menu-paint`), logging render-to-paint ms against turn count. One line per session expand. Used to size the `Items`→virtualized-`List` win (host projection is ~0.5s for 1514 turns via `turns-projection`; this is the client-render half). |
| `commit-find-scroll` / `diff-find-scroll` | `__bramCommitScrollToActive` / `__bramDiffScrollToActive` in `helpers.js` | outer: `row`, `cursor`, `total`, `hasRef`; inner: `rows`, `active`, `row`, `hasRef` | Soak trace for the find-in-diff nav (search-index-commit-diffs iterate): one line per ▲▼ step at each scroll stage — outer (CommitDetail's block list) and inner (DiffView's line list). A step that doesn't land names its failure: no line = the ChangeListener never fired; `row=-1` = the match walker missed; `hasRef=false` = the List ref wasn't up when the step ran. Remove once the nav mechanics are trusted. |
| `ai-describe` (host) | `handle_describe_command` in `lib.rs` | `op=call` (`ms`, `model`, `input_tokens`, `output_tokens`, `upgraded`, `ctx`, `result`, `redactions`, `id`); `op=hit` (`id`); `op=skip` (`reason=disabled\|no-key`); `op=error` (`status`, `ms`, `detail`) | One line per `/__describe-command` request — Haiku intent-header synthesis for tool expansions. Default off: requires explicit project `ai.describeCommands: true` plus `ANTHROPIC_API_KEY`. Command/diff/write/access material, context, result excerpts, and existing descriptions are redacted before request construction. `op=call` carries latency, token counts, and the count of masked spans, never prompt content. Explicit opt-in is the security boundary because heuristic redaction cannot guarantee arbitrary content is secret-free. |
