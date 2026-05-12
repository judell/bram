  # xmlui-desktop

A desktop app that pairs an AI coding agent with the XMLUI app it's building.

- **Left pane** — a real terminal, where you run an AI coding agent
  (e.g. `claude` or `codex`).
- **Right pane** — the project's XMLUI app (`Main.xmlui` at the repo
  root), served via the binary's `xmlui://` URI scheme.
- **File watcher** — as files in the project change, the right pane
  reloads automatically. No manual refresh.
- **Agent-tools drawer** — toggle from the toolbar to open a side
  panel with Talk (live transcript), Worklist (proposed → applied →
  committed flow), Commits, Issues, Sessions, Context, and README
  tabs. Drives multi-step coordination between you and the agent
  without typing in the terminal. The Context tab shows what Claude
  Code is loading for the current project — CLAUDE.md and its
  @-imports, the per-project memory tree, hooks, and settings — with
  substring search and grep-style hit snippets.

The two panes can also talk: XMLUI components in the right pane post
text back into the terminal via `window.toShell` / `window.toTurn`
helpers, so buttons, selects, and forms can become input to whatever
agent is running on the left.

See [`CLAUDE.md`](./CLAUDE.md) for the conventions Claude Code follows
when driving the right pane.

https://github.com/user-attachments/assets/3d617d7a-f864-41f4-bc77-c6449a8c1bf2

## Prerequisites

xmlui-desktop is built around the git commit lifecycle — the Worklist
transitions through proposed → applied → committed, the Commits tab
reads `git log`, and the agent runs `git commit` / `git push`
directly. Run it inside a local git repo with an XMLUI project.

1. **`git`** — usually preinstalled on macOS and Linux; install via
   your package manager if missing.

2. **A local git repo with an XMLUI project.** If you have neither,
   follow the installation steps at <https://xmlui.org/get-started>
   — that gets you the XMLUI CLI (which includes the MCP server). If
   you followed those instructions to completion and have created
   `~/xmlui-weather`, remove it and instead
   `git clone https://github.com/xmlui-org/xmlui-weather`. That gives
   you a repo with pre-existing git history to explore in the
   xmlui-desktop Commits pane; you can stage work items as local git
   commits to get a feel for what that is like.

3. **GitHub CLI (`gh`) — recommended.** Powers the Issues tab in the
   agent-tools drawer and the agent's issue create / close / comment
   operations. Install from <https://cli.github.com/> and run
   `gh auth login` once. Without it, the Issues tab shows an empty
   state.

4. **`whisper-server` — optional.** Powers the 🎤 voice toggle in the
   parent shell's toolbar and the agent-tools drawer's AppHeader. See
   [Voice input](#voice-input) below for per-platform install.

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

## Working with a real backend

xmlui-desktop binds the right-pane HTTP server to
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

### Declare a project server (`.xmlui-desktop.json`)

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

### Fallback: the redirect pattern

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

### Auth callbacks won't reach the right pane

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
