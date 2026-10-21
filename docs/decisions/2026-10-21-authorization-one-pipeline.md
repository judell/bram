# Authorization becomes one pipeline, designed for a cooperative agent that makes mistakes

Status: decided 2026-10-21 (Jon, with Codex's and Claude's reviews below) · revisit when Bram must resist a hostile same-user process, or an external client needs its own way to confirm

**Question:** how should Bram's authorization be simplified before more single-purpose routes such as #446 are added?

**Decision:**

- **Design target: a cooperative agent that makes mistakes.** Not a hostile process running as the same OS user, which [authorization.md](../authorization.md#authority-and-its-limits) already says Bram can't stop. A check earns its place by catching mistakes: wrong ids, a stale board, edits outside an item's scope, a commit nobody asked for. Checks that only half-defend against an adversary we've conceded we can't stop are candidates for removal. Jon agreed to this target on 2026-10-21.
- **Scope: the three Worklist verbs, apply, commit and drop.** Direct edits ("just do it" / `skip-worklist:` for file changes), Push and queued issue closes keep their own permissions. This note doesn't change them, and "operation" below means only the three verbs. #413 plans can also carry **feedback** steps (`8045e8f`). Those stay on their existing path: they send an addressed turn and grant nothing. Bringing them in would need its own decision.
- **One pipeline for those verbs:** propose an operation → authorize it → grant → execute → receipt.
- **Two ways to authorize, which are different in kind:**
  - **User confirmation.** A **click** (the pane writes and confirms the plan through its own channel, Tauri IPC), or a **phrase** ("just do it" in the user's own turn, checked by the host against a specific, current plan).
  - **Evidence eligibility.** Repository facts show that the work has already landed, so retiring the row records what happened rather than deciding anything. This is `clear-landed`. It applies only to retiring rows, keeps its own checks, and is never described as user consent.
- **One executor per verb**, whatever authorized the operation.
- **Grants are kept per item and per verb** (#325). Reading a grant changes nothing.
- **Grant lifecycle:**
  - A grant is **live** from authorization until it **completes** or **ends**.
  - **Complete:** the operation succeeded and the receipt is written. The grant retires.
  - **Known failure:** the host observed that the operation didn't happen, such as a refusal before any write, or an error with the repository unchanged. The grant stays live, so a **retry** of the same operation reuses it and isn't counted as a new confirmation. The failure goes on the receipt.
  - **Uncertain:** the outcome wasn't observed, as with a timeout or a lost response. A failed response can follow a commit that succeeded. The receipt records `uncertain`, and nothing retries the operation blind. The host first reconciles against repository state and any receipt:
    - if the operation happened, the grant **completes**;
    - if it didn't, the outcome becomes a **known failure**.

    This is the existing commit rule, "on a timeout, never re-POST: check `git log` first" (`app/__shell/conventions.md`), stated for every verb.
  - **End without completing:** the user cancels, the grant expires (a TTL counted from authorization), or the plan it confirmed goes stale because the board version or the items changed. The receipt says which, and a new attempt needs a new authorization.
  - At any moment a grant is exactly one of live, uncertain, completed or ended. There's no state like today's "consumed but still usable."
- **Receipts record how the operation was authorized**: click, phrase or evidence. A phrase is never presented as a click, and evidence is never presented as consent.
- **No route records its own authorization.**
- **Every migration step deletes a path or an exception.** A step that only adds one isn't simplification.

Under this model, a Start, Commit or Drop click is the pane proposing and confirming in one step. Do recommended (#413) is the agent's plan confirmed by a click. #446 is the agent's plan confirmed by a phrase. `clear-landed` is row retirement made eligible by evidence.

**Evidence:**

- [authorization.md](../authorization.md) needs 13 action-matrix rows, spread across seven documents, to describe what ships today.
- `handle_worklist_drop_direct` records its own grant without establishing that a click happened (#340, `c2ff3e6`).
- Start and Commit share kind `approved`, and `commitToo` patches the gap. A Drop record isn't consumed when it's read; an approval is, though not completely.
- Some approval checks only log what they would deny (#387).
- [#389, 2026-09-23](https://github.com/judell/bram/issues/389): a token file the agent can read can't tell the user's own client from the agent.
- #413 already makes recommendations data, bound to the board version.
- #446's case on its own terms: what a phrase confirms is narrow. It's a plan the agent posted before the user agreed, still current when the host checks it, limited to the plan's drop steps, and limited to items with nothing of their own on disk. The host reads the phrase from the user's own turn, not from the agent's account of it.

**Migration order**, each step its own Worklist item:

0. Settle the grant and receipt semantics, including the uncertain outcome, with tests, before changing code (Codex's follow-up).
1. The pane's Drop records its grant through IPC, and `/__worklist/drop` stops minting one.
2. Apply and commit become separate grants, which retires `commitToo`.
3. A plan plus a confirmation becomes the single path for operations the agent originates. #446 is a phrase-confirmed plan.
4. Grants follow the lifecycle above: reading changes nothing, completion retires, a known failure keeps the grant for retry, an uncertain outcome is reconciled before anything else, and cancel, expiry or a stale plan end it.
5. Each observation-only check is either made to block or deleted.

**Alternatives:**

- *Keep per-route rules and document them.* Rejected: that's how the current account grew to its size.
- *HTTP arming verbs behind a token file* (#389's proposed walk step). Rejected: the token doesn't establish who is calling.
- *Treat a phrase as equal to a click.* Rejected: it's weaker evidence. It's adequate for this design target, and the receipt records which one it was.
- *Count evidence as a third kind of confirmation.* Rejected (Codex's review): evidence establishes that something already happened, not that the user agreed to something. It authorizes only row retirement, with a receipt that says so.
- *Fold direct edits, Push and issue close into the pipeline.* Not now (Codex's review): they have separate permissions with different effects. Bringing any of them in would need its own decision.

**Revisit when:** Bram needs to defend against a hostile same-user process (an OS-level boundary, which is a separate project), or an external client such as #389's TUI needs its own way to confirm.

## Codex review · 2026-10-21

*Claude, 2026-10-21: the decision body above now incorporates all four points below: scope, evidence eligibility, the grant lifecycle, and #446 argued on its own terms. The review is kept as written. Its closing "three confirmation sources" is superseded by the body: two ways to confirm (click, phrase) plus evidence eligibility for retiring rows.*

**Position:** I support the direction, with these clarifications before this
becomes a decided policy:

- **Scope:** say this pipeline governs the three Worklist verbs. Direct edits,
  Push and issue close have separate permissions and should not silently be
  swept into “every operation.”
- **Evidence:** `clear-landed` is evidence-based eligibility for retiring a
  row after repository work has landed. It is not the same kind of user
  confirmation as a click or phrase. Keep its checks and receipt distinct
  while routing eligible row retirement through the shared executor.
- **Lifecycle:** define expiry, cancellation, and retry. A failed or
  interrupted execution must not leave an ambiguous grant; a retry of the
  same confirmed operation should not require pretending it was a new
  confirmation.
- **Risk comparison:** phrase-confirmed Drop should be justified as a
  host-validated, current, narrowly scoped plan. Direct edits can have broader
  effects, but that does not make their risk interchangeable with dropping a
  row; compare the guarantees and limits directly.

These points refine the proposal rather than reject its core: three verbs,
three confirmation sources, grants per item and verb, one executor, and a
receipt that preserves how consent was obtained. The note remains **proposed**
until Jon, Claude and Codex agree on the clarified scope and lifecycle.

— **Codex**

## Codex follow-up · 2026-10-21

*Claude, 2026-10-21: all three points below are now in the body: the wording note under the review, feedback steps in Scope, and the uncertain outcome in the lifecycle, with Codex's semantics-first step as migration step 0. The follow-up is kept as written.*

**Position:** the revised lifecycle is a useful target, but I recommend
resolving three details before adopting it:

- **Evidence is not a confirmation source.** The body correctly separates
  user confirmation (click or phrase) from evidence-based eligibility
  (`clear-landed`). Its closing review sentence still says “three
  confirmation sources.” Change that to two confirmation methods plus an
  evidence-based retirement path, so the receipt never implies that Git
  evidence records consent.
- **Keep the scope boundary explicit.** The current recommendation-plan
  implementation also supports feedback steps. Say whether feedback stays on
  its existing path or is deliberately brought into a later decision; the
  proposed apply/commit/drop pipeline should not silently absorb it.
- **Define uncertain completion before enabling retry.** A failed response
  may follow a successful commit. In that case, blindly retrying could repeat
  an operation that already completed. Record an explicit uncertain outcome
  and reconcile against repository state and the operation receipt before
  retrying or requesting new authorization. A known failure can retain its
  grant for retry; an unobserved outcome should not be treated as a known
  failure.

I support moving forward with small, independently reviewable migrations:
first settle these semantics and tests; then route ordinary Drop through the
host executor and remove route-local grant creation; separate apply and
commit grants; implement lifecycle and receipt handling with uncertain
outcome reconciliation; finally make or remove observation-only checks one
at a time. Each step should retire an existing path and update its docs.

— **Codex**
