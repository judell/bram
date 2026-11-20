# Fanout

*History: moved here 2026-11-19. A June 2026 conceptual explainer; superseded by the code itself (`subscribeTauriEvent` and the PushSource pattern in `app/__shell/helpers.js`) and the "Push over polling" rule in `docs/developing-bram.md`.*

Fanout = **one source event reaches N destinations.**

It's necessary the moment you have one emitter and more than one
consumer. The Tauri backend can't know that two XMLUI components both
care about `talk-session-changed`; *something* has to deliver the same
event to both. Either:

- the runtime fans out (each component calls `event.listen` directly,
  Tauri delivers to all of them), or
- you fan out in JS (one parent listener, an array of subscribers, loop
  over them — what `app/__shell/helpers.js` does for
  `talk-session-changed` and the generic `subscribeTauriEvent` path).

Either way, 1 → N. So it's not optional in principle; the only
question is **where the fan happens** and **who owns the subscriber
list.**

## When fanout is good

Decoupling. The emitter doesn't need to know who's listening, and
consumers can come and go without touching the emitter. That's why
Bram does it.

## When fanout is bad

Three failure modes, all of which Bram has hit in production traces:

1. **Amplification.** If each of the N subscribers does expensive
   work (a refetch, a re-render), one cheap event becomes N units of
   expensive work. Example: a single `inflight-claim-changed` firing
   7 listeners simultaneously; a single `talk-session-changed`
   firing 4. Each one drove a `refetch-called`, and the main thread
   spent its budget servicing the amplified work instead of input.

2. **Leak.** If `subscribe(...)` is called more times than its
   returned `unsub()`, N grows without bound. The `helpers.js` design
   specifically guards against this (`window[key]()` revokes the prior
   slot before pushing a new one), but only if every callsite uses a
   stable key. Multiple callsites sharing one key, or a callsite
   remounting before its prior unsub runs, can defeat the guard.

3. **Cascade.** A subscriber's reaction emits another event that
   fans out again. One keystroke → `talk-session-changed` → 4
   refetches → each refetch updates state → ChangeListener fires →
   emits another event → … The cost compounds across the cascade,
   not just within a single fanout.

## Diagnosing fanout cost in Bram

The trace subkinds in `resources/bram-traces/bram-trace.log` give you the two
relevant knobs.

**Is N too high?** Group `listener-fired` entries by `at:` timestamp
and `context:`. Multiple entries with the same `at:` and same context
mean the same Tauri event was delivered to multiple JS subscribers in
that tick. Expected count depends on which components are mounted;
unexpected counts (e.g. 4 entries for one `talk-session-changed` when
only Workspace.xmlui subscribes) point at a leak.

**Is work-per-subscriber heavy?** `heartbeat-batch` tells you main-thread
drift. Steady-state target is `avgDriftMs:10–11, spikes:0`. A batch
with `avgDriftMs:2000+, spikes:50/50` means every timer fire in that
window missed its slot — the thread was busy servicing fanned-out work.
Cross-reference the timestamp against the `listener-fired` / `refetch-called`
density in the same window to confirm.

## The fix shape

The fix is rarely "remove fanout" — you need 1 → N delivery somewhere.
The fix is to **keep N at exactly the number of actual subscribers**
and to **keep per-subscriber work cheap and idempotent.**

For N:

- One stable `window[key]` slot per subscriber identity.
- Every subscribe path must revoke its prior slot before pushing.
- Hot-reload / re-mount paths must complete the revoke synchronously
  — see the comment block above `__tauriEventSubscribers` in
  `helpers.js` (refs #81) for why a Promise-window mid-subscribe
  defeats the guard.

For per-subscriber work:

- Debounce or coalesce refetches; multiple events in the same tick
  should produce one refetch, not N.
- Keep handlers idempotent so a duplicate delivery is at worst a
  wasted cycle, not a correctness bug.
- Avoid emitting a new event inside a subscriber unless the cascade
  is intentional and bounded.
