# The pane-idle flush is retired whole; nothing replaces it

Status: decided 2026-11-24 · revisit if the pane-age climb returns on a post-0.8.11 build, or #515's cause turns out to be unrelated to the reload

**Question:** Whether to keep the pane-idle flush (`ui.paneIdleFlush`, items `pane-idle-flush-when-degraded` and `pane-idle-flush-default-on`). It reloaded the agent pane once focused heartbeat spikes crossed a rate while the window had been unfocused for three minutes and nothing was in progress. It treated judell/bram#482, the pane slowing with use.

**Decision:** Remove the workaround whole: the helpers.js tracker and decision, the `pane-flush` trace line, `POST /__pane/flush` (now a 404 like any unknown route), the `ui.paneIdleFlush` setting and its Settings switch. A `ui.paneIdleFlush` key left in a project's `.bram.json` is ignored. The three trace lines from `pane-wedge-instrumentation` (`pane-boot`, `pane-alive`, `pane-click-miss`) stay, to attribute any wedge after a manual reload; `pane-boot` loses its `source` field, since a reload now has one source, the user's.

**Evidence:**

- The cause of #482 was XMLUI's unbounded verbose trace buffer, fixed in `e7df16d3` / `e731d3ab` / `b16724fd` (v0.8.11).
- Rescored 2026-11-24 with `python3 -I scripts/pane-age-health.py --build-since 2026-11-19T00:32:55Z` over five days: spikes over 1 s per active hour by pane age were 11.6 / 7.1 / 15.0 / 0 (0–0.5 h, 0.5–1 h, 1–2 h, 2–4 h) and 0 at 8+ h, in 1,337 ticks, against 20 / 48 / 89 / 92 before the fix. Median projection 19–22 ms at every age. The flush's arming condition no longer occurs.
- judell/bram#515 (0.8.9): the one wedge on record began at the reboot the flush triggered, a reload taken in a hidden window with nobody watching. Cause not established, but with no slowdown left to treat, the workaround carries risk and no benefit.

**Alternatives:**

- Default it off but keep the code. Rejected: dead code, and a setting with nothing to do.
- Keep it on. Rejected: risk (#515) with no benefit.

**Revisit when:** `pane-age-health.py` shows the climb again on a post-0.8.11 build, or #515's cause turns out to be unrelated to the reload.
