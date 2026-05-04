// Shell-side helpers exposed to any XMLUI app served by the xmlui-desktop
// binary. Include from your project's index.html with:
//
//   <script src="xmlui://localhost/__shell/helpers.js"></script>
//
// The helpers all communicate with the parent shell via window.parent
// .postMessage and the matching dispatcher in app/main.js. Tauri's IPC
// (window.__TAURI__.core.invoke) is used directly when reachable.

window._xsLogs = window._xsLogs || [];

// Three intents the right pane can send to the parent shell:
//   to-shell      → inject text into the PTY (foreground process reads it as user input)
//   log           → record in cargo run stderr only, don't bother the shell
//   open-devtools → internal command (handled by parent.invoke)
window.toShell = function (text) {
  window.parent.postMessage(
    { type: "right-pane", kind: "to-shell", text: String(text) },
    "*",
  );
};
window.toTurn = function (text) {
  window.parent.postMessage(
    { type: "right-pane", kind: "to-turn", text: String(text) },
    "*",
  );
};
window.logToHost = function (payload) {
  window.parent.postMessage(
    { type: "right-pane", kind: "log", payload },
    "*",
  );
};
window.openExternal = function (url) {
  window.parent.postMessage(
    { type: "right-pane", kind: "open-url", url: String(url) },
    "*",
  );
};
// Push local commits to origin and refetch a DataSource (typically
// the commits list) when the push completes, so the pushed flags
// refresh without a manual reload.
var _gitPushPending = null;
window.gitPush = function (commitsDs) {
  var invoke = getTauriInvoke();
  if (invoke) {
    invoke("git_push", {})
      .then(function () {
        if (commitsDs && typeof commitsDs.refetch === "function") {
          commitsDs.refetch();
        }
      })
      .catch(function (e) {
        window.logToHost({ kind: "git-push", phase: "err", error: String(e) });
      });
    return;
  }
  // No direct invoke (cross-origin iframe). Round-trip via the parent
  // shell; the parent posts back a "git-push-result" we listen for.
  _gitPushPending = commitsDs;
  window.parent.postMessage(
    { type: "right-pane", kind: "git-push" },
    "*"
  );
};
window.addEventListener("message", function (event) {
  var data = event.data;
  if (!data || data.type !== "git-push-result") return;
  if (_gitPushPending && typeof _gitPushPending.refetch === "function") {
    _gitPushPending.refetch();
  }
  _gitPushPending = null;
});
// In-flight marker that persists across iframe reloads. At click
// time we snapshot the current proposal's item IDs; the XMLUI side
// clears the flag whenever proposal.json's items differ from that
// snapshot — works on the initial fetch too (so refresh recovers
// from a stale flag), not just on refetches.
window.markInflight = function (items) {
  try {
    var sig = (items || [])
      .filter(function (i) { return i && i.id; })
      .map(function (i) { return i.id + ":" + (i.status || "applied"); })
      .sort()
      .join(",");
    localStorage.setItem("inflight", JSON.stringify({ ids: sig }));
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
// Route external (http/https/file) anchor clicks through openExternal so
// Markdown links and any other <a> tags open in the system browser
// instead of trying to navigate the Tauri WebView (which 404s). Capture
// phase so we run before XMLUI's Markdown-internal click handlers.
document.addEventListener("click", function (e) {
  var a = e.target && e.target.closest && e.target.closest("a");
  if (!a) return;
  var href = a.getAttribute("href");
  if (!href) return;
  if (/^(https?|file):/i.test(href)) {
    e.preventDefault();
    e.stopPropagation();
    window.openExternal(href);
  }
}, true);
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
  for (var i = 0; i < nodes.length; i += 1) {
    var el = nodes[i];
    if (!el) continue;
    if (el.scrollHeight > el.clientHeight + 8) {
      try {
        el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
      } catch (e) {
        el.scrollTop = el.scrollHeight;
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
  var requestId = "trace-export-" + Date.now() + "-" + Math.random().toString(36).slice(2);
  var source = event.source;

  function reply(payload) {
    if (source && typeof source.postMessage === "function") {
      source.postMessage(payload, "*");
    }
  }

  function onResult(resultEvent) {
    var result = resultEvent.data;
    if (!result || result.type !== "save-trace-export-result" || result.requestId !== requestId) {
      return;
    }
    window.removeEventListener("message", onResult);
    reply(
      result.ok
        ? { type: "inspector-export-result", ok: true, path: result.path }
        : { type: "inspector-export-result", ok: false, error: result.error }
    );
  }

  var invoke = getTauriInvoke();
  if (invoke) {
    try {
      var path = await invoke("save_trace_export", {
        filename: String(data.filename || "xs-trace.json"),
        content: String(data.content || ""),
        mimeType: String(data.mimeType || "application/octet-stream")
      });
      reply({ type: "inspector-export-result", ok: true, path: path });
      return;
    } catch (e) {
      logToHost({
        kind: "trace-export-direct-failed",
        error: String((e && e.message) || e),
        at: new Date().toISOString(),
      });
    }
  }

  window.addEventListener("message", onResult);
  window.parent.postMessage(
    {
      type: "right-pane",
      kind: "save-trace-export",
      requestId: requestId,
      filename: String(data.filename || "xs-trace.json"),
      content: String(data.content || ""),
      mimeType: String(data.mimeType || "application/octet-stream")
    },
    "*"
  );
});

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
