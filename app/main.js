// Tauri 2 exposes the API on window.__TAURI__ when withGlobalTauri is true.
// https://v2.tauri.app/reference/javascript/api/
const { invoke, Channel } = window.__TAURI__.core;

const term = new Terminal({
  fontFamily: 'Menlo, Monaco, "Courier New", monospace',
  fontSize: 13,
  cursorBlink: true,
  theme: { background: "#000000", foreground: "#e0e0e0" },
  scrollback: 10000,
  allowProposedApi: true,
});

const fitAddon = new FitAddon.FitAddon();
term.loadAddon(fitAddon);

const container = document.getElementById("terminal");
term.open(container);

try {
  const webgl = new WebglAddon.WebglAddon();
  term.loadAddon(webgl);
  webgl.onContextLoss(() => webgl.dispose());
} catch (e) {
  console.warn("webgl addon failed, falling back to canvas/dom renderer", e);
}

fitAddon.fit();
window.addEventListener("resize", () => fitAddon.fit());

// PTY wiring: stdout from Rust arrives over a Channel; stdin goes via invoke.
// https://v2.tauri.app/develop/calling-frontend/#channels
const ptyChannel = new Channel();
ptyChannel.onmessage = (chunk) => {
  // chunk is a number[] (bytes) coming from Rust
  term.write(new Uint8Array(chunk));
};

term.onData((data) => {
  invoke("pty_write", { data }).catch((e) => console.error("pty_write", e));
});

term.onResize(({ cols, rows }) => {
  invoke("pty_resize", { cols, rows }).catch((e) => console.error("pty_resize", e));
});

(async () => {
  try {
    await invoke("pty_spawn", {
      cmd: "/bin/bash",
      args: ["-l"],
      cols: term.cols,
      rows: term.rows,
      onData: ptyChannel,
    });
    // Auto-launch claude as the first command. Ctrl-C drops back to bash.
    await invoke("pty_write", { data: "claude\n" });
    term.focus();
  } catch (e) {
    term.writeln(`\r\n\x1b[31mfailed to start pty: ${e}\x1b[0m`);
  }
})();

// Right pane → parent shell dispatcher. The iframe posts events declaring
// one of three intents:
//   to-shell      — inject text into the PTY
//   log           — record in cargo run stderr only
//   open-devtools — internal command
window.addEventListener("message", (ev) => {
  if (!ev.data || ev.data.type !== "right-pane") return;
  const data = ev.data;

  switch (data.kind) {
    case "to-shell":
      invoke("pty_write", { data: (data.text ?? "") + "\n" }).catch((e) =>
        console.error("pty_write inject", e),
      );
      return;
    case "open-devtools":
      invoke("open_devtools").catch((e) =>
        console.error("open_devtools", e),
      );
      return;
    case "log":
    default:
      invoke("log_from_right_pane", {
        payload: data.payload ?? data,
      }).catch((e) => console.error("log_from_right_pane", e));
      return;
  }
});

// Reassigning src works cross-origin; iframe.contentWindow.location.reload()
// is blocked because the parent shell is on tauri:// and the iframe on xmlui://.
const RIGHT_PANE_SRC = "xmlui://localhost/right/index.html";
function reloadRightPane() {
  const iframe = document.getElementById("right-pane");
  if (!iframe) return;
  iframe.src = RIGHT_PANE_SRC + "?t=" + Date.now();
}

// Manual reload button in the toolbar.
document
  .getElementById("reload-right")
  ?.addEventListener("click", reloadRightPane);

// Live reload: Rust filesystem watcher emits "right-pane-reload" when files in
// app/right/ change. https://v2.tauri.app/develop/calling-frontend/#event-system
const { listen } = window.__TAURI__.event;
listen("right-pane-reload", reloadRightPane);
