use std::path::PathBuf;

// Sync an installed .claude/skills/<name>/SKILL.md copy from its canonical
// app/skills/<name>/SKILL.md source on every build — the bundled-skills
// pattern (formerly shared with the retired Python hook sync), so the source repo dogfoods the same installed-copy
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
    std::fs::create_dir_all(&installed_dir)
        .unwrap_or_else(|e| panic!("failed to create {}: {}", installed_dir.display(), e));
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
    // (Python hook syncs retired — retire-python-hooks-rust-only.)
    sync_skill(&manifest_dir, "loose-ends");

    tauri_build::build()
}
