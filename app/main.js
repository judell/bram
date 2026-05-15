// Tauri 2 exposes the API on window.__TAURI__ when withGlobalTauri is true.
// https://v2.tauri.app/reference/javascript/api/
const { invoke, Channel } = window.__TAURI__.core;

invoke("log_from_right_pane", {
  payload: { kind: "main.js-loaded", at: new Date().toISOString() },
}).catch(() => {});

const TERM_FONT_KEY = "xmlui-desktop.terminal.fontSize";
const TERM_FONT_MIN = 8;
const TERM_FONT_MAX = 32;
const TERM_FONT_DEFAULT = 13;

const clampFontSize = (n) =>
  Math.max(TERM_FONT_MIN, Math.min(TERM_FONT_MAX, Math.round(Number(n) || 0)));

const readSavedFontSize = () => {
  try {
    const raw = parseInt(localStorage.getItem(TERM_FONT_KEY) ?? "", 10);
    return Number.isFinite(raw) ? clampFontSize(raw) : TERM_FONT_DEFAULT;
  } catch {
    return TERM_FONT_DEFAULT;
  }
};

const term = new Terminal({
  fontFamily: 'Menlo, Monaco, "Courier New", monospace',
  fontSize: readSavedFontSize(),
  cursorBlink: true,
  theme: { background: "#000000", foreground: "#e0e0e0" },
  scrollback: 10000,
  allowProposedApi: true,
});

const fitAddon = new FitAddon.FitAddon();
term.loadAddon(fitAddon);
const PTY_RESIZE_MIN_INTERVAL_MS = 40;
const VIEWPORT_RESTORE_WINDOW_MS = 750;

const container = document.getElementById("terminal");
term.open(container);

try {
  const webgl = new WebglAddon.WebglAddon();
  term.loadAddon(webgl);
  webgl.onContextLoss(() => webgl.dispose());
} catch (e) {
  console.warn("webgl addon failed, falling back to canvas/dom renderer", e);
}

const captureViewport = () => {
  const buffer = term.buffer?.active;
  if (!buffer) return null;
  const viewportEl = container.querySelector(".xterm-viewport");
  return {
    viewportY: buffer.viewportY || 0,
    baseY: buffer.baseY || 0,
    atBottom: (buffer.baseY || 0) - (buffer.viewportY || 0) <= 1,
    domScrollTop: viewportEl ? viewportEl.scrollTop : null,
  };
};

const restoreViewport = (snapshot) => {
  if (!snapshot) return;
  const buffer = term.buffer?.active;
  if (!buffer) return;
  const viewportEl = container.querySelector(".xterm-viewport");
  if (snapshot.atBottom) {
    term.scrollToBottom();
    if (viewportEl) viewportEl.scrollTop = viewportEl.scrollHeight;
    return;
  }
  const maxViewport = buffer.baseY || 0;
  const target = Math.max(0, Math.min(snapshot.viewportY, maxViewport));
  term.scrollToLine(target);
  if (viewportEl && snapshot.domScrollTop !== null) {
    viewportEl.scrollTop = snapshot.domScrollTop;
  }
};

let pendingViewportRestore = null;
let pendingViewportRestoreUntil = 0;
let pendingViewportRestoreTimer = null;

const clearPendingViewportRestore = () => {
  pendingViewportRestore = null;
  pendingViewportRestoreUntil = 0;
  clearTimeout(pendingViewportRestoreTimer);
  pendingViewportRestoreTimer = null;
};

const armViewportRestore = (snapshot) => {
  if (!snapshot) return;
  pendingViewportRestore = snapshot;
  pendingViewportRestoreUntil = Date.now() + VIEWPORT_RESTORE_WINDOW_MS;
  clearTimeout(pendingViewportRestoreTimer);
  pendingViewportRestoreTimer = setTimeout(
    clearPendingViewportRestore,
    VIEWPORT_RESTORE_WINDOW_MS,
  );
};

const restorePendingViewport = () => {
  if (!pendingViewportRestore) return;
  if (Date.now() > pendingViewportRestoreUntil) {
    clearPendingViewportRestore();
    return;
  }
  restoreViewport(pendingViewportRestore);
};

const runTerminalFit = ({ preserveViewport = true } = {}) => {
  const snapshot = preserveViewport ? captureViewport() : null;
  fitAddon.fit();
  if (!snapshot) return;
  armViewportRestore(snapshot);
  requestAnimationFrame(() => {
    restoreViewport(snapshot);
    requestAnimationFrame(() => restoreViewport(snapshot));
  });
};

let fitScheduled = false;
let fitNeedsViewportPreserve = false;
const scheduleTerminalFit = ({ preserveViewport = true } = {}) => {
  fitNeedsViewportPreserve = fitNeedsViewportPreserve || preserveViewport;
  if (fitScheduled) return;
  fitScheduled = true;
  requestAnimationFrame(() => {
    const shouldPreserve = fitNeedsViewportPreserve;
    fitScheduled = false;
    fitNeedsViewportPreserve = false;
    runTerminalFit({ preserveViewport: shouldPreserve });
  });
};

scheduleTerminalFit({ preserveViewport: false });
window.addEventListener("resize", () => scheduleTerminalFit());

const setTerminalFontSize = (n) => {
  const size = clampFontSize(n);
  term.options.fontSize = size;
  runTerminalFit();
  try {
    localStorage.setItem(TERM_FONT_KEY, String(size));
  } catch {}
};

const isMac = /Mac|iPhone|iPod|iPad/i.test(navigator.userAgent);

term.attachCustomKeyEventHandler((ev) => {
  if (ev.type !== "keydown") return true;
  // Don't interfere with AltGr (Ctrl+Alt on Win/Linux produces @, |, [, ]
  // etc. on non-US layouts).
  if (ev.altKey) return true;

  // Non-macOS terminal copy/paste:
  //   - Plain Ctrl+C: copies the selection when one exists (Windows Terminal
  //     behavior); falls through to xterm.js → SIGINT when there's no
  //     selection. We avoid Ctrl+Shift+C because WebView2 owns that combo
  //     at the native layer for the Edge "Inspect Element" devtools
  //     accelerator, which fires before our JS handler can preventDefault.
  //   - Ctrl+Shift+V: pastes clipboard text via the bracketed-paste path.
  //     preventDefault stops the WebView's native paste event from also
  //     firing — without it, xterm.js's textarea paste listener would
  //     write the clipboard a second time.
  if (!isMac && ev.ctrlKey && !ev.shiftKey && (ev.key === "c" || ev.key === "C")) {
    const sel = term.getSelection();
    if (sel) {
      ev.preventDefault();
      navigator.clipboard.writeText(sel).catch((e) =>
        console.error("clipboard write", e),
      );
      return false;
    }
    // No selection: let xterm.js send ^C → SIGINT.
  }
  if (!isMac && ev.ctrlKey && ev.shiftKey && (ev.key === "V" || ev.key === "v")) {
    ev.preventDefault();
    navigator.clipboard
      .readText()
      .then((text) => {
        if (!text) return;
        invoke("pty_write", {
          data: "\x1b[200~" + text + "\x1b[201~",
        }).catch((e) => console.error("pty_write paste", e));
      })
      .catch((e) => console.error("clipboard read", e));
    return false;
  }

  // Font-size shortcuts: Cmd on macOS, Ctrl elsewhere.
  const mod = isMac ? ev.metaKey : ev.ctrlKey;
  if (!mod) return true;
  if (ev.key === "=" || ev.key === "+") {
    setTerminalFontSize(term.options.fontSize + 1);
    return false;
  }
  if (ev.key === "-" || ev.key === "_") {
    setTerminalFontSize(term.options.fontSize - 1);
    return false;
  }
  if (ev.key === "0") {
    setTerminalFontSize(TERM_FONT_DEFAULT);
    return false;
  }
  return true;
});

document
  .getElementById("font-smaller")
  ?.addEventListener("click", () => setTerminalFontSize(term.options.fontSize - 1));
document
  .getElementById("font-larger")
  ?.addEventListener("click", () => setTerminalFontSize(term.options.fontSize + 1));

(() => {
  const TERMINAL_HIDDEN_KEY = "xmlui-desktop.terminal.hidden";
  const btn = document.getElementById("toggle-terminal");
  if (!btn) return;

  const apply = (hidden) => {
    document.body.classList.toggle("terminal-hidden", hidden);
    if (!hidden) {
      // Re-measure xterm.js once the layout settles.
      scheduleTerminalFit();
    }
  };

  let initial = false;
  try {
    initial = localStorage.getItem(TERMINAL_HIDDEN_KEY) === "1";
  } catch {}
  apply(initial);

  btn.addEventListener("click", () => {
    const hidden = !document.body.classList.contains("terminal-hidden");
    apply(hidden);
    try {
      localStorage.setItem(TERMINAL_HIDDEN_KEY, hidden ? "1" : "0");
    } catch {}
  });
})();

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
      scheduleTerminalFit();
    };
    const onUp = (ev) => {
      splitter.releasePointerCapture(ev.pointerId);
      splitter.classList.remove("dragging");
      document.body.classList.remove("splitter-dragging");
      splitter.removeEventListener("pointermove", onMove);
      splitter.removeEventListener("pointerup", onUp);
      runTerminalFit();
    };
    splitter.addEventListener("pointermove", onMove);
    splitter.addEventListener("pointerup", onUp);
  });
})();

// Horizontal splitter resizes the tools drawer (only operative when drawer
// is open; the splitter is `display: none` when the .hidden class is set).
(() => {
  const hSplitter = document.getElementById("h-splitter");
  const column = document.querySelector(".right-column");
  if (!hSplitter || !column) return;

  const MIN_PX = 80;

  hSplitter.addEventListener("pointerdown", (e) => {
    // Re-query the tools iframe on each drag — swapToolsIframe replaces
    // it on every watcher reload, and a captured reference from IIFE
    // boot would point at a detached node (cursor changed on hover but
    // dragging silently set flexBasis on a node no longer in the layout).
    const tools = document.getElementById("tools-pane");
    if (!tools) return;
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
  if (pendingViewportRestore) {
    requestAnimationFrame(() => {
      restorePendingViewport();
      requestAnimationFrame(() => restorePendingViewport());
    });
  }
};

term.onData((data) => {
  invoke("pty_write", { data }).catch((e) => console.error("pty_write", e));
});

let pendingPtySize = null;
let lastSentPtySize = null;
let ptyResizeTimer = null;
let lastPtyResizeAt = 0;

const samePtySize = (a, b) => !!a && !!b && a.cols === b.cols && a.rows === b.rows;

const flushPtyResize = () => {
  if (!pendingPtySize) return;
  const now = Date.now();
  const sinceLast = now - lastPtyResizeAt;
  if (sinceLast < PTY_RESIZE_MIN_INTERVAL_MS) {
    clearTimeout(ptyResizeTimer);
    ptyResizeTimer = setTimeout(
      flushPtyResize,
      PTY_RESIZE_MIN_INTERVAL_MS - sinceLast,
    );
    return;
  }
  const next = pendingPtySize;
  pendingPtySize = null;
  if (samePtySize(next, lastSentPtySize)) return;
  armViewportRestore(captureViewport());
  lastSentPtySize = next;
  lastPtyResizeAt = now;
  invoke("pty_resize", next).catch((e) => console.error("pty_resize", e));
};

term.onResize(({ cols, rows }) => {
  const next = { cols, rows };
  if (samePtySize(next, pendingPtySize) || samePtySize(next, lastSentPtySize)) return;
  pendingPtySize = next;
  flushPtyResize();
});

const isWindows = navigator.userAgent.toLowerCase().includes("windows");
const ptyShell = isWindows
  ? {
      cmd: "powershell.exe",
      args: [
        "-NoLogo",
        "-NoExit",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "./app/shell/claude-code-profile.ps1",
      ],
    }
  : {
      cmd: "/bin/bash",
      args: ["--noprofile", "--rcfile", "./app/shell/claude-code-shellrc", "-i"],
    };

(async () => {
  try {
    await invoke("pty_spawn", {
      ...ptyShell,
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
    case "to-turn": {
      const turnText = String(data.text ?? "");
      invoke("log_from_right_pane", {
        payload: {
          kind: "to-turn",
          stage: "sink",
          textLength: turnText.length,
          textPreview: turnText.slice(0, 80),
          at: new Date().toISOString(),
        },
      }).catch(() => {});
      invoke("pty_write", {
        data: "\x15\x1b[200~" + turnText + "\x1b[201~\r",
      }).catch((e) => console.error("pty_write turn", e));
      return;
    }
    case "send-keys":
      invoke("pty_write", { data: String(data.text ?? "") }).catch((e) =>
        console.error("pty_write send-keys", e),
      );
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
    case "get-right-pane-size": {
      const iframe = document.getElementById("right-pane");
      const rect = iframe ? iframe.getBoundingClientRect() : { width: 0, height: 0 };
      if (ev.source && typeof ev.source.postMessage === "function") {
        ev.source.postMessage(
          {
            type: "right-pane-size-result",
            requestId: data.requestId,
            width: Math.round(rect.width),
            height: Math.round(rect.height),
          },
          "*",
        );
      }
      return;
    }
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
    case "capture-screenshot":
      invoke("capture_screenshot", {})
        .then((path) => {
          if (ev.source && typeof ev.source.postMessage === "function") {
            ev.source.postMessage(
              {
                type: "capture-screenshot-result",
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
                type: "capture-screenshot-result",
                requestId: data.requestId,
                ok: false,
                error: String(e?.message ?? e ?? "capture failed"),
              },
              "*",
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

// Broadcast the right-pane iframe's pixel size to any subscribed iframe
// whenever it resizes — window resize, splitter drag, drawer toggle,
// anything that changes its box. ResizeObserver covers them all without
// having to instrument each interaction. helpers.js'
// subscribeRightPaneSize() picks this up and forwards to a callback so
// the info-dialog "Screen size" readout stays live.
(() => {
  const iframe = document.getElementById("right-pane");
  if (!iframe || typeof ResizeObserver !== "function") return;
  const broadcast = () => {
    // Drag-throttle: while a splitter is being dragged, broadcasting on
    // every pointer-move event cascades through both iframes' reactivity
    // and locks up the UI. The class is set by the v-splitter and
    // h-splitter handlers; cleared on pointerup. We broadcast once on
    // release via the pointerup handler below.
    if (document.body.classList.contains("splitter-dragging")) return;
    const rect = iframe.getBoundingClientRect();
    const payload = {
      type: "right-pane-size-changed",
      width: Math.round(rect.width),
      height: Math.round(rect.height),
    };
    const tools = document.getElementById("tools-pane");
    if (tools && tools.contentWindow) {
      tools.contentWindow.postMessage(payload, "*");
    }
    if (iframe.contentWindow) {
      iframe.contentWindow.postMessage(payload, "*");
    }
  };
  new ResizeObserver(broadcast).observe(iframe);
  // After a splitter drag ends, fire one final broadcast so subscribers
  // see the post-drag size. Using capture phase + body-level listener so
  // we don't have to plumb through every splitter handler.
  document.addEventListener("pointerup", () => {
    // The class is removed by the splitter handlers' onUp; defer one
    // frame to make sure that's happened, then broadcast.
    requestAnimationFrame(broadcast);
  });
})();

// Right-pane base URL is provisioned by the Rust backend on startup
// (loopback HTTP server bound to a random port). We have to ask for it
// before we can set iframe.src or wire reload. Reassigning src works
// cross-origin; iframe.contentWindow.location.reload() is blocked
// because the parent shell is on tauri:// and the iframe on http://.
const { listen } = window.__TAURI__.event;
(async () => {
  const iframe = document.getElementById("right-pane");
  if (!iframe) return;
  let RIGHT_PANE_SRC, TOOLS_PANE_SRC;
  try {
    [RIGHT_PANE_SRC, TOOLS_PANE_SRC] = await Promise.all([
      invoke("get_right_pane_url"),
      invoke("get_tools_pane_url"),
    ]);
  } catch (e) {
    console.error("get_*_pane_url failed", e);
    return;
  }
  const tools = document.getElementById("tools-pane");
  // Cache-bust by appending t=<now>. RIGHT_PANE_SRC may already contain
  // a path (e.g. http://localhost:8080/) but no query string — we
  // document `path` in .xmlui-desktop.json as path-only, so `?` is safe.
  const bust = (u) => u + (u.includes("?") ? "&" : "?") + "t=" + Date.now();
  // Double-buffer swap for the tools iframe so reloads don't flash a
  // blank frame. Create a new iframe off-screen, wait for `load`, then
  // promote it (replace the old in the DOM with the new, inheriting the
  // id/class/style so the rest of the parent shell keeps working). A
  // single-flight guard prevents overlapping swaps; the debounced
  // watcher emits at most one reload per 500ms anyway.
  let toolsSwapping = false;
  function swapToolsIframe(newSrc) {
    const oldTools = document.getElementById("tools-pane");
    if (!oldTools) return;
    const parent = oldTools.parentElement;
    if (!parent || toolsSwapping) return;
    toolsSwapping = true;

    const newTools = document.createElement("iframe");
    newTools.setAttribute("allow", oldTools.getAttribute("allow") || "");
    // Load off-screen so the user never sees the blank intermediate
    // state. Tiny size keeps it from affecting layout.
    newTools.style.cssText =
      "position:absolute;visibility:hidden;left:-99999px;top:0;width:1px;height:1px;";

    function onLoad() {
      newTools.removeEventListener("load", onLoad);
      // Promote: inherit the live class/style and the id, then replace
      // in the same DOM position. The toggle-tools button keeps working
      // because it queries by id on each click.
      newTools.className = oldTools.className;
      newTools.style.cssText = oldTools.style.cssText;
      newTools.id = "tools-pane";
      parent.replaceChild(newTools, oldTools);
      toolsSwapping = false;
    }
    newTools.addEventListener("load", onLoad);
    parent.appendChild(newTools);
    newTools.src = newSrc;
  }
  // reloadAll: reload BOTH iframes. Used by the manual "reload xmlui app"
  // toolbar button and by the "tools-pane-reload" watcher event (drawer
  // code changed, both panes may consume it). Right pane swaps src in
  // place (the user's project app handles its own loading state); tools
  // pane goes through swapToolsIframe to avoid the flash.
  function reloadAll() {
    iframe.src = bust(RIGHT_PANE_SRC);
    swapToolsIframe(bust(TOOLS_PANE_SRC));
  }
  // reloadRightPaneOnly: reload only the right pane. Used by the
  // "right-pane-reload" watcher event for user-project file changes AND
  // for .xmlui-desktop.json hot-reload (path/query updates). We re-fetch
  // the URL each time instead of reusing the captured one so config edits
  // are picked up. The drawer is poll-driven so it does NOT need to reload
  // here, and keeping it stable avoids postMessage-vs-iframe-rebuild races
  // on Approve/Drop clicks while the agent is writing files.
  async function reloadRightPaneOnly() {
    try {
      RIGHT_PANE_SRC = await invoke("get_right_pane_url");
    } catch (e) {
      console.error("get_right_pane_url failed", e);
    }
    iframe.src = bust(RIGHT_PANE_SRC);
  }
  // Single-shot retry: if the right-pane iframe hasn't fired `load`
  // within 1.5s, the project-managed server (from .xmlui-desktop.json)
  // is probably still starting up — connection is stuck. Bust and try
  // once more. Iframes fire `load` even for error pages, so this
  // specifically catches the "still connecting" state. `error` is not
  // reliable for iframes; we don't bother listening for it.
  let loaded = false;
  iframe.addEventListener("load", () => { loaded = true; });
  iframe.src = RIGHT_PANE_SRC;
  setTimeout(() => {
    if (!loaded) iframe.src = bust(RIGHT_PANE_SRC);
  }, 1500);
  if (tools) tools.src = TOOLS_PANE_SRC;
  document
    .getElementById("reload-right")
    ?.addEventListener("click", reloadAll);
  document
    .getElementById("open-devtools")
    ?.addEventListener("click", () => {
      invoke("open_devtools").catch((e) => console.error("open_devtools", e));
    });
  listen("right-pane-reload", reloadRightPaneOnly);
  listen("tools-pane-reload", reloadAll);
  // Push Claude Code session JSONL changes to the tools iframe so Talk
  // can refetch its DataSource immediately — the menu state lives in a
  // window that's usually shorter than the poll interval.
  listen("talk-session-changed", () => {
    const tools = document.getElementById("tools-pane");
    if (tools && tools.contentWindow) {
      tools.contentWindow.postMessage({ type: "talk-session-changed" }, "*");
    }
  });
})();

// Click-to-toggle voice. The toolbar 🎤 button toggles its own recording;
// iframes (Workspace, etc.) drive the same recorder via voice-start/voice-stop
// messages. Auto-starts whisper-server on first record click.
(() => {
  const WHISPER_HOST = "http://127.0.0.1:18080";
  const WHISPER_URL = WHISPER_HOST + "/inference";
  const MODEL_PATH = "~/.local/share/whisper-models/ggml-small.en.bin";
  const READY_TIMEOUT_MS = 15000;
  const READY_POLL_MS = 300;

  const toolbarBtn = document.getElementById("voice-toggle");
  if (!toolbarBtn) return;

  // Structured voice-pipeline logging via the existing log_from_right_pane
  // command, so every stage shows up in cargo run stderr tagged with the
  // session's requestId and a timestamp. See the voice-instrumentation
  // worklist item for the rationale.
  const voiceLog = (stage, payload) => {
    try {
      invoke(
        "log_from_right_pane",
        Object.assign(
          { kind: "voice-host", stage, at: new Date().toISOString() },
          payload || {},
        ),
      ).catch(() => {});
    } catch (e) {}
  };
  // Last few transcripts (keyed by requestId) so we can detect when whisper
  // returns a byte-for-byte duplicate of a recent response — the most
  // suspect failure mode behind the "stuck on 'push it'" symptom.
  const recentTranscripts = [];
  const RECENT_TRANSCRIPT_WINDOW_MS = 60_000;

  let mediaRecorder = null;
  let audioChunks = [];
  let stream = null;
  // active === null         → idle
  // active === "toolbar"    → toolbar mic; transcript → pty_write
  // active === { source, requestId } → iframe round-trip; transcript → postMessage
  let active = null;
  // Synthetic requestId for toolbar sessions — keeps log entries correlated
  // even though the toolbar path never receives an iframe-supplied id.
  let toolbarRequestId = null;
  const currentRequestId = () =>
    active === "toolbar"
      ? toolbarRequestId
      : active && typeof active === "object"
        ? active.requestId
        : null;

  const setToolbarState = (state) => {
    toolbarBtn.dataset.state = state;
    toolbarBtn.innerHTML =
      state === "recording"
        ? "&#x23F9; stop"
        : state === "starting"
          ? "&#x23F3; starting"
          : "&#x1F3A4; voice";
  };
  setToolbarState("idle");

  const ensureServerRunning = async () => {
    let status;
    try {
      status = await invoke("whisper_status");
    } catch (e) {
      console.error("whisper_status", e);
      return false;
    }
    if (status && status.running) return true;
    try {
      await invoke("whisper_start", { modelPath: MODEL_PATH });
    } catch (e) {
      console.error("whisper_start", e);
      return false;
    }
    const deadline = Date.now() + READY_TIMEOUT_MS;
    while (Date.now() < deadline) {
      try {
        const res = await fetch(WHISPER_HOST + "/", { method: "GET" });
        if (res.ok) return true;
      } catch (_) {}
      await new Promise((r) => setTimeout(r, READY_POLL_MS));
    }
    return false;
  };

  const stopStream = () => {
    if (stream) {
      stream.getTracks().forEach((t) => t.stop());
      stream = null;
    }
  };

  const deliverTranscript = (transcript) => {
    const reqId = currentRequestId();
    const text = String(transcript || "");
    voiceLog("deliverTranscript", {
      requestId: reqId,
      target:
        active === "toolbar"
          ? "toolbar"
          : active && typeof active === "object"
            ? "iframe"
            : "none",
      transcriptLength: text.length,
      transcriptPreview: text.slice(0, 80),
    });
    if (active && typeof active === "object" && active.source) {
      // iframe round-trip
      try {
        active.source.postMessage(
          {
            type: "voice-into-result",
            requestId: active.requestId,
            transcript: text,
          },
          "*",
        );
      } catch (e) {
        console.error("postMessage voice-into-result", e);
        voiceLog("deliverTranscript-postMessage-error", {
          requestId: reqId,
          error: String(e),
        });
      }
    } else if (active === "toolbar" && text) {
      // Prefix with "voice: " so the receiving agent (typically Claude Code)
      // can distinguish dictated content from typed input — see the
      // verbal-vs-structured guardrail in app/__shell/conventions.md.
      invoke("pty_write", {
        data: "\x1b[200~voice: " + text + "\x1b[201~\r",
      }).catch((e) => {
        console.error("pty_write voice", e);
        voiceLog("deliverTranscript-pty-error", {
          requestId: reqId,
          error: String(e),
        });
      });
    }
    active = null;
    toolbarRequestId = null;
    if (toolbarBtn.dataset.state !== "idle") setToolbarState("idle");
  };

  const startRecording = async (target) => {
    const incomingId =
      target === "toolbar"
        ? "toolbar-" + Date.now() + "-" + Math.random().toString(36).slice(2)
        : target && target.requestId;
    voiceLog("startRecording-enter", {
      requestId: incomingId,
      target: target === "toolbar" ? "toolbar" : "iframe",
      activeWas: active === null ? null : active === "toolbar" ? "toolbar" : "iframe",
    });
    if (active) {
      voiceLog("startRecording-rejected-busy", {
        requestId: incomingId,
        activeRequestId: currentRequestId(),
      });
      // Already busy: tell the new requester nothing came of it.
      if (target && typeof target === "object" && target.source) {
        try {
          target.source.postMessage(
            {
              type: "voice-into-result",
              requestId: target.requestId,
              transcript: "",
            },
            "*",
          );
        } catch (_) {}
      }
      return;
    }
    active = target;
    const isToolbar = target === "toolbar";
    if (isToolbar) toolbarRequestId = incomingId;
    if (isToolbar) setToolbarState("starting");
    const ready = await ensureServerRunning();
    if (!ready) {
      console.error("whisper-server did not become ready");
      const t = active;
      active = null;
      if (isToolbar) setToolbarState("idle");
      if (t && typeof t === "object" && t.source) {
        try {
          t.source.postMessage(
            { type: "voice-into-result", requestId: t.requestId, transcript: "" },
            "*",
          );
        } catch (_) {}
      }
      return;
    }
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (e) {
      console.error("getUserMedia", e);
      const t = active;
      active = null;
      if (isToolbar) setToolbarState("idle");
      if (t && typeof t === "object" && t.source) {
        try {
          t.source.postMessage(
            { type: "voice-into-result", requestId: t.requestId, transcript: "" },
            "*",
          );
        } catch (_) {}
      }
      return;
    }
    audioChunks = [];
    mediaRecorder = new MediaRecorder(stream);
    mediaRecorder.ondataavailable = (e) => {
      if (e.data && e.data.size > 0) audioChunks.push(e.data);
    };
    mediaRecorder.onstop = async () => {
      const reqId = currentRequestId();
      stopStream();
      const blob = new Blob(audioChunks, { type: "audio/webm" });
      audioChunks = [];
      voiceLog("mediaRecorder-onstop", {
        requestId: reqId,
        blobSize: blob.size,
      });
      if (blob.size === 0) {
        voiceLog("transcribe-skipped-empty-blob", { requestId: reqId });
        deliverTranscript("");
        return;
      }
      let transcript = "";
      let httpStatus = null;
      try {
        const formData = new FormData();
        formData.append("file", blob, "recording.webm");
        formData.append("response_format", "json");
        const reqStart = Date.now();
        voiceLog("whisper-request", {
          requestId: reqId,
          blobSize: blob.size,
        });
        const res = await fetch(WHISPER_URL, { method: "POST", body: formData });
        httpStatus = res.status;
        if (res.ok) {
          const data = await res.json();
          transcript = (data.text || "").trim();
        } else {
          console.error("transcribe HTTP", res.status, res.statusText);
        }
        voiceLog("whisper-response", {
          requestId: reqId,
          httpStatus: httpStatus,
          elapsedMs: Date.now() - reqStart,
          transcriptLength: transcript.length,
          transcriptPreview: transcript.slice(0, 80),
        });
      } catch (e) {
        console.error("transcribe", e);
        voiceLog("whisper-error", { requestId: reqId, error: String(e) });
      }
      // Stale-duplicate detection: warn if whisper returned exactly the same
      // text as a recent prior response. This is the prime suspect behind
      // the "different utterance, same wrong transcript" bug.
      if (transcript) {
        const now = Date.now();
        for (let i = recentTranscripts.length - 1; i >= 0; i--) {
          const r = recentTranscripts[i];
          if (now - r.at > RECENT_TRANSCRIPT_WINDOW_MS) {
            recentTranscripts.splice(0, i + 1);
            break;
          }
          if (r.text === transcript) {
            voiceLog("whisper-duplicate-transcript", {
              requestId: reqId,
              previousRequestId: r.requestId,
              ageMs: now - r.at,
              transcriptPreview: transcript.slice(0, 80),
            });
            break;
          }
        }
        recentTranscripts.push({ requestId: reqId, text: transcript, at: now });
      }
      deliverTranscript(transcript);
    };
    mediaRecorder.start();
    voiceLog("mediaRecorder-start", { requestId: incomingId });
    if (isToolbar) setToolbarState("recording");
  };

  const stopRecording = () => {
    voiceLog("stopRecording", {
      requestId: currentRequestId(),
      mediaRecorderState: mediaRecorder ? mediaRecorder.state : "null",
    });
    if (mediaRecorder && mediaRecorder.state === "recording") {
      mediaRecorder.stop();
    }
  };

  toolbarBtn.addEventListener("click", () => {
    const state = toolbarBtn.dataset.state;
    if (state === "recording") {
      stopRecording();
    } else if (state === "idle") {
      startRecording("toolbar");
    }
    // ignore clicks while "starting"
  });

  window.addEventListener("keydown", (ev) => {
    if (ev.key === "d" && ev.shiftKey && (ev.metaKey || ev.ctrlKey)) {
      ev.preventDefault();
      toolbarBtn.click();
    }
  });

  // iframe-driven flow: voice-start begins recording on the iframe's behalf;
  // voice-stop stops the (single in-flight) recording.
  window.addEventListener("message", (ev) => {
    const d = ev.data;
    if (!d || d.type !== "right-pane") return;
    if (d.kind === "voice-start") {
      voiceLog("iframe-voice-start", { requestId: d.requestId });
      startRecording({ source: ev.source, requestId: d.requestId });
    } else if (d.kind === "voice-stop") {
      voiceLog("iframe-voice-stop", { requestId: d.requestId });
      stopRecording();
    }
  });
})();
