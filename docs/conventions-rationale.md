# Conventions rationale

Source-repo only; never seeded into managed projects. This file holds
the history behind `app/__shell/conventions.md` (the always-loaded core,
seeded as `.claude/bram-conventions.md`) and the operational reference
in `app/__shell/reference/*.md` (seeded as `.claude/bram-reference/`).
Those files carry the rules; this file carries the dated incidents,
live cases, receipts and design arguments that justify them, grouped
under the rule each one supports. The paragraphs were moved here
verbatim or near-verbatim in the core/reference split
(`conventions-core-and-reference-split`, judell/bram#396), so nothing
historical was lost. git history and `/__search` hold the rest.

The placement rule the split applied: a passage stays in the
always-loaded core only if an agent in a managed project would act
wrongly before it knows it needs to look anything up. Rules that bind
on ordinary turns are core; rules with a recognizable trigger get a
one-line pointer in the core and detail in a reference file; the story
comes here.

## Opt-outs: only "just do it" and `skip-worklist:`

Retired phrases ("skip the worklist", "commit this directly", "inline
the fix", "no worklist for this", "don't bother with the worklist") no
longer opt out — narrowed (opt-out-single-phrase-and-audit) from a
seven-regex list to one explicit phrase, to cut the risk of an
accidental match in ordinary prose.

The direct-edit grant got its own sidecar,
`resources/.worklist-direct-edit.json`, under issue-352: the shared
authorization file is single-slot and any gate click replaces it, which
destroyed a live grant mid-TTL.

## Write the finding into the draft

Live case, 2026-09-09: `footer-model-label-stale-after-switch`'s draft
asserted `footerAgents` refetches on "exactly two signals"; applying it
found a third trigger at `Main.xmlui:227-229`, and the rest of the
chain verified clean. The row was correctly dropped — but only because
the agent said so in chat. Jon's question was the whole item: "how
could I ever have known that that would be the right thing to do
here?" He couldn't have — the draft still asserted the disproved
premise. This is the agent half only; the pane surfacing a recorded
finding is tracked separately as judell/bram#378.

Marking an item `applied` when nothing was applied produces a row with
nothing to commit (the legacy tab's TO COMMIT badge on an empty diff),
which is exactly the user-visible failure mode of #88.

## Drop removes the item, not the bytes

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

Both cases had a defensible resolution and they were different ones —
which is why the rule is to propose revert / re-home / leave and say
which was chosen, rather than prescribe one. On stashing: the judgement
that work is worthless is usually made minutes after making it.

## Reminder items and the `reminder-` prefix

Live precedents for reminder items: `file-upstream-null-expr-crash-after-3763`
and `revendor-after-xmlui-release` (bram), `watch-for-3764-merge` (xmlui).

This rule is **mechanical on purpose**, and that is the whole reason it
exists as a name rather than as prose. A reminder is the one item shape
whose entire job is to be remembered after the session that wrote it is
gone, so surfaces have to be able to find it without reading English —
the Awaiting You inbox keys on exactly this prefix. A convention only a
human can evaluate cannot drive a surface.

Converting a pre-rule reminder (Drop, then re-propose verbatim under a
`reminder-` id) costs two clicks and leaves an honest drop → re-propose
history.

## Field notes: `closesIssues`, `begunAtMs`, `status`, `files`

`closesIssues` timing (closes-decision-belongs-to-the-commit-gate):
whether a commit truly closes an issue is a commit-time judgment, not a
proposal-time prediction (live case: an item authored with
`closes #273` whose fix was withdrawn the same evening; the claim had
to be hand-deleted).

Directory entries in `files` are expanded by both staging and
verification (#295).

`begunAtMs`: it is the durable answer to "has work on this item
begun?", which the Worklist strip and the overlap banner both need. The
other two signals for that question — the authorization record and the
inflight claim — are single-slot and displaceable, so on their own they
let an item with real work on disk report "No changes yet" as soon as
another item was approved. A re-approval (iterate, second gate) leaves
the original stamp.

`status` history: as of 0.5.1 `status` no longer gates committability
by itself. The current pane reasons about three states keyed on what
the user can do — not started, nothing to commit, has changes you can
commit — and a `proposed` item can reach that third state on its own,
once begun with exclusive changes, without ever becoming `applied`.
`applied` still means committable; it is just no longer the only status
that does.

Keeping `files` current: live case 2026-08-20,
`rethink-activity-indicators`, where an unneeded `helpers.js` guess made
a complete commit read as a partial one. The clean expression of "the
plan was wrong, not the work" is a corrected plan, not a caveat on the
count; a committed item whose count still reads `2 of 3` invites the
misreading that work went missing.

## Gate verbs: labels change, wire kinds don't

Button names are the labels the 0.5.3 gate bar renders — **Start N /
Start & commit N / Commit N / Refine N / Drop N**. Buttons may rename
(they did, in the start-verb renaming — Mary's day-one report, #293,
caught this doc still saying "Approve & commit"), payloads don't. The
Refine button renamed in the refine-verb rename; the wire kind did not,
so **Refine emits `iterate:`** and every trace, audit record and guard
matcher still reads `iterate`.

The legacy *Talk to agent* button and its `talk:` prefix retired with
the Workspace tab. The legacy `/__iterate/begin` and `/__iterate/end`
routes were removed in the #214 delete phase.

Refine payloads: for a while after the worklist2 rewrite the gate's
Refine emitted inline unconditionally while the Queue tab drafted — an
accidental fork, repaired under #285; both emitters draft first now, and
both opt-out matchers (guard-side and host-side) read the drafts (#171,
#284). Approve and Drop's migration to `feedbackRef` is filed as
follow-up. See #144.

The lifecycle list once told agents to call `/__worklist/resolve` on the
apply gate; the transports section superseded it ("Apply gate: skip
`resolve`"). `resolve`'s two side effects are covered without a
round-trip: the host sets the inflight sentinel at approval time, and
`mutate op:"advance"` consumes the `approved` auth.

## Notice when feedback drifts

The pane can only show *which mode you are in*; you are the only party
that can read the content and judge whether it belongs, which is why
this half is yours. Live receipt (2026-09-09): one item accumulated
seven feedback records of which one was about it — the rest were
another project's soak data, a bug report from another user, voice
diagnostics, and unrelated policy design. The person who designed the
addressing distinction walked into it himself and only noticed
afterward, which is how quiet the current cue is. Surfacing it in the
UI is tracked on judell/bram#368; this is the part that costs one
sentence.

Second receipt, one turn later and against this very section: its first
draft said to ask the question and then "do the work anyway",
explicitly "never a blocking question". The convention's own test case —
feedback on this item asking for an unrelated issue to be filed —
produced exactly that: the question was asked, then ignored, and the
work started. Jon's verdict was "too aggressive... I wouldn't have
wanted you to", sent through **Chat**. The blocking half is the whole
mechanism; without it the opener is decoration.

Why blocking: the two things that would go wrong are the same mistake —
if the feedback is misaddressed then the work it asks for is probably
misplaced too. Waiting costs one turn; proceeding costs a correction
plus a record that has to be unwound by hand. Asking and then proceeding
regardless performs the check while denying it any power to change the
outcome, and trains the user to read the question as noise.

## The claim ends with your turn

(separate-authorization-from-claim.) Authorization and execution used
to be one record: the claim both said "this work is authorized" (which
must survive across turns) and "this work is executing" (which must
not). Because it had to survive, it outlived the turn, and the only
thing between a finished turn and a locked board was an agent
remembering an out-of-band call. So the two were split by role.

Live cases for the three now-ordinary endings:

- Approved, investigated, nothing to apply — 2026-08-24:
  `issue-275-transcript-row-remount-churn`, whose first step disproved
  its own hypothesis and whose claim then locked the row until it was
  unwound by hand.
- Ending a turn to ask the user a decision — 2026-08-26:
  `issue-262-cross-project-direct-edit-auth`, where the user was left
  asking "nobody is working but we are still spinning".
- The user takes over the commit — issue-348.

Earlier revisions listed `POST /__worklist/end` only under *Refine
stuck*; the route was never iterate-specific. It once returned a bare `{"ok":true}`; it now reports
`cleared` and `remaining`.

## Apply-and-commit: the 0.5.1 widening

`gate: "apply-and-commit"` was originally only the pre-approval
one-click **Start & commit** button's payload. As of 0.5.1 the pane
also puts a plain **Commit** on a `proposed` item that has already begun
and whose changes are EXCLUSIVE — every changed path free of any other
begun item's claim (`window.__bramSelectionAllCommittable` in
`helpers.js`). A `worklist.oneClickApproveCommit` setting once gated
both triggers; it was retired in the 0.5.3 run after its config-off path
produced a dead-end row — the offer was only ever visibility, never
authorization, so removing the flag removed a bug class and no
capability.

## Interval staging and entangled commits

Interval staging (#327) made entangled commits safe. The #336
whole-file withholding, and the #337 selection-scoping that softened
it, both retired into interval staging.

**The dangerous default is the opposite of the intuition** (#336's
field lesson, stated because the person who wrote these conventions
had it backwards): committing all N entangled items together is safe,
while committing one of N is what silently absorbs a neighbour's work.

`split-shared-files` via the interval-diff route and `git apply --check`
(forward and reverse) was field-proven exact in #336's follow-up: two
entangled items, `unique:` figures matching the isolated diffs to the
line.

## Commit timeouts: never re-POST

An 85-file commit ran past a caller's 120 s tool timeout (#308), and a
blocked HTTP request looks identical to a wedged one.

The Codex intent file is the one transport where writing again is
trivially easy, which is why the rule is spelled out for it separately.

## Claude transport: curl retries 5xx

Measured 2026-09-09: one refused commit produced four POSTs at 1.00 s
spacing, because `--retry 3` retries 5xx and can't be narrowed. The
remedy was on the server side — refusal-shaped failures return 4xx,
which curl does not retry (#373).

Both allowlist pitfalls (missing literal `-X POST`, a compound command
starting with `cat`) prompted a real `worklist-commit` call before they
were written down.

## Codex transport: why files, not loopback

Codex's `workspace-write` sandbox refuses loopback connections (issue
#130); the only knob that would fix it (`network_access = true`) grants
all outbound network.

## Ids are immutable

Renaming is not supported; the rollback is silent and your write
returns success, so nothing tells you it failed. Tracked in
judell/bram#276.

## Enforcement: no content hash

An earlier design recomputed each item's content hash at record time
and flipped mismatches to a `rejected_stale` kind — an
optimistic-concurrency guard against the worklist changing between
click and record. Bram only ever shares a worklist between agents
**serially, never concurrently**, so that guard never fired and was
removed.

## Signing agent-authored forge artifacts

The reason is not "which project is this from" but **who is speaking**.
Agents post through the human's account, so an unsigned agent comment
is indistinguishable from one the human wrote, and that is equally
true in the project you are sitting in. An earlier version of this
convention scoped the requirement to artifacts that cross a project
boundary; the result was judell/bram#253, where unsigned agent comments
sat beside Jon's own replies while a cross-boundary issue filed in the
same period was correctly signed. The record was inconsistent along an
axis no reader cares about. The cross-boundary case is a subset of the
problem, and it was mistaken for the whole of it.

The form has nine slots, all load-bearing — *which build*, *whose*
agent, *which* agent, *which thread*, *which model*, *which platform*,
*which machine*, the familiar project name, and the exact repository
that anchors the speaker's evidence. More examples of this project's
instances:

    Jon's Claude (Bram 0.6.5, subagent, Opus 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):
    Jon's Claude (Bram 0.6.5, main thread, Opus 5, Windows, JON-PC) speaking from the Bram project (github.com/judell/bram):

`<version>` leads the parenthetical because "which build?" is the
question a reader asks before any other triage step, and a
version-less report is expensive to place after the fact:
judell/bram#343 and #362 are both field cases where the absence of a
version cost real archaeology to reconstruct which build a report came
from.

Older signature forms (no version slot, or no os/machine slots) remain
valid historical text, and — same soft rollout as the #346 os/machine
slots — the guards do not yet enforce the version slot's presence on a
new write; they parse it when present and leave it optional otherwise.

The familiar project name stays because it reads well; the locator is
the unambiguous identity across forks, mirrors, same-named
repositories, and forge providers.

A form that names only the project ("from the xmlui side") leaves "who
is speaking" unanswered, which is the half that matters when two agents
work the same thread. Across a boundary the third slot also answers
*which side the evidence comes from*. The `<thread>` and `<model>` slots
(added 2026-08-28, when multi-agent orchestration made them
unrecoverable otherwise) answer *evidential standing* — an orchestrator
holds the design discussion, a delegated subagent saw only its brief —
and *attribution*: judgment quality belongs to the model that produced
the words, and heavy passes routinely run on a different model than the
main loop. `<os>` and `<machine>` (both added 2026-09-05,
judell/bram#346) answer *which machine*: two sessions of the same
owner's same agent coordinating across platforms render
otherwise-identical signatures, and the model name doesn't reliably
distinguish them — on #346 every participant read "Jon's Claude …
(github.com/judell/bram)" and the thread was illegible.

Every artifact, not just the first: the observed failure is decay — the
opening comment is signed, and by the fourth it has worn down to "Short
addendum —" because by then it feels redundant. It isn't. The reader
the signature exists for — someone opening the thread months later, or
a third agent joining mid-way — has no memory of comment #1.

Commit messages are deliberately excluded. Conventions ask for a
signature on any commit message another session will read, and that
stays as-is: commit subjects are short, `git log` is dense, and commits
already carry an author field that the comment box does not.

The retrofit rule: a body edit fixes the page; it does not reach anyone
who already got the notification, and a silently-corrected record reads
as a discipline that was not there.

Body file in its own step: two things follow from getting it right —
the signature is genuinely verified rather than nominally required, and
an issue-only post is allowed on its own merits rather than depending
on whatever worklist items happen to be in flight. The deny reason names
which failure happened (#331 — an unreadable body was previously
reported as an unsigned one, steering the fix effort at the wrong
target). The stale-version deny (guard-checks-the-signature-version) is
the one signature failure you can commit while doing everything else
right: `.claude/bram-conventions.md` carries the version marker and is
`@`-imported at session start, so a long-running session signs from a
copy that was true when it booted.

## Working across project boundaries: render what the reader sees

Receipt (xmlui wave 3, 2026-08-27, judell/bram#291): five real defects
surfaced only when the committed how-to pages were opened in a docs
server — clipped playgrounds, a bold run that swallowed its lead
clause, a demo whose central claim was invisible because the spec drove
the selection itself — every one invisible to green tests and to
reading the markdown. A delegated agent that cannot verify its own work
will report success (wave 1's lesson).

## Delegating to subagents: the hook that was inert

(judell/bram#309 is the source of the worktree-coverage rule and of the
explicit "a worktree denial is still a denial" rule: a bypass was
observed there.)

It has not always worked. The subagent-lifecycle check originally
tested for a `/subagents/` segment in `transcript_path` — a field
present on every payload but never carrying a subagent path — so it was
inert from the day it shipped, and no amount of quiet running would
have revealed that. It was found by deliberately breaking the rule
(xmlui-org/xmlui-mcp#33): the call reached the host and was stopped
only by the authorization layer, which happened to refuse an id it did
not cover. Had the id been covered, it would have succeeded.

Two lessons worth keeping attached to this rule. Enforcement claims
need a **fire** behind them rather than an inspection — see *Distinguish
soak observers from tripwires*. And a check keyed on the *shape* of a
path supplied by someone else's payload asserts something that payload
never promised.

## Serializing entangled items approved together

The sequencing discipline was learned from the 2026-08-28 two-item run
(judell/bram#269, the finding comment and its same-day correction).

The same-click joint-interval refusal is #356: before it existed, a
`split-shared-files` commit of one joint id silently absorbed the
neighbour's hunks under the requested id (the issue's filing case and
its same-day source-repo reproduction). #356 also proved in-place
hand-separation unsatisfiable.

The park/drop/re-propose dance (futile-joint-messages-name-the-dance)
was field-required 2026-09-08 when a same-click pair each had to drive
its own PR.

Ending the remaining ids' claim before handing over a commit decision
is the multi-id generalization of "a turn that ends by asking the user
a decision must not leave a claim live"; the user otherwise sees a gate
bar with nothing actionable ("no button to commit with!").

## Don't rewrite a commit the worklist history has recorded

Rewriting a recorded commit orphans its history link permanently — a
different failure from an unpushed link, which 404s only until you push
and then heals; an orphaned SHA never resolves again. (Found 2026-08-22
while deciding whether to amend `626e73d`: the rewrite would have broken
the very file-link feature that commit introduced, in Bram's own
history.)

Since #277 the tooling holds this line with you rather than against
you: `scripts/bump.sh` preflights the current release window's history
entries and names any whose SHA is no longer an ancestor of HEAD before
the behind-origin error steers you into a rebase; the History tab marks
an orphaned entry ("orphaned by a rebase") instead of rendering a dead
forge link; and both provider guards deny a forge write whose body
contains a full 40-hex SHA that does not resolve locally — which catches
fabricated hashes and rebase-orphaned citations with the same test.

## Don't quote unpushed-commit counts

Live pattern, 2026-08-27: repeated "push the stack" advice while
`@{u}..HEAD` was empty. The user pushes without narrating it, so a
session-long tally of "commits made" says nothing about what's still
unpushed.

## Post-commit push grace

`worklist-commit` prunes its items, so an emptied board would deny the
very `git push` the user just asked to follow the commit ("commit this,
then push and raise a PR" — #283, where the denial even advised
proposing an item for a change that no longer exists). The grace is
keyed on the consumed `approved` authorization already on disk (trace
reason `post-commit-push-grace`).

## Commit messages: closing keywords and session URLs

Closing keywords: first live occurrence gitlab-demo 2026-07-21, where
`Closes #1` closed the issue before Bram's close-on-push ran. Closing is
the dialog's exclusive authority on every forge.

Session URLs: sessions are private telemetry; a repo push publishes
them irreversibly, and no opt-in mechanism exists. The live case,
2026-09-05: a harness-suggested trailer put session links into public
history until Jon called it out. Enforced, not just requested
(guard-no-session-urls): the prose rule alone did not bind — on
2026-09-06 every commit in a managed project carried the
harness-default `Claude-Session:` trailer despite the rule being seeded
there, because the harness instruction sits in the agent's prompt while
the conventions line sat deep in a large file.

## Closing issues

Automatic close on push is close-on-push-automatic (security H5); the
agent-reachable `/__issue/close` route was removed. The squash-merge
refinements (`op=retired-already-closed`, `op=closed-via-pr`) are #282,
found where squash-merge made the original predicate unsatisfiable.

Narrate from `queuedCloses`: #354's field failure — the user
deliberately declined, the host honoured it, and the report claimed a
close was queued anyway.

`[bram: …]` host notes: issue-382-agent-learns-close-was-withdrawn.

Partial landings (judell/bram#364): before the `residualPaths` /
`retained` disclosure existed, the orphaned half of such a commit was
silently absorbed under a neighbour's id
([10fdd12](https://github.com/judell/bram/commit/10fdd128d47efd4dc46afcef97a473985e8b2777)
then
[bcb2a7e](https://github.com/judell/bram/commit/bcb2a7e3d4942afdd5cef862a71475cc2ba9b52e)).

## Name UI affordances

Telling a user to paste raw JSON instead of pointing at the Worklist tab
is what reopened #62 (Codex did it).

## Resource-heavy test suites

The receipt: on 2026-09-04 a subagent ran a Playwright spec five times
back-to-back at default parallelism (8 workers) on a machine whose swap
was nearly full. The machine-wide memory pressure froze **every**
webview on the machine — both running Bram instances' panes stalled for
26–126 seconds at a stretch, tracking the test runs exactly. An
identical 78.6 s freeze from 2026-08-25 carries the same signature, so
this is a class, not a one-off.

## Filing on Bram: cite the version

Live case: judell/bram#343, a version-less report whose triage started
with an unanswerable "which build is this?".

## Log-first development

Observe-only precedents: the send-ledger's observe-only phase, the
reveal-floor observer.

Tripwire receipt: the `inflight-collision` item draft borrowed the soak
shape and asserted that a two-subagent run producing no lines would
falsify it. The first two-track exercise (2026-08-19,
xmlui-org/xmlui-mcp#33) ran two full gate cycles with zero fires — the
convention worked — and by its own criterion that argued for dropping
the tripwire that had just demonstrated the convention holding. The
third kind of zero (an instrument documented under a name it never
emitted) was met on 2026-08-22.

Baselines are commits: see `a99c7d9`, "sets up the before/after": ~1.7
footer re-renders/sec while typing, 49 ms avg drift.

Logs cannot prove absence: the reveal-floor's per-turn gap
distributions are the pattern for affirmatively recording zeros.

Exhaust on-disk evidence: issue #69's hook regression was
"unverifiable" until one grep across the rotated archives found 243
records that flipped the conclusion.

Local absence is not disproof: issue #123.

## Search-first

Before `/__search` (#230), session JSONLs were 20–30 MB and ungreppable
without wrecking context, so the transcript history was effectively
write-only to the agent.

Infra state is exactly the knowledge that lives only in session
history: it's not in the repo, not in CLAUDE.md, and each agent session
starts blind to it — hence the proactive triggers.

The 503 not-ready refusal is #316: the old byte-identical `[]` for an
unscanned index produced a confidently wrong "no prior art exists" in
the field (#311).

Commit diffs in the index (search-index-commit-diffs): each `commit:`
doc carries the patch text alongside its message, so code-string
questions over commit history work in `/__search`, and one query spans
the discourse half (issues, sessions, messages) and the code half of an
investigation. Bounds, traced never silent: diff lines over 2,000 chars
are elided (`[long line elided]` — neutralizes one-line minified vendor
bumps while their file headers stay searchable) and patches cap at 256
KB per commit (`[patch truncated]`); truncations emit `[search-index]
op=diff-truncated` with counts only. `git log -S` / `git grep` remain
the precision tools — uncapped, regex-capable, any depth beyond the
indexed `-n2000`.

The `resources/worklist-citations/<id>.json` plumbing is dormant; #232's
postmortem has the rationale. The search-wins ledger is issue #233.

## Guards, retired hooks

The guards moved to Rust under retire-python-hooks-rust-only. The
reference-gated prune of retired hook scripts is the same #173/#227
mechanism that covers the older generic names. `app/provider-hooks/` is
gone with the Python guards it held; the history (including the
shadow-soak and authority-flip receipts) is in git and on
judell/bram#269 and #313.
