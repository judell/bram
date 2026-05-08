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
  committed flow), Commits, Sessions, Issues, and README tabs.
  Drives multi-step coordination between you and the agent without
  typing in the terminal.

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

### The redirect pattern

Run your frontend on a known port in a separate terminal:

```
python3 -m http.server 8080
```

Add a self-redirect at the top of your project's `index.html`:

```html
<script>
  if (location.hostname === '127.0.0.1' && location.port !== '8080') {
    var devQuery = location.search || '?defaultParam=value';
    location.replace('http://localhost:8080' + location.pathname + devQuery + location.hash);
  }
</script>
```

Launch `xmlui-desktop` from the project root. Its iframe loads the
random-port URL once, your script bounces it to `localhost:8080`, and
your fixed-origin bindings line up.

### URL parameters

Use query strings to parameterize the frontend without rebuilding —
e.g. `?city=santarosa` to switch tenant. The redirect above preserves
whatever `?key=value` you launch with, or supplies a default when
launched without one.

### Working example

[community-calendar](https://github.com/judell/community-calendar) uses
this pattern for GitHub-OAuth-via-Supabase. See `xmlui/index.html` for
the redirect snippet and
[`docs/app-architecture.md`](https://github.com/judell/community-calendar/blob/main/docs/app-architecture.md)
for the Supabase URL-Configuration setup that requires the fixed
`localhost:8080/**` origin.

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
