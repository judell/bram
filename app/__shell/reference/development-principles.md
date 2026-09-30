# Bram reference: development principles

Seeded by Bram Setup as `.claude/bram-reference/development-principles.md`.
General engineering rules for any project developed with Bram, whatever
its language or framework. Read it when you design anything that waits,
retries, polls, or decides that something is "ready".

Each rule is short, gives its reason, and cites where it was learned.
Framework-specific rules live elsewhere: XMLUI rules come from the
xmlui-mcp server's `xmlui_rules`, and Bram's own source rules live in
Bram's developer docs, which aren't seeded.

## A fixed interval is guaranteed to be wrong at some point

**Gate on evidence the system emits, not on elapsed time.** Evidence
means a byte sequence, a file appearing, a state change, an event, a
process exiting: something that happens *because* the thing you're
waiting for happened.

- **A timer may bound how long to wait. It never decides "ready".** A
  timeout that gives up and reports failure is fine. A delay after
  which you assume success is a guess, and it will be wrong on a slower
  machine, a busier moment, or the next release of whatever you're
  waiting on.
- **When no evidence exists yet, say so.** Instrument to find some
  (see `.claude/bram-reference/diagnostics.md`, §Log-first development),
  and until you have it, mark the timer in the code and the commit as a
  known exception with the evidence it's waiting for.
- **Check again at the moment of acting.** Evidence that was true when
  you decided can be stale by the time you act. Re-read the state just
  before the write, not only when the decision is made.

**Why:** the same code in Bram broke this twice in a day. A New session
handoff was moved "from time to evidence" (Bram `fcab2b3`; judell/bram#314,
"still time-gated, not evidence-gated"). A 400 ms settle window then
added to it (`4fab6f0`) worked when Claude Code's startup mode bounce
came after 290 ms, and failed at 2026-09-30 22:10Z when it came after
437 ms: the first message was pasted into a terminal that had just
switched its input off. The bounce itself was the evidence to wait for.
