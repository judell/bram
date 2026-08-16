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
// after a hit-snippet click so the target app shows context around the
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

// Split an FTS5 search snippet into render segments [{ text, hit }] so hit
// rows can highlight matched terms (SearchResults.xmlui). The server marks
// matches with single [ / ] chars (snippet(search_index, -1, '[', ']', '…',
// 40) in search_index.rs), but project content is full of LITERAL brackets
// ([ai-describe], [iframe], trace subkinds...), so a bracketed span counts
// as a match only when its inner text STARTS WITH one of the query's terms
// (case-insensitive); any other bracket is literal content and stays in the
// text. Marker chars are stripped from hit segments; literal brackets are
// preserved.
function snippetSegments(snippet, query) {
  const text = snippet || '';
  const terms = (query || '')
    .replace(/"/g, ' ')
    .split(/\s+/)
    .filter((t) => t.length >= 2)
    .map((t) => t.toLowerCase());
  const segs = [];
  let plain = '';
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '[' && terms.length) {
      const close = text.indexOf(']', i + 1);
      if (close > i) {
        const inner = text.slice(i + 1, close);
        const lower = inner.toLowerCase();
        const isHit = terms.filter((t) => lower.indexOf(t) === 0).length > 0;
        if (isHit) {
          if (plain) { segs.push({ text: plain, hit: false }); plain = ''; }
          segs.push({ text: inner, hit: true });
          i = close + 1;
          continue;
        }
      }
    }
    plain += ch;
    i += 1;
  }
  if (plain) segs.push({ text: plain, hit: false });
  return segs;
}

// Order filtered search hits for the Search tab's sort control
// (search-results-date-sort). 'relevance' returns the input untouched — the
// bucket-merged rank order. 'newest' / 'oldest' compare valid epoch
// timestamps; undated or malformed-date hits sort last in either direction;
// equal dates tie-break by relevance rank, then by the stable result key.
function sortSearchHits(hits, order) {
  if (order !== 'newest' && order !== 'oldest') return hits;
  const dir = order === 'newest' ? -1 : 1;
  return hits.slice().sort((a, b) => {
    const ad = Number(a.date) > 0 ? Number(a.date) : null;
    const bd = Number(b.date) > 0 ? Number(b.date) : null;
    if (ad === null && bd !== null) return 1;
    if (bd === null && ad !== null) return -1;
    if (ad !== null && bd !== null && ad !== bd) return (ad - bd) * dir;
    if ((a.rank || 0) !== (b.rank || 0)) return (a.rank || 0) - (b.rank || 0);
    return String(a.key || '') < String(b.key || '') ? -1 : 1;
  });
}

// carry-search-query-to-tabs: the Search page's current query, published so
// Sessions / Issues / History seed their filter from it at mount ("switch
// tabs mid-investigation" carry-over). Deliberately in-memory xs module
// state — it lives exactly as long as the pane, so tabs opened days later
// are never pre-filtered by a stale query (localStorage was rejected for
// that reason). Accessors because handler arrow bodies can't assign module
// vars they don't own, and window-member assignment is the engine-rejected
// shape; both functions hoist onto window for qualified handler calls.
var lastSearchQuery = '';
function setLastSearchQuery(q) {
  lastSearchQuery = String(q || '');
}
function getLastSearchQuery() {
  return lastSearchQuery;
}

// browse-tabs-adopt-searchresults: browse-tab needle filters as xs functions
// with the needle passed EXPLICITLY. Inline arrows inside <variable>
// expressions captured the outer needle var as '' (compiled-bindings capture
// failure, same engine family as the frozen date labels: the DEBUG probe
// showed needle "subagent" at the top level while the arrow admitted all
// 100 rows). Explicit arguments sidestep the capture entirely.
function filterCommitList(list, needle) {
  if (!needle) return list || [];
  return (list || []).filter((c) => [
    c.sha || '',
    (c.commit && c.commit.message) || '',
    (c.commit && c.commit.author && c.commit.author.login) || '',
    (c.commit && c.commit.author && c.commit.author.name) || '',
    (c.commit && c.commit.author && c.commit.author.email) || ''
  ].join(' ').toLowerCase().includes(needle));
}
function filterHistoryList(list, needle) {
  if (!needle) return list || [];
  return (list || []).filter((h) =>
    ((h.id || '') + ' ' + (h.title || '')).toLowerCase().includes(needle)
    || JSON.stringify(h.extra || {}).toLowerCase().includes(needle));
}

function statusSectionSubhead(title) {
  const descriptions = {
    'Startup Run': 'first-minute load',
    'Worklist': 'item lifecycle health',
    'Inflight Sentinel': 'agent action claims',
    'Hooks': 'agent guard setup',
    'Agent Coordination': 'Setup-managed file health',
    'Authorization': 'approval record flow',
    'Guards, Staleness, Interrupts, Traces': 'safety signal trail',
    'Search indexer': 'FTS index contents',
  };
  return descriptions[title] || '';
}

function statusSignalDescription(signal) {
  const descriptions = {
    'Renderer drift': 'Measures PTY volume and heartbeat delay during startup. High drift means the UI thread or terminal stream was busy enough to delay visible updates.',
    'Inspector export': 'Shows whether a recent XMLUI Inspector trace exists. A fresh export gives agents concrete interaction, API, and state-change evidence instead of guessing from markup.',
    'Current items': 'Counts active Worklist rows by lifecycle phase. It tells you whether Bram is waiting for apply approval, commit approval, or cleanup.',
    'Recent transitions': 'Summarizes recent Worklist lifecycle snapshots. Use it to confirm approve, apply, commit, and drop actions are moving instead of leaving stale rows behind.',
    'Applied integrity': 'Checks whether applied Worklist items still match the files they changed. A warning means the working tree drifted after apply, so commit approval may no longer describe reality.',
    'Current claim': 'Shows the active host-managed spinner claim. If it is not idle, Bram believes an approve, drop, or iterate cycle is still in progress.',
    'Trace pairs': 'Counts recent inflight sentinel writes and clears. Balanced pairs mean spinner state is being created and cleared through the expected host lifecycle.',
    'Turn completion': 'Reports the latest agent-turn-end decision. It helps explain why a spinner cleared, stayed up, or was skipped when no active claim existed.',
    'Port file': 'Checks Bram port metadata on disk. Stale or mismatched port files explain failed loopback calls and coordination requests that never reach the shell.',
    'Loopback HTTP': 'Probes Bram HTTP on 127.0.0.1. If this fails, agent pane routes, close helpers, or legacy loopback workflows may be unreachable.',
    'Python 3': 'Confirms Python is available for worklist guard hooks. Without it, Claude and Codex hooks may be installed but unable to enforce Bram edits.',
    'Claude hook': 'Shows whether Claude Code has Bram’s PreToolUse guard installed and registered. It protects repo files from unapproved direct edits.',
    'Codex hook': 'Shows whether Codex has Bram’s worklist guard installed and registered. It enforces the same proposal and approval gate for Codex file mutations.',
    'Latest record': 'Shows the newest structured authorization or close-helper record. It helps diagnose stale approvals, consumed payloads, and what the agent is currently allowed to do.',
    'Record age': 'Reports how old the latest coordination record is. Old unconsumed records often explain stuck buttons, stale approvals, or surprising guard behavior.',
    'Guard decisions': 'Counts recent guard blocks. Warnings here mean an agent tried to mutate files outside the approved Worklist path.',
    'Stale approvals': 'Counts rejected stale approvals. These happen when Worklist content changed after the user clicked, so the agent must not apply that payload.',
    'Interrupts': 'Shows recent interruption or silence-clear events. These explain why an agent cycle stopped, a spinner cleared, or an active turn ended unexpectedly.',
    'Inspector exports': 'Reports recent Inspector trace availability. Traces give agents exact UI evidence when markup alone does not explain a bug.',
    'session': 'Number of indexed session transcripts (Claude + Codex) in the full-text search index.',
    'commit': 'Number of indexed git commits in the full-text search index.',
    'issue': 'Number of indexed forge issues in the full-text search index.',
    'worklist-history': 'Number of indexed worklist-history entries in the full-text search index.',
    'Total indexed': 'Total document rows across all buckets in the FTS search index.',
    'Index db': 'On-disk size and path of the SQLite FTS index (a rebuildable cache).',
  };
  if (descriptions[signal]) return descriptions[signal];
  const s = signal || '';
  if (s.indexOf('CLAUDE.md') >= 0) return 'Checks Bram guidance embedded for Claude. Missing, stale, or legacy marker blocks can leave Claude following old coordination rules.';
  if (s.indexOf('AGENTS.md') >= 0) return 'Checks Bram guidance embedded for Codex. Missing, stale, or legacy marker blocks can leave Codex following old coordination rules.';
  if (s.indexOf('settings.json') >= 0) return 'Checks Claude hook registration. The hook file can exist but still be ineffective if settings.json does not reference it.';
  if (s.indexOf('config.toml') >= 0) return 'Checks Codex global configuration managed by Bram Setup. Stale hook blocks or developer instructions can strand Codex on old coordination behavior.';
  if (s.indexOf('worklist-guard.py') >= 0) return 'Checks a Bram worklist guard script against the bundled version. Stale scripts can allow unsafe edits or block valid approved changes.';
  if (s.indexOf('bram-conventions.md') >= 0) return 'Checks the shared Bram conventions sidecar. Stale guidance means agents may follow outdated approval, commit, or cleanup rules.';
  return 'Reports one coordination signal from Bram Status. Use its level, state, detail, and timestamp to decide whether setup, mode routing, or agent communication needs attention.';
}

// Past transcripts often contain broken docs.xmlui.org/... URLs (the form the
// xmlui-mcp server reports as Source). The live docs are hosted at
// www.xmlui.org/docs/... with a `reference/` segment for component pages.
// Rewrite on the way to Markdown so links resolve when clicked.

// Iframe-side trace helper for the [iframe] category of the comms-path
// iframeTrace and _traceHelperTiming bodies live in
// app/__shell/helpers.js as plain JS (window.__bramIframeTrace,
// window.__bramTraceHelperTiming). The `__bram` prefix is critical:
// browser top-level `function iframeTrace(...)` declarations hoist
// onto `window.iframeTrace`, so if the window helper were also named
// `iframeTrace`, this xs declaration would overwrite the plain-JS
// implementation, and the delegator's call would recurse into itself.
// The prefixed name keeps the two namespaces independent. Same
// pattern as `applyAgentMenu` / `window.__bramApplyAgentMenu` (commit
// ea9480e).


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
function normalizeCloseIssue(entry) {
  if (entry && typeof entry === 'object') {
    return {
      number: entry.number,
      title: (entry.title || '').trim(),
    };
  }
  return {
    number: entry,
    title: '',
  };
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
// Worklist-hotspot instrumentation helpers (`Workspace.xmlui` per-item
// Approve / Iterate / Drop + closeIssues dialog). Each helper calls
// `App.mark(label)` — the xmlui-native, sandbox-safe replacement for
// the soon-to-be-banned `performance.*` family (see plan #17 step 2.5
// in the xmlui repo). `App` is spread into xs-script expression scope
// the same way `formatDate` / `navigate` / etc. are, so these helpers
// can live alongside the other Globals.xs functions — no separate
// window-global script needed. App.mark pushes a `kind: "app:mark"`
// record with `ts` (Unix ms) and `perfTs` to the inspector buffer,
// directly mergeable with bram-trace.log on the same Unix-ms clock.
function settingsBatch(s) {
  return !!(s && s.worklist && s.worklist.batchCommitActions);
}
function settingsOneClickApproveCommit(s) {
  return !!(s && s.worklist && s.worklist.oneClickApproveCommit);
}
function settingsShowTargetApp(s) {
  return !!(s && s.ui && s.ui.showTargetApp);
}
function settingsInspectorTap(s) {
  return !!(s && s.traces && s.traces.inspectorTap);
}
function settingsTracingEnabled(s) {
  return !!(s && s.traces && s.traces.enabled);
}
// Default OFF — only explicit `true` enables. Matches the host
// default; the setting is opt-in. Bram-on-Bram developers turn
// this on if they want source-edit hot-reload; everyone else
// experiences no observable difference either way (their edits
// trigger right-pane-reload, not tools-pane-reload).
function settingsToolsPaneHotReload(s) {
  if (!s || !s.ui) return false;
  return s.ui.toolsPaneHotReload === true;
}
// Default ON — only explicit `false` disables. Matches the host
// default for ai.describeCommands; the effective gate is
// ANTHROPIC_API_KEY in the host environment (no key, no calls).
function settingsDescribeCommands(s) {
  if (!s || !s.ai) return true;
  return s.ai.describeCommands !== false;
}

// Diff rendering — used by the DiffView component, which all three
// diff sites (Transcript, Workspace, Commits) share. Per-line
// classification + theme-variable backgrounds; no syntax highlighter
// is bundled with xmlui-standalone so we hand-classify.
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

// Normalize either the backend's annotated rows (with optional per-line
// `segments`) or, as a fallback while the backend round-trip is in
// flight, the locally-classified rows from diffLineRows(). Returns rows
// in a single uniform shape DiffView can iterate: each row carries
// row-level `bg`/`color` plus a non-empty `segments` array. Segments
// without their own `bg` render transparent (no intra-line emphasis).
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
      segments: segs.map(function (s) {
        return { text: s.text, bg: s.bg || null, color: lineColor };
      }),
    };
  });
}

// Build a unified-diff string from an Edit/MultiEdit tool's
// old_string/new_string so DiffView can render it the same way it
// renders git's output.
function unifiedDiffFromEdit(input) {
  if (!input) return '';
  const oldLines = (input.old_string || '').split('\n');
  const newLines = (input.new_string || '').split('\n');
  const head = '--- a\n+++ b\n';
  const hunk = '@@ -1,' + oldLines.length + ' +1,' + newLines.length + ' @@\n';
  const minus = oldLines.map(function (l) { return '-' + l; }).join('\n');
  const plus  = newLines.map(function (l) { return '+' + l; }).join('\n');
  const body = (oldLines.length && newLines.length) ? (minus + '\n' + plus) : (minus + plus);
  return head + hunk + body;
}

// Feedback route helpers — parallel to the history* family. The Feedback
// component browses entries from /__feedback-history/list, each shaped as
// { ts: <unix_ms>, itemId: <string>, fileName: <string> }.
function feedbackHistoryItemTitle(entry) {
  return (entry && entry.itemId) || '(unknown item)';
}
function feedbackHistoryDateLine(entry) {
  if (!entry || !entry.ts) return '';
  const d = new Date(Number(entry.ts));
  if (isNaN(d.getTime())) return '';
  return d.toISOString().slice(0, 16).replace('T', ' ');
}

// ---- Worklist "message agent" box: persistence + lifecycle helpers ----
// Kept here so Workspace.xmlui can stay markup-only per xmlui_rules #9.

// Worklist message-agent persistence + lifecycle. Bodies live in
// app/__shell/helpers.js as plain JS (window.__bram*). Same migration
// shape and naming convention as the iframeTrace / agent-menu work:
// distinct `__bram`-prefixed window names dodge the trap where xs
// `function foo` would hoist onto `window.foo` and overwrite the
// helpers.js implementation
// (see memory: xs-to-window-migration-name-collision).

// Mirrors toTurn's `s.replace(/\s+/g, ' ').trim()` collapse in
// app/__shell/helpers.js so the JSONL-recorded user text (post-collapse)
// can be matched against the locally-stored submittedWorklistMessage
// (pre-collapse). Strict === would fail whenever the submitted text
// contained any internal whitespace runs.
// Map the inflight sentinel's `kind` field to the gerund verb shown below
// the in-flight item ("Approving", "Iterating", "Dropping"). Returns '' for
// unknown / missing kind so the calling markup hides cleanly.

// Per-tab splitter persistence. XMLUI's documented `resize` event
// delivers the primary panel size in pixels, while older traces showed
// `[primary, secondary]` arrays. Preserve both forms: pixel events are
// stored as `Npx`, arrays are normalized to a percentage.
// Note: `writeLocalStorage('bram.splitter.<key>', v)` does
// persist to native localStorage, but XMLUI nests dotted keys under
// the top-level — value lands at `localStorage.bram.splitter.<key>`
// inside the JSON blob at `localStorage['bram']`, not as a flat
// `localStorage['bram.splitter.<key>']` entry. A flat-key sqlite
// probe will miss it; check the `bram` top-level instead.
// Keys: `bram.splitter.<key>` (worklist, sessions, commits, context,
// issues). The outer-shell `bram.splitter.shell` key is owned by
// app/main.js and uses native localStorage flat keys.

var worklistVoiceTarget = '';
var worklistVoiceText = '';
var worklistVoiceMeta = null;
var worklistVoiceSeq = 0;
var worklistVoiceProcessing = false;
var worklistVoiceProcessingTarget = '';
// True between mediaRecorder actually starting in the parent shell and the
// user clicking stop / a transcript arriving. Drives the tri-state voice
// buttons so they show ⏳ during the start-up gap (parent runs
// ensureServerRunning + getUserMedia + new MediaRecorder before
// mediaRecorder.start() fires) instead of ⏹ instantly. Without this the
// iframe button flips to ⏹ synchronously and users start speaking into a
// not-yet-recording stream, losing the first phoneme(s).
var worklistVoiceRecordingActive = false;


function setWorklistVoiceTarget(target) {
  window.__bramIframeTrace('voice-trace', { stage: 'setTarget-enter', target: target || '', current: worklistVoiceTarget || '' });
  const next = target || '';
  window.bramSetActiveVoiceTargetMirror(next);
  if (worklistVoiceTarget === next) {
    window.__bramIframeTrace('voice-trace', { stage: 'setTarget-noop', target: next });
    return;
  }
  worklistVoiceTarget = next;
  window.__bramIframeTrace('voice-input', { target: worklistVoiceTarget || 'terminal', stage: 'target' });
  window.__bramIframeTrace('voice-trace', { stage: 'setTarget-exit', target: worklistVoiceTarget || '' });
}

function isWorklistVoiceProcessingTarget(target) {
  const t = target || '';
  const p = worklistVoiceProcessingTarget || '';
  return !!worklistVoiceProcessing && (p === t || (t === 'feedback' && p.indexOf('feedback:') === 0));
}

// Top-level xs function — writes to module-scope worklistVoiceSeq
// correctly. The arrow body inside toggleVoiceForCurrentTarget's
// voiceStop callback can't do this assignment (closure-local bug);
// see fix-voice-iframetrace-bare-name draft for full background.
function bumpWorklistVoiceSeq() {
  const before = worklistVoiceSeq;
  worklistVoiceSeq = worklistVoiceSeq + 1;
  window.__bramIframeTrace('voice-trace', { stage: 'bumpSeq', before: before, after: worklistVoiceSeq });
}

// Footer perf instrument (instrument-footer-composer-rerender): log each time
// the footer's agent-status or session-meta source changes, stamped with
// whether the message composer is focused. A typing-burst window in
// bram-trace.log then shows how often the shared footer scope churns while the
// user is actually typing; correlate with heartbeat-batch drift over the same
// window to quantify the cost the isolate-footer-composer fix removes.
function __bramTraceFooterChurn(source, composerFocused) {
  window.__bramIframeTrace('footer-churn', { source: source || '', composerFocused: !!composerFocused });
}

var bramFocusedFeedbackItemId = '';
function setFocusedFeedbackItemId(id) {
  bramFocusedFeedbackItemId = id || '';
  window.bramSetActiveFocusedFeedbackItemIdMirror(id || '');
}
// Decide the iframe-side state update for the `inflightClaim` DataSource
// (the wrapper around resources/.inflight-claim.json). Sentinel is the
// single source of truth for the spinner. Returns an object the caller
// destructures and assigns; xs scope rules prevent us from writing
// App-level vars from a function defined here (that's the same lvalue
// constraint we hit on the active-tool path in 525a718). Kinds:
//   - 'submit' : sentinel claims an item; caller sets submitting +
//                submittedItemId + actionProgressKind.
//   - 'clear'  : sentinel went empty after a submitting state; caller
//                runs the cleanup block (and emits the iframe-clear trace
//                with the returned trace payload).
//   - 'none'   : no transition needed.
//
// IMPORTANT non-reset in the 'clear' branch:
//
//   - setWorklistVoiceTarget('message-agent') IS called in the
//     'clear' branch (and also via the reactive listener below).
//     Belt-and-suspenders: after an action cycle completes, the
//     feedback panel unmounts along with the ChangeListener that
//     delivers worklistVoiceText into feedbackBox. If
//     worklistVoiceTarget stayed 'feedback', the next voice cycle's
//     transcript would land nowhere (only the message-agent
//     ChangeListener is always mounted, and it gates on the target
//     matching). The reactive listener below covers every other
//     path that unmounts the feedback panel.

// Reset worklistVoiceTarget to 'message-agent' whenever the feedback
// panel is no longer mounted. Mounted condition:
//   selected !== null AND feedbackExpanded === true.
//
// When the panel unmounts via any path (radio-dot click on a different
// row, inflight-clear, item swap from the worklist, etc.), the feedback
// ChangeListener that consumes worklistVoiceText goes with it. The
// message-agent ChangeListener is always mounted but gates on
// target === 'message-agent', so a stale 'feedback' target drops
// transcripts on the floor (diagnosed 2026-06-10 06:18:12: [voice]
// stage=voice-into-result fired, no subkind=voice-input stage=append).
// Wired into a ChangeListener at the Workspace VStack so every
// transition that affects panel mount-state triggers the check.
function resetVoiceTargetIfFeedbackPanelGone(selected, feedbackExpanded) {
  if (!selected || !feedbackExpanded) {
    setWorklistVoiceTarget('message-agent');
  }
}

// Voice-transcript arrival helper: appends to the TextArea AND mirrors
// the resulting value into feedbackDraftsById so the iterate-clear path
// (which gates on map presence) and the per-row clear ChangeListener
// (which targets the DOM) see consistent state. Returns the new drafts
// map so the caller can assign it back into Workspace's reactive var
// without nested arrow bodies in the attribute. Extracted to xs scope
// because inline multi-statement attributes with object literals were
// silently dropping their tail statements in this codebase (verified
// 2026-06-16 trace: voice append fired, subsequent persist did not).
function handleFeedbackVoiceArrival(feedbackBox, itemId, currentDrafts, currentExpandedIds) {
  window.__bramIframeTrace('voice-helper', { stage: 'enter', itemId: itemId || '' });
  const transcript = (typeof window !== 'undefined' && window.__bramLatestVoiceTranscript) || worklistVoiceText || '';
  const appendedValue = appendVoiceTranscript(feedbackBox, transcript);
  const existingValue = String(((currentDrafts || {})[itemId]) || '');
  const nextValue = appendedValue === false ? existingValue : appendedValue;
  window.__bramIframeTrace('voice-helper', {
    stage: 'after-append',
    returnedLen: nextValue.length,
    valueLen: (feedbackBox && feedbackBox.value ? String(feedbackBox.value).length : 0)
  });
  const next = Object.assign({}, currentDrafts || {});
  next[itemId] = nextValue;
  window.__bramIframeTrace('voice-helper', { stage: 'before-persist', nextLen: (next[itemId] || '').length });
  persistWorklistUiState({ expandedItemIds: currentExpandedIds || [], feedbackDraftsById: next });
  window.__bramIframeTrace('voice-helper', { stage: 'after-persist' });
  return next;
}

function appendVoiceTranscript(component, transcript) {
  window.__bramIframeTrace('voice-trace', { stage: 'appendVoice-enter', tLen: (transcript || '').length, hasComponent: !!component });
  if (!component || !transcript) {
    window.__bramIframeTrace('voice-trace', { stage: 'appendVoice-early-return', reason: !component ? 'no-component' : 'no-transcript' });
    return false;
  }
  const meta = worklistVoiceMeta || {};
  const current = String(component.value || '');
  const cleaned = transcript.replace(/\r?\n/g, ' ').replace(/[ \t]+/g, ' ').trim();
  if (!cleaned) {
    window.__bramIframeTrace('voice-trace', { stage: 'appendVoice-cleaned-empty' });
    return false;
  }
  const spacer = current && !/\s$/.test(current) ? ' ' : '';
  const appended = spacer + cleaned;
  const next = current + appended;
  window.__bramIframeTrace('voice-trace', { stage: 'appendVoice-calling-setValue', currentLen: current.length, nextLen: next.length });
  component.setValue(next);
  window.__bramIframeTrace('voice-trace', { stage: 'appendVoice-after-setValue' });
  const restore = () => {
    let focused = false;
    let cursorAtEnd = false;
    if (typeof component.focus === 'function') {
      component.focus();
      focused = true;
    }
    if (typeof component.setSelectionRange === 'function') {
      component.setSelectionRange(next.length, next.length);
      cursorAtEnd = true;
    }
    window.__bramIframeTrace('voice-input', {
      target: worklistVoiceTarget || 'message-agent',
      stage: 'append',
      requestId: meta.requestId || null,
      stopAtMs: meta.stopAtMs || null,
      stopToAppendMs: typeof meta.stopAtMs === 'number' ? Date.now() - meta.stopAtMs : null,
      stopToResultMs: typeof meta.stopToResultMs === 'number' ? meta.stopToResultMs : null,
      parentStopToDeliverMs:
        typeof meta.parentStopToDeliverMs === 'number' ? meta.parentStopToDeliverMs : null,
      chars: cleaned.length,
      rawChars: transcript.length,
      focused,
      cursorAtEnd
    });
  };
  delay(0);
  restore();
  return next;
}

const ESC_TOOLBAR_DEDUPE_MS = 200;
var lastEscToolbarClickAtMs = 0;

// Toolbar Esc — always interrupts. The #210 hold-gate was reverted: every gating
// signal we tried (awaitingResponse → held the whole turn; a 1s time-debounce →
// too short for a 3.4s strand window; a JSONL first-output check → held during
// tool-use turns because tool_results are role `user`) broke the ability to
// interrupt, which matters more than the recoverable send-strand it prevented.
// #210 stays open for a non-Esc-blocking approach (detect + clear the bounce-back).
function escToolbarClick() {
  const now = Date.now();
  const deltaMs = lastEscToolbarClickAtMs ? now - lastEscToolbarClickAtMs : -1;
  if (deltaMs >= 0 && deltaMs < ESC_TOOLBAR_DEDUPE_MS) {
    window.__bramTraceToolbarKey('esc', { suppressed: 1, reason: 'dedupe', deltaMs });
    return;
  }
  lastEscToolbarClickAtMs = now;
  window.__bramTraceToolbarKey('esc', { suppressed: 0, deltaMs });
  window.sendKeys('\x1b');
}

// Toolbar PTY keystroke instrumentation for #182 incident 6: tracks
// the iframe's current view of pendingMenu at the moment the user
// clicks a toolbar button (1/2/3/Yes/No/Esc), so post-hoc analysis
// can tell whether the click landed on a menu that was actually
// still open vs one the host had already cleared.
// setToolbarPendingMenuFromEvent / setToolbarPendingMenuFromTurnState /
// traceToolbarKey live in app/__shell/helpers.js as window globals.
// xs callers (Main.xmlui's subscribeTauriEvent callbacks and the
// toolbar onClick handlers) resolve them via bare-name window lookup
// — same pattern as logToHost / toTurn / sendKeys. No xs declarations
// here so there's no statement-queue cost or hoist-collision risk.

// Menu state moved into helpers.js (window.bramAgentMenu et al). The
// xs setters below are thin delegators kept for any caller still
// hitting them from xs scope; the actual work, including
// `listener-fired` trace emission, lives in window.__bramApply* /
// window.__bramSetAgentMenu* and runs in plain JS to skip XMLUI's
// processStatementQueueAsync per-statement awaits
// (xmlui/src/components-core/script-runner/process-statement-async.ts:115-166).
// Source of truth: window.bramAgentMenu. Read it directly from xs
// (this file) and from XMLUI markup through bramSubscribeAgentMenu.




function getAgentMenu(turnState) {
  const current = (typeof window !== 'undefined') ? window.bramAgentMenu : null;
  const suppress = (typeof window !== 'undefined') ? window.bramAgentMenuSuppressFallback : true;
  return current || (!suppress && turnState && turnState.pendingMenu) || null;
}

// Toolbar PTY delegators. Required even though the actual work lives
// on window.__bram*: XMLUI's expression engine analyzes identifiers
// inside arrow-function bodies passed to subscribeTauriEvent (e.g.,
// Main.xmlui's onInit), and a bare name with no xs declaration causes
// silent registration failure that aborts the rest of the onInit and
// cascades into AgentMenu's mount. With these declarations present
// xs callers — Main.xmlui's subscriber arrows and the toolbar button
// onClick handlers — resolve as expected.

function toggleVoiceForCurrentTarget(recording) {
  const activeSession = !!(window.__bramHasActiveVoiceSession && window.__bramHasActiveVoiceSession());
  const activeTarget = (window.__bramActiveVoiceSessionTarget && window.__bramActiveVoiceSessionTarget()) || '';
  const currentTarget = worklistVoiceTarget || '';
  window.__bramIframeTrace('voice-trace', { stage: 'toggle-enter', recording: !!recording, activeSession: activeSession, activeTarget: activeTarget, target: currentTarget });
  if (!recording && activeSession && activeTarget && activeTarget !== currentTarget) {
    window.__bramIframeTrace('voice-trace', { stage: 'toggle-rejected-busy-target', activeTarget: activeTarget, target: currentTarget });
    if (window.__bramNotifyVoiceBusy) {
      window.__bramNotifyVoiceBusy({
        requester: 'iframe',
        activeWas: 'iframe',
        activeTarget: activeTarget
      });
    }
    return false;
  }
  if (recording || activeSession) {
    const stoppingTarget = activeTarget || currentTarget;
    worklistVoiceRecordingActive = false;
    worklistVoiceProcessing = true;
    worklistVoiceProcessingTarget = stoppingTarget;
    window.__bramIframeTrace('voice-input', { target: stoppingTarget || 'terminal', stage: 'processing-start' });
    window.__bramIframeTrace('voice-trace', { stage: 'toggle-calling-voiceStop', target: stoppingTarget });
    voiceStop((t, meta) => {
      const deliveryTarget = (meta && meta.target) || stoppingTarget || '';
      window.__bramIframeTrace('voice-trace', { stage: 'voiceStop-cb-enter', tLen: (t || '').length, target: worklistVoiceTarget || '', deliveryTarget: deliveryTarget });
      worklistVoiceProcessing = false;
      worklistVoiceProcessingTarget = '';
      if (!t) {
        window.__bramIframeTrace('voice-input', { target: stoppingTarget || 'terminal', stage: 'processing-empty' });
        return;
      }
      if (window.__bramIsWorklistTextVoiceTarget(deliveryTarget)) {
        window.__bramIframeTrace('voice-trace', { stage: 'voiceStop-cb-text-target-branch', target: deliveryTarget });
        worklistVoiceText = t;
        window.__bramIframeTrace('voice-trace', { stage: 'voiceStop-cb-after-text-assign' });
        window.__bramSetLatestVoiceState(t, Object.assign({}, meta || {}, { target: deliveryTarget }));
        window.__bramIframeTrace('voice-trace', { stage: 'voiceStop-cb-after-setLatest' });
        worklistVoiceMeta = Object.assign({}, meta || {}, { target: deliveryTarget });
        window.bumpWorklistVoiceSeq();
        window.__bramIframeTrace('voice-trace', { stage: 'voiceStop-cb-after-bumpSeq' });
        window.__bramIframeTrace('voice-input', {
          target: deliveryTarget || 'message-agent',
          stage: 'stop',
          requestId: meta && meta.requestId ? meta.requestId : null,
          stopAtMs: meta && meta.stopAtMs ? meta.stopAtMs : null,
          stopToResultMs: meta && typeof meta.stopToResultMs === 'number' ? meta.stopToResultMs : null
        });
      } else {
        window.__bramIframeTrace('voice-input', { target: deliveryTarget || 'terminal', stage: 'fallback-terminal' });
        toTurn('voice: ' + t);
      }
      window.__bramIframeTrace('voice-input', { target: stoppingTarget || 'terminal', stage: 'processing-end' });
      window.__bramIframeTrace('voice-trace', { stage: 'voiceStop-cb-exit' });
    });
    window.__bramIframeTrace('voice-trace', { stage: 'toggle-exit-stop', returning: false });
    return false;
  }
  const startingTarget = worklistVoiceTarget || '';
  window.__bramIframeTrace('voice-input', { target: startingTarget || 'terminal', stage: 'start' });
  window.__bramIframeTrace('voice-trace', { stage: 'toggle-start-branch', target: startingTarget });
  worklistVoiceRecordingActive = false;
  worklistVoiceProcessing = false;
  worklistVoiceProcessingTarget = '';
  window.__bramIframeTrace('voice-trace', { stage: 'toggle-calling-voiceStart', target: startingTarget });
  voiceStart(
    () => {
      window.__bramIframeTrace('voice-trace', { stage: 'voiceStart-cb-enter' });
      worklistVoiceRecordingActive = true;
      window.__bramIframeTrace('voice-input', { target: startingTarget || 'terminal', stage: 'recording-started' });
      window.__bramIframeTrace('voice-trace', { stage: 'voiceStart-cb-exit' });
    },
    (data) => {
      window.__bramIframeTrace('voice-trace', { stage: 'voiceStart-failed-cb-enter', target: worklistVoiceTarget || '' });
      worklistVoiceRecordingActive = false;
      worklistVoiceProcessing = false;
      worklistVoiceProcessingTarget = '';
      window.__bramSetLatestVoiceState('', {
        requestId: data && data.requestId ? data.requestId : null,
        target: startingTarget,
        stopAtMs: data && data.stopAtMs ? data.stopAtMs : Date.now(),
        stopToResultMs: 0,
        parentStopToDeliverMs: data && typeof data.stopToDeliverMs === 'number' ? data.stopToDeliverMs : null
      });
      window.__bramIframeTrace('voice-input', { target: startingTarget || 'terminal', stage: 'start-rejected' });
      window.__bramIframeTrace('voice-trace', { stage: 'voiceStart-failed-cb-exit' });
    },
    { target: startingTarget }
  );
  window.__bramIframeTrace('voice-trace', { stage: 'toggle-exit-start', returning: true });
  return true;
}
