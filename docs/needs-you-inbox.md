# The "Needs You" inbox — design plan

Status: **design, pre-implementation.** This document is the deliverable of
the `needs-you-inbox-design` worklist item (#338). It translates Julia's
design into a Bram-grounded implementation plan and decomposes a v1. No
product code ships from this item; implementation items are proposed from
this plan afterward, each with its own gate — the design-first treatment the
worklist redesign got in #278.

## Who this is for, and the one question it answers

A growing class of Bram users are **non-engineers overseeing a codebase**
built by engineers and agents — a founder, a PM, a domain expert. They own
decisions and reviews but GitHub is opaque to them: its notification stream is
every comment, CI run, and bot event, undifferentiated, and the two things
actually blocked on them are buried in it. The inbox answers one question, and
only that one:

> **What is waiting on *me*, right now?**

Julia's design (mildly iterated, kept private):
<https://gist.github.com/JuliaTogetherGroups/7fbb6056b561f0048ddf4cddb186f6f4>

## The core: one ownership rule, one pure function

Everything — lane, sort order, ping trigger, visibility — derives from a
single rule:

> An item is in the user's lane when **the last move wasn't theirs** and it's
> **theirs to make next**.

So the engine is one pure function, not a pile of per-source heuristics:

```
owner_state(item_events, user_identity) -> { court: user | others,
                                             blocking: bool,
                                             verified: bool }
```

- `court` — whose move is next, re-derived from the *latest* event, never from
  a title or a cached status.
- `blocking` — does anything move before the user acts, or is this a call they
  can make when ready.
- `verified` — did reality confirm the above, or is this the honest
  **unverified** fallback (see Requirement 0).

The four lanes are a projection of `(court, blocking)`:

- 🔴 **Needs you now** — `court=user, blocking=true`. Nothing moves until you act.
- 🟡 **Your decision, when you're ready** — `court=user, blocking=false`.
- 🟢 **In someone else's hands** — `court=others`. You did your part; you'll be
  pinged when it returns.
- ⚪ **Loose ends** — one-click Bram-local actions (unpushed commits, queued
  closes).

Bram already computes half of "whose court" in scattered places — the
close-queue's rebase-stable and merged-PR state, push state (`@{u}..HEAD`),
worklist begun/applied/committable state. The plan's first job is to name
which existing signals feed `owner_state` and what is genuinely new (chiefly
PR review-request and check state).

## Requirement 0 — verify before you display (non-negotiable)

The prototype earned this the hard way: its first dry-run against a live
project had **3 of 8 items wrong** — a risk-sounding title that was a
requested feature, a "please run a sample" that had already run, a "ready to
push" already pushed, a "100% done" that was 17%. Every failure came from
trusting a *title* or a *last-known status* instead of reality.

So the inbox is a **reconciliation engine first, a UI second.** No item is
rendered from a title or cached status. Each is checked against:

- **latest content** (not the title the issue was filed under),
- **whether the work already happened** (the "already ran / already pushed"
  class),
- **quantitative claims against the real artifact** (the "100% / actually 17%"
  class),
- **whose court, re-derived from the latest event.**

Anything that cannot be confirmed is labelled **unverified** and rendered as
such — never asserted.

The load-bearing point for Bram: **verification is not greenfield here.** The
"check reality" inputs already exist and this plan maps Requirement 0 onto
them rather than inventing a verifier:

- the close-queue's `op=closed-via-pr` / merged-PR reconciliation (a court
  change Bram already detects),
- `/__issues?fresh=1` live re-fetch for an issue's current state,
- the FTS `/__search` index for "did this already happen / was this decided",
- worklist / commit / push state for the Bram-local lane.

The plan states, **per lane and per item type, what "verified" means and what
the unverified fallback renders.**

## Data sources and their freshness

| lane input | source | freshness |
|---|---|---|
| Issues | the ~45s issues indexer + `/__issues?fresh=1` on open | near-real-time, live on demand |
| Commits | git branch state — **including commits that arrived from elsewhere** (another machine or session, a push to the branch), reconciled against `@{u}..HEAD` and the default branch | live; reconciled, not assumed local |
| PRs *(new)* | forge adapter — reviews-requested, checks, merge state | **new work**; GitHub first per `docs/forge-adapter.md` |
| Bram-local | worklist items awaiting the user, queued closes | already local, already live |

Commits are their **own** source, not folded into Bram-local, because a commit
is not necessarily this session's local work: it can show up from elsewhere,
so it must be *reconciled against branch state* rather than assumed. PRs are
the one genuinely new data source. They enter through the forge
adapter (`docs/forge-adapter.md`), GitHub first, so the surface stays
forge-agnostic the way issues already are.

## The surface

A new pane tab (XMLUI), four lanes, and per item:

- a **plain-language line** — outcome plus the next action, not the raw event
  ("Julia's review is requested on the auth change" — not "review_requested
  event on PR #14"),
- a **two-tier link-out** — the item, and the *exact artifact* to look at (the
  file to approve, the specific comment to read),
- **no PR-rendering layer in v1.** "Open on GitHub" handles acting; Bram's
  value is the triage, not a PR viewer.
- a **progress annotation while verification is in flight.** Requirement 0's
  freshness / reconciliation check runs at startup and takes real time — it is
  per-item live checks against issues, branch state, and the search index. The
  surface must show that it is *verifying* (a per-lane or per-item progress
  state) rather than rendering empty or unverified-looking content in the gap.
  An inbox that arrives blank for several seconds reads as "nothing waiting" —
  which is the one false signal this surface exists to prevent.

## The ping

Host-driven, per the push-over-polling discipline. A court change **into** the
user's lane synthesizes a Tauri event; the pane batches; **quiet by default** —
a count badge, not an interruption — unless an item is flagged urgent. This
extends the existing event precedents (`issues-changed`, `git-status-changed`)
rather than adding a client poll. Putting the ownership engine host-side (see
open questions) is what lets the ping fire even when the pane is closed.

## A concrete v1 cut

Ship first:

- **Lanes:** all four, because the ownership rule already produces them; the
  cost is in *sources*, not lanes.
- **Sources:** issues, **commits**, and Bram-local. Commits are their own
  source, not folded into Bram-local, precisely because a commit can show up
  from elsewhere and must be reconciled against branch state. All three reuse
  machinery that already exists and already verifies. **PRs deferred** to v2 —
  they are the new data source *and* the one whose verification (checks, merge
  state, review threads) is least covered today.
- **Verification:** only what Bram can do honestly now. An item whose court
  Bram cannot confirm renders **unverified**, never asserted — which is itself
  a shippable v1 behavior, not a gap.

Explicitly deferred: PR rendering (always); PR-sourced lanes (v2); any item
whose verification Bram cannot yet do honestly.

## The reconciliation cost budget

Per #323's lesson: verification is **per-item live checks**, so cost is real
and must be placed deliberately. The plan says, per input, what runs:

- **on open** (the user is looking; a fresh re-fetch is warranted),
- **on a timer / on an event** (ride the ~45s issues indexer and the existing
  Tauri events; do not add a new poll — the #329 close-sweep is the precedent
  for a consumer on work already being done),
- **lazily** (the exact-artifact link resolves only when the item is expanded).

The steady state must be spawn-free and silent when nothing is waiting.

## Open questions to settle in the plan, not now

- **Where the ownership engine lives** — host-side (Rust, so the ping can fire
  with the pane closed) or client-side. Leaning host-side for the ping's sake.
- **How "the user" is identified** — forge login vs. Bram account.
- **Whether v1 is GitHub-only** — almost certainly yes, via the forge adapter,
  with the surface staying forge-agnostic.

## Why not just build from the gist

- **Build straight from the gist** — rejected: its Requirement 0 is
  *architectural*, not a detail, and building the UI before the reconciliation
  engine is exactly the title-trusting failure it warns against.
- **One monolithic implementation item** — rejected: the feature decomposes
  cleanly (engine → sources → surface → ping) and each wants its own gate.
