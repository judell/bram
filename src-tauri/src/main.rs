// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Reentrant guard mode (bram-guard-reentrant-menu-hooks): `bram guard
    // <hook>` answers a provider hook on stdin and exits before any Tauri,
    // webview, PTY, port-file, or trace side effect can run.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "guard" {
        std::process::exit(bram_lib::guard::run_guard_mode(&args[2..]));
    }
    bram_lib::run()
}
