use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

struct PtyState {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

#[derive(Default)]
struct AppState(Mutex<Option<PtyState>>);

// Active project root — resolved once at startup from a CLI arg
// (xmlui-desktop /path/to/project) or std::env::current_dir(). Read by
// the URI handler, watcher, git/sessions/PTY commands.
struct ActiveProjectState(Mutex<PathBuf>);

fn determine_project_root() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let candidate: PathBuf = if args.len() >= 2 && !args[1].starts_with('-') {
        PathBuf::from(&args[1])
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    candidate.canonicalize().unwrap_or(candidate)
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
    for a in args {
        command.arg(a);
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

#[tauri::command]
fn log_from_right_pane(payload: serde_json::Value) {
    eprintln!("[right-pane] {}", payload);
}

#[tauri::command]
fn open_devtools(window: tauri::WebviewWindow) {
    #[cfg(debug_assertions)]
    window.open_devtools();
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
}

fn sessions_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let root = project_root(Some(app)).ok_or("could not resolve project root")?;
    let abs = root.canonicalize().map_err(|e| e.to_string())?;
    let encoded = abs.to_string_lossy().replace('/', "-");
    let home = std::env::var("HOME").map_err(|_| "no HOME")?;
    Ok(PathBuf::from(home).join(".claude").join("projects").join(encoded))
}

// Best-effort label for a session: prefers the most recent custom-title record
// (set via /rename), falls back to a snippet of the first user message.
fn session_title(path: &Path) -> std::io::Result<Option<String>> {
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

fn message_text(record: &serde_json::Value) -> String {
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

fn search_sessions<R: tauri::Runtime>(
    app: &AppHandle<R>,
    query: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let dir = sessions_dir(app)?;
    let q_lower = q.to_lowercase();

    let mut entries: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        entries.push((path, modified, meta.len()));
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    entries.truncate(limit);

    let mut results: Vec<serde_json::Value> = Vec::new();
    for (path, modified, size) in entries {
        let title = session_title(&path).ok().flatten();
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let mut all_text = String::new();
        for line in content.lines() {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let role = record.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            let text = message_text(&record);
            if !text.is_empty() {
                all_text.push_str(&text);
                all_text.push('\n');
            }
        }
        let snippets = find_snippets(&all_text, &q_lower, 3);
        if !snippets.is_empty() {
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let mtime_secs = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            results.push(serde_json::json!({
                "id": id,
                "title": title,
                "mtime": mtime_secs,
                "size": size,
                "snippets": snippets,
            }));
        }
    }
    Ok(results)
}

fn list_sessions<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<SessionEntry>, String> {
    let dir = sessions_dir(app)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = metadata.len();
        let title = session_title(&path).ok().flatten();
        entries.push(SessionEntry { id, mtime, size, title });
    }
    entries.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    Ok(entries)
}

fn read_session<R: tauri::Runtime>(app: &AppHandle<R>, id: &str) -> Result<Vec<u8>, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("invalid session id".to_string());
    }
    let dir = sessions_dir(app)?;
    let path = dir.join(format!("{}.jsonl", id));
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    let dir_canon = dir.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&dir_canon) {
        return Err("path traversal".to_string());
    }
    std::fs::read(&resolved).map_err(|e| e.to_string())
}

fn read_latest_session<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Vec<u8>, String> {
    let dir = sessions_dir(app)?;
    let mut latest: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        match &latest {
            Some((_, t)) if modified <= *t => {}
            _ => latest = Some((path, modified)),
        }
    }
    let (path, _) = latest.ok_or("no sessions found")?;
    std::fs::read(&path).map_err(|e| e.to_string())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_proj = determine_project_root();
    eprintln!("[xmlui-desktop] project root: {}", initial_proj.display());
    if !initial_proj.join("index.html").exists() {
        eprintln!(
            "[xmlui-desktop] WARNING: no index.html at project root; the right pane will fail to load. Run with `xmlui-desktop /path/to/project` or cd into the project before launching."
        );
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .register_uri_scheme_protocol("xmlui", move |ctx, request| {
            let path = request.uri().path().trim_start_matches('/');
            let query = request.uri().query().unwrap_or("");
            let app = ctx.app_handle();

            // Local-git commit list with pushed/unpushed flags.
            if path == "__commits" {
                return match git_log_recent(app, 30) {
                    Ok(bytes) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json; charset=utf-8")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Cow::Owned(bytes))
                        .unwrap(),
                    Err(e) => {
                        eprintln!("[xmlui://__commits] {}", e);
                        tauri::http::Response::builder()
                            .status(500)
                            .body(Cow::Owned(e.into_bytes()))
                            .unwrap()
                    }
                };
            }

            // Local-git per-commit detail (mirrors GitHub API shape enough to
            // reuse the existing inline-diff renderer).
            if path == "__commit" {
                let mut sha = String::new();
                for pair in query.split('&') {
                    if let Some(v) = pair.strip_prefix("sha=") {
                        sha = percent_decode(v);
                        break;
                    }
                }
                return match git_commit_detail(app, &sha) {
                    Ok(bytes) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json; charset=utf-8")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Cow::Owned(bytes))
                        .unwrap(),
                    Err(e) => {
                        eprintln!("[xmlui://__commit sha={}] {}", sha, e);
                        tauri::http::Response::builder()
                            .status(500)
                            .body(Cow::Owned(e.into_bytes()))
                            .unwrap()
                    }
                };
            }

            // Serve arbitrary local files (used by the Sessions browser to
            // render inline image attachments inside the xmlui:// origin,
            // since file:// can't load cross-origin).
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
                    Ok(bytes) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", mime_for(p))
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Cow::Owned(bytes))
                        .unwrap(),
                    Err(e) => {
                        eprintln!("[xmlui://__file path={}] {}", file_path, e);
                        tauri::http::Response::builder()
                            .status(404)
                            .body(Cow::Owned(Vec::new()))
                            .unwrap()
                    }
                };
            }

            // Special path prefix routes session-data requests through the
            // same scheme as the static assets, avoiding cross-origin CORS.
            if let Some(rest) = path.strip_prefix("__sessions/") {
                let (content_type, result): (&str, Result<Vec<u8>, String>) = if rest == "list" {
                    (
                        "application/json; charset=utf-8",
                        list_sessions(app).and_then(|entries| {
                            serde_json::to_vec(&entries).map_err(|e| e.to_string())
                        }),
                    )
                } else if rest == "latest" {
                    ("text/plain; charset=utf-8", read_latest_session(app))
                } else if rest == "search" {
                    let mut q = String::new();
                    let mut scope = String::from("recent");
                    for pair in query.split('&') {
                        if let Some(v) = pair.strip_prefix("q=") {
                            q = percent_decode(v);
                        } else if let Some(v) = pair.strip_prefix("scope=") {
                            scope = percent_decode(v);
                        }
                    }
                    let limit = if scope == "all" { usize::MAX } else { 10 };
                    (
                        "application/json; charset=utf-8",
                        search_sessions(app, &q, limit).and_then(|entries| {
                            serde_json::to_vec(&entries).map_err(|e| e.to_string())
                        }),
                    )
                } else {
                    ("text/plain; charset=utf-8", read_session(app, rest))
                };
                return match result {
                    Ok(bytes) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", content_type)
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Cow::Owned(bytes))
                        .unwrap(),
                    Err(e) => {
                        eprintln!("[xmlui://__sessions/{}] {}", rest, e);
                        tauri::http::Response::builder()
                            .status(500)
                            .header("Content-Type", "text/plain; charset=utf-8")
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Cow::Owned(e.into_bytes()))
                            .unwrap()
                    }
                };
            }

            // System namespaces served from the binary's bundled app/ dir.
            // Project-relative paths everywhere else.
            let app_root = resolve_app_root(Some(app)).unwrap_or_else(|| PathBuf::from("."));
            let full: PathBuf = if let Some(rest) = path.strip_prefix("__shell/") {
                app_root.join("__shell").join(rest)
            } else if let Some(rest) = path.strip_prefix("__vendor/") {
                app_root.join("vendor").join(rest)
            } else {
                let proj = project_root(Some(app)).unwrap_or_else(|| PathBuf::from("."));
                proj.join(path)
            };
            match std::fs::read(&full) {
                Ok(bytes) => tauri::http::Response::builder()
                    .status(200)
                    .header("Content-Type", mime_for(&full))
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Cow::Owned(bytes))
                    .unwrap(),
                Err(_) => tauri::http::Response::builder()
                    .status(404)
                    .body(Cow::Owned(Vec::new()))
                    .unwrap(),
            }
        })
        .manage(AppState::default())
        .manage(ActiveProjectState(Mutex::new(initial_proj)))
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            log_from_right_pane,
            open_devtools,
            open_url,
            save_trace_export,
            git_push,
        ])
        .setup(move |app| {
            use tauri::Emitter;

            let Some(app_root) = resolve_app_root(Some(app.handle())) else {
                eprintln!("[watcher] could not resolve app root");
                return Ok(());
            };
            let Some(proj_root) = project_root(Some(app.handle())) else {
                eprintln!("[watcher] could not resolve project root");
                return Ok(());
            };
            let watch_paths = vec![
                proj_root,
                app_root.join("__shell"),
                app_root.join("vendor"),
            ];
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
                    eprintln!("[watcher] change detected, emitting right-pane-reload");
                    let _ = app_handle.emit("right-pane-reload", ());
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
