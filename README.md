# xmlui-desktop

## Who is this for?

You are managing AI agents doing software development, and want to coordinate their use of git and GitHub on your behalf so the project's evolution is orderly and well-documented. You also want to search and manage git/GitHub issues and commits, as well as agent sessions and memories, in a consistent way.

## What does the app do?

It is a desktop app that pairs an AI coding agent with the web app it's building.

- **Left pane** — a real terminal, where you run an AI coding agent
  (e.g. `claude` or `codex`).
- **Right pane** — the project's web app that is under development.
- **File watcher** — as files in the project change, the right pane
  reloads automatically. No manual refresh.
- **Agent-tools drawer** — toggle from the toolbar to open a side
  panel with Talk (live transcript), Worklist (proposed → applied →
  committed flow), Commits, Issues, Sessions, Context, and README.

See [`CLAUDE.md`](./CLAUDE.md) for the conventions Claude Code follows
when driving the right pane.

<img width="1379" height="857" alt="image" src="https://github.com/user-attachments/assets/51728554-ae75-4508-b70c-0716ed555479" />


## Prerequisites

xmlui-desktop is built around the git commit lifecycle — the Worklist
transitions through proposed → applied → committed, the Commits tab
reads `git log`, and the agent runs `git commit` / `git push`
directly. Run it inside a local git repo.

1. **`git`** — usually preinstalled on macOS and Linux; install via
   your package manager if missing.

3. **GitHub CLI (`gh`) — recommended.** Powers the Issues tab in the
   agent-tools drawer and the agent's issue create / close / comment
   operations. Install from <https://cli.github.com/> and run
   `gh auth login` once. Without it, the Issues tab shows an empty
   state.

4. **`whisper-server` — optional.** Powers the 🎤 voice toggle in the
   parent shell's toolbar and the agent-tools drawer's AppHeader. See
   [Voice input](#voice-input) below for per-platform install.

5. **XMLUI CLI - optional.** If you are developing an XMLUI app, or if you are developing`xmlui-desktop` itself (the agent-tools UI is an embedded XMLUI app) you will want the XMLUI MCP server. Follow the steps [here](https://xmlui.org/get-started) to get it.

## [Download the latest release →](https://github.com/judell/xmlui-desktop/releases/latest)

### Agent tools

- **Talk** — live transcript of the active claude/codex session, rendered as it streams. Includes 🎤 voice dictation, scroll-to-bottom and scroll-to-top controls, and an Inspector launcher for XMLUI trace export (helps you identify and report issues with xmlui-desktop).

- **Worklist** — the two-stage `proposed → applied → committed` approval surface that coordinates multi-step agent work. Each item is a small, independently approvable diff with a `before → after` summary; the agent applies on TO APPLY approval and commits only on TO COMMIT approval, never unilaterally.

- **Commits** — HSplitter list of recent commits on the left, selected commit on the right. Full-history search via `git log --grep` across subject, body, and author; matched commits expand to clickable hit-row snippets, and the right pane stacks `snippetAroundLine` previews for every hit. The right-pane header is an `ExpandableItem` revealing the full commit message body. Unpushed commits surface a "Push" button that runs `git push origin`.

- **Issues** — HSplitter list of GitHub issues on the left (via `gh issue list`), selected issue on the right. Search runs `gh issue list --search` and tags hits per title/body line; clicking a hit filters the right-pane body to paragraphs containing the query. The expanded issue refetches every 30s so edits made via `gh` or github.com surface without collapse-and-reopen.

- **Sessions** — HSplitter list of local claude/codex JSONL sessions on the left, selected session's turns on the right. Search runs server-side across user and assistant text; hits filter the right pane to matching paragraphs. Each row has a ✕ delete (with confirm) and a ✎ rename (Claude only, via `custom-title` append); after the action, the row dims and the buttons disable until the next agent restart picks up the change.

- **Context** — HSplitter view of everything claude is loading for this project: CLAUDE.md and its @-imports, the per-project memory tree, hooks, and settings. Substring search with grep-style hit snippets in the list and `snippetAroundLine` context on the right.

- **README** — the rendered project README, so the agent and the user share the same source-of-truth doc.

### Toolbar

- **↻ reload xmlui app** — force-reload the right-pane iframe (file watcher does this automatically, but useful after edits to the parent shell).
- **🔍 browser devtools** — open the WebView devtools for debugging the right pane.
- **🛠 agent tools** — toggle the agent-tools drawer above.
- **▢ terminal** — toggle the terminal pane (hide it to give the web app full width).
- **A− / A+** — decrease / increase the terminal font size (Cmd+− / Cmd+=).
- **🎤 voice** — toggle Whisper-based voice dictation into the terminal (Cmd+Shift+D).

### Agent Toolbar

Pinned across the top of the agent-tools drawer (stays reachable from any tab):

- **ⓘ info** — show a right-pane info modal (URL, project-server status, "Open in browser").
- **A− / A+** — decrease / increase the right-pane / drawer iframe font size.
- **🎤 voice** — local-Whisper dictation; click to start, click again to send the transcript as a fresh user turn. Same engine as the parent-shell toolbar's voice button.
- **📸 screenshot** — capture a region of the screen and attach it to the agent as an image input.
- **1 / 2 / 3** — send numeric keystrokes for claude's permission menus (Allow once / Allow always / Deny).
- **Yes / No** — send "yes" or "no" as a complete user turn (handy for the agent's conversational prompts).
- **Esc** — send `Esc` to interrupt the agent mid-response.
- **🔍 Inspector** — open the XMLUI Inspector to reproduce a UI issue and export a trace JSON for analysis.

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

## Voice input

Click the 🎤 button (in the parent shell's toolbar or in the agent
tools drawer's AppHeader) once to start recording, click again to stop.
The transcript is sent to the agent in the terminal as a `voice: ...`
line so it's distinguishable from typed input.

xmlui-desktop spawns a local
[`whisper-server`](https://github.com/ggml-org/whisper.cpp/tree/master/examples/server)
on first record click and kills it on app exit. You don't manage the
process; you just need the binary, ffmpeg, and a model file installed.

### macOS — verified

```bash
brew install whisper-cpp ffmpeg
mkdir -p ~/.local/share/whisper-models
curl -L -o ~/.local/share/whisper-models/ggml-small.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
```

`small.en` is ~466 MB, English-only, real-time on Apple Silicon. Swap
in a different model from the same Hugging Face repo if you want
different size/accuracy/language. The bundled `Info.plist` declares
`NSMicrophoneUsageDescription`, so WKWebView's `getUserMedia` triggers
the standard macOS mic permission prompt on first use.

### Linux — expected to work, untested

Both pieces are available via native package managers, though the
exact incantation depends on your distro:

```bash
# Arch
sudo pacman -S whisper.cpp ffmpeg

# Debian/Ubuntu — ffmpeg is in apt; whisper.cpp usually needs a build
# from source: https://github.com/ggml-org/whisper.cpp#build
sudo apt install ffmpeg
git clone https://github.com/ggml-org/whisper.cpp && cd whisper.cpp && cmake -B build && cmake --build build -j

# Or, if you have Linuxbrew:
brew install whisper-cpp ffmpeg
```

Make sure `whisper-server` is on `PATH`. The model download path is
the same as on macOS. WebKit2GTK supports `getUserMedia` and prompts
via the desktop's standard mic permission flow. If something doesn't
work, please open an issue.

### Windows — best guess, untested

There's no official one-line installer for whisper.cpp on Windows.
Best guess at the rough shape:

1. Grab a prebuilt release from
   <https://github.com/ggml-org/whisper.cpp/releases> (look for an
   x64 binary asset) and put `whisper-server.exe` somewhere on `PATH`.
   Or build from source with CMake / Visual Studio.
2. Install ffmpeg via Chocolatey (`choco install ffmpeg`) or Scoop
   (`scoop install ffmpeg`).
3. Download the model into a directory of your choice. Note: the path
   used by the `whisper_start` Rust command is currently hardcoded to
   `~/.local/share/whisper-models/ggml-small.en.bin`, which expands to
   `%USERPROFILE%\.local\share\whisper-models\ggml-small.en.bin` —
   create that path or expect to patch the const.

WebView2 (Chromium) handles `getUserMedia` with the standard Windows
microphone permission. We haven't actually tested any of this; if you
try it, please open an issue with what worked or didn't so we can
firm up these instructions.

## Screen capture

Click the 📸 button (in the parent shell's toolbar or in the agent
tools drawer's AppHeader) to grab an interactive rect-select
screenshot. xmlui-desktop writes the PNG to the OS app cache and
injects `Read this screenshot: @<path>` into the terminal as a fresh
user turn, so the agent picks it up via its `Read` tool. No install
ceremony — it shells out to a system binary.

Currently **macOS-only**: the implementation invokes
`/usr/sbin/screencapture -i`, which ships with macOS. On Linux and
Windows the 📸 button returns "screenshot capture is currently
macOS-only"; if you want a port (e.g. via `grim` / `slurp` on Wayland
or a PowerShell snippet on Windows), please open an issue.

## Configuration

`.xmlui-desktop.json` at project root is the config file.

### Startup

You can specify how to launch the agent in the terminal pane.

```
{
  "shell": {
    "agent": "claude --continue"
  }
}
```

### Working with a real backend

`xmlui-desktop` binds the right-pane HTTP server to
`127.0.0.1:<random-port>` (it uses port `0` and lets the OS pick).
That's fine for projects that talk only to public APIs or static
files. It breaks when your project needs a **fixed origin** — OAuth
callbacks, CORS allowlists, hardcoded API base URLs.

> **Compatibility note.** The right pane is an iframe. Backends that
> send `X-Frame-Options: DENY` or `Content-Security-Policy:
> frame-ancestors 'none'` (common for security-sensitive admin UIs)
> cannot be loaded into the right pane regardless of port. Workarounds:
> configure the backend's dev mode to relax those headers, or serve
> the UI files via a permissive dev server (e.g. `npx http-server`)
> while keeping the real backend running for API calls. Otherwise,
> open the project in a standalone browser.

#### Declare a project server

Add `.xmlui-desktop.json` at the project root:

```json
{
  "server": {
    "command": "python3 -m http.server 8080",
    "cwd": "xmlui",
    "port": 8080,
    "path": "/"
  }
}
```

| field | meaning |
|---|---|
| `command` | shell command to bring up the project's server. Run via `sh -c` (Unix) or `cmd /C` (Windows). |
| `cwd` | working directory for the command, relative to the project root. Optional; defaults to the project root. |
| `port` | TCP port the iframe should target. xmlui-desktop probes this port at startup. |
| `path` | URL path appended to `http://localhost:<port>` for the iframe. Optional; defaults to `/`. |

At startup, xmlui-desktop:

- probes `127.0.0.1:<port>`. If it's already listening, it logs a notice
  and reuses the running server (useful when you start the server
  manually for log visibility);
- otherwise spawns `command` in `cwd`, with stdout/stderr forwarded to
  xmlui-desktop's own stderr (prefixed `[server]`);
- waits up to 5s for the port to come up, then points the right-pane
  iframe at `http://localhost:<port><path>`. The iframe retries once on
  load error to absorb the case where the server takes a moment to bind;
- on app exit, kills the spawned child.

The agent-tools drawer continues to load from xmlui-desktop's internal
loopback server regardless of this setting.

The app-under-test does not need to be an XMLUI app — `.xmlui-desktop.json`
is xmlui-desktop's own config file, separate from XMLUI's `config.json`.

### URL parameters

Use query strings to parameterize the frontend without rebuilding —
e.g. `?city=santarosa` to switch tenant. Pass them on the command line
to your server's command or bake them into `path` (e.g.
`"path": "/?city=santarosa"`).

### Working example

[community-calendar](https://github.com/judell/community-calendar) uses
`.xmlui-desktop.json` for GitHub-OAuth-via-Supabase development. See
[`docs/app-architecture.md`](https://github.com/judell/community-calendar/blob/main/docs/app-architecture.md)
for the Supabase URL-Configuration setup that requires the fixed
`localhost:8080/**` origin.

#### Fallback: the redirect pattern

If you can't add a config file (e.g. you're working in a repo you
don't own), you can still target a fixed origin by adding a
self-redirect at the top of the project's `index.html`:

```html
<script>
  if (location.hostname === '127.0.0.1' && location.port !== '8080') {
    var devQuery = location.search || '?defaultParam=value';
    location.replace('http://localhost:8080' + location.pathname + devQuery + location.hash);
  }
</script>
```

Run your frontend on a known port in a separate terminal
(`python3 -m http.server 8080`) and launch xmlui-desktop from the
project root. Its iframe loads the random-port URL once, your script
bounces it to `localhost:8080`. `.xmlui-desktop.json` is the preferred
mechanism — it auto-spawns the server, surfaces logs, and doesn't
pollute the project's HTML.

#### Auth callbacks won't reach the right pane

The right-pane webview has its own browser storage, isolated from
your system browser's storage at the same origin. That breaks any
auth flow that hands off to the system browser and expects a session
to come back into the webview:

- **Magic links in email.** Clicking the link opens your default
  browser, completes auth there, and stores the session in the
  *browser's* `localStorage`. The right pane never sees it.
- **OAuth provider redirects** that leave the webview have the same
  shape — the callback session lands in the wrong storage.

Even when the redirect script above lines the right pane up on
`localhost:8080`, that origin's storage in the Tauri webview is a
different store from `localhost:8080` storage in Safari or Chrome.

**Workaround for email auth: send a one-time code, not a link.** If
your backend supports OTP codes (Supabase, Auth0, Clerk, Cognito all
do), have the user paste the code from the email into a field in
your dialog. No callback URL, no cross-context handoff. Works
identically in the browser and inside xmlui-desktop.

For Supabase specifically:

1. Add `{{ .Token }}` to the Magic Link email template (Supabase
   dashboard → Authentication → Email Templates) so the email
   includes the 6/8-digit code. Docs:
   <https://supabase.com/docs/guides/auth/auth-email-templates>
2. After `signInWithOtp`, render a code-input field and call
   `verifyOtp({ email, token, type: 'email' })`. Docs:
   <https://supabase.com/docs/guides/auth/auth-email-passwordless>
3. The existing `onAuthStateChange` handler fires on `verifyOtp`
   success — no other plumbing needed.

[community-calendar](https://github.com/judell/community-calendar)
implements this in `xmlui/components/SignInDialog.xmlui` and
`xmlui/shell.js` (`window.signInWithEmail` + `window.verifyEmailOtp`).

### DevTools

Tauri uses the platform's native webview, so the DevTools you get
inside the right pane depend on the OS:

| Platform | Webview | DevTools |
|---|---|---|
| macOS | WKWebView | Safari Web Inspector |
| Linux | WebKitGTK | Safari Web Inspector |
| Windows | WebView2 (Chromium) | Chromium DevTools |

To open them, **right-click inside the right pane → Inspect Element**
in dev/debug builds (`cargo run` or `cargo tauri dev`). Release
builds disable DevTools by default. The execution context belongs to
the right-pane document specifically — that matters because the
shell window (`tauri.localhost`) and the right pane have separate
storage, so logging into one tells you nothing about the other.

#### WebKit quirks worth knowing

The macOS/Linux Web Inspector behaves differently from Chromium's
DevTools in a few ways that bite when you're testing auth flows:

- **`const`/`let` redeclaration throws.** Pasting `const sb = …` a
  second time in the same console session yields *"Unexpected
  identifier 'sb'. Expected ';' after variable declaration."*
  Chromium silently redeclares; WebKit doesn't. Wrap repeated
  snippets in an async IIFE (`(async () => { … })();`) so the
  bindings are scoped to each call.
- **Frame/context switcher is sparser.** The dropdown that picks the
  execution context (top-level vs iframes) often won't expose every
  frame the page contains. Right-clicking inside the frame you
  actually want and choosing **Inspect Element** is more reliable
  than picking it from the dropdown.
- **Service-worker and storage panels are less complete** than
  Chromium's. If you need to inspect IndexedDB or service-worker
  scope details, run the same project in a regular Chrome/Edge tab
  pointed at `localhost:8080`.

If you'd rather use Chromium DevTools on macOS/Linux, you can run
your project in a regular browser tab pointed at the same
`localhost:8080` origin — but remember that the tab's `localStorage`
is a separate store from the right pane's, so a session created
there won't carry into xmlui-desktop.
