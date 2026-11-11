// Reentrant guard mode: `bram-guard guard <hook-name>` (via the dedicated
// GUI-subsystem bin, or `bram guard …` on the main binary) runs the deciding
// hooks — the sole implementation since retire-python-hooks-rust-only
// (judell/bram#269 and #313 carry the port's migration and validation
// receipts; the Python guards this replaced live in git history). Worklist
// hooks decide with real exit codes and fail CLOSED on guard faults
// (bram-guard-authority-fail-closed); menu hooks POST the pane's menu
// claims/clears and stay deliberately fail-open, exit 0 always — a menu hook
// must never gate or delay a tool call. Every invocation appends one
// breadcrumb to <state>/bram-traces/hook-events.log, tagged `claude-rs` /
// `codex-rs` / `opencode-rs` / `antigravity-rs`, written only when the root's
// state folder already exists — `<root>/.bram` if it exists, else
// `<root>/resources` (issue-469, `guard_policy::state_dir`). Outside that,
// guard mode creates nothing (`guard_mode_is_inert_without_resources_dir`
// and its `.bram/` twin hold the line).

use std::io::Read;
use std::path::{Path, PathBuf};

// issue-439: shared with the host, which rotates the file at this size
// (`rotate_hook_events_log`, lib.rs). Here it is only the backstop for when no
// Bram host is running to rotate.
pub(crate) const EVENTS_LOG_CAP_BYTES: u64 = 5 * 1024 * 1024;
// The line the guard writes once when it finds the log over the cap.
pub(crate) const BREADCRUMBS_PAUSED_MARKER: &str = "breadcrumbs-paused";

pub fn run_guard_mode(args: &[String]) -> i32 {
    let hook = args.first().map(String::as_str).unwrap_or("");
    // `--authority` is accepted and ignored: it named the deciding mode
    // during the shadow-and-flip transition, and sessions launched before a
    // Setup rewrite still invoke the old registration strings. Deciding is
    // the only mode now.
    let started = std::time::Instant::now();
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    // bram-guard-authority-fail-closed: keep the parse failure instead of
    // silently substituting `{}` — an empty payload has no tool name, which
    // the policy ALLOWS, so the substitution made every unreadable payload
    // fail open (the #249 class, reproduced in the new decider before both
    // Python guards' deliberate fail-closed behavior was ported).
    let (payload, stdin_error) = match serde_json::from_str(input.trim()) {
        Ok(v) => (v, None),
        Err(e) => (serde_json::json!({}), Some(e.to_string())),
    };
    if let Some(code) = route_to_project_guard(hook, args, &payload, &input) {
        return code;
    }
    match hook {
        "claude-permission-menu" => authority_menu_hook("claude-rs", &payload, started),
        "codex-permission-menu" => authority_menu_hook("codex-rs", &payload, started),
        "claude-worklist" => authority_guarded_dispatch(
            "claude-rs",
            &payload,
            stdin_error,
            started,
            authority_worklist_hook,
        ),
        // current-session-from-events: Codex reports from the guard it already
        // runs, because registering a new hook event raises its "Hooks need
        // review" prompt. So a change shows at the next tool call.
        "codex-worklist" => {
            session_report("codex", &payload);
            authority_guarded_dispatch(
                "codex-rs",
                &payload,
                stdin_error,
                started,
                authority_worklist_hook,
            )
        }
        // opencode-worklist-guard: called by the Bram OpenCode plugin's
        // `execute.before` hook with OpenCode's own tool event.
        "opencode-worklist" => authority_guarded_dispatch(
            "opencode-rs",
            &opencode_policy_payload(&payload),
            stdin_error,
            started,
            authority_worklist_hook,
        ),
        // opencode-edit-advisory: called by the plugin's `execute.after` once
        // an OpenCode edit has landed.
        "opencode-after" => opencode_after_hook(&opencode_policy_payload(&payload), started),
        // antigravity-worklist-guard: registered by Setup in the project's
        // .agents/hooks.json as agy's PreToolUse hook. agy spawns hooks from
        // `<workspace>/.agents` rather than `workspacePaths[0]`, and Claude's
        // `normalize_target` resolves relative strings against the process cwd;
        // staying in `.agents/` turns every whitespace-free string in
        // `call_mcp_tool`'s `Arguments` (such as file `content`) into a
        // creatable `.agents/<token>` candidate and blocks covered MCP writes.
        "antigravity-worklist" => {
            enter_antigravity_workspace(&payload);
            antigravity_worklist_hook(&payload, stdin_error, started)
        }
        "antigravity-session" => {
            session_report("antigravity", &payload);
            0
        }
        other => {
            // Misregistration surfaces in stderr, never in the exit code: a
            // nonzero exit from a PreToolUse-shaped hook could block a tool
            // call over a typo in a registration string.
            eprintln!("bram guard: unknown hook '{}'", other);
            0
        }
    }
}

// --- Per-project guard routing (guard-link-per-project) -------------------
//
// ~/.bram/bram-guard is one link per machine, and every running Bram
// re-points it to the guard beside its own binary every 60 s, so with two
// instances on different builds every project's hooks ran whichever guard
// wrote last (2026-09-30: ~/bram's hooks on the installed guard while it
// ran a dev build). Each instance now records its own guard in
// <root>/resources/.bram-guard-target; whichever guard the link reaches
// hands the call to that one. Registrations don't change for either
// provider. Any routing failure falls back to deciding in-process, as
// before; routing never denies on its own.
//
// issue-469: the file lives in the root's state folder, `.bram/` if it
// exists, else `resources/`. `GUARD_TARGET_REL` keeps the legacy spelling
// for the host's callers until they move to `guard_target_path`.
pub const GUARD_TARGET_REL: &str = "resources/.bram-guard-target";
pub const GUARD_TARGET_LEAF: &str = ".bram-guard-target";

/// `<root>/<state folder>/.bram-guard-target`, for the folder resolved for `root`.
pub fn guard_target_path(root: &Path) -> PathBuf {
    crate::guard_policy::state_path(root, GUARD_TARGET_LEAF)
}

/// The first readable `<dir>/<state folder>/<leaf>` walking up from `start`,
/// trying `.bram/` then `resources/` at each level before moving up, so a
/// nested instance is found before its parent. Returns the file's text and
/// the directory it was found under.
fn find_state_file_upward(start: &Path, leaf: &str) -> Option<(String, PathBuf)> {
    let mut cur = start.to_path_buf();
    loop {
        for dir in crate::guard_policy::state_dir_candidates(&cur) {
            if let Ok(text) = std::fs::read_to_string(dir.join(leaf)) {
                return Some((text, cur));
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}
const GUARD_ROUTED_ENV: &str = "BRAM_GUARD_ROUTED";

// The project's recorded guard, when it exists and isn't `current`, with
// the project root it was recorded in (where the breadcrumb belongs; the
// hook's cwd can be a subdirectory with no resources/).
pub(crate) fn route_target(start: &Path, current: &Path) -> Option<(PathBuf, PathBuf)> {
    let (text, cur) = find_state_file_upward(start, GUARD_TARGET_LEAF)?;
    let target = PathBuf::from(text.trim());
    if text.trim().is_empty() || !target.is_file() {
        return None;
    }
    // guard-self-route-hardlink: this asks "is the target the binary already
    // running?", and path comparison answers it wrongly on Windows. The
    // per-machine link at ~/.bram/bram-guard.exe is a HARD link there (creating
    // a symlink needs Developer Mode or elevation), and canonicalize resolves
    // symlinks only — a hard link has no other name to collapse to, so the two
    // paths never compared equal and every hook spawned a second guard to route
    // to itself. Measured 2026-10-04 on Windows: 56.3 ms per hook routed vs
    // 23.0 ms direct, on every file-mutating tool call, with a single build on
    // the machine. 2ed8f2c's "about 5-7 ms, only while another build holds the
    // link" described the symlink case only.
    //
    // is_same_file compares the volume serial and file index on Windows, and
    // device+inode on Unix, so it recognises hard links, symlinks and plain
    // paths alike. The cheap path comparison stays in front of it: the common
    // cross-build case differs by path and needn't open two handles.
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(&target) == canon(current) {
        return None;
    }
    if same_file::is_same_file(&target, current).unwrap_or(false) {
        return None;
    }
    Some((target, cur))
}

fn route_to_project_guard(
    hook: &str,
    args: &[String],
    payload: &serde_json::Value,
    input: &str,
) -> Option<i32> {
    if std::env::var_os(GUARD_ROUTED_ENV).is_some() {
        return None;
    }
    let provider = if hook.starts_with("codex-") {
        "codex-rs"
    } else if hook.starts_with("opencode-") {
        "opencode-rs"
    } else if hook.starts_with("antigravity-") {
        "antigravity-rs"
    } else {
        "claude-rs"
    };
    let start = resolve_project_root(provider, payload);
    let current = std::env::current_exe().ok()?;
    let (target, root) = route_target(&start, &current)?;
    let mut child_args: Vec<String> = vec!["guard".to_string()];
    child_args.extend(args.iter().cloned());
    let spawned = std::process::Command::new(&target)
        .args(&child_args)
        .env(GUARD_ROUTED_ENV, "1")
        .stdin(std::process::Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            append_breadcrumb(
                &root,
                provider,
                "route-skip",
                hook,
                &format!("to={} reason=spawn-failed: {}", target.display(), e),
            );
            return None;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(input.as_bytes());
    }
    match child.wait().ok().and_then(|s| s.code()) {
        Some(code) => {
            append_breadcrumb(
                &root,
                provider,
                "route",
                hook,
                &format!(
                    "from={} to={} exit={}",
                    current.display(),
                    target.display(),
                    code
                ),
            );
            Some(code)
        }
        None => {
            append_breadcrumb(
                &root,
                provider,
                "route-skip",
                hook,
                &format!("to={} reason=no-exit-code", target.display()),
            );
            None
        }
    }
}

// --- Deciding worklist hooks ----------------------------------------------

// Walk up from `start` to the nearest `.bram/.bram-port` or
// `resources/.bram-port` (both names at each level, `.bram/` first,
// issue-469), mirroring the Python guard's port discovery so the [hook]
// trace lands the same way.
fn find_port_upward(start: &Path) -> Option<u16> {
    let (text, _) = find_state_file_upward(start, ".bram-port")?;
    text.trim().parse().ok()
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

// agy-reads-conversation-id: rule 1 of the session spine
// (docs/decisions/2026-10-27-bram-chooses-the-session.md). agy's PreInvocation
// names the conversation that is running, including after `/resume` inside
// the TUI. Captured on agy 1.4.12, 2026-11-02:
// - An agy in another terminal has no BRAM_PORT or BRAM_MENU_TOKEN, so it
//   reports nothing, and a mismatched token is refused by the route.
// - A subagent fires PreInvocation under its own conversationId, interleaved
//   with its parent's, with an empty lastUserInput; the parent's carries the
//   prompt. That is observed, not documented. If it stops holding, the
//   Transcript follows the subagent until the parent's next invocation.
fn session_report_body(
    provider: &str,
    payload: &serde_json::Value,
    token: &str,
) -> Option<serde_json::Value> {
    let (session, mine) = if provider == "antigravity" {
        (
            str_field(payload, "conversationId"),
            !str_field(payload, "lastUserInput").is_empty(),
        )
    } else {
        // current-session-from-events: Claude's and Codex's subagent hooks
        // carry the parent's session_id plus an agent_id (Claude 2.1, captured
        // 2026-11-02; Codex: docs/agent-specimens/codex). Skipped regardless,
        // so a CLI that changes this can't move the session.
        (
            str_field(payload, "session_id"),
            str_field(payload, "agent_id").is_empty(),
        )
    };
    if session.is_empty() || token.is_empty() || !mine {
        return None;
    }
    Some(serde_json::json!({
        "provider": provider,
        "session": session,
        "bram_token": token,
    }))
}

// POST a token-authenticated report to the Bram that launched this agent. The
// port comes from this terminal's environment, not resources/.bram-port,
// which names whichever instance wrote it last; the token is per terminal, so
// any other instance would refuse it anyway. `body` builds the request from
// the token and returns None when there is nothing to report. Never affects
// the hook's outcome: agy blocks on a failing hook, and this only informs.
fn post_terminal_report(
    path: &str,
    body: impl FnOnce(&str) -> Option<serde_json::Value>,
) -> &'static str {
    let token = std::env::var("BRAM_MENU_TOKEN").unwrap_or_default();
    let Some(body) = body(&token) else {
        return "skipped=no-agent-or-token";
    };
    let Some(port) = std::env::var("BRAM_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return "skipped=no-port";
    };
    post_json_outcome(port, path, &body.to_string())
}

fn session_report(provider: &str, payload: &serde_json::Value) {
    let _ = post_terminal_report("/__agent/session", |token| {
        session_report_body(provider, payload, token)
    });
}

// delegated-work-action-gates: the /__agent/subagent body for a Claude
// SubagentStart / SubagentStop payload. None without an agent_id or a token
// (an agent Bram didn't launch), which the route would refuse anyway.
fn subagent_report_body(payload: &serde_json::Value, token: &str) -> Option<serde_json::Value> {
    let agent_id = str_field(payload, "agent_id");
    if agent_id.is_empty() || token.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "event": str_field(payload, "hook_event_name"),
        "agent_id": agent_id,
        "agent_type": str_field(payload, "agent_type"),
        "session_id": str_field(payload, "session_id"),
        // A subagent that backgrounds a command and ends its turn fires
        // SubagentStop with that task still pending; it resumes when the task
        // finishes. The host keeps the binding while this list is non-empty
        // (delegated-work-action-gates; seen live 2026-11-04).
        "background_tasks": payload
            .get("background_tasks")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
        // delegated-stop-counts-only-the-subagents-own-tasks: the list above is
        // session-wide; the host reads this transcript to see which tasks are
        // the subagent's own.
        "agent_transcript_path": str_field(payload, "agent_transcript_path"),
        "bram_token": token,
    }))
}

fn subagent_report(payload: &serde_json::Value) -> &'static str {
    post_terminal_report("/__agent/subagent", |token| {
        subagent_report_body(payload, token)
    })
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

// opencode-worklist-guard: OpenCode's tool events in the vocabulary the
// policy already judges, so OpenCode gets the same gate rather than a third
// copy of it. `edit` / `write` / `shell` take Claude's shapes; `patch` carries
// the same `*** Begin Patch` text as Codex's apply_patch, so it takes Codex's.
// Any other tool passes through by name, which the policy allows. Both
// flavors read coverage from the same worklist_covered_files.
//
// opencode-mcp-guard: an MCP call reaches the plugin as `<server>_<tool>`,
// which can't be told from a built-in by name (built-ins have namespaces too:
// `opencode_worktree_create`, `browser_fill`). The plugin names the server
// and tool when OpenCode's own lists say the tool's namespace is a connected
// MCP server; spelled `mcp__<server>__<tool>`, the call then takes the
// policy's MCP branch exactly as Claude's do.
fn opencode_policy_payload(payload: &serde_json::Value) -> serde_json::Value {
    let input = payload
        .get("input")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let cwd = str_field(payload, "cwd");
    let mcp = payload.get("mcp").and_then(|m| {
        let server = m.get("server")?.as_str()?;
        let tool = m.get("tool")?.as_str()?;
        (!server.is_empty() && !tool.is_empty()).then(|| format!("mcp__{}__{}", server, tool))
    });
    if let Some(tool_name) = mcp {
        return serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": tool_name,
            "tool_input": input,
            "cwd": cwd,
        });
    }
    let field = |k: &str| input.get(k).cloned().unwrap_or(serde_json::Value::Null);
    // The plugin sends absolute paths today; a relative one is resolved the
    // way OpenCode resolves it, against the project directory.
    let path = input
        .get("path")
        .or_else(|| input.get("filePath"))
        .and_then(|v| v.as_str())
        .map(|p| {
            if Path::new(p).is_absolute() || cwd.is_empty() {
                p.to_string()
            } else {
                Path::new(&cwd).join(p).to_string_lossy().into_owned()
            }
        });
    let (tool_name, tool_input) = match str_field(payload, "tool").as_str() {
        "edit" => (
            "Edit".to_string(),
            serde_json::json!({
                "file_path": path,
                "old_string": field("oldString"),
                "new_string": field("newString"),
                "replace_all": field("replaceAll"),
            }),
        ),
        "write" => (
            "Write".to_string(),
            serde_json::json!({ "file_path": path, "content": field("content") }),
        ),
        "shell" => (
            "Bash".to_string(),
            serde_json::json!({ "command": field("command") }),
        ),
        "patch" => (
            "apply_patch".to_string(),
            serde_json::json!({ "input": field("patchText") }),
        ),
        other => (other.to_string(), input.clone()),
    };
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": tool_name,
        "tool_input": tool_input,
        "cwd": cwd,
    })
}

// antigravity-worklist-guard: agy's PreToolUse payload in the vocabulary the
// policy already judges, as OpenCode's is. The three native tools that change
// files take Claude's shapes, and `call_mcp_tool` maps `ServerName` and
// `ToolName` to `mcp__<server>__<tool>` with `Arguments` as `tool_input` so
// Claude's MCP branch screens it (agy-guard-mcp-coverage). A payload that
// doesn't have the captured shape never gets here: antigravity_worklist_hook
// refuses it first, because this mapping would turn it into an empty tool name
// or empty input, which the policy allows. Field names are from
// docs/agent-specimens/antigravity/hooks/payloads/PreToolUse.jsonl.
//
// The payload has no `cwd`, and the hook runner's working directory isn't the
// workspace (the specimen pack's relative logger path failed there), so the
// root is `workspacePaths[0]`. No `transcript_path` either: agy's transcript
// isn't Claude-shaped, so the typed opt-out reaches this guard through the
// host's direct-edit record, as it does for Codex and OpenCode.
fn antigravity_policy_payload(payload: &serde_json::Value) -> serde_json::Value {
    let cwd = antigravity_workspace(payload).unwrap_or_default();
    let call = payload
        .get("toolCall")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let name = str_field(&call, "name");
    let args = call
        .get("args")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let field = |k: &str| args.get(k).cloned().unwrap_or(serde_json::Value::Null);
    // agy sends absolute paths; a relative one is resolved against the
    // workspace, as for OpenCode.
    let path = args.get("Path").and_then(|v| v.as_str()).map(|p| {
        if Path::new(p).is_absolute() || cwd.is_empty() {
            p.to_string()
        } else {
            Path::new(&cwd).join(p).to_string_lossy().into_owned()
        }
    });
    let (tool_name, tool_input) = match name.as_str() {
        "write_file" => (
            "Write".to_string(),
            serde_json::json!({ "file_path": path, "content": field("Content") }),
        ),
        "edit_file" => (
            "Edit".to_string(),
            serde_json::json!({
                "file_path": path,
                "old_string": field("Target"),
                "new_string": field("Replacement"),
            }),
        ),
        "run_command" => (
            "Bash".to_string(),
            serde_json::json!({ "command": field("CommandLine") }),
        ),
        "call_mcp_tool" => (
            format!(
                "mcp__{}__{}",
                str_field(&args, "ServerName"),
                str_field(&args, "ToolName")
            ),
            args.get("Arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        ),
        other => (other.to_string(), args.clone()),
    };
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": tool_name,
        "tool_input": tool_input,
        "cwd": cwd,
    })
}

// agy-guard-fail-closed-on-drift: what's wrong with a payload that doesn't
// have the captured shape, or None when it does. The hook's matcher admits
// only the four captured tools, so anything else means agy's payload changed,
// and agy updates itself without a Bram release. Run on the raw payload,
// because translating erases the difference: renaming `toolCall`, `name`,
// `Path` or `Arguments` comes out as a tool or input the policy has nothing to
// refuse.
fn antigravity_payload_fault(payload: &serde_json::Value) -> Option<String> {
    if antigravity_workspace(payload).is_none() {
        return Some("no workspacePaths".to_string());
    }
    let Some(call) = payload.get("toolCall") else {
        return Some("no toolCall".to_string());
    };
    let name = str_field(call, "name");
    if name == "call_mcp_tool" {
        let Some(args) = call.get("args") else {
            return Some("call_mcp_tool has no ServerName".to_string());
        };
        for key in ["ServerName", "ToolName"] {
            let ok = args
                .get(key)
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            if !ok {
                return Some(format!("call_mcp_tool has no {}", key));
            }
        }
        if !args.get("Arguments").is_some_and(|v| v.is_object()) {
            return Some("call_mcp_tool has no Arguments".to_string());
        }
        return None;
    }
    let required = match name.as_str() {
        "write_file" | "edit_file" => "Path",
        "run_command" => "CommandLine",
        "" => return Some("no tool name".to_string()),
        other => return Some(format!("unexpected tool {}", other)),
    };
    let present = call
        .get("args")
        .and_then(|args| args.get(required))
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    if present {
        None
    } else {
        Some(format!("{} has no {}", name, required))
    }
}

// Unparseable stdin keeps its own fault in the dispatch; the shape check is
// for a payload that parsed.
fn antigravity_worklist_hook(
    payload: &serde_json::Value,
    stdin_error: Option<String>,
    started: std::time::Instant,
) -> i32 {
    let fault = if stdin_error.is_none() {
        antigravity_payload_fault(payload)
    } else {
        None
    };
    if let Some(detail) = fault {
        let tool = payload
            .get("toolCall")
            .map(|call| str_field(call, "name"))
            .unwrap_or_default();
        return authority_fail_closed(
            &resolve_project_root("antigravity-rs", payload),
            "antigravity-rs",
            &tool,
            GuardFault::UnrecognizedPayload(detail),
            started,
        );
    }
    authority_guarded_dispatch(
        "antigravity-rs",
        &antigravity_policy_payload(payload),
        stdin_error,
        started,
        authority_worklist_hook,
    )
}

fn antigravity_workspace(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("workspacePaths")?
        .as_array()?
        .first()?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn enter_antigravity_workspace(payload: &serde_json::Value) {
    if let Some(ws) = antigravity_workspace(payload) {
        let _ = std::env::set_current_dir(ws);
    }
}

// agy reads one JSON object from stdout. An explicit `allow` doesn't skip
// agy's own permission check (the spike's `run_command` was still held for
// agy's "command" permission), so the guard never answers `auto_approve`,
// which would. Empty output also allows, so every path prints a decision.
fn antigravity_answer(decision: &str, reason: &str) -> i32 {
    let out = if reason.is_empty() {
        serde_json::json!({ "decision": decision })
    } else {
        serde_json::json!({ "decision": decision, "reason": reason })
    };
    println!("{}", out);
    0
}

// opencode-edit-advisory: the addressed-turn advisory Claude and Codex get as
// PostToolUse additionalContext. OpenCode has no such field; its plugin appends
// the note to the tool's result, which is what OpenCode's model reads. One JSON
// line, `advisory` null when there is nothing to say. Exit 0 always and never
// a deny: the edit has already happened.
fn opencode_after_hook(payload: &serde_json::Value, started: std::time::Instant) -> i32 {
    let tool = str_field(payload, "tool_name");
    let advisory = if matches!(tool.as_str(), "Edit" | "Write" | "apply_patch") {
        crate::guard_policy::opencode_addressed_turn_advisory(payload)
    } else {
        None
    };
    append_breadcrumb(
        &resolve_project_root("opencode-rs", payload),
        "opencode-rs",
        "PostToolUse",
        &tool,
        &format!(
            "advisory={} ms={}",
            if advisory.is_some() { "yes" } else { "no" },
            started.elapsed().as_millis()
        ),
    );
    println!("{}", serde_json::json!({ "advisory": advisory }));
    0
}

// The OpenCode plugin reads one JSON line from stdout; anything else, it
// treats as a deny.
fn opencode_answer(decision: &str, message: &str) -> i32 {
    println!(
        "{}",
        serde_json::json!({ "decision": decision, "message": message })
    );
    0
}

fn authority_worklist_hook(
    provider: &str,
    payload: &serde_json::Value,
    started: std::time::Instant,
) -> i32 {
    let tool = str_field(payload, "tool_name");
    let root = resolve_project_root(provider, payload);
    let policy_provider = match provider {
        "opencode-rs" if tool == "apply_patch" => "codex-rs",
        "opencode-rs" | "antigravity-rs" => "claude-rs",
        other => other,
    };
    let Some(v) = crate::guard_policy::shadow_worklist_decision(policy_provider, payload) else {
        return match provider {
            "opencode-rs" => opencode_answer("allow", ""),
            "antigravity-rs" => antigravity_answer("allow", ""),
            _ => 0,
        };
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
                    "provider": provider.trim_end_matches("-rs"),
                    "source": "bram-guard-opt-out",
                    "turnKey": turn_key,
                });
                post_json_fail_silent(port, "/__audit/direct-edit", &body.to_string());
            }
        }
        if provider == "opencode-rs" {
            return opencode_answer("allow", "");
        }
        if provider == "antigravity-rs" {
            return antigravity_answer("allow", "");
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
    if provider == "opencode-rs" {
        return opencode_answer(
            "deny",
            if message.is_empty() {
                "blocked"
            } else {
                &message
            },
        );
    }
    if provider == "antigravity-rs" {
        return antigravity_answer(
            "deny",
            if message.is_empty() {
                "blocked"
            } else {
                &message
            },
        );
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
    // agy-guard-fail-closed-on-drift: parsed, but not the captured shape.
    UnrecognizedPayload(String),
    Panic(String),
}

fn fault_summary(fault: &GuardFault) -> String {
    match fault {
        GuardFault::UnparseableStdin(detail) => format!("unparseable stdin: {}", detail),
        GuardFault::UnrecognizedPayload(detail) => format!("unrecognized payload: {}", detail),
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
        GuardFault::UnrecognizedPayload(_) | GuardFault::Panic(_) => format!(
            "Blocked: the Bram worklist guard failed and denied by default. {}. This is a guard bug, not a policy decision; please report it.",
            truncate_chars(&fault_summary(fault), 500)
        ),
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
    // codex-stdin-fault-decided-error: every guard fault traces decided=error
    // on both providers — the one greppable signature separating "the guard
    // broke" from "the guard refused". (The port originally transcribed
    // Codex-Python's deny() classification for the stdin case; that fidelity
    // constraint retired with the Python guards. The PROVIDER protocol is
    // untouched — Codex still answers with the structured deny below.)
    let decision = "error";
    let message = if provider == "codex-rs" {
        codex_fault_message(&fault)
    } else {
        claude_fault_message(&summary)
    };
    let trace_reason = truncate_chars(&summary, 200);
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
    if provider == "opencode-rs" {
        eprintln!("{}", message);
        return opencode_answer("deny", &message);
    }
    // agy would block a crashed guard anyway (the spike: any nonzero exit,
    // a kill at the timeout, or invalid JSON blocks the call), but an
    // explicit deny carries the reason to the model.
    if provider == "antigravity-rs" {
        eprintln!("{}", message);
        return antigravity_answer("deny", &message);
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
    // antigravity-worklist-guard: per-project routing reads agy's raw
    // payload, which names the workspace but has no `cwd`, and agy doesn't
    // run hooks from the workspace.
    if provider == "antigravity-rs" {
        if let Some(ws) = antigravity_workspace(payload) {
            return PathBuf::from(ws);
        }
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
    std::fs::read_to_string(crate::guard_policy::state_path(root, ".bram-port"))
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
fn menu_post_body(provider: &str, event: &str, payload: &serde_json::Value) -> serde_json::Value {
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
    // current-session-from-events: SessionStart fires at once on `/clear` and
    // on `/resume` inside the CLI, with the new id. UserPromptSubmit restores
    // the session after a nested `claude -p`, which inherits the token.
    if provider == "claude-rs" && matches!(event.as_str(), "SessionStart" | "UserPromptSubmit") {
        session_report("claude", payload);
        let cwd_s = str_field(payload, "cwd");
        let cwd = if cwd_s.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            std::path::PathBuf::from(cwd_s)
        };
        if crate::guard_policy::claude_enforcement_paused(&cwd) {
            let out = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": event,
                    "additionalContext": crate::guard_policy::enforcement_paused_context(),
                }
            });
            println!("{}", out);
            let root = resolve_project_root(provider, payload);
            append_breadcrumb(
                &root,
                provider,
                &event,
                "",
                "action=enforcement-paused-context",
            );
        }
        return 0;
    }
    // delegated-work-action-gates: report subagent start/stop so the host can
    // bind the subagent to the claim it was spawned under and release it when
    // it finishes. Fail-open like the session report: always exit 0 and print
    // nothing. SubagentStop's exit 2 would PREVENT the subagent stopping, and
    // its stdout `decision: block` would too, so neither is ever produced.
    if provider == "claude-rs" && matches!(event.as_str(), "SubagentStart" | "SubagentStop") {
        let root = resolve_project_root(provider, payload);
        let outcome = subagent_report(payload);
        append_breadcrumb(
            &root,
            provider,
            &event,
            "",
            &format!(
                "post=/__agent/subagent {} ms={}",
                outcome,
                started.elapsed().as_millis()
            ),
        );
        return 0;
    }
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
    // addressed-turn-edits-stay-in-item-files: after a file-tool edit, tell
    // the agent when the target isn't among the addressed items' files.
    // PostToolUse additionalContext is the documented channel to the model
    // (https://code.claude.com/docs/en/agent-sdk/hooks) and for Codex, where
    // it is "added as extra developer context"
    // (https://learn.chatgpt.com/docs/hooks). Codex's file edit is
    // apply_patch, also matched as Edit / Write. Still exit 0: this hook
    // never gates.
    let advisory = if event != "PostToolUse" {
        None
    } else if provider == "claude-rs"
        && matches!(
            tool.as_str(),
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
        )
    {
        crate::guard_policy::addressed_turn_advisory(payload)
    } else if provider == "codex-rs" && matches!(tool.as_str(), "apply_patch" | "Edit" | "Write") {
        crate::guard_policy::codex_addressed_turn_advisory(payload)
    } else {
        None
    };
    {
        if let Some(msg) = advisory {
            let out = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": msg,
                }
            });
            println!("{}", out);
        }
    }
    0
}

// One line per invocation into the hook breadcrumb file, same
// refusal-to-create discipline the retired Python hooks applied: no
// state folder under <root> (`.bram/` or `resources/`), no write anywhere;
// the existing log over the cap stops
// growing rather than rotating. `reason=` fields are deliberately LAST and
// run to end-of-line: Codex reasons are prose with spaces, so every
// fixed-width field must precede them for lines to stay
// whitespace-splittable.
pub(crate) fn append_breadcrumb(root: &Path, provider: &str, event: &str, tool: &str, tail: &str) {
    let state = crate::guard_policy::state_dir(root);
    if !state.is_dir() {
        return;
    }
    let dir = state.join("bram-traces");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("hook-events.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > EVENTS_LOG_CAP_BYTES {
            // issue-439: stopping silently left two weeks of guard decisions
            // missing with nothing to say why. Say so once, then stop. The
            // host rotates the file within a minute when Bram is running.
            append_paused_marker_once(&path, provider, meta.len());
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

// issue-439: append the paused marker unless the file's tail already has it.
// Reading the last 4 KB keeps this cheap on a 5 MB file, and once the marker
// is the last line every later call finds it there.
fn append_paused_marker_once(path: &Path, provider: &str, bytes: u64) {
    use std::io::{Seek, SeekFrom, Write};
    let Ok(mut f) = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let tail_len = bytes.min(4096);
    let mut tail = Vec::new();
    if f.seek(SeekFrom::End(-(tail_len as i64))).is_ok() {
        let _ = (&f).take(tail_len).read_to_end(&mut tail);
    }
    if String::from_utf8_lossy(&tail).contains(BREADCRUMBS_PAUSED_MARKER) {
        return;
    }
    let line = format!(
        "{} {} {} - reason=cap bytes={} cap={} (no host rotated the log; later breadcrumbs are dropped)\n",
        utc_iso_millis(),
        provider,
        BREADCRUMBS_PAUSED_MARKER,
        bytes,
        EVENTS_LOG_CAP_BYTES
    );
    let _ = f.write_all(line.as_bytes());
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
    let target = own_guard_binary()?;
    install_guard_link(&home, &target).ok()
}

// The guard this instance ships with: the dedicated guard binary beside the
// running main binary, else the main binary itself. The link points here,
// and the host records it in <root>/resources/.bram-guard-target.
pub fn own_guard_binary() -> Option<PathBuf> {
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
    Some(match sibling {
        Some(s) if s.exists() => s,
        _ => exe,
    })
}

pub fn guard_hook_command(link: &Path, hook: &str) -> String {
    format!("\"{}\" guard {}", link.display(), hook)
}

#[cfg(test)]
mod guard_mode_tests {
    use super::*;

    #[test]
    fn subagent_report_carries_agent_identity_and_never_blocks() {
        // Shape from docs/agent-specimens/claude/hooks/SubagentStop.json.
        let stop = serde_json::json!({
            "session_id": "4f97c209",
            "agent_id": "ae3fb2df9de52ea71",
            "agent_type": "",
            "hook_event_name": "SubagentStop",
            "stop_hook_active": false,
        });
        let body = subagent_report_body(&stop, "tok").unwrap();
        assert_eq!(body["event"], "SubagentStop");
        assert_eq!(body["agent_id"], "ae3fb2df9de52ea71");
        assert_eq!(body["session_id"], "4f97c209");
        assert_eq!(body["bram_token"], "tok");
        // No background_tasks in the payload: an empty list, so the host releases.
        assert_eq!(body["background_tasks"], serde_json::json!([]));
        // A pause with a pending background task is forwarded as-is, so the
        // host can keep the binding until the subagent's final stop.
        // delegated-stop-counts-only-the-subagents-own-tasks: forwarded for the host.
        let mut with_path = stop.clone();
        with_path["agent_transcript_path"] = serde_json::json!("/tmp/agent-x.jsonl");
        let body = subagent_report_body(&with_path, "tok").unwrap();
        assert_eq!(body["agent_transcript_path"], "/tmp/agent-x.jsonl");
        let mut paused = stop.clone();
        paused["background_tasks"] = serde_json::json!([{ "id": "bdv5odlm8" }]);
        let body = subagent_report_body(&paused, "tok").unwrap();
        assert_eq!(
            body["background_tasks"].as_array().map(|a| a.len()),
            Some(1)
        );
        // No token (not launched by Bram) or no agent_id: nothing to send.
        assert!(subagent_report_body(&stop, "").is_none());
        let no_agent = serde_json::json!({ "hook_event_name": "SubagentStart" });
        assert!(subagent_report_body(&no_agent, "tok").is_none());
        // Both events exit 0 (exit 2 on SubagentStop would keep the subagent
        // from stopping). No agent_id here on purpose: a test run inside a
        // Bram terminal inherits BRAM_PORT/BRAM_MENU_TOKEN, and a real POST
        // would bind a fake agent in that live instance.
        for event in ["SubagentStart", "SubagentStop"] {
            let p = serde_json::json!({ "hook_event_name": event });
            assert_eq!(
                authority_menu_hook("claude-rs", &p, std::time::Instant::now()),
                0
            );
        }
    }

    #[test]
    fn session_report_skips_subagents_and_foreign_terminals() {
        let turn = serde_json::json!({ "conversationId": "c1", "lastUserInput": "hi" });
        let body = session_report_body("antigravity", &turn, "tok").unwrap();
        assert_eq!(body["session"], "c1");
        assert_eq!(body["bram_token"], "tok");
        // Another terminal has no token.
        assert!(session_report_body("antigravity", &turn, "").is_none());
        // A subagent's payload has an empty lastUserInput (agy 1.4.12).
        let sub = serde_json::json!({ "conversationId": "c2", "lastUserInput": "" });
        assert!(session_report_body("antigravity", &sub, "tok").is_none());
        let none = serde_json::json!({ "lastUserInput": "hi" });
        assert!(session_report_body("antigravity", &none, "tok").is_none());
        for provider in ["claude", "codex"] {
            let start = serde_json::json!({ "session_id": "s1", "source": "clear" });
            let body = session_report_body(provider, &start, "tok").unwrap();
            assert_eq!(body["provider"], provider);
            assert_eq!(body["session"], "s1");
            assert!(session_report_body(provider, &start, "").is_none());
            let sub = serde_json::json!({ "session_id": "s1", "agent_id": "a1" });
            assert!(session_report_body(provider, &sub, "tok").is_none());
        }
    }

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

    // guard-link-per-project
    #[test]
    fn route_target_finds_the_project_guard_from_a_subdirectory() {
        let root = scratch("route-found");
        let nested = root.join("app").join("tools");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let project_guard = root.join("project-guard");
        std::fs::write(&project_guard, "").unwrap();
        let running_guard = root.join("running-guard");
        std::fs::write(&running_guard, "").unwrap();
        std::fs::write(
            root.join(GUARD_TARGET_REL),
            format!("{}\n", project_guard.display()),
        )
        .unwrap();
        let (target, found_root) = route_target(&nested, &running_guard).expect("routed");
        assert_eq!(
            target.canonicalize().unwrap(),
            project_guard.canonicalize().unwrap()
        );
        assert_eq!(found_root, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn route_target_declines_self_missing_and_absent_targets() {
        let root = scratch("route-declines");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let running_guard = root.join("running-guard");
        std::fs::write(&running_guard, "").unwrap();
        // No target file anywhere up the tree from here is fine only if the
        // walk ends; scratch dirs live under the system temp dir.
        std::fs::write(
            root.join(GUARD_TARGET_REL),
            format!("{}\n", running_guard.display()),
        )
        .unwrap();
        assert_eq!(route_target(&root, &running_guard), None, "self");
        std::fs::write(
            root.join(GUARD_TARGET_REL),
            format!("{}\n", root.join("gone").display()),
        )
        .unwrap();
        assert_eq!(route_target(&root, &running_guard), None, "missing binary");
        std::fs::write(root.join(GUARD_TARGET_REL), "  \n").unwrap();
        assert_eq!(route_target(&root, &running_guard), None, "empty file");
        let _ = std::fs::remove_dir_all(&root);
    }

    // guard-self-route-hardlink: ~/.bram/bram-guard.exe is a hard link on
    // Windows, and canonicalize cannot collapse one, so the self check above
    // passed its own test while failing in the field — every hook spawned a
    // guard to route to itself. This fails on the path-comparison-only code.
    #[test]
    fn route_target_declines_a_hard_link_to_itself() {
        let root = scratch("route-declines-hardlink");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let running_guard = root.join("running-guard");
        std::fs::write(&running_guard, "guard").unwrap();

        // The per-machine link: a second name for the very same file.
        let link = root.join("linked-guard");
        if std::fs::hard_link(&running_guard, &link).is_err() {
            // Some filesystems refuse hard links; nothing to assert there.
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        assert_ne!(link, running_guard, "the two names must differ");

        std::fs::write(
            root.join(GUARD_TARGET_REL),
            format!("{}\n", running_guard.display()),
        )
        .unwrap();
        assert_eq!(
            route_target(&root, &link),
            None,
            "a hard link to the running guard is the running guard"
        );
        let _ = std::fs::remove_dir_all(&root);
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
        let code = authority_menu_hook("codex-rs", &payload, std::time::Instant::now());
        assert_eq!(code, 0);
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
        authority_menu_hook("codex-rs", &payload, std::time::Instant::now());
        let log = std::fs::read_to_string(root.join("resources/bram-traces/hook-events.log"))
            .expect("breadcrumb written");
        assert!(
            log.contains("codex-rs PermissionRequest Bash"),
            "log: {log}"
        );
        assert!(log.contains("post=skipped port=none"), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // issue-439: over the cap, the guard says so once and then stays quiet.
    #[test]
    fn over_the_cap_writes_the_paused_marker_once() {
        let root = scratch("crumb-cap");
        let dir = root.join("resources/bram-traces");
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("hook-events.log");
        std::fs::write(&log, vec![b'x'; (EVENTS_LOG_CAP_BYTES + 10) as usize]).unwrap();
        for _ in 0..3 {
            append_breadcrumb(&root, "claude-rs", "PreToolUse", "Bash", "decided=allow");
        }
        let text = String::from_utf8_lossy(&std::fs::read(&log).unwrap()).into_owned();
        assert_eq!(text.matches(BREADCRUMBS_PAUSED_MARKER).count(), 1);
        assert!(!text.contains("decided=allow"), "breadcrumbs stay dropped");
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
        assert!(log.contains("decided=error target=- ms="), "log: {log}");
        assert!(log.contains("reason=unparseable stdin:"), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_authority_fails_closed_on_unparseable_stdin() {
        // Python twin: codex-guard-stdin-fail-closed — deny() protocol
        // (stdout deny JSON, exit 0). The trace label is decided=error on
        // both providers (codex-stdin-fault-decided-error), so guard faults
        // grep apart from ordinary policy denials.
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
        assert!(
            log.contains("codex-rs PreToolUse apply_patch"),
            "log: {log}"
        );
        assert!(log.contains("decided=error target=- ms="), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // opencode-worklist-guard: the shapes the 2026-10-23 spike captured from
    // OpenCode 2.1.4's execute.before (`path`, not `filePath`).
    #[test]
    fn opencode_events_translate_to_the_policy_vocabulary() {
        let t = |v: serde_json::Value| opencode_policy_payload(&v);
        let edit = t(serde_json::json!({
            "tool": "edit", "cwd": "/p",
            "input": { "path": "/p/a.txt", "oldString": "x", "newString": "y" }
        }));
        assert_eq!(edit["hook_event_name"], "PreToolUse");
        assert_eq!(edit["tool_name"], "Edit");
        assert_eq!(edit["tool_input"]["file_path"], "/p/a.txt");
        assert_eq!(edit["tool_input"]["old_string"], "x");
        assert_eq!(edit["tool_input"]["new_string"], "y");
        assert_eq!(edit["cwd"], "/p");
        let write = t(serde_json::json!({
            "tool": "write", "cwd": "/p", "input": { "path": "b.txt", "content": "hi" }
        }));
        assert_eq!(write["tool_name"], "Write");
        assert_eq!(
            write["tool_input"]["file_path"],
            Path::new("/p").join("b.txt").to_string_lossy().as_ref()
        );
        assert_eq!(write["tool_input"]["content"], "hi");
        let shell = t(serde_json::json!({
            "tool": "shell", "cwd": "/p", "input": { "command": "echo done" }
        }));
        assert_eq!(shell["tool_name"], "Bash");
        assert_eq!(shell["tool_input"]["command"], "echo done");
        let patch = t(serde_json::json!({
            "tool": "patch", "cwd": "/p", "input": { "patchText": "*** Begin Patch" }
        }));
        assert_eq!(patch["tool_name"], "apply_patch");
        assert_eq!(patch["tool_input"]["input"], "*** Begin Patch");
        let read = t(serde_json::json!({
            "tool": "read", "cwd": "/p", "input": { "path": "/p/a.txt" }
        }));
        assert_eq!(read["tool_name"], "read");
        assert_eq!(read["tool_input"]["path"], "/p/a.txt");
        // opencode-mcp-guard: the 2026-10-26 spike's event for a local MCP
        // server's write_file, with the server and tool the plugin adds.
        let mcp = t(serde_json::json!({
            "tool": "spike_write_file", "cwd": "/p",
            "mcp": { "server": "spike", "tool": "write_file" },
            "input": { "path": "mcp-out.txt", "content": "hi" }
        }));
        assert_eq!(mcp["tool_name"], "mcp__spike__write_file");
        assert_eq!(
            mcp["tool_input"],
            serde_json::json!({ "path": "mcp-out.txt", "content": "hi" })
        );
        // Unmarked, the same name stays an unknown tool, as built-ins do.
        let unmarked = t(serde_json::json!({
            "tool": "opencode_worktree_create", "cwd": "/p", "input": { "name": "w" }
        }));
        assert_eq!(unmarked["tool_name"], "opencode_worktree_create");
    }

    // opencode-edit-advisory: what the plugin's execute.after gets back.
    #[test]
    fn opencode_edits_outside_the_addressed_items_get_the_advisory() {
        let root = scratch("opencode-advisory");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::write(root.join("resources/.worklist-authorization.json"), "{}").unwrap();
        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":1,"items":[
                {"id":"mine","status":"proposed","files":["a.txt"]},
                {"id":"other","status":"proposed","files":["b.txt"]}]}"#,
        )
        .unwrap();
        let context = root.join("resources/.turn-context.json");
        std::fs::write(
            &context,
            r#"{"schema":1,"addressedIds":["mine"],"kind":"approved"}"#,
        )
        .unwrap();
        let advise = |tool: &str, input: serde_json::Value| {
            crate::guard_policy::opencode_addressed_turn_advisory(&opencode_policy_payload(
                &serde_json::json!({ "tool": tool, "cwd": root.to_string_lossy(), "input": input }),
            ))
        };
        let at = |name: &str| root.join(name).to_string_lossy().into_owned();
        assert_eq!(
            advise(
                "edit",
                serde_json::json!({ "path": at("a.txt"), "oldString": "x", "newString": "y" })
            ),
            None
        );
        let stray = advise(
            "write",
            serde_json::json!({ "path": at("b.txt"), "content": "y" }),
        )
        .unwrap();
        assert!(
            stray.starts_with("Bram: `b.txt` isn't in the files"),
            "{stray}"
        );
        assert!(stray.contains("(mine)"), "{stray}");
        let both = advise(
            "patch",
            serde_json::json!({ "patchText": "*** Begin Patch\n*** Update File: a.txt\n@@\n-a\n+b\n*** Update File: b.txt\n@@\n-a\n+b\n*** End Patch" }),
        )
        .unwrap();
        assert!(both.starts_with("Bram: `b.txt` isn't"), "{both}");
        assert!(!both.contains("`a.txt`"), "{both}");
        // A read isn't an edit, and a general turn has nothing to be outside of.
        assert_eq!(
            advise("read", serde_json::json!({ "path": at("b.txt") })),
            None
        );
        std::fs::remove_file(&context).unwrap();
        assert_eq!(
            advise(
                "write",
                serde_json::json!({ "path": at("b.txt"), "content": "y" })
            ),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opencode_decisions_follow_worklist_coverage() {
        let root = scratch("opencode-coverage");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::write(root.join("resources/.worklist-authorization.json"), "{}").unwrap();
        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":1,"items":[{"id":"x","status":"proposed","files":["covered.txt"]}]}"#,
        )
        .unwrap();
        let decide = |tool: &str, input: serde_json::Value| {
            let p = opencode_policy_payload(&serde_json::json!({
                "tool": tool, "cwd": root.to_string_lossy(), "input": input
            }));
            let flavor = if p["tool_name"] == "apply_patch" {
                "codex-rs"
            } else {
                "claude-rs"
            };
            crate::guard_policy::shadow_worklist_decision(flavor, &p)
                .unwrap()
                .decision
        };
        let at = |name: &str| root.join(name).to_string_lossy().into_owned();
        let patch = |name: &str| {
            format!(
                "*** Begin Patch\n*** Update File: {}\n@@\n-a\n+b\n*** End Patch",
                name
            )
        };
        assert_eq!(
            decide(
                "write",
                serde_json::json!({ "path": at("covered.txt"), "content": "ok" })
            ),
            "allow"
        );
        assert_eq!(
            decide(
                "write",
                serde_json::json!({ "path": at("other.txt"), "content": "no" })
            ),
            "deny"
        );
        assert_eq!(
            decide(
                "edit",
                serde_json::json!({ "path": at("other.txt"), "oldString": "a", "newString": "b" })
            ),
            "deny"
        );
        assert_eq!(
            decide(
                "patch",
                serde_json::json!({ "patchText": patch("covered.txt") })
            ),
            "allow"
        );
        assert_eq!(
            decide(
                "patch",
                serde_json::json!({ "patchText": patch("other.txt") })
            ),
            "deny"
        );
        assert_eq!(
            decide("read", serde_json::json!({ "path": at("other.txt") })),
            "allow"
        );
        // opencode-mcp-guard: marked MCP calls are judged as Claude's are.
        // The MCP path search counts a bare name only if it exists, so both
        // files do, and the verdicts below come from coverage.
        std::fs::write(root.join("covered.txt"), "a").unwrap();
        std::fs::write(root.join("other.txt"), "a").unwrap();
        let decide_mcp = |tool: &str, input: serde_json::Value| {
            let p = opencode_policy_payload(&serde_json::json!({
                "tool": format!("fs_{}", tool), "cwd": root.to_string_lossy(),
                "mcp": { "server": "fs", "tool": tool }, "input": input
            }));
            crate::guard_policy::shadow_worklist_decision("claude-rs", &p)
                .unwrap()
                .decision
        };
        assert_eq!(
            decide_mcp(
                "write_file",
                serde_json::json!({ "path": at("covered.txt"), "content": "ok" })
            ),
            "allow"
        );
        assert_eq!(
            decide_mcp(
                "write_file",
                serde_json::json!({ "path": at("other.txt"), "content": "no" })
            ),
            "deny"
        );
        assert_eq!(
            decide_mcp(
                "read_text_file",
                serde_json::json!({ "path": at("other.txt") })
            ),
            "allow"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opencode_authority_fails_closed_on_unparseable_stdin() {
        let root = scratch("fault-opencode-stdin");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let code = authority_fail_closed(
            &root,
            "opencode-rs",
            "Write",
            GuardFault::UnparseableStdin("EOF while parsing".into()),
            std::time::Instant::now(),
        );
        assert_eq!(code, 0, "OpenCode denies via its stdout JSON line, exit 0");
        let log = read_crumbs(&root);
        assert!(log.contains("opencode-rs PreToolUse Write"), "log: {log}");
        assert!(log.contains("decided=error target=- ms="), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // antigravity-worklist-guard: agy 1.4.12's PreToolUse payloads as the
    // specimen pack captured them (docs/agent-specimens/antigravity/hooks/
    // payloads/PreToolUse.jsonl), trimmed to the fields the guard reads.
    #[test]
    fn antigravity_payloads_translate_to_the_policy_vocabulary() {
        let t = |name: &str, args: serde_json::Value| {
            antigravity_policy_payload(&serde_json::json!({
                "conversationId": "c", "stepIdx": 2, "workspacePaths": ["/p"],
                "toolCall": { "name": name, "args": args }
            }))
        };
        let write = t(
            "write_file",
            serde_json::json!({ "Content": "FIRST-VALUE", "Overwrite": true, "Path": "/p/a.txt" }),
        );
        assert_eq!(write["hook_event_name"], "PreToolUse");
        assert_eq!(write["tool_name"], "Write");
        assert_eq!(write["tool_input"]["file_path"], "/p/a.txt");
        assert_eq!(write["tool_input"]["content"], "FIRST-VALUE");
        assert_eq!(write["cwd"], "/p");
        let edit = t(
            "edit_file",
            serde_json::json!({ "Path": "/p/a.txt", "Target": "FIRST-VALUE", "Replacement": "SECOND-VALUE" }),
        );
        assert_eq!(edit["tool_name"], "Edit");
        assert_eq!(edit["tool_input"]["file_path"], "/p/a.txt");
        assert_eq!(edit["tool_input"]["old_string"], "FIRST-VALUE");
        assert_eq!(edit["tool_input"]["new_string"], "SECOND-VALUE");
        let relative = t(
            "write_file",
            serde_json::json!({ "Content": "x", "Path": "b.txt" }),
        );
        assert_eq!(
            relative["tool_input"]["file_path"],
            Path::new("/p").join("b.txt").to_string_lossy().as_ref()
        );
        let shell = t(
            "run_command",
            serde_json::json!({ "CommandLine": "echo RUN-COMMAND-7731", "WaitMsBeforeAsync": 5000, "toolSummary": "Echo" }),
        );
        assert_eq!(shell["tool_name"], "Bash");
        assert_eq!(shell["tool_input"]["command"], "echo RUN-COMMAND-7731");
        let mcp = t(
            "call_mcp_tool",
            serde_json::json!({
                "ServerName": "fsprobe_filesystem",
                "ToolName": "write_file",
                "Arguments": { "path": "/p/sub/mcp-out.txt", "content": "MCP-PROBE-7731" }
            }),
        );
        assert_eq!(mcp["tool_name"], "mcp__fsprobe_filesystem__write_file");
        assert_eq!(
            mcp["tool_input"],
            serde_json::json!({ "path": "/p/sub/mcp-out.txt", "content": "MCP-PROBE-7731" })
        );
        let read = t("read_file", serde_json::json!({ "Path": "/p/a.txt" }));
        assert_eq!(read["tool_name"], "read_file");
        assert_eq!(read["tool_input"]["Path"], "/p/a.txt");
        // No workspace: an empty cwd, never a guess.
        let bare = antigravity_policy_payload(&serde_json::json!({
            "toolCall": { "name": "write_file", "args": { "Path": "/p/a.txt", "Content": "x" } }
        }));
        assert_eq!(bare["cwd"], "");
        assert_eq!(bare["tool_input"]["file_path"], "/p/a.txt");
    }

    #[test]
    fn antigravity_decisions_follow_worklist_coverage() {
        let root = scratch("antigravity-coverage");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::write(root.join("resources/.worklist-authorization.json"), "{}").unwrap();
        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":1,"items":[{"id":"x","status":"proposed","files":["covered.txt"]}]}"#,
        )
        .unwrap();
        let at = |name: &str| root.join(name).to_string_lossy().into_owned();
        let payload = |name: &str, args: serde_json::Value| {
            antigravity_policy_payload(&serde_json::json!({
                "workspacePaths": [root.to_string_lossy()],
                "toolCall": { "name": name, "args": args }
            }))
        };
        let decide = |name: &str, args: serde_json::Value| {
            crate::guard_policy::shadow_worklist_decision("claude-rs", &payload(name, args))
                .unwrap()
                .decision
        };
        assert_eq!(
            decide(
                "write_file",
                serde_json::json!({ "Path": at("covered.txt"), "Content": "ok" })
            ),
            "allow"
        );
        assert_eq!(
            decide(
                "write_file",
                serde_json::json!({ "Path": at("other.txt"), "Content": "no" })
            ),
            "deny"
        );
        assert_eq!(
            decide(
                "edit_file",
                serde_json::json!({ "Path": at("other.txt"), "Target": "a", "Replacement": "b" })
            ),
            "deny"
        );
        // Through the deciding hook itself: agy's answer is a stdout JSON
        // line with exit 0 either way, and the breadcrumb names the provider.
        let code = authority_worklist_hook(
            "antigravity-rs",
            &payload(
                "write_file",
                serde_json::json!({ "Path": at("other.txt"), "Content": "no" }),
            ),
            std::time::Instant::now(),
        );
        assert_eq!(code, 0, "agy reads the deny from stdout, exit 0");
        let log = read_crumbs(&root);
        assert!(
            log.contains("antigravity-rs PreToolUse Write decided=deny"),
            "log: {log}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn antigravity_authority_fails_closed_on_unparseable_stdin() {
        let root = scratch("fault-antigravity-stdin");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let code = authority_fail_closed(
            &root,
            "antigravity-rs",
            "",
            GuardFault::UnparseableStdin("EOF while parsing".into()),
            std::time::Instant::now(),
        );
        assert_eq!(code, 0, "agy denies via its stdout JSON line, exit 0");
        let log = read_crumbs(&root);
        assert!(log.contains("antigravity-rs PreToolUse ?"), "log: {log}");
        assert!(log.contains("decided=error target=- ms="), "log: {log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // agy-guard-fail-closed-on-drift: each of these was answered with an
    // explicit allow on a project that denies the correct payload.
    #[test]
    fn antigravity_payload_drift_is_a_fault() {
        let path = |p: &str| serde_json::json!({ "Path": p, "Content": "x" });
        let drifted = [
            serde_json::json!({ "workspacePaths": ["/p"], "tool_call": { "name": "write_file", "args": path("/p/a") } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "tool": "write_file", "args": path("/p/a") } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "write_file", "args": { "FilePath": "/p/a" } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "edit_file", "args": { "Target": "a" } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "write_file", "args": path("  ") } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "write_file" } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "run_command", "args": { "Command": "echo x > a" } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "delete_file", "args": path("/p/a") } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "mcp_filesystem_write_file", "args": { "path": "/p/a", "content": "x" } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ToolName": "write_file", "Arguments": {} } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "  ", "ToolName": "write_file", "Arguments": {} } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "fs", "Arguments": {} } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "fs", "ToolName": "", "Arguments": {} } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "fs", "ToolName": "write_file" } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "fs", "ToolName": "write_file", "Input": { "path": "/p/a" } } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "fs", "ToolName": "write_file", "Arguments": "{\"path\":\"/p/a\"}" } } }),
            serde_json::json!({ "workspacePaths": ["/p"], "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "fs", "ToolName": "write_file", "Arguments": null } } }),
            serde_json::json!({ "toolCall": { "name": "write_file", "args": path("/p/a") } }),
            serde_json::json!({ "workspacePaths": [], "toolCall": { "name": "write_file", "args": path("/p/a") } }),
            serde_json::json!({ "workspacePaths": [""], "toolCall": { "name": "write_file", "args": path("/p/a") } }),
            serde_json::json!({}),
        ];
        for payload in &drifted {
            assert!(antigravity_payload_fault(payload).is_some(), "{payload}");
        }
        // The captured shapes still reach the policy.
        for call in [
            serde_json::json!({ "name": "write_file", "args": path("/p/a") }),
            serde_json::json!({ "name": "edit_file", "args": { "Path": "/p/a", "Target": "a", "Replacement": "b" } }),
            serde_json::json!({ "name": "run_command", "args": { "CommandLine": "git status" } }),
            serde_json::json!({ "name": "call_mcp_tool", "args": { "ServerName": "xmlui", "ToolName": "xmlui_status", "Arguments": {} } }),
            serde_json::json!({ "name": "call_mcp_tool", "args": { "ServerName": "fsprobe_filesystem", "ToolName": "write_file", "Arguments": { "path": "/p/sub/mcp-out.txt", "content": "MCP-PROBE-7731" } } }),
        ] {
            let payload = serde_json::json!({ "workspacePaths": ["/p"], "toolCall": call });
            assert_eq!(antigravity_payload_fault(&payload), None, "{payload}");
        }
    }

    // agy-guard-mcp-coverage: run the pinned PreToolUse.jsonl records through
    // the real hook so both the shape check and Claude's MCP coverage branch
    // are tested against captured payloads rather than hand-crafted copies.
    #[test]
    fn antigravity_pre_tool_use_specimens_parse_and_gate_mcp_writes() {
        let specimens =
            include_str!("../../docs/agent-specimens/antigravity/hooks/payloads/PreToolUse.jsonl");
        let root = scratch("antigravity-mcp-specimens");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("resources/.worklist-authorization.json"), "{}").unwrap();
        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":1,"items":[]}"#,
        )
        .unwrap();
        // antigravity-specimen-test-escapes-windows-path: `<workspace>` sits
        // inside JSON strings, and a Windows path carries backslashes, so
        // splice the JSON-escaped form (quotes trimmed) rather than the raw path.
        let ws_json = serde_json::to_string(&root.to_string_lossy().as_ref()).unwrap();
        let ws = ws_json.trim_matches('"');
        let records: Vec<serde_json::Value> = specimens
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| serde_json::from_str(&line.replace("<workspace>", ws)).unwrap())
            .collect();
        assert!(records.len() >= 13);
        for rec in &records {
            assert_eq!(antigravity_payload_fault(rec), None, "{rec}");
        }
        let mcp_read = records
            .iter()
            .find(|r| r["toolCall"]["args"]["ToolName"] == "xmlui_status")
            .expect("pinned xmlui_status specimen");
        let mcp_write = records
            .iter()
            .find(|r| {
                r["toolCall"]["name"] == "call_mcp_tool"
                    && r["toolCall"]["args"]["ToolName"] == "write_file"
            })
            .expect("pinned call_mcp_tool write_file specimen");

        antigravity_worklist_hook(mcp_read, None, std::time::Instant::now());
        let crumbs = read_crumbs(&root);
        assert!(
            crumbs.contains("PreToolUse mcp__xmlui__xmlui_status decided=allow target=- ms=")
                && crumbs.contains("reason=mcp-read-only"),
            "crumbs: {crumbs}"
        );

        antigravity_worklist_hook(mcp_write, None, std::time::Instant::now());
        let crumbs = read_crumbs(&root);
        assert!(
            crumbs.contains("PreToolUse mcp__fsprobe_filesystem__write_file decided=deny"),
            "uncovered MCP write must be denied, crumbs: {crumbs}"
        );

        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":2,"items":[{"id":"mcp","status":"proposed","files":["sub/mcp-out.txt"]}]}"#,
        )
        .unwrap();
        antigravity_worklist_hook(mcp_write, None, std::time::Instant::now());
        let crumbs = read_crumbs(&root);
        assert!(
            crumbs.contains("PreToolUse mcp__fsprobe_filesystem__write_file decided=allow"),
            "covered MCP write must be allowed, crumbs: {crumbs}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // Live agy 1.4.12 runs `.agents/hooks.json` hooks with process cwd set to
    // `<workspace>/.agents`. Run in an isolated child process so `current_dir`
    // starts in `.agents/` without racing other test threads: without
    // `enter_antigravity_workspace`, `content: "MCP-PROBE-7731"` resolves to
    // `.agents/MCP-PROBE-7731` and falsely blocks a covered MCP write.
    #[test]
    fn antigravity_mcp_hook_from_dot_agents_cwd_ignores_content_tokens() {
        const CHILD_ENV: &str = "BRAM_TEST_AGY_DOT_AGENTS_WS";
        if let Ok(ws) = std::env::var(CHILD_ENV) {
            let root = PathBuf::from(&ws);
            let payload = serde_json::json!({
                "conversationId": "c",
                "stepIdx": 6,
                "workspacePaths": [ws],
                "toolCall": {
                    "name": "call_mcp_tool",
                    "args": {
                        "ServerName": "fsprobe_filesystem",
                        "ToolName": "write_file",
                        "Arguments": {
                            "path": root.join("sub/mcp-out.txt").to_string_lossy(),
                            "content": "MCP-PROBE-7731"
                        }
                    }
                }
            });
            enter_antigravity_workspace(&payload);
            antigravity_worklist_hook(&payload, None, std::time::Instant::now());
            let crumbs = read_crumbs(&root);
            assert!(
                crumbs.contains("PreToolUse mcp__fsprobe_filesystem__write_file decided=allow"),
                "covered MCP write from .agents cwd must be allowed, crumbs: {crumbs}"
            );
            return;
        }
        // Resolved, because macOS's temp dir is a symlink: with the unresolved
        // root the child's cwd sits outside it, the candidate is dropped, and
        // this test passes with the workspace switch removed.
        let root = scratch("antigravity-dot-agents-cwd")
            .canonicalize()
            .unwrap();
        std::fs::create_dir_all(root.join(".agents")).unwrap();
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("resources/.worklist-authorization.json"), "{}").unwrap();
        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":1,"items":[{"id":"mcp","status":"proposed","files":["sub/mcp-out.txt"]}]}"#,
        )
        .unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .current_dir(root.join(".agents"))
            .env(CHILD_ENV, root.to_string_lossy().as_ref())
            .args([
                "guard::guard_mode_tests::antigravity_mcp_hook_from_dot_agents_cwd_ignores_content_tokens",
                "--exact",
                "--nocapture",
            ])
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            out.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Through the hook itself: drift is the guard breaking (decided=error),
    // never a decision, even in a project with nothing else to refuse it.
    #[test]
    fn antigravity_hook_fails_closed_on_drift_and_decides_the_captured_shape() {
        let root = scratch("antigravity-drift");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::write(root.join("resources/.worklist-authorization.json"), "{}").unwrap();
        std::fs::write(
            root.join("resources/worklist.json"),
            r#"{"version":1,"items":[]}"#,
        )
        .unwrap();
        let file = root.join("a.txt").to_string_lossy().into_owned();
        let workspace = root.to_string_lossy().into_owned();
        let drifted = serde_json::json!({
            "workspacePaths": [workspace],
            "toolCall": { "name": "write_file", "args": { "FilePath": file, "Content": "x" } }
        });
        let code = antigravity_worklist_hook(&drifted, None, std::time::Instant::now());
        assert_eq!(code, 0, "agy denies via its stdout JSON line, exit 0");
        let log = read_crumbs(&root);
        assert!(log.contains("decided=error"), "log: {log}");
        assert!(
            log.contains("unrecognized payload: write_file has no Path"),
            "log: {log}"
        );
        assert!(!log.contains("decided=allow"), "log: {log}");

        let captured = serde_json::json!({
            "workspacePaths": [workspace],
            "toolCall": { "name": "write_file", "args": { "Path": file, "Content": "x" } }
        });
        antigravity_worklist_hook(&captured, None, std::time::Instant::now());
        assert!(
            read_crumbs(&root).contains("PreToolUse Write decided=deny"),
            "the captured shape still reaches the policy"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // Per-project routing reads agy's raw payload, before translation.
    #[test]
    fn antigravity_root_comes_from_the_workspace() {
        let raw = serde_json::json!({
            "workspacePaths": ["/work/space"],
            "toolCall": { "name": "write_file", "args": {} }
        });
        assert_eq!(
            resolve_project_root("antigravity-rs", &raw),
            PathBuf::from("/work/space")
        );
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
        let code = authority_guarded_dispatch(
            "claude-rs",
            &payload,
            None,
            std::time::Instant::now(),
            boom,
        );
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
        assert!(
            read_crumbs(&root).contains("action=none ms="),
            "no action=none crumb"
        );

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

    // --- issue-469: the `.bram/` twins ----------------------------------------

    #[test]
    fn route_target_finds_the_project_guard_under_dot_bram() {
        let root = scratch("route-found-dot-bram");
        let nested = root.join("app").join("tools");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".bram")).unwrap();
        std::fs::write(root.join(".bram/.gitignore"), "*\n").unwrap();
        let project_guard = root.join("project-guard");
        std::fs::write(&project_guard, "").unwrap();
        let running_guard = root.join("running-guard");
        std::fs::write(&running_guard, "").unwrap();
        assert_eq!(
            guard_target_path(&root),
            root.join(".bram/.bram-guard-target")
        );
        std::fs::write(
            guard_target_path(&root),
            format!("{}\n", project_guard.display()),
        )
        .unwrap();
        let (target, found_root) = route_target(&nested, &running_guard).expect("routed");
        assert_eq!(
            target.canonicalize().unwrap(),
            project_guard.canonicalize().unwrap()
        );
        assert_eq!(found_root, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_mode_is_inert_without_either_state_dir() {
        // No `.bram/` and no `resources/` under root: nothing written, and
        // no `.bram/` created either.
        let root = scratch("inert-both");
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "cwd": root.to_string_lossy(),
        });
        authority_menu_hook("codex-rs", &payload, std::time::Instant::now());
        authority_menu_hook("claude-rs", &payload, std::time::Instant::now());
        assert_eq!(count_entries(&root), 0, "guard mode created files");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn breadcrumb_lands_under_dot_bram_when_it_exists() {
        let root = scratch("crumb-dot-bram");
        std::fs::create_dir_all(root.join(".bram")).unwrap();
        std::fs::write(root.join(".bram/.gitignore"), "*\n").unwrap();
        // A legacy folder beside it is not the state folder any more.
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "cwd": root.to_string_lossy(),
        });
        authority_menu_hook("codex-rs", &payload, std::time::Instant::now());
        let log = std::fs::read_to_string(root.join(".bram/bram-traces/hook-events.log"))
            .expect("breadcrumb written under .bram");
        assert!(
            log.contains("codex-rs PermissionRequest Bash"),
            "log: {log}"
        );
        assert!(log.contains("post=skipped port=none"), "log: {log}");
        assert!(!root.join("resources/bram-traces").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn port_is_read_from_either_state_dir() {
        let legacy = scratch("port-legacy");
        std::fs::create_dir_all(legacy.join("resources")).unwrap();
        std::fs::write(legacy.join("resources/.bram-port"), "4101\n").unwrap();
        assert_eq!(read_port(&legacy), Some(4101));
        assert_eq!(find_port_upward(&legacy), Some(4101));

        let dot = scratch("port-dot-bram");
        std::fs::create_dir_all(dot.join(".bram")).unwrap();
        std::fs::write(dot.join(".bram/.gitignore"), "*\n").unwrap();
        std::fs::write(dot.join(".bram/.bram-port"), "4102\n").unwrap();
        assert_eq!(read_port(&dot), Some(4102));
        let deep = dot.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_port_upward(&deep), Some(4102));
        let _ = std::fs::remove_dir_all(&legacy);
        let _ = std::fs::remove_dir_all(&dot);
    }

    #[test]
    fn walk_up_finds_a_nested_dot_bram_instance_before_its_parent() {
        // root/resources/.bram-port (parent, old name) and
        // root/sub/.bram/.bram-port (nested, new name): a run from
        // root/sub/x resolves to root/sub.
        let root = scratch("nested-dot-bram");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::write(root.join("resources/.bram-port"), "5001").unwrap();
        let sub = root.join("sub");
        std::fs::create_dir_all(sub.join(".bram")).unwrap();
        std::fs::write(sub.join(".bram/.gitignore"), "*\n").unwrap();
        std::fs::write(sub.join(".bram/.bram-port"), "5002").unwrap();
        let x = sub.join("x");
        std::fs::create_dir_all(&x).unwrap();
        let (_, found) = find_state_file_upward(&x, ".bram-port").expect("found");
        assert_eq!(found, sub);
        assert_eq!(find_port_upward(&x), Some(5002));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_up_reaches_the_parent_when_the_child_has_neither_name() {
        let root = scratch("parent-only");
        std::fs::create_dir_all(root.join("resources")).unwrap();
        std::fs::write(root.join("resources/.bram-port"), "5003").unwrap();
        let x = root.join("sub").join("x");
        std::fs::create_dir_all(&x).unwrap();
        let (_, found) = find_state_file_upward(&x, ".bram-port").expect("found");
        assert_eq!(found, root);
        assert_eq!(find_port_upward(&x), Some(5003));
        // The child, having no state folder, gets no breadcrumb either.
        append_breadcrumb(&root.join("sub"), "claude-rs", "PreToolUse", "Edit", "t");
        assert!(!root.join("sub/resources").exists());
        assert!(!root.join("sub/.bram").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
