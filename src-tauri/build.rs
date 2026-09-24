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

// conventions-core-and-reference-split: the source repo's CLAUDE.md imports
// app/__shell/conventions.md live, and its when-to-read pointers name
// .claude/bram-reference/<name>.md -- the path Setup seeds in managed
// projects. Mirror every app/__shell/reference/*.md there (enumerated, not
// listed) so one pointer path resolves everywhere, and drop mirrored files
// whose source is gone. Unstamped: this is the canonical text, not a seed.
fn sync_reference_docs(manifest_dir: &str) {
    let src: PathBuf = [manifest_dir, "..", "app", "__shell", "reference"]
        .iter()
        .collect();
    let dst: PathBuf = [manifest_dir, "..", ".claude", "bram-reference"]
        .iter()
        .collect();
    println!("cargo:rerun-if-changed=../app/__shell/reference");
    let Ok(rd) = std::fs::read_dir(&src) else {
        return;
    };
    let names: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md"))
        .collect();
    std::fs::create_dir_all(&dst)
        .unwrap_or_else(|e| panic!("failed to create {}: {}", dst.display(), e));
    for name in &names {
        std::fs::copy(src.join(name), dst.join(name))
            .unwrap_or_else(|e| panic!("failed to sync reference doc {}: {}", name, e));
        println!("cargo:rerun-if-changed=../app/__shell/reference/{}", name);
    }
    if let Ok(rd) = std::fs::read_dir(&dst) {
        for e in rd.flatten() {
            if let Ok(n) = e.file_name().into_string() {
                if n.ends_with(".md") && !names.contains(&n) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    // (Python hook syncs retired — retire-python-hooks-rust-only.)
    sync_skill(&manifest_dir, "loose-ends");
    sync_reference_docs(&manifest_dir);

    tauri_build::build()
}
