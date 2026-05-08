use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

use include_dir::{include_dir, Dir};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

// The `app/` tree is embedded in the binary at compile time so
// release artifacts ship as a single self-contained file. We
// deliberately do *not* reuse Tauri's asset_resolver (which also
// embeds `app/` via frontendDist) because that resolver
// SPA-fallbacks unknown paths to index.html — disastrous for
// XMLUI's optional code-behind probes that legitimately 404. The
// duplication costs ~6MB; the reliability is worth it.
static EMBEDDED_APP: Dir = include_dir!("$CARGO_MANIFEST_DIR/../app");

// Resolve a path within `app/` to (bytes, mime). When on-disk app/
// exists, that is ground truth — a missing file is genuinely missing.
// Only fall back to the embedded tree when there is no on-disk app/.
fn serve_app_file<R: tauri::Runtime>(
    app: Option<&AppHandle<R>>,
    rel: &str,
) -> Option<(Vec<u8>, &'static str)> {
    if let Some(root) = resolve_app_root(app) {
        let p = root.join(rel);
        return std::fs::read(&p).ok().map(|bytes| (bytes, mime_for(&p)));
    }
    EMBEDDED_APP
        .get_file(rel)
        .map(|file| (file.contents().to_vec(), mime_for(std::path::Path::new(rel))))
}

// Resolve a path within `app/` to a real on-disk path. If the
// on-disk app_root is present, returns app_root/rel directly. Else
// extracts the embedded file into a per-binary cache dir and returns
// that path. Used for things that need a real filesystem path —
// bash --rcfile, etc. — not just bytes.
fn extract_app_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    rel: &str,
) -> Result<PathBuf, String> {
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

fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home, rest);
        }
    }
    p.to_string()
}

// Active project root — resolved once at startup from a CLI arg
// (xmlui-desktop /path/to/project) or std::env::current_dir(). Read by
// the HTTP server, watcher, git/sessions/PTY commands.
struct ActiveProjectState(Mutex<PathBuf>);

// Base URL of the loopback HTTP server hosting the right pane. Populated
// in setup() once the listener is bound. Service workers (used by MSW
// and xmlui's apiInterceptor) require an http(s) secure-context origin,
// which a custom URI scheme cannot provide — hence the move from
// xmlui:// to http://127.0.0.1:<port>.
struct RightPaneUrlState(Mutex<String>);

fn determine_project_root() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let candidate: PathBuf = if args.len() >= 2 && !args[1].starts_with('-') {
        PathBuf::from(&args[1])
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    candidate.canonicalize().unwrap_or(candidate)
}

fn parse_cli_flags() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return;
    }
    match args[1].as_str() {
        "-h" | "--help" => {
            println!(
                "Usage: xmlui-desktop [PROJECT_DIR]\n\n\
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
            println!("xmlui-desktop {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        s if s.starts_with('-') => {
            eprintln!("xmlui-desktop: unknown option '{}'", s);
            eprintln!("Try 'xmlui-desktop --help' for more information.");
            std::process::exit(1);
        }
        _ => {}
    }
}

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

    let count_arg = format!("-n{}", count);
    let format = "--format=%H%x09%an%x09%aI%x09%s";
    let log_out = git_run(app, &["log", &count_arg, format])?;

    let mut commits: Vec<serde_json::Value> = Vec::new();
    for line in log_out.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() != 4 {
            continue;
        }
        let sha = parts[0].to_string();
        let pushed = !unpushed.contains(&sha);
        let html_url = if html_base.is_empty() {
            String::new()
        } else {
            format!("{}/commit/{}", html_base, sha)
        };
        commits.push(serde_json::json!({
            "sha": sha,
            "html_url": html_url,
            "pushed": pushed,
            "commit": {
                "author": { "name": parts[1], "date": parts[2] },
                "message": parts[3],
            },
        }));
    }
    serde_json::to_vec(&commits).map_err(|e| e.to_string())
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
    let detail = serde_json::json!({
        "sha": sha,
        "stats": { "additions": total_add, "deletions": total_del },
        "files": files_json,
    });
    serde_json::to_vec(&detail).map_err(|e| e.to_string())
}

fn resolve_app_root<R: tauri::Runtime>(app: Option<&AppHandle<R>>) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(app) = app {
        if let Ok(resource_dir) = app.path().resource_dir() {
            candidates.push(resource_dir.join("app"));
        }
        if let Ok(executable_dir) = app.path().executable_dir() {
            candidates.push(executable_dir.join("app"));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("app"));
            candidates.push(dir.join("../Resources/app"));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("app"));
        candidates.push(cwd.join("..").join("app"));
    }

    candidates.into_iter().find(|path| path.exists())
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
    } else if let Ok(home) = std::env::var("HOME") {
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
        command.env("XMLUI_DESKTOP_AGENT_HINT", hint_path.to_string_lossy().into_owned());
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

    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if on_data.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn pty_write(data: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.0.lock().unwrap();
    let pty = guard.as_mut().ok_or("pty not started")?;
    pty.writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    pty.writer.flush().map_err(|e| e.to_string())
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

#[derive(serde::Serialize)]
struct WhisperStatusReport {
    running: bool,
    pid: Option<u32>,
}

#[tauri::command]
fn whisper_start(
    model_path: String,
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
    let candidates = ["whisper-server", "/opt/homebrew/bin/whisper-server"];
    let mut last_err = String::new();
    for bin in &candidates {
        match std::process::Command::new(bin)
            .arg("-m")
            .arg(&model)
            .arg("--convert")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                let pid = child.id();
                eprintln!("[whisper] spawned {} pid={} -m {}", bin, pid, model);
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
fn log_from_right_pane(payload: serde_json::Value) {
    eprintln!("[right-pane] {}", payload);
}

#[tauri::command]
fn get_right_pane_url(state: State<'_, RightPaneUrlState>) -> String {
    state.0.lock().unwrap().clone()
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
    git_run(&app, &["push"]).map(|_| ())
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
fn save_trace_export(filename: String, content: String, mime_type: String) -> Result<String, String> {
    let safe_name = filename
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '_',
            _ => c,
        })
        .collect::<String>();

    let base_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
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
    let abs = root.canonicalize().map_err(|e| e.to_string())?;
    let encoded = abs.to_string_lossy().replace('/', "-");
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

fn claude_sessions_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let root = project_root(Some(app)).ok_or("could not resolve project root")?;
    let abs = root.canonicalize().map_err(|e| e.to_string())?;
    let encoded = abs.to_string_lossy().replace('/', "-");
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(PathBuf::from(home).join(".claude").join("projects").join(encoded))
}

// Best-effort label for a session: prefers the most recent custom-title record
// (set via /rename), falls back to a snippet of the first user message.
fn claude_session_title(path: &Path) -> std::io::Result<Option<String>> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut custom_title: Option<String> = None;
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
    Ok(custom_title.or(first_user))
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
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let path = PathBuf::from(home).join(".codex").join("session_index.jsonl");
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
        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
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
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    let sessions_root = PathBuf::from(home).join(".codex").join("sessions");
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
    let (_, sessions) = sessions_for_provider(app, preferred)?;
    Ok(sessions
        .into_iter()
        .enumerate()
        .map(|(idx, session)| SessionEntry {
            id: session.id,
            mtime: session.mtime,
            size: session.size,
            title: session.title,
            provider: session.provider,
            current: idx == 0,
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

fn read_latest_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    preferred: Option<SessionProvider>,
) -> Result<Vec<u8>, String> {
    let (_, sessions) = sessions_for_provider(app, preferred)?;
    let session = sessions.first().ok_or("no sessions found")?;
    std::fs::read(&session.path).map_err(|e| e.to_string())
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
const ENHANCE_HOOK_SCRIPT_REL: &str = ".claude/hooks/proposal-guard.py";
const ENHANCE_SETTINGS_REL: &str = ".claude/settings.json";
const ENHANCE_HOOK_BUNDLE_REL: &str = "__shell/proposal-guard.py";
const ENHANCE_HOOK_COMMAND: &str = "$CLAUDE_PROJECT_DIR/.claude/hooks/proposal-guard.py";
// Presence of this file in the project root means the project IS the
// xmlui-desktop source repo (it bundles the conventions). enhance_status
// treats it as a valid sidecar location; run_enhance skips the parts
// that would otherwise self-overwrite the source.
const ENHANCE_SOURCE_BUNDLE_REL: &str = "app/__shell/conventions.md";

fn settings_has_proposal_guard_hook(settings_path: &Path) -> bool {
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
                                .map(|s| s.contains("proposal-guard.py"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

// Idempotent merge: append a PreToolUse hook entry referencing
// proposal-guard.py to settings.json, preserving other keys. Returns
// Ok(true) if the entry was added, Ok(false) if it was already present.
fn merge_proposal_guard_into_settings(settings_path: &Path) -> Result<bool, String> {
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
    let already_present = pre_arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hs| {
                hs.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|s| s.contains("proposal-guard.py"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });
    if already_present {
        return Ok(false);
    }
    pre_arr.push(serde_json::json!({
        "matcher": "Write|Edit",
        "hooks": [{
            "type": "command",
            "command": ENHANCE_HOOK_COMMAND,
        }]
    }));
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

fn enhance_status<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    let proj = project_root(Some(app)).ok_or("no project root")?;
    let claude_md = proj.join("CLAUDE.md");
    let sidecar = proj.join(ENHANCE_SIDECAR_REL);
    let hook_script = proj.join(ENHANCE_HOOK_SCRIPT_REL);
    let settings = proj.join(ENHANCE_SETTINGS_REL);
    let claude_md_has_marker = std::fs::read_to_string(&claude_md)
        .map(|s| s.contains(ENHANCE_MARKER_START))
        .unwrap_or(false);
    // Source repo treats the bundle itself as the canonical sidecar.
    let sidecar_exists = sidecar.exists() || proj.join(ENHANCE_SOURCE_BUNDLE_REL).exists();
    let hook_script_exists = hook_script.exists();
    let hook_registered = settings_has_proposal_guard_hook(&settings);
    let body = serde_json::json!({
        "enhanced": claude_md_has_marker && sidecar_exists && hook_script_exists && hook_registered,
        "claudeMd": claude_md_has_marker,
        "sidecar": sidecar_exists,
        "hookScript": hook_script_exists,
        "hookRegistered": hook_registered,
        "claudeMdPath": claude_md.display().to_string(),
        "sidecarPath": sidecar.display().to_string(),
        "hookScriptPath": hook_script.display().to_string(),
        "settingsPath": settings.display().to_string(),
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
        std::fs::write(&sidecar_path, &conventions)
            .map_err(|e| format!("write sidecar: {}", e))?;
        wrote.push(sidecar_path.display().to_string());
    }

    // Proposal-guard hook script (idempotent — same content on re-run).
    let (hook_bytes, _mime) = serve_app_file(Some(app), ENHANCE_HOOK_BUNDLE_REL)
        .ok_or_else(|| "proposal-guard.py bundle not found".to_string())?;
    let hook_path = proj.join(ENHANCE_HOOK_SCRIPT_REL);
    if let Some(parent) = hook_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&hook_path, &hook_bytes)
        .map_err(|e| format!("write hook: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)
            .map_err(|e| format!("stat hook: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)
            .map_err(|e| format!("chmod hook: {}", e))?;
    }
    wrote.push(hook_path.display().to_string());

    // Register hook in settings.json (idempotent merge).
    let settings_path = proj.join(ENHANCE_SETTINGS_REL);
    merge_proposal_guard_into_settings(&settings_path)?;
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

    let body = serde_json::json!({
        "enhanced": true,
        "isSourceRepo": is_source_repo,
        "wrote": wrote,
    });
    serde_json::to_vec(&body).map_err(|e| e.to_string())
}

// Routing for the right-pane HTTP server. Returns (status, content-type, body).
fn route_request<R: tauri::Runtime>(
    app: &AppHandle<R>,
    path: &str,
    query: &str,
) -> (u16, &'static str, Vec<u8>) {
    if path == "__commits" {
        return match git_log_recent(app, 30) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(e) => {
                eprintln!("[http /__commits] {}", e);
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

    if let Some(rest) = path.strip_prefix("__sessions/") {
        let mut provider: Option<SessionProvider> = None;
        let mut session_id = String::new();
        let mut q = String::new();
        let mut scope = String::from("recent");
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("provider=") {
                provider = SessionProvider::from_str(&percent_decode(v));
            } else if let Some(v) = pair.strip_prefix("id=") {
                session_id = percent_decode(v);
            } else if let Some(v) = pair.strip_prefix("q=") {
                q = percent_decode(v);
            } else if let Some(v) = pair.strip_prefix("scope=") {
                scope = percent_decode(v);
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
            ("text/plain; charset=utf-8", read_latest_session(app, provider))
        } else if rest == "content" {
            ("text/plain; charset=utf-8", read_session(app, &session_id, provider))
        } else if rest == "search" {
            let limit = if scope == "all" { usize::MAX } else { 10 };
            (
                "application/json; charset=utf-8",
                search_sessions(app, &q, limit, provider)
                    .and_then(|entries| serde_json::to_vec(&entries).map_err(|e| e.to_string())),
            )
        } else {
            ("text/plain; charset=utf-8", read_session(app, rest, provider))
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

    // proposal.json is the worklist convention. Treat it as always-present
    // (empty when the project hasn't opted in) so the Workspace tool can
    // poll without flooding devtools with 404s in guest projects.
    if path == "resources/proposal.json" {
        let proj = project_root(Some(app)).unwrap_or_else(|| PathBuf::from("."));
        return match std::fs::read(proj.join(path)) {
            Ok(bytes) => (200, "application/json; charset=utf-8", bytes),
            Err(_) => (
                200,
                "application/json; charset=utf-8",
                br#"{"description":"","items":[]}"#.to_vec(),
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

fn handle_http<R: tauri::Runtime>(app: &AppHandle<R>, request: tiny_http::Request) {
    let url = request.url().to_string();
    let (raw_path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url.as_str(), ""),
    };
    let path = raw_path.trim_start_matches('/');
    let (status, content_type, body) = route_request(app, path, query);

    let response = tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
        )
        .with_header(
            tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
        );
    let _ = request.respond(response);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    parse_cli_flags();
    let initial_proj = determine_project_root();
    eprintln!("[xmlui-desktop] project root: {}", initial_proj.display());
    if !initial_proj.join("index.html").exists() {
        eprintln!(
            "[xmlui-desktop] WARNING: no index.html at project root; the right pane will fail to load. Run with `xmlui-desktop /path/to/project` or cd into the project before launching."
        );
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .manage(WhisperState::default())
        .manage(ActiveProjectState(Mutex::new(initial_proj)))
        .manage(RightPaneUrlState(Mutex::new(String::new())))
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            log_from_right_pane,
            open_devtools,
            open_url,
            save_trace_export,
            git_push,
            get_right_pane_url,
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
            let url = format!("http://127.0.0.1:{}", port);
            eprintln!("[xmlui-desktop] right pane HTTP server: {}", url);
            *app.state::<RightPaneUrlState>().0.lock().unwrap() = url;

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
            let mut watch_paths: Vec<std::path::PathBuf> = vec![proj_root_path.clone()];
            watch_paths.extend(tools_pane_paths.iter().cloned());
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
                for res in rx {
                    let Ok(event) = res else { continue };
                    if !matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    ) {
                        continue;
                    }
                    // Skip events whose paths are entirely inside noisy or
                    // data-only directories. resources/ is data the DataSource
                    // polls; target/, .git/, node_modules/ are build/VCS noise.
                    let ignored = ["resources", "target", ".git", "node_modules"];
                    if event.paths.iter().all(|p| {
                        p.components().any(|c| {
                            ignored.iter().any(|ig| c.as_os_str() == *ig)
                        })
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
                    let is_tools_event = event.paths.iter().any(|p| {
                        tools_pane_paths.iter().any(|tp| p.starts_with(tp))
                    });
                    if is_tools_event {
                        eprintln!("[watcher] change detected, emitting tools-pane-reload");
                        let _ = app_handle.emit("tools-pane-reload", ());
                    } else {
                        eprintln!("[watcher] change detected, emitting right-pane-reload");
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
                let state = app.state::<WhisperState>();
                let mut guard = state.0.lock().unwrap();
                if let Some(mut child) = guard.take() {
                    let pid = child.id();
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("[whisper] killed pid={} on exit", pid);
                }
            }
        });
}
