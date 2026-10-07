# Decisions

One short file per decision: a choice between alternatives, with the evidence behind it, that someone would otherwise re-argue or re-derive. Decisions live here rather than in issues, so the tracker stays for work and reports, and a decision doesn't sit open waiting for someone to close it.

A decision note is not a design doc. Design docs (`docs/*.md`) describe how something works. A decision note records what was chosen, what was rejected, and the numbers that settled it.

## Writing one

Name it `YYYY-MM-DD-<slug>.md`, dated the day the decision was made, and add it to the index below, newest first. Keep it short. Five minutes is the target.

```markdown
# <the decision, as a statement>

Status: decided YYYY-MM-DD · revisit <when>

**Question:** what was being decided.

**Decision:** what we chose.

**Evidence:** the numbers and where they came from (a report, a trace query, a commit).

**Alternatives:** each option not taken, and why.

**Revisit when:** the condition that should reopen it, if any.
```

**Superseding.** Don't rewrite an old note. Write a new one, and change the old one's status line to `Status: superseded by <file> (YYYY-MM-DD)`. The history of why is the point.

**Finding them.** Grep this directory, or `/__search` (the commit that adds a note is indexed with its diff).

## Index

- [2026-10-07 — No wider automatic switching to the Transcript](2026-10-07-no-wider-transcript-autoswitch.md)
