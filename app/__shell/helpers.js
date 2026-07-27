// Shell-side helpers exposed to any XMLUI app served by Bram
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

// ResizeObserver flood detector (diagnostic, #150 startup unresponsiveness).
// The browser logs "ResizeObserver loop completed with undelivered
// notifications" but names no culprit. We wrap the constructor (class extends
// so observe/disconnect/instanceof keep working natively; we only intercept
// the callback to count fires) and, once per second while the global fire rate
// exceeds a flood threshold, log to bram-trace WHICH element(s) are looping.
// Loaded before xmlui-standalone (tools/index.html), so it wraps every XMLUI
// Splitter/layout observer. Remove once the flood source is identified.
(function installResizeObserverFloodDetector() {
  var Native = window.ResizeObserver;
  if (!Native || Native.__bramFloodWrapped) return;
  var FLOOD_PER_SEC = 50;
  var RING_MAX = 60;
  var total = 0;
  var counts = Object.create(null);
  // Identity + geometry ring (ro-flood-identity-ring-buffer): the flood
  // line alone cannot distinguish (a) one row oscillating between two
  // heights, (b) many rows re-measuring under container size churn, or
  // (c) rows remounting — observe() fires one initial notification per
  // new element, so a remount loop floods the counter with zero real
  // resizes. Each fire records element identity (data-index, seen-before)
  // and contentRect geometry; the detail line dumps the ring when the
  // flood threshold trips.
  var seen = typeof WeakSet === "function" ? new WeakSet() : null;
  var newElements = 0;
  var repeatFires = 0;
  var ring = [];
  var lastFireMs = 0;
  // Sync in-callback dump (ro-flood-sync-dump): the interval dump below
  // requires the main thread to yield, so a non-converging RO loop — the
  // terminal-freeze variant, e.g. 2026-07-11T17:07 (last iframe line, then
  // 53 min of silence) — dies without testifying. firesSinceTick is reset
  // by every interval tick; if it reaches SYNC_BURST_FIRES the thread has
  // NOT yielded through a whole flooding second, and we emit the detail
  // line directly from the callback via logToHost → invoke, whose IPC
  // dispatch the host logs even if the iframe never yields again (the
  // describe-patch precedent).
  var SYNC_BURST_FIRES = 120;
  var SYNC_MIN_GAP_MS = 2000;
  var firesSinceTick = 0;
  var lastSyncDumpMs = 0;
  function describe(el) {
    try {
      if (!el || el.nodeType !== 1) return String(el);
      var id = el.id ? "#" + el.id : "";
      var cls = (typeof el.className === "string" && el.className.trim())
        ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".")
        : "";
      return el.tagName.toLowerCase() + id + cls;
    } catch (e) { return "?"; }
  }
  function shortKey(k) {
    if (k.indexOf("._row_") >= 0) return "row";
    if (k.indexOf("._mainContentArea_") >= 0) return "main";
    if (k.indexOf("html") === 0) return "html";
    return k.slice(0, 24);
  }
  function encodeRing() {
    var enc = ring.map(function (e) {
      return "+" + e[0] + " " + e[1] + (e[2] != null ? "#" + e[2] : "") +
        " " + e[3] + "x" + e[4] + (e[5] ? "*" : "");
    }).join(";");
    ring = [];
    return enc;
  }
  function emitDetail(via, newN, repN, enc) {
    if (typeof window.__bramIframeTrace !== "function") return;
    var detail = {
      context: "iframe", via: via, newElements: newN, repeatFires: repN,
      ring1: enc.slice(0, 480),
    };
    if (enc.length > 480) detail.ring2 = enc.slice(480, 960);
    if (enc.length > 960) detail.ring3 = enc.slice(960, 1440);
    if (enc.length > 1440) detail.ring4 = enc.slice(1440, 1920);
    window.__bramIframeTrace("resizeobserver-flood-detail", detail);
  }
  var Wrapped = class extends Native {
    constructor(cb) {
      super(function (entries, observer) {
        total += entries.length || 1;
        var now = Math.round(performance.now());
        for (var i = 0; i < entries.length; i++) {
          var el = entries[i] && entries[i].target;
          var k = describe(el);
          counts[k] = (counts[k] || 0) + 1;
          var isNew = false;
          if (seen && el && el.nodeType === 1) {
            if (seen.has(el)) { repeatFires++; }
            else { seen.add(el); isNew = true; newElements++; }
          }
          var r = entries[i] && entries[i].contentRect;
          ring.push([
            lastFireMs ? now - lastFireMs : 0,
            shortKey(k),
            el && el.getAttribute ? el.getAttribute("data-index") : null,
            r ? Math.round(r.width * 10) / 10 : null,
            r ? Math.round(r.height * 10) / 10 : null,
            isNew,
          ]);
          lastFireMs = now;
          if (ring.length > RING_MAX) ring.shift();
          firesSinceTick++;
          if (firesSinceTick >= SYNC_BURST_FIRES && now - lastSyncDumpMs >= SYNC_MIN_GAP_MS) {
            firesSinceTick = 0;
            lastSyncDumpMs = now;
            var newN = 0;
            for (var j = 0; j < ring.length; j++) if (ring[j][5]) newN++;
            emitDetail("sync", newN, ring.length - newN, encodeRing());
          }
        }
        return cb.call(this, entries, observer);
      });
    }
  };
  Wrapped.__bramFloodWrapped = true;
  window.ResizeObserver = Wrapped;
  setInterval(function () {
    var t = total; total = 0;
    // Stash the per-second RO fire rate (every second, flood or not) so the
    // heartbeat-batch line can pair it with drift — RO-rate ↔ drift in one grep.
    window.__bramRoFiresPerSec = t;
    var snap = counts; counts = Object.create(null);
    var newN = newElements; newElements = 0;
    var repN = repeatFires; repeatFires = 0;
    // Every tick proves the thread yielded: reset the sync-dump burst
    // counter so the in-callback dump fires only when a whole flooding
    // second passes without this tick running (i.e., a hard freeze).
    firesSinceTick = 0;
    if (t < FLOOD_PER_SEC) return;
    var top = Object.keys(snap)
      .map(function (k) { return [k, snap[k]]; })
      .sort(function (a, b) { return b[1] - a[1]; })
      .slice(0, 6)
      .map(function (p) { return p[0] + "=" + p[1]; });
    // One entry per fire: "+dt key#idx WxH*" — dt is ms since the prior
    // fire, #idx is the element's data-index when present (XMLUI's List
    // Item sets it), trailing * marks a first-ever observation (mount).
    // Encoded as chunked strings because the trace serializer summarizes
    // arrays to 3 samples and truncates strings at 500 chars
    // (__bramTraceSafeValue).
    if (typeof window.__bramIframeTrace === "function") {
      window.__bramIframeTrace("resizeobserver-flood", {
        context: "iframe", firesPerSec: t, top: top,
      });
      emitDetail("interval", newN, repN, encodeRing());
    }
  }, 1000);
})();

// Input-latency probe (diagnostic, #150 startup unresponsiveness). The other
// instruments measure iframe-JS steady state; this measures the actual
// symptom — input responsiveness. Capture-phase pointerdown/keydown stamp a
// time; the next animation frame measures how long the main thread took to
// come back. A gap > threshold means input was starved (JS saturation or
// render jank). hadFocus is logged too, since "needs a double-click after a
// reload" is often a focus artifact, not saturation. Remove once the #150
// responsiveness cause is identified.
(function installInputLatencyProbe() {
  if (window.__bramInputLatencyProbe) return;
  window.__bramInputLatencyProbe = true;
  var THRESHOLD_MS = 200;
  var lastLog = 0;
  function describe(el) {
    try {
      if (!el || el.nodeType !== 1) return String(el);
      var id = el.id ? "#" + el.id : "";
      var cls = (typeof el.className === "string" && el.className.trim())
        ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".")
        : "";
      return el.tagName.toLowerCase() + id + cls;
    } catch (e) { return "?"; }
  }
  function onInput(ev) {
    var t0 = performance.now();
    var type = ev.type;
    var hadFocus = false;
    try { hadFocus = document.hasFocus(); } catch (e) {}
    var tgt = describe(ev.target);
    requestAnimationFrame(function () {
      var dt = performance.now() - t0;
      if (dt < THRESHOLD_MS) return;
      if (t0 - lastLog < THRESHOLD_MS) return;
      lastLog = t0;
      if (typeof window.__bramIframeTrace === "function") {
        window.__bramIframeTrace("input-latency", {
          context: "iframe", event: type, latencyMs: Math.round(dt),
          hadFocus: hadFocus, target: tgt,
        });
      }
    });
  }
  document.addEventListener("pointerdown", onInput, true);
  document.addEventListener("keydown", onInput, true);
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
  if (window.location.pathname.indexOf("/tools/") === -1) return;
  var key = "bram.tools.route";
  var legacyKey = "xmlui-desktop.tools.route";
  var bootedAt = Date.now();
  var STARTUP_SUPPRESS_MS = 1500;
  function trace(subkind, fields) {
    setTimeout(function () {
      try {
        if (typeof window.logToHost !== "function") return;
        var payload = {
          kind: "iframe-trace",
          subkind: subkind,
          at: new Date().toISOString(),
        };
        if (fields && typeof fields === "object") {
          Object.assign(payload, fields);
        }
        window.logToHost(payload);
      } catch (e) {}
    }, 0);
  }
  try {
    var current = window.location.hash;
    var saved = localStorage.getItem(key) || localStorage.getItem(legacyKey) || "";
    trace("tools-route-boot", {
      current: current || "",
      saved: saved,
      pathname: window.location.pathname || "",
    });
    if (!current || current === "#/") {
      if (saved && saved !== "#/") {
        window.location.hash = saved;
        trace("tools-route-restore", {
          from: current || "",
          route: saved,
        });
      }
    }
    // react-router-dom uses history.pushState which doesn't fire
    // hashchange, so poll instead of listening.
    setInterval(function () {
      var h = window.location.hash;
      var stored = localStorage.getItem(key) || "";
      if (
        h === "#/" &&
        stored &&
        stored !== "#/" &&
        Date.now() - bootedAt < STARTUP_SUPPRESS_MS
      ) {
        trace("tools-route-skip-root-save", {
          stored: stored,
          elapsedMs: Date.now() - bootedAt,
        });
        return;
      }
      if (h && h !== localStorage.getItem(key)) {
        localStorage.setItem(key, h);
        trace("tools-route-save", {
          route: h,
          previous: stored,
          elapsedMs: Date.now() - bootedAt,
        });
      }
    }, 500);
  } catch (e) {}
})();

// Main-thread heartbeat for the drawer iframe. setInterval scheduled
// every 200ms; if the actual gap exceeds the threshold the main thread
// was blocked (typically by heavy Markdown re-renders during JSONL
// cascade — the same condition that delays the inflightClaim
// DataSource's onLoaded handler). Emits one record per blockage with
// drift_ms, so a swallowed click can be correlated with main-thread
// busyness in bram-trace.log. Scoped to the drawer because that's
// where worklist clicks live; the right pane is a separate iframe with
// its own load profile.
(function heartbeat() {
  if (window.location.pathname.indexOf("/tools/") === -1) return;
  setTimeout(function () {
    try {
      window.logToHost && window.logToHost({
        kind: "iframe-trace",
        subkind: "helpers-js-loaded",
        build: "batch-v2",
        at: new Date().toISOString(),
      });
    } catch (e) {}
  }, 500);
  var TICK_MS = 200;
  // Threshold is configurable via appGlobals.heartbeatDriftThresholdMs
  // (see config.json). Defaults to 500ms when unset. Lower values
  // catch sub-second blockages at the cost of more records during
  // normal hot-render bursts.
  var DRIFT_THRESHOLD_MS =
    (window.appGlobals && Number(window.appGlobals.heartbeatDriftThresholdMs)) || 500;
  var last = performance.now();
  var batch = { fires: 0, sumDrift: 0, maxDrift: 0, spikes: 0, sinceMs: 0, bgFires: 0 };
  // Batch summary every 50 fires (~10s nominal). Emits aggregate
  // drift stats so we can see overall main-thread health independent
  // of individual spike records.
  function batchTick(drift, bg) {
    if (batch.fires === 0) batch.sinceMs = Date.now();
    batch.fires += 1;
    if (bg) batch.bgFires += 1;
    batch.sumDrift += drift;
    if (drift > batch.maxDrift) batch.maxDrift = drift;
    if (drift >= DRIFT_THRESHOLD_MS) batch.spikes += 1;
    if (batch.fires >= 50) {
      // Gate: skip the emit while a PTY menu is pending.
      // window.__bramMenuPending mirrors bramAgentMenu (set by
      // Globals.xs applyAgentMenu). Reset still runs so a fresh
      // window starts post-dismiss.
      if (!window.__bramMenuPending) {
        try {
          window.logToHost({
            kind: "iframe-trace",
            subkind: "heartbeat-batch",
            fires: batch.fires,
            spanMs: Date.now() - batch.sinceMs,
            sumDriftMs: Math.round(batch.sumDrift),
            avgDriftMs: Math.round(batch.sumDrift / batch.fires),
            maxDriftMs: Math.round(batch.maxDrift),
            spikes: batch.spikes,
            bgFires: batch.bgFires,
            roFiresPerSec: window.__bramRoFiresPerSec || 0,
            at: new Date().toISOString(),
          });
        } catch (e) {}
      }
      batch = { fires: 0, sumDrift: 0, maxDrift: 0, spikes: 0, sinceMs: 0 };
    }
  }
  setInterval(function () {
    var now = performance.now();
    var drift = now - last - TICK_MS;
    last = now;
    // Focus/visibility at this tick. Browsers throttle setInterval to ~1s
    // when the window is hidden/unfocused, so drift then reads ~800ms
    // (1000 - TICK_MS) even though the main thread is idle — a throttle
    // artifact, not lag. Stamp each record with hidden/focused so a
    // backgrounded window is distinguishable from real saturation, and
    // count backgrounded fires per batch (bgFires): a high maxDrift with
    // bgFires≈fires is throttling; with bgFires≈0 it is genuine.
    var hidden = typeof document !== "undefined" && document.hidden === true;
    var focused =
      typeof document === "undefined" || typeof document.hasFocus !== "function"
        ? true
        : document.hasFocus();
    var bg = hidden || !focused;
    batchTick(drift, bg);
    if (drift >= DRIFT_THRESHOLD_MS && !window.__bramMenuPending) {
      try {
        window.logToHost({
          kind: "iframe-trace",
          subkind: "heartbeat-drift",
          drift_ms: Math.round(drift),
          hidden: hidden,
          focused: focused,
          at: new Date().toISOString(),
        });
      } catch (e) {}
    }
    // WebKit has no Long Tasks API, so the PerformanceObserver('longtask')
    // below records nothing. The heartbeat IS the working stall source: a
    // foreground tick late by >=200ms means the main thread was blocked that
    // long. Emit the long-task signal from here (source:"heartbeat"), gated to
    // the foreground so setInterval's ~1s background throttle isn't misread as
    // a stall. Overlaps heartbeat-drift (>=500ms) intentionally — this widens
    // coverage down to 200ms.
    if (drift >= 200 && !bg && !window.__bramMenuPending) {
      window.__bramIframeTrace("long-task", {
        ms: Math.round(drift),
        name: "heartbeat",
        source: "heartbeat",
      });
    }
  }, TICK_MS);
})();

// Capture-phase click listener on `document` for the drawer iframe.
// Fires for EVERY click that reaches the DOM, BEFORE XMLUI's own
// onClick handlers. Distinguishes "click reached document but XMLUI's
// onClick didn't run" from "click never registered at all" — the
// former produces a `dom-click` record without a matching XMLUI
// `subkind=click`, pointing at button-disabled/re-rendered/dead-space
// failure modes that helpers.js can't otherwise detect. Capture phase
// (true) ensures this runs before bubbling-phase handlers.
(function captureClicks() {
  if (window.location.pathname.indexOf("/tools/") === -1) return;
  document.addEventListener("click", function (e) {
    try {
      var t = e.target;
      var tagName = t && t.tagName;
      var ariaLabel = (t && t.getAttribute && t.getAttribute("aria-label")) || "";
      var role = (t && t.getAttribute && t.getAttribute("role")) || "";
      var disabled = !!(t && t.disabled);
      window.logToHost({
        kind: "iframe-trace",
        subkind: "dom-click",
        tagName: String(tagName || ""),
        ariaLabel: String(ariaLabel),
        role: String(role),
        disabled: disabled,
        x: e.clientX,
        y: e.clientY,
        at: new Date().toISOString(),
      });
    } catch (le) {}
  }, true);
})();

// Outbound right-pane → PTY intents route through `queue_pty_intent`
// (#86), which appends to `resources/.pty-intent.jsonl` and drains
// under a process-wide mutex. The disk hop keeps each click durably
// recorded even if the iframe context is unsettled when the IPC fires
// — the host drains independently of the originating iframe state.
//
// `toShell` / `toTurn` / `sendKeys` keep their application-level
// responsibilities (whitespace normalization in `toTurn`, the
// implicit "\n" semantic in `toShell`, the "no framing" contract in
// `sendKeys`); PTY framing (bracketed-paste markers around toTurn
// data, trailing newline for toShell) is applied host-side in the
// drain so the right pane stays ignorant of terminal escape
// sequences.
// Write per-item feedback to resources/feedback-drafts/<feedbackId>.md
// without going through the PTY paste channel. toTurn collapses every
// whitespace run into a single space (line 227) and the receiving TUI's
// bracketed-paste buffer has its own content limits, so long Iterate
// feedback can lose structure or get truncated. Iterate now writes the
// feedback to disk via this helper and sends only a small feedbackRef
// in the toTurn payload; the agent reads the draft file directly. See
// #144.
window.queueFeedbackDraft = function (feedbackId, text) {
  var id = String(feedbackId || "");
  var s = String(text == null ? "" : text);
  // stage=source: what the iframe got from the textbox. stage=sink:
  // what was passed to the invoke. Identical lengths confirm no
  // client-side mangling; a delta points at iframe-side regression.
  try {
    window.logToHost({
      kind: "iframe-trace",
      subkind: "feedback-draft-write",
      stage: "source",
      feedback_id: id,
      source_bytes: s.length,
      at: new Date().toISOString(),
    });
  } catch (e) {}
  var invoke = getTauriInvoke();
  if (!invoke) return Promise.resolve(false);
  try {
    invoke("log_from_right_pane", {
      payload: {
        kind: "iframe-trace",
        subkind: "feedback-draft-write",
        stage: "sink",
        feedback_id: id,
        sink_bytes: s.length,
        at: new Date().toISOString(),
      },
    }).catch(function () {});
  } catch (e) {}
  return invoke("queue_feedback_draft", { payload: { feedback_id: id, text: s } })
    .then(function () {
      return true;
    })
    .catch(function (e) {
      console.error("queueFeedbackDraft invoke", e);
      try {
        window.logToHost({
          kind: "iframe-trace",
          subkind: "feedback-draft-write-failed",
          feedback_id: id,
          error: String((e && e.message) || e),
          at: new Date().toISOString(),
        });
      } catch (le) {}
      return false;
    });
};

window.sendIterateWithFeedbackDraft = function (items, selectedId, text) {
  var feedbackId = Date.now() + "-" + selectedId;
  window.queueFeedbackDraft(feedbackId, text).then(function (wroteDraft) {
    window.toTurn("iterate: " + JSON.stringify({
      items: (items || []).filter(function (i) { return i.id === selectedId; })
        .map(function (i) {
          return wroteDraft
            ? { id: i.id, feedbackRef: feedbackId }
            : { id: i.id, feedback: text };
        }),
    }));
  });
};

// issue-221-skill-launcher: build and submit a `/skill args` turn from the
// Skills launcher — straight to the agent via toTurn. One trace line per launch.
window.__bramRunSkill = function (name, argsRaw) {
  if (!name) return;
  var args = String(argsRaw || "").trim();
  var cmd = args ? "/" + name + " " + args : "/" + name;
  try {
    window.logToHost({
      kind: "iframe-trace",
      subkind: "skill-invoke",
      name: name,
      args_len: args.length,
      at: new Date().toISOString(),
    });
  } catch (e) {}
  window.toTurn(cmd);
};

window.toShell = function (text) {
  var s = String(text);
  // Trace the entry so #86's "click swallowed" diagnostic flow can
  // distinguish between "helper never invoked" (no trace line) and
  // "helper invoked but queue / drain lost" (trace line present but
  // no [pty-intent] op=enqueue follows). kind: "iframe-trace" routes
  // through log_from_right_pane's iframe-trace branch into the
  // [iframe] category of resources/bram-traces/bram-trace.log.
  try {
    window.logToHost({
      kind: "iframe-trace",
      subkind: "to-shell",
      stage: "source",
      textLength: s.length,
      textPreview: s.slice(0, 80),
      at: new Date().toISOString(),
    });
  } catch (e) {}
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("queue_pty_intent", { payload: { kind: "toShell", data: s } }).catch(function (e) {
    console.error("toShell queue_pty_intent", e);
    try {
      window.logToHost({
        kind: "iframe-trace",
        subkind: "to-shell-invoke-failed",
        error: String((e && e.message) || e),
        at: new Date().toISOString(),
      });
    } catch (le) {}
  });
};
window.toTurn = function (text) {
  var s = String(text);
  try {
    window.logToHost({
      kind: "iframe-trace",
      subkind: "to-turn",
      stage: "source",
      textLength: s.length,
      textPreview: s.slice(0, 80),
      at: new Date().toISOString(),
    });
  } catch (e) {}
  // Send the text RAW. Per-transport normalization is the host's job now
  // (docs/turn-transport-redesign.md step 6): the host collapses whitespace
  // only for small inline sends, while substantial/image-bearing sends ride
  // a filesystem envelope with full fidelity — multiline text survives.
  var normalized = s;
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("log_from_right_pane", {
    payload: {
      kind: "iframe-trace",
      subkind: "to-turn",
      stage: "sink",
      textLength: normalized.length,
      textPreview: normalized.slice(0, 80),
      at: new Date().toISOString(),
    },
  }).catch(function () {});
  invoke("queue_pty_intent", { payload: { kind: "toTurn", data: normalized } }).catch(function (e) {
    console.error("toTurn queue_pty_intent", e);
    try {
      window.logToHost({
        kind: "iframe-trace",
        subkind: "to-turn-invoke-failed",
        error: String((e && e.message) || e),
        at: new Date().toISOString(),
      });
    } catch (le) {}
  });
};

window.recordWorklistActionAuthorization = function (payload) {
  var invoke = getTauriInvoke();
  if (!invoke || !payload) return Promise.resolve(false);
  return invoke("record_worklist_action_authorization", { payload: payload })
    .then(function () { return true; })
    .catch(function (e) {
      console.error("recordWorklistActionAuthorization invoke", e);
      try {
        window.logToHost({
          kind: "iframe-trace",
          subkind: "worklist-action-auth-failed",
          error: String((e && e.message) || e),
          at: new Date().toISOString(),
        });
      } catch (le) {}
      return false;
    });
};

window.submitAuthorizedWorklistTurn = function (result, onFailure) {
  result = result || {};
  var payload = result.authorizationPayload || null;
  var turnText = result.turnText || "";
  if (!payload) {
    if (turnText) window.toTurn(turnText);
    return;
  }
  window.recordWorklistActionAuthorization(payload).then(function (ok) {
    if (ok && turnText) window.toTurn(turnText);
    if (!ok && typeof onFailure === "function") onFailure();
  });
};
// sendKeys writes raw bytes to the PTY with NO trailing newline (unlike
// toShell which always appends \n). Use it for control sequences like ESC,
// arrow keys, or single-keypress menu shortcuts.
window.sendKeys = function (text) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("queue_pty_intent", { payload: { kind: "sendKeys", data: String(text) } }).catch(function (e) {
    console.error("sendKeys queue_pty_intent", e);
    try {
      window.logToHost({
        kind: "iframe-trace",
        subkind: "send-keys-invoke-failed",
        error: String((e && e.message) || e),
        at: new Date().toISOString(),
      });
    } catch (le) {}
  });
};
window.__bramAgentProviderFromSettings = function (agentSetting) {
  var s = String(agentSetting || "").toLowerCase();
  return s.indexOf("codex") >= 0 ? "codex" : "claude";
};
window.__bramAgentSwitcherOptions = function () {
  return [
    { value: "claude", label: "Claude" },
    { value: "codex", label: "Codex" },
  ];
};
window.__bramAgentSwitcherInitialProvider = function (agentSetting) {
  try {
    var saved = window.sessionStorage && sessionStorage.getItem("bram.agentSwitcher.provider");
    if (saved === "codex" || saved === "claude") return saved;
  } catch (e) {}
  return window.__bramAgentProviderFromSettings(agentSetting);
};
window.__bramAgentSwitcherTrace = function (stage, fields) {
  try {
    var payload = Object.assign({ stage: stage, at: new Date().toISOString() }, fields || {});
    window.__bramIframeTrace("agent-switcher", payload);
  } catch (e) {}
};
window.__bramInitAgentSwitcher = function (select, agentSetting, switching, setSelected) {
  var key = window.__bramAgentSwitcherInitialProvider(agentSetting);
  window.__bramAgentSwitcherTrace("settings-loaded", {
    provider: key,
    switching: !!switching,
    hasSelect: !!select,
    agentSetting: String(agentSetting || ""),
  });
  if (switching) return key;
  if (typeof setSelected === "function") setSelected(key);
  if (select && typeof select.setValue === "function") {
    select.setValue(key);
    window.__bramAgentSwitcherTrace("select-set-value", { provider: key });
  }
  return key;
};
window.__bramRememberAgentSwitcherProvider = function (provider) {
  var key = String(provider || "").toLowerCase() === "codex" ? "codex" : "claude";
  try {
    if (window.sessionStorage) sessionStorage.setItem("bram.agentSwitcher.provider", key);
  } catch (e) {}
  return key;
};
window.__bramAgentSwitcherLabel = function (provider) {
  return String(provider || "").toLowerCase() === "codex" ? "Codex" : "Claude";
};
window.__bramWithAgentCommandTimeout = function (promise, label) {
  var timeoutMs = 8000;
  var timeout = new Promise(function (_, reject) {
    setTimeout(function () {
      reject(new Error((label || "agent command") + " did not finish within " + (timeoutMs / 1000) + "s"));
    }, timeoutMs);
  });
  return Promise.race([promise, timeout]);
};
window.__bramSwitchAgent = function (provider) {
  var key = String(provider || "").toLowerCase() === "codex" ? "codex" : "claude";
  var invoke = getTauriInvoke();
  if (!invoke) return Promise.reject(new Error("Tauri IPC unavailable"));
  window.__bramAgentSwitcherTrace("invoke", { provider: key });
  return window.__bramWithAgentCommandTimeout(invoke("switch_agent", { provider: key }), "agent switch").then(function () {
    window.__bramAgentSwitcherTrace("sent", { provider: key });
    return key;
  }).catch(function (e) {
    window.__bramAgentSwitcherTrace("error", {
      provider: key,
      error: String((e && e.message) || e),
    });
    throw e;
  });
};
window.__bramHandleAgentSwitcherChange = function (next, previous, select, setSelected, setSwitching, toastApi) {
  var key = String(next || "").toLowerCase() === "codex" ? "codex" : (String(next || "").toLowerCase() === "claude" ? "claude" : "");
  var prev = String(previous || "").toLowerCase() === "codex" ? "codex" : "claude";
  window.__bramAgentSwitcherTrace("change", { next: key, previous: prev, hasSelect: !!select });
  if (!key || key === prev) return;
  if (typeof setSwitching === "function") setSwitching(true);
  if (typeof setSelected === "function") setSelected(key);
  window.__bramRememberAgentSwitcherProvider(key);
  window.__bramSwitchAgent(key).then(function () {
    window.__bramAgentSwitcherTrace("complete", { provider: key });
    if (typeof setSwitching === "function") setSwitching(false);
  }).catch(function (e) {
    window.__bramAgentSwitcherTrace("revert", {
      provider: key,
      previous: prev,
      error: String((e && e.message) || e),
    });
    window.__bramRememberAgentSwitcherProvider(prev);
    if (typeof setSelected === "function") setSelected(prev);
    if (select && typeof select.setValue === "function") select.setValue(prev);
    if (typeof setSwitching === "function") setSwitching(false);
    try {
      if (toastApi && typeof toastApi.error === "function") {
        toastApi.error("Could not switch agent: " + String((e && e.message) || e));
      }
    } catch (le) {}
  });
};
window.__bramReloadAgentSession = function (provider, sessionId) {
  var key = String(provider || "").toLowerCase() === "codex" ? "codex" : "claude";
  var id = String(sessionId || "");
  var invoke = getTauriInvoke();
  if (!invoke) return Promise.reject(new Error("Tauri IPC unavailable"));
  try {
    window.__bramIframeTrace("agent-reload", {
      stage: "invoke",
      provider: key,
      session: id,
      at: new Date().toISOString(),
    });
  } catch (e) {}
  return window.__bramWithAgentCommandTimeout(invoke("reload_agent_session", { provider: key, session: id }), "agent reload").then(function () {
    try {
      window.__bramIframeTrace("agent-reload", {
        stage: "sent",
        provider: key,
        session: id,
        at: new Date().toISOString(),
      });
    } catch (e) {}
    return key;
  }).catch(function (e) {
    try {
      window.__bramIframeTrace("agent-reload", {
        stage: "error",
        provider: key,
        session: id,
        error: String((e && e.message) || e),
        at: new Date().toISOString(),
      });
    } catch (le) {}
    throw e;
  });
};
// sessions-new-named-session: start a fresh session for the current provider,
// optionally naming it. The host kills+relaunches the agent without --continue
// and applies the name when the new session's JSONL surfaces.
window.__bramCreateNewSession = function (provider, title) {
  var key = String(provider || "").toLowerCase() === "codex" ? "codex" : "claude";
  var invoke = getTauriInvoke();
  if (!invoke) return Promise.reject(new Error("Tauri IPC unavailable"));
  return window.__bramWithAgentCommandTimeout(
    invoke("create_new_session", { provider: key, title: String(title || "") }),
    "new session"
  );
};
window.__bramCreateNewSessionClick = function (provider, name, toastApi) {
  if (typeof toastApi === "function") toastApi("Starting a new session…");
  window.__bramCreateNewSession(provider, name).catch(function (e) {
    if (toastApi && typeof toastApi.error === "function") {
      toastApi.error("Could not create session: " + String((e && e.message) || e));
    }
  });
};
window.__bramReloadAgentSessionClick = function (provider, sessionId, toastApi) {
  var key = String(provider || "").toLowerCase() === "codex" ? "codex" : "claude";
  var id = String(sessionId || "");
  try {
    window.__bramIframeTrace("agent-reload", {
      stage: "click",
      provider: key,
      session: id,
      at: new Date().toISOString(),
    });
  } catch (e) {}
  window.__bramReloadAgentSession(key, id).catch(function (e) {
    try {
      if (toastApi && typeof toastApi.error === "function") {
        toastApi.error("Could not reload session: " + String((e && e.message) || e));
      }
    } catch (le) {}
  });
  try {
    if (typeof toastApi === "function") toastApi("Reloading session - killing the running agent and resuming.");
  } catch (e) {}
};
// Pick the live session from a /__sessions/list payload: the entry flagged
// current, else the first returned. Shared by the Transcript header and the
// footer echo so both point at the same session.
window.__bramCurrentSessionOf = function (list) {
  var arr = Array.isArray(list) ? list : [];
  return arr.find(function (s) { return s && s.current; }) || arr[0] || null;
};
// One-line session metadata used at the top of the Transcript and echoed
// under the footer message box. Plain-JS date formatting (no formatDateTime
// dependency) keeps it usable from any surface. Returns "" for no session.
window.__bramSessionMetaLine = function (s) {
  if (!s) return "";
  var provider = String(s.provider || "").toUpperCase();
  var title = s.title || "(untitled)";
  var id = String(s.id || "");
  var shortId = id.length > 12 ? id.slice(0, 12) : (id || "unknown");
  var when = "";
  if (s.mtime) {
    var d = new Date(s.mtime * 1000);
    var pad = function (n) { return n < 10 ? "0" + n : "" + n; };
    when =
      d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) +
      " " + pad(d.getHours()) + ":" + pad(d.getMinutes());
  }
  var kb = Math.round((s.size || 0) / 1024) + " KB";
  var parts = [title, "id " + shortId];
  if (when) parts.push(when);
  parts.push(kb);
  return (provider ? provider + "  " : "") + parts.join("  ·  ");
};
window.recordToolbarPendingMenuFromEvent = function (event) {
  window.__bramToolbarMenuState = {
    present: !!(event && event.payload),
    atMs: Date.now(),
  };
};
window.getToolbarPendingMenuState = function () {
  return window.__bramToolbarMenuState || { present: false, atMs: 0 };
};
// Toolbar PTY subscribers. Invoked via xs delegators in Globals.xs.
//
// Originally migrated in commit d532432 step 5: the xs declarations
// were removed and Main.xmlui's bare-name calls were expected to
// resolve directly to `window.setToolbarPendingMenuFromEvent` etc.
// — that worked for the toolbar onClick handlers where the call is a
// top-level expression, but XMLUI's expression engine analyzes
// identifiers inside arrow-function bodies passed to
// subscribeTauriEvent and silently aborts the registration when a
// bare name has no xs declaration. Main.xmlui's onInit then stopped
// running its remaining statements partway through (statement 5
// onward), AgentMenu's mount cascade was disrupted, and menus
// stopped appearing. The fix: distinct __bram-prefixed window
// helpers paired with thin xs delegators below — the same pattern
// every other migrated function uses.
window.__bramSetToolbarPendingMenuFromEvent = function (e) {
  window.recordToolbarPendingMenuFromEvent(e);
};
window.__bramSetToolbarPendingMenuFromTurnState = function (turnState) {
  window.recordToolbarPendingMenuFromEvent({ payload: turnState && turnState.pendingMenu });
};
window.__bramTraceToolbarKey = function (key, extra) {
  var state = window.getToolbarPendingMenuState();
  var payload = {
    key: key,
    menuPresent: state.present ? 1 : 0,
    menuAgeMs: state.atMs ? (Date.now() - state.atMs) : -1,
  };
  if (extra && typeof extra === "object") {
    Object.keys(extra).forEach(function (k) {
      payload[k] = extra[k];
    });
  }
  window.__bramIframeTrace("toolbar-key", payload);
};
window.logToHost = function (payload) {
  // Master-flag short-circuit. Paired with `window.iframeTrace`
  // below. When traces are off, skip the Tauri IPC invoke (the
  // dominant per-event cost). Default-ON so behavior is preserved
  // during the brief startup window before the self-init fetch
  // below resolves the actual setting.
  if (window.__bramTracesEnabled === false) return;
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("log_from_right_pane", { payload: payload }).catch(function () {});
};
window.__bramSensitiveTraceKey = function (key) {
  var normalized = String(key || "").toLowerCase().replace(/[^a-z0-9]/g, "");
  return /(?:token|password|secret|apikey|accesskey|privatekey|credential)$/.test(normalized);
};
window.__bramRedactSensitiveText = function (value) {
  var marker = "[REDACTED]";
  var text = String(value == null ? "" : value);
  text = text.replace(
    /-----BEGIN [^\r\n]*PRIVATE KEY-----[\s\S]*?-----END [^\r\n]*PRIVATE KEY-----/g,
    marker
  );
  text = text.replace(/-----BEGIN [^\r\n]*PRIVATE KEY-----[\s\S]*$/g, marker);
  text = text.replace(
    /\b(?:sk-ant-|sk-proj-|github_pat_|gh[pousr]_)[A-Za-z0-9._+\-\/=]{12,}/g,
    marker
  );
  text = text.replace(/\bsk-[A-Za-z0-9._+\-\/=]{20,}/g, marker);
  text = text.replace(/\bAKIA[A-Z0-9]{16}\b/g, marker);
  text = text.replace(
    /\b(Bearer|Basic)(\s+)[A-Za-z0-9._~+\-\/=]+/gi,
    function (_, scheme, space) { return scheme + space + marker; }
  );
  text = text.replace(
    /(\b(?:token|password|secret|api[_-]?key|access[_-]?key|private[_-]?key|credential)\b\s*[:=]\s*)(["'])([\s\S]*?)\2/gi,
    function (_, prefix, quote) { return prefix + quote + marker + quote; }
  );
  text = text.replace(
    /(\b(?:token|password|secret|api[_-]?key|access[_-]?key|private[_-]?key|credential)\b\s*[:=]\s*)(?!["'\[])([^\s,;}\]]+)/gi,
    function (_, prefix) { return prefix + marker; }
  );
  return text;
};
window.__bramTraceSafeValue = function (value, depth) {
  depth = depth || 0;
  if (value == null) return value;
  var t = typeof value;
  if (t === "string") {
    var redacted = window.__bramRedactSensitiveText(value);
    return redacted.length > 500
      ? redacted.slice(0, 500) + "...[truncated " + redacted.length + " chars]"
      : redacted;
  }
  if (t === "number" || t === "boolean") return value;
  if (t !== "object") return String(value);
  if (depth >= 2) {
    if (Array.isArray(value)) return { __summary: "array", length: value.length };
    var keys = Object.keys(value);
    return { __summary: "object", keys: keys.slice(0, 12), keyCount: keys.length };
  }
  if (Array.isArray(value)) {
    return {
      __summary: "array",
      length: value.length,
      sample: value.slice(0, 3).map(function (v) { return window.__bramTraceSafeValue(v, depth + 1); }),
    };
  }
  var out = {};
  var objectKeys = Object.keys(value);
  for (var i = 0; i < objectKeys.length && i < 20; i++) {
    var key = objectKeys[i];
    out[key] = window.__bramSensitiveTraceKey(key)
      ? "[REDACTED]"
      : window.__bramTraceSafeValue(value[key], depth + 1);
  }
  if (objectKeys.length > 20) out.__truncatedKeys = objectKeys.length - 20;
  return out;
};

// iframeTrace: the [iframe] category of the comms-path trace log
// (issue #49). Forwards a structured record to the host's
// `log_from_right_pane` Tauri command, which routes records whose
// `kind` is `"iframe-trace"` into resources/bram-traces/bram-trace.log
// when BRAM_TRACE=1 is set on the host. No-op when logToHost isn't
// wired up. subkind is a token from the spec's maintained vocabulary
// (click, inflight-set, inflight-clear, listener-fired, ...); fields
// are arbitrary per-event metadata.
//
// Lives in plain JS so callers from XMLUI-evaluated arrow function
// bodies and xs functions don't pay the per-statement-await cost of
// processStatementQueueAsync
// (xmlui/src/components-core/script-runner/process-statement-async.ts:115-166).
// The xs declaration in Globals.xs is a thin delegator that calls
// this; the window helper uses the `__bram` prefix to avoid the
// trap where xs's `function iframeTrace` declaration overwrites
// `window.iframeTrace` (browser scripts hoist top-level function
// declarations onto window), which would turn the delegator's
// `window.iframeTrace(...)` call into recursion-to-itself. Same
// pattern as `window.__bramApplyAgentMenu` paired with the xs
// `applyAgentMenu` delegator (commit ea9480e).
window.__bramIframeTrace = function (subkind, fields) {
  try {
    if (window.__bramTracesEnabled === false) return;
    if (typeof window.logToHost !== "function") return;
    var payload = { kind: "iframe-trace", subkind: subkind, at: new Date().toISOString() };
    if (fields && typeof fields === "object") {
      Object.keys(fields).forEach(function (key) {
        payload[key] = window.__bramTraceSafeValue(fields[key], 0);
      });
    }
    window.logToHost(payload);
  } catch (e) {}
};

// Iframe long-task tracer (2026-07-09 describe-freeze hunt). The
// xterm-liveness watchdog covers the PARENT main thread; a frozen
// IFRAME was invisible — the trace just went silent at the freeze
// instant with nothing attributing the block. This logs every iframe
// main-thread task ≥200ms at recovery, so the next freeze names its
// duration instead of leaving a gap. Attribution granularity is
// whatever the webview provides (often just "self"), but duration +
// timing against the surrounding trace is the diagnostic payload.
//
// NOTE: WebKit (Bram's WKWebView) does NOT implement the Long Tasks API,
// so this observer records nothing here — a platform gap, not a bug. The
// working stall source is the heartbeat above, which emits the same
// `long-task` subkind with source:"heartbeat" for foreground ticks late by
// >=200ms. This observer stays for Chromium-based webviews and future WebKit.
try {
  if (typeof PerformanceObserver === "function") {
    new PerformanceObserver(function (list) {
      var entries = list.getEntries();
      for (var i = 0; i < entries.length; i++) {
        var e = entries[i];
        if (e.duration >= 200) {
          window.__bramIframeTrace("long-task", {
            ms: Math.round(e.duration),
            name: e.name || "",
          });
        }
      }
    }).observe({ entryTypes: ["longtask"] });
  }
} catch (e) { /* longtask unsupported: instrument absent, not broken */ }

// backgrounded-pane-menu-paint-observer: pane visibility transitions.
// One line per transition; pairs with the menu-paint marker (see
// __bramApplyAgentMenu) to prove/refute that a backgrounded window
// starves the menu paint until refocus (2026-07-19: a Write menu sat
// 28.8s, answered only after the focus-in escape). Observe-only.
// __bramPaneLastVisibleMs is the correlation timestamp: a menu-paint
// whose paint lands after this refocus instant is the specimen.
window.__bramPaneLastVisibleMs = 0;
try {
  document.addEventListener("visibilitychange", function () {
    if (!document.hidden) window.__bramPaneLastVisibleMs = Date.now();
    window.__bramIframeTrace("pane-visibility", {
      state: document.hidden ? "hidden" : "visible",
      via: "visibilitychange",
    });
  });
  window.addEventListener("blur", function () {
    window.__bramIframeTrace("pane-visibility", { state: "blur", via: "window" });
  });
  window.addEventListener("focus", function () {
    window.__bramPaneLastVisibleMs = Date.now();
    window.__bramIframeTrace("pane-visibility", { state: "focus", via: "window" });
  });
} catch (e) { /* observe-only: absent, not broken */ }

// asset-probe (promote-tool-descriptions-to-row forensics): the pane
// rendered pre-feature markup while the host provably served enriched
// projections, and a full WebKit cache clear did not change it. Every
// layer was verified EXCEPT the bytes the webview receives for the
// component markup itself — so observe them. 3s after boot in the
// tools pane, fetch our own Transcript.xmlui through the same origin
// the component loader uses, cached and no-store, and trace size +
// whether the new bindings are present. One grep then names the stale
// layer: cached==old & no-store==new -> cache; both old -> the serving
// store is stale (embedded assets); both new -> the loader itself.
try {
  if (window.location.pathname.indexOf("/tools/") !== -1) {
    setTimeout(function () {
      ["default", "no-store"].forEach(function (mode) {
        try {
          window
            .fetch("components/Transcript.xmlui", mode === "no-store" ? { cache: "no-store" } : {})
            .then(function (r) {
              return r.text().then(function (t) {
                window.__bramIframeTrace("asset-probe", {
                  path: "components/Transcript.xmlui",
                  mode: mode,
                  status: r.status,
                  bytes: t.length,
                  hasNameDetail: t.indexOf("nameDetail") >= 0,
                  hasAiDescription: t.indexOf("aiDescription") >= 0,
                  hasEagerComment: t.indexOf("inverted") >= 0,
                  origin: String(window.location.origin || ""),
                });
              });
            })
            .catch(function (e) {
              window.__bramIframeTrace("asset-probe", {
                path: "components/Transcript.xmlui",
                mode: mode,
                error: String(e),
              });
            });
        } catch (e) {}
      });
    }, 3000);
  }
} catch (e) { /* observe-only */ }

// Cascade-diagnosis instrumentation (refs #93). Emits a helper-call
// record when a hot JSONL-walking helper exceeds the threshold. Cheap
// paths (no-op early returns, cache hits) don't log because their _t0
// measurement is sub-ms. Threshold deliberately low to catch
// sub-frame stalls that compound across the cascade.
window.__bramTraceHelperTiming = function (name, t0, extra) {
  try {
    var elapsed = (typeof performance !== "undefined" && performance.now)
      ? performance.now() - t0
      : Date.now() - t0;
    if (elapsed < 2) return;
    if (typeof window.logToHost !== "function") return;
    var payload = {
      kind: "iframe-trace",
      subkind: "helper-call",
      name: name,
      ms: Math.round(elapsed),
      at: new Date().toISOString(),
    };
    if (extra && typeof extra === "object") Object.assign(payload, extra);
    window.logToHost(payload);
  } catch (e) {}
};

// Plain-JS equivalents of XMLUI's xs-only readLocalStorage /
// writeLocalStorage built-ins
// (xmlui/src/components-core/appContext/local-storage-functions.ts).
// Same dot-path semantics: the first segment is the localStorage entry
// name, remaining segments are a property path inside the parsed JSON
// object. Used by the __bram-prefixed localStorage shim helpers below
// so they can run in plain JS without re-entering XMLUI's statement
// queue. `bram.worklistMessageDraft` reads
// `JSON.parse(localStorage.bram).worklistMessageDraft`. Splitter keys
// like `bram.splitter.worklist` are two-level.
function __bramSplitKey(key) {
  var s = String(key);
  var dot = s.indexOf(".");
  return dot === -1 ? [s, undefined] : [s.substring(0, dot), s.substring(dot + 1)];
}

function __bramReadLS(key, fallback) {
  try {
    var parts = __bramSplitKey(key);
    var raw = localStorage.getItem(parts[0]);
    if (raw === null) return fallback;
    var root;
    try { root = JSON.parse(raw); } catch (e) { return fallback; }
    if (parts[1] === undefined) return root;
    var sub = parts[1].split(".");
    var cur = root;
    for (var i = 0; i < sub.length; i++) {
      if (cur == null || typeof cur !== "object") return fallback;
      cur = cur[sub[i]];
    }
    return cur === undefined ? fallback : cur;
  } catch (e) { return fallback; }
}

function __bramWriteLS(key, value) {
  try {
    var parts = __bramSplitKey(key);
    if (parts[1] === undefined) {
      if (value === undefined) localStorage.removeItem(parts[0]);
      else localStorage.setItem(parts[0], JSON.stringify(value));
      return;
    }
    var raw = localStorage.getItem(parts[0]);
    var root;
    if (raw === null) {
      root = {};
    } else {
      try { root = JSON.parse(raw); } catch (e) { root = {}; }
      if (!root || typeof root !== "object") root = {};
    }
    var sub = parts[1].split(".");
    var cur = root;
    for (var i = 0; i < sub.length - 1; i++) {
      var k = sub[i];
      if (!cur[k] || typeof cur[k] !== "object") cur[k] = {};
      cur = cur[k];
    }
    var last = sub[sub.length - 1];
    if (value === undefined) delete cur[last];
    else cur[last] = value;
    localStorage.setItem(parts[0], JSON.stringify(root));
  } catch (e) {}
}

function __bramReadSS(key, fallback) {
  try {
    if (!window.sessionStorage) return fallback;
    var v = sessionStorage.getItem(key);
    return v === null ? fallback : v;
  } catch (e) { return fallback; }
}

function __bramWriteSS(key, value) {
  try {
    if (!window.sessionStorage) return;
    if (value === undefined || value === null || value === "") {
      sessionStorage.removeItem(key);
    } else {
      sessionStorage.setItem(key, String(value));
    }
  } catch (e) {}
}

// Worklist "message agent" persistence + lifecycle shims. Counterparts
// for the xs delegators in Globals.xs (audit step 3, 2026-06-14).
// Each is invoked through bare-name `restoreWorklistDraft(...)` from
// xmlui markup or other xs code, which resolves to the xs delegator,
// which routes here. The cost saving is per-call body collapse: each
// of these used to run through processStatementQueueAsync's 3-await
// loop for every statement in the body; now the entire body runs as
// one plain-JS function call (one xs statement total).

var __bramWorklistDraftPersistTimer = null;
var __bramWorklistDraftPending = null;

function __bramFlushWorklistDraft() {
  if (__bramWorklistDraftPersistTimer) {
    clearTimeout(__bramWorklistDraftPersistTimer);
    __bramWorklistDraftPersistTimer = null;
  }
  if (__bramWorklistDraftPending !== null) {
    __bramWriteLS("bram.worklistMessageDraft", __bramWorklistDraftPending);
    __bramWorklistDraftPending = null;
  }
}

window.__bramRestoreWorklistDraft = function () {
  return __bramReadLS("bram.worklistMessageDraft", "");
};

window.__bramPersistWorklistDraft = function (text) {
  __bramWorklistDraftPending = String(text || "");
  if (__bramWorklistDraftPersistTimer) clearTimeout(__bramWorklistDraftPersistTimer);
  __bramWorklistDraftPersistTimer = setTimeout(__bramFlushWorklistDraft, 400);
};

window.__bramClearWorklistDraft = function () {
  if (__bramWorklistDraftPersistTimer) {
    clearTimeout(__bramWorklistDraftPersistTimer);
    __bramWorklistDraftPersistTimer = null;
  }
  __bramWorklistDraftPending = null;
  __bramWriteLS("bram.worklistMessageDraft", "");
};

window.__bramFlushWorklistDraft = __bramFlushWorklistDraft;

window.addEventListener("beforeunload", __bramFlushWorklistDraft);

// Worklist UI state model is now multi-expand: any number of items can be
// "open" simultaneously, each with its own feedback-draft text. State shape:
//   { expandedItemIds: string[], feedbackDraftsById: Record<string, string> }
// Legacy fields (selected, expandedItemId, feedbackExpanded, selectedFeedback)
// are honored on read for migration from pre-sticky-expansion sessions; they
// are never written back. After the first save in the new shape, the legacy
// keys disappear.
window.__bramReadWorklistUiStateObject = function () {
  var raw = __bramReadLS("bram.worklistUiState", "");
  if (!raw) return {};
  var saved;
  if (typeof raw === "object") {
    saved = raw;
  } else {
    try { saved = JSON.parse(raw); } catch (e) { saved = null; }
  }
  return (saved && typeof saved === "object") ? saved : {};
};

window.__bramRestoreWorklistUiState = function (field) {
  var saved = window.__bramReadWorklistUiStateObject();
  if (field === "expandedItemIds") {
    // New canonical field. Fall back to legacy single-id on first migration.
    var arr = Array.isArray(saved.expandedItemIds) ? saved.expandedItemIds.slice() : null;
    if (!arr) {
      var legacy = saved.expandedItemId || saved.selected || null;
      arr = legacy ? [legacy] : [];
    }
    window.__bramIframeTrace("worklist-ui-state-restore", { field: field, count: arr.length });
    return arr;
  }
  if (field === "feedbackDraftsById") {
    // New canonical field. Migrate legacy { selected, selectedFeedback }.
    var map = (saved.feedbackDraftsById && typeof saved.feedbackDraftsById === "object")
      ? Object.assign({}, saved.feedbackDraftsById)
      : null;
    if (!map) {
      map = {};
      if (saved.selected && saved.selectedFeedback) {
        map[saved.selected] = String(saved.selectedFeedback);
      }
    }
    window.__bramIframeTrace("worklist-ui-state-restore", { field: field, count: Object.keys(map).length });
    return map;
  }
  // Legacy single-value fields retained for any stragglers; new code shouldn't read these.
  if (field === "feedbackExpanded") return !!saved.feedbackExpanded;
  if (field === "selectedFeedback") return String(saved.selectedFeedback || "");
  if (field === "selected") return saved.selected || null;
  if (field === "expandedItemId") return saved.expandedItemId || null;
  return null;
};

window.__bramPersistWorklistUiState = function (state) {
  // state: { expandedItemIds: string[], feedbackDraftsById: Record<string, string> }
  var ids = (state && Array.isArray(state.expandedItemIds)) ? state.expandedItemIds.slice() : [];
  var drafts = (state && state.feedbackDraftsById && typeof state.feedbackDraftsById === "object") ? state.feedbackDraftsById : {};
  // Garbage-collect drafts whose item is no longer expanded — keeps storage bounded.
  var prunedDrafts = {};
  for (var i = 0; i < ids.length; i++) {
    var id = ids[i];
    if (drafts[id]) prunedDrafts[id] = String(drafts[id]);
  }
  window.__bramIframeTrace("worklist-ui-state-save", {
    expandedCount: ids.length,
    draftCount: Object.keys(prunedDrafts).length,
  });
  __bramWriteLS("bram.worklistUiState", JSON.stringify({
    expandedItemIds: ids,
    feedbackDraftsById: prunedDrafts,
  }));
};

window.__bramClearWorklistUiState = function () {
  window.__bramIframeTrace("worklist-ui-state-clear", {});
  __bramWriteLS("bram.worklistUiState", "");
};

window.__bramRestoreWorklistSubmittedMessage = function () {
  return __bramReadLS("bram.worklistSubmittedMessage", "");
};

window.__bramRestoreWorklistSessionSubmittedMessage = function () {
  return __bramReadSS("bram.worklistSessionSubmittedMessage", "");
};

window.__bramShouldDimAgentDockOnMount = function () {
  var key = "bram.agentDockLaunchDimConsumed";
  var consumed = __bramReadSS(key, "");
  if (consumed === "1") return false;
  __bramWriteSS(key, "1");
  return true;
};

window.__bramRestoreWorklistSubmittedKind = function () {
  var kind = __bramReadLS("bram.worklistSubmittedKind", "");
  return kind === "message" || kind === "action" ? kind : null;
};

window.__bramSetWorklistSubmittedKind = function (kind) {
  if (kind === "message" || kind === "action") {
    __bramWriteLS("bram.worklistSubmittedKind", kind);
  } else {
    __bramWriteLS("bram.worklistSubmittedKind", "");
  }
  return kind || null;
};

window.__bramRestoreSplitterSize = function (key, fallback) {
  var raw = __bramReadLS("bram.splitter." + key, "");
  var s = String(raw || "").trim();
  var n = parseFloat(s);
  var hasUnit = /(?:px|%)$/i.test(s);
  var result = (!isNaN(n) && n > 0)
    ? (hasUnit ? s : (n < 100 ? (n + "%") : (n + "px")))
    : fallback;
  window.__bramIframeTrace("splitter-restore", { key: key, raw: raw, result: result });
  return result;
};

window.__bramSaveSplitterSize = function (key, sizes) {
  if (Array.isArray(sizes)) {
    var a = Number(sizes[0]);
    var b = Number(sizes[1]);
    var total = a + b;
    var pct = total > 0 ? (a / total) * 100 : 0;
    window.__bramIframeTrace("splitter-save", { key: key, sizes: sizes, pct: pct, unit: "%" });
    if (pct > 0 && pct < 100) {
      __bramWriteLS("bram.splitter." + key, String(Math.round(pct * 10) / 10) + "%");
    }
    return;
  }
  var px = Number(sizes);
  window.__bramIframeTrace("splitter-save", { key: key, sizes: sizes, px: px, unit: "px" });
  if (px > 0) {
    __bramWriteLS("bram.splitter." + key, String(Math.round(px)) + "px");
  }
};

// Body strings for the Settings tab info dialogs. Lifted out of
// Settings.xmlui to keep the markup readable; the dialog itself
// stays inline in Settings since it's a single consumer.
window.settingsInfoBodies = {
  shell:
    "## Agent command\n\n" +
    "The agent to launch: `claude` or `codex`.\n\n" +
    "## Continue most recent session on startup\n\n" +
    "When on, the agent launches resuming its most recent session " +
    "(`claude --continue` / `codex resume --last`) instead of starting fresh. " +
    "Each CLI continues its own latest session natively — no session id needed.\n\n" +
    "## Arguments\n\n" +
    "Optional extra launch-time flags passed to the selected agent. Appended " +
    "after the launch (or continue) command.\n\n" +
    "## First command\n\n" +
    "An optional command sent to the agent's TUI once it starts, for example " +
    "`/resume` to open the interactive picker. Empty by default (send nothing).",
  batchCommitActions:
    "## Batch commit actions\n\n" +
    "Shows Approve all / Drop all when two or more TO COMMIT items are " +
    "present. Approve all lets the agent commit those items in one turn. " +
    "Drop all removes them from the worklist; on-disk edits stay unless " +
    "you ask the agent to discard them.\n\n" +
    "## Mirroring to GitHub issues\n\n" +
    "When enabled, issue-linked worklist items post lifecycle comments to " +
    "GitHub. Bram uses `closesIssues` first, then falls back to an " +
    "`issue-<number>-...` item id. Batch commits do not auto-close issues.",
  ui:
    "## Show or Hide Target App\n\n" +
    "Usually off. Most people run their app in their own browser, so the " +
    "target-app pane stays hidden and the agent pane fills the space. Turn " +
    "it on to preview a simple app inside Bram; turn it off to reclaim the " +
    "room." +
    "\n\n## Agent-pane hot-reload\n\n" +
    "Only matters when developing Bram itself: when on, the agent pane " +
    "reloads automatically as you edit Bram’s own source. Leave it off " +
    "otherwise.",
  ai:
    "## Tool Descriptions\n\n" +
    "When a tool-use expansion in the Transcript shows a command with no " +
    "agent-authored intent sentence (or a weak one), Bram asks Claude Haiku " +
    "for a one-line description and renders it as a `#` header above the " +
    "command. Eligible visible rows are described eagerly; expanding a row " +
    "can also request or improve its description. Results are cached, and " +
    "each call is a fraction of a cent.\n\n" +
    "Off by default. Turning it on is project-level consent to send the " +
    "selected tool material to the Anthropic API when `ANTHROPIC_API_KEY` " +
    "is set in Bram's environment. Depending on the row, that material can " +
    "include command text, a file diff or written content, a file/search " +
    "target, the agent's preceding context, and a result excerpt. Bram " +
    "uses a Rust secret-scanning library to redact common credential shapes " +
    "before sending, but heuristic " +
    "redaction cannot guarantee arbitrary content contains no secrets. " +
    "The key itself is read from the environment only and is never written " +
    "to `.bram.json`.\n\n" +
    "**Billing note.** Setting `ANTHROPIC_API_KEY` also affects Claude " +
    "Code's own billing. If Claude Code prompts you to approve the key and " +
    "you accept, it authenticates with that key and bills **per-token to " +
    "your API account** instead of your Max/Pro subscription (an approved " +
    "API key outranks subscription login). To keep Bram's descriptions on " +
    "the key while leaving Claude Code on your subscription, run `/config` " +
    "in the agent and turn OFF \"Use custom API key\" — that rejects the key " +
    "for Claude Code only; Bram still reads it from the environment. Confirm " +
    "with `/status`. Docs: https://code.claude.com/docs/en/authentication.md\n\n" +
    "Per-call cost and redaction count are traced as `[ai-describe]` in " +
    "resources/bram-traces/bram-trace.log.\n\n" +
    "Persists in .bram.json under ai.describeCommands.",
  traces:
    "## Tracing enabled\n\n" +
    "Master switch for writes to " +
    "resources/bram-traces/bram-trace.log. When off, every [emit] / " +
    "[iframe] / [route] line is a no-op regardless of the Inspector " +
    "trace tap below. If BRAM_TRACE is set in the environment at " +
    "launch (e.g. BRAM_TRACE=1 cargo run), it wins and this switch is " +
    "ignored — so CI and explicit launch environments keep behaving the same." +
    "\n\n## Inspector trace tap\n\n" +
    "Forwards XMLUI Inspector events " +
    "(window._xsLogs) from the agent pane into bram-trace.log as " +
    "[iframe] subkind=inspector-event, so they interleave with host " +
    "traces live (no Inspector export needed). Capped at 50 entries " +
    "per 200ms tick; overflow emits subkind=inspector-overflow. " +
    "Inspector traces are intentionally complete and noisy (one per " +
    "keystroke, etc.); values pass through Bram's trace sanitizer first. " +
    "Requires Tracing enabled above." +
    "\n\n" +
    "At startup, raw trace archives older than the configured window are " +
    "sanitized with the host credential redactor and written as gzip files. " +
    "The raw source is removed only after its sanitized archive is safely " +
    "installed. Compressed history is retained indefinitely, so storage use " +
    "is intentionally unbounded. The raw window defaults to 14 days and can " +
    "be changed from 1 to 3650 days. Redaction is heuristic defense in depth, " +
    "not proof that arbitrary trace content is secret-free.\n\n" +
    "These settings persist in .bram.json under traces.enabled, " +
    "traces.inspectorTap, and traces.archiveAfterDays.",
};

// "Claude Code" for the claude provider, Title-cased provider name
// otherwise ("codex" → "Codex"). Falls back through
// mainAgentStatus.provider → enhanceStatus.activeProvider → '' so the
// idle state still gets a label. Guards mainAgentStatus against null.
window.providerDisplayName = function (mainAgentStatus, enhanceStatusValue) {
  var p =
    (mainAgentStatus && mainAgentStatus.provider) ||
    (enhanceStatusValue && enhanceStatusValue.activeProvider) ||
    "";
  if (p === "claude") return "Claude Code";
  return p ? p.charAt(0).toUpperCase() + p.slice(1) : p;
};

// Should the idle-state provider label be visible? True when we have
// some agent state, we're NOT currently working or finished, and
// there's a provider name available to display.
window.shouldShowIdleProvider = function (mainAgentStatus, enhanceStatusValue) {
  if (!mainAgentStatus && !enhanceStatusValue) return false;
  if (mainAgentStatus &&
      (mainAgentStatus.state === "working" || mainAgentStatus.state === "finished")) {
    return false;
  }
  return Boolean(
    (mainAgentStatus && mainAgentStatus.provider) ||
    (enhanceStatusValue && enhanceStatusValue.activeProvider)
  );
};

// "<provider> <verb>… (<elapsed> · <substate>)" for the working state. Now
// that the grid supplies clean full-fidelity elapsed + the substate signal
// ("thinking", "almost done thinking", …), surface them on the row. Tokens
// intentionally omitted (per user: distracting).
window.headerWorkingLabel = function (mainAgentStatus, enhanceStatusValue) {
  var s = mainAgentStatus || {};
  var verb = s.verb || "working";
  var label =
    window.providerDisplayName(mainAgentStatus, enhanceStatusValue) +
    ": " +
    verb +
    "…";
  var detail = [s.elapsedText, s.substate].filter(Boolean).join(" · ");
  return detail ? label + " (" + detail + ")" : label;
};

// "<provider> <verb> · <elapsed>" for the finished state. Verb
// fall-through: status.verb (when finished) → status.verb (when
// non-working) → lastSeenAgentVerb (when non-working) → "Finished".
window.headerFinishedLabel = function (mainAgentStatus, enhanceStatusValue, lastSeenAgentVerb) {
  var s = mainAgentStatus || {};
  var verb;
  if (s.state === "finished") {
    verb = s.verb || "Finished";
  } else if (s.verb && s.verb !== "working") {
    verb = s.verb;
  } else if (lastSeenAgentVerb && lastSeenAgentVerb !== "working") {
    verb = lastSeenAgentVerb;
  } else {
    verb = "Finished";
  }
  var base = window.providerDisplayName(mainAgentStatus, enhanceStatusValue) + ": " + verb;
  return base + (s.elapsedText ? " · " + s.elapsedText : "");
};

// Compute the next sort state for a clickable table-header. If the
// column is already active, flip the direction; otherwise switch to
// the new column with its default direction. Returns {field, dir}.
window.toggleSort = function (currentField, currentDir, newField, defaultDir) {
  if (currentField === newField) {
    return { field: newField, dir: currentDir === "asc" ? "desc" : "asc" };
  }
  return { field: newField, dir: defaultDir };
};

// Render a table-header label with an active-column arrow.
// "STATE ↑" / "STATE ↓" if currentField matches; "STATE" otherwise.
window.sortLabel = function (label, currentField, currentDir, fieldName) {
  if (currentField !== fieldName) return label;
  return label + (currentDir === "asc" ? " ↑" : " ↓");
};

// Select the list to display in a searchable tab. If query is 2+
// chars, return the search results (accepting either the raw-array
// shape Sessions uses or the {results} wrapper used elsewhere).
// Otherwise return the full list. Used by Feedback, History, Issues,
// Sessions.
window.selectDisplayed = function (query, searchValue, fullList) {
  if (query && query.trim().length >= 2) {
    if (Array.isArray(searchValue)) return searchValue;
    return (searchValue && searchValue.results) || [];
  }
  return fullList || [];
};

// Normalize a path/URL for an XMLUI Image's src binding. Pass through
// data: and http(s) URLs verbatim; otherwise route through the
// /__file?path= shim with optional file://(localhost)? prefix stripped.
// Used by every Image preview in the agent pane.
window.imageSrcForPath = function (path) {
  var p = path || "";
  if (p.startsWith("data:") || p.startsWith("http")) return p;
  var cleaned = p.startsWith("file://")
    ? p.replace(/^file:\/\/(localhost)?/, "")
    : p;
  return "/__file?path=" + encodeURIComponent(cleaned);
};

// extractImagePaths — extracts [Image: source: <path>] marker paths.
// Used by the submit path (staged-image bookkeeping); turn display
// resolution lives in the host projection.
window.__bramExtractImagePaths = function (text) {
  if (!text) return [];
  var paths = [];
  var imagePath = "(?:/[^\\]]+|[A-Za-z]:\\\\[^\\]]+)\\.(?:png|jpg|jpeg|gif|webp)";
  var re = new RegExp("\\[Image: source: (" + imagePath + ")\\]", "gi");
  var m;
  while ((m = re.exec(text)) !== null) paths.push(m[1]);
  return paths;
};
function __bramExtractImagePaths(text) {
  // Kept as a local alias so the step-3 submission trio above (defined
  // before the window helper) still resolves.
  return window.__bramExtractImagePaths(text);
}

// Submission trio. submitWorklistMessageFast needs the xs-side
// voiceTarget (still an xs var; step 4 will mirror it onto window).
// For now the xs delegator passes it as the third argument.
window.__bramSubmitWorklistMessageFast = function (text, voiceTarget) {
  if (!text || !text.trim()) return false;
  var userTyped = text.trim();
  var toSend = window.__bramWithStagedImageMarkers(userTyped, "message-agent", voiceTarget);
  var sentAt = Date.now();
  window.__bramIframeTrace("message-agent-submit", { stage: "before-toTurn", chars: toSend.length, sentAt: sentAt });
  if (typeof window.toTurn === "function") window.toTurn(toSend);
  window.__bramIframeTrace("message-agent-submit", { stage: "after-toTurn", chars: toSend.length, sentAt: sentAt });
  var baseline = 0;
  __bramWriteLS("bram.worklistMessageDraft", "");
  __bramWriteLS("bram.worklistSubmittedMessage", userTyped);
  __bramWriteSS("bram.worklistSessionSubmittedMessage", userTyped);
  window.__bramSetWorklistSubmittedKind("message");
  return { message: userTyped, images: __bramExtractImagePaths(toSend), baseline: baseline, sentAtText: new Date().toLocaleTimeString() };
};

window.__bramWithStagedImageMarkers = function (text, target, voiceTarget) {
  var requestedTarget = target || voiceTarget || "";
  var consumeTarget = requestedTarget;
  if (requestedTarget === "feedback") {
    var focusedFeedback = window.bramActiveFocusedFeedbackItemIdMirror || "";
    if (focusedFeedback) {
      consumeTarget = "feedback:" + focusedFeedback;
    } else if (window.bramCurrentPasteTarget) {
      consumeTarget = window.bramCurrentPasteTarget() || requestedTarget;
    }
  }
  bramTracePasteImage("with-markers", {
    requestedTarget: requestedTarget,
    voiceTarget: voiceTarget || "",
    consumeTarget: consumeTarget,
    pendingBefore: bramPendingPastedImageSummary()
  });
  var paths = window.bramConsumePastedImagePaths
    ? window.bramConsumePastedImagePaths(consumeTarget)
    : [];
  if (!paths || paths.length === 0) return text;
  var lines = paths.map(function (p) { return "Read this screenshot: @" + p + "\n[Image: source: " + p + "]"; });
  var markers = lines.join("\n");
  var skipPrefix = "skip-worklist:";
  var trimmedStart = (text || "").trimStart();
  if (trimmedStart.indexOf(skipPrefix) === 0) {
    var leading = text.slice(0, text.length - trimmedStart.length);
    var rest = trimmedStart.slice(skipPrefix.length).trimStart();
    return leading + skipPrefix + " " + markers + (rest ? "\n\n" + rest : "");
  }
  return text ? markers + "\n\n" + text : markers;
};

// Pure predicate — voice-target whitelist for text-input destinations.
// xs delegator in Globals.xs preserves the bare-name callability.
window.__bramIsWorklistTextVoiceTarget = function (target) {
  var t = target || "";
  return ["message-agent", "feedback", "new-item", "new-issue"].indexOf(t) !== -1
    || t.indexOf("feedback:") === 0;
};

// Inflight + submitted-message helpers (audit step 6). All pure data
// transforms; xs delegators in Globals.xs preserve bare-name calls.
window.__bramInflightActionLabel = function (kind) {
  if (kind === "approved") return "Approving";
  if (kind === "iterate") return "Iterating";
  if (kind === "drop") return "Dropping";
  return "";
};

// Full header inflight-banner label: "<Action> <ids> (TO APPLY|TO COMMIT)".
// statusLabel is supplied by the /__inflight route from worklist.json.
window.__bramInflightBannerLabel = function (claim) {
  if (!claim || !claim.ids || !claim.ids.length) return "";
  var action = window.__bramInflightActionLabel(claim.kind);
  var ids = (claim.ids || []).join(", ");
  var status = claim.statusLabel ? " (" + claim.statusLabel + ")" : "";
  return action + " " + ids + status;
};

window.__bramStripImageMarkerPrefix = function (text) {
  return (text || "").replace(/^(\s*Read this screenshot: @\S+\s*)+/, "").trim();
};

// Plain-JS equivalent of xs `App.mark(label)`. App.mark pushes a
// `kind: "app:mark"` record to the Inspector buffer at window._xsLogs
// (xmlui/src/components-core/appContext/app-utils.ts:49-53). The
// pure-JS helpers below preserve the marks so Inspector exports stay
// comparable across the migration.
function __bramAppMark(label) {
  try {
    if (!window._xsLogs) return;
    var perfTs = (typeof performance !== "undefined" && performance.now) ? performance.now() : 0;
    window._xsLogs.push({ kind: "app:mark", ts: Date.now(), label: label, perfTs: perfTs });
  } catch (e) {}
}

window.__bramWorklistActionStatusLabel = function (item) {
  var status = (item && item.status) || "proposed";
  if (status === "applied") return "To Commit";
  if (status === "proposed") return "To Apply";
  return status ? status.charAt(0).toUpperCase() + status.slice(1) : "Worklist";
};

window.__bramWorklistActionDisplay = function (kind, items) {
  var action =
    kind === "approved" ? "Approved" :
    kind === "iterate" ? "Iterated" :
    kind === "drop" ? "Dropped" :
    "Submitted";
  var ids = (items || []).map(function (i) {
    if (typeof i === "string") return i;
    return (i && i.id) || "";
  }).filter(Boolean);
  if (ids.length === 0) return action;
  if (ids.length === 1) return action + " " + ids[0];
  return action + " " + ids.length + " items: " + ids.join(", ");
};

window.__bramWorklistActionStatusSuffix = function (item) {
  var status = (item && item.status) || "proposed";
  if (status === "applied") return " to commit";
  if (status === "proposed") return " to apply";
  return "";
};

window.__bramWorklistActionConversationDisplay = function (kind, items, selectedId, feedback) {
  var selected = (items || []).filter(function (i) { return i.id === selectedId; });
  var suffix = selected.length === 1 ? window.__bramWorklistActionStatusSuffix(selected[0]) : "";
  return window.__bramWorklistActionDisplay(kind, selected) + suffix;
};

window.__bramTraceIterateEnabled = function (submitting, selected, selectedFeedback) {
  __bramAppMark("iterate-enabled");
  return !submitting && !!selected && (selectedFeedback || "").trim().length > 0;
};

window.__bramTraceApproveDropEnabled = function (submitting, selected) {
  __bramAppMark("approve-drop-enabled");
  return !submitting && !!selected;
};

window.__bramBuildApprovePayload = function (items, selectedId, feedback) {
  __bramAppMark("build-approve-payload");
  return JSON.stringify({
    items: (items || []).filter(function (i) { return i.id === selectedId; })
      .map(function (i) { return { id: i.id, feedback: feedback }; }),
  });
};

window.__bramBuildIteratePayload = function (items, selectedId, feedback) {
  __bramAppMark("build-iterate-payload");
  // feedback may be either an inline string (backward-compat) or a
  // `{ feedbackRef: "<id>" }` object (new, from queueFeedbackDraft).
  return JSON.stringify({
    items: (items || []).filter(function (i) { return i.id === selectedId; })
      .map(function (i) {
        return feedback && typeof feedback === "object" && feedback.feedbackRef
          ? { id: i.id, feedbackRef: feedback.feedbackRef }
          : { id: i.id, feedback: feedback };
      }),
  });
};

window.__bramBuildDropPayload = function (items, selectedId, feedback) {
  __bramAppMark("build-drop-payload");
  return JSON.stringify({
    items: (items || []).filter(function (i) { return i.id === selectedId; })
      .map(function (i) { return { id: i.id, feedback: feedback }; }),
  });
};

window.__bramBuildApproveItems = function (items, selectedId, feedback) {
  return (items || []).filter(function (i) { return i.id === selectedId; })
    .map(function (i) { return { id: i.id, feedback: feedback }; });
};

window.__bramBuildDropItems = function (items, selectedId, feedback) {
  return (items || []).filter(function (i) { return i.id === selectedId; })
    .map(function (i) { return { id: i.id, feedback: feedback }; });
};

window.__bramBuildSingleItemApprovePayload = function (itemRef, feedback) {
  __bramAppMark("build-single-item-approve-payload");
  return JSON.stringify({
    items: [{ id: itemRef.id, feedback: feedback }],
  });
};

window.__bramCountByStatus = function (items, status) {
  return (items || []).filter(function (i) { return (i.status || "proposed") === status; }).length;
};

window.__bramBuildBatchApprovePayload = function (items, feedback) {
  __bramAppMark("build-batch-approve-payload");
  return JSON.stringify({
    items: (items || []).filter(function (i) { return (i.status || "proposed") === "applied"; })
      .map(function (i) { return { id: i.id, feedback: feedback || "" }; }),
  });
};

window.__bramBuildBatchApproveItems = function (items, feedback) {
  return (items || []).filter(function (i) { return (i.status || "proposed") === "applied"; })
    .map(function (i) { return { id: i.id, feedback: feedback || "" }; });
};

window.__bramBuildBatchDropPayload = function (items, feedback) {
  __bramAppMark("build-batch-drop-payload");
  return JSON.stringify({
    items: (items || []).filter(function (i) { return (i.status || "proposed") === "applied"; })
      .map(function (i) { return { id: i.id, feedback: feedback || "" }; }),
  });
};

window.__bramBuildBatchDropItems = function (items, feedback) {
  return (items || []).filter(function (i) { return (i.status || "proposed") === "applied"; })
    .map(function (i) { return { id: i.id, feedback: feedback || "" }; });
};

window.__bramPrepareBatchWorklistActionSubmission = function (opts) {
  opts = opts || {};
  var items = opts.items || [];
  var kind = opts.kind === "drop" ? "drop" : "approved";
  var target = kind === "drop" ? "drop-all" : "approve-all";
  var ids = items.filter(function (i) { return (i.status || "proposed") === "applied"; });
  window.__bramIframeTrace("click", { target: target, count: ids.length });
  window.__bramClearWorklistUiState();
  var submittedItemId = ids.length > 0 ? ids[0].id : null;
  var submittedKind = window.__bramSetWorklistSubmittedKind("action");
  window.__bramIframeTrace("inflight-set", { item: submittedItemId, via: "click", target: target });
  var authItems = kind === "drop"
    ? window.__bramBuildBatchDropItems(items, "")
    : window.__bramBuildBatchApproveItems(items, "");
  return {
    turnText: (kind === "drop" ? "drop: " : "approved: ") + (
      kind === "drop"
        ? window.__bramBuildBatchDropPayload(items, "")
        : window.__bramBuildBatchApprovePayload(items, "")
    ),
    authorizationPayload: { kind: kind, items: authItems },
    submitting: true,
    submittedItemId: submittedItemId,
    submittedKind: submittedKind,
    actionProgressScope: "batch",
    actionProgressKind: kind,
    actionProgressTick: 0,
    expandedItemIds: [],
    feedbackDraftsById: {},
  };
};

// Image-marker strip kept as a presentation helper (grid-sourced
// menu-prose and dock text are not projection output). The raw-JSONL →
// turns parser chain that used to live here (sessionTurns,
// _parseLinesToTurns, tool/codex satellites) was deleted after the host
// projection (/__turns) became the single turn source — see
// docs/turn-transport-redesign.md step 7.

window.__bramStripImagePaths = function (text) {
  if (!text) return text;
  var imagePath = "(?:/[^\\]]+|[A-Za-z]:\\\\[^\\]]+)\\.(?:png|jpg|jpeg|gif|webp)";
  return text
    .replace(new RegExp("\\n*\\[Image: source: " + imagePath + "\\]", "gi"), "")
    .replace(/^(\s*Read this screenshot: @\S+\s*)+/, "")
    .trim();
};

window.__bramExtractMarkdownImages = function (text) {
  if (!text) return [];
  var urls = [];
  var md = /!\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
  var m;
  while ((m = md.exec(text)) !== null) urls.push(m[1]);
  var html = /<img\b[^>]*\bsrc=["']([^"']+)["'][^>]*>/gi;
  while ((m = html.exec(text)) !== null) urls.push(m[1]);
  return urls;
};

window.__bramStripMarkdownImages = function (text) {
  if (!text) return text;
  return text
    .replace(/\n*!\[[^\]]*\]\([^)\s]+(?:\s+"[^"]*")?\)/g, "")
    .replace(/\n*<img\b[^>]*\bsrc=["'][^"']+["'][^>]*>/gi, "");
};

// Quote-aware top-level split of a compound command at && / || / ; / |
// boundaries (tool-expansion-wrap-and-describe). Display-only: each
// segment becomes its own line, continuation lines keep their
// separator as a prefix so the chain reads naturally. Separators
// inside single/double/back quotes are never split points. Long
// segments soft-wrap at spaces near the width cap with a hanging
// indent, so the code fence stops needing a horizontal scrollbar.
window.__bramSplitCommandSegments = function (body, widthCap) {
  var cap = widthCap || 96;
  var segs = [];
  var cur = "";
  var q = null;
  var i = 0;
  while (i < body.length) {
    var ch = body.charAt(i);
    if (q) {
      cur += ch;
      if (ch === q && body.charAt(i - 1) !== "\\") q = null;
      i++;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      q = ch;
      cur += ch;
      i++;
      continue;
    }
    var two = body.substr(i, 2);
    if (two === "&&" || two === "||") {
      segs.push(cur);
      cur = two + " ";
      i += 2;
      while (body.charAt(i) === " ") i++;
      continue;
    }
    if (ch === ";" || ch === "|") {
      segs.push(cur);
      cur = ch + " ";
      i += 1;
      while (body.charAt(i) === " ") i++;
      continue;
    }
    cur += ch;
    i++;
  }
  segs.push(cur);
  var lines = [];
  for (var s = 0; s < segs.length; s++) {
    var seg = (s === 0 ? segs[s].trim() : "  " + segs[s].trim());
    while (seg.length > cap) {
      var brk = seg.lastIndexOf(" ", cap);
      // A break point at or inside the 6-char hanging indent means the
      // visible content has no usable space: stop wrapping and leave the
      // long token on one line. With the old `brk <= 4` guard, a wrapped
      // continuation ("      " + >cap spaceless token, e.g. a long session
      // path or rg pattern) found brk=5 forever and rebuilt seg
      // byte-identical each pass — the transcript-expansion freeze
      // (fix-command-wrap-infinite-loop; probe capture 2026-07-12T03:34Z).
      // For brk >= 7 the segment strictly shrinks, so wrapping terminates.
      if (brk <= 6) break;
      lines.push(seg.slice(0, brk));
      seg = "      " + seg.slice(brk + 1);
    }
    lines.push(seg);
  }
  return lines;
};

window.__bramFormatToolCommand = function (command, description) {
  if (command == null) return "";
  var body = String(command);
  if (!body) return "";
  // render-supabase-execute-sql: a commandDisplay that is already a fenced code
  // block (the host emits ```sql for execute_sql) passes through verbatim so it
  // isn't re-wrapped in a bash fence.
  var fencedTrim = body.trim();
  if (fencedTrim.slice(0, 3) === "```" && fencedTrim.slice(-3) === "```") {
    return fencedTrim;
  }
  // Multi-line commands (heredocs, scripts) keep their own layout;
  // splitting/wrapping is for the single-line compound case.
  var display = body.indexOf("\n") >= 0
    ? body
    : window.__bramSplitCommandSegments(body).join("\n");
  // The agent-authored intent sentence renders as a comment above the
  // command — reliable because the calling agent wrote it at call
  // time; absent (e.g. codex shell calls) means no header, no
  // synthesis.
  var head = "";
  if (description) {
    head = "# " + String(description).replace(/\s+/g, " ").trim() + "\n";
  }
  var scan = head + display;
  var longest = 0, run = 0;
  for (var i = 0; i < scan.length; i++) {
    if (scan.charAt(i) === "`") {
      run++;
      if (run > longest) longest = run;
    } else {
      run = 0;
    }
  }
  var fenceLen = Math.max(3, longest + 1);
  var fence = "";
  for (var j = 0; j < fenceLen; j++) fence += "`";
  return fence + "bash\n" + head + display + "\n" + fence;
};

window.__bramToolInputJsonLines = function (input, maxLines) {
  var cap = maxLines || 20;
  if (input === null || input === undefined) return { lines: [], remaining: 0 };
  if (typeof input === "string") {
    var allStr = input.split("\n");
    return { lines: allStr.slice(0, cap), remaining: Math.max(0, allStr.length - cap) };
  }
  var json;
  try {
    json = JSON.stringify(input, null, 2);
  } catch (e) {
    return { lines: ["(unserializable input)"], remaining: 0 };
  }
  var all = json.split("\n");
  return { lines: all.slice(0, cap), remaining: Math.max(0, all.length - cap) };
};

// History helpers (audit step 8). All pure. Internal calls go through
// the window.__bram* versions directly so the whole chain stays in
// plain JS (xs delegators below are entry points only).

window.__bramHistoryPhaseKind = function (phase) {
  var summary = ((phase && phase.summary) || "").toLowerCase();
  if (summary.indexOf("applied") >= 0) return "applied";
  if (summary.indexOf("proposed") >= 0) return "proposed";
  return "";
};

window.__bramHistoryDecodeJsonStringValue = function (raw) {
  if (!raw) return "";
  try {
    return JSON.parse('"' + raw + '"');
  } catch (err) {
    return raw.replace(/\\n/g, "\n").replace(/\\"/g, '"');
  }
};

window.__bramHistoryExtractProseFromDiff = function (diff) {
  var lines = (diff || "").split("\n");
  var before = "";
  var after = "";
  for (var i = 0; i < lines.length; i++) {
    var line = lines[i];
    var afterMatch = line.match(/^\+\s+"after":\s+"(.*)"[,]?$/);
    if (afterMatch) {
      after = window.__bramHistoryDecodeJsonStringValue(afterMatch[1].replace(/",?$/, ""));
      continue;
    }
    var beforeMatch = line.match(/^\+\s+"before":\s+"(.*)"[,]?$/);
    if (beforeMatch) {
      before = window.__bramHistoryDecodeJsonStringValue(beforeMatch[1].replace(/",?$/, ""));
    }
  }
  return after || before;
};

window.__bramHistoryLatestPhase = function (group) {
  var phases = (group && group.phases) || [];
  return phases.length > 0 ? phases[phases.length - 1] : null;
};

window.__bramHistoryCurrentItem = function (group) {
  return (group && group.currentItem) || null;
};

window.__bramHistoryItemProse = function (item) {
  if (!item) return "";
  var after = typeof item.after === "string" ? item.after.trim() : "";
  if (after) return after;
  var before = typeof item.before === "string" ? item.before.trim() : "";
  return before;
};

window.__bramHistoryCurrentProsePhase = function (group) {
  var item = window.__bramHistoryCurrentItem(group);
  var itemProse = window.__bramHistoryItemProse(item);
  if (itemProse) {
    return {
      phase: window.__bramHistoryLatestPhase(group),
      prose: itemProse,
      source: "snapshot",
    };
  }
  var phases = (group && group.phases) || [];
  for (var i = phases.length - 1; i >= 0; i--) {
    var prose = window.__bramHistoryExtractProseFromDiff(phases[i].diff || "");
    if (prose) {
      return { phase: phases[i], prose: prose, source: "diff" };
    }
  }
  return { phase: null, prose: "", source: "" };
};

window.__bramHistoryCardProsePreview = function (group) {
  var current = window.__bramHistoryCurrentProsePhase(group).prose || "";
  var normalized = current.replace(/\s+/g, " ").trim();
  if (!normalized) return "";
  if (normalized.length <= 240) return normalized;
  return normalized.slice(0, 237).trimEnd() + "...";
};

window.__bramHistoryDateParts = function (iso) {
  if (!iso) return { date: "", time: "" };
  var d = new Date(iso);
  if (isNaN(d.getTime())) {
    return { date: iso.slice(0, 10), time: iso.slice(11, 16) };
  }
  var pad = function (n) { return String(n).padStart(2, "0"); };
  return {
    date: d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()),
    time: pad(d.getHours()) + ":" + pad(d.getMinutes()),
  };
};

window.__bramHistoryDateRangeLine = function (group) {
  var phases = (group && group.phases) || [];
  if (!phases.length) return "";
  var first = window.__bramHistoryDateParts((phases[0] || {}).iso || "");
  var last = window.__bramHistoryDateParts((phases[phases.length - 1] || {}).iso || "");
  if (first.date && first.date === last.date) {
    return "On " + first.date + " from " + first.time + " to " + last.time;
  }
  return "From " + first.date + " " + first.time + " to " + last.date + " " + last.time;
};

window.__bramHistoryPhaseLabel = function (phase) {
  if (phase && phase.kind === "feedback") return "Feedback";
  var summary = ((phase && phase.summary) || "").toLowerCase();
  if (summary.indexOf("committed") >= 0) return "Committed";
  if (summary.indexOf("applied") >= 0) return "Applied";
  if (summary.indexOf("proposed") >= 0) return "Proposed";
  if (summary.indexOf("dropped") >= 0 || summary.indexOf("pruned") >= 0) return "Dropped";
  return (phase && phase.summary) || "Changed";
};

window.__bramHistoryPhasePath = function (group) {
  var phases = (group && group.phases) || [];
  var labels = [];
  for (var i = 0; i < phases.length; i++) {
    var label = window.__bramHistoryPhaseLabel(phases[i]);
    if (labels[labels.length - 1] !== label) labels.push(label);
  }
  return labels.join(" -> ");
};

window.__bramHistoryCommitUrl = function (group) {
  var phases = (group && group.phases) || [];
  for (var i = phases.length - 1; i >= 0; i--) {
    var phase = phases[i] || {};
    var summary = (phase.summary || "").toLowerCase();
    var url = typeof phase.commitUrl === "string" ? phase.commitUrl.trim() : "";
    if (url && summary.indexOf("committed") >= 0) return url;
  }
  return "";
};

window.__bramHistoryItemFieldMarkdown = function (group, field) {
  var item = window.__bramHistoryCurrentItem(group);
  var value = item && typeof item[field] === "string" ? item[field].trim() : "";
  return value || "";
};

window.__bramHistoryItemFilesLine = function (group) {
  var item = window.__bramHistoryCurrentItem(group);
  if (!item) return "";
  if (Array.isArray(item.files)) return item.files.join(", ");
  if (typeof item.file === "string") return item.file;
  return "";
};

window.__bramWorklistItemFiles = function (itemOrGroup) {
  var item = itemOrGroup;
  if (itemOrGroup && itemOrGroup.currentItem) {
    item = itemOrGroup.currentItem;
  }
  if (!item) return [];
  if (Array.isArray(item.files)) {
    return item.files
      .filter(function (file) {
        return typeof file === "string" && file.trim();
      })
      .map(function (file) { return file.trim(); });
  }
  if (typeof item.file === "string" && item.file.trim()) {
    return [item.file.trim()];
  }
  return [];
};

window.__bramHistoryLatestProseChanged = function (group) {
  var phase = window.__bramHistoryLatestPhase(group);
  var diff = (phase && phase.diff) || "";
  return diff.indexOf('"before"') >= 0 || diff.indexOf('"after"') >= 0;
};

window.__bramHistoryDraftWasMissing = function (group) {
  var item = window.__bramHistoryCurrentItem(group);
  return !!(item && item._draftMissing);
};

window.__bramHistoryItemFate = function (group) {
  var phases = (group && group.phases) || [];
  for (var i = phases.length - 1; i >= 0; i--) {
    var summary = ((phases[i] && phases[i].summary) || "").toLowerCase();
    if (summary.indexOf("committed") >= 0) return "Fate: committed.";
    if (summary.indexOf("dropped") >= 0 || summary.indexOf("pruned") >= 0) return "Fate: dropped.";
  }
  return "Fate: still active.";
};

window.__bramInflightSentinelDecide = function (data, prevSubmitting, prevSubmittedItemId) {
  var claimIds = (data && data.ids) || [];
  if (claimIds.length > 0) {
    var targeted = claimIds[0];
    var transitioning = !prevSubmitting || prevSubmittedItemId !== targeted;
    return {
      kind: "submit",
      submitting: transitioning ? true : prevSubmitting,
      submittedItemId: transitioning ? targeted : prevSubmittedItemId,
      actionProgressKind: (data && data.kind) || "",
    };
  } else if (prevSubmitting) {
    return {
      kind: "clear",
      trace: { reason: "sentinel-cleared", item: prevSubmittedItemId || "" },
    };
  }
  return { kind: "none" };
};

window.__bramRecordWorklistFeedbackConversation = function (text) {
  if (!text || !text.trim()) return false;
  var message = text.trim();
  var baseline = 0;
  __bramWriteLS("bram.worklistSubmittedMessage", message);
  __bramWriteSS("bram.worklistSessionSubmittedMessage", message);
  window.__bramSetWorklistSubmittedKind("action");
  return { message: message, images: __bramExtractImagePaths(message), baseline: baseline, sentAtText: new Date().toLocaleTimeString() };
};

window.__bramPrepareWorklistMessageSubmission = function (opts) {
  opts = opts || {};
  var rawText = opts.text || "";
  var skipWorklist = opts.mode === "skip-worklist";
  window.__bramWorklistMessageSubmissionSeq = (window.__bramWorklistMessageSubmissionSeq || 0) + 1;
  var seq = window.__bramWorklistMessageSubmissionSeq;
  // Every submit attempt traces BEFORE any gating/empty checks, so a send that silently goes nowhere is visible in bram-trace (2026-07-03: "message 2 resent got eaten" left zero traces).
  try {
    window.__bramIframeTrace("message-agent-submit", {
      stage: "attempt",
      seq: seq,
      chars: rawText.length,
      skipWorklist: skipWorklist,
    });
  } catch (e) {}
  if (skipWorklist && !rawText.trim()) return { submitted: false, seq: seq };
  var text = skipWorklist ? ("skip-worklist: " + rawText.trim()) : rawText;
  if (!text.trim()) return { submitted: false, seq: seq };

  if (window.__bramFlushWorklistDraft) window.__bramFlushWorklistDraft();
  var sent = window.__bramSubmitWorklistMessageFast(text);
  if (!sent) return { submitted: false, seq: seq };

  var pasteState = window.__bramPasteStateSnapshot(opts.voiceTarget || "message-agent");
  var submittedImages = sent.images || [];
  window.__bramIframeTrace("submitted-images", {
    kind: skipWorklist ? "message-skip-worklist" : "message",
    count: submittedImages.length,
    first: submittedImages[0] || "",
  });

  return {
    submitted: true,
    seq: seq,
    pendingPastedImageCount: pasteState.count,
    pendingPastedImagePaths: pasteState.paths,
    stagingPastedImageCount: pasteState.staging,
    submittedWorklistImages: submittedImages,
    submittedWorklistMessage: sent.message,
    messageSentAtText: sent.sentAtText,
    submittedKind: window.__bramSetWorklistSubmittedKind("message"),
    // Optimistic close; the host-derived awaitingTurn on /__send-ledger
    // takes over on the next refetch (issue-214-tranche-3b).
    awaitingResponse: true,
  };
};

window.__bramPrepareWorklistActionSubmission = function (opts) {
  opts = opts || {};
  window.__bramWorklistActionSubmissionSeq = (window.__bramWorklistActionSubmissionSeq || 0) + 1;
  var seq = window.__bramWorklistActionSubmissionSeq;
  var kind = opts.kind || "";
  var items = opts.items || [];
  var selectedId = opts.selectedId || "";
  var pasteTarget = opts.pasteTarget || ("feedback:" + selectedId);
  var rawFeedback = opts.rawFeedback || "";
  var feedback = window.__bramWithStagedImageMarkers(rawFeedback, pasteTarget);
  var displayItems = opts.displayItems || items;
  var displayText = window.__bramWorklistActionConversationDisplay(kind, displayItems, selectedId, feedback);
  var sent = window.__bramRecordWorklistFeedbackConversation(feedback ? (displayText + "\n\n" + feedback) : displayText);
  var submittedImages = [];
  var awaitingResponse = false;

  if (sent) {
    submittedImages = ((sent.images && sent.images.length > 0) ? sent.images : window.__bramExtractImagePaths(feedback));
    window.__bramIframeTrace("submitted-images", {
      kind: "action",
      action: opts.imageAction || kind,
      count: submittedImages.length,
      first: submittedImages[0] || "",
    });
    // Optimistic close; host-derived awaitingTurn takes over on the
    // next /__send-ledger refetch (issue-214-tranche-3b).
    awaitingResponse = true;
  }

  if (opts.inflightTarget) {
    window.__bramIframeTrace("inflight-set", {
      item: selectedId,
      via: "click",
      target: opts.inflightTarget,
    });
  }

  var feedbackDraftsById = opts.feedbackDraftsById || {};
  var nextFeedbackDrafts = Object.assign({}, feedbackDraftsById);
  delete nextFeedbackDrafts[selectedId];
  window.__bramPersistWorklistUiState({
    expandedItemIds: opts.expandedItemIds || [],
    feedbackDraftsById: nextFeedbackDrafts,
  });

  var payloadFeedback = Object.prototype.hasOwnProperty.call(opts, "payloadFeedback")
    ? opts.payloadFeedback
    : feedback;
  var turnText = "";
  var authorizationPayload = null;
  if (opts.payloadKind === "single-approve") {
    turnText = "approved: " + window.__bramBuildSingleItemApprovePayload(opts.itemRef, payloadFeedback);
    authorizationPayload = { kind: "approved", items: [{ id: opts.itemRef.id, feedback: payloadFeedback }] };
  } else if (kind === "approved") {
    turnText = "approved: " + window.__bramBuildApprovePayload(items, selectedId, payloadFeedback);
    authorizationPayload = { kind: "approved", items: window.__bramBuildApproveItems(items, selectedId, payloadFeedback) };
  } else if (kind === "drop") {
    turnText = "drop: " + window.__bramBuildDropPayload(items, selectedId, payloadFeedback);
    authorizationPayload = { kind: "drop", items: window.__bramBuildDropItems(items, selectedId, payloadFeedback) };
  }

  var pasteState = window.__bramPasteStateSnapshot(opts.voiceTarget || "message-agent");
  return {
    seq: seq,
    feedback: feedback,
    turnText: turnText,
    authorizationPayload: authorizationPayload,
    pendingPastedImageCount: pasteState.count,
    pendingPastedImagePaths: pasteState.paths,
    stagingPastedImageCount: pasteState.staging,
    submittedWorklistImages: submittedImages,
    submittedWorklistMessage: sent ? sent.message : "",
    messageSentAtText: sent ? sent.sentAtText : "",
    awaitingResponse: awaitingResponse,
    submittedItemId: selectedId,
    submittedKind: window.__bramSetWorklistSubmittedKind("action"),
    submitting: true,
    actionProgressKind: kind,
    actionProgressTick: 0,
    feedbackDraftsById: nextFeedbackDrafts,
  };
};

function __bramBuildCloseIssueLines(state) {
  var lines = [];
  Object.keys(state || {}).forEach(function (key) {
    var v = state[key];
    if (!v || !v.close) return;
    var comment = (v.comment || "").trim();
    if (comment) lines.push("close-issue: " + key + " comment: " + JSON.stringify(comment));
    else lines.push("close-issue: " + key);
  });
  return lines;
}

function __bramCombineFeedbackWithCloseLines(base, lines) {
  var baseTrim = (base || "").trim();
  var generated = [];
  if (lines && lines.length > 0) generated.push.apply(generated, lines);
  if (generated.length === 0) return baseTrim;
  if (!baseTrim) return generated.join("\n");
  return baseTrim + "\n\n" + generated.join("\n");
}

window.__bramPrepareCloseIssueWorklistActionSubmission = function (opts) {
  opts = opts || {};
  var item = opts.item || {};
  var feedbackDraftsById = opts.feedbackDraftsById || {};
  var rawFeedback = feedbackDraftsById[item.id] || "";
  var pasteTarget = "feedback:" + item.id;
  var payloadFeedback = rawFeedback;
  var imageAction = "approved-no-close";

  if (opts.closeIssues) {
    payloadFeedback = __bramCombineFeedbackWithCloseLines(
      window.__bramWithStagedImageMarkers(rawFeedback, pasteTarget),
      __bramBuildCloseIssueLines(opts.closeIssuesState),
    );
    imageAction = "approved-close";
  }

  return window.__bramPrepareWorklistActionSubmission({
    kind: "approved",
    items: [item],
    displayItems: [item],
    selectedId: item.id,
    itemRef: item,
    payloadKind: "single-approve",
    rawFeedback: rawFeedback,
    payloadFeedback: payloadFeedback,
    feedbackDraftsById: feedbackDraftsById,
    expandedItemIds: opts.expandedItemIds || [],
    voiceTarget: opts.voiceTarget || "message-agent",
    imageAction: imageAction,
  });
};

// Self-init: read `traces.enabled` from `/__settings` once at iframe
// load and cache the result on `window.__bramTracesEnabled`. The
// `iframeTrace` (above) and `logToHost` (above) bodies gate on
// this flag so trace-off sessions skip the IPC roundtrip entirely
// instead of paying the cost only for the host to drop the line.
// Default-ON until the fetch resolves preserves current behavior
// during the ~50 ms startup window. Iframe-reload re-runs this on
// every settings change (existing watcher pattern), so live
// reactivity isn't needed here.
(function loadTracesEnabledFlag() {
  if (typeof window === "undefined") return;
  if (window.__bramTracesEnabled !== undefined) return;
  window.__bramTracesEnabled = true;
  if (typeof fetch !== "function") return;
  fetch("/__settings")
    .then(function (r) { return r && r.ok ? r.json() : null; })
    .then(function (s) {
      if (s && s.traces && typeof s.traces.enabled === "boolean") {
        window.__bramTracesEnabled = s.traces.enabled;
      }
    })
    .catch(function () {});
})();

// Interleave devtools console output + unhandled-error paths into
// bram-trace.log via the iframe-trace channel. Catches what previously
// only landed in the browser devtools panel (e.g. the toolbar
// __toolbarPendingMenuPresent scope errors fixed in 4ad0716). Inherits
// the master-flag short-circuit via the gate in `logToHost` above.
//
// Uses window.logToHost directly rather than `window.iframeTrace`
// above; payload shape is the same (kind="iframe-trace", subkind=...)
// but the explicit logToHost call sidesteps a re-entrancy risk if
// iframeTrace ever logged a console error.
(function installConsoleInterleave() {
  if (typeof window.logToHost !== "function") return;
  if (window.__bramConsoleInterleaveInstalled) return;
  window.__bramConsoleInterleaveInstalled = true;

  var inTrace = false;
  function safeStringify(a) {
    try {
      return typeof a === "string" ? a : JSON.stringify(a);
    } catch (e) {
      return String(a);
    }
  }
  function consoleArgDetail(a) {
    var isError = a && (a instanceof Error || a.stack || a.message);
    if (isError) {
      return {
        type: (a && a.name) || "Error",
        message: String((a && a.message) || a),
        stack: a && a.stack ? String(a.stack) : "",
      };
    }
    return {
      type: typeof a,
      preview: safeStringify(a),
    };
  }
  function consoleArgDetails(args) {
    return args.map(consoleArgDetail);
  }
  function firstConsoleStack(args) {
    for (var i = 0; i < args.length; i += 1) {
      if (args[i] && args[i].stack) return String(args[i].stack);
    }
    return "";
  }
  function runtimeErrorFields(message, source, lineno, colno, error, via) {
    return {
      message: message || (error && error.message) || "window error",
      filename: source,
      lineno: lineno,
      colno: colno,
      errorName: error && error.name,
      errorMessage: error && error.message,
      stack: error && error.stack,
      source: via,
    };
  }
  function emit(subkind, fields) {
    if (inTrace) return;
    inTrace = true;
    try {
      var payload = {
        kind: "iframe-trace",
        subkind: subkind,
        at: new Date().toISOString(),
      };
      Object.keys(fields || {}).forEach(function (k) {
        if (fields[k] !== undefined) payload[k] = fields[k];
      });
      window.logToHost(payload);
    } catch (_) {}
    inTrace = false;
  }

  ["log", "warn", "error"].forEach(function (level) {
    var orig = console[level];
    console[level] = function () {
      var args = Array.prototype.slice.call(arguments);
      emit("console-" + level, {
        message: args.map(safeStringify).join(" "),
        args: consoleArgDetails(args),
        stack: firstConsoleStack(args),
      });
      orig.apply(console, args);
    };
  });

  var previousOnError = window.onerror;
  window.onerror = function (message, source, lineno, colno, error) {
    emit("console-error", runtimeErrorFields(message, source, lineno, colno, error, "window.onerror"));
    if (typeof previousOnError === "function") {
      return previousOnError.apply(this, arguments);
    }
    return false;
  };

  window.addEventListener("error", function (e) {
    emit("console-error", runtimeErrorFields(
      e && e.message,
      e && e.filename,
      e && e.lineno,
      e && e.colno,
      e && e.error,
      "window.error"
    ));
  });

  window.addEventListener("unhandledrejection", function (e) {
    var reason = e && e.reason;
    emit("console-unhandledrejection", {
      message:
        (reason && (reason.message || String(reason))) || "unhandled rejection",
      stack: reason && reason.stack,
    });
  });
})();
// Setter for window.__bramMenuPending, called from Globals.xs
// applyAgentMenu. XMLUI's expression engine can't handle
// `window.__bramMenuPending = ...` as an assignment target (it parses
// the LHS as a bare variable and emits "Left value variable
// (__bramMenuPending) not found in the scope"), but function calls on
// window members evaluate fine. Bridging through this setter keeps
// the assignment in plain-JS scope.
window.__bramSetMenuPending = function (v) {
  window.__bramMenuPending = !!v;
};

// Plain-JS wrappers for the agent-menu pty-menu-changed and
// turn-state-changed subscriber callbacks. XMLUI's expression engine runs subscriber
// arrow-function bodies through processStatementQueueAsync
// (xmlui/src/components-core/script-runner/process-statement-async.ts:115-166),
// which `await`s three times per statement — onStatementStarted,
// processStatementAsync, onStatementCompleted. Under iframe load
// each await is a microtask boundary that yields to the event
// loop, queueing the body behind pending macrotasks (DataSource
// polls, ChangeListener fires, JSONL broadcasts). End-to-end:
// 2-3 s between subscriber-fired (callback wrapper returns in 0 ms)
// and listener-fired (the iframeTrace inside setAgentMenuFromEvent
// actually emits). Collapsing the body to one window function call
// keeps applyAgentMenu, agentMenuTraceFields, iframeTrace, and the
// menu-pending mirror all on the synchronous JS side so the entire
// chain is one XMLUI statement instead of N.
// Native plain-JS AgentMenu state + handlers. Source of truth lives
// on window so xs scope can read it (Globals.xs getAgentMenu,
// Main.xmlui suppression gates) and JS scope can write it without
// going through XMLUI's expression engine.
//
// XMLUI evaluates xs function bodies via processStatementQueueAsync,
// awaiting three times per statement
// (xmlui/src/components-core/script-runner/process-statement-async.ts:115-166).
// Under iframe load — DataSource polls, ChangeListener fires, JSONL
// pipeline — each await yields to the event loop and the body
// serialises behind pending macrotasks. The full menu-state update
// (apply + trace) used to take 2-3 s end-to-end despite the JS-level
// subscriber wrapper returning in 0 ms. Doing the work natively
// here, before the XMLUI subscriber runs, drops that to the IPC
// delivery floor.
if (typeof window.bramAgentMenu === "undefined") window.bramAgentMenu = null;
if (typeof window.bramAgentMenuSuppressFallback === "undefined") window.bramAgentMenuSuppressFallback = true;
if (typeof window.bramAgentMenuLastHostMs === "undefined") window.bramAgentMenuLastHostMs = 0;
if (typeof window.bramAgentMenuLastSource === "undefined") window.bramAgentMenuLastSource = "";

function __bramAgentMenuHostMs(menu) {
  return menu && typeof menu.atHostMs === "number" ? menu.atHostMs : 0;
}

window.__bramHashString = function (text) {
  var s = String(text || "");
  var h = 2166136261;
  for (var i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0).toString(36);
};

window.__bramMenuIdentity = function (menu) {
  if (!menu) return "(none)";
  var opts = menu.options || [];
  var parts = [
    __bramAgentMenuHostMs(menu) || "",
    menu.cacheSource || "",
    menu.tool || "",
    menu.toolCallSignature || "",
    menu.toolCallContent || "",
    menu.text || "",
  ];
  for (var i = 0; i < opts.length; i++) {
    var opt = opts[i] || {};
    parts.push(opt.key || "");
    parts.push(opt.label || "");
  }
  return window.__bramHashString(parts.join("\n"));
};

function __bramAgentMenuTraceFields(menu) {
  var hostMs = __bramAgentMenuHostMs(menu);
  return {
    tool: (menu && menu.tool) || "",
    menuId: window.__bramMenuIdentity(menu),
    hasSignature: !!(menu && menu.toolCallSignature),
    signatureChars: menu && menu.toolCallSignature ? menu.toolCallSignature.length : 0,
    assignedMenu: window.bramAgentMenu ? window.bramAgentMenu.tool : "",
    suppressFallback: window.bramAgentMenuSuppressFallback,
    at_host_ms: hostMs,
    delta_to_emit_ms: hostMs ? (Date.now() - hostMs) : -1,
    cache_source: (menu && menu.cacheSource) || "",
    last_host_ms: window.bramAgentMenuLastHostMs,
    last_cache_source: window.bramAgentMenuLastSource,
    stale: hostMs && window.bramAgentMenuLastHostMs && hostMs < window.bramAgentMenuLastHostMs ? 1 : 0,
  };
}

function __bramEmitMenuTrace(subkind, fields) {
  if (typeof window.logToHost !== "function") return;
  var payload = { kind: "iframe-trace", subkind: subkind, at: new Date().toISOString() };
  Object.keys(fields || {}).forEach(function (k) {
    if (fields[k] !== undefined) payload[k] = fields[k];
  });
  window.logToHost(payload);
}

window.__bramApplyAgentMenu = function (menu, suppressFallback, source) {
  var hostMs = __bramAgentMenuHostMs(menu);
  var stale = !!(hostMs && window.bramAgentMenuLastHostMs && hostMs < window.bramAgentMenuLastHostMs);
  if (stale) {
    __bramEmitMenuTrace("agent-menu-stale", {
      incoming_host_ms: hostMs,
      current_host_ms: window.bramAgentMenuLastHostMs,
      incoming_source: (menu && menu.cacheSource) || source || "",
      current_source: window.bramAgentMenuLastSource,
      incoming_tool: (menu && menu.tool) || "",
      current_tool: (window.bramAgentMenu && window.bramAgentMenu.tool) || "",
    });
    return true;
  }
  window.bramAgentMenu = menu || null;
  // Menu-row trace at the canonical setter — the single place the applied
  // menu state changes, run once per change with no churning subscriber.
  // Deduped by menu key; emits transcript-menu-row stage=source. Gated to
  // the agent pane (/tools/): helpers.js loads in both iframes and each
  // would emit, but the inline-menu render staleness only manifests where
  // the Transcript lives. Pairs with host pty-menu-changed to localize it:
  // a clean object here + a blended row on screen => render layer; a fused
  // object here => data layer.
  try {
    if (window.location.pathname.indexOf("/tools/") !== -1 &&
        window.__bramMenuRowKey && window.__bramTraceMenuRow) {
      var __menuRowKey = window.__bramMenuRowKey(window.bramAgentMenu);
      if (__menuRowKey !== window.__bramMenuRowTraceLastKey) {
        window.__bramMenuRowTraceLastKey = __menuRowKey;
        window.__bramTraceMenuRow(window.bramAgentMenu, "source");
      }
    }
  } catch (e) {}
  // backgrounded-pane-menu-paint-observer: receive-vs-paint marker.
  // Double-rAF is the paint proxy — the second callback runs only after
  // a real frame, and rAF stalls while the webview is hidden/throttled,
  // so a menu received hidden that paints only on refocus shows up as
  // receive_to_paint_ms spanning the hidden period with
  // painted_after_refocus=true (paired with the pane-visibility lines
  // and the host's prompt-lifecycle op=shown). Gated to the agent pane;
  // observe-only, one probe per applied menu.
  try {
    if (menu && window.location.pathname.indexOf("/tools/") !== -1) {
      var __paintReceiveMs = Date.now();
      var __paintHiddenAtReceive = !!document.hidden;
      var __paintFocusedAtReceive = !!(document.hasFocus && document.hasFocus());
      var __paintTool = menu.tool || "";
      var __paintMenuId = window.__bramMenuIdentity ? window.__bramMenuIdentity(menu) : __paintTool;
      requestAnimationFrame(function () {
        requestAnimationFrame(function () {
          var __paintMs = Date.now();
          window.__bramIframeTrace("menu-paint", {
            tool: __paintTool,
            menuId: __paintMenuId,
            hidden_at_receive: __paintHiddenAtReceive,
            focused_at_receive: __paintFocusedAtReceive,
            receive_to_paint_ms: __paintMs - __paintReceiveMs,
            painted_after_refocus:
              __paintHiddenAtReceive &&
              (window.__bramPaneLastVisibleMs || 0) > __paintReceiveMs,
          });
        });
      });
    }
  } catch (e) {}
  window.bramAgentMenuSuppressFallback = suppressFallback;
  window.__bramMenuPending = !!menu;
  if (hostMs) {
    window.bramAgentMenuLastHostMs = hostMs;
    window.bramAgentMenuLastSource = (menu && menu.cacheSource) || source || "";
  } else if (!menu) {
    window.bramAgentMenuLastHostMs = Date.now();
    window.bramAgentMenuLastSource = source || "";
  }
  return false;
};

window.__bramTraceAgentMenuRender = function (menu, surface) {
  try {
    window.__bramIframeTrace("agent-menu-render", {
      surface: surface || "",
      present: !!menu,
      tool: (menu && menu.tool) || "",
      options: (menu && menu.options && menu.options.length) || 0,
      menuId: window.__bramMenuIdentity(menu),
      transcriptMounted: !!window.__bramTranscriptMounted,
    });
  } catch (e) {}
};

window.__bramSetAgentMenuFromEvent = function (e, surface) {
  var payload = e && e.payload ? e.payload : null;
  var incoming = payload && payload.tool ? payload : null;
  var stale = window.__bramApplyAgentMenu(incoming, !incoming, "setAgentMenuFromEvent");
  var fields = __bramAgentMenuTraceFields(incoming);
  fields.context = "pty-menu-changed";
  fields.surface = surface || "agent-menu";
  fields.stale = stale;
  __bramEmitMenuTrace("listener-fired", fields);
};

window.__bramSetAgentMenuFromTurnState = function (turnState, surface) {
  var p = turnState || {};
  var incoming = p.pendingMenu || null;
  var stale = window.__bramApplyAgentMenu(incoming, !incoming, "setAgentMenuFromTurnState");
  var fields = __bramAgentMenuTraceFields(incoming);
  fields.context = "turn-state-changed";
  fields.surface = surface || "agent-menu";
  fields.phase = p.phase || "";
  fields.source = p.source || "";
  fields.menu = p.pendingMenu ? p.pendingMenu.tool : "";
  fields.stale = stale;
  __bramEmitMenuTrace("listener-fired", fields);
};

// Native subscriber registration lives further down in this file
// (search "__bramNativePtyMenuUnsub"). subscribeTauriEvent is defined
// later than this block, so calling it here at top level throws and
// aborts the rest of the script — taking down voice helpers, the
// console-interleave, and the Tauri-listener machinery itself
// (incident 2026-06-14: blank menus + voice broken). Register after
// subscribeTauriEvent exists.
window.openExternal = function (url) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  return invoke("open_url", { url: String(url) }).catch(function (e) {
    console.error("openExternal open_url", e);
    if (typeof window.__bramShowLinkPreviewError === "function") {
      window.__bramShowLinkPreviewError(String(url), String(e && e.message || e));
    }
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

// Stage a clipboard-pasted image to disk via /__paste-image and remember its
// path so submitWorklistMessageFast can prepend the `[Image: source: <path>]`
// marker on the next form submit. Mirrors the marker protocol that
// captureScreenshot uses and that st_extract_image_paths reads back.
//
// We listen for paste events at document level so any Cmd/Ctrl+V — including
// one fired from the TextArea — stages clipboard images. The original
// FileUploadDropZone-based UX required clicking the dropzone first, but the
// underlying react-dropzone setup is configured with noKeyboard:true, which
// strips the rootDiv's tabIndex (react-dropzone/src/index.js:920); without
// focus the rootDiv never receives the React paste event, so click-then-paste
// silently no-ops. Window-level listening sidesteps the focus problem.
window.bramPendingPastedImages = window.bramPendingPastedImages || [];
window.bramStagingPastedImages = window.bramStagingPastedImages || 0;

// Paste-state pub/sub registry — bridge from helpers.js (canonical store) to
// XMLUI via the <External> component's `(emit) => unsubscribe` contract.
// helpers.js owns window.bramPendingPastedImages and
// window.bramStagingPastedImages above; every mutation site below calls
// bramNotifyPasteState() so the subscribers below re-snapshot and push the
// new value to their XMLUI-side observers. Replaces the 4 Hz <Timer> polling
// loop the strip used to do.
var bramPasteStateSubscribers = new Set();
function bramComputePasteState(target) {
  return {
    count: target
      ? window.bramPendingPastedImageCountForTarget(target)
      : window.bramPendingPastedImageCount(),
    paths: target
      ? window.bramPendingPastedImagePathsForTarget(target)
      : window.bramPendingPastedImagePaths(),
    staging: window.bramStagingPastedImageCount(),
  };
}
window.__bramPasteStateSnapshot = function (target) {
  return bramComputePasteState(target);
};
function bramNotifyPasteState() {
  bramPasteStateSubscribers.forEach(function (cb) {
    try { cb(); } catch (e) { console.error("[bram-paste] subscriber threw:", e); }
  });
}
// Memoize the per-target subscribe closure. XMLUI re-evaluates
// `subscribe="{window.bramSubscribePasteState(target)}"` on every render;
// returning a fresh closure each call makes the <External> useEffect's
// [subscribeFn] dep see a new identity each time, which kicks off a
// subscribe → emit → re-render → re-subscribe loop. Caching keyed on
// target gives every call with the same target the same function
// identity, so useEffect runs exactly once per real target change.
var bramSubscribePasteStateCache = Object.create(null);
window.bramSubscribePasteState = function (target) {
  var key = target == null ? "" : String(target);
  if (bramSubscribePasteStateCache[key]) return bramSubscribePasteStateCache[key];
  var cached = function (emit) {
    var fire = function () { emit(bramComputePasteState(target)); };
    bramPasteStateSubscribers.add(fire);
    fire();  // seed initial value synchronously
    return function () { bramPasteStateSubscribers.delete(fire); };
  };
  bramSubscribePasteStateCache[key] = cached;
  return cached;
};
window.bramActiveVoiceTargetMirror = window.bramActiveVoiceTargetMirror || "";
window.bramActiveFocusedFeedbackItemIdMirror = window.bramActiveFocusedFeedbackItemIdMirror || "";
window.bramSetActiveVoiceTargetMirror = function (v) {
  var prev = window.bramActiveVoiceTargetMirror || "";
  var next = v || "";
  window.bramActiveVoiceTargetMirror = next;
  if (window.__bramIframeTrace) window.__bramIframeTrace("paste-target-mirror", { kind: "voice", value: next, prev: prev });
};
window.bramSetActiveFocusedFeedbackItemIdMirror = function (v) {
  var prev = window.bramActiveFocusedFeedbackItemIdMirror || "";
  var next = v || "";
  window.bramActiveFocusedFeedbackItemIdMirror = next;
  if (window.__bramIframeTrace) window.__bramIframeTrace("paste-target-mirror", { kind: "focused-feedback-item", value: next, prev: prev });
};
window.bramCurrentPasteTarget = function () {
  var voice = window.bramActiveVoiceTargetMirror || "";
  var focusedFeedback = window.bramActiveFocusedFeedbackItemIdMirror || "";
  var active = document.activeElement;
  var placeholder = active && active.getAttribute && (active.getAttribute("placeholder") || "");
  var activeLooksLikeFeedback = placeholder === "Message to agent";
  var activeLooksLikeMessage = placeholder.indexOf("Message agent") === 0;
  var result;
  if (activeLooksLikeFeedback && focusedFeedback) {
    result = "feedback:" + focusedFeedback;
  } else if (activeLooksLikeMessage) {
    result = "message-agent";
  } else {
    result = voice;
  }
  if (window.__bramIframeTrace) window.__bramIframeTrace("paste-current-target", {
    voice: voice,
    focusedFeedback: focusedFeedback,
    placeholder: placeholder,
    activeLooksLikeFeedback: activeLooksLikeFeedback,
    activeLooksLikeMessage: activeLooksLikeMessage,
    result: result
  });
  return result;
};
window.bramPastedImageForCurrentTurn = window.bramPastedImageForCurrentTurn || false;
window.bramPastedImageTarget = window.bramPastedImageTarget || "";
window.bramLastConsumedPastedImages = window.bramLastConsumedPastedImages || [];
window.bramPasteImageTraceSigs = window.bramPasteImageTraceSigs || {};
function bramPendingPastedImageSummary() {
  return (window.bramPendingPastedImages || []).map(function (e) {
    if (typeof e === "string") return { path: e, target: "" };
    return { path: (e && e.path) || "", target: (e && e.target) || "" };
  }).filter(function (e) { return !!e.path; });
}
function bramActiveElementSummary() {
  var el = document.activeElement;
  if (!el) return "";
  var bits = [];
  if (el.tagName) bits.push(String(el.tagName).toLowerCase());
  if (el.id) bits.push("#" + el.id);
  var aria = el.getAttribute && (el.getAttribute("aria-label") || el.getAttribute("placeholder"));
  if (aria) bits.push("[" + String(aria).slice(0, 40) + "]");
  return bits.join("");
}
function bramTracePasteImage(stage, payload, sampleKey) {
  try {
    var p = Object.assign({ stage: stage }, payload || {});
    if (sampleKey) {
      var sig = JSON.stringify(p);
      if (window.bramPasteImageTraceSigs[sampleKey] === sig) return;
      window.bramPasteImageTraceSigs[sampleKey] = sig;
    }
    if (typeof window.__bramIframeTrace === "function") {
      window.__bramIframeTrace("paste-image", p);
    }
  } catch (e) {}
}
document.addEventListener("paste", function (event) {
  if (!event.clipboardData) return;
  var items = event.clipboardData.items || [];
  var imageFiles = [];
  for (var i = 0; i < items.length; i++) {
    var item = items[i];
    if (item.kind === "file" && /^image\//.test(item.type || "")) {
      var f = item.getAsFile();
      if (f) imageFiles.push(f);
    }
  }
  if (imageFiles.length === 0) return;
  // Accumulate pasted images across paste events within a single turn.
  // Originally (804bc37) this point cleared `bramPendingPastedImages`
  // on every paste to avoid sticking on stale images from abandoned
  // drafts, but the clear made multi-paste-event accumulation
  // impossible — pasting four screenshots one after another into a
  // single Iterate feedback box dropped all but one (race-dependent
  // first or last). Staleness is now handled by
  // `bramConsumePastedImagePaths` on turn submission and by the
  // `bramPastedImageForCurrentTurn` flag below.
  window.bramPastedImageForCurrentTurn = true;
  var currentTarget = (window.bramCurrentPasteTarget && window.bramCurrentPasteTarget()) || "";
  var pasteTarget = currentTarget || "message-agent";
  window.bramPastedImageTarget = pasteTarget;
  bramTracePasteImage("intake", {
    source: "paste",
    currentTarget: currentTarget,
    target: pasteTarget,
    activeElement: bramActiveElementSummary(),
    fileCount: imageFiles.length,
    pendingBefore: bramPendingPastedImageSummary()
  });
  // Suppress the default paste so the TextArea doesn't pick up any file-path
  // or filename text the OS may have placed on the clipboard alongside the
  // image (Finder copy-image, macOS screenshot tool, etc.).
  event.preventDefault();
  for (var j = 0; j < imageFiles.length; j++) {
    window.bramStagePastedImage(imageFiles[j], pasteTarget);
  }
});
// Drag-and-drop image intake — parallels the paste handler above.
function bramImageFilesFromDataTransfer(dt) {
  if (!dt) return [];
  var imageFiles = [];
  var items = dt.items || [];
  for (var i = 0; i < items.length; i++) {
    var item = items[i];
    if (item.kind === "file" && /^image\//.test(item.type || "")) {
      var f = item.getAsFile();
      if (f) imageFiles.push(f);
    }
  }
  if (imageFiles.length > 0) return imageFiles;
  var files = dt.files || [];
  for (var j = 0; j < files.length; j++) {
    var file = files[j];
    if (file && /^image\//.test(file.type || "")) imageFiles.push(file);
  }
  return imageFiles;
}
document.addEventListener("dragover", function (event) {
  if (bramImageFilesFromDataTransfer(event.dataTransfer).length === 0) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
});
document.addEventListener("drop", function (event) {
  var imageFiles = bramImageFilesFromDataTransfer(event.dataTransfer);
  if (imageFiles.length === 0) return;
  window.bramPastedImageForCurrentTurn = true;
  var currentTarget = (window.bramCurrentPasteTarget && window.bramCurrentPasteTarget()) || "";
  var dropTarget = currentTarget || "message-agent";
  window.bramPastedImageTarget = dropTarget;
  bramTracePasteImage("intake", {
    source: "drop",
    currentTarget: currentTarget,
    target: dropTarget,
    activeElement: bramActiveElementSummary(),
    fileCount: imageFiles.length,
    pendingBefore: bramPendingPastedImageSummary()
  });
  event.preventDefault();
  for (var i = 0; i < imageFiles.length; i++) {
    window.bramStagePastedImage(imageFiles[i], dropTarget);
  }
});
window.bramStagePastedImage = function (file, target) {
  if (!file) return Promise.reject(new Error("no file"));
  var type = file.type || "image/png";
  var url = "/__paste-image?type=" + encodeURIComponent(type);
  var stageTarget = target || window.bramPastedImageTarget || "message-agent";
  // Read as ArrayBuffer first. `fetch(url, { body: file })` with a File body
  // in this Tauri webview wrote 0-byte files server-side (the host saw an
  // empty request body). Sending an ArrayBuffer via fetch reliably carries
  // the bytes through.
  return new Promise(function (resolve, reject) {
    var reader = new FileReader();
    window.bramStagingPastedImages++;
    bramNotifyPasteState();
    bramTracePasteImage("stage-start", { target: stageTarget, type: type, staging: window.bramStagingPastedImages });
    reader.onload = function () {
      if (!reader.result || reader.result.byteLength === 0) {
        var empty = new Error("paste-image: empty clipboard image");
        bramTracePasteImage("empty", { target: stageTarget });
        window.bramStagingPastedImages = Math.max(0, (window.bramStagingPastedImages || 0) - 1);
        bramNotifyPasteState();
        reject(empty);
        return;
      }
      fetch(url, {
        method: "POST",
        body: reader.result,
        headers: { "Content-Type": type },
      })
        .then(function (r) {
          if (!r.ok) throw new Error("paste-image HTTP " + r.status);
          return r.json();
        })
        .then(function (json) {
          if (!json || !json.path) throw new Error("paste-image: no path in response");
          var entry = { path: json.path, target: stageTarget };
          window.bramPendingPastedImages.push(entry);
          bramNotifyPasteState();
          bramTracePasteImage("staged", {
            path: json.path,
            target: stageTarget,
            currentGlobalTarget: window.bramPastedImageTarget || "",
            bytes: reader.result.byteLength,
            pendingAfter: bramPendingPastedImageSummary()
          });
          resolve(json.path);
        })
        .catch(function (e) {
          bramTracePasteImage("error", { target: stageTarget, message: String((e && e.message) || e) });
          reject(e);
        })
        .finally(function () {
          window.bramStagingPastedImages = Math.max(0, (window.bramStagingPastedImages || 0) - 1);
          bramNotifyPasteState();
        });
    };
    reader.onerror = function () {
      bramTracePasteImage("read-error", { target: stageTarget, message: String(reader.error || "") });
      window.bramStagingPastedImages = Math.max(0, (window.bramStagingPastedImages || 0) - 1);
      bramNotifyPasteState();
      reject(reader.error);
    };
    reader.readAsArrayBuffer(file);
  });
};
window.bramConsumePastedImagePaths = function (target) {
  if (!window.bramPastedImageForCurrentTurn) {
    window.bramPendingPastedImages = [];
    window.bramPastedImageForCurrentTurn = false;
    window.bramPastedImageTarget = "";
    window.bramLastConsumedPastedImages = [];
    bramTracePasteImage("consume", { target: target || "", reason: "no-current-turn", consumed: [], retained: [] });
    bramNotifyPasteState();
    return [];
  }
  var arr = window.bramPendingPastedImages || [];
  if (!target) {
    var allPaths = arr.map(function (e) { return e && e.path; }).filter(Boolean);
    window.bramPendingPastedImages = [];
    window.bramPastedImageForCurrentTurn = false;
    window.bramPastedImageTarget = "";
    window.bramLastConsumedPastedImages = allPaths.slice();
    bramTracePasteImage("consume", { target: "", mode: "drain-all", consumed: allPaths, retained: [] });
    bramNotifyPasteState();
    return allPaths;
  }
  var kept = [];
  var taken = [];
  for (var i = 0; i < arr.length; i++) {
    var e = arr[i];
    if (e && (e.target || "") === target) {
      if (e.path) taken.push(e.path);
    } else if (e) {
      kept.push(e);
    }
  }
  window.bramPendingPastedImages = kept;
  if (kept.length === 0) {
    window.bramPastedImageForCurrentTurn = false;
    window.bramPastedImageTarget = "";
  }
  window.bramLastConsumedPastedImages = taken.slice();
  bramTracePasteImage("consume", {
    target: target,
    mode: "target",
    consumed: taken,
    retained: bramPendingPastedImageSummary()
  });
  bramNotifyPasteState();
  return taken;
};
window.bramLastConsumedPastedImagePaths = function () {
  return (window.bramLastConsumedPastedImages || []).slice();
};
window.bramRemovePastedImagePath = function (path) {
  if (!path) return;
  var arr = window.bramPendingPastedImages || [];
  for (var i = 0; i < arr.length; i++) {
    var e = arr[i];
    if (e && e.path === path) {
      arr.splice(i, 1);
      bramTracePasteImage("removed", { path: path, target: e.target || "", pendingAfter: bramPendingPastedImageSummary() });
      bramNotifyPasteState();
      return;
    }
  }
};
window.bramHasPendingPastedImages = function () {
  return (window.bramPendingPastedImages || []).length > 0;
};
window.bramPendingPastedImageCount = function () {
  return (window.bramPendingPastedImages || []).length;
};
window.bramPendingPastedImageCountForTarget = function (target) {
  var t = target || "";
  var count = (window.bramPendingPastedImages || []).filter(function (e) {
    return e && (e.target || "") === t;
  }).length;
  bramTracePasteImage("query-count", { target: t, count: count }, "count:" + t);
  return count;
};
window.bramPendingPastedImagePaths = function () {
  return (window.bramPendingPastedImages || []).map(function (e) { return e && e.path; }).filter(Boolean);
};
window.bramPendingPastedImagePathsForTarget = function (target) {
  var t = target || "";
  var paths = (window.bramPendingPastedImages || [])
    .filter(function (e) { return e && (e.target || "") === t; })
    .map(function (e) { return e.path; })
    .filter(Boolean);
  bramTracePasteImage("query-paths", { target: t, count: paths.length, paths: paths }, "paths:" + t);
  return paths;
};
window.bramTracePastedImageStrip = function (source, target, count, paths, staging) {
  bramTracePasteImage("strip", {
    source: source || "",
    target: target || "",
    count: count || 0,
    paths: paths || [],
    staging: staging || 0
  }, "strip:" + (source || "") + ":" + (target || ""));
};
window.bramStagingPastedImageCount = function () {
  return window.bramStagingPastedImages || 0;
};

// Click-to-toggle voice. Single in-flight session per iframe.
//   voiceStart()              — starts recording (parent records on iframe's behalf).
//   voiceStop(callback)       — stops; callback(transcript) fires when transcript is ready.
// XMLUI's onClick expression evaluator does not reliably execute .then() callbacks
// attached during expression evaluation; passing a callback function as an argument
// works, since the callback is invoked from plain JS later.
window._voiceSession = null;
window._voiceStartedListener = null;
window._voiceSessionTarget = "";
window.__bramVoiceRecorderState = window.__bramVoiceRecorderState || {
  state: "idle",
  requestId: null,
  target: "",
  at: Date.now(),
};
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
window.__bramHasActiveVoiceSession = function () {
  return !!window._voiceSession;
};
window.__bramActiveVoiceSessionTarget = function () {
  return window._voiceSessionTarget || "";
};
window.__bramNotifyVoiceBusy = function (detail) {
  try {
    window.dispatchEvent(new CustomEvent("bram:voice-busy", {
      detail: Object.assign({ at: Date.now() }, detail || {}),
    }));
  } catch (e) {
    console.error("[bram] voice-busy dispatch failed:", e);
  }
};
function _voiceRemoveStartedListener() {
  if (window._voiceStartedListener) {
    try {
      window.removeEventListener("message", window._voiceStartedListener);
    } catch (e) {}
    window._voiceStartedListener = null;
  }
}
window.voiceStart = function (onStarted, onFailed) {
  var meta =
    arguments.length >= 3 && arguments[2] && typeof arguments[2] === "object"
      ? arguments[2]
      : {};
  if (window._voiceSession) {
    _voiceLog("voiceStart-rejected-already-active", {
      currentSession: window._voiceSession,
      target: window._voiceSessionTarget || "",
    });
    if (typeof onFailed === "function") {
      try {
        onFailed({
          requestId: window._voiceSession,
          reason: "already-active",
          target: window._voiceSessionTarget || "",
        });
      } catch (e) {}
    }
    return;
  }
  _voiceRemoveStartedListener();
  var requestId =
    "voice-" + Date.now() + "-" + Math.random().toString(36).slice(2);
  window._voiceSession = requestId;
  window._voiceSessionTarget = meta.target || "";
  _voiceLog("voiceStart", { requestId: requestId, target: window._voiceSessionTarget });
  function onStartedMsg(ev) {
    var data = ev && ev.data;
    if (!data || (data.type !== "voice-recording-started" && data.type !== "voice-into-result")) return;
    if (data.requestId !== requestId) return;
    window.removeEventListener("message", onStartedMsg);
    if (window._voiceStartedListener === onStartedMsg) {
      window._voiceStartedListener = null;
    }
    if (data.type === "voice-into-result") {
      if (window._voiceSession === requestId) {
        window._voiceSession = null;
        window._voiceSessionTarget = "";
      }
      _voiceLog("voiceStart-rejected-by-parent", {
        requestId: requestId,
        reason: data.reason || "",
        activeWas: data.activeWas || "",
        activeRequestId: data.activeRequestId || "",
        transcriptLength: String(data.transcript || "").length,
      });
      if (typeof onFailed === "function") {
        try { onFailed(data); } catch (e) {}
      }
      return;
    }
    if (window._voiceSession !== requestId) {
      _voiceLog("voice-recording-started-stale", { requestId: requestId });
      return;
    }
    _voiceLog("voice-recording-started", { requestId: requestId });
    if (typeof onStarted === "function") {
      try { onStarted(); } catch (e) {}
    }
  }
  window._voiceStartedListener = onStartedMsg;
  window.addEventListener("message", onStartedMsg);
  window.parent.postMessage(
    {
      type: "right-pane",
      kind: "voice-start",
      requestId: requestId,
      target: window._voiceSessionTarget,
    },
    "*",
  );
};
window.voiceStop = function (callback) {
  var requestId = window._voiceSession;
  var target = window._voiceSessionTarget || "";
  var stopAtMs = Date.now();
  window._voiceSession = null;
  window._voiceSessionTarget = "";
  _voiceRemoveStartedListener();
  if (!requestId) {
    _voiceLog("voiceStop-no-session", { stopAtMs: stopAtMs });
    if (typeof callback === "function") callback("");
    return;
  }
  _voiceLog("voiceStop", { requestId: requestId, stopAtMs: stopAtMs, target: target });
  function onResult(ev) {
    var data = ev && ev.data;
    if (!data || data.type !== "voice-into-result") return;
    var resultAtMs = Date.now();
    if (data.requestId !== requestId) {
      _voiceLog("voice-into-result-mismatch", {
        expected: requestId,
        received: data.requestId,
        stopAtMs: stopAtMs,
        stopToResultMs: resultAtMs - stopAtMs,
        transcriptPreview: String(data.transcript || "").slice(0, 80),
      });
      return;
    }
    window.removeEventListener("message", onResult);
    var transcript = String(data.transcript || "");
    var resultStopAtMs = Number(data.stopAtMs || stopAtMs);
    var voiceMeta = {
      requestId: requestId,
      stopAtMs: resultStopAtMs,
      stopToResultMs: resultAtMs - resultStopAtMs,
      parentStopToDeliverMs:
        typeof data.stopToDeliverMs === "number" ? data.stopToDeliverMs : null,
      target: data.target || target || "",
    };
    _voiceLog("voice-into-result", {
      requestId: requestId,
      stopAtMs: resultStopAtMs,
      stopToResultMs: voiceMeta.stopToResultMs,
      parentStopToDeliverMs: voiceMeta.parentStopToDeliverMs,
      target: voiceMeta.target,
      transcriptLength: transcript.length,
      transcriptPreview: transcript.slice(0, 80),
    });
    if (typeof callback === "function") callback(transcript, voiceMeta);
  }
  window.addEventListener("message", onResult);
  window.parent.postMessage(
    {
      type: "right-pane",
      kind: "voice-stop",
      requestId: requestId,
      target: target,
      stopAtMs: stopAtMs,
    },
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
// iframes consume them through this bridge. It is the change-signal tick
// that drives the projected-turns refetch on provider session-file writes.
var __talkSessionSubscribers = [];
var __talkSessionMainUnsub = null;
window.onTalkSessionChange = function (fn) {
  if (typeof __talkSessionMainUnsub === "function") {
    try { __talkSessionMainUnsub(); } catch (e) {}
    __talkSessionMainUnsub = null;
  }
  if (typeof fn !== "function") return function () {};
  __talkSessionMainUnsub = window.subscribeTalkSessionChange("__bramMainTalkSessionUnsub", fn);
  return __talkSessionMainUnsub;
};
window.subscribeTalkSessionChange = function (key, fn) {
  if (typeof window[key] === "function") {
    try { window[key](); } catch (e) {}
  }
  if (typeof fn !== "function") {
    window[key] = null;
    return function () {};
  }
  __talkSessionSubscribers.push(fn);
  // Subscriber-lifecycle trace for the talk-session event-drop
  // investigation (#tsc-drop): a sub/resub churn pattern would explain
  // some of the 175→83 delivery gap if the parent listen() were
  // racing the iframe's swap window.
  try {
    if (typeof window.logToHost === "function") {
      window.logToHost({
        kind: "iframe-trace",
        subkind: "subscriber-changed",
        at: new Date().toISOString(),
        context: "talk-session-changed",
        op: "subscribe",
        key: key,
        count: __talkSessionSubscribers.length,
      });
    }
  } catch (e) {}
  window[key] = function () {
    var idx = __talkSessionSubscribers.indexOf(fn);
    if (idx >= 0) __talkSessionSubscribers.splice(idx, 1);
    try {
      if (typeof window.logToHost === "function") {
        window.logToHost({
          kind: "iframe-trace",
          subkind: "subscriber-changed",
          at: new Date().toISOString(),
          context: "talk-session-changed",
          op: "unsubscribe",
          key: key,
          count: __talkSessionSubscribers.length,
        });
      }
    } catch (e) {}
    window[key] = null;
  };
  return window[key];
};
// Cascade-diagnosis instrumentation (refs #93). Counts every
// talk-session-changed delivery and emits a rolling batch record
// every 10 events so we can see per-event cost + frequency without
// flooding bram-trace.
var __tscBatch = { count: 0, totalMs: 0, maxMs: 0, sinceMs: 0 };
function __tscBatchTick(elapsedMs) {
  if (__tscBatch.count === 0) __tscBatch.sinceMs = Date.now();
  __tscBatch.count += 1;
  __tscBatch.totalMs += elapsedMs;
  if (elapsedMs > __tscBatch.maxMs) __tscBatch.maxMs = elapsedMs;
  if (__tscBatch.count >= 10) {
    try {
      if (typeof window.logToHost === "function" && !window.__bramMenuPending) {
        window.logToHost({
          kind: "iframe-trace",
          subkind: "talk-session-batch",
          at: new Date().toISOString(),
          count: __tscBatch.count,
          sumMs: Math.round(__tscBatch.totalMs * 10) / 10,
          avgMs: Math.round((__tscBatch.totalMs / __tscBatch.count) * 10) / 10,
          maxMs: Math.round(__tscBatch.maxMs * 10) / 10,
          spanMs: Date.now() - __tscBatch.sinceMs,
        });
      }
    } catch (e) {}
    __tscBatch = { count: 0, totalMs: 0, maxMs: 0, sinceMs: 0 };
  }
}
// Parent-window-scoped Tauri-listener dedup, fixing the iframe-reload
// accumulation leak.
//
// Both ev.listen() call sites in this file (the direct
// talk-session-changed listener below and the dynamic one inside
// __ensureTauriEventListener) register on `window.parent.__TAURI__.event`,
// which lives on the parent shell webview and PERSISTS across iframe
// reloads. The iframe's own module-level state
// (__tauriEventListening / __tauriEventSubscribers) re-initialises on
// every load, so each fresh load thought no listener existed and
// registered another one — old closures from prior loads stayed live
// on the parent registry. One host emit then fanned out to N copies
// of every subscriber, multiplying refetch-called fires, debounce
// schedules, DataSource reloads, etc.
//
// Symptom we measured during the Globals.xs migration (commit d532432):
// listener-fired count per pty-menu-changed event grew from 4 → 5
// across two manual reloads of the same Bram session. Same pattern
// for talk-session-changed.
//
// Fix: keep a parent-window-scoped map of eventName → unsub function
// (or pending listen() promise). On each iframe load, drain the
// stale entry before calling ev.listen() again. Trace the drain so
// we can verify the dedup is firing.
function __bramListenWithDedup(ev, eventName, callback) {
  if (!ev || typeof ev.listen !== "function") return Promise.resolve(null);
  var parent;
  try {
    parent = (window.parent && window.parent !== window) ? window.parent : window;
  } catch (e) {
    parent = window;
  }
  try {
    if (!parent.__bramTauriListenerUnsubs) parent.__bramTauriListenerUnsubs = {};
  } catch (e) {}
  var store = null;
  try { store = parent.__bramTauriListenerUnsubs; } catch (e) {}
  // Dedup key must include iframe identity, not just eventName. Tools-pane
  // and right-pane both register Tauri listeners against the parent webview
  // (window.parent.__TAURI__.event), and each iframe's listener callback
  // closes over its OWN __tauriEventSubscribers array. Keying by eventName
  // alone made any later iframe's load drain the prior iframe's listener —
  // leaving the orphaned iframe's subscriber array (AgentMenu + Toolbar +
  // native, for the tools-pane) silently unwatched, so menus didn't render
  // on cold start until a manual reload made the affected iframe the last
  // to register. Same-iframe reloads still drain themselves (the original
  // 4→5 stale-listener bug from commit d532432 stays fixed).
  var iframeKey = (function () {
    try { return window.location.pathname || ""; } catch (e) { return ""; }
  })();
  var storeKey = eventName + "::" + iframeKey;
  var stale = store ? store[storeKey] : null;
  if (stale) {
    try {
      if (typeof stale === "function") {
        try { stale(); } catch (e) {}
      } else if (stale && typeof stale.then === "function") {
        stale.then(function (fn) { if (typeof fn === "function") { try { fn(); } catch (e) {} } }, function () {});
      }
    } catch (e) {}
    try { if (store) store[storeKey] = null; } catch (e) {}
    try {
      if (typeof window.logToHost === "function") {
        window.logToHost({
          kind: "iframe-trace",
          subkind: "tauri-listener-dedup",
          at: new Date().toISOString(),
          event_name: eventName,
          iframe_key: iframeKey,
          stage: "drained-stale",
        });
      }
    } catch (e) {}
  }
  var listenResult;
  try {
    listenResult = ev.listen(eventName, callback);
  } catch (e) {
    return Promise.resolve(null);
  }
  try { if (store) store[storeKey] = listenResult; } catch (e) {}
  Promise.resolve(listenResult).then(function (unsub) {
    try { if (store) store[storeKey] = unsub; } catch (e) {}
  }, function () {});
  return Promise.resolve(listenResult);
}
try {
  if (window.parent && window.parent.__TAURI__ && window.parent.__TAURI__.event) {
    __bramListenWithDedup(window.parent.__TAURI__.event, "talk-session-changed", function (event) {
      var t0 = (typeof performance !== "undefined" && performance.now) ? performance.now() : Date.now();
      // Per-emit correlation id from the host (see Rust
      // emit_talk_session_changed). Logged here so the trace records
      // the parent→iframe hand-off independently of any subscriber's
      // own listener-fired trace. at_host_ms lets each iframe-side
      // trace report delta_to_emit_ms — host emit → this point and,
      // via subscriber forwarding, host emit → listener-fired and
      // host emit → refetch-called.
      var payload = (event && event.payload) || {};
      var correlationId = payload.correlation_id || "";
      var atHostMs = (typeof payload.at_host_ms === "number") ? payload.at_host_ms : 0;
      try {
        if (typeof window.logToHost === "function") {
          window.logToHost({
            kind: "iframe-trace",
            subkind: "event-received",
            at: new Date().toISOString(),
            context: "talk-session-changed",
            correlation_id: correlationId,
            subscribers: __talkSessionSubscribers.length,
            at_host_ms: atHostMs,
            delta_to_emit_ms: atHostMs ? (Date.now() - atHostMs) : -1,
          });
        }
      } catch (e) {}
      var n = __talkSessionSubscribers.length;
      for (var i = 0; i < n; i++) {
        try { __talkSessionSubscribers[i](correlationId, atHostMs, payload); } catch (e) {}
      }
      var t1 = (typeof performance !== "undefined" && performance.now) ? performance.now() : Date.now();
      __tscBatchTick(t1 - t0);
    });
  }
} catch (e) {}

// Generic keyed-slot subscription to a parent-shell Tauri event (#81).
// Mirrors subscribeTalkSessionChange so the same leak fix applies to
// any event name: ONE parent listener per eventName, registered lazily
// on first subscribe and guarded so it attaches exactly once per
// helpers.js load, fanning out to a synchronous subscriber array. The
// per-mount subscribe call is fully synchronous (revoke window[key],
// push, store unsub) — no tauri.event.listen Promise window — so a
// component's onInit re-running on hot-reload keeps the live-subscriber
// count at exactly one. The prior direct tauri.event.listen(...).then()
// blocks stacked one live listener per onInit re-run.
var __tauriEventSubscribers = {};
var __tauriEventListening = {};
var __tauriEventListenReady = {};
function __ensureTauriEventListener(eventName) {
  if (__tauriEventListening[eventName]) return __tauriEventListenReady[eventName] || Promise.resolve(true);
  var ev = (window.parent && window.parent.__TAURI__ && window.parent.__TAURI__.event)
    || (window.__TAURI__ && window.__TAURI__.event);
  if (!ev || typeof ev.listen !== "function") return Promise.resolve(false);
  __tauriEventListening[eventName] = true;
  try {
    var listenResult = __bramListenWithDedup(ev, eventName, function (e) {
      var subs = __tauriEventSubscribers[eventName] || [];
      try {
        if (typeof window.logToHost === "function") {
          window.logToHost({
            kind: "iframe-trace",
            subkind: "event-received",
            at: new Date().toISOString(),
            event_name: eventName,
            subscribers: subs.length,
          });
        }
      } catch (err) {}
      for (var i = 0; i < subs.length; i++) {
        var subStart = (typeof performance !== "undefined" && performance.now)
          ? performance.now()
          : Date.now();
        try { subs[i](e); } catch (err) {}
        try {
          if (typeof window.logToHost === "function") {
            var subEnd = (typeof performance !== "undefined" && performance.now)
              ? performance.now()
              : Date.now();
            window.logToHost({
              kind: "iframe-trace",
              subkind: "subscriber-fired",
              at: new Date().toISOString(),
              event_name: eventName,
              subscriber_index: i,
              elapsed_ms: Math.round(subEnd - subStart),
            });
          }
        } catch (err) {}
      }
    });
    __tauriEventListenReady[eventName] = Promise.resolve(listenResult).then(
      function () { return true; },
      function () {
        __tauriEventListening[eventName] = false;
        return false;
      },
    );
  } catch (err) {
    __tauriEventListening[eventName] = false;
    __tauriEventListenReady[eventName] = Promise.resolve(false);
  }
  return __tauriEventListenReady[eventName];
}
function __notifyStartupReadyForEvent(eventName) {
  if (typeof window.fetch !== "function") return;
  window.fetch("/__startup-ready?event=" + encodeURIComponent(eventName), { cache: "no-store" })
    .then(function () {})
    .catch(function () {});
}
window.subscribeTauriEvent = function (key, eventName, fn) {
  if (typeof window[key] === "function") {
    try { window[key](); } catch (e) {}
  }
  if (typeof fn !== "function") {
    window[key] = null;
    return function () {};
  }
  if (!__tauriEventSubscribers[eventName]) __tauriEventSubscribers[eventName] = [];
  var listenReady = __ensureTauriEventListener(eventName);
  __tauriEventSubscribers[eventName].push(fn);
  window[key] = function () {
    var subs = __tauriEventSubscribers[eventName] || [];
    var idx = subs.indexOf(fn);
    if (idx >= 0) subs.splice(idx, 1);
    window[key] = null;
  };
  Promise.resolve(listenReady).then(function (ready) {
    if (!ready) return;
    var subs = __tauriEventSubscribers[eventName] || [];
    if (subs.indexOf(fn) >= 0) __notifyStartupReadyForEvent(eventName);
  });
  return window[key];
};

// Native plain-JS subscribers for the AgentMenu pipeline. Counterpart
// to window.__bramApplyAgentMenu / window.__bramSetAgentMenuFrom*
// defined earlier in this file. Registered here, AFTER
// window.subscribeTauriEvent exists, before any External subscribers
// attach through bramSubscribeAgentMenu. Subscribers are dispatched by
// __ensureTauriEventListener in registration order, so the native handler
// updates window.bramAgentMenu in plain JS before XMLUI consumers read it.
window.subscribeTauriEvent("__bramNativePtyMenuUnsub", "pty-menu-changed", function (e) {
  window.__bramSetAgentMenuFromEvent(e, "agent-menu");
});
// Diagnostic tap for the send-restore chain (2026-07-03): the host emit
// reaches the iframe (event-received traces) but the markup applier has
// never traced. This native subscriber proves whether payloads survive
// the bridge; the actual restore logic stays in __bramApplySendRestore.
window.subscribeTauriEvent("__bramNativeSendRestoreUnsub", "send-restore", function (e) {
  try {
    var p = (e && e.payload) || null;
    window.__bramIframeTrace("send-restore", {
      stage: "native",
      hasPayload: !!p,
      chars: (p && p.text && p.text.length) || 0,
    });
  } catch (err) {}
});
window.subscribeTauriEvent("__bramNativeTurnStateUnsub", "turn-state-changed", function (e) {
  window.__bramSetAgentMenuFromTurnState((e && e.payload) || {}, "agent-menu");
});

// Native subscribers for toolbar pending-menu state. Moved out of
// Main.xmlui's onInit blob (item: main-xmlui-tauri-subscribers-external).
// The arrow bodies that used to live in markup only called
// window.__bramSetToolbarPendingMenuFrom* — pure side-effects on
// window state, no App-level var dependencies. Same pattern as the
// AgentMenu native subscribers above.
window.subscribeTauriEvent("__bramNativeToolbarTurnStateUnsub",
  "turn-state-changed", function (e) {
    window.__bramSetToolbarPendingMenuFromTurnState((e && e.payload) || null);
  });
window.subscribeTauriEvent("__bramNativeToolbarPtyMenuUnsub",
  "pty-menu-changed", function (e) {
    window.__bramSetToolbarPendingMenuFromEvent(e);
  });

// External-driven agent-status bridge. Emits the agent-status-changed
// event payload; also performs the agent-header-status-loaded trace
// emit that used to live in Main.xmlui's onInit arrow body.
// One tauri subscription, two fan-outs:
//  - bramSubscribeAgentStatus (deduped): notifies only when a meaningful field
//    (state/verb/provider/substate/source) changes. The many app-wide consumers
//    use this, so the ~1/sec elapsedText tick no longer re-renders the whole
//    agent-status surface every second.
//  - bramSubscribeAgentStatusRaw: notifies on every push, including the elapsed
//    tick, for the single isolated component that shows the running timer
//    (FooterAgentStatus). See decouple-elapsed-from-agent-status-broadcast.
(function () {
  var rawSubs = new Set();
  var dedupSubs = new Set();
  var lastValue = null;
  var lastSig = null;
  var sigOf = function (v) {
    return v ? [v.state, v.verb, v.provider, v.substate, v.source].join("|") : "";
  };
  var notify = function (set) {
    set.forEach(function (fn) {
      try { fn(); } catch (e) { console.error("[bramSubscribeAgentStatus] subscriber threw:", e); }
    });
  };
  var subscribed = false;
  var ensureSubscribed = function () {
    if (subscribed) return;
    subscribed = true;
    window.subscribeTauriEvent("__bramAgentStatusExternalUnsub",
      "agent-status-changed", function (e) {
        lastValue = (e && e.payload) || null;
        if (!window.bramAgentMenu) {
          window.__bramIframeTrace("agent-header-status-loaded", {
            state: (lastValue && lastValue.state) || "",
            verb: (lastValue && lastValue.verb) || "",
            provider: (lastValue && lastValue.provider) || "",
            source: (lastValue && lastValue.source) || "",
            elapsed: (lastValue && lastValue.elapsedText) || ""
          });
        }
        notify(rawSubs);
        var sig = sigOf(lastValue);
        if (sig !== lastSig) {
          lastSig = sig;
          notify(dedupSubs);
        }
      });
  };
  var makeFactory = function (set) {
    var factory;
    return function () {
      if (factory) return factory;
      ensureSubscribed();
      factory = function (emit) {
        var fire = function () { emit(lastValue); };
        set.add(fire);
        fire();
        return function () { set.delete(fire); };
      };
      return factory;
    };
  };
  window.bramSubscribeAgentStatus = makeFactory(dedupSubs);
  window.bramSubscribeAgentStatusRaw = makeFactory(rawSubs);
})();

// Host suspicious-silence + parent terminal-visibility join. The Rust host
// owns the adaptive PTY/turn predicate; main.js owns terminal visibility.
// This isolated bridge exposes only the derived warning state to the footer.
(function () {
  var subscribers = new Set();
  var hostValue = null;
  var terminalHidden = null;
  var lastTerminalHidden = null;
  var suppressedEpisode = "";
  var lastValue = { active: false, terminalHidden: null };
  var lastSignature = "";
  var hostSubscribed = false;

  var episodeOf = function (value) {
    return value && value.episodeId ? String(value.episodeId) : "";
  };
  var derivedValue = function () {
    var episode = episodeOf(hostValue);
    var active = !!(hostValue && hostValue.active && terminalHidden === true &&
      (!suppressedEpisode || suppressedEpisode !== episode));
    return Object.assign({}, hostValue || {}, {
      active: active,
      terminalHidden: terminalHidden,
    });
  };
  var notify = function (reason) {
    var next = derivedValue();
    var signature = JSON.stringify(next);
    if (signature === lastSignature) return;
    var wasActive = !!(lastValue && lastValue.active);
    lastSignature = signature;
    lastValue = next;
    if (wasActive !== !!next.active) {
      window.__bramIframeTrace("terminal-suspicious-silence", {
        op: next.active ? "warn" : "cleared",
        reason: reason || next.reason || "state-change",
        terminalHidden: terminalHidden,
        provider: next.provider || "",
        turn: next.turnStamp || "",
        episode: next.episodeId || "",
        silence_ms: next.silenceMs || 0,
        threshold_ms: next.thresholdMs || 0,
        gap_p95_ms: next.gapP95Ms || 0,
        gaps_n: next.gapsN || 0,
        test_mode: !!next.testMode,
      });
    }
    subscribers.forEach(function (fn) {
      try { fn(); } catch (e) { console.error("[bram] suspicious-silence subscriber threw:", e); }
    });
  };

  window.addEventListener("message", function (event) {
    var data = event && event.data;
    if (!data || event.source !== window.parent) return;
    if (data.type === "bram-terminal-suspicious-silence-test") {
      var testEpisode = data.episodeId ? String(data.episodeId) : "self-test:" + Date.now();
      if (suppressedEpisode !== testEpisode) suppressedEpisode = "";
      hostValue = {
        active: true,
        reason: "self-test",
        episodeId: testEpisode,
        provider: "self-test",
        turnStamp: "",
        silenceMs: Number(data.silenceMs) || 3000,
        thresholdMs: Number(data.thresholdMs) || 3000,
        gapP95Ms: Number(data.gapP95Ms) || 0,
        gapsN: Number(data.gapsN) || 0,
        testMode: true,
        at: Number(data.at) || Date.now(),
      };
      notify("self-test");
      return;
    }
    if (data.type !== "bram-terminal-visibility") return;
    var nextHidden = !!data.hidden;
    if (data.dismissedEpisode) suppressedEpisode = String(data.dismissedEpisode);
    if (lastTerminalHidden === true && nextHidden === false && hostValue && hostValue.active) {
      // Reopening the terminal dismisses this episode. Hiding it again must
      // not re-warn until real PTY activity rearms the host detector.
      suppressedEpisode = episodeOf(hostValue);
      window.parent.postMessage({
        type: "bram-terminal-silence-dismissed",
        episodeId: suppressedEpisode,
      }, "*");
    }
    lastTerminalHidden = nextHidden;
    terminalHidden = nextHidden;
    notify(nextHidden ? "terminal-closed" : "terminal-opened");
  });

  var ensureHostSubscribed = function () {
    if (hostSubscribed) return;
    hostSubscribed = true;
    window.subscribeTauriEvent(
      "__bramTerminalSuspiciousSilenceUnsub",
      "terminal-suspicious-silence",
      function (event) {
        hostValue = (event && event.payload) || null;
        if (!hostValue || !hostValue.active) suppressedEpisode = "";
        notify((hostValue && hostValue.reason) || "host-state");
      }
    );
  };

  window.bramSubscribeTerminalSuspiciousSilence = (function () {
    var factory;
    return function () {
      if (factory) return factory;
      ensureHostSubscribed();
      factory = function (emit) {
        var fire = function () { emit(lastValue); };
        subscribers.add(fire);
        fire();
        return function () { subscribers.delete(fire); };
      };
      return factory;
    };
  })();

  window.__bramOpenTerminalForSuspiciousSilence = function () {
    if (hostValue && hostValue.active) {
      suppressedEpisode = episodeOf(hostValue);
      notify("open-terminal-click");
    }
    window.parent.postMessage({
      type: "bram-open-terminal",
      episodeId: episodeOf(hostValue),
    }, "*");
  };

  window.parent.postMessage({ type: "bram-terminal-visibility-request" }, "*");
})();

// External-driven PTY-throughput bridge (transcript-nav-activity-sparkline).
// The host emits `pty-throughput` a few times/sec with a 0..1 intensity
// derived from the byte rate flowing through the PTY reader loop. Subscribers
// (the Transcript nav activity row) map it to a dot count + pulse. Mirrors
// bramSubscribeAgentStatus above.
window.bramSubscribePtyThroughput = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var lastValue = 0;
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bramSubscribePtyThroughput] subscriber threw:", e); }
      });
    };
    window.subscribeTauriEvent("__bramPtyThroughputExternalUnsub",
      "pty-throughput", function (e) {
        lastValue = (e && typeof e.payload === "number") ? e.payload : 0;
        notify();
      });
    factory = function (emit) {
      var fire = function () { emit(lastValue); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// External-driven enhance-status tick. Emits an incrementing tick on
// each enhance-status-changed event so a downstream ChangeListener can
// trigger DataSource.refetch() (a markup-only operation).
window.bramSubscribeEnhanceStatusTick = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var tick = 0;
    window.subscribeTauriEvent("__bramEnhanceStatusExternalUnsub",
      "enhance-status-changed", function () {
        tick += 1;
        subscribers.forEach(function (fn) {
          try { fn(); } catch (e) { console.error("[bramSubscribeEnhanceStatusTick] subscriber threw:", e); }
        });
      });
    factory = function (emit) {
      var fire = function () { emit(tick); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// Voice transcript scratch setter — invoked from xs arrow bodies that
// can't write `window.foo = x` as an LValue (XMLUI's expression engine
// rejects member-expression LValues with "Left value variable not
// found in scope" — see bram-trace 2026-06-17 00:43:03). Plain JS, no
// xs evaluator involvement.
// Plain-JS append helper. xs `function foo()` declarations do NOT
// reliably hoist onto window from the iframe's runtime context — see
// 2026-06-17 voice debugging where window.appendVoiceTranscript and
// window.bumpWorklistVoiceSeq calls returned without entering the
// function body. Defining the append helper directly on window
// guarantees the call lands.
window.__bramAppendVoiceToBox = function (component, transcript) {
  try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "windowAppend-enter", tLen: (transcript || "").length, hasComponent: !!component }); } catch (e) {}
  if (!component || !transcript) {
    try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "windowAppend-early-return", reason: !component ? "no-component" : "no-transcript" }); } catch (e) {}
    return false;
  }
  var current = String(component.value || "");
  var cleaned = transcript.replace(/\r?\n/g, " ").replace(/[ \t]+/g, " ").trim();
  if (!cleaned) {
    try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "windowAppend-cleaned-empty" }); } catch (e) {}
    return false;
  }
  var spacer = current && !/\s$/.test(current) ? " " : "";
  var next = current + spacer + cleaned;
  try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "windowAppend-calling-setValue", currentLen: current.length, nextLen: next.length }); } catch (e) {}
  try {
    component.setValue(next);
    try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "windowAppend-after-setValue" }); } catch (e) {}
  } catch (e) {
    try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "windowAppend-setValue-threw", error: String(e && e.message) }); } catch (e2) {}
    return false;
  }
  try {
    if (typeof component.focus === "function") component.focus();
    if (typeof component.setSelectionRange === "function") component.setSelectionRange(next.length, next.length);
  } catch (e) {}
  return next;
};

window.__bramSetLatestVoiceState = function (t, meta) {
  try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "setLatest-enter", tLen: (t || "").length }); } catch (e) {}
  window.__bramLatestVoiceTranscript = t || "";
  window.__bramLatestVoiceMeta = meta || null;
  try {
    window.dispatchEvent(new CustomEvent("bram:voice-arrival", {
      detail: { transcript: t || "", meta: meta || null, at: Date.now() },
    }));
    try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "setLatest-dispatched" }); } catch (e) {}
  } catch (e) {
    console.error("[bram] voice-arrival dispatch failed:", e);
  }
};

window.addEventListener("message", function (ev) {
  var data = ev && ev.data;
  if (!data || data.type !== "voice-state") return;
  var state = data.state || "idle";
  var requestId = data.requestId || null;
  window.__bramVoiceRecorderState = {
    state: state,
    requestId: requestId,
    target: data.target || "",
    reason: data.reason || "",
    transcriptLength:
      typeof data.transcriptLength === "number" ? data.transcriptLength : null,
    at: Date.now(),
  };
  if (state === "idle" && (!requestId || requestId === window._voiceSession)) {
    window._voiceSession = null;
    window._voiceSessionTarget = "";
    _voiceRemoveStartedListener();
  }
  try {
    window.dispatchEvent(new CustomEvent("bram:voice-recorder-state", {
      detail: window.__bramVoiceRecorderState,
    }));
  } catch (e) {
    console.error("[bram] voice-recorder-state dispatch failed:", e);
  }
});

window.bramSubscribeVoiceRecorderState = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bram] voice-recorder-state subscriber threw:", e); }
      });
    };
    window.addEventListener("bram:voice-recorder-state", notify);
    factory = function (emit) {
      var fire = function () { emit(window.__bramVoiceRecorderState); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// External-driven voice-arrival bridge. xs-side writes to module vars
// (worklistVoiceSeq, worklistVoiceText) don't propagate through XMLUI's
// reactive system when triggered from arrow-body callbacks (see
// 2026-06-17 voice debugging). This External listens to a window-side
// CustomEvent that __bramSetLatestVoiceState dispatches, giving the
// XMLUI reactivity layer a path it can observe.
window.bramSubscribeVoiceArrival = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var currentEvent = null;
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bram] voice-arrival subscriber threw:", e); }
      });
    };
    window.addEventListener("bram:voice-arrival", function (evt) {
      currentEvent = (evt && evt.detail) || null;
      try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "external-event-received", tLen: ((currentEvent && currentEvent.transcript) || "").length, subscribers: subscribers.size }); } catch (e) {}
      notify();
      currentEvent = null;
    });
    factory = function (emit) {
      var fire = function () {
        try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "external-fire", hasEvent: !!currentEvent }); } catch (e) {}
        emit(currentEvent);
      };
      subscribers.add(fire);
      try { window.__bramIframeTrace && window.__bramIframeTrace("voice-trace", { stage: "external-subscribed", totalSubscribers: subscribers.size }); } catch (e) {}
      emit(null);
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// Parent → agent-pane bridge for whisper-server failure notices.
// main.js posts { type: "bram-whisper-unavailable", reason, kind?, detail? }
// to the tools-pane iframe when voice cannot start or transcription fails.
// Re-dispatch as a window CustomEvent that the External below observes,
// giving XMLUI markup a path to toast. Same indirection as
// __bramSetLatestVoiceState / voice-arrival.
window.addEventListener("message", function (event) {
  var data = event && event.data;
  if (!data || data.type !== "bram-whisper-unavailable") return;
  try {
    window.dispatchEvent(new CustomEvent("bram:whisper-unavailable", {
      detail: {
        reason: String(data.reason || ""),
        kind: String(data.kind || ""),
        detail: String(data.detail || ""),
        at: Date.now(),
      },
    }));
  } catch (e) {
    console.error("[bram] whisper-unavailable dispatch failed:", e);
  }
});

window.addEventListener("message", function (event) {
  var data = event && event.data;
  if (!data || data.type !== "bram-voice-busy") return;
  try {
    window.dispatchEvent(new CustomEvent("bram:voice-busy", {
      detail: {
        requester: String(data.requester || ""),
        activeWas: String(data.activeWas || ""),
        activeRequestId: String(data.activeRequestId || ""),
        activeTarget: String(data.activeTarget || ""),
        at: Date.now(),
      },
    }));
  } catch (e) {
    console.error("[bram] voice-busy dispatch failed:", e);
  }
});

window.bramSubscribeWhisperUnavailable = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var lastEvent = null;
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bram] whisper-unavailable subscriber threw:", e); }
      });
    };
    window.addEventListener("bram:whisper-unavailable", function (evt) {
      lastEvent = (evt && evt.detail) || null;
      notify();
    });
    factory = function (emit) {
      var fire = function () { emit(lastEvent); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

window.__bramToastWhisperNotice = function (notice, toastApi) {
  if (!notice || !toastApi || typeof toastApi.error !== "function") return;
  if (notice.kind === "transcription-failed") {
    toastApi.error(
      "Voice transcription failed (" + String(notice.detail || "unknown error") +
      "). Recording worked; the whisper server could not transcribe it."
    );
    return;
  }
  toastApi.error(
    "Whisper server is not running and could not be started automatically. " +
    "Start it manually — see the README Voice input section: " +
    "https://github.com/judell/bram#voice-input"
  );
};

window.bramSubscribeVoiceBusy = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var lastEvent = null;
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bram] voice-busy subscriber threw:", e); }
      });
    };
    window.addEventListener("bram:voice-busy", function (evt) {
      lastEvent = (evt && evt.detail) || null;
      notify();
    });
    factory = function (emit) {
      var fire = function () { emit(lastEvent); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// External-driven right-pane-size bridge. Same shape as the Tauri /
// agent-status / agent-menu factories, but the underlying source is
// the custom subscribeRightPaneSize API (window resize observer).
window.bramSubscribeRightPaneSize = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var lastSize = null;
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bramSubscribeRightPaneSize] subscriber threw:", e); }
      });
    };
    window.subscribeRightPaneSize(function (s) {
      lastSize = s || null;
      notify();
    });
    factory = function (emit) {
      var fire = function () { emit(lastSize); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

window.__bramLocalLinkPreview = null;
window.__bramLocalLinkPreviewSubscribers = new Set();
window.__bramNotifyLocalLinkPreview = function () {
  window.__bramLocalLinkPreviewSubscribers.forEach(function (fn) {
    try { fn(); } catch (e) { console.error("[bramLocalLinkPreview] subscriber threw:", e); }
  });
};
window.bramSubscribeLocalLinkPreview = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    factory = function (emit) {
      var fire = function () { emit(window.__bramLocalLinkPreview); };
      window.__bramLocalLinkPreviewSubscribers.add(fire);
      fire();
      return function () { window.__bramLocalLinkPreviewSubscribers.delete(fire); };
    };
    return factory;
  };
})();
window.__bramCloseLocalLinkPreview = function () {
  window.__bramLocalLinkPreview = null;
  window.__bramNotifyLocalLinkPreview();
};
window.__bramSetLocalLinkPreview = function (payload) {
  window.__bramLocalLinkPreview = payload || null;
  window.__bramNotifyLocalLinkPreview();
};
window.__bramShowLinkPreviewError = function (href, error) {
  window.__bramSetLocalLinkPreview({
    ok: false,
    href: String(href || ""),
    displayPath: String(href || ""),
    title: "Link unavailable",
    error: String(error || "Could not open this link."),
    content: "",
    language: "",
    renderMode: "error",
    at: Date.now(),
  });
};
window.__bramLocalLinkPreviewTitle = function (preview) {
  if (!preview) return "File";
  if (preview.title) return preview.title;
  return preview.name || preview.displayPath || preview.path || preview.href || "File";
};
window.__bramLocalLinkPreviewMeta = function (preview) {
  if (!preview) return "";
  if (preview.error) return preview.displayPath || preview.href || "";
  var bits = [];
  if (preview.displayPath) bits.push(preview.displayPath);
  if (preview.line) bits.push("line " + preview.line);
  if (preview.truncated) bits.push("truncated");
  return bits.join(" · ");
};
window.__bramFormatLocalLinkPreview = function (preview) {
  if (!preview) return "";
  if (preview.error) return preview.error;
  var content = preview.content == null ? "" : String(preview.content);
  if (preview.renderMode === "markdown") return content;
  return window.__bramFenceMarkdown(content, preview.language || "");
};
window.__bramFenceMarkdown = function (body, lang) {
  body = body == null ? "" : String(body);
  var longest = 0, run = 0;
  for (var i = 0; i < body.length; i++) {
    if (body.charAt(i) === "`") { run++; if (run > longest) longest = run; }
    else { run = 0; }
  }
  var fence = "";
  var fenceLen = Math.max(3, longest + 1);
  for (var j = 0; j < fenceLen; j++) fence += "`";
  return fence + (lang || "") + "\n" + body + "\n" + fence;
};
window.__bramLocalLinkRequestFromHref = function (href) {
  href = String(href || "").trim();
  if (!href) return null;
  // XMLUI/Markdown rewrites many local hrefs into hash routes
  // (`/Users/me/x.md` -> `#/Users/me/x.md`, `README.md` -> `#README.md`).
  // Treat hash-prefixed file-like values as local links, but leave ordinary
  // page anchors (`#section`) alone.
  if (href.charAt(0) === "#") {
    var hashPath = href.slice(1);
    if (
      !hashPath ||
      !(/^\/|^~\/|^\.\.?\/|^[A-Za-z]:[\\/]/.test(hashPath) || /\.[A-Za-z0-9]+(?::\d+)?(?:[?#].*)?$/.test(hashPath))
    ) {
      return null;
    }
    href = hashPath;
  }
  if (/^(mailto|tel|javascript):/i.test(href)) {
    return { skip: true, reason: "scheme", href: href };
  }

  var raw = href;
  var m;
  if ((m = raw.match(/^file:\/\/(?:localhost)?([^?#]*)(?:[?#].*)?$/i))) {
    raw = decodeURIComponent(m[1] || "");
  } else if (/^[a-z][a-z0-9+.-]*:/i.test(raw)) {
    return { skip: true, reason: "external-scheme", href: href };
  }
  raw = raw.replace(/[?#].*$/, "");

  // Bram nav routes render as hash routes (#/transcript, #/worklist). But
  // XMLUI's Markdown resolves RELATIVE links against the current route, so a
  // link like `[x](src/foo.rs)` from the transcript arrives as
  // `#/transcript/src/foo.rs`. Distinguish the two: a bare route (no
  // remainder) is navigation -> skip; a route followed by a file-like
  // remainder is a relative file link XMLUI prefixed with the current route
  // -> strip the route segment and preview the remainder. Absolute links
  // (/Users, /etc, ...) don't start with a known route and fall through.
  // Keep this alternation in sync with Main.xmlui's NavLink routes — a nav
  // route missing here gets intercepted as a local FILE link and the click
  // opens a "File unavailable" preview instead of the page (the /queue
  // launch bug, 2026-07-23).
  var routeMatch = raw.match(
    /^\/(worklist|transcript|search|issues|commits|queue|history|sessions|settings|status|context)(\/.*)?$/
  );
  if (routeMatch) {
    var rest = routeMatch[2] ? routeMatch[2].slice(1) : "";
    if (!rest || !/\.[A-Za-z0-9]+(?::\d+)?$/.test(rest)) {
      return { skip: true, reason: "app-route", href: href, raw: raw };
    }
    raw = rest;
  }

  var line = null;
  var lineMatch = raw.match(/^(.*):(\d+)$/);
  if (lineMatch && !/^[A-Za-z]:\\/.test(raw)) {
    raw = lineMatch[1];
    line = parseInt(lineMatch[2], 10);
  }
  if (!raw) return null;
  if (raw.indexOf("://") >= 0) return { skip: true, reason: "unknown-url", href: href, raw: raw };
  return { path: raw, line: line, href: href };
};
// issue-230 Search facets: add/remove a type from the selectedTypes array
// (keeps array-mutation logic out of the Checkbox onDidChange attribute).
window.__bramToggleType = function (types, t, on) {
  var set = Array.isArray(types) ? types.slice() : [];
  if (on) {
    if (set.indexOf(t) < 0) set.push(t);
  } else {
    set = set.filter(function (x) { return x !== t; });
  }
  return set;
};
// issue-230: measure the session-transcript render cost. Called on /__turns
// load; a double-rAF waits through any synchronous render freeze, then logs the
// paint delta + turn count as a `search-render` trace line (persistent, so we
// always see render-to-paint vs. turn count).
window.__bramMeasureTurnsRender = function (count) {
  var now = function () {
    return window.performance && performance.now ? performance.now() : Date.now();
  };
  var t0 = now();
  var raf = window.requestAnimationFrame || function (f) { return setTimeout(f, 16); };
  raf(function () {
    raf(function () {
      try {
        window.__bramIframeTrace("search-render", {
          op: "turns",
          turns: count,
          ms: Math.round(now() - t0),
        });
      } catch (e) {}
    });
  });
};
window.__bramOpenLocalLinkPreview = function (request) {
  if (!request || !request.path) return;
  var qs = "path=" + encodeURIComponent(request.path);
  if (request.line) qs += "&line=" + encodeURIComponent(String(request.line));
  window.__bramSetLocalLinkPreview({
    ok: true,
    href: request.href || request.path,
    displayPath: request.path,
    title: "Loading file...",
    content: "",
    renderMode: "loading",
    at: Date.now(),
  });
  try {
    window.__bramIframeTrace("local-link-preview", {
      stage: "fetch",
      href: request.href || "",
      path: request.path || "",
      line: request.line || null,
    });
  } catch (e) {}
  window.fetch("/__local-file-preview?" + qs, { cache: "no-store" })
    .then(function (r) { return r.json(); })
    .then(function (payload) {
      payload = payload || {};
      payload.href = request.href || request.path;
      payload.at = Date.now();
      try {
        window.__bramIframeTrace("local-link-preview", {
          stage: "response",
          ok: !!payload.ok,
          href: request.href || "",
          path: payload.path || request.path || "",
          displayPath: payload.displayPath || "",
          renderMode: payload.renderMode || "",
          error: payload.error || "",
        });
      } catch (e) {}
      window.__bramSetLocalLinkPreview(payload);
    })
    .catch(function (e) {
      try {
        window.__bramIframeTrace("local-link-preview", {
          stage: "fetch-error",
          href: request.href || "",
          path: request.path || "",
          error: String(e && e.message || e),
        });
      } catch (traceErr) {}
      window.__bramShowLinkPreviewError(request.href || request.path, String(e && e.message || e));
    });
};

// External-driven talk-session-change bridge. Emits an event with
// the correlation id and host timestamp on each talk-session
// rotation.
window.bramSubscribeTalkSessionChange = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var lastEvent = null;
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bramSubscribeTalkSessionChange] subscriber threw:", e); }
      });
    };
    window.subscribeTalkSessionChange(
      "__bramTalkSessionExternalUnsub",
      function (correlationId, atHostMs) {
        lastEvent = {
          correlationId: correlationId || "",
          atHostMs: atHostMs || 0,
          at: Date.now(),
        };
        notify();
      }
    );
    factory = function (emit) {
      var fire = function () { emit(lastEvent); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// Generic External-driven Tauri event factory. Memoizes per event
// name. Emits { tick, payload } on each fire — tick strictly
// increments to guarantee identity-change for listenTo expressions;
// payload carries the event data for consumers that need it.
window.bramSubscribeTauriEvent = (function () {
  var byEvent = Object.create(null);
  return function (eventName) {
    if (byEvent[eventName]) return byEvent[eventName];
    var subscribers = new Set();
    var tick = 0;
    var lastPayload = null;
    window.subscribeTauriEvent(
      "__bramTauriExternal_" + eventName,
      eventName,
      function (e) {
        tick += 1;
        lastPayload = (e && e.payload) || null;
        var snapshot = { tick: tick, payload: lastPayload };
        subscribers.forEach(function (fn) {
          try { fn(snapshot); } catch (err) {
            console.error("[bramSubscribeTauriEvent] subscriber threw:", err);
          }
        });
      }
    );
    var replayLatest = function () {
      if (typeof window.fetch !== "function") return;
      window.fetch("/__event/latest?name=" + encodeURIComponent(eventName), { cache: "no-store" })
        .then(function (r) { return r.json(); })
        .then(function (data) {
          if (!data || !data.exists) return;
          tick += 1;
          lastPayload = data.payload || null;
          var snapshot = { tick: tick, payload: lastPayload, replayed: true };
          subscribers.forEach(function (fn) {
            try { fn(snapshot); } catch (err) {
              console.error("[bramSubscribeTauriEvent] replay subscriber threw:", err);
            }
          });
        })
        .catch(function () {});
    };
    var factory = function (emit) {
      var fire = function (snapshot) {
        emit(snapshot || { tick: tick, payload: lastPayload });
      };
      subscribers.add(fire);
      fire();
      replayLatest();
      return function () { subscribers.delete(fire); };
    };
    byEvent[eventName] = factory;
    return factory;
  };
})();

// External-driven AgentMenu bridge — emits the current pending menu
// when either Tauri event fires. Subscribes lazily on first call so
// the native subscribers above (registered at module load) are
// guaranteed to fire FIRST and update window.bramAgentMenu before
// compute() reads it.
window.bramSubscribeAgentMenu = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var lastTurnState = null;
    var subscribers = new Set();
    var compute = function () {
      var current = window.bramAgentMenu || null;
      var suppress = window.bramAgentMenuSuppressFallback !== false;
      return current ||
        (!suppress && lastTurnState && lastTurnState.pendingMenu) ||
        null;
    };
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bramSubscribeAgentMenu] subscriber threw:", e); }
      });
    };
    window.subscribeTauriEvent(
      "__bramAgentMenuExternalTurnUnsub",
      "turn-state-changed",
      function (e) { lastTurnState = (e && e.payload) || null; notify(); }
    );
    window.subscribeTauriEvent(
      "__bramAgentMenuExternalPtyUnsub",
      "pty-menu-changed",
      notify
    );
    factory = function (emit) {
      var fire = function () { emit(compute()); };
      subscribers.add(fire);
      fire();
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// (issue-214 candidate #5: the shared raw-JSONL cache, its startup
// gating, and bramSubscribeLatestJsonl were retired here — no consumer
// remained. talk-session-changed is now a slim change-signal tick; see
// startBramLatestJsonlPush below.)

// --- Projected turns (single host projection; docs/turn-transport-redesign.md)
// Turn-display surfaces bound to the LIVE session consume /__turns through
// this pipeline instead of parsing raw JSONL. talk-session-changed is the
// CHANGE SIGNAL: each tick coalesces into one projection refetch. Turn
// objects are reference-preserved across fetches
// so XMLUI lists don't re-mount unchanged rows.
var __projectedTurnsValue = null; // { sid, provider, turns } | null
var __projectedTurnsSubscribers = [];
var __projectedTurnsTimer = null;
var __projectedTurnsSeq = 0;
var __projectedTurnsRevision = 0;

window.getProjectedTurns = function () { return __projectedTurnsValue; };
window.onProjectedTurnsChange = function (fn) {
  if (typeof fn !== "function") return function () {};
  __projectedTurnsSubscribers.push(fn);
  return function () {
    var idx = __projectedTurnsSubscribers.indexOf(fn);
    if (idx >= 0) __projectedTurnsSubscribers.splice(idx, 1);
  };
};

// Loose per-turn equality for reference preservation. Includes result
// value + error/layout flags because tool results stream onto entries that
// otherwise look unchanged.
window.__bramProjectedTurnEqual = function (a, b) {
  if (!a || !b) return false;
  if (a.role !== b.role || a.text !== b.text) return false;
  var ae = a.entries || [], be = b.entries || [];
  if (ae.length !== be.length) return false;
  var ai = a.images || [], bi = b.images || [];
  if (ai.length !== bi.length) return false;
  for (var i = 0; i < ae.length; i++) {
    var x = ae[i] || {}, y = be[i] || {};
    if (x.kind !== y.kind) return false;
    if (x.kind === "tool") {
      if (
        x.id !== y.id ||
        x.name !== y.name ||
        x.summary !== y.summary ||
        x.commandDisplay !== y.commandDisplay ||
        x.commandMarkdown !== y.commandMarkdown ||
        // description participates: the ai-describe overlay changes ONLY
        // this field, and an "equal" verdict would reuse the stale turn
        // reference and silently drop the new header (2026-07-08 "no
        // description line appeared").
        (x.description || "") !== (y.description || "") ||
        // aiDescription participates for the same reason description does:
        // the eager/expand describe patch changes ONLY this field, and an
        // equality check that ignores it discards the splice on rebroadcast
        // (2026-07-22, the second field-whitelist bite in one day).
        (x.aiDescription || "") !== (y.aiDescription || "") ||
        // menuAnswer participates for the same reason: the menu-answer
        // overlay changes ONLY this field on an otherwise-unchanged turn.
        (x.menuAnswer || "") !== (y.menuAnswer || "") ||
        x.result !== y.result ||
        !!x.isError !== !!y.isError ||
        !!x.resultStructured !== !!y.resultStructured
      ) return false;
      if ((x.agentId || "") !== (y.agentId || "")) return false;
    } else if (x.text !== y.text) {
      return false;
    }
  }
  return true;
};

window.__bramBroadcastProjectedTurns = function (payload) {
  var prev = __projectedTurnsValue;
  if (payload && prev && prev.sid === payload.sid) {
    var prevTurns = prev.turns || [];
    var nextTurns = payload.turns || [];
    var limit = Math.min(prevTurns.length, nextTurns.length);
    for (var i = 0; i < limit; i++) {
      if (window.__bramProjectedTurnEqual(prevTurns[i], nextTurns[i])) {
        nextTurns[i] = prevTurns[i];
      }
    }
  }
  payload.revision = ++__projectedTurnsRevision;
  __projectedTurnsValue = payload;
  var n = __projectedTurnsSubscribers.length;
  for (var j = 0; j < n; j++) {
    try { __projectedTurnsSubscribers[j](payload); } catch (e) {}
  }
  __bramEagerDescribe(payload);
};

// promote-tool-descriptions-to-row (eager): request descriptions as
// command-bearing rows ARRIVE instead of waiting for a click-expand —
// the row leads with intent, hover shows the raw command. Bounded to
// the newest turns so loading a large session cannot fire a historical
// describe burst; per-id dedupe (plus the host cache) keeps re-broadcasts
// free, and the unavailable latch (set on a disabled/no-key answer)
// stops eager re-POST churn on installs with the feature off — a manual
// expand still tries, and any success clears the latch.
// Full-transcript coverage, NEWEST FIRST: what the user sees at the
// bottom adjusts immediately; backscroll fills in as the queue drains.
// Cost is accepted (per 2026-07-22 direction); responsiveness is
// protected by pacing — a bounded-concurrency queue instead of firing
// hundreds of requests (and hundreds of full re-renders) at once on a
// large session. The queue is rebuilt per broadcast; per-id dedupe and
// the host result cache make that free.
var __bramDescribeQueue = [];
var __bramDescribeInFlight = 0;
var __BRAM_DESCRIBE_CONCURRENCY = 3;
// The List is virtualized: only near-viewport rows exist in the DOM,
// so mounted rows (virtua data-index wrappers) ARE the visibility
// signal. Re-partition on every pump — scrolling re-prioritizes a
// draining queue with no extra listeners.
// Primary signal: the List's visibleRangeDidChange event (first-class API,
// added upstream in xmlui feat/list-visible-range). The [data-index] DOM
// scrape remains as fallback for the window before the first event lands.
window.__bramSetVisibleRange = function (range) {
  window.__bramVisibleRange = range || null;
  try { __bramPumpDescribeQueue(); } catch (e) {}
};
function __bramVisibleToolIds() {
  var ids = {};
  try {
    var evs = window.__bramLastTranscriptEvents || [];
    var r = window.__bramVisibleRange;
    if (r && r.startIndex >= 0) {
      for (var j = r.startIndex; j <= r.endIndex && j < evs.length; j++) {
        var ev = evs[j];
        if (ev && ev.kind === "tool" && ev.id) ids[ev.id] = true;
      }
      return ids;
    }
    var nodes = document.querySelectorAll("[data-index]");
    for (var i = 0; i < nodes.length; i++) {
      var idx = parseInt(nodes[i].getAttribute("data-index"), 10);
      var e = evs[idx];
      if (e && e.kind === "tool" && e.id) ids[e.id] = true;
    }
  } catch (e) {}
  return ids;
}
function __bramPumpDescribeQueue() {
  try {
    var vis = __bramVisibleToolIds();
    if (__bramDescribeQueue.length > 1) {
      var front = [], rest = [];
      for (var qi = 0; qi < __bramDescribeQueue.length; qi++) {
        (vis[__bramDescribeQueue[qi].id] ? front : rest).push(__bramDescribeQueue[qi]);
      }
      __bramDescribeQueue = front.concat(rest);
    }
  } catch (e) {}
  while (__bramDescribeInFlight < __BRAM_DESCRIBE_CONCURRENCY && __bramDescribeQueue.length) {
    var e = __bramDescribeQueue.shift();
    if (!e || !e.id || window.__bramDescribeRequested[e.id]) continue;
    __bramDescribeInFlight++;
    window.__bramRequestCommandDescription(e, function () {
      __bramDescribeInFlight--;
      __bramPumpDescribeQueue();
    });
  }
}
setInterval(function () {
  if (__bramDescribeQueue.length && !window.__bramDescribeUnavailable) {
    __bramPumpDescribeQueue();
  }
}, 1500);
function __bramEagerDescribe(payload) {
  try {
    if (window.__bramDescribeUnavailable) return;
    if (!payload || !payload.turns) return;
    if (window.location.pathname.indexOf("/tools/") === -1) return;
    var turns = payload.turns;
    var queue = [];
    for (var i = turns.length - 1; i >= 0; i--) {
      var entries = (turns[i] && turns[i].entries) || [];
      for (var k = entries.length - 1; k >= 0; k--) {
        var e = entries[k];
        if (!e || e.kind !== "tool" || !e.id) continue;
        if (e.aiDescription) continue;
        if (!window.__bramDescribeMaterial(e)) continue;
        if (window.__bramDescribeRequested[e.id]) continue;
        queue.push(e);
      }
    }
    __bramDescribeQueue = queue;
    __bramPumpDescribeQueue();
  } catch (err) {}
}

// Splice a latest=N window onto the accumulated full projection
// (bound-turns-projection-and-gate-edit-hints). Returns null when the
// window cannot be aligned — sid change (rotation), total shrink
// (compaction), or a gap (more than N new turns since the last fetch)
// — and the caller falls back to a full fetch. Prefix turns are reused
// by reference, so the broadcast's index-wise reference preservation
// keeps unchanged rows mounted for free.
window.__bramMergeProjectedTurnsWindow = function (prev, payload) {
  if (!prev || !payload) return null;
  if (!payload.sid || payload.sid !== prev.sid) return null;
  var ws = payload.windowStart;
  var total = payload.total;
  if (typeof ws !== "number" || typeof total !== "number") return null;
  var prevTurns = prev.turns || [];
  if (ws > prevTurns.length) return null;
  if (total < prevTurns.length) return null;
  var turns = prevTurns.slice(0, ws).concat(payload.turns || []);
  if (turns.length !== total) return null;
  return { sid: payload.sid, provider: payload.provider, turns: turns };
};

// ai-describe delivery: patch the described entry into the accumulated
// projection directly and re-broadcast. A refetch cannot deliver it —
// tick refetches are windowed (latest=8) and an expanded row usually
// sits OUTSIDE that window, so the merge keeps the stale entry
// (2026-07-08 "no description line appeared"); it's also ~1s of wasted
// projection work on multi-MB codex sessions. The describe response
// already carries the description, so this is a pure client-side splice:
// clone the turn/entry (never mutate — the broadcast's reference
// preservation depends on prev staying pristine) and re-push.
// describe-rebroadcast-coalesce (perf audit 2026-07-22): completions are
// ENQUEUED and flushed in one rebroadcast per ~400ms window, not one per
// result. The full-backscroll eager describe made 524 calls on an 18MB /
// 1200-turn session, and a fan-out per completion (each subscriber re-runs
// the events adapter) degraded heartbeat drift to avg 277ms / max 4.1s and
// a tab-switch subscribe refetch to 3.1s. Cache-hit boots are denser still
// (no Haiku latency between completions), so per-result broadcasting gets
// WORSE after first backfill. setTimeout is fine here (helpers.js is real
// JS, outside the XMLUI expression engine).
var __describePendingPatches = {};
var __describeFlushArmed = false;
window.__bramPatchProjectedToolDescription = function (toolId, description) {
  if (!toolId || !description) return false;
  __describePendingPatches[toolId] = description;
  if (!__describeFlushArmed) {
    __describeFlushArmed = true;
    setTimeout(function () { window.__bramFlushDescribePatches(); }, 400);
  }
  return true;
};
window.__bramFlushDescribePatches = function () {
  __describeFlushArmed = false;
  var pending = __describePendingPatches;
  __describePendingPatches = {};
  var ids = Object.keys(pending);
  if (!ids.length) return;
  var prev = __projectedTurnsValue;
  if (!prev || !prev.turns) return;
  var turns = prev.turns;
  var newTurns = null;
  var applied = 0;
  for (var i = 0; i < turns.length; i++) {
    var entries = (turns[i] && turns[i].entries) || [];
    var newEntries = null;
    for (var k = 0; k < entries.length; k++) {
      var e = entries[k];
      if (!e || e.kind !== "tool" || !e.id) continue;
      var desc = pending[e.id];
      if (!desc || (e.aiDescription || "") === desc) continue;
      // Clone, never mutate — the broadcast's per-turn reference
      // preservation depends on prev staying pristine.
      if (!newEntries) newEntries = entries.slice();
      newEntries[k] = Object.assign({}, e, { aiDescription: desc });
      applied++;
    }
    if (newEntries) {
      if (!newTurns) newTurns = turns.slice();
      newTurns[i] = Object.assign({}, turns[i], { entries: newEntries });
    }
  }
  if (!newTurns) return;
  // Observe-only bracket (describe-freeze lineage, 2026-07-11): begin rides
  // logToHost -> invoke so the host records the attempt even if the iframe
  // freezes inside the broadcast; a begin with no end names the culprit.
  // The bracket now measures the real unit of work: one flush of N patches.
  try {
    window.__bramIframeTrace("describe-patch", {
      stage: "begin",
      patches: applied,
      queued: ids.length,
      provider: prev.provider || "",
      turns: turns.length,
    });
  } catch (traceErr) { /* ignore */ }
  var __describePatchT0 = Date.now();
  window.__bramBroadcastProjectedTurns({
    sid: prev.sid,
    provider: prev.provider,
    turns: newTurns,
  });
  try {
    window.__bramIframeTrace("describe-patch", {
      stage: "end",
      patches: applied,
      provider: prev.provider || "",
      turns: turns.length,
      ms: Date.now() - __describePatchT0,
    });
  } catch (traceErr2) { /* ignore */ }
};

// Adaptive coalesce (2026-07-07 codex esc wedge): one full /__turns
// fetch+parse+broadcast of a long session costs real main-thread time
// (~1.3 s p50 observed on multi-MB sessions), so the window scales to
// the LAST observed cost (4x, floor 250 ms, cap 5 s). With the windowed
// tick below, steady-state fetches are small and the cadence stays at
// the 250 ms floor; the scaling still guards the full-fetch fallbacks.
var __projectedTurnsLastCostMs = 0;
// Tail-window size for tick refreshes. Streaming mutates only the
// in-flight turn (tool results appending) and appends new turns; 8
// covers both with margin. Worst-case tick payload is bounded by turn
// size, not session size.
var __projectedTurnsTickWindow = 8;
window.__bramRefetchProjectedTurns = function (reason, forceFull) {
  if (typeof window.fetch !== "function") return;
  if (__projectedTurnsTimer) return; // trailing-edge coalesce
  var delayMs = Math.max(250, Math.min(4 * __projectedTurnsLastCostMs, 5000));
  __projectedTurnsTimer = window.setTimeout(function () {
    __projectedTurnsTimer = null;
    var seq = ++__projectedTurnsSeq;
    var startedMs = Date.now();
    var prev = __projectedTurnsValue;
    // forceFull: the window-miss re-entry below must NOT re-window against the
    // stale prev — on a session rotation prev still holds the old (>window)
    // session, so a recomputed windowed=true would merge-miss forever. A full
    // fetch converges (rotation or gap).
    var windowed = !forceFull && !!(prev && prev.sid && prev.turns
      && prev.turns.length > __projectedTurnsTickWindow);
    var url = windowed
      ? "/__turns?latest=" + __projectedTurnsTickWindow
      : "/__turns";
    window.fetch(url, { cache: "no-store" })
      .then(function (r) { return r.json(); })
      .then(function (payload) {
        if (seq !== __projectedTurnsSeq) return; // superseded by a later fetch
        var next = windowed
          ? window.__bramMergeProjectedTurnsWindow(prev, payload)
          : payload;
        if (!next) {
          // Rotation/compaction/gap: re-enter for a full fetch (the
          // timer is clear, so this schedules normally). forceFull=true so
          // the re-entry ignores the stale prev and actually fetches full —
          // otherwise it re-windows against the old session and loops.
          window.__bramRefetchProjectedTurns((reason || "") + "-window-miss", true);
          return;
        }
        window.__bramBroadcastProjectedTurns(next);
        __projectedTurnsLastCostMs = Date.now() - startedMs;
        try {
          if (window.logToHost && !window.__bramMenuPending) {
            window.logToHost({
              kind: "iframe-trace",
              subkind: "projected-turns",
              at: new Date().toISOString(),
              reason: reason || "",
              windowed: windowed ? 1 : 0,
              sid: (next && next.sid) || "",
              turns: (next && next.turns && next.turns.length) || 0,
              ms: Date.now() - startedMs,
            });
          }
        } catch (e) {}
      })
      .catch(function () {});
  }, delayMs);
};

// External subscribe factory for projected turns. Same memoized-singleton
// shape as bramSubscribeLatestJsonl above.
window.bramSubscribeProjectedTurns = (function () {
  var factory;
  return function () {
    if (factory) return factory;
    var subscribers = new Set();
    var lastValue = window.getProjectedTurns();
    var notify = function () {
      subscribers.forEach(function (fn) {
        try { fn(); } catch (e) { console.error("[bramSubscribeProjectedTurns] subscriber threw:", e); }
      });
    };
    window.onProjectedTurnsChange(function (v) { lastValue = v; notify(); });
    factory = function (emit) {
      var fire = function () { emit(lastValue); };
      subscribers.add(fire);
      fire();
      window.__bramRefetchProjectedTurns(lastValue == null ? "first-subscribe" : "subscribe");
      return function () { subscribers.delete(fire); };
    };
    return factory;
  };
})();

// transcript-scroll-gestures: the footer's transcript-only jump arrows live
// in Main.xmlui and cannot reach the Transcript component's transcriptList id
// directly, so the Transcript registers its scroll closures at mount. The
// closures capture xs scope (atBottom, transcriptList); this shim only stores
// and dispatches them. Mount-time re-registration overwrites stale closures
// from a prior mount, so no unregister step is needed.
window.__bramRegisterTranscriptScroll = function (goTop, goBottom) {
  window.__bramTranscriptScrollActions = { top: goTop, bottom: goBottom };
};
window.__bramTranscriptScroll = function (dir) {
  var a = window.__bramTranscriptScrollActions;
  if (!a) return;
  try {
    if (dir === "top" && a.top) a.top();
    else if (a.bottom) a.bottom();
  } catch (e) {}
};

window.__bramTranscriptMount = function () {
  if (window.__bramSetTranscriptMounted) window.__bramSetTranscriptMounted(true);
  if (window.__bramRefetchProjectedTurns) window.__bramRefetchProjectedTurns("transcript-mount");
};

window.__bramWorkspaceMount = function (worklistDataSource) {
  if (worklistDataSource && typeof worklistDataSource.refetch === "function") {
    worklistDataSource.refetch();
  }
  if (window.__bramRefetchProjectedTurns) window.__bramRefetchProjectedTurns("worklist-mount");
};

// Map a Read/Write/Edit path hint (the tool summary) to a Markdown code-fence
// language. Used only for file-op tools so a Bash command that merely mentions
// a ".json" path doesn't mislabel its output.
window.__bramLangFromHint = function (hint) {
  if (!hint) return "";
  var m = String(hint).match(/\.([A-Za-z0-9]+)\b/);
  if (!m) return "";
  var map = {
    rs: "rust", js: "javascript", xs: "javascript", ts: "typescript",
    jsx: "jsx", tsx: "tsx", py: "python", json: "json", xml: "xml",
    xmlui: "xml", html: "html", css: "css", sh: "bash", bash: "bash",
    md: "markdown", toml: "toml", yaml: "yaml", yml: "yaml", sql: "sql",
    go: "go", c: "c", h: "c", rb: "ruby", java: "java"
  };
  return map[m[1].toLowerCase()] || "";
};

// Format a tool-result string for the Transcript expansion as a Markdown
// string: detect JSON / diff / file-by-extension and wrap in a fence-safe code
// block so <Markdown overflowMode="scroll"> renders monospace with preserved
// structure and horizontal scroll. Pure, no side effects.
// execute-sql-long-string-cells: resolve the working text for the
// execute_sql formatters. Unwraps (a) the MCP content-block array
// (`[{"type":"text","text":"…"}]` — older-host transcripts and hot loads
// reach the client un-normalized, and parsing the wrapper as rows
// rendered a nonsense |type|text| table) and (b) the `{"result":"…"}`
// envelope. Returns the innermost prose+rows text.
window.__bramExecuteSqlInnerText = function (text) {
  var inner = String(text == null ? "" : text);
  var t = inner.trim();
  if (t.charAt(0) === "[") {
    try {
      var blocks = JSON.parse(t);
      if (
        Array.isArray(blocks) && blocks.length > 0 &&
        blocks.every(function (b) {
          return b && b.type === "text" && typeof b.text === "string";
        })
      ) {
        inner = blocks.map(function (b) { return b.text; }).join("\n");
        t = inner.trim();
      }
    } catch (e) {}
  }
  if (t.charAt(0) === "{") {
    try {
      var obj = JSON.parse(t);
      if (obj && typeof obj.result === "string") inner = obj.result;
    } catch (e) {}
  }
  return inner;
};

// execute-sql-long-string-cells: single-row rendering when a value is a
// whole document (pg_get_viewdef's view definition et al). Short values
// render as `key: value` lines; long strings as fenced code blocks —
// `sql` when the text looks like SQL; nested objects as fenced JSON.
window.__bramExecuteSqlRowSections = function (row) {
  var LONG = 300, CAP = 16384;
  var parts = [];
  var keys = Object.keys(row);
  for (var i = 0; i < keys.length; i++) {
    var k = keys[i], v = row[k];
    if (typeof v === "string" && v.length > LONG) {
      var s = v;
      if (s.length > CAP) s = s.slice(0, CAP) + "\n… (truncated)";
      var lang = /^\s*(with|select|create|alter|insert|update|delete)\b/i.test(s) ? "sql" : "";
      parts.push("**" + k + "**:\n\n```" + lang + "\n" + s + "\n```");
    } else if (v !== null && typeof v === "object") {
      var js = JSON.stringify(v, null, 2);
      if (js.length > CAP) js = js.slice(0, CAP) + "\n… (truncated)";
      parts.push("**" + k + "**:\n\n```json\n" + js + "\n```");
    } else {
      parts.push("**" + k + "**: " + (v === null || v === undefined ? "" : String(v)));
    }
  }
  return parts.join("\n\n");
};

// mcp-sql-shape-driven-rendering: does this MCP tool result carry a
// SQL-shaped payload? Two positive signatures, no tool-name list:
// - the `<untrusted-data-…>` boundary tag — the Supabase MCP wrapper's
//   own prompt-injection fence, stamped on every SQL-backed result
//   (execute_sql, get_logs, get_advisors, …) and never on prose;
// - the result parses WHOLESALE as a JSON rows-array or object
//   (bare-rows servers like the postgres MCP) — wholesale, so a JSON
//   array merely embedded in prose can't false-fire.
// Content-block arrays (elements with an MCP `type` tag) are excluded:
// they are transport wrapper, not rows — a wrapped SQL payload is
// caught by the boundary-tag arm after unwrap instead.
window.__bramMcpSqlShaped = function (text) {
  try {
    var inner = window.__bramExecuteSqlInnerText(text);
    if (inner.indexOf("<untrusted-data-") >= 0) return true;
    var t = inner.trim();
    if (t.charAt(0) !== "[" && t.charAt(0) !== "{") return false;
    var parsed = JSON.parse(t);
    if (Array.isArray(parsed)) {
      if (parsed.length === 0) return false;
      var blockTypes = { text: 1, image: 1, audio: 1, resource: 1, resource_link: 1, tool_use: 1, tool_result: 1 };
      var allObjects = parsed.every(function (r) {
        return r && typeof r === "object" && !Array.isArray(r);
      });
      if (!allObjects) return false;
      var contentBlocks = parsed.every(function (r) {
        return typeof r.type === "string" && blockTypes[r.type] === 1;
      });
      return !contentBlocks;
    }
    return parsed !== null && typeof parsed === "object";
  } catch (e) {
    return false;
  }
};

// render-supabase-execute-sql: turn a Supabase execute_sql result into a
// Markdown table, or null if it doesn't look like rows (DDL, no rows, parse
// failure) so the caller falls back to generic formatting. The rows are a JSON
// array inside the tool's `{"result": "…<untrusted-data-…>[rows]</…>…"}` shape.
window.__bramSupabaseSqlTable = function (text) {
  try {
    var inner = window.__bramExecuteSqlInnerText(text);
    // The rows are the one JSON array in the result. Extract first "[" to last
    // "]"; the preamble/postamble are prose (they even mention the
    // <untrusted-data-…> tag, so keying on that tag mis-captures the prose).
    var lb = inner.indexOf("["), rb = inner.lastIndexOf("]");
    if (lb < 0 || rb <= lb) return null;
    var arrText = inner.slice(lb, rb + 1);
    var rows = JSON.parse(arrText);
    if (!Array.isArray(rows) || rows.length === 0) return null;
    var cols = [];
    for (var i = 0; i < rows.length; i++) {
      var r = rows[i];
      if (!r || typeof r !== "object" || Array.isArray(r)) return null;
      for (var k in r) {
        if (Object.prototype.hasOwnProperty.call(r, k) && cols.indexOf(k) < 0) cols.push(k);
      }
    }
    if (cols.length === 0) return null;
    // execute-sql-json-result-fenced: a single row whose sole value is a
    // nested object/array makes a useless 1x1 table (the whole JSON blob
    // newline-collapsed into one cell — the pa11 STATE specimen). Decline
    // so the caller's JSON formatter pretty-prints it instead.
    if (rows.length === 1 && cols.length === 1) {
      var only = rows[0][cols[0]];
      if (only && typeof only === "object") return null;
    }
    // execute-sql-long-string-cells: tables are for scannable values. A
    // cell holding a whole DOCUMENT (pg_get_viewdef's view definition,
    // 900-2300 chars in the pa11 specimens) is unreadable
    // newline-collapsed and Markdown-mangles its * and _ (count(*)
    // rendered as italic count()) — decline so the caller's section
    // renderer takes it. Merely long-ish cells (a 313-char log
    // event_message across 21 rows — the get_logs specimen) keep the
    // table and truncate below; declining them dropped a genuinely
    // tabular result to the generic envelope blob.
    // Single-row results are document-shaped as soon as any value runs
    // long (a lone view definition deserves a code block, not a cell);
    // multi-row results stay tables up to a document-sized cell, with
    // truncation below keeping them scannable.
    var DOC_CELL = rows.length === 1 ? 300 : 1000;
    for (var ri = 0; ri < rows.length; ri++) {
      for (var rk in rows[ri]) {
        if (!Object.prototype.hasOwnProperty.call(rows[ri], rk)) continue;
        var rv = rows[ri][rk];
        var rs = rv === null || rv === undefined
          ? ""
          : typeof rv === "object" ? JSON.stringify(rv) : String(rv);
        if (rs.length > DOC_CELL) return null;
      }
    }
    var esc = function (v) {
      if (v === null || v === undefined) return "";
      var s = typeof v === "object" ? JSON.stringify(v) : String(v);
      if (s.length > 300) s = s.slice(0, 297) + "…";
      return s.replace(/\|/g, "\\|").replace(/[\r\n]+/g, " ");
    };
    var CAP = 50;
    var out = "| " + cols.map(esc).join(" | ") + " |\n";
    out += "| " + cols.map(function () { return "---"; }).join(" | ") + " |\n";
    var n = Math.min(rows.length, CAP);
    for (var j = 0; j < n; j++) {
      var row = rows[j];
      out += "| " + cols.map(function (c) { return esc(row[c]); }).join(" | ") + " |\n";
    }
    if (rows.length > CAP) out += "\n_+" + (rows.length - CAP) + " more rows_\n";
    return out;
  } catch (e) {
    return null;
  }
};

// execute-sql-json-result-fenced: pretty-print an execute_sql result that is
// JSON but not tabular — a lone object, or a single row whose sole value is
// a nested object/array (the case __bramSupabaseSqlTable declines). Same
// payload extraction as the table formatter; null on anything else so the
// caller falls back to generic formatting.
window.__bramSupabaseSqlJson = function (text) {
  try {
    var inner = window.__bramExecuteSqlInnerText(text);
    var payload = null;
    var lb = inner.indexOf("["), rb = inner.lastIndexOf("]");
    if (lb >= 0 && rb > lb) {
      try { payload = JSON.parse(inner.slice(lb, rb + 1)); } catch (e) {}
    }
    if (payload == null) {
      var lbo = inner.indexOf("{"), rbo = inner.lastIndexOf("}");
      if (lbo >= 0 && rbo > lbo) {
        try { payload = JSON.parse(inner.slice(lbo, rbo + 1)); } catch (e) {}
      }
    }
    if (payload == null || typeof payload !== "object") return null;
    var value = payload;
    if (Array.isArray(payload)) {
      // execute-sql-long-string-cells: rows carrying document-sized
      // strings (view definitions) get the section renderer — the table
      // declined them, and pretty JSON would flatten the document into
      // one escaped line. Bounded to a few rows; larger long-string
      // result sets fall through to generic formatting.
      var objRows = payload.length > 0 && payload.every(function (r) {
        return r && typeof r === "object" && !Array.isArray(r);
      });
      if (objRows) {
        var anyLong = payload.some(function (r) {
          return Object.keys(r).some(function (k) {
            return typeof r[k] === "string" && r[k].length > 300;
          });
        });
        if (anyLong && payload.length <= 5) {
          return payload.map(window.__bramExecuteSqlRowSections).join("\n\n---\n\n");
        }
      }
      if (payload.length !== 1) return null;
      value = payload[0];
      if (value && typeof value === "object" && !Array.isArray(value)) {
        var keys = Object.keys(value);
        if (keys.length === 1 && keys[0] && value[keys[0]] && typeof value[keys[0]] === "object") {
          value = value[keys[0]];
        }
      }
    }
    if (!value || typeof value !== "object") return null;
    var pretty = JSON.stringify(value, null, 2);
    if (!pretty) return null;
    // Same size discipline as the generic formatter's cap: an enormous
    // result stays useful without freezing layout (tool-format lineage).
    if (pretty.length > 16384) {
      pretty = pretty.slice(0, 16384) + "\n… (truncated)";
    }
    return "```json\n" + pretty + "\n```";
  } catch (e) {
    return null;
  }
};

// transcript-wrap-freeform-feedback: a Read of an iterate feedback draft
// (resources/feedback-drafts/<ref>.md) is our own freeform prose — no
// alignment to preserve, so it renders wrapped instead of as a scrolling
// code block. Substring match covers absolute and repo-relative hints.
window.__bramIsFeedbackDraftRead = function (toolName, hint) {
  return String(toolName || "") === "Read" &&
    String(hint || "").indexOf("/feedback-drafts/") >= 0;
};

// Normalize nested structured results from Codex unified exec and standard
// MCP CallToolResult envelopes. The Rust projection performs the same
// normalization for current sessions; this provider-neutral client fallback
// also covers Claude records and hot-loaded transcripts projected by an older
// host binary. Mixed MCP blocks stay as complete JSON so no content is lost.
window.__bramParseStructuredJsonSequence = function (value) {
  var source = String(value == null ? "" : value).trim();
  if (source.charAt(0) !== "{" && source.charAt(0) !== "[") return null;
  var values = [];
  var start = -1, depth = 0, quote = false, escaped = false;
  for (var i = 0; i < source.length; i++) {
    var ch = source.charAt(i);
    if (start < 0) {
      if (/\s/.test(ch)) continue;
      if (ch !== "{" && ch !== "[") return null;
      start = i;
      depth = 1;
      continue;
    }
    if (quote) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === "\"") quote = false;
      continue;
    }
    if (ch === "\"") quote = true;
    else if (ch === "{" || ch === "[") depth++;
    else if (ch === "}" || ch === "]") {
      depth--;
      if (depth < 0) return null;
      if (depth === 0) {
        try { values.push(JSON.parse(source.slice(start, i + 1))); }
        catch (e) { return null; }
        start = -1;
      }
    }
  }
  return start < 0 && values.length > 0 ? values : null;
};

window.__bramNormalizeStructuredToolResult = function (result) {
  var text = String(result == null ? "" : result);
  var failed = text.indexOf("Script failed\n") === 0;
  if (text.indexOf("Script completed\n") === 0 || failed) {
    var outputAt = text.indexOf("\nOutput:\n");
    if (outputAt >= 0) text = text.slice(outputAt + "\nOutput:\n".length);
  }

  var preamble = "";
  var truncated = text.match(/^(Warning: truncated output \(original token count: \d+\)\nTotal output lines: \d+\n\n)([\s\S]*)$/);
  if (truncated) {
    preamble = truncated[1];
    text = truncated[2];
  }
  function withPreamble(value) { return preamble + value; }

  var trimmed = text.trim();
  if (trimmed.charAt(0) !== "{" && trimmed.charAt(0) !== "[") {
    return withPreamble(text);
  }
  var parsedValues = window.__bramParseStructuredJsonSequence(trimmed);
  if (!parsedValues) return withPreamble(text);
  if (parsedValues.length > 1) {
    var normalizedValues = parsedValues.map(function (value) {
      return window.__bramNormalizeStructuredToolResult(JSON.stringify(value));
    }).filter(function (value) { return value !== ""; });
    return withPreamble(normalizedValues.join("\n"));
  }
  var parsed = parsedValues[0];
  try {

    if (!Array.isArray(parsed)) {
      var execKeys = [
        "chunk_id", "wall_time_seconds", "exit_code",
        "original_token_count", "session_id", "metadata"
      ];
      var isExecEnvelope = Object.prototype.hasOwnProperty.call(parsed, "output") &&
        execKeys.some(function (key) {
          return Object.prototype.hasOwnProperty.call(parsed, key);
        });
      if (isExecEnvelope) {
        var useful = parsed.output;
        if ((useful === "" || useful == null) && parsed.stderr != null) useful = parsed.stderr;
        if (typeof useful === "string") {
          return withPreamble(window.__bramNormalizeStructuredToolResult(useful));
        }
        return withPreamble(JSON.stringify(useful, null, 2));
      }

      if (Array.isArray(parsed.content) && parsed.content.length > 0) {
        var allText = parsed.content.every(function (part) {
          return part &&
            /^(text|input_text|output_text)$/.test(String(part.type || "")) &&
            typeof part.text === "string";
        });
        if (allText) {
          return withPreamble(window.__bramNormalizeStructuredToolResult(
            parsed.content.map(function (part) { return part.text; }).join("\n")
          ));
        }
      }
    }
    return withPreamble(JSON.stringify(parsed, null, 2));
  } catch (e) {
    return withPreamble(text);
  }
};

window.__bramIsMcpToolName = function (name) {
  var text = String(name || "");
  return /^mcp__.+__.+$/.test(text) || /^[A-Za-z0-9_-]+\.[A-Za-z0-9_.-]+$/.test(text);
};

window.__bramToolResultIsStructuredJson = function (result) {
  var raw = String(result == null ? "" : result);
  if (raw.indexOf("Script completed\n") === 0 || raw.indexOf("Script failed\n") === 0) {
    var outputAt = raw.indexOf("\nOutput:\n");
    if (outputAt >= 0) raw = raw.slice(outputAt + "\nOutput:\n".length);
  }
  raw = raw.replace(
    /^Warning: truncated output \(original token count: \d+\)\nTotal output lines: \d+\n\n/,
    ""
  ).trim();
  if (window.__bramParseStructuredJsonSequence(raw)) return true;

  var text = window.__bramNormalizeStructuredToolResult(result).trim();
  text = text.replace(
    /^Warning: truncated output \(original token count: \d+\)\nTotal output lines: \d+\n\n/,
    ""
  ).trim();
  return !!window.__bramParseStructuredJsonSequence(text);
};

// transcript-render-menu-answers iterate: AskUserQuestion results arrive as
// a quoted sentence — `Your questions have been answered: "Q"="A", ...` —
// which read as a scrolling monospace blob. Parse the "Q"="A" pairs into
// question/answer prose; null on parse miss so the caller self-declines to
// the generic fence.
window.__bramAskUserQuestionQA = function (text) {
  var pairs = String(text || "").match(/"[^"]+"="[^"]*"/g);
  if (!pairs || !pairs.length) return null;
  var out = [];
  for (var i = 0; i < pairs.length; i++) {
    var mm = /"([^"]+)"="([^"]*)"/.exec(pairs[i]);
    if (mm) out.push("**" + mm[1] + "**\n\n✔ " + mm[2]);
  }
  return out.length ? out.join("\n\n") : null;
};

// Overflow mode for a transcript tool-result Markdown: feedback-draft prose
// and structured JSON wrap ('flow'); everything else keeps horizontal scroll.
window.__bramFreeformResultMode = function (item) {
  if (!item) return "scroll";
  if (window.__bramIsFeedbackDraftRead(item.name, item.summary)) return "flow";
  if (window.__bramIsMcpToolName(item.name)) return "flow";
  if (item.name === "AskUserQuestion") return "flow";
  if (item.resultStructured) return "flow";
  return window.__bramToolResultIsStructuredJson(item.result) ? "flow" : "scroll";
};

// Whether the expanded tool row shows the command/summary block. Only when
// commandDisplay adds something beyond the header: the summary-only fallback
// (MCP tools, Read, Grep, …) would just repeat the summary the row header
// already shows. apply_patch renders its command as a DiffView instead; a
// feedback-draft Read's wrapped content stands alone in the command's place.
window.__bramShowToolCommand = function (item) {
  if (!item || !item.commandDisplay) return false;
  if (item.name === "apply_patch") return false;
  return !window.__bramIsFeedbackDraftRead(item.name, item.summary);
};

window.__bramFormatToolResult = function (result, toolName, hint) {
  if (result == null) return "";
  var text = String(result);
  if (text.trim() === "") return text;
  // mcp-sql-shape-driven-rendering: render SQL-shaped MCP results as a
  // Markdown table / pretty JSON / long-string sections, recognized by
  // result SHAPE (boundary tag or wholesale rows JSON), not tool name —
  // name gates missed twice in one day (the claude.ai connector's server
  // segment, then mcp__supabase__get_logs returning the same envelope).
  // The pipeline self-declines back to generic formatting on anything
  // non-tabular/non-JSON.
  var __toolNameStr = String(toolName || "");
  if (__toolNameStr.indexOf("mcp__") === 0 && window.__bramMcpSqlShaped(text)) {
    var sqlTable = window.__bramSupabaseSqlTable(text);
    if (sqlTable) return sqlTable;
    // execute-sql-json-result-fenced: JSON-but-not-tabular results (lone
    // object, single row wrapping a nested object) render as pretty JSON.
    var sqlJson = window.__bramSupabaseSqlJson(text);
    if (sqlJson) return sqlJson;
  }
  // AskUserQuestion: question/answer prose instead of a code fence
  // (transcript-render-menu-answers iterate); generic path on parse miss.
  if (__toolNameStr === "AskUserQuestion") {
    var qa = window.__bramAskUserQuestionQA(text);
    if (qa) return qa;
  }
  // tool-format sync bracket (variant-B expansion freeze, 2026-07-11
  // 22:48Z): the freeze lives somewhere in formatter → Markdown → WebKit
  // layout of a large expanded row, with the click handler exonerated by
  // the host-side describe route entry. Bracket the formatter's string
  // work synchronously (logToHost → invoke survives a freeze) for large
  // inputs only: begin-no-end names the formatter; begin+end then silence
  // names Markdown/layout by elimination. longestLine captures the
  // dimension the 16KB cap below does NOT bound — the rg row's 11.5K-char
  // single line is the prime layout suspect.
  var bracketT0 = 0;
  var bracketBig = text.length > 8000;
  if (bracketBig) {
    var lineLongest = 0, lineCur = 0;
    for (var bi = 0; bi < text.length; bi++) {
      if (text.charCodeAt(bi) === 10) { lineCur = 0; }
      else { lineCur++; if (lineCur > lineLongest) lineLongest = lineCur; }
    }
    bracketT0 = performance.now();
    window.__bramIframeTrace("tool-format", {
      stage: "begin", tool: String(toolName || ""),
      chars: text.length, longestLine: lineLongest,
    });
  }
  // Strip ANSI escape sequences so raw \x1b[...m bytes don't render literally.
  text = text.replace(/\x1b\[[0-9;?]*[ -\/]*[@-~]/g, "");
  text = window.__bramNormalizeStructuredToolResult(text);

  var MAX_RENDER = 16000;

  // Feedback-draft Reads: strip the cat -n line-number gutter and return
  // unfenced prose; the Transcript pairs this with overflowMode="flow" via
  // __bramFreeformResultMode so it wraps.
  if (window.__bramIsFeedbackDraftRead(toolName, hint)) {
    var prose = text.replace(/^\s*\d+\t/gm, "");
    if (prose.length > MAX_RENDER) {
      prose = prose.slice(0, MAX_RENDER) +
        "\n… (+" + (prose.length - MAX_RENDER) + " more chars — full output in the session JSONL)";
    }
    if (bracketBig) {
      window.__bramIframeTrace("tool-format", {
        stage: "end", tool: String(toolName || ""),
        ms: Math.round(performance.now() - bracketT0), outChars: prose.length,
      });
    }
    return prose;
  }

  var lang = "";
  var body = text;
  var trimmed = text.trim();

  // JSON object/array that round-trips -> pretty-print for structure.
  if (trimmed.charAt(0) === "{" || trimmed.charAt(0) === "[") {
    try {
      var parsed = JSON.parse(trimmed);
      if (parsed && typeof parsed === "object") {
        body = JSON.stringify(parsed, null, 2);
        lang = "json";
      }
    } catch (e) { /* not JSON */ }
  }

  // Unified diff markers (any line).
  if (lang === "" && /^(diff --git |@@ |\+\+\+ |--- )/m.test(text)) {
    lang = "diff";
  }

  // File-op tools: language by extension from the path hint.
  if (lang === "" && /^(Read|Write|Edit|NotebookEdit)$/.test(String(toolName || ""))) {
    lang = window.__bramLangFromHint(hint);
  }

  // Render cap (2026-07-09 describe-freeze): an expanded 41KB result
  // re-rendering through Markdown froze the codex transcript's main
  // thread hard (trace went silent at the describe-patch instant; the
  // 4.8KB row a few seconds earlier sailed through). Unbounded content
  // in the webview is the recurring codex-session killer, so bound the
  // rendered block; the full output remains in the session JSONL
  // (Sessions tab / /__tool-detail).
  if (body.length > MAX_RENDER) {
    body =
      body.slice(0, MAX_RENDER) +
      "\n… (+" + (body.length - MAX_RENDER) + " more chars — full output in the session JSONL)";
  }

  // Fence-safety: the fence must be longer than the longest backtick run in
  // the body, or content containing ``` would break the block.
  var longest = 0, run = 0;
  for (var i = 0; i < body.length; i++) {
    if (body.charAt(i) === "`") { run++; if (run > longest) longest = run; }
    else { run = 0; }
  }
  var fence = "";
  var fenceLen = Math.max(3, longest + 1);
  for (var j = 0; j < fenceLen; j++) { fence += "`"; }

  var out = fence + lang + "\n" + body + "\n" + fence;
  if (bracketBig) {
    window.__bramIframeTrace("tool-format", {
      stage: "end", tool: String(toolName || ""),
      ms: Math.round(performance.now() - bracketT0), outChars: out.length,
    });
  }
  return out;
};

// Append the live pending agent menu (if any) as the last transcript event.
// Called by the projected-turns adapter (__bramTranscriptEventsFromTurns).
window.__bramAppendMenuEvents = function (events, menu) {
  // menu-stack-pty-inflight-prose: while a permission menu is up, Claude hasn't
  // written the turn's assistant record to its JSONL yet, so the explanatory
  // prose is missing from the transcript. Show the grid-sourced prose
  // (menu.inflightProse) as a PROVISIONAL block above the menu. A live menu
  // supplies the prose; once it's dismissed there is a ~0.5–1.6s gap before the
  // real record lands, during which we KEEP showing the prose (bridge) so it
  // doesn't blink out and back. The bridge clears when the real record lands
  // (content match) or after an 8s backstop (a no-tool_use prompt may never
  // produce a record). The provisional is a separate block, never inserted into
  // the record list, so there is no duplicate risk; a stable id keeps XMLUI from
  // remounting (flashing) across the live→bridge→swap transitions.
  var prose = ((menu && menu.inflightProse) || "").trim();
  if (prose) {
    window.__bramPendingProse = { text: prose, atMs: Date.now() };
  } else if (
    window.__bramPendingProse &&
    Date.now() - window.__bramPendingProse.atMs < 8000
  ) {
    prose = window.__bramPendingProse.text;
  }
  if (prose) {
    var key = prose.replace(/\s+/g, " ").trim().toLowerCase().slice(0, 40);
    var present = false;
    for (var k = events.length - 1; k >= 0 && events.length - k <= 10; k--) {
      if (
        events[k].kind === "text" &&
        key &&
        events[k].text.replace(/\s+/g, " ").toLowerCase().indexOf(key) >= 0
      ) {
        present = true;
        break;
      }
    }
    if (present) {
      window.__bramPendingProse = null;
    } else {
      events.push({ id: "menu-prose", kind: "menu-prose", text: prose });
    }
  }
  if (menu) {
    events.push({ id: "menu-pending-" + window.__bramMenuIdentity(menu), kind: "menu", menu: menu });
  }
  return events;
};

// Presentation adapter: flatten projected turns (host shape: { role, text,
// entries[], images[] }) into the Transcript's flat event stream. No JSONL
// parsing and no resolution rules here — that is the host projection's job
// (docs/turn-transport-redesign.md). Per-turn event slices are cached by
// turn object identity: the broadcast preserves unchanged turn refs, so
// unchanged rows keep identical event objects and the List doesn't
// re-mount them.
window.__bramProjectedEventCache = (typeof WeakMap === "function") ? new WeakMap() : null;
window.__bramTranscriptEventsFromTurns = function (payload, menu) {
  // viewport-priority describe: the pump maps mounted rows' data-index
  // back to entries through this snapshot of the adapter's last output.

  var turns = (payload && payload.turns) || [];
  var events = [];
  for (var ti = 0; ti < turns.length; ti++) {
    var t = turns[ti] || {};
    var slice = window.__bramProjectedEventCache && window.__bramProjectedEventCache.get(t);
    if (!slice) {
      slice = [];
      var baseId = "pt" + ti;
      if (t.notification) {
        // Host-reclassified task notification (subagent completion
        // report): a quiet system-note row, never a "You" turn.
        slice.push({
          id: baseId + "-n",
          kind: "notification",
          text: t.text || "",
          taskId: t.taskId || "",
          toolUseId: t.toolUseId || "",
        });
      } else if (t.role === "user") {
        if (t.text && String(t.text).trim()) {
          slice.push({ id: baseId + "-u", kind: "user", text: t.text });
        }
      } else {
        var entries = t.entries || [];
        for (var ei = 0; ei < entries.length; ei++) {
          var e = entries[ei] || {};
          var eid = baseId + "-" + ei;
          if (e.kind === "text") {
            if (e.text && String(e.text).trim()) {
              slice.push({ id: eid, kind: "text", text: e.text });
            }
          } else if (e.kind === "thinking") {
            slice.push({ id: eid, kind: "thinking", text: e.text || "" });
          } else if (e.kind === "tool") {
            // Spread-through, not whitelist: this copy used to enumerate
            // fields, which silently stripped any NEW projection field
            // before the List rendered (the 2026-07-22 "feature does not
            // exist" hunt: host, serving, caches, and markup were all
            // current; this adapter dropped nameDetail/aiDescription).
            // All of e passes through; the explicit keys below only pin
            // defaults and identity.
            slice.push(Object.assign({}, e, {
              id: e.id || eid,
              kind: "tool",
              toolId: e.id || "",
              name: e.name || "Tool",
              summary: e.summary || "",
              commandDisplay: e.commandDisplay || "",
              commandMarkdown: e.commandMarkdown || "",
              description: e.description || "",
              nameDetail: e.nameDetail || "",
              aiDescription: e.aiDescription || "",
              menuAnswer: e.menuAnswer || "",
              result: e.result || "",
              resultStructured: !!e.resultStructured,
              // Edit/MultiEdit reconstructed diff from the host projection
              // (claude-edit-tool-result-diff-preview). Transcript.xmlui
              // renders it via DiffView when present; the adapter must pass
              // it through or $item.diff is undefined and the view never
              // mounts.
              diff: e.diff || "",
              isError: !!e.isError,
              agentId: e.agentId || "",
            }));
          }
        }
      }
      if (t.images && t.images.length) {
        slice.push({ id: baseId + "-images", kind: "images", role: t.role || "", images: t.images });
      }
      if (window.__bramProjectedEventCache) window.__bramProjectedEventCache.set(t, slice);
    }
    for (var si = 0; si < slice.length; si++) events.push(slice[si]);
  window.__bramLastTranscriptEvents = events;
  }
  return window.__bramAppendMenuEvents(events, menu);
};

// Pure consumer shim for the Sessions tab: unwrap the /__turns envelope.
window.__bramProjectedSessionTurns = function (payload) {
  return (payload && payload.turns) || [];
};

// ---- In-view find (search-in-view-find) ----
// Ordered indices of the projected turns whose text contains the needle
// (case-insensitive). The index IS the List data index, so the caller can
// List.scrollToIndex() straight to a match. Gated at >= 2 chars to skip the
// noise scans of a single keystroke (mirrors the Search page's `ready` gate).
window.__bramFindMatchingTurnIndices = function (turns, needle) {
  var q = (needle == null ? "" : String(needle)).trim().toLowerCase();
  if (q.length < 2 || !turns || !turns.length) return [];
  var out = [];
  for (var i = 0; i < turns.length; i++) {
    var t = turns[i];
    var text = t && t.text ? String(t.text).toLowerCase() : "";
    if (text.indexOf(q) !== -1) out.push(i);
  }
  return out;
};

// Step a match cursor by `dir` (+1 / -1) with wraparound. Returns 0 when there
// are no matches, so the caller never indexes an empty array.
window.__bramFindStep = function (indices, cur, dir) {
  var n = indices && indices.length ? indices.length : 0;
  if (!n) return 0;
  var c = Number(cur) || 0;
  return (c + dir + n) % n;
};

// ---- Cross-block in-view find (search-in-view-transplant) ----
// The non-virtualized detail views (Commit/Issue/History) render several
// Markdown blocks. A single global cursor steps every match across all blocks;
// the block holding the active match gets its local occurrence index (fed to
// Markdown's highlightActiveIndex), everyone else gets -1.

// Wraparound step over a flat count of matches.
window.__bramCursorStep = function (total, cur, dir) {
  var n = Number(total) || 0;
  if (!n) return 0;
  var c = Number(cur) || 0;
  return (c + dir + n) % n;
};

// Case-insensitive match count of needle in one block of text.
window.__bramCountOccurrences = function (text, needle) {
  var s = text == null ? "" : String(text);
  var q = (needle == null ? "" : String(needle)).trim().toLowerCase();
  if (q.length < 2 || !s) return 0;
  var hay = s.toLowerCase();
  var n = 0, i = hay.indexOf(q);
  while (i !== -1) { n++; i = hay.indexOf(q, i + q.length); }
  return n;
};

// Per-block counts + total for an ordered array of block texts.
window.__bramBlockMatchCounts = function (blocks, needle) {
  var arr = blocks || [], counts = [], total = 0;
  for (var i = 0; i < arr.length; i++) {
    var c = window.__bramCountOccurrences(arr[i], needle);
    counts.push(c);
    total += c;
  }
  return { counts: counts, total: total };
};

// Given per-block counts and a global cursor, the local occurrence index active
// in `blockIdx`, or -1 when the cursor falls in a different block.
window.__bramActiveOccForBlock = function (counts, cursor, blockIdx) {
  var c = counts || [], k = Number(cursor) || 0, acc = 0;
  for (var i = 0; i < c.length; i++) {
    var cnt = c[i] || 0;
    if (i === blockIdx) return (k >= acc && k < acc + cnt) ? (k - acc) : -1;
    acc += cnt;
  }
  return -1;
};

// Ordered block-text arrays per detail view (index positions must match the
// order the components render their Markdown blocks).
window.__bramCommitBlocks = function (commit) {
  return [(commit && commit.message) || ""];
};
window.__bramIssueBlocks = function (issue) {
  var out = [(issue && issue.body) || ""];
  var comments = (issue && issue.comments) || [];
  for (var i = 0; i < comments.length; i++) out.push((comments[i] && comments[i].body) || "");
  return out;
};
window.__bramHistoryBlocks = function (g) {
  var out = [
    window.__bramHistoryItemFieldMarkdown(g, "before") || "",
    window.__bramHistoryItemFieldMarkdown(g, "after") || "",
  ];
  var phases = (g && g.phases) || [];
  for (var i = 0; i < phases.length; i++) {
    if (phases[i] && phases[i].kind === "feedback") out.push(phases[i].body || "");
  }
  return out;
};
// Global block index of a History phase's feedback Markdown (before=0, after=1,
// feedback phases follow in order), or -1 for a non-feedback phase.
window.__bramHistoryPhaseBlockIndex = function (g, phaseItemIndex) {
  var phases = (g && g.phases) || [];
  var p = phases[phaseItemIndex];
  if (!p || p.kind !== "feedback") return -1;
  var n = 0;
  for (var i = 0; i < phaseItemIndex; i++) {
    if (phases[i] && phases[i].kind === "feedback") n++;
  }
  return 2 + n;
};

// A List/Items row template is an isolated binding scope — it can read $item
// but NOT the enclosing component's reassigned var.* (appliedNeedle/counts/
// cursor). So the search state must ride in the row's data and be read via
// $item. These decorators fold it in; callers rebuild only when the search
// changes (not every render), so the RO-flood fix stays intact. See the
// feedback_xmlui_list_row_scope_isolated learning.

// Decorate an arbitrary nested-loop array (issue comments, history phases)
// with the current search state under __-prefixed keys, preserving each
// item's own fields via a shallow copy.
window.__bramWithSearch = function (arr, needle, counts, cursor) {
  var a = Array.isArray(arr) ? arr : [];
  var n = needle || "", c = counts || [], k = (typeof cursor === "number" ? cursor : 0);
  return a.map(function (x) {
    return Object.assign({}, x, { __needle: n, __counts: c, __cursor: k });
  });
};

// Decorate SessionDetail's projected turns with the needle (paints every
// match in each turn) and an __active flag on the turn holding the cursor's
// match (emphasized + centered). activeIdx is matchIndices[currentMatch].
window.__bramSessionSearchRows = function (turns, needle, matchIndices, currentMatch) {
  var arr = Array.isArray(turns) ? turns : [];
  var n = needle || "";
  var mi = Array.isArray(matchIndices) ? matchIndices : [];
  var activeIdx = mi.length ? mi[Number(currentMatch) || 0] : -1;
  return arr.map(function (t, i) {
    return Object.assign({}, t, { __needle: n, __active: i === activeIdx });
  });
};

window.__bramProjectedLastExchange = function (payload) {
  var turns = (payload && payload.turns) || [];
  var lastUser = null;
  var lastAssistantText = "";
  for (var i = 0; i < turns.length; i++) {
    var t = turns[i] || {};
    if (t.notification) continue;
    if (t.role === "user") {
      lastUser = {
        userText: t.text || "",
        userImages: t.images || [],
        assistantText: "",
      };
    } else if (t.role === "assistant") {
      var parts = [];
      var entries = t.entries || [];
      for (var j = 0; j < entries.length; j++) {
        var e = entries[j] || {};
        if (e.kind === "text" && e.text) parts.push(e.text);
      }
      var text = parts.join("\n\n").trim();
      if (text) {
        lastAssistantText = text;
        if (lastUser) lastUser.assistantText = text;
      }
    }
  }
  return {
    lastAssistantText: { text: lastAssistantText },
    lastExchange: lastUser || { userText: "", userImages: [], assistantText: "" },
  };
};

// ---- Subagent visibility (surface-subagent-activity-in-pane) ----
// The Transcript viewport switch is an inline ternary in Transcript.xmlui
// (not a helper here) so the hot-reloaded markup keeps working against a
// running binary that predates this file.

// Footer chip label: description (fallback agentType), truncated, with a
// running/finished glyph.
window.__bramAgentChipLabel = function (agent) {
  if (!agent) return "";
  var label = agent.description || agent.agentType || agent.agentId || "";
  if (label.length > 28) label = label.slice(0, 27) + "…";
  return label + (agent.finished ? " ✓" : " ●");
};

// "claude-fable-5" → "Fable 5", "claude-haiku-4-5-20251001" → "Haiku 4.5":
// strip the vendor prefix and date suffix, capitalize the family, join
// version parts with dots. Display-only; tooltips keep the raw id.
window.__bramPrettyModel = function (model) {
  if (!model) return "";
  var raw = String(model);
  if (/^gpt[-.]/i.test(raw)) return raw.replace(/^gpt/i, "GPT");
  var s = raw.replace(/^claude-/, "").replace(/-\d{8}$/, "");
  var parts = s.split("-");
  if (!parts.length) return model;
  var family = parts[0].charAt(0).toUpperCase() + parts[0].slice(1);
  var nums = parts.slice(1).filter(function (p) { return p !== ""; });
  return nums.length ? family + " " + nums.join(".") : family;
};

// Main-chip tooltip: base text plus the main session's current model
// (roster's mainModel, host-extracted from the session tail).
window.__bramMainChipTooltip = function (roster) {
  var base = "Show the main conversation in the Transcript tab";
  var m = roster && roster.mainModel;
  return m ? base + " — " + m : base;
};

// Chip / overflow-item tooltip: type, description, and the model the
// subagent ran on (host-extracted from the transcript head).
window.__bramAgentChipTooltip = function (agent) {
  if (!agent) return "";
  var s = (agent.agentType || "agent") + ": " + (agent.description || agent.agentId || "");
  if (agent.model) s += " — " + agent.model;
  return s;
};

// Dismissible footer subagent chips (dismissible-subagent-chips): a
// per-session set of dismissed agent ids so a finished subagent's chip can
// be hidden from the footer strip without deleting anything host-side (the
// roster keeps tracking it). sessionStorage scope is deliberate — a new
// agent session gets a fresh roster, so a dismissal shouldn't outlive the
// session. Stored as a JSON array of ids under one key.
var __BRAM_DISMISSED_AGENTS_KEY = "bram.dismissedAgentIds";

window.__bramRestoreDismissedAgents = function () {
  var raw = __bramReadSS(__BRAM_DISMISSED_AGENTS_KEY, "");
  if (!raw) return [];
  try {
    var arr = JSON.parse(raw);
    return Array.isArray(arr) ? arr : [];
  } catch (e) { return []; }
};

// Append agentId to the dismissed set, persist, and return the new array
// (the XMLUI caller assigns the result back to its var — same shape as
// __bramDismissSendNotice).
window.__bramDismissAgent = function (dismissed, agentId) {
  var list = Array.isArray(dismissed) ? dismissed.slice() : [];
  if (agentId && list.indexOf(agentId) === -1) list.push(agentId);
  __bramWriteSS(__BRAM_DISMISSED_AGENTS_KEY, list.length ? JSON.stringify(list) : "");
  return list;
};

// The footer roster minus dismissed ids. Drives the strip's when, the
// top-3 Items, and the "+N more" dropdown so a dismissal promotes the next
// agent up from overflow. Returns the raw agents array untouched when
// nothing is dismissed.
window.__bramVisibleFooterAgents = function (roster, dismissed) {
  var agents = (roster && roster.agents) || [];
  if (!dismissed || !dismissed.length) return agents;
  return agents.filter(function (a) {
    return dismissed.indexOf(a && a.agentId) === -1;
  });
};

// Footer session-info line with the transcript viewport spliced in after
// the provider token: "CLAUDE · Main · july5 · id …" or
// "CLAUDE · subagent: <description> · july5 · id …". The viewport lives
// HERE rather than in chip styling so dropdown-overflow agents get the
// same selection indicator as chip agents.
window.__bramFooterSessionLine = function (session, agentId, roster) {
  var meta = window.__bramSessionMetaLine(session) || "";
  var agents = (roster && roster.agents) || [];
  var mainModel = roster && roster.mainModel;
  // Zero-subagent sessions still get the plain meta line unless we have
  // the main model to report; a bare "Main" is footer noise.
  if (!agentId && agents.length === 0 && !mainModel) return meta;
  var view = "Main";
  if (agentId) {
    var match = null;
    for (var i = 0; i < agents.length; i++) {
      if (agents[i].agentId === agentId) { match = agents[i]; break; }
    }
    view = "subagent: " + ((match && (match.description || match.agentType)) || agentId);
    if (match && match.model) view += " (" + window.__bramPrettyModel(match.model) + ")";
  } else if (mainModel) {
    view = "Main (" + window.__bramPrettyModel(mainModel) + ")";
  }
  if (!meta) return view;
  var sp = meta.indexOf(" ");
  if (sp < 0) return meta + " · " + view;
  return meta.slice(0, sp) + " · " + view + " ·" + meta.slice(sp);
};

// One-line header for a subagent view (Transcript header + inline peek),
// from the /__turns?agent= envelope.
window.__bramSubagentHeaderLine = function (payload) {
  if (!payload || !payload.agentId) return "";
  var label = payload.description || payload.agentId;
  var qual = [];
  if (payload.agentType) qual.push(payload.agentType);
  if (payload.model) qual.push(payload.model);
  var type = qual.length ? " (" + qual.join(" · ") + ")" : "";
  return "Subagent" + type + ": " + label + " — " + (payload.finished ? "finished" : "running…");
};

// Passive send-ledger notice (esc/resend redesign phase 3). Returns the
// status-note text for the most recent RESOLVED ledger entry within the
// last 10 minutes, or "" for silence. No action buttons by design:
// recovery is automatic (restore / trust-gated auto-resend), and
// landed-then-aborted gets silence.
// The ledger entry a notice would be about: latest resolved. Shared by
// the notice text builder and the dismissal key
// (esc-banner-dismissable).
function __bramLatestResolvedLedgerEntry(payload) {
  var entries = (payload && payload.entries) || [];
  var latest = null;
  for (var i = 0; i < entries.length; i++) {
    var e = entries[i];
    if (!e || !e.resolvedAtMs) continue;
    if (!latest || e.resolvedAtMs > latest.resolvedAtMs) latest = e;
  }
  return latest;
}

// Key identifying the current notice for dismissal: the producing
// ledger entry's id. A dismissed key hides THAT notice only; a new
// resolved entry (new id) notices normally.
window.__bramSendLedgerNoticeKey = function (payload) {
  var latest = __bramLatestResolvedLedgerEntry(payload);
  return (latest && latest.id) || "";
};

window.__bramDismissSendNotice = function (key) {
  __bramWriteLS("bram.sendNoticeDismissed", key || "");
  return key || "";
};

window.__bramRestoreSendNoticeDismissed = function () {
  return __bramReadLS("bram.sendNoticeDismissed", "");
};

window.__bramSendLedgerNotice = function (payload, dismissedKey) {
  var entries = (payload && payload.entries) || [];
  var nowMs = (payload && payload.nowMs) || Date.now();
  var staleTerminalInput = !!(payload && payload.staleTerminalInput);
  var latest = __bramLatestResolvedLedgerEntry(payload);
  if (!latest) return "";
  if (dismissedKey && latest.id === dismissedKey) return "";
  if (nowMs - latest.resolvedAtMs > 2 * 60 * 1000) return "";
  // The user moving on dismisses the note: any send injected after this
  // entry resolved means they have re-engaged (2026-07-03: "too sticky").
  for (var j = 0; j < entries.length; j++) {
    var n = entries[j];
    if (n && n.injectedAtMs > latest.resolvedAtMs) return "";
  }
  var label = "";
  try {
    var pv = String(latest.preview || "").replace(/\s+/g, " ").trim();
    if (pv.length > 40) pv = pv.slice(0, 40) + "…";
    if (pv) label = " “" + pv + "”";
  } catch (e) { label = ""; }
  if (latest.state === "stranded" && latest.cause === "user") {
    return "Your message" + label + " didn’t go through — it’s back in the composer, edit and send when ready.";
  }
  if (latest.state === "landed" && latest.cause === "retracted") {
    return "Your message" + label + " was interrupted before the agent took it — it's back in the composer.";
  }
  if (latest.state === "landed" && latest.cause === "aborted") {
    if (!staleTerminalInput) return "";
    // Truthful semantics (2026-07-06 esc drill): the send landed as a
    // transport record but Esc made Claude Code retract and re-stage it
    // in the TERMINAL input, unanswered — and the copy there prepends
    // onto the next terminal-submitted send if not cleared.
    return "Response interrupted — your message" + label + " is back in the terminal input, unanswered. Press Enter there to resend it, or clear it before sending anything new.";
  }
  if (latest.state === "landed" && latest.retried) {
    return "A lost send" + label + " was redelivered automatically.";
  }
  if (latest.state === "stranded" && latest.cause === "mechanical") {
    return "A send" + label + " did not reach the agent; its text is kept in the send ledger (Status tab).";
  }
  return "";
};

// Apply a host `send-restore` event: the restored text goes into the
// composer box and the persisted draft, so the restore survives remounts.
// Aborted restores (p.aborted === true) skip when the composer already holds
// a non-empty draft that differs from the restored text — an already-delivered
// message must not clutter a new draft (2026-07-03 acid test). Strand restores
// (p.aborted falsy) always apply: the text was never delivered and must be
// preserved. When a restore does apply and the composer is non-empty, the
// restored text is appended below a blank line rather than overwriting.
// Called from Main.xmlui / Workspace.xmlui ChangeListeners with their
// respective composer refs.
// toast-issue-closed-on-push: format the host's `issues-closed-on-push`
// payload and toast it. `evtValue` is the bramSubscribeTauriEvent wrapper
// `{ tick, payload }`; the host payload is `{ issues: [n, ...] }`.
window.__bramToastIssuesClosed = function (evtValue, toastApi) {
  var issues = (evtValue && evtValue.payload && evtValue.payload.issues) || [];
  if (!issues.length || typeof toastApi !== "function") return;
  var list = issues
    .map(function (n) {
      return "#" + n;
    })
    .join(", ");
  toastApi("Closed " + list + " on push");
};

window.__bramApplySendRestore = function (snapshot, box) {
  try {
    window.__bramIframeTrace("send-restore", {
      stage: "enter",
      hasSnapshot: !!snapshot,
      hasText: !!(snapshot && snapshot.payload && snapshot.payload.text),
      hasBox: !!box,
    });
  } catch (e) {}
  var p = snapshot && snapshot.payload;
  var text = p && p.text;
  if (!text) return;
  var existing = "";
  try {
    if (box && typeof box.value === "string") {
      existing = box.value;
    } else if (box && box.value != null) {
      existing = String(box.value);
    }
  } catch (e) { existing = ""; }
  var aborted = !!(p && p.aborted);
  if (aborted && existing.trim() !== "" && existing !== text) {
    try {
      window.__bramIframeTrace("send-restore", { chars: text.length, skipped: true });
    } catch (e) {}
    return;
  }
  var merged;
  if (existing.trim() === "" || existing === text) {
    merged = text;
  } else {
    merged = existing + "\n\n" + text;
  }
  try { window.localStorage.setItem("bram.worklistMessageDraft", merged); } catch (e) {}
  if (box && typeof box.setValue === "function") {
    try { box.setValue(merged); } catch (e) {}
  }
  try {
    window.__bramIframeTrace("send-restore", { chars: text.length, merged: existing.trim().length > 0 });
  } catch (e) {}
};

// Stable identity key for the Transcript's pending-menu row: present /
// tool / option keys. Drives the row-lifecycle trace below and is the
// natural candidate for re-keying the synthetic menu event (vs the
// constant "menu-pending") if the stale-row-reuse hypothesis is confirmed.
window.__bramMenuRowKey = function (menu) {
  return window.__bramMenuIdentity(menu);
};

// Trace pending-menu state against Transcript mount state. The menu remains
// interleaved on Transcript; other tabs do not render the full menu.
window.__bramTraceMenuRow = function (menu, stage) {
  try {
    window.__bramIframeTrace("transcript-menu-row", {
      stage: stage || "change",
      present: !!menu,
      tool: (menu && menu.tool) || "",
      options: (menu && menu.options && menu.options.length) || 0,
      key: window.__bramMenuRowKey(menu),
      // Whether the Transcript page is currently mounted. The host setter
      // fires this trace regardless of active tab, so `present:true` with
      // `transcriptMounted:false` means the host pushed a menu while
      // Transcript was unmounted.
      transcriptMounted: !!window.__bramTranscriptMounted,
    });
  } catch (e) {}
};

// Set by Transcript.xmlui on mount/unmount so __bramTraceMenuRow can record
// whether a host menu push happened while Transcript was active. On mount,
// also emit a `stage:mount` row carrying the current menu key.
// Refs menu-miss-mount-instrumentation.
window.__bramSetTranscriptMounted = function (mounted) {
  window.__bramTranscriptMounted = !!mounted;
  if (mounted) window.__bramTraceMenuRow(window.bramAgentMenu, "mount");
  else window.__bramTraceMenuRow(window.bramAgentMenu, "unmount");
};

// The menu-row trace lives in window.__bramApplyAgentMenu (the canonical
// menu-state setter), NOT as a separate pty-menu-changed subscriber — a
// fourth subscriber just joined the churning subscribeTauriEvent registry
// and never reliably fired. See subscribe-tauri-event-churn for that smell.

// Immutable toggle of an id in an array (proven per-item expand pattern,
// matching Workspace's expandedItemIds — avoids object-literal var inits
// that XMLUI's expression engine mishandles).
window.__bramToggleInArray = function (arr, id) {
  arr = arr || [];
  if (arr.indexOf(id) >= 0) return arr.filter(function (x) { return x !== id; });
  return arr.concat([id]);
};

// ai-describe (haiku-command-descriptions): tool-row expand handler.
// Toggles the fold like __bramToggleInArray, and on OPEN of a
// command-bearing entry fires a describe request. The host route is
// double-gated (ai.describeCommands flag + ANTHROPIC_API_KEY) and
// answers {ok:false, reason} when off, so this is a no-op by default.
// Sent even when an agent-authored description exists — the host prompt
// keeps a good description unchanged and upgrades a weak one (approval
// feedback on haiku-command-descriptions).
window.__bramDescribeRequested = {};
// describe-edit-write-rows: the describable text per row. Bash/exec
// (and apply_patch's patch) carry commandDisplay; Edit/MultiEdit carry
// the host-reconstructed diff; a markdown Write carries its content in
// commandMarkdown; other Writes fall back to the summary (tool + path
// — with the agent-context rider, enough for an intent line).
//
// codex-tooluse-describe: everything else falls back to the row summary
// GENERICALLY — no per-tool enumeration. The host summarizer (st_tool_summary)
// probes a candidate arg-field list for unknown tools, so a Codex web/browse
// call, a Task dispatch, a WebFetch, an MCP call, etc. carry real material
// in `summary` instead of a bare name. Read/Grep/Glob are subsumed (their
// summary already carries the target). The floor: skip when `summary` is just
// the tool name (the host probe found nothing) — feeding Haiku a bare name
// would spend a call on a useless line.
window.__bramDescribeMaterial = function (item) {
  if (!item) return "";
  if (item.commandDisplay) return item.commandDisplay;
  var editFamily = { Edit: 1, MultiEdit: 1, NotebookEdit: 1, Write: 1, apply_patch: 1 };
  if (editFamily[item.name]) return item.diff || item.commandMarkdown || item.summary || "";
  var summary = item.summary || "";
  return summary && summary !== item.name ? summary : "";
};
window.__bramExpandTool = function (arr, item) {
  // Arm the xmlui freeze-probe window (xmlui-eval-probe-vendor): for 1.5s
  // after an expansion click, the instrumented vendored engine emits
  // xmlui-probe trace lines (op=eval|stmt|action) synchronously via
  // logToHost → invoke, so a hang in the ensuing re-render names its exact
  // site — the trace stream ends AT the hanging statement/binding/action.
  try { window.__bramXmluiTraceUntil = performance.now() + 1500; } catch (e) {}
  var next = window.__bramToggleInArray(arr, item && item.id);
  try {
    var opening = item && item.id && (next || []).indexOf(item.id) >= 0;
    if (opening && window.__bramDescribeMaterial(item)) {
      window.__bramRequestCommandDescription(item);
    }
  } catch (e) { /* expand must never fail on describe plumbing */ }
  return next;
};

// Fire-and-forget describe POST. The route is synchronous (the host
// serves each request on its own thread); on success the description is
// spliced straight into the accumulated projection via
// __bramPatchProjectedToolDescription (no refetch — see that helper for
// why a windowed refetch can't deliver it). The host also caches the
// result, so later full projections re-serve it via the overlay. Per-id
// dedupe keeps re-expands from re-POSTing while a request is in flight
// (the host also dedupes).
// The prose nearest BEFORE the tool entry — the agent's stated intent
// for the call ("Let me check whether ..."), the highest-signal
// describe context. Scans backward across TURN BOUNDARIES: Claude
// records prose + tool_use in one assistant turn, but Codex records
// each function_call as its own turn with the prose in a PRECEDING
// turn (the 2026-07-09 ctx=0 finding — same-turn-only lookup found
// nothing on codex). A user turn is an acceptable source too: when a
// command directly answers the user's request, that request IS the
// intent. Lookback bounded to 4 turns; tail-capped to 500 chars so the
// sentence closest to the call survives. Empty when the entry isn't in
// the main projection (e.g. subagent views).
window.__bramDescribeContextForTool = function (toolId) {
  var prev = window.getProjectedTurns && window.getProjectedTurns();
  if (!prev || !prev.turns || !toolId) return "";
  var turns = prev.turns;
  for (var i = 0; i < turns.length; i++) {
    var entries = (turns[i] && turns[i].entries) || [];
    for (var k = 0; k < entries.length; k++) {
      var e = entries[k];
      if (!e || e.kind !== "tool" || e.id !== toolId) continue;
      var prose = "";
      var ti = i, ei = k - 1, back = 0;
      while (!prose && back <= 4 && ti >= 0) {
        var es = (turns[ti] && turns[ti].entries) || [];
        for (var p = ei; p >= 0; p--) {
          var t = es[p];
          if (t && t.kind === "text" && t.text) { prose = t.text; break; }
        }
        if (!prose && ti > 0) {
          // Fall back to the turn's own text (user turns carry their
          // message there rather than in a text entry).
          var tt = turns[ti - 1] && turns[ti - 1].text;
          if (tt) { prose = tt; }
        }
        ti -= 1;
        ei = ((turns[ti] && turns[ti].entries) || []).length - 1;
        back += 1;
      }
      return prose.length > 500 ? prose.slice(-500) : prose;
    }
  }
  return "";
};

window.__bramRequestCommandDescription = function (item, onDone) {
  var id = item && item.id;
  var done = function () { try { if (onDone) onDone(); } catch (e) {} };
  if (!id || window.__bramDescribeRequested[id]) { done(); return; }
  window.__bramDescribeRequested[id] = true;
  window
    .fetch("/__describe-command", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        id: id,
        name: item.name || "",
        // Material, not just commandDisplay (describe-edit-write-rows):
        // diffs and patches can be large — cap client-side; the host
        // caps context/result but not command.
        command: (window.__bramDescribeMaterial(item) || "").slice(0, 4000),
        description: item.description || "",
        // Intent prose + result head (iterate 2026-07-08): the agent's
        // stated reason for the call and what it produced — Haiku
        // describes intent, not just syntax. Both capped; the host
        // re-caps defensively.
        context: window.__bramDescribeContextForTool(id),
        result: String(item.result || "").slice(0, 400),
      }),
    })
    .then(function (r) { return r.json(); })
    .then(function (res) {
      if (res && res.ok && res.description) {
        window.__bramDescribeUnavailable = false;
        // Direct splice into the accumulated projection — see
        // __bramPatchProjectedToolDescription for why a refetch can't
        // deliver this. Patch misses (subagent view: the entry isn't in
        // the main projection) are covered by the host's
        // subagents-changed emit, which refetches the subagent stream
        // with the overlay applied.
        window.__bramPatchProjectedToolDescription(id, res.description);
      } else {
        // Feature off: latch so the eager scan stops re-POSTing every
        // broadcast; a manual expand bypasses the latch and can revive.
        if (res && (res.reason === "disabled" || res.reason === "no-key")) {
          window.__bramDescribeUnavailable = true;
        }
        // Allow a retry on a later expand (disabled/no-key/error).
        delete window.__bramDescribeRequested[id];
      }
      done();
    })
    .catch(function () {
      delete window.__bramDescribeRequested[id];
      done();
    });
};
// Slim change-signal tick (issue-214 candidate #5). The latest-tail
// content pipeline is retired: talk-session-changed IS the signal that
// the live session file changed, and its only remaining consumer is the
// projected-turns refetch (coalesced + reference-preserved above), so
// each tick just requests one. Cross-provider ticks are dropped, like
// the old pipeline's provider-mismatch guard: a background provider's
// session write cannot change the active /__turns projection, and
// refetching a multi-MB projection for it is pure waste (2026-07-07
// codex esc wedge: this session's writes were triggering 1.2 s fetches
// of the codex rollout). The function keeps its historical name —
// Main.xmlui calls it on init and on provider changes with a
// getProvider reading the active provider; re-invocation is idempotent
// because subscribeTalkSessionChange unsubscribes the prior handler
// stored under the same key.
var __bramTurnsTickLast = { sid: "", len: -1 };
window.startBramLatestJsonlPush = function (getProvider) {
  window.__bramRefetchProjectedTurns("provider-start");
  return window.subscribeTalkSessionChange(
    "__bramTurnsTickUnsub",
    function (correlationId, atHostMs, payload) {
      var active = "";
      try {
        var v = typeof getProvider === "function" ? getProvider() : "";
        if (typeof v === "string") active = v;
      } catch (e) {}
      if (active && payload && payload.provider && active !== payload.provider) return;
      // Zero-delta suppression — the old cursor pipeline's dedupe
      // without the cursors: watchers fire 2-3 events per session
      // write, and a tick whose session file identity AND byte size
      // are unchanged cannot change the projection. Append-only JSONL
      // makes (sid, len) a safe change key. Ticks without a usable
      // len (len < 0) always refetch.
      var sid = (payload && payload.sid) || "";
      var len = (payload && typeof payload.len === "number") ? payload.len : -1;
      if (sid && len >= 0 && sid === __bramTurnsTickLast.sid && len === __bramTurnsTickLast.len) return;
      __bramTurnsTickLast = { sid: sid, len: len };
      window.__bramRefetchProjectedTurns("tick");
    }
  );
};

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
// Push local commits from the branch the UI last rendered and refetch
// relevant DataSources when the push completes, so branch and pushed
// state refresh without a manual reload.
window.gitPush = function (commitsDs, statusDs, branch, onError) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("git_push", { branch: branch || null })
    .then(function () {
      if (commitsDs && typeof commitsDs.refetch === "function") {
        commitsDs.refetch();
      }
      if (statusDs && typeof statusDs.refetch === "function") {
        statusDs.refetch();
      }
    })
    .catch(function (e) {
      window.logToHost({ kind: "git-push", phase: "err", error: String(e) });
      if (typeof onError === "function") onError(String(e));
    });
};
// issue-90-q-page: Queue tab helpers. The entries array is the component's
// working copy; every mutator returns a NEW array (xs reactivity needs the
// identity change) and schedules a debounced host save to /__queue/save so
// notes survive reloads and restarts. A synchronous sessionStorage mirror
// closes the debounce window on iframe refresh: restore prefers that unsaved
// snapshot, retries the host write, and clears it only after the matching
// payload is acknowledged. Sends ride toTurn — send-gate, send ledger, and
// strand forensics apply like any other pane send.
var __BRAM_QUEUE_RECOVERY_KEY = "bram.agent-message-queue.unsaved";
var __bramQueueSaveTimer = null;
function __bramQueueScheduleSave(entries) {
  var snapshot = entries || [];
  var payload = JSON.stringify({ entries: snapshot });
  try {
    sessionStorage.setItem(__BRAM_QUEUE_RECOVERY_KEY, payload);
  } catch {}
  if (__bramQueueSaveTimer) clearTimeout(__bramQueueSaveTimer);
  __bramQueueSaveTimer = setTimeout(function () {
    __bramQueueSaveTimer = null;
    fetch("/__queue/save", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: payload,
    })
      .then(function (response) {
        if (!response.ok) throw new Error("queue save returned " + response.status);
        // queue-remount-stale-hydration: do NOT clear the snapshot on save.
        // It is the session-scoped source of truth (overwritten on every
        // mutation); clearing it handed remount back to the /__queue
        // DataSource's stale-while-revalidate cache, which reverted adds
        // (item gone) and deletes (item back). The snapshot is cleared only
        // by the browser session ending; the host /__queue is the
        // cross-restart backstop, read when the snapshot is absent.
      })
      .catch(function (e) {
        window.logToHost({ kind: "queue-save", phase: "err", error: String(e) });
      });
  }, 400);
}
window.__bramQueueRestore = function (hostEntries) {
  var fallback = Array.isArray(hostEntries) ? hostEntries : [];
  var raw = "";
  try {
    raw = sessionStorage.getItem(__BRAM_QUEUE_RECOVERY_KEY) || "";
  } catch {}
  if (!raw) return fallback;
  try {
    var recovered = JSON.parse(raw);
    if (!recovered || !Array.isArray(recovered.entries)) throw new Error("invalid entries");
    // queue-remount-stale-hydration: the snapshot is now persistent, so
    // reschedule the host write only when it is genuinely ahead of the
    // host — otherwise every tab switch would emit a redundant save.
    if (JSON.stringify(recovered.entries) !== JSON.stringify(fallback)) {
      __bramQueueScheduleSave(recovered.entries);
    }
    return recovered.entries;
  } catch (e) {
    try { sessionStorage.removeItem(__BRAM_QUEUE_RECOVERY_KEY); } catch {}
    window.logToHost({ kind: "queue-restore", phase: "err", error: String(e) });
    return fallback;
  }
};
window.__bramQueueUpdate = function (entries, idx, text) {
  var next = (entries || []).slice();
  if (!next[idx]) return entries;
  next[idx] = Object.assign({}, next[idx], {
    text: String(text == null ? "" : text),
    updatedAtMs: Date.now(),
  });
  // queue-mutation-trace: length only, never content (queue prose is
  // user-authored and can carry secrets — same discipline as the describe
  // redaction and the send-forensics previews).
  window.__bramIframeTrace("queue", { op: "update", id: next[idx].id || "", chars: String(next[idx].text || "").length });
  __bramQueueScheduleSave(next);
  return next;
};
window.__bramQueueSendMode = function (entry) {
  return entry && entry.sendMode === "iterate" ? "iterate" : "message";
};
window.__bramQueueSetSendMode = function (entries, idx, sendMode, worklistItems) {
  var next = (entries || []).slice();
  if (!next[idx]) return entries;
  var mode = sendMode === "iterate" ? "iterate" : "message";
  var targetItemId = String(next[idx].targetItemId || "");
  var items = worklistItems || [];
  if (mode === "iterate" && !items.some(function (item) { return item.id === targetItemId; })) {
    targetItemId = items.length ? String(items[0].id || "") : "";
  }
  next[idx] = Object.assign({}, next[idx], {
    sendMode: mode,
    targetItemId: targetItemId,
    updatedAtMs: Date.now(),
  });
  __bramQueueScheduleSave(next);
  return next;
};
window.__bramQueueSetTargetItem = function (entries, idx, targetItemId) {
  var next = (entries || []).slice();
  if (!next[idx]) return entries;
  next[idx] = Object.assign({}, next[idx], {
    targetItemId: String(targetItemId || ""),
    updatedAtMs: Date.now(),
  });
  __bramQueueScheduleSave(next);
  return next;
};
window.__bramQueueAdd = function (entries) {
  var next = (entries || []).slice();
  var now = Date.now();
  next.push({
    id: "q-" + now + "-" + Math.floor(Math.random() * 1e6),
    text: "",
    sendMode: "message",
    targetItemId: "",
    updatedAtMs: now,
  });
  window.__bramIframeTrace("queue", { op: "add", id: next[next.length - 1].id, chars: 0 });
  __bramQueueScheduleSave(next);
  return next;
};
// suppressTrace: set by __bramQueueSend so a send logs op=send, not a
// second op=delete for the same removal. The Delete button calls with two
// args, so a user delete always traces.
window.__bramQueueRemove = function (entries, idx, suppressTrace) {
  var next = (entries || []).slice();
  var removed = next[idx];
  if (!suppressTrace) {
    window.__bramIframeTrace("queue", { op: "delete", id: (removed && removed.id) || "", chars: String((removed && removed.text) || "").length });
  }
  next.splice(idx, 1);
  __bramQueueScheduleSave(next);
  return next;
};
// __bramQueueReorder: persist a drag-reordered entries array. newOrder comes
// from DndItems' onReorder (the same entry objects, reordered), so we adopt it
// as the new order and save it the same way every other queue mutation does.
window.__bramQueueReorder = function (entries, newOrder) {
  var next = Array.isArray(newOrder) ? newOrder.slice() : (entries || []).slice();
  // queue-mutation-trace: op=reorder, count only — never content (queue prose
  // is user-authored, kept secret-safe like the other queue ops).
  window.__bramIframeTrace("queue", { op: "reorder", count: next.length });
  __bramQueueScheduleSave(next);
  return next;
};
window.__bramQueueCanSendWhenReady = function (entry, ready, worklistItems) {
  if (!ready) return false;
  if (!entry || !String(entry.text || "").trim()) return false;
  if (window.__bramQueueSendMode(entry) !== "iterate") return true;
  var targetItemId = String(entry.targetItemId || "");
  return (worklistItems || []).some(function (item) { return item.id === targetItemId; });
};
window.__bramQueueCanSend = function (entry, status, menu, worklistItems) {
  return window.__bramQueueCanSendWhenReady(
    entry,
    window.__bramQueueReady(status, menu),
    worklistItems
  );
};
window.__bramQueueSend = function (entries, idx, worklistItems) {
  var entry = (entries || [])[idx];
  var text = entry && String(entry.text || "").trim();
  if (!text) return entries;
  var mode = window.__bramQueueSendMode(entry);
  if (mode === "iterate") {
    var targetItemId = String(entry.targetItemId || "");
    var items = worklistItems || [];
    if (!items.some(function (item) { return item.id === targetItemId; })) return entries;
    window.sendIterateWithFeedbackDraft(items, targetItemId, text);
  } else {
    toTurn(text);
  }
  window.__bramIframeTrace("queue", { op: "send", id: entry.id || "", mode: mode, chars: text.length });
  return window.__bramQueueRemove(entries, idx, true);
};
// Ready = no open turn (agent-status not "working") and no pending menu.
// Advisory dimming only — the host send-gate remains the enforcement layer
// for a menu racing in at click time.
window.__bramQueueReady = function (status, menu) {
  var working = !!(status && status.state === "working");
  return !working && !menu;
};
window.__bramQueueReadyLabel = function (status, menu) {
  if (menu) return "menu pending — hold";
  if (status && status.state === "working") return "agent working — hold";
  return "ready to send";
};
window.__bramQueueEditedLabel = function (updatedAtMs) {
  var ms = Number(updatedAtMs) || 0;
  if (!ms) return "Last edited —";
  var edited = new Date(ms);
  if (isNaN(edited.getTime())) return "Last edited —";
  return "Last edited " + edited.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
};

// issues-tab-close-via-invoke: manual Close-issue for the Issues tab.
// Rides the issue_close_manual Tauri invoke, NOT an HTTP route — invokes
// are reachable only from Bram's same-origin agent pane (loopback curl and
// the C1-isolated target pane cannot call them), so the H5 close-authority
// contract holds: a clicked button, never an agent channel.
window.__bramCloseIssue = function (number, comment, onDone, onError) {
  var invoke = getTauriInvoke();
  if (!invoke) return;
  invoke("issue_close_manual", { number: number, comment: comment || "" })
    .then(function () {
      if (typeof onDone === "function") onDone();
    })
    .catch(function (e) {
      window.logToHost({ kind: "issue-close-manual", phase: "err", error: String(e) });
      if (typeof onError === "function") onError(String(e));
    });
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
    // Clear on read: the dim is meant to signal "reload Bram to see
    // the new title". A fresh iframe boot means the dim's job is done.
    // Sessions renamed later in this iframe lifetime stay dimmed via
    // the in-memory append in Sessions.xmlui's onSuccess handler.
    localStorage.removeItem("session-pending-renames");
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
// Route external anchors through openExternal and local-file anchors through
// Bram's in-pane preview modal instead of letting the Tauri WebView navigate
// to dead routes. Capture phase so we run before XMLUI's Markdown-internal
// click handlers.
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
  var linkText = (a.textContent || "").trim().slice(0, 120);
  try {
    window.__bramIframeTrace("local-link-click", {
      stage: "anchor",
      href: href,
      text: linkText,
      tagName: String((e.target && e.target.tagName) || ""),
    });
  } catch (traceErr) {}
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
      return;
    }
  }
  var localRequest = window.__bramLocalLinkRequestFromHref(href);
  if (localRequest && !localRequest.skip) {
    e.preventDefault();
    e.stopPropagation();
    try {
      window.__bramIframeTrace("local-link-click", {
        stage: "intercept",
        href: href,
        path: localRequest.path || "",
        line: localRequest.line || null,
      });
    } catch (traceErr2) {}
    window.__bramOpenLocalLinkPreview(localRequest);
    return;
  }
  if (localRequest && localRequest.skip) {
    try {
      window.__bramIframeTrace("local-link-click", {
        stage: "skip",
        href: href,
        reason: localRequest.reason || "",
        raw: localRequest.raw || "",
      });
    } catch (traceErr3) {}
  }
  if (/^https?:/i.test(href)) {
    e.preventDefault();
    e.stopPropagation();
    window.openExternal(href);
    return;
  }
}, true);
// Click-driven; scan the DOM per call.
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

// Inspector trace tap (#181). When enabled via the Settings-tab switch
// (traces.inspectorTap in .bram.json), forwards new entries from the
// XMLUI Inspector's window._xsLogs into bram-trace.log as
// [iframe] subkind=inspector-event so they interleave with host traces
// live. Polls at 200 ms with a per-tick cap; overflow emits
// subkind=inspector-overflow. Every field passes through
// __bramTraceSafeValue before IPC; selectivity (filter by category,
// drop per-keystroke noise, etc.) remains a follow-up.
var __inspectorTap = {
  intervalId: null,
  highWater: 0,
  perTickCap: 50,
};
function __inspectorTrace(subkind, fields) {
  try {
    if (typeof window.logToHost !== "function") return;
    var payload = {
      kind: "iframe-trace",
      subkind: subkind,
      at: new Date().toISOString(),
    };
    if (fields && typeof fields === "object") {
      for (var k in fields) {
        if (Object.prototype.hasOwnProperty.call(fields, k)) {
          payload[k] = window.__bramSensitiveTraceKey(k)
            ? "[REDACTED]"
            : window.__bramTraceSafeValue(fields[k], 0);
        }
      }
    }
    window.logToHost(payload);
  } catch (e) {}
}
function __inspectorTapTick() {
  try {
    var logs = window._xsLogs;
    if (!logs || typeof logs.length !== "number") return;
    var total = logs.length;
    if (total <= __inspectorTap.highWater) return;
    var available = total - __inspectorTap.highWater;
    var toSend = Math.min(available, __inspectorTap.perTickCap);
    var t0 = (typeof performance !== "undefined" && performance.now) ? performance.now() : Date.now();
    for (var i = 0; i < toSend; i++) {
      __inspectorTrace("inspector-event", {
        entry: logs[__inspectorTap.highWater + i],
      });
    }
    if (available > toSend) {
      __inspectorTrace("inspector-overflow", {
        dropped: available - toSend,
        totalSeen: total,
      });
      __inspectorTap.highWater = total;
    } else {
      __inspectorTap.highWater += toSend;
    }
    var t1 = (typeof performance !== "undefined" && performance.now) ? performance.now() : Date.now();
    __inspectorTrace("inspector-tap-tick", {
      batch: toSend,
      available: available,
      ms: Math.round((t1 - t0) * 10) / 10,
    });
  } catch (e) {}
}
function __startInspectorTap() {
  if (__inspectorTap.intervalId !== null) return;
  try {
    var logs = window._xsLogs;
    __inspectorTap.highWater =
      logs && typeof logs.length === "number" ? logs.length : 0;
  } catch (e) {
    __inspectorTap.highWater = 0;
  }
  __inspectorTap.intervalId = setInterval(__inspectorTapTick, 200);
}
function __stopInspectorTap() {
  if (__inspectorTap.intervalId === null) return;
  clearInterval(__inspectorTap.intervalId);
  __inspectorTap.intervalId = null;
}
function __applyInspectorTapSetting(enabled) {
  if (enabled) __startInspectorTap();
  else __stopInspectorTap();
}
function __loadInspectorTapSetting() {
  if (typeof window.fetch !== "function") return;
  window
    .fetch("/__settings", { cache: "no-store" })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (s) {
      var enabled = !!(s && s.traces && s.traces.inspectorTap);
      __applyInspectorTapSetting(enabled);
    })
    .catch(function () {});
}
__loadInspectorTapSetting();
try {
  window.subscribeTauriEvent(
    "__bramInspectorTapSettingsUnsub",
    "settings-changed",
    function () { __loadInspectorTapSetting(); }
  );
} catch (e) {}

// Adjustable root font-size for the XMLUI surface (mirrors the terminal-side
// pattern in app/main.js). Buttons in AppHeader call setAppFontSize /
// getAppFontSize. The right pane and the agent tools drawer share origin
// and localStorage; a BroadcastChannel keeps their runtime sizes in lockstep.
(function () {
  var APP_FONT_KEY = "bram.app.fontSize";
  var LEGACY_APP_FONT_KEY = "xmlui-desktop.app.fontSize";
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
      var raw = parseInt(
        localStorage.getItem(APP_FONT_KEY) ||
          localStorage.getItem(LEGACY_APP_FONT_KEY) ||
          "",
        10
      );
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
