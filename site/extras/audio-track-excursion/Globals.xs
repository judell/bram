// Copied from app/tools/Globals.xs: the rows DiffView renders.
function diffLineRows(text) {
  if (!text) return [];
  return text.split('\n').map(function (line) {
    let kind = 'context';
    if (line.startsWith('@@')) kind = 'hunk';
    else if (line.startsWith('+++') || line.startsWith('---')) kind = 'fileheader';
    else if (line.startsWith('diff ') || line.startsWith('index ')) kind = 'fileheader';
    else if (line.startsWith('+')) kind = 'add';
    else if (line.startsWith('-')) kind = 'del';
    return { kind: kind, text: line || ' ' };
  });
}

function diffLineBg(kind) {
  if (kind === 'add') return '$color-success-100';
  if (kind === 'del') return '$color-danger-100';
  if (kind === 'hunk') return '$color-info-100';
  return 'transparent';
}

function diffLineColor(kind) {
  if (kind === 'fileheader') return '$textColor-secondary';
  return '$textColor-primary';
}

function diffViewRows(apiResult, fallbackText) {
  const raw = (apiResult && apiResult.length) ? apiResult : diffLineRows(fallbackText);
  return raw.map(function (row) {
    const lineColor = diffLineColor(row.kind);
    const segs = (row.segments && row.segments.length)
      ? row.segments
      : [{ text: row.text }];
    return {
      kind: row.kind,
      bg: diffLineBg(row.kind),
      color: lineColor,
      // Word-diff emphasis is a NAMED VARIANT now, not a baked colour:
      // Text resolves backgroundColor-mark-<variant>-Text from the theme
      // (xmlui-org/xmlui#3782), declared in DiffView's Theme wrapper.
      // Two names rather than one — the backend emits $color-danger-200
      // for deletions and $color-success-200 for additions, so a single
      // "emphasis" variant would flatten the red/green distinction.
      // Per-segment colour is gone: the line's colour is row-level and
      // rides the outer Text.
      segments: segs.map(function (s) {
        return {
          text: s.text,
          variant: s.bg ? (row.kind === 'add' ? 'diffAdd' : 'diffDel') : null,
        };
      }),
    };
  });
}

