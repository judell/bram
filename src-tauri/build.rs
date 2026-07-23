use std::path::PathBuf;

// Sync an installed .claude/hooks/<name> copy from its canonical
// app/provider-hooks/<name> source on every build, so editing the canonical
// source resyncs the runtime copy (the source repo's installed hook never
// goes stale). Provider hook adapters carry provider-prefixed names (#217).
fn sync_hook(manifest_dir: &str, name: &str) {
    let canonical: PathBuf = [manifest_dir, "..", "app", "provider-hooks", name]
        .iter()
        .collect();
    let installed: PathBuf = [manifest_dir, "..", ".claude", "hooks", name]
        .iter()
        .collect();
    if !canonical.exists() {
        panic!(
            "{} canonical source not found at {}; refusing to sync the installed hook from a stale or missing source",
            name,
            canonical.display()
        );
    }
    std::fs::copy(&canonical, &installed).unwrap_or_else(|e| {
        panic!(
            "failed to sync {} from {} to {}: {}",
            name,
            canonical.display(),
            installed.display(),
            e
        )
    });
    // std::fs::copy carries the canonical's mode bits — a non-executable
    // canonical would leave the installed hook unrunnable ("/bin/sh: ...:
    // Permission denied"), silently disabling it. Force the executable bit so
    // the hook always runs regardless of the source file's mode. Mirrors the
    // chmod run_enhance does on a fresh install.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&installed) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&installed, perms);
        }
    }
    println!("cargo:rerun-if-changed=../app/provider-hooks/{}", name);
}

// Sync an installed .claude/skills/<name>/SKILL.md copy from its canonical
// app/skills/<name>/SKILL.md source on every build — the bundled-skills
// analog of sync_hook, so the source repo dogfoods the same installed-copy
// layout Setup seeds into managed projects (bram-bundled-skills).
fn sync_skill(manifest_dir: &str, name: &str) {
    let canonical: PathBuf = [manifest_dir, "..", "app", "skills", name, "SKILL.md"]
        .iter()
        .collect();
    let installed_dir: PathBuf = [manifest_dir, "..", ".claude", "skills", name]
        .iter()
        .collect();
    if !canonical.exists() {
        panic!(
            "{} canonical skill not found at {}; refusing to sync the installed copy from a stale or missing source",
            name,
            canonical.display()
        );
    }
    std::fs::create_dir_all(&installed_dir).unwrap_or_else(|e| {
        panic!("failed to create {}: {}", installed_dir.display(), e)
    });
    let installed = installed_dir.join("SKILL.md");
    std::fs::copy(&canonical, &installed).unwrap_or_else(|e| {
        panic!(
            "failed to sync {} from {} to {}: {}",
            name,
            canonical.display(),
            installed.display(),
            e
        )
    });
    println!("cargo:rerun-if-changed=../app/skills/{}/SKILL.md", name);
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    sync_hook(&manifest_dir, "claude-worklist-guard.py");
    sync_hook(&manifest_dir, "claude-permission-menu-hook.py");
    sync_skill(&manifest_dir, "loose-ends");

    tauri_build::build()
}
