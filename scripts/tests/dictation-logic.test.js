// Logic tests for app/__shell/dictation.js (judell/bram#417).
// Run: node scripts/tests/dictation-logic.test.js
// Drives the live loop and the field half by hand: audio blocks in, one
// loop step at a time, a stubbed speech engine. No AudioContext, no timers.
const assert = require("node:assert");
const path = require("node:path");
const BramDictation = require(path.join(__dirname, "../../app/__shell/dictation.js"));

const RATE = 16000;
const BLOCK = 4096;
const block = (amp) => new Float32Array(BLOCK).fill(amp);

// A stub engine endpoint: live windows get the next text from `windows`,
// segment repairs get the next from `repairs`. `gate`, when set, holds a
// repair open until released.
function stubEngine(windows, repairs) {
  const calls = { live: 0, segment: 0 };
  let gate = null;
  const fetchStub = async (_url, init) => {
    const name = init.body.get("file").name;
    let text;
    if (name === "segment.wav") {
      text = repairs[calls.segment++];
      if (gate) await gate.promise;
    } else {
      text = windows[calls.live++];
    }
    return { ok: true, status: 200, json: async () => ({ text }) };
  };
  const hold = () => {
    let release;
    const promise = new Promise((r) => (release = r));
    gate = { promise };
    return () => {
      gate = null;
      release();
    };
  };
  return { fetchStub, calls, hold };
}

function harness(windows, repairs, opts) {
  const box = { value: (opts && opts.initial) || "" };
  const traces = [];
  const trace = (stage, fields) => traces.push(Object.assign({ stage }, fields));
  const engine = stubEngine(windows, repairs);
  let loop;
  const field = BramDictation.createField({
    get: () => box.value,
    set: (v) => (box.value = v),
    trace,
    onEditBoundary: (fromSeq) => {
      if (!opts || opts.wireBoundary !== false) loop.setEditBoundary(fromSeq);
    },
  });
  let lastParts = [];
  loop = BramDictation.createLoop({
    requestId: "test",
    trace,
    fetch: engine.fetchStub,
    onPartial: (p) => {
      lastParts = p.parts;
      field.showLive(p);
    },
  });
  const t = loop._test;
  const speak = (seconds) => {
    for (let i = 0; i < Math.ceil((seconds * RATE) / BLOCK); i++) t.frame(block(0.1), RATE);
  };
  const quiet = (seconds) => {
    for (let i = 0; i < Math.ceil((seconds * RATE) / BLOCK); i++) t.frame(block(0), RATE);
  };
  // Speak, pause past pauseMs, and step: one final window.
  const sentence = async () => {
    speak(3);
    quiet(1.3);
    await t.tickOnce();
  };
  // The same, with a provisional update partway through, as in real use.
  const sentenceWithPartial = async () => {
    speak(1.5);
    await t.tickOnce();
    speak(1.5);
    quiet(1.3);
    await t.tickOnce();
  };
  const finish = async () => {
    const result = await loop.finish();
    const written = field.finalize(result.text, result.parts);
    return { result, written };
  };
  return { box, traces, engine, loop, field, t, speak, quiet, sentence, sentenceWithPartial, finish, parts: () => lastParts };
}

const tests = [];
const test = (name, fn) => tests.push({ name, fn });

test("pauses are repaired into one sentence when nothing is edited", async () => {
  const h = harness(["One.", "Two."], ["One two."]);
  await h.sentence();
  assert.strictEqual(h.box.value, "One.");
  await h.sentence();
  assert.strictEqual(h.box.value, "One. Two.");
  await h.t.tickOnce(); // the repair
  assert.strictEqual(h.box.value, "One two.");
  const { result } = await h.finish();
  assert.strictEqual(result.text, "One two.");
  assert.strictEqual(h.box.value, "One two.");
});

// The 2026-10-02 receipt: an edit after the first sentence, then two pause
// repairs. Before the fix each repair erased everything said since the edit
// (264 characters delivered, 22 kept).
test("a pause repair after an adopted edit erases nothing", async () => {
  const h = harness(
    ["One.", "Two", "Two.", "Three", "Three.", "Four", "Four."],
    ["Two three.", "Two three four."],
    { initial: "Note:" },
  );
  await h.sentence();
  assert.strictEqual(h.box.value, "Note: One.");
  h.box.value += " (typed)";
  await h.sentenceWithPartial(); // the provisional update adopts the edit
  assert.strictEqual(h.box.value, "Note: One. (typed) Two.");
  assert.strictEqual(h.t.state.editSeq, 1);
  await h.sentenceWithPartial();
  assert.strictEqual(h.box.value, "Note: One. (typed) Two. Three.");
  await h.t.tickOnce(); // repair over the two windows after the edit
  assert.strictEqual(h.box.value, "Note: One. (typed) Two three.");
  await h.sentenceWithPartial();
  await h.t.tickOnce(); // second repair, three windows
  assert.strictEqual(h.box.value, "Note: One. (typed) Two three four.");
  const repairs = h.traces.filter((x) => x.stage === "voice-repair");
  assert.deepStrictEqual(repairs.map((r) => [r.applied, r.windows]), [[true, 2], [true, 3]]);
  await h.finish();
  assert.strictEqual(h.box.value, "Note: One. (typed) Two three four.");
  const fin = h.traces.find((x) => x.stage === "finalize-into");
  assert.strictEqual(fin.edited, true);
});

// The edit is first noticed as a window commits (no provisional update in
// between). That window's segment began before the edit, so it is closed
// and the window stands alone: its pause stays unrepaired, and nothing is
// erased.
test("an edit noticed at a commit costs one unrepaired pause, no text", async () => {
  const h = harness(["One.", "Two.", "Three.", "Four."], ["Three four."], { initial: "Note:" });
  await h.sentence();
  h.box.value += " (typed)";
  await h.sentence();
  assert.strictEqual(h.box.value, "Note: One. (typed) Two.");
  await h.sentence();
  await h.t.tickOnce(); // nothing to repair yet
  assert.strictEqual(h.box.value, "Note: One. (typed) Two. Three.");
  await h.sentence();
  await h.t.tickOnce(); // repair over "Three." and "Four."
  assert.strictEqual(h.box.value, "Note: One. (typed) Two. Three four.");
  await h.finish();
  assert.strictEqual(h.box.value, "Note: One. (typed) Two. Three four.");
});

test("a repair in flight when an edit is adopted is discarded", async () => {
  const h = harness(["One.", "Two.", "Three."], ["One two."]);
  await h.sentence();
  await h.sentence();
  const release = h.engine.hold();
  const repairing = h.t.tickOnce(); // repair over windows 0 and 1, held open
  await new Promise((r) => setImmediate(r));
  h.box.value += " (typed)";
  h.field.showLive({ parts: h.parts(), provisional: "", openSeq: h.t.state.seq });
  assert.strictEqual(h.t.state.editSeq, 2);
  release();
  await repairing;
  const repair = h.traces.find((x) => x.stage === "voice-repair");
  assert.strictEqual(repair.applied, false);
  assert.strictEqual(repair.reason, "edit-boundary");
  assert.strictEqual(h.box.value, "One. Two. (typed)");
  await h.sentence();
  assert.strictEqual(h.box.value, "One. Two. (typed) Three.");
});

test("erased text stays erased", async () => {
  const h = harness(["One.", "Two."], []);
  await h.sentence();
  h.box.value = "";
  await h.sentence();
  assert.strictEqual(h.box.value, "Two.");
  await h.finish();
  assert.strictEqual(h.box.value, "Two.");
});

// judell/bram#416 relies on this: dictated text never carries a line break.
test("dictated text never contains a newline", async () => {
  const h = harness(["Line one.\nLine two.", "Line\r\nthree."], ["Line one.\n\nLine two. Line three."]);
  await h.sentence();
  assert.ok(!/[\r\n]/.test(h.box.value), "live window");
  await h.sentence();
  await h.t.tickOnce();
  assert.ok(!/[\r\n]/.test(h.box.value), "after repair");
  const { result } = await h.finish();
  assert.ok(!/[\r\n]/.test(result.text), "final text");
  assert.ok(!/[\r\n]/.test(h.box.value), "final write");
  const f = BramDictation.createField({ get: () => "", set: (v) => (f.value = v) });
  f.finalize("a\nb\r\nc", []);
  assert.strictEqual(f.value, "a b c");
});

test("silence and blips are never sent", async () => {
  const h = harness([], []);
  h.quiet(3);
  await h.t.tickOnce();
  h.t.frame(block(0.1), RATE); // one 256 ms blip, under LIVE_MIN_VOICED_S
  h.quiet(1);
  await h.t.tickOnce();
  assert.strictEqual(h.engine.calls.live, 0);
  assert.strictEqual(h.t.state.winLen, 0);
});

// judell/bram#417, Studio's first run: the host's get() returned the text
// from before the dictation (an XMLUI component snapshot), so every window
// read as an edit and the final write emptied the field. The script can't
// fix the host, but the trace names the problem, once.
test("a stale get() is flagged in the trace", async () => {
  const traces = [];
  let shown = "";
  const field = BramDictation.createField({
    get: () => "", // frozen at the pre-dictation text
    set: (v) => (shown = v),
    trace: (stage, fields) => traces.push(Object.assign({ stage }, fields)),
  });
  field.showLive({ parts: [], provisional: "Hello", openSeq: 0 });
  field.showLive({ parts: [{ seq: 0, text: "Hello." }], provisional: "", openSeq: 1 });
  field.showLive({ parts: [{ seq: 0, text: "Hello." }], provisional: "World", openSeq: 1 });
  const flags = traces.filter((x) => x.stage === "live-get-stale-suspected");
  assert.strictEqual(flags.length, 1);
  assert.strictEqual(flags[0].readLen, 0);
  assert.strictEqual(flags[0].writtenLen, 5);
});

test("an ordinary edit is not flagged as a stale get()", async () => {
  const h = harness(["One.", "Two"], [], { initial: "Note:" });
  await h.sentence();
  h.box.value += " (typed)";
  h.speak(1.5);
  await h.t.tickOnce();
  assert.ok(h.traces.some((x) => x.stage === "live-edit-adopted"));
  assert.ok(!h.traces.some((x) => x.stage === "live-get-stale-suspected"));
});

test("a stopped field is left alone", async () => {
  const h = harness(["One."], []);
  h.field.markStopped();
  await h.sentence();
  assert.strictEqual(h.box.value, "");
  const { written } = await h.finish();
  assert.strictEqual(written, null);
  assert.strictEqual(h.box.value, "");
});

(async () => {
  let failed = 0;
  for (const { name, fn } of tests) {
    try {
      await fn();
      console.log("ok   " + name);
    } catch (e) {
      failed++;
      console.log("FAIL " + name + "\n     " + String(e && e.message).split("\n").join("\n     "));
    }
  }
  console.log(failed ? failed + " of " + tests.length + " failed" : "all " + tests.length + " passed");
  process.exit(failed ? 1 : 0);
})();
