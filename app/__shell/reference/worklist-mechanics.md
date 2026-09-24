# Bram reference: worklist mechanics

Seeded by Bram Setup as `.claude/bram-reference/worklist-mechanics.md`.
The always-loaded conventions (`.claude/bram-conventions.md`) point
here when one of these situations arises. Everything here binds when
it applies; the core conventions stay authoritative for ordinary turns.

## Opt-out plumbing

How the two direct-edit opt-outs ("just do it" and `skip-worklist:`)
are honored. The user-facing contract is identical for both agents.

- **Claude, prose opt-out.** Claude's guard matches `_OPT_OUT_PATTERNS`
  against `transcript_path` on every `PreToolUse` and allows inline.
  Claude prose opt-outs have no host chokepoint on their allow path, so
  the guard POSTs a best-effort breadcrumb to `POST /__audit/direct-edit`
  right before allowing — one `direct-edit` audit-ledger record per
  opted-out turn, deduped host-side so the guard's per-tool-call firing
  doesn't produce duplicates.
- **Codex, prose opt-out.** Bram's host-side `toTurn` path matches the
  same phrase and writes a one-turn `direct-edit` record
  (`kind:"direct-edit"`, `paths:["*"]`, 1h TTL) to
  `resources/.worklist-direct-edit.json` — the grant's own sidecar,
  separate from the single-slot `.worklist-authorization.json` that any
  gate click replaces. The single Codex `PreToolUse` hook reads it via
  `fresh_bypass()` (legacy records in `.worklist-authorization.json`
  stay honored for one release). The same `toTurn` path records a
  `direct-edit` line in the audit ledger.
- **`skip-worklist:` prefix (both agents).** When the host's `toTurn`
  write path sees the prefix it writes the same one-turn `direct-edit`
  record to `resources/.worklist-direct-edit.json`, then forwards the
  entire turn text including the prefix. The PreToolUse hook allows the
  edits via the existing `fresh_bypass()` path.
- Both opt-out matchers (guard-side and host-side) read the item-
  feedback drafts as well as inline text.

## Feedback payload details

Item feedback is a message sent with Worklist items selected; it arrives
as an `iterate:` turn.

- A feedback item is `{id, feedbackRef}`; `feedbackRef` names
  `resources/feedback-drafts/<feedbackRef>.md`, the user's full-fidelity
  feedback text. Read that file directly: `toTurn`'s `\s+ → " "`
  collapse and the receiving TUI's bracketed-paste limits don't apply,
  because the text never rode the PTY paste channel.
- Inline `{id, feedback}` on a feedback item is the **degradation
  fallback only**, taken per item when its draft write failed so the
  iterate still lands rather than blocking the send. Its text rode the
  paste channel and is subject to the collapse above.
- Successful `/__worklist/mutate` advance/prune promotes matching drafts
  from `feedback-drafts/` to `feedback-history/`, so drafts don't
  accumulate. Each draft write emits a `[feedback-draft] op=write` trace
  line with `feedback_id` and byte count.
- Approve and Drop still use the inline `{id, feedback}` shape (their
  feedback is usually short).
- The host detects the `iterate:` prefix on the `toTurn` write path and
  sets the inflight sentinel; the same turn-finished detectors that
  clear approve/drop sentinels clear iterate's. There are no
  `/__iterate/begin` / `/__iterate/end` routes.

## The claim and the authorization

Authorization and execution are separate records:

- **Authorization is durable.** The record `mutate op:"advance"` and
  `worklist-commit` consume is unchanged, survives across turns, and
  retires incrementally by resolved ids. Forgetting a lifecycle call no
  longer strands anything: the item stays where it is, visible and
  actionable.
- **The claim is execution state.** The host writes it while a turn
  runs and retires it when the turn ends (`op=clear-at-turn-end`),
  re-establishing it at the next turn's start whenever an authorization
  is still live (`op=rearm-at-turn-start`) — so a multi-turn apply keeps
  every turn's work attributed, and the gate is locked exactly while
  something is running.

On an apply gate, `resolve` is skipped because the host sets the
sentinel at approval time and `mutate op:"advance"` consumes the
`approved` auth; `resolve`'s return value would only repeat the
proposal you authored.

`worklist-commit` retires claims by the ids it resolves, whichever
approval carried the click.

`POST /__worklist/end` is an **explicit early release** — use it to
unlock the board mid-turn when you know you're done holding it. It is
not an obligation. Both the method and the body are required: a `GET`
returns `POST only`, and an empty body returns
`{"error":"ids[] required"}`. It returns
`{"ok":true,"cleared":<bool>,"remaining":[...]}`; `cleared:false` with a
non-empty `remaining` means the call resolved only part of a multi-id
claim, not that it failed.

## Interval staging and entangled commits

When a commit would stage a path carrying lines attributed to a begun
item *outside the request*, `worklist-commit` doesn't refuse — it
stages only the requested items' OWN hunks: their claim-interval
patches applied to a scratch index seeded from `HEAD`, committed via
`git commit-tree` + `update-ref`, with the worktree and the real index
**never touched**. The neighbour's uncommitted work stays in the
worktree (`git diff HEAD` then shows exactly it). The commit is
hunk-exact, preserving per-item authorship in git history (trace:
`op=entangled-interval-stage` then `op=interval-staged`).

Order-independence is decided by git, not enforced: the interval patch
is gated with `git apply --check` against the HEAD-seeded index.
Success means any order is fine; failure means the requested item's
change is *defined relative to* another begun item's work not in the
request, and the response names the item to commit first. `--3way` is
deliberately not used (it writes conflict markers and succeeds).

The **409 refusal survives only** when an entangled item has NO claim
interval to stage from — work predating the capture phase, or done
with no claim live (the `no-interval` fallback). Then commit the items
together (safe: every line is accounted for by an id in that commit) or
separate the hunks by hand.

So the pane offers Commit on any begun item with changes of its own
(exclusive or shared), and the strip's `unique:` figure is what a
commit takes on a contended path.

**The dangerous default is the opposite of the intuition:** committing
all N entangled items **together** is safe — every changed line is
accounted for by an id in that commit — while committing **one of N**
is what can absorb a neighbour's work. Separation is the risky
operation, not aggregation.

**Commit-gate requirements.** The host verifies approved auth, requires
every requested id to be `applied` (relaxed to also accept `proposed`
only when the click's `commitToo` auth is set — the apply-and-commit
path, `allow_proposed`), stages only those items' files, refuses
unrelated staged files, commits, and prunes the requested items. Both
apply-and-commit triggers (the one-click **Start & commit** and the plain
**Commit** on a begun `proposed` item) are always available; the widening
is only in when the pane *offers* the button, not in what the host
accepts.

**`split-shared-files`.** To commit entangled items one at a time:
isolate one item's hunks on disk, call `worklist-commit` for that id,
restore the next item's isolated hunks, and repeat. The interval-diff
route serves each item's own patch, and `git apply --check` (forward
and reverse) gates each step. **Build-gate every intermediate state
before committing it**: hunk isolation can produce a tree that compiles
in neither direction, and a non-building intermediate commit poisons
`git bisect`. Each successful subset call retires only those ids from
the authorization and inflight claim; the remaining ids and embedded
bodies stay live for later commits, and the final call consumes the
record. A same-click joint interval can't be split this way — see
*Serializing entangled items approved together*.

## Commit timeouts: never re-POST

`worklist-commit` is single-shot per approval. A large commit takes as
long as `git` takes, and can run past a caller's tool timeout; a
blocked HTTP request looks identical to a wedged one. It is not wedged,
and the retry is the dangerous move: a timeout tells you nothing about
the route — it may still be staging, or it may have committed and
returned into a socket nobody was reading.

So when the call times out, **check `git log` first** and treat what you
find there as the answer. The route serializes against itself, so a
re-POST queues behind the in-flight commit rather than racing its `git`
invocations, and the approval is consumed exactly once, so a second
request can't produce a second commit. It returns instead an error
naming the completed commit ("this commit ALREADY RAN … verify with git
log"); read that as success, not failure. Watch the phases in
`resources/bram-traces/bram-trace.log` — `[worklist-commit] op=staging`,
`op=committing`, `op=committed` — to tell a long commit from a stuck one
without touching git state.

The Codex intent channel carries the same rule: **do not rewrite
`resources/.worklist-intent.json` with a fresh nonce after a slow
commit.** That is the re-POST in another spelling. Read
`resources/.worklist-result.json` and the git state first; a result
whose nonce matches your original request is still coming, and the
commit it describes may already exist.

## Claude transport: flag rationale and stale ports

The literal port matches the `.claude/settings.json` allowlist and runs
without a prompt. `$BRAM_PORT` won't work — Claude Code's permission
matcher doesn't expand variables, so `$` breaks the match (see
https://code.claude.com/docs/en/permissions.md). The POST routes
(`worklist-mutate`, `worklist-commit`) have their own, narrow allowlist
entries. Why `-X POST` must be literal: the POST entries require it;
relying on `--data` to imply POST matches neither the POST entries nor
the GET entry (whose URL must follow `--retry-delay 1` with no flags
between). Why the curl must stand alone: a compound `cat <<EOF … && jq
… && curl …` makes the command string start with `cat`, so no `curl …`
prefix can match.

Flag rationale:

- `-4` + `127.0.0.1` (not `localhost`): Bram binds IPv4 only;
  `localhost` may try `::1` first and fail with `curl: (7)`.
- `-sS` (not `-s`): `-s` swallows `Failed to connect`, so a stale-port
  race surfaces as `(no output)` instead of `curl: (7)`.
- `--retry 3 --retry-delay 1` **retries 5xx responses, not just
  connection failures** — and can't be narrowed, because
  `--retry-connrefused` (which covers the stale-port race) is inert
  without `--retry N`, and curl has no flag for "retry connection errors
  but not server errors". So on any 5xx the transport re-POSTs three
  more times, beneath the agent, silently. Read *single-shot per
  approval, never re-POST* as a rule for the AGENT, not a guarantee the
  transport upholds. Refusal-shaped failures return 4xx, which curl
  doesn't retry, so a 5xx from a Bram route is genuinely a server fault,
  and seeing one repeat four times is expected rather than a second bug.

**If the port keeps refusing** after fresh re-reads, treat it as a
stale-port / restarting-server diagnostic — don't continue without the
lifecycle call. Check the Status tab's **Port file** row, which
cross-checks the running process, `.bram-port`, and the
`.bram-port.json` sidecar (port + pid + project root + startup
timestamp). If `.bram-port` is missing entirely (agent launched outside
Bram's PTY shell), fall back to `lsof -nP -iTCP -sTCP:LISTEN | grep bram`.

## Codex transport: filesystem intent/result files

Codex's `workspace-write` sandbox refuses loopback connections, and the
only knob that would fix it (`network_access = true`) grants all
outbound network. So Codex drives the lifecycle through two
coordination dot-files. Both transports dispatch through the same
host-side handlers, so response kinds, consume-on-read, the inflight
sentinel, and the auth checks are identical.

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
   user's next Push. The agent never writes a close intent.

2. **Read** `resources/.worklist-result.json` for the record whose
   `nonce` matches (ignore stale results from prior requests):

   ```json
   { "nonce": "<echoed>", "ok": true,  "status": 200, "result": { ... }, "completedAtMs": 0 }
   { "nonce": "<echoed>", "ok": false, "status": 400, "error":  { ... }, "completedAtMs": 0 }
   ```

   `result` is byte-for-byte what the HTTP route would have returned.
   The host writes within watcher latency (a few ms) and then deletes
   the intent file; a brief read-retry covers the race. **Do not
   continue silently** on a missing result or `ok: false`. After a slow
   commit, never rewrite the intent with a fresh nonce (see *Commit
   timeouts*).

The Codex PreToolUse guard exempts `.worklist-intent.json` from
worklist coverage — it's a coordination file, like the loopback curl is
for Claude. Trace each drain by grepping `[worklist-intent]` in
`resources/bram-traces/bram-trace.log`.

## Incremental claim and authorization retirement

A claim can cover more than one id — approving several unentangled
items in one click writes one claim listing all of them. Resolving just
one of those ids is normal, not refused: `mutate op:"advance"`,
`op:"prune"`, and `worklist-commit` (which delegates a prune to the same
path) each retire exactly the ids they resolved and rewrite the claim
with whatever is left, tracing
`[inflight-sentinel] op=clear-shrink resolved=[...] remaining=[...]`.
The claim file disappears only once the last id resolves. Two disjoint
items started in one click can therefore be completed — applied,
committed, or dropped — in separate turns, each clearing its own slice
of the spinner. (Where the core conventions say a call "clears the
sentinel", that is the one-id case.)

The authorization record retires by the same named subset. Until the
last id resolves it keeps `consumedAtMs` empty, removes the completed
ids and their embedded bodies, and traces
`[auth-record] op=consume-shrink resolved=[...] remaining=[...]`. This
is load-bearing for `split-shared-files`: one plural Commit approval
can produce several sequential `worklist-commit` calls without the first
commit consuming authority for the rest. The original `issuedAtMs` and
interrupt flag remain unchanged, so TTL and cancel fail-closed behavior
still cover the whole sequence.

This is distinct from `op=clear-partial`, which is still a refusal: the
blunt clears — turn-end detectors, cancel paths, startup cleanup, the
drop policy validator — can't name which ids they resolve, so they
require full coverage of the live claim and log `op=clear-partial` when
a request covers only part of it. That signals something colliding (a
second claimant overwrote or partly overlapped the live claim), not
ordinary progress — don't read `clear-partial` as "working as
intended" the way `clear-shrink` is. Only routes that resolve specific,
named ids may shrink.

## Delegating worklist items to subagents

Parallelize the *work*; serialize the *gates*.

- Subagents receive file paths and instructions only. They do **not**
  call `/__worklist/resolve`, `/__worklist/mutate`, or
  `worklist-commit`. Every lifecycle call stays in the orchestrator's
  own turn, after the subagents return.
- The reason is structural: the inflight sentinel holds one claim
  (writing a second overwrites the first), and an authorization record
  carries one whole-record consumed flag (the first `mutate` consumes
  it, and the next subagent is told `no_active_authorization` for work
  that was in fact approved). Neither surface can represent two
  concurrent claimants.
- Before delegating in parallel, intersect the `files` lists of the
  candidate items. **Non-empty intersection → do not parallelize those
  two**; run them in sequence.
- Worktrees under `.claude/worktrees/<name>/` inherit the corresponding
  real-tree coverage: an item covering `app/x.js` also covers
  `.claude/worktrees/agent-a/app/x.js`. Don't add worktree-prefixed
  twins to an item's `files`; the guards strip that one prefix for
  coverage matching, including declared-directory coverage, while
  keeping the original path in traces and diagnostics.
- A worktree denial is still a denial. Report it to the orchestrator;
  never route around it through another tool or scripted write. A
  blocked worktree with zero changes is eligible for harness cleanup,
  so report promptly rather than waiting in place.
- Attribution: `worklist.json` has no agent field, so a committable
  item's diff produced by several subagents carries no record of which
  one made which edit. If that matters, commit the items separately.
- **Hook-enforced on Claude.** A lifecycle call from a delegated
  subagent is denied at `PreToolUse`
  (`decision=deny reason=subagent-lifecycle-call`) and never reaches the
  host. The check keys on the payload's `agent_id`, populated only for
  subagent-originated tool calls. Enforcement claims need a *fire*
  behind them (a deliberate violation), not an inspection.

## Serializing entangled items approved together

One gate click can approve multiple items whose `files` intersect. The
parallel-delegation rule forbids working those concurrently; this is
the discipline for completing them serially.

- **Complete them strictly in sequence, committing the first before
  applying the second**, so the shared file never carries two items'
  hunks. Each commit then stages a clean per-item diff with no hunk
  surgery, and the second item's changes become exclusive the moment
  the first commit lands.
- **A same-click plural approval cannot split its shared files at the
  gate.** One click writes ONE claim and one capture boundary, so both
  items' edits to a shared declared path land in a JOINT interval that
  per-item staging has nothing to stage from. A `split-shared-files`
  commit of one such id is **refused** (409, `op=refuse-joint-interval`,
  claim released), naming the joint ids. Commit the joint items
  **together** (safe — every changed line is accounted for by an id in
  that commit), or separate the hunks by hand. When per-item commits on
  a shared file are the goal, approve the items in **separate clicks**:
  each gets its own claim and boundary, and interval staging splits
  them correctly.
- **The satisfiable third out for an existing joint: the
  park/drop/re-propose dance.** This is CONTENT surgery, not in-place
  hand-separation — editing the worktree alone can never clear the
  refusal, because the joint attribution is recorded, not derived from
  the diff. In order: (1) the agent parks one item's entire diff (save
  its patch — the interval-diff route or `git diff` scoped to its
  regions — then reverse-apply, keeping the patch file); (2) the user
  **Drops** that item — the step that dissolves the block, since a
  dropped partner is no longer begun and staging then follows current
  content; (3) commit the surviving item (whole-file, clean; cut a
  branch first when each item drives its own PR); (4) the agent
  restores the patch and re-proposes the dropped item — same id, same
  draft, its history honestly reading drop → re-propose; (5) the user
  Starts & commits it under its own fresh boundary. Every line lands
  under its right id; the cost is one extra Drop and one extra Start.
  The host's `op=refuse-joint-interval` message names this path; don't
  undo a correctly parked worktree because the refusal fired before the
  Drop.
- **A live claim locks row selection**, and the locked selection is
  where the Commit button lives. The claim retires on its own when your
  turn ends, so ending the turn to hand the user a commit decision is
  enough. Only if you hand over the decision and keep working in the
  same turn, release it early with `POST /__worklist/end` naming the
  still-claimed ids.
- **Don't plan to resume the later item on the claim or the
  authorization record.** Both are single-slot and displaceable: the
  user's next gate click — typically the very commit being waited
  for — writes a fresh authorization that displaces the surviving one,
  and a later `mutate op:"advance"` is denied `id not in auth`. The
  durable resume state is `begunAtMs` plus the on-disk diff. After the
  first commit cleans the shared file, the later item's changes are
  exclusive and the pane's plain **Commit** offer on a begun `proposed`
  item is the legitimate resume channel; a fresh **Start** click is the
  fallback when the later item's edits don't exist yet.

## Close-on-push and partial landings

- The user's close-on-commit choices arrive in the per-item `feedback`
  of the `approved:` payload as `close-issue:` lines after any free-text
  feedback:

  ```
  close-issue: 52
  close-issue: 50 comment: "shipped, see commit message"
  ```

- At the commit gate the host parses these verified selections and
  records, for the requested ids only, a pending close bound to the new
  commit SHA. Nothing closes or pushes at commit time. On the user's
  next explicit **Push**, once each commit reaches the default branch,
  the host closes its issue with a `Closed by <commit-url>` comment
  (prefixed with the user's comment when given).
- A record whose issue is already closed by other means retires quietly
  (`op=retired-already-closed`). On GitHub, a **merged PR** containing
  the bound commit completes the close (`op=closed-via-pr`,
  `Closed by <pr-url>`) even though a squashed SHA never lands on the
  default branch itself.
- There is no agent-reachable close route (`/__issue/close` was
  removed) and no `issue-close` intent. Don't resolve the SHA; don't
  tell the user to run a close step. Closing never pushes, so closing one
  issue can never push others stacked behind it.
- The close-on-commit dialog shows one row per issue plus an optional
  close-comment textbox; "commit only" queues nothing. A residual
  `push-before-close:` toggle in the dialog is inert (the backend ignores
  it).
- `worklist-commit`'s success body carries `queuedCloses`
  (`{"ok":true,"sha":"…","queuedCloses":[16]}`; the Codex result file
  carries the same bytes). `closesIssues` is the *offer* that makes the
  gate render the tick and stays set even when the user unticks, so
  narrating from it reports intent as outcome.
- **Partial landings.** An interval-staged commit stages only the lines
  the requested item's claim intervals attribute. Work the item owns but
  that was edited in *unclaimed time* (no claim live, e.g. a redo after
  the item had already advanced) has no interval and stays in the
  worktree. The success body then carries `residualPaths: [{path,
  owner}]` for each declared path with leftover diffs — `owner` naming
  the begun neighbour whose staying work accounts for them (normal,
  nothing to do) or `"unowned"` — and `retained: [ids]` for requested
  items with unowned residue, which the host does **not** prune: the row
  stays on the board with its changes, and the pane's Commit offer (a
  fresh gate click) is the resume channel. Report the partial landing
  instead of announcing completion, and don't re-POST: the approval is
  consumed and the claim released.
- **`[bram: …]` notes** are host-written, e.g. `[bram: the queued close
  of #21 (commit 94f7666) was withdrawn … it will NOT fire on Push, and
  #21 stays open]`. They ride only plain message turns, never
  `approved:` / `drop:` / `iterate:` / `skip-worklist:` turns.

## Enforcement and security contract

The structured `approved:` / `drop:` line is not authority by itself.
The host records each clicked id into
`resources/.worklist-authorization.json` with its kind (`approved` /
`drop`); `/__worklist/resolve` is the only way an agent receives the
recorded item bodies; `/__worklist/mutate` is the only way an agent
advances or prunes:

- `advance` requires an `approved` auth record covering every id.
- `prune` requires `drop`, except the post-commit prune path also
  accepts `approved` when the requested ids are already `applied`.

Same-turn `resolve → edit files → mutate` is valid: `mutate` reads the
stored auth record, not just resolve's consumption state.

There is **no content-hash verification**. Bram shares a worklist
between agents serially, never concurrently. The remaining concurrency
guard is the `version` integer on `worklist.json` (file-write races,
hook-enforced); self-authorization is gated structurally — `resolve` /
`mutate` are the only channels and the auth record is consumed on
read — not by a hash.

Defense in depth: Claude and Codex each install PreToolUse hooks that
validate worklist coverage before file-mutating tools run, and the
desktop watcher reverts unauthorized prunes. Both guards also reject
`worklist.json` writes that put non-empty `before` / `after` on any
proposed item — prose must live in `resources/worklist-drafts/<id>.md`.

## Signature deny reasons

Both provider guards check the forge-artifact signature on `PreToolUse`
and **deny** rather than warn. They fail open only when no inspectable
prose body exists (`--body-file -` or a metadata-only command with no
body flag). Coverage includes issue/PR/MR prose writes, GitHub gists,
GitLab snippets, and inspectable `gh api` comment/gist writes.
Recognized file-backed writes whose named file can't be read are
denied. The guards require the full current form on every new
inspectable artifact; the version slot is parsed when present and
otherwise optional for now.

Writing the body file in the same command as the post defeats the
check (the file doesn't exist yet when the guard looks), and a heredoc
redirect in the same command is another write pattern, which also
disqualifies the issue-only worklist exemption. Deny reasons:

- `crossboundary-unparsed:body-file-unreadable` — the named body file
  couldn't be read; the write is denied (never silently bypassed) and
  the message names the path that failed to open. Quoted paths and, on
  Windows, MSYS `/c/...` spellings are accepted.
- `crossboundary-unsigned` — the body was read and lacks the signature.
- `crossboundary-stale-version:<signed>` — the body IS signed, but its
  version slot names a build other than the one running. The deny names
  both versions. The usual cause is a long-running session signing from
  the `@`-imported conventions copy that was true when it booted; read
  `GET /__app-info` for the live version. A signature with **no**
  version slot is still allowed.
- `no-session-url` — a direct `git commit` or forge write whose command
  text or readable message/body file carries a session URL or
  `Claude-Session:` trailer. Greps and trace reads that mention the URL
  are untouched; only publishing-shaped commands are screened.
- A forge write whose body contains a full 40-hex SHA that doesn't
  resolve locally is denied (catches fabricated hashes and
  rebase-orphaned citations alike).

Signature slot meanings, for readers: the version answers "which
build?"; thread and model give evidential standing (an orchestrator
held the design discussion, a subagent saw only its brief) and
attribution; os and machine distinguish sessions of the same owner's
same agent across machines. The hostname also makes the signature
machine-readable provenance: Awaiting You classifies a same-account
comment by it — unsigned means the human typed it, this host's name
means this machine's agent, any other means your agent elsewhere moved.
