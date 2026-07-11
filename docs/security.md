# Bram security assessment and action plan

Status: draft assessment, 2026-07-10. Source: a six-agent read-only audit,
one agent per trust boundary, each citing `file:line` evidence against the
real `src-tauri/src/lib.rs`, the provider guards, `app/__shell/helpers.js`,
and the XMLUI surfaces. Tracks umbrella issue #108 and its children #109,
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
  content — vanilla HTML/JS, a third-party dev server, a compromised CDN
  script. Should be treated as fully untrusted.
- **Any other local process / browser tab.** The host serves a loopback HTTP
  API on `127.0.0.1`; co-resident software can reach it if it learns the port
  (written to `resources/.bram-port`).

## Root causes

Most Critical/High findings are consequences of four shared roots, not twenty
independent bugs. Fixing the roots collapses the majority of the findings.

1. **The target-app iframe is same-origin with the shell.** The project is
   proxied through `tauri://localhost/__project/*` so it shares the shell's
   origin (`lib.rs:28946`), `withGlobalTauri: true` exposes `window.__TAURI__`
   to it, and `security.csp` is `null` (`tauri.conf.json`). Untrusted page
   content therefore inherits the full Tauri IPC command surface and the full
   loopback route surface.
2. **The loopback server is unauthenticated with wildcard CORS.** No per-session
   token, and every response sets `Access-Control-Allow-Origin: *`
   (`lib.rs:28906`). Any local caller that learns the port can drive the routes
   and read the responses. The codebase already has the right pattern to copy —
   the `BRAM_MENU_TOKEN` foreign-agent guard at `lib.rs:3107`.
3. **Authorization is derived from PTY input bytes.**
   `record_worklist_authorization_from_input` runs on every PTY write, including
   relayed iframe intents (`lib.rs:8483` → `24187`). Anything that can write to
   the PTY can forge a worklist `approved:` record.
4. **The Claude PreToolUse guard does not gate the `Bash` write surface.** For
   `tool_name == "Bash"` the guard checks only one `gh --body @` antipattern and
   then `sys.exit(0)` (`app/__shell/worklist-guard.py:492`). Redirections,
   `tee`, `sed -i`, `python -c`, and `git commit/push`/`gh issue close` all
   bypass the worklist. Codex gates these; Claude does not.

## Ranked action plan

Severity is impact-weighted against the threat model. Effort: **S** = < half a
day, **M** = half to two days, **L** = more than two days. Issue column maps to
the tracking issues.

### Critical

| # | Finding | Evidence | Issue | Effort |
|---|---------|----------|-------|--------|
| C1 | Same-origin iframe inherits Tauri IPC; `pty_write` / `queue_pty_intent` inject arbitrary bytes into the agent's stdin → arbitrary command execution with no user gesture. | `lib.rs:8397`, `8567`, `28946`; `tauri.conf.json` (`withGlobalTauri`, `csp:null`) | #113 #112 #121 | L (root) |
| C2 | `/__file` reads any absolute path with no canonicalization or containment; returns raw bytes (e.g. `~/.ssh/id_rsa`). | `lib.rs:26351` | #110 #113 | S |
| C3 | `/__context/file` reads any absolute path, returns content as JSON. Legitimate need (fixed home-dir config set) argues for an allowlist, not root containment. | `lib.rs:25836` | #110 | M |

### High

| # | Finding | Evidence | Issue | Effort |
|---|---------|----------|-------|--------|
| H1 | Bracketed-paste framing does not neutralize `\x1b[201~` in the payload; a payload containing the paste-end sequence + `\r` escapes the frame and auto-submits smuggled terminal input. | `lib.rs:8907` | #112 | S |
| H2 | Worklist authorization is forged from PTY input: relayed `toShell`/`toTurn`/`sendKeys` text of the form `approved: {...}` writes an auth record → injection self-authorizes repo mutations. | `lib.rs:8483`, `24187`, `8831` | #112 #109 | M |
| H3 | Claude guard exits 0 for all `Bash`; `>`, `tee`, `sed -i`, `python -c`, `git commit/push`, `gh issue close` bypass worklist coverage entirely. Codex's `_BASH_WRITE_PATTERNS` already implements the fix. | `worklist-guard.py:492`; `worklist-guard-codex.py:280` | #119 #118 | M |
| H4 | Interrupt/cancel clears the inflight sentinel but never consumes the `approved` auth record; `handle_worklist_mutate` deliberately ignores `consumedAtMs` → a stale approval can advance/prune on a later turn with no fresh click. | `lib.rs:6278`, `27732`, `27443` | #120 | M |
| H5 | `/__issue/close` closes an issue **and** performs `git push` (`push=true`) with no auth record, no `closesIssues` check, and no confirmation; agent-reachable via the allowlisted loopback curl. | `lib.rs:26292`, `7417`, `7471` | #118 #121 | M (S stopgap) |
| H6 | `Access-Control-Allow-Origin: *` on every loopback response amplifies the read + mutation routes to any local browser tab or process that learns the port. | `lib.rs:28906` | #110 #113 #121 | M |

### Medium

| # | Finding | Evidence | Issue | Effort |
|---|---------|----------|-------|--------|
| M1 | Guard fails **open** when Python is missing (Claude Code treats a failed PreToolUse hook as non-blocking); no host-side backstop for arbitrary-file writes. | `README.md:206`; `lib.rs:22045` | #119 | M (S for Setup hard-fail) |
| M2 | Terminal I/O previews (`pty-in` / `pty-out`) land verbatim in the agent-readable, default-on, unbounded trace log with no redaction. | `lib.rs:8254`, `8441`, `903` | #114 #111 | M |
| M3 | The PTY child inherits the full host environment including `ANTHROPIC_API_KEY` / `GITHUB_TOKEN`; the agent can `echo` them into its own context. | `lib.rs:7993` | #114 #111 | M |
| M4 | Auditability gap: a successful worklist commit emits no trace line (no sha/ids/files); the approval trail is gated on tracing being enabled and is otherwise ephemeral. | `lib.rs:27925`, `24247` | #114 #111 | M |
| M5 | Codex Bash gate is path-blind: any live proposed/applied item authorizes a Bash write to any *other* file. | `worklist-guard-codex.py:1049` | #119 | M |
| M6 | `open_url` routes `file://` URLs to `open_path`, opening any local file in its default app; scheme is not validated. | `lib.rs:11021`; `helpers.js:2434` | #113 #110 #121 | S |
| M7 | Issues-tab close/comment buttons call the host routes directly with only the frontend `enabled` binding as the gate; the routes have no independent auth check. | `Issues.xmlui:80`, `494`; `lib.rs:26270` | #121 | S |
| M8 | Inspector trace tap forwards agent-pane XMLUI entries verbatim, bypassing the `__bramTraceSafeValue` sanitizer; captures input values keystroke-by-keystroke. Off by default. | `helpers.js:5280`, `742` | #114 #111 | S–M |
| M9 | `drop` auth is written on resolve but consumed only later at `prune`; if prune never runs it lingers with no TTL and can drive a later prune. | `lib.rs:27242`, `27901` | #120 | S |

### Low

| # | Finding | Evidence | Issue | Effort |
|---|---------|----------|-------|--------|
| L1 | Trace logs are unbounded and never pruned (~11 GB across 1087 files at audit time); amplifies M2. `*.log` is gitignored, so exposure is local-disk only. | `lib.rs:759` | #114 #111 | S |
| L2 | `/__worklist-history/snapshot` joins a caller `ts` into a filename with no `..` guard (constrained to `.json` targets). | `lib.rs:27099` | #110 | S |
| L3 | `session_path_for_id` joins a caller session id into a path; constrained by the `.jsonl` suffix and `.exists()`, and the result feeds a session reload rather than an HTTP body (no exfil channel). | `lib.rs:9028` | #110 | S |
| L4 | No `mcp__*` matcher in `.claude/settings.json` PreToolUse; if a Claude session ever gains a filesystem MCP server, its writes would be ungated. Latent given current server inventory. | `.claude/settings.json` | #119 | S–M |
| L5 | Guard doc/path drift: `CLAUDE.md` / `conventions.md` cite `app/__shell/worklist-guard-codex.py`, but the real canonical path is `app/shell/worklist-guard-codex.py`. | `conventions.md` | #119 | S |
| L6 | `ai-describe` ships the command text, preceding agent prose, and command-output head to `api.anthropic.com`. Key handling is correct (never logged), but a secret in a command line is sent to the API. | `lib.rs:28654` | #114 | S |

## Recommended sequence

Ordered to buy the most risk reduction per unit effort. The Phase 0 wins are
all small-effort and independently shippable; Phase 1 is the structural fix that
shrinks the reachability of much of the rest.

- **Phase 0 — quick wins (all S, ship first).**
  - H1: strip / neutralize `\x1b[201~` (and ideally all `\x1b`) from outbound
    payloads before the `format!` at `lib.rs:8907`; add a unit test with an
    embedded paste-end.
  - C2: add root/allowlist containment to `/__file`, reusing the
    `canonicalize()` + `starts_with(root_canon)` guard already present in
    `/__local-file-preview` (`lib.rs:25882`).
  - M6: validate `open_url` schemes; drop or containment-check the `file://`
    → `open_path` branch.
  - L2/L3: reject `..` / path separators in the `ts` and `id` path parameters.

- **Phase 1 — the root fix (L).**
  - C1: cut the target-app iframe off from `window.__TAURI__` and from
    `pty_write` / `queue_pty_intent` — a distinct webview/origin, an
    `sandbox`ed iframe without `allow-same-origin`, or a capability scoped to
    the tools origin only. Set a real CSP. This simultaneously shrinks H2 and
    the *reachability* of C2, C3, H5, H6, M6.

- **Phase 2 — guard parity + fail-closed (M).**
  - H3: lift Codex's `_BASH_WRITE_PATTERNS` + coverage check into the Claude
    guard's `Bash` branch; re-sync the installed copy.
  - M1: have Setup refuse to manage a repo (hard Status error) when
    `python3` is absent, so the file-write gate cannot silently fail open.
  - M5: extract redirect / `tee` / `sed -i` / `cp` / `mv` targets from Bash
    commands and intersect with covered paths instead of "any coverage passes."

- **Phase 3 — auth lifecycle (M).**
  - H4 + M9: call `consume_worklist_authorization` on every cancel/interrupt
    path; add a freshness/TTL and an "interrupted" flag so
    `validate_worklist_mutate_authorization` rejects a cancelled record and a
    replay requires a fresh `approved:` payload. Narrow the
    `mutate`-ignores-`consumedAtMs` invariant to "same turn as the resolve."

- **Phase 4 — route hardening (M).**
  - H5: gate `/__issue/close` on a fresh `approved` auth record whose item
    declares `closesIssues` containing the number; require an explicit push
    authorization rather than the `push=true` side effect; make it POST-only.
  - H6: add a per-session bearer token (write it alongside `.bram-port`,
    require it on `/__*` routes) and tighten `Access-Control-Allow-Origin`
    from `*` to the shell origin. Copy the `BRAM_MENU_TOKEN` pattern
    (`lib.rs:3107`).
  - M7: add a confirm step to the Issues-tab close/comment buttons; the H5
    host check then covers the agent-direct path too.

- **Phase 5 — secrets and audit (M).**
  - M2: add a redaction pass to `bram_trace_preview` matching known secret
    shapes (`sk-ant-`, `ghp_` / `gho_` / `github_pat_`, `AKIA`, `Bearer`,
    `token=`/`password=`/`secret=`), and/or a default-off flag to suppress
    `pty-in` / `pty-out` previews while keeping byte-count metrics.
  - M3: pass the PTY child an env allowlist rather than `std::env::vars()`,
    gated behind an opt-in so it doesn't break `gh` / agent auth.
  - M4: emit a `worklist-commit op=commit sha=… ids=… files=…` line on
    success, and add a durable, always-on append-only audit record for
    commit / push / issue-close / approval that survives `traces.enabled:
    false`.
  - L1: cap trace retention (age or total bytes) in
    `prepare_bram_trace_log`.
  - M8: route the Inspector tap through `__bramTraceSafeValue`; keep it off
    when handling credentials.
  - L4/L5/L6: add an `mcp__.*` matcher to `.claude/settings.json`; fix the
    guard doc path references; optionally redact secret shapes from the
    `ai-describe` prompt.

## Per-issue map

- **#109 — worklist / approval gates (umbrella for the below).** Core state
  machine is sound: `/__worklist/mutate` and `/__worklist/commit` each
  independently re-read the auth record and enforce `kind`, id membership,
  `applied` status, and staged-file scoping. Residual risk is concentrated in
  the children below.
- **#110 — filesystem containment.** Repo-vs-host separation is otherwise good
  (git is root-scoped, writes are contained, `/__local-file-preview` already
  does the right thing). Exposure is C2, C3, and H6 plus L2/L3. **High** until
  the two arbitrary-read routes are contained.
- **#111 / #114 — secrets hygiene and auditability (duplicates).** No secret is
  committed, exposed in a response, or leaked to the iframe; `ANTHROPIC_API_KEY`
  handling in `ai-describe` is correct. Residual risk is the local, default-on,
  agent-readable trace log (M2, L1) and the auditability gap (M4). **Medium.**
- **#112 — PTY / shell injection.** H1 (paste-escape smuggling) and H2 (forged
  auth) are the sharp edges; both are reachable through the same-origin iframe
  (C1). **High.**
- **#113 — host-side IPC scoping.** The functional authorization plumbing is
  real, but the same-origin iframe design punches through it (C1) and `/__file`
  lacks containment (C2). Also update `docs/apis.md` to state the trust boundary
  per command/route explicitly. **High.**
- **#118 — gate commit/push/issue-close on worklist state.** Commit and prune
  are correctly gated; issue-close and push are not (H5), and Claude can bypass
  commit/push via Bash (H3). Two of four side effects ungated. **Moderate–High.**
- **#119 — guard coverage across agents.** Guards are in sync, executable, and
  registered, but the Claude `Bash` surface is ungated (H3), the guard fails
  open without Python (M1), and the Codex Bash gate is path-blind (M5).
  **High.**
- **#120 — inflight/interrupt fail-closed.** Sentinel lifecycle is well
  instrumented and the spinner recovers, but the *authorization* half fails open
  on interrupt (H4, M9). **High.**
- **#121 — UI-only affordances must not be the policy authority.** Worklist
  buttons correctly delegate to re-verifying host routes. But `/__issue/close`,
  `/__issue/comment`, and `open_url` are gated only by frontend state or bare
  reachability (H5, M6, M7). Same-origin policy blocks the cross-origin-page
  vector, keeping this from High. **Moderate.**
