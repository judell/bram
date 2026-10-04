# Turn-state spine

The spine is Bram's host-owned record of what the agent turn is
doing right now.

It exists because no single signal is authoritative enough to drive
the agent pane by itself. PTY output is immediate but noisy. Provider
JSONL is structured but delayed and sometimes arrives in awkward
bursts. XMLUI state is where the user sees the result, but it should
not have to infer turn state from scattered symptoms. The spine gives
those pieces a shared place to meet.

In code the current spine is the `/__turn-state` payload and its
host-side update path. As architecture, it is the decision to stop
letting every surface independently guess from PTY, JSONL, menus,
sentinels, and status rows.

## Why it appeared

The first pressure came from issue 182: menu rows, tool-use rows,
conversation text, and status labels were all telling slightly
different stories. Some views flickered because one source briefly
said "nothing here" while another still had useful state. Some views
stayed stuck because a stale PTY redraw looked like new work. Some
views lost a menu because the bytes had scrolled out of the small
tail even though the terminal was still presenting the prompt.

A big unified abstraction was tempting, but premature. We did not
yet know which signals were reliable, which were merely fast, and
which failures were edge cases versus core design mistakes. The
practical move was smaller: add one host-level turn-state spine,
keep adding instrumentation, and let the evidence show where the
next boundary should be.

That remains the posture. Bram sits between the desire for a clean
unified model and the reality of a chaotic communications path:
terminal bytes, provider session files, Tauri events, XMLUI
reactivity, user clicks, voice input, hot reload, and agent-specific
behavior all interleave. The spine is not a claim that the chaos is
gone. It is a place to collect and arbitrate it.

## Inputs

The spine receives facts from several channels.

PTY activity is the fastest signal. It sees spinner glyphs, terminal
title updates, permission menus, Esc/cancel output, and natural-end
status banners. It is good for immediacy and for things the provider
has not yet flushed to JSONL. It is also prone to stale redraws and
partial screen state.

Provider JSONL is the structured signal. Claude contributes
`stop_reason: "end_turn"` and non-final assistant records. Codex
contributes `task_complete`, `final_answer` messages, user messages,
tool calls, and tool outputs. JSONL is better for deciding whether a
turn is truly complete, but it can lag or be observed after the next
turn has already begun.

Permission-menu detection is a hybrid. Provider hooks (via `bram-guard`)
post the structured menu, and the xterm.js grid confirms which menu is
actually on screen; PTY bytes are no longer scanned for menus. JSONL can
provide signatures and tool-call context that
make the menu actionable. The spine carries the current `pendingMenu`
so consumers do not each have to rediscover or dismiss it.

Status emission is both an input and a consumer. A real finished
status updates the spine's phase and completion cursor. Later stale
PTY spinner pulses are then rejected until a newer user turn is
observed.

## State carried

The payload is intentionally small. The important fields are:

- `provider`: which agent currently owns the turn, for example
  `claude` or `codex`.
- `phase`: the user-visible phase, such as `working`,
  `waiting-for-permission`, `finished`, or `idle`.
- `turnStamp`: the newest known user-turn timestamp.
- `pendingMenu`: the currently actionable permission menu, if any.
- `lastPtyActivityAtMs` and `lastJsonlActivityAtMs`: clocks for the
  two main signal families.
- `lastJsonlReason`: the latest JSONL classifier result, such as
  `task_complete`, `active-tool-call`, or `next-task-started`.
- `lastCompletionAtMs` and `lastCompletionSource`: the completion
  cursor that prevents older PTY activity from relighting `working`.
- `source` and `reason`: why this version of the spine changed.

That is enough for UI consumers to render without reconstructing
the whole history.

## Consumers

The toolbar uses agent status and turn state to show whether the
agent is working, waiting for input, or finished.

The Worklist and Transcript panes use the spine to decide when an action cycle
is awaiting a response, when current-turn edit hints should refresh, and when a
permission menu should appear or disappear.

The status page and trace tools use the spine as a diagnostic lens.
They do not just ask "what is the latest PTY chunk?" or "what is the
latest JSONL line?" They ask which source last changed the host's
turn-state view, whether that change emitted to XMLUI, and whether a
later source contradicted it.

## Evidence-first workflow

The spine changed how we debug Bram.

The default instruction became: do not start by reading code and
inferring behavior. Read the evidence. If the evidence proves the
hypothesis, fix the path it identifies. If the evidence falsifies
the hypothesis, abandon it. If the evidence is insufficient, add
instrumentation before changing behavior.

That is why many of the release commits added trace points before
or alongside fixes. The point was not more logging for its own sake.
The point was to make the system falsifiable:

- Did the host emit `agent-status-changed state=finished`?
- Did XMLUI receive the event?
- Did a subscriber fire?
- Did the route replay the same payload later?
- Did `turn-state-changed` emit, or did only source metadata change?
- Did JSONL classify the tail as `task_complete`,
  `active-tool-call`, `next-task-started`, or something else?
- Did PTY menu detection dismiss because of user input, suppression,
  buffer eviction, or grace expiry?
- Did stale PTY spinner activity get blocked by the completion cursor?

When those questions are answerable from `resources/bram-traces`,
we can debug from facts instead of guesses.

## Trace vocabulary

Useful trace families:

- `[turn-state]`: records every host-side state mutation, including
  source-only changes that do not emit to XMLUI.
- `kind=turn-state-changed`: the replayable Tauri event emitted only
  for visible state changes: phase, provider, turn stamp, or menu.
- `[jsonl-turn-end]`: shows JSONL handoff, scan tail types, classifier
  decision, sentinel relationship, and skip reasons.
- `[finished-cue]`: marks the point where JSONL or PTY produced a real
  finished signal.
- `[agent-status]`: records status-specific decisions such as
  skipped finished emits, stale Codex spinner suppression, and PTY
  active-state clears.
- `[pty-menu]`, `[grid-menu]`, `[hook-menu]` and `[pty-menu-options]`:
  explain menu detection, joins, dismissal, and option parsing.
  (`[pty-menu-scan]` was deleted with the byte scanner in #214.)
- `[iframe] subkind=subscriber-fired`: proves whether the XMLUI side
  received and handled an event.

The useful pattern is to follow one incident across sources:

1. Find the user action or provider output in JSONL / PTY trace.
2. Find the corresponding `[turn-state]` transition.
3. Check whether it emitted a replayable event.
4. Check whether XMLUI received it.
5. Check whether a later source overrode it.

## What the spine fixed

The spine itself did not solve every issue. It made later fixes
possible and diagnosable.

Examples from the release:

- Tool-use rows could stop flashing because consumers no longer had
  to treat every source update as a full semantic reset.
- Permission menus could be held across partial PTY tail eviction,
  with traceable reasons for shown, holding, dismissed, and expired.
- Source-only turn-state updates stopped fanning out into expensive
  XMLUI refetch cascades.
- Codex final responses could be recognized from JSONL and translated
  into a finished status.
- Stale Codex PTY spinner redraws could be rejected after a verified
  completion cursor.
- The toolbar could display the provider and final state without
  confusing a cached working verb for a finished label.

The common thread is not that all bugs had the same fix. The common
thread is that each fix could now attach to a shared host-side account
of the turn.

## Boundaries

The spine is not a full event-sourcing system. It does not preserve
every raw PTY byte or JSONL record. The trace does that.

The spine is not the only state in Bram. Some caches remain closer to
their domain: PTY tail bytes, menu suppression windows, sticky status
verbs, latest JSONL fanout buffers, worklist authorization records,
and conversation-pane display state.

The spine is not a promise that one signal wins forever. PTY may be
right first; JSONL may be right later; user input may invalidate both.
The value is that each handoff is explicit and traceable.

## Guidance for future work

When a sync bug appears, start with evidence:

- Capture or inspect the trace around the incident.
- Identify which source changed first and which source contradicted it.
- Check whether the spine represented the correct state.
- If the spine was right, fix the consumer.
- If the spine was wrong, fix the source update or arbitration rule.
- If the trace cannot answer the question, add instrumentation first.

Prefer small, evidence-backed extensions over a sweeping abstraction
that assumes the communications path is cleaner than it is. The
spine is the current compromise: unified enough to coordinate the UI,
modest enough to keep learning from production traces.
