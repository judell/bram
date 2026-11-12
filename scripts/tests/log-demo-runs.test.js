// Log folds runs of demo-instance lines (log-collapses-demo-runs).
// Run: node scripts/tests/log-demo-runs.test.js
// Loads the REAL app/__shell/helpers.js under a stub window and feeds
// __bramStoryTell trace lines.
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


const tell = real.__bramStoryTell;
const line = real.__bramStoryScenarioLine;
const hhmm = (ms) => { const d = new Date(ms); return ("0" + d.getHours()).slice(-2) + ":" + ("0" + d.getMinutes()).slice(-2); };
const T0 = Date.parse("2026-11-11T14:48:00");
const demo = (min, text) => ({ ts: new Date(T0 + min * 60000).toISOString(), cat: "demo-instance", text });
const sel = (min) => ({
  ts: new Date(T0 + min * 60000).toISOString(), cat: "iframe",
  text: 'subkind=w2-selection-restore {"op":"restore","ids":["alpha"]}',
});
const rows = (lines) => tell({ lines }).filter((r) => r.kind === "demo" || r.kind === "selection");

// 1. start, stop, restart, start folds into one row.
const run = [
  demo(0, "op=launch pid=100 scenario=held-mixed"),
  demo(1, "op=stop pid=100 scenario=held-mixed"),
  demo(2, "op=stop pid=101 scenario=dependency"),
  demo(2.05, "op=launch pid=102 scenario=dependency"),
  demo(4, "op=launch pid=103 scenario=dependency"),
];
const one = rows(run);
assert.strictEqual(one.length, 1);
console.log(one[0].text);
assert.strictEqual(one[0].text, "The demo instance started 3 times and stopped 2 times, 14:48\u201314:52.");
assert.strictEqual(one[0].at, hhmm(T0 + 4 * 60000));
assert.strictEqual(one[0].t, T0 + 4 * 60000);
assert.strictEqual(one[0].id, "demo:" + (T0 + 4 * 60000));
assert.strictEqual(one[0].runs.length, 4);
assert.ok(one[0].scenario);
const detail = line(one[0]);
console.log(detail);
assert.match(detail, /^Runs: 14:48 started \(PID 100\) \u00b7 14:49 stopped \(PID 100\) \u00b7 14:50 restarted \(PID 101 \u2192 102\) \u00b7 14:52 started \(PID 103\)\. Scenarios: held-mixed, dependency$/);

// 2. only starts: just that, with "once" where needed.
const starts = rows([demo(0, "op=launch pid=1 scenario=a"), demo(1, "op=launch pid=2 scenario=a"), demo(2, "op=launch pid=3 scenario=a")]);
console.log(starts[0].text);
assert.strictEqual(starts[0].text, "A demo instance started 3 times, 14:48\u201314:50.");
const mixed = rows([demo(0, "op=launch pid=1 scenario=a"), demo(1, "op=launch pid=2 scenario=a"), demo(2, "op=launch pid=3 scenario=a"), demo(3, "op=stop pid=3 scenario=a")]);
console.log(mixed[0].text);
assert.strictEqual(mixed[0].text, "The demo instance started 3 times and stopped once, 14:48\u201314:51.");

// 3. A shown entry in the middle splits the run in two (newest first).
const split = rows([
  demo(0, "op=launch pid=1 scenario=a"), demo(1, "op=stop pid=1 scenario=a"),
  sel(2),
  demo(3, "op=launch pid=2 scenario=a"), demo(4, "op=stop pid=2 scenario=a"),
]);
assert.strictEqual(split.map((r) => r.kind).join(","), "demo,selection,demo");
console.log(split.map((r) => r.text).join("\n"));
assert.strictEqual(split[0].text, "The demo instance started once and stopped once, 14:51\u201314:52.");
assert.strictEqual(split[2].text, "The demo instance started once and stopped once, 14:48\u201314:49.");

// 4. A single demo line is unchanged.
const single = rows([demo(0, "op=launch pid=7 scenario=held-mixed")]);
assert.strictEqual(single.length, 1);
assert.strictEqual(single[0].text, "A demo instance started (PID 7).");
assert.strictEqual(single[0].runs, undefined);
assert.strictEqual(line(single[0]), "Scenario: held-mixed (a saved demo board)");

console.log("log-demo-runs: all assertions passed");
