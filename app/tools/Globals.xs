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
  const re = /\[Image: source: ([^\]]+)\]/g;
  let m;
  while ((m = re.exec(text)) !== null) paths.push(m[1]);
  return paths;
}
function stripImagePaths(text) {
  if (!text) return text;
  return text.replace(/\n*\[Image: source: [^\]]+\]/g, '');
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

function sessionTurns(jsonlText) {
  if (!jsonlText) return [];
  const turns = [];
  for (const line of jsonlText.split('\n')) {
    if (!line) continue;
    let r;
    try { r = JSON.parse(line); } catch (e) { continue; }
    let role = null;
    let text = '';
    const inlineImages = [];
    if (r.type === 'user' || r.type === 'assistant') {
      if (!r.message || !r.message.content) continue;
      role = r.type;
      const content = r.message.content;
      if (typeof content === 'string') {
        text = content;
      } else if (Array.isArray(content)) {
        text = content
          .filter(c => c && c.type === 'text')
          .map(c => c.text)
          .join('\n\n');
        for (const c of content) {
          if (c && c.type === 'image' && c.source && c.source.type === 'base64' && c.source.data) {
            const mt = c.source.media_type || 'image/png';
            inlineImages.push('data:' + mt + ';base64,' + c.source.data);
          }
        }
      }
    } else if (r.type === 'event_msg' && r.payload) {
      if (r.payload.type === 'user_message') role = 'user';
      if (r.payload.type === 'agent_message') role = 'assistant';
      text = r.payload.message || '';
    }
    if (!role) continue;
    if (!text && inlineImages.length === 0) continue;
    const rewritten = rewriteXmluiDocUrls(text);
    turns.push({
      role,
      text: stripImagePaths(rewritten),
      images: extractImagePaths(rewritten).concat(inlineImages)
    });
  }
  return turns;
}
