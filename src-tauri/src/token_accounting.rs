//! issue-486-token-accounting-instrument: per-call token accounting across
//! the agents' own transcripts.
//!
//! Every provider records usage per model call; nothing in Bram read it
//! before. This module reads it (typed, tolerant of unknown fields), joins
//! it to the state-mirror claim ledger, and answers three questions:
//!
//! - what each session and item spent (`/__token-accounting`,
//!   `bram --token-accounting <root>`);
//! - how much context Bram's real work needs, and how much of it is Bram's
//!   own fixed cost (the `constrained` section: window fit, floor,
//!   uncached tokens per call);
//! - which files the agents read into context, and how much of `lib.rs` is
//!   test code (`files`).
//!
//! It is its own module on purpose: `lib.rs` is the question this
//! instrument measures (#487), and the instrument must not grow it. Tests
//! live in `token_accounting_tests.rs`.
//!
//! # Context, per provider
//!
//! `context` is what the model was shown on one call: `input + cacheRead +
//! cacheCreation`. The providers report it differently, so the readers
//! normalize it:
//!
//! - **Claude**: `input_tokens` EXCLUDES cache; `context = input_tokens +
//!   cache_read_input_tokens + cache_creation_input_tokens`.
//! - **Codex**: `input_tokens` INCLUDES `cached_input_tokens` (and, as far as
//!   the records show, `cache_write_input_tokens`), so `context =
//!   input_tokens`; `cacheRead = cached_input_tokens`, `cacheCreation =
//!   cache_write_input_tokens`, and the reported `input` is the remainder.
//! - **OpenCode**: `tokens.input` EXCLUDES cache (observed: input 4 beside
//!   cache.read 30633), so `context = input + cache.read + cache.write`.
//! - **Antigravity**: unavailable. Its brain log has per-step tokens but no
//!   model field, and Bram's exported copy drops usage.
//!
//! Uncached tokens per call (the prompt processing a server with no prompt
//! cache would do) is `context - cacheRead` for every provider.

use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::worklist_state::{claims_overlapping, ClaimWindow};

#[cfg(test)]
#[path = "token_accounting_tests.rs"]
mod tests;

pub const ANTIGRAVITY_REASON: &str =
    "brain log has per-step tokens but no model field; Bram's export drops usage";

/// Context windows the fit table reports, as shipped: 32k, 64k, 128k and 256k
/// are powers of two, 200k is the round number Claude sells.
pub const FIT_WINDOWS: [(&str, u64); 5] = [
    ("32k", 32_768),
    ("64k", 65_536),
    ("128k", 131_072),
    ("200k", 200_000),
    ("256k", 262_144),
];

/// Two calls this close in one session are the same turn's work.
const TURN_GAP_MS: i64 = 120_000;
const LIB_RS: &str = "src-tauri/src/lib.rs";
const FILES_CAP: usize = 300;

// --- wire types -----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Thread {
    Main,
    Subagent,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Attribution {
    Items(Vec<String>),
    Chat,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ProviderStatus {
    Read { sessions: usize, calls: usize },
    Unavailable { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallReading {
    pub provider: String,
    pub session: String,
    pub thread: Thread,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub model: String,
    pub at_ms: i64,
    pub context: u64,
    pub input: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub output: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_output: Option<u64>,
    pub attribution: Attribution,
}

/// One model call as a reader found it, before attribution.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RawCall {
    pub at_ms: i64,
    pub model: String,
    pub context: u64,
    pub input: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub output: u64,
    pub reasoning: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    pub resets_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
}

/// A file credited with bytes read into context.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadEvent {
    pub path: String,
    pub bytes: u64,
    pub via_shell: bool,
    pub at_ms: i64,
    /// (line number, bytes) of each line returned; kept only for `lib.rs`.
    pub lines: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedFile {
    pub provider: &'static str,
    pub session_id: String,
    pub thread: Option<Thread>,
    pub agent_id: Option<String>,
    pub label: Option<String>,
    pub calls: Vec<RawCall>,
    pub reads: Vec<ReadEvent>,
    pub unreadable: usize,
    pub compactions: usize,
    pub quota: Option<Quota>,
}

// --- tolerant record shapes -----------------------------------------------

/// Message or tool-result content: a string, or a list of blocks. Anything
/// else reads as `Other` instead of failing the line.
#[derive(Debug, Default)]
enum Content {
    Text(String),
    Blocks(Vec<Block>),
    #[default]
    Other,
}

struct ContentVisitor;

impl<'de> Visitor<'de> for ContentVisitor {
    type Value = Content;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a string, a list of blocks, or anything else")
    }
    fn visit_str<E>(self, v: &str) -> Result<Content, E> {
        Ok(Content::Text(v.to_string()))
    }
    fn visit_string<E>(self, v: String) -> Result<Content, E> {
        Ok(Content::Text(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Content, A::Error> {
        let mut v = Vec::new();
        while let Some(b) = seq.next_element::<Block>()? {
            v.push(b);
        }
        Ok(Content::Blocks(v))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Content, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Content::Other)
    }
    fn visit_unit<E>(self) -> Result<Content, E> {
        Ok(Content::Other)
    }
    fn visit_bool<E>(self, _: bool) -> Result<Content, E> {
        Ok(Content::Other)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Content, E> {
        Ok(Content::Other)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Content, E> {
        Ok(Content::Other)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Content, E> {
        Ok(Content::Other)
    }
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ContentVisitor)
    }
}

impl Content {
    fn text(&self) -> String {
        match self {
            Content::Text(s) => s.clone(),
            Content::Blocks(bs) => {
                let mut out = String::new();
                for b in bs {
                    let t = b.all_text();
                    if t.is_empty() {
                        continue;
                    }
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&t);
                }
                out
            }
            Content::Other => String::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct Block {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<ToolInput>,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content: Option<Box<Content>>,
}

impl Block {
    fn all_text(&self) -> String {
        if let Some(t) = &self.text {
            return t.clone();
        }
        self.content.as_ref().map(|c| c.text()).unwrap_or_default()
    }
}

/// The few tool-input fields the accounting looks at. Tool inputs are the
/// tool's own shape; the rest is ignored.
#[derive(Debug, Default, Deserialize)]
struct ToolInput {
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    output_mode: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
    #[serde(default)]
    content: Option<Content>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeRecord {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<ClaudeMessage>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cached_input_tokens: Option<u64>,
    #[serde(default)]
    cache_write_input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    reasoning_output_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexInfo {
    #[serde(default)]
    total_token_usage: Option<CodexUsage>,
    #[serde(default)]
    last_token_usage: Option<CodexUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexRateWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    window_minutes: Option<u64>,
    #[serde(default)]
    resets_at: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexRateLimits {
    #[serde(default)]
    primary: Option<CodexRateWindow>,
    #[serde(default)]
    secondary: Option<CodexRateWindow>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexPayload {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    info: Option<CodexInfo>,
    #[serde(default)]
    rate_limits: Option<CodexRateLimits>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    parent_thread_id: Option<String>,
    #[serde(default)]
    thread_source: Option<String>,
    #[serde(default)]
    agent_nickname: Option<String>,
    #[serde(default)]
    subagent_history_start_ordinal: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    arguments: Option<Content>,
    #[serde(default)]
    input: Option<Content>,
    #[serde(default)]
    output: Option<Content>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexRecord {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    ordinal: Option<u64>,
    #[serde(default)]
    payload: Option<CodexPayload>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeCache {
    #[serde(default)]
    read: Option<u64>,
    #[serde(default)]
    write: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeTokens {
    #[serde(default)]
    input: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
    #[serde(default)]
    reasoning: Option<u64>,
    #[serde(default)]
    cache: Option<OpenCodeCache>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeModel {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeTime {
    #[serde(default)]
    created: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeData {
    #[serde(default)]
    tokens: Option<OpenCodeTokens>,
    #[serde(default)]
    model: Option<OpenCodeModel>,
    #[serde(default)]
    time: Option<OpenCodeTime>,
}

// --- tool results: bytes read into context --------------------------------

/// What a pending tool call will credit when its result arrives.
#[derive(Clone, Debug, PartialEq)]
enum PendingTool {
    Read { path: String },
    GrepContent { default_path: Option<String> },
    GrepFiles,
    Shell { files: Vec<String> },
}

fn is_lib_rs(path: &str) -> bool {
    path.ends_with("src/lib.rs")
}

/// `cat -n` output: each line is spaces, the line number, a tab, the text.
/// Returns (line number, bytes including the newline) per line.
pub fn read_result_lines(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let t = line.trim_start_matches(' ');
        let digits = t.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 || t.as_bytes().get(digits) != Some(&b'\t') {
            continue;
        }
        if let Ok(n) = t[..digits].parse::<u32>() {
            out.push((n, (line.len() + 1) as u32));
        }
    }
    out
}

/// Split a content-mode Grep result into per-file (bytes, lines). Lines are
/// `path:line:text`, or `path-line-text` for context lines. With one file the
/// path is omitted and `default_path` names it.
pub fn grep_content_by_file(
    text: &str,
    default_path: Option<&str>,
) -> Vec<(String, u64, Vec<(u32, u32)>)> {
    let mut order: Vec<String> = Vec::new();
    let mut by: HashMap<String, (u64, Vec<(u32, u32)>)> = HashMap::new();
    for line in text.split('\n') {
        if line.is_empty() || line == "--" {
            continue;
        }
        let Some((path, no)) = split_grep_line(line, default_path) else {
            continue;
        };
        let bytes = (line.len() + 1) as u64;
        let e = by.entry(path.clone()).or_insert_with(|| {
            order.push(path.clone());
            (0, Vec::new())
        });
        e.0 += bytes;
        if is_lib_rs(&path) {
            e.1.push((no, bytes as u32));
        }
    }
    order
        .into_iter()
        .map(|p| {
            let (b, l) = by.remove(&p).unwrap_or_default();
            (p, b, l)
        })
        .collect()
}

/// (path, line number) of one grep content line, or None for a line that
/// isn't one (a footer such as "[Showing results with pagination ...]").
fn split_grep_line(line: &str, default_path: Option<&str>) -> Option<(String, u32)> {
    for sep in [':', '-'] {
        let b = line.as_bytes();
        let mut i = 1;
        while i < b.len() {
            if b[i] as char == sep {
                let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 && b.get(i + 1 + digits).map(|c| *c as char) == Some(sep) {
                    if let Ok(n) = line[i + 1..i + 1 + digits].parse::<u32>() {
                        return Some((line[..i].to_string(), n));
                    }
                }
            }
            i += 1;
        }
    }
    // A single-file grep: `12:text` with the path elsewhere.
    let digits = line.bytes().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && matches!(line.as_bytes().get(digits), Some(b':') | Some(b'-')) {
        let n = line[..digits].parse::<u32>().ok()?;
        return Some((default_path?.to_string(), n));
    }
    None
}

const SHELL_READERS: [&str; 10] = [
    "sed", "cat", "head", "tail", "grep", "rg", "nl", "awk", "less", "bat",
];
const FILE_EXTENSIONS: [&str; 22] = [
    "rs", "js", "ts", "tsx", "md", "xmlui", "xs", "json", "jsonl", "py", "sh", "toml", "html",
    "css", "yml", "yaml", "txt", "log", "ps1", "mjs", "lock", "csv",
];

/// The files a shell command line reads, when it uses a reader such as
/// `sed -n`, `cat`, `head`, `tail` or `grep`. A command naming several files
/// is returned with all of them; the caller splits the bytes evenly, which is
/// approximate.
pub fn shell_named_files(cmd: &str) -> Vec<String> {
    let words: Vec<&str> = cmd
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')' | '<' | '>'))
        .collect();
    let reads = words
        .iter()
        .any(|w| SHELL_READERS.contains(&w.trim_matches('"')));
    if !reads {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for w in words {
        let w = w.trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == ',');
        if w.is_empty() || w.starts_with('-') || w.contains('*') || w.contains('$') {
            continue;
        }
        let name = w.rsplit('/').next().unwrap_or(w);
        let Some((stem, ext)) = name.rsplit_once('.') else {
            continue;
        };
        if stem.is_empty() || !FILE_EXTENSIONS.contains(&ext) {
            continue;
        }
        if !w
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '#'))
        {
            continue;
        }
        let w = w.strip_prefix("./").unwrap_or(w).to_string();
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

fn credit_result(pending: &PendingTool, text: &str, at_ms: i64, out: &mut Vec<ReadEvent>) {
    match pending {
        PendingTool::Read { path } => {
            let lines = if is_lib_rs(path) {
                read_result_lines(text)
            } else {
                Vec::new()
            };
            out.push(ReadEvent {
                path: path.clone(),
                bytes: text.len() as u64,
                via_shell: false,
                at_ms,
                lines,
            });
        }
        PendingTool::GrepContent { default_path } => {
            for (path, bytes, lines) in grep_content_by_file(text, default_path.as_deref()) {
                out.push(ReadEvent {
                    path,
                    bytes,
                    via_shell: false,
                    at_ms,
                    lines,
                });
            }
        }
        PendingTool::GrepFiles => {
            for line in text.split('\n') {
                let l = line.trim();
                if l.is_empty()
                    || l.starts_with("Found ")
                    || l.starts_with("No files")
                    || l.starts_with('[')
                {
                    continue;
                }
                out.push(ReadEvent {
                    path: l.to_string(),
                    bytes: (line.len() + 1) as u64,
                    via_shell: false,
                    at_ms,
                    lines: Vec::new(),
                });
            }
        }
        PendingTool::Shell { files } => {
            if files.is_empty() {
                return;
            }
            let n = files.len() as u64;
            let each = text.len() as u64 / n;
            let extra = text.len() as u64 - each * n;
            for (i, f) in files.iter().enumerate() {
                out.push(ReadEvent {
                    path: f.clone(),
                    bytes: each + if i == 0 { extra } else { 0 },
                    via_shell: true,
                    at_ms,
                    lines: Vec::new(),
                });
            }
        }
    }
}

// --- Claude reader --------------------------------------------------------

/// Incremental reader of a Claude transcript, shared by the report (whole
/// file) and the turn-end sampler (the tail since its last sample).
#[derive(Debug, Default)]
pub struct ClaudeParser {
    pub calls: Vec<RawCall>,
    idx: HashMap<String, usize>,
    pub reads: Vec<ReadEvent>,
    pub unreadable: usize,
    pub compactions: usize,
    pending: HashMap<String, PendingTool>,
    collect_reads: bool,
    /// (index of the first call, timestamp) of the turn the last real user
    /// message started.
    pub turn_start: (usize, i64),
}

impl ClaudeParser {
    pub fn new(collect_reads: bool) -> Self {
        ClaudeParser {
            collect_reads,
            ..Default::default()
        }
    }

    pub fn feed(&mut self, text: &str) {
        for line in text.lines() {
            self.feed_line(line);
        }
    }

    pub fn feed_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let rec: ClaudeRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => {
                self.unreadable += 1;
                return;
            }
        };
        match rec.kind.as_str() {
            "assistant" => self.assistant(rec),
            "user" => self.user(rec),
            "system" if rec.subtype.as_deref() == Some("compact_boundary") => {
                self.compactions += 1;
            }
            _ => {}
        }
    }

    fn assistant(&mut self, rec: ClaudeRecord) {
        let at_ms = rec
            .timestamp
            .as_deref()
            .and_then(crate::parse_iso_to_epoch_ms)
            .unwrap_or(0);
        let Some(msg) = rec.message else {
            return;
        };
        if msg.model.as_deref() == Some("<synthetic>") {
            return;
        }
        if self.collect_reads {
            if let Some(Content::Blocks(blocks)) = &msg.content {
                for b in blocks {
                    self.register_tool_use(b);
                }
            }
        }
        let Some(u) = msg.usage else {
            return;
        };
        let input = u.input_tokens.unwrap_or(0);
        let cache_read = u.cache_read_input_tokens.unwrap_or(0);
        let cache_creation = u.cache_creation_input_tokens.unwrap_or(0);
        let call = RawCall {
            at_ms,
            model: msg.model.unwrap_or_default(),
            context: input + cache_read + cache_creation,
            input,
            cache_read,
            cache_creation,
            output: u.output_tokens.unwrap_or(0),
            reasoning: None,
        };
        // A streamed message repeats its usage under one message.id, one
        // record per content block: one call, the last record's numbers, the
        // first record's time.
        match msg.id.as_ref().and_then(|id| self.idx.get(id)).copied() {
            Some(i) => {
                let first = self.calls[i].at_ms;
                self.calls[i] = RawCall {
                    at_ms: first,
                    ..call
                };
            }
            None => {
                if let Some(id) = msg.id {
                    self.idx.insert(id, self.calls.len());
                }
                self.calls.push(call);
            }
        }
    }

    fn register_tool_use(&mut self, b: &Block) {
        if b.kind.as_deref() != Some("tool_use") {
            return;
        }
        let (Some(id), Some(name)) = (&b.id, &b.name) else {
            return;
        };
        let input = b.input.as_ref();
        let pending = match name.as_str() {
            "Read" => input
                .and_then(|i| i.file_path.clone())
                .map(|path| PendingTool::Read { path }),
            "Grep" => match input.and_then(|i| i.output_mode.as_deref()) {
                Some("content") => Some(PendingTool::GrepContent {
                    default_path: input.and_then(|i| i.path.clone()),
                }),
                _ => Some(PendingTool::GrepFiles),
            },
            "Bash" => input
                .and_then(|i| i.command.as_deref())
                .map(shell_named_files)
                .filter(|f| !f.is_empty())
                .map(|files| PendingTool::Shell { files }),
            _ => None,
        };
        if let Some(p) = pending {
            self.pending.insert(id.clone(), p);
        }
    }

    fn user(&mut self, rec: ClaudeRecord) {
        let at_ms = rec
            .timestamp
            .as_deref()
            .and_then(crate::parse_iso_to_epoch_ms)
            .unwrap_or(0);
        let Some(msg) = rec.message else {
            return;
        };
        match msg.content {
            Some(Content::Blocks(blocks)) => {
                let mut has_result = false;
                for b in &blocks {
                    if b.kind.as_deref() == Some("tool_result") {
                        has_result = true;
                        if !self.collect_reads {
                            continue;
                        }
                        let Some(id) = &b.tool_use_id else {
                            continue;
                        };
                        if let Some(p) = self.pending.remove(id) {
                            let text = b.content.as_ref().map(|c| c.text()).unwrap_or_default();
                            credit_result(&p, &text, at_ms, &mut self.reads);
                        }
                    }
                }
                if !has_result {
                    self.turn_start = (self.calls.len(), at_ms);
                }
            }
            Some(Content::Text(_)) => self.turn_start = (self.calls.len(), at_ms),
            _ => {}
        }
    }

    pub fn into_parsed(self, session: &str, thread: Thread, agent: Option<String>) -> ParsedFile {
        ParsedFile {
            provider: "claude",
            session_id: session.to_string(),
            thread: Some(thread),
            agent_id: agent,
            calls: self.calls,
            reads: self.reads,
            unreadable: self.unreadable,
            compactions: self.compactions,
            ..Default::default()
        }
    }
}

pub fn parse_claude_text(
    text: &str,
    session: &str,
    thread: Thread,
    agent: Option<String>,
) -> ParsedFile {
    let mut p = ClaudeParser::new(true);
    p.feed(text);
    p.into_parsed(session, thread, agent)
}

// --- Codex reader ---------------------------------------------------------

#[derive(Debug, Default)]
pub struct CodexParser {
    pub calls: Vec<RawCall>,
    pub reads: Vec<ReadEvent>,
    pub unreadable: usize,
    pub compactions: usize,
    pub quota: Option<Quota>,
    pub session_id: Option<String>,
    /// For a subagent rollout: the thread that spawned it.
    pub parent_id: Option<String>,
    pub is_subagent: bool,
    pub nickname: Option<String>,
    start_ordinal: Option<u64>,
    last_total: Option<u64>,
    model: String,
    line_no: u64,
    pending: HashMap<String, PendingTool>,
    collect_reads: bool,
    pub turn_start: (usize, i64),
}

const CODEX_SHELL_TOOLS: [&str; 6] = [
    "exec",
    "exec_command",
    "shell",
    "shell_command",
    "local_shell",
    "container.exec",
];

impl CodexParser {
    pub fn new(collect_reads: bool) -> Self {
        CodexParser {
            collect_reads,
            ..Default::default()
        }
    }

    pub fn feed(&mut self, text: &str) {
        for line in text.lines() {
            self.feed_line(line);
        }
    }

    pub fn feed_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let line_index = self.line_no;
        self.line_no += 1;
        let rec: CodexRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => {
                self.unreadable += 1;
                return;
            }
        };
        let at_ms = rec
            .timestamp
            .as_deref()
            .and_then(crate::parse_iso_to_epoch_ms)
            .unwrap_or(0);
        let ordinal = rec.ordinal.unwrap_or(line_index);
        let Some(p) = rec.payload else {
            return;
        };
        match rec.kind.as_str() {
            "session_meta" => {
                if self.session_id.is_none() {
                    self.session_id = p.id.clone();
                    self.is_subagent = p.thread_source.as_deref() == Some("subagent");
                    self.parent_id = p.parent_thread_id.clone();
                    self.nickname = p.agent_nickname.clone();
                    self.start_ordinal = p.subagent_history_start_ordinal;
                }
            }
            "turn_context" => {
                if let Some(m) = p.model {
                    self.model = m;
                }
            }
            "event_msg" => match p.kind.as_deref() {
                Some("token_count") => self.token_count(p, at_ms, ordinal),
                Some("task_started") => self.turn_start = (self.calls.len(), at_ms),
                _ => {}
            },
            "compacted" => self.compactions += 1,
            "response_item" => {
                if self.collect_reads {
                    self.response_item(p, at_ms);
                }
            }
            _ => {}
        }
    }

    fn token_count(&mut self, p: CodexPayload, at_ms: i64, ordinal: u64) {
        if let Some(r) = p.rate_limits {
            let w = |w: CodexRateWindow| QuotaWindow {
                used_percent: w.used_percent,
                window_minutes: w.window_minutes,
                resets_at: w.resets_at,
            };
            self.quota = Some(Quota {
                primary: r.primary.map(w),
                secondary: r.secondary.map(w),
            });
        }
        // A forked subagent rollout starts with a copy of its parent's
        // records; only those at or after the start ordinal are its own.
        if let Some(start) = self.start_ordinal {
            if ordinal < start {
                return;
            }
        }
        let Some(info) = p.info else {
            return;
        };
        let total = info.total_token_usage.as_ref().and_then(|t| t.total_tokens);
        if total.is_some() && total == self.last_total {
            return;
        }
        if total.is_some() {
            self.last_total = total;
        }
        let Some(u) = info.last_token_usage else {
            return;
        };
        let context = u.input_tokens.unwrap_or(0);
        if context == 0 && u.output_tokens.unwrap_or(0) == 0 {
            // An event that carries no usage is not a call.
            return;
        }
        let cache_read = u.cached_input_tokens.unwrap_or(0);
        let cache_creation = u.cache_write_input_tokens.unwrap_or(0);
        self.calls.push(RawCall {
            at_ms,
            model: self.model.clone(),
            context,
            input: context.saturating_sub(cache_read + cache_creation),
            cache_read,
            cache_creation,
            output: u.output_tokens.unwrap_or(0),
            reasoning: u.reasoning_output_tokens,
        });
    }

    fn response_item(&mut self, p: CodexPayload, at_ms: i64) {
        let Some(id) = p.call_id.clone() else {
            return;
        };
        match p.kind.as_deref() {
            Some("function_call") | Some("custom_tool_call") => {
                let name = p.name.as_deref().unwrap_or("");
                if !CODEX_SHELL_TOOLS.contains(&name) {
                    return;
                }
                let cmd = p
                    .input
                    .as_ref()
                    .or(p.arguments.as_ref())
                    .map(|c| c.text())
                    .unwrap_or_default();
                let files = shell_named_files(&cmd);
                if !files.is_empty() {
                    self.pending.insert(id, PendingTool::Shell { files });
                }
            }
            Some("function_call_output") | Some("custom_tool_call_output") => {
                if let Some(pend) = self.pending.remove(&id) {
                    let text = p.output.as_ref().map(|c| c.text()).unwrap_or_default();
                    credit_result(&pend, &text, at_ms, &mut self.reads);
                }
            }
            _ => {}
        }
    }

    pub fn into_parsed(self, fallback_id: &str) -> ParsedFile {
        let own = self
            .session_id
            .clone()
            .unwrap_or_else(|| fallback_id.to_string());
        let (session, thread, agent) = match (self.is_subagent, &self.parent_id) {
            (true, Some(parent)) => (parent.clone(), Thread::Subagent, Some(own)),
            (true, None) => (own.clone(), Thread::Subagent, Some(own)),
            _ => (own, Thread::Main, None),
        };
        ParsedFile {
            provider: "codex",
            session_id: session,
            thread: Some(thread),
            agent_id: agent,
            label: self.nickname,
            calls: self.calls,
            reads: self.reads,
            unreadable: self.unreadable,
            compactions: self.compactions,
            quota: self.quota,
        }
    }
}

pub fn parse_codex_text(text: &str, fallback_id: &str) -> ParsedFile {
    let mut p = CodexParser::new(true);
    p.feed(text);
    p.into_parsed(fallback_id)
}

// --- OpenCode reader ------------------------------------------------------

/// One OpenCode assistant message's `data` JSON as a call, or None for a
/// message with no token record.
pub fn opencode_call(data: &str, created_fallback: i64) -> Result<Option<RawCall>, ()> {
    let d: OpenCodeData = serde_json::from_str(data).map_err(|_| ())?;
    let Some(t) = d.tokens else {
        return Ok(None);
    };
    let cache = t.cache.unwrap_or_default();
    let input = t.input.unwrap_or(0);
    let cache_read = cache.read.unwrap_or(0);
    let cache_creation = cache.write.unwrap_or(0);
    Ok(Some(RawCall {
        at_ms: d.time.and_then(|t| t.created).unwrap_or(created_fallback),
        model: d.model.and_then(|m| m.id).unwrap_or_default(),
        context: input + cache_read + cache_creation,
        input,
        cache_read,
        cache_creation,
        output: t.output.unwrap_or(0),
        reasoning: t.reasoning,
    }))
}

/// This project's OpenCode sessions (children as subagents of their parent).
pub fn opencode_parse(conn: &rusqlite::Connection, root: &Path) -> Vec<ParsedFile> {
    let canonical = crate::canonical_path_string(root);
    let raw = root.to_string_lossy().into_owned();
    let dirs = crate::opencode_directory_spellings(&canonical, &raw);
    let Ok(mut sessions) =
        conn.prepare("SELECT id, parent_id FROM session_v2 WHERE directory IN (?1, ?2, ?3, ?4)")
    else {
        return Vec::new();
    };
    let Ok(rows) = sessions.query_map(rusqlite::params![dirs[0], dirs[1], dirs[2], dirs[3]], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    }) else {
        return Vec::new();
    };
    let sessions: Vec<(String, Option<String>)> = rows.flatten().collect();
    let mut out = Vec::new();
    for (sid, parent) in sessions {
        let Ok(mut stmt) = conn.prepare(
            "SELECT data, time_created FROM session_message \
             WHERE session_id = ?1 AND type = 'assistant' ORDER BY seq",
        ) else {
            return out;
        };
        let Ok(rows) = stmt.query_map(rusqlite::params![sid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        }) else {
            continue;
        };
        let mut pf = ParsedFile {
            provider: "opencode",
            session_id: parent.clone().unwrap_or_else(|| sid.clone()),
            thread: Some(if parent.is_some() {
                Thread::Subagent
            } else {
                Thread::Main
            }),
            agent_id: parent.as_ref().map(|_| sid.clone()),
            ..Default::default()
        };
        for (data, created) in rows.flatten() {
            match opencode_call(&data, created) {
                Ok(Some(c)) => pf.calls.push(c),
                Ok(None) => {}
                Err(()) => pf.unreadable += 1,
            }
        }
        if !pf.calls.is_empty() || pf.unreadable > 0 {
            out.push(pf);
        }
    }
    out
}

// --- lib.rs test spans ----------------------------------------------------

/// The 1-based inclusive line spans of every `#[cfg(test)] mod name { ... }`
/// in Rust source, found by brace counting that skips strings, raw strings,
/// char literals and comments.
pub fn test_spans(src: &str) -> Vec<(u32, u32)> {
    const ATTR: &str = "#[cfg(test)]";
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut line: u32 = 1;
    let mut depth: i64 = 0;
    let mut spans = Vec::new();
    let mut pending: Option<u32> = None;
    let mut saw_mod = false;
    let mut open: Option<(u32, i64)> = None;
    while i < n {
        let c = b[i];
        match c {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut nest = 1;
                i += 2;
                while i < n && nest > 0 {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        nest += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        nest -= 1;
                        i += 2;
                    } else {
                        if b[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                }
            }
            b'"' => {
                i += 1;
                while i < n && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    if i < n && b[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'\'' => {
                let rest = &src[i + 1..];
                match rest.chars().next() {
                    Some('\\') => {
                        // '\n', '\'', '\u{..}': skip to the closing quote.
                        i += 2;
                        while i < n && b[i] != b'\'' {
                            i += 1;
                        }
                        i += 1;
                    }
                    Some(ch) if b.get(i + 1 + ch.len_utf8()) == Some(&b'\'') => {
                        i += 2 + ch.len_utf8();
                    }
                    _ => i += 1, // a lifetime
                }
            }
            b'#' if src[i..].starts_with(ATTR) => {
                if open.is_none() {
                    pending = Some(line);
                    saw_mod = false;
                }
                i += ATTR.len();
            }
            b'{' => {
                if let Some(l) = pending.take() {
                    if saw_mod && open.is_none() {
                        open = Some((l, depth));
                    }
                }
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if let Some((l, d)) = open {
                    if depth == d {
                        spans.push((l, line));
                        open = None;
                    }
                }
                i += 1;
            }
            b';' => {
                pending = None;
                i += 1;
            }
            c if c == b'_' || c.is_ascii_alphabetic() => {
                let start = i;
                while i < n && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                let word = &src[start..i];
                if matches!(word, "r" | "br" | "cr") && matches!(b.get(i), Some(b'"') | Some(b'#'))
                {
                    let mut hashes = 0;
                    let mut j = i;
                    while b.get(j) == Some(&b'#') {
                        hashes += 1;
                        j += 1;
                    }
                    if b.get(j) == Some(&b'"') {
                        j += 1;
                        loop {
                            if j >= n {
                                break;
                            }
                            if b[j] == b'\n' {
                                line += 1;
                            }
                            if b[j] == b'"' && (0..hashes).all(|k| b.get(j + 1 + k) == Some(&b'#'))
                            {
                                j += 1 + hashes;
                                break;
                            }
                            j += 1;
                        }
                        i = j;
                        continue;
                    }
                }
                if pending.is_some() && word == "mod" {
                    saw_mod = true;
                }
            }
            _ => i += 1,
        }
    }
    spans
}

/// Test lines and bytes among the lines a read returned.
pub fn test_share(lines: &[(u32, u32)], spans: &[(u32, u32)]) -> (u64, u64) {
    let mut n = 0;
    let mut bytes = 0;
    for (no, b) in lines {
        if spans.iter().any(|(s, e)| no >= s && no <= e) {
            n += 1;
            bytes += *b as u64;
        }
    }
    (n, bytes)
}

/// Where the report gets lib.rs's test spans for a call's time.
pub trait SpanSource {
    fn spans_at(&mut self, at_ms: i64) -> Option<Arc<Vec<(u32, u32)>>>;
}

/// The spans of `src-tauri/src/lib.rs` at the last commit at or before the
/// call, from git; spans are computed once per commit.
pub struct GitSpans {
    root: PathBuf,
    commits: Option<Vec<(String, i64)>>,
    cache: HashMap<String, Arc<Vec<(u32, u32)>>>,
}

impl GitSpans {
    pub fn new(root: &Path) -> Self {
        GitSpans {
            root: root.to_path_buf(),
            commits: None,
            cache: HashMap::new(),
        }
    }

    fn git(&self, args: &[&str]) -> Option<Vec<u8>> {
        let out = std::process::Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    }
}

impl SpanSource for GitSpans {
    fn spans_at(&mut self, at_ms: i64) -> Option<Arc<Vec<(u32, u32)>>> {
        if self.commits.is_none() {
            let log = self
                .git(&["log", "--format=%H %ct", "--", LIB_RS])
                .unwrap_or_default();
            let list = String::from_utf8_lossy(&log)
                .lines()
                .filter_map(|l| {
                    let (sha, ct) = l.split_once(' ')?;
                    Some((sha.to_string(), ct.trim().parse::<i64>().ok()?))
                })
                .collect();
            self.commits = Some(list);
        }
        // Newest first: the first commit at or before the call.
        let sha = self
            .commits
            .as_ref()?
            .iter()
            .find(|(_, ct)| ct * 1000 <= at_ms)
            .map(|(s, _)| s.clone())?;
        if let Some(s) = self.cache.get(&sha) {
            return Some(s.clone());
        }
        let blob = self.git(&["show", &format!("{sha}:{LIB_RS}")])?;
        let spans = Arc::new(test_spans(&String::from_utf8_lossy(&blob)));
        self.cache.insert(sha, spans.clone());
        Some(spans)
    }
}

// --- discovery and the per-file cache --------------------------------------

#[derive(Clone, Debug)]
pub enum Source {
    ClaudeMain {
        id: String,
        path: PathBuf,
    },
    ClaudeSubagent {
        id: String,
        agent: String,
        path: PathBuf,
    },
    Codex {
        id: String,
        path: PathBuf,
    },
}

impl Source {
    fn path(&self) -> &Path {
        match self {
            Source::ClaudeMain { path, .. }
            | Source::ClaudeSubagent { path, .. }
            | Source::Codex { path, .. } => path,
        }
    }
}

/// Claude transcripts (main and `<id>/subagents/agent-*.jsonl`) and Codex
/// rollouts (main and subagent) for a project root.
pub fn discover(root: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let Some(home) = crate::home_dir() else {
        return out;
    };
    let Ok(canon) = root.canonicalize() else {
        return out;
    };
    let abs = crate::strip_unc_prefix(canon);
    let lookup =
        crate::find_existing_claude_sessions_dir(&home.join(".claude").join("projects"), &abs);
    if let Ok(entries) = std::fs::read_dir(&lookup.chosen) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let subdir = lookup.chosen.join(&id).join("subagents");
            if let Ok(subs) = std::fs::read_dir(&subdir) {
                for sub in subs.flatten() {
                    let sp = sub.path();
                    let stem = sp.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    if sp.extension().and_then(|s| s.to_str()) == Some("jsonl")
                        && stem.starts_with("agent-")
                    {
                        out.push(Source::ClaudeSubagent {
                            id: id.clone(),
                            agent: stem.trim_start_matches("agent-").to_string(),
                            path: sp,
                        });
                    }
                }
            }
            out.push(Source::ClaudeMain { id, path });
        }
    }
    let cwd = crate::canonical_path_string(root);
    let mut paths = Vec::new();
    let _ = crate::collect_codex_session_paths(&home.join(".codex").join("sessions"), &mut paths);
    for path in paths {
        let Ok(Some((id, rollout_cwd))) = crate::codex_session_meta(&path) else {
            continue;
        };
        if crate::canonical_path_string(Path::new(&rollout_cwd)) == cwd {
            out.push(Source::Codex { id, path });
        }
    }
    out
}

type CacheKey = (i64, u64);
static FILE_CACHE: OnceLock<Mutex<HashMap<PathBuf, (CacheKey, Arc<ParsedFile>)>>> = OnceLock::new();

fn file_key(path: &Path) -> Option<CacheKey> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some((mtime, m.len()))
}

/// Parse one transcript, reusing the last parse while (mtime, size) hold.
pub fn parse_source(src: &Source) -> Option<Arc<ParsedFile>> {
    let path = src.path();
    let key = file_key(path)?;
    let cache = FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(g) = cache.lock() {
        if let Some((k, v)) = g.get(path) {
            if *k == key {
                return Some(v.clone());
            }
        }
    }
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let parsed = Arc::new(match src {
        Source::ClaudeMain { id, .. } => parse_claude_text(&text, id, Thread::Main, None),
        Source::ClaudeSubagent { id, agent, .. } => {
            parse_claude_text(&text, id, Thread::Subagent, Some(agent.clone()))
        }
        Source::Codex { id, .. } => parse_codex_text(&text, id),
    });
    if let Ok(mut g) = cache.lock() {
        g.insert(path.to_path_buf(), (key, parsed.clone()));
    }
    Some(parsed)
}

// --- the report -------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStats {
    pub total: u64,
    pub peak: u64,
    pub last: u64,
    pub first: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketRow {
    pub calls: f64,
    pub context: u64,
    pub output: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRow {
    pub id: String,
    pub calls: f64,
    pub context: u64,
    pub output: u64,
    pub peak_context: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub provider: String,
    pub id: String,
    pub thread: Thread,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub models: Vec<String>,
    pub calls: usize,
    pub context: ContextStats,
    pub input: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub output: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub items: Vec<ItemRow>,
    pub chat: BucketRow,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<Quota>,
    pub unreadable: usize,
    pub compactions: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Percentiles {
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub peak: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FloorStats {
    pub p50: u64,
    pub min: u64,
    pub max: u64,
    pub sessions: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct P50P90 {
    pub p50: u64,
    pub p90: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct P50 {
    pub p50: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub provider: String,
    pub model: String,
    pub calls: usize,
    pub sessions: usize,
    pub context: Percentiles,
    /// Share of calls whose context stayed within each window.
    pub fits: BTreeMap<String, f64>,
    /// Share of sessions whose peak stayed within each window.
    pub sessions_fit: BTreeMap<String, f64>,
    pub floor: FloorStats,
    pub uncached_per_call: P50P90,
    pub cache_hit_share: f64,
    pub gap_ms: P50,
    pub output_per_call: P50,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Constrained {
    pub by_model: Vec<ModelRow>,
    pub instruction_bytes: Vec<InstructionFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub path: String,
    pub reads: u64,
    pub bytes: u64,
    pub via_shell_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_lines: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution_ {
    pub source: String,
    pub windows: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibRsNow {
    pub lines: u64,
    pub test_lines: u64,
    pub test_spans: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub root: String,
    pub providers: BTreeMap<String, ProviderStatus>,
    pub attribution: Attribution_,
    pub sessions: Vec<SessionRow>,
    pub constrained: Constrained,
    pub files: Vec<FileRow>,
    pub files_total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib_rs_now: Option<LibRsNow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<Vec<CallReading>>,
    pub ms: u128,
}

pub struct ReportInput<'a> {
    pub root: &'a str,
    pub files: Vec<Arc<ParsedFile>>,
    /// None when the claim ledger could not be opened: everything reads as chat.
    pub windows: Option<Vec<ClaimWindow>>,
    pub since_ms: Option<i64>,
    pub session_filter: Option<(String, String)>,
    pub include_calls: bool,
    pub instruction_bytes: Vec<InstructionFile>,
}

/// Nearest-rank percentile of an ascending slice: the value at rank
/// `ceil(p/100 * n)` (1-based). Empty reads as 0.
pub fn nearest_rank(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Who a call at `at_ms` belongs to: the items of the claim window it fell
/// in, or chat. `windows` must be sorted by `from_ms`.
pub fn attribute(windows: &[ClaimWindow], at_ms: i64) -> Attribution {
    let upto = windows.partition_point(|w| w.from_ms <= at_ms);
    for w in windows[..upto].iter().rev().take(16) {
        if at_ms <= w.to_ms {
            return if w.ids.is_empty() {
                Attribution::Chat
            } else {
                Attribution::Items(w.ids.clone())
            };
        }
    }
    Attribution::Chat
}

fn normalize_path(path: &str, root: &str) -> String {
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let p = path.strip_prefix(&prefix).unwrap_or(path);
    p.strip_prefix("./").unwrap_or(p).to_string()
}

pub fn build_report(
    input: ReportInput,
    spans: &mut dyn SpanSource,
    lib_rs_now: Option<LibRsNow>,
) -> Report {
    let started = std::time::Instant::now();
    let windows: Vec<ClaimWindow> = {
        let mut w = input.windows.clone().unwrap_or_default();
        w.sort_by_key(|w| w.from_ms);
        w
    };
    let since = input.since_ms.unwrap_or(i64::MIN);
    let mut sessions: Vec<SessionRow> = Vec::new();
    let mut readings: Vec<CallReading> = Vec::new();
    let mut statuses: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut file_aggs: HashMap<String, FileRow> = HashMap::new();
    let mut model_acc: HashMap<(String, String), ModelAcc> = HashMap::new();

    for pf in &input.files {
        let Some(thread) = pf.thread else { continue };
        if let Some((prov, id)) = &input.session_filter {
            if prov != pf.provider || *id != pf.session_id {
                continue;
            }
        }
        let calls: Vec<&RawCall> = pf.calls.iter().filter(|c| c.at_ms >= since).collect();
        // Reads are credited whether or not the session made calls in range.
        for r in pf.reads.iter().filter(|r| r.at_ms >= since) {
            let path = normalize_path(&r.path, input.root);
            let e = file_aggs.entry(path.clone()).or_insert_with(|| FileRow {
                path: path.clone(),
                ..Default::default()
            });
            if r.via_shell {
                e.via_shell_bytes += r.bytes;
            } else {
                e.reads += 1;
                e.bytes += r.bytes;
                if path == LIB_RS && !r.lines.is_empty() {
                    let (tl, tb) = spans
                        .spans_at(r.at_ms)
                        .map(|s| test_share(&r.lines, &s))
                        .unwrap_or((0, 0));
                    *e.lines_read.get_or_insert(0) += r.lines.len() as u64;
                    *e.test_lines.get_or_insert(0) += tl;
                    *e.test_bytes.get_or_insert(0) += tb;
                }
            }
        }
        if calls.is_empty() {
            continue;
        }
        let st = statuses.entry(pf.provider.to_string()).or_default();
        st.0 += 1;
        st.1 += calls.len();

        let mut row = SessionRow {
            provider: pf.provider.to_string(),
            id: pf.session_id.clone(),
            thread,
            agent_id: pf.agent_id.clone(),
            label: pf.label.clone(),
            models: Vec::new(),
            calls: calls.len(),
            context: ContextStats {
                first: calls[0].context,
                last: calls[calls.len() - 1].context,
                ..Default::default()
            },
            input: 0,
            cache_read: 0,
            cache_creation: 0,
            output: 0,
            first_ms: calls.iter().map(|c| c.at_ms).min().unwrap_or(0),
            last_ms: calls.iter().map(|c| c.at_ms).max().unwrap_or(0),
            items: Vec::new(),
            chat: BucketRow::default(),
            quota: pf.quota.clone(),
            unreadable: pf.unreadable,
            compactions: pf.compactions,
        };
        let mut items: BTreeMap<String, ItemRow> = BTreeMap::new();
        let mut chat_calls = 0f64;
        let (mut chat_ctx, mut chat_out) = (0f64, 0f64);
        let mut item_ctx: BTreeMap<String, f64> = BTreeMap::new();
        let mut item_out: BTreeMap<String, f64> = BTreeMap::new();
        for c in &calls {
            row.context.total += c.context;
            row.context.peak = row.context.peak.max(c.context);
            row.input += c.input;
            row.cache_read += c.cache_read;
            row.cache_creation += c.cache_creation;
            row.output += c.output;
            if !c.model.is_empty() && !row.models.contains(&c.model) {
                row.models.push(c.model.clone());
            }
            let attribution = if input.windows.is_some() {
                attribute(&windows, c.at_ms)
            } else {
                Attribution::Chat
            };
            match &attribution {
                Attribution::Items(ids) => {
                    let share = 1.0 / ids.len() as f64;
                    for id in ids {
                        let it = items.entry(id.clone()).or_insert_with(|| ItemRow {
                            id: id.clone(),
                            ..Default::default()
                        });
                        it.calls += share;
                        it.peak_context = it.peak_context.max(c.context);
                        *item_ctx.entry(id.clone()).or_default() += c.context as f64 * share;
                        *item_out.entry(id.clone()).or_default() += c.output as f64 * share;
                    }
                }
                Attribution::Chat => {
                    chat_calls += 1.0;
                    chat_ctx += c.context as f64;
                    chat_out += c.output as f64;
                }
            }
            if input.include_calls {
                readings.push(CallReading {
                    provider: pf.provider.to_string(),
                    session: pf.session_id.clone(),
                    thread,
                    agent_id: pf.agent_id.clone(),
                    model: c.model.clone(),
                    at_ms: c.at_ms,
                    context: c.context,
                    input: c.input,
                    cache_read: c.cache_read,
                    cache_creation: c.cache_creation,
                    output: c.output,
                    reasoning_output: c.reasoning,
                    attribution,
                });
            }
        }
        row.items = items
            .into_values()
            .map(|mut it| {
                it.context = item_ctx.get(&it.id).copied().unwrap_or(0.0).round() as u64;
                it.output = item_out.get(&it.id).copied().unwrap_or(0.0).round() as u64;
                it.calls = (it.calls * 100.0).round() / 100.0;
                it
            })
            .collect();
        row.chat = BucketRow {
            calls: chat_calls,
            context: chat_ctx.round() as u64,
            output: chat_out.round() as u64,
        };

        if thread == Thread::Main {
            accumulate_models(&mut model_acc, pf.provider, &pf.session_id, &calls);
        }
        sessions.push(row);
    }

    sessions.sort_by(|a, b| {
        (
            a.provider.as_str(),
            b.first_ms,
            &a.id,
            a.thread == Thread::Subagent,
        )
            .cmp(&(
                b.provider.as_str(),
                a.first_ms,
                &b.id,
                b.thread == Thread::Subagent,
            ))
    });

    let mut providers: BTreeMap<String, ProviderStatus> = BTreeMap::new();
    for name in ["claude", "codex", "opencode"] {
        let (s, c) = statuses.get(name).copied().unwrap_or((0, 0));
        providers.insert(
            name.to_string(),
            ProviderStatus::Read {
                sessions: s,
                calls: c,
            },
        );
    }
    providers.insert(
        "antigravity".to_string(),
        ProviderStatus::Unavailable {
            reason: ANTIGRAVITY_REASON.to_string(),
        },
    );

    let mut files: Vec<FileRow> = file_aggs.into_values().collect();
    files.sort_by(|a, b| {
        (b.bytes + b.via_shell_bytes, &a.path).cmp(&(a.bytes + a.via_shell_bytes, &b.path))
    });
    let files_total = files.len();
    files.truncate(FILES_CAP);

    Report {
        root: input.root.to_string(),
        providers,
        attribution: Attribution_ {
            source: if input.windows.is_some() {
                "state-mirror"
            } else {
                "unavailable"
            }
            .to_string(),
            windows: windows.len(),
        },
        constrained: Constrained {
            by_model: finish_models(model_acc),
            instruction_bytes: input.instruction_bytes,
        },
        sessions,
        files,
        files_total,
        lib_rs_now,
        calls: input.include_calls.then_some(readings),
        ms: started.elapsed().as_millis(),
    }
}

#[derive(Default)]
struct ModelAcc {
    contexts: Vec<u64>,
    uncached: Vec<u64>,
    outputs: Vec<u64>,
    gaps: Vec<u64>,
    cache_read: u64,
    context_sum: u64,
    /// session -> peak context on this model
    peaks: HashMap<String, u64>,
    /// session -> context of its first main-thread call, when on this model
    floors: HashMap<String, u64>,
}

fn accumulate_models(
    acc: &mut HashMap<(String, String), ModelAcc>,
    provider: &str,
    session: &str,
    calls: &[&RawCall],
) {
    let mut prev: Option<i64> = None;
    for (i, c) in calls.iter().enumerate() {
        let a = acc
            .entry((provider.to_string(), c.model.clone()))
            .or_default();
        a.contexts.push(c.context);
        a.uncached.push(c.context.saturating_sub(c.cache_read));
        a.outputs.push(c.output);
        a.cache_read += c.cache_read;
        a.context_sum += c.context;
        let p = a.peaks.entry(session.to_string()).or_default();
        *p = (*p).max(c.context);
        if i == 0 {
            a.floors.insert(session.to_string(), c.context);
        }
        if let Some(t) = prev {
            let gap = c.at_ms - t;
            if (0..=TURN_GAP_MS).contains(&gap) {
                a.gaps.push(gap as u64);
            }
        }
        prev = Some(c.at_ms);
    }
}

fn finish_models(acc: HashMap<(String, String), ModelAcc>) -> Vec<ModelRow> {
    let mut rows: Vec<ModelRow> = acc
        .into_iter()
        .map(|((provider, model), mut a)| {
            a.contexts.sort_unstable();
            a.uncached.sort_unstable();
            a.outputs.sort_unstable();
            a.gaps.sort_unstable();
            let n = a.contexts.len();
            let mut fits = BTreeMap::new();
            let mut sessions_fit = BTreeMap::new();
            for (name, limit) in FIT_WINDOWS {
                let within = a.contexts.iter().filter(|c| **c <= limit).count();
                fits.insert(name.to_string(), round4(within as f64 / n.max(1) as f64));
                let s_within = a.peaks.values().filter(|p| **p <= limit).count();
                sessions_fit.insert(
                    name.to_string(),
                    round4(s_within as f64 / a.peaks.len().max(1) as f64),
                );
            }
            let mut floors: Vec<u64> = a.floors.values().copied().collect();
            floors.sort_unstable();
            ModelRow {
                provider,
                model,
                calls: n,
                sessions: a.peaks.len(),
                context: Percentiles {
                    p50: nearest_rank(&a.contexts, 50.0),
                    p90: nearest_rank(&a.contexts, 90.0),
                    p99: nearest_rank(&a.contexts, 99.0),
                    peak: a.contexts.last().copied().unwrap_or(0),
                },
                fits,
                sessions_fit,
                floor: FloorStats {
                    p50: nearest_rank(&floors, 50.0),
                    min: floors.first().copied().unwrap_or(0),
                    max: floors.last().copied().unwrap_or(0),
                    sessions: floors.len(),
                },
                uncached_per_call: P50P90 {
                    p50: nearest_rank(&a.uncached, 50.0),
                    p90: nearest_rank(&a.uncached, 90.0),
                },
                cache_hit_share: round4(a.cache_read as f64 / a.context_sum.max(1) as f64),
                gap_ms: P50 {
                    p50: nearest_rank(&a.gaps, 50.0),
                },
                output_per_call: P50 {
                    p50: nearest_rank(&a.outputs, 50.0),
                },
            }
        })
        .collect();
    rows.sort_by(|a, b| (&a.provider, b.calls, &a.model).cmp(&(&b.provider, a.calls, &b.model)));
    rows
}

// --- instruction files ------------------------------------------------------

/// The seeded instruction files a session loads: CLAUDE.md, AGENTS.md and the
/// files they `@`-import (resolved from the project root), with byte sizes.
pub fn instruction_files(root: &Path) -> Vec<InstructionFile> {
    let mut out: Vec<InstructionFile> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut queue: Vec<(PathBuf, u8)> = ["CLAUDE.md", "AGENTS.md"]
        .iter()
        .map(|n| (root.join(n), 0))
        .collect();
    while let Some((path, depth)) = queue.pop() {
        if !path.is_file() || !seen.insert(path.clone()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        out.push(InstructionFile {
            path: normalize_path(&path.to_string_lossy(), &root.to_string_lossy()),
            bytes: text.len() as u64,
        });
        if depth >= 4 {
            continue;
        }
        for token in text.split_whitespace() {
            let Some(rel) = token.strip_prefix('@') else {
                continue;
            };
            let rel = rel.trim_matches(|c: char| {
                !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
            });
            if rel.is_empty() || rel.starts_with('/') {
                continue;
            }
            queue.push((root.join(rel), depth + 1));
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

// --- entry points -----------------------------------------------------------

fn lib_rs_now(root: &Path) -> Option<LibRsNow> {
    let text = std::fs::read_to_string(root.join(LIB_RS)).ok()?;
    let spans = test_spans(&text);
    let test_lines: u64 = spans.iter().map(|(s, e)| (e - s + 1) as u64).sum();
    Some(LibRsNow {
        lines: text.lines().count() as u64,
        test_lines,
        test_spans: spans.len(),
    })
}

/// The state-mirror db for `root`, read-only, without an AppHandle. The path
/// mirrors `worklist_state_db_path`: the app cache dir (the bundle identifier
/// under the platform's cache folder), `worklist-state/<project key>.db`.
pub fn state_db_path(root: &Path) -> Option<PathBuf> {
    let home = crate::home_dir()?;
    let cache = if cfg!(target_os = "macos") {
        home.join("Library").join("Caches")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".cache"))
    };
    let dir = cache.join("org.xmlui.bram").join("worklist-state");
    let canonical = root.canonicalize().ok().map(crate::strip_unc_prefix);
    [Some(root.to_path_buf()), canonical]
        .into_iter()
        .flatten()
        .map(|r| dir.join(format!("{}.db", crate::encode_path_for_filename(&r))))
        .find(|p| p.is_file())
}

fn open_state_db_read_only(root: &Path) -> Option<rusqlite::Connection> {
    let path = state_db_path(root)?;
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(250));
    Some(conn)
}

/// Everything the report needs from disk, then the report. `windows_conn` is
/// the claim ledger when the caller has one open.
pub fn run(
    root: &Path,
    since_ms: Option<i64>,
    session_filter: Option<(String, String)>,
    include_calls: bool,
    windows_conn: Option<&rusqlite::Connection>,
) -> (Report, usize) {
    let started = std::time::Instant::now();
    let sources = discover(root);
    let mut files: Vec<Arc<ParsedFile>> = sources.iter().filter_map(parse_source).collect();
    if let Some(conn) = crate::opencode_open_db() {
        files.extend(opencode_parse(&conn, root).into_iter().map(Arc::new));
    }
    let (from, to) = files
        .iter()
        .flat_map(|f| f.calls.iter().map(|c| c.at_ms))
        .fold((i64::MAX, i64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
    let windows = windows_conn.and_then(|conn| {
        if from > to {
            Some(Vec::new())
        } else {
            claims_overlapping(conn, from, to).ok()
        }
    });
    let root_s = root.to_string_lossy().into_owned();
    let n_files = files.len();
    let mut spans = GitSpans::new(root);
    let mut report = build_report(
        ReportInput {
            root: &root_s,
            files,
            windows,
            since_ms,
            session_filter,
            include_calls,
            instruction_bytes: instruction_files(root),
        },
        &mut spans,
        lib_rs_now(root),
    );
    report.ms = started.elapsed().as_millis();
    (report, n_files)
}

fn report_json(report: &Report) -> serde_json::Value {
    serde_json::to_value(report).unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
}

/// `bram --token-accounting <root> [--since <ISO>] [--calls]`.
pub fn run_cli(root: &Path, since: Option<&str>, include_calls: bool) -> serde_json::Value {
    let since_ms = since.and_then(crate::parse_iso_to_epoch_ms);
    let conn = open_state_db_read_only(root);
    let (report, _) = run(root, since_ms, None, include_calls, conn.as_ref());
    report_json(&report)
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|p| p.strip_prefix(&format!("{key}=")))
        .map(crate::percent_decode)
}

/// `GET /__token-accounting?since=&session=<provider>:<id>&calls=1`.
pub fn route<R: tauri::Runtime>(app: &tauri::AppHandle<R>, query: &str) -> Vec<u8> {
    let Some(root) = crate::project_root(Some(app)) else {
        return br#"{"error":"could not resolve project root"}"#.to_vec();
    };
    let since_ms = query_param(query, "since").and_then(|s| crate::parse_iso_to_epoch_ms(&s));
    let session_filter = query_param(query, "session").and_then(|s| {
        s.split_once(':')
            .map(|(p, i)| (p.to_string(), i.to_string()))
    });
    let include_calls = query.split('&').any(|p| p == "calls=1");
    let conn = crate::worklist_state_open(app);
    let (report, n_files) = run(
        &root,
        since_ms,
        session_filter,
        include_calls,
        conn.as_ref(),
    );
    if crate::bram_trace_enabled() {
        let calls: usize = report.sessions.iter().map(|s| s.calls).sum();
        crate::append_bram_trace_line(
            app,
            "token-accounting",
            &format!(
                "op=report sessions={} calls={} files={} ms={}",
                report.sessions.len(),
                calls,
                n_files,
                report.ms
            ),
        );
    }
    serde_json::to_vec(&report).unwrap_or_default()
}

// --- turn-end sampler -------------------------------------------------------

/// What one turn-end line says.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sample {
    pub provider: &'static str,
    pub session: String,
    pub subagent: bool,
    pub model: String,
    pub tokens: u64,
    pub turn_calls: usize,
    pub turn_output: u64,
    pub cache_read: u64,
    pub compactions: usize,
    pub agents: usize,
    pub five_hour_pct: Option<f64>,
    pub weekly_pct: Option<f64>,
}

pub fn format_sample(s: &Sample) -> String {
    let mut line = format!("op=sample provider={}", s.provider);
    if s.subagent {
        line.push_str(" thread=subagent");
    }
    line.push_str(&format!(
        " session={} model={} tokens={} turnCalls={} turnOutput={} cacheRead={}",
        s.session,
        if s.model.is_empty() {
            "(unknown)"
        } else {
            &s.model
        },
        s.tokens,
        s.turn_calls,
        s.turn_output,
        s.cache_read
    ));
    if s.subagent {
        line.push_str(&format!(" agents={}", s.agents));
    } else {
        line.push_str(&format!(" compactions={}", s.compactions));
    }
    if let Some(p) = s.five_hour_pct {
        line.push_str(&format!(" fiveHourPct={}", p));
    }
    if let Some(p) = s.weekly_pct {
        line.push_str(&format!(" weeklyPct={}", p));
    }
    line
}

enum LiveParser {
    Claude(ClaudeParser),
    Codex(CodexParser),
}

struct LiveState {
    offset: u64,
    parser: LiveParser,
    /// (calls so far, last call time) of the last line emitted.
    emitted: (usize, i64),
}

static LIVE: OnceLock<Mutex<HashMap<PathBuf, LiveState>>> = OnceLock::new();

/// Read the bytes appended to `path` since `offset` (whole lines only).
fn read_tail(path: &Path, offset: u64) -> Option<(String, u64, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let reset = len < offset;
    let start = if reset { 0 } else { offset };
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let end = buf
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    buf.truncate(end);
    Some((
        String::from_utf8_lossy(&buf).into_owned(),
        start + end as u64,
        reset,
    ))
}

/// Advance the live session's parser over what was appended, and say what the
/// turn that just ended cost. None when nothing new was recorded since the
/// last sample.
fn sample_live(path: &Path, provider: &str) -> Vec<Sample> {
    let map = LIVE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut g) = map.lock() else {
        return Vec::new();
    };
    let Some((chunk, new_offset, reset)) =
        read_tail(path, g.get(path).map(|s| s.offset).unwrap_or(0))
    else {
        return Vec::new();
    };
    let st = g.entry(path.to_path_buf()).or_insert_with(|| LiveState {
        offset: 0,
        parser: if provider == "codex" {
            LiveParser::Codex(CodexParser::new(false))
        } else {
            LiveParser::Claude(ClaudeParser::new(false))
        },
        emitted: (0, 0),
    });
    if reset {
        st.parser = if provider == "codex" {
            LiveParser::Codex(CodexParser::new(false))
        } else {
            LiveParser::Claude(ClaudeParser::new(false))
        };
        st.emitted = (0, 0);
    }
    st.offset = new_offset;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let (calls, turn_start, compactions, quota, session): (
        &Vec<RawCall>,
        (usize, i64),
        usize,
        Option<Quota>,
        String,
    ) = match &mut st.parser {
        LiveParser::Claude(p) => {
            p.feed(&chunk);
            (&p.calls, p.turn_start, p.compactions, None, stem.clone())
        }
        LiveParser::Codex(p) => {
            p.feed(&chunk);
            let id = p.session_id.clone().unwrap_or(stem.clone());
            (&p.calls, p.turn_start, p.compactions, p.quota.clone(), id)
        }
    };
    let Some(last) = calls.last() else {
        return Vec::new();
    };
    let key = (calls.len(), last.at_ms);
    if key == st.emitted {
        return Vec::new();
    }
    st.emitted = key;
    let turn: &[RawCall] = &calls[turn_start.0.min(calls.len())..];
    let mut out = vec![Sample {
        provider: if provider == "codex" {
            "codex"
        } else {
            "claude"
        },
        session: session.clone(),
        subagent: false,
        model: last.model.clone(),
        tokens: last.context,
        turn_calls: turn.len(),
        turn_output: turn.iter().map(|c| c.output).sum(),
        cache_read: calls.iter().map(|c| c.cache_read).sum(),
        compactions,
        agents: 0,
        five_hour_pct: quota
            .as_ref()
            .and_then(|q| q.primary.as_ref())
            .and_then(|w| w.used_percent),
        weekly_pct: quota
            .as_ref()
            .and_then(|q| q.secondary.as_ref())
            .and_then(|w| w.used_percent),
    }];
    if provider != "codex" {
        if let Some(sub) = claude_subagent_sample(path, &session, turn_start.1) {
            out.push(sub);
        }
    }
    out
}

/// Subagent calls recorded since the turn began, for a Claude session at
/// `path` (`<id>.jsonl`; subagents in `<id>/subagents/agent-*.jsonl`).
fn claude_subagent_sample(path: &Path, session: &str, since_ms: i64) -> Option<Sample> {
    let dir = path.with_extension("").join("subagents");
    let mut agents = 0;
    let (mut n, mut out_tokens, mut cache_read, mut last_ctx, mut last_at) =
        (0, 0u64, 0u64, 0u64, i64::MIN);
    let mut model = String::new();
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let p = entry.path();
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") || !stem.starts_with("agent-") {
            continue;
        }
        let src = Source::ClaudeSubagent {
            id: session.to_string(),
            agent: stem.trim_start_matches("agent-").to_string(),
            path: p,
        };
        let Some(pf) = parse_source(&src) else {
            continue;
        };
        let fresh: Vec<&RawCall> = pf.calls.iter().filter(|c| c.at_ms >= since_ms).collect();
        if fresh.is_empty() {
            continue;
        }
        agents += 1;
        for c in fresh {
            n += 1;
            out_tokens += c.output;
            cache_read += c.cache_read;
            if c.at_ms >= last_at {
                last_at = c.at_ms;
                last_ctx = c.context;
                model = c.model.clone();
            }
        }
    }
    (n > 0).then(|| Sample {
        provider: "claude",
        session: session.to_string(),
        subagent: true,
        model,
        tokens: last_ctx,
        turn_calls: n,
        turn_output: out_tokens,
        cache_read,
        agents,
        ..Default::default()
    })
}

/// Called where the host decides a main-thread turn has ended (Claude and
/// Codex). Reads only the live session file's tail since the last sample.
pub fn sample_turn_end<R: tauri::Runtime>(app: &tauri::AppHandle<R>, provider: &str, path: &Path) {
    if !crate::bram_trace_enabled() {
        return;
    }
    let base = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if base.starts_with("agent-") {
        return;
    }
    for s in sample_live(path, provider) {
        crate::append_bram_trace_line(app, "session-context", &format_sample(&s));
    }
}

/// OpenCode's turn end: the latest assistant message's tokens, the session's
/// cache reads so far, and the calls since the last user message.
pub fn sample_opencode_turn_end<R: tauri::Runtime>(app: &tauri::AppHandle<R>, sid: &str) {
    if !crate::bram_trace_enabled() {
        return;
    }
    let Some(conn) = crate::opencode_open_db() else {
        return;
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT type, data, time_created FROM session_message \
         WHERE session_id = ?1 AND type IN ('assistant', 'user') ORDER BY seq",
    ) else {
        return;
    };
    let Ok(rows) = stmt.query_map(rusqlite::params![sid], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    }) else {
        return;
    };
    let mut calls: Vec<RawCall> = Vec::new();
    let mut turn_start = 0;
    for (kind, data, created) in rows.flatten() {
        if kind == "user" {
            turn_start = calls.len();
        } else if let Ok(Some(c)) = opencode_call(&data, created) {
            calls.push(c);
        }
    }
    let Some(last) = calls.last() else {
        return;
    };
    let turn = &calls[turn_start.min(calls.len())..];
    let s = Sample {
        provider: "opencode",
        session: sid.to_string(),
        model: last.model.clone(),
        tokens: last.context,
        turn_calls: turn.len(),
        turn_output: turn.iter().map(|c| c.output).sum(),
        cache_read: calls.iter().map(|c| c.cache_read).sum(),
        ..Default::default()
    };
    crate::append_bram_trace_line(app, "session-context", &format_sample(&s));
}
