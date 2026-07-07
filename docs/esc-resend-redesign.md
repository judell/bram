# Esc / strand / resend redesign — plan of record

## Current status

Implemented through phase 3 for v0.2.17: Bram now records every
agent-pane send in the host-side ledger, confirms delivery from the
session JSONL, restores user-caused or retracted sends into the composer,
and removes the old Resend button from the recovery path. Mechanical
auto-resend remains trust-gated off by default until more soak time proves
zero false strands.

## Why this document exists

On 2026-07-03, while testing the turn-transport redesign, an Esc during an
in-flight turn popped a queued outbound-turn **frame** (opaque plumbing text)
back into the terminal input. The user cleared it by hand; the bounce banner
appeared; nothing could restore the message. Verdict on approving the
transport cleanup: *"the esc/resend mechanism is a botch, needs total redo."*

The mechanism is not one design but accreted layers, each patched in against
one observed failure mode. This document names the layers, the root problems,
and the contracts for the rebuild — before any code changes, per the
turn-transport playbook (`docs/turn-transport-redesign.md`).

## Inventory: the current layers

1. **Bounce detector** (host, `lib.rs` ~7892–7959): on an agent-pane Esc,
   if the grid reported no first output AND a toTurn landed within 120 s,
   emit `message-bounced`. Observe-only; cause-anchored heuristics.
2. **First-output-seen reporter** (parent shell grid → host cell,
   `report_first_output_seen`): grid-derived boolean the bounce detector
   pairs with send recency. Observe-only groundwork (#210).
3. **Bounce banner + Resend** (iframe, `Main.xmlui` ~475–487): banner on
   `message-bounced`; **Resend sends a bare `\r`** — it re-submits whatever
   still sits in the terminal input. It does not resend any stored message.
4. **Send/esc capture scrapers** (parent shell, `main.js`): fingerprint
   terminal rows around sends and escapes (`send-capture` / `esc-capture`
   traces). Forensics only; nothing consumes them at runtime.
5. **Post-escape windows** (host): 15 s no-kill window after Esc, double-tap
   hard kill, single-Esc soft turn-end. (Turn-lifecycle semantics — mostly
   fine, and explicitly NOT the target of this redo.)
6. **Workspace landing tracking** (iframe): `awaitingResponse` +
   `submittedWorklistMessage` + baseline in localStorage, compared against
   the exchange via `__bramWorklistSubmittedMatches`, cleared by four
   separate turn-end listeners.

## Root problems

- **Recovery has no source of truth.** Resend presumes the message still
  sits in the input; Esc-restore shows the frame, not the user's words.
  Nothing recovers from the durable artifact.
- **Detection is cause-anchored, not outcome-anchored.** Each detector keys
  on a cause (Esc pressed, no first output) instead of the one question that
  matters: *did the send land as a user turn?* Causes multiply; the outcome
  is singular.
- **State is scattered** across host cells, parent-shell captures, and
  iframe localStorage, with no single record of a send's lifecycle.
- **Envelopes changed the ground truth and nothing uses it.** Every
  substantial send now has a persistent envelope with a stable id, and the
  frame carries that id into the session JSONL — landing confirmation for
  enveloped sends is an exact id match, not a text heuristic. None of the
  six layers exploit this.

## Invariants for the rebuild

### 1. One outbound-send ledger (host)

Every toTurn send gets a ledger entry at injection time: send id (the
envelope id when framed; a generated id otherwise), the collapsed inline
text or envelope ref, and a state machine: `injected → landed | stranded`.
The ledger is host-side, in one place, and is the ONLY source the UI reads.

### 2. Outcome-based landing detection (host)

The host owns the session JSONL and the turn projection. It confirms
`landed` by observing the send appear as a user turn — exact envelope-id
match for framed sends, normalized-text match for inline sends. `stranded`
is `injected` + agent settled + never landed. No Esc anchoring, no grid
heuristics in the decision; the grid signal may remain as a fast hint only.

### 3. Recovery from the envelope, never from the terminal — and no Resend button

A Resend button is an admission of uncertainty; the ledger removes the
uncertainty, so the button goes away entirely (user direction,
2026-07-03: "needing a resend button is a smell we should make go
away"). Recovery is a taxonomy, not an affordance:

- **Mechanical strand** (no user interrupt in the send's window: paste
  raced the TUI, submit CR didn't take, ConPTY truncation):
  **auto-resend** from the ledger/envelope — silent, traced
  (`[send-ledger] op=auto-resend`), bounded to one retry per send id.
  Safe by construction: landing detection proved non-delivery, and the
  envelope preserves fidelity.
- **User-caused strand** (an Esc falls between injection and strand
  detection): never auto-resend — Esc-then-strand can be intentional
  cancellation. Restore the user's words to the composer as an editable
  draft (and clear any frame left in the terminal input; mode prefix
  restored). Sending again is the ordinary send action, not a recovery
  ritual.
- **Landed-then-aborted** (soak discovery, 2026-07-03, reproduced on
  both providers): the send DELIVERED as a user turn, then Esc aborted
  the agent's response, and the agent CLI itself restored the text for
  re-editing. The ledger correctly reports `landed`; Bram must do
  nothing here — above all, not claim "not delivered". Transport
  outcome and turn outcome are separate facts and the affordance must
  never conflate them.

### 4. Esc stays non-blocking

Every Esc interrupts, always. The #210 hold-gate was reverted for good
reason; the redo must not reintroduce input gating as a detection aid.

### 5. One affordance, ledger-driven — and passive

A single banner state machine driven by ledger events (injected / landed /
stranded), replacing the cause-specific `message-bounced` wiring and the
four-listener localStorage clearing dance. With recovery automatic
(invariant 3), the banner carries **no action buttons**: at most a passive
status note ("your message was restored to the composer" /
"a lost send was redelivered"), and silence for landed-then-aborted.

## What gets deleted when the rebuild completes

Phase-4 execution ledger (delete-phase tranche 3a, #214, 2026-07-06):

- **DONE** Bounce heuristics (first-output + recency pairing) — host
  cells/command/detection block and the parent-shell push deleted; the
  orphaned `message-bounced` emit went with them.
- **PARTIAL** Iframe landing state: `__bramWorklistSubmittedMatches`
  (zero callers) and the `submittedTurnsBaseline` plumbing (write-only)
  deleted. `submittedWorklistMessage` KEPT (read to seed state on
  mount). `awaitingResponse` + the localStorage awaiting keys KEPT —
  they are the live submit-button gate; their removal is tranche 3b, a
  ledger-driven rewire needing its own design.
- **DONE (tranche 3b, 2026-07-06)** The awaiting rewire: `awaitingTurn`
  is now derived host-side on `/__send-ledger` (newest ledger entry +
  turn-completion detectors) and mirrored into the `awaitingResponse`
  var; the four iframe end-detectors, `__bramMarkTurnEnded`, and the
  localStorage awaiting keys are deleted. `submittedKind` + its
  localStorage key remain (still the `worklistActionResult` validity
  signal); with the detectors gone its persisted value has no readers
  left at turn end — a candidate for the next dead-state sweep.
- **DONE (phase 3)** The `\r` Resend.
- **KEPT with evidence** Capture scrapers: demoted to diagnostics as
  planned, and their send-capture traces did real forensic work on
  2026-07-06 (both queued-send diagnoses). Revisit after a release in
  which they go unconsulted.

The redo must end net-simpler, as the transport redesign did.

## Sequencing

1. DONE: Implement the ledger + landing detection host-side, initially
   observe-only:
   trace lines + a Status-tab row, no UI behavior change.
2. DONE: Soak against real usage (both providers); tune until zero false
   strands / false landings.
3. DONE: Switch the affordance to ledger events: the passive banner, the
   composer-restore for user-caused strands, and auto-resend for
   mechanical strands. The Resend button is deleted, not rewired.
   **Trust gate:** auto-resend ships OFF and turns on only after the
   phase-2 soak shows zero false strands — a false strand plus
   auto-resend would create a duplicate send, the one harm the old
   button never had.
4. TODO: Delete the superseded layers.

Never span old and new detection across the affordance at the same time.

## Relation to existing worklist items

`detect-stranded-unsent-message` (including the 2026-07-03 folded-in
restore scope) is **subsumed by this plan** — its detector is this plan's
host-side ledger, and its banner is the ledger-driven passive affordance.
Follow-up work should focus on deleting superseded layers rather than
reviving the old cause-specific detector.

## Remaining questions

- Envelope-for-all-sends: should tiny inline sends also write envelopes so
  the ledger has one recovery shape? (Cost is trivial; decide when
  auto-resend lands — inline sends currently recover from ledger text.)
- ~~Codex parity~~ — RESOLVED 2026-07-03: the delivery-semantics matcher
  covers Codex record shapes (`event_msg` user_message, `response_item`
  user-role message) with unit coverage, and the live Codex sweep passed
  the full send matrix including the esc test.
- ~~Queued-send interplay~~ — RESOLVED 2026-07-03: delivery requires a
  `user` record or `queued_command` attachment; `queue-operation`
  bookkeeping never lands a send, so enqueued-then-killed sends strand
  correctly. The landed-then-aborted case (invariant 3) came out of the
  same soak.
- Typed-directly-in-terminal strands (text never sent through Bram) stay
  out of scope: nothing was injected, so there is nothing to ledger. The
  old detect-stranded draft reached the same conclusion.
