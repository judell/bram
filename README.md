# xmlui-desktop

A desktop app that pairs an AI coding agent with the XMLUI app it's building.

- **Left pane** — a real terminal, where you run an AI coding agent
  (e.g. `claude` or `codex`).
- **Right pane** — the project's XMLUI app (`Main.xmlui` at the repo
  root), served via the binary's `xmlui://` URI scheme.
- **File watcher** — as files in the project change, the right pane
  reloads automatically. No manual refresh.

The two panes can also talk: XMLUI components in the right pane post
text back into the terminal via `window.toShell` / `window.toTurn`
helpers, so buttons, selects, and forms can become input to whatever
agent is running on the left.

See [`CLAUDE.md`](./CLAUDE.md) for the conventions Claude Code follows
when driving the right pane.

https://github.com/user-attachments/assets/3d617d7a-f864-41f4-bc77-c6449a8c1bf2

## Prerequisites

xmlui-desktop opens an XMLUI app next to your terminal — you need a
project for it to open. If you don't already have one, follow
<https://xmlui.org/get-started> to scaffold one, then run
`xmlui-desktop` from its root.

## [Download the latest release →](https://github.com/judell/xmlui-desktop/releases/latest)

## Build

The frontend is static — no bundler, no `package.json`. The only build
step is the Tauri/Rust build.

From `src-tauri/`:

- **Dev:** `cargo run` (or `cargo tauri dev` with the Tauri CLI)
- **Release:** `cargo tauri build`

Tauri docs: <https://tauri.app/develop/>, <https://tauri.app/distribute/>.

## Layout

- `Main.xmlui`, `components/`, `resources/`, `manual.md`, `Globals.xs`,
  `config.json`, `index.html` — the XMLUI app at the repo root.
- `app/` — parent shell (Tauri webview entry, terminal wiring, vendor
  scripts, and `__shell/helpers.js` that the right pane includes).
- `src-tauri/` — Rust backend (PTY for the terminal, custom `xmlui://`
  URI scheme, filesystem watcher, IPC handlers).
- `scripts/` — auxiliary scripts.
