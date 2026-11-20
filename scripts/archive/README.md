# Archived scripts

These are one-investigation probes and censuses, kept for the record. Each ran against the trace layouts and state folder of its time (`resources/` before 0.8.4, `.bram/` after) and may not run unchanged today; some compute their repo root from their old location (`scripts/..`) and would need a path fix. The live instruments are listed in the reference pointer list of `docs/developing-bram.md`.

Dates are when the script was added (`git log --diff-filter=A`); issues come from the script's own header.

| Name | What it measured or probed | Served | Live replacement |
|---|---|---|---|
| `update_xmlui_org_repos.py` | Refreshed the xmlui-org repo snapshot JSON from the GitHub API | 2026-05-03, snapshot updater | none |
| `howto-probe.py` | Ran one `xmlui_search_howto` query against a pinned docs corpus | 2026-08-12, docs before/after gap-close regression | `xmlui-howto-gap-miner.py` (different job: mines gaps) |
| `smoke-hook-write-trace.sh` | Smoke test that the guard writes its trace line | 2026-06-13, guard tracing | none |
| `smoke-test-worklist-resolver.sh` | Smoke test of `/__worklist/resolve` consume-on-read | 2026-05-22, #63 | the host's Rust tests |
| `trace-uncertainty-report.py` | Heuristic report of agent exploration/"gear shifting" per turn from traces | 2026-06-29, agent-uncertainty evidence | none |
| `membership-triage.py` | Swept traces for membership-engine divergence evidence and queued adjudications | 2026-09-08, attribution-model flip decision | `membership-starter-check.py` |
| `codex-permission-capture.py` | Observe-only capture of Codex PermissionRequest payload shapes | 2026-06-29, Codex approval-menu parity | none |
| `notification-boundary-census.py` | Counted background-task notifications against inflight-sentinel vetoes | 2026-09-12, turn-boundary fix (b72a29b) | none |
| `send-ledger-settle-probe.py` | Measured how long after injection `payloadInTail` flips true | 2026-09-14, #372 ask 2 | none |
| `pty-menu-timeline.py` | Episode/succession analysis of PTY menu detection traces | 2026-06-22, menu-detection misses | `pty-menu-scan-report.py` |
| `turn-state-probe.sh` | Sampled every "agent is busy" source and the consumers' derivations | 2026-08-22, `docs/turn-state-consumers.md` | none |
| `right-pane-reload-probe.sh` | Unattended repro and regression guard for the preview pane reloading on any file change | 2026-09-21, #385 | none |
| `would-deny-probe.sh` | Deliberate authorization violation to prove the `would-deny` observer fires | 2026-09-21, #368 and #387 | the guard's own trace ops |
| `would-deny-census.py` | Recomputed the #368 would-deny predicate from primary data | 2026-09-14, #368 | the guard's own trace ops |
| `send-switch-report.py` | Summarized what follows a pane send, from traces | 2026-09-24, Transcript auto-switch policy | the Log tab |
| `xmlui-nav-repro/` | Fixture checking a `DataSource` with a nested payload does not pin the router; run after each XMLUI revendor | 2026-09-09, #371, xmlui 0.14.27 revendor | none |
