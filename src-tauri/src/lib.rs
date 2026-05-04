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
    let project_root = resolve_app_root(Some(&app))
        .and_then(|app_root| app_root.parent().map(|parent| parent.to_path_buf()));
    if let Some(root) = project_root {
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
fn open_url(url: String, app: AppHandle) -> Result<(), String> {
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
    let project_root = resolve_app_root(Some(app))
        .and_then(|app_root| app_root.parent().map(|p| p.to_path_buf()))
        .ok_or("could not resolve project root")?;
    let abs = project_root.canonicalize().map_err(|e| e.to_string())?;
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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .register_uri_scheme_protocol("xmlui", move |ctx, request| {
            let path = request.uri().path().trim_start_matches('/');
            let app = ctx.app_handle();

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

            let app_root = resolve_app_root(Some(app)).unwrap_or_else(|| PathBuf::from("."));
            let full = app_root.join(path);
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
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            log_from_right_pane,
            open_devtools,
            open_url,
            save_trace_export,
        ])
        .setup(move |app| {
            use tauri::Emitter;

            let Some(app_root) = resolve_app_root(Some(app.handle())) else {
                eprintln!("[watcher] could not resolve app root");
                return Ok(());
            };
            let watch_paths = vec![app_root.join("right"), app_root.join("vendor")];
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
                    // Skip changes under right/resources/ — those are data files
                    // (proposal.json etc.) that DataSources poll on their own.
                    // Reloading the iframe for them wipes in-flight UI state
                    // (e.g., the Pending-items gray-out).
                    if event.paths.iter().all(|p| {
                        p.components()
                            .any(|c| c.as_os_str() == "resources")
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
