// Reentrant guard mode (bram-guard-reentrant-menu-hooks): `bram guard
// <hook-name>` runs the same binary the desktop shell ships, branched at the
// top of main() before any Tauri, webview, PTY, port-file, or trace-rotation
// side effect can execute. This is phase 1 of the Python-hook retirement
// (judell/bram#269 carries the migration receipts): SHADOW-ONLY. The ported
// hooks never POST, never decide, and always exit 0 — they compute what the
// authoritative Python hook would do and append one breadcrumb per
// invocation to the same resources/bram-traces/hook-events.log the Python
// menu hooks write, tagged `claude-rs` / `codex-rs`, so the divergence check
// is a grep-join of the two streams.
//
// Inertness is contractual, not incidental: outside the breadcrumb (written
// only when <root>/resources already exists — the same guard the Python
// hooks apply), guard mode creates nothing. `guard_mode_is_inert_without_
// resources_dir` is the test holding that line.

use std::io::Read;
use std::path::{Path, PathBuf};

const EVENTS_LOG_CAP_BYTES: u64 = 5 * 1024 * 1024;

pub fn run_guard_mode(args: &[String]) -> i32 {
    let hook = args.first().map(String::as_str).unwrap_or("");
    // Authority mode (bram-guard-authority-flip): `--authority` makes this
    // invocation the DECIDER — real exit codes, real deny output, the
    // Python guards deregistered by Setup while the guards.rustAuthority
    // flag is on. Without the flag, shadow semantics are byte-identical to
    // before: observe, breadcrumb, always exit 0.
    let authority = args.iter().any(|a| a == "--authority");
    let started = std::time::Instant::now();
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    // bram-guard-authority-fail-closed: keep the parse failure instead of
    // silently substituting `{}` — an empty payload has no tool name, which
    // the policy ALLOWS, so the substitution made every unreadable payload
    // fail open (the #249 class, reproduced in the new decider). Both
    // Python guards fail closed here: Codex explicitly
    // (codex-guard-stdin-fail-closed) and Claude via its top-level
    // catch-all ("allowing on a bug is an invisible hole").
    let (payload, stdin_error) = match serde_json::from_str(input.trim()) {
        Ok(v) => (v, None),
        Err(e) => (serde_json::json!({}), Some(e.to_string())),
    };
    match hook {
        "claude-permission-menu" if authority => {
            return authority_menu_hook("claude-rs", &payload, started);
        }
        "codex-permission-menu" if authority => {
            return authority_menu_hook("codex-rs", &payload, started);
        }
        "claude-permission-menu" => shadow_menu_hook("claude-rs", &payload, started),
        "codex-permission-menu" => shadow_menu_hook("codex-rs", &payload, started),
        "claude-worklist" if authority => {
            return authority_guarded_dispatch(
                "claude-rs",
                &payload,
                stdin_error,
                started,
                authority_worklist_hook,
            );
        }
        "codex-worklist" if authority => {
            return authority_guarded_dispatch(
                "codex-rs",
                &payload,
                stdin_error,
                started,
                authority_worklist_hook,
            );
        }
        "claude-worklist" => {
            shadow_worklist_hook_checked("claude-rs", &payload, stdin_error.as_deref(), started)
        }
        "codex-worklist" => {
            shadow_worklist_hook_checked("codex-rs", &payload, stdin_error.as_deref(), started)
        }
        other => {
            // Misregistration surfaces in stderr, never in the exit code: a
            // nonzero exit from a PreToolUse-shaped hook can block the tool
            // call, and shadow mode must be incapable of affecting anything.
            eprintln!("bram guard: unknown hook '{}'", other);
        }
    }
    0
}

// --- Authority mode (bram-guard-authority-flip) ---------------------------

// Walk up from `start` to the nearest resources/.bram-port, mirroring the
// Python guard's port discovery so the [hook] trace lands the same way.
fn find_port_upward(start: &Path) -> Option<u16> {
    let mut cur = start.to_path_buf();
    loop {
        let candidate = cur.join("resources").join(".bram-port");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            return text.trim().parse().ok();
        }
        if !cur.pop() {
            return None;
        }
    }
}

// Minimal fail-silent HTTP POST over std TcpStream — the guard must never
// block or fail a tool call because Bram is down (the Python guards use a
// 400ms urllib timeout for the same reason).
fn post_json_fail_silent(port: u16, path: &str, body: &str) {
    let _ = post_json_outcome(port, path, body);
}

// The same fire-and-forget POST, reporting an outcome token for the
// breadcrumb (the Python menu hook's phase-1 attribution: fail-silent made
// "lost in transit" and "host-side" indistinguishable, so the outcome rides
// the breadcrumb).
fn post_json_outcome(port: u16, path: &str, body: &str) -> &'static str {
    use std::io::Write as _;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = std::time::Duration::from_millis(400);
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, timeout) else {
        return "err=connect";
    };
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_read_timeout(Some(timeout));
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        port,
        body.len(),
        body
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return "err=io";
    }
    let mut buf = [0u8; 256];
    let _ = std::io::Read::read(&mut stream, &mut buf);
    "ok"
}

// The [hook] decision line, shipped exactly the way the Python guards ship
// theirs (POST /__hook-trace; the host writes it through the standard
// bram-trace path, gated on the LIVE Traces setting).
fn post_hook_trace(root: &Path, tool: &str, target: &str, decision: &str, reason: &str) {
    let Some(port) = find_port_upward(root) else {
        return;
    };
    let body = serde_json::json!({
        "script": "bram-guard",
        "event": "PreToolUse",
        "tool": tool,
        "target": target.chars().take(300).collect::<String>(),
        "cwd": root.to_string_lossy(),
        "decision": decision,
        "reason": reason,
    });
    post_json_fail_silent(port, "/__hook-trace", &body.to_string());
}

fn authority_worklist_hook(
    provider: &str,
    payload: &serde_json::Value,
    started: std::time::Instant,
) -> i32 {
    let tool = str_field(payload, "tool_name");
    let root = resolve_project_root(provider, payload);
    let Some(v) = crate::guard_policy::shadow_worklist_decision(provider, payload) else {
        return 0;
    };
    post_hook_trace(&root, &tool, &v.target, &v.decision, &v.reason);
    append_breadcrumb(
        &root,
        provider,
        "PreToolUse",
        &tool,
        &format!(
            "decided={} target={} ms={} reason={}",
            v.decision,
            if v.target.is_empty() { "-" } else { &v.target },
            started.elapsed().as_millis(),
            v.reason
        ),
    );
    if v.decision == "allow" {
        // Lifecycle bookkeeping writes: emit Claude's PreToolUse allow
        // decision so the user is not prompted (transcribed from
        // emit_allow_for_lifecycle in the Python guard).
        if provider == "claude-rs" && v.reason == "bram-lifecycle-channel" {
            let out = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "permissionDecisionReason": format!(
                        "Bram lifecycle channel ({}): implicitly authorized by the worklist flow, no per-write confirmation needed.",
                        v.target
                    ),
                }
            });
            println!("{}", out);
        }
        // Prose opt-out: land the direct-edit breadcrumb in the audit
        // ledger, as the Python guard does (opt-out-single-phrase-and-audit;
        // host-side dedup keys on turnKey, so any stable per-turn hash
        // works — the Python guard never runs concurrently with authority
        // mode, so sha256 compatibility is not required).
        if let Some(turn_key) = &v.audit_turn_key {
            if let Some(port) = find_port_upward(&root) {
                let body = serde_json::json!({
                    "provider": if provider == "codex-rs" { "codex" } else { "claude" },
                    "source": "bram-guard-opt-out",
                    "turnKey": turn_key,
                });
                post_json_fail_silent(port, "/__audit/direct-edit", &body.to_string());
            }
        }
        return 0;
    }
    // Deny. Codex answers by stdout JSON (permissionDecision deny, exit 0);
    // Claude by stderr message and exit 2 — each transcribed from its
    // Python guard's protocol.
    let message = if !v.message.is_empty() {
        v.message.clone()
    } else {
        crate::guard_policy::fallback_deny_message(&tool, &v.target, &root, &v.reason)
    };
    if provider == "codex-rs" {
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": if message.is_empty() { "blocked".to_string() } else { message },
            }
        });
        println!("{}", out);
        return 0;
    }
    eprintln!("{}", message);
    2
}

// --- Fail-closed on guard faults (bram-guard-authority-fail-closed) --------
//
// Two fault classes, each ported from the Python guard that handles it:
//
// - Unparseable stdin. Codex-Python denies explicitly
//   (codex-guard-stdin-fail-closed, a #249 follow-up); Claude-Python reaches
//   the same end through its catch-all. The Rust guard previously substituted
//   `{}` and allowed — silently, with exit 0 and no receipt.
// - Panics — the Rust spelling of Python's top-level catch-all. Uncontained,
//   a panic exits 101, which Claude Code treats as a NON-blocking hook error:
//   the tool call proceeds. So without containment every crash path in the
//   authority decider failed open.
//
// Shadow mode is deliberately untouched: its inert exit-0 contract matches
// the Python menu hooks' fail-open stance for observe-only surfaces.

enum GuardFault {
    UnparseableStdin(String),
    Panic(String),
}

fn fault_summary(fault: &GuardFault) -> String {
    match fault {
        GuardFault::UnparseableStdin(detail) => format!("unparseable stdin: {}", detail),
        GuardFault::Panic(detail) => format!("guard panic: {}", detail),
    }
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// Claude's message: the Python catch-all's three-line block verbatim
// (claude-worklist-guard.py tail), summary substituted.
fn claude_fault_message(summary: &str) -> String {
    format!(
        "Blocked: the Bram worklist guard failed and denied by default.\n  - {}\n  - This is a guard bug, not a policy decision. Please report it.",
        truncate_chars(summary, 500)
    )
}

// Codex's message: the stdin case uses codex-guard-stdin-fail-closed's
// sentence (codex-worklist-guard.py:1696ff); the panic case uses its
// catch-all's sentence. Same skeletons, Rust detail in the slot where
// Python names the exception.
fn codex_fault_message(fault: &GuardFault) -> String {
    match fault {
        GuardFault::UnparseableStdin(detail) => format!(
            "Blocked: the Bram worklist guard could not parse its hook payload (unparseable stdin: {}). This is a guard/runtime issue, not a policy decision; please report it.",
            truncate_chars(detail, 200)
        ),
        GuardFault::Panic(_) => format!(
            "Blocked: the Bram worklist guard failed and denied by default. {}. This is a guard bug, not a policy decision; please report it.",
            truncate_chars(&fault_summary(fault), 500)
        ),
    }
}

// Trace decision per provider+fault, matching where each Python guard
// classifies: Codex's stdin failure goes through deny() (decision=deny);
// everything else is the catch-all's decision=error.
fn fault_trace_decision(provider: &str, fault: &GuardFault) -> &'static str {
    match (provider, fault) {
        ("codex-rs", GuardFault::UnparseableStdin(_)) => "deny",
        _ => "error",
    }
}

fn panic_summary(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

// The fail-closed exit itself: trace + breadcrumb + per-provider protocol.
// Codex answers by stdout deny JSON (exit 0) with the message mirrored to
// stderr, exactly like its deny(); Claude by stderr + exit 2, exactly like
// its catch-all. Root is passed in so tests can point the side effects at a
// scratch tree.
fn authority_fail_closed(
    root: &Path,
    provider: &str,
    tool: &str,
    fault: GuardFault,
    started: std::time::Instant,
) -> i32 {
    let summary = fault_summary(&fault);
    let decision = fault_trace_decision(provider, &fault);
    let (message, trace_reason) = if provider == "codex-rs" {
        let m = codex_fault_message(&fault);
        // deny() trims to the message's first line, 120 chars; the
        // catch-all uses summary[:200].
        let r = match fault {
            GuardFault::UnparseableStdin(_) => {
                truncate_chars(m.lines().next().unwrap_or("blocked"), 120)
            }
            GuardFault::Panic(_) => truncate_chars(&summary, 200),
        };
        (m, r)
    } else {
        (claude_fault_message(&summary), truncate_chars(&summary, 200))
    };
    post_hook_trace(root, tool, "", decision, &trace_reason);
    append_breadcrumb(
        root,
        provider,
        "PreToolUse",
        tool,
        &format!(
            "decided={} target=- ms={} reason={}",
            decision,
            started.elapsed().as_millis(),
            summary
        ),
    );
    if provider == "codex-rs" {
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": message,
            }
        });
        println!("{}", out);
        eprintln!("{}", message);
        return 0;
    }
    eprintln!("{}", message);
    2
}

// Authority dispatch with both fault paths in front of the policy: stdin
// that never parsed fails closed before the hook runs, and a panic inside
// the hook is contained to the same fail-closed exit instead of escaping as
// exit 101. The hook is a parameter so a test can drive containment with a
// deliberately panicking hook (tripwire provenance: deliberate fire, never
// a wait).
fn authority_guarded_dispatch(
    provider: &str,
    payload: &serde_json::Value,
    stdin_error: Option<String>,
    started: std::time::Instant,
    hook: fn(&str, &serde_json::Value, std::time::Instant) -> i32,
) -> i32 {
    let root = resolve_project_root(provider, payload);
    let tool = str_field(payload, "tool_name");
    if let Some(detail) = stdin_error {
        return authority_fail_closed(
            &root,
            provider,
            &tool,
            GuardFault::UnparseableStdin(detail),
            started,
        );
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hook(provider, payload, started)
    })) {
        Ok(code) => code,
        Err(p) => authority_fail_closed(
            &root,
            provider,
            &tool,
            GuardFault::Panic(panic_summary(&p)),
            started,
        ),
    }
}

// Shadow twin of the stdin fault path: no decision, no exit-code change —
// one breadcrumb naming the would-deny so the parity join covers the path.
fn shadow_worklist_hook_checked(
    provider: &str,
    payload: &serde_json::Value,
    stdin_error: Option<&str>,
    started: std::time::Instant,
) {
    if stdin_error.is_some() {
        let root = resolve_project_root(provider, payload);
        append_breadcrumb(
            &root,
            provider,
            "PreToolUse",
            "?",
            &format!(
                "would=deny target=- ms={} reason=unparseable-stdin",
                started.elapsed().as_millis()
            ),
        );
        return;
    }
    shadow_worklist_hook(provider, payload, started);
}

fn str_field(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// Mirror of the Python adapters' root resolution. Claude: CLAUDE_PROJECT_DIR
// first — the payload cwd tracks the shell's persistent `cd`, and a session
// working outside the repo resolved a root with no resources/.bram-port,
// silently losing every POST (2026-07-18 pane-blind windows). Codex has no
// equivalent env pin; its hook payloads carry the project cwd.
fn resolve_project_root(provider: &str, payload: &serde_json::Value) -> PathBuf {
    let cwd = str_field(payload, "cwd");
    if provider == "claude-rs" {
        if let Some(dir) = std::env::var_os("CLAUDE_PROJECT_DIR") {
            if !dir.is_empty() {
                return PathBuf::from(dir);
            }
        }
    }
    if !cwd.is_empty() {
        return PathBuf::from(cwd);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// The would-decision: which menu route the authoritative Python hook POSTs
// for this event, or None when it does nothing. This mapping IS the ported
// policy for the two observe-only menu hooks; keeping it a pure function is
// what makes the divergence check and the unit tests cheap.
fn menu_would_action(provider: &str, event: &str, tool: &str) -> Option<&'static str> {
    match provider {
        "claude-rs" => match event {
            "PermissionRequest" => Some("/__menu/permission"),
            // Family B: AskUserQuestion has no PermissionRequest; its
            // choices live in tool_input.questions.
            "PreToolUse" if tool == "AskUserQuestion" => Some("/__menu/permission"),
            // Answered (PostToolUse) or declined via No/Esc
            // (PermissionDenied) — either way the prompt resolved.
            "PostToolUse" | "PermissionDenied" => Some("/__menu/permission/clear"),
            _ => None,
        },
        "codex-rs" => match event {
            "PermissionRequest" => Some("/__menu/permission"),
            "PostToolUse" => Some("/__menu/permission/clear"),
            _ => None,
        },
        _ => None,
    }
}

fn read_port(root: &Path) -> Option<u16> {
    std::fs::read_to_string(root.join("resources").join(".bram-port"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

// --- Authority menu mode (guards-rust-authority-covers-menu-hooks) ---------
//
// The shadow's would-POSTs made real: the same routes, payloads, token echo,
// and 400ms fail-silent discipline as the Python menu hooks. Deliberately
// FAIL-OPEN and always exit 0, matching the Python hooks' observe-only
// stance — a menu hook must never gate or delay a tool call (contrast with
// the worklist guard's fail-closed authority mode). Unparseable stdin
// arrives here as `{}`, which matches no event and breadcrumbs action=none —
// the same end state as Python's `payload = {}` fallback.

// The POST body per provider+event, transcribed from the Python hooks. Raw
// payload values ride through (nulls included) so the host sees identical
// JSON from either implementation.
fn menu_post_body(
    provider: &str,
    event: &str,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let field = |k: &str| payload.get(k).cloned().unwrap_or(serde_json::Value::Null);
    let tool_input = payload
        .get("tool_input")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if provider == "codex-rs" {
        return serde_json::json!({
            "provider": "codex",
            "hook_event_name": event,
            "tool_name": field("tool_name"),
            "tool_input": tool_input,
            "permission_mode": field("permission_mode"),
            "session_id": field("session_id"),
            "turn_id": field("turn_id"),
            "transcript_path": field("transcript_path"),
        });
    }
    match event {
        "PermissionRequest" => serde_json::json!({
            "tool_name": field("tool_name"),
            "tool_input": tool_input,
            "permission_suggestions": payload
                .get("permission_suggestions")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
            "tool_use_id": field("tool_use_id"),
        }),
        // Family B: AskUserQuestion via PreToolUse — no suggestions; the
        // host builds the menu from tool_input.questions.
        "PreToolUse" => serde_json::json!({
            "tool_name": field("tool_name"),
            "tool_input": tool_input,
            "permission_suggestions": [],
            "tool_use_id": field("tool_use_id"),
        }),
        // PostToolUse / PermissionDenied: clear, keyed by signature —
        // PermissionRequest claims carry no tool_use_id, so id-only clears
        // matched nothing (parallel-menu-claim-queue soak, 2026-07-18).
        _ => serde_json::json!({
            "tool_use_id": field("tool_use_id"),
            "tool_name": field("tool_name"),
            "tool_input": tool_input,
        }),
    }
}

fn authority_menu_hook(
    provider: &str,
    payload: &serde_json::Value,
    started: std::time::Instant,
) -> i32 {
    let event = str_field(payload, "hook_event_name");
    let tool = str_field(payload, "tool_name");
    let root = resolve_project_root(provider, payload);
    let Some(path) = menu_would_action(provider, &event, &tool) else {
        append_breadcrumb(
            &root,
            provider,
            &event,
            &tool,
            &format!("action=none ms={}", started.elapsed().as_millis()),
        );
        return 0;
    };
    // Root-local port only (no walk-up), like the Python menu hooks' _port:
    // a session outside the project must skip, not POST to a parent Bram.
    let Some(port) = read_port(&root) else {
        append_breadcrumb(&root, provider, &event, &tool, "post=skipped port=none");
        return 0;
    };
    let mut body = menu_post_body(provider, &event, payload);
    // Echo the per-session token Bram set in this agent's env so the route
    // can tell our POSTs from a foreign agent's. Absent (agent not launched
    // by Bram) -> route rejects, which is the intended behavior.
    if let Ok(token) = std::env::var("BRAM_MENU_TOKEN") {
        if !token.is_empty() {
            body["bram_token"] = serde_json::Value::String(token);
        }
    }
    let outcome = post_json_outcome(port, path, &body.to_string());
    append_breadcrumb(
        &root,
        provider,
        &event,
        &tool,
        &format!(
            "post={} {} ms={}",
            path,
            outcome,
            started.elapsed().as_millis()
        ),
    );
    0
}

fn shadow_menu_hook(provider: &str, payload: &serde_json::Value, started: std::time::Instant) {
    let event = str_field(payload, "hook_event_name");
    let tool = str_field(payload, "tool_name");
    let root = resolve_project_root(provider, payload);
    let tail = match menu_would_action(provider, &event, &tool) {
        Some(path) => format!(
            "would-post={} port={} ms={}",
            path,
            read_port(&root)
                .map(|p| p.to_string())
                .unwrap_or_else(|| "none".to_string()),
            started.elapsed().as_millis()
        ),
        None => format!("action=none ms={}", started.elapsed().as_millis()),
    };
    append_breadcrumb(&root, provider, &event, &tool, &tail);
}

// The worklist guard's shadow: compute the would-decision, write one
// breadcrumb, decide nothing. `shadow_worklist_decision` returning None means
// this provider has no ported policy yet, which records as `action=none` — the
// same shape the menu shadow uses for an event it would not act on.
fn shadow_worklist_hook(provider: &str, payload: &serde_json::Value, started: std::time::Instant) {
    let event = str_field(payload, "hook_event_name");
    let tool = str_field(payload, "tool_name");
    let root = resolve_project_root(provider, payload);
    // `reason=` is deliberately LAST and runs to end-of-line: Codex reasons
    // are prose derived from the Python guard's deny message (spaces and
    // all, byte-for-byte parity), so every fixed-width field must precede
    // it for the line to stay whitespace-splittable.
    let tail = match crate::guard_policy::shadow_worklist_decision(provider, payload) {
        Some(v) => format!(
            "would={} target={} ms={} reason={}",
            v.decision,
            if v.target.is_empty() { "-" } else { &v.target },
            started.elapsed().as_millis(),
            v.reason,
        ),
        None => format!("action=none ms={}", started.elapsed().as_millis()),
    };
    append_breadcrumb(&root, provider, &event, &tool, &tail);
}

// One line per invocation into the Python menu hooks' breadcrumb file, same
// refusal-to-create discipline: no <root>/resources, no write anywhere; the
// existing log over the cap stops growing rather than rotating.
fn append_breadcrumb(root: &Path, provider: &str, event: &str, tool: &str, tail: &str) {
    let resources = root.join("resources");
    if !resources.is_dir() {
        return;
    }
    let dir = resources.join("bram-traces");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("hook-events.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > EVENTS_LOG_CAP_BYTES {
            return;
        }
    }
    let line = format!(
        "{} {} {} {} {}\n",
        utc_iso_millis(),
        provider,
        if event.is_empty() { "?" } else { event },
        if tool.is_empty() { "?" } else { tool },
        tail
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

// ISO-8601 UTC with milliseconds, matching the Python breadcrumbs' shape,
// without pulling a chrono dependency into guard mode. Civil-from-days per
// Howard Hinnant's algorithm.
fn utc_iso_millis() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ms = now.as_millis() as u64;
    let secs = ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year,
        month,
        d,
        h,
        m,
        s,
        ms % 1000
    )
}

// --- Stable registration path -------------------------------------------

// `~/.bram/bram-guard` is the path hook config references, refreshed to
// point at the current binary so app moves and updates never orphan the
// registration (the ~/.bram install-and-refresh discipline the Codex Python
// guard already uses, pointed at a link instead of a script). Symlink on
// Unix; hardlink-else-copy on Windows, where symlinks need privileges.
pub fn install_guard_link(home: &Path, exe: &Path) -> std::io::Result<PathBuf> {
    let dir = home.join(".bram");
    std::fs::create_dir_all(&dir)?;
    let link = dir.join(if cfg!(windows) {
        "bram-guard.exe"
    } else {
        "bram-guard"
    });
    #[cfg(unix)]
    {
        match std::fs::read_link(&link) {
            Ok(target) if target == exe => return Ok(link),
            Ok(_) => std::fs::remove_file(&link)?,
            Err(_) => {
                if link.exists() {
                    std::fs::remove_file(&link)?;
                }
            }
        }
        std::os::unix::fs::symlink(exe, &link)?;
    }
    #[cfg(windows)]
    {
        let same = match (std::fs::metadata(&link), std::fs::metadata(exe)) {
            (Ok(a), Ok(b)) => {
                a.len() == b.len()
                    && a.modified().ok().is_some()
                    && a.modified().ok() == b.modified().ok()
            }
            _ => false,
        };
        if !same {
            if link.exists() {
                std::fs::remove_file(&link)?;
            }
            if std::fs::hard_link(exe, &link).is_err() {
                std::fs::copy(exe, &link)?;
            }
        }
    }
    Ok(link)
}

pub fn ensure_bram_guard_link() -> Option<PathBuf> {
    let home = crate::home_dir()?;
    let exe = std::env::current_exe().ok()?;
    // windows-guard-bin-no-console: prefer the dedicated guard entrypoint
    // sitting beside the main binary — GUI subsystem on Windows in every
    // profile, so hook spawns never allocate a conhost. Resolve through
    // symlinks first (the documented ./bram launch reports the symlink
    // path, whose parent is the repo root, not the target dir). Fall back
    // to the main binary when the artifact is absent (older build), which
    // preserves exactly the previous behavior.
    let real = exe.canonicalize().unwrap_or_else(|_| exe.clone());
    let sibling = real.parent().map(|d| {
        d.join(if cfg!(windows) {
            "bram-guard.exe"
        } else {
            "bram-guard"
        })
    });
    let target = match sibling {
        Some(s) if s.exists() => s,
        _ => exe,
    };
    install_guard_link(&home, &target).ok()
}

pub fn guard_hook_command(link: &Path, hook: &str) -> String {
    format!("\"{}\" guard {}", link.display(), hook)
}

#[cfg(test)]
mod guard_mode_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bram-guard-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn count_entries(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
    }

    #[test]
    fn would_action_mirrors_the_python_menu_hooks() {
        // Claude adapter.
        assert_eq!(
            menu_would_action("claude-rs", "PermissionRequest", "Bash"),
            Some("/__menu/permission")
        );
        assert_eq!(
            menu_would_action("claude-rs", "PreToolUse", "AskUserQuestion"),
            Some("/__menu/permission")
        );
        assert_eq!(menu_would_action("claude-rs", "PreToolUse", "Bash"), None);
        assert_eq!(
            menu_would_action("claude-rs", "PostToolUse", "Bash"),
            Some("/__menu/permission/clear")
        );
        assert_eq!(
            menu_would_action("claude-rs", "PermissionDenied", "Write"),
            Some("/__menu/permission/clear")
        );
        // Codex adapter: no PermissionDenied event, no AskUserQuestion family.
        assert_eq!(
            menu_would_action("codex-rs", "PermissionRequest", "apply_patch"),
            Some("/__menu/permission")
        );
        assert_eq!(
            menu_would_action("codex-rs", "PostToolUse", "apply_patch"),
            Some("/__menu/permission/clear")
        );
        assert_eq!(
            menu_would_action("codex-rs", "PermissionDenied", "Bash"),
            None
        );
    }

    #[test]
    fn guard_mode_is_inert_without_resources_dir() {
        // The contract: no <root>/resources, no write anywhere under root.
        let root = scratch("inert");
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "cwd": root.to_string_lossy(),
        });
        shadow_menu_hook("codex-rs", &payload, std::time::Instant::now());
        assert_eq!(count_entries(&root), 0, "guard mode created files");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn breadcrumb_lands_when_resources_exists() {
        let root = scratch("crumb");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "cwd": root.to_string_lossy(),
        });
        shadow_menu_hook("codex-rs", &payload, std::time::Instant::now());
        let log = std::fs::read_to_string(root.join("resources/bram-traces/hook-events.log"))
            .expect("breadcrumb written");
        assert!(
            log.contains("codex-rs PermissionRequest Bash"),
            "log: {log}"
        );
        assert!(
            log.contains("would-post=/__menu/permission port=none ms="),
            "log: {log}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn read_crumbs(root: &Path) -> String {
        std::fs::read_to_string(root.join("resources/bram-traces/hook-events.log"))
            .unwrap_or_default()
    }

    #[test]
    fn claude_authority_fails_closed_on_unparseable_stdin() {
        // Python twin: the catch-all — decision=error trace, "denied by
        // default" stderr, exit 2. The pre-fix Rust behavior was allow/exit 0
        // with no receipt.
        let root = scratch("fault-claude-stdin");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let code = authority_fail_closed(
            &root,
            "claude-rs",
            "Write",
            GuardFault::UnparseableStdin("expected value at line 1 column 1".into()),
            std::time::Instant::now(),
        );
        assert_eq!(code, 2, "Claude fail-closed must exit 2 (blocking)");
        let log = read_crumbs(&root);
        assert!(log.contains("claude-rs PreToolUse Write"), "log: {log}");
        assert!(
            log.contains("decided=error target=- ms="),
            "log: {log}"
        );
        assert!(log.contains("reason=unparseable stdin:"), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_authority_fails_closed_on_unparseable_stdin() {
        // Python twin: codex-guard-stdin-fail-closed — deny() protocol
        // (stdout deny JSON, exit 0), decision=deny trace.
        let root = scratch("fault-codex-stdin");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let code = authority_fail_closed(
            &root,
            "codex-rs",
            "apply_patch",
            GuardFault::UnparseableStdin("EOF while parsing".into()),
            std::time::Instant::now(),
        );
        assert_eq!(code, 0, "Codex denies via stdout protocol, exit 0");
        let log = read_crumbs(&root);
        assert!(log.contains("codex-rs PreToolUse apply_patch"), "log: {log}");
        assert!(log.contains("decided=deny target=- ms="), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn authority_dispatch_contains_panics_to_fail_closed() {
        // Deliberate fire: a panicking hook must convert to the fail-closed
        // exit, never escape as 101 (which Claude Code reads as a
        // NON-blocking error — the tool call would proceed).
        let root = scratch("fault-panic");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "cwd": root.to_string_lossy(),
        });
        fn boom(_: &str, _: &serde_json::Value, _: std::time::Instant) -> i32 {
            panic!("deliberate test panic");
        }
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let code =
            authority_guarded_dispatch("claude-rs", &payload, None, std::time::Instant::now(), boom);
        std::panic::set_hook(prev);
        assert_eq!(code, 2, "panic must fail closed, not exit 101");
        let log = read_crumbs(&root);
        assert!(log.contains("decided=error"), "log: {log}");
        assert!(
            log.contains("reason=guard panic: deliberate test panic"),
            "log: {log}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn authority_dispatch_prefers_stdin_fault_over_hook() {
        // With a parse failure recorded, the hook must never run at all —
        // the empty substitute payload is exactly what the policy would
        // wrongly allow.
        let root = scratch("fault-stdin-first");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let payload = serde_json::json!({ "cwd": root.to_string_lossy() });
        fn must_not_run(_: &str, _: &serde_json::Value, _: std::time::Instant) -> i32 {
            panic!("hook ran despite stdin fault");
        }
        let code = authority_guarded_dispatch(
            "claude-rs",
            &payload,
            Some("bad json".into()),
            std::time::Instant::now(),
            must_not_run,
        );
        assert_eq!(code, 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shadow_breadcrumbs_unparseable_stdin_and_stays_inert() {
        // Shadow keeps its exit-0 observe-only contract but names the
        // would-deny so the parity join covers the path.
        let root = scratch("fault-shadow");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let payload = serde_json::json!({ "cwd": root.to_string_lossy() });
        shadow_worklist_hook_checked(
            "codex-rs",
            &payload,
            Some("bad json"),
            std::time::Instant::now(),
        );
        let log = read_crumbs(&root);
        assert!(
            log.contains("would=deny target=- ms="),
            "log: {log}"
        );
        assert!(log.contains("reason=unparseable-stdin"), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fault_messages_match_the_python_shapes() {
        let m = claude_fault_message("unparseable stdin: x");
        assert!(m.starts_with("Blocked: the Bram worklist guard failed and denied by default."));
        assert!(m.contains("  - unparseable stdin: x"));
        assert!(m.contains("This is a guard bug, not a policy decision. Please report it."));

        let s = codex_fault_message(&GuardFault::UnparseableStdin("x".into()));
        assert!(s.contains("could not parse its hook payload (unparseable stdin: x)"));
        assert!(s.contains("guard/runtime issue, not a policy decision"));

        let p = codex_fault_message(&GuardFault::Panic("y".into()));
        assert!(p.contains("failed and denied by default. guard panic: y."));

        assert_eq!(
            fault_trace_decision("codex-rs", &GuardFault::UnparseableStdin("x".into())),
            "deny"
        );
        assert_eq!(
            fault_trace_decision("codex-rs", &GuardFault::Panic("x".into())),
            "error"
        );
        assert_eq!(
            fault_trace_decision("claude-rs", &GuardFault::UnparseableStdin("x".into())),
            "error"
        );
    }

    #[test]
    fn menu_post_bodies_match_the_python_hooks() {
        // Claude PermissionRequest: the four-field claim body.
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": { "command": "ls" },
            "permission_suggestions": [{ "mode": "acceptEdits" }],
            "tool_use_id": "toolu_1",
        });
        let body = menu_post_body("claude-rs", "PermissionRequest", &payload);
        assert_eq!(body["tool_name"], "Bash");
        assert_eq!(body["tool_input"]["command"], "ls");
        assert_eq!(body["permission_suggestions"][0]["mode"], "acceptEdits");
        assert_eq!(body["tool_use_id"], "toolu_1");

        // Family B: AskUserQuestion via PreToolUse — empty suggestions.
        let body = menu_post_body("claude-rs", "PreToolUse", &payload);
        assert_eq!(body["permission_suggestions"], serde_json::json!([]));

        // Clear: signature-keyed (tool_use_id + tool_name + tool_input),
        // no suggestions field.
        let body = menu_post_body("claude-rs", "PostToolUse", &payload);
        assert_eq!(body["tool_use_id"], "toolu_1");
        assert_eq!(body["tool_name"], "Bash");
        assert!(body.get("permission_suggestions").is_none());

        // Codex: one shape for claim and clear, provider-tagged.
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "apply_patch",
            "tool_input": { "patch": "x" },
            "permission_mode": "on-request",
            "session_id": "s1",
            "turn_id": "t1",
            "transcript_path": "/p",
        });
        let body = menu_post_body("codex-rs", "PermissionRequest", &payload);
        assert_eq!(body["provider"], "codex");
        assert_eq!(body["hook_event_name"], "PermissionRequest");
        assert_eq!(body["permission_mode"], "on-request");
        assert_eq!(body["session_id"], "s1");
        assert_eq!(body["turn_id"], "t1");
        assert_eq!(body["transcript_path"], "/p");
    }

    #[test]
    fn authority_menu_stays_exit_zero_and_breadcrumbs_outcomes() {
        // No port file: skip, breadcrumb the skip, exit 0 — the menu hook is
        // deliberately fail-open (observe-only), unlike the worklist guard.
        let root = scratch("menu-auth");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "cwd": root.to_string_lossy(),
        });
        let code = authority_menu_hook("codex-rs", &payload, std::time::Instant::now());
        assert_eq!(code, 0);
        let log = read_crumbs(&root);
        assert!(log.contains("post=skipped port=none"), "log: {log}");

        // Unmatched event (unparseable stdin arrives as {}): action=none.
        let empty = serde_json::json!({ "cwd": root.to_string_lossy() });
        let code = authority_menu_hook("claude-rs", &empty, std::time::Instant::now());
        assert_eq!(code, 0);
        assert!(read_crumbs(&root).contains("action=none ms="), "no action=none crumb");

        // With a (dead) port: the POST fails, the breadcrumb names the
        // outcome, and the exit stays 0.
        std::fs::write(root.join("resources/.bram-port"), "1").unwrap();
        let code = authority_menu_hook("codex-rs", &payload, std::time::Instant::now());
        assert_eq!(code, 0);
        assert!(
            read_crumbs(&root).contains("post=/__menu/permission err="),
            "log: {}",
            read_crumbs(&root)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn install_guard_link_creates_and_retargets() {
        let home = scratch("link-home");
        let exe_a = home.join("bram-a");
        let exe_b = home.join("bram-b");
        std::fs::write(&exe_a, "a").unwrap();
        std::fs::write(&exe_b, "b").unwrap();

        let link = install_guard_link(&home, &exe_a).expect("create link");
        assert_eq!(std::fs::read_link(&link).unwrap(), exe_a);
        // Idempotent when the target is unchanged.
        let again = install_guard_link(&home, &exe_a).expect("recreate link");
        assert_eq!(again, link);
        // Retargets when the binary moved (app update / relocation).
        install_guard_link(&home, &exe_b).expect("retarget link");
        assert_eq!(std::fs::read_link(&link).unwrap(), exe_b);
        let _ = std::fs::remove_dir_all(&home);
    }
}
