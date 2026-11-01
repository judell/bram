# Turn transport redesign — plan of record

## Why this document exists

On 2026-07-02 an attempt to move user-authored turn content off the fragile
PTY/terminal injection path (Andrew's Windows ConPTY truncation report) devolved
into a tangle. Four commits — `3021d7c`, `dea7796`, `110b4c6`, `0f4ecad` — were
reset off `main` back to the `v0.2.17` release and preserved on branch
`archive/transport-2026-07-02` for reference and cherry-pick. The full
post-mortem is `~/Desktop/disaster.md` (not committed).

The transport idea was right; the **execution** was wrong: it was added
incrementally through every display surface without first naming the canonical
boundaries. This document names them, so the rebuild is a set of consumers of
one projection instead of a set of independently-patched panes.

## The root mistake

Three concerns were never separated, so every bug looked like a local rendering
problem and got patched at a display edge:

1. **Transport** — how a user-authored turn gets from Bram to the agent.
2. **Persistence** — where Bram stores the authored turn (text, images, mode).
3. **Projection** — how Transcript, Sessions, dock, search, and worklist
   previews reconstruct turns from session logs plus sidecar files.

Symptoms that all trace back to this: message body shows as an `@path` frame in
one pane but not another; image appears in Transcript but not the dock; feedback
and message-box use different codepaths; transcript dedupe hacked in the client
and again in the host; raw-JSONL parsing and host `/__session-turns` drift;
legacy image markers linger because some paths still depend on them.

## Invariants for the rebuild

### 1. One canonical authored turn (Persistence)

Every Bram-authored user turn is a filesystem envelope (under
`resources/outbound-turns/`) with `text`, `images[]`, `mode`/control metadata,
and a stable id. **Message box and feedback both use this one shape.** Images are
not a different transport model.

### 2. Minimal agent injection (Transport)

The agent-facing send injects only a compact frame pointing at the envelope. The
PTY is never the authoritative carrier for substantial user text or image
metadata.

### 3. One host projection is the single source of truth (Projection)

One host route returns normalized turns from JSONL + outbound envelopes. It owns:

- outbound turn envelopes,
- legacy message-draft refs (while old history exists),
- legacy feedback refs (while old history exists),
- image paths,
- duplicate suppression from Codex storage fanout.

**UI surfaces — Transcript, Sessions, dock, search, worklist preview — are pure
consumers of this projection.** If client helpers remain, they are presentation
adapters only, never independent parsers with their own resolution rules.

### 4. Legacy compatibility at the boundary only

Legacy formats are resolved inside the host projection. They never leak into
XMLUI components as special cases. New outbound turns do **not** emit legacy
`[Image: source: …]` markers to satisfy old renderers.

### 5. Worklist lifecycle stays frozen

Only the transport underneath changes. Approve / drop / iterate semantics and
their documentation do not co-evolve with the transport this time — co-evolving
both at once is part of why every bug looked local.

## Sequencing (non-negotiable order)

The discipline that was missing: **migrate every read surface onto the
projection while writes still use the old inline send; stabilize; only then
switch writes to envelopes.** Never span old and new contracts across reads and
writes at the same time.

1. Define the outbound turn envelope schema.
2. Implement one host route that returns normalized turns from JSONL + outbound
   envelopes.
3. Move Transcript to that route.
4. Move Sessions / search to that same route.
5. Move the Worklist preview to the same Transcript component/projection — only
   after the route is stable.
6. Only then switch the message-box and feedback send paths to emit envelope
   frames.
7. Remove client raw-JSONL parsing from primary UI paths.

**Status, read from the code on 2026-10-31:**

- Steps 1–3: shipped (`edcfe3c3`).
- Step 4: half. `SessionDetail.xmlui` reads `/__turns`, but the search indexer
  has its own Rust passes and doesn't call the route.
- Step 5: not done as written. The Worklist's preview is gone, and the
  projected-turns machinery is "dormant here" (`Worklist.xmlui`).
- Steps 6 and 7: shipped (`69f027bd`, `b773098a`).

