// Shell-side helpers exposed to any XMLUI app served by the xmlui-desktop
// binary. Include from your project's index.html with:
//
//   <script src="tauri://localhost/__shell/helpers.js"></script>
//
// Both iframes (right pane and agent-tools drawer) are same-origin with
// the parent shell at tauri://localhost, so these helpers call Tauri IPC
// directly via window.parent.__TAURI__.core.invoke. `getTauriInvoke()`
// formalizes the lookup with a window.__TAURI__ → window.parent → window.top
// fallback chain. The legacy postMessage bridge to app/main.js has been
// retired; voice recording (voiceStart / voiceStop) is the one remaining
// exception, because the parent shell owns the MediaRecorder pipeline.

window._xsLogs = window._xsLogs || [];

// Diagnostic: log every fetch URL to the host. Strip auth/etc — just URL.
// Temporary instrumentation for the queryParams investigation.
(function logFetches() {
  if (window._fetchLogged) return;
  window._fetchLogged = true;
  var orig = window.fetch;
  window.fetch = function (input, init) {
    try {
      var url = typeof input === "string" ? input : (input && input.url);
      if (url && url.indexOf("/__sessions/latest-tail") !== -1) {
        window.logToHost({ kind: "fetch-url", url: url });
      }
    } catch (e) {}
    return orig.apply(this, arguments);
  };
})();

// Persist the tools-pane route across iframe reloads. main.js reassigns
// tools.src on every tools-pane-reload event (drawer code changed under
// app/tools/), which drops the hash and lands the user on the default
// route (Worklist). We solve this from inside the iframe: restore the
// saved hash on boot, save the current hash on change.
//
// Scoped to the tools iframe — user-project apps in the right pane have
// their own route conventions and should not be affected.
(function persistToolsRoute() {
  if (window.location.pathname.indexOf("/__tools/") === -1) return;
  var key = "xmlui-desktop.tools.route";
  try {
    var current = window.location.hash;
    if (!current || current === "#/") {
      var saved = localStorage.getItem(key);
      if (saved && saved !== "#/") {
        window.location.hash = saved;
      }
    }
    // react-router-dom uses history.pushState which doesn't fire
    // hashchange, so poll instead of listening.
    setInterval(function () {
      var h = window.location.hash;
      if (h && h !== localStorage.getItem(key)) {
        localStorage.setItem(key, h);
      }
    }, 500);
  } catch (e) {}
})();

window.toShell = function (text) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("pty_write", { data: String(text) + "\n" }).catch(function (e) {
    console.error("toShell pty_write", e);
  });
};
window.toTurn = function (text) {
  var s = String(text);
  try {
    window.logToHost({
      kind: "to-turn",
      stage: "source",
      textLength: s.length,
      textPreview: s.slice(0, 80),
      at: new Date().toISOString(),
    });
  } catch (e) {}
  var normalized = s.replace(/\s+/g, " ").trim();
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("log_from_right_pane", {
    payload: {
      kind: "to-turn",
      stage: "sink",
      textLength: normalized.length,
      textPreview: normalized.slice(0, 80),
      at: new Date().toISOString(),
    },
  }).catch(function () {});
  invoke("pty_write", {
    data: "\x15\x1b[200~" + normalized + "\x1b[201~\r",
  }).catch(function (e) {
    console.error("toTurn pty_write", e);
  });
};
// sendKeys writes raw bytes to the PTY with NO trailing newline (unlike
// toShell which always appends \n). Use it for control sequences like ESC,
// arrow keys, or single-keypress menu shortcuts.
window.sendKeys = function (text) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("pty_write", { data: String(text) }).catch(function (e) {
    console.error("sendKeys pty_write", e);
  });
};
window.logToHost = function (payload) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("log_from_right_pane", { payload: payload }).catch(function () {});
};
window.openExternal = function (url) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("open_url", { url: String(url) }).catch(function (e) {
    console.error("openExternal open_url", e);
  });
};
// Capture an interactive screenshot via the host (macOS: screencapture -i)
// and inject the resulting file path into the terminal as a fresh user turn
// so claude reads it via its Read tool. User cancellation (Esc during the
// rect drag) is silent; other errors go to the host log.
window.captureScreenshot = function () {
  function deliver(path) {
    // Dual format: `@<path>` is claude-code's file-reference syntax (tells
    // the model to use its Read tool), and `[Image: source: <path>]` is
    // the marker Talk's extractImagePaths matches to render a thumbnail.
    // stripImagePaths removes the marker from the visible text, so the
    // displayed user turn shows "Read this screenshot: @path" plus the
    // inline thumbnail below.
    if (path) toTurn("Read this screenshot: @" + path + "\n[Image: source: " + path + "]");
  }
  function report(err) {
    var msg = String((err && err.message) || err);
    if (msg !== "cancelled") {
      logToHost({ kind: "screenshot", error: msg });
    }
  }
  var invoke = getTauriInvoke();
  if (!invoke) {
    report(new Error("Tauri IPC unavailable"));
    return;
  }
  invoke("capture_screenshot", {}).then(deliver).catch(report);
};

// Click-to-toggle voice. Single in-flight session per iframe.
//   voiceStart()              — starts recording (parent records on iframe's behalf).
//   voiceStop(callback)       — stops; callback(transcript) fires when transcript is ready.
// XMLUI's onClick expression evaluator does not reliably execute .then() callbacks
// attached during expression evaluation; passing a callback function as an argument
// works, since the callback is invoked from plain JS later.
window._voiceSession = null;
function _voiceLog(stage, payload) {
  try {
    window.logToHost(
      Object.assign(
        { kind: "voice", stage: stage, at: new Date().toISOString() },
        payload || {},
      ),
    );
  } catch (e) {}
}
window.voiceStart = function () {
  if (window._voiceSession) {
    _voiceLog("voiceStart-rejected-already-active", {
      currentSession: window._voiceSession,
    });
    return;
  }
  var requestId =
    "voice-" + Date.now() + "-" + Math.random().toString(36).slice(2);
  window._voiceSession = requestId;
  _voiceLog("voiceStart", { requestId: requestId });
  window.parent.postMessage(
    { type: "right-pane", kind: "voice-start", requestId: requestId },
    "*",
  );
};
window.voiceStop = function (callback) {
  var requestId = window._voiceSession;
  window._voiceSession = null;
  if (!requestId) {
    _voiceLog("voiceStop-no-session");
    if (typeof callback === "function") callback("");
    return;
  }
  _voiceLog("voiceStop", { requestId: requestId });
  function onResult(ev) {
    var data = ev && ev.data;
    if (!data || data.type !== "voice-into-result") return;
    if (data.requestId !== requestId) {
      _voiceLog("voice-into-result-mismatch", {
        expected: requestId,
        received: data.requestId,
        transcriptPreview: String(data.transcript || "").slice(0, 80),
      });
      return;
    }
    window.removeEventListener("message", onResult);
    var transcript = String(data.transcript || "");
    _voiceLog("voice-into-result", {
      requestId: requestId,
      transcriptLength: transcript.length,
      transcriptPreview: transcript.slice(0, 80),
    });
    if (typeof callback === "function") callback(transcript);
  }
  window.addEventListener("message", onResult);
  window.parent.postMessage(
    { type: "right-pane", kind: "voice-stop", requestId: requestId },
    "*",
  );
};
// Snapshot of the iframe's current pixel size. Same-origin iframes can
// read their own viewport dimensions directly — no parent round-trip
// needed. Callback receives { width, height } as integers (rounded).
window.getRightPaneSize = function (callback) {
  if (typeof callback !== "function") return;
  callback({
    width: Math.round(window.innerWidth || 0),
    height: Math.round(window.innerHeight || 0),
  });
};

// Subscribe to session-JSONL change events. The parent shell receives
// `talk-session-changed` Tauri events from the file watcher; same-origin
// iframes can listen for that event directly via window.parent.__TAURI__.
// Used by Transcript / Workspace to refetch immediately on provider
// session-file writes — eliminates the poll-window lag where short-lived
// menu or turn-boundary state could come and go between ticks.
var __talkSessionSubscriber = null;
window.onTalkSessionChange = function (fn) {
  __talkSessionSubscriber = typeof fn === "function" ? fn : null;
};
try {
  if (window.parent && window.parent.__TAURI__ && window.parent.__TAURI__.event) {
    window.parent.__TAURI__.event.listen("talk-session-changed", function () {
      if (typeof __talkSessionSubscriber === "function") {
        __talkSessionSubscriber();
      }
    });
  }
} catch (e) {}

// Continuous variant: register a callback that fires on every resize
// (window.resize event inside the iframe) plus once with the current
// size at registration time. Use this when you want a readout that
// stays live, not just a snapshot on a button click.
var __rpsSubscriber = null;
var __rpsListenerAttached = false;
function __rpsBroadcast() {
  if (typeof __rpsSubscriber === "function") {
    __rpsSubscriber({
      width: Math.round(window.innerWidth || 0),
      height: Math.round(window.innerHeight || 0),
    });
  }
}
window.subscribeRightPaneSize = function (callback) {
  __rpsSubscriber = typeof callback === "function" ? callback : null;
  if (!__rpsSubscriber) return;
  __rpsBroadcast();
  if (!__rpsListenerAttached) {
    window.addEventListener("resize", __rpsBroadcast);
    __rpsListenerAttached = true;
  }
};
// Push local commits to origin and refetch a DataSource (typically
// the commits list) when the push completes, so the pushed flags
// refresh without a manual reload.
window.gitPush = function (commitsDs) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("git_push", {})
    .then(function () {
      if (commitsDs && typeof commitsDs.refetch === "function") {
        commitsDs.refetch();
      }
    })
    .catch(function (e) {
      window.logToHost({ kind: "git-push", phase: "err", error: String(e) });
    });
};
// In-flight marker that persists across iframe reloads. At click
// time we snapshot the current worklist's item IDs; the XMLUI side
// clears the flag whenever worklist.json's items differ from that
// snapshot — works on the initial fetch too (so refresh recovers
// from a stale flag), not just on refetches.
window.markInflight = function (items) {
  try {
    var sig = (items || [])
      .filter(function (i) { return i && i.id; })
      .map(function (i) { return i.id + ":" + (i.status || "proposed"); })
      .sort()
      .join(",");
    localStorage.setItem("inflight", JSON.stringify({ ids: sig, ts: Date.now() }));
  } catch (e) {}
};
window.getInflight = function () {
  try {
    var raw = localStorage.getItem("inflight");
    if (!raw) return null;
    var v = JSON.parse(raw);
    return v && typeof v === "object" ? v : null;
  } catch (e) {
    return null;
  }
};
window.clearInflight = function () {
  try {
    localStorage.removeItem("inflight");
  } catch (e) {}
};
// Workspace pending-items selection persists across iframe reloads.
// Stored as a single JSON array of currently-checked item ids.
window.loadChecked = function () {
  try {
    var raw = localStorage.getItem("workspace-checked");
    if (!raw) return [];
    var v = JSON.parse(raw);
    return Array.isArray(v) ? v : [];
  } catch (e) { return []; }
};
window.saveChecked = function (ids) {
  try {
    localStorage.setItem("workspace-checked", JSON.stringify(ids || []));
  } catch (e) {}
};
// Sessions tab: pending-delete and pending-rename ids persist across
// iframe reloads, so the dim+disable state survives until the user
// explicitly clears it (or the JSONL stops resolving to the same id).
// Two separate keys mirror the in-memory pendingDeletes / pendingRenames
// vars in Sessions.xmlui.
window.loadPendingSessionDeletes = function () {
  try {
    var raw = localStorage.getItem("session-pending-deletes");
    if (!raw) return [];
    var v = JSON.parse(raw);
    return Array.isArray(v) ? v : [];
  } catch (e) { return []; }
};
window.savePendingSessionDeletes = function (ids) {
  try {
    localStorage.setItem("session-pending-deletes", JSON.stringify(ids || []));
  } catch (e) {}
};
window.loadPendingSessionRenames = function () {
  try {
    var raw = localStorage.getItem("session-pending-renames");
    if (!raw) return [];
    var v = JSON.parse(raw);
    return Array.isArray(v) ? v : [];
  } catch (e) { return []; }
};
window.savePendingSessionRenames = function (ids) {
  try {
    localStorage.setItem("session-pending-renames", JSON.stringify(ids || []));
  } catch (e) {}
};
// Drop ids from saved selection that no longer appear in the live
// worklist (executed/dropped). Returns the pruned array.
window.pruneChecked = function (validIds) {
  try {
    var current = window.loadChecked();
    var valid = {};
    (validIds || []).forEach(function (id) { valid[id] = true; });
    var pruned = current.filter(function (id) { return valid[id]; });
    if (pruned.length !== current.length) {
      window.saveChecked(pruned);
    }
    return pruned;
  } catch (e) { return []; }
};
// Route external (http/https/file) anchor clicks through openExternal so
// Markdown links and any other <a> tags open in the system browser
// instead of trying to navigate the Tauri WebView (which 404s). Capture
// phase so we run before XMLUI's Markdown-internal click handlers.
//
// Also routes relative *.md anchors (the MEMORY.md cross-references like
// `[foo.md](memory/foo.md)`) to a callback installed via
// registerContextMemorySelector below. We can't intercept these from
// XMLUI's onClick — the event handler cache deep-clones args, so the DOM
// target / preventDefault are gone by the time the XMLUI expression runs.
// And we can't install the window callback from XMLUI either — the
// scripting engine doesn't expose `window`.
var __contextMemorySelector = null;
window.registerContextMemorySelector = function (fn) {
  __contextMemorySelector = typeof fn === "function" ? fn : null;
};
window.clearContextMemorySelector = function () {
  __contextMemorySelector = null;
};
document.addEventListener("click", function (e) {
  var a = e.target && e.target.closest && e.target.closest("a");
  if (!a) return;
  var href = a.getAttribute("href");
  if (!href) return;
  if (/^(https?|file):/i.test(href)) {
    e.preventDefault();
    e.stopPropagation();
    window.openExternal(href);
    return;
  }
  if (href.indexOf("://") === -1 && /\.md(?:[?#].*)?$/i.test(href)) {
    if (typeof __contextMemorySelector === "function") {
      e.preventDefault();
      e.stopPropagation();
      var m = href.match(/([^\/?#]+\.md)(?:[?#]|$)/i);
      var basename = m ? m[1] : "";
      try {
        __contextMemorySelector(basename);
      } catch (err) {
        logToHost({ kind: "memory-link-error", error: String(err && err.message || err) });
      }
    }
  }
}, true);
function _refreshScrollables() {
  var nodes = document.querySelectorAll("*");
  var found = [];
  for (var i = 0; i < nodes.length; i += 1) {
    var el = nodes[i];
    if (el && el.scrollHeight > el.clientHeight + 8) {
      found.push(el);
    }
  }
  window._scrollables = found;
  return found;
}

// Click-driven; scan the DOM per call. _scrollables cache is for the
// RAF loop in scrollAfterDomUpdate, not here — it would poison
// after the first call if the DOM happened to have no scrollables
// at that moment ([] is truthy, so the || fallback would never fire).
window.scrollAllToTop = function () {
  var root = document.scrollingElement || document.documentElement || document.body;
  if (root) {
    window.scrollTo({ top: 0, behavior: "smooth" });
  }
  var nodes = document.querySelectorAll("*");
  for (var i = 0; i < nodes.length; i += 1) {
    var el = nodes[i];
    if (!el) continue;
    if (el.scrollHeight > el.clientHeight + 8) {
      try {
        el.scrollTo({ top: 0, behavior: "smooth" });
      } catch (e) {
        el.scrollTop = 0;
      }
    }
  }
};
window.scrollAllToBottom = function () {
  var root = document.scrollingElement || document.documentElement || document.body;
  if (root) {
    window.scrollTo({ top: root.scrollHeight, behavior: "smooth" });
  }
  var nodes = document.querySelectorAll("*");
  for (var j = 0; j < nodes.length; j += 1) {
    var sc = nodes[j];
    if (!sc) continue;
    if (sc.scrollHeight > sc.clientHeight + 8) {
      try {
        sc.scrollTo({ top: sc.scrollHeight, behavior: "smooth" });
      } catch (e) {
        sc.scrollTop = sc.scrollHeight;
      }
    }
  }
};
function getTauriInvoke() {
  try {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === "function") {
      return window.__TAURI__.core.invoke.bind(window.__TAURI__.core);
    }
  } catch (e) {}
  try {
    if (window.parent && window.parent.__TAURI__ && window.parent.__TAURI__.core && typeof window.parent.__TAURI__.core.invoke === "function") {
      return window.parent.__TAURI__.core.invoke.bind(window.parent.__TAURI__.core);
    }
  } catch (e) {}
  try {
    if (window.top && window.top.__TAURI__ && window.top.__TAURI__.core && typeof window.top.__TAURI__.core.invoke === "function") {
      return window.top.__TAURI__.core.invoke.bind(window.top.__TAURI__.core);
    }
  } catch (e) {}
  return null;
}
window.addEventListener("message", async (event) => {
  var data = event.data;
  if (!data || data.type !== "inspector-export") return;
  var source = event.source;

  function reply(payload) {
    if (source && typeof source.postMessage === "function") {
      source.postMessage(payload, "*");
    }
  }

  var invoke = getTauriInvoke();
  if (!invoke) {
    reply({ type: "inspector-export-result", ok: false, error: "Tauri IPC unavailable" });
    return;
  }
  try {
    var path = await invoke("save_trace_export", {
      filename: String(data.filename || "xs-trace.json"),
      content: String(data.content || ""),
      mimeType: String(data.mimeType || "application/octet-stream")
    });
    reply({ type: "inspector-export-result", ok: true, path: path });
  } catch (e) {
    logToHost({
      kind: "trace-export-direct-failed",
      error: String((e && e.message) || e),
      at: new Date().toISOString(),
    });
    reply({ type: "inspector-export-result", ok: false, error: String((e && e.message) || e) });
  }
});

// Adjustable root font-size for the xmlui surface (mirrors the terminal-side
// pattern in app/main.js). Buttons in AppHeader call setAppFontSize /
// getAppFontSize. The right pane and the agent tools drawer share origin
// and localStorage; a BroadcastChannel keeps their runtime sizes in lockstep.
(function () {
  var APP_FONT_KEY = "xmlui-desktop.app.fontSize";
  var APP_FONT_MIN = 10;
  var APP_FONT_MAX = 28;
  var APP_FONT_DEFAULT = 16;

  function clampAppFontSize(n) {
    var v = Math.round(Number(n) || 0);
    if (v < APP_FONT_MIN) v = APP_FONT_MIN;
    if (v > APP_FONT_MAX) v = APP_FONT_MAX;
    return v;
  }

  function applyFontSize(size) {
    try {
      document.documentElement.style.fontSize = size + "px";
    } catch (e) {}
  }

  var bc = null;
  try {
    bc = new BroadcastChannel(APP_FONT_KEY);
    bc.onmessage = function (ev) {
      if (!ev || !ev.data) return;
      applyFontSize(clampAppFontSize(ev.data.size));
    };
  } catch (e) {}

  window.getAppFontSize = function () {
    try {
      var raw = parseInt(localStorage.getItem(APP_FONT_KEY) || "", 10);
      return isFinite(raw) ? clampAppFontSize(raw) : APP_FONT_DEFAULT;
    } catch (e) {
      return APP_FONT_DEFAULT;
    }
  };

  window.setAppFontSize = function (n) {
    var size = clampAppFontSize(n);
    applyFontSize(size);
    try {
      localStorage.setItem(APP_FONT_KEY, String(size));
    } catch (e) {}
    if (bc) {
      try { bc.postMessage({ size: size }); } catch (e) {}
    }
    return size;
  };

  window.resetAppFontSize = function () {
    return window.setAppFontSize(APP_FONT_DEFAULT);
  };

  applyFontSize(window.getAppFontSize());
})();

// Pin-to-bottom tracking for transcript-style views (Talk, etc).
// _talkPinned reflects whether the user is currently within ~100px of
// the bottom of the page. Auto-scroll-on-new-turn handlers consult
// wasPinnedToBottom() before scrolling, so the user is never yanked
// down while re-reading earlier content.
(function () {
  var PIN_THRESHOLD = 100;
  window._talkPinned = true;
  function recompute() {
    var root = document.scrollingElement || document.documentElement || document.body;
    if (!root) return;
    var dist = root.scrollHeight - root.scrollTop - root.clientHeight;
    window._talkPinned = dist < PIN_THRESHOLD;
  }
  window.addEventListener("scroll", recompute, { passive: true });
  window.wasPinnedToBottom = function () {
    return window._talkPinned !== false;
  };
  // Used by Talk's auto-expand-latest-edit hook. The expansion grows
  // the DOM after the scroll-to-bottom listener has already run, so we
  // capture the pre-expand pin state and re-scroll multiple times as
  // the layout settles (two RAFs aren't enough — XMLUI renders the
  // expanded diff over several frames). Instant scrolls — smooth ones
  // overlap and fight each other.
  window.scrollAfterDomUpdate = function () {
    var wasPinned = window.wasPinnedToBottom();
    if (!wasPinned) return;
    // Pin to bottom every animation frame for ~1.5s — long enough for
    // XMLUI to finish rendering the expanded diff content, which can
    // span many frames. Each pin is instant (no smooth animation that
    // would fight a still-growing layout). The scrollable element list
    // is cached; doing querySelectorAll('*') per RAF stalled the main
    // thread on transcripts with many Markdown turns.
    var deadline = Date.now() + 1500;
    var t0 = (typeof performance !== 'undefined') ? performance.now() : 0;
    var frames = 0;
    var list = window._scrollables || (typeof _refreshScrollables === 'function' ? _refreshScrollables() : []);
    function pin() {
      frames += 1;
      var root = document.scrollingElement || document.documentElement || document.body;
      if (root) root.scrollTop = root.scrollHeight;
      for (var i = 0; i < list.length; i += 1) {
        var el = list[i];
        if (el && el.scrollHeight > el.clientHeight + 8) {
          try { el.scrollTop = el.scrollHeight; } catch (e) {}
        }
      }
    }
    function loop() {
      pin();
      if (Date.now() < deadline) {
        requestAnimationFrame(loop);
      } else if (t0) {
        try {
          window.logToHost({
            kind: 'scrollAfterDomUpdate',
            frames: frames,
            ms: Math.round(performance.now() - t0),
          });
        } catch (e) {}
      }
    }
    loop();
  };
})();

// Surface JS errors and lifecycle events to the host log channel.
window.addEventListener("error", (e) => {
  logToHost({
    kind: "error",
    message: e.message,
    source: e.filename,
    lineno: e.lineno,
    colno: e.colno,
    stack: e.error && e.error.stack,
    at: new Date().toISOString(),
  });
});
window.addEventListener("unhandledrejection", (e) => {
  logToHost({
    kind: "unhandledrejection",
    reason: String(e.reason),
    stack: e.reason && e.reason.stack,
    at: new Date().toISOString(),
  });
});
