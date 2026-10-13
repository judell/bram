// Selection prediction in the gate bar (issue-425-retire-replay, Codex review).
// Run: node scripts/tests/selection-prediction.test.js
// Loads the REAL app/__shell/helpers.js under a stub window (the loader of
// dictation-pane.test.js) and checks the two pane rules the review asked for:
//   1. the request key changes when membership turns fresh under an unchanged
//      state key, so "not ready" can't stay hidden after it becomes ready;
//   2. an answer renders only when fresh, for this selection, and computed
//      under the board's current state key.
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

const real = {
  console, setTimeout, clearTimeout, setInterval: () => 0, clearInterval: () => {},
  Date, JSON, Math, Object, Array, String, Number, Boolean, RegExp, Error, Promise, Map, Set, WeakMap, Symbol,
  parseInt, parseFloat, isNaN, isFinite, encodeURIComponent, decodeURIComponent, undefined, NaN, Infinity,
  fetch: () => Promise.resolve({ ok: false, status: 0, json: async () => ({}), text: async () => "" }),
  addEventListener: () => {},
  removeEventListener: () => {},
  dispatchEvent: () => true,
  parent: { postMessage: () => {} },
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
vm.runInContext(fs.readFileSync(path.join(shell, "helpers.js"), "utf8"), sandbox, {
  filename: "helpers.js",
});

const key = real.__bramSelectionPredictionKey;
const line = real.__bramSelectionTakeLine;
const sel = ["top", "bottom"];
const board = (k, stale) => ({ membershipStateKey: k, membershipStale: stale, items: [] });
const answer = (over) =>
  Object.assign(
    {
      ready: true, stale: false, mode: "whole-file", added: 3, removed: 3,
      unowned: [{ path: "f.txt", added: 1, removed: 1 }],
      ids: ["top", "bottom"], stateKey: "K",
    },
    over || {},
  );

// 1. Readiness changes the request even when the state key doesn't.
assert.notStrictEqual(key(board("K", true), sel), key(board("K", false), sel));
assert.strictEqual(key(board("", false), sel), "", "no key until membership has one");
assert.strictEqual(key(board("K", false), ["top"]), "", "one item: the row says it");

// 2. Only a fresh answer, for this selection, under the board's key.
const ok = line(answer(), sel, board("K", false));
assert.match(ok, /\+3 −3/);
assert.match(ok, /no item wrote/);
assert.strictEqual(line(answer(), ["bottom", "top"], board("K", false)), ok, "order-insensitive");
assert.strictEqual(line(answer({ stale: true }), sel, board("K", false)), "", "stale answer");
assert.strictEqual(line(answer({ stateKey: "K-old" }), sel, board("K", false)), "", "other state");
assert.strictEqual(line(answer({ ids: ["top", "other"] }), sel, board("K", false)), "", "other selection");
assert.strictEqual(line(answer(), sel, board("", false)), "", "board has no key");
assert.strictEqual(line(answer({ ready: false }), sel, board("K", false)), "", "not ready");

console.log("selection-prediction: all assertions passed");
