use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let canonical: PathBuf = [&manifest_dir, "..", ".claude", "hooks", "worklist-guard.py"]
        .iter()
        .collect();
    let bundle: PathBuf = [&manifest_dir, "..", "app", "__shell", "worklist-guard.py"]
        .iter()
        .collect();

    if !canonical.exists() {
        panic!(
            "worklist-guard canonical not found at {}; refusing to build a bundle from a stale or missing source",
            canonical.display()
        );
    }
    std::fs::copy(&canonical, &bundle).unwrap_or_else(|e| {
        panic!(
            "failed to sync worklist-guard from {} to {}: {}",
            canonical.display(),
            bundle.display(),
            e
        )
    });

    println!("cargo:rerun-if-changed=../.claude/hooks/worklist-guard.py");

    tauri_build::build()
}
