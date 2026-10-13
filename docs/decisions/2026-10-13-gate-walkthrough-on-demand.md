# The gate walkthrough runs on demand; field reports are the release check

Status: decided 2026-10-13 · revisit if a release ships a gate regression the walkthrough would have caught

**Question:** before a release, run the hand-driven Worklist gate walkthrough (`docs/worklist-gate-walkthrough.md`), or rely on field reports?

**Decision:** rely on field reports. Keep the walkthrough current and run it on demand, when a report or a change to the gate surfaces calls for it, and then only the phases the report points at.

**Evidence:**

- The recent gate bugs all came from the field:
  - #432, a stale joint record refusing an unrelated commit (Raymond's aauth-flow board);
  - #433, a Drop recorded as a commit (Andrew's CodeExam project);
  - #434, false state drift on CRLF files under Windows `core.autocrlf` (Andrew's);
  - #419, a new file left out of a commit (a live board).

  Each was found by someone using Bram on real work, and each was reproduced and checked on a demo instance afterwards.
- The walkthrough had fallen behind the gate. Checked on 2026-10-13, about half of it tested behaviour that had been removed (the "One commit / A commit each" choice; "Commit withheld on an entangled item", which ended with hunk-by-hunk commits in #327), and 7 of its 10 quoted wordings no longer existed in the code. A mandatory run would have scored the doc, not the gate.
- A full run costs about half an hour of hand-driving per release.

**Alternatives:**

- *A mandatory pre-release run.* Rejected: the cost per release is high, and the field has been catching what matters.
- *Automate it now.* Not yet: driving the pane from a script is still unsolved (the walkthrough's own graduation note). The host-observable half (claim retirement, refusals, trace checks) could be automated first.
- *Retire the walkthrough.* Rejected: when a report lands, a ready board of starters and expectations is the fastest way to reproduce and check it. That's why it was rewritten on 2026-10-13 instead of dropped.

**Revisit when:** a release ships a gate regression the walkthrough would have caught, or the pane can be driven by a script.
