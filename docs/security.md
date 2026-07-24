# Bram security assessment and action plan

Status: original assessment 2026-07-10; **verified re-review 2026-07-24**.
Source: a six-agent read-only audit, one agent per trust boundary, each citing
`file:line` evidence against the real `src-tauri/src/lib.rs`, the provider
guards, `app/__shell/helpers.js`, and the XMLUI surfaces. The 2026-07-24
re-review cross-referenced every finding against the commit record; findings
marked **DONE** cite the landing commit or the current code location and are
verifiable by `git show`. Tracks umbrella issue #108 and its children #109,
#110, #111/#114, #112, #113, #118, #119, #120, #121.

This document is the plan the child issues execute against. It is not itself a
code change; it closes no issue on commit.

## Threat model

Bram runs AI agents with access to the web, the local filesystem, git, GitHub,
and host-side IPC. The security goal (from #108) is a deliberate boundary:

> Untrusted content — web pages, agent-generated text, and the optional
> target-app iframe — may **inform** the agent, but only the host plus explicit
> user approval may **authorize** mutations or external effects.

The actors that matter:

- **The agent process** (Claude/Codex in the PTY). Trusted to read, untrusted
  to mutate without worklist coverage. Its file writes are gated by the
  provider PreToolUse guard; its git/GitHub side effects are gated by the
  worklist authorization record.
- **The target-app iframe** (optional, off by default). May host arbitrary web
  content. As of C1 it is served at a distinct `bramapp://localhost` origin,
  cut off from `window.__TAURI__` and the dynamic host routes — display-only.
- **Any other local process / browser tab.** The host serves a loopback HTTP
  API on `127.0.0.1`; co-resident software can reach it if it learns the port
  (written to `resources/.bram-port`). A cross-origin browser tab can no longer
  *read* the responses (H6 — CORS restricted to the shell origin); a same-user
  local process remains an accepted residual.

## Root causes

The original assessment traced most Critical/High findings to four shared
roots. All now fixed.

1. ~~**The target-app iframe is same-origin with the shell.**~~ **FIXED** (C1,
   `c8c320c` + CSP `99591ce`). The pane is served at a distinct
   `bramapp://localhost` origin; cross-origin `getTauriInvoke()` returns null,
   the PTY-driving helpers no-op, and `handle_target_scheme` refuses the dynamic
   host routes. Untrusted page content no longer inherits the IPC/route surface.
2. ~~**The loopback server is unauthenticated with wildcard CORS.**~~ **FIXED
   for the browser vector** (H6). The wildcard ACAO is replaced by a grant only
   to the shell origin, so no cross-origin browser page can read loopback
   responses. Residual: a same-user local process is not CORS-bound (accepted;
   a per-session token was designed and dropped as low-value — see H6).
3. ~~**Authorization is derived from PTY input bytes.**~~ **FIXED** (H2,
   `6d49edf`). Approve/drop authorization is host-owned; a forged `approved:`
   in relayed PTY input no longer writes an auth record.
4. ~~**The Claude PreToolUse guard does not gate the `Bash` write surface.**~~
   **FIXED** (H3). The Claude guard now carries `_BASH_WRITE_PATTERNS`
   (`claude-worklist-guard.py:113`) covering `>`, `tee`, `sed -i`, etc., at
   parity with the Codex guard.

## Ranked action plan

Severity is impact-weighted against the threat model. Effort: **S** = < half a
day, **M** = half to two days, **L** = more than two days.

### Critical — all resolved

| # | Finding | Status |
|---|---------|--------|
| C1 | Same-origin iframe inherits Tauri IPC / PTY injection. | **DONE** — distinct `bramapp://` origin + CSP, display-only pane (`c8c320c`, `99591ce`). |
| C2 | `/__file` read any absolute path. | **DONE** — contained to an allowlist of roots (`9ce3911`; `lib.rs:33262`). |
| C3 | `/__context/file` read any absolute path. | **DONE** — contained to the enumerated Context set (`f2a9862`). |

### High

| # | Finding | Status |
|---|---------|--------|
| H1 | Bracketed-paste `\x1b[201~` smuggling / auto-submit. | **DONE** — paste-end neutralized in outbound payloads (`6a13cde`). |
| H2 | Worklist authorization forged from PTY input. | **DONE** — host-owned approve/drop auth (`6d49edf`). Root cause #3. |
| H3 | Claude guard exits 0 for all `Bash`; write ops bypass the worklist. | **DONE** — `_BASH_WRITE_PATTERNS` in the Claude guard (`claude-worklist-guard.py:113`). Root cause #4. |
| H4 | Auth did not fail closed on interrupt. | **DONE** — `invalidate_worklist_authorization` at every interrupt site; `validate_*`/`ensure_*` reject an interrupted or past-TTL record (`d044b34`, `e75fea2`). |
| H5 | Issue-close / push side effects ungated. | **DONE** — agent `/__issue/close` route removed; closing is a host consequence of the user's explicit Push (`8c64ef7`). |
| H6 | Loopback wildcard CORS let any cross-origin browser page that learned the port read responses. | **DONE — browser vector** (`issue-113-h6-loopback-cors-and-token`). The blanket `Access-Control-Allow-Origin: *` is gone; ACAO is echoed only for the shell (agent-pane) origin and omitted for every other Origin and for no-Origin callers (`cors_allowed_origin`). Confirmed by soak that real pane traffic sends no Origin, so the change is transparent to it; a cross-origin browser page — and the target pane's `bramapp://` origin — can no longer read loopback responses. **Residual (accepted):** a same-user *local process* is not browser-CORS-bound; a per-session token was designed and deliberately dropped as low-value, since such a process could equally read the token (env via `ps eww`, or a port-adjacent file). |

### Medium

| # | Finding | Status |
|---|---------|--------|
| M1 | Guard fails **open** when Python is missing; Setup surfaces `python: missing` (`lib.rs:28122`) but does not hard-block. | **OPEN (partial).** Make Setup refuse to manage a repo when `python3` is absent. Effort S. #119. |
| M2 | Terminal I/O previews leak secrets. | **DONE** (`issue-114-secret-safe-observability`) — previews pass through the `loomweave-scanner`-backed host redactor before escaping/truncation. |
| M3 | PTY child inherits the full host env (`ANTHROPIC_API_KEY`/`GITHUB_TOKEN`); the agent can `echo` them. | **OPEN.** Pass the child an env allowlist behind an opt-in so it doesn't break `gh`/agent auth. Effort M. #114. |
| M4 | No durable, always-on record of commit/approval; a successful commit emits no trace line and the auth record is consumed-on-read. | **OPEN.** Append-only audit ledger for commit/push/issue-close/approval that survives `traces.enabled: false`. Effort M. #114. |
| M5 | Codex Bash gate path-blindness. | **NEEDS CONFIRMATION.** The Codex guard now has both `covered_paths` (`codex-worklist-guard.py:202`) and `_BASH_WRITE_PATTERNS` (`:306`); whether it intersects write targets with covered paths (vs. "any coverage passes") is unverified. #119. |
| M6 | `open_url` opened any `file://` in its default app. | **DONE** — `open_url` enforces a URL allowlist; `file://` is not permitted (`lib.rs:14205`). |
| M7 | `/__issue/comment` posts directly with only the frontend `enabled` binding as the gate. | **OPEN.** Route has no independent host auth check (`lib.rs:33200`). (The close path was resolved by H5.) Effort S. #121. |
| M8 | Inspector-tap fields forwarded unsanitized. | **DONE** (`issue-114-secret-safe-observability`) — fields pass through `__bramTraceSafeValue` before IPC; host redacts again before persistence. |
| M9 | `drop` auth lingered with no TTL. | **DONE** — the auth record (drop included) is rejected once past `WORKLIST_AUTH_TTL_MS` (`lib.rs:137`, `34490`), via the H4 work. |

### Low

| # | Finding | Status |
|---|---------|--------|
| L1 | Unbounded trace retention. | **DONE** (`issue-114-secret-safe-observability`) — raw archives past a configurable window stream through the redactor into `.log.gz`, retained indefinitely; failures preserve the raw file. |
| L2 | `/__worklist-history/snapshot` joins a caller `ts` into a filename (constrained to `.json`). | **OPEN (low).** Reject `..`/separators in the `ts` param. #110. |
| L3 | `session_path_for_id` joins a caller id into a path (constrained by `.jsonl` + `.exists()`, feeds a reload not an HTTP body). | **OPEN (low, latent).** #110. |
| L4 | No `mcp__*` matcher in `.claude/settings.json` PreToolUse. | **OPEN (latent).** Add the matcher before any filesystem MCP server is introduced. #119. |
| L5 | Guard doc/path drift to `app/__shell/*-guard.py`. | **DONE** — canonical paths moved to `app/provider-hooks/` and docs updated (`#217`, `96636b3`). |
| L6 | Tool Descriptions default-on with just an API key. | **DONE** (`issue-114-secret-safe-observability`) — explicit `ai.describeCommands` opt-in; material redacted before the request; trace records `redactions=N` only. |

## Live plan (what remains, ranked)

The Phase 0/1 quick wins, the root fix, and H6 (CORS) have all landed
(C1/C2/C3/H1/H2/H3/H6/M6/L5). The remaining work, in priority order:

1. **M3 — PTY child env allowlist**, behind an opt-in. (#114)
2. **Confirm M5** (Codex path intersection) and **close M1** (Setup hard-fail on
   missing Python). (#119)
3. **M4 — durable audit ledger** — the remaining #114 auditability tranche;
   value scales with shared use / demonstrability. (#114)
4. **M7** — an independent host check on `/__issue/comment`. (#121)
5. **Cleanups:** L2/L3 path-param guards, L4 `mcp__*` matcher, and per-command
   trust-boundary docs in `docs/apis.md`. (#110, #119, #113)

## Per-issue map

- **#108 — umbrella.** Open until the children below resolve.
- **#109 — worklist / approval gates.** Core state machine sound; H2 and H4
  landed and mutate/commit each independently re-verify the auth record. Direct
  concerns resolved; residual coverage risk is delegated to #119. **Effectively
  satisfied.**
- **#110 — filesystem containment.** Both arbitrary-read routes contained (C2,
  C3). Residual is L2/L3 only. **Effectively satisfied (Low residual).**
- **#111 / #114 — secrets hygiene and auditability (duplicates; #111 closed).**
  Observability/external-processing tranche complete (M2, M8, L1, L6). Two risks
  remain: M3 (env) and M4 (audit ledger). Heuristic redaction cannot prove
  arbitrary content secret-free. **Medium until M3/M4 land.**
- **#112 — PTY / shell injection (closed).** H1 and H2 resolved.
- **#113 — host-side IPC scoping.** C1 (the structural fix), C2, and H6 (CORS
  restricted to the shell origin) all landed; a same-user local process is the
  accepted residual. Remaining: per-command trust-boundary documentation in
  `docs/apis.md`. **Low.**
- **#118 — commit/push/issue-close gating (closed).** H5 resolved.
- **#119 — guard coverage across agents.** H3 and L5 landed. Residual: M1
  (fail-open without Python), M5 (confirm Codex path intersection), L4 (mcp
  matcher), and the self-test diagnostic. Note: the issue body still lists
  pre-#217 guard paths. **Medium.**
- **#120 — inflight/interrupt fail-closed (closed).** H4 and M9 resolved.
- **#121 — UI affordances must not be policy authority.** `/__issue/close`
  removed (H5); `open_url` allowlisted (M6). Residual is M7 (`/__issue/comment`
  gated only by frontend state). Same-origin/loopback exposure now concentrated
  in H6. **Effectively satisfied (M7 residual).**
