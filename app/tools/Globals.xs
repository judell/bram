// Slice a file's content into a grep -C style window around a 1-indexed
// target line. Returns [{ line, text, isMatch }, ...]. Used by Context.xmlui
// to render search-hit snippets without re-fetching from the server.
function snippetAroundLine(content, line, context) {
  if (!content || !line) return [];
  const lines = content.split('\n');
  const target = line - 1;
  const ctx = context || 6;
  const start = Math.max(0, target - ctx);
  const end = Math.min(lines.length, target + ctx + 1);
  const slice = [];
  for (let i = start; i < end; i++) {
    slice.push({ line: i + 1, text: lines[i] || '', isMatch: i === target });
  }
  return slice;
}

// Reduce a (potentially huge) turn body to just the paragraphs that
// contain the query (case-insensitive substring). Used by Sessions.xmlui
// after a hit-snippet click so the right pane shows context around the
// match instead of the whole turn. Returns the joined paragraphs (still
// valid Markdown for the Markdown component).
function paragraphsContaining(text, query) {
  if (!text) return '';
  const q = (query || '').trim().toLowerCase();
  if (!q) return text;
  const paragraphs = text.split(/\n{2,}/);
  const hits = paragraphs.filter((p) => p.toLowerCase().includes(q));
  return hits.length > 0 ? hits.join('\n\n') : text;
}

function currentSourceFile(pathname) {
  if (pathname === '/sessions') return 'components/Sessions.xmlui';
  if (pathname === '/') return 'components/Transcript.xmlui';
  if (pathname === '/worklist') return 'components/Workspace.xmlui';
  return 'Main.xmlui';
}

// Past transcripts often contain broken docs.xmlui.org/... URLs (the form the
// xmlui-mcp server reports as Source). The live docs are hosted at
// www.xmlui.org/docs/... with a `reference/` segment for component pages.
// Rewrite on the way to Markdown so links resolve when clicked.
function rewriteXmluiDocUrls(text) {
  if (!text) return text;
  return text
    .replace(/https:\/\/docs\.xmlui\.org\/components\//g, 'https://www.xmlui.org/docs/reference/components/')
    .replace(/https:\/\/docs\.xmlui\.org\//g, 'https://www.xmlui.org/docs/');
}

// XMLUI's Markdown sanitizes file:// URLs and rewrites their anchors into
// non-clickable spans, so we can't get a working file-link out of Markdown.
// Strip the image-source footers from the markdown text and return them as
// a separate array; the Transcript component renders them as inline thumbnails
// and Sessions as XMLUI Links.
function extractImagePaths(text) {
  if (!text) return [];
  const paths = [];
  const re = /\[Image: source: (\/[^\]]+\.(?:png|jpg|jpeg|gif|webp))\]/gi;
  let m;
  while ((m = re.exec(text)) !== null) paths.push(m[1]);
  return paths;
}
function stripImagePaths(text) {
  if (!text) return text;
  return text.replace(/\n*\[Image: source: \/[^\]]+\.(?:png|jpg|jpeg|gif|webp)\]/gi, '');
}

// Same shape as extractImagePaths/stripImagePaths but for GitHub-flavored
// markdown: `![alt](url)` and `<img src="url">`. Used by Issues to mirror
// Sessions' thumbnail-with-fullscreen pattern.
function extractMarkdownImages(text) {
  if (!text) return [];
  const urls = [];
  const md = /!\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
  let m;
  while ((m = md.exec(text)) !== null) urls.push(m[1]);
  const html = /<img\b[^>]*\bsrc=["']([^"']+)["'][^>]*>/gi;
  while ((m = html.exec(text)) !== null) urls.push(m[1]);
  return urls;
}
function stripMarkdownImages(text) {
  if (!text) return text;
  return text
    .replace(/\n*!\[[^\]]*\]\([^)\s]+(?:\s+"[^"]*")?\)/g, '')
    .replace(/\n*<img\b[^>]*\bsrc=["'][^"']+["'][^>]*>/gi, '');
}

// True when the most recent textful turn in the session is a user turn —
// i.e. the user has spoken (or a worklist button submitted via toTurn) but
// the assistant has not yet emitted text. tool_use-only assistant records
// and tool_result-only user records are skipped so a long tool cycle still
// reads as "waiting". Used by Transcript's "agent is thinking" spinner.
function isWaitingForAssistant(jsonlText) {
  if (!jsonlText) return false;
  const lines = jsonlText.split('\n');
  let lastRole = null;
  for (const line of lines) {
    if (!line) continue;
    let r;
    try { r = JSON.parse(line); } catch (e) { continue; }
    if (r.type === 'user' && r.message && r.message.content) {
      const content = r.message.content;
      if (Array.isArray(content) && content.length > 0 &&
          content.every(c => c && c.type === 'tool_result')) continue;
      lastRole = 'user';
    } else if (r.type === 'assistant' && r.message && r.message.content) {
      const content = r.message.content;
      if (typeof content === 'string') {
        lastRole = 'assistant';
      } else if (Array.isArray(content) && content.some(c => c && c.type === 'text')) {
        lastRole = 'assistant';
      }
    } else if (r.type === 'event_msg' && r.payload) {
      if (r.payload.type === 'user_message') lastRole = 'user';
      if (r.payload.type === 'agent_message') lastRole = 'assistant';
    }
  }
  return lastRole === 'user';
}

// True when the most recent meaningful record signals the agent has FINISHED
// its turn (not just emitted a first text response). isWaitingForAssistant
// flips false the moment the assistant says anything, which is too early for
// UX surfaces that want to dim through the whole turn — mid-turn narration
// would clear the dim while the agent is still working. This helper walks
// for the real turn-boundary markers each provider emits:
//   Claude: assistant record with `message.stop_reason: "end_turn"`. Records
//     with stop_reason: "tool_use" (mid-turn narration that ends in a tool
//     call) keep the state as `assistant_busy`, not idle.
//   Codex:  event_msg with `payload.type: "task_complete"`. Codex emits this
//     as a separate event at the end of every turn (verified against a live
//     session JSONL alongside task_started / user_message / agent_message).
// The state machine: 'user' → busy (turn just started or mid-tool), 'assistant_busy'
// → busy (mid-turn narration), 'idle' → turn-end marker seen. Returns true
// only when we end on 'idle'.
function isAgentIdle(jsonlText) {
  if (!jsonlText) return false;
  const lines = jsonlText.split('\n');
  let lastState = null;
  for (const line of lines) {
    if (!line) continue;
    let r;
    try { r = JSON.parse(line); } catch (e) { continue; }
    if (r.type === 'user' && r.message && r.message.content) {
      const content = r.message.content;
      if (Array.isArray(content) && content.length > 0 &&
          content.every(c => c && c.type === 'tool_result')) continue;
      lastState = 'user';
    } else if (r.type === 'assistant' && r.message && r.message.content) {
      const content = r.message.content;
      const hasText = (typeof content === 'string') ||
        (Array.isArray(content) && content.some(c => c && c.type === 'text'));
      if (!hasText) continue;
      lastState = r.message.stop_reason === 'end_turn' ? 'idle' : 'assistant_busy';
    } else if (r.type === 'event_msg' && r.payload) {
      if (r.payload.type === 'user_message') lastState = 'user';
      else if (r.payload.type === 'agent_message') lastState = 'assistant_busy';
      else if (r.payload.type === 'task_complete') lastState = 'idle';
    }
  }
  return lastState === 'idle';
}

// Iframe-side trace helper for the [iframe] category of the comms-path
// trace log (issue #49). Forwards a structured record to the host's
// `log_from_right_pane` Tauri command, which routes records whose
// `kind` is `"iframe-trace"` into resources/bram-trace.log when
// BRAM_TRACE=1 is set on the host. No-op when logToHost isn't wired up.
// subkind is a token from the spec's maintained vocabulary (click,
// inflight-set, inflight-clear, listener-fired, ...); fields are
// arbitrary per-event metadata (target, item, reason, paths, etc.).
function iframeTrace(subkind, fields) {
  try {
    if (typeof logToHost !== 'function') return;
    const payload = { kind: 'iframe-trace', subkind: subkind, at: new Date().toISOString() };
    if (fields && typeof fields === 'object') {
      Object.assign(payload, fields);
    }
    logToHost(payload);
  } catch (e) {}
}

// Clean a user turn for transcript display: strip protocol prefixes
// (`voice: `, `talk: `) so spoken / typed content reads as plain text;
// summarize structured `approved:` / `drop:` payloads to a one-line
// glyph + count instead of dumping JSON. Anything else passes through.
function formatUserTurnForTranscript(text) {
  if (!text) return '';
  const stripped = text.replace(/^(voice|talk):\s*/, '');
  if (stripped !== text) return stripped;
  const m = text.match(/^(approved|drop):\s*(.*)$/s);
  if (m) {
    const kind = m[1];
    try {
      const data = JSON.parse(m[2]);
      if (kind === 'approved') {
        const n = (data.items || []).length;
        return '✓ Approved ' + n + ' item' + (n === 1 ? '' : 's');
      }
      const n = (data.ids || []).length;
      return '✕ Dropped ' + n + ' item' + (n === 1 ? '' : 's');
    } catch (e) {
      return text;
    }
  }
  return text;
}

// Compact one-line summary for a tool_use block. Falls back to the tool
// name when the input shape is unfamiliar.
function toolSummary(name, input) {
  if (!input || typeof input !== 'object') return name || '';
  if (name === 'Edit' || name === 'MultiEdit') {
    return (input.file_path || '') + ' edited';
  }
  if (name === 'Write') {
    const lines = (input.content || '').split('\n').length;
    return (input.file_path || '') + ' — wrote ' + lines + ' line' + (lines === 1 ? '' : 's');
  }
  if (name === 'Bash') {
    const cmd = input.command || '';
    return cmd.length > 80 ? cmd.slice(0, 80) + '…' : cmd;
  }
  if (name === 'Read') {
    let s = input.file_path || '';
    if (input.offset || input.limit) {
      const start = input.offset || 1;
      s += ':' + start;
      if (input.limit) s += '-' + (start + input.limit - 1);
    }
    return s;
  }
  if (name === 'Grep' || name === 'Glob') {
    return (input.pattern || '') + (input.path ? ' in ' + input.path : '');
  }
  if (name === 'Task' || name === 'Agent') {
    return (input.subagent_type || '') + (input.description ? ' — ' + input.description : '');
  }
  return name || '';
}

function parseJsonString(value) {
  if (typeof value !== 'string') return null;
  try {
    return JSON.parse(value);
  } catch (e) {
    return null;
  }
}

function codexToolName(payload) {
  if (!payload) return '';
  if (payload.namespace) return payload.namespace.replace(/^mcp__/, '') + '.' + (payload.name || '');
  return payload.name || '';
}

function codexToolInput(payload) {
  if (!payload) return {};
  if (payload.type === 'function_call') {
    const parsed = parseJsonString(payload.arguments);
    return parsed !== null ? parsed : (payload.arguments || {});
  }
  if (payload.type === 'custom_tool_call') {
    const parsed = parseJsonString(payload.input);
    return parsed !== null ? parsed : (payload.input || '');
  }
  return {};
}

function codexToolSummary(payload) {
  if (!payload) return '';
  const name = codexToolName(payload);
  const input = codexToolInput(payload);
  if (payload.name === 'exec_command' && input && typeof input === 'object' && input.cmd) {
    return input.cmd.length > 80 ? input.cmd.slice(0, 80) + '…' : input.cmd;
  }
  if (payload.name === 'write_stdin' && input && typeof input === 'object') {
    const chars = input.chars || '';
    const session = input.session_id ? ('session ' + input.session_id) : 'stdin';
    if (!chars) return session;
    const label = chars === '\u001b' ? 'Esc' : chars.replace(/\r/g, '\\r').replace(/\n/g, '\\n');
    return session + ' ← ' + (label.length > 40 ? label.slice(0, 40) + '…' : label);
  }
  if (payload.name === 'apply_patch' && typeof input === 'string') {
    const m = input.match(/\*\*\* (?:Add|Update|Delete) File: ([^\n]+)/);
    return m ? (m[1] + ' patch') : 'patch';
  }
  if (name.startsWith('filesystem.') && input && typeof input === 'object' && input.path) {
    return input.path;
  }
  if (name.startsWith('xmlui.') && input && typeof input === 'object') {
    return input.path || input.component || input.query || name;
  }
  if (input && typeof input === 'object') return toolSummary(payload.name || name, input);
  return name;
}

// Synthetic diff for an Edit/MultiEdit tool_use input. Returns one entry
// per line, prefixed sign + kind so the renderer can tint accordingly.
function editDiffLines(input) {
  if (!input) return [];
  const oldLines = (input.old_string || '').split('\n');
  const newLines = (input.new_string || '').split('\n');
  const out = [];
  for (const line of oldLines) out.push({ sign: '-', kind: 'del', text: line });
  for (const line of newLines) out.push({ sign: '+', kind: 'add', text: line });
  return out;
}

// Aggregate the file-modifying tool calls in the most recent turn. A
// "turn" boundary is the most recent user message; all assistant
// tool_use entries after it that touch files (Edit / MultiEdit / Write)
// belong to the current turn. Group by file_path; multiple edits to the
// same file accumulate in chronological order.
//
// Returns: [{
//   filePath: string,
//   kind: 'edited' | 'multi-edited' | 'written' | 'mixed',
//   edits: [{ kind: 'edit' | 'write', before, after }],
//   added: int,
//   removed: int,
// }]
//
// Empty array when no current-turn file mutations exist. Skips Read,
// Bash, Grep, Glob, and all other read-only tools.
function currentTurnEdits(jsonlText) {
  if (!jsonlText) return currentTurnEdits._cache || [];
  if (currentTurnEdits._cacheKey === jsonlText && currentTurnEdits._cache) {
    return currentTurnEdits._cache;
  }
  // Fast path: skip parsing entirely if the text has no tool-call
  // signal of either provider. The expensive split + per-line
  // JSON.parse below would otherwise run on every render against the
  // full JSONL tail. Takes the common case (idle session, no in-flight
  // edits) from O(lines) to a single O(text.length) byte scan.
  // - "tool_use" -> Claude assistant content[*].type
  // - "function_call" / "custom_tool_call" -> Codex response_item payload types
  if (jsonlText.indexOf('"tool_use"') === -1 &&
      jsonlText.indexOf('"function_call"') === -1 &&
      jsonlText.indexOf('"custom_tool_call"') === -1) {
    currentTurnEdits._cacheKey = jsonlText;
    currentTurnEdits._cache = [];
    return currentTurnEdits._cache;
  }

  // Walk lines in chronological order to find the index of the most
  // recent user-message line (i.e., the boundary between previous turn
  // and current turn). All assistant tool-use entries AFTER that index
  // belong to the in-flight turn. Handles both Claude (`type:"user"`
  // with non-tool_result content) and Codex (`type:"event_msg"`,
  // `payload.type:"user_message"`).
  const lines = jsonlText.split('\n');
  let lastUserIdx = -1;
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!lines[i]) continue;
    let r;
    try { r = JSON.parse(lines[i]); } catch (e) { continue; }

    // Claude
    if (r.type === 'user' && r.message && r.message.content) {
      const content = r.message.content;
      // Skip user-role turns that are tool_result wrappers — those are
      // tool outputs, not actual user messages.
      if (Array.isArray(content) && content.length > 0 &&
          content.every(c => c && c.type === 'tool_result')) {
        continue;
      }
      lastUserIdx = i;
      break;
    }

    // Codex
    if (r.type === 'event_msg' && r.payload && r.payload.type === 'user_message') {
      lastUserIdx = i;
      break;
    }
  }

  // Pass over lines AFTER the user boundary, collecting both Claude
  // tool_use blocks and Codex function_call / custom_tool_call records.
  // Group by file_path.
  const byFile = {};
  const order = [];
  const start = lastUserIdx + 1;

  function ensureBucket(filePath) {
    if (!byFile[filePath]) {
      byFile[filePath] = {
        filePath: filePath,
        kind: null,
        edits: [],
        added: 0,
        removed: 0,
        lastToolId: null,
      };
      order.push(filePath);
    }
    return byFile[filePath];
  }

  for (let i = start; i < lines.length; i++) {
    if (!lines[i]) continue;
    let r;
    try { r = JSON.parse(lines[i]); } catch (e) { continue; }

    // ---------- Codex branch ----------
    if (r.type === 'response_item' && r.payload) {
      const p = r.payload;
      if (p.type !== 'function_call' && p.type !== 'custom_tool_call') continue;
      // Only apply_patch is a file mutator in standard Codex; MCP
      // filesystem.* tools (if used) could be added here later.
      if (p.name !== 'apply_patch') continue;
      const input = codexToolInput(p);
      const patchText = typeof input === 'string' ? input : '';
      if (!patchText) continue;

      // Parse the patch text into per-file sections. Format:
      //   *** Add|Update|Delete File: <path>
      //   <hunk lines: ' ' context, '+' add, '-' remove, '@@ hunk header'>
      //   *** End Patch
      // Counts: each '+' line is +1 added, each '-' line is +1 removed.
      // We skip diff context (' '), hunk headers ('@@'), and the
      // patch-format header lines ('*** ...').
      let current = null;
      const patchLines = patchText.split('\n');
      for (const pl of patchLines) {
        const m = pl.match(/^\*\*\* (Add|Update|Delete) File: (.+)$/);
        if (m) {
          current = ensureBucket(m[2].trim());
          const action = m[1].toLowerCase();
          current.kind = current.kind
            ? (current.kind === action + 'ed' ? current.kind : 'mixed')
            : (action === 'add' ? 'added' : action === 'delete' ? 'deleted' : 'updated');
          if (p.call_id) current.lastToolId = p.call_id;
          continue;
        }
        if (pl === '*** End Patch' || pl.startsWith('*** ')) { current = null; continue; }
        if (!current) continue;
        if (pl.startsWith('+') && !pl.startsWith('+++')) current.added += 1;
        else if (pl.startsWith('-') && !pl.startsWith('---')) current.removed += 1;
      }
      continue;
    }

    // ---------- Claude branch ----------
    if (r.type !== 'assistant' || !r.message || !r.message.content) continue;
    const content = r.message.content;
    if (!Array.isArray(content)) continue;
    for (const c of content) {
      if (!c || c.type !== 'tool_use') continue;
      const name = c.name;
      const input = c.input || {};
      const filePath = input.file_path;
      if (!filePath) continue;
      if (name !== 'Edit' && name !== 'MultiEdit' && name !== 'Write') continue;

      const bucket = ensureBucket(filePath);
      // Track the most recent tool_use_id that touched this file so the
      // footer row can deep-link into the existing tool-detail modal.
      if (c.id) bucket.lastToolId = c.id;

      if (name === 'Edit') {
        const before = input.old_string || '';
        const after = input.new_string || '';
        bucket.edits.push({ kind: 'edit', before: before, after: after });
        bucket.removed += (before ? before.split('\n').length : 0);
        bucket.added += (after ? after.split('\n').length : 0);
        bucket.kind = bucket.kind ? (bucket.kind === 'edited' ? 'edited' : 'mixed') : 'edited';
      } else if (name === 'MultiEdit') {
        const subEdits = Array.isArray(input.edits) ? input.edits : [];
        for (const e of subEdits) {
          if (!e) continue;
          const before = e.old_string || '';
          const after = e.new_string || '';
          bucket.edits.push({ kind: 'edit', before: before, after: after });
          bucket.removed += (before ? before.split('\n').length : 0);
          bucket.added += (after ? after.split('\n').length : 0);
        }
        bucket.kind = bucket.kind ? (bucket.kind === 'multi-edited' ? 'multi-edited' : 'mixed') : 'multi-edited';
      } else if (name === 'Write') {
        const after = input.content || '';
        bucket.edits.push({ kind: 'write', before: null, after: after });
        bucket.added += (after ? after.split('\n').length : 0);
        // No removed count — we don't have prior content. If this file
        // also got Edited earlier in the turn, kind becomes 'mixed'.
        bucket.kind = bucket.kind ? (bucket.kind === 'written' ? 'written' : 'mixed') : 'written';
      }
    }
  }

  const result = order.map(fp => byFile[fp]);
  currentTurnEdits._cacheKey = jsonlText;
  currentTurnEdits._cache = result;
  return result;
}

// First N lines of a Write tool's content, plus the leftover count for
// the truncation footer.
function writeBodyLines(input, maxLines) {
  const cap = maxLines || 20;
  if (!input || !input.content) return { lines: [], remaining: 0 };
  const all = input.content.split('\n');
  return { lines: all.slice(0, cap), remaining: Math.max(0, all.length - cap) };
}

// Pretty-print arbitrary tool input as JSON, truncated to N lines.
function toolInputJsonLines(input, maxLines) {
  const cap = maxLines || 20;
  if (input === null || input === undefined) return { lines: [], remaining: 0 };
  if (typeof input === 'string') {
    const all = input.split('\n');
    return { lines: all.slice(0, cap), remaining: Math.max(0, all.length - cap) };
  }
  let json;
  try {
    json = JSON.stringify(input, null, 2);
  } catch (e) {
    return { lines: ['(unserializable input)'], remaining: 0 };
  }
  const all = json.split('\n');
  return { lines: all.slice(0, cap), remaining: Math.max(0, all.length - cap) };
}

// Concatenate the text content of a tool_result block (handles both
// string and array-of-blocks shapes).
function toolResultText(content) {
  if (!content) return '';
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    return content
      .filter(c => c && c.type === 'text')
      .map(c => c.text || '')
      .join('\n');
  }
  return '';
}

// True if a tool_result block carries an error (either flagged via
// is_error or detected by an Error:/<tool_use_error> prefix). Used to
// tint the inline result banner red.
function isErrorResult(block) {
  if (!block) return false;
  if (block.is_error) return true;
  const text = toolResultText(block.content);
  return text.startsWith('Error:') || text.startsWith('<tool_use_error>');
}

// First N lines of a tool_result, plus the leftover count. Returns null
// if the result has no text content.
function extractToolResult(block, maxLines) {
  const cap = maxLines || 20;
  const text = toolResultText(block && block.content);
  if (!text) return null;
  const all = text.split('\n');
  return { lines: all.slice(0, cap), remaining: Math.max(0, all.length - cap) };
}

function extractTextResult(text, maxLines) {
  const cap = maxLines || 20;
  if (!text) return null;
  const all = text.split('\n');
  return { lines: all.slice(0, cap), remaining: Math.max(0, all.length - cap) };
}

function codexToolOutput(payload) {
  if (!payload || (payload.type !== 'function_call_output' && payload.type !== 'custom_tool_call_output')) {
    return null;
  }
  const raw = payload.output;
  if (typeof raw !== 'string') return { text: '', errored: false };
  const parsed = parseJsonString(raw);
  if (parsed && typeof parsed === 'object') {
    const text = typeof parsed.output === 'string'
      ? parsed.output
      : typeof parsed.stderr === 'string'
        ? parsed.stderr
        : raw;
    const exitCode = parsed.metadata && typeof parsed.metadata.exit_code === 'number'
      ? parsed.metadata.exit_code
      : null;
    return { text, errored: exitCode !== null && exitCode !== 0 };
  }
  const exitMatch = raw.match(/Process exited with code (\d+)/);
  const exitCode = exitMatch ? parseInt(exitMatch[1], 10) : 0;
  return { text: raw, errored: !!exitMatch && exitCode !== 0 };
}

// Find a tool entry by id across all turns. Used by Transcript to render the
// open-tool modal — we keep `openToolId` as the source of truth and
// derive the entry from it on each render.
function findToolInTurns(turns, toolId) {
  if (!turns || !toolId) return null;
  for (const t of turns) {
    if (!t || !t.entries) continue;
    for (const e of t.entries) {
      if (e && e.kind === 'tool' && e.id === toolId) return e;
    }
  }
  return null;
}

// Scan the JSONL once to recover the full input + result for a single
// tool by id. sessionTurns no longer carries this data — fetching it on
// expand keeps polling cheap and avoids ballooning the in-memory turn
// list with full Write/Read contents.
function getToolDetail(jsonlText, toolId) {
  if (!jsonlText || !toolId) return null;
  let input = null;
  let result = null;
  for (const line of jsonlText.split('\n')) {
    if (!line) continue;
    let r;
    try { r = JSON.parse(line); } catch (e) { continue; }
    if (r.message && r.message.content && Array.isArray(r.message.content)) {
      for (const c of r.message.content) {
        if (!c) continue;
        if (c.type === 'tool_use' && c.id === toolId) {
          input = c.input || {};
        } else if (c.type === 'tool_result' && c.tool_use_id === toolId) {
          result = extractToolResult(c, 20);
        }
      }
    } else if (r.type === 'response_item' && r.payload) {
      const p = r.payload;
      if ((p.type === 'function_call' || p.type === 'custom_tool_call') && p.call_id === toolId) {
        input = codexToolInput(p);
      } else if ((p.type === 'function_call_output' || p.type === 'custom_tool_call_output') && p.call_id === toolId) {
        const output = codexToolOutput(p);
        result = extractTextResult(output && output.text, 20);
      }
    }
    if (input !== null && result !== null) break;
  }
  return { input: input || {}, result };
}

// Shallow turn equality: enough to tell "unchanged turn" from
// "changed/new" without doing a full JSON.stringify. Used by sessionTurns
// to preserve object refs for stable turns so XMLUI's Items doesn't
// re-mount the whole list on every poll.
function turnsLooselyEqual(a, b) {
  if (!a || !b) return false;
  if (a.role !== b.role) return false;
  if (a.text !== b.text) return false;
  const ae = a.entries || [], be = b.entries || [];
  if (ae.length !== be.length) return false;
  for (let i = 0; i < ae.length; i++) {
    const x = ae[i], y = be[i];
    if (!x || !y) return false;
    if (x.kind !== y.kind) return false;
    if (x.kind === 'text') {
      if (x.text !== y.text) return false;
    } else {
      // tool: id is stable, errored/result may change between polls
      if (x.id !== y.id) return false;
      if (!!x.errored !== !!y.errored) return false;
    }
  }
  const ai = a.images || [], bi = b.images || [];
  if (ai.length !== bi.length) return false;
  return true;
}

// Return the last N turns, reusing the previous result by reference when
// every visible turn is still the same object. `sessionTurns` already
// preserves stable refs across polls, so on a steady-state idle session
// every element of `prev` and `cur` matches and we hand back the same
// array — XMLUI's Items can then skip remounting the visible list.
function visibleTurns(turns, n) {
  if (!turns || !n) return visibleTurns._cacheValue || [];
  const start = Math.max(0, turns.length - n);
  const prev = visibleTurns._cacheValue;
  if (prev && prev.length === turns.length - start) {
    let same = true;
    for (let i = 0; i < prev.length; i++) {
      if (prev[i] !== turns[start + i]) { same = false; break; }
    }
    if (same) return prev;
  }
  const out = turns.slice(start);
  visibleTurns._cacheValue = out;
  return out;
}

// Extract just the text of the most recent assistant turn from a session
// JSONL tail. Used by Workspace's inline Agent-response panel — a slim
// "final response" readout next to the worklist, without reproducing
// Transcript's full timeline / tool-call rendering. Returns '' if no
// assistant text turn is present in the tail.
function lastAssistantText(jsonlText) {
  const turns = sessionTurns(jsonlText);
  for (let i = turns.length - 1; i >= 0; i--) {
    if (turns[i].role === 'assistant') {
      const text = (turns[i].entries || [])
        .filter(e => e.kind === 'text')
        .map(e => e.text)
        .join('\n\n');
      if (text) return text;
    }
  }
  return '';
}

function sessionTurns(jsonlText) {
  // Sticky empty: during a refetch the DataSource value can briefly be
  // null/undefined. Returning [] would blank the transcript and cause a
  // dramatic flash; instead, hold the last result until the new value
  // arrives.
  if (!jsonlText) return sessionTurns._cacheValue || [];
  // Function-property memoization: skip the reparse when the polled
  // JSONL hasn't changed since last call. Identity comparison is enough
  // because the DataSource hands us a fresh string only when the file
  // actually grew.
  if (sessionTurns._cacheKey === jsonlText && sessionTurns._cacheValue) {
    return sessionTurns._cacheValue;
  }
  // Instrumentation: log cache-miss parses. Tracks how often we do real
  // work and how long it takes.
  const _t0 = (typeof performance !== 'undefined') ? performance.now() : 0;
  sessionTurns._parseCount = (sessionTurns._parseCount || 0) + 1;
  const turns = [];
  // tool_use_id → entry, so a later user-turn tool_result can flag the
  // originating tool as errored.
  const toolIndex = {};
  for (const line of jsonlText.split('\n')) {
    if (!line) continue;
    let r;
    try { r = JSON.parse(line); } catch (e) { continue; }
    let role = null;
    const entries = [];
    const inlineImages = [];
    if (r.type === 'user' || r.type === 'assistant') {
      if (!r.message || !r.message.content) continue;
      role = r.type;
      const content = r.message.content;
      if (typeof content === 'string') {
        if (content) entries.push({ kind: 'text', text: content });
      } else if (Array.isArray(content)) {
        for (const c of content) {
          if (!c) continue;
          if (c.type === 'text' && c.text) {
            entries.push({ kind: 'text', text: c.text });
          } else if (c.type === 'tool_use') {
            // Keep entries lightweight — only what the collapsed row
            // needs. Full input/result are fetched on expand via
            // getToolDetail.
            const entry = {
              kind: 'tool',
              id: c.id,
              name: c.name,
              summary: toolSummary(c.name, c.input || {}),
            };
            entries.push(entry);
            if (c.id) toolIndex[c.id] = entry;
          } else if (c.type === 'tool_result') {
            const matching = c.tool_use_id && toolIndex[c.tool_use_id];
            if (matching) {
              matching.errored = isErrorResult(c);
              if (matching.errored) {
                const txt = toolResultText(c.content);
                matching.errorText = txt.split('\n')[0].slice(0, 200);
              }
            }
          } else if (c.type === 'image' && c.source && c.source.type === 'base64' && c.source.data) {
            const mt = c.source.media_type || 'image/png';
            inlineImages.push('data:' + mt + ';base64,' + c.source.data);
          }
        }
      }
    } else if (r.type === 'event_msg' && r.payload) {
      if (r.payload.type === 'user_message') role = 'user';
      if (r.payload.type === 'agent_message') role = 'assistant';
      const t = r.payload.message || '';
      if (t) entries.push({ kind: 'text', text: t });
    } else if (r.type === 'response_item' && r.payload) {
      const p = r.payload;
      if (p.type === 'function_call' || p.type === 'custom_tool_call') {
        role = 'assistant';
        const entry = {
          kind: 'tool',
          id: p.call_id,
          name: codexToolName(p),
          summary: codexToolSummary(p),
        };
        entries.push(entry);
        if (p.call_id) toolIndex[p.call_id] = entry;
      } else if (p.type === 'function_call_output' || p.type === 'custom_tool_call_output') {
        const matching = p.call_id && toolIndex[p.call_id];
        if (matching) {
          const output = codexToolOutput(p);
          matching.errored = !!(output && output.errored);
          if (output && output.text) {
            const firstLine = output.text.split('\n')[0].slice(0, 200);
            if (matching.errored) matching.errorText = firstLine;
          }
        }
      }
    }
    if (!role) continue;
    if (entries.length === 0 && inlineImages.length === 0) continue;
    // Capture image paths from the ORIGINAL text before stripping — strip
    // and extract operate on the same patterns, so we have to read before
    // we clean. (Was previously running extract on already-stripped text,
    // which made the [Image: source: ...] fallback dead code.)
    const originalJoined = entries.filter(e => e.kind === 'text').map(e => e.text).join('\n\n');
    const pathsFromText = extractImagePaths(originalJoined);
    // Apply text rewrites + strip image-path footers from text entries.
    for (const e of entries) {
      if (e.kind === 'text') {
        e.text = stripImagePaths(rewriteXmluiDocUrls(e.text));
      }
    }
    const textJoined = entries.filter(e => e.kind === 'text').map(e => e.text).join('\n\n');
    // Skip user turns that are pure image-path bookkeeping (preserved from prior behavior).
    if (role === 'user' && inlineImages.length === 0 && entries.every(e => e.kind === 'text')
        && /^(\[Image: source: [^\]]+\]\s*)+$/.test(originalJoined.trim())) continue;
    // After tool_result filtering, a user turn may have nothing left.
    if (entries.length === 0 && inlineImages.length === 0) continue;
    turns.push({
      role,
      text: textJoined,
      entries,
      images: inlineImages.length > 0 ? inlineImages : pathsFromText,
    });
  }
  // Structural-share with the previous result: for each turn that's
  // structurally equal to the previous turn at the same index, reuse
  // the previous reference. XMLUI's reactivity treats reference
  // equality as "unchanged", so the Items in Transcript skips re-mounting
  // those turns — eliminating the per-poll flash. JSONL is append-only
  // in practice, so the first N-K turns are typically identical and
  // only the last few are new or growing.
  const prev = sessionTurns._cacheValue || [];
  for (let i = 0; i < turns.length && i < prev.length; i++) {
    if (turnsLooselyEqual(turns[i], prev[i])) {
      turns[i] = prev[i];
    } else {
      break;
    }
  }
  sessionTurns._cacheKey = jsonlText;
  sessionTurns._cacheValue = turns;
  if (_t0) {
    const _elapsed = performance.now() - _t0;
    if (_elapsed > 5) {
      try {
        logToHost({
          kind: 'sessionTurns-parse',
          ms: Math.round(_elapsed),
          len: jsonlText.length,
          turns: turns.length,
          n: sessionTurns._parseCount,
        });
      } catch (e) {}
    }
  }
  return turns;
}

// Worklist close-issue dialog state helpers. The dialog opens when a TO COMMIT
// item carries closesIssues: [N, ...]. State shape is { <issueNumber>: { close,
// comment } } so per-issue checkbox + comment edits update one branch without
// disturbing the rest. Immutable updates so XMLUI's reactivity refreshes.
function initCloseIssueState(closesIssues) {
  const state = {};
  for (const entry of (closesIssues || [])) {
    const n = (entry && typeof entry === 'object') ? entry.number : entry;
    state[n] = { close: true, comment: '' };
  }
  return state;
}
function setCloseIssueClose(state, n, close) {
  const prev = (state && state[n]) || { close: true, comment: '' };
  return Object.assign({}, state || {}, { [n]: Object.assign({}, prev, { close: !!close }) });
}
function setCloseIssueComment(state, n, comment) {
  const prev = (state && state[n]) || { close: true, comment: '' };
  return Object.assign({}, state || {}, { [n]: Object.assign({}, prev, { comment: comment || '' }) });
}
// Produce the `close-issue:` lines the agent reads out of the approved
// payload's feedback. Lines look like `close-issue: 52` or
// `close-issue: 52 comment: "shipped"`. JSON.stringify on the comment keeps
// embedded quotes / newlines unambiguous for the agent's parse.
function buildCloseIssueLines(state) {
  const lines = [];
  for (const key of Object.keys(state || {})) {
    const v = state[key];
    if (!v || !v.close) continue;
    const c = (v.comment || '').trim();
    if (c) lines.push('close-issue: ' + key + ' comment: ' + JSON.stringify(c));
    else lines.push('close-issue: ' + key);
  }
  return lines;
}
// Merge user-typed feedback with the dialog-generated close-issue lines.
// Empty base + no lines → empty string; otherwise lines come after the user's
// text separated by a blank line so the agent can split on `\n\n`.
function combineFeedbackWithCloseLines(base, lines) {
  const baseTrim = (base || '').trim();
  if (!lines || lines.length === 0) return baseTrim;
  if (!baseTrim) return lines.join('\n');
  return baseTrim + '\n\n' + lines.join('\n');
}

