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
    let started = std::time::Instant::now();
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let payload: serde_json::Value =
        serde_json::from_str(input.trim()).unwrap_or_else(|_| serde_json::json!({}));
    match hook {
        "claude-permission-menu" => shadow_menu_hook("claude-rs", &payload, started),
        "codex-permission-menu" => shadow_menu_hook("codex-rs", &payload, started),
        "claude-worklist" => shadow_worklist_hook("claude-rs", &payload, started),
        "codex-worklist" => shadow_worklist_hook("codex-rs", &payload, started),
        other => {
            // Misregistration surfaces in stderr, never in the exit code: a
            // nonzero exit from a PreToolUse-shaped hook can block the tool
            // call, and shadow mode must be incapable of affecting anything.
            eprintln!("bram guard: unknown hook '{}'", other);
        }
    }
    0
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
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
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
    install_guard_link(&home, &exe).ok()
}

pub fn guard_hook_command(link: &Path, hook: &str) -> String {
    format!("\"{}\" guard {}", link.display(), hook)
}

#[cfg(test)]
mod guard_mode_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bram-guard-test-{}-{}",
            name,
            std::process::id()
        ));
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
        assert_eq!(menu_would_action("codex-rs", "PermissionDenied", "Bash"), None);
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
        assert!(log.contains("codex-rs PermissionRequest Bash"), "log: {log}");
        assert!(
            log.contains("would-post=/__menu/permission port=none ms="),
            "log: {log}"
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
