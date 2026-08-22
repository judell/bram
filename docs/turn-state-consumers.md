# Who consumes turn state, and what they do when it is wrong

Three surfaces have reported the same underlying condition separately, each
found by a person noticing rather than by a check: the status row pinning at
"working" after an interrupt (#259), a Worklist row regressing to "No changes
yet" mid-apply (`a1e92ba`), and the Queue holding every Send dimmed behind
"agent working — hold" while the terminal sat idle (2026-08-21).

The survey that produced the structured interrupt edge (`9a7e4ef`) swept the
**source** axis — where a lifecycle signal comes from, and which mechanisms
inferred one from repaintable terminal text. It never enumerated the
**consumer** axis. This is that list.

## The inventory

| surface | state it reads | derived from | role |
|---|---|---|---|
| `components/FooterAgentStatus.xmlui` | `agentStatus.state` (`working` / `finished`), verb, elapsed | `/__agent-status`, `agent-status-changed` | **display** |
| `components/AgentMessageQueue.xmlui` | `agentStatus.state === 'working'`, plus a pending menu | same, plus `pty-menu-changed` | **gate** + display |
| `components/Worklist.xmlui` | the inflight claim's `ids` | `.inflight-claim.json` via `/__inflight`, `inflight-claim-changed` | **gate** + display |
| `components/Workspace.xmlui` (legacy) | `sendLedgerGate.awaitingTurn` | `/__send-ledger` | **gate** |
| host: `write_inflight_claim_sentinel` (`src-tauri/src/lib.rs`) | — | — | authority |

`Main.xmlui` appears in a grep for agent-status but is not a consumer: it
delegates to `FooterAgentStatus`, isolated into its own component so frequent
status pushes do not re-render the footer's composer.

## The finding: three notions of "busy", one root

The four consumers do not read one state. They read **three**, each with its own
derivation:

1. **`agentStatus.state`** — the PTY/JSONL-derived turn status. Read by the
   footer (display) and the Queue (gate).
2. **The inflight claim** — an authorization being carried out. Read by the
   Worklist gate, where `__bramInflightBlocker` is the single reason its buttons
   ever disable.
3. **`sendLedgerGate.awaitingTurn`** — host-computed from ledger state *plus the
   turn-completion detectors*. Read by the legacy Workspace gate.

They are not independent. All three descend from turn completion, so a root that
cannot end a turn holds open **every consumer whose state is live for that
turn**. **Fixing the source is therefore sufficient; no consumer holds
independently of it.** That was the open question this inventory existed to
answer, and the answer is no.

Which consumers are live depends on the turn. Two samples from
`scripts/turn-state-probe.sh` make the distinction concrete.

An ordinary message turn — no approved item in flight:

```
agent-status.state           working
inflight claim               (none)
send-ledger.awaitingTurn     true
--
Queue Send (gate)            HELD (agent working — hold)
Worklist gate                buttons enabled (no claim)
Workspace legacy (gate)      HELD (awaitingResponse)
```

A claimed turn — an approved item being applied:

```
agent-status.state           working
inflight claim               turn-state-probe (kind=approved)
send-ledger.awaitingTurn     true
--
Queue Send (gate)            HELD (agent working — hold)
Worklist gate                HELD (claim live: turn-state-probe)
Workspace legacy (gate)      HELD (awaitingResponse)
```

So the Worklist gate participates only in the second case: a message turn has no
claim to hold. An earlier draft of this document said a stuck root "holds all
three at once", which is true only of a claimed turn — corrected here after one
probe run showed an empty claim where a hold had been asserted.

What the three derivations buy is the ability to *disagree in transit*. Each has
its own event, its own refetch timing, and its own intermediate state, so one can
lag or diverge from another without either being wrong at its source. When two
surfaces contradict each other, that gap is the first place to look, and the
divergence is a transport question rather than a detector question.

## Loud versus silent failure

The distinction that matters is not display-versus-gate on its own. It is
whether a wrong state is *legible* as wrong.

- **Footer (display)** — fails loud. A counter climbing while the terminal sits
  idle contradicts what the user can see.
- **Worklist (gate)** — fails loud. While a claim is live the action bar is
  inert *and* the claimed row names its verb (`Approving…`, `Committing…`), so a
  disabled button always has a visible reason beside it.
- **Queue (gate)** — fails **silent**. A dimmed Send under the label "agent
  working — hold" is indistinguishable from correct behaviour. The user cannot
  tell a true hold from a stuck one, and the control that is blocked is the one
  that would let them report the problem.
- **Workspace (gate, legacy)** — fails silent, same shape: buttons disabled on
  `awaitingResponse` with no accompanying account.

The Queue case is the worst not because its state is more likely to be wrong,
but because a plausible explanation sits next to the blocked control. A gate
whose disabled state carries its reason degrades into a puzzle; a gate whose
disabled state carries a *confident wrong* reason degrades into a dead end.

## Release conditions

Each gate releases when its state clears, and every clear path ends at turn
completion:

- **Queue** — releases when `agentStatus.state` leaves `working` and no menu is
  pending. Turn completion is what moves it.
- **Worklist** — releases when the claim is consumed (`mutate`,
  `worklist-commit`) or cleared by the host's turn-finished detectors.
- **Workspace** — releases when the host recomputes `awaitingTurn`, which reads
  the turn-completion detectors directly.

So the verification for all three is one act: **interrupt a turn, then assert
every gate releases.** That is the cheapest way to produce the condition
deliberately, and it exercises the shared root rather than each consumer's
plumbing.

`scripts/turn-state-probe.sh` samples all four inputs at once and prints what
each consumer derives. It is read-only and takes no arguments; `--watch` samples
on an interval, since the observation that matters is a transition or its
absence.

### What the probe can establish, and what it cannot yet

- **Testable now — coupling.** Interrupt a turn, probe, and confirm
  `agent-status` stays `working` and `awaitingTurn` stays `true` while the
  terminal sits idle. That is the shared-root evidence, and it is available
  today precisely *because* the root is broken.
- **Not testable until `interrupt-end-staleness-conjunct` lands — release.**
  The positive half — every gate clearing when the root clears — cannot be
  observed while the root never clears after an interrupt. The probe exists
  before the fix so that fix arrives with a before-and-after rather than with a
  reading of its own diff.
- **The Worklist gate needs a claimed turn.** Interrupting mid-apply rather than
  mid-message. Worth doing once, deliberately, rather than folding into the
  routine check.

The probe deliberately prints rather than asserts. The release half cannot pass
until the staleness fix lands, and a check that is red for reasons unrelated to
the change under test teaches people to ignore it. Graduate to assertions once
the root is fixed.

`scripts/setup-harness.sh` is the wrong home for this — that harness owns
startup and drives throwaway projects, while this needs a live agent session
with a real turn to interrupt. Same technique, different subject
(`docs/developing-bram.md`, *Developing and testing the startup dance*).

## Known open

`#259` is not closed. The turn-completion veto is a conjunction and only its
first half is repaired: the detector now recognises an interrupt
(`decision=user-interrupted`), while a staleness comparison still holds the turn
open, because an interrupt record is itself a *user* record and so advances the
comparison point past every assistant entry that exists. Tracked as the worklist
item `interrupt-end-staleness-conjunct`.

Until that lands, every gate in this table can still hold after an interrupt.
The inventory does not change that; it records who is affected, and that fixing
one root releases all of them.
