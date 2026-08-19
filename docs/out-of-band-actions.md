# Out-of-band actions

Bram attaches host-side consequences to actions taken in its UI: flushing
queued issue-closes after a push, refreshing the search index when an issue is
created, emitting `git-status-changed` on a branch switch.

Every one of those actions can also happen **outside** the UI. The user runs
`git push` in a terminal, `bump.sh` pushes at release time, an agent runs
`gh issue comment` as a tool call. Where an out-of-band twin has no observer,
the consequence is silently skipped.

Verified 2026-08-18 against `e537157`. Line numbers move; the function names
are the durable reference.

## The matrix

| action | host-side consequence | observer for the out-of-band twin |
|---|---|---|
| commit | stage, commit, prune item, consume auth, queue closes | **prevention** — the PreToolUse guard denies uncovered writes (`app/provider-hooks/claude-worklist-guard.py`) |
| **push** | `rebuild_commits_list_cache`, `git-status-changed`, `flush_pending_worklist_push_mirrors` (`lib.rs:16115`), `flush_pending_issue_closes` (`lib.rs:16118`) — all in `finish_git_push` (`lib.rs:16103`) | **none** — judell/bram#253 |
| branch switch | `git-status-changed` | **watch** — `start_git_head_watch` on `.git/HEAD` (`lib.rs:16155`) |
| issue create | index refresh | **sighting** — `scan_jsonl_tail_for_new_issues` (`lib.rs:11530`) → `refresh_issue_now` (`lib.rs:11400`) |
| issue comment | index refresh | **sighting** — same path, comment-id keyed (`c8dd94d`) |
| **issue close via CLI** | index refresh | **none** — falls to the 60s issues poll (`issues_poll`, `lib.rs:19763`) |
| worklist mutate | advance/prune, sentinel | **prevention** — guard + version check; watcher reverts unauthorised prunes |
| Setup | seeded conventions, hooks, skills | **periodic** — drift detection in the needs-setup banner |
| commits arriving from outside | commits index, `commits:list` | **periodic** — `run_index_buckets` each cycle (`lib.rs:19607`) |

Seven of nine covered. That number is better than it looks and worse than it
sounds — see *What this is not* below.

## The four mechanisms

Each was built for a specific bug. Naming them as a set is what this document
is for.

- **Watch** — observe a filesystem trace the action leaves. Cheap, immediate,
  no polling. A branch switch writes `.git/HEAD`; a push writes
  `refs/remotes/<remote>/` and `packed-refs`.
- **Periodic** — recompute on a cycle. Right when the state is remote and
  leaves no local trace, e.g. issue edits made by other people.
- **Sighting** — scan the agent transcript tail for evidence the action
  happened. Right when the actor is an agent, because the agent cannot forget
  to report something it is not asked to report.
- **Prevention** — make the twin not happen. The commit row has no observer
  because the guard refuses uncovered writes. Easy to overlook when auditing,
  precisely because a prevented action leaves nothing to observe.

## Choosing one

- Leaves a local filesystem trace → **watch**.
- Remote-only, no local trace → **periodic**, host-side.
- Performed by an agent → **sighting**.
- Must not happen at all → **prevention**.

By that test the push gap is unambiguous: a push writes refs, so it wants a
watcher.

## The predictor: flushes strand, recomputations heal

The most useful thing this inventory produced is not the blanks themselves but
what they have in common.

**Cache rebuilds and index refreshes self-heal.** Something periodic recomputes
them, so a missed trigger costs latency, not correctness. A terminal push skips
`rebuild_commits_list_cache`, and the indexer rebuilds it on the next pass.

**Queued one-shot work strands.** Nothing re-attempts it. A terminal push skips
`flush_pending_issue_closes` and the close never happens — the issue stays open
with the user's consent already recorded.

So when adding a host-side consequence, ask whether it is idempotent
recomputation or a one-shot flush. The second kind needs an observer or it will
strand, and it will strand silently, because the successful part of the action
(the push) reports success.

That predictor found a bug: `flush_pending_worklist_push_mirrors` sits on the
same call site as the reported close bug and has the identical defect. Nobody
had reported it — the mirror setting is off by default — and it surfaced only
because someone enumerated the row rather than fixing the ticket.

## What this is not

**Not a completeness claim.** The coverage is *retrospective*. Every mechanism
here was built for a specific bug, and no one chose them as a set. Two of nine
rows are blank, and one of those was found by accident.

**Not self-maintaining.** A stale inventory is worse than none, because it
converts "unknown" into "checked". This document states its verification date
and commit for that reason. If a row's observer claim cannot be traced to code
today, treat the row as unverified rather than as true.

The value is narrow and real: the next blank should be a diff against a table
rather than an archaeology exercise. That is the same argument as the per-agent
guard coverage matrix in `docs/security.md`, which exists because a surface sat
uncovered for three and a half weeks while the document called it handled
(judell/bram#261).

## See also

- `docs/security.md` — per-agent guard coverage matrix; the prevention row here
  is that document's subject
- judell/bram#253 — the push gap, and the thread where this class was named
