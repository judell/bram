use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;

use include_dir::{include_dir, Dir};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{http, ipc::Channel, AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

// The `app/` tree is embedded in the binary at compile time so
// release artifacts ship as a single self-contained file. We
// deliberately do *not* reuse Tauri's asset_resolver (which also
// embeds `app/` via frontendDist) because that resolver
// SPA-fallbacks unknown paths to index.html — disastrous for
// XMLUI's optional code-behind probes that legitimately 404. The
// duplication costs ~6MB; the reliability is worth it.
static EMBEDDED_APP: Dir = include_dir!("$CARGO_MANIFEST_DIR/../app");

// Resolve a path within Bram's own `app/` bundle to (bytes, mime). When
// an on-disk Bram app/ exists, that is ground truth — a missing file is
// genuinely missing. Deliberately do NOT fall back to project-relative
// `cwd/app`: user projects are allowed to have their own app/ folder, and
// letting that shadow Bram's shell assets breaks routing/watchers (#58).
// Only fall back to the embedded tree when there is no on-disk Bram app/.
fn serve_app_file<R: tauri::Runtime>(
    app: Option<&AppHandle<R>>,
    rel: &str,
) -> Option<(Vec<u8>, &'static str)> {
    if let Some(root) = resolve_app_root(app) {
        let p = root.join(rel);
        return std::fs::read(&p).ok().map(|bytes| (bytes, mime_for(&p)));
    }
    EMBEDDED_APP.get_file(rel).map(|file| {
        (
            file.contents().to_vec(),
            mime_for(std::path::Path::new(rel)),
        )
    })
}

// Resolve a path within `app/` to a real on-disk path. If the
// on-disk app_root is present, returns app_root/rel directly. Else
// extracts the embedded file into a per-binary cache dir and returns
// that path. Used for things that need a real filesystem path —
// bash --rcfile, etc. — not just bytes.
fn extract_app_file<R: tauri::Runtime>(app: &AppHandle<R>, rel: &str) -> Result<PathBuf, String> {
    if let Some(root) = resolve_app_root(Some(app)) {
        let p = root.join(rel);
        return if p.exists() {
            Ok(p)
        } else {
            Err(format!("on-disk app file not found: {}", p.display()))
        };
    }
    let file = EMBEDDED_APP
        .get_file(rel)
        .ok_or_else(|| format!("embedded app file not found: {}", rel))?;
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache dir: {}", e))?
        .join("app");
    let target = cache_root.join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, file.contents()).map_err(|e| e.to_string())?;
    Ok(target)
}

struct PtyState {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

#[derive(Default)]
struct AppState(Mutex<Option<PtyState>>);

// Lifecycle owner for an optional whisper-server child. Spawn via the
// whisper_start command; killed by whisper_stop or on app exit.
#[derive(Default)]
struct WhisperState(Mutex<Option<std::process::Child>>);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorklistAuthorizationRecord {
    // "approved" | "drop" | "rejected_stale" | "none"
    kind: String,
    #[serde(default)]
    ids: Vec<String>,
    // Full verified item objects, populated when the approve/drop payload
    // arrived with per-item content hashes that matched the on-disk file.
    // Empty for legacy payloads (no hash supplied) and for rejected_stale.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<serde_json::Value>,
    // Ids whose supplied hash did not match the current `worklist.json`.
    // Non-empty implies kind == "rejected_stale".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mismatched_ids: Vec<String>,
    issued_at_ms: i64,
    source: String,
    #[serde(default)]
    consumed_at_ms: Option<i64>,
}

// Cross-platform home directory: $HOME on Unix, %USERPROFILE% on Windows.
// Returned as PathBuf so callers can .join() directly without re-parsing.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return format!("{}/{}", home.display(), rest);
        }
    }
    p.to_string()
}

// Active project root — resolved once at startup from a CLI arg
// (xmlui-desktop /path/to/project) or std::env::current_dir(). Read by
// the HTTP server, watcher, git/sessions/PTY commands.
struct ActiveProjectState(Mutex<PathBuf>);

// URLs for the two iframes.
//
// `tools` is always the internal loopback (xmlui-desktop's own server,
// serving /__tools/index.html, /__shell/*, embedded assets, git/issues
// endpoints, etc.).
//
// `right_pane` is now always `tauri://localhost/__project/index.html`
// regardless of project configuration. The tauri:// scheme handler in
// `handle_tauri_scheme` intercepts paths under `/__project/*` and
// proxies them to `right_pane_upstream` (loopback default, or external
// dev server when `.xmlui-desktop.json` declares one). Net effect: the
// right-pane iframe is same-origin with the shell, while the actual
// bytes still come from the upstream.
//
// `right_pane_upstream` is the proxy target (always ends with `/`).
//
// Service workers (used by MSW and xmlui's apiInterceptor) require a
// secure-context origin. Custom URI schemes are not secure contexts on
// macOS, so SW capability is lost in the right-pane iframe on macOS.
// Acceptable for normal apps; playground apps that synthesize APIs
// will not work in xd on macOS — see worklist item
// `same-origin-iframe-via-tauri-scheme-proxy` for the full discussion.
struct PaneUrlsState(Mutex<PaneUrls>);

#[derive(Default, Clone)]
struct PaneUrls {
    right_pane: String,
    tools: String,
    // Loopback-served URL used when no project server is declared. Used by
    // the agent-tools drawer's right-pane-info display (so the user sees
    // the actual upstream URL, not the tauri:// proxy URL) and as the
    // fallback upstream after the server block is removed from
    // .xmlui-desktop.json at runtime.
    default_right_pane: String,
    // Base URL the tauri:// scheme handler proxies right-pane requests to.
    // Always ends with `/`. Switches between the loopback default and an
    // external server based on .xmlui-desktop.json at startup and on
    // config reload.
    right_pane_upstream: String,
    // Always the internal-loopback base URL (ends with `/`), regardless of
    // any external dev-server declared in .xmlui-desktop.json. Used by the
    // scheme handler to route xd-internal `/__*` requests (sessions,
    // worklist, app-info, etc.) — these never live on the project's dev
    // server even when one is declared.
    loopback_origin: String,
}

// Project-level config read from .xmlui-desktop.json at the project
// root. Distinct from XMLUI's own config.json (the app-under-test
// isn't necessarily an XMLUI app). All fields optional.
#[derive(Default, Clone, serde::Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    server: Option<ServerConfig>,
    #[serde(default)]
    shell: Option<ShellConfig>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct ServerConfig {
    command: String,
    #[serde(default)]
    cwd: Option<String>,
    port: u16,
    #[serde(default = "default_server_path")]
    path: String,
}

// Optional shell-startup block. `agent` is a single command string
// typed verbatim into the bash prompt right after pty_spawn — bash
// parses it, so flags work: `claude --continue`, `codex resume`, etc.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct ShellConfig {
    #[serde(default)]
    agent: Option<String>,
}

fn default_server_path() -> String {
    "/".to_string()
}

// Lifecycle owner for an optional project-server child spawned per
// .xmlui-desktop.json. Killed on ExitRequested, or on hot-reload when the
// declared command/cwd/port changes. Carries the spawn-time config so the
// reload path can diff against the new file and decide whether to respawn.
struct SpawnedServer {
    child: std::process::Child,
    config: ServerConfig,
}

#[derive(Default)]
struct SpawnedServerState(Mutex<Option<SpawnedServer>>);

// Windows' canonicalize() returns `\\?\C:\…` extended-length paths.
// PowerShell tolerates them, but cmd.exe child processes don't ("UNC paths
// are not supported. Defaulting to Windows directory.") — and silent
// fallback to %WINDIR% means any tool that resolves its workspace from
// cwd ends up rooted in C:\Windows. Strip the prefix unless this is a
// genuine UNC path (`\\?\UNC\server\share\…`), which must keep it.
fn strip_unc_prefix(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if !rest.starts_with("UNC\\") {
            return PathBuf::from(rest);
        }
    }
    p
}

// Flatten a filesystem path into a filename-safe identifier. On Linux/macOS
// this matches Claude Code's `~/.claude/projects/<encoded>/` scheme
// (`/Users/foo` → `-Users-foo`). On Windows we also fold `\` and `:` so
// `C:\Users\foo` becomes `C--Users-foo`; this is a conservative encoding
// for our own files (agent-hint), but for claude_sessions_dir it's a
// best-effort guess at Claude Code's Windows scheme — adjust if confirmed.
fn encode_path_for_filename(p: &std::path::Path) -> String {
    p.to_string_lossy()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c => c,
        })
        .collect()
}

fn determine_project_root() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let candidate: PathBuf = if args.len() >= 2 && !args[1].starts_with('-') {
        PathBuf::from(&args[1])
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    let canonical = candidate.canonicalize().unwrap_or(candidate);
    strip_unc_prefix(canonical)
}

fn parse_cli_flags() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return;
    }
    match args[1].as_str() {
        "-h" | "--help" => {
            println!(
                "Usage: bram [PROJECT_DIR]\n\n\
                 Tauri shell that pairs a terminal with an XMLUI surface.\n\n\
                 Arguments:\n  \
                   [PROJECT_DIR]    Path to the XMLUI project to load (defaults to current directory)\n\n\
                 Options:\n  \
                   -h, --help       Print this help and exit\n  \
                   -V, --version    Print version and exit"
            );
            std::process::exit(0);
        }
        "-V" | "--version" => {
            println!("bram {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        s if s.starts_with('-') => {
            eprintln!("bram: unknown option '{}'", s);
            eprintln!("Try 'bram --help' for more information.");
            std::process::exit(1);
        }
        _ => {}
    }
}

fn first_nonempty_env(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

// --- Comms-path trace foundation -----------------------------------------
//
// Process-global toggle for the comms-path trace log
// (`resources/bram-trace.log`, with prior runs archived at startup as
// `resources/bram-trace-YYYY-MM-DD*.log`). Set once at startup by
// `init_bram_trace_from_env`; every potential waypoint checks
// `bram_trace_enabled()` (a single atomic load) before doing any work,
// so the cost when off is essentially zero. Spec:
// https://github.com/judell/bram/issues/49#issuecomment-4524234976
//
// This commit installs the foundation only. NO call sites are wired up
// yet — `append_bram_trace_line` is intentionally unused. Subsequent
// commits add one trace category at a time, per the spec's verification
// discipline.
static BRAM_TRACE_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

// Defer tools-pane-reload while a cycle is active (refs #93).
// Set when the watcher would otherwise emit during sentinel-active.
// Cleared and flushed once the sentinel is cleared. Single boolean
// so N watcher events during one cycle coalesce into one post-cycle
// reload — intended behavior.
static PENDING_TOOLS_RELOAD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

// Cached open handle for the live trace file. Lazy-init on first
// write: truncate-open, emit the session-start line, store the handle.
// Subsequent writes reuse the handle, dropping the per-event cost from
// open + write + close (3 syscalls) to write (1 syscall). Refs #82
// trace-cache-file-handle. Replaces the previous BRAM_TRACE_FIRST_WRITE
// flag — `guard.is_none()` IS the "first write" check now.
static BRAM_TRACE_FILE: std::sync::OnceLock<Mutex<Option<std::fs::File>>> =
    std::sync::OnceLock::new();

fn bram_trace_file_cell() -> &'static Mutex<Option<std::fs::File>> {
    BRAM_TRACE_FILE.get_or_init(|| Mutex::new(None))
}

fn bram_trace_enabled() -> bool {
    BRAM_TRACE_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

fn init_bram_trace_from_env() {
    let on = std::env::var("BRAM_TRACE")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false);
    BRAM_TRACE_ENABLED.store(on, std::sync::atomic::Ordering::Relaxed);
    if on {
        eprintln!(
            "[bram-trace] enabled (BRAM_TRACE=1); live trace destination: <project_root>/resources/bram-trace.log; previous runs archived at startup as <project_root>/resources/bram-trace-YYYY-MM-DD*.log"
        );
    }
}

#[allow(dead_code)]
fn bram_trace_log_file<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    project_root(Some(app)).map(|p| p.join("resources/bram-trace.log"))
}

fn is_bram_trace_archive_rel(rel: &str) -> bool {
    rel.starts_with("resources/bram-trace-") && rel.ends_with(".log")
}

fn bram_trace_date_stamp_local() -> String {
    #[cfg(unix)]
    {
        if let Ok(out) = std::process::Command::new("date").arg("+%Y-%m-%d").output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "(Get-Date).ToString('yyyy-MM-dd')"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
    }
    let secs = unix_now_ms() / 1000;
    let days = secs.div_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, mo, d)
}

fn next_bram_trace_archive_path(active_path: &Path) -> Option<PathBuf> {
    let parent = active_path.parent()?;
    let stamp = bram_trace_date_stamp_local();
    for n in 1.. {
        let name = if n == 1 {
            format!("bram-trace-{}.log", stamp)
        } else {
            format!("bram-trace-{}-{}.log", stamp, n)
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn prepare_bram_trace_log<R: tauri::Runtime>(app: &AppHandle<R>) {
    if !bram_trace_enabled() {
        return;
    }
    let Some(active_path) = bram_trace_log_file(app) else {
        return;
    };
    if let Some(parent) = active_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "[bram-trace] failed to create trace directory {}: {}",
                parent.display(),
                e
            );
            return;
        }
    }

    let had_prior_log = std::fs::metadata(&active_path)
        .map(|m| m.len() > 0)
        .unwrap_or(false);
    if had_prior_log {
        let Some(archive_path) = next_bram_trace_archive_path(&active_path) else {
            eprintln!("[bram-trace] failed to choose archive path for previous live log");
            return;
        };
        let archived = std::fs::rename(&active_path, &archive_path).or_else(|rename_err| {
            std::fs::copy(&active_path, &archive_path)
                .map(|_| ())
                .map_err(|copy_err| {
                    std::io::Error::other(format!(
                        "rename failed: {}; copy failed: {}",
                        rename_err, copy_err
                    ))
                })
        });
        match archived {
            Ok(()) => {
                eprintln!(
                    "[bram-trace] archived previous live log to {}",
                    archive_path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "[bram-trace] failed to archive previous live log {}: {}",
                    active_path.display(),
                    e
                );
                return;
            }
        }
    }

    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&active_path)
    {
        eprintln!(
            "[bram-trace] failed to create fresh live log {}: {}",
            active_path.display(),
            e
        );
    }
}

// Single write API for every trace category. Format:
//   [<ISO-8601-UTC>] [<category>] <body>
//
// No-op when the toggle is off. Truncates the file on first call per
// session and writes a `[bram-trace] event=session-start pid=<pid>`
// header before the first real record.
//
// `category` is one of the closed-vocabulary tokens from the spec
// (`bram-trace`, `pty-out`, `pty-in`, `pty-menu`, `auth-record`,
// `watcher`, `emit`, `route`, `hook`, `iframe`). The function does not
// validate — the caller is expected to use a token from the spec, and
// `category` is checked in code review, not at runtime.
fn append_bram_trace_line<R: tauri::Runtime>(app: &AppHandle<R>, category: &str, body: &str) {
    if !bram_trace_enabled() {
        return;
    }
    let Some(path) = bram_trace_log_file(app) else {
        return;
    };
    let Ok(mut guard) = bram_trace_file_cell().lock() else {
        return;
    };
    // First write of the session: ensure the parent dir exists, open
    // with truncate, write the session-start line, then cache the
    // handle. Every subsequent write reuses the cached handle and
    // pays only a single `write(2)` per line — measurably keeps the
    // PTY read loop responsive under heavy TUI animation. Refs #82.
    if guard.is_none() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let opened = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path);
        let Ok(mut file) = opened else {
            return;
        };
        let _ = writeln!(
            file,
            "[{}] [bram-trace] event=session-start pid={}",
            format_iso_utc_ms(unix_now_ms()),
            std::process::id()
        );
        *guard = Some(file);
    }
    if let Some(file) = guard.as_mut() {
        let _ = writeln!(
            file,
            "[{}] [{}] {}",
            format_iso_utc_ms(unix_now_ms()),
            category,
            body
        );
    }
}

// Render a short, single-line, escape-aware preview of `data` capped at
// `max` chars. Shared by every trace category that surfaces raw bytes
// (currently `[pty-out]`; future: `[pty-in]`, `[route]` request body).
// Control characters render as `\xNN`, common whitespace gets the usual
// escapes, and the output is double-quoted so a trailing space or
// trailing escape doesn't get visually merged with adjacent text.
fn bram_trace_preview(data: &str, max: usize) -> String {
    let mut out = String::with_capacity(max + 8);
    out.push('"');
    let mut count = 0usize;
    for ch in data.chars() {
        if count >= max {
            out.push_str("...");
            break;
        }
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x1b' => out.push_str("\\x1b"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
        count += 1;
    }
    out.push('"');
    out
}

// True when the normalized turn submission begins with one of the
// structured-intent prefixes recognized by `parse_worklist_authorization_message`
// (`approved:`, `drop:`) or by chat-side convention (`iterate:`, `talk:`).
// Used by `[pty-out]` to flag candidate authorization writes; the
// authoritative parse still happens in `record_worklist_authorization_from_input`.
fn is_structured_intent_prefix(data: &str) -> bool {
    let n = normalize_turn_submission(data);
    let s = n.as_str();
    s.starts_with("approved:")
        || s.starts_with("drop:")
        || s.starts_with("iterate:")
        || s.starts_with("talk:")
}

// Trace a host → iframe Tauri emit with no payload. Spec fields:
// kind=<event name>, payload_size=0, correlation_id=<empty for now>.
// Place a call immediately above each unit-payload `emit` site.
fn trace_emit_signal<R: tauri::Runtime>(app: &AppHandle<R>, kind: &str) {
    if !bram_trace_enabled() {
        return;
    }
    append_bram_trace_line(
        app,
        "emit",
        &format!("kind={} payload_size=0 correlation_id=", kind),
    );
}

// Trace a host → iframe Tauri emit with a payload. Serializes the
// payload to JSON to count bytes; matches what Tauri itself does on the
// wire, modulo MessagePack vs JSON depending on Tauri version (the
// byte count here is the JSON form, used as a proxy for payload scale).
fn trace_emit_payload<R: tauri::Runtime, S: serde::Serialize>(
    app: &AppHandle<R>,
    kind: &str,
    payload: &S,
) {
    if !bram_trace_enabled() {
        return;
    }
    let size = serde_json::to_vec(payload).map(|v| v.len()).unwrap_or(0);
    append_bram_trace_line(
        app,
        "emit",
        &format!("kind={} payload_size={} correlation_id=", kind, size),
    );
}

// Process-local sequence number for [route] correlation ids. Combined
// with the entry timestamp it disambiguates two concurrent requests
// that arrive in the same millisecond.
static ROUTE_TRACE_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn next_route_correlation_id() -> String {
    let n = ROUTE_TRACE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("route-{}-{}", unix_now_ms(), n)
}

// Trace an HTTP route entry. Generated by handle_http for every
// inbound request before any work happens, so the entry timestamp is
// the receive moment (not the post-dispatch moment).
fn trace_route_entry<R: tauri::Runtime>(
    app: &AppHandle<R>,
    method: &str,
    path: &str,
    query: &str,
    correlation_id: &str,
) {
    if !bram_trace_enabled() {
        return;
    }
    append_bram_trace_line(
        app,
        "route",
        &format!(
            "phase=entry method={} path={} query={} correlation_id={}",
            method, path, query, correlation_id
        ),
    );
}

// Map a notify::EventKind into a short label for the [watcher] trace.
// Keep the vocabulary small and stable so analysis scripts can rely on
// it (`create`, `modify`, `remove`, `access`, `other`).
fn notify_event_kind_label(kind: &notify::EventKind) -> &'static str {
    use notify::EventKind;
    match kind {
        EventKind::Create(_) => "create",
        EventKind::Modify(_) => "modify",
        EventKind::Remove(_) => "remove",
        EventKind::Access(_) => "access",
        _ => "other",
    }
}

// Trace an HTTP route exit. Logged just before the response is written
// back to the socket so `duration_ms` is the full host-side handling
// time. `body_size` is the response body's byte length, not the wire
// length (which would include HTTP headers).
fn trace_route_exit<R: tauri::Runtime>(
    app: &AppHandle<R>,
    method: &str,
    path: &str,
    correlation_id: &str,
    status: u16,
    body_size: usize,
    duration_ms: u128,
) {
    if !bram_trace_enabled() {
        return;
    }
    append_bram_trace_line(
        app,
        "route",
        &format!(
            "phase=exit method={} path={} status={} body_size={} duration_ms={} correlation_id={}",
            method, path, status, body_size, duration_ms, correlation_id
        ),
    );
}

// --- End comms-path trace foundation -------------------------------------

fn project_root<R: tauri::Runtime>(app: Option<&AppHandle<R>>) -> Option<PathBuf> {
    if let Some(a) = app {
        if let Some(state) = a.try_state::<ActiveProjectState>() {
            if let Ok(p) = state.0.lock() {
                if !p.as_os_str().is_empty() {
                    return Some(p.clone());
                }
            }
        }
    }
    // Fallback for code paths that run before .manage() (rare; mostly
    // setup-time helpers). Old behavior.
    resolve_app_root(app).and_then(|app_root| app_root.parent().map(|p| p.to_path_buf()))
}

fn git_run<R: tauri::Runtime>(app: &AppHandle<R>, args: &[&str]) -> Result<String, String> {
    let root = project_root(Some(app)).ok_or_else(|| "no project root".to_string())?;
    let out = std::process::Command::new("git")
        .current_dir(&root)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// Update-availability check. Fetched once from GitHub at first request and
// cached for the process lifetime — repeat hits are cheap and we don't
// want to thrash the API's 60/hr anonymous limit. `XMLUI_DESKTOP_FAKE_CURRENT`
// substitutes the env value for CARGO_PKG_VERSION so the banner UI can be
// exercised ahead of an actual release: `XMLUI_DESKTOP_FAKE_CURRENT=0.0.1 cargo run`.
#[derive(serde::Serialize, Clone)]
struct AppInfo {
    current: String,
    latest: Option<String>,
    has_update: bool,
    release_url: Option<String>,
}

static APP_INFO_CACHE: OnceLock<Mutex<Option<AppInfo>>> = OnceLock::new();

// Cache of the "currently live" Claude session JSONL path and its mtime.
// Updated by latest_claude_session_path with hysteresis so the choice
// doesn't oscillate when claude touches an old session file.
static LIVE_CLAUDE_SESSION: OnceLock<Mutex<Option<(PathBuf, std::time::SystemTime)>>> =
    OnceLock::new();
static LIVE_CODEX_SESSION: OnceLock<Mutex<Option<(PathBuf, std::time::SystemTime)>>> =
    OnceLock::new();

// Real-time detection of claude's permission menu by tapping the PTY
// byte stream. Necessary because the JSONL file lags claude's actual
// state by ~10-20s (claude buffers a turn's records and flushes as one
// batch), so /__sessions/latest-pending can never see a tool_use until
// after the user has already responded. The PTY is the only signal
// available *while* the menu is on screen.
//
// PTY_TAIL holds a rolling ~8KB of the most recent PTY output bytes.
// PTY_MENU holds the currently-displayed menu (or None). Updated from
// the pty_spawn reader thread; cleared on pty_write (any user input
// dismisses the menu).
// PTY_MENU_SUPPRESSED records (tool, when_cleared) for ~2s after a
// user-driven dismissal. Without it, the next PTY chunk arrives, the
// detector re-scans the rolling buffer, and the *just-dismissed* menu
// text is still in the buffer — so the detector re-fires and the agent
// pane shows the menu briefly after click. Suppression breaks that
// loop until either time elapses or a *different* tool's menu appears.
static PTY_TAIL: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();
static PTY_MENU: OnceLock<Mutex<Option<PtyMenu>>> = OnceLock::new();
static PTY_MENU_SUPPRESSED: OnceLock<Mutex<Option<(String, std::time::Instant)>>> = OnceLock::new();
// Grace window for the "buffer briefly evicted a still-live menu" case.
// When `pty_menu_detect` returns None but `PTY_MENU` is currently Some,
// we defer the dismiss emit for MENU_EVICTION_GRACE_MS and re-check on
// the next pty_menu_update cycle. Re-detection within the window
// suppresses both the dismiss and the re-show; grace expiry without
// re-detection emits the dismiss normally. Refs #77 menu-detector
// stabilization.
static PTY_MENU_EVICTION_GRACE: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
const MENU_EVICTION_GRACE_MS: u128 = 350;

// The loopback HTTP server's port, captured at setup. Used to inject
// XMLUI_DESKTOP_PORT into the PTY child's environment so the agent can
// reach /__worklist/resolve and other /__* routes without rediscovering
// the random port each turn.
static LOOPBACK_PORT: OnceLock<u16> = OnceLock::new();

#[derive(serde::Serialize, Clone)]
struct PtyMenu {
    tool: String,
    text: String,
}

// PtyMenu equality compares only `tool` — `text` carries surrounding
// PTY bytes captured by position (`pos1 - 200`..`pos2 + 200`), which
// shifts as the rolling 8 KB tail evolves even when the user-visible
// menu is unchanged. Comparing text would defeat dedup-on-emit and
// produce bursty `state=shown` re-emits for the same on-screen prompt.
// Refs #77 tighten-pty-menu-emit-cadence.
impl PartialEq for PtyMenu {
    fn eq(&self, other: &Self) -> bool {
        self.tool == other.tool
    }
}
impl Eq for PtyMenu {}

// Sentinel for "menu detected via `❯1.` cursor bytes but the
// `Do you want to use X?` header text has not yet landed in the
// rolling buffer". First detection cycle stores this state but does
// NOT emit `state=shown`; the second cycle either resolves a real
// tool name from the now-buffered header or falls back to the
// pre-menu grep. Race observed when a user-input dismissal arrives
// between the cursor's PTY chunk and the header's PTY chunk, which
// previously caused `tool=Bash` to leak from a prior prompt onto a
// Read prompt's shown emit. Refs #77 tighten-pty-menu-emit-cadence.
const PENDING_TOOL: &str = "<pending>";

fn pty_tail_cell() -> &'static Mutex<Vec<u8>> {
    PTY_TAIL.get_or_init(|| Mutex::new(Vec::with_capacity(8192)))
}

// PTY agent-turn state machine (issue #70, later extended for Codex
// parity in #74). Detects end-of-turn from spinner/activity glyphs in
// the PTY stream: while the agent is running, the terminal redraws
// every 100-200ms with a spinner-like glyph. When that activity stops,
// the next non-spinner PTY chunk (typically the prompt redraw) fires
// `agent-turn-end`. The iframe consumes that event for a fast inflight
// clear path, bypassing the multi-second JSONL flush chain.
struct AgentTurnState {
    last_spinner_at: Option<std::time::Instant>,
    is_active: bool,
    last_emit_at: Option<std::time::Instant>,
    // Set when is_active transitions false → true; cleared on the
    // reverse transition. Lets pty_agent_turn_update emit
    // `[spinner-period] state=ended duration_ms=<n>` carrying the
    // active-period length, so #78 analysis can see whether premature
    // turn-end fires correlate with short active periods. Refs #78.
    active_since: Option<std::time::Instant>,
}

fn agent_turn_state_cell() -> &'static Mutex<AgentTurnState> {
    static CELL: OnceLock<Mutex<AgentTurnState>> = OnceLock::new();
    CELL.get_or_init(|| {
        Mutex::new(AgentTurnState {
            last_spinner_at: None,
            is_active: false,
            last_emit_at: None,
            active_since: None,
        })
    })
}

// 800ms threshold: activity ticks are 100-200ms apart while thinking;
// 800ms of no spinner/activity reliably indicates the agent stopped
// updating.
const AGENT_TURN_IDLE_THRESHOLD_MS: u128 = 800;

// Sentinel-clear gate (#91 follow-up). The emit threshold above
// (~800ms) misfires on natural inter-burst pauses; a real
// end-of-turn typically shows silence >= 3s. clear_active_sentinel
// fires only when the silence-detected turn-end exceeds this gate,
// avoiding premature spinner clears mid-burst.
const MIN_SILENCE_FOR_SENTINEL_CLEAR_MS: u128 = 3000;

// Suppress repeat emits within 5s of the last one. After a real
// end-of-turn the TUIs sometimes re-pulse briefly (input-box re-render,
// scroll update, title refresh, etc.) — that re-arms the detector and
// would fire a second turn-end ~1s later. The cooldown keeps the trace
// clean and the iframe listener from doing extra no-op work. State
// transitions still happen (is_active flips); only the outbound emit is
// gated.
const AGENT_TURN_EMIT_COOLDOWN_MS: u128 = 5000;

// Byte-level check for turn-activity glyphs without allocating a
// String. Asterisk family is U+2700..U+277F (UTF-8 prefix 0xE2 0x9C);
// braille patterns are U+2800..U+283F (prefix 0xE2 0xA0). In practice
// this covers both Claude's spinner redraws and Codex's braille/title
// activity updates. Middle dot U+00B7 is deliberately NOT matched — it
// appears in non-spinner TUI text (for example token-count separators)
// and would over-fire the detector.
fn pty_chunk_has_turn_activity_glyph(chunk: &[u8]) -> bool {
    for w in chunk.windows(2) {
        if w[0] == 0xE2 && (w[1] == 0x9C || w[1] == 0xA0) {
            return true;
        }
    }
    false
}

fn pty_agent_turn_update<R: tauri::Runtime>(app: &AppHandle<R>, chunk: &[u8]) {
    let now = std::time::Instant::now();
    let has_spinner = pty_chunk_has_turn_activity_glyph(chunk);
    let mut emit_now = false;
    let mut spinner_started = false;
    let mut spinner_ended_duration_ms: Option<u128> = None;
    let mut turn_end_silence_ms: Option<u128> = None;
    if let Ok(mut state) = agent_turn_state_cell().lock() {
        if has_spinner {
            if !state.is_active {
                // false -> true transition: start of a spinner-active
                // period. Refs #78 detector instrumentation.
                spinner_started = true;
                state.active_since = Some(now);
            }
            state.last_spinner_at = Some(now);
            state.is_active = true;
        } else if state.is_active {
            if let Some(last) = state.last_spinner_at {
                let silence = now.saturating_duration_since(last).as_millis();
                if silence > AGENT_TURN_IDLE_THRESHOLD_MS {
                    // true -> false transition: spinner-active period
                    // ends. Capture duration_ms (active_since → now)
                    // and silence_ms (last spinner glyph → now) for
                    // the [spinner-period] and [turn-end] traces.
                    // Refs #78 detector instrumentation.
                    if let Some(started) = state.active_since {
                        spinner_ended_duration_ms =
                            Some(now.saturating_duration_since(started).as_millis());
                    }
                    state.active_since = None;
                    state.is_active = false;
                    let in_cooldown = state.last_emit_at.map_or(false, |t| {
                        now.saturating_duration_since(t).as_millis()
                            < AGENT_TURN_EMIT_COOLDOWN_MS
                    });
                    if !in_cooldown {
                        state.last_emit_at = Some(now);
                        emit_now = true;
                        turn_end_silence_ms = Some(silence);
                    }
                }
            }
        }
    }
    if bram_trace_enabled() {
        if spinner_started {
            append_bram_trace_line(app, "spinner-period", "state=started");
        }
        if let Some(d) = spinner_ended_duration_ms {
            append_bram_trace_line(
                app,
                "spinner-period",
                &format!("state=ended duration_ms={}", d),
            );
        }
    }
    if emit_now {
        if bram_trace_enabled() {
            // Include the silence gap that triggered the fire. A
            // premature fire (#78) typically shows silence_ms close to
            // AGENT_TURN_IDLE_THRESHOLD_MS (~800-1000); a real
            // end-of-turn fire shows much longer silence (5s+).
            let silence_str = turn_end_silence_ms
                .map(|s| format!(" silence_ms={}", s))
                .unwrap_or_default();
            append_bram_trace_line(
                app,
                "turn-end",
                &format!("source=pty-turn-activity-stop{}", silence_str),
            );
        }
        trace_emit_signal(app, "agent-turn-end");
        let _ = app.emit("agent-turn-end", ());

        // Host-side guarantee: clear any active inflight sentinel when
        // the agent's turn truly ends. Gated on a higher silence
        // threshold than the emit threshold — premature fires
        // (silence ~800-1000ms during inter-burst pauses) would clear
        // the spinner mid-turn; real turn-ends show silence_ms
        // >= 3000ms (typically much more after the emit cooldown).
        // Refs #91 follow-up. Agents have trouble making
        // /__worklist/end or /__iterate/end their literal LAST action
        // — they tend to acknowledge tool results with one more
        // sentence. Tying the sentinel-clear to silence-detected
        // turn-end removes that risk class. The explicit end routes
        // still work for agents that want to clear early.
        if turn_end_silence_ms.map_or(false, |s| s >= MIN_SILENCE_FOR_SENTINEL_CLEAR_MS) {
            clear_active_sentinel(app);
        }
    }
}

fn pty_menu_cell() -> &'static Mutex<Option<PtyMenu>> {
    PTY_MENU.get_or_init(|| Mutex::new(None))
}

fn pty_menu_suppressed_cell() -> &'static Mutex<Option<(String, std::time::Instant)>> {
    PTY_MENU_SUPPRESSED.get_or_init(|| Mutex::new(None))
}

fn pty_menu_eviction_grace_cell() -> &'static Mutex<Option<std::time::Instant>> {
    PTY_MENU_EVICTION_GRACE.get_or_init(|| Mutex::new(None))
}

// Update the menu detection state with a fresh chunk of PTY output.
// Maintains a rolling 8KB tail buffer; checks for claude's menu signature
// ("1. Yes" + "2. Yes" within proximity); transitions PTY_MENU accordingly.
// Logs every state transition to stderr so failures-to-render can be
// correlated against actual detector activity.
fn pty_menu_update<R: tauri::Runtime>(app: &AppHandle<R>, chunk: &[u8]) {
    let tail_cell = pty_tail_cell();
    let mut tail = match tail_cell.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    tail.extend_from_slice(chunk);
    if tail.len() > 8192 {
        let drop = tail.len() - 8192;
        tail.drain(..drop);
    }
    let mut detected = pty_menu_detect(&tail);
    drop(tail);

    // Post-click suppression: if the user just dismissed a menu for
    // tool X, ignore detections of tool X for ~2s. The just-dismissed
    // menu's text is still sitting in PTY_TAIL.
    if let Some(ref new_menu) = detected {
        if let Ok(suppressed) = pty_menu_suppressed_cell().lock() {
            if let Some((suppressed_tool, when)) = suppressed.as_ref() {
                if suppressed_tool == &new_menu.tool
                    && when.elapsed() < std::time::Duration::from_secs(2)
                {
                    eprintln!(
                        "[pty-menu] suppressed re-detection of tool={} ({}ms after dismissal)",
                        new_menu.tool,
                        when.elapsed().as_millis()
                    );
                    detected = None;
                }
            }
        }
    }

    // Compute transition + apply under lock, then emit outside the lock.
    // Emitting under the lock would risk deadlock if listeners synchronously
    // call back into pty_menu_cell (they don't today, but cheap to avoid).
    let mut emit_payload: Option<Option<PtyMenu>> = None;

    if let Ok(mut menu) = pty_menu_cell().lock() {
        let prev_menu = menu.as_ref().cloned();

        // Sticky-against-eviction guard. If detection returned None but
        // a menu was previously shown, defer the dismiss emit for up
        // to MENU_EVICTION_GRACE_MS. Re-detection within the window
        // suppresses both the dismiss and the re-show — the menu was
        // just briefly hidden in the rolling buffer behind intervening
        // TUI noise. The user-input dismissal path (`pty_menu_clear`)
        // is independent and unaffected. Refs #77 menu-detector
        // stabilization.
        if detected.is_none() && prev_menu.is_some() {
            if let Ok(mut grace) = pty_menu_eviction_grace_cell().lock() {
                match *grace {
                    None => {
                        *grace = Some(std::time::Instant::now());
                        eprintln!(
                            "[pty-menu] eviction grace started for tool={}",
                            prev_menu.as_ref().map(|p| p.tool.as_str()).unwrap_or("?")
                        );
                        return;
                    }
                    Some(started)
                        if started.elapsed().as_millis() < MENU_EVICTION_GRACE_MS =>
                    {
                        return;
                    }
                    Some(started) => {
                        *grace = None;
                        eprintln!(
                            "[pty-menu] eviction grace expired ({}ms); proceeding with dismiss",
                            started.elapsed().as_millis()
                        );
                    }
                }
            }
        } else if detected.is_some() {
            if let Ok(mut grace) = pty_menu_eviction_grace_cell().lock() {
                if grace.is_some() {
                    eprintln!("[pty-menu] eviction grace cleared by re-detection");
                    *grace = None;
                }
            }
        }

        // Don't downgrade a known menu to pending. If detection returned
        // a pending menu but the previous cycle had a definitive tool
        // name, this is just the rolling buffer drifting the header out
        // of the captured `text` window — the on-screen menu hasn't
        // changed. Carry the previous tool forward. Refs #77
        // tighten-pty-menu-emit-cadence.
        if let (Some(d), Some(p)) = (detected.as_mut(), prev_menu.as_ref()) {
            if d.tool == PENDING_TOOL && p.tool != PENDING_TOOL {
                d.tool = p.tool.clone();
            }
        }

        let state_changed = prev_menu.as_ref() != detected.as_ref();
        // First-cycle pending: store the state so the next detect cycle
        // can see we've already waited one, but suppress the `shown`
        // emit and trace until the tool name resolves. Refs #77.
        let detected_is_pending =
            matches!(detected.as_ref(), Some(d) if d.tool == PENDING_TOOL);
        let should_emit_change = state_changed && !detected_is_pending;

        match (&prev_menu, &detected) {
            (None, Some(nm)) => eprintln!("[pty-menu] None -> Some(tool={})", nm.tool),
            (Some(o), Some(nm)) if o != nm => {
                eprintln!("[pty-menu] Some(tool={}) -> Some(tool={})", o.tool, nm.tool)
            }
            (Some(o), None) => {
                eprintln!("[pty-menu] Some(tool={}) -> None (buffer evicted)", o.tool)
            }
            _ => {}
        }

        if should_emit_change && bram_trace_enabled() {
            // Structured [pty-menu] trace, distinct from the operator
            // -facing eprintln! above. `reason=byte-pattern` for shows,
            // `reason=buffer-evicted` when the detector lost the
            // pattern out of PTY_TAIL without the user dismissing it.
            // Explicit user dismissals get their own trace from
            // pty_menu_clear with reason=user-input.
            match (&detected, &prev_menu) {
                (Some(nm), _) => append_bram_trace_line(
                    app,
                    "pty-menu",
                    &format!("state=shown tool={} reason=byte-pattern", nm.tool),
                ),
                (None, Some(prev)) if prev.tool != PENDING_TOOL => append_bram_trace_line(
                    app,
                    "pty-menu",
                    &format!("state=dismissed tool={} reason=buffer-evicted", prev.tool),
                ),
                _ => {}
            }
        }

        *menu = detected;

        if should_emit_change {
            // Emit only when the user-visible menu state changes.
            // PtyMenu's PartialEq compares `tool` only — text/cursor
            // drift no longer flaps the emit cadence. Refs #77.
            emit_payload = Some(menu.clone());
        }
    }

    if let Some(payload) = emit_payload {
        trace_emit_payload(app, "pty-menu-changed", &payload);
        let _ = app.emit("pty-menu-changed", &payload);
    }
}

// Strip ANSI escape sequences so literal-byte matchers aren't fragmented
// by cursor-positioning / color codes that xterm.js renders correctly
// but a byte-level scan does not. Handles the forms Claude Code's TUI
// emits: CSI (ESC '[' params final-byte 0x40..=0x7E), OSC (ESC ']' ...
// terminated by BEL or ESC '\\'), and plain ESC + single byte.
fn strip_ansi(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == 0x1B && i + 1 < input.len() {
            match input[i + 1] {
                b'[' => {
                    let mut j = i + 2;
                    while j < input.len() {
                        let c = input[j];
                        j += 1;
                        if (0x40..=0x7E).contains(&c) {
                            break;
                        }
                    }
                    i = j;
                    continue;
                }
                b']' => {
                    let mut j = i + 2;
                    while j < input.len() {
                        if input[j] == 0x07 {
                            j += 1;
                            break;
                        }
                        if input[j] == 0x1B && j + 1 < input.len() && input[j + 1] == b'\\' {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
                _ => {
                    i += 2;
                    continue;
                }
            }
        }
        out.push(b);
        i += 1;
    }
    out
}

// Look for claude's permission menu in the rolling tail. Pattern:
// "1. Yes" appears, followed by "2. " within ~512 bytes (the menu's
// options are tightly grouped). Tool name is best-effort guessed
// from preceding context. Runs on the ANSI-stripped tail — the raw
// bytes contain escape sequences interleaved within the visible menu
// text, which would fragment the literal needle match.
fn pty_menu_detect(tail: &[u8]) -> Option<PtyMenu> {
    let stripped = strip_ansi(tail);
    let tail = stripped.as_slice();
    // Anchor on the menu's selection-cursor (❯, U+276F) directly
    // followed by "1." — appears only on the first option of a live
    // permission menu. Looking for "1. Yes" or "1.Yes" alone doesn't
    // work because inter-word spacing on option lines is rendered via
    // cursor-positioning escapes (not literal spaces), so strip_ansi
    // can leave "1.Yes" with no separator. See diagnostic captures
    // in /tmp/pty-menu-snapshot.txt.
    let needle1: &[u8] = b"\xe2\x9d\xaf1.";
    let needle2: &[u8] = b"2.";
    let header: &[u8] = b"Do you want";
    let pos1_opt = tail.windows(needle1.len()).rposition(|w| w == needle1);
    let pos_header = tail.windows(header.len()).rposition(|w| w == header);

    let result = (|| -> Option<PtyMenu> {
        let pos1 = pos1_opt?;
        let after = &tail[pos1..];
        let rel = after.windows(needle2.len()).position(|w| w == needle2)?;
        let pos2 = pos1 + rel;
        if pos2 - pos1 > 512 {
            return None;
        }
        let start = pos1.saturating_sub(200);
        let end = (pos2 + 200).min(tail.len());
        let text = String::from_utf8_lossy(&tail[start..end]).into_owned();
        // Prefer parsing the tool name from the menu's own
        // "Do you want to use X?" header (which lives inside `text`).
        // Falls back to the pre-menu context grep when the header is
        // missing or unparseable. The header-parse moves with the menu
        // through the rolling buffer, so the reported tool name stays
        // stable across short eviction cycles instead of flipping to
        // whatever earlier prompt's tool name happens to still be in
        // the 200-byte pre-context window. Refs #77 menu-detector
        // stabilization (the 18:52:51Z 31-events-in-one-second burst
        // with Bash <-> Tool <-> Read flapping).
        let tool = match pty_menu_tool_from_header(&text) {
            Some(t) => t,
            None => {
                // Header text hasn't landed in this cycle's tail. If
                // the previous cycle was already pending, we've waited
                // a full cycle and the header still isn't here — fall
                // back to the pre-menu grep now. Otherwise mark the
                // menu pending so `pty_menu_update` suppresses the
                // shown emit until we either get a header next cycle
                // or convert to grep on the cycle after. Refs #77
                // tighten-pty-menu-emit-cadence.
                let prev_was_pending = pty_menu_cell()
                    .lock()
                    .ok()
                    .and_then(|m| m.as_ref().map(|p| p.tool == PENDING_TOOL))
                    .unwrap_or(false);
                if prev_was_pending {
                    pty_menu_guess_tool(&tail[..pos1])
                } else {
                    PENDING_TOOL.to_string()
                }
            }
        };
        Some(PtyMenu { tool, text })
    })();

    // Diagnostic: when the menu prompt header is present but detection
    // returned None, log what we found AND dump the full stripped tail
    // to /tmp/pty-menu-snapshot.txt so we can iterate on the matcher.
    if result.is_none() {
        if let Some(h) = pos_header {
            let pos2_after_pos1 = pos1_opt.and_then(|p1| {
                tail[p1..]
                    .windows(needle2.len())
                    .position(|w| w == needle2)
                    .map(|rel| p1 + rel)
            });
            let dump_end = (h + 300).min(tail.len());
            let dump = &tail[h..dump_end];
            let mut printable = String::new();
            for &b in dump {
                match b {
                    b'\n' => printable.push_str("\\n"),
                    b'\r' => printable.push_str("\\r"),
                    b'\t' => printable.push_str("\\t"),
                    0x20..=0x7E => printable.push(b as char),
                    _ => printable.push_str(&format!("\\x{:02x}", b)),
                }
            }
            eprintln!(
                "[pty-menu-trace] miss: stripped_len={} header_at={} '1. Yes'_at={:?} '2. '_after_pos1_at={:?} after_header={:?}",
                tail.len(),
                h,
                pos1_opt,
                pos2_after_pos1,
                printable
            );
            let _ = std::fs::write("/tmp/pty-menu-snapshot.txt", tail);
        }
    }

    result
}

// Extract the tool name from a "Do you want to use X?" prompt header
// inside the captured menu text. Returns None when the header is absent
// or unparseable so the caller can fall back to pre-menu context grep.
// The token after "use " is read up to the first non-name character;
// covers `Bash`, `Edit`, `Write`, `MultiEdit`, `Read`, `mcp__foo__bar`,
// `WebFetch`, etc. Trailing punctuation (`?`, `,`, whitespace) is not
// captured. Refs #77 menu-detector stabilization.
fn pty_menu_tool_from_header(text: &str) -> Option<String> {
    let needle = "Do you want to use ";
    let start = text.find(needle)? + needle.len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.' && c != '-')
        .unwrap_or(rest.len());
    let tok = &rest[..end];
    if tok.is_empty() {
        None
    } else {
        Some(tok.to_string())
    }
}

fn pty_menu_guess_tool(context: &[u8]) -> String {
    let s = String::from_utf8_lossy(context);
    for tool in &[
        "MultiEdit",
        "ToolSearch",
        "WebFetch",
        "WebSearch",
        "Bash",
        "Edit",
        "Write",
        "Read",
        "Grep",
        "Glob",
    ] {
        if s.contains(tool) {
            return (*tool).to_string();
        }
    }
    "Tool".to_string()
}

// Called from pty_write on user input. Records the dismissed menu's
// tool name into PTY_MENU_SUPPRESSED so the detector won't immediately
// re-fire when the next PTY chunk arrives (the dismissed text is still
// in the rolling buffer).
fn pty_menu_clear<R: tauri::Runtime>(app: &AppHandle<R>) {
    let dismissed_tool: Option<String> = match pty_menu_cell().lock() {
        Ok(mut menu) => {
            let tool = menu.as_ref().map(|m| m.tool.clone());
            *menu = None;
            tool
        }
        Err(_) => None,
    };
    // Drain PTY_TAIL so the dismissed menu's bytes can't trigger a stale
    // re-detection once PTY_MENU_SUPPRESSED expires. Only genuinely new
    // PTY output can re-fire the detector after this point.
    if let Ok(mut tail) = pty_tail_cell().lock() {
        tail.clear();
    }
    // User dismissal supersedes any pending eviction-grace deferral.
    if let Ok(mut grace) = pty_menu_eviction_grace_cell().lock() {
        *grace = None;
    }
    if let Some(tool) = dismissed_tool {
        // Pending menus never emitted `state=shown` to subscribers
        // (their tool name hadn't resolved yet). Don't emit the matching
        // `state=dismissed` trace and don't add a re-detection
        // suppression entry — there's no concrete tool name to suppress
        // against, and the iframe was never told about the menu so it
        // has nothing to clear. Refs #77 tighten-pty-menu-emit-cadence.
        if tool == PENDING_TOOL {
            eprintln!("[pty-menu] cleared by user input (pending menu — shown emit was deferred)");
            return;
        }
        eprintln!(
            "[pty-menu] cleared by user input (tool={}, suppressing for 2s)",
            tool
        );
        if bram_trace_enabled() {
            append_bram_trace_line(
                app,
                "pty-menu",
                &format!("state=dismissed tool={} reason=user-input", tool),
            );
        }
        if let Ok(mut s) = pty_menu_suppressed_cell().lock() {
            *s = Some((tool, std::time::Instant::now()));
        }
        // Tell subscribers the menu went away. Emit AFTER releasing all
        // pty_menu_* locks for the same anti-deadlock reason as in
        // pty_menu_update.
        trace_emit_signal(app, "pty-menu-changed");
        let _ = app.emit::<Option<PtyMenu>>("pty-menu-changed", None);
    }
}

// Hex+ASCII dump of bytes — for the /__pty-tail debug endpoint.
fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4);
    for (i, chunk) in bytes.chunks(16).enumerate() {
        out.push_str(&format!("{:08x}  ", i * 16));
        for (j, b) in chunk.iter().enumerate() {
            out.push_str(&format!("{:02x} ", b));
            if j == 7 {
                out.push(' ');
            }
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        if chunk.len() <= 8 {
            out.push(' ');
        }
        out.push_str(" |");
        for b in chunk {
            if (0x20..0x7f).contains(b) {
                out.push(*b as char);
            } else {
                out.push('.');
            }
        }
        out.push_str("|\n");
    }
    out
}

fn compare_versions(current: &str, latest: &str) -> bool {
    let parse =
        |s: &str| -> Vec<u32> { s.split('.').filter_map(|x| x.parse::<u32>().ok()).collect() };
    parse(latest) > parse(current)
}

fn fetch_app_info() -> AppInfo {
    let current = first_nonempty_env(&["BRAM_FAKE_CURRENT", "XMLUI_DESKTOP_FAKE_CURRENT"])
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    // curl ships on macOS / Linux / Windows 10+; avoids pulling in an HTTP
    // dependency for a single, tolerant-of-failure fetch.
    let output = std::process::Command::new("curl")
        .args([
            "-sf",
            "-m",
            "5",
            "-H",
            "User-Agent: bram",
            "-H",
            "Accept: application/vnd.github+json",
            "https://api.github.com/repos/judell/bram/releases/latest",
        ])
        .output();

    let bytes = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            return AppInfo {
                current,
                latest: None,
                has_update: false,
                release_url: None,
            }
        }
    };

    let v: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            return AppInfo {
                current,
                latest: None,
                has_update: false,
                release_url: None,
            }
        }
    };

    let tag = v.get("tag_name").and_then(|x| x.as_str()).unwrap_or("");
    let latest_str = tag.trim_start_matches('v').to_string();
    if latest_str.is_empty() {
        return AppInfo {
            current,
            latest: None,
            has_update: false,
            release_url: None,
        };
    }
    let release_url = v.get("html_url").and_then(|x| x.as_str()).map(String::from);
    let has_update = compare_versions(&current, &latest_str);
    AppInfo {
        current,
        latest: Some(latest_str),
        has_update,
        release_url,
    }
}

fn get_app_info() -> AppInfo {
    let cache = APP_INFO_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if let Some(cached) = guard.as_ref() {
        return cached.clone();
    }
    let info = fetch_app_info();
    *guard = Some(info.clone());
    info
}

fn git_log_recent<R: tauri::Runtime>(app: &AppHandle<R>, count: usize) -> Result<Vec<u8>, String> {
    use std::collections::HashSet;
    // Determine which commits are ahead of the remote; treat the rest as pushed.
    // If there's no upstream tracking, we just call everything "unpushed".
    let unpushed: HashSet<String> = git_run(app, &["rev-list", "@{u}..HEAD"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // Resolve the GitHub URL for the html_url field, if any.
    let remote_url = git_run(app, &["remote", "get-url", "origin"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let html_base = remote_to_html(&remote_url);

    // Local git identity + GitHub login so we can map commits authored by
    // the current user to their actual GH username (not just their email
    // local-part — `jon@jonudell.info` resolves to GH login `judell`).
    let local_email = git_run(app, &["config", "user.email"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let local_login: Option<String> = project_root(Some(app)).and_then(|root| {
        std::process::Command::new("gh")
            .current_dir(&root)
            .args(&["api", "/user", "--jq", ".login"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    });

    let count_arg = format!("-n{}", count);
    // __C__ sentinel marks the start of each commit; --shortstat lines
    // appear in between. Merge commits and commits with no file changes
    // emit no shortstat line, so we finalize on the next sentinel.
    // %ae = full author email (matched against local git user.email);
    // %al = email local-part used as a fallback / for non-local authors.
    let format = "--format=__C__%H%x09%an%x09%aI%x09%ae%x09%al%x09%s";
    let log_out = git_run(app, &["log", &count_arg, "--shortstat", format])?;

    let mut commits: Vec<serde_json::Value> = Vec::new();
    let mut header_parts: Option<(String, String, String, String, String)> = None;
    let mut additions: u64 = 0;
    let mut deletions: u64 = 0;

    let finalize = |hdr: &Option<(String, String, String, String, String)>,
                    adds: u64,
                    dels: u64,
                    out: &mut Vec<serde_json::Value>| {
        if let Some((sha, author, date, login, subject)) = hdr {
            let pushed = !unpushed.contains(sha);
            let html_url = if html_base.is_empty() {
                String::new()
            } else {
                format!("{}/commit/{}", html_base, sha)
            };
            out.push(serde_json::json!({
                "sha": sha,
                "html_url": html_url,
                "pushed": pushed,
                "additions": adds,
                "deletions": dels,
                "commit": {
                    "author": { "name": author, "date": date, "login": login },
                    "message": subject,
                },
            }));
        }
    };

    for line in log_out.lines() {
        if let Some(rest) = line.strip_prefix("__C__") {
            finalize(&header_parts, additions, deletions, &mut commits);
            additions = 0;
            deletions = 0;
            let parts: Vec<&str> = rest.splitn(6, '\t').collect();
            if parts.len() == 6 {
                let author_email = parts[3];
                let raw_local = parts[4];
                // Prefer the live `gh api /user` login when the commit was
                // authored with the local git identity; otherwise strip the
                // GitHub noreply `<digits>+` prefix from the email local-part.
                let login = if !author_email.is_empty()
                    && author_email == local_email
                    && local_login.is_some()
                {
                    local_login.clone().unwrap()
                } else if let Some(idx) = raw_local.find('+') {
                    if !raw_local[..idx].is_empty()
                        && raw_local[..idx].chars().all(|c| c.is_ascii_digit())
                    {
                        raw_local[idx + 1..].to_string()
                    } else {
                        raw_local.to_string()
                    }
                } else {
                    raw_local.to_string()
                };
                header_parts = Some((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                    login,
                    parts[5].to_string(),
                ));
            } else {
                header_parts = None;
            }
        } else if header_parts.is_some() {
            // Shortstat line, e.g.: " 3 files changed, 18 insertions(+), 2 deletions(-)"
            for part in line.trim().split(", ") {
                if let Some(idx) = part.find(' ') {
                    let n: u64 = part[..idx].parse().unwrap_or(0);
                    let rest = &part[idx + 1..];
                    if rest.contains("insertion") {
                        additions = n;
                    } else if rest.contains("deletion") {
                        deletions = n;
                    }
                }
            }
        }
    }
    finalize(&header_parts, additions, deletions, &mut commits);

    serde_json::to_vec(&commits).map_err(|e| e.to_string())
}

// Full-history commit search. Walks `git log` (full body via %B) and
// matches each commit's subject+body lines and author against the
// query (case-insensitive substring). Returns the Context-shaped
// payload: `{ results: [{...commit fields, hits: [{line, snippet,
// field}]}], truncated }`. Capped at MAX_RESULTS commits scanned and
// MAX_HITS total hits so a wide-net query doesn't pin git.
fn git_log_search<R: tauri::Runtime>(app: &AppHandle<R>, query: &str) -> Result<Vec<u8>, String> {
    use serde_json::json;
    use std::collections::HashSet;
    let q = query.trim();
    if q.is_empty() {
        return Ok(b"{\"results\":[],\"truncated\":false}".to_vec());
    }
    let needle = q.to_lowercase();
    const MAX_RESULTS: usize = 50;
    const MAX_HITS: usize = 200;

    let unpushed: HashSet<String> = git_run(app, &["rev-list", "@{u}..HEAD"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let remote_url = git_run(app, &["remote", "get-url", "origin"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let html_base = remote_to_html(&remote_url);

    // Use record/field separators that won't appear in commit
    // messages so we can reassemble multi-line bodies safely.
    // %x1e between records, %x1f between fields, body last.
    let format = "--format=%H%x1f%an%x1f%aI%x1f%B%x1e";
    let log_out = git_run(app, &["log", "-n2000", format])?;

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut total_hits = 0usize;
    let mut truncated = false;

    for record in log_out.split('\x1e') {
        let record = record.trim_start_matches('\n');
        if record.is_empty() {
            continue;
        }
        let parts: Vec<&str> = record.splitn(4, '\x1f').collect();
        if parts.len() != 4 {
            continue;
        }
        if results.len() >= MAX_RESULTS || total_hits >= MAX_HITS {
            truncated = true;
            break;
        }
        let sha = parts[0].to_string();
        let author = parts[1];
        let date = parts[2];
        let body = parts[3].trim_end_matches('\n');
        let subject = body.lines().next().unwrap_or("").to_string();

        let mut hits: Vec<serde_json::Value> = Vec::new();
        for (i, line) in body.lines().enumerate() {
            if total_hits >= MAX_HITS {
                truncated = true;
                break;
            }
            if line.to_lowercase().contains(&needle) {
                let snippet: String = line.trim().chars().take(200).collect();
                hits.push(json!({
                    "line": i + 1,
                    "snippet": snippet,
                    "field": if i == 0 { "subject" } else { "body" },
                }));
                total_hits += 1;
            }
        }
        if author.to_lowercase().contains(&needle) && total_hits < MAX_HITS {
            hits.push(json!({
                "line": 0,
                "snippet": author,
                "field": "author",
            }));
            total_hits += 1;
        }

        if hits.is_empty() {
            continue;
        }
        let pushed = !unpushed.contains(&sha);
        let html_url = if html_base.is_empty() {
            String::new()
        } else {
            format!("{}/commit/{}", html_base, sha)
        };
        results.push(json!({
            "sha": sha,
            "html_url": html_url,
            "pushed": pushed,
            "commit": {
                "author": { "name": author, "date": date },
                "message": subject,
            },
            "body": body,
            "hits": hits,
        }));
    }

    serde_json::to_vec(&json!({ "results": results, "truncated": truncated }))
        .map_err(|e| e.to_string())
}

// Shell out to `gh` to list issues for the current repo. Returns the raw
// JSON bytes from `gh`. On any failure (gh missing, not a GitHub repo,
// auth missing, etc) returns an empty JSON array so the frontend renders
// a friendly empty state rather than a 500.
// gh issue list caps at the N newest issues across all states; set well
// above the repo's total issue count so no open issue is dropped (issue
// #104). Both gh_issues_list and gh_issues_search must share this so they
// can't drift. Bump if the repo ever approaches this many issues.
const GH_ISSUE_LIST_LIMIT: &str = "500";

fn gh_issues_list<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    let root = project_root(Some(app)).ok_or_else(|| "no project root".to_string())?;
    let repo_slug = repo_owner_name(app);
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(&[
            "issue",
            "list",
            "--json",
            "number,title,state,author,createdAt,updatedAt,labels,url,comments",
            "--limit",
            GH_ISSUE_LIST_LIMIT,
            "--state",
            "all",
        ])
        .output();
    match out {
        Ok(out) if out.status.success() => {
            let mut issues: Vec<serde_json::Value> = match serde_json::from_slice(&out.stdout) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[gh issue list] parse: {}", e);
                    return Ok(b"[]".to_vec());
                }
            };
            for issue in &mut issues {
                enrich_issue_activity(app, issue, repo_slug.as_deref());
            }
            serde_json::to_vec(&issues).map_err(|e| e.to_string())
        }
        Ok(out) => {
            eprintln!(
                "[gh issue list] non-zero exit: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            Ok(b"[]".to_vec())
        }
        Err(e) => {
            eprintln!("[gh issue list] failed to spawn: {}", e);
            Ok(b"[]".to_vec())
        }
    }
}

// Issue search: shells out to `gh issue list --search "<q>"`, then for each
// matched issue computes per-line hits across `title` and `body` (no comment
// search yet — adding that requires a second `gh issue view` per hit). Same
// shape as Commits search: { results: [{...fields, hits: [{line, snippet,
// field}]}], truncated }. On any gh failure returns the empty envelope so
// the frontend renders cleanly.
fn gh_issues_search<R: tauri::Runtime>(app: &AppHandle<R>, query: &str) -> Result<Vec<u8>, String> {
    use serde_json::json;
    let q = query.trim();
    if q.is_empty() {
        return Ok(b"{\"results\":[],\"truncated\":false}".to_vec());
    }
    let needle = q.to_lowercase();
    const MAX_HITS: usize = 200;

    let root = project_root(Some(app)).ok_or_else(|| "no project root".to_string())?;
    let repo_slug = repo_owner_name(app);
    // Fetch the same issue window as gh_issues_list (shared
    // GH_ISSUE_LIST_LIMIT, no --search flag); local grep over title + body
    // + comment bodies. One gh call; latency scales with the actual issue
    // count, gaining comment-text search for free without doubling it.
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(&[
            "issue",
            "list",
            "--json",
            "number,title,state,author,createdAt,updatedAt,labels,url,body,comments",
            "--limit",
            GH_ISSUE_LIST_LIMIT,
            "--state",
            "all",
        ])
        .output();
    let stdout = match out {
        Ok(out) if out.status.success() => out.stdout,
        Ok(out) => {
            eprintln!(
                "[gh issue list] non-zero exit: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            return Ok(b"{\"results\":[],\"truncated\":false}".to_vec());
        }
        Err(e) => {
            eprintln!("[gh issue list] failed to spawn: {}", e);
            return Ok(b"{\"results\":[],\"truncated\":false}".to_vec());
        }
    };
    let issues: Vec<serde_json::Value> = match serde_json::from_slice(&stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[gh issue list] parse: {}", e);
            return Ok(b"{\"results\":[],\"truncated\":false}".to_vec());
        }
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut total_hits = 0usize;
    let mut truncated = false;

    for mut issue in issues {
        enrich_issue_activity(app, &mut issue, repo_slug.as_deref());
        if total_hits >= MAX_HITS {
            truncated = true;
            break;
        }
        let title = issue.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let body = issue.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let issue_author = issue
            .get("author")
            .and_then(|v| v.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut hits: Vec<serde_json::Value> = Vec::new();
        if title.to_lowercase().contains(&needle) {
            let snippet: String = title.trim().chars().take(200).collect();
            hits.push(json!({
                "line": 0,
                "snippet": snippet,
                "field": "title",
            }));
            total_hits += 1;
        }
        if total_hits < MAX_HITS && issue_author.to_lowercase().contains(&needle) {
            hits.push(json!({
                "line": 0,
                "snippet": issue_author,
                "field": "author",
            }));
            total_hits += 1;
        }
        for (i, line) in body.lines().enumerate() {
            if total_hits >= MAX_HITS {
                truncated = true;
                break;
            }
            if line.to_lowercase().contains(&needle) {
                let snippet: String = line.trim().chars().take(200).collect();
                hits.push(json!({
                    "line": i + 1,
                    "snippet": snippet,
                    "field": "body",
                }));
                total_hits += 1;
            }
        }
        // Grep each comment's body, per-line. `comments` is an array of
        // {body, author: {login}, ...} from the JSON fields list.
        if total_hits < MAX_HITS {
            if let Some(comments) = issue.get("comments").and_then(|v| v.as_array()) {
                'comments: for (ci, comment) in comments.iter().enumerate() {
                    let cbody = comment.get("body").and_then(|v| v.as_str()).unwrap_or("");
                    let cauthor = comment
                        .get("author")
                        .and_then(|v| v.get("login"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if cauthor.to_lowercase().contains(&needle) {
                        if total_hits >= MAX_HITS {
                            truncated = true;
                            break 'comments;
                        }
                        hits.push(json!({
                            "line": 0,
                            "snippet": cauthor,
                            "field": "author",
                            "commentIndex": ci,
                            "commentAuthor": cauthor,
                        }));
                        total_hits += 1;
                    }
                    for (i, line) in cbody.lines().enumerate() {
                        if total_hits >= MAX_HITS {
                            truncated = true;
                            break 'comments;
                        }
                        if line.to_lowercase().contains(&needle) {
                            let snippet: String = line.trim().chars().take(200).collect();
                            hits.push(json!({
                                "line": i + 1,
                                "snippet": snippet,
                                "field": "comment",
                                "commentIndex": ci,
                                "commentAuthor": cauthor,
                            }));
                            total_hits += 1;
                        }
                    }
                }
            }
        }
        if hits.is_empty() {
            continue;
        }
        let mut out_issue = issue.clone();
        if let Some(obj) = out_issue.as_object_mut() {
            obj.insert("hits".into(), serde_json::Value::Array(hits));
        }
        results.push(out_issue);
    }

    serde_json::to_vec(&json!({ "results": results, "truncated": truncated }))
        .map_err(|e| e.to_string())
}

// Shell out to `gh issue view <number> --json ...` and return the raw JSON
// bytes. Same failure envelope as gh_issues_list — empty object on any
// error so the frontend can render something rather than 500.
fn gh_issue_view<R: tauri::Runtime>(app: &AppHandle<R>, number: u64) -> Result<Vec<u8>, String> {
    let root = project_root(Some(app)).ok_or_else(|| "no project root".to_string())?;
    let n = number.to_string();
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(&[
            "issue",
            "view",
            &n,
            "--json",
            "number,title,body,state,author,createdAt,updatedAt,labels,url,comments",
        ])
        .output();
    match out {
        Ok(out) if out.status.success() => {
            let mut issue: serde_json::Value = match serde_json::from_slice(&out.stdout) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[gh issue view {}] parse: {}", n, e);
                    return Ok(b"{}".to_vec());
                }
            };
            enrich_issue_activity(app, &mut issue, None);
            let cross_refs = gh_issue_cross_references(app, number);
            let is_closed = issue.get("state").and_then(|v| v.as_str()) == Some("CLOSED");
            let closed_event = if is_closed {
                repo_owner_name(app).and_then(|slug| gh_issue_closed_event_actor(app, &slug, number))
            } else {
                None
            };
            if let Some(obj) = issue.as_object_mut() {
                obj.insert(
                    "crossReferences".to_string(),
                    serde_json::Value::Array(cross_refs),
                );
                if let Some((by, by_at)) = closed_event {
                    obj.insert("closedBy".to_string(), serde_json::Value::String(by));
                    obj.insert("closedByAt".to_string(), serde_json::Value::String(by_at));
                }
            }
            serde_json::to_vec(&issue).map_err(|e| e.to_string())
        }
        Ok(out) => {
            eprintln!(
                "[gh issue view {}] non-zero exit: {}",
                n,
                String::from_utf8_lossy(&out.stderr)
            );
            Ok(b"{}".to_vec())
        }
        Err(e) => {
            eprintln!("[gh issue view {}] failed to spawn: {}", n, e);
            Ok(b"{}".to_vec())
        }
    }
}

// Fetch issues that cross-reference the given issue, via the GitHub timeline
// API. Returns [{number, title, state}, ...] sorted by issue number. State is
// normalized to uppercase to match the rest of the issue payload. Empty on any
// failure — the cross-reference list is an enhancement, not a hard
// requirement, so quiet degradation is the right behavior.
fn gh_issue_cross_references<R: tauri::Runtime>(
    app: &AppHandle<R>,
    number: u64,
) -> Vec<serde_json::Value> {
    let Some(root) = project_root(Some(app)) else {
        return vec![];
    };
    let n = number.to_string();
    let endpoint = format!("repos/:owner/:repo/issues/{}/timeline", n);
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(&[
            "api",
            &endpoint,
            "--jq",
            r#"[.[] | select(.event == "cross-referenced" and .source.issue) | {number: .source.issue.number, title: .source.issue.title, state: (.source.issue.state | ascii_upcase)}]"#,
        ])
        .output();
    match out {
        Ok(out) if out.status.success() => {
            serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout).unwrap_or_default()
        }
        Ok(out) => {
            eprintln!(
                "[gh api .../issues/{}/timeline] non-zero exit: {}",
                n,
                String::from_utf8_lossy(&out.stderr)
            );
            vec![]
        }
        Err(e) => {
            eprintln!("[gh api .../issues/{}/timeline] failed to spawn: {}", n, e);
            vec![]
        }
    }
}

// Post a comment to a GitHub issue via `gh issue comment <n> --body "..."`.
// Returns `{"ok":true}` on success; on failure returns the gh stderr as the
// error body so the frontend can surface it. Empty/whitespace bodies are
// rejected up front since gh would reject them anyway.
fn gh_issue_comment<R: tauri::Runtime>(
    app: &AppHandle<R>,
    number: u64,
    body: &str,
) -> Result<Vec<u8>, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("empty comment body".to_string());
    }
    let root = project_root(Some(app)).ok_or_else(|| "no project root".to_string())?;
    let n = number.to_string();
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(&["issue", "comment", &n, "--body", trimmed])
        .output()
        .map_err(|e| format!("failed to spawn gh: {}", e))?;
    if out.status.success() {
        Ok(b"{\"ok\":true}".to_vec())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        eprintln!("[gh issue comment {}] non-zero exit: {}", n, stderr);
        Err(stderr)
    }
}

// Close a GitHub issue via `gh issue close <n>`, optionally with a comment.
// Returns `{"ok":true}` on success; on failure returns the gh stderr as the
// error body so the frontend can surface it.
fn gh_issue_close<R: tauri::Runtime>(
    app: &AppHandle<R>,
    number: u64,
    comment: &str,
) -> Result<Vec<u8>, String> {
    let root = project_root(Some(app)).ok_or_else(|| "no project root".to_string())?;
    let n = number.to_string();
    let mut args: Vec<&str> = vec!["issue", "close", &n];
    let trimmed = comment.trim();
    if !trimmed.is_empty() {
        args.push("-c");
        args.push(trimmed);
    }
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to spawn gh: {}", e))?;
    if out.status.success() {
        Ok(b"{\"ok\":true}".to_vec())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        eprintln!("[gh issue close {}] non-zero exit: {}", n, stderr);
        Err(stderr)
    }
}

fn issue_actor_label(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let obj = value.as_object()?;
    for key in ["login", "name"] {
        let Some(s) = obj.get(key).and_then(|v| v.as_str()) else {
            continue;
        };
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn repo_owner_name<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<String> {
    let remote_url = git_run(app, &["remote", "get-url", "origin"]).ok()?;
    let html_base = remote_to_html(remote_url.trim());
    let slug = html_base.strip_prefix("https://github.com/")?;
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_string())
    }
}

fn gh_issue_closed_event_actor<R: tauri::Runtime>(
    app: &AppHandle<R>,
    repo_slug: &str,
    number: u64,
) -> Option<(String, String)> {
    let root = project_root(Some(app))?;
    let path = format!("repos/{}/issues/{}/events", repo_slug, number);
    let out = std::process::Command::new("gh")
        .current_dir(&root)
        .args(["api", &path])
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "[gh issue events {}] non-zero exit: {}",
            number,
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    let events: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
    let mut latest: Option<(String, String)> = None;
    for event in events {
        if event.get("event").and_then(|v| v.as_str()) != Some("closed") {
            continue;
        }
        let actor = event.get("actor").and_then(issue_actor_label)?;
        let created_at = event.get("created_at").and_then(|v| v.as_str())?;
        let should_replace = latest
            .as_ref()
            .map(|(_, current_at)| created_at > current_at.as_str())
            .unwrap_or(true);
        if should_replace {
            latest = Some((actor, created_at.to_string()));
        }
    }
    latest
}

fn enrich_issue_activity<R: tauri::Runtime>(
    app: &AppHandle<R>,
    issue: &mut serde_json::Value,
    repo_slug: Option<&str>,
) {
    let Some(obj) = issue.as_object_mut() else {
        return;
    };

    let mut latest_comment_at: Option<String> = None;
    let mut latest_comment_author: Option<String> = None;
    let mut comment_counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    if let Some(comments) = obj.get("comments").and_then(|v| v.as_array()) {
        for comment in comments {
            let Some(created_at) = comment.get("createdAt").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(author) = comment.get("author").and_then(issue_actor_label) {
                *comment_counts.entry(author).or_insert(0) += 1;
            }
            let should_replace = latest_comment_at
                .as_deref()
                .map(|current| created_at > current)
                .unwrap_or(true);
            if should_replace {
                latest_comment_at = Some(created_at.to_string());
                latest_comment_author = comment
                    .get("author")
                    .and_then(issue_actor_label)
                    .or_else(|| Some(String::new()));
            }
        }
    }
    if !comment_counts.is_empty() {
        let mut entries: Vec<(String, usize)> = comment_counts.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let summary = entries
            .into_iter()
            .map(|(author, count)| format!("{}: {}", author, count))
            .collect::<Vec<_>>()
            .join(", ");
        obj.insert(
            "commentSummary".to_string(),
            serde_json::Value::String(summary),
        );
    }

    let activity_at = latest_comment_at
        .clone()
        .or_else(|| obj.get("updatedAt").and_then(|v| v.as_str()).map(str::to_string))
        .or_else(|| obj.get("createdAt").and_then(|v| v.as_str()).map(str::to_string));

    if let Some(activity_at) = activity_at {
        obj.insert("activityAt".to_string(), serde_json::Value::String(activity_at));
    }
    if let Some(latest_comment_at) = latest_comment_at {
        obj.insert(
            "latestCommentAt".to_string(),
            serde_json::Value::String(latest_comment_at),
        );
    }
    if let Some(latest_comment_author) = latest_comment_author {
        if !latest_comment_author.is_empty() {
            obj.insert(
                "latestCommentAuthor".to_string(),
                serde_json::Value::String(latest_comment_author),
            );
        }
    }
    let _ = repo_slug;
}

// Resolve the GitHub web URL of the configured origin remote. Used by the
// Issues tab's "New issue" button so the frontend doesn't have to parse the
// remote URL itself. Returns an empty string for both htmlBase and the
// composed URLs when there is no GitHub remote.
fn repo_origin_info<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    let remote_url = git_run(app, &["remote", "get-url", "origin"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let html_base = remote_to_html(&remote_url);
    let issues_url = if html_base.is_empty() {
        String::new()
    } else {
        format!("{}/issues", html_base)
    };
    let issues_new_url = if html_base.is_empty() {
        String::new()
    } else {
        format!("{}/issues/new", html_base)
    };
    let info = serde_json::json!({
        "remoteUrl": remote_url,
        "htmlBase": html_base,
        "issuesUrl": issues_url,
        "issuesNewUrl": issues_new_url,
    });
    serde_json::to_vec(&info).map_err(|e| e.to_string())
}

fn remote_to_html(remote: &str) -> String {
    let r = remote.trim().trim_end_matches(".git");
    if let Some(rest) = r.strip_prefix("git@github.com:") {
        return format!("https://github.com/{}", rest);
    }
    if r.starts_with("https://github.com/") || r.starts_with("http://github.com/") {
        return r.to_string();
    }
    String::new()
}

fn git_commit_detail<R: tauri::Runtime>(app: &AppHandle<R>, sha: &str) -> Result<Vec<u8>, String> {
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid sha".to_string());
    }
    // numstat first, then patch — git lets us combine in one call.
    let numstat = git_run(app, &["show", "--format=", "--numstat", sha])?;
    let mut total_add: u64 = 0;
    let mut total_del: u64 = 0;
    let mut files: Vec<(String, u64, u64)> = Vec::new();
    for line in numstat.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let a: u64 = parts[0].parse().unwrap_or(0);
        let d: u64 = parts[1].parse().unwrap_or(0);
        total_add += a;
        total_del += d;
        files.push((parts[2].to_string(), a, d));
    }

    // Per-file patch via `git show -p --format= -- <file>` would be cleanest,
    // but that's N git invocations. Use one show and split on `diff --git`.
    let patch_all = git_run(app, &["show", "--format=", "-p", sha])?;
    let mut patches: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current_file = String::new();
    let mut current_buf = String::new();
    for line in patch_all.lines() {
        if line.starts_with("diff --git ") {
            if !current_file.is_empty() {
                patches.insert(current_file.clone(), current_buf.clone());
            }
            current_buf.clear();
            current_buf.push_str(line);
            current_buf.push('\n');
            // Extract filename from "diff --git a/<path> b/<path>"
            // Use the b/ side for renames.
            if let Some(rest) = line.strip_prefix("diff --git ") {
                if let Some(b_idx) = rest.find(" b/") {
                    current_file = rest[b_idx + 3..].to_string();
                } else {
                    current_file.clear();
                }
            }
        } else {
            current_buf.push_str(line);
            current_buf.push('\n');
        }
    }
    if !current_file.is_empty() {
        patches.insert(current_file, current_buf);
    }

    let files_json: Vec<serde_json::Value> = files
        .iter()
        .map(|(name, a, d)| {
            serde_json::json!({
                "filename": name,
                "additions": a,
                "deletions": d,
                "patch": patches.get(name).cloned().unwrap_or_default(),
            })
        })
        .collect();
    // Full commit message (%B). Used by the right-pane expander so a
    // commit's body paragraphs are available without `git show` in a
    // terminal.
    let message = git_run(app, &["show", "-s", "--format=%B", sha])
        .unwrap_or_default()
        .trim_end_matches('\n')
        .to_string();
    let detail = serde_json::json!({
        "sha": sha,
        "stats": { "additions": total_add, "deletions": total_del },
        "files": files_json,
        "message": message,
    });
    serde_json::to_vec(&detail).map_err(|e| e.to_string())
}

fn bram_app_root_candidates(
    resource_dir: Option<PathBuf>,
    executable_dir: Option<PathBuf>,
    current_exe: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(resource_dir) = resource_dir {
        candidates.push(resource_dir.join("app"));
    }
    if let Some(executable_dir) = executable_dir {
        candidates.push(executable_dir.join("app"));
    }
    if let Some(exe) = current_exe {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("app"));
            candidates.push(dir.join("../Resources/app"));
        }
    }

    candidates
}

fn resolve_app_root<R: tauri::Runtime>(app: Option<&AppHandle<R>>) -> Option<PathBuf> {
    let resource_dir = app.and_then(|app| app.path().resource_dir().ok());
    let executable_dir = app.and_then(|app| app.path().executable_dir().ok());
    let current_exe = std::env::current_exe().ok();

    bram_app_root_candidates(resource_dir, executable_dir, current_exe)
        .into_iter()
        .find(|path| path.exists())
}

#[tauri::command]
fn pty_spawn(
    cmd: String,
    args: Vec<String>,
    cols: u16,
    rows: u16,
    on_data: Channel<Vec<u8>>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut command = CommandBuilder::new(cmd);
    // Substitute placeholder paths under ./app/ with absolute paths into
    // the bundled app dir, so bash's --rcfile (and any future
    // app-relative arg) resolves correctly regardless of the project
    // root we set as the PTY's cwd. Falls back to extracting from the
    // embedded tree when no on-disk app/ is alongside the binary.
    for a in args {
        let resolved = if let Some(rest) = a.strip_prefix("./app/") {
            match extract_app_file(&app, rest) {
                Ok(p) => p.to_string_lossy().into_owned(),
                Err(e) => {
                    eprintln!("[pty_spawn] could not resolve {}: {}", a, e);
                    a
                }
            }
        } else {
            a
        };
        command.arg(resolved);
    }
    if let Some(root) = project_root(Some(&app)) {
        command.cwd(root);
    } else if let Some(home) = home_dir() {
        command.cwd(home);
    }
    for (k, v) in std::env::vars() {
        command.env(k, v);
    }
    command.env("TERM", "xterm-256color");
    if let Ok(hint_path) = active_agent_hint_path(&app) {
        if let Some(parent) = hint_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&hint_path);
        let hint = hint_path.to_string_lossy().into_owned();
        command.env("BRAM_AGENT_HINT", hint.clone());
        command.env("XMLUI_DESKTOP_AGENT_HINT", hint);
    }
    if let Some(p) = LOOPBACK_PORT.get() {
        command.env("BRAM_PORT", p.to_string());
        command.env("XMLUI_DESKTOP_PORT", p.to_string());
    }
    // Propagate trace toggle + log path into the PTY child so hook
    // scripts (worklist-guard.py for Claude, worklist-guard-codex.py
    // for Codex) can write [hook] records into the same trace file as
    // the host. See trace-category-hook.
    if bram_trace_enabled() {
        command.env("BRAM_TRACE", "1");
        if let Some(path) = project_root(Some(&app)).map(|p| p.join("resources/bram-trace.log")) {
            command.env("BRAM_TRACE_LOG", path.to_string_lossy().into_owned());
        }
    }

    let _child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    *state.0.lock().unwrap() = Some(PtyState {
        master: pair.master,
        writer,
    });

    let app_for_thread = app.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        // [pty-in] throttling state, per the issue #49 spec. Small
        // (<16-byte) reads inside a 50ms window collapse into one
        // summary line; larger reads flush the accumulator and log
        // individually. All state is thread-local — no locking.
        const SMALL_THRESHOLD: usize = 16;
        const SMALL_WINDOW_MS: u128 = 50;
        let mut small_bytes: usize = 0;
        let mut small_runs: usize = 0;
        let mut small_first_preview: String = String::new();
        let mut small_last: Option<std::time::Instant> = None;
        // Time of the previous `[pty-in]` trace emission, used to compute
        // `gap_ms=<n>` for each emit. Lets #78 analysis correlate
        // turn-end fires with the silence gap that triggered them — a
        // premature fire would show `gap_ms` just past the 800 ms
        // threshold; a real end-of-turn shows a much larger gap.
        let mut last_pty_in_emit_at: Option<std::time::Instant> = None;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    if bram_trace_enabled() && small_runs > 0 {
                        let gap_ms = last_pty_in_emit_at
                            .map(|t| t.elapsed().as_millis())
                            .unwrap_or(0);
                        append_bram_trace_line(
                            &app_for_thread,
                            "pty-in",
                            &format!(
                                "gap_ms={} bytes={} runs={} preview={}",
                                gap_ms, small_bytes, small_runs, small_first_preview
                            ),
                        );
                    }
                    break;
                }
                Ok(n) => {
                    if bram_trace_enabled() {
                        let data = &buf[..n];
                        if n >= SMALL_THRESHOLD {
                            // Flush any pending small-read accumulator
                            // first so the order in the log matches the
                            // order of arrivals.
                            if small_runs > 0 {
                                let gap_ms = last_pty_in_emit_at
                                    .map(|t| t.elapsed().as_millis())
                                    .unwrap_or(0);
                                append_bram_trace_line(
                                    &app_for_thread,
                                    "pty-in",
                                    &format!(
                                        "gap_ms={} bytes={} runs={} preview={}",
                                        gap_ms, small_bytes, small_runs, small_first_preview
                                    ),
                                );
                                last_pty_in_emit_at = Some(std::time::Instant::now());
                                small_bytes = 0;
                                small_runs = 0;
                                small_first_preview.clear();
                                small_last = None;
                            }
                            let preview =
                                bram_trace_preview(&String::from_utf8_lossy(data), 80);
                            let gap_ms = last_pty_in_emit_at
                                .map(|t| t.elapsed().as_millis())
                                .unwrap_or(0);
                            append_bram_trace_line(
                                &app_for_thread,
                                "pty-in",
                                &format!("gap_ms={} bytes={} preview={}", gap_ms, n, preview),
                            );
                            last_pty_in_emit_at = Some(std::time::Instant::now());
                        } else {
                            let within_window = small_last
                                .map(|t| t.elapsed().as_millis() < SMALL_WINDOW_MS)
                                .unwrap_or(false);
                            if within_window {
                                small_bytes += n;
                                small_runs += 1;
                            } else {
                                if small_runs > 0 {
                                    let gap_ms = last_pty_in_emit_at
                                        .map(|t| t.elapsed().as_millis())
                                        .unwrap_or(0);
                                    append_bram_trace_line(
                                        &app_for_thread,
                                        "pty-in",
                                        &format!(
                                            "gap_ms={} bytes={} runs={} preview={}",
                                            gap_ms, small_bytes, small_runs, small_first_preview
                                        ),
                                    );
                                    last_pty_in_emit_at = Some(std::time::Instant::now());
                                }
                                small_bytes = n;
                                small_runs = 1;
                                small_first_preview =
                                    bram_trace_preview(&String::from_utf8_lossy(data), 80);
                            }
                            small_last = Some(std::time::Instant::now());
                        }
                    }
                    pty_menu_update(&app_for_thread, &buf[..n]);
                    pty_agent_turn_update(&app_for_thread, &buf[..n]);
                    if on_data.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    if bram_trace_enabled() && small_runs > 0 {
                        let gap_ms = last_pty_in_emit_at
                            .map(|t| t.elapsed().as_millis())
                            .unwrap_or(0);
                        append_bram_trace_line(
                            &app_for_thread,
                            "pty-in",
                            &format!(
                                "gap_ms={} bytes={} runs={} preview={}",
                                gap_ms, small_bytes, small_runs, small_first_preview
                            ),
                        );
                    }
                    break;
                }
            }
        }
    });

    // Optional auto-launch: type the configured agent at the bash
    // prompt. Bash buffers input until interactive; if it's still in
    // rcfile init the keystrokes queue and run as the first command.
    if let Some(root) = project_root(Some(&app)) {
        if let Some(cfg) = load_project_config(&root) {
            if let Some(agent) = cfg.shell.and_then(|s| s.agent) {
                let trimmed = agent.trim();
                if !trimmed.is_empty() {
                    let payload = format!("{}\r", trimmed);
                    let mut guard = state.0.lock().unwrap();
                    if let Some(pty) = guard.as_mut() {
                        if let Err(e) = pty.writer.write_all(payload.as_bytes()) {
                            eprintln!("[pty_spawn] failed to write agent autostart: {}", e);
                        }
                        let _ = pty.writer.flush();
                    }
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn pty_write(app: AppHandle, data: String, state: State<'_, AppState>) -> Result<(), String> {
    pty_write_internal(&app, &state, &data, "unknown")
}

// Shared body of `pty_write` so the disk-mediated relay (#86) can write
// queued intents through the same trace + menu-clear + auth-record
// pipeline as direct callers. `caller_hint` flows into the `[pty-out]`
// trace so we can distinguish direct writes from `pty-intent-*` drains
// at investigation time.
fn pty_write_internal<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    data: &str,
    caller_hint: &str,
) -> Result<(), String> {
    if bram_trace_enabled() && !data.is_empty() {
        append_bram_trace_line(
            app,
            "pty-out",
            &format!(
                "bytes={} preview={} is_structured={} caller_hint={}",
                data.len(),
                bram_trace_preview(data, 80),
                is_structured_intent_prefix(data),
                caller_hint,
            ),
        );
    }
    if !data.is_empty() {
        // \x1b[O (focus-out) and \x1b[I (focus-in) are pure focus-tracking
        // escape sequences that xterm.js emits as side effects of its
        // iframe gaining/losing focus — not user keystrokes. Dismissing
        // a permission menu on these dismisses it prematurely when the
        // user clicks a drawer menu button (which moves focus away from
        // the terminal). Skip the menu-clear for these specific 3-byte
        // sequences; still write them to the PTY (Claude Code may use
        // the focus signal). Closes #94.
        let is_focus_track = data == "\x1b[O" || data == "\x1b[I";
        if !is_focus_track {
            pty_menu_clear(app);
        } else if bram_trace_enabled() {
            let tool = pty_menu_cell()
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|m| m.tool.clone()))
                .unwrap_or_default();
            if !tool.is_empty() {
                append_bram_trace_line(
                    app,
                    "pty-menu",
                    &format!(
                        "state=preserved tool={} reason=focus-track preview={}",
                        tool,
                        bram_trace_preview(data, 16),
                    ),
                );
            }
        }
        record_worklist_authorization_from_input(app, data);
    }
    let mut guard = state.0.lock().unwrap();
    let pty = guard.as_mut().ok_or("pty not started")?;
    pty.writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    pty.writer.flush().map_err(|e| e.to_string())
}

// Disk-mediated relay for right-pane click intents (#86). The right-pane
// helpers (`toShell` / `toTurn` / `sendKeys` in `app/__shell/helpers.js`)
// call this instead of `pty_write` directly. Each invocation appends a
// JSONL line to `resources/.pty-intent.jsonl` then drains the queue
// under a process-wide mutex so concurrent calls can't race the
// read-then-truncate phase. Bracketed-paste framing for `kind:
// "toTurn"` is applied here (in the drain) so the right pane stays
// ignorant of PTY framing.
#[tauri::command]
fn queue_pty_intent(
    app: AppHandle,
    payload: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let kind = payload
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or("missing kind")?
        .to_string();
    let data = payload
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or("missing data")?
        .to_string();
    if !matches!(kind.as_str(), "toShell" | "toTurn" | "sendKeys") {
        return Err(format!("unknown kind: {}", kind));
    }
    let Some(path) = pty_intent_file(&app) else {
        return Err("project root unknown".to_string());
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let seq = PTY_INTENT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let id = format!("intent-{}-{}", unix_now_ms(), seq);
    let line = serde_json::json!({
        "id": id,
        "kind": kind,
        "data": data,
        "at": unix_now_ms(),
    });
    let line_str = serde_json::to_string(&line).map_err(|e| e.to_string())?;

    let _drain_guard = pty_intent_lock().lock().map_err(|e| e.to_string())?;

    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{}", line_str).map_err(|e| e.to_string())?;
    }
    if bram_trace_enabled() {
        append_bram_trace_line(
            &app,
            "pty-intent",
            &format!(
                "op=enqueue id={} kind={} bytes={}",
                id,
                kind,
                data.len()
            ),
        );
    }
    drain_pty_intents(&app, &state)
}

// Drains every queued intent in `resources/.pty-intent.jsonl` through
// `pty_write_internal` (preserves the [pty-out] trace + menu-clear +
// worklist-auth-record pipeline). On a PTY write failure, the failing
// line and all subsequent lines stay in the file for the next drain
// attempt; on success the file is removed. Caller must hold
// `pty_intent_lock()`.
fn drain_pty_intents<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
) -> Result<(), String> {
    let Some(path) = pty_intent_file(app) else {
        return Ok(());
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            if bram_trace_enabled() {
                append_bram_trace_line(app, "pty-intent", "op=drain-read-failed");
            }
            return Ok(());
        }
    };
    if content.trim().is_empty() {
        let _ = std::fs::remove_file(&path);
        if bram_trace_enabled() {
            append_bram_trace_line(app, "pty-intent", "op=drain-empty");
        }
        return Ok(());
    }

    let mut wrote: usize = 0;
    let mut remaining: Vec<String> = Vec::new();
    let mut drain_error: Option<String> = None;

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        if drain_error.is_some() {
            remaining.push(line.to_string());
            continue;
        }
        let intent: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = intent.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let data = intent.get("data").and_then(|v| v.as_str()).unwrap_or("");
        let write_result = match kind {
            "toShell" => {
                let wrapped = format!("{}\n", data);
                pty_write_internal(app, state, &wrapped, "pty-intent-toShell")
            }
            "toTurn" => write_pty_turn_intent(app, state, data),
            "sendKeys" => pty_write_internal(app, state, data, "pty-intent-sendKeys"),
            _ => continue,
        };
        match write_result {
            Ok(()) => {
                wrote += 1;
            }
            Err(e) => {
                drain_error = Some(e);
                remaining.push(line.to_string());
            }
        }
    }

    if remaining.is_empty() {
        let _ = std::fs::remove_file(&path);
    } else if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    {
        for line in &remaining {
            let _ = writeln!(file, "{}", line);
        }
    }

    if bram_trace_enabled() {
        append_bram_trace_line(
            app,
            "pty-intent",
            &format!("op=drain wrote={} remaining={}", wrote, remaining.len()),
        );
    }

    match drain_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn write_pty_turn_intent<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    data: &str,
) -> Result<(), String> {
    if cfg!(windows) {
        pty_write_internal(app, state, "\x15", "pty-intent-toTurn-windows-clear")?;
        pty_write_internal(app, state, data, "pty-intent-toTurn-windows-payload")?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        pty_write_internal(app, state, "\r", "pty-intent-toTurn-windows-submit")
    } else {
        let wrapped = format!("\x15\x1b[200~{}\x1b[201~\r", data);
        pty_write_internal(app, state, &wrapped, "pty-intent-toTurn")
    }
}

#[tauri::command]
fn pty_resize(cols: u16, rows: u16, state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.0.lock().unwrap();
    let pty = guard.as_ref().ok_or("pty not started")?;
    pty.master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

// --- Project-server (.bram.json / legacy .xmlui-desktop.json) -------------

fn load_project_config(root: &Path) -> Option<ProjectConfig> {
    for rel in [".bram.json", ".xmlui-desktop.json"] {
        let path = root.join(rel);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        return match serde_json::from_slice::<ProjectConfig>(&bytes) {
            Ok(cfg) => {
                eprintln!("[project-config] loaded {}", path.display());
                Some(cfg)
            }
            Err(e) => {
                eprintln!("[project-config] failed to parse {}: {}", path.display(), e);
                None
            }
        };
    }
    None
}

fn is_project_config_path(path: &Path) -> bool {
    path.file_name()
        .map_or(false, |n| n == ".bram.json" || n == ".xmlui-desktop.json")
}

fn is_port_listening(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

// Distinguishes a healthy reuse candidate from a wedged orphan. A bare TCP
// connect is not enough — a python -m http.server that was reparented to
// launchd after its xmlui-desktop parent died accepts connects but never
// returns a response. Setup uses this to decide whether to reuse, log a
// loud warning, or spawn fresh.
enum PortStatus {
    Live,
    Unresponsive(String),
    NotListening,
}

fn probe_port_http(port: u16, path: &str) -> PortStatus {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
        Ok(s) => s,
        Err(_) => return PortStatus::NotListening,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let req_path = {
        let p = path.split('?').next().unwrap_or("/");
        if p.is_empty() {
            "/"
        } else {
            p
        }
    };
    let req = format!(
        "GET {} HTTP/1.0\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
        req_path, port
    );
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return PortStatus::Unresponsive(format!("write failed: {}", e));
    }
    let mut buf = [0u8; 64];
    match stream.read(&mut buf) {
        Ok(0) => PortStatus::Unresponsive("empty reply".into()),
        Ok(n) => {
            if buf[..n].starts_with(b"HTTP/") {
                PortStatus::Live
            } else {
                let preview = String::from_utf8_lossy(&buf[..n.min(40)]).to_string();
                PortStatus::Unresponsive(format!("non-HTTP reply: {:?}", preview))
            }
        }
        Err(e) => PortStatus::Unresponsive(format!("read failed: {}", e)),
    }
}

// Spawn the project's server per ServerConfig. Returns the Child on
// success. stdout/stderr are piped and forwarded to xmlui-desktop's
// stderr with a `[server]` prefix. Caller is responsible for waiting
// on the port and storing the Child in state.
fn spawn_project_server(
    cfg: &ServerConfig,
    project_root: &Path,
) -> Result<std::process::Child, String> {
    let cwd = match cfg.cwd.as_deref() {
        Some(rel) => project_root.join(rel),
        None => project_root.to_path_buf(),
    };
    if !cwd.is_dir() {
        return Err(format!("cwd does not exist: {}", cwd.display()));
    }

    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg(&cfg.command);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(&cfg.command);
        c
    };

    let mut child = command
        .current_dir(&cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn `{}`: {}", cfg.command, e))?;

    let pid = child.id();
    eprintln!(
        "[server] spawned pid={} cwd={} cmd={}",
        pid,
        cwd.display(),
        cfg.command
    );

    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[server] {}", line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[server] {}", line);
            }
        });
    }

    Ok(child)
}

// Block until the port answers or the timeout elapses. Returns true if
// the port came up. Used after spawn_project_server so we can warn (but
// not fail) if the server is taking longer than expected; the iframe
// loads eagerly and retries on its own.
fn wait_for_port(port: u16, total_ms: u64) -> bool {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_millis(total_ms);
    while Instant::now() < deadline {
        if is_port_listening(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

// Reconcile xmlui-desktop's runtime state with .xmlui-desktop.json after the
// file changes on disk. Kills the prior project-server child only when its
// command/cwd/port no longer match the file; otherwise we keep the running
// process and just refresh path/query. Always updates PaneUrlsState and emits
// right-pane-reload so main.js re-fetches the URL. Port changes do respawn,
// but the iframe origin shifts — service workers (XMLUI's apiInterceptor,
// MSW) won't rebind cleanly, so we log a warning telling the user to restart.
fn handle_project_config_reload<R: tauri::Runtime>(app_handle: &AppHandle<R>, proj_root: &Path) {
    use tauri::Emitter;

    let new_cfg = load_project_config(proj_root);
    let new_server = new_cfg.as_ref().and_then(|c| c.server.as_ref()).cloned();

    let mut spawned = false;
    let mut port_changed = false;
    {
        let state = app_handle.state::<SpawnedServerState>();
        let mut guard = state.0.lock().unwrap();
        let needs_respawn = match (&new_server, guard.as_ref()) {
            (Some(new), Some(cur)) => {
                new.command != cur.config.command
                    || new.cwd != cur.config.cwd
                    || new.port != cur.config.port
            }
            (Some(_), None) => true,
            (None, Some(_)) => true,
            (None, None) => false,
        };
        if needs_respawn {
            if let Some(mut cur) = guard.take() {
                port_changed = new_server.as_ref().map(|n| n.port) != Some(cur.config.port);
                let pid = cur.child.id();
                let _ = cur.child.kill();
                let _ = cur.child.wait();
                eprintln!("[server] killed pid={} on config reload", pid);
            }
            if let Some(cfg) = new_server.as_ref() {
                match spawn_project_server(cfg, proj_root) {
                    Ok(child) => {
                        *guard = Some(SpawnedServer {
                            child,
                            config: cfg.clone(),
                        });
                        spawned = true;
                    }
                    Err(e) => eprintln!("[server] respawn failed: {}", e),
                }
            }
        }
    }

    if spawned {
        if let Some(cfg) = new_server.as_ref() {
            if !wait_for_port(cfg.port, 5000) {
                eprintln!(
                    "[server] WARNING: respawned port {} did not come up within 5s; right-pane iframe will retry",
                    cfg.port
                );
            } else {
                eprintln!("[server] respawned; port {} is up", cfg.port);
            }
        }
    }

    if port_changed {
        eprintln!(
            "[server] WARNING: port changed via .bram.json; service workers were bound to the old origin and will not rebind cleanly — restart Bram to fully apply"
        );
    }

    // The iframe URL splices `cfg.path` into the /__project namespace,
    // and the upstream is the bare origin (http://host:port/). Both
    // change when the config changes; main.js re-reads them via
    // get_right_pane_url() on the right-pane-reload event below.
    let (new_right_pane_url, new_right_pane_upstream) = match new_server.as_ref() {
        Some(cfg) => {
            let path = if cfg.path.starts_with('/') {
                cfg.path.clone()
            } else {
                format!("/{}", cfg.path)
            };
            (
                format!("{}/__project{}", SHELL_ORIGIN, path),
                format!("http://localhost:{}/", cfg.port),
            )
        }
        None => {
            // Default fallback: iframe loads /__project/index.html and
            // the proxy forwards to the internal loopback (origin +
            // trailing slash). Derive both from default_right_pane.
            let default = {
                let state = app_handle.state::<PaneUrlsState>();
                let urls = state.0.lock().unwrap();
                urls.default_right_pane.clone()
            };
            let upstream = default
                .rsplit_once('/')
                .map(|(base, _)| format!("{}/", base))
                .unwrap_or_else(|| default.clone());
            (format!("{}/__project/index.html", SHELL_ORIGIN), upstream)
        }
    };
    {
        let state = app_handle.state::<PaneUrlsState>();
        let mut urls = state.0.lock().unwrap();
        urls.right_pane = new_right_pane_url.clone();
        urls.right_pane_upstream = new_right_pane_upstream.clone();
    }
    eprintln!(
        "[project-config] reloaded; right-pane url -> {} upstream -> {}",
        new_right_pane_url, new_right_pane_upstream
    );
    trace_emit_signal(&app_handle, "right-pane-reload");
    let _ = app_handle.emit("right-pane-reload", ());
}

#[derive(serde::Serialize)]
struct WhisperStatusReport {
    running: bool,
    pid: Option<u32>,
}

#[tauri::command]
fn whisper_start(
    model_path: String,
    app: AppHandle,
    state: State<'_, WhisperState>,
) -> Result<u32, String> {
    let mut guard = state.0.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(None) => return Err("whisper-server already running".into()),
            Ok(Some(_)) => {}
            Err(_) => {}
        }
    }
    let model = expand_tilde(&model_path);
    // Keep whisper-server's transcoded WAV temp files outside the
    // watched project tree (otherwise the watcher fires right-pane-reload
    // mid-recording).
    let tmp_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache dir: {}", e))?
        .join("whisper");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let tmp_dir_str = tmp_dir.to_string_lossy().to_string();
    let candidates = ["whisper-server", "/opt/homebrew/bin/whisper-server"];
    let mut last_err = String::new();
    for bin in &candidates {
        match std::process::Command::new(bin)
            .arg("-m")
            .arg(&model)
            .arg("--convert")
            .arg("--tmp-dir")
            .arg(&tmp_dir_str)
            .arg("--port")
            .arg("18080")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                let pid = child.id();
                eprintln!(
                    "[whisper] spawned {} pid={} --port 18080 -m {} --tmp-dir {}",
                    bin, pid, model, tmp_dir_str
                );
                *guard = Some(child);
                return Ok(pid);
            }
            Err(e) => last_err = format!("{}: {}", bin, e),
        }
    }
    Err(format!("failed to spawn whisper-server: {}", last_err))
}

#[tauri::command]
fn whisper_stop(state: State<'_, WhisperState>) -> Result<(), String> {
    let mut guard = state.0.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let pid = child.id();
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("[whisper] killed pid={}", pid);
    }
    Ok(())
}

#[tauri::command]
fn whisper_status(state: State<'_, WhisperState>) -> WhisperStatusReport {
    let mut guard = state.0.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(None) => WhisperStatusReport {
                running: true,
                pid: Some(child.id()),
            },
            _ => {
                *guard = None;
                WhisperStatusReport {
                    running: false,
                    pid: None,
                }
            }
        }
    } else {
        WhisperStatusReport {
            running: false,
            pid: None,
        }
    }
}

#[tauri::command]
fn log_from_right_pane(app: AppHandle, payload: serde_json::Value) {
    // Iframe-side trace records arrive with `kind: "iframe-trace"` and a
    // `subkind` field; route them to the [iframe] category in the trace
    // log. Other payloads keep the existing stderr behavior so unrelated
    // iframe logging (e.g. git-push status from helpers.js) still shows
    // up at the command line.
    if payload.get("kind").and_then(|v| v.as_str()) == Some("iframe-trace") {
        if bram_trace_enabled() {
            let subkind = payload
                .get("subkind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            // Render remaining fields (anything other than kind/subkind/at)
            // as a compact JSON object so the line stays scannable. `at`
            // is already captured by the outer [<ISO timestamp>]; the
            // iframe-side `at` is preserved inside the JSON for cases
            // where event-loop scheduling pushes the host's receive
            // moment well after the iframe's send moment.
            let mut rest = serde_json::Map::new();
            if let Some(obj) = payload.as_object() {
                for (k, v) in obj {
                    if k == "kind" || k == "subkind" {
                        continue;
                    }
                    rest.insert(k.clone(), v.clone());
                }
            }
            let rest_str = serde_json::to_string(&serde_json::Value::Object(rest))
                .unwrap_or_else(|_| "{}".to_string());
            append_bram_trace_line(
                &app,
                "iframe",
                &format!("subkind={} {}", subkind, rest_str),
            );
        }
        return;
    }
    eprintln!("[right-pane] {}", payload);
}

#[tauri::command]
fn get_right_pane_url(state: State<'_, PaneUrlsState>) -> String {
    state.0.lock().unwrap().right_pane.clone()
}

#[tauri::command]
fn get_tools_pane_url(state: State<'_, PaneUrlsState>) -> String {
    state.0.lock().unwrap().tools.clone()
}

#[tauri::command]
fn open_devtools(window: tauri::WebviewWindow) {
    #[cfg(debug_assertions)]
    {
        if window.is_devtools_open() {
            window.close_devtools();
        } else {
            window.open_devtools();
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = window;
}

#[tauri::command]
fn git_push(app: AppHandle) -> Result<(), String> {
    let stderr = match git_run(&app, &["push"]) {
        Ok(_) => return Ok(()),
        Err(e) => e,
    };
    let is_nonff = stderr.contains("non-fast-forward") || stderr.contains("fetch first");
    if !is_nonff {
        return Err(stderr);
    }
    auto_rebase_and_push(&app).map_err(|e| format!("non-fast-forward; {}", e))
}

// Rebase local commits on top of origin and retry push. Stashes any
// uncommitted working-tree changes first (rebase requires a clean
// tree) and pops the stash after, regardless of whether the rebase /
// push succeeded. If the stash pop has conflicts, the stash is left
// in place so the user can recover via `git stash list` / `git stash apply`.
fn auto_rebase_and_push<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let dirty = git_run(app, &["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let mut stashed = false;
    if dirty {
        git_run(
            app,
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "bram-auto-rebase",
            ],
        )
        .map_err(|e| format!("auto-stash failed: {}", e))?;
        stashed = true;
    }

    let result: Result<(), String> = (|| {
        let branch =
            git_run(app, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string())?;
        git_run(app, &["fetch", "origin"])?;
        let upstream = format!("origin/{}", branch);
        match git_run(app, &["rebase", &upstream]) {
            Ok(_) => git_run(app, &["push"]).map(|_| ()),
            Err(rebase_err) => {
                let _ = git_run(app, &["rebase", "--abort"]);
                Err(format!(
                    "rebase conflicts (aborted, working tree clean — re-run the rebase manually or ask the agent, then push): {}",
                    rebase_err.trim()
                ))
            }
        }
    })();

    if stashed {
        if let Err(pop_err) = git_run(app, &["stash", "pop"]) {
            let prefix = result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "push succeeded".to_string());
            return Err(format!(
                "{}; stash pop failed: {} (stash retained — recover with `git stash list` / `git stash apply`)",
                prefix,
                pop_err.trim()
            ));
        }
    }

    result
}

#[tauri::command]
fn open_url(url: String, app: AppHandle) -> Result<(), String> {
    // file:// URLs aren't permitted by the opener URL allowlist; route them
    // through open_path so the OS opens the file in its default app.
    if let Some(rest) = url.strip_prefix("file://") {
        // Strip an optional host (e.g. "file:///path" → host="" rest="/path";
        // "file://localhost/path" → strip "localhost" leaving "/path").
        let path = rest.strip_prefix("localhost").unwrap_or(rest);
        return app
            .opener()
            .open_path(path.to_string(), None::<String>)
            .map_err(|e| e.to_string());
    }
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn capture_screenshot<R: tauri::Runtime>(app: AppHandle<R>) -> Result<String, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        return Err("screenshot capture is currently macOS-only".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let out = cache.join(format!("screenshot-{}.png", ts));

        let status = std::process::Command::new("/usr/sbin/screencapture")
            .arg("-i")
            .arg(&out)
            .status()
            .map_err(|e| format!("failed to spawn screencapture: {}", e))?;
        if !status.success() {
            return Err(format!("screencapture exited with status {}", status));
        }
        if !out.exists() {
            // User pressed Esc during the interactive selection.
            return Err("cancelled".to_string());
        }
        eprintln!("[screenshot] wrote {}", out.display());
        Ok(out.to_string_lossy().into_owned())
    }
}

#[tauri::command]
fn save_trace_export(
    filename: String,
    content: String,
    mime_type: String,
) -> Result<String, String> {
    let safe_name = filename
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '_',
            _ => c,
        })
        .collect::<String>();

    let base_dir = home_dir()
        .map(|home| home.join("Downloads"))
        .filter(|path| path.exists())
        .or_else(|| std::env::current_dir().ok())
        .ok_or("could not resolve export directory")?;

    let target = base_dir.join(safe_name);
    std::fs::write(&target, content.as_bytes()).map_err(|e| e.to_string())?;
    eprintln!(
        "[trace-export] wrote {} bytes as {} to {}",
        content.len(),
        mime_type,
        target.display()
    );
    Ok(target.display().to_string())
}

#[derive(serde::Serialize)]
struct SessionEntry {
    id: String,
    mtime: u64,
    size: u64,
    title: Option<String>,
    provider: SessionProvider,
    current: bool,
}

#[derive(Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum SessionProvider {
    Claude,
    Codex,
}

impl SessionProvider {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct SessionRecord {
    provider: SessionProvider,
    id: String,
    path: PathBuf,
    mtime: u64,
    size: u64,
    title: Option<String>,
}

#[derive(serde::Serialize)]
struct SessionsMeta {
    provider: SessionProvider,
    count: usize,
    current_id: Option<String>,
}

fn active_agent_hint_path<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let root = project_root(Some(app)).ok_or("could not resolve project root")?;
    let abs = strip_unc_prefix(root.canonicalize().map_err(|e| e.to_string())?);
    let encoded = encode_path_for_filename(&abs);
    let cache_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    Ok(cache_dir
        .join("agent-hints")
        .join(format!("{}.json", encoded)))
}

fn hinted_session_provider<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<SessionProvider> {
    let path = active_agent_hint_path(app).ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    let record = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    SessionProvider::from_str(record.get("provider")?.as_str()?)
}

fn active_agent_hint_mtime<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<std::time::SystemTime> {
    let path = active_agent_hint_path(app).ok()?;
    std::fs::metadata(path).ok()?.modified().ok()
}

fn claude_sessions_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let root = project_root(Some(app)).ok_or("could not resolve project root")?;
    let abs = strip_unc_prefix(root.canonicalize().map_err(|e| e.to_string())?);
    let encoded = encode_path_for_filename(&abs);
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    Ok(home.join(".claude").join("projects").join(encoded))
}

// Best-effort label for a Claude session. Precedence (matches what
// `claude /resume` displays):
//   1. Most recent `custom-title` record — set via the rename surface
//      (rename_session() at lib.rs:2397). User-supplied, overrides everything.
//   2. Most recent `ai-title` record (field: `aiTitle`) — Claude Code itself
//      writes these auto-generated titles as the conversation evolves and
//      uses them in /resume listings. Walking to the latest one keeps XD in
//      sync with CC after compaction or topic shifts.
//   3. First `user` message snippet — last-resort fallback only used when
//      no title records exist yet (very fresh sessions before CC has
//      generated an ai-title).
// All title-record scans walk the whole file so a custom-title or ai-title
// appended after compaction still wins.
fn claude_session_title(path: &Path) -> std::io::Result<Option<String>> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut first_user: Option<String> = None;
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match record.get("type").and_then(|v| v.as_str()) {
            Some("custom-title") => {
                if let Some(t) = record.get("customTitle").and_then(|v| v.as_str()) {
                    custom_title = Some(t.to_string());
                }
            }
            Some("ai-title") => {
                if let Some(t) = record.get("aiTitle").and_then(|v| v.as_str()) {
                    ai_title = Some(t.to_string());
                }
            }
            Some("user") if first_user.is_none() => {
                if let Some(content) = record.pointer("/message/content") {
                    let text = match content {
                        serde_json::Value::String(s) => s.clone(),
                        _ => content.to_string(),
                    };
                    first_user = Some(text.chars().take(120).collect());
                }
            }
            _ => {}
        }
    }
    Ok(custom_title.or(ai_title).or(first_user))
}

fn claude_message_text(record: &serde_json::Value) -> String {
    let Some(content) = record.pointer("/message/content") else {
        return String::new();
    };
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|c| {
                if c.get("type").and_then(|v| v.as_str()) == Some("text") {
                    c.get("text").and_then(|v| v.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn codex_message_text(record: &serde_json::Value) -> Option<String> {
    if record.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
        return None;
    }
    let payload = record.get("payload")?;
    match payload.get("type").and_then(|v| v.as_str()) {
        Some("user_message") | Some("agent_message") => payload
            .get("message")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    }
}

fn canonical_path_string(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn codex_session_index() -> Result<HashMap<String, String>, String> {
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    let path = home.join(".codex").join("session_index.jsonl");
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e.to_string()),
    };
    let reader = BufReader::new(file);
    let mut titles = HashMap::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = record.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(title) = record.get("thread_name").and_then(|v| v.as_str()) else {
            continue;
        };
        titles.insert(id.to_string(), title.to_string());
    }
    Ok(titles)
}

fn collect_codex_session_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_codex_session_paths(&path, paths)?;
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            paths.push(path);
        }
    }
    Ok(())
}

fn codex_session_meta(path: &Path) -> std::io::Result<Option<(String, String)>> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    for line in reader.lines().take(20) {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if record.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
            continue;
        }
        let Some(payload) = record.get("payload") else {
            continue;
        };
        let Some(id) = payload.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(cwd) = payload.get("cwd").and_then(|v| v.as_str()) else {
            continue;
        };
        return Ok(Some((id.to_string(), cwd.to_string())));
    }
    Ok(None)
}

fn codex_session_title(path: &Path) -> std::io::Result<Option<String>> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if record.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
            continue;
        }
        let Some(payload) = record.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(|v| v.as_str()) != Some("user_message") {
            continue;
        }
        let Some(message) = payload.get("message").and_then(|v| v.as_str()) else {
            continue;
        };
        return Ok(Some(message.chars().take(120).collect()));
    }
    Ok(None)
}

fn find_snippets(text: &str, q_lower: &str, max_count: usize) -> Vec<String> {
    let half: usize = 40;
    let text_lower = text.to_lowercase();
    let mut snippets: Vec<String> = Vec::new();
    let mut search_start: usize = 0;
    while snippets.len() < max_count && search_start < text.len() {
        let Some(rel) = text_lower[search_start..].find(q_lower) else {
            break;
        };
        let abs = search_start + rel;
        let mut s_start = abs.saturating_sub(half);
        while s_start < text.len() && !text.is_char_boundary(s_start) {
            s_start += 1;
        }
        let mut s_end = (abs + q_lower.len() + half).min(text.len());
        while s_end > 0 && !text.is_char_boundary(s_end) {
            s_end -= 1;
        }
        if s_start >= s_end {
            break;
        }
        let snippet: String = text[s_start..s_end]
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        snippets.push(snippet);
        search_start = abs + q_lower.len();
    }
    snippets
}

fn discover_claude_sessions<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<Vec<SessionRecord>, String> {
    let dir = claude_sessions_dir(app)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };

    let mut sessions = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let title = claude_session_title(&path).ok().flatten();
        sessions.push(SessionRecord {
            provider: SessionProvider::Claude,
            id,
            path,
            mtime,
            size: metadata.len(),
            title,
        });
    }
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    Ok(sessions)
}

fn discover_codex_sessions<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<Vec<SessionRecord>, String> {
    let project = project_root(Some(app)).ok_or("could not resolve project root")?;
    let project_cwd = canonical_path_string(&project);
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    let sessions_root = home.join(".codex").join("sessions");
    let titles = codex_session_index()?;
    let mut paths = Vec::new();
    collect_codex_session_paths(&sessions_root, &mut paths)?;

    let mut sessions = Vec::new();
    for path in paths {
        let Some((id, cwd)) = codex_session_meta(&path).map_err(|e| e.to_string())? else {
            continue;
        };
        if canonical_path_string(Path::new(&cwd)) != project_cwd {
            continue;
        }
        let metadata = path.metadata().map_err(|e| e.to_string())?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let title = titles
            .get(&id)
            .cloned()
            .or_else(|| codex_session_title(&path).ok().flatten());
        sessions.push(SessionRecord {
            provider: SessionProvider::Codex,
            id: id.clone(),
            path,
            mtime,
            size: metadata.len(),
            title,
        });
    }
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    Ok(sessions)
}

fn choose_session_provider(
    preferred: Option<SessionProvider>,
    claude: &[SessionRecord],
    codex: &[SessionRecord],
) -> SessionProvider {
    if let Some(provider) = preferred {
        return provider;
    }
    match (codex.first(), claude.first()) {
        (Some(codex_latest), Some(claude_latest)) if codex_latest.mtime >= claude_latest.mtime => {
            SessionProvider::Codex
        }
        (Some(_), None) => SessionProvider::Codex,
        _ => SessionProvider::Claude,
    }
}

fn sessions_for_provider<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<(SessionProvider, Vec<SessionRecord>), String> {
    let claude = discover_claude_sessions(app)?;
    let codex = discover_codex_sessions(app)?;
    let preferred = preferred.or_else(|| hinted_session_provider(app));
    let provider = choose_session_provider(preferred, &claude, &codex);
    let sessions = match provider {
        SessionProvider::Claude => claude,
        SessionProvider::Codex => codex,
    };
    Ok((provider, sessions))
}

fn session_meta<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<SessionsMeta, String> {
    let (provider, sessions) = sessions_for_provider(app, preferred)?;
    Ok(SessionsMeta {
        provider,
        count: sessions.len(),
        current_id: sessions.first().map(|s| s.id.clone()),
    })
}

fn search_sessions<R: tauri::Runtime>(
    app: &AppHandle<R>,
    query: &str,
    limit: usize,
    preferred: Option<SessionProvider>,
) -> Result<Vec<serde_json::Value>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let q_lower = q.to_lowercase();
    let (provider, mut sessions) = sessions_for_provider(app, preferred)?;
    sessions.truncate(limit);

    let mut results: Vec<serde_json::Value> = Vec::new();
    for session in sessions {
        let Ok(content) = std::fs::read_to_string(&session.path) else {
            continue;
        };
        let mut all_text = String::new();
        for line in content.lines() {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let text = match provider {
                SessionProvider::Claude => {
                    let role = record.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if role != "user" && role != "assistant" {
                        continue;
                    }
                    claude_message_text(&record)
                }
                SessionProvider::Codex => match codex_message_text(&record) {
                    Some(text) => text,
                    None => continue,
                },
            };
            if !text.is_empty() {
                all_text.push_str(&text);
                all_text.push('\n');
            }
        }
        let snippets = find_snippets(&all_text, &q_lower, 3);
        if !snippets.is_empty() {
            results.push(serde_json::json!({
                "id": session.id,
                "title": session.title,
                "mtime": session.mtime,
                "size": session.size,
                "provider": session.provider,
                "current": false,
                "snippets": snippets,
            }));
        }
    }
    Ok(results)
}

fn list_sessions<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Vec<SessionEntry>, String> {
    let (provider, sessions) = sessions_for_provider(app, preferred)?;
    // For Claude sessions, mark the live one using the same hysteresis-
    // backed picker that the Transcript pane uses. For Codex (or anything else),
    // fall back to "newest mtime" via idx == 0.
    let live_claude_id: Option<String> = match provider {
        SessionProvider::Claude => latest_claude_session_path(app)
            .ok()
            .flatten()
            .and_then(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            }),
        _ => None,
    };
    Ok(sessions
        .into_iter()
        .enumerate()
        .map(|(idx, session)| {
            let current = match &live_claude_id {
                Some(live_id) => session.id == *live_id,
                None => idx == 0,
            };
            SessionEntry {
                id: session.id,
                mtime: session.mtime,
                size: session.size,
                title: session.title,
                provider: session.provider,
                current,
            }
        })
        .collect())
}

fn read_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    id: &str,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("invalid session id".to_string());
    }
    let (_, sessions) = sessions_for_provider(app, preferred)?;
    let session = sessions
        .into_iter()
        .find(|session| session.id == id)
        .ok_or("session not found")?;
    std::fs::read(&session.path).map_err(|e| e.to_string())
}

// Remove a session's JSONL file. Validates the id is a safe filename
// (alphanumeric + hyphen, same as read_session) and resolves the path
// via sessions_for_provider so we never touch anything outside the
// session dirs.
fn delete_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    id: &str,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("invalid session id".to_string());
    }
    let (_, sessions) = sessions_for_provider(app, preferred)?;
    let session = sessions
        .into_iter()
        .find(|session| session.id == id)
        .ok_or("session not found")?;
    std::fs::remove_file(&session.path).map_err(|e| e.to_string())?;
    Ok(b"{\"ok\":true}".to_vec())
}

// Format `SystemTime::now()` as an RFC3339 string in UTC (seconds precision,
// no subseconds). Used for the `updated_at` field codex writes into
// session_index.jsonl entries. xmlui-desktop has no date-formatting crate
// dependency; this inline implementation uses Howard Hinnant's gregorian
// algorithm to avoid adding one. Codex does not parse `updated_at` back
// (only `id` + `thread_name` are read), but writing a real RFC3339 keeps the
// file compatible with codex's own writers.
fn rfc3339_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let secs_in_day = secs.rem_euclid(86400);
    let hour = (secs_in_day / 3600) as u32;
    let minute = ((secs_in_day / 60) % 60) as u32;
    let second = (secs_in_day % 60) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month as u32, day, hour, minute, second
    )
}

// Rename a session. For Claude: append `{type: "custom-title", customTitle: ...}`
// to the session JSONL so claude_session_title at lib.rs:1893 picks it up on
// next read. For codex: append `{id, thread_name, updated_at}` to
// ~/.codex/session_index.jsonl (append-only, last entry wins) so both
// codex_session_index in xmlui-desktop and codex's own session listing see the
// new title. Codex contract verified against codex-rs/rollout/src/session_index.rs.
fn rename_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    id: &str,
    preferred: Option<SessionProvider>,
    title: &str,
) -> Result<Vec<u8>, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("invalid session id".to_string());
    }
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("empty title".to_string());
    }
    let (provider, sessions) = sessions_for_provider(app, preferred)?;
    let session = sessions
        .into_iter()
        .find(|session| session.id == id)
        .ok_or("session not found")?;
    use std::io::Write;
    match provider {
        SessionProvider::Claude => {
            let record = serde_json::json!({ "type": "custom-title", "customTitle": trimmed });
            let mut line = serde_json::to_string(&record).map_err(|e| e.to_string())?;
            line.push('\n');
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&session.path)
                .map_err(|e| e.to_string())?;
            f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        }
        SessionProvider::Codex => {
            let home = home_dir().ok_or("no HOME or USERPROFILE")?;
            let index_path = home.join(".codex").join("session_index.jsonl");
            if let Some(parent) = index_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let record = serde_json::json!({
                "id": session.id,
                "thread_name": trimmed,
                "updated_at": rfc3339_now(),
            });
            let mut line = serde_json::to_string(&record).map_err(|e| e.to_string())?;
            line.push('\n');
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&index_path)
                .map_err(|e| e.to_string())?;
            f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        }
    }
    Ok(b"{\"ok\":true}".to_vec())
}

fn read_latest_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    let Some(path) = latest_session_path(app, preferred)? else {
        return Ok(Vec::new());
    };
    std::fs::read(&path).map_err(|e| e.to_string())
}

// Fast lookup for the most-recent Claude session JSONL by mtime alone.
// Avoids `discover_claude_sessions` which reads each file to extract a
// title — fine for the Sessions tab but catastrophic to call on every
// pending-poll (60+ sessions × multi-MB each = ~1GB of disk I/O per
// call). Latest-* endpoints only need the path; titles are unused.
fn latest_claude_session_path<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<std::path::PathBuf>, String> {
    let dir = claude_sessions_dir(app)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    // Collect every jsonl with its mtime in one pass.
    let mut all: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(mtime) = metadata.modified() else {
            continue;
        };
        all.push((mtime, path));
    }
    if all.is_empty() {
        return Ok(None);
    }
    // Raw best = newest mtime.
    let (raw_best_mtime, raw_best_path) = all
        .iter()
        .max_by_key(|(t, _)| *t)
        .cloned()
        .expect("non-empty");
    // Hysteresis: prefer the previously-chosen path unless it's clearly stale.
    let cache_cell = LIVE_CLAUDE_SESSION.get_or_init(|| Mutex::new(None));
    let mut cached = cache_cell.lock().map_err(|e| e.to_string())?;
    let chosen: PathBuf = match cached.as_ref() {
        Some((cached_path, _)) => {
            // Look up the cached path's current mtime in our scan.
            let cached_now = all.iter().find(|(_, p)| p == cached_path).map(|(t, _)| *t);
            match cached_now {
                None => {
                    // (a) cached path no longer exists → switch.
                    raw_best_path.clone()
                }
                Some(cached_mtime) => {
                    let now = std::time::SystemTime::now();
                    let cached_age = now
                        .duration_since(cached_mtime)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                    let raw_age = now
                        .duration_since(raw_best_mtime)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                    let different = &raw_best_path != cached_path;
                    let cond_b = cached_age > std::time::Duration::from_secs(30)
                        && raw_best_mtime > cached_mtime;
                    let cond_c = raw_age < std::time::Duration::from_secs(5)
                        && cached_age > std::time::Duration::from_secs(5);
                    if different && (cond_b || cond_c) {
                        raw_best_path.clone()
                    } else {
                        cached_path.clone()
                    }
                }
            }
        }
        None => raw_best_path.clone(),
    };
    // Record current mtime for the chosen path (could be raw_best or cached).
    let chosen_mtime = all
        .iter()
        .find(|(_, p)| p == &chosen)
        .map(|(t, _)| *t)
        .unwrap_or(raw_best_mtime);
    *cached = Some((chosen.clone(), chosen_mtime));
    Ok(Some(chosen))
}

fn latest_codex_session_path<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<std::path::PathBuf>, String> {
    let project = project_root(Some(app)).ok_or("could not resolve project root")?;
    let project_cwd = canonical_path_string(&project);
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    let sessions_root = home.join(".codex").join("sessions");
    let mut paths = Vec::new();
    collect_codex_session_paths(&sessions_root, &mut paths)?;
    let mut all: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for path in paths {
        let Some((_, cwd)) = codex_session_meta(&path).map_err(|e| e.to_string())? else {
            continue;
        };
        if canonical_path_string(Path::new(&cwd)) != project_cwd {
            continue;
        }
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        let Ok(mtime) = metadata.modified() else {
            continue;
        };
        all.push((mtime, path));
    }
    if all.is_empty() {
        return Ok(None);
    }
    let (raw_best_mtime, raw_best_path) = all
        .iter()
        .max_by_key(|(t, _)| *t)
        .cloned()
        .expect("non-empty");
    let cache_cell = LIVE_CODEX_SESSION.get_or_init(|| Mutex::new(None));
    let mut cached = cache_cell.lock().map_err(|e| e.to_string())?;
    let chosen: PathBuf = match cached.as_ref() {
        Some((cached_path, _)) => {
            let cached_now = all.iter().find(|(_, p)| p == cached_path).map(|(t, _)| *t);
            match cached_now {
                None => raw_best_path.clone(),
                Some(cached_mtime) => {
                    let now = std::time::SystemTime::now();
                    let cached_age = now
                        .duration_since(cached_mtime)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                    let raw_age = now
                        .duration_since(raw_best_mtime)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                    let different = &raw_best_path != cached_path;
                    let cond_b = cached_age > std::time::Duration::from_secs(30)
                        && raw_best_mtime > cached_mtime;
                    let cond_c = raw_age < std::time::Duration::from_secs(5)
                        && cached_age > std::time::Duration::from_secs(5);
                    if different && (cond_b || cond_c) {
                        raw_best_path.clone()
                    } else {
                        cached_path.clone()
                    }
                }
            }
        }
        None => raw_best_path.clone(),
    };
    let chosen_mtime = all
        .iter()
        .find(|(_, p)| p == &chosen)
        .map(|(t, _)| *t)
        .unwrap_or(raw_best_mtime);
    *cached = Some((chosen.clone(), chosen_mtime));
    Ok(Some(chosen))
}

fn latest_session_path<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Option<std::path::PathBuf>, String> {
    let hinted = preferred.or_else(|| hinted_session_provider(app));
    let Some(provider) = hinted else {
        return Ok(None);
    };
    let path = match provider {
        SessionProvider::Claude => latest_claude_session_path(app)?,
        SessionProvider::Codex => latest_codex_session_path(app)?,
    };
    let Some(path) = path else {
        return Ok(None);
    };
    let Some(hint_mtime) = active_agent_hint_mtime(app) else {
        return Ok(None);
    };
    let session_mtime = match std::fs::metadata(&path).and_then(|md| md.modified()) {
        Ok(mtime) => mtime,
        Err(_) => return Ok(None),
    };
    if session_mtime < hint_mtime {
        return Ok(None);
    }
    Ok(Some(path))
}

// Tail variant: return only the last N records of the JSONL. Lets Transcript
// poll aggressively without round-tripping the entire (multi-MB) file.
// Uses a seek-from-EOF, read-backward-in-chunks loop so server cost is
// proportional to N, not file size.
fn read_latest_session_tail<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
    lines: usize,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let Some(path) = latest_session_path(app, preferred)? else {
        return Ok(Vec::new());
    };
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    if file_size == 0 || lines == 0 {
        return Ok(Vec::new());
    }
    // Need `lines + 1` newlines walking back from EOF to delimit `lines`
    // records (the +1 accounts for the trailing newline of the previous
    // record). If the file has fewer newlines we just start at offset 0.
    let target_newlines = lines + 1;
    let chunk_size: u64 = 64 * 1024;
    let mut buf = vec![0u8; chunk_size as usize];
    let mut pos: u64 = file_size;
    let mut newlines_seen: usize = 0;
    let mut start_offset: u64 = 0;
    while pos > 0 {
        let read_size = chunk_size.min(pos);
        pos -= read_size;
        file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
        file.read_exact(&mut buf[..read_size as usize])
            .map_err(|e| e.to_string())?;
        let mut done = false;
        for i in (0..read_size as usize).rev() {
            if buf[i] == b'\n' {
                newlines_seen += 1;
                if newlines_seen >= target_newlines {
                    start_offset = pos + i as u64 + 1;
                    done = true;
                    break;
                }
            }
        }
        if done {
            break;
        }
    }
    file.seek(SeekFrom::Start(start_offset))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity((file_size - start_offset) as usize);
    file.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

// Detect whether the latest session has a pending tool_use awaiting
// permission. Returns JSON describing the tool, or `{"pending":null}`
// when not pending. Reads ~32KB of the file's tail so the walk-back
// can find a complete most-recent record even when it contains a
// large Edit/MultiEdit tool_use (10-15KB is not unusual). DO NOT
// shrink below ~16KB: text.lines().rev() needs the start of the
// latest record to be in the buffer, otherwise the leading partial
// line fails JSON parse and the walk lands on an older record —
// producing false negatives on the menu.
fn read_latest_session_pending<R: tauri::Runtime>(
    app: &AppHandle<R>,
    _preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let _start = std::time::Instant::now();
    let Some(path) = latest_claude_session_path(app)? else {
        return Ok(br#"{"pending":null}"#.to_vec());
    };
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    let want: u64 = 32 * 1024;
    let read_from = file_size.saturating_sub(want);
    file.seek(SeekFrom::Start(read_from))
        .map_err(|e| e.to_string())?;
    let mut tail = Vec::with_capacity((file_size - read_from) as usize);
    file.read_to_end(&mut tail).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&tail);
    // Walk newest-first. Collect tool_result.tool_use_id values from user
    // records into `resolved`, then when we find an assistant record with
    // tool_use blocks, return the FIRST unresolved one. This handles the
    // multi-tool-batch case: claude proposes A+B in one turn, user approves
    // A, tool_result A is the latest user record but B is still pending.
    let mut pending: Option<serde_json::Value> = None;
    let mut resolved: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_break_reason: &'static str = "no-record";
    for line in text.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(r) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let typ = r.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if typ != "assistant" && typ != "user" {
            continue;
        }
        let content = r
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array());
        if typ == "user" {
            // Collect tool_result.tool_use_id from this user record and
            // keep walking back. A user record with NO tool_result content
            // (a genuine user message) means there's no pending tool
            // because the user has already responded — break.
            let Some(arr) = content else {
                last_break_reason = "user-no-content";
                break;
            };
            let mut had_tool_result = false;
            for c in arr {
                if c.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    had_tool_result = true;
                    if let Some(id) = c.get("tool_use_id").and_then(|v| v.as_str()) {
                        resolved.insert(id.to_string());
                    }
                }
            }
            if !had_tool_result {
                last_break_reason = "user-no-tool-result";
                break;
            }
            continue;
        }
        if content.is_none() {
            last_break_reason = "assistant-no-content";
            break;
        }
        let arr = content.unwrap();
        let has_tool_use = arr
            .iter()
            .any(|c| c.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
        if !has_tool_use {
            last_break_reason = "assistant-no-tool-use";
            break;
        }
        // Return the first tool_use whose id is NOT in `resolved`.
        for c in arr {
            if c.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if resolved.contains(id) {
                continue;
            }
            pending = Some(c.clone());
            break;
        }
        last_break_reason = if pending.is_some() {
            "tool-use-found"
        } else {
            "all-tools-resolved"
        };
        break;
    }
    // Always log the outcome (cheap; helps diagnose missing-menu reports).
    let tool_name = pending
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    eprintln!(
        "[pending-endpoint] reason={} tool={} tail_bytes={}",
        last_break_reason,
        tool_name,
        file_size - read_from,
    );
    let body = serde_json::json!({ "pending": pending });
    let result = serde_json::to_vec(&body).map_err(|e| e.to_string());
    let elapsed = _start.elapsed();
    if elapsed > std::time::Duration::from_millis(10) {
        eprintln!(
            "[pending-endpoint] slow: {}ms (file_size={}, tail_read={})",
            elapsed.as_millis(),
            file_size,
            file_size - read_from
        );
    }
    result
}

// Host-side last-assistant-text extraction. Architectural experiment
// for the "derive-at-the-boundary, not in the iframe" thread: the
// JSONL helper `lastAssistantText` was costing 250-300ms per fanout
// on the iframe main thread. This route does the same walk in Rust
// once per request and returns just the text, so the iframe binds to
// a small value instead of re-walking 100 KB+ every broadcast.
//
// Reads the trailing 32 KB of the current Claude session JSONL (same
// budget as `read_latest_session_pending`), walks lines in reverse,
// and returns the most recent assistant text turn. Returns empty
// string when no assistant text is found in the tail window.
fn read_last_assistant_text<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    // latest_session_path falls back to hinted_session_provider when
    // preferred is None, so the same route serves whichever agent
    // (Claude or Codex) most recently spoke.
    let Some(path) = latest_session_path(app, preferred)? else {
        return Ok(br#"{"text":""}"#.to_vec());
    };
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    let want: u64 = 32 * 1024;
    let read_from = file_size.saturating_sub(want);
    file.seek(SeekFrom::Start(read_from))
        .map_err(|e| e.to_string())?;
    let mut tail = Vec::with_capacity((file_size - read_from) as usize);
    file.read_to_end(&mut tail).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&tail);
    let mut found = String::new();
    for line in text.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(r) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let typ = r.get("type").and_then(|v| v.as_str()).unwrap_or("");
        // Claude shape: type="assistant", message.content is string or
        // array-of-content-blocks (text blocks have type="text" + text).
        if typ == "assistant" {
            let content = match r.get("message").and_then(|m| m.get("content")) {
                Some(c) => c,
                None => continue,
            };
            if let Some(s) = content.as_str() {
                if !s.is_empty() {
                    found = s.to_string();
                    break;
                }
            }
            if let Some(arr) = content.as_array() {
                let mut texts: Vec<String> = Vec::new();
                for c in arr {
                    if c.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
                            if !t.is_empty() {
                                texts.push(t.to_string());
                            }
                        }
                    }
                }
                if !texts.is_empty() {
                    found = texts.join("\n\n");
                    break;
                }
            }
            continue;
        }
        // Codex shape: type="event_msg" with payload.type="agent_message"
        // and payload.message carrying the text.
        if typ == "event_msg" {
            if let Some(payload) = r.get("payload") {
                if payload.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                    if let Some(msg) = payload.get("message").and_then(|v| v.as_str()) {
                        if !msg.is_empty() {
                            found = msg.to_string();
                            break;
                        }
                    }
                }
            }
            continue;
        }
    }
    // Note: we deliberately skip the iframe's stripImagePaths /
    // rewriteXmluiDocUrls transforms here. They're cheap on a 2 KB
    // string and can stay iframe-side if needed; the responsiveness
    // win comes from not walking the buffer, not from those passes.
    let body = serde_json::json!({ "text": found });
    serde_json::to_vec(&body).map_err(|e| e.to_string())
}

// Host-side current-turn edits extraction. Mirrors the iframe helper
// `currentTurnEdits(jsonlText)` in Globals.xs: walks backward to find
// the most recent user-message boundary, then collects per-file
// aggregates (kind, added/removed line counts, lastToolId) from
// Claude tool_use blocks and Codex apply_patch payloads after that
// boundary.
//
// Returns a JSON array of {filePath, kind, added, removed, lastToolId}
// in first-touch order. Empty array when there are no edits in the
// current turn or no session.
fn read_current_turn_edits<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let Some(path) = latest_session_path(app, preferred)? else {
        return Ok(b"[]".to_vec());
    };
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    // 64 KB tail covers ~100 records comfortably even with Codex's
    // verbose apply_patch payloads. Bigger than read_last_assistant_text's
    // 32 KB because patch records can be heavy.
    let want: u64 = 64 * 1024;
    let read_from = file_size.saturating_sub(want);
    file.seek(SeekFrom::Start(read_from))
        .map_err(|e| e.to_string())?;
    let mut tail = Vec::with_capacity((file_size - read_from) as usize);
    file.read_to_end(&mut tail).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&tail);
    let lines: Vec<&str> = text.lines().collect();

    // Walk backward to the most recent user-message boundary.
    // tool_result-only Claude user records don't count as the boundary
    // (they're tool outputs, not actual user messages).
    let mut last_user_idx: Option<usize> = None;
    for i in (0..lines.len()).rev() {
        let line = lines[i].trim();
        if line.is_empty() {
            continue;
        }
        let Ok(r) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let typ = r.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if typ == "user" {
            if let Some(arr) = r
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                let all_tool_result = !arr.is_empty()
                    && arr.iter().all(|c| {
                        c.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                    });
                if all_tool_result {
                    continue;
                }
            }
            last_user_idx = Some(i);
            break;
        }
        if typ == "event_msg" {
            if let Some(p) = r.get("payload") {
                if p.get("type").and_then(|v| v.as_str()) == Some("user_message") {
                    last_user_idx = Some(i);
                    break;
                }
            }
        }
    }

    struct Bucket {
        kind: Option<&'static str>,
        added: u64,
        removed: u64,
        last_tool_id: Option<String>,
    }
    let mut by_file: std::collections::HashMap<String, Bucket> =
        std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();

    fn merge_kind(prev: Option<&'static str>, new_kind: &'static str) -> &'static str {
        match prev {
            None => new_kind,
            Some(p) if p == new_kind => p,
            _ => "mixed",
        }
    }

    fn ensure<'a>(
        by_file: &'a mut std::collections::HashMap<String, Bucket>,
        order: &mut Vec<String>,
        path: &str,
    ) -> &'a mut Bucket {
        if !by_file.contains_key(path) {
            by_file.insert(
                path.to_string(),
                Bucket {
                    kind: None,
                    added: 0,
                    removed: 0,
                    last_tool_id: None,
                },
            );
            order.push(path.to_string());
        }
        by_file.get_mut(path).unwrap()
    }

    let start = last_user_idx.map(|i| i + 1).unwrap_or(0);
    for i in start..lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            continue;
        }
        let Ok(r) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let typ = r.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Codex: response_item with function_call/custom_tool_call,
        // name == apply_patch, arguments carrying a patch text.
        if typ == "response_item" {
            let Some(p) = r.get("payload") else { continue };
            let ptype = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ptype != "function_call" && ptype != "custom_tool_call" {
                continue;
            }
            if p.get("name").and_then(|v| v.as_str()) != Some("apply_patch") {
                continue;
            }
            // arguments is typically a JSON string {"input": "<patch text>"}.
            // Fall back to direct field access if the shape differs.
            let patch_text: String = match p.get("arguments") {
                Some(serde_json::Value::String(s)) => serde_json::from_str::<serde_json::Value>(s)
                    .ok()
                    .and_then(|v| v.get("input").and_then(|i| i.as_str()).map(String::from))
                    .unwrap_or_default(),
                Some(v) => v
                    .get("input")
                    .and_then(|i| i.as_str())
                    .map(String::from)
                    .unwrap_or_default(),
                None => String::new(),
            };
            if patch_text.is_empty() {
                continue;
            }
            let call_id = p.get("call_id").and_then(|v| v.as_str()).map(String::from);
            let mut current_path: Option<String> = None;
            for pl in patch_text.lines() {
                if let Some(rest) = pl.strip_prefix("*** ") {
                    let mut new_kind: Option<&'static str> = None;
                    let mut new_path: Option<String> = None;
                    if let Some(p) = rest.strip_prefix("Add File: ") {
                        new_kind = Some("added");
                        new_path = Some(p.trim().to_string());
                    } else if let Some(p) = rest.strip_prefix("Update File: ") {
                        new_kind = Some("updated");
                        new_path = Some(p.trim().to_string());
                    } else if let Some(p) = rest.strip_prefix("Delete File: ") {
                        new_kind = Some("deleted");
                        new_path = Some(p.trim().to_string());
                    }
                    if let (Some(kind), Some(path)) = (new_kind, new_path) {
                        let bucket = ensure(&mut by_file, &mut order, &path);
                        bucket.kind = Some(merge_kind(bucket.kind, kind));
                        if let Some(id) = &call_id {
                            bucket.last_tool_id = Some(id.clone());
                        }
                        current_path = Some(path);
                    } else {
                        current_path = None;
                    }
                    continue;
                }
                let Some(path) = current_path.as_ref() else {
                    continue;
                };
                if pl.starts_with("+++") || pl.starts_with("---") {
                    continue;
                }
                if let Some(bucket) = by_file.get_mut(path) {
                    if pl.starts_with('+') {
                        bucket.added += 1;
                    } else if pl.starts_with('-') {
                        bucket.removed += 1;
                    }
                }
            }
            continue;
        }

        // Claude: assistant.message.content[*] with type=tool_use,
        // name in {Edit, MultiEdit, Write}.
        if typ != "assistant" {
            continue;
        }
        let Some(content_arr) = r
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for c in content_arr {
            if c.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name != "Edit" && name != "MultiEdit" && name != "Write" {
                continue;
            }
            let Some(input) = c.get("input") else {
                continue;
            };
            let file_path = match input.get("file_path").and_then(|p| p.as_str()) {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => continue,
            };
            let call_id = c.get("id").and_then(|v| v.as_str()).map(String::from);
            let bucket = ensure(&mut by_file, &mut order, &file_path);
            if let Some(id) = &call_id {
                bucket.last_tool_id = Some(id.clone());
            }
            match name {
                "Edit" => {
                    let before = input
                        .get("old_string")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let after = input
                        .get("new_string")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !before.is_empty() {
                        bucket.removed += before.lines().count() as u64;
                    }
                    if !after.is_empty() {
                        bucket.added += after.lines().count() as u64;
                    }
                    bucket.kind = Some(merge_kind(bucket.kind, "edited"));
                }
                "MultiEdit" => {
                    if let Some(edits) = input.get("edits").and_then(|v| v.as_array()) {
                        for e in edits {
                            let before = e
                                .get("old_string")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let after = e
                                .get("new_string")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !before.is_empty() {
                                bucket.removed += before.lines().count() as u64;
                            }
                            if !after.is_empty() {
                                bucket.added += after.lines().count() as u64;
                            }
                        }
                    }
                    bucket.kind = Some(merge_kind(bucket.kind, "multi-edited"));
                }
                "Write" => {
                    let after = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if !after.is_empty() {
                        bucket.added += after.lines().count() as u64;
                    }
                    bucket.kind = Some(merge_kind(bucket.kind, "written"));
                }
                _ => {}
            }
        }
    }

    let result: Vec<serde_json::Value> = order
        .iter()
        .filter_map(|fp| {
            by_file.remove(fp).map(|b| {
                serde_json::json!({
                    "filePath": fp,
                    "kind": b.kind,
                    "added": b.added,
                    "removed": b.removed,
                    "lastToolId": b.last_tool_id,
                })
            })
        })
        .collect();
    serde_json::to_vec(&result).map_err(|e| e.to_string())
}

// Cheap variant for polling: just the file size + mtime. Lets Transcript
// detect changes without re-fetching the full (multi-MB) JSONL each
// tick. The frontend then bumps a cache-busting param to trigger a
// real fetch only when size has changed.
fn read_latest_session_meta<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    let Some(path) = latest_session_path(app, preferred)? else {
        return Ok(b"null".to_vec());
    };
    let md = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let body = format!(r#"{{"size":{},"mtime":{},"now":{}}}"#, md.len(), mtime, now);
    Ok(body.into_bytes())
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        let unreserved =
            c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' || c == b'~';
        if unreserved {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{:02X}", c));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        // Tauri's bundled-frontend protocol stamps unknown extensions as text/html,
        // which xmlui-standalone rejects. Serve them as text/plain instead.
        Some("xmlui") | Some("xs") => "text/plain; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// Markers used to identify the xmlui-desktop block inside a project's
// CLAUDE.md. The block contains a Claude Code @-import that pulls in
// the full conventions sidecar; future runs of run_enhance replace
// what's between the markers without disturbing surrounding content.
const ENHANCE_MARKER_START: &str = "<!-- xmlui-desktop:start -->";
const ENHANCE_MARKER_END: &str = "<!-- xmlui-desktop:end -->";
const ENHANCE_SIDECAR_REL: &str = ".claude/xmlui-desktop-conventions.md";
const ENHANCE_CODEX_AGENTS_REL: &str = "AGENTS.md";
const ENHANCE_CODEX_BUNDLE_REL: &str = "shell/codex-startup-instructions.md";
const ENHANCE_HOOK_SCRIPT_REL: &str = ".claude/hooks/worklist-guard.py";
const ENHANCE_SETTINGS_REL: &str = ".claude/settings.json";
const ENHANCE_HOOK_BUNDLE_REL: &str = "__shell/worklist-guard.py";
// Codex's worklist guard runs as a PreToolUse hook in codex's user-global
// config. The bundle ships with Bram and is copied to
// ~/.bram/codex-worklist-guard.py the first time setup runs in any
// project; the hook config registration in ~/.codex/config.toml is identical
// across projects because the script self-detects whether the active cwd is
// Bram-managed (presence of resources/.worklist-authorization.json).
const ENHANCE_CODEX_HOOK_BUNDLE_REL: &str = "shell/worklist-guard-codex.py";
const ENHANCE_CODEX_HOOK_INSTALL_REL: &str = ".bram/codex-worklist-guard.py";
const ENHANCE_CODEX_TRUST_ACK_REL: &str = ".bram/codex-trust-ack";
const ENHANCE_CODEX_CONFIG_REL: &str = ".codex/config.toml";
// TOML-comment markers delimit the Bram block inside codex's
// config.toml so re-runs can replace it without disturbing surrounding entries.
const ENHANCE_CODEX_TOML_MARKER_START: &str = "# bram:start";
const ENHANCE_CODEX_TOML_MARKER_END: &str = "# bram:end";
const ENHANCE_CODEX_LEGACY_TOML_MARKER_START: &str = "# xmlui-desktop:start";
const ENHANCE_CODEX_LEGACY_TOML_MARKER_END: &str = "# xmlui-desktop:end";
// developer_instructions is a top-level scalar in config.toml. TOML requires
// top-level keys to come BEFORE any [section] table, so this block lives at
// the head of the file in its own marker. Verified via `codex debug
// prompt-input` to land in the developer-role context part between
// permissions_instructions and skills_instructions — a higher-priority slot
// than the user-role AGENTS.md path. That's why this surface carries the gate
// prose now instead of a per-turn UserPromptSubmit injection.
const ENHANCE_CODEX_INSTR_MARKER_START: &str = "# bram-instructions:start";
const ENHANCE_CODEX_INSTR_MARKER_END: &str = "# bram-instructions:end";
const ENHANCE_CODEX_LEGACY_INSTR_MARKER_START: &str = "# xmlui-desktop-instructions:start";
const ENHANCE_CODEX_LEGACY_INSTR_MARKER_END: &str = "# xmlui-desktop-instructions:end";
const ENHANCE_CODEX_TYPO_INSTR_MARKER_END: &str = "# brraminstructions:end";
// Single-source-of-truth gate prose embedded in the Bram binary. Mirrors
// what the original UserPromptSubmit injection emitted, kept in sync with
// the convention in app/__shell/conventions.md.
const ENHANCE_CODEX_GATE_PROSE: &str = "bram worklist gate. \
First response to a change request must be (a) clarify, \
(b) propose items to resources/worklist.json (each with non-empty \
id, file or files, before, and after), or (c) read-only investigation \
prefaced \"I don't yet have enough context to propose\". Mutations \
outside approved items are blocked at runtime by a PreToolUse hook. \
On approved:/drop: turns, GET \
http://localhost:$(cat resources/.bram-port)/__worklist/resolve \
to read verified item content, then POST /__worklist/mutate with \
op:advance (after applying) or op:prune (after a drop or commit). \
On iterate: turns, POST /__iterate/begin as your first action and \
/__iterate/end as your last. Don't edit resources/worklist.json \
directly for state changes — the routes drive the inflight sentinel \
that keeps the Worklist tab UI in sync. \
Use curl --retry-connrefused --retry 3 --retry-delay 1 for these \
loopback calls — Bram restarts briefly drop the port and a fresh \
connection can land in that window. \
Full convention: .claude/xmlui-desktop-conventions.md";
const WORKLIST_AUTH_REL: &str = "resources/.worklist-authorization.json";
// Host-managed inflight sentinel (#84). Written when /__worklist/resolve
// serves an approved or drop record, OR when /__iterate/begin is
// called. Cleared by /__worklist/mutate (advance/prune covering the
// claimed ids) or /__iterate/end. The iframe will derive its spinner
// state from this file once item 3 of the #84 sequence lands; for now
// the sentinel is informational and verifiable via the
// [inflight-sentinel] trace.
const INFLIGHT_CLAIM_REL: &str = "resources/.inflight-claim.json";
// Right-pane pty-intent relay (#86). Append-only JSONL queue persisted
// to disk so right-pane clicks (toShell / toTurn / sendKeys) survive an
// iframe-reload-mid-click. Drained synchronously by queue_pty_intent;
// startup cleanup deletes any stale queue from a prior session.
const PTY_INTENT_REL: &str = "resources/.pty-intent.jsonl";
// On Unix, the bare path runs via the script's `#!/usr/bin/env python3`
// shebang (set executable by run_enhance under #[cfg(unix)]). On Windows
// there's no shebang resolution and no chmod, so we invoke through the
// `py` launcher — it ships with the python.org installer and resolves
// Python via the registry, independent of PATH.
#[cfg(windows)]
const ENHANCE_HOOK_COMMAND: &str = "py -3 \"$CLAUDE_PROJECT_DIR/.claude/hooks/worklist-guard.py\"";
#[cfg(not(windows))]
const ENHANCE_HOOK_COMMAND: &str = "$CLAUDE_PROJECT_DIR/.claude/hooks/worklist-guard.py";
// Presence of this file in the project root means the project IS the
// xmlui-desktop source repo (it bundles the conventions). enhance_status
// treats it as a valid sidecar location; run_enhance skips the parts
// that would otherwise self-overwrite the source.
const ENHANCE_SOURCE_BUNDLE_REL: &str = "app/__shell/conventions.md";

fn settings_has_worklist_guard_hook(settings_path: &Path) -> bool {
    let content = match std::fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    value
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hs| {
                        hs.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|s| s.contains("worklist-guard.py"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

// Remove any PreToolUse hook entries whose `command` contains
// `proposal-guard.py` (the pre-rename script name, see bc3ee31). Hooks
// arrays emptied by the prune are dropped; entries with no remaining
// hooks are removed. Returns Ok(true) if anything was changed, Ok(false)
// if there was nothing to prune (or the file/JSON shape made it a no-op).
fn prune_proposal_guard_from_settings(settings_path: &Path) -> Result<bool, String> {
    let existing = match std::fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    if existing.trim().is_empty() {
        return Ok(false);
    }
    let mut value: serde_json::Value = serde_json::from_str(&existing)
        .map_err(|e| format!("parse {}: {}", settings_path.display(), e))?;
    let Some(pre_arr) = value
        .get_mut("hooks")
        .and_then(|h| h.get_mut("PreToolUse"))
        .and_then(|p| p.as_array_mut())
    else {
        return Ok(false);
    };
    let mut changed = false;
    pre_arr.retain_mut(|entry| {
        let Some(hooks_arr) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
            return true;
        };
        let before = hooks_arr.len();
        hooks_arr.retain(|h| {
            !h.get("command")
                .and_then(|c| c.as_str())
                .map(|s| s.contains("proposal-guard.py"))
                .unwrap_or(false)
        });
        if hooks_arr.len() != before {
            changed = true;
        }
        !hooks_arr.is_empty()
    });
    if !changed {
        return Ok(false);
    }
    let serialized = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("serialize settings.json: {}", e))?;
    std::fs::write(settings_path, format!("{}\n", serialized))
        .map_err(|e| format!("write {}: {}", settings_path.display(), e))?;
    Ok(true)
}

// Ensure settings.json contains exactly one PreToolUse hook entry whose
// command matches ENHANCE_HOOK_COMMAND for this platform, preserving
// other keys. Existing worklist-guard.py entries with a different
// command string (e.g. the pre-cfg-windows bare path on a Windows
// project that was set up before the py-launcher migration) are
// removed and the correct entry is appended. Returns Ok(true) if any
// change was made, Ok(false) if the settings already had the exact
// entry and nothing needed migrating.
fn merge_worklist_guard_into_settings(settings_path: &Path) -> Result<bool, String> {
    let existing = std::fs::read_to_string(settings_path).unwrap_or_default();
    let mut value: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&existing)
            .map_err(|e| format!("parse {}: {}", settings_path.display(), e))?
    };
    if !value.is_object() {
        return Err(format!(
            "{} root is not a JSON object",
            settings_path.display()
        ));
    }
    let root = value.as_object_mut().unwrap();
    let hooks = root
        .entry("hooks".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        return Err(format!(
            "{}: hooks is not a JSON object",
            settings_path.display()
        ));
    }
    let hooks_obj = hooks.as_object_mut().unwrap();
    let pre = hooks_obj
        .entry("PreToolUse".to_string())
        .or_insert_with(|| serde_json::json!([]));
    if !pre.is_array() {
        return Err(format!(
            "{}: hooks.PreToolUse is not a JSON array",
            settings_path.display()
        ));
    }
    let pre_arr = pre.as_array_mut().unwrap();

    // Drop worklist-guard.py entries whose command differs from the
    // current platform's ENHANCE_HOOK_COMMAND. Migrates an existing
    // bare-path Windows install to `py -3 ...` (and would also handle
    // the reverse if a project moved between platforms).
    let mut migrated = false;
    pre_arr.retain_mut(|entry| {
        let Some(hooks_arr) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
            return true;
        };
        let before = hooks_arr.len();
        hooks_arr.retain(|h| {
            let Some(cmd) = h.get("command").and_then(|c| c.as_str()) else {
                return true;
            };
            !(cmd.contains("worklist-guard.py") && cmd != ENHANCE_HOOK_COMMAND)
        });
        if hooks_arr.len() != before {
            migrated = true;
        }
        !hooks_arr.is_empty()
    });

    let exact_present = pre_arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hs| {
                hs.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|s| s == ENHANCE_HOOK_COMMAND)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });
    if exact_present && !migrated {
        return Ok(false);
    }
    if !exact_present {
        pre_arr.push(serde_json::json!({
            "matcher": "Write|Edit",
            "hooks": [{
                "type": "command",
                "command": ENHANCE_HOOK_COMMAND,
            }]
        }));
    }
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    let serialized = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("serialize settings.json: {}", e))?;
    std::fs::write(settings_path, format!("{}\n", serialized))
        .map_err(|e| format!("write {}: {}", settings_path.display(), e))?;
    Ok(true)
}

// Provider-aware Context tab. Claude shows the project-local import chain
// plus Claude-managed memory/hooks/settings. Codex shows the durable local
// Codex-side sources that shape behavior on this machine: config, project
// files, memories, and rules.

struct ContextFile {
    category: &'static str,
    path: PathBuf,
    display: String,
    kind: Option<&'static str>,
}

fn context_provider<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> SessionProvider {
    preferred
        .or_else(|| hinted_session_provider(app))
        .unwrap_or(SessionProvider::Claude)
}

fn collect_claude_context_files<R: tauri::Runtime>(app: &AppHandle<R>) -> Vec<ContextFile> {
    let mut out: Vec<ContextFile> = Vec::new();
    let Some(proj_root) = project_root(Some(app)) else {
        return out;
    };

    let claude_md = proj_root.join("CLAUDE.md");
    if claude_md.exists() {
        out.push(ContextFile {
            category: "project",
            path: claude_md.clone(),
            display: "CLAUDE.md".to_string(),
            kind: Some("claude-md"),
        });
        if let Ok(content) = std::fs::read_to_string(&claude_md) {
            for line in content.lines() {
                let trimmed = line.trim();
                let import_path = match trimmed.strip_prefix('@') {
                    Some(p) if !p.is_empty() && !p.starts_with(char::is_whitespace) => p,
                    _ => continue,
                };
                let abs = proj_root.join(import_path);
                if abs.exists() {
                    out.push(ContextFile {
                        category: "project",
                        path: abs,
                        display: import_path.to_string(),
                        kind: Some("import"),
                    });
                }
            }
        }
    }

    if let Some(home) = home_dir() {
        let memory_dir = home
            .join(".claude")
            .join("projects")
            .join(encode_path_for_filename(&proj_root))
            .join("memory");
        if memory_dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&memory_dir)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            paths.sort();
            for path in paths {
                let display = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                out.push(ContextFile {
                    category: "memory",
                    path,
                    display,
                    kind: None,
                });
            }
        }
    }

    let hooks_dir = proj_root.join(".claude").join("hooks");
    if hooks_dir.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&hooks_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            let display = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            out.push(ContextFile {
                category: "hooks",
                path,
                display,
                kind: None,
            });
        }
    }

    let claude_dir = proj_root.join(".claude");
    for name in ["settings.json", "settings.local.json"] {
        let path = claude_dir.join(name);
        if path.exists() {
            out.push(ContextFile {
                category: "settings",
                path,
                display: name.to_string(),
                kind: None,
            });
        }
    }

    out
}

fn collect_codex_context_files<R: tauri::Runtime>(app: &AppHandle<R>) -> Vec<ContextFile> {
    let mut out: Vec<ContextFile> = Vec::new();
    let Some(proj_root) = project_root(Some(app)) else {
        return out;
    };
    let Some(home) = home_dir() else {
        return out;
    };

    let codex_dir = home.join(".codex");
    let config_toml = codex_dir.join("config.toml");
    let agents_md = proj_root.join("AGENTS.md");
    if agents_md.exists() {
        out.push(ContextFile {
            category: "project",
            path: agents_md,
            display: "AGENTS.md".to_string(),
            kind: Some("codex-agents"),
        });
    }
    if config_toml.exists() {
        out.push(ContextFile {
            category: "project",
            path: config_toml,
            display: "config.toml".to_string(),
            kind: Some("codex-config"),
        });
    }

    let project_dot_codex = proj_root.join(".codex");
    if project_dot_codex.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&project_dot_codex)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            let display = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            out.push(ContextFile {
                category: "project",
                path,
                display,
                kind: Some("project-codex"),
            });
        }
    }

    let memories_dir = codex_dir.join("memories");
    if memories_dir.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&memories_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            let display = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            out.push(ContextFile {
                category: "memory",
                path,
                display,
                kind: None,
            });
        }
    }

    let rules_dir = codex_dir.join("rules");
    if rules_dir.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&rules_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            let display = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            out.push(ContextFile {
                category: "rules",
                path,
                display,
                kind: None,
            });
        }
    }

    out
}

fn collect_context_files<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Vec<ContextFile> {
    match context_provider(app, preferred) {
        SessionProvider::Claude => collect_claude_context_files(app),
        SessionProvider::Codex => collect_codex_context_files(app),
    }
}

// Group the flat ContextFile list into category buckets for the Context tab's
// left-pane list.
fn context_list<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> serde_json::Value {
    use serde_json::json;
    let provider = context_provider(app, preferred);
    let mut project: Vec<serde_json::Value> = Vec::new();
    let mut memory: Vec<serde_json::Value> = Vec::new();
    let mut hooks: Vec<serde_json::Value> = Vec::new();
    let mut settings: Vec<serde_json::Value> = Vec::new();
    let mut rules: Vec<serde_json::Value> = Vec::new();
    for f in collect_context_files(app, Some(provider)) {
        let mut item = json!({
            "path": f.path.to_string_lossy(),
            "display": f.display,
        });
        if let Some(k) = f.kind {
            item["kind"] = json!(k);
        }
        match f.category {
            "project" => project.push(item),
            "memory" => memory.push(item),
            "hooks" => hooks.push(item),
            "settings" => settings.push(item),
            "rules" => rules.push(item),
            _ => {}
        }
    }

    let (provider_name, summary, sections) = match provider {
        SessionProvider::Claude => (
            "claude",
            "Claude Context shows the repo-local `CLAUDE.md` import chain plus Claude-managed memory, hooks, and settings for this project.",
            vec![
                json!({ "key": "project", "label": "Project", "items": project }),
                json!({ "key": "memory", "label": "Memory", "items": memory }),
                json!({ "key": "hooks", "label": "Hooks", "items": hooks }),
                json!({ "key": "settings", "label": "Settings", "items": settings }),
            ],
        ),
        SessionProvider::Codex => (
            "codex",
            "Codex Context shows the repo-local `AGENTS.md` instructions when present plus durable Codex-side sources on this machine, such as `~/.codex/config.toml`, project-local `.codex/` files, memories, and rules.",
            vec![
                json!({ "key": "project", "label": "Project", "items": project }),
                json!({ "key": "memory", "label": "Memories", "items": memory }),
                json!({ "key": "rules", "label": "Rules", "items": rules }),
            ],
        ),
    };
    json!({ "provider": provider_name, "summary": summary, "sections": sections })
}

// Case-insensitive substring search across the same file set as
// context_list. Returns groups of { path, display, category, hits: [{ line,
// snippet }] }. Capped at 50 total hits to keep payloads bounded.
fn context_search<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
    q: &str,
) -> serde_json::Value {
    use serde_json::json;
    let provider = context_provider(app, preferred);
    let needle = q.trim().to_lowercase();
    if needle.is_empty() {
        return json!({
            "provider": match provider {
                SessionProvider::Claude => "claude",
                SessionProvider::Codex => "codex",
            },
            "results": []
        });
    }
    const MAX_HITS: usize = 50;
    let mut total_hits = 0usize;
    let mut results: Vec<serde_json::Value> = Vec::new();
    for file in collect_context_files(app, Some(provider)) {
        if total_hits >= MAX_HITS {
            break;
        }
        let content = match std::fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut hits: Vec<serde_json::Value> = Vec::new();
        for (i, line) in content.lines().enumerate() {
            if total_hits >= MAX_HITS {
                break;
            }
            if line.to_lowercase().contains(&needle) {
                let snippet: String = line.trim().chars().take(200).collect();
                hits.push(json!({
                    "line": i + 1,
                    "snippet": snippet,
                }));
                total_hits += 1;
            }
        }
        if !hits.is_empty() {
            results.push(json!({
                "category": file.category,
                "path": file.path.to_string_lossy(),
                "display": file.display,
                "hits": hits,
            }));
        }
    }
    json!({
        "provider": match provider {
            SessionProvider::Claude => "claude",
            SessionProvider::Codex => "codex",
        },
        "results": results,
        "truncated": total_hits >= MAX_HITS
    })
}

// Compare the on-disk copy of a hook script against the bundled copy
// embedded in this binary. Returns false if the on-disk file is missing,
// unreadable, or differs by even one byte from the bundle. Used by
// enhance_status to flip claude_installed / codex_installed false when a
// previously-set-up project still has a stale hook from an older release —
// without this, the Setup button stays hidden after an upgrade and
// enhance_run never re-fires to overwrite the stale file.
fn hook_matches_bundle<R: tauri::Runtime>(
    app: &AppHandle<R>,
    on_disk: &Path,
    bundle_rel: &str,
) -> bool {
    let Ok(disk_bytes) = std::fs::read(on_disk) else {
        return false;
    };
    let Some((bundle_bytes, _)) = serve_app_file(Some(app), bundle_rel) else {
        return false;
    };
    disk_bytes == bundle_bytes
}

fn enhance_status<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    use serde_json::json;
    let proj = project_root(Some(app)).ok_or("no project root")?;
    let claude_md = proj.join("CLAUDE.md");
    let codex_agents = proj.join(ENHANCE_CODEX_AGENTS_REL);
    let sidecar = proj.join(ENHANCE_SIDECAR_REL);
    let hook_script = proj.join(ENHANCE_HOOK_SCRIPT_REL);
    let settings = proj.join(ENHANCE_SETTINGS_REL);
    let worklist_auth = proj.join(WORKLIST_AUTH_REL);
    let codex_hook_script = home_dir().map(|h| h.join(ENHANCE_CODEX_HOOK_INSTALL_REL));
    let active_provider = hinted_session_provider(app);
    let claude_md_has_marker = std::fs::read_to_string(&claude_md)
        .map(|s| s.contains(ENHANCE_MARKER_START))
        .unwrap_or(false);
    // Source repo treats the bundle itself as the canonical sidecar.
    let sidecar_exists = sidecar.exists() || proj.join(ENHANCE_SOURCE_BUNDLE_REL).exists();
    let hook_script_exists = hook_script.exists();
    let hook_script_current =
        hook_script_exists && hook_matches_bundle(app, &hook_script, ENHANCE_HOOK_BUNDLE_REL);
    let hook_registered = settings_has_worklist_guard_hook(&settings);
    let codex_agents_has_marker = std::fs::read_to_string(&codex_agents)
        .map(|s| s.contains(ENHANCE_MARKER_START))
        .unwrap_or(false);
    let codex_hook_current = codex_hook_script
        .as_ref()
        .map(|p| hook_matches_bundle(app, p, ENHANCE_CODEX_HOOK_BUNDLE_REL))
        .unwrap_or(false);
    let codex_trust_ack = home_dir()
        .and_then(|h| {
            let stored = std::fs::read_to_string(h.join(ENHANCE_CODEX_TRUST_ACK_REL)).ok()?;
            let current = hook_fingerprint(&h.join(ENHANCE_CODEX_HOOK_INSTALL_REL))?;
            Some(stored.trim() == current)
        })
        .unwrap_or(false);
    let core_installed = worklist_auth.exists();
    let claude_installed =
        claude_md_has_marker && sidecar_exists && hook_script_current && hook_registered;
    let codex_installed = core_installed && codex_agents_has_marker && codex_hook_current;
    let claude_needs_setup = !core_installed || !claude_installed;
    let codex_needs_setup = !core_installed || !codex_installed;
    let provider_needs_setup = match active_provider {
        Some(SessionProvider::Claude) => claude_needs_setup,
        Some(SessionProvider::Codex) => codex_needs_setup,
        None => false,
    };
    let active_provider_json = match active_provider {
        Some(SessionProvider::Claude) => json!("claude"),
        Some(SessionProvider::Codex) => json!("codex"),
        None => serde_json::Value::Null,
    };
    let body = serde_json::json!({
        "enhanced": core_installed && claude_installed && codex_installed,
        "activeProvider": active_provider_json,
        "coreInstalled": core_installed,
        "claudeInstalled": claude_installed,
        "codexInstalled": codex_installed,
        "claudeNeedsSetup": claude_needs_setup,
        "codexNeedsSetup": codex_needs_setup,
        "providerNeedsSetup": provider_needs_setup,
        "claudeMd": claude_md_has_marker,
        "codexAgents": codex_agents_has_marker,
        "sidecar": sidecar_exists,
        "hookScript": hook_script_exists,
        "hookScriptCurrent": hook_script_current,
        "codexHookCurrent": codex_hook_current,
        "codexTrustAck": codex_trust_ack,
        "hookRegistered": hook_registered,
        "fallbackMode": "watcher-revert",
        "claudeMdPath": claude_md.display().to_string(),
        "codexAgentsPath": codex_agents.display().to_string(),
        "sidecarPath": sidecar.display().to_string(),
        "hookScriptPath": hook_script.display().to_string(),
        "settingsPath": settings.display().to_string(),
        "worklistAuthPath": worklist_auth.display().to_string(),
    });
    serde_json::to_vec(&body).map_err(|e| e.to_string())
}

fn run_enhance<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    let proj = project_root(Some(app)).ok_or("no project root")?;
    // When running on the source repo, skip writes that would
    // self-overwrite (recreating the deleted local sidecar, reverting
    // the @-import path in CLAUDE.md). Idempotent installs (hook
    // script, settings.json merge) still run.
    let is_source_repo = proj.join(ENHANCE_SOURCE_BUNDLE_REL).exists();

    let mut wrote: Vec<String> = Vec::new();

    // Provider-neutral worklist authorization cache. xmlui-desktop records the
    // latest structured `approved:` / `drop:` payload here so the desktop-side
    // watcher can enforce the two-stage worklist policy even when the active
    // provider has no native pre-tool hook support.
    let worklist_auth_path = proj.join(WORKLIST_AUTH_REL);
    if let Some(parent) = worklist_auth_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    if !worklist_auth_path.exists() {
        let core_stub = WorklistAuthorizationRecord {
            kind: "none".to_string(),
            ids: Vec::new(),
            items: Vec::new(),
            mismatched_ids: Vec::new(),
            issued_at_ms: 0,
            source: "setup".to_string(),
            consumed_at_ms: None,
        };
        let serialized_core = serde_json::to_string_pretty(&core_stub)
            .map_err(|e| format!("serialize worklist authorization stub: {}", e))?;
        std::fs::write(&worklist_auth_path, format!("{}\n", serialized_core))
            .map_err(|e| format!("write {}: {}", worklist_auth_path.display(), e))?;
        wrote.push(worklist_auth_path.display().to_string());
    }

    // Empty worklist.json — created here so setup is the single on-ramp for
    // worklist-driven coordination. init_worklist_file and /__worklist/init
    // remain available but the UI no longer surfaces a manual init button.
    if let Some(worklist_path) = worklist_file(app) {
        if !worklist_path.exists() {
            std::fs::write(&worklist_path, empty_worklist_json())
                .map_err(|e| format!("write {}: {}", worklist_path.display(), e))?;
            if let Ok(mut guard) = last_worklist_cell().lock() {
                *guard = Some(empty_worklist_json().to_string());
            }
            wrote.push(worklist_path.display().to_string());
        }
    }

    // Conventions sidecar — skipped on the source repo.
    let sidecar_path = proj.join(ENHANCE_SIDECAR_REL);
    if !is_source_repo {
        let (conventions_bytes, _mime) = serve_app_file(Some(app), "__shell/conventions.md")
            .ok_or_else(|| "conventions template not found".to_string())?;
        let conventions = String::from_utf8(conventions_bytes)
            .map_err(|e| format!("conventions template not utf-8: {}", e))?;
        if let Some(parent) = sidecar_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
        std::fs::write(&sidecar_path, &conventions).map_err(|e| format!("write sidecar: {}", e))?;
        wrote.push(sidecar_path.display().to_string());
    }

    let codex_agents_path = proj.join(ENHANCE_CODEX_AGENTS_REL);
    let (codex_seed_bytes, _mime) = serve_app_file(Some(app), ENHANCE_CODEX_BUNDLE_REL)
        .ok_or_else(|| "codex startup instructions bundle not found".to_string())?;
    let codex_seed = String::from_utf8(codex_seed_bytes)
        .map_err(|e| format!("codex startup instructions not utf-8: {}", e))?;
    let codex_block = format!(
        "{}\n{}\n{}",
        ENHANCE_MARKER_START,
        codex_seed.trim_end(),
        ENHANCE_MARKER_END
    );
    let existing_agents = std::fs::read_to_string(&codex_agents_path).unwrap_or_default();
    let new_agents = if let Some(start_idx) = existing_agents.find(ENHANCE_MARKER_START) {
        let tail = &existing_agents[start_idx..];
        let end_offset = tail
            .find(ENHANCE_MARKER_END)
            .map(|i| start_idx + i + ENHANCE_MARKER_END.len())
            .unwrap_or(existing_agents.len());
        let mut s = existing_agents.clone();
        s.replace_range(start_idx..end_offset, &codex_block);
        s
    } else if existing_agents.is_empty() {
        format!("{}\n", codex_block)
    } else {
        format!("{}\n\n{}\n", existing_agents.trim_end(), codex_block)
    };
    std::fs::write(&codex_agents_path, &new_agents)
        .map_err(|e| format!("write AGENTS.md: {}", e))?;
    wrote.push(codex_agents_path.display().to_string());

    // Proposal-guard hook script (idempotent — same content on re-run).
    let (hook_bytes, _mime) = serve_app_file(Some(app), ENHANCE_HOOK_BUNDLE_REL)
        .ok_or_else(|| "worklist-guard.py bundle not found".to_string())?;
    let hook_path = proj.join(ENHANCE_HOOK_SCRIPT_REL);
    if let Some(parent) = hook_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&hook_path, &hook_bytes).map_err(|e| format!("write hook: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)
            .map_err(|e| format!("stat hook: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms).map_err(|e| format!("chmod hook: {}", e))?;
    }
    wrote.push(hook_path.display().to_string());

    // Pre-rename leftover script (bc3ee31). Idempotent: NotFound is fine.
    let old_hook_path = proj.join(".claude/hooks/proposal-guard.py");
    let _ = std::fs::remove_file(&old_hook_path);

    // Register hook in settings.json (idempotent merge). Prune any
    // pre-rename proposal-guard.py PreToolUse entries first so upgraded
    // projects don't end up running both hooks on every Write/Edit.
    let settings_path = proj.join(ENHANCE_SETTINGS_REL);
    prune_proposal_guard_from_settings(&settings_path)?;
    merge_worklist_guard_into_settings(&settings_path)?;
    wrote.push(settings_path.display().to_string());

    // CLAUDE.md marker block — skipped on the source repo.
    let claude_md_path = proj.join("CLAUDE.md");
    if !is_source_repo {
        let existing = std::fs::read_to_string(&claude_md_path).unwrap_or_default();
        let block = format!(
            "{}\n@{}\n{}",
            ENHANCE_MARKER_START, ENHANCE_SIDECAR_REL, ENHANCE_MARKER_END
        );
        let new_content = if let Some(start_idx) = existing.find(ENHANCE_MARKER_START) {
            // Replace existing block in-place.
            let tail = &existing[start_idx..];
            let end_offset = tail
                .find(ENHANCE_MARKER_END)
                .map(|i| start_idx + i + ENHANCE_MARKER_END.len())
                .unwrap_or(existing.len());
            let mut s = existing.clone();
            s.replace_range(start_idx..end_offset, &block);
            s
        } else if existing.is_empty() {
            format!("{}\n", block)
        } else {
            format!("{}\n\n{}\n", existing.trim_end(), block)
        };
        std::fs::write(&claude_md_path, &new_content)
            .map_err(|e| format!("write CLAUDE.md: {}", e))?;
        wrote.push(claude_md_path.display().to_string());
    }

    // Codex user-global hook install. Runs unconditionally (incl. source repo)
    // because the install is keyed to $HOME, not the project.
    let codex_hook_install = install_codex_worklist_guard(app)?;
    for path in &codex_hook_install.wrote {
        wrote.push(path.clone());
    }
    // Developer-instructions install — top-level config.toml scalar carrying
    // the gate prose. Replaced the per-turn UserPromptSubmit injection after
    // verifying developer-role context is salient enough on its own.
    install_codex_developer_instructions()?;

    let body = serde_json::json!({
        "enhanced": true,
        "isSourceRepo": is_source_repo,
        "wrote": wrote,
        "codexHookInstalled": codex_hook_install.installed,
        "codexHookScriptPath": codex_hook_install.script_path,
        "codexConfigPath": codex_hook_install.config_path,
        "codexHookNeedsTrust": codex_hook_install.installed,
    });
    serde_json::to_vec(&body).map_err(|e| e.to_string())
}

fn hook_fingerprint(path: &Path) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(path).ok()?;
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    Some(format!("{:016x}", h.finish()))
}

fn write_codex_trust_ack() -> Result<(), String> {
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    let hook = home.join(ENHANCE_CODEX_HOOK_INSTALL_REL);
    let fp = hook_fingerprint(&hook)
        .ok_or_else(|| format!("read {}: hook not installed", hook.display()))?;
    let marker = home.join(ENHANCE_CODEX_TRUST_ACK_REL);
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&marker, fp.as_bytes())
        .map_err(|e| format!("write {}: {}", marker.display(), e))?;
    Ok(())
}

struct CodexHookInstall {
    installed: bool,
    script_path: String,
    config_path: String,
    wrote: Vec<String>,
}

fn install_codex_worklist_guard<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<CodexHookInstall, String> {
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    let script_path = home.join(ENHANCE_CODEX_HOOK_INSTALL_REL);
    let config_path = home.join(ENHANCE_CODEX_CONFIG_REL);
    let mut wrote: Vec<String> = Vec::new();

    let (script_bytes, _mime) = serve_app_file(Some(app), ENHANCE_CODEX_HOOK_BUNDLE_REL)
        .ok_or_else(|| "worklist-guard-codex.py bundle not found".to_string())?;
    if let Some(parent) = script_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&script_path, &script_bytes)
        .map_err(|e| format!("write codex hook script: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)
            .map_err(|e| format!("stat codex hook: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms)
            .map_err(|e| format!("chmod codex hook: {}", e))?;
    }
    wrote.push(script_path.display().to_string());

    // Build the TOML block. On Windows we invoke through `py -3` for the
    // same reason as the Claude hook; on Unix we run the script directly via
    // its shebang. The matcher regex covers codex's canonical apply_patch +
    // Bash and the Claude-style Write/Edit aliases codex accepts.
    let script_str = script_path.display().to_string();
    #[cfg(windows)]
    let command_line = format!("py -3 \"{}\"", script_str.replace('"', "\\\""));
    #[cfg(not(windows))]
    let command_line = script_str.clone();
    // Matcher covers codex's canonical apply_patch + Bash, the Claude-style
    // Write/Edit aliases codex accepts, and any MCP tool (mcp__<server>__<tool>).
    // The MCP surface matters: a user with [mcp_servers.filesystem] configured
    // can route file edits through mcp__filesystem__write_text_file / edit_file
    // and bypass apply_patch entirely. The guard script branches by tool_name
    // and only blocks MCP calls whose names signal mutation (write/edit/create/
    // delete/move/...).
    //
    // The pre-emptive nudge (was UserPromptSubmit injection earlier) is now
    // carried by `developer_instructions` at top-level config — verified to
    // be rendered in the developer-role context part, higher priority than
    // AGENTS.md (which is user-role). install_codex_developer_instructions
    // writes that field; this function only installs the runtime backstop.
    let toml_block = format!(
        "{start}\n\
         [[hooks.PreToolUse]]\n\
         matcher = \"^(apply_patch|Bash|Write|Edit|mcp__.*)$\"\n\
         \n\
         [[hooks.PreToolUse.hooks]]\n\
         type = \"command\"\n\
         command = {command_quoted}\n\
         timeout = 10\n\
         statusMessage = \"Bram worklist guard\"\n\
         {end}",
        start = ENHANCE_CODEX_TOML_MARKER_START,
        end = ENHANCE_CODEX_TOML_MARKER_END,
        command_quoted = toml_basic_string(&command_line),
    );

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let cleaned = strip_marker_block(
        &existing,
        ENHANCE_CODEX_LEGACY_TOML_MARKER_START,
        ENHANCE_CODEX_LEGACY_TOML_MARKER_END,
    );
    let new_content = if let Some(start_idx) = cleaned.find(ENHANCE_CODEX_TOML_MARKER_START) {
        let tail = &cleaned[start_idx..];
        let end_offset = tail
            .find(ENHANCE_CODEX_TOML_MARKER_END)
            .map(|i| start_idx + i + ENHANCE_CODEX_TOML_MARKER_END.len())
            .unwrap_or(cleaned.len());
        let mut s = cleaned.clone();
        s.replace_range(start_idx..end_offset, &toml_block);
        s
    } else if cleaned.trim().is_empty() {
        format!("{}\n", toml_block)
    } else {
        format!("{}\n\n{}\n", cleaned.trim_end(), toml_block)
    };
    std::fs::write(&config_path, &new_content)
        .map_err(|e| format!("write codex config.toml: {}", e))?;
    wrote.push(config_path.display().to_string());

    Ok(CodexHookInstall {
        installed: true,
        script_path: script_path.display().to_string(),
        config_path: config_path.display().to_string(),
        wrote,
    })
}

fn install_codex_developer_instructions() -> Result<(), String> {
    let home = home_dir().ok_or("no HOME or USERPROFILE")?;
    let config_path = home.join(ENHANCE_CODEX_CONFIG_REL);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();

    // Strip managed legacy blocks so setup replaces them with the Bram block
    // instead of creating duplicate top-level `developer_instructions` keys.
    let cleaned = strip_marker_block(
        &existing,
        "# xmlui-desktop-test-instr:start",
        "# xmlui-desktop-test-instr:end",
    );
    let cleaned = strip_marker_block(
        &cleaned,
        ENHANCE_CODEX_LEGACY_INSTR_MARKER_START,
        ENHANCE_CODEX_LEGACY_INSTR_MARKER_END,
    );
    let cleaned = if cleaned.contains(ENHANCE_CODEX_TYPO_INSTR_MARKER_END) {
        strip_marker_block(
            &cleaned,
            ENHANCE_CODEX_INSTR_MARKER_START,
            ENHANCE_CODEX_TYPO_INSTR_MARKER_END,
        )
    } else {
        cleaned
    };

    let block = format!(
        "{start}\ndeveloper_instructions = {body}\n{end}",
        start = ENHANCE_CODEX_INSTR_MARKER_START,
        end = ENHANCE_CODEX_INSTR_MARKER_END,
        body = toml_basic_string(ENHANCE_CODEX_GATE_PROSE),
    );

    let new_content = if let Some(start_idx) = cleaned.find(ENHANCE_CODEX_INSTR_MARKER_START) {
        let tail = &cleaned[start_idx..];
        let end_offset = tail
            .find(ENHANCE_CODEX_INSTR_MARKER_END)
            .map(|i| start_idx + i + ENHANCE_CODEX_INSTR_MARKER_END.len())
            .unwrap_or(cleaned.len());
        let mut s = cleaned.clone();
        s.replace_range(start_idx..end_offset, &block);
        s
    } else if cleaned.trim().is_empty() {
        format!("{}\n", block)
    } else {
        // Prepend at top of file. developer_instructions is a top-level scalar
        // and TOML requires those before any [section] table.
        format!("{}\n\n{}", block, cleaned.trim_start_matches('\n'))
    };
    std::fs::write(&config_path, &new_content)
        .map_err(|e| format!("write codex config.toml: {}", e))?;
    Ok(())
}

fn strip_marker_block(content: &str, start: &str, end: &str) -> String {
    let mut result = content.to_string();
    while let Some(start_idx) = result.find(start) {
        let tail = &result[start_idx..];
        let end_offset = match tail.find(end) {
            Some(i) => start_idx + i + end.len(),
            None => result.len(),
        };
        // Also consume the trailing newline if present, so we don't leave a blank line.
        let mut cut_to = end_offset;
        if result.as_bytes().get(cut_to) == Some(&b'\n') {
            cut_to += 1;
        }
        result.replace_range(start_idx..cut_to, "");
    }
    result
}

// Quote a string as a TOML basic string literal — wraps in double quotes and
// escapes backslashes / double quotes / control chars per TOML spec.
fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Worklist history (issue #18: save and browse completed worklist items)
//
// The filesystem watcher detects writes to resources/worklist.json and
// appends a timestamped JSON snapshot of the *prior* contents to
// resources/worklist-history/, plus a sibling .md changelog summarizing
// what changed. The cache lives in process memory so we can diff old
// vs new without re-reading from disk.
// ============================================================================

static LAST_WORKLIST: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn last_worklist_cell() -> &'static Mutex<Option<String>> {
    LAST_WORKLIST.get_or_init(|| Mutex::new(None))
}

fn worklist_file<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    project_root(Some(app)).map(|p| p.join("resources").join("worklist.json"))
}

fn worklist_auth_file<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    project_root(Some(app)).map(|p| p.join(WORKLIST_AUTH_REL))
}

fn worklist_history_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    project_root(Some(app)).map(|p| p.join("resources").join("worklist-history"))
}

fn inflight_claim_file<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    project_root(Some(app)).map(|p| p.join(INFLIGHT_CLAIM_REL))
}

// Write the inflight sentinel (#84). Atomic via .tmp + rename so the
// file is either absent or contains valid JSON. Caller has verified
// `ids` is non-empty. `kind` is one of "approved", "drop", "iterate".
fn write_inflight_claim_sentinel<R: tauri::Runtime>(
    app: &AppHandle<R>,
    ids: &[String],
    kind: &str,
) {
    let Some(path) = inflight_claim_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::json!({
        "ids": ids,
        "claimedAt": unix_now_ms(),
        "kind": kind,
    });
    let body = match serde_json::to_string_pretty(&payload) {
        Ok(s) => s,
        Err(_) => return,
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, format!("{}\n", body)).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &path);
    if bram_trace_enabled() {
        append_bram_trace_line(
            app,
            "inflight-sentinel",
            &format!(
                "op=write kind={} ids={}",
                kind,
                serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string())
            ),
        );
    }
    trace_emit_signal(app, "inflight-claim-changed");
    let _ = app.emit("inflight-claim-changed", ());
}

// Clear the inflight sentinel (#84). Conditions: a sentinel exists,
// AND every id currently claimed is in `mutated_ids`. Partial coverage
// leaves the sentinel alone — partial completion is a diagnostic
// signal worth surfacing (stuck spinner = stuck claim once item 3
// lands).
fn clear_inflight_claim_sentinel<R: tauri::Runtime>(
    app: &AppHandle<R>,
    mutated_ids: &[String],
) {
    let Some(path) = inflight_claim_file(app) else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let claim: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    let claimed_ids: Vec<String> = claim
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if claimed_ids.is_empty() {
        return;
    }
    let fully_covered = claimed_ids
        .iter()
        .all(|cid| mutated_ids.iter().any(|mid| mid == cid));
    if !fully_covered {
        return;
    }
    let _ = std::fs::remove_file(&path);
    if bram_trace_enabled() {
        append_bram_trace_line(
            app,
            "inflight-sentinel",
            &format!(
                "op=clear ids={}",
                serde_json::to_string(&claimed_ids).unwrap_or_else(|_| "[]".to_string())
            ),
        );
    }
    trace_emit_signal(app, "inflight-claim-changed");
    let _ = app.emit("inflight-claim-changed", ());

    // Flush a deferred tools-pane-reload if one was queued during the
    // cycle (refs #93). Atomic swap-to-false; the previous value tells
    // us whether to fire.
    if PENDING_TOOLS_RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst) {
        if bram_trace_enabled() {
            append_bram_trace_line(app, "tools-pane-reload", "op=flushed-on-clear");
        }
        trace_emit_signal(app, "tools-pane-reload");
        let _ = app.emit("tools-pane-reload", ());
    }
}

// True iff resources/.inflight-claim.json exists, parses, and lists at
// least one claimed id. Used by the watcher to decide whether to emit
// tools-pane-reload now or defer it until the cycle clears (refs #93).
fn inflight_sentinel_is_active<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    let Some(path) = inflight_claim_file(app) else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return false;
    };
    let claim: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    claim
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
}

// Emit tools-pane-reload, OR defer it if a cycle is currently active
// (refs #93). The host owns the cycle-active signal via the inflight
// sentinel; suppressing reloads during cycles prevents iframe remount
// from blowing away the user's mid-cycle context and causing the 7+s
// of drift / click swallows we measured pre-fix.
fn emit_or_defer_tools_pane_reload<R: tauri::Runtime>(app: &AppHandle<R>) {
    if inflight_sentinel_is_active(app) {
        PENDING_TOOLS_RELOAD.store(true, std::sync::atomic::Ordering::SeqCst);
        if bram_trace_enabled() {
            append_bram_trace_line(app, "tools-pane-reload", "op=deferred reason=sentinel-active");
        }
        return;
    }
    trace_emit_signal(app, "tools-pane-reload");
    let _ = app.emit("tools-pane-reload", ());
}

// Read the current sentinel's claimed ids and call the regular clear
// path with them — used by the agent-turn-end hook to fire a clear
// without the caller having to know who's claimed. No-op if the
// sentinel is absent or has no ids. Refs #91 follow-up.
fn clear_active_sentinel<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(path) = inflight_claim_file(app) else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let claim: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    let ids: Vec<String> = claim
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if !ids.is_empty() {
        clear_inflight_claim_sentinel(app, &ids);
    }
}

// JSONL-driven turn-end detection (#91 follow-up). The PTY-silence
// path (`pty_agent_turn_update`) fires `agent-turn-end` events on
// silence_ms exceeding a threshold, then clears the sentinel — but
// a multi-second silence between bursts is indistinguishable from a
// real end-of-turn via PTY signal alone. The session JSONL has an
// explicit `stop_reason: "end_turn"` marker on the assistant's final
// message of a turn, which is the durable, structured signal we want.
//
// First cut: Claude Code sessions only (detected by the `.claude`
// segment in the path). Codex sessions don't carry a `stop_reason`
// field on assistant messages, so this parser's `end_turn` branch
// would never fire for them — the silence-detector fallback at
// `MIN_SILENCE_FOR_SENTINEL_CLEAR_MS=3000ms` covers Codex today.
// A Codex-shaped detector is only worth adding if that 3s floor
// becomes user-visibly slow.
//
// Stale-line guard: if the file's mtime predates the sentinel's
// `claimedAt`, the last line is from a prior turn that ended before
// the current click — skip and trace as `skipped=stale-prior-turn`.
fn check_jsonl_for_turn_end<R: tauri::Runtime>(app: &AppHandle<R>, path: &std::path::Path) {
    let path_str = path.to_string_lossy();
    if !path_str.contains("/.claude/") {
        return;
    }

    let basename = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if bram_trace_enabled() {
        append_bram_trace_line(
            app,
            "jsonl-turn-end",
            &format!("op=enter path={}", basename),
        );
    }

    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    let file_mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };

    // Claude Code appends `last-prompt` and `permission-mode` metadata
    // lines after every assistant turn, so the file's last non-empty
    // line is reliably NOT the assistant message. Scan backwards,
    // skipping unparseable lines and non-assistant types, to find the
    // most recent `type=assistant` entry.
    let mut assistant_entry: Option<serde_json::Value> = None;
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        if entry.get("type").and_then(|v| v.as_str()) == Some("assistant") {
            assistant_entry = Some(entry);
            break;
        }
    }
    let Some(entry) = assistant_entry else {
        return;
    };

    let stop_reason = entry
        .get("message")
        .and_then(|m| m.get("stop_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if stop_reason != "end_turn" {
        return;
    }

    let Some(sentinel_path) = inflight_claim_file(app) else {
        return;
    };
    let Ok(sentinel_content) = std::fs::read_to_string(&sentinel_path) else {
        return;
    };
    let Ok(claim) = serde_json::from_str::<serde_json::Value>(&sentinel_content) else {
        return;
    };

    let claimed_ids: Vec<String> = claim
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if claimed_ids.is_empty() {
        return;
    }

    let claimed_at = claim
        .get("claimedAt")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if file_mtime_ms < claimed_at {
        if bram_trace_enabled() {
            append_bram_trace_line(
                app,
                "jsonl-turn-end",
                &format!(
                    "op=detect kind=claude stop_reason=end_turn skipped=stale-prior-turn claimed={}",
                    serde_json::to_string(&claimed_ids).unwrap_or_else(|_| "[]".to_string())
                ),
            );
        }
        return;
    }

    if bram_trace_enabled() {
        append_bram_trace_line(
            app,
            "jsonl-turn-end",
            &format!(
                "op=detect kind=claude stop_reason=end_turn claimed={}",
                serde_json::to_string(&claimed_ids).unwrap_or_else(|_| "[]".to_string())
            ),
        );
    }

    clear_inflight_claim_sentinel(app, &claimed_ids);
}

// Startup cleanup. Removes any stale inflight sentinel from a prior
// session that didn't complete (Bram killed mid-cycle, agent crashed
// before mutate, etc.).
fn cleanup_stale_inflight_claim<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(path) = inflight_claim_file(app) else {
        return;
    };
    if !path.exists() {
        return;
    }
    let _ = std::fs::remove_file(&path);
    if bram_trace_enabled() {
        append_bram_trace_line(app, "inflight-sentinel", "op=stale-startup-clear");
    }
    trace_emit_signal(app, "inflight-claim-changed");
    let _ = app.emit("inflight-claim-changed", ());
}

fn pty_intent_file<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    project_root(Some(app)).map(|p| p.join(PTY_INTENT_REL))
}

// Serializes append + drain in queue_pty_intent so concurrent calls
// don't race the read-then-truncate phase and lose intents.
static PTY_INTENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn pty_intent_lock() -> &'static Mutex<()> {
    PTY_INTENT_LOCK.get_or_init(|| Mutex::new(()))
}

// Monotonic counter for `[pty-intent]` trace ids. Doesn't need to be
// globally unique — only readable within one session's trace log.
static PTY_INTENT_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

// Startup cleanup. Removes any stale pty-intent queue from a prior
// session so its intents don't replay into a fresh PTY (#86).
fn cleanup_stale_pty_intents<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(path) = pty_intent_file(app) else {
        return;
    };
    if !path.exists() {
        return;
    }
    let _ = std::fs::remove_file(&path);
    if bram_trace_enabled() {
        append_bram_trace_line(app, "pty-intent", "op=stale-startup-clear");
    }
}

fn empty_worklist_json() -> &'static str {
    "{\n  \"description\": \"\",\n  \"items\": []\n}\n"
}

// Per-item content hash exposed via /__worklist. The UI reads it and
// propagates it verbatim into the `approved:` / `drop:` payload, so the
// PTY watcher can recompute the same fingerprint from the on-disk file
// and detect mid-flight drift without ever shipping full item content
// back into the conversation context.
//
// Canonicalization: serde_json is built WITHOUT the preserve_order
// feature in this crate, so its Map is BTreeMap-backed and
// `to_string` emits keys in sorted order at every depth. That gives
// us a deterministic byte sequence on both sides of the channel
// without needing a separate canonicalizer.
fn canonical_item_hash(item: &serde_json::Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let canonical = serde_json::to_string(item).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn base_worklist_doc_from_parsed(parsed_doc: Option<serde_json::Value>) -> serde_json::Value {
    use serde_json::json;

    match parsed_doc {
        Some(serde_json::Value::Object(obj)) => serde_json::Value::Object(obj),
        Some(serde_json::Value::Array(_)) => json!({
            "description": "",
            "items": [],
            "schemaError": "root-array",
            "schemaErrorMessage": "resources/worklist.json must be an object with { \"description\": string, \"items\": [] }, not a bare JSON array",
        }),
        Some(_) => json!({
            "description": "",
            "items": [],
            "schemaError": "root-non-object",
            "schemaErrorMessage": "resources/worklist.json must be a JSON object with { \"description\": string, \"items\": [] } at the root",
        }),
        None => json!({ "description": "", "items": [] }),
    }
}

fn worklist_doc<R: tauri::Runtime>(app: &AppHandle<R>) -> serde_json::Value {
    let path = worklist_file(app);
    let exists = path.as_ref().map_or(false, |p| p.is_file());
    let resources_exists = path
        .as_ref()
        .and_then(|p| p.parent())
        .map_or(false, |p| p.is_dir());
    let path_str = path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let parsed_doc: Option<serde_json::Value> = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok());
    let mut doc = base_worklist_doc_from_parsed(parsed_doc);
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("exists".to_string(), serde_json::Value::Bool(exists));
        obj.insert(
            "resourcesExists".to_string(),
            serde_json::Value::Bool(resources_exists),
        );
        obj.insert("path".to_string(), serde_json::Value::String(path_str));
        if !obj.contains_key("description") {
            obj.insert(
                "description".to_string(),
                serde_json::Value::String(String::new()),
            );
        }
        if !obj.contains_key("items") {
            obj.insert("items".to_string(), serde_json::Value::Array(Vec::new()));
        }
        // Inject per-item hashes computed from the RAW item content (i.e.
        // before any server-added fields like `hash` or `diff`). The PTY
        // watcher recomputes against the same on-disk raw content, so the
        // fingerprint round-trips through the UI without drift.
        if let Some(items) = obj.get_mut("items").and_then(|v| v.as_array_mut()) {
            for item in items {
                let hash = canonical_item_hash(item);
                if let Some(item_obj) = item.as_object_mut() {
                    item_obj.insert("hash".to_string(), serde_json::Value::String(hash));
                }
            }
        }
        doc
    } else {
        serde_json::json!({
            "description": "",
            "items": [],
            "exists": exists,
            "resourcesExists": resources_exists,
            "path": path_str,
        })
    }
}

#[cfg(test)]
mod worklist_doc_tests {
    use super::base_worklist_doc_from_parsed;
    use serde_json::json;

    #[test]
    fn bare_array_root_sets_schema_error() {
        let doc = base_worklist_doc_from_parsed(Some(json!([
            { "id": "x", "file": "foo.txt", "before": "", "after": "" }
        ])));

        assert_eq!(doc.get("schemaError").and_then(|v| v.as_str()), Some("root-array"));
        assert_eq!(doc.get("description").and_then(|v| v.as_str()), Some(""));
        assert_eq!(
            doc.get("items")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(0)
        );
    }

    #[test]
    fn scalar_root_sets_non_object_schema_error() {
        let doc = base_worklist_doc_from_parsed(Some(json!("oops")));

        assert_eq!(
            doc.get("schemaError").and_then(|v| v.as_str()),
            Some("root-non-object")
        );
        assert_eq!(doc.get("description").and_then(|v| v.as_str()), Some(""));
        assert_eq!(
            doc.get("items")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(0)
        );
    }
}

#[cfg(test)]
mod app_root_resolution_tests {
    use super::bram_app_root_candidates;
    use std::path::PathBuf;

    #[test]
    fn candidates_include_only_bram_owned_locations() {
        let candidates = bram_app_root_candidates(
            Some(PathBuf::from("/bundle/resources")),
            Some(PathBuf::from("/bundle/bin")),
            Some(PathBuf::from("/bundle/bin/bram")),
        );

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/bundle/resources/app"),
                PathBuf::from("/bundle/bin/app"),
                PathBuf::from("/bundle/bin/app"),
                PathBuf::from("/bundle/bin/../Resources/app"),
            ]
        );
        assert!(
            candidates
                .iter()
                .all(|p| !p.to_string_lossy().contains("/project/app"))
        );
    }
}

fn init_worklist_file<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    let path = worklist_file(app).ok_or("could not resolve project root")?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("could not resolve parent for {}", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {}", parent.display(), e))?;
    if !path.exists() {
        std::fs::write(&path, empty_worklist_json())
            .map_err(|e| format!("write {}: {}", path.display(), e))?;
        if let Ok(mut guard) = last_worklist_cell().lock() {
            *guard = Some(empty_worklist_json().to_string());
        }
    }
    serde_json::to_vec(&worklist_doc(app)).map_err(|e| e.to_string())
}

fn unix_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// Howard Hinnant's days-from-civil, inverted. Used by format_iso_utc to
// avoid pulling in chrono just for one timestamp formatter.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

fn format_iso_utc(ms: i64) -> String {
    let secs = ms / 1000;
    let days = secs.div_euclid(86400);
    let secs_of_day = secs.rem_euclid(86400);
    let h = secs_of_day / 3600;
    let m = secs_of_day % 3600 / 60;
    let s = secs_of_day % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

// Millisecond-precision variant for bram-trace lines, where sub-second
// alignment with inspector trace `ts` fields matters.
fn format_iso_utc_ms(ms: i64) -> String {
    let secs = ms / 1000;
    let sub = ms.rem_euclid(1000);
    let days = secs.div_euclid(86400);
    let secs_of_day = secs.rem_euclid(86400);
    let h = secs_of_day / 3600;
    let m = secs_of_day % 3600 / 60;
    let s = secs_of_day % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, mo, d, h, m, s, sub)
}

fn worklist_item_id(item: &serde_json::Value) -> Option<String> {
    item.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn worklist_item_status(item: &serde_json::Value) -> &str {
    item.get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("proposed")
}

fn worklist_item_str_field<'a>(item: &'a serde_json::Value, key: &str) -> &'a str {
    item.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn worklist_items(doc: &serde_json::Value) -> Vec<serde_json::Value> {
    doc.get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn worklist_description(doc: &serde_json::Value) -> String {
    doc.get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn normalize_turn_submission(data: &str) -> String {
    // toTurn in app/main.js sends `\x15\x1b[200~<text>\x1b[201~\r` — the
    // leading \x15 (NAK, Ctrl-U) clears any pre-existing input line before
    // the bracketed-paste payload arrives. After stripping the bracketed-
    // paste markers, trim any remaining C0 control characters (NAK, CR,
    // LF, etc.) so the structured-line prefix check finds `approved:` /
    // `drop:` at offset 0 instead of `\x15approved:`.
    data.replace("\u{1b}[200~", "")
        .replace("\u{1b}[201~", "")
        .trim_matches(|c: char| c.is_control())
        .trim()
        .to_string()
}

// Pure parser of an `approved:` / `drop:` turn line. Returns the kind
// and the list of (id, optional supplied hash) requests. The hash is
// None for legacy payloads that arrived as full item objects
// (`{items: [<full>]}` or `{ids: [...]}` for old drop), or as plain
// items without a `hash` field.
struct ParsedWorklistAuthorization {
    kind: String,
    requests: Vec<(String, Option<String>, Option<String>)>,
}

fn parse_worklist_authorization_message(text: &str) -> Option<ParsedWorklistAuthorization> {
    let trimmed = text.trim();
    for (prefix, kind) in [("approved:", "approved"), ("drop:", "drop")] {
        let Some(rest) = trimmed.strip_prefix(prefix) else {
            continue;
        };
        let value = serde_json::from_str::<serde_json::Value>(rest.trim()).ok()?;
        let mut requests: Vec<(String, Option<String>, Option<String>)> = Vec::new();
        // Preferred shape (new per-item, plus legacy approve which carried
        // top-level feedback): {items: [{id, hash?, feedback?}, ...]}.
        if let Some(items) = value.get("items").and_then(|v| v.as_array()) {
            for item in items {
                let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let hash = item
                    .get("hash")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let feedback = item
                    .get("feedback")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                requests.push((id.to_string(), hash, feedback));
            }
        }
        // Legacy drop shape: {ids: [...]} — accept if items[] wasn't present.
        if requests.is_empty() {
            if let Some(ids) = value.get("ids").and_then(|v| v.as_array()) {
                for v in ids {
                    if let Some(id) = v.as_str() {
                        requests.push((id.to_string(), None, None));
                    }
                }
            }
        }
        return Some(ParsedWorklistAuthorization {
            kind: kind.to_string(),
            requests,
        });
    }
    None
}

fn record_worklist_authorization_from_input<R: tauri::Runtime>(app: &AppHandle<R>, data: &str) {
    let normalized = normalize_turn_submission(data);
    let Some(parsed) = parse_worklist_authorization_message(&normalized) else {
        return;
    };

    // Look up each requested id in the on-disk worklist, recompute its
    // canonical hash, and compare against the supplied hash. Mismatches
    // (or supplied-but-missing items) flip the record to "rejected_stale"
    // so the agent surfaces the staleness rather than acting blind.
    let on_disk_items: Vec<serde_json::Value> = worklist_file(app)
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|doc| doc.get("items").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let mut ids: Vec<String> = Vec::with_capacity(parsed.requests.len());
    let mut verified_items: Vec<serde_json::Value> = Vec::new();
    let mut mismatched_ids: Vec<String> = Vec::new();
    let mut any_hash_supplied = false;

    for (id, supplied_hash, supplied_feedback) in &parsed.requests {
        ids.push(id.clone());
        let found = on_disk_items.iter().find(|it| {
            it.get("id")
                .and_then(|v| v.as_str())
                .map_or(false, |x| x == id)
        });
        match (supplied_hash, found) {
            (Some(supplied), Some(item)) => {
                any_hash_supplied = true;
                if &canonical_item_hash(item) == supplied {
                    // Clone the on-disk item, then attach the per-item
                    // feedback from the incoming payload so the agent
                    // sees it via /__worklist/resolve. Always set the
                    // field (even when empty) so consumers can read it
                    // uniformly.
                    let mut enriched = item.clone();
                    if let Some(obj) = enriched.as_object_mut() {
                        obj.insert(
                            "feedback".to_string(),
                            serde_json::Value::String(
                                supplied_feedback.clone().unwrap_or_default(),
                            ),
                        );
                    }
                    verified_items.push(enriched);
                } else {
                    mismatched_ids.push(id.clone());
                }
            }
            (Some(_), None) => {
                any_hash_supplied = true;
                mismatched_ids.push(id.clone());
            }
            (None, _) => {
                // Legacy payload — no hash to verify, no items array stored.
            }
        }
    }

    let kind = if any_hash_supplied && !mismatched_ids.is_empty() {
        "rejected_stale".to_string()
    } else {
        parsed.kind
    };
    let items = if mismatched_ids.is_empty() {
        verified_items
    } else {
        Vec::new()
    };

    let record = WorklistAuthorizationRecord {
        kind,
        ids,
        items,
        mismatched_ids,
        issued_at_ms: unix_now_ms(),
        source: "pty-write".to_string(),
        consumed_at_ms: None,
    };

    let Some(path) = worklist_auth_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("[worklist-auth] create {} failed: {}", parent.display(), e);
            return;
        }
    }
    // Detect clobber: if a prior, not-yet-consumed record exists with a
    // different kind, the new write overwrites it. Read the file before
    // serializing the new record so the prior_kind lookup doesn't race
    // with our own write.
    if bram_trace_enabled() {
        let prior_kind = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<WorklistAuthorizationRecord>(&s).ok())
            .and_then(|prior| {
                if prior.consumed_at_ms.is_some() {
                    None
                } else if prior.kind == record.kind {
                    None
                } else {
                    Some(prior.kind)
                }
            });
        let op = if prior_kind.is_some() { "clobber" } else { "write" };
        let prior_field = prior_kind
            .as_deref()
            .map(|k| format!(" prior_kind={}", k))
            .unwrap_or_default();
        append_bram_trace_line(
            app,
            "auth-record",
            &format!(
                "op={} kind={} ids={}{} source={}",
                op,
                record.kind,
                serde_json::to_string(&record.ids).unwrap_or_else(|_| "[]".to_string()),
                prior_field,
                record.source
            ),
        );
    }
    let body = match serde_json::to_string_pretty(&record) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[worklist-auth] serialize failed: {}", e);
            return;
        }
    };
    if let Err(e) = std::fs::write(&path, format!("{}\n", body)) {
        eprintln!("[worklist-auth] write {} failed: {}", path.display(), e);
    }
}

fn read_active_worklist_authorization<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Option<WorklistAuthorizationRecord> {
    let path = worklist_auth_file(app)?;
    let content = std::fs::read_to_string(path).ok()?;
    let record = serde_json::from_str::<WorklistAuthorizationRecord>(&content).ok()?;
    if record.consumed_at_ms.is_some() {
        return None;
    }
    match record.kind.as_str() {
        "approved" | "drop" => Some(record),
        _ => None,
    }
}

fn consume_worklist_authorization<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(path) = worklist_auth_file(app) else {
        return;
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut record = match serde_json::from_str::<WorklistAuthorizationRecord>(&content) {
        Ok(r) => r,
        Err(_) => return,
    };
    if record.consumed_at_ms.is_some() {
        return;
    }
    if bram_trace_enabled() {
        append_bram_trace_line(
            app,
            "auth-record",
            &format!(
                "op=consume kind={} ids={}",
                record.kind,
                serde_json::to_string(&record.ids).unwrap_or_else(|_| "[]".to_string())
            ),
        );
    }
    record.consumed_at_ms = Some(unix_now_ms());
    let body = match serde_json::to_string_pretty(&record) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = std::fs::write(&path, format!("{}\n", body));
}

// Auto-advance any still-`proposed` item whose `file` / `files` are touched
// by an authorized write. Closes the convention-vs-enforcement gap in the
// state-machine gate that lets agents apply edits without flipping the
// worklist status (issue #60). Provider-neutral — same trigger fires
// regardless of which agent landed the writes.
fn try_auto_advance_proposed_items<R: tauri::Runtime>(
    app: &AppHandle<R>,
    project_rel_paths: &[String],
) -> Vec<String> {
    if project_rel_paths.is_empty() {
        return Vec::new();
    }
    let Some(wl_path) = worklist_file(app) else {
        return Vec::new();
    };
    let Some(auth_path) = worklist_auth_file(app) else {
        return Vec::new();
    };
    let Ok(auth_text) = std::fs::read_to_string(&auth_path) else {
        return Vec::new();
    };
    let auth: serde_json::Value = match serde_json::from_str(&auth_text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    if auth.get("kind").and_then(|v| v.as_str()) != Some("approved") {
        return Vec::new();
    }
    let auth_ids: std::collections::HashSet<String> = auth
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if auth_ids.is_empty() {
        return Vec::new();
    }
    let Ok(wl_text) = std::fs::read_to_string(&wl_path) else {
        return Vec::new();
    };
    let mut wl: serde_json::Value = match serde_json::from_str(&wl_text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let touched: std::collections::HashSet<&str> =
        project_rel_paths.iter().map(|s| s.as_str()).collect();
    let mut advanced: Vec<String> = Vec::new();
    let Some(items) = wl.get_mut("items").and_then(|v| v.as_array_mut()) else {
        return Vec::new();
    };
    for item in items.iter_mut() {
        let Some(id) = item.get("id").and_then(|v| v.as_str()).map(String::from) else {
            continue;
        };
        if !auth_ids.contains(&id) {
            continue;
        }
        if worklist_item_status(item) != "proposed" {
            continue;
        }
        let item_files: Vec<String> =
            if let Some(arr) = item.get("files").and_then(|v| v.as_array()) {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            } else if let Some(s) = item.get("file").and_then(|v| v.as_str()) {
                vec![s.to_string()]
            } else {
                Vec::new()
            };
        if item_files.iter().any(|f| touched.contains(f.as_str())) {
            if let Some(obj) = item.as_object_mut() {
                obj.insert(
                    "status".to_string(),
                    serde_json::Value::String("applied".to_string()),
                );
                advanced.push(id);
            }
        }
    }
    if advanced.is_empty() {
        return Vec::new();
    }
    let new_text = serde_json::to_string_pretty(&wl).unwrap_or_default();
    let payload = format!("{}\n", new_text);
    if let Err(e) = std::fs::write(&wl_path, &payload) {
        eprintln!("[auto-advance] write failed: {}", e);
        return Vec::new();
    }
    eprintln!("[auto-advance] flipped proposed→applied: {:?}", advanced);
    advanced
}

fn maybe_enforce_worklist_policy<R: tauri::Runtime>(
    app: &AppHandle<R>,
    prior_str: &str,
    current_str: &str,
) -> bool {
    let prior_doc: serde_json::Value = serde_json::from_str(prior_str).unwrap_or_default();
    let current_doc: serde_json::Value = serde_json::from_str(current_str).unwrap_or_default();
    let prior_items = worklist_items(&prior_doc);
    let current_items = worklist_items(&current_doc);
    let current_by_id: HashMap<String, &serde_json::Value> = current_items
        .iter()
        .filter_map(|item| worklist_item_id(item).map(|id| (id, item)))
        .collect();
    let auth = read_active_worklist_authorization(app);
    let auth_ids: std::collections::HashSet<String> = auth
        .as_ref()
        .map(|record| record.ids.iter().cloned().collect())
        .unwrap_or_default();
    let mut dropped_via_auth: Vec<String> = Vec::new();
    let mut violations: Vec<(String, String)> = Vec::new();

    for item in &prior_items {
        let Some(id) = worklist_item_id(item) else {
            continue;
        };
        if current_by_id.contains_key(&id) {
            continue;
        }
        let status = worklist_item_status(item).to_string();
        if status == "applied" {
            continue;
        }
        if auth.as_ref().map(|a| a.kind.as_str()) == Some("drop") && auth_ids.contains(&id) {
            dropped_via_auth.push(id);
            continue;
        }
        violations.push((id, status));
    }

    if violations.is_empty() {
        if !dropped_via_auth.is_empty() {
            // Agent-path-symmetry fix: when an agent (Codex) edits
            // worklist.json directly to prune a drop-authorized item
            // instead of going through /__worklist/resolve +
            // /__worklist/mutate, no inflight sentinel write/clear
            // fires and the iframe's `submitting=true` (set on click)
            // never gets cleared — the Worklist tab becomes
            // unselectable. Mirror what /resolve + /mutate would have
            // emitted: write the sentinel, then immediately clear it.
            // The two inflight-claim-changed events drive the iframe's
            // DataSource to refetch /__inflight, find no claim, and
            // clear local submitting state. Same outcome as the
            // Claude path; symmetric across agents.
            //
            // Harmless when invoked via the Claude path too — by the
            // time the policy validator runs after /mutate, the
            // sentinel has already been cleared, so write+clear here
            // is a small redundant pair of events. No state
            // divergence.
            write_inflight_claim_sentinel(app, &dropped_via_auth, "drop");
            clear_inflight_claim_sentinel(app, &dropped_via_auth);
            consume_worklist_authorization(app);
        }
        return true;
    }

    let Some(path) = worklist_file(app) else {
        return false;
    };
    let bad = violations
        .iter()
        .map(|(id, status)| format!("\"{}\" (status={})", id, status))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "[worklist-enforce] reverting unauthorized removal of {} via watcher fallback; last auth kind={}",
        bad,
        auth.as_ref().map(|a| a.kind.as_str()).unwrap_or("none")
    );
    if let Err(e) = std::fs::write(&path, prior_str) {
        eprintln!(
            "[worklist-enforce] failed to restore {}: {}",
            path.display(),
            e
        );
        return false;
    }
    false
}

// Computes the diff between two worklist snapshots and renders it as a
// markdown changelog. Returns None when no meaningful change is detected
// (no items proposed/advanced/renamed/pruned and description unchanged),
// so the caller can suppress trivial snapshots.
// Classify each diffed worklist change by the lifecycle phase it represents:
//   proposed:  new id introduced (status="proposed" or unset)
//   applied:   existing id transitioned status (was proposed, now applied)
//   committed: id removed AND auth record says kind="approved" for that id
//   dropped:   id removed AND auth record says kind="drop" for that id
// The 'renamed' and 'pruned' buckets from earlier versions were removed:
// rename_from was retired in 67eff19, and unauthorized prunes are already
// blocked by maybe_enforce_worklist_policy before they can reach the watcher,
// so every removal flows through either the approved or drop channel.
fn generate_worklist_changelog<R: tauri::Runtime>(
    app: &AppHandle<R>,
    prior: &serde_json::Value,
    current: &serde_json::Value,
    ts_ms: i64,
) -> Option<String> {
    let prior_items = worklist_items(prior);
    let current_items = worklist_items(current);
    let prior_by_id: HashMap<String, &serde_json::Value> = prior_items
        .iter()
        .filter_map(|i| worklist_item_id(i).map(|id| (id, i)))
        .collect();
    let current_by_id: HashMap<String, &serde_json::Value> = current_items
        .iter()
        .filter_map(|i| worklist_item_id(i).map(|id| (id, i)))
        .collect();

    // Read the auth record once so we can classify removals as committed,
    // dropped, or pruned. Failure to load returns None → all removals fall
    // back to the 'pruned' bucket.
    let auth: Option<WorklistAuthorizationRecord> = worklist_auth_file(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok());

    let mut proposed: Vec<&serde_json::Value> = Vec::new();
    let mut applied: Vec<(&serde_json::Value, String, String)> = Vec::new();

    for item in &current_items {
        let id = match worklist_item_id(item) {
            Some(id) => id,
            None => continue,
        };
        match prior_by_id.get(&id) {
            Some(prev) => {
                let prev_status = worklist_item_status(prev).to_string();
                let new_status = worklist_item_status(item).to_string();
                if prev_status != new_status {
                    applied.push((item, prev_status, new_status));
                }
            }
            None => {
                proposed.push(item);
            }
        }
    }

    let mut committed: Vec<&serde_json::Value> = Vec::new();
    let mut dropped: Vec<&serde_json::Value> = Vec::new();
    for item in &prior_items {
        let id = match worklist_item_id(item) {
            Some(id) => id,
            None => continue,
        };
        if current_by_id.contains_key(&id) {
            continue;
        }
        // Removal classification, in order:
        //   1. Prior status == "applied" → committed unconditionally. Matches
        //      the maybe_enforce_worklist_policy hook, which lets applied-status
        //      items prune freely (commit-then-prune is legitimate, no fresh
        //      auth required). Without this branch, a commit-prune whose auth
        //      record was overwritten between approve and prune (any subsequent
        //      drop:/approved: payload replaces the single-slot record) falls
        //      out of both buckets and the history UI shows "change".
        //   2. auth.kind == "drop" and id in auth.ids → dropped (user pruned
        //      a still-proposed item via the drop authorization channel).
        //   3. auth.kind == "approved" and id in auth.ids → committed (unusual:
        //      a proposed item was removed via approved auth without first
        //      transitioning to applied — included for completeness).
        //   4. else → skip silently; the policy hook would have reverted any
        //      other case before the watcher saw it.
        let prior_status = worklist_item_status(item).to_string();
        if prior_status == "applied" {
            committed.push(item);
            continue;
        }
        if let Some(rec) = auth.as_ref() {
            if rec.ids.iter().any(|i| i == &id) {
                match rec.kind.as_str() {
                    "drop" => dropped.push(item),
                    "approved" => committed.push(item),
                    _ => {}
                }
            }
        }
    }

    let prior_desc = worklist_description(prior);
    let current_desc = worklist_description(current);
    let description_changed = prior_desc != current_desc;

    // Suppress only when nothing meaningful happened. committed/dropped ARE
    // meaningful (they mark lifecycle endpoints), so a clean prune-on-commit
    // sequence now leaves a history entry instead of being silently skipped.
    if !description_changed
        && proposed.is_empty()
        && applied.is_empty()
        && committed.is_empty()
        && dropped.is_empty()
    {
        return None;
    }

    let mut out = String::new();
    out.push_str(&format!(
        "# Worklist change @ {} ({})\n\n",
        format_iso_utc(ts_ms),
        ts_ms
    ));
    let mut tallies: Vec<String> = Vec::new();
    if !proposed.is_empty() {
        tallies.push(format!("{} proposed", proposed.len()));
    }
    if !applied.is_empty() {
        tallies.push(format!("{} applied", applied.len()));
    }
    if !committed.is_empty() {
        tallies.push(format!("{} committed", committed.len()));
    }
    if !dropped.is_empty() {
        tallies.push(format!("{} dropped", dropped.len()));
    }
    if !tallies.is_empty() {
        out.push_str(&format!("**Summary:** {}\n\n", tallies.join(", ")));
    }

    if description_changed {
        out.push_str("## Description changed\n\n");
        out.push_str(&format!(
            "Was: `{}`\nNow: `{}`\n\n",
            prior_desc, current_desc
        ));
    }

    if !proposed.is_empty() {
        out.push_str("## Items proposed\n\n");
        for item in &proposed {
            let id = worklist_item_id(item).unwrap_or_default();
            out.push_str(&format!(
                "- `{}` ({}, `{}`)\n",
                id,
                worklist_item_status(item),
                worklist_item_str_field(item, "file")
            ));
            let before = worklist_item_str_field(item, "before");
            if !before.is_empty() {
                out.push_str(&format!("  - **Before:** {}\n", before.replace('\n', " ")));
            }
            let after = worklist_item_str_field(item, "after");
            if !after.is_empty() {
                out.push_str(&format!("  - **After:** {}\n", after.replace('\n', " ")));
            }
        }
        out.push('\n');
    }

    if !applied.is_empty() {
        out.push_str("## Items applied\n\n");
        for (item, from, to) in &applied {
            let id = worklist_item_id(item).unwrap_or_default();
            out.push_str(&format!("- `{}`: {} → {}\n", id, from, to));
        }
        out.push('\n');
    }

    let emit_removed_section = |out: &mut String, header: &str, items: &[&serde_json::Value]| {
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("## {}\n\n", header));
        for item in items {
            let id = worklist_item_id(item).unwrap_or_default();
            out.push_str(&format!(
                "- `{}` (was {}, `{}`)\n",
                id,
                worklist_item_status(item),
                worklist_item_str_field(item, "file")
            ));
            let before = worklist_item_str_field(item, "before");
            if !before.is_empty() {
                out.push_str(&format!("  - **Before:** {}\n", before.replace('\n', " ")));
            }
            let after = worklist_item_str_field(item, "after");
            if !after.is_empty() {
                out.push_str(&format!("  - **After:** {}\n", after.replace('\n', " ")));
            }
        }
        out.push('\n');
    };
    emit_removed_section(&mut out, "Items committed", &committed);
    emit_removed_section(&mut out, "Items dropped", &dropped);

    // Trailing-newline padding kept from the legacy bottom of the function.
    {
        out.push('\n');
    }

    Some(out)
}

// Called from the watcher when resources/worklist.json changes. Reads the
// current file, compares to the cached prior contents, and if different
// writes the *prior* contents to worklist-history/<unix_ms>.json plus a
// changelog .md. Best-effort: errors here must not break the underlying
// worklist write, which has already completed.
fn maybe_snapshot_worklist<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(file) = worklist_file(app) else {
        return;
    };
    let Some(history_dir) = worklist_history_dir(app) else {
        return;
    };
    let current_str = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(_) => return,
    };
    let cell = last_worklist_cell();
    let mut guard = match cell.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let prior_str = match guard.clone() {
        Some(s) => s,
        None => {
            // First observation — seed cache, no snapshot.
            *guard = Some(current_str);
            return;
        }
    };
    if prior_str == current_str {
        return;
    }
    if !maybe_enforce_worklist_policy(app, &prior_str, &current_str) {
        return;
    }
    // Always update the cache so the next change diffs against the most
    // recent contents, even when this change is suppressed below.
    *guard = Some(current_str.clone());

    let ts = unix_now_ms();
    let prior_doc: serde_json::Value = serde_json::from_str(&prior_str).unwrap_or_default();
    let current_doc: serde_json::Value = serde_json::from_str(&current_str).unwrap_or_default();
    let changelog = match generate_worklist_changelog(app, &prior_doc, &current_doc, ts) {
        Some(s) => s,
        None => {
            eprintln!("[worklist-history] suppressed trivial change @ {}", ts);
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&history_dir) {
        eprintln!("[worklist-history] create_dir_all failed: {}", e);
        return;
    }
    // Snapshot the POST-change state — each snapshot is a checkpoint of
    // the worklist as it stands at that moment. The .md changelog
    // describes the transition from the prior checkpoint.
    let snapshot_path = history_dir.join(format!("{}.json", ts));
    if let Err(e) = std::fs::write(&snapshot_path, &current_str) {
        eprintln!("[worklist-history] write snapshot failed: {}", e);
    }
    let changelog_path = history_dir.join(format!("{}.md", ts));
    if let Err(e) = std::fs::write(&changelog_path, changelog) {
        eprintln!("[worklist-history] write changelog failed: {}", e);
    }
    eprintln!(
        "[worklist-history] snapshot @ {} ({} bytes)",
        ts,
        current_str.len()
    );
}

fn init_worklist_cache<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(file) = worklist_file(app) else {
        return;
    };
    if let Ok(s) = std::fs::read_to_string(&file) {
        if let Ok(mut guard) = last_worklist_cell().lock() {
            *guard = Some(s);
        }
    }
}

// Routing for the right-pane HTTP server. Returns (status, content-type, body).
fn route_request<R: tauri::Runtime>(
    app: &AppHandle<R>,
    path: &str,
    query: &str,
) -> (u16, &'static str, Vec<u8>) {
    if path == "__context/list" {
        let mut provider: Option<SessionProvider> = None;
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("provider=") {
                provider = SessionProvider::from_str(&percent_decode(enc));
                break;
            }
        }
        let body = serde_json::to_vec(&context_list(app, provider)).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    if path == "__context/search" {
        let mut q = String::new();
        let mut provider: Option<SessionProvider> = None;
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("q=") {
                q = percent_decode(enc);
            } else if let Some(enc) = pair.strip_prefix("provider=") {
                provider = SessionProvider::from_str(&percent_decode(enc));
            }
        }
        let body = serde_json::to_vec(&context_search(app, provider, &q)).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    if path == "__context/file" {
        let mut file_path = String::new();
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("path=") {
                file_path = percent_decode(enc);
                break;
            }
        }
        let p = std::path::Path::new(&file_path);
        let result = match std::fs::read_to_string(p) {
            Ok(content) => serde_json::json!({ "content": content }),
            Err(e) => {
                eprintln!("[http /__context/file path={}] {}", file_path, e);
                serde_json::json!({ "content": "", "error": e.to_string() })
            }
        };
        let body = serde_json::to_vec(&result).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    if path == "__app-info" {
        let info = get_app_info();
        let body = serde_json::to_vec(&info).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    if path == "__right-pane-info" {
        #[derive(serde::Serialize)]
        struct RightPaneInfo<'a> {
            url: &'a str,
            default_right_pane: &'a str,
            spawned: Option<&'a ServerConfig>,
        }
        let pane_state = app.state::<PaneUrlsState>();
        let urls = pane_state.0.lock().unwrap();
        let spawn_state = app.state::<SpawnedServerState>();
        let spawned_guard = spawn_state.0.lock().unwrap();
        let info = RightPaneInfo {
            url: &urls.right_pane,
            default_right_pane: &urls.default_right_pane,
            spawned: spawned_guard.as_ref().map(|s| &s.config),
        };
        let body = serde_json::to_vec(&info).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    if path == "__restart-server" {
        use tauri::Emitter;

        let (cfg, pid) = {
            let spawn_state = app.state::<SpawnedServerState>();
            let mut guard = spawn_state.0.lock().unwrap();
            let Some(mut spawned) = guard.take() else {
                return (
                    400,
                    "text/plain; charset=utf-8",
                    b"no spawned project server".to_vec(),
                );
            };
            let cfg = spawned.config.clone();
            let Some(proj_root) = project_root(Some(app)) else {
                let body = serde_json::json!({
                    "ok": false,
                    "error": "no project root",
                });
                return (
                    500,
                    "application/json; charset=utf-8",
                    serde_json::to_vec(&body).unwrap_or_default(),
                );
            };

            let old_pid = spawned.child.id();
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
            eprintln!("[server] killed pid={} on manual restart", old_pid);

            match spawn_project_server(&cfg, &proj_root) {
                Ok(child) => {
                    let pid = child.id();
                    *guard = Some(SpawnedServer {
                        child,
                        config: cfg.clone(),
                    });
                    (cfg, pid)
                }
                Err(e) => {
                    eprintln!("[server] restart failed: {}", e);
                    let body = serde_json::json!({
                        "ok": false,
                        "error": e,
                    });
                    return (
                        500,
                        "application/json; charset=utf-8",
                        serde_json::to_vec(&body).unwrap_or_default(),
                    );
                }
            }
        };

        let port_up = wait_for_port(cfg.port, 5000);
        if !port_up {
            eprintln!(
                "[server] WARNING: restarted port {} did not come up within 5s; right-pane iframe will retry",
                cfg.port
            );
        } else {
            eprintln!("[server] restarted pid={}; port {} is up", pid, cfg.port);
        }
        trace_emit_signal(app, "right-pane-reload");
        let _ = app.emit("right-pane-reload", ());
        let body = serde_json::json!({
            "ok": true,
            "pid": pid,
            "port_up": port_up,
        });
        return (
            200,
            "application/json; charset=utf-8",
            serde_json::to_vec(&body).unwrap_or_default(),
        );
    }

    if path == "__error" {
        let mut reason = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("reason=") {
                reason = percent_decode(v);
                break;
            }
        }
        let escape = |s: &str| -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>Bram: project server unavailable</title>\
             <style>body{{font-family:system-ui,-apple-system,sans-serif;padding:32px;background:#1e1e1e;color:#e0e0e0;line-height:1.5}}\
             h1{{color:#ff7a7a;margin:0 0 16px;font-size:18px}}p{{margin:8px 0}}code{{background:#333;color:#e0e0e0;padding:2px 6px;border-radius:4px;font-family:Menlo,Monaco,monospace}}</style>\
             <h1>Bram: project server unavailable</h1>\
             <p>{}</p>",
            escape(&reason)
        );
        return (200, "text/html; charset=utf-8", html.into_bytes());
    }

    if path == "__commits" {
        return match git_log_recent(app, 30) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__commits] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__commits/search" {
        let mut q = String::new();
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("q=") {
                q = percent_decode(enc);
                break;
            }
        }
        return match git_log_search(app, &q) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__commits/search q={}] {}", q, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__issues" {
        return match gh_issues_list(app) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__issues] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__issues/search" {
        let mut q = String::new();
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("q=") {
                q = percent_decode(enc);
                break;
            }
        }
        return match gh_issues_search(app, &q) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__issues/search q={}] {}", q, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__issue" {
        let mut number: u64 = 0;
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("number=") {
                number = percent_decode(v).parse().unwrap_or(0);
                break;
            }
        }
        if number == 0 {
            return (400, "text/plain; charset=utf-8", b"missing number".to_vec());
        }
        return match gh_issue_view(app, number) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__issue number={}] {}", number, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__issue/comment" {
        let mut number: u64 = 0;
        let mut body = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("number=") {
                number = percent_decode(v).parse().unwrap_or(0);
            } else if let Some(v) = pair.strip_prefix("body=") {
                body = percent_decode(v);
            }
        }
        if number == 0 {
            return (400, "text/plain; charset=utf-8", b"missing number".to_vec());
        }
        return match gh_issue_comment(app, number, &body) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__issue/comment number={}] {}", number, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__issue/close" {
        let mut number: u64 = 0;
        let mut comment = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("number=") {
                number = percent_decode(v).parse().unwrap_or(0);
            } else if let Some(v) = pair.strip_prefix("comment=") {
                comment = percent_decode(v);
            }
        }
        if number == 0 {
            return (400, "text/plain; charset=utf-8", b"missing number".to_vec());
        }
        return match gh_issue_close(app, number, &comment) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__issue/close number={}] {}", number, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__repo/origin" {
        return match repo_origin_info(app) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__repo/origin] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__commit" {
        let mut sha = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("sha=") {
                sha = percent_decode(v);
                break;
            }
        }
        return match git_commit_detail(app, &sha) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__commit sha={}] {}", sha, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if path == "__file" {
        let mut file_path = String::new();
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("path=") {
                file_path = percent_decode(enc);
                break;
            }
        }
        let p = std::path::Path::new(&file_path);
        return match std::fs::read(p) {
            Ok(bytes) => (200, mime_for(p), bytes),
            Err(e) => {
                eprintln!("[http /__file path={}] {}", file_path, e);
                (404, "text/plain; charset=utf-8", Vec::new())
            }
        };
    }

    // Architectural experiment: derive-at-the-boundary for the
    // "last assistant text" panel. Iframe binds to this route's
    // {text} field instead of calling lastAssistantText(lastJsonl) and
    // walking the buffer per fanout. Refetch is event-driven via
    // talk-session-changed.
    if path == "__last-assistant-text" {
        return match read_last_assistant_text(app, None) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__last-assistant-text] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    // Companion to /__last-assistant-text: per-file edit aggregates for
    // the current turn. Same architecture (host parses 64 KB tail once
    // per request, iframe binds via DataSource), replaces the iframe's
    // currentTurnEdits(lastJsonl) helper which had started exceeding
    // XMLUI's 1000 ms sync-evaluation limit on busy turns.
    if path == "__current-turn-edits" {
        return match read_current_turn_edits(app, None) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__current-turn-edits] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    if let Some(rest) = path.strip_prefix("__sessions/") {
        let mut provider: Option<SessionProvider> = None;
        let mut session_id = String::new();
        let mut q = String::new();
        let mut scope = String::from("recent");
        let mut title = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("provider=") {
                provider = SessionProvider::from_str(&percent_decode(v));
            } else if let Some(v) = pair.strip_prefix("id=") {
                session_id = percent_decode(v);
            } else if let Some(v) = pair.strip_prefix("q=") {
                q = percent_decode(v);
            } else if let Some(v) = pair.strip_prefix("scope=") {
                scope = percent_decode(v);
            } else if let Some(v) = pair.strip_prefix("title=") {
                title = percent_decode(v);
            }
        }

        let (content_type, result): (&'static str, Result<Vec<u8>, String>) = if rest == "meta" {
            (
                "application/json; charset=utf-8",
                session_meta(app, provider)
                    .and_then(|meta| serde_json::to_vec(&meta).map_err(|e| e.to_string())),
            )
        } else if rest == "list" {
            (
                "application/json; charset=utf-8",
                list_sessions(app, provider)
                    .and_then(|entries| serde_json::to_vec(&entries).map_err(|e| e.to_string())),
            )
        } else if rest == "latest" {
            (
                "text/plain; charset=utf-8",
                read_latest_session(app, provider),
            )
        } else if rest == "latest-meta" {
            (
                "application/json; charset=utf-8",
                read_latest_session_meta(app, provider),
            )
        } else if rest == "latest-pending" {
            (
                "application/json; charset=utf-8",
                read_latest_session_pending(app, provider),
            )
        } else if rest == "latest-tail" {
            // Issue #100 / #71: diff-based response. Clients pass `?since=<N>`
            // and `?sid=<id>`; when sid matches the current latest session,
            // server returns bytes from offset `since` to EOF. Otherwise it
            // falls back to last-N-lines (or full file with `lines=all`).
            // Response is always a JSON envelope: { sid, offset, content, reset }
            // so the client can detect session rotation (sid change ⇒ reset)
            // and update its `since` cursor for the next poll.
            let mut lines_param: Option<String> = None;
            let mut since: u64 = 0;
            let mut expected_sid = String::new();
            for pair in query.split('&') {
                if let Some(v) = pair.strip_prefix("lines=") {
                    lines_param = Some(percent_decode(v));
                } else if let Some(v) = pair.strip_prefix("since=") {
                    since = percent_decode(v).parse().unwrap_or(0);
                } else if let Some(v) = pair.strip_prefix("sid=") {
                    expected_sid = percent_decode(v);
                }
            }
            // Resolve current latest session path; derive a stable sid from
            // the file stem so the diff response can carry it back to the client.
            let path_opt = latest_session_path(app, provider).unwrap_or(None);
            let (current_sid, file_size) = match &path_opt {
                Some(path) => {
                    let sid = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    (sid, size)
                }
                None => (String::new(), 0),
            };
            // since > 0 guards against the iframe reactivity race where
            // sessionSid is updated before sinceOffset — without this,
            // `since=0&sid=X` would read the whole file as an "incremental
            // delta from byte 0" (issue #100 smoke-test caught this).
            let incremental = !expected_sid.is_empty()
                && expected_sid == current_sid
                && since > 0
                && since <= file_size;
            let content_result: Result<Vec<u8>, String> = if incremental {
                match &path_opt {
                    Some(path) => {
                        use std::io::{Read, Seek, SeekFrom};
                        std::fs::File::open(path)
                            .map_err(|e| e.to_string())
                            .and_then(|mut f| {
                                f.seek(SeekFrom::Start(since)).map_err(|e| e.to_string())?;
                                let mut out =
                                    Vec::with_capacity((file_size - since) as usize);
                                f.read_to_end(&mut out).map_err(|e| e.to_string())?;
                                Ok(out)
                            })
                    }
                    None => Ok(Vec::new()),
                }
            } else {
                // Fresh fetch (no sid yet, or sid mismatch, or since past EOF).
                // Default-safe: lines absent or unparseable → last 200 records.
                // `lines=all` is the only path to the full file.
                match lines_param.as_deref() {
                    Some("all") => read_latest_session(app, provider),
                    None => read_latest_session_tail(app, provider, 200),
                    Some(s) => match s.parse::<usize>() {
                        Ok(n) => read_latest_session_tail(app, provider, n),
                        Err(_) => read_latest_session_tail(app, provider, 200),
                    },
                }
            };
            let result = content_result.and_then(|content| {
                let appended = content.len();
                eprintln!(
                    "[latest-tail] mode={} sid={} since={} eof={} bytes={}",
                    if incremental { "diff" } else { "fresh" },
                    current_sid,
                    since,
                    file_size,
                    appended,
                );
                let envelope = serde_json::json!({
                    "sid": current_sid,
                    "offset": file_size,
                    "content": String::from_utf8_lossy(&content).into_owned(),
                    // reset=true ⇒ client REPLACES its lastJsonl buffer.
                    // reset=false ⇒ client APPENDS content. Authoritative
                    // signal so the client doesn't have to infer from
                    // sid equality (handles file-shrink case too).
                    "reset": !incremental,
                });
                serde_json::to_vec(&envelope).map_err(|e| e.to_string())
            });
            ("application/json; charset=utf-8", result)
        } else if rest == "content" {
            (
                "text/plain; charset=utf-8",
                read_session(app, &session_id, provider),
            )
        } else if rest == "search" {
            let limit = if scope == "all" { usize::MAX } else { 10 };
            (
                "application/json; charset=utf-8",
                search_sessions(app, &q, limit, provider)
                    .and_then(|entries| serde_json::to_vec(&entries).map_err(|e| e.to_string())),
            )
        } else if rest == "delete" {
            (
                "application/json; charset=utf-8",
                delete_session(app, &session_id, provider),
            )
        } else if rest == "rename" {
            (
                "application/json; charset=utf-8",
                rename_session(app, &session_id, provider, &title),
            )
        } else {
            (
                "text/plain; charset=utf-8",
                read_session(app, rest, provider),
            )
        };
        return match result {
            Ok(bytes) => (200, content_type, bytes),
            Err(e) => {
                eprintln!("[http /__sessions/{}] {}", rest, e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    // Enhance status / action: tells the agent tools banner whether the
    // current project has the conventions sidecar + CLAUDE.md import.
    if path == "__enhance/status" {
        return match enhance_status(app) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__enhance/status] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }
    if path == "__enhance/run" {
        return match run_enhance(app) {
            Ok(bytes) => {
                eprintln!("[http /__enhance/run] wrote sidecar + updated CLAUDE.md");
                (200, "application/json; charset=utf-8", bytes)
            }
            Err(e) => {
                eprintln!("[http /__enhance/run] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }
    if path == "__enhance/codex-trust-ack" {
        return match write_codex_trust_ack() {
            Ok(()) => {
                trace_emit_signal(app, "enhance-status-changed");
                let _ = app.emit("enhance-status-changed", ());
                (
                    200,
                    "application/json; charset=utf-8",
                    br#"{"ok":true}"#.to_vec(),
                )
            }
            Err(e) => {
                eprintln!("[http /__enhance/codex-trust-ack] {}", e);
                (500, "text/plain; charset=utf-8", e.into_bytes())
            }
        };
    }

    // PTY rolling-buffer hex dump — for debugging menu detection.
    // Returns the last 2KB of PTY_TAIL as a hexdump (offsets, hex bytes,
    // ASCII gutter). Use to inspect what claude actually wrote when a
    // menu fails to render in the agent pane.
    if path == "__pty-tail" {
        let dump = match pty_tail_cell().lock() {
            Ok(tail) => {
                let n = tail.len();
                let start = n.saturating_sub(2048);
                hex_dump(&tail[start..])
            }
            Err(_) => String::from("(could not lock PTY_TAIL)\n"),
        };
        return (200, "text/plain; charset=utf-8", dump.into_bytes());
    }

    // strip_ansi'd view of PTY_TAIL — for inspecting exactly what the menu
    // detector matches against. Plain UTF-8 (lossy) so a human can read it.
    if path == "__pty-stripped" {
        let body = match pty_tail_cell().lock() {
            Ok(tail) => {
                let stripped = strip_ansi(&tail);
                String::from_utf8_lossy(&stripped).into_owned()
            }
            Err(_) => String::from("(could not lock PTY_TAIL)\n"),
        };
        return (200, "text/plain; charset=utf-8", body.into_bytes());
    }

    // PTY-tap menu detection — see pty_menu_update for rationale.
    // Returns {"menu": {"tool":..., "text":...}} when claude is currently
    // displaying its permission menu in the terminal, else {"menu": null}.
    if path == "__pty-menu" {
        let body = match pty_menu_cell().lock() {
            Ok(menu) => match &*menu {
                Some(m) => serde_json::json!({"menu": m}).to_string().into_bytes(),
                None => br#"{"menu":null}"#.to_vec(),
            },
            Err(_) => br#"{"menu":null}"#.to_vec(),
        };
        return (200, "application/json; charset=utf-8", body);
    }

    // /__worklist — same shape as /resources/worklist.json but with a
    // `diff` field injected on each `applied` item (the `git diff <file>`
    // output). The Workspace pane polls this so the TO COMMIT rows can
    // surface their pending diff inline.
    if path == "__worklist" {
        let mut doc = worklist_doc(app);
        if let Some(items) = doc.get_mut("items").and_then(|v| v.as_array_mut()) {
            for item in items {
                let status = item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("proposed")
                    .to_string();
                if status != "applied" {
                    continue;
                }
                // Item scope: prefer `files: [...]` array, fall back to the
                // legacy single `file: <string>` for backward compat.
                let file_paths: Vec<String> =
                    if let Some(arr) = item.get("files").and_then(|v| v.as_array()) {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .filter(|s| !s.is_empty())
                            .collect()
                    } else if let Some(s) = item.get("file").and_then(|v| v.as_str()) {
                        if s.is_empty() {
                            Vec::new()
                        } else {
                            vec![s.to_string()]
                        }
                    } else {
                        Vec::new()
                    };
                if file_paths.is_empty() {
                    continue;
                }
                let mut combined = String::new();
                for fp in &file_paths {
                    let mut diff = git_run(app, &["diff", "--", fp]).unwrap_or_default();
                    if diff.is_empty() {
                        // git diff returns nothing for untracked files. Fall back
                        // to --no-index against /dev/null, which always produces
                        // an "add the whole file" diff. That command exits 1 when
                        // it finds differences, so git_run would treat it as an
                        // error and discard stdout — shell out directly here.
                        if let Some(root) = project_root(Some(app)) {
                            if let Ok(out) = std::process::Command::new("git")
                                .current_dir(&root)
                                .args(&["diff", "--no-index", "--", "/dev/null", fp])
                                .output()
                            {
                                diff = String::from_utf8_lossy(&out.stdout).into_owned();
                            }
                        }
                    }
                    if !combined.is_empty() && !diff.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&diff);
                }
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("diff".to_string(), serde_json::Value::String(combined));
                }
            }
        }
        let body = serde_json::to_vec(&doc).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    if path == "__worklist/init" {
        return match init_worklist_file(app) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => (500, "text/plain; charset=utf-8", e.into_bytes()),
        };
    }

    // /__worklist/resolve[?ids=foo,bar] — verified-authorization endpoint
    // the agent reads instead of parsing the `approved:` / `drop:` turn
    // line. Returns the current `.worklist-authorization.json` body, with
    // optional id-filtering.
    //
    // Response kinds:
    //   - "approved" / "drop": active authorization, body carries items/ids.
    //     Approved records are consume-on-read here so a confused agent that
    //     reflexively calls the resolver on a non-authorization turn (iterate,
    //     talk) can't replay stale approval. Drop consumption stays in
    //     `maybe_enforce_worklist_policy` so authorized prunes aren't reverted.
    //   - "rejected_stale": on-disk worklist drifted between the user's click
    //     and the watcher reading it — agent should surface staleness.
    //   - "no_active_authorization": prior record has been consumed; the agent
    //     must NOT treat this as authorization. Returned for any consumed
    //     record regardless of original kind.
    if path == "__worklist/resolve" {
        let mut id_filter: Option<Vec<String>> = None;
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("ids=") {
                let decoded = percent_decode(enc);
                let parsed: Vec<String> = decoded
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !parsed.is_empty() {
                    id_filter = Some(parsed);
                }
                break;
            }
        }
        let Some(auth_path) = worklist_auth_file(app) else {
            return (
                404,
                "text/plain; charset=utf-8",
                b"no project root".to_vec(),
            );
        };
        let Ok(raw) = std::fs::read_to_string(&auth_path) else {
            return (
                404,
                "text/plain; charset=utf-8",
                b"no authorization record".to_vec(),
            );
        };
        let mut record_value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                return (
                    500,
                    "text/plain; charset=utf-8",
                    b"malformed authorization record".to_vec(),
                );
            }
        };
        let consumed_at = record_value
            .get("consumedAtMs")
            .and_then(|v| v.as_i64());
        if let Some(ts) = consumed_at {
            let body = serde_json::json!({
                "kind": "no_active_authorization",
                "consumedAtMs": ts,
            })
            .to_string()
            .into_bytes();
            return (200, "application/json; charset=utf-8", body);
        }
        if let Some(filter) = id_filter {
            if let Some(items) = record_value.get_mut("items").and_then(|v| v.as_array_mut()) {
                items.retain(|it| {
                    it.get("id")
                        .and_then(|v| v.as_str())
                        .map_or(false, |id| filter.iter().any(|f| f == id))
                });
            }
            if let Some(ids_v) = record_value.get_mut("ids").and_then(|v| v.as_array_mut()) {
                ids_v.retain(|v| {
                    v.as_str()
                        .map_or(false, |id| filter.iter().any(|f| f == id))
                });
            }
        }
        let kind = record_value
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Write the inflight sentinel for approved/drop (#84). Pulled
        // from the (possibly filter-narrowed) items array so the
        // sentinel's ids match what the agent has just been authorized
        // to act on. Cleared by /__worklist/mutate when the agent
        // completes the state transition.
        if kind == "approved" || kind == "drop" {
            let sentinel_ids: Vec<String> = record_value
                .get("items")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|it| {
                            it.get("id")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !sentinel_ids.is_empty() {
                write_inflight_claim_sentinel(app, &sentinel_ids, &kind);
            }
        }
        let body = record_value.to_string().into_bytes();
        if kind == "approved" {
            consume_worklist_authorization(app);
        }
        return (200, "application/json; charset=utf-8", body);
    }

    // /__inflight — host-managed inflight sentinel (#84). Returns the
    // contents of resources/.inflight-claim.json or `{}` if no claim is
    // active. The iframe refetches this on receipt of the
    // `inflight-claim-changed` Tauri event and derives spinner state
    // from the response (`ids` array, `kind`, `claimedAt`).
    if path == "__inflight" {
        let body = match inflight_claim_file(app) {
            Some(p) => std::fs::read_to_string(&p).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        };
        return (200, "application/json; charset=utf-8", body.into_bytes());
    }

    // /__pty-intent — diagnostic readout of the right-pane intent queue
    // (#86). Returns `{"queue": [<parsed-jsonl-line>, ...]}`. Empty
    // queue or missing file returns `{"queue": []}`. Read-only.
    if path == "__pty-intent" {
        let queue: Vec<serde_json::Value> = pty_intent_file(app)
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .map(|s| {
                s.lines()
                    .filter(|l| !l.is_empty())
                    .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                    .collect()
            })
            .unwrap_or_default();
        let body = serde_json::json!({ "queue": queue }).to_string();
        return (200, "application/json; charset=utf-8", body.into_bytes());
    }

    // /__git-diff?path=<file> — plain text `git diff -- <path>` output.
    if path == "__git-diff" {
        let mut file_path = String::new();
        for pair in query.split('&') {
            if let Some(enc) = pair.strip_prefix("path=") {
                file_path = percent_decode(enc);
                break;
            }
        }
        let diff = git_run(app, &["diff", "--", &file_path]).unwrap_or_default();
        return (200, "text/plain; charset=utf-8", diff.into_bytes());
    }

    // /__worklist-history/list — reverse-chronological list of snapshots
    // from resources/worklist-history/, each with a one-line summary
    // parsed from the .md changelog.
    if path == "__worklist-history/list" {
        let Some(dir) = worklist_history_dir(app) else {
            return (200, "application/json; charset=utf-8", b"[]".to_vec());
        };
        let mut json_files: Vec<(i64, PathBuf)> = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&dir) {
            for entry in read_dir.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e == "json") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(ts) = stem.parse::<i64>() {
                            json_files.push((ts, p));
                        }
                    }
                }
            }
        }
        json_files.sort_by(|a, b| b.0.cmp(&a.0));
        let mut entries: Vec<serde_json::Value> = Vec::new();
        for (ts, json_path) in json_files {
            let md_path = json_path.with_extension("md");
            let changelog = std::fs::read_to_string(&md_path).unwrap_or_default();
            let summary = changelog
                .lines()
                .find(|l| l.starts_with("**Summary:**"))
                .map(|l| l.trim_start_matches("**Summary:**").trim().to_string())
                .unwrap_or_else(|| {
                    // No item transitions in this snapshot. If the changelog
                    // recorded a description edit, label it explicitly rather
                    // than the generic fallback "change".
                    if changelog.contains("## Description changed") {
                        String::from("description changed")
                    } else {
                        String::from("change")
                    }
                });
            // Item ids are the leading-backtick tokens on changelog bullets.
            // Three shapes exist across the lifecycle:
            //   - `<id>` (was applied, `<path>`)        ← committed
            //   - `<id>` (proposed, `<path>`)           ← newly proposed
            //   - `<id>`: proposed → applied            ← applied (status change)
            // The discriminator after the closing backtick is either ` (` or
            // `: ` followed by one of the lifecycle status keywords, which
            // doesn't collide with backtick-bearing nested bullets in
            // before/after prose.
            let mut ids: Vec<String> = Vec::new();
            for line in changelog.lines() {
                if !line.starts_with("- `") {
                    continue;
                }
                let rest = &line[3..];
                if let Some(end) = rest.find('`') {
                    let after = &rest[end + 1..];
                    let looks_like_item = after.starts_with(" (was ")
                        || after.starts_with(" (proposed")
                        || after.starts_with(" (applied")
                        || after.starts_with(" (committed")
                        || after.starts_with(" (dropped")
                        || after.starts_with(": proposed")
                        || after.starts_with(": applied")
                        || after.starts_with(": committed")
                        || after.starts_with(": dropped");
                    if looks_like_item {
                        ids.push(rest[..end].to_string());
                    }
                }
            }
            // Fallback for snapshots with no item transitions (e.g.
            // description-only edits): read the sibling `.json` worklist
            // snapshot and surface the ids that were present at that moment,
            // so the row carries context instead of an empty "items" cell.
            if ids.is_empty() {
                if let Ok(raw) = std::fs::read_to_string(&json_path) {
                    if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&raw) {
                        if let Some(items) = doc.get("items").and_then(|v| v.as_array()) {
                            for item in items {
                                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                    ids.push(id.to_string());
                                }
                            }
                        }
                    }
                }
            }
            entries.push(serde_json::json!({
                "ts": ts,
                "iso": format_iso_utc(ts),
                "summary": summary,
                "ids": ids,
                "changelog": changelog,
            }));
        }
        let body = serde_json::to_vec(&entries).unwrap_or_default();
        return (200, "application/json; charset=utf-8", body);
    }

    // /__worklist-history/changelog?ts=<unix_ms> — raw .md body
    if path == "__worklist-history/changelog" {
        let mut ts = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("ts=") {
                ts = percent_decode(v);
                break;
            }
        }
        let Some(dir) = worklist_history_dir(app) else {
            return (404, "text/plain; charset=utf-8", Vec::new());
        };
        let p = dir.join(format!("{}.md", ts));
        return match std::fs::read(&p) {
            Ok(bytes) => (200, "text/markdown; charset=utf-8", bytes),
            Err(_) => (404, "text/plain; charset=utf-8", Vec::new()),
        };
    }

    // /__worklist-history/snapshot?ts=<unix_ms> — raw .json snapshot
    if path == "__worklist-history/snapshot" {
        let mut ts = String::new();
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("ts=") {
                ts = percent_decode(v);
                break;
            }
        }
        let Some(dir) = worklist_history_dir(app) else {
            return (404, "text/plain; charset=utf-8", Vec::new());
        };
        let p = dir.join(format!("{}.json", ts));
        return match std::fs::read(&p) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(_) => (404, "text/plain; charset=utf-8", Vec::new()),
        };
    }

    // worklist.json is the worklist convention. Treat it as always-present
    // (empty when the project hasn't opted in) so the Workspace tool can
    // poll without flooding devtools with 404s in guest projects.
    if path == "resources/worklist.json" {
        let proj = project_root(Some(app)).unwrap_or_else(|| PathBuf::from("."));
        return match std::fs::read(proj.join(path)) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(_) => (
                200,
                "application/json; charset=utf-8",
                empty_worklist_json().as_bytes().to_vec(),
            ),
        };
    }

    // System namespaces served from the binary's bundled app/ dir
    // (on-disk if present, embedded otherwise).
    let app_rel: Option<String> = if let Some(rest) = path.strip_prefix("__shell/") {
        Some(format!("__shell/{}", rest))
    } else if let Some(rest) = path.strip_prefix("__vendor/") {
        Some(format!("vendor/{}", rest))
    } else if let Some(rest) = path.strip_prefix("__tools/") {
        Some(format!("tools/{}", rest))
    } else {
        None
    };
    if let Some(rel) = app_rel {
        return match serve_app_file(Some(app), &rel) {
            Some((bytes, mime)) => (200, mime, bytes),
            None => (404, "text/plain; charset=utf-8", Vec::new()),
        };
    }

    // Project-relative paths everywhere else.
    let proj = project_root(Some(app)).unwrap_or_else(|| PathBuf::from("."));
    let full = proj.join(path);
    match std::fs::read(&full) {
        Ok(bytes) => (200, mime_for(&full), bytes),
        Err(_) => (404, "text/plain; charset=utf-8", Vec::new()),
    }
}

// POST /__worklist/mutate — server-side mechanical mutations (prune,
// advance status) symmetric to /__worklist/resolve. This is the
// canonical state-machine path for approval-driven worklist transitions;
// direct edits to resources/worklist.json remain for authoring/refining
// items, not for mechanical prune/advance. The agent issues a one-line
// curl instead of an Edit on resources/worklist.json, so the chat
// doesn't render a diff. Authorization is checked against
// resources/.worklist-authorization.json before the write: prune
// requires `kind: "drop"`, advance requires `kind: "approved"`, and
// every requested id must appear in the auth record's ids.
// POST /__iterate/begin — the agent calls this at the start of any
// iterate cycle. Writes the inflight sentinel with kind="iterate" and
// the ids from the iterate payload. Refs #84.
fn handle_iterate_begin<R: tauri::Runtime>(
    app: &AppHandle<R>,
    body: &[u8],
) -> (u16, &'static str, Vec<u8>) {
    let req_json: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                400,
                "application/json; charset=utf-8",
                format!("{{\"error\":\"invalid JSON: {}\"}}", e).into_bytes(),
            );
        }
    };
    let ids: Vec<String> = req_json
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return (
            400,
            "application/json; charset=utf-8",
            br#"{"error":"ids[] required"}"#.to_vec(),
        );
    }
    write_inflight_claim_sentinel(app, &ids, "iterate");
    (
        200,
        "application/json; charset=utf-8",
        br#"{"ok":true}"#.to_vec(),
    )
}

// POST /__iterate/end — the agent calls this at the end of any
// iterate cycle. Clears the sentinel if it fully covers the supplied
// ids (same coverage rule as /__worklist/mutate). Refs #84.
fn handle_iterate_end<R: tauri::Runtime>(
    app: &AppHandle<R>,
    body: &[u8],
) -> (u16, &'static str, Vec<u8>) {
    let req_json: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                400,
                "application/json; charset=utf-8",
                format!("{{\"error\":\"invalid JSON: {}\"}}", e).into_bytes(),
            );
        }
    };
    let ids: Vec<String> = req_json
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return (
            400,
            "application/json; charset=utf-8",
            br#"{"error":"ids[] required"}"#.to_vec(),
        );
    }
    clear_inflight_claim_sentinel(app, &ids);
    (
        200,
        "application/json; charset=utf-8",
        br#"{"ok":true}"#.to_vec(),
    )
}

fn handle_worklist_mutate<R: tauri::Runtime>(
    app: &AppHandle<R>,
    body: &[u8],
) -> (u16, &'static str, Vec<u8>) {
    let req_json: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                400,
                "application/json; charset=utf-8",
                format!("{{\"error\":\"invalid JSON: {}\"}}", e).into_bytes(),
            );
        }
    };
    let op = req_json.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let ids: Vec<String> = req_json
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return (
            400,
            "application/json; charset=utf-8",
            br#"{"error":"ids[] required"}"#.to_vec(),
        );
    }
    let required_kind = match op {
        "prune" => "drop",
        "advance" => "approved",
        _ => {
            return (
                400,
                "application/json; charset=utf-8",
                format!("{{\"error\":\"unknown op: {}\"}}", op).into_bytes(),
            );
        }
    };

    // Auth check. Deliberately ignores consumedAtMs: same-turn
    // resolve -> edit files -> mutate is valid, and resolve's
    // consume-on-read is only meant to block future resolver reads
    // from replaying stale approval.
    let auth_path = match worklist_auth_file(app) {
        Some(p) => p,
        None => {
            return (
                500,
                "application/json; charset=utf-8",
                br#"{"error":"no project root"}"#.to_vec(),
            );
        }
    };
    let auth: serde_json::Value = std::fs::read_to_string(&auth_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let auth_kind = auth.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    // For prune, also accept kind=approved (post-commit prune case);
    // applied-status check happens after the worklist read below.
    let kind_ok = auth_kind == required_kind || (op == "prune" && auth_kind == "approved");
    if !kind_ok {
        return (
            400,
            "application/json; charset=utf-8",
            format!(
                "{{\"error\":\"auth kind mismatch: expected {}{}, got {}\"}}",
                required_kind,
                if op == "prune" { " or approved" } else { "" },
                auth_kind
            )
            .into_bytes(),
        );
    }
    let auth_ids: Vec<String> = auth
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for id in &ids {
        if !auth_ids.iter().any(|aid| aid == id) {
            return (
                400,
                "application/json; charset=utf-8",
                format!("{{\"error\":\"id not in auth: {}\"}}", id).into_bytes(),
            );
        }
    }

    // Apply the op to worklist.json.
    let wl_path = match worklist_file(app) {
        Some(p) => p,
        None => {
            return (
                500,
                "application/json; charset=utf-8",
                br#"{"error":"no project root"}"#.to_vec(),
            );
        }
    };
    let mut wl: serde_json::Value = std::fs::read_to_string(&wl_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"items":[]}));
    let items = match wl.get_mut("items").and_then(|v| v.as_array_mut()) {
        Some(arr) => arr,
        None => {
            return (
                500,
                "application/json; charset=utf-8",
                br#"{"error":"worklist missing items[]"}"#.to_vec(),
            );
        }
    };

    // Post-commit prune safeguard: pruning with kind=approved is only
    // allowed when every requested id is already status=applied —
    // blocks an agent from pruning an as-yet-unapplied approved item,
    // which would lose the work.
    if op == "prune" && auth_kind == "approved" {
        for id in &ids {
            let item = items
                .iter()
                .find(|it| it.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
            let status = item
                .and_then(|it| it.get("status").and_then(|v| v.as_str()))
                .unwrap_or("proposed");
            if status != "applied" {
                return (
                    400,
                    "application/json; charset=utf-8",
                    format!(
                        "{{\"error\":\"post-commit prune requires applied status: {} is {}\"}}",
                        id, status
                    )
                    .into_bytes(),
                );
            }
        }
    }

    let mut affected: Vec<String> = Vec::new();
    if op == "prune" {
        items.retain(|item| {
            let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if ids.iter().any(|id| id == item_id) {
                affected.push(item_id.to_string());
                false
            } else {
                true
            }
        });
    } else {
        // advance
        let new_status = req_json
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("applied")
            .to_string();
        for item in items.iter_mut() {
            let item_id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if ids.iter().any(|id| id == &item_id) {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert(
                        "status".to_string(),
                        serde_json::Value::String(new_status.clone()),
                    );
                    affected.push(item_id);
                }
            }
        }
    }

    let new_text = serde_json::to_string_pretty(&wl).unwrap_or_default();
    if let Err(e) = std::fs::write(&wl_path, format!("{}\n", new_text)) {
        return (
            500,
            "application/json; charset=utf-8",
            format!("{{\"error\":\"write failed: {}\"}}", e).into_bytes(),
        );
    }

    // Inflight sentinel clear is no longer a side effect of mutate.
    // The sentinel now clears either when the agent calls
    // POST /__worklist/end (or /__iterate/end — same alias handler),
    // OR when the host-side `pty_agent_turn_update` silence-detector
    // fires a real turn-end (silence >= MIN_SILENCE_FOR_SENTINEL_CLEAR_MS).
    // Both anchor the spinner-clear to actual end-of-turn rather than
    // to the mid-turn state transition. Closes #91.

    let result_key = if op == "prune" { "pruned" } else { "advanced" };
    let response = format!(
        "{{\"ok\":true,\"{}\":{}}}",
        result_key,
        serde_json::to_string(&affected).unwrap_or_else(|_| "[]".to_string())
    );
    (
        200,
        "application/json; charset=utf-8",
        response.into_bytes(),
    )
}

fn handle_http<R: tauri::Runtime>(app: &AppHandle<R>, mut request: tiny_http::Request) {
    let url = request.url().to_string();
    let method = request.method().as_str().to_uppercase();
    let (raw_path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url.as_str(), ""),
    };
    let path = raw_path.trim_start_matches('/');

    let route_correlation_id = next_route_correlation_id();
    let route_start = std::time::Instant::now();
    trace_route_entry(app, &method, path, query, &route_correlation_id);

    // POST-only routes (route_request is GET-only).
    let (status, content_type, body) = if path == "__worklist/mutate" {
        if method != "POST" {
            (405, "text/plain; charset=utf-8", b"POST only".to_vec())
        } else {
            let mut buf = Vec::new();
            let _ = request.as_reader().read_to_end(&mut buf);
            handle_worklist_mutate(app, &buf)
        }
    } else if path == "__iterate/begin" {
        if method != "POST" {
            (405, "text/plain; charset=utf-8", b"POST only".to_vec())
        } else {
            let mut buf = Vec::new();
            let _ = request.as_reader().read_to_end(&mut buf);
            handle_iterate_begin(app, &buf)
        }
    } else if path == "__iterate/end" || path == "__worklist/end" {
        // `/__worklist/end` is the alias agents call as the last action
        // of approved/drop turns (closing the cycle the resolve handler
        // opened by writing the sentinel). Both names route through the
        // same kind-agnostic handler — the sentinel doesn't care
        // whether it was written with kind:"approved", "drop", or
        // "iterate", and the clear logic only needs the id set.
        // Closes #91.
        if method != "POST" {
            (405, "text/plain; charset=utf-8", b"POST only".to_vec())
        } else {
            let mut buf = Vec::new();
            let _ = request.as_reader().read_to_end(&mut buf);
            handle_iterate_end(app, &buf)
        }
    } else {
        route_request(app, path, query)
    };

    let body_size = body.len();
    trace_route_exit(
        app,
        &method,
        path,
        &route_correlation_id,
        status,
        body_size,
        route_start.elapsed().as_millis(),
    );

    let response = tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
        )
        .with_header(
            tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
        )
        .with_header(
            // Internal API endpoints serve live state (sessions JSONL, git
            // status, etc.). Browser HTTP caching would defeat polling.
            tiny_http::Header::from_bytes(
                &b"Cache-Control"[..],
                &b"no-store, no-cache, must-revalidate"[..],
            )
            .unwrap(),
        );
    let _ = request.respond(response);
}

// Shell-document origin string used as the prefix for URLs that load
// in the shell or in same-origin iframes. Tauri 2 presents the scheme
// differently per platform:
//   - macOS / iOS / Linux: `tauri://localhost/...`
//   - Windows / Android:   `http://tauri.localhost/...` (HTTP localhost
//     subdomain — browser policy treats this as a secure context, so
//     service workers work; the macOS form is custom-scheme and not a
//     secure context, so SW does not work there)
// The scheme NAME we register against (`tauri`) is the same across
// platforms; only the URL form the WebView sees changes. The iframe
// `src` we hand to the JS side must match the platform's actual form
// so navigations resolve to our scheme handler rather than 404.
#[cfg(any(target_os = "windows", target_os = "android"))]
const SHELL_ORIGIN: &str = "http://tauri.localhost";

#[cfg(not(any(target_os = "windows", target_os = "android")))]
const SHELL_ORIGIN: &str = "tauri://localhost";

// Tauri custom-scheme handler that overrides Tauri's default tauri://
// behavior. Tauri 2 skips registering its built-in handler when an
// app-level handler with the same scheme name is present (see
// tauri-2.11.0/src/manager/webview.rs:267). With our handler in place:
//
//   - `tauri://localhost/__project/*` is proxied to the upstream URL in
//     PaneUrlsState.right_pane_upstream (the loopback HTTP server by
//     default, or an external project dev server when .xmlui-desktop.json
//     declares one). The iframe's origin stays `tauri://localhost`, same
//     as the shell — same-origin policy then permits direct cross-frame
//     JS access. This is the whole point: shell and target can share
//     window globals (e.g., _xsLogs) without a postMessage bridge, and
//     the target reaches window.__TAURI__ directly.
//
//   - All other paths fall back to xd's own app/ tree via serve_app_file
//     (on-disk preferred, embedded fallback). Replicates Tauri's default
//     resource-loading behavior so the shell's own index.html, main.js,
//     vendor/*, __shell/*, __tools/* keep loading exactly as before.
//
// Hop-by-hop request and response headers are filtered out per RFC 7230.
// Connection failures to the upstream surface as 502 responses with a
// short error body so the iframe shows something rather than hanging.
fn handle_tauri_scheme<R: tauri::Runtime>(
    app: &AppHandle<R>,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    let uri = request.uri().clone();
    let path = uri.path();
    let rel = path.trim_start_matches('/');

    // Tier 1: /__project/* — project content escape hatch under the __
    // namespace. Strip prefix and proxy to right_pane_upstream (loopback
    // default or external dev server).
    if let Some(after) = rel.strip_prefix("__project/") {
        let upstream = {
            let state = app.state::<PaneUrlsState>();
            let urls = state.0.lock().unwrap();
            urls.right_pane_upstream.clone()
        };
        return proxy_to_target(upstream, after, uri.query(), request);
    }

    // Tier 2: shell assets from xd's app/ tree. Covers the shell's own
    // index.html, main.js, styles.css, vendor/*, and the tools pane at
    // tools/index.html + tools/components/*, tools/manual.md, etc.
    // Both panes can hit this directly via tauri://localhost/<path>.
    let app_rel = if rel.is_empty() { "index.html" } else { rel };
    if let Some((bytes, mime)) = serve_app_file(Some(app), app_rel) {
        return http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .body(bytes)
            .unwrap_or_else(|_| {
                http::Response::builder()
                    .status(500)
                    .body(Vec::new())
                    .unwrap()
            });
    }

    // Tier 3: other /__* paths — xd-internal HTTP endpoints (sessions,
    // worklist, app-info, file, error, enhance, restart-server, etc.).
    // These always live on the loopback regardless of which project is
    // loaded; the loopback's own routing (route_request in lib.rs) knows
    // how to serve them. Includes the __vendor / __shell namespaces,
    // which the loopback maps to xd's app/vendor and app/__shell when
    // serve_app_file misses (e.g., for the `__vendor` -> `vendor`
    // prefix-strip mapping in the loopback's handler).
    if rel.starts_with("__") {
        let loopback = {
            let state = app.state::<PaneUrlsState>();
            let urls = state.0.lock().unwrap();
            urls.loopback_origin.clone()
        };
        return proxy_to_target(loopback, rel, uri.query(), request);
    }

    // Tier 4: everything else — project content at a non-`__*` absolute
    // path (e.g., /xmlui/foo.js for xmlui-weather, /Main.xmlui,
    // /resources/foo.svg). Proxy to right_pane_upstream so external dev
    // servers also work.
    let upstream = {
        let state = app.state::<PaneUrlsState>();
        let urls = state.0.lock().unwrap();
        urls.right_pane_upstream.clone()
    };
    proxy_to_target(upstream, rel, uri.query(), request)
}

fn proxy_to_target(
    upstream_base: String,
    path_after_origin: &str,
    query: Option<&str>,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    let mut url = format!("{}{}", upstream_base, path_after_origin);
    if let Some(q) = query {
        url.push('?');
        url.push_str(q);
    }
    proxy_to_upstream(url, request)
}

fn proxy_to_upstream(url: String, request: http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
    let method = request.method().clone();
    let (parts, body) = request.into_parts();

    let mut req = ureq::request(method.as_str(), &url);
    for (name, value) in parts.headers.iter() {
        let name_str = name.as_str();
        // Skip hop-by-hop headers and Host (ureq sets Host automatically
        // from the request URL — forwarding the shell's `tauri.localhost`
        // would confuse the upstream).
        if is_hop_by_hop(name_str) || name_str.eq_ignore_ascii_case("host") {
            continue;
        }
        if let Ok(value_str) = value.to_str() {
            req = req.set(name_str, value_str);
        }
    }

    let result = if body.is_empty() {
        req.call()
    } else {
        req.send_bytes(&body)
    };

    let response = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[scheme-proxy] {} {} -> {}", method, url, e);
            return http::Response::builder()
                .status(502)
                .header("Content-Type", "text/plain")
                .body(format!("upstream proxy error for {}: {}", url, e).into_bytes())
                .unwrap();
        }
    };

    let status = response.status();
    // Snapshot headers before consuming response into a reader.
    let header_names = response.headers_names();
    let header_pairs: Vec<(String, String)> = header_names
        .iter()
        .filter_map(|name| response.header(name).map(|v| (name.clone(), v.to_string())))
        .collect();

    let mut body_bytes = Vec::new();
    if let Err(e) = response.into_reader().read_to_end(&mut body_bytes) {
        eprintln!("[scheme-proxy] read error from {}: {}", url, e);
        return http::Response::builder()
            .status(502)
            .header("Content-Type", "text/plain")
            .body(format!("upstream read error: {}", e).into_bytes())
            .unwrap();
    }

    let mut builder = http::Response::builder().status(status);
    for (name, value) in header_pairs {
        if is_hop_by_hop(&name) {
            continue;
        }
        builder = builder.header(&name, &value);
    }
    builder.body(body_bytes).unwrap_or_else(|_| {
        http::Response::builder()
            .status(502)
            .body(Vec::new())
            .unwrap()
    })
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
// Bump RLIMIT_NOFILE on Unix so tiny_http's accept loop doesn't panic
// on EMFILE during long sessions. macOS default soft limit is often 256,
// which is too low once the filesystem watcher, iframe scheme-proxy,
// session-JSONL pollers, etc. accumulate. Target 8192 (or hard limit
// if lower). No-op on Windows — different FD semantics. See issue #44.
#[cfg(unix)]
fn raise_open_files_limit() {
    use libc::{getrlimit, rlimit, setrlimit, RLIMIT_NOFILE};
    unsafe {
        let mut current = rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if getrlimit(RLIMIT_NOFILE, &mut current) != 0 {
            eprintln!("[rlimit] getrlimit failed; not bumping");
            return;
        }
        let target: libc::rlim_t = 8192;
        let new_cur = std::cmp::min(target, current.rlim_max);
        if new_cur <= current.rlim_cur {
            eprintln!(
                "[rlimit] soft={} already meets target={}; not bumping",
                current.rlim_cur, target
            );
            return;
        }
        let new_limits = rlimit {
            rlim_cur: new_cur,
            rlim_max: current.rlim_max,
        };
        if setrlimit(RLIMIT_NOFILE, &new_limits) == 0 {
            eprintln!(
                "[rlimit] bumped soft FD limit {} -> {} (hard={})",
                current.rlim_cur, new_cur, current.rlim_max
            );
        } else {
            eprintln!(
                "[rlimit] setrlimit failed; staying at soft={}",
                current.rlim_cur
            );
        }
    }
}

#[cfg(not(unix))]
fn raise_open_files_limit() {}

pub fn run() {
    raise_open_files_limit();
    parse_cli_flags();
    init_bram_trace_from_env();
    let initial_proj = determine_project_root();
    eprintln!("[bram] project root: {}", initial_proj.display());
    if !initial_proj.join("index.html").exists() {
        eprintln!(
            "[bram] WARNING: no index.html at project root; the right pane will fail to load. Run with `bram /path/to/project` or cd into the project before launching."
        );
    }
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("tauri", |ctx, request, responder| {
            // Offload to a thread so the WebView's request thread is not
            // blocked while the proxy fetches from the upstream. The
            // responder takes ownership and can be called from any thread.
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let response = handle_tauri_scheme(&app, request);
                responder.respond(response);
            });
        })
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .manage(WhisperState::default())
        .manage(SpawnedServerState::default())
        .manage(ActiveProjectState(Mutex::new(initial_proj)))
        .manage(PaneUrlsState(Mutex::new(PaneUrls::default())))
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            queue_pty_intent,
            pty_resize,
            log_from_right_pane,
            open_devtools,
            open_url,
            save_trace_export,
            capture_screenshot,
            git_push,
            get_right_pane_url,
            get_tools_pane_url,
            whisper_start,
            whisper_stop,
            whisper_status,
        ])
        .setup(move |app| {
            use tauri::Emitter;

            // Bind the right-pane HTTP server before anything else so the
            // URL is available the moment the parent shell asks for it.
            let server = tiny_http::Server::http("127.0.0.1:0")
                .map_err(|e| format!("failed to bind right-pane http server: {}", e))?;
            let port = server
                .server_addr()
                .to_ip()
                .map(|sa| sa.port())
                .ok_or("right-pane server bound to non-ip address")?;
            let _ = LOOPBACK_PORT.set(port);
            // Write the bound port as a plain decimal to a known file
            // so PTY-child agents can read the literal port via their
            // Read tool and substitute it into curl URLs — `${BRAM_PORT}`
            // expansion trips Claude Code's Bash permission matcher
            // (variable expansion is fragile per the permissions spec).
            // No cleanup on shutdown: overwritten on next startup;
            // staleness manifests as connection-refused, a clearer
            // signal than a misleading-but-valid path.
            if let Some(proj) = project_root(Some(app.handle())) {
                let port_path = proj.join("resources/.bram-port");
                if let Some(parent) = port_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&port_path, port.to_string()) {
                    Ok(()) => eprintln!("[bram] wrote port file: {}", port_path.display()),
                    Err(e) => eprintln!(
                        "[bram] failed to write port file {}: {}",
                        port_path.display(),
                        e
                    ),
                }
            }
            prepare_bram_trace_log(app.handle());
            // Remove any stale inflight sentinel from a prior session
            // that didn't complete cleanly. Refs #84.
            cleanup_stale_inflight_claim(app.handle());
            // Remove any stale pty-intent queue from a prior session so
            // its intents don't replay into the fresh PTY. Refs #86.
            cleanup_stale_pty_intents(app.handle());
            let internal_origin = format!("http://127.0.0.1:{}", port);
            eprintln!("[bram] internal HTTP server: {}", internal_origin);
            // Tools pane lives at xd's app/tools/index.html, served via
            // the scheme handler's Tier 2 (shell asset) directly. Same
            // origin as the shell. SHELL_ORIGIN picks the right URL form
            // per platform — see the const definition above.
            let tools_url = format!("{}/tools/index.html", SHELL_ORIGIN);

            // .bram.json may declare an external server for the
            // right pane. The tools-pane URL always points at the internal
            // loopback (so the drawer keeps working regardless).
            let project_cfg = project_root(Some(app.handle()))
                .as_deref()
                .and_then(load_project_config);
            let default_right_pane = format!("{}/index.html", internal_origin);
            let internal_base = format!("{}/", internal_origin);
            // `right_pane` is the URL the iframe loads. With the
            // tauri:// scheme proxy, it's always a same-origin URL
            // under /__project/. `right_pane_upstream` is the bare
            // origin (http://host:port/) the scheme handler proxies
            // to; the configured `cfg.path` (including any query)
            // gets spliced into the iframe URL so the browser's own
            // relative-URL resolution produces clean sub-resource
            // paths that pass through the proxy unchanged.
            let (right_pane_url, right_pane_upstream) = if let Some(cfg) = project_cfg.as_ref().and_then(|c| c.server.as_ref()) {
                let external_origin = format!("http://localhost:{}/", cfg.port);
                let right_pane_external = {
                    let path = if cfg.path.starts_with('/') {
                        cfg.path.clone()
                    } else {
                        format!("/{}", cfg.path)
                    };
                    format!("{}/__project{}", SHELL_ORIGIN, path)
                };
                match probe_port_http(cfg.port, &cfg.path) {
                    PortStatus::Live => {
                        eprintln!(
                            "[server] port {} is live (HTTP responsive); reusing (skipping spawn of `{}`)",
                            cfg.port, cfg.command
                        );
                        (right_pane_external.clone(), external_origin.clone())
                    }
                    PortStatus::Unresponsive(reason) => {
                        eprintln!(
                            "[server] port {} is in use but unresponsive ({}); refusing to reuse",
                            cfg.port, reason
                        );
                        eprintln!(
                            "[server] HINT: a previous server is likely wedged. Run `lsof -i :{}` to find the pid, then kill it and restart Bram.",
                            cfg.port
                        );
                        // Surface the problem in the iframe via /__error
                        // on the internal loopback (the scheme handler
                        // proxies there; the iframe's URL stays under
                        // /__project so origin remains tauri://localhost).
                        let error_path = format!(
                            "__project/__error?reason={}",
                            percent_encode(&format!(
                                "Port {} is in use but unresponsive ({}). The previous Bram session likely left an orphan process. Run `lsof -i :{}` and kill the listed pid, then restart Bram.",
                                cfg.port, reason, cfg.port
                            ))
                        );
                        (
                            format!("{}/{}", SHELL_ORIGIN, error_path),
                            internal_base.clone(),
                        )
                    }
                    PortStatus::NotListening => {
                        if let Some(root) = project_root(Some(app.handle())) {
                            match spawn_project_server(cfg, &root) {
                                Ok(child) => {
                                    *app.state::<SpawnedServerState>().0.lock().unwrap() =
                                        Some(SpawnedServer {
                                            child,
                                            config: cfg.clone(),
                                        });
                                    if !wait_for_port(cfg.port, 5000) {
                                        eprintln!(
                                            "[server] WARNING: port {} did not come up within 5s; right-pane iframe will retry",
                                            cfg.port
                                        );
                                    } else {
                                        eprintln!("[server] port {} is up", cfg.port);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[server] spawn failed: {} — falling back to internal URL", e);
                                }
                            }
                        }
                        (right_pane_external.clone(), external_origin.clone())
                    }
                }
            } else {
                (
                    format!("{}/__project/index.html", SHELL_ORIGIN),
                    internal_base.clone(),
                )
            };
            eprintln!("[bram] right pane URL: {}", right_pane_url);
            eprintln!("[bram] right pane upstream: {}", right_pane_upstream);
            eprintln!("[bram] tools pane URL: {}", tools_url);
            *app.state::<PaneUrlsState>().0.lock().unwrap() = PaneUrls {
                right_pane: right_pane_url,
                tools: tools_url,
                default_right_pane,
                right_pane_upstream,
                loopback_origin: internal_base.clone(),
            };

            let server_app = app.handle().clone();
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    let app = server_app.clone();
                    std::thread::spawn(move || handle_http(&app, request));
                }
            });

            let Some(proj_root) = project_root(Some(app.handle())) else {
                eprintln!("[watcher] could not resolve project root");
                return Ok(());
            };
            // Seed the worklist cache so the first detected change can
            // diff against the on-disk baseline rather than treating the
            // entire current file as "new".
            init_worklist_cache(app.handle());
            // Watch contract: events are emitted on two channels, NOT one.
            //   - "right-pane-reload" fires for changes inside proj_root only;
            //     main.js reloads the right-pane iframe alone. The agent
            //     tools drawer is poll-driven, so it does NOT need to reload
            //     when user-project files change. Keeping the drawer iframe
            //     stable here prevents postMessage-into-torn-down-iframe
            //     races on Approve/Drop clicks.
            //   - "tools-pane-reload" fires for changes under app/__shell,
            //     app/vendor, or app/tools; main.js reloads BOTH iframes
            //     (the drawer's own code changed, and the right pane may
            //     consume __shell/helpers.js too).
            // Do not collapse these back into a single event.
            let proj_root_path = proj_root.clone();
            let mut tools_pane_paths: Vec<std::path::PathBuf> = Vec::new();
            if let Some(app_root) = resolve_app_root(Some(app.handle())) {
                tools_pane_paths.push(app_root.join("__shell"));
                tools_pane_paths.push(app_root.join("vendor"));
                tools_pane_paths.push(app_root.join("tools"));
            } else {
                eprintln!("[watcher] no on-disk app/; using embedded tree (no app/ reload)");
            }
            // Provider session JSONLs get their own dispatch. Watch the
            // containing roots (not the file — the file rotates per session
            // and may not exist at startup) so the tools pane can refetch
            // immediately instead of waiting on fallback polling.
            let claude_sessions_dir = claude_sessions_dir(&app.handle()).ok();
            let codex_sessions_dir = home_dir().map(|h| h.join(".codex").join("sessions"));
            // Agent-hint file lives at <app_cache>/agent-hints/<encoded-cwd>.json
            // and is rewritten by the shell wrapper's _xmlui_mark_agent when
            // the user switches between `claude` and `codex`. Watching this
            // dir lets the agent-tools drawer refetch /__enhance/status when
            // the active provider flips.
            let agent_hints_dir = active_agent_hint_path(&app.handle())
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));
            // ~/.bram/ holds the codex-trust-ack marker. Ensure it exists at
            // startup so the watcher can attach to it — without this, deleting
            // the marker (the documented "force the banner back" gesture) would
            // not trigger a refetch and the iframe would keep the stale state.
            let bram_dir = home_dir().map(|h| h.join(".bram"));
            if let Some(ref bd) = bram_dir {
                let _ = std::fs::create_dir_all(bd);
            }
            let mut watch_paths: Vec<std::path::PathBuf> = vec![proj_root_path.clone()];
            watch_paths.extend(tools_pane_paths.iter().cloned());
            if let Some(ref sd) = claude_sessions_dir {
                if sd.exists() {
                    watch_paths.push(sd.clone());
                }
            }
            if let Some(ref sd) = codex_sessions_dir {
                if sd.exists() {
                    watch_paths.push(sd.clone());
                }
            }
            if let Some(ref bd) = bram_dir {
                if bd.exists() {
                    watch_paths.push(bd.clone());
                }
            }
            if let Some(ref ah) = agent_hints_dir {
                if ah.exists() {
                    watch_paths.push(ah.clone());
                }
            }
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
                use std::sync::mpsc::channel;
                use std::time::{Duration, Instant};

                let (tx, rx) = channel::<notify::Result<Event>>();
                let mut watcher = match recommended_watcher(tx) {
                    Ok(w) => w,
                    Err(e) => {
                        eprintln!("[watcher] init failed: {}", e);
                        return;
                    }
                };
                for watch_path in &watch_paths {
                    if let Err(e) = watcher.watch(watch_path, RecursiveMode::Recursive) {
                        eprintln!("[watcher] watch {:?} failed: {}", watch_path, e);
                        return;
                    }
                    eprintln!("[watcher] watching {:?}", watch_path);
                }

                let mut last_emit = Instant::now() - Duration::from_secs(1);
                let mut last_config_emit = Instant::now() - Duration::from_secs(1);
                // Debounce tools-pane reloads: defer the emit until 500ms
                // after the last tools-pane event, so a rapid burst of
                // saves coalesces into a single reload (single flash
                // instead of N). Other channels stay immediate.
                let tools_debounce = Duration::from_millis(500);
                let mut pending_tools_since: Option<Instant> = None;
                // Debounce worklist-changed emits: notify-rs produces ~6
                // events per atomic rename + modify of worklist.json
                // (and worklist-history snapshots), and the iframe's
                // worklist-changed listener triggers /__worklist and
                // /__worklist-history/list refetches per event. A 200ms
                // debounce coalesces the cascade into one emit per
                // logical change. Refs #85.
                let worklist_debounce = Duration::from_millis(200);
                let mut pending_worklist_since: Option<Instant> = None;
                use std::sync::mpsc::RecvTimeoutError;
                loop {
                    let res = rx.recv_timeout(Duration::from_millis(100));
                    // Always check the pending tools emit before processing
                    // the new event — a recv_timeout wake with no event is
                    // the typical trigger for firing a deferred reload.
                    if let Some(since) = pending_tools_since {
                        if since.elapsed() >= tools_debounce {
                            eprintln!("[watcher] change detected, emitting tools-pane-reload (debounced)");
                            emit_or_defer_tools_pane_reload(&app_handle);
                            pending_tools_since = None;
                        }
                    }
                    if let Some(since) = pending_worklist_since {
                        if since.elapsed() >= worklist_debounce {
                            eprintln!("[watcher] change detected, emitting worklist-changed (debounced)");
                            trace_emit_signal(&app_handle, "worklist-changed");
                            let _ = app_handle.emit("worklist-changed", ());
                            pending_worklist_since = None;
                        }
                    }
                    let event = match res {
                        Ok(Ok(ev)) => ev,
                        Ok(Err(_)) => continue,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let in_ignored_dir = |p: &std::path::Path| {
                        let igs = [".git", "target", "node_modules", "resources"];
                        p.components()
                            .any(|c| igs.iter().any(|ig| c.as_os_str() == *ig))
                    };

                    // [watcher] trace: one record per path per notify
                    // event, before any dispatch. Logs project-relative
                    // paths; absolute / outside-project paths fall back
                    // to file_name only so no host filesystem layout
                    // leaks into the log.
                    //
                    // Skip the live trace log and dated trace archives
                    // to avoid self-feeding loop / reload noise: the
                    // live file is written by append_bram_trace_line,
                    // and startup archiving may create
                    // resources/bram-trace-YYYY-MM-DD*.log files.
                    // Neither should emit more watcher trace or reload
                    // behavior. See fix-watcher-trace-self-feeding-loop.
                    if bram_trace_enabled() {
                        let change = notify_event_kind_label(&event.kind);
                        for p in &event.paths {
                            if p.starts_with(&proj_root_path) && in_ignored_dir(p) {
                                continue;
                            }
                            let rel = p
                                .strip_prefix(&proj_root_path)
                                .ok()
                                .map(|r| r.to_string_lossy().replace('\\', "/"))
                                .unwrap_or_else(|| {
                                    p.file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("")
                                        .to_string()
                                });
                            if rel == "resources/bram-trace.log"
                                || is_bram_trace_archive_rel(&rel)
                            {
                                continue;
                            }
                            append_bram_trace_line(
                                &app_handle,
                                "watcher",
                                &format!("path={} change={} dedup=false", rel, change),
                            );
                        }
                    }

                    // Session JSONL changes get their own dispatch. The
                    // Transcript / Workspace panes subscribe to
                    // talk-session-changed and refetch without waiting on
                    // the regular fallback poll interval.
                    let is_session_event = event.paths.iter().any(|p| {
                        p.extension().map_or(false, |e| e == "jsonl")
                            && (claude_sessions_dir.as_ref().map_or(false, |sd| p.starts_with(sd))
                                || codex_sessions_dir.as_ref().map_or(false, |sd| p.starts_with(sd)))
                    });
                    if is_session_event {
                        // [jsonl] trace: ground-truth signal that the
                        // agent is producing structured output. Lets
                        // #78 analysis tell a premature `[turn-end]`
                        // from a real one — a premature fire is
                        // followed within seconds by another `[jsonl]`
                        // line, proving the agent was still working.
                        // One trace line per jsonl path per watcher
                        // event (the filesystem batches at write-flush
                        // granularity, which is the cadence we want).
                        if bram_trace_enabled() {
                            for p in &event.paths {
                                if p.extension().map_or(false, |e| e == "jsonl") {
                                    let name = p
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("");
                                    let size =
                                        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                                    append_bram_trace_line(
                                        &app_handle,
                                        "jsonl",
                                        &format!("file={} bytes={}", name, size),
                                    );
                                }
                            }
                        }
                        // JSONL-driven turn-end detection (#91 follow-up).
                        // Parses each changed JSONL for a Claude
                        // `stop_reason: "end_turn"` last-line marker; if
                        // present and the sentinel is claimed, clears it
                        // directly. More reliable than the PTY-silence
                        // path for cycles where the agent has multi-second
                        // pauses between bursts. The silence-driven path
                        // remains as fallback (gated by
                        // MIN_SILENCE_FOR_SENTINEL_CLEAR_MS); if this
                        // detector fires first, the silence-driven clear
                        // is a no-op because the sentinel is already gone.
                        for p in &event.paths {
                            if p.extension().map_or(false, |e| e == "jsonl") {
                                check_jsonl_for_turn_end(&app_handle, p);
                            }
                        }
                        // Removed the 100ms leading-edge debounce: it
                        // suppressed the FINAL write of an agent
                        // response burst (the one that flips
                        // isWaitingForAssistant to false), wedging the
                        // tools-pane spinner + disabled input until the
                        // next user activity. XMLUI dedupes refetches
                        // via structural sharing, so burst-emitting per
                        // event is fine.
                        trace_emit_signal(&app_handle, "talk-session-changed");
                        let _ = app_handle.emit("talk-session-changed", ());
                        continue;
                    }

                    // enhance-status (agent-setup state) changes when any
                    // of these files are touched. The tools-pane
                    // Main.xmlui listens for `enhance-status-changed` and
                    // refetches /__enhance/status, replacing the prior
                    // 2-second poll that ran while the user was typing.
                    // Not a `continue` — the regular tools-pane reload
                    // still fires on its normal path below.
                    let is_enhance_event = event.paths.iter().any(|p| {
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        let in_claude_dir = p.components().any(|c| c.as_os_str() == ".claude");
                        let in_agent_hints = agent_hints_dir
                            .as_ref()
                            .map_or(false, |ah| p.starts_with(ah));
                        let in_bram_dir = bram_dir.as_ref().map_or(false, |bd| p.starts_with(bd));
                        name == "CLAUDE.md"
                            || name == "AGENTS.md"
                            || (in_claude_dir
                                && (name == "settings.json"
                                    || name == "settings.local.json"
                                    || name == "worklist-guard.py"))
                            || in_agent_hints
                            || in_bram_dir
                    });
                    if is_enhance_event {
                        trace_emit_signal(&app_handle, "enhance-status-changed");
                        let _ = app_handle.emit("enhance-status-changed", ());
                    }

                    // sessions-list-changed: any mutation under either
                    // Claude or Codex sessions dir — broader than
                    // talk-session-changed (which is JSONL-write-only).
                    // Sessions.xmlui listens for this to refetch its
                    // list (renames, deletes, new sessions all surface).
                    let is_sessions_list_event = event.paths.iter().any(|p| {
                        claude_sessions_dir
                            .as_ref()
                            .map_or(false, |sd| p.starts_with(sd))
                            || codex_sessions_dir
                                .as_ref()
                                .map_or(false, |sd| p.starts_with(sd))
                    });
                    if is_sessions_list_event {
                        trace_emit_signal(&app_handle, "sessions-list-changed");
                        let _ = app_handle.emit("sessions-list-changed", ());
                    }

                    // worklist-changed: resources/worklist.json or any
                    // file under resources/worklist-history/. Workspace
                    // refetches its DataSources on this.
                    let is_worklist_change = event.paths.iter().any(|p| {
                        let in_resources = p.components().any(|c| c.as_os_str() == "resources");
                        let file = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        let in_history = p
                            .components()
                            .any(|c| c.as_os_str() == "worklist-history");
                        in_resources && (file == "worklist.json" || in_history)
                    });
                    if is_worklist_change {
                        // Defer the emit; pending_worklist_since either
                        // starts a fresh 200 ms window or rolls forward
                        // on a continuing burst. The loop's wake check
                        // above drains it when the window elapses. Refs
                        // #85 worklist-watcher-debounce.
                        pending_worklist_since = Some(Instant::now());
                    }

                    // Auto-advance proposed→applied when an authorized write
                    // covers a still-proposed item. Closes the state-machine
                    // gate that the convention alone can't enforce (#60). The
                    // resulting worklist.json write surfaces via the
                    // is_worklist_change branch on the next event cycle, which
                    // emits worklist-changed and snapshots the transition.
                    let project_rel_paths: Vec<String> = event
                        .paths
                        .iter()
                        .filter_map(|p| p.strip_prefix(&proj_root_path).ok())
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !project_rel_paths.is_empty() {
                        let _ = try_auto_advance_proposed_items(&app_handle, &project_rel_paths);
                    }

                    // git-status-changed: any project file change that's
                    // not in the standard ignored directories. Commits
                    // refetches its log. Noisier than the others but
                    // bounded by the watcher's existing 100ms debounce.
                    let is_git_status_event = event.paths.iter().any(|p| {
                        p.starts_with(&proj_root_path) && !in_ignored_dir(p)
                    });
                    if is_git_status_event {
                        trace_emit_signal(&app_handle, "git-status-changed");
                        let _ = app_handle.emit("git-status-changed", ());
                    }

                    // .bram.json gets its own dispatch: we have to
                    // process it on any event kind (editors atomic-save via
                    // rename, which arrives as Create or Remove rather than
                    // Modify), and its handler may need to respawn the
                    // project server before reloading the iframe.
                    let is_config_event = event.paths.iter().any(|p| {
                        is_project_config_path(&p)
                    });
                    if is_config_event {
                        if last_config_emit.elapsed() < Duration::from_millis(300) {
                            continue;
                        }
                        last_config_emit = Instant::now();
                        eprintln!("[project-config] change detected");
                        handle_project_config_reload(&app_handle, &proj_root_path);
                        continue;
                    }

                    if !matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    ) {
                        continue;
                    }
                    // Worklist history capture. Worklist writes are otherwise
                    // skipped by the ignored-resources rule below (they
                    // shouldn't reload the iframe — the DataSource polls).
                    // Detect them here, snapshot the prior contents, then
                    // fall through to the normal skip.
                    let is_worklist_event = event.paths.iter().any(|p| {
                        p.ends_with("worklist.json")
                            && p.components().any(|c| c.as_os_str() == "resources")
                    });
                    if is_worklist_event {
                        maybe_snapshot_worklist(&app_handle);
                    }
                    // Skip events whose paths are entirely inside noisy or
                    // data-only directories. resources/ is data the DataSource
                    // polls; target/, .git/, node_modules/ are build/VCS noise.
                    if event.paths.iter().all(|p| {
                        p.starts_with(&proj_root_path) && in_ignored_dir(p)
                    }) {
                        continue;
                    }
                    // Editors often write twice (atomic save). Debounce 100ms.
                    if last_emit.elapsed() < Duration::from_millis(100) {
                        continue;
                    }
                    last_emit = Instant::now();
                    // Classify: any path under a tools_pane_paths root → tools event.
                    // Otherwise (paths only under proj_root) → right-pane-only event.
                    // Skip doc-only changes (e.g. conventions.md edits)
                    // from triggering a tools-pane-reload. They don't run
                    // code; the rebuild would only churn live UI state.
                    let is_tools_event = event.paths.iter().any(|p| {
                        let is_doc = p.extension().map_or(false, |e| e == "md");
                        !is_doc && tools_pane_paths.iter().any(|tp| p.starts_with(tp))
                    });
                    if is_tools_event {
                        // Defer the emit; pending_tools_since either starts
                        // the debounce window or resets it on burst writes.
                        pending_tools_since = Some(Instant::now());
                    } else {
                        eprintln!("[watcher] change detected, emitting right-pane-reload");
                        trace_emit_signal(&app_handle, "right-pane-reload");
                        let _ = app_handle.emit("right-pane-reload", ());
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                {
                    let state = app.state::<WhisperState>();
                    let mut guard = state.0.lock().unwrap();
                    if let Some(mut child) = guard.take() {
                        let pid = child.id();
                        let _ = child.kill();
                        let _ = child.wait();
                        eprintln!("[whisper] killed pid={} on exit", pid);
                    }
                }
                {
                    let state = app.state::<SpawnedServerState>();
                    let mut guard = state.0.lock().unwrap();
                    if let Some(mut spawned) = guard.take() {
                        let pid = spawned.child.id();
                        let _ = spawned.child.kill();
                        let _ = spawned.child.wait();
                        eprintln!("[server] killed pid={} on exit", pid);
                    }
                }
            }
        });
}
