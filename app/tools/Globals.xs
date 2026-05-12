// Reactive global: when true, the Talk page hides its spinner+Esc inline
// row (the user has already pressed Esc and the response was interrupted).
// Reset to false by Talk's ChangeListener on the user-submission count, so
// the next genuine submission re-enables the spinner.
var escSuppressed = false;

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

function currentSourceFile(pathname) {
  if (pathname === '/sessions') return 'components/Sessions.xmlui';
  if (pathname === '/') return 'components/Workspace.xmlui';
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
// a separate array; the Sessions component renders them as XMLUI Links.
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

// True when the latest JSONL record is an assistant tool_use without a
// following tool_result — i.e. Claude Code is showing the numbered
// permission prompt and waiting for approval. Used to gate the Yes/No
// menu-navigation buttons on Talk.
function isAtToolApproval(jsonlText) {
  if (!jsonlText) return false;
  const lines = jsonlText.split('\n').filter(l => l);
  for (let i = lines.length - 1; i >= 0; i--) {
    let r;
    try { r = JSON.parse(lines[i]); } catch (e) { continue; }
    if (r.type === 'assistant' && r.message && r.message.content) {
      const content = r.message.content;
      if (Array.isArray(content) &&
          content.length > 0 &&
          content.some(c => c && c.type === 'tool_use') &&
          !content.some(c => c && c.type === 'text')) {
        return true;
      }
      return false;
    }
    if (r.type === 'user' && r.message && r.message.content) {
      return false;
    }
  }
  return false;
}

// True when the most recent textful turn in the session is a user turn —
// i.e. the user has spoken (or a worklist button submitted via toTurn) but
// the assistant has not yet emitted text. tool_use-only assistant records
// and tool_result-only user records are skipped so a long tool cycle still
// reads as "waiting".
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

function lastAssistantText(jsonlText) {
  if (!jsonlText) return '';
  const lines = jsonlText.split('\n');
  let lastClaude = null;
  let lastCodex = '';
  for (const line of lines) {
    if (!line) continue;
    try {
      const r = JSON.parse(line);
      if (r.type === 'assistant' && r.message && r.message.content) {
        lastClaude = r;
      } else if (r.type === 'event_msg' && r.payload && r.payload.type === 'agent_message') {
        lastCodex = r.payload.message || '';
      }
    } catch (e) {}
  }
  if (lastCodex) return rewriteXmluiDocUrls(lastCodex);
  if (!lastClaude) return '';
  const content = lastClaude.message.content;
  if (typeof content === 'string') return rewriteXmluiDocUrls(content);
  return rewriteXmluiDocUrls(
    (Array.isArray(content) ? content : [])
      .filter(c => c && c.type === 'text')
      .map(c => c.text)
      .join('\n\n')
  );
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

// When the agent is paused waiting on a tool-use permission decision
// (same state isAtToolApproval flags), return a description of the
// menu we can render in Talk: tool name + summary + the standard three
// choices that send keystrokes back to the PTY. Returns null otherwise.
// Walks JSONL from the end so it stops as soon as it finds the
// relevant record.
function pendingPermissionMenu(jsonlText) {
  if (!jsonlText) return null;
  const lines = jsonlText.split('\n').filter(l => l);
  for (let i = lines.length - 1; i >= 0; i--) {
    let r;
    try { r = JSON.parse(lines[i]); } catch (e) { continue; }
    if (r.type === 'assistant' && r.message && r.message.content) {
      const content = r.message.content;
      if (!Array.isArray(content) || content.length === 0) return null;
      const hasText = content.some(c => c && c.type === 'text');
      const hasToolUse = content.some(c => c && c.type === 'tool_use');
      // tool_use + no text = pending permission. tool_use + text = the
      // agent already explained and now there's nothing to decide.
      if (hasText || !hasToolUse) return null;
      const firstToolUse = content.find(c => c && c.type === 'tool_use');
      const summary = toolSummary(firstToolUse.name, firstToolUse.input || {});
      return {
        toolName: firstToolUse.name,
        toolSummary: summary,
        toolInput: firstToolUse.input || {},
        choices: [
          { key: '1', label: 'Yes' },
          { key: '2', label: "Yes, allow and don't ask again" },
          { key: '3', label: 'No, and tell Claude what to do' },
        ],
      };
    }
    if (r.type === 'user' && r.message && r.message.content) {
      return null;
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
    if (!r.message || !r.message.content || !Array.isArray(r.message.content)) continue;
    for (const c of r.message.content) {
      if (!c) continue;
      if (c.type === 'tool_use' && c.id === toolId) {
        input = c.input || {};
      } else if (c.type === 'tool_result' && c.tool_use_id === toolId) {
        result = extractToolResult(c, 20);
      }
    }
    if (input !== null && result !== null) break;
  }
  return { input: input || {}, result };
}

function sessionTurns(jsonlText) {
  if (!jsonlText) return [];
  // Function-property memoization: skip the reparse when the polled
  // JSONL hasn't changed since last call. Identity comparison is enough
  // because the DataSource hands us a fresh string only when the file
  // actually grew.
  if (sessionTurns._cacheKey === jsonlText && sessionTurns._cacheValue) {
    return sessionTurns._cacheValue;
  }
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
    }
    if (!role) continue;
    if (entries.length === 0 && inlineImages.length === 0) continue;
    // Apply text rewrites + strip image-path footers from text entries.
    for (const e of entries) {
      if (e.kind === 'text') {
        e.text = stripImagePaths(rewriteXmluiDocUrls(e.text));
      }
    }
    const textJoined = entries.filter(e => e.kind === 'text').map(e => e.text).join('\n\n');
    // Skip user turns that are pure image-path bookkeeping (preserved from prior behavior).
    if (role === 'user' && inlineImages.length === 0 && entries.every(e => e.kind === 'text')
        && /^(\[Image: source: [^\]]+\]\s*)+$/.test(textJoined.trim())) continue;
    // After tool_result filtering, a user turn may have nothing left.
    if (entries.length === 0 && inlineImages.length === 0) continue;
    turns.push({
      role,
      text: textJoined,
      entries,
      images: inlineImages.length > 0 ? inlineImages : extractImagePaths(textJoined),
    });
  }
  sessionTurns._cacheKey = jsonlText;
  sessionTurns._cacheValue = turns;
  return turns;
}
