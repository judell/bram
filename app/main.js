// Tauri 2 exposes the API on window.__TAURI__ when withGlobalTauri is true.
// https://v2.tauri.app/reference/javascript/api/
const { invoke, Channel } = window.__TAURI__.core;

invoke("log_from_right_pane", {
  payload: { kind: "main.js-loaded", at: new Date().toISOString() },
}).catch(() => {});

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

(() => {
  const splitter = document.getElementById("splitter");
  const left = document.querySelector(".pane-left");
  const split = document.querySelector(".split");
  if (!splitter || !left || !split) return;

  const MIN_PX = 200;

  splitter.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    splitter.setPointerCapture(e.pointerId);
    splitter.classList.add("dragging");
    document.body.classList.add("splitter-dragging");

    const onMove = (ev) => {
      const rect = split.getBoundingClientRect();
      let x = ev.clientX - rect.left;
      const max = rect.width - MIN_PX - splitter.offsetWidth;
      if (x < MIN_PX) x = MIN_PX;
      if (x > max) x = max;
      left.style.flexBasis = x + "px";
      fitAddon.fit();
    };
    const onUp = (ev) => {
      splitter.releasePointerCapture(ev.pointerId);
      splitter.classList.remove("dragging");
      document.body.classList.remove("splitter-dragging");
      splitter.removeEventListener("pointermove", onMove);
      splitter.removeEventListener("pointerup", onUp);
      fitAddon.fit();
    };
    splitter.addEventListener("pointermove", onMove);
    splitter.addEventListener("pointerup", onUp);
  });
})();

// Horizontal splitter resizes the tools drawer (only operative when drawer
// is open; the splitter is `display: none` when the .hidden class is set).
(() => {
  const hSplitter = document.getElementById("h-splitter");
  const tools = document.getElementById("tools-pane");
  const column = document.querySelector(".right-column");
  if (!hSplitter || !tools || !column) return;

  const MIN_PX = 80;

  hSplitter.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    hSplitter.setPointerCapture(e.pointerId);
    hSplitter.classList.add("dragging");
    document.body.classList.add("splitter-dragging");

    const onMove = (ev) => {
      const rect = column.getBoundingClientRect();
      // Drawer height = distance from pointer to bottom of column.
      let h = rect.bottom - ev.clientY;
      const max = rect.height - MIN_PX - hSplitter.offsetHeight;
      if (h < MIN_PX) h = MIN_PX;
      if (h > max) h = max;
      tools.style.flexBasis = h + "px";
    };
    const onUp = (ev) => {
      hSplitter.releasePointerCapture(ev.pointerId);
      hSplitter.classList.remove("dragging");
      document.body.classList.remove("splitter-dragging");
      hSplitter.removeEventListener("pointermove", onMove);
      hSplitter.removeEventListener("pointerup", onUp);
    };
    hSplitter.addEventListener("pointermove", onMove);
    hSplitter.addEventListener("pointerup", onUp);
  });
})();

// Bottom-toolbar drawer toggle. v1 has a single "tools" button — later
// stages will add per-tool buttons (Workspace, Sessions) that swap the
// tools iframe's hash route while keeping the drawer open.
(() => {
  const btn = document.getElementById("toggle-tools");
  const tools = document.getElementById("tools-pane");
  const hSplitter = document.getElementById("h-splitter");
  if (!btn || !tools || !hSplitter) return;
  btn.addEventListener("click", () => {
    const opening = tools.classList.contains("hidden");
    tools.classList.toggle("hidden", !opening);
    hSplitter.classList.toggle("hidden", !opening);
  });
})();

// PTY wiring: stdout from Rust arrives over a Channel; stdin goes via invoke.
// https://v2.tauri.app/develop/calling-frontend/#channels
const ptyChannel = new Channel();

ptyChannel.onmessage = (chunk) => {
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
      args: ["--noprofile", "--rcfile", "./app/shell/claude-code-shellrc", "-i"],
      cols: term.cols,
      rows: term.rows,
      onData: ptyChannel,
    });
    term.focus();
  } catch (e) {
    term.writeln(`\r\n\x1b[31mfailed to start pty: ${e}\x1b[0m`);
  }
})();

// Right pane → parent shell dispatcher.
window.addEventListener("message", (ev) => {
  if (!ev.data || ev.data.type !== "right-pane") return;
  const data = ev.data;

  switch (data.kind) {
    case "to-shell":
      invoke("pty_write", { data: (data.text ?? "") + "\n" }).catch((e) =>
        console.error("pty_write inject", e),
      );
      return;
    case "to-turn":
      invoke("pty_write", {
        data: "\x1b[200~" + String(data.text ?? "") + "\x1b[201~\r",
      }).catch((e) => console.error("pty_write turn", e));
      return;
    case "open-devtools":
      invoke("open_devtools").catch((e) =>
        console.error("open_devtools", e),
      );
      return;
    case "open-url":
      invoke("open_url", { url: String(data.url ?? "") }).catch((e) =>
        console.error("open_url", e),
      );
      return;
    case "git-push":
      invoke("git_push")
        .then(() => {
          if (ev.source && typeof ev.source.postMessage === "function") {
            ev.source.postMessage({ type: "git-push-result", ok: true }, "*");
          }
        })
        .catch((e) => {
          invoke("log_from_right_pane", {
            payload: { kind: "git-push", phase: "err", error: String(e) },
          }).catch(() => {});
          if (ev.source && typeof ev.source.postMessage === "function") {
            ev.source.postMessage(
              { type: "git-push-result", ok: false, error: String(e) },
              "*"
            );
          }
        });
      return;
    case "save-trace-export":
      invoke("save_trace_export", {
        filename: String(data.filename ?? "xs-trace.json"),
        content: String(data.content ?? ""),
        mimeType: String(data.mimeType ?? "application/octet-stream"),
      })
        .then((path) => {
          if (ev.source && typeof ev.source.postMessage === "function") {
            ev.source.postMessage(
              {
                type: "save-trace-export-result",
                requestId: data.requestId,
                ok: true,
                path,
              },
              "*",
            );
          }
        })
        .catch((e) => {
          if (ev.source && typeof ev.source.postMessage === "function") {
            ev.source.postMessage(
              {
                type: "save-trace-export-result",
                requestId: data.requestId,
                ok: false,
                error: String(e?.message ?? e ?? "export failed"),
              },
              "*",
            );
          }
          console.error("save_trace_export", e);
        });
      return;
    case "log":
    default:
      invoke("log_from_right_pane", {
        payload: data.payload ?? data,
      }).catch((e) => console.error("log_from_right_pane", e));
      return;
  }
});

// Right-pane base URL is provisioned by the Rust backend on startup
// (loopback HTTP server bound to a random port). We have to ask for it
// before we can set iframe.src or wire reload. Reassigning src works
// cross-origin; iframe.contentWindow.location.reload() is blocked
// because the parent shell is on tauri:// and the iframe on http://.
const { listen } = window.__TAURI__.event;
(async () => {
  const iframe = document.getElementById("right-pane");
  if (!iframe) return;
  let base;
  try {
    base = await invoke("get_right_pane_url");
  } catch (e) {
    console.error("get_right_pane_url failed", e);
    return;
  }
  const RIGHT_PANE_SRC = base + "/index.html";
  function reloadRightPane() {
    iframe.src = RIGHT_PANE_SRC + "?t=" + Date.now();
  }
  iframe.src = RIGHT_PANE_SRC;
  document
    .getElementById("reload-right")
    ?.addEventListener("click", reloadRightPane);
  listen("right-pane-reload", reloadRightPane);
})();
