// Pane-side dictation tests (judell/bram#417).
// Run: node scripts/tests/dictation-pane.test.js
// Loads the REAL app/__shell/dictation.js and app/__shell/helpers.js under a
// stub window, then drives the helper functions the pane's editors call
// (__bramVoiceShowLive, __bramVoiceFinalizeInto, __bramVoiceStopOnSend) with
// the messages main.js sends. It covers the wiring between helpers.js and
// BramDictation.createField; the field logic itself is in
// dictation-logic.test.js. No microphone, no XMLUI, no Bram window.
//
// The stub answers anything helpers.js touches at load with an inert value.
// If a later helpers.js needs something real at load, this fails at the
// load step and names the line; add that one thing to `real` below.
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert");
const shell = path.join(__dirname, "../../app/__shell");

const inert = () =>
  new Proxy(function () {}, {
    get: (t, k) => {
      if (k === Symbol.toPrimitive) return () => "";
      if (k === "then") return undefined;
      if (k === Symbol.iterator) return function* () {};
      if (k === "length") return 0;
      return inert();
    },
    apply: () => inert(),
    construct: () => inert(),
    has: () => true,
  });

const listeners = {};
const posted = []; // what the pane posts to the shell
const real = {
  console, setTimeout, clearTimeout, setInterval: () => 0, clearInterval: () => {},
  Date, JSON, Math, Object, Array, String, Number, Boolean, RegExp, Error, Promise, Map, Set, WeakMap, Symbol,
  parseInt, parseFloat, isNaN, isFinite, encodeURIComponent, decodeURIComponent, undefined, NaN, Infinity,
  fetch: () => Promise.resolve({ ok: false, status: 0, json: async () => ({}), text: async () => "" }),
  CustomEvent: function (type, init) {
    this.type = type;
    this.detail = init && init.detail;
  },
  addEventListener: (type, fn) => (listeners[type] = listeners[type] || []).push(fn),
  removeEventListener: (type, fn) => {
    listeners[type] = (listeners[type] || []).filter((f) => f !== fn);
  },
  dispatchEvent: (ev) => {
    (listeners[ev.type] || []).slice().forEach((fn) => fn(ev));
    return true;
  },
  parent: { postMessage: (m) => posted.push(m) },
  document: new Proxy(
    { activeElement: null, addEventListener: () => {}, currentScript: null },
    { get: (t, k) => (k in t ? t[k] : inert()) },
  ),
};
const sandbox = new Proxy(real, {
  has: () => true,
  get: (t, k) => {
    if (k === "window" || k === "self" || k === "globalThis" || k === "top") return sandbox;
    if (k in t) return t[k];
    if (typeof k === "symbol") return undefined;
    return inert();
  },
  set: (t, k, v) => {
    t[k] = v;
    return true;
  },
});
vm.createContext(sandbox);
for (const f of ["dictation.js", "helpers.js"]) {
  vm.runInContext(fs.readFileSync(path.join(shell, f), "utf8"), sandbox, { filename: f });
}

const traces = [];
real.__bramIframeTrace = (kind, info) => {
  if (kind === "voice-trace") traces.push(info);
};

// An XMLUI box as page script sees it: a snapshot of the value at the
// moment markup made the call, with working methods. The pane's markup
// passes a fresh one on every call, so each call here makes a new one.
const field = (initial) => {
  const s = { text: initial };
  s.box = () => ({
    value: s.text,
    setValue: (v) => {
      s.text = v;
    },
    focus() {},
    setSelectionRange() {},
  });
  return s;
};
// main.js -> pane: a partial, then the editor's reaction to it.
const show = (f, target, requestId, parts, provisional, openSeq) => {
  real.dispatchEvent({
    type: "message",
    data: {
      type: "voice-into-partial",
      target,
      requestId,
      committed: parts.map((p) => p.text).join(" "),
      provisional,
      parts,
      openSeq,
    },
  });
  real.__bramVoiceShowLive(f.box(), real.__bramVoicePartial, target);
};
// main.js -> pane: the delivered transcript, then the editor's final write.
const finalize = (f, target, requestId, transcript) => {
  real.dispatchEvent({ type: "message", data: { type: "voice-into-result", target, requestId, transcript } });
  return real.__bramVoiceFinalizeInto(f.box(), target, transcript);
};

const tests = [];
const test = (name, fn) => tests.push({ name, fn });

test("message box: an edit mid-dictation, stopped with the mic", () => {
  const t = "message-agent";
  const r = "req-A";
  const f = field("Draft:");
  show(f, t, r, [], "Hello", 0);
  assert.strictEqual(f.text, "Draft: Hello");
  show(f, t, r, [{ seq: 0, text: "Hello there." }], "", 1);
  assert.strictEqual(f.text, "Draft: Hello there.");
  f.text += " ok";
  show(f, t, r, [{ seq: 0, text: "Hello there." }], "And", 1);
  assert.strictEqual(f.text, "Draft: Hello there. ok And");
  // Objects from the vm context have another realm's prototypes; compare as JSON.
  assert.strictEqual(
    JSON.stringify(posted.filter((m) => m.kind === "voice-edit-boundary" && m.requestId === r)),
    JSON.stringify([{ type: "right-pane", kind: "voice-edit-boundary", requestId: r, fromSeq: 1 }]),
  );
  show(f, t, r, [{ seq: 0, text: "Hello there." }, { seq: 1, text: "And more." }], "", 2);
  assert.strictEqual(f.text, "Draft: Hello there. ok And more.");
  const out = finalize(f, t, r, "Hello there. And more.");
  assert.strictEqual(out, "Draft: Hello there. ok And more.");
  assert.strictEqual(f.text, "Draft: Hello there. ok And more.");
  const fin = traces.filter((x) => x.stage === "finalize-into").pop();
  assert.strictEqual(fin.edited, true);
  assert.strictEqual(fin.target, t);
});

test("feedback box: live text, then the final transcript exactly once", () => {
  const t = "feedback:item-1";
  const r = "req-B";
  const f = field("");
  show(f, t, r, [], "Looks", 0);
  show(f, t, r, [{ seq: 0, text: "Looks good." }], "", 1);
  assert.strictEqual(f.text, "Looks good.");
  const out = finalize(f, t, r, "Looks good to me.");
  assert.strictEqual(out, "Looks good to me.");
  assert.strictEqual(f.text, "Looks good to me.");
});

test("two boxes keep separate records", () => {
  const a = field("A:");
  const b = field("B:");
  show(a, "feedback:a", "req-a", [{ seq: 0, text: "One." }], "", 1);
  show(b, "feedback:b", "req-b", [{ seq: 0, text: "Two." }], "", 1);
  assert.strictEqual(finalize(a, "feedback:a", "req-a", "One."), "A: One.");
  assert.strictEqual(finalize(b, "feedback:b", "req-b", "Two."), "B: Two.");
});

test("a send while recording: nothing is written afterwards", () => {
  const t = "message-agent";
  const r = "req-C";
  const f = field("");
  show(f, t, r, [], "Sending", 0);
  assert.strictEqual(f.text, "Sending");
  real._voiceSession = r;
  real._voiceSessionTarget = t;
  real.__bramVoiceStopOnSend(t);
  f.text = "";
  show(f, t, r, [{ seq: 0, text: "Sending now." }], "", 1);
  assert.strictEqual(f.text, "");
  assert.strictEqual(finalize(f, t, r, "Sending now."), false);
  assert.strictEqual(f.text, "");
  assert.ok(traces.some((x) => x.stage === "finalize-into-stopped" && x.target === t));
});

test("a stale partial after delivery writes nothing", () => {
  const t = "feedback:item-3";
  const r = "req-E";
  const f = field("");
  show(f, t, r, [], "Late", 0);
  const stale = real.__bramVoicePartial;
  finalize(f, t, r, "Late words.");
  assert.strictEqual(f.text, "Late words.");
  real.__bramVoiceShowLive(f.box(), stale, t);
  assert.strictEqual(f.text, "Late words.");
});

test("no live record (the reserve full pass): plain append", () => {
  const f = field("Note:");
  const out = real.__bramVoiceFinalizeInto(f.box(), "feedback:item-2", "Appended.");
  assert.strictEqual(out, "Note: Appended.");
  assert.strictEqual(f.text, "Note: Appended.");
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
    console.log("ok   " + name);
  } catch (e) {
    failed++;
    console.log("FAIL " + name + "\n     " + String(e && e.message).split("\n").join("\n     "));
  }
}
console.log(failed ? failed + " of " + tests.length + " failed" : "all " + tests.length + " passed");
// helpers.js leaves timers running; don't wait on them.
process.exit(failed ? 1 : 0);
