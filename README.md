# xmlui-desktop

## What is it?

A desktop app that helps you make best use of git and GitHub for AI-assisted software development.

## Who is it for?

Anyone who wants to use AI coding agents in a safe and accountable way.

<img width="1719" height="1082" alt="image" src="https://github.com/user-attachments/assets/3da680a2-1f9a-411d-93f0-5c87dbf80413" />



This app has opinions. It thinks versioning and collaboration are well-handled by git and GitHub, so it guides agents to make best use of them on your behalf, in conversation with you. And it thinks GitHub is great for accountability, so it also guides agents to join you in orderly and well-documented collaboration that leaves an auditable trail. 


## How does it work?

### Layout

- **Left pane** - A terminal where you run an AI coding agent  (e.g. `claude` or `codex`).

- **Right pane top** — An app under development in a local repo.

- **Right pane bottom** - An embedded app that talks to the terminal. (In this case it *is* the app under development.)

As files in the project change, the right pane reloads automatically

### Workflow

There are three phases for an item on the worklist: **proposed*** → **applied** → **committed**. The arrows between the phases are approval gates where you converse with the agent as it works on the proposed item: researching and writing code; creating or closing issues; organizing commits as an orderly and well-defined audit trail.

### Agent conventions

Project conventions are authored in
[`app/__shell/conventions.md`](./app/__shell/conventions.md). Claude is
bound to that file directly through `CLAUDE.md` and the installed
`.claude` hook/config path. Codex is bound through a repo-local
`AGENTS.md` block installed by setup, with the shell startup seed as a
backup for wrapped launches, then reinforced by the shared
provider-neutral setup machinery plus local Codex config, memories, and
rules.

<img width="1384" height="878" alt="image" src="https://github.com/user-attachments/assets/81fdac93-783e-489a-82b4-e0062950b83a" />


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

4. **XMLUI CLI - optional.** If you are developing an XMLUI app, or if you are developing`xmlui-desktop` itself (the agent-tools UI is an embedded XMLUI app) you will want the XMLUI MCP server. Follow the steps [here](https://xmlui.org/get-started) to get it.

## [Download the latest release →](https://github.com/judell/xmlui-desktop/releases/latest)

## Install

### macOS / Linux

```bash
curl -fsSL https://github.com/judell/xmlui-desktop/releases/latest/download/install.sh | bash
```

The script detects your platform, verifies the archive's SHA256 against the published `SHA256SUMS`, extracts the binary, and copies it to `/usr/local/bin` (if writable) or `~/.local/bin`. On macOS it also clears the `com.apple.quarantine` xattr. No `sudo` required.

### Windows

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://github.com/judell/xmlui-desktop/releases/latest/download/install.ps1 | iex"
```

Downloads `xmlui-desktop-windows-amd64.zip`, verifies its SHA256, extracts `xmlui-desktop.exe` to `~/bin`, and adds `~/bin` to your user `PATH`.

#### Smart App Control

On some Windows 11 setups, Smart App Control may block the unsigned binary — most users report no problem. If you do hit a block, you can disable SAC under **Windows Security → App & browser control → Smart App Control settings**. Before flipping the switch, read Microsoft's [Smart App Control FAQ](https://support.microsoft.com/en-us/windows/smart-app-control-frequently-asked-questions-285ea03d-fa88-4d56-882e-6698afdb7003) so you understand the consequences for your machine — the re-enable path has changed across Windows updates.

### Agent tools

- **Transcript** — the current active-session transcript for Claude or Codex. It follows the current session and renders turns plus inline tool activity for both providers, but it is intentionally a reader, not a full realtime control surface. Use Sessions to browse/search older transcripts.

- **Worklist** — the `proposed → applied → committed` approval surface that coordinates multi-step agent work. Each item is a small, independently approvable diff with a `before → after` summary. Select one item at a time (radio); three ghost actions act on it — **Approve** (TO APPLY → on-disk edits, transitions to TO COMMIT; TO COMMIT → git commit), **Iterate** (refine in place — agent revises the proposed text or edits the on-disk files per your feedback, item keeps its state), **Drop** (remove the item; for TO COMMIT, disk edits stay until you ask the agent to revert). Each row's `+ feedback` link expands a per-item message-to-agent textarea that travels with whichever action you click. The agent never advances state unilaterally. xmlui-desktop always writes local `resources/worklist-history/` snapshots for auditability, while committing that directory is an opt-in repo policy.

- **Commits** — HSplitter list of recent commits on the left, selected commit on the right. Full-history search via `git log --grep` across subject, body, and author; matched commits expand to clickable hit-row snippets, and the right pane stacks `snippetAroundLine` previews for every hit. The right-pane header is an `ExpandableItem` revealing the full commit message body. Unpushed commits surface a "Push" button that runs `git push origin`.

- **Issues** — HSplitter list of GitHub issues on the left (via `gh issue list`), selected issue on the right. Search runs `gh issue list --search` and tags hits per title/body line; clicking a hit filters the right-pane body to paragraphs containing the query. The expanded issue refetches every 30s so edits made via `gh` or github.com surface without collapse-and-reopen.

- **Sessions** — HSplitter list of local claude/codex JSONL sessions on the left, selected session's turns on the right. Search runs server-side across user and assistant text; hits filter the right pane to matching paragraphs. Each row has a ✕ delete (with confirm) and a ✎ rename: on Claude the rename appends a `custom-title` record to the session JSONL, on codex it appends a `{id,thread_name,updated_at}` entry to `~/.codex/session_index.jsonl`. After the action, the row dims and the buttons disable until the next agent restart picks up the change. Codex's `/resume` creates a forked session with a new id, so the `[current]` marker won't follow a renamed codex session — the rename modal documents that caveat inline.

- **Context** — provider-aware HSplitter view of the active agent's durable local context sources. For Claude, that means `CLAUDE.md`, its `@`-imports, the per-project memory tree, hooks, and settings. For Codex, that means repo-local `AGENTS.md` when present plus Codex-side sources such as `~/.codex/config.toml`, project-local `.codex/` files, memories, and rules. Substring search shows grep-style hit snippets in the list and `snippetAroundLine` context on the right.

The toolbar's `ⓘ` (top-right of the drawer's AppHeader) opens a right-pane info modal with the current URL, version, project-server config, and a `README on GitHub` link to this document.

### Toolbar

- **↻ reload xmlui app** — force-reload the right-pane iframe (file watcher does this automatically, but useful after edits to the parent shell).
- **🔍 browser devtools** — open the WebView devtools for debugging the right pane.
- **🛠 agent tools** — toggle the agent-tools drawer above.
- **▢ terminal** — toggle the terminal pane (hide it to give the web app full width). Window and splitter resizes preserve the terminal viewport instead of snapping scrollback to the top.
- **A− / A+** — decrease / increase the terminal font size (Cmd+− / Cmd+=).

Pinned across the top of the agent-tools drawer (stays reachable from any tab):

- **ⓘ info** — show a right-pane info modal (URL, project-server status, "Open in browser").
- **A− / A+** — decrease / increase the right-pane / drawer iframe font size.
- **1 / 2 / 3** — send numeric keystrokes to the active agent's terminal session.
- **Yes / No** — send "yes" or "no" as a complete user turn (handy for the agent's conversational prompts).
- **Esc** — send `Esc` to interrupt the agent mid-response.
- **🔍 Inspector** — open the XMLUI Inspector to reproduce a UI issue and export a trace JSON for analysis.
### Provider-aware setup

Once you launch an agent through the wrapped terminal functions (`claude` or `codex`), the drawer checks what that provider still needs for the current repo and prompts only when setup is missing.

Current behavior:

- **Claude in a fresh repo** — prompt once. Setup installs the provider-neutral core plus the Claude-specific adapter.
- **Claude in a repo that is already set up** — no prompt.
- **Codex in a fresh repo** — prompt once. Setup installs the provider-neutral core, the codex hook adapter, and the codex `developer_instructions`, and it also refreshes the shared Claude-side artifacts that live in the repo.
- **Codex in a repo where setup has already run** — no prompt. The repo and user-global Codex setup artifacts are already in place.

When the prompt runs, xmlui-desktop installs two layers:

- A provider-neutral core: xmlui-desktop records the latest structured `approved:` / `drop:` payload in `resources/.worklist-authorization.json` and uses that local record when validating worklist removals. The desktop watcher can revert an invalid prune as a defense-in-depth fallback if a hook ever fails to fire.
- A Claude adapter: `.claude/hooks/worklist-guard.py`, registered in `.claude/settings.json` to fire on `Write|Edit`. The hook denies edits to project files not covered by a proposed/applied worklist item (with explicit opt-out phrases in the last user message as the escape hatch), and validates worklist-prune authorization for changes to `resources/worklist.json` itself.
- A codex adapter: `~/.xmlui-desktop/codex-worklist-guard.py`, registered in `~/.codex/config.toml` as a `PreToolUse` hook with matcher `^(apply_patch|Bash|Write|Edit|mcp__.*)$`. Same coverage logic as the Claude hook, broadened to catch codex's `apply_patch` tool, mutation-shaped Bash commands, and MCP filesystem write/edit/create/move calls. Setup also writes `developer_instructions` into the codex config so the gate prose lands in the developer-role context part of every session, not just the user-role `AGENTS.md`.

PreToolUse hooks are the generic extension point — both Claude Code and codex expose them — so the two adapters share the same shape: each runs *before* the agent invokes a tool, receives a JSON payload describing the pending call on stdin, and can exit 0 to allow, return a deny decision to block (stderr/permissionDecisionReason goes back to the agent as a tool error), or fail to launch.

That means first-run setup is provider-aware in when it prompts but provider-symmetric in what it installs: launching either `claude` or `codex` and accepting the prompt sets up the shared core, the codex-side `AGENTS.md` guidance block, the codex `developer_instructions`, and the Claude and codex hook adapters.

### How `conventions.md` governs both agents

`app/__shell/conventions.md` is the canonical project convention file.
It governs Claude and Codex in different ways:

- **Claude: direct prompt binding plus enforcement.** Setup copies that file to `.claude/xmlui-desktop-conventions.md`, adds an `@`-import block to `CLAUDE.md`, and installs the `worklist-guard.py` PreToolUse hook. A new Claude session therefore reads the conventions file directly and is also mechanically blocked from unsafe worklist edits.
- **Codex: repo-local AGENTS.md plus native hook enforcement.** Setup writes a marked xmlui-desktop block into repo-root `AGENTS.md`, installs top-level `developer_instructions` in `~/.codex/config.toml`, and registers the codex worklist guard as a native `PreToolUse` hook. Wrapped `codex` launches also receive the same concise worklist guidance as a startup seed. The app reinforces that with the shared local authorization record in `resources/.worklist-authorization.json` and the watcher-revert fallback as defense in depth.

So the practical rule is: both agents are governed by the same worklist
conventions, with Claude reading the imported conventions file directly
and Codex receiving the equivalent guidance through AGENTS, top-level
`developer_instructions`, and its native hook adapter.

`worklist-guard.py` watches Write/Edit operations targeting `resources/worklist.json`. It simulates the change, diffs items by `id`, and for any item that would disappear it checks the `status`:

- `applied` (TO COMMIT) — removal allowed. Commit-then-prune is legitimate.
- `proposed` (TO APPLY) — removal allowed **only** if the user's most recent message starts with `drop: {"ids":[...]}` listing that id.

Violating writes are rejected with a "Blocked: removing X (status=proposed)..." stderr message that the agent sees and reacts to. Both providers run native PreToolUse hooks via this path, so the worklist-prune validation is enforced symmetrically. The watcher-based fallback (compare old/new worklist snapshots, consult `resources/.worklist-authorization.json`, restore prior contents if the prune wasn't authorized) is retained as defense-in-depth — it fires later than a native hook, but it covers the case where a hook fails to launch (e.g., Python missing) or where a future provider integration lacks a comparable extension point.

The hook is a Python script and needs Python 3 to run. On macOS and Linux it's invoked directly via its shebang (`#!/usr/bin/env python3`), so `python3` must be on PATH — almost always the case. On Windows it's invoked via `py -3 <path>`; the `py` launcher ships with the python.org installer and resolves Python via the Windows registry, independent of PATH. If Python isn't installed at all, Claude Code shows "Failed with non-blocking status code" for every Write/Edit and the validator is silently inert — writes still proceed, but the worklist guard isn't actually checking them. Install Python 3 to enable enforcement.

## Build

The frontend is static — no bundler, no `package.json`. The only build
step is the Tauri/Rust build.

From `src-tauri/`:

- **Dev:** `cargo run` (or `cargo tauri dev` with the Tauri CLI)
- **Release:** `cargo tauri build`

Tauri docs: <https://tauri.app/develop/>, <https://tauri.app/distribute/>.

### Calling xmlui-desktop from project code

Because the right pane is same-origin with the parent shell
(`tauri://localhost`), project code can reach the Tauri command bridge
directly through `window.parent` — no `postMessage` shim needed:

```js
const { invoke } = window.parent.__TAURI__.core;
const url = await invoke("get_right_pane_url");
```

Use this when an XMLUI app embedded in the right pane needs to read
filesystem state, hit one of xmlui-desktop's `__`-prefixed loopback
endpoints, or invoke any of the Rust IPC commands. The `helpers.js`
script loaded by the embedded XMLUI surfaces (`toShell`, `toTurn`,
`openExternal`, `logToHost`) is built on top of this bridge — opt
into the helpers for project XMLUI apps that need to talk back to
the running agent.

## Layout

- `Main.xmlui`, `components/`, `resources/`, `Globals.xs`,
  `config.json`, `index.html` — the XMLUI app at the repo root.
- `app/` — parent shell (Tauri webview entry, terminal wiring, vendor
  scripts, and `__shell/helpers.js` that the right pane includes).
- `src-tauri/` — Rust backend (PTY for the terminal, custom `tauri://`
  URI scheme handler that proxies the right-pane iframe to the project's
  HTTP server, filesystem watcher, IPC handlers).
- `scripts/` — auxiliary scripts.

## Screen capture

The screenshot helper currently exists but is not surfaced in the
default Codex-themed UI. When enabled, it grabs an interactive
rect-select screenshot, writes the PNG to the OS app cache, and
injects `Read this screenshot: @<path>` into the terminal as a fresh
user turn so the agent picks it up via its `Read` tool. No install
ceremony — it shells out to a system binary.

Currently **macOS-only**: the implementation invokes
`/usr/sbin/screencapture -i`, which ships with macOS. On Linux and
Windows it returns "screenshot capture is currently
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

#### Service workers don't register on macOS/Linux

The right-pane iframe loads at `tauri://localhost`, and the WebKit
engines on macOS (WKWebView) and Linux (WebKitGTK) don't treat
custom-scheme origins as secure contexts. Service-worker registration
silently fails there, so project features that depend on a service
worker — Mock Service Worker (MSW), XMLUI's in-page
`apiInterceptor`, custom offline caches — won't activate inside
xmlui-desktop on those platforms. Windows uses WebView2 (Chromium)
with the `http://tauri.localhost` form, which *is* a secure context,
so service workers register normally there.

Apps that hit a real HTTP backend are unaffected; the constraint only
applies to in-page request interception. If you're developing against
MSW or `apiInterceptor`, run your project in a regular browser tab at
`localhost:8080` while keeping xmlui-desktop pointed at the same
server for the agent loop.

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
the right-pane document specifically. The shell window and the right
pane both load at `tauri://localhost` (the parent shell directly, the
right pane via the scheme handler that proxies project content under
`/__project/*`), so they share an origin and therefore a `localStorage`
/ `IndexedDB` partition — a console session in either reaches the
same storage. A regular browser tab pointed at the project's own
`localhost:8080` server, by contrast, is a different origin with its
own independent storage.

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
your project in a regular browser tab pointed at its `localhost:8080`
origin — but remember that the tab's `localStorage` is a separate
store from the right pane's (the right pane is at `tauri://localhost`,
a different origin), so a session created there won't carry into
xmlui-desktop.
