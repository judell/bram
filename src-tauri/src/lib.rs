use std::borrow::Cow;
use std::io::{Read, Write};
use std::path::PathBuf;
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
            let app_root = resolve_app_root(Some(ctx.app_handle()))
                .unwrap_or_else(|| PathBuf::from("."));
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
