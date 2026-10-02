// Bram's live dictation, host-neutral (judell/bram#417). Capture, the live
// loop and the field half, with no Tauri call, no postMessage and no
// localStorage: whatever is host-specific comes in as an option. Bram's
// shell (app/main.js) and pane (helpers.js) are the first two callers; a
// project page served by Bram loads this file from /__shell/dictation.js
// and calls BramDictation.attach().
//
// The loop (issue-407): a Web Audio tap on the mic stream feeds a sliding
// window to the speech engine. Every LIVE_STEP_MS the audio since the last
// commit is transcribed as provisional text; a pause (pauseMs under
// LIVE_RMS, with at least LIVE_MIN_FINAL_S of audio) or LIVE_MAX_WIN_S makes
// the next result final, which commits its text and drops its audio. One
// request at a time; the committed tail goes along as `prompt`. Measured in
// the #407 prototype: ~420 ms per request, matching a one-request pass over
// the whole recording word for word. voice-final logs every pause
// (pausesMs) so the default pause can be revisited from data.
(function (root) {
  // Speech-to-text engines. The live loop and the boundary repair talk only
  // to the selected engine; what is specific to one engine (endpoint,
  // request format, model, quirks) lives in its adapter. Whisper is the only
  // engine today.
  const engines = {
    whisper: {
      id: "whisper",
      host: "http://127.0.0.1:18080",
      url: "http://127.0.0.1:18080/inference",
      // Host-native macOS/Linux launch expands this on the host. Windows launch
      // sends it to WSL so it resolves under the WSL user's home directory.
      modelPath: "~/.local/share/whisper-models/ggml-small.en.bin",
      caps: {
        // Batch only: live text comes from polling a sliding window.
        streams: false,
        // Each request comes back as finished sentences, so a window cut at
        // a pause ends with a period; the live loop repairs those boundaries.
        punctuatesPerRequest: true,
        // Invents "Thank you." from silence; silence is never sent alone.
        hallucinatesOnSilence: true,
      },
      // Whisper's stock output for near-silence. Only traced (silencePhrase
      // on voice-window), never dropped: a 0.5 s cutoff deleted a deliberate
      // "thank you" (0.43 s voiced) while the LIVE_MIN_VOICED_S gate alone
      // kept every phantom out. If phantoms return, their voicedS sets the
      // cutoff.
      silencePhrases: /^(thank you|thanks|thanks for watching|thank you for watching|bye|you)$/,
      // `exact` pins decoding (no temperature fallback) for the live windows.
      form(file, name, opts) {
        const fd = new FormData();
        fd.append("file", file, name);
        fd.append("response_format", "json");
        if (opts && opts.exact) {
          fd.append("temperature", "0.0");
          fd.append("temperature_inc", "0.0");
        }
        if (opts && typeof opts.prompt === "string") fd.append("prompt", opts.prompt);
        return fd;
      },
      text(json) {
        return (json && json.text) || "";
      },
    },
  };

  const LIVE_RATE = 16000;
  const LIVE_STEP_MS = 700;
  // 700 ms broke sentences at thinking pauses (Jon: "a bit aggressive about
  // stopping in the middle of the sentence"); a final window ends with a
  // period and the next starts capitalized.
  const LIVE_PAUSE_MS = 1200;
  // Quiet this long after a sub-LIVE_MIN_VOICED_S blip means it was a blip.
  const LIVE_BLIP_QUIET_MS = 700;
  // Pauses shorter than this are gaps between words, not worth logging.
  const LIVE_PAUSE_LOG_MIN_MS = 250;
  const LIVE_MIN_FINAL_S = 1.5;
  const LIVE_MAX_WIN_S = 12;
  // At the cap, cut at the quietest block in this much trailing audio
  // instead of at the very end, so the cut falls between words (a hard cut
  // produced "prototyp ing", 2026-09-26). The rest stays for the next window.
  const LIVE_CAP_LOOKBACK_S = 1.5;
  // A request that hasn't answered by now is abandoned, so a stuck
  // whisper-server costs seconds, not WebKit's ~60 s fetch timeout per
  // request (seen 2026-09-26 when a full stderr pipe wedged the server).
  const LIVE_REQUEST_TIMEOUT_MS = 15000;
  const LIVE_RMS = 0.012;
  const LIVE_SILENCE_KEEP_MS = 2000;
  // A click or breath is one or two ~85 ms blocks over LIVE_RMS; speech is
  // far more. Windows below this much voiced audio are never sent.
  const LIVE_MIN_VOICED_S = 0.3;
  // Boundary repair (dictation-pauses-become-sentence-breaks). A window
  // finalized at a pause comes back as a finished sentence ("my dictation
  // into. Bram. Itself.": 2026-09-29, breaks at exactly the pauses over
  // pauseMs). So consecutive finals collect into a segment, and after each
  // new final the whole segment is transcribed again in one request, whose
  // text replaces the segment's parts: one context, no pause breaks. A
  // segment holds at most this much audio; past that a new one starts, and
  // that one boundary stays unrepaired.
  const LIVE_SEGMENT_MAX_S = 20;

  // The capture worklet sits beside this file.
  const defaultWorkletUrl = (() => {
    try {
      const src = typeof document !== "undefined" && document.currentScript && document.currentScript.src;
      if (src) return src.split("?")[0].replace(/dictation\.js$/, "dictation-capture-worklet.js");
    } catch (_) {}
    return "/__shell/dictation-capture-worklet.js";
  })();

  const liveResample = (input, fromRate) => {
    if (fromRate === LIVE_RATE) return new Float32Array(input);
    const ratio = fromRate / LIVE_RATE;
    const out = new Float32Array(Math.floor(input.length / ratio));
    for (let i = 0; i < out.length; i++) {
      const x = i * ratio;
      const j = Math.floor(x);
      const f = x - j;
      out[i] = input[j] * (1 - f) + (input[j + 1] !== undefined ? input[j + 1] : input[j]) * f;
    }
    return out;
  };

  const liveWav = (samples) => {
    const buf = new ArrayBuffer(44 + samples.length * 2);
    const v = new DataView(buf);
    const s = (o, t) => {
      for (let i = 0; i < t.length; i++) v.setUint8(o + i, t.charCodeAt(i));
    };
    s(0, "RIFF");
    v.setUint32(4, 36 + samples.length * 2, true);
    s(8, "WAVE");
    s(12, "fmt ");
    v.setUint32(16, 16, true);
    v.setUint16(20, 1, true);
    v.setUint16(22, 1, true);
    v.setUint32(24, LIVE_RATE, true);
    v.setUint32(28, LIVE_RATE * 2, true);
    v.setUint16(32, 2, true);
    v.setUint16(34, 16, true);
    s(36, "data");
    v.setUint32(40, samples.length * 2, true);
    for (let i = 0; i < samples.length; i++) {
      v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, samples[i])) * 0x7fff, true);
    }
    return new Blob([buf], { type: "audio/wav" });
  };

  // Every window's text is one line: bracketed tags out, whitespace runs
  // (newlines included) to a single space. Dictation never adds a line break
  // (judell/bram#416 relies on this).
  const liveClean = (t) =>
    String(t || "")
      .replace(/\[[^\]]*\]|\([^)]*\)/g, " ")
      .replace(/\s+/g, " ")
      .trim();

  const validPauseMs = (v) => {
    const n = Number(v);
    return n >= 300 && n <= 5000 ? n : LIVE_PAUSE_MS;
  };

  // Capture plus the live loop for one dictation.
  //   engine      adapter (default engines.whisper)
  //   trace       (stage, fields) => void
  //   requestId   carried on every trace line
  //   pauseMs     300..5000, else LIVE_PAUSE_MS
  //   workletUrl  default: beside this file
  //   onPartial   ({ committed, provisional, parts, openSeq, repairing })
  //   shouldAttach () => false when a stop arrived while the worklet loaded
  //   onAttach    () => void, at the moment capture is live
  //   fetch       override, for tests
  const createLoop = (options) => {
    const o = options || {};
    const engine = o.engine || engines.whisper;
    const trace = typeof o.trace === "function" ? o.trace : () => {};
    const doFetch = typeof o.fetch === "function" ? o.fetch : (url, init) => fetch(url, init);
    const requestId = o.requestId || "";
    const L = {
      requestId,
      ctx: null,
      nodes: [],
      win: [],
      winLen: 0,
      sentLen: 0,
      quietMs: 0,
      pauseMs: validPauseMs(o.pauseMs),
      pauses: [],
      spoke: false,
      committed: [],
      parts: [],
      seq: 0,
      provisional: "",
      busy: false,
      pending: null,
      stopped: false,
      // Aborted or finished: late results are dropped and nothing publishes.
      dead: false,
      timer: null,
      latencies: [],
      errors: 0,
      capture: "",
      segment: null,
      repairDue: false,
      repairMs: [],
      // Parts below this seq belong to text the field adopted as an edit.
      editSeq: 0,
    };

    // Seconds of voiced audio in the first n samples of the window (all of
    // it by default).
    const liveVoicedS = (n = L.winLen) => {
      let voiced = 0;
      let o2 = 0;
      for (const c of L.win) {
        if (o2 >= n) break;
        const k = Math.min(c.length, n - o2);
        if (c.voiced) voiced += k;
        o2 += k;
      }
      return voiced / LIVE_RATE;
    };
    // Whether the window holds enough speech to transcribe; a window of
    // silence and stray blips is dropped instead.
    const liveHasSpeech = () => liveVoicedS() >= LIVE_MIN_VOICED_S;
    const liveIsSilencePhrase = (text) =>
      !!engine.silencePhrases &&
      engine.silencePhrases.test(
        String(text || "")
          .toLowerCase()
          .replace(/[^a-z ]/g, "")
          .trim(),
      );
    const liveDropWindow = () => {
      L.win = [];
      L.winLen = 0;
      L.sentLen = 0;
    };

    // The running text goes to whoever owns the field.
    const livePublish = () => {
      if (L.dead || typeof o.onPartial !== "function") return;
      try {
        o.onPartial({
          committed: L.committed.join(" "),
          provisional: L.provisional,
          parts: L.parts,
          openSeq: L.seq,
          // A boundary repair is due or in flight: a send waits for it
          // (helpers.js __bramVoiceRepairWait).
          repairing: !!L.repairDue,
        });
      } catch (_) {}
    };

    const liveFlat = (n) => {
      const out = new Float32Array(n);
      let o2 = 0;
      for (const c of L.win) {
        const k = Math.min(c.length, n - o2);
        out.set(c.subarray(0, k), o2);
        o2 += k;
        if (o2 >= n) break;
      }
      return out;
    };

    // The sample index to cut a capped window at: the middle of the quietest
    // block in the last LIVE_CAP_LOOKBACK_S, or the end if none is known.
    const liveQuietCut = () => {
      const from = L.winLen - LIVE_RATE * LIVE_CAP_LOOKBACK_S;
      let o2 = 0;
      let best = -1;
      let bestRms = Infinity;
      for (const c of L.win) {
        const end = o2 + c.length;
        if (end >= from && typeof c.rms === "number" && c.rms < bestRms) {
          bestRms = c.rms;
          best = o2 + Math.floor(c.length / 2);
        }
        o2 = end;
      }
      return best > 0 ? best : L.winLen;
    };

    // A segment that starts before the field's adopted edit (L.editSeq, from
    // setEditBoundary) must not be repaired: the repaired text would land
    // in a part the field no longer reads while the parts it does read go
    // empty, erasing everything spoken since the edit (2026-10-02: 264 chars
    // delivered, 22 kept).
    const liveSegmentBeforeEdit = (S) => {
      const p = S && L.parts[S.firstPart];
      return !!p && p.seq < L.editSeq;
    };

    // Add a finalized window's audio to the current segment, or start a new
    // one; once a segment spans a boundary, a repair is due.
    const liveSegmentAdd = (samples, partIdx) => {
      const S = L.segment;
      if (
        S &&
        S.lastPart === partIdx - 1 &&
        !liveSegmentBeforeEdit(S) &&
        S.len + samples.length <= LIVE_RATE * LIVE_SEGMENT_MAX_S
      ) {
        S.audio.push(samples);
        S.len += samples.length;
        S.lastPart = partIdx;
        L.repairDue = true;
      } else {
        L.segment = { audio: [samples], len: samples.length, firstPart: partIdx, lastPart: partIdx };
      }
    };

    // Transcribe the current segment again as one request. Its text goes in
    // the segment's first part and the rest go empty; their seq numbers stay.
    // The field's edit tracking lines up only while no segment spans an
    // adopted edit, which liveSegmentBeforeEdit guarantees.
    const liveRepair = async () => {
      const S = L.segment;
      L.repairDue = false;
      if (!S || S.lastPart <= S.firstPart || liveSegmentBeforeEdit(S)) return;
      L.busy = true;
      const first = S.firstPart;
      const through = S.lastPart;
      const flat = new Float32Array(S.len);
      let o2 = 0;
      for (const a of S.audio) {
        flat.set(a, o2);
        o2 += a.length;
      }
      const before = L.parts
        .slice(0, first)
        .map((p) => p.text)
        .filter(Boolean)
        .join(" ");
      const fd = engine.form(liveWav(flat), "segment.wav", {
        exact: true,
        prompt: before.slice(-200),
      });
      const t0 = Date.now();
      let status = null;
      let timedOut = false;
      let applied = false;
      let silencePhrase = false;
      let editBoundary = false;
      const abort = new AbortController();
      const timer = setTimeout(() => {
        timedOut = true;
        abort.abort();
      }, LIVE_REQUEST_TIMEOUT_MS);
      try {
        const res = await doFetch(engine.url, { method: "POST", body: fd, signal: abort.signal });
        status = res.status;
        const text = res.ok ? liveClean(engine.text(await res.json())) : "";
        if (L.dead) return;
        silencePhrase = liveIsSilencePhrase(text);
        // An edit adopted while this request was out: drop the result.
        editBoundary = liveSegmentBeforeEdit(S);
        if (text && !silencePhrase && !editBoundary) {
          L.parts[first].text = text;
          for (let i = first + 1; i <= through; i++) L.parts[i].text = "";
          L.committed = L.parts.map((p) => p.text).filter(Boolean);
          applied = true;
        }
      } catch (e) {
        L.errors++;
      } finally {
        clearTimeout(timer);
        const ms = Date.now() - t0;
        L.repairMs.push(ms);
        trace("voice-repair", {
          requestId: L.requestId,
          windows: through - first + 1,
          segmentS: Math.round((S.len / LIVE_RATE) * 10) / 10,
          applied,
          ...(editBoundary ? { reason: "edit-boundary" } : {}),
          ...(silencePhrase ? { silencePhrase } : {}),
          ...(timedOut ? { timedOut } : {}),
          latencyMs: ms,
          httpStatus: status,
        });
        L.busy = false;
        livePublish();
      }
    };

    const liveTranscribe = async (final, cutAt) => {
      L.busy = true;
      const n = cutAt || L.winLen;
      const voicedS = liveVoicedS(n);
      let silencePhrase = false;
      L.sentLen = n;
      const samples = liveFlat(n);
      const fd = engine.form(liveWav(samples), "live.wav", {
        exact: true,
        prompt: L.committed.join(" ").slice(-200),
      });
      const t0 = Date.now();
      let status = null;
      let timedOut = false;
      const abort = new AbortController();
      const timer = setTimeout(() => {
        timedOut = true;
        abort.abort();
      }, LIVE_REQUEST_TIMEOUT_MS);
      try {
        const res = await doFetch(engine.url, { method: "POST", body: fd, signal: abort.signal });
        status = res.status;
        const text = res.ok ? liveClean(engine.text(await res.json())) : "";
        if (L.dead) return;
        L.latencies.push(Date.now() - t0);
        if (final) {
          silencePhrase = liveIsSilencePhrase(text);
          if (text) {
            L.committed.push(text);
            L.parts.push({ seq: L.seq, text });
            liveSegmentAdd(samples, L.parts.length - 1);
          }
          // Every final closes a window, kept or not; the field counts
          // windows to tell which text it already showed before an edit.
          L.seq++;
          let drop = n;
          while (drop > 0 && L.win.length) {
            if (L.win[0].length <= drop) {
              drop -= L.win[0].length;
              L.winLen -= L.win.shift().length;
            } else {
              const { voiced, rms } = L.win[0];
              L.win[0] = L.win[0].subarray(drop);
              L.win[0].voiced = voiced;
              L.win[0].rms = rms;
              L.winLen -= drop;
              drop = 0;
            }
          }
          L.sentLen = 0;
          L.provisional = "";
        } else {
          L.provisional = text;
        }
        livePublish();
      } catch (e) {
        L.errors++;
      } finally {
        clearTimeout(timer);
        trace("voice-window", {
          requestId: L.requestId,
          final,
          windowS: Math.round((n / LIVE_RATE) * 10) / 10,
          voicedS: Math.round(voicedS * 100) / 100,
          ...(silencePhrase ? { silencePhrase } : {}),
          ...(cutAt ? { cut: "quiet" } : {}),
          ...(timedOut ? { timedOut } : {}),
          latencyMs: Date.now() - t0,
          httpStatus: status,
        });
        L.busy = false;
      }
    };

    // One step of the loop, without rescheduling.
    const liveTickOnce = async () => {
      if (!L.busy && L.repairDue) {
        // A repair goes ahead of the next live window: a send may be waiting
        // on it.
        L.pending = liveRepair();
        await L.pending;
      } else if (!L.busy && L.winLen > 0 && !liveHasSpeech()) {
        // Too little voice to send. Once the quiet has lasted a pause, it was
        // a blip, not the start of speech: drop it.
        if (L.quietMs >= LIVE_BLIP_QUIET_MS) liveDropWindow();
      } else if (!L.busy && L.winLen > 0) {
        const secs = L.winLen / LIVE_RATE;
        const fresh = L.winLen - L.sentLen;
        const paused = L.quietMs >= L.pauseMs && secs >= LIVE_MIN_FINAL_S;
        const capped = secs >= LIVE_MAX_WIN_S && !paused;
        const final = capped || (paused && fresh > 0);
        if (final || fresh >= LIVE_RATE * 0.3) {
          L.pending = liveTranscribe(final, capped ? liveQuietCut() : 0);
          await L.pending;
        }
      }
    };

    const liveTick = async () => {
      if (L.dead || L.stopped) return;
      await liveTickOnce();
      if (!L.dead && !L.stopped) L.timer = setTimeout(liveTick, LIVE_STEP_MS);
    };

    const liveFrame = (data, sampleRate) => {
      if (L.stopped) return;
      let sum = 0;
      for (let i = 0; i < data.length; i++) sum += data[i] * data[i];
      const rms = Math.sqrt(sum / data.length);
      const voiced = rms > LIVE_RMS;
      if (voiced) {
        // A pause inside speech just ended; keep its length for voice-final.
        if (L.spoke && L.quietMs >= LIVE_PAUSE_LOG_MIN_MS && L.pauses.length < 500) {
          L.pauses.push(Math.round(L.quietMs / 10) * 10);
        }
        L.spoke = true;
        L.quietMs = 0;
      }
      else L.quietMs += (data.length / sampleRate) * 1000;
      // Silence doesn't grow the window: not before the first word, and not
      // after LIVE_SILENCE_KEEP_MS of quiet. Whisper invents "Thank you." from
      // silence (seen in the first live test), so silence is never sent alone.
      if (!voiced && (L.winLen === 0 || L.quietMs > LIVE_SILENCE_KEEP_MS)) return;
      const c = liveResample(data, sampleRate);
      c.voiced = voiced;
      c.rms = rms;
      L.win.push(c);
      L.winLen += c.length;
    };

    const stopLiveCapture = () => {
      L.stopped = true;
      if (L.timer) clearTimeout(L.timer);
      L.timer = null;
      for (const n of L.nodes) {
        try {
          n.disconnect();
        } catch (_) {}
      }
      try {
        L.ctx && L.ctx.close();
      } catch (_) {}
    };

    // Attach capture to the stream and start the loop. Resolves true when
    // capture is live, false when it couldn't start or came too late.
    const start = async (mediaStream) => {
      try {
        const Ctx = root.AudioContext || root.webkitAudioContext;
        if (!Ctx) throw new Error("no AudioContext");
        L.ctx = new Ctx();
        const src = L.ctx.createMediaStreamSource(mediaStream);
        const mute = L.ctx.createGain();
        mute.gain.value = 0;
        mute.connect(L.ctx.destination);
        let node = null;
        if (L.ctx.audioWorklet && typeof AudioWorkletNode === "function") {
          try {
            await L.ctx.audioWorklet.addModule(o.workletUrl || defaultWorkletUrl);
            node = new AudioWorkletNode(L.ctx, "bram-voice-capture");
            node.port.onmessage = (e) => liveFrame(e.data, L.ctx.sampleRate);
            L.capture = "worklet";
          } catch (e) {
            trace("voice-live-worklet-error", { requestId, error: String(e) });
            node = null;
          }
        }
        if (!node) {
          node = L.ctx.createScriptProcessor(4096, 1, 1);
          node.onaudioprocess = (e) => liveFrame(e.inputBuffer.getChannelData(0), L.ctx.sampleRate);
          L.capture = "script-processor";
        }
        src.connect(node);
        node.connect(mute);
        L.nodes = [src, node, mute];
        // A stop that arrived while the worklet loaded has already taken the
        // host's other path; don't attach a capture nobody will finish.
        if (typeof o.shouldAttach === "function" && !o.shouldAttach()) {
          stopLiveCapture();
          L.dead = true;
          trace("voice-live-too-late", { requestId });
          return false;
        }
        if (typeof o.onAttach === "function") o.onAttach();
        trace("voice-live-start", {
          requestId,
          capture: L.capture,
          sampleRate: L.ctx.sampleRate,
          pauseMs: L.pauseMs,
        });
        L.timer = setTimeout(liveTick, LIVE_STEP_MS);
        return true;
      } catch (e) {
        trace("voice-live-unavailable", { requestId, error: String(e) });
        try {
          L.ctx && L.ctx.close();
        } catch (_) {}
        L.dead = true;
        return false;
      }
    };

    // Stop capturing, commit what's left, and return the live text.
    const finish = async () => {
      stopLiveCapture();
      if (L.pending) {
        try {
          await L.pending;
        } catch (_) {}
      }
      if (L.winLen >= LIVE_RATE * 0.3 && liveHasSpeech()) await liveTranscribe(true);
      // The stop's own final can make a repair due; the delivered text should
      // be the repaired one.
      if (L.repairDue) await liveRepair();
      // A successful final clears the provisional text. If it's still set, the
      // last window never came back (whisper-server stuck or down): keep what
      // was on screen rather than lose it. The 2026-09-26 hang delivered an
      // empty result because nothing had been committed yet.
      if (L.provisional) {
        trace("voice-live-kept-provisional", { requestId: L.requestId, chars: L.provisional.length });
        L.committed.push(L.provisional);
        L.parts.push({ seq: L.seq, text: L.provisional });
        L.seq++;
        L.provisional = "";
        livePublish();
      }
      const text = L.committed.join(" ").trim();
      const lat = L.latencies;
      trace("voice-final", {
        requestId: L.requestId,
        mode: "live",
        capture: L.capture,
        words: text ? text.split(/\s+/).length : 0,
        windows: lat.length,
        avgLatencyMs: lat.length ? Math.round(lat.reduce((a, b) => a + b, 0) / lat.length) : null,
        errors: L.errors,
        pauseMs: L.pauseMs,
        pausesMs: L.pauses,
        repairs: L.repairMs.length,
        repairAvgMs: L.repairMs.length
          ? Math.round(L.repairMs.reduce((a, b) => a + b, 0) / L.repairMs.length)
          : null,
      });
      L.dead = true;
      return { text, errors: L.errors, parts: L.parts };
    };

    const abort = () => {
      stopLiveCapture();
      L.dead = true;
    };

    // The field adopted an edit: windows below fromSeq are the user's text
    // now. The boundary only moves forward. A segment that begins below it
    // is closed, so no repair spans the edit.
    const setEditBoundary = (fromSeq) => {
      const n = Number(fromSeq);
      const accepted = !L.dead && Number.isFinite(n) && n > L.editSeq;
      let segmentClosed = false;
      if (accepted) {
        L.editSeq = n;
        if (liveSegmentBeforeEdit(L.segment)) {
          L.segment = null;
          L.repairDue = false;
          segmentClosed = true;
        }
      }
      return { accepted, segmentClosed };
    };

    return {
      requestId,
      start,
      finish,
      abort,
      setEditBoundary,
      // For scripts/tests/dictation-logic.test.js: feed audio blocks and step
      // the loop by hand, with no AudioContext and no timers.
      _test: { state: L, frame: liveFrame, tickOnce: liveTickOnce },
    };
  };

  // The field half for one dictation: live text goes INTO the field (Jon:
  // "why not put the text into the message box instead of below it"). The
  // field's text from before the dictation is the base; each partial
  // rewrites the field as base plus the live transcript, and finalize sets
  // base plus the final transcript in one step, so the final text lands
  // exactly once. A textarea can't gray out part of its text, so provisional
  // words look like the rest until they're replaced.
  //
  // Your edits win. Each window the loop transcribes has a number (seq); the
  // partial carries the committed parts with their numbers and the number of
  // the window still open. If the field no longer holds what dictation last
  // wrote, you edited it: your text becomes the new base, and only windows
  // you hadn't seen yet are added after it — erased text stays erased
  // (Jon: "I had to erase it … and it came back"). Clearing the field is an
  // edit like any other; dictation carries on into the empty field.
  //   get, set        read and write the field's text
  //   trace           (stage, fields) => void
  //   onEditBoundary  (fromSeq) => void, to pass to the loop's setEditBoundary
  //
  // `get` must return the field's text AT THE MOMENT IT IS CALLED. A stale
  // `get` fails silently and looks like a dictation that erases itself: every
  // window reads as an edit back to the old text, and the final write empties
  // the field (judell/bram#417, Studio's first run). In an XMLUI page, a
  // component handed from markup to page script is a snapshot: its methods
  // keep working but its `value` stays frozen, so `get: () => box.value` is
  // stale. Push the value from the field's onDidChange and return that. The
  // live-get-stale-suspected trace line flags the pattern.
  const createField = (options) => {
    const o = options || {};
    const trace = typeof o.trace === "function" ? o.trace : () => {};
    const get = () => String(o.get() || "");
    const F = {
      started: false,
      // The field's text when the dictation began; `base` moves with edits.
      startBase: "",
      staleFlagged: false,
      base: "",
      fromSeq: 0,
      edited: false,
      stopped: false,
      written: undefined,
      prevWritten: undefined,
      wOpenSeq: 0,
      wHadProv: false,
    };

    const liveText = (parts, provisional, openSeq) => {
      const words = [];
      (parts || []).forEach((p) => {
        if (p && p.seq >= F.fromSeq && p.text) words.push(p.text);
      });
      if (provisional && openSeq >= F.fromSeq) words.push(provisional);
      return words.join(" ");
    };
    const join = (base, text) => {
      const spacer = base && text && !/\s$/.test(base) ? " " : "";
      return base + spacer + text;
    };
    const begin = () => {
      if (F.started) return false;
      F.started = true;
      F.base = get();
      F.startBase = F.base;
      return true;
    };

    // Adopt the field's current text as the base when it differs from what
    // dictation last wrote. Returns true when an edit was adopted.
    const adoptEdit = () => {
      if (F.written === undefined) return false;
      const cur = get();
      // The field can lag a set (the 311→597 duplication), so the value
      // written just before the last one isn't an edit either.
      // An empty field is never a lag, though: it's a send or a clear.
      if (cur === F.written || (cur.trim() && cur === F.prevWritten)) return false;
      // Lengths and the first differing position, to tell a keystroke from
      // the field misreading its own write (2026-10-02: an adoption 14 ms
      // after a window was committed, 117 -> 118 chars, cause unknown).
      const was = String(F.written || "");
      let diffAt = 0;
      while (diffAt < cur.length && diffAt < was.length && cur.charCodeAt(diffAt) === was.charCodeAt(diffAt)) diffAt++;
      const prevLen = F.prevWritten === undefined ? null : String(F.prevWritten).length;
      // Dictation wrote text and the field reads back exactly as it was
      // before the dictation began: either you deleted precisely what was
      // dictated, or the host's `get` is stale. Traced once, never acted on.
      if (!F.staleFlagged && cur === F.startBase && was !== cur) {
        F.staleFlagged = true;
        trace("live-get-stale-suspected", {
          readLen: cur.length,
          writtenLen: was.length,
          hint: "get() returned the pre-dictation text after a write; it must return the field's current text",
        });
      }
      F.edited = true;
      // The window that showed as provisional text is part of what you
      // edited; skip it too, or its final text would bring the erased words
      // back.
      F.fromSeq = F.wHadProv ? F.wOpenSeq + 1 : F.wOpenSeq;
      F.base = cur;
      F.written = cur;
      F.prevWritten = undefined;
      // Tell the loop where the edit is, so a pause repair never folds later
      // windows into a part below fromSeq (which this field no longer reads).
      try {
        if (typeof o.onEditBoundary === "function") o.onEditBoundary(F.fromSeq);
      } catch (_) {}
      trace("live-edit-adopted", {
        baseLen: cur.length,
        writtenLen: was.length,
        prevWrittenLen: prevLen,
        diffAt,
        fromSeq: F.fromSeq,
      });
      return true;
    };

    // Write a partial into the field. Returns { next, first }, or null when
    // the field was stopped.
    const showLive = (partial) => {
      if (F.stopped || !partial) return null;
      const first = begin();
      adoptEdit();
      const next = join(F.base, liveText(partial.parts, partial.provisional, partial.openSeq));
      F.prevWritten = F.written;
      F.written = next;
      F.wOpenSeq = partial.openSeq;
      F.wHadProv = !!partial.provisional;
      try {
        o.set(next);
      } catch (_) {}
      return { next, first };
    };

    // Write the final transcript: base + final text, set ONCE. A separate
    // restore-then-append read the field back before the restore had been
    // applied, and appended onto the live text (first test: 311 chars became
    // 597, the dictation doubled). After an edit, the final text is the
    // windows you hadn't seen (from `parts`), not the whole transcript; after
    // a stop the field is left alone. Returns { next, added }, or null when
    // nothing was written.
    const finalize = (transcript, parts) => {
      if (F.stopped) {
        trace("finalize-into-stopped", {});
        return null;
      }
      begin();
      adoptEdit();
      const base = F.base;
      const text = F.edited ? liveText(parts || [], "", 0) : transcript;
      const cleaned = String(text || "").replace(/\r?\n/g, " ").replace(/[ \t]+/g, " ").trim();
      const next = join(base, cleaned);
      try {
        o.set(next);
      } catch (_) {
        return null;
      }
      trace("finalize-into", { baseLen: base.length, transcriptLen: cleaned.length, nextLen: next.length, edited: F.edited });
      return { next, added: !!cleaned };
    };

    // A send or a clear while recording: neither a late partial nor the
    // final transcript may write into the field afterwards.
    const markStopped = () => {
      F.stopped = true;
    };

    return {
      showLive,
      finalize,
      markStopped,
      get stopped() {
        return F.stopped;
      },
      get edited() {
        return F.edited;
      },
      _test: { state: F },
    };
  };

  // Both halves in one frame, for a project page: asks for the microphone,
  // runs the loop and writes into the field.
  //   get, set   read and write the field's text (required). `get` must
  //              return the text at the moment it is called; see createField
  //              for the XMLUI snapshot trap.
  //   engine, trace, pauseMs, workletUrl   as createLoop. For an engine at
  //              another address: Object.assign({}, BramDictation.engines.whisper,
  //              { host: "http://127.0.0.1:8767", url: "http://127.0.0.1:8767/inference" })
  //   audio      getUserMedia audio constraints, to choose a microphone
  //              (e.g. { deviceId: { exact: id } }); default: the browser's
  //              default input
  //   onState    (state, extra) with "starting" | "recording" | "processing" | "idle"
  // start() rejects with an Error whose .code is "engine-unavailable",
  // "no-microphone" or "capture-unavailable". This script cannot start the
  // speech engine: a page gets dictation only while the engine is already
  // running (in Bram, after a first 🎤 click in its pane).
  const attach = (options) => {
    const o = options || {};
    const engine = o.engine || engines.whisper;
    const trace = typeof o.trace === "function" ? o.trace : () => {};
    const state = (s, extra) => {
      try {
        if (typeof o.onState === "function") o.onState(s, extra || {});
      } catch (_) {}
    };
    const fail = (code, message) => Object.assign(new Error(message), { code });
    let loop = null;
    let field = null;
    let stream = null;
    let busy = false;
    const stopStream = () => {
      if (stream) {
        stream.getTracks().forEach((t) => t.stop());
        stream = null;
      }
    };

    const start = async () => {
      if (busy) return false;
      busy = true;
      state("starting");
      try {
        let up = false;
        try {
          const res = await fetch(engine.host + "/", { method: "GET", cache: "no-store" });
          up = res.ok;
        } catch (_) {}
        if (!up) {
          throw fail("engine-unavailable", "The speech engine isn't running at " + engine.host + ".");
        }
        if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
          throw fail("no-microphone", "This page can't reach the microphone.");
        }
        try {
          stream = await navigator.mediaDevices.getUserMedia({ audio: o.audio || true });
        } catch (e) {
          throw fail("no-microphone", "Microphone access failed: " + String(e));
        }
        const requestId = "dictation-" + Date.now() + "-" + Math.random().toString(36).slice(2);
        const thisLoop = createLoop({
          engine,
          trace,
          requestId,
          pauseMs: o.pauseMs,
          workletUrl: o.workletUrl,
          onPartial: (p) => field && field.showLive(p),
        });
        field = createField({
          get: o.get,
          set: o.set,
          trace,
          onEditBoundary: (fromSeq) => {
            const r = thisLoop.setEditBoundary(fromSeq);
            trace("voice-edit-boundary", { requestId, fromSeq, accepted: r.accepted, segmentClosed: r.segmentClosed });
          },
        });
        if (!(await thisLoop.start(stream))) {
          throw fail("capture-unavailable", "Audio capture couldn't start.");
        }
        loop = thisLoop;
        state("recording");
        return true;
      } catch (e) {
        stopStream();
        loop = null;
        field = null;
        busy = false;
        state("idle", { error: String((e && e.message) || e), code: (e && e.code) || "" });
        throw e;
      }
    };

    // Stop, write the final text into the field, and resolve to the
    // transcript.
    const stop = async () => {
      if (!loop) return "";
      const thisLoop = loop;
      const thisField = field;
      loop = null;
      state("processing");
      const result = await thisLoop.finish();
      stopStream();
      if (thisField) thisField.finalize(result.text, result.parts);
      field = null;
      busy = false;
      state("idle", { transcriptLength: result.text.length });
      return result.text;
    };

    return {
      start,
      stop,
      get recording() {
        return !!loop;
      },
    };
  };

  const api = { engines, createLoop, createField, attach, LIVE_PAUSE_MS };
  root.BramDictation = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
