// Tauri 2 exposes the API on window.__TAURI__ when withGlobalTauri is true.
// https://v2.tauri.app/reference/javascript/api/
const { invoke, Channel } = window.__TAURI__.core;

invoke("log_from_right_pane", {
  payload: { kind: "main.js-loaded", at: new Date().toISOString() },
}).catch(() => {});

// ResizeObserver flood detector (diagnostic, #150 startup unresponsiveness).
// Parent-window twin of the iframe detector in helpers.js — catches the
// terminal fitAddon/webgl observers and shell-button layout here, while the
// iframe copy catches XMLUI Splitters. Wraps the constructor (class extends so
// observe/disconnect/instanceof keep working) and logs to bram-trace, once per
// second while the global fire rate exceeds a flood threshold, WHICH element
// is looping. Installed before the Terminal + observeTerminalSize() below.
// Remove once the flood source is identified.
(function installResizeObserverFloodDetector() {
  const Native = window.ResizeObserver;
  if (!Native || Native.__bramFloodWrapped) return;
  const FLOOD_PER_SEC = 50;
  let total = 0;
  let counts = Object.create(null);
  const describe = (el) => {
    try {
      if (!el || el.nodeType !== 1) return String(el);
      const id = el.id ? "#" + el.id : "";
      const cls = typeof el.className === "string" && el.className.trim()
        ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".")
        : "";
      return el.tagName.toLowerCase() + id + cls;
    } catch (e) {
      return "?";
    }
  };
  const Wrapped = class extends Native {
    constructor(cb) {
      super(function (entries, observer) {
        total += entries.length || 1;
        for (let i = 0; i < entries.length; i++) {
          const k = describe(entries[i] && entries[i].target);
          counts[k] = (counts[k] || 0) + 1;
        }
        return cb.call(this, entries, observer);
      });
    }
  };
  Wrapped.__bramFloodWrapped = true;
  window.ResizeObserver = Wrapped;
  setInterval(() => {
    const t = total;
    total = 0;
    const snap = counts;
    counts = Object.create(null);
    if (t < FLOOD_PER_SEC) return;
    const top = Object.keys(snap)
      .map((k) => [k, snap[k]])
      .sort((a, b) => b[1] - a[1])
      .slice(0, 6)
      .map((p) => p[0] + "=" + p[1]);
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "resizeobserver-flood",
        context: "parent",
        firesPerSec: t,
        top,
      },
    }).catch(() => {});
  }, 1000);
})();

// Parent main-thread liveness watchdog (xterm-wedge-liveness-
// instrumentation). xterm renders on THIS thread; when it freezes the
// user feels a wedged terminal, but until the 2026-07-07 codex-esc
// wedge hunt nothing measured it — freezes were inferred by
// elimination from iframe/route logs. A 250 ms heartbeat notes when a
// beat lands late by >500 ms and logs the observed gap. One line per
// stall, logged at recovery (a frozen thread cannot log mid-stall):
// timers coalesce while blocked, so the late beat's gap IS the stall
// length. Stall lines bracketed by a slow named host operation
// implicate it; stall absence during a felt wedge relocates the
// problem below the webview (PTY / process level).
(function installXtermLivenessWatchdog() {
  const TICK_MS = 250;
  const STALL_MS = 500;
  let last = Date.now();
  setInterval(() => {
    const now = Date.now();
    const gap = now - last - TICK_MS;
    last = now;
    if (gap < STALL_MS) return;
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "xterm-liveness",
        context: "parent",
        op: "stall",
        gap_ms: gap,
        at: new Date().toISOString(),
      },
    }).catch(() => {});
  }, TICK_MS);
})();

// Input-latency probe (diagnostic, #150). Parent-window twin of the iframe
// probe in helpers.js — measures responsiveness of the terminal + shell
// chrome (e.g. the #reload-right button). Capture-phase pointerdown/keydown
// stamp a time; the next frame measures how long the main thread took to come
// back. hadFocus distinguishes saturation from a post-reload focus artifact.
// Remove once the #150 responsiveness cause is identified.
(function installInputLatencyProbe() {
  if (window.__bramInputLatencyProbe) return;
  window.__bramInputLatencyProbe = true;
  const THRESHOLD_MS = 200;
  let lastLog = 0;
  const describe = (el) => {
    try {
      if (!el || el.nodeType !== 1) return String(el);
      const id = el.id ? "#" + el.id : "";
      const cls = typeof el.className === "string" && el.className.trim()
        ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".")
        : "";
      return el.tagName.toLowerCase() + id + cls;
    } catch (e) {
      return "?";
    }
  };
  const onInput = (ev) => {
    const t0 = performance.now();
    const type = ev.type;
    let hadFocus = false;
    try { hadFocus = document.hasFocus(); } catch (e) {}
    const tgt = describe(ev.target);
    requestAnimationFrame(() => {
      const dt = performance.now() - t0;
      if (dt < THRESHOLD_MS) return;
      if (t0 - lastLog < THRESHOLD_MS) return;
      lastLog = t0;
      invoke("log_from_right_pane", {
        payload: {
          kind: "iframe-trace",
          subkind: "input-latency",
          context: "parent",
          event: type,
          latencyMs: Math.round(dt),
          hadFocus,
          target: tgt,
        },
      }).catch(() => {});
    });
  };
  document.addEventListener("pointerdown", onInput, true);
  document.addEventListener("keydown", onInput, true);
})();

const TERM_FONT_KEY = "bram.terminal.fontSize";
const LEGACY_TERM_FONT_KEY = "xmlui-desktop.terminal.fontSize";
const TERM_FONT_MIN = 8;
const TERM_FONT_MAX = 32;
const TERM_FONT_DEFAULT = 13;

const clampFontSize = (n) =>
  Math.max(TERM_FONT_MIN, Math.min(TERM_FONT_MAX, Math.round(Number(n) || 0)));

const readSavedFontSize = () => {
  try {
    const raw = parseInt(
      localStorage.getItem(TERM_FONT_KEY) ??
        localStorage.getItem(LEGACY_TERM_FONT_KEY) ??
        "",
      10,
    );
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
const isWindows = navigator.userAgent.toLowerCase().includes("windows");

const container = document.getElementById("terminal");
term.open(container);
window.submitTerminalEnter = function () {
  try {
    term.focus();
    invoke("pty_write", { data: "\r" }).catch((e) =>
      console.error("submitTerminalEnter pty_write", e),
    );
  } catch (e) {
    console.error("submitTerminalEnter", e);
  }
};

window.submitTerminalTurn = function (text) {
  try {
    term.focus();
    invoke("pty_write", { data: String(text) })
      .then(() => invoke("pty_write", { data: "\r" }))
      .catch((e) => console.error("submitTerminalTurn pty_write", e));
  } catch (e) {
    console.error("submitTerminalTurn", e);
  }
};

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
  if (!resizing) {
    requestAnimationFrame(() => {
      restorePendingViewport();
      requestAnimationFrame(() => restorePendingViewport());
    });
  }
};

const scheduleStartupTerminalFit = () => {
  const run = () => {
    scheduleTerminalFit({ preserveViewport: false });
    if (isWindows) {
      setTimeout(() => scheduleTerminalFit({ preserveViewport: false }), 150);
      setTimeout(() => scheduleTerminalFit({ preserveViewport: false }), 500);
    }
  };
  const afterReady = () => {
    requestAnimationFrame(() => {
      requestAnimationFrame(run);
    });
  };
  const fontsReady = document.fonts?.ready;
  if (fontsReady && typeof fontsReady.then === "function") {
    fontsReady.then(afterReady, afterReady);
  } else {
    afterReady();
  }
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

let terminalResizeObserver = null;
let terminalResizeObserverTimer = null;
let lastObservedTerminalSize = null;
const observeTerminalSize = () => {
  if (!window.ResizeObserver || !container) return;
  terminalResizeObserver = new ResizeObserver((entries) => {
    const entry = entries[0];
    if (!entry) return;
    const { width, height } = entry.contentRect;
    if (width <= 0 || height <= 0) return;
    const size = `${Math.round(width)}x${Math.round(height)}`;
    if (size === lastObservedTerminalSize) return;
    lastObservedTerminalSize = size;
    clearTimeout(terminalResizeObserverTimer);
    terminalResizeObserverTimer = setTimeout(() => {
      scheduleTerminalFit({ preserveViewport: false });
    }, 0);
  });
  terminalResizeObserver.observe(container);
};

observeTerminalSize();

let resizing = false;
let resizingRestoreTimer = null;
window.addEventListener("resize", () => {
  resizing = true;
  clearTimeout(resizingRestoreTimer);
  resizingRestoreTimer = setTimeout(() => {
    resizing = false;
    requestAnimationFrame(() => {
      restorePendingViewport();
      requestAnimationFrame(() => restorePendingViewport());
    });
  }, 50);
  scheduleTerminalFit();
});

const setTerminalFontSize = (n) => {
  const size = clampFontSize(n);
  term.options.fontSize = size;
  runTerminalFit();
  try {
    localStorage.setItem(TERM_FONT_KEY, String(size));
  } catch {}
};

const isMac = /Mac|iPhone|iPod|iPad/i.test(navigator.userAgent);

const isTerminalEventTarget = (target) => {
  if (!target || !container) return false;
  return target === container || container.contains(target);
};

const writeTerminalPaste = (text, source) => {
  if (!text) return;
  invoke("pty_write", {
    data: "\x1b[200~" + text + "\x1b[201~",
  }).catch((e) => console.error("pty_write paste", source, e));
};

const copyTerminalSelection = (text) => {
  if (!text) return;
  if (navigator.clipboard?.writeText) {
    navigator.clipboard.writeText(text).catch((e) => {
      console.error("clipboard write", e);
      copyTerminalSelectionFallback(text);
    });
    return;
  }
  copyTerminalSelectionFallback(text);
};

const copyTerminalSelectionFallback = (text) => {
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "readonly");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  try {
    document.execCommand("copy");
  } catch (e) {
    console.error("clipboard copy fallback", e);
  } finally {
    textarea.remove();
    term.focus();
  }
};

const readClipboardAndPasteToTerminal = (source) => {
  if (!navigator.clipboard?.readText) return false;
  navigator.clipboard
    .readText()
    .then((text) => writeTerminalPaste(text, source))
    .catch((e) => console.error("clipboard read", source, e));
  return true;
};

container.addEventListener("paste", (ev) => {
  if (isMac || !isTerminalEventTarget(ev.target)) return;
  const text = ev.clipboardData?.getData("text/plain");
  if (!text) return;
  ev.preventDefault();
  writeTerminalPaste(text, "paste-event");
});

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
  //   - Plain Ctrl+V and Ctrl+Shift+V paste clipboard text via the
  //     bracketed-paste path. Ctrl+V is the Windows expectation; Ctrl+Shift+V
  //     remains supported for Linux terminal muscle memory.
  //     preventDefault stops the WebView's native paste event from also
  //     firing — without it, xterm.js's textarea paste listener would
  //     write the clipboard a second time.
  if (!isMac && ev.ctrlKey && !ev.shiftKey && (ev.key === "c" || ev.key === "C")) {
    const sel = term.getSelection();
    if (sel) {
      ev.preventDefault();
      copyTerminalSelection(sel);
      return false;
    }
    // No selection: let xterm.js send ^C → SIGINT.
  }
  if (!isMac && ev.ctrlKey && (ev.key === "V" || ev.key === "v")) {
    const handled = readClipboardAndPasteToTerminal(
      ev.shiftKey ? "ctrl-shift-v" : "ctrl-v",
    );
    if (!handled) return true;
    ev.preventDefault();
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
  const TERMINAL_HIDDEN_KEY = "bram.terminal.hidden";
  const LEGACY_TERMINAL_HIDDEN_KEY = "xmlui-desktop.terminal.hidden";
  const btn = document.getElementById("toggle-terminal");
  if (!btn) return;
  let dismissedSuspiciousEpisode = "";

  const postTerminalVisibility = (source) => {
    const tools = document.getElementById("tools-pane");
    if (!tools || !tools.contentWindow) return;
    tools.contentWindow.postMessage(
      {
        type: "bram-terminal-visibility",
        hidden: document.body.classList.contains("terminal-hidden"),
        dismissedEpisode: dismissedSuspiciousEpisode,
        source: source || "parent",
        at: Date.now(),
      },
      "*",
    );
  };

  const postSuspiciousSilenceSelfTest = () => {
    const tools = document.getElementById("tools-pane");
    if (!tools || !tools.contentWindow) return;
    const at = Date.now();
    const episodeId = `self-test:${at}`;
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "terminal-suspicious-silence",
        context: "parent",
        op: "self-test-dispatched",
        episode: episodeId,
      },
    }).catch(() => {});
    tools.contentWindow.postMessage(
      {
        type: "bram-terminal-suspicious-silence-test",
        episodeId,
        silenceMs: 3000,
        thresholdMs: 3000,
        gapP95Ms: 0,
        gapsN: 0,
        at,
      },
      "*",
    );
  };

  // A deliberate, inert self-test for the compaction-in-progress banner
  // (compaction-in-progress-banner). Modeled on
  // postSuspiciousSilenceSelfTest above: posts a synthetic
  // "bram-compaction-test" active:true straight to the tools iframe (the
  // bridge in helpers.js treats it exactly like a real `compaction-changed`
  // host payload), then after a short delay posts active:false so the
  // banner's auto-clear is exercisable too, without waiting for a real
  // compaction or touching the live turn / host detector.
  const postCompactionSelfTest = () => {
    const tools = document.getElementById("tools-pane");
    if (!tools || !tools.contentWindow) return;
    const at = Date.now();
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "compaction",
        context: "parent",
        op: "self-test-dispatched",
        atMs: at,
      },
    }).catch(() => {});
    tools.contentWindow.postMessage(
      { type: "bram-compaction-test", active: true, provider: "self-test", atMs: at },
      "*",
    );
    setTimeout(() => {
      const toolsNow = document.getElementById("tools-pane");
      if (!toolsNow || !toolsNow.contentWindow) return;
      toolsNow.contentWindow.postMessage(
        { type: "bram-compaction-test", active: false, provider: "self-test", atMs: at },
        "*",
      );
    }, 4000);
  };

  const apply = (hidden, source, persist) => {
    const wasHidden = document.body.classList.contains("terminal-hidden");
    document.body.classList.toggle("terminal-hidden", hidden);
    if (!hidden) {
      // Re-measure xterm.js once the layout settles.
      scheduleTerminalFit();
    }
    if (persist) {
      try {
        localStorage.setItem(TERMINAL_HIDDEN_KEY, hidden ? "1" : "0");
      } catch {}
    }
    postTerminalVisibility(source);
    if (wasHidden !== hidden) {
      invoke("log_from_right_pane", {
        payload: {
          kind: "iframe-trace",
          subkind: "terminal-visibility",
          context: "parent",
          op: hidden ? "closed" : "opened",
          source: source || "parent",
        },
      }).catch(() => {});
    }
  };

  let initial = false;
  try {
    initial =
      (localStorage.getItem(TERMINAL_HIDDEN_KEY) ??
        localStorage.getItem(LEGACY_TERMINAL_HIDDEN_KEY)) === "1";
  } catch {}
  apply(initial, "startup", false);
  if (!initial) {
    scheduleStartupTerminalFit();
  }

  btn.addEventListener("click", (event) => {
    const hidden = !document.body.classList.contains("terminal-hidden");
    apply(hidden, "toolbar-toggle", true);
    // A deliberate, inert self-test for the closed-terminal recovery path.
    // It exercises the parent/tools bridge and real footer action without
    // modifying the live turn or weakening the host detector.
    if (hidden && event.shiftKey) {
      postSuspiciousSilenceSelfTest();
    }
    // Same idea, distinct modifier, for the compaction-in-progress banner.
    if (hidden && event.altKey) {
      postCompactionSelfTest();
    }
  });

  window.addEventListener("message", (event) => {
    const data = event && event.data;
    if (!data || (
      data.type !== "bram-terminal-visibility-request" &&
      data.type !== "bram-open-terminal" &&
      data.type !== "bram-terminal-silence-dismissed"
    )) {
      return;
    }
    const tools = document.getElementById("tools-pane");
    if (!tools || !tools.contentWindow || event.source !== tools.contentWindow) {
      return;
    }
    if (data.type === "bram-terminal-visibility-request") {
      postTerminalVisibility("tools-request");
      return;
    }
    if (data.episodeId) dismissedSuspiciousEpisode = String(data.episodeId);
    if (data.type === "bram-terminal-silence-dismissed") {
      postTerminalVisibility("silence-dismissed");
      return;
    }
    apply(false, "suspicious-silence-button", true);
  });
})();

// Vertical splitter between terminal (left pane) and right column.
// Persists the chosen flexBasis pixel width to localStorage as
// `bram.splitter.left` and rehydrates it on startup. Pixels (not
// percentage) match how the drag handler writes flexBasis.
const LEFT_SPLITTER_KEY = "bram.splitter.left";
(() => {
  const left = document.querySelector(".pane-left");
  if (!left) return;
  const raw = localStorage.getItem(LEFT_SPLITTER_KEY);
  const px = parseFloat(raw);
  if (!isNaN(px) && px > 0) {
    left.style.flexBasis = px + "px";
  }
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

    let lastX = null;
    const onMove = (ev) => {
      const rect = split.getBoundingClientRect();
      let x = ev.clientX - rect.left;
      const max = rect.width - MIN_PX - splitter.offsetWidth;
      if (x < MIN_PX) x = MIN_PX;
      if (x > max) x = max;
      lastX = x;
      left.style.flexBasis = x + "px";
      scheduleTerminalFit();
    };
    const onUp = (ev) => {
      splitter.releasePointerCapture(ev.pointerId);
      splitter.classList.remove("dragging");
      document.body.classList.remove("splitter-dragging");
      splitter.removeEventListener("pointermove", onMove);
      splitter.removeEventListener("pointerup", onUp);
      if (lastX !== null && lastX > 0) {
        const val = String(Math.round(lastX));
        localStorage.setItem(LEFT_SPLITTER_KEY, val);
        console.log("[splitter-save]", LEFT_SPLITTER_KEY, val);
      }
      runTerminalFit();
    };
    splitter.addEventListener("pointermove", onMove);
    splitter.addEventListener("pointerup", onUp);
  });
})();

// Horizontal splitter resizes the tools drawer (only operative when drawer
// is open; the splitter is `display: none` when the .hidden class is set).
// Persists the chosen flexBasis percentage to localStorage as
// `bram.splitter.shell` and rehydrates it on startup.
const SHELL_SPLITTER_KEY = "bram.splitter.shell";
const TOOLS_ROUTE_KEY = "bram.tools.route";
const LEGACY_TOOLS_ROUTE_KEY = "xmlui-desktop.tools.route";
const TOOLS_ROUTE_POLL_MS = 500;

const logShellEvent = (payload) => {
  try {
    invoke("log_from_right_pane", { payload }).catch(() => {});
  } catch {}
};

const tracePaneReload = (stage, fields = {}) => {
  logShellEvent({
    kind: "iframe-trace",
    subkind: "pane-reload",
    at: new Date().toISOString(),
    stage,
    ...fields,
  });
};

(() => {
  const tools = document.getElementById("tools-pane");
  if (!tools) return;
  const raw = localStorage.getItem(SHELL_SPLITTER_KEY);
  const pct = parseFloat(raw);
  if (!isNaN(pct) && pct > 0 && pct < 100) {
    tools.style.flexBasis = pct + "%";
  }
})();
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

    let lastPct = null;
    const onMove = (ev) => {
      const rect = column.getBoundingClientRect();
      // Drawer height = distance from pointer to bottom of column.
      let h = rect.bottom - ev.clientY;
      const max = rect.height - MIN_PX - hSplitter.offsetHeight;
      if (h < MIN_PX) h = MIN_PX;
      if (h > max) h = max;
      // Store as percentage of the column height so window resizes
      // preserve the user's chosen proportion. Absolute pixels (h +
      // "px") locked the drawer to a fixed height and left a gap or
      // overshoot when the Tauri window grew or shrank.
      lastPct = (h / rect.height) * 100;
      tools.style.flexBasis = lastPct + "%";
    };
    const onUp = (ev) => {
      hSplitter.releasePointerCapture(ev.pointerId);
      hSplitter.classList.remove("dragging");
      document.body.classList.remove("splitter-dragging");
      hSplitter.removeEventListener("pointermove", onMove);
      hSplitter.removeEventListener("pointerup", onUp);
      if (lastPct !== null && lastPct > 0 && lastPct < 100) {
        const val = String(Math.round(lastPct * 10) / 10);
        localStorage.setItem(SHELL_SPLITTER_KEY, val);
        console.log("[splitter-save]", SHELL_SPLITTER_KEY, val);
      }
    };
    hSplitter.addEventListener("pointermove", onMove);
    hSplitter.addEventListener("pointerup", onUp);
  });
})();

// The tools drawer is always visible. The old "agent tools" toolbar
// toggle persisted a hidden state in localStorage that survived every
// reset short of DevTools surgery and presented as a broken app
// (Andrew's 2026-08-07 blank-pane outage); the feature existed only to
// maximize an embedded target app, a minority case not worth durable
// invisible state. Clear any stale key so stuck installs self-heal.
try {
  localStorage.removeItem("bram.tools.hidden");
} catch {}

// PTY wiring: stdout from Rust arrives over a Channel; stdin goes via invoke.
// https://v2.tauri.app/develop/calling-frontend/#channels
const ptyChannel = new Channel();

let startupFitDone = false;

// ===== #210 Esc-residue characterization (observe-only) =====
// When Esc is pressed while a permission menu is live, capture three correlated
// snapshots so a problematic Esc self-diagnoses: the prompt region AT Esc, the
// raw PTY bytes the agent emits for the next ESC_CAPTURE_MS, and the same region
// after it SETTLES. Writes NOTHING to the terminal — it only observes, so the
// agent's native post-Esc behavior is unperturbed. The post-bytes capture is the
// decisive signal: bytes that repaint the region mean the residue is the agent's
// own redraw (not Bram's to clear); no bytes mean it's stale rows Bram rendered.
// Tunables: the observation window and the rows captured above the menu block.
const ESC_CAPTURE_MS = 750;
const ESC_CAPTURE_PAD = 4;
const ESC_CAPTURE_MAX_CHUNKS = 24;
let __escCaptureSeq = 0;
let __escCapture = null; // { id, startedAt, until, chunks } while observing

function __escBytesPreview(u8, max) {
  let out = "";
  for (let i = 0; i < u8.length && out.length < max; i++) {
    const b = u8[i];
    if (b === 0x1b) out += "\\x1b";
    else if (b === 0x0d) out += "\\r";
    else if (b === 0x0a) out += "\\n";
    else if (b === 0x09) out += "\\t";
    else if (b >= 0x20 && b < 0x7f) out += String.fromCharCode(b);
    else out += "\\x" + b.toString(16).padStart(2, "0");
  }
  return out.length >= max ? out + "…" : out;
}

// Snapshot the prompt region (menu block + ESC_CAPTURE_PAD rows above, through
// the end of the live tail where residue would sit) plus viewport context.
function __escRegionRows() {
  const rows = __gridReadLiveRows() || [];
  const menu = rows.length ? __gridDetectMenu(rows) : null;
  const buf = term.buffer && term.buffer.active;
  const blockTop = menu ? menu.blockTop : -1;
  const top =
    blockTop >= 0
      ? Math.max(0, blockTop - ESC_CAPTURE_PAD)
      : Math.max(0, rows.length - 40);
  return {
    rows: rows.slice(top).map((r) => r.text.replace(/\s+$/, "")),
    blockTop,
    meta: {
      liveRows: rows.length,
      termRows: term.rows || 0,
      bufLen: buf ? buf.length : 0,
      baseY: buf ? buf.baseY : 0,
      header:
        (__gridLastMenu && __gridLastMenu.header) || (menu && menu.header) || "",
      optionCount: menu
        ? menu.options.length
        : __gridLastMenu
          ? __gridLastMenu.options.length
          : 0,
    },
  };
}

function __escBeginCapture(kind, source) {
  const subkind = kind + "-capture";
  const id = kind + "-" + Date.now() + "-" + ++__escCaptureSeq;
  const startedAt = Date.now();
  const at = __escRegionRows();
  logShellEvent({
    kind: "iframe-trace",
    subkind: subkind,
    stage: "at-" + kind,
    escId: id,
    source: source || "unknown",
    menuPresent: __gridMenuPresent,
    firstOutputSeen: __gridFirstOutputSeen(),
    blockTop: at.blockTop,
    meta: at.meta,
    rows: at.rows,
    at: new Date().toISOString(),
  });
  __escCapture = { id, startedAt, until: startedAt + ESC_CAPTURE_MS, chunks: [], subkind: subkind };
  setTimeout(() => {
    const cap = __escCapture;
    if (!cap || cap.id !== id) return;
    __escCapture = null;
    logShellEvent({
      kind: "iframe-trace",
      subkind: cap.subkind,
      stage: "post-bytes",
      escId: id,
      windowMs: ESC_CAPTURE_MS,
      chunkCount: cap.chunks.length,
      totalBytes: cap.chunks.reduce((n, c) => n + c.len, 0),
      chunks: cap.chunks,
      at: new Date().toISOString(),
    });
    const settle = __escRegionRows();
    logShellEvent({
      kind: "iframe-trace",
      subkind: cap.subkind,
      stage: "settle",
      escId: id,
      changed: at.rows.join("\n") !== settle.rows.join("\n"),
      rows: settle.rows,
      at: new Date().toISOString(),
    });
  }, ESC_CAPTURE_MS);
}

// Frame provenance (causal-menu-staleness): the host prefixes every PTY
// channel frame with the cumulative output offset after the chunk (8
// bytes LE). __ptyParsedOffset advances only when xterm has PARSED that
// chunk (term.write completion callback), so a grid report stamped with
// it states exactly which output its frame was painted from. Host and
// parent ship in the same binary — no version skew on the framing.
let __ptyParsedOffset = 0;
ptyChannel.onmessage = (chunk) => {
  let bytes = new Uint8Array(chunk);
  let frameEndOffset = 0;
  if (bytes.length >= 8) {
    const dv = new DataView(bytes.buffer, bytes.byteOffset, 8);
    frameEndOffset = dv.getUint32(0, true) + dv.getUint32(4, true) * 4294967296;
    bytes = bytes.subarray(8);
  }
  if (
    __escCapture &&
    Date.now() < __escCapture.until &&
    __escCapture.chunks.length < ESC_CAPTURE_MAX_CHUNKS
  ) {
    __escCapture.chunks.push({
      dtMs: Date.now() - __escCapture.startedAt,
      len: bytes.length,
      text: __escBytesPreview(bytes, 400),
    });
  }
  term.write(bytes, () => {
    if (frameEndOffset > __ptyParsedOffset) __ptyParsedOffset = frameEndOffset;
  });
  if (!startupFitDone) {
    startupFitDone = true;
    // First PTY output means the shell has started and the WebView2 window
    // has finished its initial layout pass — safe to do a definitive fit.
    requestAnimationFrame(() => scheduleTerminalFit({ preserveViewport: false }));
  }
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

// ===== xterm-grid screen reader (branch xterm-grid-screen-read) =====
// Phase 1: read the CLEAN rendered screen from xterm.js's grid instead of
// the host's strip_ansi parse of the raw PTY stream. Detect a LIVE
// permission menu by shape near the cursor, parse its options + the
// command/prose above it, and shadow-log the structured result
// (`[iframe] subkind=xterm-grid-menu`) so we can compare it against the
// host's parse before any cut-over. Self-contained.
let __gridLastMenuKey = null;

// Live region only: the menu always sits at/near the cursor, never deep in
// scrollback. Reading the last ~60 rows avoids matching scrollback menus or
// menu-shaped text shown in a diff.
function __gridReadLiveRows() {
  const buf = term.buffer && term.buffer.active;
  if (!buf) return null;
  const len = buf.length;
  // Wide enough to hold the whole menu even when a TUI leaves stale lines
  // below it (Codex renders its menu well above the buffer bottom).
  const start = Math.max(0, len - 200);
  const rows = [];
  for (let i = start; i < len; i++) {
    const ln = buf.getLine(i);
    rows.push(
      ln
        ? { text: ln.translateToString(true), wrapped: ln.isWrapped }
        : { text: "", wrapped: false },
    );
  }
  return rows;
}

// A menu requires the full shape: a footer ("Esc to cancel"), a header
// ("Do you want to…" / "requires approval"), and >= 2 numbered options
// between them. Wrapped option labels (option 2 spilling onto the next row)
// are rejoined onto the prior option.
function __gridDetectMenu(rows) {
  // Shape-driven so it covers both providers:
  //  - Claude: "Do you want to proceed?" + numbered options + "Esc to cancel".
  //  - Codex:  "Would you like to run…" + numbered options ending in
  //    (y)/(p)/(esc); NO "Esc to cancel" footer.
  // A menu is a contiguous block of numbered options near the cursor, with a
  // cursor marker (❯ or >) on the selected one, plus a recognizable signal
  // (header / footer / Codex keystroke hint) to guard against numbered lists
  // in ordinary content.
  const opts = [];
  let inOption = false;
  for (let r = 0; r < rows.length; r++) {
    const t = rows[r].text;
    // \s* (not \s+) after the dot: grid stale-cell garbling can collapse
    // "2. Yes" to "2.Yes" with no space, which dropped the option and made
    // the whole menu undetectable (the Claude menu-miss bug).
    const m = t.match(/^\s*([❯›>])?\s*(\d)\.\s*(.+)$/);
    if (m) {
      opts.push({ n: Number(m[2]), label: m[3].trim(), selected: !!m[1], row: r, rowEnd: r });
      inOption = true;
    } else if (!t.trim()) {
      inOption = false;
    } else if (inOption) {
      if (/Esc to cancel|esc to cancel|Press enter to confirm/i.test(t))
        inOption = false;
      else {
        // wrap-aware option parser (grid-menu-join-word-wrapped-labels):
        // accept xterm soft wraps, full-width anchors, and Ink's hard
        // word-boundary wraps. Ink moves a long next token wholesale even
        // when the anchor row itself is well short of the edge; that is a
        // genuine continuation iff the first token would not have fit on
        // the anchor. A short anchor ("  3. No") plus a short repaint
        // fragment still fails every signal, preserving the guard against
        // the historical "Nommands" debris join (2026-07-24 04:55).
        const prev = opts[opts.length - 1];
        const anchor = rows[prev.rowEnd] ? rows[prev.rowEnd].text : "";
        const cols = (typeof term !== "undefined" && term && term.cols) || 80;
        const firstWord = t.trim().split(/\s+/, 1)[0] || "";
        const softWrapped = !!rows[r].wrapped;
        const filledAnchor = anchor.length >= cols - 2;
        const wordBoundaryWrapped =
          firstWord.length > 0 && anchor.length + 1 + firstWord.length >= cols - 2;
        if (softWrapped || filledAnchor || wordBoundaryWrapped) {
          // Ink also treats a slash as a legal hard-wrap boundary inside a
          // token (for example `/usr/` + `bin/stat`). Preserve that token;
          // ordinary word-boundary wraps still need the separating space.
          const separator = /[\\/]$/.test(anchor.trimEnd()) ? "" : " ";
          prev.label += separator + t.trim();
          prev.rowEnd = r;
        }
      }
    }
  }
  if (opts.length < 2) return null;
  // Candidate runs bottom-most first, validating EACH — not just the last.
  // The agent's own prose renders numbered markdown lists into the terminal
  // ("1. **Claims arrive without…**"); a stale such line below the live menu
  // used to become the sole trailing run, fail validation, and null the whole
  // detect while a valid menu sat above (2026-07-18 20:16 jq-prompt miss:
  // 90+ s displayed, zero reports). A poisoned fragment now costs one
  // iteration instead of the menu.
  const headerRe = /Do you want to|requires approval|Would you like to run|wants to/i;
  const starts = [];
  for (let i = 0; i < opts.length; i++) if (opts[i].n === 1) starts.push(i);
  for (let s = starts.length - 1; s >= 0; s--) {
    const start = starts[s];
    const menu = [opts[start]];
    for (let i = start + 1; i < opts.length; i++) {
      if (opts[i].n === menu.length + 1) menu.push(opts[i]);
      else break;
    }
    if (menu.length < 2) continue;

    const blockTop = menu[0].row;
    const blockBottom = menu[menu.length - 1].row;
    // Codex prompt bodies wrap according to terminal width. Eight physical
    // rows was not enough for a two-line reason plus wrapped options: the
    // recognizable header fell just outside the window, leaving the scene
    // formally unbounded. Search a wider prompt-local window and still take
    // the nearest matching header below.
    const aboveRows = rows.slice(Math.max(0, blockTop - 16), blockTop);
    const belowText = rows
      .slice(blockBottom + 1, blockBottom + 4)
      .map((r) => r.text)
      .join(" ");
    const labels = menu.map((o) => o.label).join(" ");
    const headerSignal = aboveRows.some((r) => headerRe.test(r.text));
    const footerSignal =
      /Esc to cancel|esc to cancel|Press enter to confirm/i.test(belowText);
    const codexSignal =
      /\((y|p|esc)\)/.test(labels) ||
      /tell Codex/.test(labels) ||
      /Yes, proceed/.test(labels) ||
      /Press enter to confirm/i.test(belowText);
    // KEY discriminator from the agent's OWN prose: a permission menu's first
    // option is always "Yes" (Claude: "Yes"; Codex: "Yes, proceed (y)"). Prose
    // can contain numbered lists + menu-ish words ((esc) / Esc to cancel / tell
    // Codex) and otherwise false-trigger here, rendering a bogus menu.
    // Requiring option 1 == "Yes…" excludes prose while accepting both
    // providers.
    const isYesMenu = /^\s*Yes\b/i.test(menu[0].label);
    // grid-detect-allow-deny-dialog: the claude-in-chrome connection
    // dialog ("Claude in Chrome wants to create a browser window…" /
    // ❯ 1. Allow / 2. Deny (esc)) has no Yes-first option, so the gate
    // above dropped it — the grid never reported, claim reconciliation
    // never re-ran, and the pane froze on a hook-synthesized menu that
    // disagreed with the terminal (pa11 2026-07-21). Admit the Allow
    // family only with a rendered cursor (prose lists lack ❯); Deny's
    // "(esc)" hint satisfies codexSignal below.
    const isAllowMenu =
      !isYesMenu && /^\s*Allow\b/i.test(menu[0].label) && menu.some((o) => o.selected);
    // Picker family (detect-session-resume-picker): Claude Code's numbered
    // pickers — session resume ("1. Resume from summary…") and structural
    // twins — have no "Yes" first option, so the gate above would drop them
    // (Eric 2026-07-19 20:08: first send pasted into the undetected resume
    // picker, CR confirmed option 1, message stranded). Admit them only on
    // the strict picker footer — BOTH "Enter to confirm" AND "Esc to cancel"
    // in the rows directly below the block — plus a rendered cursor on one
    // option, so prose that merely quotes menu phrases still fails.
    const pickerSignal =
      /Enter to confirm/i.test(belowText) &&
      /Esc to cancel/i.test(belowText) &&
      menu.some((o) => o.selected);
    if (isYesMenu || isAllowMenu) {
      if (!(headerSignal || footerSignal || codexSignal)) continue;
    } else if (!pickerSignal) continue;

    const headerRow = aboveRows
      .slice()
      .reverse()
      .find((r) => headerRe.test(r.text));
    const headerIndex = headerRow ? rows.lastIndexOf(headerRow, blockTop - 1) : -1;
    let headerText = headerRow ? headerRow.text.trim() : "";
    if (!headerText && !isYesMenu && !isAllowMenu) {
      // Pickers have no "Do you want to…" header; carry the nearest
      // non-empty banner row so the pane shows why the picker is up
      // ("This session is 1d 2h old and 383.2k tokens.").
      const near = aboveRows
        .slice()
        .reverse()
        .find((r) => r.text.trim());
      headerText = near ? near.text.trim() : "";
    }
    // The host scores queued claims against this scene. When a recognizable
    // permission header is present, bound the scene to that prompt box;
    // parallel Codex status rows immediately above it ("Running …") name
    // other live claims and would otherwise make the longest command win.
    // Headerless pickers and unusually tall/scrolled boxes retain the broad
    // fallback used before this boundary was available.
    const promptTop = headerIndex >= 0 ? headerIndex : Math.max(0, blockTop - 12);
    const above = rows
      .slice(promptTop, blockTop)
      .map((r) => r.text.replace(/^[⏺⎿\s]+/, "").trimEnd())
      .filter((s) => s.trim());
    return {
      header: headerText,
      options: menu.map((o) => ({ n: o.n, label: o.label, selected: o.selected })),
      above,
      blockTop,
      picker: !isYesMenu && !isAllowMenu,
    };
  }
  return null;
}

// Extract the in-flight turn's assistant prose from the grid for the
// menu-stack-pty-inflight-prose feature: the explanatory text the agent
// printed just above the permission box. Conservative and FAIL-SILENT —
// returns "" on any ambiguity so we fall back to today's behavior rather than
// ever show wrong text (the JSONL delivers the exact prose moments later and
// replaces whatever we showed). Structure above the options (verified live):
//   ⏺ Tool(…) → ⎿ Waiting… → ──── border → header → command → description →
//   "Do you want to proceed?" → options
// The #206 manual-approval shape has no ⏺ Tool( bullet; its top is the border
// / ⎿ Waiting. The prose is the "⏺ <text>" bullet block directly above that
// anchor, where the bullet is NOT a tool call (⏺ Capitalized( ).
function __gridExtractInflightProse(rows, blockTop) {
  const toolBullet = /^\s*⏺\s+[A-Z][A-Za-z0-9]*\(/;
  const anyBullet = /^\s*⏺\s+/;
  const waiting = /⎿\s*Waiting/i;
  const border = /^[\s─-]{12,}$/;
  // 1. Anchor = top of the permission box: prefer the gated ⏺ Tool( bullet,
  // else the first (closest) border / ⎿ Waiting going up. Scan up to 80 rows
  // so a TALL command box (whose box top sits far above the options) is still
  // reachable — the old 18-row cap missed those and returned "" (no prose).
  // Break on the first border/waiting rather than the topmost, so widening
  // can't over-scan onto an earlier ──── / ⎿ in prior content.
  let anchor = -1;
  let fallback = -1;
  for (let i = blockTop - 1; i >= 0 && blockTop - i <= 80; i--) {
    const t = rows[i].text;
    if (toolBullet.test(t)) { anchor = i; break; }
    if ((waiting.test(t) || border.test(t)) && fallback < 0) fallback = i;
  }
  if (anchor < 0) anchor = fallback;
  if (anchor < 0) return "";
  // 2. The line directly above the box (skipping blanks) must be assistant
  // prose; if it's tool output (⎿), a user line (>), or a tool bullet, this
  // action had no preceding prose → fail silent.
  let j = anchor - 1;
  while (j >= 0 && !rows[j].text.trim()) j--;
  if (j < 0) return "";
  const top = rows[j].text;
  if (/^\s*⎿/.test(top) || /^\s*>/.test(top) || toolBullet.test(top)) return "";
  // 3. The block is prose ONLY if a ⏺ <non-tool> bullet is the first structural
  // marker above j. A ⎿ (tool output — e.g. an Edit diff), a ⏺ Tool( bullet, or
  // a > user line first means the gated call was preceded by tool output, not
  // prose → fail silent rather than leak that output as "prose".
  let start = -1;
  for (let k = j; k >= 0 && j - k <= 40; k--) {
    const tk = rows[k].text;
    if (/^\s*⎿/.test(tk) || /^\s*>/.test(tk) || toolBullet.test(tk)) return "";
    if (anyBullet.test(tk)) { start = k; break; }
  }
  if (start < 0) return "";
  // 4. Join start..j: strip the ⏺ glyph and cosmetic alignment indent, keep
  // line breaks (hard-wrapped continuations render fine as Markdown lines).
  const out = [];
  for (let r = start; r <= j; r++) {
    out.push(
      rows[r].text.replace(/^\s*⏺\s+/, "").replace(/\s+$/, "").replace(/^ {1,2}/, ""),
    );
  }
  let prose = out.join("\n").trim();
  if (prose.length > 1200) prose = prose.slice(0, 1200) + "…";
  return prose;
}

// #210 prove-step (observe-only): grid-based "has the agent produced output for
// the CURRENT turn yet?" Provider-aware — the prove step showed the two TUIs use
// different glyphs:
//   user prompt:   ❯ >  (Claude)   ›  (Codex)
//   agent bullet:  ⏺    (Claude)   •  (Codex) — but Codex reuses • for its
//                  "• Working (…)" indicator, which is NOT output.
// Anchor on the submitted user line, then look for a real output bullet below it.
// The trailing input box / placeholder (e.g. Codex "› Write tests for …") is
// excluded by requiring the anchored user line to have agent ACTIVITY below it
// (output, a status bullet, a spinner, or the "esc to interrupt" bar) — the input
// box is followed only by a status line. FAIL-SILENT (null) on error: this drives
// NO behavior, it only feeds traces to prove faithfulness before any Esc gate.
function __gridFirstOutputSeen() {
  try {
    const rows = __gridReadLiveRows();
    if (!rows || rows.length === 0) return null;
    const isUser = (t) => /^\s*[>❯›]\s+\S/.test(t);
    // Bullets that are status/working, not real output (Codex "• Working …",
    // Claude "⏺ Worked …" completion banners, gerund spinners).
    const isStatusBullet = (t) =>
      /^\s*[⏺•]\s+(Working|Worked|Vibing|Grooving|Crunch|Spelunk|Thinking)/i.test(t);
    const isOutput = (t) => /^\s*[⏺•]\s+\S/.test(t) && !isStatusBullet(t);
    // "Agent is active here" — distinguishes the submitted message (activity
    // below it) from the trailing input box / placeholder (status line only).
    const isActive = (t) =>
      isOutput(t) ||
      isStatusBullet(t) ||
      /esc to interrupt|·\s*thinking/i.test(t) ||
      /^\s*[✶✳✻✽✢✺✷◐◓◑◒]\s/.test(t);
    let lastUser = -1;
    for (let i = rows.length - 1; i >= 0; i--) {
      if (!isUser(rows[i].text)) continue;
      let activityBelow = false;
      for (let j = i + 1; j < rows.length; j++) {
        if (isActive(rows[j].text)) {
          activityBelow = true;
          break;
        }
      }
      if (activityBelow) {
        lastUser = i;
        break;
      }
    }
    if (lastUser < 0) return false;
    for (let i = lastUser + 1; i < rows.length; i++) {
      if (isOutput(rows[i].text)) return true;
    }
    return false;
  } catch (e) {
    return null;
  }
}

// Trace firstOutputSeen transitions so we can watch it flip false→true at first
// output and measure the pre-⏺ window. Per render frame, transition-only (quiet).
let __gridLastFOS = null;
function __gridTraceFirstOutputTransition() {
  try {
    const fos = __gridFirstOutputSeen();
    if (fos === __gridLastFOS) return;
    __gridLastFOS = fos;
    // (The #210 bounce-heuristic push to the host was deleted in delete-phase
    // tranche 3a; the FOS transition trace below stays as diagnostics.)
    logShellEvent({
      kind: "iframe-trace",
      subkind: "first-output-seen",
      value: fos,
      at: new Date().toISOString(),
    });
  } catch (e) {}
}

let __gridMenuPresent = false;
let __gridLastMenu = null;
let __gridMissKey = null;
function __gridMenuIdentity(menu) {
  const normalize = (value) =>
    String(value || "")
      .trim()
      .replace(/\s+/g, " ");
  // Parallel permission prompts intentionally reuse the same option family.
  // The command/context above the options is therefore part of menu identity;
  // omitting it freezes both the preview and capture-time parsedOffset on the
  // first prompt while the terminal advances through later prompts.
  const scene = [menu.header].concat(menu.above || []).map(normalize).join("|");
  const options = (menu.options || [])
    .map((option) => String(option.n) + normalize(option.label))
    .join("|");
  return scene + "|" + options;
}
function __gridShadowCheck() {
  try {
    const rows = __gridReadLiveRows();
    if (!rows) return;
    const menu = __gridDetectMenu(rows);
    if (!menu) {
      // Miss diagnostic: the grid clearly holds a menu (numbered options + a
      // Claude/Codex signal) but __gridDetectMenu rejected it. Log the raw
      // live rows once so we can see why (cursor glyph, option format) and
      // tune the patterns against the real rendered text.
      const txt = rows.map((r) => r.text).join("\n");
      if (
        /^\s*[>❯›]?\s*1\.\s*Yes\b/im.test(txt) &&
        /proceed|tell Codex|\(esc\)|Do you want to|Esc to cancel|Press enter to confirm/i.test(
          txt,
        )
      ) {
        const k = txt.slice(-220);
        if (k !== __gridMissKey) {
          __gridMissKey = k;
          invoke("log_from_right_pane", {
            payload: {
              kind: "iframe-trace",
              subkind: "xterm-grid-miss",
              rows: rows.filter((r) => r.text.trim()).map((r) => r.text),
            },
          }).catch(() => {});
        }
      }
      if (__gridMenuPresent) {
        __gridMenuPresent = false;
        __gridLastMenu = null;
        __gridLastMenuKey = null;
        invoke("report_grid_menu", { payload: { present: false } }).catch(
          () => {},
        );
      }
      return;
    }
    // A successful detect ends any miss episode: re-arm the miss diagnostic
    // so the NEXT miss can capture (the old never-reset key logged a
    // persistent miss at most once ever — the 20:16 episode logged nothing).
    __gridMissKey = null;
    const key = __gridMenuIdentity(menu);
    if (key === __gridLastMenuKey) return;
    __gridLastMenuKey = key;
    // menu-stack-pty-inflight-prose: extract the in-flight turn's prose from
    // the grid (fail-silent → "" when ambiguous) so the host can ship it with
    // the menu. Trace it so we can verify the extractor against real menus
    // before the iframe render consumes it.
    const inflightProse = __gridExtractInflightProse(rows, menu.blockTop);
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "inflight-prose",
        blockTop: menu.blockTop,
        chars: inflightProse.length,
        preview: inflightProse.slice(0, 240),
      },
    }).catch(() => {});
    __gridMenuPresent = true;
    __gridLastMenu = {
      header: menu.header,
      options: menu.options,
      above: menu.above,
      picker: !!menu.picker,
      prose: inflightProse,
      // Provenance travels WITH the snapshot: the keep-alive interval
      // below re-sends this capture-time offset, not a fresh one — the
      // frame is still the one parsed at capture. (Pre-fix, the 1 s
      // keep-alive re-reported unstamped and overwrote the stamped
      // first report in the host cell: 686 parsed_offset=none vs a
      // handful stamped in the 2026-07-06 census.)
      parsedOffset: __ptyParsedOffset,
    };
    // Authoritative: feed the clean structure to the host, which splices the
    // options into the emitted permission menu (or builds it when the host
    // missed). `above` carries the command/context lines for the Codex build
    // (Codex renders no header, so the confirmed command lives there).
    invoke("report_grid_menu", {
      payload: {
        present: true,
        header: menu.header,
        options: menu.options,
        above: menu.above,
        picker: !!menu.picker,
        prose: inflightProse,
        // Provenance stamp: the PTY output offset this frame was parsed
        // from (causal-menu-staleness).
        parsedOffset: __ptyParsedOffset,
      },
    }).catch(() => {});
    // Light shadow trace for comparison against the host's parse.
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "xterm-grid-menu",
        header: menu.header,
        options: menu.options,
        sceneRows: menu.above.length,
        sceneBounded: !!menu.header,
        parsedOffset: __ptyParsedOffset,
      },
    }).catch(() => {});
  } catch (e) {
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "xterm-grid-menu-error",
        error: String(e),
      },
    }).catch(() => {});
  }
}
term.onWriteParsed(() =>
  requestAnimationFrame(() => {
    __gridShadowCheck();
    __gridTraceFirstOutputTransition();
  }),
);
// While a menu is shown the PTY goes quiet, so onWriteParsed stops firing and
// the host's grid snapshot would age out. Re-report on a timer to keep it
// fresh (and to re-assert a host-missed menu the host can't see on its own).
setInterval(() => {
  if (__gridMenuPresent && __gridLastMenu) {
    invoke("report_grid_menu", {
      payload: {
        present: true,
        header: __gridLastMenu.header,
        options: __gridLastMenu.options,
        above: __gridLastMenu.above,
        picker: !!__gridLastMenu.picker,
        prose: __gridLastMenu.prose,
        parsedOffset: __gridLastMenu.parsedOffset,
      },
    }).catch(() => {});
  }
}, 1000);
// ===== xterm-grid status/banner reader (Phase C, shadow) =====
// Read the rotating status line and the end-of-turn banner from xterm's clean
// grid. strip_ansi loses digits here (e.g. "1m 22s" -> "1m 2s"); the grid has
// them intact. Shadow-log for now to compare against the host's parse.
let __gridBannerKey = null;
let __gridStatusVerbKey = null;
function __gridReadStatus() {
  try {
    const buf = term.buffer && term.buffer.active;
    if (!buf) return;
    const len = buf.length;
    const rows = [];
    for (let i = Math.max(0, len - 20); i < len; i++) {
      const ln = buf.getLine(i);
      if (ln) {
        const t = ln.translateToString(true);
        if (t.trim()) rows.push(t);
      }
    }
    // End-of-turn banner: "<glyph> <Verb> for <duration>" (capitalized verb
    // distinguishes it from the lowercase "thought for 7s" substate).
    for (let r = rows.length - 1; r >= 0; r--) {
      const m = rows[r].match(/\b([A-Z][a-zé]+(?:-[a-zé]+)*) for ((?:\d+m ?)?\d+s)\b/);
      if (m) {
        // Report every read while the banner is on screen so the host's cell
        // is fresh when the JSONL turn-end fires.
        invoke("report_grid_banner", {
          payload: { verb: m[1], elapsed: m[2] },
        }).catch(() => {});
        const banner = m[1] + " for " + m[2];
        if (banner !== __gridBannerKey) {
          __gridBannerKey = banner;
          invoke("log_from_right_pane", {
            payload: { kind: "iframe-trace", subkind: "grid-banner", banner },
          }).catch(() => {});
        }
        break;
      }
    }
    // Rotating status line: "<Verb>… (<elapsed> · <tokens> · <substate>)".
    // Parse to {verb, elapsed, substate} and report it structured so the host
    // can drive the agent-status row from the grid (clean, full-fidelity, no
    // sticky-cache flicker). Strip the leading rotating spinner glyph first.
    for (let r = rows.length - 1; r >= 0; r--) {
      const cm = rows[r].match(/\bWorking\s*\(([^)]*esc to interrupt[^)]*)\)/i);
      if (cm) {
        const segs = cm[1].split(/[·•]/).map((x) => x.trim());
        const elapsed =
          segs.find((x) => /\d+\s*[hms]\b/.test(x) && !/token/i.test(x)) || null;
        if (elapsed) {
          invoke("report_grid_status", {
            payload: { provider: "codex", verb: "Working", elapsed, substate: null },
          }).catch(() => {});
          const verbKey = "codex|Working|";
          if (verbKey !== __gridStatusVerbKey) {
            __gridStatusVerbKey = verbKey;
            invoke("log_from_right_pane", {
              payload: {
                kind: "iframe-trace",
                subkind: "grid-status",
                status: rows[r].trim(),
              },
            }).catch(() => {});
          }
        }
        break;
      }
      // Core shape: "<Verb>… (<elapsed> [· <tokens>] [· <substate>])" — match
      // it directly so we catch early-turn statuses without tokens/·, not just
      // the fully-painted ones. The verb char class includes '-' so hyphenated
      // verbs ("Razzle-dazzling") are captured whole; the first char is a
      // letter so a stray leading hyphen can't sneak in.
      const sm = rows[r].match(/([A-Za-zé'][A-Za-zé'-]{2,})…\s*\(([^)]*)\)/);
      if (sm && /\d+\s*[hms]\b/.test(sm[2])) {
        const verb = sm[1];
        const segs = sm[2].split("·").map((x) => x.trim());
        const elapsed =
          segs.find((x) => /\d+\s*[hms]\b/.test(x) && !/token/i.test(x)) || null;
        const substate =
          segs.find(
            (x) => /[a-z]{4}/i.test(x) && !/token/i.test(x) && x !== elapsed,
          ) || null;
        if (verb && elapsed) {
          invoke("report_grid_status", {
            payload: { provider: "claude", verb, elapsed, substate },
          }).catch(() => {});
          const verbKey = "claude|" + verb + "|" + (substate || "");
          if (verbKey !== __gridStatusVerbKey) {
            __gridStatusVerbKey = verbKey;
            invoke("log_from_right_pane", {
              payload: {
                kind: "iframe-trace",
                subkind: "grid-status",
                status: rows[r].trim(),
              },
            }).catch(() => {});
          }
        }
        break;
      }
    }
  } catch (e) {}
}
term.onWriteParsed(() => requestAnimationFrame(__gridReadStatus));
// ===== end xterm-grid screen reader =====

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

// Delay pty_spawn until fonts and layout have settled so the PTY gets
// the correct initial dimensions. If we spawn immediately, term.cols/rows
// are xterm.js defaults (80×24); the subsequent fitAddon.fit() then sends
// pty_resize, but shells don't redraw existing output after SIGWINCH so
// the prompt stays confined to the upper portion of the terminal.
const _spawnPty = async () => {
  // Fit right before reading cols/rows so the PTY inherits the actual
  // container dimensions, not the xterm.js defaults.
  fitAddon.fit();
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
};
{
  const _ready = document.fonts?.ready;
  const _afterFonts = () =>
    requestAnimationFrame(() => requestAnimationFrame(_spawnPty));
  if (_ready && typeof _ready.then === "function") {
    _ready.then(_afterFonts, _afterFonts);
  } else {
    _afterFonts();
  }
}

// Right-pane base URL is provisioned by the Rust backend on startup —
// it returns the tauri:// scheme URL whose path the scheme handler
// routes to the project's content (`/__project/*` proxied to the
// loopback HTTP server) or to embedded shell assets. We ask for it
// before setting iframe.src so the path picked by the backend wins
// over any default; reload happens via re-assigning src with a cache
// buster.
const { listen } = window.__TAURI__.event;

const CONTROL_ESCAPE_SOURCES = new Set([
  "agent-switch-escape",
  "agent-reload-escape",
]);
const isControlEscapeSource = (source) => CONTROL_ESCAPE_SOURCES.has(String(source || ""));

// #210: drive user-Esc captures off the host's PTY-write detector
// (pty-esc-sent), not just keys that pass through term.onData. Shell-control
// Esc origins (agent switch/reload) are intentionally excluded: they are PTY
// control bytes, not user interruption gestures, and must not start xterm grid
// capture on the parent webview hot path. MUST stay below the `const { listen }`
// above: a top-level listen() call before that line throws at load and aborts
// the whole shell. The payload's `source` tags origin.
listen("pty-esc-sent", (e) => {
  const source = (e && e.payload && e.payload.source) || "unknown";
  if (isControlEscapeSource(source)) {
    logShellEvent({
      kind: "iframe-trace",
      subkind: "esc-capture",
      stage: "skipped",
      reason: "control-escape",
      source,
      at: new Date().toISOString(),
    });
    return;
  }
  if (__escCapture) return;
  try {
    __escBeginCapture("esc", source);
  } catch (err) {
    logShellEvent({
      kind: "iframe-trace",
      subkind: "esc-capture",
      stage: "error",
      error: String(err),
      at: new Date().toISOString(),
    });
  }
});

// send-path-strand-instrumentation: capture agent-pane sends (toTurn submit) the
// same way, to characterize the rare "message stranded in the input as if it
// needed a newline" case. Same machinery; kind="send" → subkind send-capture.
listen("pty-send-sent", (e) => {
  if (__escCapture) return;
  try {
    __escBeginCapture("send", (e && e.payload && e.payload.source) || "unknown");
  } catch (err) {
    logShellEvent({
      kind: "iframe-trace",
      subkind: "send-capture",
      stage: "error",
      error: String(err),
      at: new Date().toISOString(),
    });
  }
});

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
  // document `path` in .bram.json as path-only, so `?` is safe.
  const bust = (u) => u + (u.includes("?") ? "&" : "?") + "t=" + Date.now();
  const normalizeToolsRoute = (loc) => {
    const hash = loc?.hash || "";
    if (hash.startsWith("#/")) return hash;
    const path = loc?.pathname || "";
    if (!path || path === "/" || path === "/tools/" || path === "/tools/index.html") {
      return "";
    }
    return path.startsWith("/") ? "#" + path : "#/" + path;
  };
  const readSavedToolsHash = () => {
    try {
      const saved =
        localStorage.getItem(TOOLS_ROUTE_KEY) ||
        localStorage.getItem(LEGACY_TOOLS_ROUTE_KEY) ||
        "";
      const route = saved.startsWith("#/") ? saved : "";
      logShellEvent({
        kind: "tools-route-read",
        saved,
        route,
        at: new Date().toISOString(),
      });
      return route;
    } catch {
      return "";
    }
  };
  let lastPersistedToolsRoute = "";
  const persistToolsRoute = (toolsWindow) => {
    try {
      const route = normalizeToolsRoute(toolsWindow?.location);
      if (route && route !== lastPersistedToolsRoute) {
        localStorage.setItem(TOOLS_ROUTE_KEY, route);
        lastPersistedToolsRoute = route;
        logShellEvent({
          kind: "tools-route-save",
          route,
          pathname: toolsWindow?.location?.pathname || "",
          hash: toolsWindow?.location?.hash || "",
          at: new Date().toISOString(),
        });
      }
      return route;
    } catch {
      return "";
    }
  };
  const toolsSrcWithHash = (src, hash) => {
    const route = hash || readSavedToolsHash();
    logShellEvent({
      kind: "tools-route-restore",
      route,
      hasHash: !!hash,
      at: new Date().toISOString(),
    });
    return route ? src + route : src;
  };
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

    // Preserve the current XMLUI route across hot-reload. Without this,
    // the new iframe loads tools/index.html with no hash → router
    // restarts at the root route, yanking the user away from
    // /worklist or wherever else they were. Same-origin iframe
    // (tauri://localhost), so contentWindow.location.hash is readable.
    let preservedHash = "";
    try {
      preservedHash = persistToolsRoute(oldTools.contentWindow);
    } catch (e) {}

    const newTools = document.createElement("iframe");
    newTools.setAttribute("allow", oldTools.getAttribute("allow") || "");
    // Load off-screen so the user never sees the blank intermediate
    // state. Tiny size keeps it from affecting layout.
    newTools.style.cssText =
      "position:absolute;visibility:hidden;left:-99999px;top:0;width:1px;height:1px;";

    function onLoad() {
      newTools.removeEventListener("load", onLoad);
      // Promote: inherit the live class/style and the id, then replace
      // in the same DOM position. The h-splitter drag handler keeps
      // working because it re-queries by id on each pointerdown.
      newTools.className = oldTools.className;
      newTools.style.cssText = oldTools.style.cssText;
      newTools.id = "tools-pane";
      parent.replaceChild(newTools, oldTools);
      toolsSwapping = false;
    }
    newTools.addEventListener("load", onLoad);
    parent.appendChild(newTools);
    newTools.src = toolsSrcWithHash(newSrc, preservedHash);
  }
  // reloadAll: reload BOTH iframes. Used by the manual "reload xmlui app"
  // toolbar button and by the "tools-pane-reload" watcher event (drawer
  // code changed, both panes may consume it). Right pane swaps src in
  // place (the user's project app handles its own loading state); tools
  // pane goes through swapToolsIframe to avoid the flash.
  function reloadAll() {
    const rightSrc = bust(RIGHT_PANE_SRC);
    const toolsSrc = bust(TOOLS_PANE_SRC);
    tracePaneReload("reload-all-received", {
      rightSrc,
      toolsSrc,
    });
    iframe.src = rightSrc;
    tracePaneReload("right-src-set", {
      source: "reload-all",
      src: rightSrc,
    });
    swapToolsIframe(toolsSrc);
    tracePaneReload("tools-swap-start", {
      source: "reload-all",
      src: toolsSrc,
    });
  }
  // reloadRightPaneOnly: reload only the right pane. Used by the
  // "right-pane-reload" watcher event for user-project file changes AND
  // for .bram.json hot-reload (path/query updates). We re-fetch
  // the URL each time instead of reusing the captured one so config edits
  // are picked up. The drawer is poll-driven so it does NOT need to reload
  // here, and keeping it stable avoids postMessage-vs-iframe-rebuild races
  // on Approve/Drop clicks while the agent is writing files.
  async function reloadRightPaneOnly() {
    const startedAt = Date.now();
    tracePaneReload("right-listener-fired", {
      priorSrc: RIGHT_PANE_SRC,
      iframeSrc: iframe.getAttribute("src") || "",
    });
    try {
      RIGHT_PANE_SRC = await invoke("get_right_pane_url");
      tracePaneReload("right-url-resolved", {
        elapsedMs: Date.now() - startedAt,
        src: RIGHT_PANE_SRC,
      });
    } catch (e) {
      console.error("get_right_pane_url failed", e);
      tracePaneReload("right-url-error", {
        elapsedMs: Date.now() - startedAt,
        message: String((e && e.message) || e),
      });
    }
    const nextSrc = bust(RIGHT_PANE_SRC);
    iframe.src = nextSrc;
    tracePaneReload("right-src-set", {
      source: "right-pane-reload",
      elapsedMs: Date.now() - startedAt,
      src: nextSrc,
    });
  }
  // Single-shot retry: if the right-pane iframe hasn't fired `load`
  // within 1.5s, the project-managed server (from .bram.json)
  // is probably still starting up — connection is stuck. Bust and try
  // once more. Iframes fire `load` even for error pages, so this
  // specifically catches the "still connecting" state. `error` is not
  // reliable for iframes; we don't bother listening for it.
  let loaded = false;
  iframe.addEventListener("load", () => {
    loaded = true;
    tracePaneReload("right-load", {
      src: iframe.getAttribute("src") || "",
    });
  });
  iframe.src = RIGHT_PANE_SRC;
  tracePaneReload("right-initial-src-set", {
    src: RIGHT_PANE_SRC,
  });
  setTimeout(() => {
    if (!loaded) {
      const retrySrc = bust(RIGHT_PANE_SRC);
      tracePaneReload("right-initial-retry", {
        src: retrySrc,
      });
      iframe.src = retrySrc;
    } else {
      tracePaneReload("right-initial-retry-skipped", {
        src: iframe.getAttribute("src") || "",
      });
    }
  }, 1500);
  if (tools) {
    const toolsInitialSrc = toolsSrcWithHash(TOOLS_PANE_SRC);
    tools.src = toolsInitialSrc;
    tracePaneReload("tools-initial-src-set", {
      src: toolsInitialSrc,
    });
  }
  setInterval(() => {
    const currentTools = document.getElementById("tools-pane");
    if (currentTools?.contentWindow) persistToolsRoute(currentTools.contentWindow);
  }, TOOLS_ROUTE_POLL_MS);
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
  // #150 follow-up (refs #170): the host now stores right-pane-reload
  // and tools-pane-reload payloads via emit_replayable_signal so a
  // late-attaching listener can recover them. main.js's listeners
  // attach after Tauri is ready; if the host fired a reload during
  // the gap (project-config-reload at startup, etc.), the live emit
  // was lost and the iframes stayed frozen. Ask the host to replay
  // each on attach; the request is idempotent on no-stored-payload.
  fetch("/__startup-ready?event=right-pane-reload", { cache: "no-store" }).catch(() => {});
  fetch("/__startup-ready?event=tools-pane-reload", { cache: "no-store" }).catch(() => {});
})();

// ui.showTargetApp driver. The Settings tab and hand-edits to
// .bram.json both flow here via the `settings-changed` event (emitted
// by handle_project_config_reload in src-tauri/src/lib.rs).
//
// Default OFF: the embedded target app is a minority case — most users
// run their own web app in their own server and view it in their own
// browser — so Bram ships with the pane hidden. When showTargetApp is
// absent or false, #right-pane and the h-splitter get display:none and
// the agent-tools pane grows to fill the right column (overriding its
// CSS `flex: 0 0 240px`). True restores the normal vertical split.
(() => {
  let shown = false;
  function apply() {
    const right = document.getElementById("right-pane");
    const hSplitter = document.getElementById("h-splitter");
    const tools = document.getElementById("tools-pane");
    if (!right || !hSplitter || !tools) return;
    right.style.display = shown ? "" : "none";
    hSplitter.style.display = shown ? "" : "none";
    // Hidden: grow the tools pane to fill the column. Shown: clear the
    // overrides so CSS (flex: 0 0 240px) and the h-splitter drag handler
    // take over again.
    tools.style.flexGrow = shown ? "" : "1";
    tools.style.flexShrink = shown ? "" : "1";
    tools.style.flexBasis = shown ? "" : "0";
  }
  function set(next) {
    shown = !!next;
    apply();
  }
  fetch("/__settings")
    .then((r) => r.json())
    .then((v) => set(v && v.ui && v.ui.showTargetApp))
    .catch(() => {});
  listen("settings-changed", (e) => {
    const v = e && e.payload;
    set(v && v.ui && v.ui.showTargetApp);
  });
})();

// Click-to-toggle voice. The toolbar 🎤 button toggles its own recording;
// iframes (Workspace, etc.) drive the same recorder via voice-start/voice-stop
// messages. Auto-starts whisper-server on first record click.
(() => {
  const WHISPER_HOST = "http://127.0.0.1:18080";
  const WHISPER_URL = WHISPER_HOST + "/inference";
  // Host-native macOS/Linux launch expands this on the host. Windows launch
  // sends it to WSL so it resolves under the WSL user's home directory.
  const MODEL_PATH = "~/.local/share/whisper-models/ggml-small.en.bin";
  const READY_TIMEOUT_MS = 15000;
  const READY_POLL_MS = 300;
  const IFRAME_ORPHAN_GRACE_MS = 10000;
  const IFRAME_RECORDING_WATCHDOG_MS = 5 * 60 * 1000;

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
        {
          payload: Object.assign(
            { kind: "voice-host", stage, at: new Date().toISOString() },
            payload || {},
          ),
        },
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
  let activeStopAtMs = null;
  let activeStopReceivedAtMs = null;
  let activeStartedAtMs = null;
  let activeRecorderStarted = false;
  let activeStopRequested = false;
  let activeWatchdogTimer = null;
  // Synthetic requestId for toolbar sessions — keeps log entries correlated
  // even though the toolbar path never receives an iframe-supplied id.
  let toolbarRequestId = null;
  const currentRequestId = () =>
    active === "toolbar"
      ? toolbarRequestId
      : active && typeof active === "object"
        ? active.requestId
        : null;
  const activeKind = () =>
    active === null ? null : active === "toolbar" ? "toolbar" : "iframe";
  const activeAgeMs = () =>
    typeof activeStartedAtMs === "number" ? Date.now() - activeStartedAtMs : null;

  const clearActiveWatchdog = () => {
    if (activeWatchdogTimer) {
      clearTimeout(activeWatchdogTimer);
      activeWatchdogTimer = null;
    }
  };

  const postIframeVoiceState = (state, extra) => {
    if (!active || active === "toolbar" || !active.source) return;
    try {
      active.source.postMessage(
        Object.assign(
          {
            type: "voice-state",
            state,
            requestId: active.requestId,
            target: active.voiceTarget || "",
          },
          extra || {},
        ),
        "*",
      );
    } catch (e) {
      voiceLog("voice-state-postmessage-error", {
        requestId: currentRequestId(),
        state,
        error: String(e),
      });
    }
  };

  const resetActiveState = () => {
    clearActiveWatchdog();
    active = null;
    activeStopAtMs = null;
    activeStopReceivedAtMs = null;
    activeStartedAtMs = null;
    activeRecorderStarted = false;
    activeStopRequested = false;
    toolbarRequestId = null;
  };

  const recoverStaleActiveRecording = (reason) => {
    const staleRequestId = currentRequestId();
    const staleKind = activeKind();
    const recorderState = mediaRecorder ? mediaRecorder.state : "null";
    const ageMs = activeAgeMs();
    voiceLog("stale-recording-recovered", {
      requestId: staleRequestId,
      reason,
      activeWas: staleKind,
      ageMs,
      recorderState,
      recorderStarted: activeRecorderStarted,
      stopRequested: activeStopRequested,
    });
    clearActiveWatchdog();
    if (mediaRecorder) {
      try {
        mediaRecorder.ondataavailable = null;
        mediaRecorder.onstop = null;
        if (mediaRecorder.state === "recording") {
          mediaRecorder.stop();
        }
      } catch (e) {
        voiceLog("stale-recording-stop-error", {
          requestId: staleRequestId,
          reason,
          error: String(e),
        });
      }
      mediaRecorder = null;
    }
    stopStream();
    audioChunks = [];
    resetActiveState();
    if (toolbarBtn.dataset.state !== "idle") setToolbarState("idle");
  };

  const armIframeRecordingWatchdog = () => {
    clearActiveWatchdog();
    activeWatchdogTimer = setTimeout(() => {
      if (active && active !== "toolbar" && !activeStopRequested) {
        recoverStaleActiveRecording("iframe-watchdog-timeout");
      }
    }, IFRAME_RECORDING_WATCHDOG_MS);
  };

  const canRecoverStaleIframeForNewStart = (target) =>
    active &&
    active !== "toolbar" &&
    target &&
    typeof target === "object" &&
    target.source &&
    active.source !== target.source &&
    typeof activeStartedAtMs === "number" &&
    Date.now() - activeStartedAtMs >= IFRAME_ORPHAN_GRACE_MS;

  const setToolbarState = (state) => {
    toolbarBtn.dataset.state = state;
    toolbarBtn.innerHTML =
      state === "recording"
        ? "&#x23F9;"
        : state === "processing"
          ? '<span class="voice-spinner" aria-label="Processing voice input"></span>'
        : state === "starting"
          ? "&#x23F3;"
          : "&#x1F3A4;";
  };
  setToolbarState("idle");

  const mediaRecorderConfig = () => {
    const opusWebm = "audio/webm;codecs=opus";
    if (
      window.MediaRecorder &&
      typeof MediaRecorder.isTypeSupported === "function" &&
      MediaRecorder.isTypeSupported(opusWebm)
    ) {
      return { options: { mimeType: opusWebm }, codecHint: opusWebm };
    }
    return { options: undefined, codecHint: "default" };
  };

  const probeWhisperServer = async (stage) => {
    try {
      const res = await fetch(WHISPER_HOST + "/", {
        method: "GET",
        cache: "no-store",
      });
      voiceLog(stage, { httpStatus: res.status, ready: res.ok });
      return res.ok;
    } catch (e) {
      voiceLog(stage + "-error", { error: String(e) });
      return false;
    }
  };

  const ensureServerRunning = async () => {
    const startedAt = Date.now();
    let polls = 0;
    voiceLog("ensure-server-enter");
    if (await probeWhisperServer("whisper-preflight")) {
      voiceLog("ensure-server-ready", {
        elapsedMs: Date.now() - startedAt,
        source: "preflight",
      });
      return { ready: true, reason: "preflight" };
    }
    let status;
    try {
      status = await invoke("whisper_status");
    } catch (e) {
      console.error("whisper_status", e);
      voiceLog("whisper-status-error", { error: String(e) });
      return { ready: false, reason: "status-error" };
    }
    voiceLog("whisper-status", {
      running: !!(status && status.running),
      pid: status && status.pid ? status.pid : null,
    });
    voiceLog("ensure-server-status-result", {
      running: !!(status && status.running),
      pid: status && status.pid ? status.pid : null,
    });
    if (status && status.running) {
      voiceLog("ensure-server-ready", {
        elapsedMs: Date.now() - startedAt,
        source: "status",
      });
      return { ready: true, reason: "status" };
    }
    try {
      voiceLog("ensure-server-start-invoked", { modelPath: MODEL_PATH });
      const pid = await invoke("whisper_start", { modelPath: MODEL_PATH });
      voiceLog("whisper-started", { pid });
    } catch (e) {
      console.error("whisper_start", e);
      voiceLog("whisper-start-error", { error: String(e) });
      voiceLog("ensure-server-start-error", { error: String(e) });
      return { ready: false, reason: "start-failed" };
    }
    const deadline = Date.now() + READY_TIMEOUT_MS;
    while (Date.now() < deadline) {
      polls += 1;
      try {
        const res = await fetch(WHISPER_HOST + "/", { method: "GET" });
        voiceLog("ensure-server-poll-tick", {
          elapsedMs: Date.now() - startedAt,
          fetchOk: true,
          httpStatus: res.status,
          poll: polls,
        });
        if (res.ok) {
          voiceLog("whisper-ready", { httpStatus: res.status });
          voiceLog("ensure-server-ready", { elapsedMs: Date.now() - startedAt });
          return { ready: true, reason: "started" };
        }
        voiceLog("whisper-ready-poll", { httpStatus: res.status });
      } catch (e) {
        voiceLog("ensure-server-poll-tick", {
          elapsedMs: Date.now() - startedAt,
          fetchOk: false,
          error: String(e),
          poll: polls,
        });
        voiceLog("whisper-ready-poll-error", { error: String(e) });
      }
      await new Promise((r) => setTimeout(r, READY_POLL_MS));
    }
    voiceLog("whisper-ready-timeout", { timeoutMs: READY_TIMEOUT_MS });
    voiceLog("ensure-server-timeout", {
      elapsedMs: Date.now() - startedAt,
      totalPolls: polls,
    });
    return { ready: false, reason: "timeout" };
  };

  // Send startup and transcription failures to the agent-pane iframe
  // (same-origin tools-pane), which owns the toast surface. This covers every
  // mic origin — toolbar, agent pane, and any target-app iframe. Startup
  // failures retain the original reason-only payload; post-recording failures
  // add a kind and server detail so the toast can explain what broke.
  const notifyWhisperUnavailable = (reason, kind, detail) => {
    voiceLog("whisper-unavailable-notice", {
      reason: String(reason || ""),
      kind: String(kind || ""),
      detail: String(detail || ""),
    });
    try {
      const tools = document.getElementById("tools-pane");
      if (tools && tools.contentWindow) {
        const notice = { type: "bram-whisper-unavailable", reason: String(reason || "") };
        // Preserve the original server-not-running message shape. Typed fields
        // are added only for failures after recording has already succeeded.
        if (kind) notice.kind = String(kind);
        if (detail) notice.detail = String(detail);
        tools.contentWindow.postMessage(
          notice,
          "*",
        );
      }
    } catch (e) {
      voiceLog("whisper-unavailable-notice-error", { error: String(e) });
    }
  };

  const notifyVoiceBusy = (requester) => {
    const activeWas = activeKind();
    const activeRequestId = currentRequestId();
    const activeTarget =
      active && typeof active === "object" ? active.voiceTarget || "" : activeWas || "";
    voiceLog("voice-busy-notice", {
      requester: requester || "",
      activeWas,
      activeRequestId,
      activeTarget,
      activeAgeMs: activeAgeMs(),
    });
    try {
      const tools = document.getElementById("tools-pane");
      if (tools && tools.contentWindow) {
        tools.contentWindow.postMessage(
          {
            type: "bram-voice-busy",
            requester: requester || "",
            activeWas,
            activeRequestId,
            activeTarget,
          },
          "*",
        );
      }
    } catch (e) {
      voiceLog("voice-busy-notice-error", { error: String(e) });
    }
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
    const focusTerminalAfterDelivery = active === "toolbar";
    const deliveredAtMs = Date.now();
    const stopToDeliverMs =
      typeof activeStopAtMs === "number" ? deliveredAtMs - activeStopAtMs : null;
    voiceLog("deliverTranscript", {
      requestId: reqId,
      stopAtMs: activeStopAtMs,
      stopToDeliverMs: stopToDeliverMs,
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
            target: active.voiceTarget || "",
            transcript: text,
            stopAtMs: activeStopAtMs,
            stopToDeliverMs: stopToDeliverMs,
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
    postIframeVoiceState("idle", { transcriptLength: text.length });
    active = null;
    mediaRecorder = null;
    resetActiveState();
    if (toolbarBtn.dataset.state !== "idle") setToolbarState("idle");
    if (focusTerminalAfterDelivery) {
      requestAnimationFrame(() => {
        try {
          term.focus();
          voiceLog("deliverTranscript-terminal-focus", { requestId: reqId });
        } catch (e) {
          voiceLog("deliverTranscript-terminal-focus-error", {
            requestId: reqId,
            error: String(e),
          });
        }
      });
    }
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
      if (canRecoverStaleIframeForNewStart(target)) {
        recoverStaleActiveRecording("new-iframe-start-different-source");
      } else {
        voiceLog("startRecording-rejected-busy", {
          requestId: incomingId,
          activeRequestId: currentRequestId(),
          activeAgeMs: activeAgeMs(),
          activeWas: activeKind(),
        });
        notifyVoiceBusy(target === "toolbar" ? "toolbar" : "iframe");
        // Already busy: tell the new requester nothing came of it.
        if (target && typeof target === "object" && target.source) {
          try {
            target.source.postMessage(
              {
                type: "voice-into-result",
                requestId: target.requestId,
                target: target.voiceTarget || "",
                transcript: "",
                rejected: true,
                reason: "busy",
                activeWas: activeKind(),
                activeRequestId: currentRequestId(),
              },
              "*",
            );
          } catch (_) {}
        }
        return;
      }
    }
    active = target;
    activeStartedAtMs = Date.now();
    activeRecorderStarted = false;
    activeStopRequested = false;
    activeStopAtMs = null;
    activeStopReceivedAtMs = null;
    const isToolbar = target === "toolbar";
    if (isToolbar) toolbarRequestId = incomingId;
    if (isToolbar) setToolbarState("starting");
    postIframeVoiceState("starting");
    const serverResult = await ensureServerRunning();
    if (!serverResult.ready) {
      console.error("whisper-server did not become ready");
      voiceLog("startRecording-not-ready", {
        requestId: incomingId,
        reason: serverResult.reason,
      });
      notifyWhisperUnavailable(serverResult.reason);
      postIframeVoiceState("idle", { reason: serverResult.reason || "whisper-unavailable" });
      const t = active;
      resetActiveState();
      if (isToolbar) setToolbarState("idle");
      if (t && typeof t === "object" && t.source) {
        try {
          t.source.postMessage(
            {
              type: "voice-into-result",
              requestId: t.requestId,
              transcript: "",
              reason: "whisper-unavailable",
            },
            "*",
          );
        } catch (_) {}
      }
      return;
    }
    try {
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        throw new Error("navigator.mediaDevices.getUserMedia unavailable");
      }
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (e) {
      console.error("getUserMedia", e);
      voiceLog("getUserMedia-error", {
        requestId: incomingId,
        error: String(e),
        name: e && e.name ? e.name : "",
        message: e && e.message ? e.message : "",
      });
      postIframeVoiceState("idle", { reason: "getUserMedia-error" });
      const t = active;
      resetActiveState();
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
    voiceLog("getUserMedia-ok", {
      requestId: incomingId,
      tracks: stream && typeof stream.getAudioTracks === "function"
        ? stream.getAudioTracks().length
        : null,
    });
    audioChunks = [];
    const recorderConfig = mediaRecorderConfig();
    try {
      mediaRecorder = new MediaRecorder(stream, recorderConfig.options);
    } catch (e) {
      console.error("MediaRecorder", e);
      voiceLog("mediaRecorder-create-error", {
        requestId: incomingId,
        error: String(e),
      });
      postIframeVoiceState("idle", { reason: "mediaRecorder-create-error" });
      stopStream();
      const t = active;
      resetActiveState();
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
    mediaRecorder.ondataavailable = (e) => {
      const size = e.data && e.data.size ? e.data.size : 0;
      voiceLog("mediaRecorder-data", {
        requestId: currentRequestId(),
        chunkSize: size,
      });
      if (size > 0) audioChunks.push(e.data);
    };
    mediaRecorder.onstop = async () => {
      const reqId = currentRequestId();
      const onstopAtMs = Date.now();
      stopStream();
      const blobType =
        (mediaRecorder && mediaRecorder.mimeType) ||
        (recorderConfig.options && recorderConfig.options.mimeType) ||
        "audio/webm";
      const blob = new Blob(audioChunks, { type: blobType });
      audioChunks = [];
      voiceLog("mediaRecorder-onstop", {
        requestId: reqId,
        stopAtMs: activeStopAtMs,
        stopToOnstopMs:
          typeof activeStopAtMs === "number" ? onstopAtMs - activeStopAtMs : null,
        blobSize: blob.size,
        mimeType: blob.type,
        mediaRecorderState: mediaRecorder ? mediaRecorder.state : "null",
        codecHint: recorderConfig.codecHint,
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
          stopAtMs: activeStopAtMs,
          stopToRequestMs:
            typeof activeStopAtMs === "number" ? reqStart - activeStopAtMs : null,
          blobSize: blob.size,
        });
        const res = await fetch(WHISPER_URL, { method: "POST", body: formData });
        httpStatus = res.status;
        if (res.ok) {
          const data = await res.json();
          transcript = (data.text || "").trim();
        } else {
          let responseBody = "";
          let serverError = "";
          try {
            responseBody = await res.text();
            const parsed = JSON.parse(responseBody);
            if (parsed && typeof parsed.error === "string") {
              serverError = parsed.error.trim();
            }
          } catch (_) {
            // A non-JSON or unreadable body still produces the useful status.
          }
          const detail = `HTTP ${res.status}${serverError ? `: ${serverError}` : ""}`;
          console.error("transcribe HTTP", detail);
          notifyWhisperUnavailable("transcription-failed", "transcription-failed", detail);
        }
        voiceLog("whisper-response", {
          requestId: reqId,
          stopAtMs: activeStopAtMs,
          stopToResponseMs:
            typeof activeStopAtMs === "number" ? Date.now() - activeStopAtMs : null,
          httpStatus: httpStatus,
          elapsedMs: Date.now() - reqStart,
          transcriptLength: transcript.length,
          transcriptPreview: transcript.slice(0, 80),
        });
      } catch (e) {
        console.error("transcribe", e);
        notifyWhisperUnavailable("transcription-failed", "transcription-failed", String(e));
        voiceLog("whisper-error", {
          requestId: reqId,
          error: String(e),
          errorName: e && e.name ? e.name : "",
          errorMessage: e && e.message ? e.message : "",
        });
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
    activeRecorderStarted = true;
    voiceLog("mediaRecorder-start", { requestId: incomingId });
    if (isToolbar) {
      setToolbarState("recording");
    } else if (active && typeof active === "object" && active.source) {
      armIframeRecordingWatchdog();
      postIframeVoiceState("recording");
      try {
        active.source.postMessage(
          {
            type: "voice-recording-started",
            requestId: active.requestId,
            target: active.voiceTarget || "",
          },
          "*",
        );
      } catch (e) {
        voiceLog("voice-recording-started-postmessage-error", {
          requestId: incomingId,
          error: String(e),
        });
      }
    }
  };

  const stopRecording = (stopAtMs, source) => {
    activeStopAtMs = typeof stopAtMs === "number" ? stopAtMs : Date.now();
    activeStopReceivedAtMs = Date.now();
    activeStopRequested = true;
    clearActiveWatchdog();
    postIframeVoiceState("processing");
    voiceLog("stopRecording", {
      requestId: currentRequestId(),
      source: source || "unknown",
      stopAtMs: activeStopAtMs,
      stopToParentReceiveMs: activeStopReceivedAtMs - activeStopAtMs,
      mediaRecorderState: mediaRecorder ? mediaRecorder.state : "null",
    });
    if (mediaRecorder && mediaRecorder.state === "recording") {
      mediaRecorder.stop();
    } else {
      voiceLog("stopRecording-no-active-recorder", {
        requestId: currentRequestId(),
      });
      deliverTranscript("");
    }
  };

  toolbarBtn.addEventListener("click", () => {
    const state = toolbarBtn.dataset.state;
    if (active && active !== "toolbar") {
      notifyVoiceBusy("toolbar");
      return;
    }
    if (state === "recording") {
      setToolbarState("processing");
      stopRecording(Date.now(), "toolbar-click");
    } else if (state === "idle") {
      startRecording("toolbar");
    }
    // ignore clicks while starting or processing
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
      startRecording({ source: ev.source, requestId: d.requestId, voiceTarget: d.target || "" });
    } else if (d.kind === "voice-stop") {
      const stopAtMs = typeof d.stopAtMs === "number" ? d.stopAtMs : Date.now();
      voiceLog("iframe-voice-stop", {
        requestId: d.requestId,
        stopAtMs: stopAtMs,
        stopToParentReceiveMs: Date.now() - stopAtMs,
      });
      if (
        !active ||
        active === "toolbar" ||
        !d.requestId ||
        !ev.source ||
        active.source !== ev.source ||
        active.requestId !== d.requestId
      ) {
        voiceLog("iframe-voice-stop-ignored", {
          requestId: d.requestId,
          stopAtMs: stopAtMs,
          activeWas:
            active === null ? null : active === "toolbar" ? "toolbar" : "iframe",
          activeRequestId: currentRequestId(),
          hasSource: !!ev.source,
          sourceMatches:
            !!(active && typeof active === "object" && ev.source && active.source === ev.source),
        });
        try {
          ev.source &&
            ev.source.postMessage(
              {
                type: "voice-into-result",
                requestId: d.requestId,
                target: d.target || "",
                transcript: "",
                stopAtMs: stopAtMs,
                stopToDeliverMs: Date.now() - stopAtMs,
              },
              "*",
            );
        } catch (_) {}
        return;
      }
      stopRecording(stopAtMs, "iframe-message");
    }
  });
})();
