// context-growth-explains-jumps: what each Token growth point says.
// Run: node scripts/tests/context-growth-explain.test.js
// Loads the REAL app/__shell/helpers.js under the stub window of
// selection-prediction.test.js and checks the explanation builder on small
// calls + blocks fixtures: Claude's numbered blocks, Codex's call-0 blocks
// joined by time, a compaction drop, a mismatch, the user's words, escaping.
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

// Values built inside the vm carry its realm's prototypes; compare as JSON.
const eq = (a, b, m) => assert.deepStrictEqual(JSON.parse(JSON.stringify(a)), b, m || "unexpected value");
const explain = real.__bramGrowthExplain;
const chart = real.__bramContextGrowthChart;
const iso = (ms) => new Date(ms).toISOString();
const T0 = Date.parse("2026-11-25T19:46:00Z");
const call = (k, context) => ({ provider: "claude", session: "s", thread: "main", atMs: T0 + k * 10000, context });

// 1. Claude: blocks carry `call`, the chart's 0-based index.
{
  const calls = [call(0, 1000), call(1, 3000), call(2, 3100)];
  const stack = { supported: true, session: "claude:s", provider: "claude", measured: { compactions: 0 }, blocks: [
    { kind: "harness", label: "System prompt", tokens: 960, call: 0, ts: iso(T0 + 50000) },
    { kind: "user", label: "Read and follow this Bram turn: @x", tokens: 40, call: 0, ts: iso(T0 + 50000) },
    { kind: "tool-call", label: "Bash: Search project history", tokens: 50, call: 1, ts: iso(T0) },
    { kind: "tool-result", label: "Bash result", tokens: 1950, call: 1, ts: iso(T0 + 2000) },
    { kind: "agent", label: "Done.", tokens: 100, call: 2, ts: iso(T0 + 10000) },
    { kind: "agent", label: "pending", tokens: 70, call: 3, ts: iso(T0 + 20000) },
  ] };
  const turns = [{ atMs: T0 - 1000, text: "Build <it> & \"go\"" }];
  const e = explain(calls, stack, turns);
  assert.strictEqual(e[0].first, true);
  eq(e[0].blocks.map((b) => b.tokens), [960, 40], "preamble at call 0, ts ignored");
  assert.strictEqual(e[1].delta, 2000);
  eq(e[1].blocks.map((b) => b.label), ["Bash: Search project history → result", "Bash: Search project history"]);
  assert.strictEqual(e[0].startedHere, true);
  assert.strictEqual(e[1].startedHere, false, "a mid-turn jump names the turn's message, not as its cause");
  assert.strictEqual(e[1].message, "Build <it> & \"go\"");
  assert.ok(e.every((x) => !x.blocks.some((b) => b.label === "pending")), "a block for a call not made yet is left out");
  const opt = chart({ calls }, "claude:s", stack, turns);
  assert.strictEqual(typeof opt.tooltip.formatter, "function");
  const tip = opt.tooltip.formatter([{ data: opt.series[0].data[1] }]);
  assert.ok(tip.includes("+2.0k tokens"), tip);
  assert.ok(tip.includes("Build &lt;it&gt; &amp; &quot;go&quot;"), "the user's text is escaped");
  assert.ok(tip.includes("While working on your message"), tip);
  assert.ok(!tip.includes("<it>"));
}

// 2. Codex: every block is call 0; join by time. Codex writes a call's token
// reading after its output and that output's tool results (live rollouts
// 01a213ca, 01a213df: result at 17:39:30.448, reading at 17:39:30.448), so
// both enter at the next call.
{
  const c = (k, context) => Object.assign(call(k, context), { provider: "codex" });
  const calls = [c(0, 500), c(1, 1700), c(2, 1800)];
  const stack = { supported: true, session: "codex:s", provider: "codex", measured: { compactions: 0 }, blocks: [
    { kind: "harness", label: "Base instructions", tokens: 450, call: 0, ts: iso(T0 - 5000) },
    { kind: "user", label: "hi", tokens: 50, call: 0, ts: iso(T0 - 4000) },
    { kind: "tool-call", label: "exec_command: ls", tokens: 30, call: 0, ts: iso(T0 - 100) },
    { kind: "tool-result", label: "exec_command result", tokens: 1170, call: 0, ts: iso(T0 - 1) },
    { kind: "agent", label: "Here.", tokens: 100, call: 0, ts: iso(T0 + 9900) },
  ] };
  const e = explain(calls.map((x) => Object.assign({}, x)), stack, []);
  eq(e[0].blocks.map((b) => b.tokens), [450, 50]);
  eq(e[1].blocks.map((b) => b.tokens), [1170, 30]);
  eq(e[2].blocks.map((b) => b.tokens), [100]);
  assert.strictEqual(e[1].message, null, "no turns, no message line");
}

// 3. Compaction: the drop says so; points before it can't list blocks; a
// call whose blocks don't add up says that instead of naming wrong ones.
{
  const calls = [call(0, 1000), call(1, 50000), call(2, 9000), call(3, 9500), call(4, 30000)];
  const stack = { supported: true, session: "claude:s", provider: "claude", measured: { compactions: 1 }, blocks: [
    { kind: "harness", label: "System prompt", tokens: 1000, call: 0 },
    { kind: "compaction-summary", label: "Compaction summary", tokens: 8000, call: 2 },
    { kind: "tool-result", label: "Read result", tokens: 500, call: 3 },
    { kind: "tool-result", label: "Read result", tokens: 900, call: 4 },
  ] };
  const e = explain(calls, stack, []);
  assert.strictEqual(e[1].note, "before-compaction");
  assert.strictEqual(e[2].note, "compacted");
  eq(e[3].blocks.map((b) => b.tokens), [500]);
  assert.strictEqual(e[4].note, "unmatched", "900 tokens can't explain +20.5k");
  const opt = chart({ calls }, "claude:s", stack, []);
  assert.ok(opt.tooltip.formatter([{ data: opt.series[0].data[2] }]).includes("Compacted"));
}

// 3b. The kind filter governs the chart. Every kind is shown by default
// (null), stacked in the chips' order under a faint whole-context line;
// each kind's line starts at the last compaction; the tip names the shown
// kinds' blocks, or reads as unfiltered when every kind is shown.
{
  const calls = [call(0, 1000), call(1, 50000), call(2, 9000), call(3, 9500), call(4, 10400)];
  const stack = { supported: true, session: "claude:s", provider: "claude", measured: { compactions: 1 },
    byKind: [
      { kind: "harness", blocks: 1, tokens: 1000 },
      { kind: "compaction-summary", blocks: 1, tokens: 8000 },
      { kind: "tool-result", blocks: 2, tokens: 1100 },
      { kind: "agent", blocks: 1, tokens: 300 },
    ],
    blocks: [
      { kind: "harness", label: "System prompt", tokens: 1000, call: 0 },
      { kind: "compaction-summary", label: "Compaction summary", tokens: 8000, call: 2 },
      { kind: "tool-result", label: "Read result", tokens: 500, call: 3 },
      { kind: "tool-result", label: "Bash result", tokens: 600, call: 4 },
      { kind: "agent", label: "ok", tokens: 300, call: 4 },
    ] };
  // Default: all four kinds stacked, largest first, and the plain tip.
  const all = chart({ calls }, "claude:s", stack, [], null);
  const title = (k) => real.__bramStackKindTitle(k, "claude");
  eq(all.series.map((s) => s.name), ["All context", title("compaction-summary"), title("tool-result"), title("harness"), title("agent")]);
  eq(all.series.length, 5);
  assert.strictEqual(all.legend.show, false, "the chips are the legend");
  const tipAll = all.tooltip.formatter(all.series.map((s) => ({ data: s.data[4] })));
  assert.ok(tipAll.includes("+900 tokens</b>, 10k now"), tipAll);
  // One kind.
  const opt = chart({ calls }, "claude:s", stack, [], ["tool-result"]);
  eq(opt.series.length, 2);
  eq(opt.series[1].data.map((d) => d.value[1]), [null, null, 0, 500, 1100], "unknown before the compaction at call 2");
  const tip = opt.tooltip.formatter(opt.series.map((s) => ({ data: s.data[4] })));
  assert.ok(tip.includes("+600") && tip.includes("1.1k of them now"), tip);
  assert.ok(tip.includes("Bash result") && !tip.includes("ok</td>"), tip);
  // Two kinds: a stacked area each, and a tip that adds them up.
  const two = chart({ calls }, "claude:s", stack, [], ["agent", "tool-result"]);
  eq(two.series.length, 3);
  eq(two.series[2].data.map((d) => d.value[1]), [null, null, 0, 0, 300], "chips' order: tool results, then the agent");
  assert.strictEqual(two.series[1].stack, "kinds");
  const tip2 = two.tooltip.formatter(two.series.map((s) => ({ data: s.data[4] })));
  assert.ok(tip2.includes("+900 tokens of") && tip2.includes("1.4k of them now"), tip2);
  // Multiselect: clicks add and remove kinds; none selected (null) shows all.
  const toggle = (cur, k) => JSON.parse(JSON.stringify(real.__bramStackToggleKind(cur, k)));
  eq(toggle(null, "agent"), ["agent"]);
  eq(toggle(["agent"], "tool-result"), ["agent", "tool-result"], "a second kind adds");
  eq(toggle(["agent", "tool-result"], "agent"), ["tool-result"], "a selected kind removes");
  eq(toggle(["agent"], "agent"), null, "the last one removed shows all");
  eq(toggle(["agent"], "all"), null);
  eq(real.__bramStackRows(stack, "order", null).length, 5);
  eq(real.__bramStackRows(stack, "order", ["agent"]).length, 1);
  // The legend: every chip on with no filter; filtered, selected bold, the rest off.
  const legend = (k) => JSON.parse(JSON.stringify(real.__bramStackLegend(stack, k)));
  eq(legend(null).every((c) => c.active && !c.filter), true);
  assert.ok(legend(null)[0].tip.startsWith("**Filter to this category**"));
  const on = legend(["agent"]);
  eq(on.filter((c) => c.filter).map((c) => c.kind), ["agent"]);
  eq(on.filter((c) => c.active).map((c) => c.kind), ["agent"]);
  assert.ok(on.find((c) => c.kind === "agent").tip.startsWith("**Remove from filter**"));
  assert.ok(on.find((c) => c.kind === "harness").tip.startsWith("**Add to filter**"));
  const off = on.find((c) => c.kind === "harness");
  eq(real.__bramStackLegendDecoration(off), "line-through");
  eq(real.__bramStackSwatchFill(off), "transparent");
  eq(real.__bramStackLegendWeight(on.find((c) => c.kind === "agent")), "$fontWeight-bold");
}

// 4. No stack reader (OpenCode, Antigravity): today's plain tooltip.
{
  const calls = [call(0, 1000)];
  const opt = chart({ calls }, "claude:s", { supported: false, session: "claude:s", blocks: [] }, []);
  eq(opt.tooltip, { trigger: "axis" });
  eq(opt.series[0].data, [[calls[0].atMs, 1000]]);
}

// 4b. The About dialog's lines: Claude and Codex their own, any other
// provider the neutral pair (Provider parity).
{
  const about = (p) => JSON.parse(JSON.stringify(real.__bramStackAboutLines({ provider: p })));
  assert.ok(about("claude")[1].startsWith("Sizes are the bytes"));
  assert.ok(about("codex")[0].includes("Codex rollout"));
  for (const p of ["opencode", "antigravity", "someday"]) {
    const lines = about(p);
    eq(lines.length, 2);
    assert.ok(!/Claude|Codex|Anthropic/.test(lines.join(" ")), p);
  }
}

// 5. The turns query splits the stack's session.
eq(Object.assign({}, real.__bramStackTurnsQuery("codex:01a2-x")), { provider: "codex", id: "01a2-x" });
assert.strictEqual(real.__bramStackTurnsQuery(""), null);

console.log("context-growth-explain: ok");
