# Authorization in Bram

Bram's policy is that the user decides what may change, the host records and
checks that decision, and the agent carries out the authorized work. The
implementation enforces parts of this policy at different boundaries. It is
not a complete security boundary against arbitrary code running as the same
OS user, and a successful tool call does not establish user consent.

This is the source-repo entry point for that distinction: policy, shipped
checks, exceptions, and pending changes. Agents in managed projects follow
the self-contained [conventions](../app/__shell/conventions.md) and
[mechanics reference](../app/__shell/reference/worklist-mechanics.md), which
Setup installs there. This document is not seeded into those projects.

**Evidence baseline:** source inspection on 2026-10-21 at
`dfed26aa8e94e623d2a8954020b1e93bbbbf9aa8`, plus the linked history below.
Named functions are in [lib.rs](../src-tauri/src/lib.rs) unless stated
otherwise. Tests named here were inspected, not newly run for this
documentation change. Live receipts are attributed to their issue reports;
the direct-Drop exposure has not been reproduced in a new demo probe here.

## Authority and its limits

These facts answer different questions:

| Fact | What it establishes |
| --- | --- |
| Provider tool permission / sandbox access | Whether the provider lets a tool run or a process reach a resource. |
| Worklist coverage | An item declares a path. A proposal alone is not user approval. |
| Host authorization | A recorded kind and set of item ids, with freshness and interruption state. |
| Addressed turn | Which items the user's feedback concerns. It does not authorize a commit or Drop. |
| Execution claim | Which items are being worked on during a turn. It drives attribution and the spinner. |
| Line ownership | Which claim window owns changed lines. It determines what a commit includes, not whether a user approved it. |
| Commit / push / issue-close consent | Separate decisions with different effects. Approving an edit does not imply any of them. |

The protected pane records gate actions through Tauri IPC before delivering
the corresponding turn. Merely supplying an `approved:` or `drop:` string
is not the normal authority-creation path. Agent recommendations, external
content, and caller-supplied labels such as `via:"agent"` do not establish
human approval.

There are important limits to this model. CORS and the target iframe's
separate origin protect browser boundaries; a same-user process is not
CORS-bound. [The security assessment](security.md#threat-model) accepts that
residual. An agent-readable token file does not distinguish an external TUI
acting for the person from the agent acting for itself. Likewise, a phrase
found in a transcript proves the matcher found text, not that a protected
human-input channel produced it. These limits must accompany claims about
chat approval or external clients.

## Action matrix

The **policy** column governs agent behavior even where enforcement is
coarser. The next column describes current checks, not permission to exploit
their gaps. TTLs and the consumption distinction are explained below.

| Action | Policy / trigger | Shipped check and scope | Evidence / limit |
| --- | --- | --- | --- |
| Propose or revise a plan | Agent may author a proposal; user has not approved implementation. | Guard validates Worklist writes, including version increment and separate draft prose. | Worklist history; coverage can exist before approval. |
| Apply | Start approves the named change. | Host records `approved` ids and begins their claim. File tools check coverage; some stronger approval checks only observe. | `auth-record`, audit ledger, `begunAtMs`; see gaps below. |
| Iterate | Feedback on an unstarted item revises its proposal; feedback on begun work permits edits within its scope. | Host stamps addressed context and claim; guard narrows addressed writes. No new lifecycle grant. | Feedback history / claim intervals. Iterate alone does not advance, commit, or drop. |
| Advance | After authorized application, mark the work applied. | `validate_worklist_mutate_authorization`: `approved`, all requested ids, reconciliation freshness; target status must be `applied`. | Worklist history; resolved claim/auth ids retire. |
| Commit | Separate Commit approval, or explicit Start & commit. | `ensure_worklist_commit_authorized`: `approved`, ids, publish freshness; route checks work, paths, membership and commit-message restrictions. | SHA, audit ledger, history. The record does not retain a distinct apply-only versus commit-only kind; see gaps. |
| Drop with feedback | User chooses Drop; explain any bytes left on disk. | Host records `drop`; agent resolves and prunes; mutate checks ids and reconciliation freshness. | Drop history. Removes the row, not the edits. |
| Drop without feedback | User chooses Drop in the pane. | `handle_worklist_drop_direct` records authorization itself, resolves and prunes in one HTTP call. | `worklist-drop op=host-direct`; caller identity is assumed, not established by the handler. |
| Clear landed rows | Agent may clear rows for work it committed outside the gate; user may request cleanup. | `handle_worklist_clear_landed` checks repository evidence per id; no Drop grant required. | Landing SHA, `op=clear-landed`, history; partial refusal is possible. |
| Do recommended | Agent posts a plan; user clicks to execute it. | Host validates plan ids/verbs and serves it only for the current board version; pane uses existing Drop/Commit/feedback paths. Feedback and commit cannot share a plan. | Plan and existing gate traces. Posting the plan grants nothing. |
| Direct edit | Same-turn `just do it` or `skip-worklist:` opts out of proposal-first for the requested work. | Claude reads user-turn text; host writes a wildcard direct-edit sidecar for Codex / structured prefix. Guard checks its one-hour lifetime and paths. | Direct-edit audit breadcrumb. A matching phrase/sidecar is not a lifecycle grant. |
| Plain git / forge work | Requires the user's request; issue-only work needs no Worklist item. | Separate guard rules, not `ensure_worklist_commit_authorized`; Bash coverage is coarse. Forge signatures and session-URL checks apply to inspectable writes. | Git / forge records. Worklist guarantees cannot be projected onto all shell commands. |
| Push | User decides separately, normally with Push; agent pushes only when asked. | Host Push path and shell guard are different surfaces; the shell guard has a post-consumption push grace. | Remote reachability is separate from local commit success. Grace is not proof of push consent. |
| Queued issue close | User ticks the close choice; host queues against the new SHA and closes after push. | `enqueue_issue_closes_from_auth` and push-time processing; commit route rejects forge closing keywords that bypass the dialog. | `queuedCloses` / host close records. Direct forge issue actions follow the separate explicit-request policy. |

## Records, transport, and lifecycle

### Where the implementation lives

| Boundary | Implementation |
| --- | --- |
| Pane records an action before sending it | `submitAuthorizedWorklistTurn` in [helpers.js](../app/__shell/helpers.js); Tauri `record_worklist_action_authorization` → `write_worklist_authorization_record` |
| Build the grant | `parse_worklist_authorization_payload`, `build_worklist_authorization_record` |
| Read recorded bodies | `handle_worklist_resolve` |
| Advance / prune | `handle_worklist_mutate`, `validate_worklist_mutate_authorization`, `validate_post_commit_prune_status` |
| Commit | `handle_worklist_commit`, `ensure_worklist_commit_authorized` |
| Drop / verified cleanup | `handle_worklist_drop_direct`, `handle_worklist_clear_landed`, `worklist_item_landed_since` |
| Recommendation | `validate_worklist_plan_steps`, `worklist_plan_current`; pane executes the current plan |
| Codex filesystem transport | `drain_worklist_intent` dispatches to the same handlers |
| Tool enforcement | [guard.rs](../src-tauri/src/guard.rs) dispatches hooks; [guard_policy.rs](../src-tauri/src/guard_policy.rs) implements decisions for both providers |
| Retirement / interruption | `retire_worklist_authorization_record`, `shrink_inflight_claim_sentinel`, `invalidate_worklist_authorization`, `worklist_auth_freshness_ok` |

The ordinary two-stage flow is:

```text
Start click → host records approved ids + execution claim → apply turn
            → edits → advance → applied row, resolved grant/claim retired
Commit click → fresh approved record → commit → history + prune
Push decision → publish commit → process explicitly queued issue closes
```

Start & commit uses one approval and skips the advance. A begun proposed
item with changes may also be committed without first being advanced.
The pane's decision and the route's acceptance checks must be described
separately: the route checks the approved ids and actual work, not the
human-facing button label.

The authorization lives in `resources/.worklist-authorization.json`, with
`kind`, `ids`, recorded item bodies/feedback, `issuedAtMs`, `source`,
`consumedAtMs`, `interruptedAtMs`, and `commitToo`. It is a **single slot**:
a subsequent gate action replaces it. `commitToo` records the
apply-and-commit form; it is not a general distinction between apply and
commit authority. `begunAtMs` on the item records that work was started;
it is not a perpetual commit grant.

Execution lives in `resources/.inflight-claim.json`. Turn-end clears the
claim while a still-live approval may survive and rearm a later turn.
Addressed context is separate again. Thus a general turn with an active,
rearmed grant is different from an unrelated chat edit with no claim.
Attribution follows claim windows; see [the attribution model](attribution-model.md).

### Resolve, consumption, expiry, and interruption

`resolve` delivers recorded bodies and establishes a claim. It stamps
`consumedAtMs` for **approved** records; later resolver reads report
`no_active_authorization`. It does **not** consume Drop records on read.
It is not the final freshness check: mutate and commit independently check
the stored record. An approved resolve is supported when the installed
provider instructions require it; the normal Start path already has a
claim and does not need this extra read.

The mutation and commit validators deliberately do not treat
`consumedAtMs` as a blanket denial, so resolve → edit → mutate works.
Consumption therefore must not be described as preventing every replay.
In the ordinary unconsumed multi-id flow, retirement removes resolved ids
and preserves the rest; final retirement marks the record consumed.
If resolve has already consumed the whole record, the retirement helper
returns `AlreadyConsumed` instead of shrinking it. The board, route checks,
and successful commit removal also matter to repeat-call behavior.

`WORKLIST_AUTH_TTL_MS` is **30 minutes**. Properly issued records expire
against their original issuance time; retiring one id does not refresh the
others. `worklist_auth_freshness_ok` currently applies the age check only
when `issuedAtMs > 0`, another reason not to treat arbitrary writable
sidecar bytes as tamper-proof authority.

An interrupt marked on the record blocks **Publish** (commit and uses of
publish freshness), but permits **Reconcile** (advance/prune with the right
kind and ids), still subject to TTL. This was an intentional recovery change
in #255. `invalidate_worklist_authorization` skips already-consumed records;
that detail limits blanket claims that every interrupt revokes every later
route call. An expired mutate refusal also releases the refused ids' claim
so a dead approval cannot keep the board locked.

What counts as an interrupt depends on the agent. For Claude and Codex a
user Esc in the terminal or from the pane marks it at the keystroke
(`pty_menu_clear`). OpenCode stops a running turn only on a second Esc
within 5 s, so a single Esc interrupts nothing there. With Bram's OpenCode
plugin in place, the mark (and the claim clear) wait for OpenCode's own
`session.execution.interrupted` event (`opencode_turn_interrupted`), which
covers any stop, Esc-caused or not. Without the plugin no such event
arrives and the keystroke marks it, as for the other agents
(opencode-esc-interrupt).

### Direct edits and provenance

The policy opt-out is scoped to the user's request in the same turn; agents
must not carry it forward as standing permission. Its implementation differs:

- Claude's prose matcher reads the last user message, including referenced
  feedback drafts and outbound-turn files (#431), and emits a best-effort
  audit breadcrumb.
- The host recognizes Codex prose opt-outs and the structured prefix, and
  writes `resources/.worklist-direct-edit.json` with wildcard paths and a
  **one-hour TTL**. The guard checks time and paths, not a turn identity.
  Thus the stored grant can mechanically outlive the initiating turn.
- The dedicated sidecar survives a later gate click (#352). The guard still
  accepts legacy direct-edit records in the shared authorization file.

None of these creates an approved/drop record for lifecycle calls. General
chat-to-commit authorization remains a separate design question. Whether a
particular shell command passes its guard is also a separate question from
whether the user requested it.

### Transport does not confer authority

HTTP callers use the routes in [the API reference](apis.md#3-worklist--authorization).
Codex lifecycle calls use an intent file containing `{nonce, route, body}`;
the host returns a matching-nonce result with `ok`, `status`, and
`result` or `error`. The four dispatched routes are `worklist-resolve`,
`worklist-mutate`, `worklist-commit`, and `worklist-end`.
Unknown routes fail; HTTP Drop, clear-landed and plan have no equivalent in
this dispatcher at the baseline. A successful request delivery is not
approval. A missing/mismatched result or `ok:false` must be surfaced.

## Exceptions and enforcement gaps

**Verified cleanup is an intentional exception.** `clear-landed` rechecks
each id against Git. Declared paths must be clean; a qualifying commit
outside the Worklist must have touched them after the relevant starting
point. Claim evidence further excludes begun items with no work of their
own. For never-started items, the state mirror's first-seen time supplies
the starting point and the commit message must name the item. These are
evidence rules, not proof that the proposal's full semantic intent was met.
The route clears eligible ids and reports refused ones; `via` is telemetry.

**Direct Drop assumes its caller.** The feedback-less Drop route introduced
for #340 makes deletion immediate instead of waiting for an agent turn.
It creates its own grant without establishing that a pane click happened.
The later #389 discussion already recognized this exposure. Its existence
is not permission for an agent to bypass Drop approval. Protecting that
route is separate runtime work, not accomplished by documenting it.

**Coverage is weaker than approval.** File-aware tools can pass because a
proposed/applied item declares the path. `trace_would_deny_unaddressed`
records stronger approval findings; `would_deny_reason` distinguishes
`proposed-only-coverage` from `general-turn-item-edit`. These are
observations, not denials. The Bash branches are coarser still: any active
coverage can satisfy the general write check. #387 added observation to
previously unseen surfaces; logging did not turn the observation into a
blocking rule. The policy to wait for approval still binds.

**The grant does not encode every policy distinction.** Both Start and
Commit use kind `approved`; only apply-and-commit adds `commitToo`.
`ensure_worklist_commit_authorized` checks kind, ids and freshness, not a
separate commit-only capability. The post-commit prune check accepts
approved ids with applied status, or with `commitToo`; it does not itself
prove a commit occurred. Agents must use the prescribed gate sequence and
let the commit route prune, despite these coarser checks.

**Consumption is not comprehensive revocation.** Resolve and interruption
have the limitations described above. Authority files and the audit ledger
are same-user writable artifacts, not cryptographically protected records.
The ledger is best-effort, metadata-only and not tamper-evident (#229).
The host watcher and hooks provide defense in depth, not OS isolation.

**Installed instructions can lag.** Setup copies operational instructions
into managed projects; running sessions can retain older text. Content-hash
verification was removed, although item hashes may still exist in served
data. Old text naming Python guards, requiring hash-verified approval, or
requiring resolve on every apply does not describe the same implementation
generation. Follow the active higher-priority provider instructions during
a session and repair the source/Setup drift explicitly, rather than quietly
treating inconsistent text as a new policy.

## Decisions and pending work

| Decision / evidence | What it means now |
| --- | --- |
| [#108](https://github.com/judell/bram/issues/108), [#119](https://github.com/judell/bram/issues/119), [#113](https://github.com/judell/bram/issues/113) | Host-owned authorization and origin isolation are the intended defenses; same-user loopback access remains an accepted residual. |
| [Interrupt recovery, 2da168f](https://github.com/judell/bram/commit/2da168f9a254ab0d3fbbc85ef1d46140e95015c2) | Reconciliation after an interrupt is deliberately different from publication. |
| [Separate claims, debcbbf](https://github.com/judell/bram/commit/debcbbf25f9ea32310c4840288b6a9ef201fe7f0), [#352](https://github.com/judell/bram/issues/352) | Claims end with turns; authorization may endure. Direct-edit grants have their own slot. |
| [#389 correction](https://github.com/judell/bram/issues/389#issuecomment-5788067353), [revised direction](https://github.com/judell/bram/issues/389#issuecomment-5805456919) | External terminal ownership/read integration is compatible with the gate. Broad HTTP arming behind an agent-readable token was challenged and withdrawn as the immediate direction. |
| [#400](https://github.com/judell/bram/issues/400), [#402](https://github.com/judell/bram/issues/402) | Lighter ceremony and phone/chat control are proposals, not blanket shipped authority. |
| [#406](https://github.com/judell/bram/issues/406), [never-begun cleanup, cd80a16](https://github.com/judell/bram/commit/cd80a16c357b73efd95d9fb7fc2f37e14d4b6ec7) | Host-verifiable cleanup is already allowed independently of Drop approval. |
| [#413 shipping report](https://github.com/judell/bram/issues/413#issuecomment-5899296567) | A version-bound recommendation plus a human click uses existing authority paths. A plan alone does not authorize itself. |
| [#325 correction](https://github.com/judell/bram/issues/325#issuecomment-6154153257) | A later gate click replaces earlier authorization; keyed storage remains proposed. |
| [Hash removal, e2df782](https://github.com/judell/bram/commit/e2df782fabd1ea8ac485b2cfa733e52eb6223921), [Rust-only hooks, 1378e3e](https://github.com/judell/bram/commit/1378e3e7006ffb1c2902cf866049bc3aa9813cb5) | Do not describe the old hash-verification or Python-hook architecture as current. |
| [#446](https://github.com/judell/bram/issues/446) | Still proposed: host-check the user's opt-out phrase against an already-posted, current Drop plan and refuse items with uncommitted work. It grants no commit permission. |

#389 and #446 can coexist: one changes the surrounding client/terminal,
the other proposes a bounded additional source of Drop consent. The open
decisions are how to establish that consent's provenance and how to protect
the existing direct-Drop path. This document does not settle them or expand
the agent's authority.

## Looking Forward

The direction is decided:
[Authorization becomes one pipeline](decisions/2026-10-21-authorization-one-pipeline.md),
which carries Codex's and Claude's reviews. It governs where it differs from
this document's current-state account. Codex's original principles are in
`c240f5b`.

## Target grant semantics (migration step 0)

This is the contract the decision's migration steps 1 to 5 implement. It is
**not shipped behavior**: nothing calls it yet. The pure state machine in
[grant_lifecycle.rs](../src-tauri/src/grant_lifecycle.rs) encodes it, with
tests for each transition in the table below. The tests pin the code's
transition behavior; keeping this prose table aligned with them remains a
review responsibility.

**Out of scope here:** how each route calls the grant book, and the final
storage decision (#325). **Intended store:** the state mirror's database
(`worklist_state.rs`). The grant book is shadowed there first, under
migration discipline rule 1, and becomes authoritative at the switch. This
would be the mirror's first phase-B use for authorization. **Open
question:** the guard is a separate process. It reads
`resources/.worklist-authorization.json` directly for grant age
(`guard_policy.rs`, `AUTH_REL`) and uses that file's existence as the
project-root marker. It will need to read the database itself, or ask the
host over the loopback port, and the root marker must stay a file.

### The grant

A grant is keyed by **(item, verb)**, where the verb is `apply`, `commit` or
`drop`. It records:

- `confirmedBy`: `click` or `phrase`;
- the plan version it confirmed, when it confirmed a plan;
- `issuedAtMs`;
- its state.

Grants under different keys never affect each other. A new authorization
for a key whose grant is `live` ends the old grant as `replaced`, and the
ended grant goes back to the caller with its receipt, so the record of the
replacement is kept. A grant that is `uncertain` can't be replaced until it
has been reconciled.

### Evidence-based row retirement

`clear-landed` is not a third way to authorize a grant. The host verifies
that an item's declared paths are clean and that a qualifying commit landed;
for a never-begun item, the checks also require a commit after the row first
appeared whose message names the item. The verified evidence records the item,
commit SHA, verification time, and whether the item had begun. It makes that
row eligible for retirement only. A separate evidence-retirement receipt
records the evidence and retirement time. This evidence does not authorize
apply, commit, or a Drop of work that has not already landed, and it is never
recorded as user consent.

### States

| State | Meaning | May execute? |
| --- | --- | --- |
| `live` | Authorized, not yet completed. | yes |
| `uncertain` | An attempt's outcome wasn't observed. | no: reconcile first |
| `completed` | The operation happened. Terminal. | no |
| `ended(reason)` | `cancelled`, `expired`, `stale` or `replaced`. Terminal. | no |

### Transitions

Each event either moves the grant and appends one receipt entry, or is
refused, leaving the state and receipt unchanged. **Read** is a no-op in
every state, and appends nothing.

| Event | from `live` | from `uncertain` | from `completed` / `ended` |
| --- | --- | --- | --- |
| execute → ok | `completed` | refused | refused |
| execute → known failure | `live` (retry allowed) | refused | refused |
| execute → unobserved | `uncertain` | refused | refused |
| reconcile → happened | refused | `completed` | refused |
| reconcile → did not happen | refused | `live` | refused |
| cancel | `ended(cancelled)` | refused | refused |
| TTL elapsed | `ended(expired)` | refused | refused |
| plan stale | `ended(stale)` | refused | refused |
| replaced by new authorization | `ended(replaced)` | refused | refused |
| read | unchanged | unchanged | unchanged |

**Why `uncertain` refuses cancel, TTL and stale.** Whether the operation
happened is unknown, and ending the grant would discard that question. A
commit that did land changes the board, which makes its own plan stale, so
checking staleness first would misread success as staleness. Reconciliation
comes first. A grant that reconciles to `live` then faces the usual TTL and
staleness checks.

A **retry** is just another execute on a `live` grant. It isn't a new
authorization and adds no `authorized` entry.

### The receipt

The receipt is append-only, one per grant. Its entries:

- `confirmed(by)`, once, when the grant is created;
- `attempt(ok | knownFailure | unobserved)`;
- `reconciled(happened | didNotHappen)`;
- `ended(reason)`.

The grant receipt is how a phrase stays distinguishable from a click after
the fact. Evidence-based retirement has its separate receipt described above.

### What this structure simplifies and enables

Keying grants by item and verb removes unrelated permissions overwriting one
another. Reads no longer consume authority, and explicit terminal states
replace today's consumed-but-still-usable cases. The same transitions make
known-failure retry and uncertain-outcome reconciliation explicit, with a
receipt for each attempt.

Once a caller uses this contract, the Worklist can give apply, commit and
Drop one host executor each while preserving whether permission came from a
click or a phrase. That supports the migration's first steps:
`/__worklist/drop` stops recording its own grant, so the pane's Drop
confirms through its own channel and every Drop runs on one host executor;
then apply and commit grants separate; and later a current recommended plan
can be confirmed by a phrase for #446.
Evidence-based `clear-landed` can share the row-retirement executor while
remaining visibly distinct from consent. The contract is independent of its
storage backend, so the mirror can first shadow it and later become
authoritative after the guard's direct-file dependency is resolved.

## Migration discipline

Every migration step from the decision follows these rules. They come from
the last large migration, replay → membership (#425,
[decision](decisions/2026-09-22-membership-over-replay.md)). Its shadow
comparison and its one-consumer-at-a-time switch worked. Five things didn't:

- The soak was three days, on one machine's board shapes.
- The no-answer refusal shipped with no unit test and had never fired live.
- That refusal failed closed for every commit.
- The fallback was deleted six days after the gate switched.
- The regression check (`membership-starter-check.py`) arrived after the
  field incident, #437 on 2026-10-13.

The rules:

1. **Shadow before switching.** After step 0, the grant book runs beside
   the current record. It receives the same events and decides nothing.
   Whenever the two would answer a route call differently, it logs
   `op=grant-diverges`. The divergence tripwire in `worklist_state.rs` is
   the precedent. Users see no change during the shadow phase.
2. **Exit criteria are written before each step starts.** At minimum:
   - a stated number of gate operations across at least two machines,
     including one non-macOS board;
   - every divergence ruled on, with none unexplained.

   The step's item records the criteria and the evidence that met them.
3. **Switch in one release and delete in a later one.** The old path stays
   available while a switch is in the field.
4. **Fall back; don't refuse everything.** While both paths exist, an
   internal error in the new path falls through to the old one and is
   traced. A final step removes the fallback once the trace shows it has
   stopped firing.
5. **Every refusal and failure path ships tested and fired.** That means a
   unit test, plus one live firing on a demo board before the release.
   This includes the uncertain outcome.
6. **Authorization starters come first.** Before step 1, add starters to
   `scripts/demo_instance.py` for the shapes that have caused trouble,
   each with an expectation table like the membership check's:
   - a Drop while an approval is live (#325);
   - an expired approval;
   - an interrupt mid-apply;
   - a commit that times out after landing.
7. **Smallest blast radius first.** Step 1 changes one route and one pane
   call. Step 2 changes every Start and Commit, so it gets the longest
   soak.
8. **Read both records during the transition.** A pane and host from
   different builds (during an upgrade), or a grant in flight across the
   code change, must still resolve.

There is no deadline between steps. A soak that turns up divergences holds
the next step until they're explained.

## Verification and maintenance

Existing source tests give useful anchors:

- `approval_record_embeds_resolved_item_and_feedback` and
  `apply_and_commit_gate_sets_commit_too` describe recorded input.
- `mutate_authorization_rejects_wrong_kind_and_missing_ids` and
  `worklist_commit_authorization_requires_approved_ids` cover route checks.
- `plural_authorization_retires_committed_ids_incrementally`,
  `h4_interrupted_authorization_cannot_publish_but_can_reconcile`,
  `h4_stale_authorization_past_ttl_is_rejected`, and
  `h4_consumed_but_uninterrupted_drop_still_prunes` pin lifecycle distinctions.
- `would_deny_reason_separates_unstarted_proposals_from_begun_items` in
  `guard_policy.rs` tests classification, not blocking enforcement.
- `worklist_plan_withheld_tests::names_each_reason_and_serves_a_current_plan`
  covers stale-version and missing-id recommendations.

For incident verification, join `gate-click`, `auth-record`,
`inflight-sentinel`, `worklist-intent`, `worklist-drop`, and commit/history
records with `resources/bram-traces/hook-events.log`. See the
[trace vocabulary](trace-vocabulary.md). A source-level check or a past
test receipt must not be presented as a new live probe.

This account was assembled with Bram fulltext searches over issues,
commits, worklist history and sessions, then checked against current code.
Useful `mode=raw` queries are `"self-authorization" OR "arming"`,
`"clear-landed"`, and `"would-deny" OR "authorization" "TTL"`.
Search results are ranked and bounded; copied instructions in a session are
not independent evidence of a guarantee.

An authorization-changing PR must update this matrix, the corresponding
seeded instructions, and the affected API entry together. It must name the
enforcement point and evidence, and distinguish a policy decision from an
implementation change. Keep the responsibilities clear:

- This document owns the architectural account and its known gaps.
- Seeded conventions/mechanics own executable agent instructions and must
  remain useful without this repo's `docs/` directory.
- [security.md](security.md) owns the wider threat model and historical audit.
- [apis.md](apis.md) owns wire shapes; [conventions-rationale.md](conventions-rationale.md)
  retains the historical reasons behind operational rules.

Documentation validation should walk the normal two-stage and one-click
flows, iterate, interrupt/expiry, both Drop paths, verified cleanup,
stale recommendations, direct edit followed by a gate click, proposed-only
coverage, and proposed #446. Runtime changes require their own tests and
live evidence; a prose correction does not repair a runtime gap.
