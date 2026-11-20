#!/usr/bin/env node
// check-bound-helpers.mjs: helpers called from pane bindings read only their
// arguments.
//
// Why: with XMLUI's readScopedRendering on (f5c8d84a), a container re-renders
// only when values its bindings read from XMLUI state change. A helper that
// reads displayed state kept on `window` (or in a helpers.js module variable,
// storage, or the clock) is invisible to that, so its binding can go stale.
// Functions on `window` that reach the outside world (fetch, the PTY, Tauri
// events, timers, the DOM) are necessary; displayed state on `window` is a
// choice. Put that state in a `var.`, a global, or a DataSource/PushSource.
// The rule lives in docs/developing-bram.md §Code organization.
//
// What it does:
// - lists every helper reached from a display binding in app/tools/**/*.xmlui
//   (not on* handlers, subscribe=, initial*, transformResult, methods, events);
// - walks each helper through app/__shell/helpers.js transitively, skipping
//   trace and log callees, which never shape a value;
// - reports reads of window data, outer let/var or mutable const, storage
//   (@storage), the clock (@clock), or the DOM (@dom) not in the allowlist below.
//
// Usage: node scripts/check-bound-helpers.mjs [--list]
//   exit 0: clean; exit 1: unlisted reads (each printed with its binding sites).
//   --list prints every stateful helper with the allowlist reason it matched.
//
// Limits: bindings are found by a double-quoted attribute scan, so values in
// <property> children or text-content {…} aren't scanned. Analysis is static:
// a read reached only through a callback or a computed member isn't seen.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "./vendor/acorn.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const HELPERS = join(ROOT, "app/__shell/helpers.js");
const TOOLS = join(ROOT, "app/tools");

// ---- allowlist -------------------------------------------------------------
// A read that's safe for every helper, with the reason.
const READ_OK = {
  __bramWlAccepted: "set by transformResult from the /__worklist payload the binding also passes",
  __bramWlSeen: "version from that same /__worklist payload",
  __bramPaneAttributionLast: "trace de-dup only",
  __bramGateLastSessionShown: "trace de-dup only",
  lastOddLogged: "trace de-dup only",
  __bramFooterIndexBucketLabels: "constant",
  __bramTipsRegistry: "constant",
  __bramW2Selection: "fallback only when the selection argument is missing; callers pass w2Sel.value",
  __bramStateDirPlanLast: "the last non-empty value of the argument itself",
  lastSearchKey: "memo keyed by the arguments",
  lastSearchOut: "memo keyed by the arguments",
  lastRowsKey: "memo keyed by the arguments",
  lastRowsOut: "memo keyed by the arguments",
  lastView: "memo keyed by the arguments",
  lastViewRaw: "memo keyed by the arguments",
  __bramYouRowsCache: "memo keyed by the arguments",
  __bramTranscriptFindCache: "memo keyed by the arguments",
  __bramProjectedEventCache: "WeakMap memo keyed by the argument's turns",
  __bramHostNotesEventCache: "WeakMap memo keyed by the argument's events",
  __bramCompactionEventCache: "WeakMap memo keyed by the argument's events",
  __bramPendingProse: "the menu-prose bridge; set and cleared by the call from its turns and turn-end arguments",
  __bramTurnContext: "fallback only; bindings pass gateTurnCtx.value (bramSubscribeTurnContext)",
  __bramVisibleRange: "var initializers of mount state (Transcript atBottom, mountReturnState); scroll handlers reassign them",
};
// A read that's safe only for these helpers.
const HELPER_OK = {
  __bramGateCloseItems: { reads: ["__bramW2CloseMap"], why: "KNOWN DEBT: the close map lives on window; gateCloseTick tells XMLUI it changed (f5c8d84a). Declarative fix: a Globals.xs global." },
  __bramGateCloseContradiction: { reads: ["__bramW2CloseMap"], why: "KNOWN DEBT: as __bramGateCloseItems" },
  __bramGateCloseContradictionMessage: { reads: ["__bramW2CloseMap"], why: "KNOWN DEBT: as __bramGateCloseItems" },
  __bramAlertsSnapshot: { reads: ["__bramAlertsSnapshotMap", "@storage"], why: "written only inside this call, from inbox.value and $pathname" },
  __bramAlertsShouldPulse: { reads: ["__bramAlertsSnapshotMap", "@storage"], why: "seen-storage written by __bramAlertsSnapshot; $pathname re-runs it" },
  __bramPickTip: { reads: ["__bramTipSkips", "@storage", "lastValue", "@clock"], why: "footerTipTick is bumped by Next and dismiss" },
  __bramTipLink: { reads: ["__bramTipSkips", "@storage", "lastValue", "@clock"], why: "as __bramPickTip" },
  __bramTipDismissTooltip: { reads: ["@storage"], why: "cosmetic: the dismiss-interval setting, refreshed on the footer's next render" },
  __bramYouNavState: { reads: ["__bramYouRef", "__bramYouNavCache", "__bramYouNavPublishPending", "__bramYouNavSubscribers"], why: "youTick/youStepSeq passed; the stepper publishes through bramSubscribeYouNav" },
  __bramMountFollow: { reads: ["__bramScrollJumpWatch", "__bramSendFollowup", "@clock", "@dom"], why: "var initializer: mount-time state, read once on purpose" },
  __bramStoryMountKey: { reads: ["__bramStoryMounts"], why: "var initializer, once per mount" },
  __bramStorySince: { reads: ["__bramStorySinceCache", "@clock"], why: "memo keyed by mountKey.storyTick" },
  __bramStoryTell: { reads: ["@clock", "__projectedTurnsValue"], why: "story.value passed; the clock only labels ages" },
  __bramPendingIssueCloseList: { reads: ["__bramPendingIssueCloses"], why: "var initializer; handlers assign pendingCloses afterwards" },
  __bramIssueBlockRows: { reads: ["__bramPendingIssueComments", "@clock"], why: "pendingComments.value passed as its tick" },
  __bramIssueCommentHeader: { reads: ["@clock"], why: "cosmetic: 'N min ago', refreshed with the issue" },
  __bramSendLedgerNotice: { reads: ["@clock"], why: "cosmetic: the notice's 2-minute expiry; prefers the payload's nowMs" },
  __bramGateStrandedExitLine: { reads: ["@clock"], why: "gateLockTick is bumped by its Timer" },
  __bramFormatToolResult: { reads: ["@clock"], why: "timing for a trace line only" },
  __bramWorklist2Strip: { reads: ["@clock"], why: "cosmetic: 'started 3h ago' (__bramGreenLitAge), hour granularity" },
  liveIds: { reads: ["@clock"], why: "30-minute backstop; the host clears the turn context when the addressed turn ends" },
  __bramAddressedBlockMessage: { reads: ["@clock"], why: "a declared wait's 'overdue' wording; gateWaitTick is bumped by its Timer while a wait is declared" },
  __bramTipDismissed: { reads: ["@clock", "@storage"], why: "cosmetic: Tips page 'dismissed' label, refreshed on its next render" },
};
// Bindings whose helper is a once-only initializer reading storage or the DOM:
// a `var.` that handlers reassign afterwards.
const INITIALIZER = /^__bram(Restore[A-Z]\w*|\w*Initial\w*|OrientSent|StatusOpenLoad|FilesView|CompactionOpen|GateLastSession|GateLastSessionLine)$/;


// ---- bindings ---------------------------------------------------------------
const SKIP_ATTR = /^(on[A-Z]\w*|subscribe|initial\w*|transformResult|method\.\w+|event\.\w+)$/;
const xmluiFiles = [join(TOOLS, "Main.xmlui")].concat(
  readdirSync(join(TOOLS, "components")).filter((f) => f.endsWith(".xmlui")).sort().map((f) => join(TOOLS, "components", f)));
const sites = new Map(); // helper -> ["File.xmlui:line attr"]
const directReads = []; // window data read straight from a binding
const attrRe = /([\w.:-]+)="([^"]*?window\.[^"]*?)"/gs;
for (const file of xmluiFiles) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(attrRe)) {
    const [, attr, value] = m;
    if (SKIP_ATTR.test(attr)) continue;
    const at = file.split("/").pop() + ":" + (text.slice(0, m.index).split("\n").length) + " " + attr;
    for (const r of value.matchAll(/window\.(\w+)(\s*\()?/g)) {
      const list = sites.get(r[1]) || [];
      list.push(at + (r[2] ? "" : " (data)"));
      sites.set(r[1], list);
    }
  }
}

// ---- helpers.js -------------------------------------------------------------
const src = readFileSync(HELPERS, "utf8");
const ast = parse(src, { ecmaVersion: "latest" });
const kids = (n) => {
  const out = [];
  for (const k in n) {
    if (k === "type" || k === "start" || k === "end") continue;
    const v = n[k];
    if (Array.isArray(v)) v.forEach((c) => c && typeof c.type === "string" && out.push(c));
    else if (v && typeof v.type === "string") out.push(v);
  }
  return out;
};
const walkAll = (root, cb) => { const st = [root]; while (st.length) { const n = st.pop(); cb(n); st.push(...kids(n)); } };
const walkAnc = (root, vis) => {
  const go = (n, anc) => { anc.push(n); if (vis[n.type]) vis[n.type](n, anc); for (const c of kids(n)) go(c, anc); anc.pop(); };
  go(root, []);
};

// Declarations with their lexical scope: a reference resolves to the
// innermost declaration whose scope contains it.
const decls = {};
const add = (name, d) => (decls[name] = decls[name] || []).push(d);
const patNames = (p, out) => {
  if (!p) return;
  if (p.type === "Identifier") out.push(p);
  else if (p.type === "ObjectPattern") p.properties.forEach((q) => patNames(q.value || q.argument, out));
  else if (p.type === "ArrayPattern") p.elements.forEach((q) => patNames(q, out));
  else if (p.type === "AssignmentPattern") patNames(p.left, out);
  else if (p.type === "RestElement") patNames(p.argument, out);
};
const isFnNode = (n) => !!n && /^(FunctionExpression|ArrowFunctionExpression|FunctionDeclaration)$/.test(n.type);
const nearest = (anc, test) => { for (let i = anc.length - 2; i >= 0; i--) if (test(anc[i])) return anc[i]; return anc[0]; };
const fnScope = (anc) => nearest(anc, (x) => isFnNode(x) || x.type === "Program");
const blockScope = (anc) => nearest(anc, (x) => isFnNode(x) || x.type === "Program" || x.type === "BlockStatement"
  || /^For(In|Of)?Statement$/.test(x.type) || x.type === "SwitchStatement");
walkAnc(ast, {
  FunctionDeclaration(n, anc) { add(n.id.name, { scope: blockScope(anc), kind: "function", fn: n }); },
  FunctionExpression(n) { if (n.id) add(n.id.name, { scope: n, kind: "function", fn: n }); },
  VariableDeclaration(n, anc) {
    const scope = n.kind === "var" ? fnScope(anc) : blockScope(anc);
    for (const d of n.declarations) {
      const o = []; patNames(d.id, o);
      const init = d.id.type === "Identifier" ? d.init : null;
      const mutable = n.kind !== "const" || !!(init && /^(ObjectExpression|ArrayExpression|NewExpression)$/.test(init.type));
      o.forEach((i) => add(i.name, { scope, kind: n.kind, mutable, fn: isFnNode(init) ? init : null }));
    }
  },
  CatchClause(n) { if (n.param) { const o = []; patNames(n.param, o); o.forEach((i) => add(i.name, { scope: n, kind: "param" })); } },
});
walkAll(ast, (n) => {
  if (isFnNode(n)) { const o = []; n.params.forEach((q) => patNames(q, o)); o.forEach((i) => add(i.name, { scope: n, kind: "param", mutable: true })); }
});
const resolve = (name, pos) => {
  let best = null;
  for (const d of decls[name] || []) {
    if (d.scope.start <= pos && pos < d.scope.end && (!best || d.scope.end - d.scope.start < best.scope.end - best.scope.start)) best = d;
  }
  return best;
};
const topFn = (name) => { const d = resolve(name, 0); return d && d.fn ? d.fn : null; };
// Names written after their declaration: assignment, update, or a mutating
// call on the name. An outer let/var that nothing writes is a constant.
const rootName = (n) => { while (n && n.type === "MemberExpression") n = n.object; return n && n.type === "Identifier" ? n.name : null; };
const written = new Set();
walkAll(ast, (n) => {
  if (n.type === "AssignmentExpression") { const r = rootName(n.left); if (r) written.add(r); }
  if (n.type === "UpdateExpression") { const r = rootName(n.argument); if (r) written.add(r); }
  if (n.type === "UnaryExpression" && n.operator === "delete") { const r = rootName(n.argument); if (r) written.add(r); }
  if (n.type === "CallExpression" && n.callee.type === "MemberExpression" && !n.callee.computed
      && /^(set|push|add|delete|clear|splice|pop|shift|unshift|sort|reverse)$/.test(n.callee.property.name)) {
    const r = rootName(n.callee.object); if (r) written.add(r);
  }
});
// window.X = <rhs>
const winDefs = {};
walkAll(ast, (n) => {
  if (n.type === "AssignmentExpression" && n.left.type === "MemberExpression" && !n.left.computed
      && n.left.object.type === "Identifier" && n.left.object.name === "window") {
    (winDefs[n.left.property.name] = winDefs[n.left.property.name] || []).push(n.right);
  }
});
// A window name is a function when every assignment to it is a function, a
// factory call, or an IIFE returning a function; a literal, object, array,
// or an IIFE returning anything else makes it data.
const iifeReturnsFn = (call) => {
  const body = call.callee.body;
  if (!body || body.type !== "BlockStatement") return /Function/.test((body || {}).type);
  return body.body.some((s) => s.type === "ReturnStatement" && s.argument
    && (isFnNode(s.argument) || (s.argument.type === "Identifier" && !!(resolve(s.argument.name, s.argument.start) || {}).fn)));
};
const isFnValue = (r) => /Function/.test(r.type)
  || (r.type === "CallExpression" && (/Function/.test(r.callee.type) ? iifeReturnsFn(r) : true))
  || r.type === "MemberExpression" || (r.type === "Identifier" && !!(resolve(r.name, r.start) || {}).fn);
const isWinFn = (name) => winDefs[name] ? winDefs[name].every(isFnValue) : !!topFn(name);
const BROWSER = /^(location|navigator|document|parent|top|localStorage|sessionStorage|innerWidth|innerHeight|devicePixelRatio|__TAURI__|__TAURI_INTERNALS__|performance|console|crypto|Intl|URL|APP_VERSION)$/;
// Trace/log calls never shape a value; subscription setup is outside-world
// plumbing (it registers listeners; what they deliver arrives as PushSource
// values).
const SKIP_CALLEE = /[Tt]race|[Ll]og[A-Z]|^logToHost$|^log$|[Ss]ubscri|^ensure[A-Z]|^notify$|Listener|^__notifyStartup/;

function reads(node) {
  const r = { data: new Set(), wcalls: new Set(), fcalls: new Set() };
  const s = node.start, e = node.end;
  walkAnc(node, {
    NewExpression(ne) { if (ne.callee.name === "Date" && ne.arguments.length === 0) r.data.add("@clock"); },
    MemberExpression(m, anc) {
      if (m.object.type !== "Identifier" || m.object.name !== "window" || m.computed) return;
      const p = anc[anc.length - 2], name = m.property.name;
      if (p && p.type === "CallExpression" && p.callee === m) r.wcalls.add(name);
      else if (p && p.type === "AssignmentExpression" && p.left === m) { /* a write */ }
      else if (!BROWSER.test(name) && !isWinFn(name)) r.data.add(name);
    },
    Identifier(id, anc) {
      const p = anc[anc.length - 2];
      if (p && p.type === "MemberExpression" && p.property === id && !p.computed) return;
      if (p && p.type === "Property" && p.key === id && !p.computed && !p.shorthand) return;
      const d = resolve(id.name, id.start);
      if (!d) {
        if (/^(localStorage|sessionStorage)$/.test(id.name)) r.data.add("@storage");
        else if (id.name === "document") r.data.add("@dom");
        else if (id.name === "performance") r.data.add("@clock");
        else if (id.name === "Date" && p && p.type === "MemberExpression" && p.object === id && p.property.name === "now") r.data.add("@clock");
        return;
      }
      if (d.scope.start >= s && d.scope.end <= e) return; // declared inside this function
      if (d.fn) { r.fcalls.add(d.fn); d.fn.__label = d.fn.__label || id.name; return; }
      if (d.mutable && written.has(id.name)) r.data.add(id.name);
    },
  });
  return r;
}
// Every read a helper reaches, mapped to the function it happens in.
function closure(name, seen = new Set(), out = new Map()) {
  const nodes = winDefs[name] || (topFn(name) ? [topFn(name)] : []);
  for (const n of nodes) walkFn(n, name, seen, out);
  return out;
}
function walkFn(node, label, seen, out) {
  if (seen.has(node)) return;
  seen.add(node);
  const r = reads(node);
  for (const x of r.data) if (!out.has(x)) out.set(x, label);
  for (const c of r.wcalls) if (!SKIP_CALLEE.test(c)) for (const n of winDefs[c] || (topFn(c) ? [topFn(c)] : [])) walkFn(n, c, seen, out);
  for (const f of r.fcalls) if (!SKIP_CALLEE.test(f.__label)) walkFn(f, f.__label, seen, out);
}

// ---- verdicts ---------------------------------------------------------------
const listMode = process.argv.includes("--list");
const used = new Set();
let failures = 0;
const allowed = (helper, read, inFn) => {
  if (READ_OK[read]) { used.add("READ_OK:" + read); return READ_OK[read]; }
  for (const h of [helper, inFn]) {
    const ok = HELPER_OK[h];
    if (ok && ok.reads.includes(read)) { used.add("HELPER_OK:" + h + ":" + read); return ok.why; }
  }
  if (INITIALIZER.test(helper)) return "initializer: read once at mount";
  return null;
};
for (const [helper, where] of [...sites.entries()].sort()) {
  const direct = where.every((w) => w.endsWith("(data)"));
  if (direct) {
    if (BROWSER.test(helper) || isWinFn(helper)) continue;
    const why = allowed(helper, helper, helper);
    if (why) { if (listMode) console.log(`ok    window.${helper} read directly: ${why}`); continue; }
    failures++;
    console.log(`FAIL  window.${helper} is read directly by a binding, and it's data, not a function`);
    where.forEach((w) => console.log(`        at ${w}`));
    continue;
  }
  const got = closure(helper);
  const bad = [];
  for (const [read, inFn] of got) {
    const why = allowed(helper, read, inFn);
    if (why) { if (listMode) console.log(`ok    ${helper} reads ${read}${inFn !== helper ? " (in " + inFn + ")" : ""}: ${why}`); }
    else bad.push(read + (inFn !== helper ? " (in " + inFn + ")" : ""));
  }
  if (bad.length) {
    failures++;
    console.log(`FAIL  ${helper} reads ${bad.join(", ")}`);
    where.filter((w) => !w.endsWith("(data)")).slice(0, 6).forEach((w) => console.log(`        bound at ${w}`));
  }
}
const stale = [
  ...Object.keys(READ_OK).filter((k) => !used.has("READ_OK:" + k)).map((k) => "READ_OK." + k),
  ...Object.entries(HELPER_OK).flatMap(([h, o]) => o.reads.filter((r) => !used.has("HELPER_OK:" + h + ":" + r)).map((r) => `HELPER_OK.${h} ${r}`)),
];
if (stale.length) console.log(`note  allowlist entries no binding needs any more (remove them): ${stale.join(", ")}`);
console.log(`${sites.size} window names bound in ${xmluiFiles.length} files; ${failures} unlisted ${failures === 1 ? "read" : "reads"}.`);
if (failures) {
  console.log("Keep displayed state in XMLUI state (var., a global, or a DataSource/PushSource) and pass it in, or add an allowlist entry with its reason. See docs/developing-bram.md §Code organization.");
}
process.exit(failures ? 1 : 0);
