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
  let last = null;
  for (const line of lines) {
    if (!line) continue;
    try {
      const r = JSON.parse(line);
      if (r.type === 'assistant' && r.message && r.message.content) {
        last = r;
      }
    } catch (e) {}
  }
  if (!last) return '';
  const content = last.message.content;
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
    if (r.type !== 'user' && r.type !== 'assistant') continue;
    if (!r.message || !r.message.content) continue;
    let text = '';
    const content = r.message.content;
    if (typeof content === 'string') {
      text = content;
    } else if (Array.isArray(content)) {
      text = content
        .filter(c => c && c.type === 'text')
        .map(c => c.text)
        .join('\n\n');
    }
    if (!text) continue;
    const rewritten = rewriteXmluiDocUrls(text);
    turns.push({
      role: r.type,
      text: stripImagePaths(rewritten),
      images: extractImagePaths(rewritten)
    });
  }
  return turns;
}
