// Frozen copy of the app/__shell/helpers.js functions the transcript rows
// call, extracted mechanically (transitive closure) at bram commit 019047e.
// Not maintained: this page is a snapshot. Traces are off so nothing here
// ever tries to reach a Bram host.
window.__bramTracesEnabled = false;

window.logToHost = function (payload) {
  // Master-flag short-circuit. Paired with `window.iframeTrace`
  // below. When traces are off, skip the Tauri IPC invoke (the
  // dominant per-event cost). Default-ON so behavior is preserved
  // during the brief startup window before the self-init fetch
  // below resolves the actual setting.
  if (window.__bramTracesEnabled === false) return;
  var invoke = getTauriInvoke();
  if (!invoke) {
    // issue-343: the trace transport must not share its only channel with
    // the commands it witnesses. With the invoke bridge gone (the wedge
    // class #343 reported), fall back to the loopback route so evidence
    // still lands; transport=fetch-fallback marks each line as proof of
    // which channel died. In the cross-origin target pane both channels
    // are refused by design — the catch keeps that case silent, as before.
    try {
      var marked = Object.assign({ transport: "fetch-fallback" }, payload);
      window
        .fetch("/__trace/pane", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(marked),
        })
        .catch(function () {});
    } catch (e) {}
    return;
  }
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

window.__bramIframeTrace = function (subkind, fields) {
  try {
    if (window.__bramTracesEnabled === false) return;
    if (typeof window.logToHost !== "function") return;
    var payload = { kind: "iframe-trace", subkind: subkind, at: new Date().toISOString() };
    if (fields && typeof fields === "object") {
      Object.keys(fields).forEach(function (key) {
        // `kind` and `subkind` are the envelope the host routes on; a field
        // with either name used to overwrite it and send the line to stderr
        // instead of the trace (agent-launch-failure-in-pane, 2026-10-04).
        // Keep the caller's value under a prefixed name.
        var safeKey = key === "kind" || key === "subkind" ? "field_" + key : key;
        payload[safeKey] = window.__bramTraceSafeValue(fields[key], 0);
      });
    }
    window.logToHost(payload);
  } catch (e) {}
};

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

window.__bramRowIntent = function (item) {
  if (!item) return "";
  return item.aiDescription || item.description || item.precedingIntent || "";
};

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

window.__bramIsFeedbackDraftRead = function (toolName, hint) {
  return String(toolName || "") === "Read" &&
    String(hint || "").indexOf("/feedback-drafts/") >= 0;
};

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

window.__bramFreeformResultMode = function (item) {
  if (!item) return "scroll";
  if (window.__bramIsFeedbackDraftRead(item.name, item.summary)) return "flow";
  if (window.__bramIsMcpToolName(item.name)) return "flow";
  if (item.name === "AskUserQuestion") return "flow";
  if (item.resultStructured) return "flow";
  return window.__bramToolResultIsStructuredJson(item.result) ? "flow" : "scroll";
};

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

window.__bramHardBreaks = function (text) {
  if (typeof text !== "string" || text.indexOf("\n") < 0) return text;
  if (/^\s*voice:/.test(text)) return text;
  var lines = text.replace(/\r\n?/g, "\n").split("\n");
  var fence = null; // {ch, len} while inside a fenced code block
  for (var i = 0; i < lines.length - 1; i++) {
    var line = lines[i];
    var m = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (fence) {
      if (m && m[1].charAt(0) === fence.ch && m[1].length >= fence.len &&
          /^\s*$/.test(line.slice(m[0].length))) {
        fence = null;
      }
      continue;
    }
    if (m) {
      fence = { ch: m[1].charAt(0), len: m[1].length };
      continue;
    }
    if (/^\s*$/.test(line) || /^\s*$/.test(lines[i + 1])) continue;
    if (/ {2,}$/.test(line) || /\\$/.test(line)) continue;
    lines[i] = line + "  ";
  }
  return lines.join("\n");
};

window.__bramToggleInArray = function (arr, id) {
  arr = arr || [];
  if (arr.indexOf(id) >= 0) return arr.filter(function (x) { return x !== id; });
  return arr.concat([id]);
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

// Page-local: Bram's version also emits a trace line and requests an AI
// description; off-Bram, opening a row is just the toggle.
window.__bramExpandTool = function (arr, item) {
  return window.__bramToggleInArray(arr, item && item.id);
};
