# Bram

## What is it?

A desktop app that helps you make best use of git and GitHub for AI-assisted software development.

Bram runs agents mindfully.

## Who is it for?

Anyone who wants to use AI coding agents in a safe, orderly, and accountable way.

Bram has opinions. It thinks versioning and collaboration are well-handled by git and GitHub, so it guides agents to make best use of them on your behalf, in conversation with you. And it thinks GitHub is great for accountability, so it also guides agents to join you in orderly and well-documented collaboration that leaves an auditable trail.

<!--
<table>
  <tr>
    <td width="50%">
      <img src="https://github.com/user-attachments/assets/0fce4634-8912-4a53-9b1e-ddb99edf4e05" alt="image" width="100%" />
    </td>
    <td width="50%">
      <img src="https://github.com/user-attachments/assets/07129b4a-6c17-4cff-b095-150de8b2ed8c" alt="image" width="100%" />
    </td>
  </tr>
</table>
-->

<img width="1067" height="908" alt="image" src="https://github.com/user-attachments/assets/093ddc62-ffa9-4f1f-8ecd-45fcc6f154ea" />




## Blog

<a href="https://blog.jonudell.net/2026/06/02/how-to-make-best-use-of-git-and-github-for-ai-assisted-software-development/">How to make best use of git and GitHub for AI-assisted software development</a>

<a href="https://blog.jonudell.net/2026/06/17/vibe-coding-as-a-team-sport/">Vibe coding as a team sport</a>

<a href="https://blog.jonudell.net/2026/06/28/doctor-it-hurts-when-agents-create-unreviewable-prs-dont-do-that/">“Doctor, it hurts when agents create unreviewable PRs.” “Don’t do that.”</a>

<a href="https://blog.jonudell.net/2026/07/01/what-is-the-terminal/">"What is the terminal?"</a>

<a href="https://blog.jonudell.net/2026/07/08/dont-infer-behavior-from-code-observe-it-in-logs/">Don’t infer behavior from code, observe it in logs</a>

<a href="https://blog.jonudell.net/2026/07/16/talking-to-claude-code-and-codex/">Talking to Claude Code and Codex</a>

<a href="https://blog.jonudell.net/2026/07/12/small-models-can-solve-big-problems/">Small models can solve big problems</a>

<a href="https://blog.jonudell.net/2026/07/23/agents-that-narrate-their-work-are-the-best-team-players/">Agents that narrate their work are the best team players</a>

<a href="https://blog.jonudell.net/2026/08/01/make-agent-memory-searchable/">Make agent memory searchable</a>

### Elsewhere

<a href="https://waltzweb.wordpress.com/2026/07/02/github-for-todays-hybrid-teams/">Technical or not, human or AI – could Bram be the missing link?</a>

<a href="https://arxiv.org/abs/2607.25975">Who is scientific code for? Maintaining human-readable landmarks in agent-written code</a>

## How does it work?

### Terminal (optional)

A terminal where you run `claude` or `codex`. It can be hidden in favor of the
agent pane's Worklist, Transcript, and Queue views.

### Agent pane

- Header: Switches between agents and adjusts font size.

- Worklist: Where you decide what happens to your repo. Each item shows its plan, the files it alters as work happens, and your feedback history. The most recent conversation exchange stays in view.

- Transcript: Shows your terminal session in a more readable form.

- Issues: Tracks and searches GitHub issues for the repo Bram runs in.

- Commits: Tracks and searches your commits.

- Queue: Parks editable messages while the agent works and sends each one only when you choose.

- Sessions: Lists your sessions; rename, delete, or switch between them.

- History: Lists and searches your Worklist items.

- Settings: Shows and changes global (repo-independent) settings.

- Footer: Shows agent status and the current session, and lets you message the agent.


### Workflow

On the *Worklist* tab, create Worklist items in conversation ("Hey agent, let's do x") or with the *+ New item* button, which can also cite an open issue. Either way the agent proposes an item: a short plan with Before and After notes, naming the files it expects to alter.

Each item's row tells you where things stand. A fresh proposal says "No changes yet". Once the agent alters files, the row counts them ("files: 2 of 3 planned"), tallies added and removed lines, and shows the diff. Your past feedback on the item collects in its Feedback section.

To act, tick one or more items and use the buttons beside the message box. One action, and one message, covers every item you selected.

- *Approve* accepts a proposal and authorizes the agent to alter files.

- *Commit* commits an item's altered files to git.

- *Approve & commit* does both in one step. It is offered only when the selected items are proposals with no changes.

- *Iterate* sends your message as feedback so the agent can refine a proposal or rework altered files.

- *Drop* retires items you don't want. A dropped item isn't lost: you can resurrect it from the History tab, or ask the agent to file an issue first so the idea survives in GitHub.

The previous Worklist design remains available for now. Turn on *Show legacy Worklist* in Settings to show it as OldWorklist.

On the *Issues* tab, use `+ New issue` to ask Bram to file a GitHub issue. 

On the *Queue* tab, park thoughts that arrive while the agent is busy. Bram
keeps them with the repo across reloads and restarts. Edit or delete them
freely, choose whether each should become an ordinary *Message* or feedback
that *Iterates* a selected Worklist item, then use *Send* when it becomes
ready. Queued messages are never sent automatically.

Every item moves from proposal, to altered files, to a commit, and hooks hold the agent to that order: no file alterations without an approved item, no commit without your approval. Between those steps you can dwell as long as you like to:

- discuss and refine a proposal

- discuss and refine an implementation

- create, refine, and close issues

- organize commits

By default every change request flows through the Worklist. That's overkill for small things so, when messaging the agent from Bram's footer, you can use the *skip worklist* button instead of *send*. When messaging the agent from a Worklist item, you can prefix your message with *skip-worklist:* or end it with "just do it" (the only verbal opt-out phrase).


### Workflow conventions

The rules an agent follows when driving the Bram workflow (proposing Worklist
items, moving them from proposal to commit, coordinating commits and issues)
live in [`app/__shell/conventions.md`](./app/__shell/conventions.md). Each agent
loads that file automatically: Claude through `CLAUDE.md` and the installed
`.claude` hook/config path, Codex through a repo-local `AGENTS.md` block that
setup installs. These aren't merely advisory: a `PreToolUse` hook on each agent
enforces the core rule, rejecting file edits that aren't covered by an approved
Worklist item — so the convention enforces itself rather than relying on the
agent's goodwill.


## Prerequisites

1. A local git repo in which you develop your app

2. **`git`** — usually preinstalled on macOS and Linux; install via
   your package manager if missing.

3. **GitHub CLI (`gh`)**  - Powers the Issues tab in the
   agent pane and the agent's issue create / close / comment
   operations. Install from <https://cli.github.com/> and run
   `gh auth login` once. Without it, the Issues tab shows an empty
   state.

4. **XMLUI CLI - optional.** If you are developing an XMLUI app, or if you are developing `Bram` itself (the agent pane UI is an embedded XMLUI app) you will want the XMLUI MCP server. Follow the steps [here](https://xmlui.org/get-started) to get it.

5. **`whisper-server` — optional.** Powers the 🎤 voice button in the
   parent-shell toolbar and the agent pane. Tested on macOS and Windows/WSL, see [Voice input](#voice-input) below for install and per-platform
   status.

## [Download the latest release →](https://github.com/judell/bram/releases/latest)

## Install

### macOS / Linux

```bash
curl -fsSL https://github.com/judell/bram/releases/latest/download/install.sh | bash
```

The script detects your platform, verifies the archive's SHA256 against the published `SHA256SUMS`, extracts the binary, and copies it to `/usr/local/bin` (if writable) or `~/.local/bin`. On macOS it also clears the `com.apple.quarantine` xattr. No `sudo` required.

### Windows

From PowerShell:

```powershell
irm https://github.com/judell/bram/releases/latest/download/install.ps1 | Out-String | iex
```

From Command Prompt:

```batch
curl.exe -fsSL https://github.com/judell/bram/releases/latest/download/install.ps1 -o "%TEMP%\bram-install.ps1" && powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%TEMP%\bram-install.ps1" && del "%TEMP%\bram-install.ps1"
```

Downloads `bram-windows-amd64.zip`, verifies its SHA256, extracts `bram.exe` to `~/bin`, and adds `~/bin` to your user `PATH`.

#### Smart App Control

On some Windows 11 setups, Smart App Control may block the unsigned binary — most users report no problem. If you do hit a block, you can disable SAC under **Windows Security → App & browser control → Smart App Control settings**. Before flipping the switch, read Microsoft's [Smart App Control FAQ](https://support.microsoft.com/en-us/windows/smart-app-control-frequently-asked-questions-285ea03d-fa88-4d56-882e-6698afdb7003) so you understand the consequences for your machine — the re-enable path has changed across Windows updates.

### First-run setup

The first time you launch `claude` or `codex` in a repo, Bram checks what that provider needs and prompts once if anything is missing — no prompt on later launches. Accepting Bram's setup prompt installs the Worklist conventions the agent reads each session plus provider-specific hooks that enforce the Worklist and show permission menus. A provider-neutral authorization record and watcher-revert fallback back up those hooks.

Claude and Codex differ in the trust step you see after Bram writes those files:

- **Claude setup** is repo-local. Bram writes `.claude/bram-conventions.md`, `.claude/hooks/worklist-guard.py`, `.claude/hooks/permission-menu-hook.py`, and `.claude/settings.json` registrations. Claude reads that project config on launch; there is no separate Codex-style hook trust menu. The Claude `PreToolUse` hook is the safety gate for `Write`, `Edit`, and `Bash`; the permission hook shows Claude permission and AskUserQuestion prompts in Bram's agent pane.
- **Codex setup** has an explicit trust/approval step. Bram writes user-global hook scripts under `~/.bram`, updates `~/.codex/config.toml`, writes Bram `developer_instructions`, and adds a repo-local `AGENTS.md` block. Codex then shows its hook approval screen; approve it only when it points at Bram-owned paths such as `~/.bram/codex-worklist-guard.py`, `~/.bram/codex-permission-menu-hook.py`, `~/.codex/config.toml`, and this repo's `AGENTS.md`. The Codex `PreToolUse` hook gates `apply_patch`, `Bash`, `Write`, `Edit`, and mutation-shaped MCP tools; the Codex permission hook shows `PermissionRequest` / `PostToolUse` menus in Bram's agent pane.
- **Do not approve unexpected Codex paths.** If the Codex approval screen lists hook scripts somewhere other than `~/.bram` or config changes somewhere other than the expected Codex/project files, stop and inspect them first.

Hook names mean the same thing in both providers:

- **`PreToolUse`** runs before a tool executes and blocks mutations that are not covered by the Worklist or by an explicit direct-edit authorization.
- **`PermissionRequest`** lets Bram render permission prompts from structured hook payloads instead of scraping the terminal grid.
- **`PostToolUse`**, when present, is post-tool bookkeeping for the same hook-driven permission/menu path.

A healthy Codex hook screen should show Bram's installed lifecycle hooks as both **Installed** and **Active**. In the current setup that means at least `PreToolUse`, `PermissionRequest`, and `PostToolUse` each show `1 / 1`; lifecycle events Bram does not use, such as `SessionStart` or `Stop`, can remain `0 / 0`.

<details>
<summary>Setup internals: hook adapters, guard source-of-truth, and how conventions.md binds each provider</summary>

Once you launch an agent through Bram's terminal, the agent pane checks what that provider still needs for the current repo and prompts only when setup is missing.

Current behavior:

- **Claude in a fresh repo** — prompt once. Setup installs the provider-neutral core plus the Claude-specific adapter.
- **Claude in a repo that is already set up** — no prompt.
- **Codex in a fresh repo** — prompt once. Setup installs the provider-neutral core, the codex hook adapter, and the codex `developer_instructions`, and it also refreshes the shared Claude-side artifacts that live in the repo.
- **Codex in a repo where setup has already run** — no prompt. The repo and user-global Codex setup artifacts are already in place.

When the prompt runs, Bram installs two layers:

- A provider-neutral core: Bram records the latest structured `approved:` / `drop:` payload in `resources/.worklist-authorization.json` and uses that local record when validating Worklist removals. The desktop watcher can revert an invalid prune as a defense-in-depth fallback if a hook ever fails to fire.
- A Claude adapter: `.claude/hooks/worklist-guard.py`, registered in `.claude/settings.json` as a `PreToolUse` hook for `Write|Edit|Bash`. The hook denies edits to project files not covered by a proposed/applied worklist item (with the explicit opt-out phrase "just do it" in the last user message as the escape hatch), validates worklist-prune authorization for changes to `resources/worklist.json` itself, and blocks mutation-shaped Bash commands without worklist coverage. Setup also installs `.claude/hooks/permission-menu-hook.py` for Claude permission and AskUserQuestion surfacing in the agent pane.
- A Codex adapter: `~/.bram/codex-worklist-guard.py`, registered in `~/.codex/config.toml` as a `PreToolUse` hook with matcher `^(apply_patch|Bash|Write|Edit|mcp__.*)$`. Same coverage logic as the Claude hook, broadened to catch Codex's `apply_patch` tool and MCP filesystem write/edit/create/move calls. Setup also writes `developer_instructions` into the Codex config so the gate prose lands in the developer-role context part of every session, not just the user-role `AGENTS.md`, and installs `~/.bram/codex-permission-menu-hook.py` for `PermissionRequest` / `PostToolUse` menu surfacing with the xterm grid retained as fallback. Existing `~/.xmlui-desktop/codex-worklist-guard.py` installs remain accepted during migration; rerunning Setup rewrites the config to the Bram path.

In the Bram source repo, the Claude guard's source of truth is
`app/__shell/worklist-guard.py`. The `.claude/hooks/worklist-guard.py`
file is the installed runtime copy, refreshed from the source bundle by
Setup and by `src-tauri/build.rs` during Cargo builds. Functional edits
belong in `app/__shell/worklist-guard.py`; editing the installed copy
directly creates setup drift and may be overwritten. The Codex guard
uses the same source/installed split: `app/shell/worklist-guard-codex.py`
is canonical, `~/.bram/codex-worklist-guard.py` is installed.

PreToolUse hooks are the generic extension point — both Claude Code and codex expose them — so the two adapters share the same shape: each runs *before* the agent invokes a tool, receives a JSON payload describing the pending call on stdin, and can exit 0 to allow, return a deny decision to block (stderr/permissionDecisionReason goes back to the agent as a tool error), or fail to launch.

That means first-run setup is provider-aware in when it prompts but provider-symmetric in what it installs: launching either `claude` or `codex` and accepting the prompt sets up the shared core, the Codex-side `AGENTS.md` guidance block, the Codex `developer_instructions`, the Worklist guard hooks, and the permission-menu surfacing hooks.

#### How `conventions.md` governs both agents

`app/__shell/conventions.md` is the canonical project convention file.
It governs Claude and Codex in different ways:

- **Claude: direct prompt binding plus enforcement.** Setup copies that file to `.claude/bram-conventions.md`, adds an `@`-import block to `CLAUDE.md`, and installs the `worklist-guard.py` PreToolUse hook. A new Claude session therefore reads the conventions file directly and is also mechanically blocked from unsafe worklist edits. Existing projects with the legacy `.claude/xmlui-desktop-conventions.md` path are migrated to the new name on the next Setup run.
- **Codex: repo-local AGENTS.md plus native hook enforcement.** Setup writes a marked Bram block into repo-root `AGENTS.md`, installs top-level `developer_instructions` in `~/.codex/config.toml`, and registers the Codex Worklist guard as a native `PreToolUse` hook. Codex launches also receive the same concise Worklist guidance as a startup seed. The app reinforces that with the shared local authorization record in `resources/.worklist-authorization.json` and the watcher-revert fallback as defense in depth.

So the practical rule is: both agents are governed by the same Worklist
conventions, with Claude reading the imported conventions file directly
and Codex receiving the equivalent guidance through AGENTS, top-level
`developer_instructions`, and its native hook adapter.

Claude and Codex also differ in how they *call* Bram's Worklist
lifecycle routes. Claude uses the loopback HTTP endpoints directly.
Codex's sandbox refuses loopback connections (`curl: (7)` even when
Bram is listening, #130), so it drives the identical lifecycle over a
filesystem channel instead — writing `resources/.worklist-intent.json`
and reading `resources/.worklist-result.json`, which the host
dispatches through the same handlers as the HTTP routes.

The provider hooks validate direct edits to `resources/worklist.json`. Proposal authoring and iterate-time prose refinement are allowed there; mechanical prune / status-advance operations are expected to go through `POST /__worklist/mutate` instead. Both providers now reject direct Worklist edits that remove items or change their `status`, which keeps the shared backend endpoint as the canonical state machine for `advance` / `prune`. The watcher-based fallback (compare old/new worklist snapshots, consult `resources/.worklist-authorization.json`, restore prior contents if the prune wasn't authorized) remains as defense-in-depth — it fires later than a native hook, but it covers the case where a hook fails to launch (e.g., Python missing) or where a future provider integration lacks a comparable extension point.

The hook is a Python script and needs Python 3 to run. On macOS and Linux it's invoked directly via its shebang (`#!/usr/bin/env python3`), so `python3` must be on PATH — almost always the case. On Windows it's invoked via `py -3 <path>`; the `py` launcher ships with the python.org installer and resolves Python via the Windows registry, independent of PATH. If Python isn't installed at all, Claude Code shows "Failed with non-blocking status code" for every Write/Edit and the validator is silently inert — writes still proceed, but the worklist guard isn't actually checking them. Install Python 3 to enable enforcement.

</details>

## Configuration

`.bram.json` at project root is the primary config file. Legacy `.xmlui-desktop.json` is still accepted as a compatibility alias from Bram's prior name.

### Startup

Bram autostarts an agent in the terminal at launch. Configure it under
`shell` in `.bram.json` (the same keys Settings writes):

```json
{
  "shell": {
    "agent": "claude",
    "continueLast": true
  }
}
```

- `agent` — which provider to launch, `claude` or `codex` (defaults to `claude`).
- `args` — optional extra arguments appended to the launch command.
- `continueLast` — when `true`, resume the most recent session
  (`claude --continue` / `codex resume --last`) instead of starting fresh.
- `firstCommand` — optional command typed into the agent once it's ready.

## Voice input

Bram supports two ways to dictate instead of type:

- **🎤 Whisper buttons (recommended).** Local, low-latency dictation via [`whisper-server`](https://github.com/ggml-org/whisper.cpp/tree/master/examples/server). Click the 🎤 button in the parent-shell toolbar (or the agent pane) to start recording, click again to send; the transcript arrives in the terminal as a `voice: ...` line so it's distinguishable from typed input. This is the better experience — lower latency, your choice of model, good transcription quality — but it needs local setup.
- **The agent's native `/voice` command.** No local setup, but support varies by agent and platform. It's the zero-install fallback, and the working path where the Whisper button isn't proven yet.

Bram spawns the local `whisper-server` on the first record click and kills it on app exit — you don't manage the process; you just need the binary, `ffmpeg`, and a model file installed.

### macOS

```bash
brew install whisper-cpp ffmpeg
mkdir -p ~/.local/share/whisper-models
curl -L -o ~/.local/share/whisper-models/ggml-small.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
```

`small.en` is ~466 MB, English-only, real-time on Apple Silicon. Swap in a different model from the same Hugging Face repo for other size/accuracy/language tradeoffs. The bundled `Info.plist` declares `NSMicrophoneUsageDescription`, so first use triggers the standard macOS mic-permission prompt. The model path the app loads is `~/.local/share/whisper-models/ggml-small.en.bin`.

### Windows / WSL

On Windows, Bram launches `whisper-server` inside WSL via `wsl.exe bash -lc` and talks to it through `http://127.0.0.1:18080` from the WebView. WSL2 forwards that loopback port to Windows automatically, so the request path is the same as on macOS once the server is running.

**Prerequisite: WSL2 with Ubuntu.** If you don't already have it, open PowerShell and run `wsl --install`, which installs WSL and the default Ubuntu distro. Restart when prompted, then complete Ubuntu's first-run setup (pick a Linux username and password). Microsoft's full install doc: <https://learn.microsoft.com/en-us/windows/wsl/install>.

**One-time setup inside Ubuntu.** Open Ubuntu (Start menu → Ubuntu, or run `wsl` from PowerShell), then paste this whole block. The `cmake` build takes a few minutes on modern hardware; the model download is ~466 MB.

```bash
sudo apt update
sudo apt install -y build-essential cmake ffmpeg git curl
git clone https://github.com/ggml-org/whisper.cpp.git
cd whisper.cpp
cmake -B build
cmake --build build -j --config Release
sudo cp build/bin/whisper-server /usr/local/bin/
mkdir -p ~/.local/share/whisper-models
curl -L -o ~/.local/share/whisper-models/ggml-small.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
```

That's the whole install. Bram handles starting/stopping `whisper-server` on its own — first 🎤 click after launching Bram spawns it inside WSL (the button shows ⏳ for ~2-3 s while the model loads), and every click after that is fast until Bram exits.

**Notes:**

- **Mic permission.** WebView2 inherits the standard Windows microphone prompt. On first 🎤 click Windows asks to allow mic access; click **Yes**.
- **Multiple WSL distros.** If Ubuntu isn't your default distro, set `BRAM_WSL_DISTRO=Ubuntu` (or whatever distro name) in your Windows environment before launching Bram. Single-distro setups need no env var.
- **Already running?** If you happen to have `whisper-server` listening on port `18080` (e.g., started manually in another terminal), Bram detects it via a preflight probe and uses it instead of spawning a new one — no conflict.
- **Sanity check after install.** From inside Ubuntu, `which whisper-server` should print `/usr/local/bin/whisper-server`, and `ls ~/.local/share/whisper-models/` should show `ggml-small.en.bin`. If both look right, you're done.

### Linux

The same setup is expected to work on non-WSL Linux, using the host process path and port `18080`.

## Target app

An optional pane above the agent pane, **off by default** — a project iframe for previewing your app inside Bram. Most people run their app in their own server and view it in their own browser, so it stays hidden unless you turn it on (Settings → UI → "Show target app"). It's handy for a very simple app or a quick check, not the common case. When enabled, Bram runs a basic static webserver and reloads the pane automatically as project files change.

<details>
<summary>Serving a real backend: fixed origin, project server, URL parameters</summary>

#### Working with a real backend

`Bram` binds the target app HTTP server to
`127.0.0.1:<random-port>` (it uses port `0` and lets the OS pick).
That's fine for projects that talk only to public APIs or static
files. It breaks when your project needs a **fixed origin** — OAuth
callbacks, CORS allowlists, hardcoded API base URLs.

> **Compatibility note.** The target app is an iframe. Backends that
> send `X-Frame-Options: DENY` or `Content-Security-Policy:
> frame-ancestors 'none'` (common for security-sensitive admin UIs)
> cannot be loaded into the target app regardless of port. Workarounds:
> configure the backend's dev mode to relax those headers, or serve
> the UI files via a permissive dev server (e.g. `npx http-server`)
> while keeping the real backend running for API calls. Otherwise,
> open the project in a standalone browser.

##### Declare a project server

Add `.bram.json` at the project root:

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
| `port` | TCP port the iframe should target. Bram probes this port at startup. |
| `path` | URL path appended to `http://localhost:<port>` for the iframe. Optional; defaults to `/`. |

At startup, Bram:

- probes `127.0.0.1:<port>`. If it's already listening, it logs a notice
  and reuses the running server (useful when you start the server
  manually for log visibility);
- otherwise spawns `command` in `cwd`, with stdout/stderr forwarded to
  Bram's own stderr (prefixed `[server]`);
- waits up to 5s for the port to come up, then points the target app
  iframe at `http://localhost:<port><path>`. The iframe retries once on
  load error to absorb the case where the server takes a moment to bind;
- on app exit, kills the spawned child.

The agent pane continues to load from Bram's internal
loopback server regardless of this setting.

The app-under-test does not need to be an XMLUI app — `.bram.json`
is Bram's own config file, separate from XMLUI's `config.json`. Legacy
`.xmlui-desktop.json` remains supported.

#### URL parameters

Use query strings to parameterize the frontend without rebuilding —
e.g. `?city=santarosa` to switch tenant. Pass them on the command line
to your server's command or bake them into `path` (e.g.
`"path": "/?city=santarosa"`).

#### Working example

[community-calendar](https://github.com/judell/community-calendar) uses
`.bram.json` for GitHub-OAuth-via-Supabase development. See
[`docs/app-architecture.md`](https://github.com/judell/community-calendar/blob/main/docs/app-architecture.md)
for the Supabase URL-Configuration setup that requires the fixed
`localhost:8080/**` origin.

##### Fallback: the redirect pattern

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
(`python3 -m http.server 8080`) and launch Bram from the
project root. Its iframe loads the random-port URL once, your script
bounces it to `localhost:8080`. `.bram.json` is the preferred
mechanism — it auto-spawns the server, shows logs, and doesn't
pollute the project's HTML.

</details>

<details>
<summary>Service workers, auth callbacks, and DevTools</summary>

#### Service workers don't register on macOS/Linux

The target app iframe loads at `tauri://localhost`, and the WebKit
engines on macOS (WKWebView) and Linux (WebKitGTK) don't treat
custom-scheme origins as secure contexts. Service-worker registration
silently fails there, so project features that depend on a service
worker — Mock Service Worker (MSW), XMLUI's in-page
`apiInterceptor`, custom offline caches — won't activate inside
Bram on those platforms. Windows uses WebView2 (Chromium)
with the `http://tauri.localhost` form, which *is* a secure context,
so service workers register normally there.

Apps that hit a real HTTP backend are unaffected; the constraint only
applies to in-page request interception. If you're developing against
MSW or `apiInterceptor`, run your project in a regular browser tab at
`localhost:8080` while keeping Bram pointed at the same
server for the agent loop.

#### Auth callbacks won't reach the target app

The target app webview has its own browser storage, isolated from
your system browser's storage at the same origin. That breaks any
auth flow that hands off to the system browser and expects a session
to come back into the webview:

- **Magic links in email.** Clicking the link opens your default
  browser, completes auth there, and stores the session in the
  *browser's* `localStorage`. The target app never sees it.
- **OAuth provider redirects** that leave the webview have the same
  shape — the callback session lands in the wrong storage.

Even when the redirect script above lines the target app up on
`localhost:8080`, that origin's storage in the Tauri webview is a
different store from `localhost:8080` storage in Safari or Chrome.

**Workaround for email auth: send a one-time code, not a link.** If
your backend supports OTP codes (Supabase, Auth0, Clerk, Cognito all
do), have the user paste the code from the email into a field in
your dialog. No callback URL, no cross-context handoff. Works
identically in the browser and inside Bram.

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

#### DevTools

Tauri uses the platform's native webview, so the DevTools you get
inside the target app depend on the OS:

| Platform | Webview | DevTools |
|---|---|---|
| macOS | WKWebView | Safari Web Inspector |
| Linux | WebKitGTK | Safari Web Inspector |
| Windows | WebView2 (Chromium) | Chromium DevTools |

To open them, **right-click inside the target app → Inspect Element**
in dev/debug builds (`cargo run` or `cargo tauri dev`). Release
builds disable DevTools by default. The execution context belongs to
the target app document specifically. The shell window and the right
pane both load at `tauri://localhost` (the parent shell directly, the
target app via the scheme handler that proxies project content under
`/__project/*`), so they share an origin and therefore a `localStorage`
/ `IndexedDB` partition — a console session in either reaches the
same storage. A regular browser tab pointed at the project's own
`localhost:8080` server, by contrast, is a different origin with its
own independent storage.

##### WebKit quirks worth knowing

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
store from the target app's (the target app is at `tauri://localhost`,
a different origin), so a session created there won't carry into
Bram.

</details>

## Build

The frontend is static — no bundler, no `package.json`. The only build
step is the Tauri/Rust build.

From `src-tauri/`:

- **Dev:** `cargo run` (or `cargo tauri dev` with the Tauri CLI)
- **Release:** `cargo tauri build`

Tauri docs: <https://tauri.app/develop/>, <https://tauri.app/distribute/>.

### Calling Bram from project code

Because the target app is same-origin with the parent shell
(`tauri://localhost`), project code can reach the Tauri command bridge
directly through `window.parent` — no `postMessage` shim needed:

```js
const { invoke } = window.parent.__TAURI__.core;
const url = await invoke("get_right_pane_url");
```

Use this when an XMLUI app embedded in the target app needs to read
filesystem state, hit one of Bram's `__`-prefixed loopback
endpoints, or invoke any of the Rust IPC commands. The `helpers.js`
script loaded by the embedded XMLUI exposes (`toShell`, `toTurn`,
`openExternal`, `logToHost`) is built on top of this bridge — opt
into the helpers for project XMLUI apps that need to talk back to
the running agent.

## Layout

- `Main.xmlui`, `components/`, `resources/`, `Globals.xs`,
  `config.json`, `index.html` — the XMLUI app at the repo root.
- `app/` — parent shell (Tauri webview entry, terminal wiring, vendor
  scripts, and `__shell/helpers.js` that the target app includes).
- `src-tauri/` — Rust backend (PTY for the terminal, custom `tauri://`
  URI scheme handler that proxies the target app iframe to the project's
  HTTP server, filesystem watcher, IPC handlers).
- `scripts/` — auxiliary scripts.
