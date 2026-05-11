## Prerequisites

`xmlui-desktop` opens an XMLUI app next to your terminal — you need a project for it to open. If you don't already have one, follow the installation steps at <https://xmlui.org/get-started>. That gets you the XMLUI CLI (which includes the MCP server). If you followed those instructions to completion and have created ~/xmlui-weather, remove it and instead `git clone https://github.com/xmlui-org/xmlui-weather`. That will give you a repo with pre-existing git history to explore in the xmlui-desktop Workspace. You will be able to stage work items as local git commits to get a feel for what that's like.

Now continue with the steps here.

## Install

### macOS / Linux

```bash
curl -fsSL https://github.com/judell/xmlui-desktop/releases/latest/download/install.sh | bash
```

The script detects your platform, verifies the archive's SHA256 against the published `SHA256SUMS`, extracts the binary, and copies it to `/usr/local/bin` (if writable) or `~/.local/bin`. On macOS it also clears the `com.apple.quarantine` xattr. No `sudo` required.

Confirm the install:

```bash
xmlui-desktop --help
```

### Windows

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://github.com/judell/xmlui-desktop/releases/latest/download/install.ps1 | iex"
```

Downloads `xmlui-desktop-windows-amd64.zip`, verifies its SHA256, extracts `xmlui-desktop.exe` to `~/bin`, and adds `~/bin` to your user `PATH`.

Confirm the install in a new PowerShell window:

```powershell
xmlui-desktop --help
```

## Audit-friendly manual install

```bash
# Download artifact + checksums
curl -fsSLO https://github.com/judell/xmlui-desktop/releases/download/${TAG}/SHA256SUMS
curl -fsSLO https://github.com/judell/xmlui-desktop/releases/download/${TAG}/xmlui-desktop-macos-arm64.tar.gz   # or your platform
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf xmlui-desktop-*.tar.gz
sudo mv xmlui-desktop /usr/local/bin/
```

Other platforms: replace `xmlui-desktop-macos-arm64.tar.gz` with `xmlui-desktop-macos-intel.tar.gz`, `xmlui-desktop-linux-amd64.tar.gz`, or `xmlui-desktop-windows-amd64.zip`.

On macOS, if installing from a browser download instead of `curl`, also run:

```bash
xattr -d com.apple.quarantine xmlui-desktop
```

On Windows, use `Expand-Archive` on `xmlui-desktop-windows-amd64.zip`, then move `xmlui-desktop.exe` to a directory on your `PATH`.

## Change log

${CHANGELOG}

## Troubleshooting

- **`xmlui-desktop` not found on PATH.** Re-run the install script, or follow its printed PATH advice.
- **macOS Gatekeeper blocks first launch.** The install script clears the quarantine xattr automatically. For browser downloads, run the `xattr -d com.apple.quarantine` command above.
- **Linux/WSL: `error while loading shared libraries: libwebkit2gtk-4.1.so.0`.** Tauri's WebView dynamically links WebKitGTK. On Ubuntu/Debian 24.04+, install the runtime libs with `sudo apt install -y libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2`. On Ubuntu 22.04, the `4.1` package isn't in the repos — upgrade to 24.04. WSL2 also needs WSLg (ships with Windows 11 and recent Windows 10 builds).
- **Update.** Re-run the install command.
- **Uninstall.** Delete the binary from `/usr/local/bin/xmlui-desktop`, `~/.local/bin/xmlui-desktop`, or `~/bin/xmlui-desktop.exe`.
