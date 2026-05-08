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

const setTerminalFontSize = (n) => {
  const size = clampFontSize(n);
  term.options.fontSize = size;
  fitAddon.fit();
  try {
    localStorage.setItem(TERM_FONT_KEY, String(size));
  } catch {}
};

term.attachCustomKeyEventHandler((ev) => {
  if (ev.type !== "keydown" || !ev.metaKey) return true;
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
      requestAnimationFrame(() => fitAddon.fit());
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
  const TOOLS_PANE_SRC = base + "/__tools/index.html";
  const tools = document.getElementById("tools-pane");
  // reloadAll: reload BOTH iframes. Used by the manual "reload xmlui app"
  // toolbar button and by the "tools-pane-reload" watcher event (drawer
  // code changed, both panes may consume it).
  function reloadAll() {
    iframe.src = RIGHT_PANE_SRC + "?t=" + Date.now();
    if (tools) tools.src = TOOLS_PANE_SRC + "?t=" + Date.now();
  }
  // reloadRightPaneOnly: reload only the right pane. Used by the
  // "right-pane-reload" watcher event for user-project file changes.
  // The drawer is poll-driven so it does NOT need to reload here, and
  // keeping it stable avoids postMessage-vs-iframe-rebuild races on
  // Approve/Drop clicks while the agent is writing files.
  function reloadRightPaneOnly() {
    iframe.src = RIGHT_PANE_SRC + "?t=" + Date.now();
  }
  iframe.src = RIGHT_PANE_SRC;
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
})();

// Click-to-toggle voice. The toolbar 🎤 button toggles its own recording;
// iframes (Workspace, etc.) drive the same recorder via voice-start/voice-stop
// messages. Auto-starts whisper-server on first record click.
(() => {
  const WHISPER_HOST = "http://127.0.0.1:8080";
  const WHISPER_URL = WHISPER_HOST + "/inference";
  const MODEL_PATH = "~/.local/share/whisper-models/ggml-small.en.bin";
  const READY_TIMEOUT_MS = 15000;
  const READY_POLL_MS = 300;

  const toolbarBtn = document.getElementById("voice-toggle");
  if (!toolbarBtn) return;

  let mediaRecorder = null;
  let audioChunks = [];
  let stream = null;
  // active === null         → idle
  // active === "toolbar"    → toolbar mic; transcript → pty_write
  // active === { source, requestId } → iframe round-trip; transcript → postMessage
  let active = null;

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
    if (active && typeof active === "object" && active.source) {
      // iframe round-trip
      try {
        active.source.postMessage(
          {
            type: "voice-into-result",
            requestId: active.requestId,
            transcript: String(transcript || ""),
          },
          "*",
        );
      } catch (e) {
        console.error("postMessage voice-into-result", e);
      }
    } else if (active === "toolbar" && transcript) {
      // Prefix with "voice: " so the receiving agent (typically Claude Code)
      // can distinguish dictated content from typed input — see the
      // verbal-vs-structured guardrail in app/__shell/conventions.md.
      invoke("pty_write", {
        data: "\x1b[200~voice: " + transcript + "\x1b[201~\r",
      }).catch((e) => console.error("pty_write voice", e));
    }
    active = null;
    if (toolbarBtn.dataset.state !== "idle") setToolbarState("idle");
  };

  const startRecording = async (target) => {
    if (active) {
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
      stopStream();
      const blob = new Blob(audioChunks, { type: "audio/webm" });
      audioChunks = [];
      if (blob.size === 0) {
        deliverTranscript("");
        return;
      }
      let transcript = "";
      try {
        const formData = new FormData();
        formData.append("file", blob, "recording.webm");
        formData.append("response_format", "json");
        const res = await fetch(WHISPER_URL, { method: "POST", body: formData });
        if (res.ok) {
          const data = await res.json();
          transcript = (data.text || "").trim();
        } else {
          console.error("transcribe HTTP", res.status, res.statusText);
        }
      } catch (e) {
        console.error("transcribe", e);
      }
      deliverTranscript(transcript);
    };
    mediaRecorder.start();
    if (isToolbar) setToolbarState("recording");
  };

  const stopRecording = () => {
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
      startRecording({ source: ev.source, requestId: d.requestId });
    } else if (d.kind === "voice-stop") {
      stopRecording();
    }
  });
})();
