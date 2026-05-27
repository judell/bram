# Releasing Bram

Debug builds are the shipping format. The Rust side is thin glue (PTY
relay, loopback HTTP file server, small git/sessions queries); the
heavy lifting is XMLUI's TypeScript runtime in the WebView, which is
identical between debug and release. The audience is XMLUI developers,
who benefit from devtools being accessible. Don't propose `cargo build
--release`, code signing, notarization, or installer pipelines.

It's fine to leave `#[cfg(debug_assertions)]` gates in code (e.g.,
`open_devtools`) — they work in the only build we ship.

## Cutting a release

```
scripts/bump.sh 0.1.18
```

This is the atomic release entry point: bumps `src-tauri/Cargo.toml`
+ `src-tauri/tauri.conf.json` to the given version, runs `cargo build`
to refresh `Cargo.lock`, commits as `Release v0.1.18`, creates the
`v0.1.18` tag locally, pushes both commits and tag to `origin`, and
dispatches `.github/workflows/build.yml` against the new tag.

Flags:

- `--no-push` — commit and tag locally only; skip push and workflow
  dispatch. Useful for staging the release commit while you write
  release notes.
- `--branch=<name>` — expected current branch (default `main`). Errors
  out if you're on a different branch.

## Manual fallback

Use when `bump.sh` doesn't fit — re-tagging an existing commit,
dispatching the workflow against an existing tag, etc.

1. Bump `version` in `src-tauri/Cargo.toml` and
   `src-tauri/tauri.conf.json`.
2. `cargo build` to refresh `Cargo.lock`.
3. Commit, then `git tag vX.Y.Z <release-commit>` locally.
4. Push commits via the agent-tools "Push N unpushed commits" button,
   then `git push origin vX.Y.Z` for the tag separately. **The push
   button does not follow tags** — `git ls-remote --tags origin vX.Y.Z`
   after clicking it will return empty until you push the tag
   explicitly.
5. Dispatch `.github/workflows/build.yml` from the GitHub Actions UI
   (or `gh workflow run build.yml -f tag=vX.Y.Z -R judell/bram`)
   with the tag string. The workflow is `workflow_dispatch` only — it
   builds debug binaries for linux-amd64, macos-arm64, macos-intel,
   and windows-amd64, generates SHA256SUMS, and attaches `install.sh`
   / `install.ps1`.

## Testing the update banner

The `/__app-info` route reads the current version from
`CARGO_PKG_VERSION` and compares it against the latest GitHub release.
To exercise the banner UI before actually cutting a new release, launch
with `BRAM_FAKE_CURRENT=0.0.1 cargo run` — the env var
substitutes for the real package version in both the comparison and
the response's `current` field, so `has_update` flips to `true`
against whatever the real GitHub latest is, and the banner renders.
The result is cached per process, so set the env var before launch
and restart to re-test with a different fake value.

## Bram rename audit

The repo slug is already `judell/bram`, but the codebase is still a
mixed-name system: `README.md` is titled `Bram`, while most runtime,
installer, metadata, and guard surfaces still say `xmlui-desktop`.
This note inventories the remaining rename surface before any sweep.

### Cosmetic and user-facing surfaces

These are low-risk text changes as long as URLs and commands remain
valid.

- `app/index.html`, `app/tools/index.html`, `Main.xmlui`,
  `app/tools/Main.xmlui`, `app/tools/components/Architecture.xmlui`:
  browser titles, About copy, project descriptions, and help text.
- `README.md`, `CLAUDE.md`, `app/__shell/conventions.md`,
  `docs/apis.md`, `notes/enhance-subcommand-design.md`,
  `.github/release-body-template.md`: prose that still names the app
  `xmlui-desktop` or links to the old repo slug.
- `src-tauri/Info.plist`: microphone-permission string shown by macOS.

Operational impact:

- Cosmetic only if the command name, config filename, and repo URLs do
  not change at the same time.
- Risk is confusion, not breakage: the app currently presents itself as
  both `Bram` and `xmlui-desktop` depending on the surface.

### Repo, release, and distribution surfaces

These are operational. Renaming them changes how users install,
download, update, and verify the app.

- `README.md`, `.github/release-body-template.md`, `install.sh`,
  `install.ps1`, `docs/releasing.md`, `src-tauri/src/lib.rs`:
  hardcoded `judell/xmlui-desktop` GitHub URLs, including the update
  check endpoint at
  `https://api.github.com/repos/judell/xmlui-desktop/releases/latest`.
- `.github/workflows/build.yml`, `install.sh`, `install.ps1`,
  `.github/release-body-template.md`: artifact and executable names such
  as `xmlui-desktop-macos-arm64.tar.gz`, `xmlui-desktop-windows-amd64.zip`,
  `xmlui-desktop`, and `xmlui-desktop.exe`.
- `README.md`, `.github/release-body-template.md`, `install.sh`,
  `install.ps1`, `src-tauri/src/lib.rs`: command examples and help text
  that instruct users to launch `xmlui-desktop`.

Operational impact:

- Changing repo URLs to `judell/bram` is required for new releases and
  update checks to work after the rename.
- Changing artifact names and the installed binary name is a breaking
  distribution change unless the release continues to publish the old
  filenames or the installer supports both names during a transition.
- Changing the CLI name from `xmlui-desktop` to `bram` affects every
  README command, shell alias, automation script, and launch hint.

### App/package identity surfaces

These are operational and affect bundle identity, package metadata, and
what the OS shows users.

- `src-tauri/tauri.conf.json`: `productName`, window `title`, and
  bundle `identifier` currently use `xmlui-desktop` /
  `org.xmlui.xmlui-desktop`.
- `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`: Rust package name
  is `xmlui-desktop`; library crate name is `xmlui_desktop_lib`.
- `src-tauri/src/lib.rs`: help/version strings printed by the binary
  still identify as `xmlui-desktop`.

Operational impact:

- `productName` and window title are mostly user-facing and safe.
- The bundle identifier is higher risk: changing it can affect macOS
  app identity, permissions, caches, and future updater expectations.
- Cargo package and crate names are build-time identifiers; they are
  changeable, but any downstream tooling or scripts that expect the old
  package/binary names must be updated in lockstep.

### Runtime compatibility surfaces

These are the highest-risk identifiers because other tools, repos, or
saved local state may already depend on them.

- Config filename:
  `README.md`, `app/tools/Main.xmlui`, `app/tools/components/Architecture.xmlui`,
  `app/main.js`, `src-tauri/src/lib.rs`, `app/shell/claude-code-shellrc`
  all refer to `.xmlui-desktop.json`.
- Environment variables:
  `XMLUI_DESKTOP_PORT`, `XMLUI_DESKTOP_FAKE_CURRENT`,
  `XMLUI_DESKTOP_VERSION`, `XMLUI_DESKTOP_BASE_URL`,
  `XMLUI_DESKTOP_AGENT_HINT`.
- Local state keys:
  `app/main.js` and `app/__shell/helpers.js` store UI state in
  `localStorage` under `xmlui-desktop.*`.
- Setup/hook paths and markers:
  Codex now prefers `~/.bram/codex-worklist-guard.py` and
  `# bram:start` / `# bram-instructions:start` blocks in
  `~/.codex/config.toml`, while accepting legacy
  `~/.xmlui-desktop/codex-worklist-guard.py` and `# xmlui-desktop:start`
  blocks during migration. Repo-local Claude convention files now use
  `.claude/bram-conventions.md`; Setup migrates the legacy
  `.claude/xmlui-desktop-conventions.md` path on next run, and the
  shellrc / profile / codex-guard accept either filename during the
  transition. `<!-- xmlui-desktop:start -->` markers and
  `xmlui-desktop-auto-rebase` remain legacy-named.

Operational impact:

- Renaming `.xmlui-desktop.json` without alias support breaks existing
  project configs immediately.
- Renaming env vars breaks shell wrappers, the PTY child environment,
  installer overrides, and the worklist resolve/mutate flow unless old
  names continue to be accepted.
- Renaming localStorage keys resets saved UI state. That is survivable,
  but it is still migration-visible.
- Renaming `~/.xmlui-desktop` paths or marker strings can make existing
  Setup installs look unconfigured unless Setup writes the new `~/.bram`
  path and continues to honor the old path during migration.

### Checklist by decision type

Safe to change first:

- User-facing `xmlui-desktop` copy to `Bram`.
- Repo links from `judell/xmlui-desktop` to `judell/bram`.
- About/help text and release prose.

Needs a compatibility plan before renaming:

- Binary/executable name.
- Release artifact filenames.
- `.xmlui-desktop.json`.
- `XMLUI_DESKTOP_*` env vars.
- `~/.bram` Codex hook/install directory, with `~/.xmlui-desktop`
  accepted as the legacy migration source.
- `localStorage` keys with `xmlui-desktop.*`.
- Bundle identifier `org.xmlui.xmlui-desktop`.

Should likely remain legacy for one transition period even if branding changes:

- `.xmlui-desktop.json` as a supported alias, even if a new `.bram.json`
  is introduced.
- `XMLUI_DESKTOP_*` env vars as accepted aliases.
- Legacy release artifact names or installer lookup fallback.
- Existing hook markers and setup paths until migration code has run.

### Naming guidance

- Use `Bram` for human-facing product copy: window title, README title,
  About text, release notes, and help prose.
- Use `bram` for repo slug, URLs, and any future lowercase CLI or file
  names if they are intentionally renamed.
- Do not mass-replace every `xmlui-desktop` identifier. Several of them
  are compatibility contracts, not branding strings.

### Recommended phased rollout

Treat the rename as a sequence, not a sweep.

1. Change human-facing branding first.
   Update titles, About text, README headings, help prose, and release
   copy to say `Bram` while leaving operational identifiers intact.
2. Change repo and release URLs next.
   Move `judell/xmlui-desktop` references to `judell/bram` anywhere the
   app checks for releases, installers, or documentation.
3. Decide whether the CLI and artifact names are actually changing.
   If `xmlui-desktop` becomes `bram`, plan a compatibility window where
   installers, release assets, and docs recognize both names.
4. Add compatibility for config and environment identifiers before
   renaming them.
   If introducing `.bram.json` or `BRAM_*`, keep `.xmlui-desktop.json`
   and `XMLUI_DESKTOP_*` working as aliases first, then migrate docs.
5. Defer bundle-identity and persistent-state renames until last.
   `org.xmlui.xmlui-desktop` and `localStorage` keys should change only
   with explicit migration logic or a deliberate decision to reset local
   state. The Codex hook path has started that pattern by preferring
   `~/.bram` while still accepting `~/.xmlui-desktop` as the legacy
   migration source.

This order keeps the visible product rename moving while isolating the
breakage-prone compatibility work behind explicit decisions.

### Suggested follow-on worklist items

Use the rollout above to break the rename into small approvals instead
of one giant rename patch.

1. `bram-branding-copy-pass`
   Update human-facing titles and prose to `Bram` without changing any
   command names, filenames, env vars, or compatibility identifiers.
   Likely files: `README.md`, `app/index.html`, `app/tools/index.html`,
   `Main.xmlui`, `app/tools/Main.xmlui`,
   `app/tools/components/Architecture.xmlui`, `src-tauri/Info.plist`.
2. `bram-repo-url-pass`
   Replace `judell/xmlui-desktop` links with `judell/bram` in docs,
   release notes, installer scripts, and the GitHub release-check
   endpoint in Rust.
   Likely files: `README.md`, `.github/release-body-template.md`,
   `install.sh`, `install.ps1`, `docs/releasing.md`,
   `src-tauri/src/lib.rs`, `app/tools/Main.xmlui`.
3. `bram-cli-and-artifact-decision`
   Decide whether the shipped executable and release artifacts remain
   `xmlui-desktop*` for now or move to `bram*`. Do not combine the
   decision with the implementation change.
   Likely files for the eventual implementation:
   `.github/workflows/build.yml`, `install.sh`, `install.ps1`,
   `README.md`, `.github/release-body-template.md`,
   `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
   `src-tauri/src/lib.rs`.
4. `bram-config-and-env-aliases`
   Add compatibility support for any new config/env names before docs
   are switched. This is where `.bram.json` or `BRAM_*` aliases would
   be introduced if you choose to rename them.
   Likely files: `src-tauri/src/lib.rs`, `app/main.js`, `README.md`,
   `app/tools/Main.xmlui`, `app/tools/components/Architecture.xmlui`,
   `app/shell/claude-code-shellrc`.
5. `bram-persistent-identity-migration`
   Handle only the durable identifiers: bundle id, setup directory,
   localStorage keys, and marker strings. This should be its own phase
   because it needs explicit migration or a deliberate compatibility
   break.
   Likely files: `src-tauri/tauri.conf.json`, `app/main.js`,
   `app/__shell/helpers.js`, `app/shell/worklist-guard-codex.py`,
   `app/__shell/conventions.md`, `src-tauri/src/lib.rs`.

That gives you a workable sequence:

- first a branding pass
- then the repo/release plumbing
- then an explicit decision item about binary naming
- then compatibility aliasing if needed
- then the durable identity migration last
