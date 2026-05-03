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
  if (typeof content === 'string') return content;
  return (Array.isArray(content) ? content : [])
    .filter(c => c && c.type === 'text')
    .map(c => c.text)
    .join('\n\n');
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
    turns.push({ role: r.type, text });
  }
  return turns;
}
