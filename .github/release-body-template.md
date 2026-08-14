## Prerequisites

`Bram` opens an app next to your terminal, so you need a project for it to open. Any web app works — vanilla HTML/JS, a React or other Node app, a Python web app, an XMLUI app, really anything you'd otherwise iterate on in a browser tab.

One way to try Bram with real git history is:

```bash
git clone https://github.com/xmlui-org/xmlui-weather
```

That gives you a working repo to explore in the Bram workspace, and you can stage work items as local git commits to get a feel for that flow. If you want Bram to modify `xmlui-weather`, install the XMLUI CLI (which also includes the MCP server) per <https://xmlui.org/get-started>. Note that Bram itself doesn't require XMLUI; substitute whatever toolchain your project needs.

Now continue with the steps here.

## Install

### macOS / Linux

```bash
curl -fsSL https://github.com/judell/bram/releases/download/${TAG}/install.sh | bash
```

The script detects your platform, verifies the archive's SHA256 against the published `SHA256SUMS`, extracts the binary, and copies it to `/usr/local/bin` (if writable) or `~/.local/bin`. On macOS it also clears the `com.apple.quarantine` xattr. No `sudo` required.

Confirm the install:

```bash
bram --help
```

### Windows

From PowerShell:

```powershell
irm https://github.com/judell/bram/releases/download/${TAG}/install.ps1 | Out-String | iex
```

From Command Prompt:

```batch
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "irm https://github.com/judell/bram/releases/download/${TAG}/install.ps1 | Out-String | iex"
```

Downloads `bram-windows-amd64.zip`, verifies its SHA256, extracts `bram.exe` to `~/bin`, and adds `~/bin` to your user `PATH`.

Confirm the install in a new PowerShell window:

```powershell
bram --help
```

## Audit-friendly manual install

```bash
# Download artifact + checksums
curl -fsSLO https://github.com/judell/bram/releases/download/${TAG}/SHA256SUMS
curl -fsSLO https://github.com/judell/bram/releases/download/${TAG}/bram-macos-arm64.tar.gz   # or your platform
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf bram-*.tar.gz
sudo mv bram /usr/local/bin/
```

Other platforms: replace `bram-macos-arm64.tar.gz` with `bram-macos-intel.tar.gz`, `bram-linux-amd64.tar.gz`, or `bram-windows-amd64.zip`.

On macOS, if installing from a browser download instead of `curl`, also run:

```bash
xattr -d com.apple.quarantine bram
```

On Windows, use `Expand-Archive` on `bram-windows-amd64.zip`, then move `bram.exe` to a directory on your `PATH`.

## Change log

${CHANGELOG}

## Troubleshooting

- **`bram` not found on PATH.** Re-run the install script, or follow its printed PATH advice.
- **macOS Gatekeeper blocks first launch.** The install script clears the quarantine xattr automatically. For browser downloads, run the `xattr -d com.apple.quarantine` command above.
- **Linux/WSL: `error while loading shared libraries: libwebkit2gtk-4.1.so.0`.** Tauri's WebView dynamically links WebKitGTK. On Ubuntu/Debian 22.04+, install the runtime libs with `sudo apt install -y libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2`. WSL2 also needs WSLg (ships with Windows 11 and recent Windows 10 builds).
- **Tool Descriptions show nothing (trace says `reason=no-key`).** The optional Haiku-generated intent headers need `ai.describeCommands: true` in `.bram.json` **and** `ANTHROPIC_API_KEY` visible to Bram's own process. macOS/Linux: export it in your shell profile and launch `bram` from a terminal — a Dock/Finder-launched Bram doesn't read shell profiles. Windows: set a user-scoped environment variable — `setx ANTHROPIC_API_KEY "sk-ant-…"` (or System Properties → Environment Variables) — then restart Bram and any open terminals; a PowerShell `$PROFILE` export is not enough, because it reaches shell-launched processes only, not a GUI-launched Bram. One user-scoped variable serves Bram's description calls and the agent CLIs inside Bram's PTY alike. Note: when Claude Code notices the key and asks whether to use it, answer **No** — accepting switches the CLI from your claude.ai login (and disables claude.ai connectors) to pay-as-you-go API billing. The key is stored in plaintext (`HKCU\Environment` on Windows, your dotfile elsewhere) — same posture either way; treat it with dotfile-grade care.
- **Update.** Re-run the install command.
- **Uninstall.** Delete the binary from `/usr/local/bin/bram`, `~/.local/bin/bram`, or `~/bin/bram.exe`.
