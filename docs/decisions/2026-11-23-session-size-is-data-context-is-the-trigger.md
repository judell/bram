# Session size on disk is information; live context is the cost, and the trigger for a fresh session

Status: decided 2026-11-23 · revisit after a month of `[session-context]` samples carrying `floor=`, or when a quality-at-size signal exists

**Question:** Bram measures two things about a session and had stopped showing either. The file's size in megabytes had bands (green to 5 MB, amber to 15 MB, red past that; 29f4036d, 2026-06-03) and a note that was demoted from a warning on 2026-10-04 (8018d610), then lost its last reader on 2026-11-12 (a9188e13, 36a8d172). The live context in tokens, what the model re-reads on every call, was sampled at each turn end since #486 but shown nowhere. On 2026-11-22 the Workflow-or-Freestyle session (`fb876140`) grew to 94 MB and 968k of context with no notice from the pane and no fresh-session offer from the agent, which proposed items and filed an issue without asking. Which quantity should Bram show, which should trigger an offer, where, and at what number?

**Decision:**

- **Megabytes are information, never a warning.** They measure findability, resume and search (and the indexer's reindex treadmill), not cost and not pane health. The bands stay in `/__session-size` as data and the KB figure stays on the footer. The 2026-10-04 "note, not a warning" call stands for bytes.
- **Live context tokens are the cost and the trigger.** `/__session-size` carries a `context` block (tokens now, the floor, peak, compactions, growth since the last boundary) and the host emits `session-context-changed` at each turn end. The footer shows the figure beside the session line with the floor next to it, so the two numbers that matter are read together: what this call cost against what a fresh session would.
- **Act at a boundary past a threshold, not at a level.** The threshold is a project setting, `sessions.suggestAtContextTokens`, default **400,000**, `0` off. Past it, the footer shows **Start a new session** in the warning color (an action named, so the color is earned) and the agent's fresh-session rule asks once at the next natural pause: after a Commit or Drop, when an item's work is done, before proposing an item or filing an issue. "Same line of work" is no exemption for this trigger, since it is about cost.
- **Both sides offer.** The host button does not depend on agent judgment, which is what failed; the agent rule catches the pause the host can't see. The topic-shift switch defaults on with the other two.

**Evidence:**

- `fb876140` main thread (per-call rows from `/__token-accounting?session=…&calls=1`): 628 calls, 313.8M context tokens, 311.3M of them cache reads, 968k peak, 137k after its one compaction, 94 MB on disk. After issue #514 was filed at 363k: 143 calls at an average 496k, of which a fresh session at that boundary would have saved 41M cache reads (58%). From the item proposal at 230k: 327 calls at an average 616k, of which a fresh session would have saved 51M of 200M; the other 700k of context was the one item's own growth at about 2.2k per call. So a missed boundary cost about 40 to 50M cache-read tokens here, and the larger spend sat inside one long item.
- This Mac's ledger (176 items credited since 2026-11-07, `/__token-accounting?calls=1&since=2026-11-07`): context at an item's first call p25 186k, p50 330k, p75 528k, p90 717k, max 964k; 102 items started above 300k, 70 above 400k, 50 above 500k. The 70 that started above 400k ran 1,438 calls at an average 597k: 727M cache-read tokens over a 74k floor in sixteen days. Growth within an item averages 2.1k per call. Across all Claude main-thread calls here, 47% of cache reads happen above 500k.
- Indigo's ledger (#477, 2026-11-21 comment, 47 items): p50 335k, 18 of 47 above 400k; feedback per call flat across starting context (no quality signal); latency flat from 100k to 600k.
- `scripts/pane-age-health.py` (2026-11-04, 21 sessions, 373 pane loads): spikes per active hour run 19 to 82 across session-size bins with no trend; the pane slows with use since load, and a reload fixes it. The 21 MB failure the bands were set on was `/__session-turns` parsing the whole file, a route deleted in #214.
- Price (platform.claude.com/docs/en/about-claude/pricing, 2026-11-23): Fable 5.1 cache reads $0.25 per million, so `fb876140`'s main thread was about $78 in cache reads and a missed boundary about $10 at list; at 600k a call's cache reads are still about three quarters of its cost. On a subscription this is quota, not dollars.

**Alternatives:**

- Keep megabytes as the trigger and put the old bands back on the footer. Rejected: 90 MB worked after compaction, the mechanism that failed at 21 MB is gone, and the file never shrinks when the context does.
- A constant token level with a warning (300k note, 500k warning, as #477's body proposed). Rejected: on both machines the median item starts above 300k, so a 300k note is on for most item work and says nothing; and a level fires mid-work, where a fresh session is not an option.
- An agent-only rule. Rejected: it missed four boundaries in one night in `fb876140`, each defensible as the same line of work. The host button is the part that would have fired.
- Doing nothing because cache reads are cheap per token. Rejected: at the sizes sessions reach here, cache reads are most of the bill, and the item-start distribution says this is the common case, not an outlier.

**Revisit when:** a month of `[session-context]` samples with `floor=` is in the ledger and the item-start distribution can be re-read against the 400k default; a quality-at-size signal exists (feedback per call, or items dropped after Start, by starting context); or compaction behaviour changes so that the within-item growth is bounded another way.

Left open: whether `/compact` and "delegate the next read-heavy task" join the footer's offer (#477 step 2's other two options), and the Mac-side quality join the Indigo comment described.
