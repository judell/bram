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
//! - **Antigravity**: each `PLANNER_RESPONSE` record in a conversation's
//!   `transcript_full.jsonl` is one call. `input_tokens` EXCLUDES cache
//!   (observed: step 1 input 21787 beside cache_read 0, step 3 input 2547
//!   beside cache_read 19733, so the 22.3k context carried over), so `context
//!   = input_tokens + cache_read_tokens`. No record names a model: it comes
//!   from agy's own selection log (see `agy_model_selections`) and is marked
//!   `modelSource: "selection-log"` in the report.
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
    Read {
        sessions: usize,
        calls: usize,
        /// Where the model names come from when the provider's records carry
        /// none (Antigravity: `"selection-log"`).
        #[serde(rename = "modelSource", skip_serializing_if = "Option::is_none")]
        model_source: Option<&'static str>,
    },
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
    /// The provider's id for the call, unique within its source: Claude's
    /// message id, Codex's record ordinal, OpenCode's message id. The ledger's
    /// idempotency key.
    pub key: String,
    pub at_ms: i64,
    pub model: String,
    pub context: u64,
    pub input: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub output: u64,
    pub reasoning: Option<u64>,
    /// Stamped by the ledger when the call was stored; None reads attribute
    /// from the claim windows at report time.
    pub attribution: Option<Attribution>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    pub resets_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
}

/// Which lines of a file a shell command returned, when the command says.
/// `Head`, `Tail` and `All` wait for the file's length at the time; a range
/// ending at `u32::MAX` runs to the end of the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LineSpec {
    Ranges(Vec<(u32, u32)>),
    Head(u32),
    Tail(u32),
    All,
    /// Several commands in one call, each naming the file.
    Many(Vec<LineSpec>),
}

/// A file credited with bytes read into context.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ReadEvent {
    /// `<tool call id>#<n>`: unique within its source; the ledger's key.
    pub key: String,
    pub path: String,
    pub bytes: u64,
    pub via_shell: bool,
    pub at_ms: i64,
    /// (line number, bytes) of each line returned; kept only for `lib.rs`.
    pub lines: Vec<(u32, u32)>,
    /// The session's working directory when the read happened, to resolve a
    /// relative `path` at report time.
    pub cwd: Option<String>,
    /// For a shell read of a `lib.rs`: the lines the command returned, or
    /// None when it can't be told.
    pub shell_lines: Option<LineSpec>,
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
    #[serde(default)]
    cwd: Option<String>,
}

/// Just the folder a Claude session ran in.
#[derive(Debug, Default, Deserialize)]
struct CwdProbe {
    #[serde(default)]
    cwd: Option<String>,
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
    #[serde(default)]
    cwd: Option<String>,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum PendingTool {
    Read {
        path: String,
    },
    GrepContent {
        default_path: Option<String>,
    },
    GrepFiles,
    Shell {
        files: Vec<String>,
        /// The command lines, kept only when one names a `lib.rs` (its line
        /// numbers are worked out when the result arrives).
        #[serde(default)]
        cmds: Vec<String>,
        /// The folder the command ran in when it says so (`cd /abs && ...`,
        /// Codex's `workdir`).
        #[serde(default)]
        dir: Option<String>,
    },
}

fn is_lib_rs(path: &str) -> bool {
    // Windows paths use backslashes.
    path.replace('\\', "/").ends_with("src/lib.rs")
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

/// Name the reads one tool result produced `<tool call id>#<n>`.
fn key_reads(reads: &mut [ReadEvent], tool_call_id: &str) {
    for (i, r) in reads.iter_mut().enumerate() {
        r.key = format!("{tool_call_id}#{i}");
    }
}

/// A shell call waiting for its result. The command lines are kept only when
/// one names a `lib.rs`, whose line numbers the result will place.
fn shell_pending(files: Vec<String>, cmds: Vec<String>, workdir: Option<String>) -> PendingTool {
    let dir = shell_dir(&cmds, workdir);
    let cmds = if cmds.iter().any(|c| c.contains("lib.rs")) {
        cmds
    } else {
        Vec::new()
    };
    PendingTool::Shell { files, cmds, dir }
}

fn credit_result(
    pending: &PendingTool,
    text: &str,
    at_ms: i64,
    cwd: Option<&str>,
    out: &mut Vec<ReadEvent>,
) {
    let cwd = cwd.map(str::to_string);
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
                at_ms,
                lines,
                cwd,
                ..Default::default()
            });
        }
        PendingTool::GrepContent { default_path } => {
            for (path, bytes, lines) in grep_content_by_file(text, default_path.as_deref()) {
                out.push(ReadEvent {
                    path,
                    bytes,
                    at_ms,
                    lines,
                    cwd: cwd.clone(),
                    ..Default::default()
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
                    at_ms,
                    cwd: cwd.clone(),
                    ..Default::default()
                });
            }
        }
        PendingTool::Shell { files, cmds, dir } => {
            if files.is_empty() {
                return;
            }
            let n = files.len() as u64;
            let each = text.len() as u64 / n;
            let extra = text.len() as u64 - each * n;
            for (i, f) in files.iter().enumerate() {
                // A truncated result lost lines the command asked for.
                let truncated =
                    text.contains("lines truncated]") || text.contains("tokens truncated");
                let shell_lines = if f.ends_with("lib.rs") && !cmds.is_empty() && !truncated {
                    shell_line_spec(cmds, f, text)
                } else {
                    None
                };
                out.push(ReadEvent {
                    path: f.clone(),
                    bytes: each + if i == 0 { extra } else { 0 },
                    via_shell: true,
                    at_ms,
                    cwd: dir.clone().or_else(|| cwd.clone()),
                    shell_lines,
                    ..Default::default()
                });
            }
        }
    }
}

// --- which lines a shell command returned -----------------------------------

#[derive(Debug, Default, Deserialize)]
struct ShellArgs {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    command: Option<ShellCommand>,
    #[serde(default)]
    workdir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShellCommand {
    One(String),
    Argv(Vec<String>),
}

/// The command lines of one Codex shell call and its `workdir`. The call is
/// JSON arguments (`{"cmd": ...}`, `{"command": ["bash","-lc",...]}`), or the
/// JavaScript of an `exec` call that runs `tools.exec_command({cmd: "..."})`
/// one or several times; anything else is taken as the command itself.
pub fn shell_call(raw: &str) -> (Vec<String>, Option<String>) {
    if let Ok(a) = serde_json::from_str::<ShellArgs>(raw) {
        let cmd = match (a.cmd, a.command) {
            (Some(c), _) => Some(c),
            (None, Some(ShellCommand::One(c))) => Some(c),
            (None, Some(ShellCommand::Argv(v))) => match v.first().map(String::as_str) {
                Some("bash") | Some("sh") | Some("zsh") if v.len() >= 3 => v.last().cloned(),
                _ => Some(v.join(" ")),
            },
            _ => None,
        };
        if let Some(c) = cmd {
            return (vec![c], a.workdir);
        }
    }
    let mut cmds = Vec::new();
    let mut rest = raw;
    while let Some(i) = rest.find("cmd:") {
        rest = &rest[i + 4..];
        let t = rest.trim_start();
        let Some(q) = t.chars().next().filter(|c| matches!(c, '"' | '\'' | '`')) else {
            continue;
        };
        let mut lit = String::new();
        let mut chars = t[1..].chars();
        let mut used = 1;
        while let Some(c) = chars.next() {
            used += c.len_utf8();
            match c {
                '\\' => match chars.next() {
                    Some('n') => {
                        used += 1;
                        lit.push('\n');
                    }
                    Some(e) => {
                        used += e.len_utf8();
                        lit.push(e);
                    }
                    None => break,
                },
                c if c == q => break,
                c => lit.push(c),
            }
        }
        rest = &t[used.min(t.len())..];
        cmds.push(lit);
    }
    if cmds.is_empty() {
        cmds.push(raw.to_string());
    }
    (cmds, None)
}

/// The command line split into statements (`;`, `&&`, `||`, newline), each
/// into pipeline stages (`|`), each into words with quotes resolved.
/// Redirections are dropped.
#[allow(unused_assignments)]
fn shell_stages(cmd: &str) -> Vec<Vec<Vec<String>>> {
    let mut stmts: Vec<Vec<Vec<String>>> = Vec::new();
    let mut stages: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    macro_rules! end_word {
        () => {
            if in_word {
                words.push(std::mem::take(&mut cur));
                in_word = false;
            }
        };
    }
    macro_rules! end_stage {
        () => {
            end_word!();
            if !words.is_empty() {
                stages.push(std::mem::take(&mut words));
            }
        };
    }
    macro_rules! end_stmt {
        () => {
            end_stage!();
            if !stages.is_empty() {
                stmts.push(std::mem::take(&mut stages));
            }
        };
    }
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    cur.push(chars[i]);
                    i += 1;
                }
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    cur.push(chars[i]);
                    i += 1;
                }
            }
            '\\' => {
                if i + 1 < chars.len() && chars[i + 1] != '\n' {
                    in_word = true;
                    cur.push(chars[i + 1]);
                }
                i += 1;
            }
            ';' | '\n' => {
                end_stmt!();
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                end_stmt!();
                i += 1;
            }
            '&' if cur.ends_with('>') => cur.push('&'),
            '&' => {
                end_stmt!();
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                end_stmt!();
                i += 1;
            }
            '|' => {
                end_stage!();
            }
            '(' | ')' => {
                end_word!();
            }
            c if c.is_whitespace() => {
                end_word!();
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
        i += 1;
    }
    end_stmt!();
    for stmt in stmts.iter_mut() {
        for st in stmt.iter_mut() {
            *st = strip_redirects(std::mem::take(st));
        }
        stmt.retain(|st| !st.is_empty());
    }
    stmts.retain(|s| !s.is_empty());
    stmts
}

fn strip_redirects(words: Vec<String>) -> Vec<String> {
    let redirect = |w: &str| {
        let t = w.trim_start_matches(|c: char| c.is_ascii_digit());
        t.starts_with('>') || t.starts_with('<')
    };
    let mut out = Vec::new();
    let mut it = words.into_iter();
    while let Some(w) = it.next() {
        if redirect(&w) {
            // `>` alone takes the next word as its file.
            if w.trim_start_matches(|c: char| c.is_ascii_digit()).len() <= 2 && !w.contains('&') {
                let t = w.trim_start_matches(|c: char| c.is_ascii_digit());
                if t == ">" || t == ">>" || t == "<" {
                    it.next();
                }
            }
            continue;
        }
        out.push(w);
    }
    out
}

fn same_file(word: &str, target: &str) -> bool {
    word.strip_prefix("./").unwrap_or(word) == target.strip_prefix("./").unwrap_or(target)
}

/// Lines a `sed -n` script prints: `A,Bp`, `Ap`, `A,$p`, `A,+Np`, joined by
/// `;` or newlines. Anything else (a substitution, a regex address) is None.
fn sed_ranges(script: &str) -> Option<Vec<(u32, u32)>> {
    let mut out = Vec::new();
    for part in script.split([';', '\n']) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let body = p.strip_suffix('p')?.trim();
        let (a, b) = match body.split_once(',') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (body, None),
        };
        let a: u32 = a.parse().ok()?;
        let end = match b {
            None => a,
            Some("$") => u32::MAX,
            Some(b) => match b.strip_prefix('+') {
                Some(n) => a.checked_add(n.parse::<u32>().ok()?)?,
                None => b.parse().ok()?,
            },
        };
        if a == 0 || end < a {
            return None;
        }
        out.push((a, end));
    }
    (!out.is_empty()).then_some(out)
}

/// A count flag as `head` and `tail` take it: `-n N`, `-nN`, `-N`,
/// `--lines=N`. Returns (value text, files).
fn count_flag(args: &[String]) -> Option<(Option<String>, Vec<String>)> {
    let mut n: Option<String> = None;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-n" || a == "--lines" {
            n = Some(args.get(i + 1)?.clone());
            i += 1;
        } else if let Some(v) = a.strip_prefix("--lines=") {
            n = Some(v.to_string());
        } else if let Some(v) = a.strip_prefix("-n") {
            n = Some(v.to_string());
        } else if a.len() > 1 && a.starts_with('-') && a[1..].bytes().all(|b| b.is_ascii_digit()) {
            n = Some(a[1..].to_string());
        } else if a.starts_with('-') && a.len() > 1 {
            return None;
        } else {
            files.push(a.to_string());
        }
        i += 1;
    }
    Some((n, files))
}

/// What one pipeline stage that names `target` returned. `output` is the
/// call's whole result, which only a numbered grep needs.
fn stage_spec(argv: &[String], target: &str, output: &str) -> Option<LineSpec> {
    let name = argv.first()?.rsplit('/').next()?.to_string();
    let args = &argv[1..];
    match name.as_str() {
        "sed" => {
            let (mut quiet, mut scripted) = (false, false);
            let mut scripts: Vec<String> = Vec::new();
            let mut positional: Vec<String> = Vec::new();
            let mut i = 0;
            while i < args.len() {
                let a = args[i].as_str();
                if a == "--quiet" || a == "--silent" {
                    quiet = true;
                } else if let Some(e) = a.strip_prefix("--expression=") {
                    scripted = true;
                    scripts.push(e.to_string());
                } else if a.starts_with("--") {
                    if a.starts_with("--in-place") || a.starts_with("--file") {
                        return None;
                    }
                } else if a.len() > 1 && a.starts_with('-') {
                    let cluster = &a[1..];
                    for (k, ch) in cluster.char_indices() {
                        match ch {
                            'n' => quiet = true,
                            's' | 'E' | 'r' | 'u' | 'z' => {}
                            'e' => {
                                scripted = true;
                                let rest = &cluster[k + 1..];
                                if rest.is_empty() {
                                    scripts.push(args.get(i + 1)?.clone());
                                    i += 1;
                                } else {
                                    scripts.push(rest.to_string());
                                }
                                break;
                            }
                            _ => return None,
                        }
                    }
                } else {
                    positional.push(a.to_string());
                }
                i += 1;
            }
            if !scripted {
                if positional.is_empty() {
                    return None;
                }
                scripts.push(positional.remove(0));
            }
            if !quiet || positional.len() != 1 || !same_file(&positional[0], target) {
                return None;
            }
            sed_ranges(&scripts.join("\n")).map(LineSpec::Ranges)
        }
        "head" | "tail" => {
            let (n, files) = count_flag(args)?;
            if files.len() != 1 || !same_file(&files[0], target) {
                return None;
            }
            let n = n.unwrap_or_else(|| "10".to_string());
            if name == "head" {
                n.parse().ok().map(LineSpec::Head)
            } else if let Some(from) = n.strip_prefix('+') {
                let from: u32 = from.parse().ok()?;
                (from > 0).then(|| LineSpec::Ranges(vec![(from, u32::MAX)]))
            } else {
                n.parse().ok().map(LineSpec::Tail)
            }
        }
        "cat" => {
            let files: Vec<&String> = args.iter().filter(|a| a.as_str() != "-n").collect();
            (files.len() == 1 && same_file(files[0], target)).then_some(LineSpec::All)
        }
        "grep" | "egrep" | "rg" => {
            let numbered = args.iter().any(|a| {
                a == "--line-number"
                    || (a.len() > 1
                        && a.starts_with('-')
                        && !a.starts_with("--")
                        && a[1..].bytes().all(|b| b.is_ascii_alphabetic())
                        && a[1..].contains('n'))
            });
            if !numbered {
                return None;
            }
            let mut lines: Vec<u32> = Vec::new();
            for l in output.split('\n') {
                if let Some((path, no)) = split_grep_line(l, Some(target)) {
                    if path == target || path.ends_with("lib.rs") {
                        lines.push(no);
                    }
                }
            }
            lines.sort_unstable();
            lines.dedup();
            Some(LineSpec::Ranges(
                lines.into_iter().map(|n| (n, n)).collect(),
            ))
        }
        _ => None,
    }
}

/// The lines a shell call returned of `target` (the path as the command typed
/// it), or None when the commands don't say: a piped filter, `awk`, a `grep`
/// without `-n`. Several commands naming the target add up.
pub fn shell_line_spec(cmds: &[String], target: &str, output: &str) -> Option<LineSpec> {
    let mut specs: Vec<LineSpec> = Vec::new();
    for cmd in cmds {
        for stmt in shell_stages(cmd) {
            let names = |st: &Vec<String>| st.iter().skip(1).any(|w| same_file(w, target));
            if !stmt.iter().any(names) {
                continue;
            }
            // A numbered grep keeps its meaning through filters that only
            // choose among its lines.
            let filtered = stmt.len() > 1
                && stmt[0].iter().skip(1).any(|w| same_file(w, target))
                && stmt[1..].iter().all(|st| {
                    st.first()
                        .and_then(|c| c.rsplit('/').next())
                        .is_some_and(|c| matches!(c, "head" | "tail" | "sort" | "uniq"))
                })
                && !stmt[1..]
                    .iter()
                    .any(|st| st.iter().skip(1).any(|w| same_file(w, target)));
            if stmt.len() > 1 && !filtered {
                return None;
            }
            let grep = stmt[0]
                .first()
                .and_then(|c| c.rsplit('/').next())
                .is_some_and(|c| matches!(c, "grep" | "egrep" | "rg"));
            if filtered && !grep {
                return None;
            }
            specs.push(stage_spec(&stmt[0], target, output)?);
        }
    }
    match specs.len() {
        0 => None,
        1 => specs.pop(),
        _ => Some(LineSpec::Many(specs)),
    }
}

/// The absolute folder a command moved to (`cd /abs/dir && ...`).
fn shell_dir(cmds: &[String], workdir: Option<String>) -> Option<String> {
    let mut dir = workdir;
    for cmd in cmds {
        for stmt in shell_stages(cmd) {
            if let [stage] = stmt.as_slice() {
                if let [c, d] = stage.as_slice() {
                    if c == "cd" && d.starts_with('/') {
                        dir = Some(d.clone());
                    }
                }
            }
        }
    }
    dir
}

impl LineSpec {
    /// The inclusive line ranges, given the file's length when it was read
    /// (None when unknown: `Head` and `Tail` then can't be placed).
    pub fn ranges(&self, len: Option<u32>) -> Vec<(u32, u32)> {
        let clamp = |a: u32, b: u32| -> Option<(u32, u32)> {
            let b = len.map_or(b, |l| b.min(l));
            (a >= 1 && a <= b).then_some((a, b))
        };
        match self {
            LineSpec::Ranges(r) => r.iter().filter_map(|&(a, b)| clamp(a, b)).collect(),
            LineSpec::Head(n) => clamp(1, *n).into_iter().collect(),
            LineSpec::Tail(n) => match len {
                Some(l) if *n > 0 => clamp(l.saturating_sub(*n) + 1, l).into_iter().collect(),
                _ => Vec::new(),
            },
            LineSpec::All => match len {
                Some(l) => clamp(1, l).into_iter().collect(),
                None => Vec::new(),
            },
            LineSpec::Many(v) => v.iter().flat_map(|s| s.ranges(len)).collect(),
        }
    }
}

/// What a parser must remember between two passes over one growing file.
/// Stored as JSON in the ledger's `sources` row; every field is optional on
/// read so an older row still loads.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceState {
    pending: HashMap<String, PendingTool>,
    unreadable: usize,
    compactions: usize,
    quota: Option<Quota>,
    session_id: Option<String>,
    parent_id: Option<String>,
    is_subagent: bool,
    nickname: Option<String>,
    start_ordinal: Option<u64>,
    last_total: Option<u64>,
    model: String,
    line_no: u64,
    cwd: Option<String>,
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
    /// Byte offset of the line being fed; names a call that has no id.
    pub cur_offset: u64,
    /// The folder the session last reported working in.
    cwd: Option<String>,
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
        if let Some(c) = &rec.cwd {
            if self.cwd.as_deref() != Some(c.as_str()) {
                self.cwd = Some(c.clone());
            }
        }
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
            key: msg
                .id
                .clone()
                .unwrap_or_else(|| format!("@{}", self.cur_offset)),
            at_ms,
            model: msg.model.unwrap_or_default(),
            context: input + cache_read + cache_creation,
            input,
            cache_read,
            cache_creation,
            output: u.output_tokens.unwrap_or(0),
            reasoning: None,
            attribution: None,
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
            "Bash" => input.and_then(|i| i.command.as_deref()).and_then(|cmd| {
                let files = shell_named_files(cmd);
                (!files.is_empty()).then(|| shell_pending(files, vec![cmd.to_string()], None))
            }),
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
                            let before = self.reads.len();
                            credit_result(&p, &text, at_ms, self.cwd.as_deref(), &mut self.reads);
                            key_reads(&mut self.reads[before..], id);
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

    /// A parser that carries on from a stored pass (tool calls still waiting
    /// for their result, the counters), so a file can be read in pieces.
    pub fn from_state(st: &SourceState) -> Self {
        ClaudeParser {
            collect_reads: true,
            pending: st.pending.clone(),
            unreadable: st.unreadable,
            compactions: st.compactions,
            cwd: st.cwd.clone(),
            ..Default::default()
        }
    }

    pub fn state(&self) -> SourceState {
        SourceState {
            pending: self.pending.clone(),
            unreadable: self.unreadable,
            compactions: self.compactions,
            cwd: self.cwd.clone(),
            ..Default::default()
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
    cwd: Option<String>,
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
                if self.cwd.is_none() {
                    self.cwd = p.cwd.clone();
                }
            }
            "turn_context" => {
                if p.cwd.is_some() {
                    self.cwd = p.cwd.clone();
                }
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
            // The ordinal alone restarts when a rollout is rewritten in
            // place; with the record's own time, a new record never lands on
            // an old row and a replay of the same record still does.
            key: format!("{ordinal}:{at_ms}"),
            at_ms,
            model: self.model.clone(),
            context,
            input: context.saturating_sub(cache_read + cache_creation),
            cache_read,
            cache_creation,
            output: u.output_tokens.unwrap_or(0),
            reasoning: u.reasoning_output_tokens,
            attribution: None,
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
                // The raw text, and the commands decoded from it: a JSON
                // `{"command": [...]}` or the quoted `cmd:` of an exec call
                // hides its file names from a split on spaces.
                let (cmds, workdir) = shell_call(&cmd);
                let mut files = shell_named_files(&cmd);
                for c in &cmds {
                    for f in shell_named_files(c) {
                        if !files.contains(&f) {
                            files.push(f);
                        }
                    }
                }
                if !files.is_empty() {
                    self.pending.insert(id, shell_pending(files, cmds, workdir));
                }
            }
            Some("function_call_output") | Some("custom_tool_call_output") => {
                if let Some(pend) = self.pending.remove(&id) {
                    let text = p.output.as_ref().map(|c| c.text()).unwrap_or_default();
                    let before = self.reads.len();
                    credit_result(&pend, &text, at_ms, self.cwd.as_deref(), &mut self.reads);
                    key_reads(&mut self.reads[before..], &id);
                }
            }
            _ => {}
        }
    }

    /// A parser that carries on from a stored pass.
    pub fn from_state(st: &SourceState) -> Self {
        CodexParser {
            collect_reads: true,
            pending: st.pending.clone(),
            unreadable: st.unreadable,
            compactions: st.compactions,
            quota: st.quota.clone(),
            session_id: st.session_id.clone(),
            parent_id: st.parent_id.clone(),
            is_subagent: st.is_subagent,
            nickname: st.nickname.clone(),
            start_ordinal: st.start_ordinal,
            last_total: st.last_total,
            model: st.model.clone(),
            line_no: st.line_no,
            cwd: st.cwd.clone(),
            ..Default::default()
        }
    }

    pub fn state(&self) -> SourceState {
        SourceState {
            pending: self.pending.clone(),
            unreadable: self.unreadable,
            compactions: self.compactions,
            quota: self.quota.clone(),
            session_id: self.session_id.clone(),
            parent_id: self.parent_id.clone(),
            is_subagent: self.is_subagent,
            nickname: self.nickname.clone(),
            start_ordinal: self.start_ordinal,
            last_total: self.last_total,
            model: self.model.clone(),
            line_no: self.line_no,
            cwd: self.cwd.clone(),
        }
    }

    /// (session, thread, agent) this rollout reports under.
    pub fn identity(&self, fallback_id: &str) -> (String, Thread, Option<String>) {
        let own = self
            .session_id
            .clone()
            .unwrap_or_else(|| fallback_id.to_string());
        match (self.is_subagent, &self.parent_id) {
            (true, Some(parent)) => (parent.clone(), Thread::Subagent, Some(own)),
            (true, None) => (own.clone(), Thread::Subagent, Some(own)),
            _ => (own, Thread::Main, None),
        }
    }

    pub fn into_parsed(self, fallback_id: &str) -> ParsedFile {
        let (session, thread, agent) = self.identity(fallback_id);
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
        key: String::new(),
        at_ms: d.time.and_then(|t| t.created).unwrap_or(created_fallback),
        model: d.model.and_then(|m| m.id).unwrap_or_default(),
        context: input + cache_read + cache_creation,
        input,
        cache_read,
        cache_creation,
        output: t.output.unwrap_or(0),
        reasoning: t.reasoning,
        attribution: None,
    }))
}

/// (id, parent id) of this project's OpenCode sessions.
fn opencode_sessions(conn: &rusqlite::Connection, root: &Path) -> Vec<(String, Option<String>)> {
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
    rows.flatten().collect()
}

/// This project's OpenCode sessions (children as subagents of their parent).
pub fn opencode_parse(conn: &rusqlite::Connection, root: &Path) -> Vec<ParsedFile> {
    let sessions = opencode_sessions(conn, root);
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

// --- Antigravity reader ---------------------------------------------------

/// A count that agy writes as a JSON number or as a string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum AgyNum {
    Int(u64),
    Float(f64),
    Text(String),
}

impl AgyNum {
    fn get(&self) -> u64 {
        match self {
            AgyNum::Int(n) => *n,
            AgyNum::Float(f) => f.max(0.0) as u64,
            AgyNum::Text(s) => s.trim().parse::<u64>().unwrap_or(0),
        }
    }
}

/// One `transcript_full.jsonl` record; only `PLANNER_RESPONSE` ones are calls.
#[derive(Debug, Default, Deserialize)]
struct AgyRecord {
    #[serde(default)]
    step_index: Option<AgyNum>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    input_tokens: Option<AgyNum>,
    #[serde(default)]
    cache_read_tokens: Option<AgyNum>,
    #[serde(default)]
    output_tokens: Option<AgyNum>,
}

/// One model call from a transcript line, or None for any other record.
/// `input_tokens` excludes cache, so `context = input + cache_read`. The
/// model is left empty: the caller fills it from the selection log.
pub fn agy_call(line: &str) -> Result<Option<RawCall>, ()> {
    let r: AgyRecord = serde_json::from_str(line).map_err(|_| ())?;
    if r.kind.as_deref() != Some("PLANNER_RESPONSE") {
        return Ok(None);
    }
    let Some(step) = r.step_index else {
        return Err(());
    };
    let input = r.input_tokens.map(|n| n.get()).unwrap_or(0);
    let cache_read = r.cache_read_tokens.map(|n| n.get()).unwrap_or(0);
    let at_ms = r
        .created_at
        .as_deref()
        .and_then(crate::parse_iso_to_epoch_ms)
        .ok_or(())?;
    Ok(Some(RawCall {
        key: step.get().to_string(),
        at_ms,
        model: String::new(),
        context: input + cache_read,
        input,
        cache_read,
        cache_creation: 0,
        output: r.output_tokens.map(|n| n.get()).unwrap_or(0),
        reasoning: None,
        attribution: None,
    }))
}

/// A model chosen in agy, as its own log records it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelSelection {
    pub at_ms: i64,
    pub label: String,
}

/// Local calendar fields of a glog timestamp (`I1112 13:45:58.188560`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlogStamp {
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
}

const AGY_SELECTION_MARK: &str = "Propagating selected model override to backend: label=\"";

/// The glog time and model label of a selection line.
pub fn parse_agy_selection_line(line: &str) -> Option<(GlogStamp, String)> {
    let label_at = line.find(AGY_SELECTION_MARK)? + AGY_SELECTION_MARK.len();
    let label = line[label_at..].split('"').next()?.to_string();
    if label.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let head = parts.next()?;
    let clock = parts.next()?;
    let md = head.get(1..5)?;
    let num = |s: &str| s.parse::<u32>().ok();
    let (hms, frac) = clock.split_once('.').unwrap_or((clock, "0"));
    let mut hms = hms.split(':');
    let millis = format!("{frac:0<3}").get(..3).and_then(num)?;
    Some((
        GlogStamp {
            month: num(md.get(..2)?)?,
            day: num(md.get(2..)?)?,
            hour: num(hms.next()?)?,
            minute: num(hms.next()?)?,
            second: num(hms.next()?)?,
            millis,
        },
        label,
    ))
}

/// `cli-YYYYMMDD_HHMMSS.log` -> the year the log's lines belong to.
pub fn agy_log_year(file_name: &str) -> Option<i32> {
    file_name
        .strip_prefix("cli-")?
        .get(..4)?
        .parse::<i32>()
        .ok()
}

/// Local wall-clock time to epoch ms. agy's log carries no zone, so the
/// machine's zone is the only guess; off Unix the fields read as UTC.
fn local_stamp_ms(year: i32, s: GlogStamp) -> Option<i64> {
    #[cfg(unix)]
    {
        // SAFETY: tm is plain data and mktime only reads and normalizes it.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = year - 1900;
        tm.tm_mon = s.month as i32 - 1;
        tm.tm_mday = s.day as i32;
        tm.tm_hour = s.hour as i32;
        tm.tm_min = s.minute as i32;
        tm.tm_sec = s.second as i32;
        tm.tm_isdst = -1;
        let t = unsafe { libc::mktime(&mut tm) };
        (t != -1).then(|| t as i64 * 1000 + s.millis as i64)
    }
    #[cfg(not(unix))]
    {
        let month = time::Month::try_from(s.month as u8).ok()?;
        let date = time::Date::from_calendar_date(year, month, s.day as u8).ok()?;
        let t = time::Time::from_hms(s.hour as u8, s.minute as u8, s.second as u8).ok()?;
        Some(
            (time::PrimitiveDateTime::new(date, t)
                .assume_utc()
                .unix_timestamp()
                * 1000)
                + s.millis as i64,
        )
    }
}

/// Every model selection in agy's logs under `data_dir/log`, oldest first.
pub fn agy_model_selections_in(data_dir: &Path) -> Vec<ModelSelection> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(data_dir.join("log")) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.ends_with(".log") {
            continue;
        }
        let Some(year) = agy_log_year(name) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Some((stamp, label)) = parse_agy_selection_line(line) {
                if let Some(at_ms) = local_stamp_ms(year, stamp) {
                    out.push(ModelSelection { at_ms, label });
                }
            }
        }
    }
    out.sort_by_key(|s| s.at_ms);
    out
}

fn agy_model_selections() -> Vec<ModelSelection> {
    antigravity_data_dir_here()
        .map(|d| agy_model_selections_in(&d))
        .unwrap_or_default()
}

/// The last selection at or before `at_ms`, else "unknown".
pub fn agy_model_at(selections: &[ModelSelection], at_ms: i64) -> String {
    selections
        .iter()
        .rev()
        .find(|s| s.at_ms <= at_ms)
        .map(|s| s.label.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

fn antigravity_data_dir_here() -> Option<PathBuf> {
    Some(crate::home_dir()?.join(".gemini").join("antigravity-cli"))
}

/// An agy conversation of this project; `parent` set for a subagent.
#[derive(Clone, Debug, PartialEq)]
pub struct AgyConversation {
    pub id: String,
    pub parent: Option<String>,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A `file://` URI as a path string.
fn file_uri_path(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("file://")?;
    let mut path = percent_decode(rest);
    let b = path.as_bytes();
    if b.len() > 2 && b[0] == b'/' && b[2] == b':' && b[1].is_ascii_alphabetic() {
        path.remove(0);
    }
    Some(path)
}

/// `workspace_uris` is a JSON array of `file://` URIs (observed:
/// `["file:///Users/jonudell/bram"]`); a bare comma list also reads.
pub fn workspace_uris_include(uris: &str, canonical_root: &str) -> bool {
    let list: Vec<String> = serde_json::from_str(uris)
        .unwrap_or_else(|_| uris.split(',').map(|s| s.trim().to_string()).collect());
    list.iter()
        .filter_map(|u| file_uri_path(u))
        .any(|p| crate::canonical_path_string(Path::new(&p)) == canonical_root)
}

/// This project's conversations in `conversation_summaries`.
pub fn agy_conversations(conn: &rusqlite::Connection, root: &Path) -> Vec<AgyConversation> {
    let canonical = crate::canonical_path_string(root);
    let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, workspace_uris, parent_conversation_id \
         FROM conversation_summaries",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten()
        .filter(|(_, uris, _)| {
            uris.as_deref()
                .is_some_and(|u| workspace_uris_include(u, &canonical))
        })
        .map(|(id, _, parent)| AgyConversation {
            id,
            parent: parent.filter(|p| !p.is_empty()),
        })
        .collect()
}

/// agy keeps the database in WAL mode, so a plain read-only open sees the
/// newest rows; the immutable URI is the fallback when that cannot open.
fn open_agy_summaries(data_dir: &Path) -> Option<rusqlite::Connection> {
    use rusqlite::OpenFlags;
    let db = data_dir.join("conversation_summaries.db");
    if !db.is_file() {
        return None;
    }
    rusqlite::Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()
        .or_else(|| {
            let uri = format!("file:{}?mode=ro&immutable=1", db.to_string_lossy());
            rusqlite::Connection::open_with_flags(
                uri,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            )
            .ok()
        })
}

/// The transcript files of one conversation: the main file, else its chunks.
fn agy_transcript_files(data_dir: &Path, id: &str) -> Vec<PathBuf> {
    let logs = data_dir
        .join("brain")
        .join(id)
        .join(".system_generated")
        .join("logs");
    let main = logs.join("transcript_full.jsonl");
    if main.is_file() {
        return vec![main];
    }
    let mut chunks: Vec<PathBuf> = std::fs::read_dir(logs.join("chunks").join("transcript_full"))
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
                .collect()
        })
        .unwrap_or_default();
    chunks.sort();
    chunks
}

pub fn antigravity_sources_in(data_dir: &Path, root: &Path) -> Vec<Source> {
    let Some(conn) = open_agy_summaries(data_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for conv in agy_conversations(&conn, root) {
        for path in agy_transcript_files(data_dir, &conv.id) {
            out.push(Source::Antigravity {
                id: conv.id.clone(),
                parent: conv.parent.clone(),
                path,
            });
        }
    }
    out
}

fn antigravity_sources(root: &Path) -> Vec<Source> {
    antigravity_data_dir_here()
        .map(|d| antigravity_sources_in(&d, root))
        .unwrap_or_default()
}

fn agy_ident(id: &str, parent: Option<&str>) -> Ident {
    Ident {
        provider: Provider::Antigravity,
        session: parent.unwrap_or(id).to_string(),
        thread: if parent.is_some() {
            Thread::Subagent
        } else {
            Thread::Main
        },
        agent_id: parent.map(|_| id.to_string()),
        label: None,
    }
}

fn fetch_antigravity(
    chunk: &str,
    state: &SourceState,
    id: &str,
    parent: Option<&str>,
    models: &[ModelSelection],
) -> Fetched {
    let mut st = state.clone();
    let mut calls: Vec<RawCall> = Vec::new();
    feed_lines(chunk, 0, |line, _| {
        if line.trim().is_empty() {
            return;
        }
        match agy_call(line) {
            Ok(Some(mut c)) => {
                c.model = agy_model_at(models, c.at_ms);
                // A step written twice is one call: the later record wins.
                match calls.iter_mut().find(|x| x.key == c.key) {
                    Some(prev) => *prev = c,
                    None => calls.push(c),
                }
            }
            Ok(None) => {}
            Err(()) => st.unreadable += 1,
        }
    });
    Fetched {
        key: String::new(),
        path: None,
        ident: agy_ident(id, parent),
        size: 0,
        mtime_ms: 0,
        inode: 0,
        read_to: 0,
        state: st,
        quota: None,
        calls,
        reads: Vec::new(),
    }
}

pub fn parse_antigravity_text(
    text: &str,
    id: &str,
    parent: Option<&str>,
    models: &[ModelSelection],
) -> ParsedFile {
    let f = fetch_antigravity(text, &SourceState::default(), id, parent, models);
    ParsedFile {
        provider: "antigravity",
        session_id: f.ident.session,
        thread: Some(f.ident.thread),
        agent_id: f.ident.agent_id,
        calls: f.calls,
        unreadable: f.state.unreadable,
        ..Default::default()
    }
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

/// `src-tauri/src/lib.rs` as it stood at one commit, set against the file at
/// HEAD.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommitView {
    /// Lines in the file at that commit.
    pub len: u32,
    hunks: Vec<Hunk>,
}

/// One `git diff -U0` hunk: `old_count` lines at `old_start` became `new_count`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hunk {
    old_start: u32,
    old_count: u32,
    new_count: u32,
}

impl Hunk {
    /// The first old line the hunk's shift applies to.
    fn after(&self) -> u32 {
        if self.old_count > 0 {
            self.old_start + self.old_count
        } else {
            self.old_start + 1
        }
    }
}

impl CommitView {
    /// From the old file's length and the text of
    /// `git diff -U0 <commit> HEAD -- <file>`.
    pub fn from_diff(len: u32, diff: &str) -> Self {
        let mut hunks = Vec::new();
        for l in diff.lines() {
            let Some(rest) = l.strip_prefix("@@ -") else {
                continue;
            };
            let Some((old, rest)) = rest.split_once(" +") else {
                continue;
            };
            let Some((new, _)) = rest.split_once(" @@") else {
                continue;
            };
            let pair = |t: &str| -> Option<(u32, u32)> {
                match t.split_once(',') {
                    Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
                    None => Some((t.parse().ok()?, 1)),
                }
            };
            if let (Some((os, oc)), Some((_, nc))) = (pair(old), pair(new)) {
                hunks.push(Hunk {
                    old_start: os,
                    old_count: oc,
                    new_count: nc,
                });
            }
        }
        hunks.sort_by_key(|h| h.old_start);
        CommitView { len, hunks }
    }

    /// The hunks as 12 bytes each, for the map store.
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.hunks.len() * 12);
        for h in &self.hunks {
            for n in [h.old_start, h.old_count, h.new_count] {
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
        out
    }

    fn decode(len: u32, bytes: &[u8]) -> Option<Self> {
        if bytes.len() % 12 != 0 {
            return None;
        }
        let n = |c: &[u8]| u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        let hunks = bytes
            .chunks_exact(12)
            .map(|c| Hunk {
                old_start: n(&c[0..4]),
                old_count: n(&c[4..8]),
                new_count: n(&c[8..12]),
            })
            .collect();
        Some(CommitView { len, hunks })
    }

    /// Where old line `line` is in the file now: None when the diff deleted
    /// or rewrote it.
    #[cfg(test)]
    pub fn map(&self, line: u32) -> Option<u32> {
        let k = self.hunks.partition_point(|h| h.old_start <= line);
        if k > 0 {
            let h = self.hunks[k - 1];
            if h.old_count > 0 && line < h.old_start + h.old_count {
                return None;
            }
        }
        let n = self.hunks.partition_point(|h| h.after() <= line);
        let shift: i64 = self.hunks[..n]
            .iter()
            .map(|h| h.new_count as i64 - h.old_count as i64)
            .sum();
        u32::try_from(line as i64 + shift).ok().filter(|l| *l >= 1)
    }

    /// `map` over the inclusive range `a..=b`, in order, in one sweep.
    pub fn map_range(&self, a: u32, b: u32, mut f: impl FnMut(Option<u32>)) {
        let mut shift: i64 = 0;
        let mut n = 0;
        while n < self.hunks.len() && self.hunks[n].after() <= a {
            shift += self.hunks[n].new_count as i64 - self.hunks[n].old_count as i64;
            n += 1;
        }
        let mut k = self.hunks.partition_point(|h| h.old_start <= a);
        for line in a..=b {
            while k < self.hunks.len() && self.hunks[k].old_start <= line {
                k += 1;
            }
            while n < self.hunks.len() && self.hunks[n].after() <= line {
                shift += self.hunks[n].new_count as i64 - self.hunks[n].old_count as i64;
                n += 1;
            }
            let inside = k > 0 && {
                let h = self.hunks[k - 1];
                h.old_count > 0 && line < h.old_start + h.old_count
            };
            f(if inside {
                None
            } else {
                u32::try_from(line as i64 + shift).ok().filter(|l| *l >= 1)
            });
        }
    }
}

/// Where the report gets lib.rs's test spans, and its lines, for a call's time.
pub trait SpanSource {
    fn spans_at(&mut self, at_ms: i64) -> Option<Arc<Vec<(u32, u32)>>>;
    /// The file at the commit current at `at_ms`, mapped onto the file at HEAD.
    fn view_at(&mut self, _at_ms: i64) -> Option<Arc<CommitView>> {
        None
    }
    /// Get ready for views at these times (the git calls run together).
    fn warm(&mut self, _times: &[i64]) {}
}

/// Line maps kept between runs, in a small database beside the ledger. A row
/// is one commit's file set against one HEAD blob; a new HEAD blob drops the
/// old rows. Deleting the file costs only the time to compute them again.
pub struct MapStore {
    conn: rusqlite::Connection,
}

impl MapStore {
    pub fn open(path: &Path) -> Option<MapStore> {
        let conn = rusqlite::Connection::open(path).ok()?;
        let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS line_maps (
                head_blob TEXT NOT NULL, sha TEXT NOT NULL,
                len INTEGER NOT NULL, hunks BLOB NOT NULL,
                PRIMARY KEY (head_blob, sha)
             );",
        )
        .ok()?;
        Some(MapStore { conn })
    }

    fn forget_other_heads(&self, head_blob: &str) {
        let _ = self
            .conn
            .execute("DELETE FROM line_maps WHERE head_blob != ?1", [head_blob]);
    }

    fn get(&self, head_blob: &str, sha: &str) -> Option<CommitView> {
        self.conn
            .query_row(
                "SELECT len, hunks FROM line_maps WHERE head_blob = ?1 AND sha = ?2",
                [head_blob, sha],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .ok()
            .and_then(|(len, b)| CommitView::decode(len as u32, &b))
    }

    fn put(&mut self, head_blob: &str, rows: &[(String, Arc<CommitView>)]) {
        let Ok(tx) = self.conn.transaction() else {
            return;
        };
        for (sha, v) in rows {
            let _ = tx.execute(
                "INSERT OR REPLACE INTO line_maps (head_blob, sha, len, hunks) VALUES (?1,?2,?3,?4)",
                rusqlite::params![head_blob, sha, v.len as i64, v.encode()],
            );
        }
        let _ = tx.commit();
    }
}

/// The spans of `src-tauri/src/lib.rs` at the last commit at or before the
/// call, and its lines against HEAD, from git; each is computed once per
/// commit.
pub struct GitSpans {
    root: PathBuf,
    commits: Option<Vec<(String, i64)>>,
    views: HashMap<String, Option<Arc<CommitView>>>,
    spans: HashMap<String, Option<Arc<Vec<(u32, u32)>>>>,
    store: Option<MapStore>,
    head_blob: Option<Option<String>>,
}

fn git_output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = crate::hidden_command("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// Lines in a file's bytes, as `str::lines` counts them.
fn count_lines(b: &[u8]) -> u32 {
    let nl = b.iter().filter(|c| **c == b'\n').count();
    (nl + usize::from(!b.is_empty() && b.last() != Some(&b'\n'))) as u32
}

/// The file at `sha` against HEAD.
fn commit_view(root: &Path, sha: &str) -> Option<CommitView> {
    let blob = git_output(root, &["show", &format!("{sha}:{LIB_RS}")])?;
    let diff = git_output(
        root,
        &[
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            sha,
            "HEAD",
            "--",
            LIB_RS,
        ],
    )
    .unwrap_or_default();
    Some(CommitView::from_diff(
        count_lines(&blob),
        &String::from_utf8_lossy(&diff),
    ))
}

/// `src-tauri/src/lib.rs` as committed at HEAD, as the count the coverage is
/// measured against.
pub fn lib_rs_at_head(root: &Path) -> Option<LibRsNow> {
    let blob = git_output(root, &["show", &format!("HEAD:{LIB_RS}")])?;
    let text = String::from_utf8_lossy(&blob).into_owned();
    let spans = test_spans(&text);
    Some(LibRsNow {
        lines: text.lines().count() as u64,
        test_lines: spans.iter().map(|(s, e)| (e - s + 1) as u64).sum(),
        test_spans: spans.len(),
        source: Some(Arc::new(text)),
    })
}

impl GitSpans {
    pub fn new(root: &Path) -> Self {
        GitSpans {
            root: root.to_path_buf(),
            commits: None,
            views: HashMap::new(),
            spans: HashMap::new(),
            store: None,
            head_blob: None,
        }
    }

    /// Keep line maps between runs in the database at `path`.
    pub fn with_store(mut self, path: &Path) -> Self {
        self.store = MapStore::open(path);
        self
    }

    /// The last commit touching lib.rs at or before the time.
    fn sha_at(&mut self, at_ms: i64) -> Option<String> {
        if self.commits.is_none() {
            let log = git_output(&self.root, &["log", "--format=%H %ct", "--", LIB_RS])
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
        self.commits
            .as_ref()?
            .iter()
            .find(|(_, ct)| ct * 1000 <= at_ms)
            .map(|(s, _)| s.clone())
    }

    fn head_blob(&mut self) -> Option<String> {
        if self.head_blob.is_none() {
            let id = git_output(&self.root, &["rev-parse", &format!("HEAD:{LIB_RS}")])
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
                .filter(|s| !s.is_empty());
            self.head_blob = Some(id);
        }
        self.head_blob.clone().flatten()
    }
}

impl SpanSource for GitSpans {
    fn spans_at(&mut self, at_ms: i64) -> Option<Arc<Vec<(u32, u32)>>> {
        let sha = self.sha_at(at_ms)?;
        if let Some(v) = self.spans.get(&sha) {
            return v.clone();
        }
        let blob = git_output(&self.root, &["show", &format!("{sha}:{LIB_RS}")]);
        let v = blob.map(|b| Arc::new(test_spans(&String::from_utf8_lossy(&b))));
        self.spans.insert(sha, v.clone());
        v
    }

    fn view_at(&mut self, at_ms: i64) -> Option<Arc<CommitView>> {
        let sha = self.sha_at(at_ms)?;
        if let Some(v) = self.views.get(&sha) {
            return v.clone();
        }
        let v = commit_view(&self.root, &sha).map(Arc::new);
        self.views.insert(sha, v.clone());
        v
    }

    fn warm(&mut self, times: &[i64]) {
        let mut shas: Vec<String> = times.iter().filter_map(|t| self.sha_at(*t)).collect();
        shas.sort();
        shas.dedup();
        shas.retain(|s| !self.views.contains_key(s));
        if shas.is_empty() {
            return;
        }
        let head = self.head_blob();
        if let (Some(store), Some(head)) = (self.store.as_ref(), head.as_ref()) {
            store.forget_other_heads(head);
            for sha in &shas {
                if let Some(v) = store.get(head, sha) {
                    self.views.insert(sha.clone(), Some(Arc::new(v)));
                }
            }
            shas.retain(|s| !self.views.contains_key(s));
        }
        if shas.is_empty() {
            return;
        }
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 8);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let done: Mutex<Vec<(String, Option<Arc<CommitView>>)>> = Mutex::new(Vec::new());
        std::thread::scope(|sc| {
            for _ in 0..workers {
                sc.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(sha) = shas.get(i) else { break };
                    let v = commit_view(&self.root, sha).map(Arc::new);
                    if let Ok(mut d) = done.lock() {
                        d.push((sha.clone(), v));
                    }
                });
            }
        });
        let done = done.into_inner().unwrap_or_default();
        if let (Some(head), Some(store)) = (head.as_ref(), self.store.as_mut()) {
            let rows: Vec<(String, Arc<CommitView>)> = done
                .iter()
                .filter_map(|(s, v)| v.clone().map(|v| (s.clone(), v)))
                .collect();
            store.put(head, &rows);
        }
        self.views.extend(done);
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
    /// One Antigravity transcript file of a conversation (`parent` set for a
    /// subagent conversation).
    Antigravity {
        id: String,
        parent: Option<String>,
        path: PathBuf,
    },
}

impl Source {
    fn path(&self) -> &Path {
        match self {
            Source::ClaudeMain { path, .. }
            | Source::ClaudeSubagent { path, .. }
            | Source::Codex { path, .. }
            | Source::Antigravity { path, .. } => path,
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
    out.extend(antigravity_sources(root));
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
        Source::Antigravity { id, parent, .. } => {
            parse_antigravity_text(&text, id, parent.as_deref(), &agy_model_selections())
        }
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

/// The median first-call context of this agent's sessions in folders that
/// carry no instruction files, on this machine.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NativeFloor {
    pub p50: u64,
    pub sessions: usize,
}

/// Bram's seeded conventions as the project loads them. The token figure is
/// bytes / 4, an estimate, not a measurement.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConventionsPart {
    pub bytes: u64,
    pub approx_tokens: u64,
}

/// The floor in three parts: the agent's own instructions (measured), Bram's
/// conventions (sized from the file) and the project's own (the rest).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorParts {
    pub native: Option<NativeFloor>,
    /// "same model", "same provider, other models" or "not measured".
    pub native_basis: String,
    pub conventions: ConventionsPart,
    /// Approximate tokens; null when `native` is.
    pub project: Option<u64>,
}

/// First main-thread call of a session that ran in a folder with no
/// instruction files.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeSample {
    pub provider: String,
    pub model: String,
    pub context: u64,
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
    pub floor_parts: FloorParts,
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
    /// The file's size on disk now; null when it is gone.
    pub size_bytes: Option<u64>,
    /// `reads` and `bytes` split by thread (they add up to those).
    pub main_reads: u64,
    pub main_bytes: u64,
    pub subagent_reads: u64,
    pub subagent_bytes: u64,
    /// Distinct sessions that read the file; a subagent counts for its parent.
    pub sessions: u64,
    /// The most bytes any one session (main plus subagents) read of it.
    pub max_session_bytes: u64,
    /// Shell commands that read the file (their bytes are `via_shell_bytes`).
    pub shell_reads: u64,
}

/// First and last model call the report covers.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRange {
    pub from_ms: i64,
    pub to_ms: i64,
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
    /// The file's text, for the coverage regions; not part of the JSON.
    #[serde(skip)]
    pub source: Option<Arc<String>>,
}

/// Reads whose file is not in this project's checkout: another worktree, a
/// path outside the root, or a spelling that can't be resolved.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Elsewhere {
    pub reads: u64,
    pub shell_reads: u64,
    pub bytes: u64,
    pub via_shell_bytes: u64,
    pub paths: usize,
}

/// What a provider's reads and calls cover, so each figure can name its dates.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRange {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<CallRange>,
    /// The Read and Grep tools (Claude only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_tool: Option<CallRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<CallRange>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinesShare {
    pub lines: u64,
    pub of: u64,
    pub share: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionFn {
    pub name: String,
    pub line_reads: u64,
}

/// One of the busiest 200-line stretches.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub from_line: u32,
    pub to_line: u32,
    pub line_reads: u64,
    pub distinct_lines: u64,
    pub top_fns: Vec<RegionFn>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    pub test: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadToolCover {
    /// Read and content-Grep results for the file.
    pub reads: u64,
    /// Of those, how many returned numbered lines.
    pub with_lines: u64,
    pub distinct: LinesShare,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<CallRange>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellCover {
    pub reads: u64,
    /// Reads whose command says which lines it returned.
    pub placed: u64,
    pub unplaced: u64,
    pub placed_bytes: u64,
    pub unplaced_bytes: u64,
    pub distinct: LinesShare,
    /// First and last shell read, by provider.
    pub ranges: BTreeMap<String, CallRange>,
}

/// How much of a file the agents ever read, by line number mapped onto the
/// file now.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub path: String,
    pub file_lines: u64,
    pub read_tool: ReadToolCover,
    pub shell: ShellCover,
    /// Read by either way.
    pub combined: LinesShare,
    /// Read both ways.
    pub both: LinesShare,
    /// Line readings that landed on a line of the file now.
    pub line_reads: u64,
    pub reads_per_line: f64,
    /// Lines read three or more times.
    pub read_3_plus: u64,
    /// Line readings the diff to now deleted or rewrote.
    pub since_changed_line_reads: u64,
    /// Line readings with no commit to map from.
    pub unmapped_line_reads: u64,
    pub test: LinesShare,
    pub non_test: LinesShare,
    pub regions: Vec<Region>,
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
    /// First and last call covered; null when there were none.
    pub range: Option<CallRange>,
    /// Per provider: the dates of its calls, Read-tool reads and shell reads.
    pub provider_ranges: BTreeMap<String, ProviderRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib_rs_now: Option<LibRsNow>,
    pub coverage: Vec<Coverage>,
    pub elsewhere: Elsewhere,
    /// Relative paths read in a session with no recorded folder, taken as the
    /// project's.
    pub cwd_unknown_reads: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<Vec<CallReading>>,
    pub ms: u128,
}

#[derive(Default)]
pub struct ReportInput<'a> {
    pub root: &'a str,
    pub files: Vec<Arc<ParsedFile>>,
    /// None when the claim ledger could not be opened: everything reads as chat.
    pub windows: Option<Vec<ClaimWindow>>,
    pub since_ms: Option<i64>,
    pub session_filter: Option<(String, String)>,
    pub include_calls: bool,
    pub instruction_bytes: Vec<InstructionFile>,
    /// First calls of sessions in folders with no instruction files.
    pub native_samples: Vec<NativeSample>,
    /// Bytes of Bram's seeded conventions the project loads, per provider.
    pub conventions_bytes: BTreeMap<String, u64>,
    /// The user's home folder, to expand `~` in a read's path.
    pub home: Option<String>,
    /// `lib.rs` at HEAD, the file the coverage is measured against; without
    /// it the `lib_rs_now` argument is used.
    pub coverage_now: Option<LibRsNow>,
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

/// `a/./b/../c` as `a/c`; the leading `/` is kept, and so is a drive (`..`
/// never pops it).
fn clean_path(p: &str) -> String {
    if has_drive(p) {
        return format!("{}{}", &p[..2], clean_path(&p[2..]));
    }
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if p.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

#[derive(Debug, PartialEq)]
enum Resolved {
    Project(String),
    Elsewhere,
}

/// A read's path as the project's relative path, or `Elsewhere`. `~` expands
/// to `home`; a relative path is taken from the session's folder `cwd`. With
/// no folder recorded it is taken from the root, and the second value says so.
/// A diff-prefixed spelling (`a/src-tauri/...`) that isn't a folder here is
/// not guessed at.
fn resolve_read_path(
    raw: &str,
    cwd: Option<&str>,
    root: &str,
    home: Option<&str>,
) -> (Resolved, bool) {
    // Windows spellings are folded to `/` first, so one path logic serves both.
    let root_s = root.replace('\\', "/");
    let cwd_s = cwd.map(|c| c.replace('\\', "/"));
    let home_s = home.map(|h| h.replace('\\', "/"));
    let mut p = raw.replace('\\', "/");
    // A drive on the root means a Windows filesystem: case-insensitive, and
    // Git Bash may spell `C:/x` as `/c/x`.
    let win = has_drive(&root_s);
    if let Some(h) = &home_s {
        if p == "~" || p.starts_with("~/") {
            p = format!("{}{}", h.trim_end_matches('/'), &p[1..]);
        }
    }
    if win && p.starts_with('/') {
        let b = p.as_bytes();
        if b.len() >= 2 && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/') {
            p = format!("{}:{}", b[1] as char, &p[2..]);
        }
    }
    let mut unknown = false;
    if !p.starts_with('/') && !has_drive(&p) {
        let first = p.split('/').next().unwrap_or("");
        if matches!(first, "a" | "b") && p.contains('/') && !Path::new(root).join(first).exists() {
            return (Resolved::Elsewhere, false);
        }
        p = match &cwd_s {
            Some(c) => format!("{}/{}", c.trim_end_matches('/'), p),
            None => {
                unknown = true;
                format!("{}/{}", root_s.trim_end_matches('/'), p)
            }
        };
    }
    let p = clean_path(&p);
    let root_c = clean_path(&root_s);
    let prefix = format!("{}/", root_c.trim_end_matches('/'));
    let rel = if win {
        // ASCII lowering keeps byte offsets, so the slice below stays valid.
        if p.to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
        {
            Some(&p[prefix.len()..])
        } else {
            None
        }
    } else {
        p.strip_prefix(&prefix)
    };
    match rel {
        Some(rel) if !rel.is_empty() => (Resolved::Project(rel.to_string()), unknown),
        _ => (Resolved::Elsewhere, false),
    }
}

/// Starts with a drive letter and colon (`C:` or `C:/...`).
fn has_drive(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/')
}

/// Stretch a range to take in `t`.
fn widen(slot: &mut Option<CallRange>, t: i64) {
    *slot = Some(match slot.take() {
        Some(r) => CallRange {
            from_ms: r.from_ms.min(t),
            to_ms: r.to_ms.max(t),
        },
        None => CallRange {
            from_ms: t,
            to_ms: t,
        },
    });
}

/// One read of `lib.rs`, kept for the coverage count.
struct LibEvent {
    at_ms: i64,
    provider: &'static str,
    via_shell: bool,
    bytes: u64,
    /// (line number, bytes) a Read or content Grep returned.
    lines: Vec<(u32, u32)>,
    /// What a shell command returned.
    spec: Option<LineSpec>,
}

const REGION_LINES: usize = 200;
const REGIONS: usize = 10;

/// A `fn` or `mod` of the file now: name and the lines it spans.
struct CodeItem {
    name: String,
    start: usize,
    end: usize,
}

/// The name after `fn` or `mod` on a line that declares one.
fn item_head(trimmed: &str) -> Option<(bool, String)> {
    let mut words = trimmed.split_whitespace().peekable();
    while let Some(w) = words.peek() {
        let skip = matches!(
            *w,
            "pub" | "async" | "const" | "unsafe" | "extern" | "default"
        ) || w.starts_with("pub(")
            || w.starts_with('"');
        if !skip {
            break;
        }
        words.next();
    }
    let kw = words.next()?;
    let is_fn = match kw {
        "fn" => true,
        "mod" => false,
        _ => return None,
    };
    let name: String = words
        .next()?
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some((is_fn, name))
}

/// The functions and modules of Rust source with the lines they span,
/// found by indentation: an item ends at the first line that is a lone `}`
/// at its own indent.
fn code_items(src: &str) -> (Vec<CodeItem>, Vec<CodeItem>) {
    let lines: Vec<&str> = src.lines().collect();
    let (mut fns, mut mods) = (Vec::new(), Vec::new());
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        let Some((is_fn, name)) = item_head(t) else {
            continue;
        };
        let indent = l.len() - t.len();
        let mut end = None;
        if t.ends_with(';') {
            // A bodiless `mod name;` declares a file module elsewhere: it
            // encloses nothing here, so it is not a region's module.
            if !is_fn {
                continue;
            }
            end = Some(i);
        } else if t.ends_with('}') && t.contains('{') {
            end = Some(i);
        } else {
            let cap = if is_fn { 6000 } else { lines.len() };
            for (j, m) in lines.iter().enumerate().skip(i + 1).take(cap) {
                let mt = m.trim_start();
                if m.len() - mt.len() == indent && mt.starts_with('}') {
                    end = Some(j);
                    break;
                }
            }
        }
        let Some(end) = end else { continue };
        let item = CodeItem {
            name,
            start: i + 1,
            end: end + 1,
        };
        if is_fn {
            fns.push(item);
        } else {
            mods.push(item);
        }
    }
    (fns, mods)
}

/// Which item (index + 1, 0 for none) each line, 1-based, falls in; an inner
/// item wins over the one around it.
fn owners(items: &[CodeItem], lines: usize) -> Vec<u32> {
    let mut own = vec![0u32; lines + 2];
    for (k, it) in items.iter().enumerate() {
        for l in it.start..=it.end.min(lines) {
            own[l] = k as u32 + 1;
        }
    }
    own
}

/// How much of today's `lib.rs` the agents read, by line.
fn lib_coverage(
    events: &[LibEvent],
    spans: &mut dyn SpanSource,
    now: &LibRsNow,
) -> Option<Coverage> {
    let n = now.lines as usize;
    if n == 0 {
        return None;
    }
    let times: Vec<i64> = events.iter().map(|e| e.at_ms).collect();
    spans.warm(&times);
    let mut hits = vec![0u32; n + 2];
    let mut by_read = vec![false; n + 2];
    let mut by_shell = vec![false; n + 2];
    let mut out = Coverage {
        path: LIB_RS.to_string(),
        file_lines: now.lines,
        ..Default::default()
    };
    for ev in events {
        let view = spans.view_at(ev.at_ms);
        let mut mark = |old: Option<u32>, hits: &mut Vec<u32>, flag: &mut Vec<bool>| match old {
            Some(l) if (l as usize) <= n => {
                hits[l as usize] = hits[l as usize].saturating_add(1);
                flag[l as usize] = true;
                out.line_reads += 1;
            }
            _ => out.since_changed_line_reads += 1,
        };
        // Line ranges (old numbering) this read returned.
        let ranges: Vec<(u32, u32)> = if ev.via_shell {
            out.shell.reads += 1;
            match &ev.spec {
                Some(spec) => {
                    out.shell.placed += 1;
                    out.shell.placed_bytes += ev.bytes;
                    spec.ranges(view.as_ref().map(|v| v.len))
                }
                None => {
                    out.shell.unplaced += 1;
                    out.shell.unplaced_bytes += ev.bytes;
                    Vec::new()
                }
            }
        } else {
            out.read_tool.reads += 1;
            if !ev.lines.is_empty() {
                out.read_tool.with_lines += 1;
            }
            ev.lines.iter().map(|&(l, _)| (l, l)).collect()
        };
        let slot = if ev.via_shell {
            Some(
                out.shell
                    .ranges
                    .entry(ev.provider.to_string())
                    .or_insert(CallRange {
                        from_ms: ev.at_ms,
                        to_ms: ev.at_ms,
                    }),
            )
        } else {
            None
        };
        if let Some(r) = slot {
            r.from_ms = r.from_ms.min(ev.at_ms);
            r.to_ms = r.to_ms.max(ev.at_ms);
        } else {
            widen(&mut out.read_tool.range, ev.at_ms);
        }
        let flag = if ev.via_shell {
            &mut by_shell
        } else {
            &mut by_read
        };
        for (a, b) in ranges {
            match &view {
                Some(v) => v.map_range(a, b, |m| mark(m, &mut hits, flag)),
                // No commit to map from: the readings can't be placed.
                None if b == u32::MAX => {}
                None => out.unmapped_line_reads += (b - a + 1) as u64,
            }
        }
    }
    let share = |lines: u64| LinesShare {
        lines,
        of: now.lines,
        share: round4(lines as f64 / now.lines as f64),
    };
    let count = |f: &dyn Fn(usize) -> bool| (1..=n).filter(|&l| f(l)).count() as u64;
    out.read_tool.distinct = share(count(&|l| by_read[l]));
    out.shell.distinct = share(count(&|l| by_shell[l]));
    out.combined = share(count(&|l| by_read[l] || by_shell[l]));
    out.both = share(count(&|l| by_read[l] && by_shell[l]));
    out.read_3_plus = count(&|l| hits[l] >= 3);
    out.reads_per_line = if out.combined.lines > 0 {
        round4(out.line_reads as f64 / out.combined.lines as f64)
    } else {
        0.0
    };
    // Test code against the rest, by today's spans.
    let text = now.source.as_deref().map(String::as_str).unwrap_or("");
    let spans_now = test_spans(text);
    let mut is_test = vec![false; n + 2];
    for (s, e) in &spans_now {
        for l in (*s as usize)..=(*e as usize).min(n) {
            is_test[l] = true;
        }
    }
    let test_total = count(&|l| is_test[l]);
    let test_read = count(&|l| is_test[l] && (by_read[l] || by_shell[l]));
    let non_total = now.lines.saturating_sub(test_total);
    let non_read = out.combined.lines.saturating_sub(test_read);
    out.test = LinesShare {
        lines: test_read,
        of: test_total,
        share: round4(test_read as f64 / test_total.max(1) as f64),
    };
    out.non_test = LinesShare {
        lines: non_read,
        of: non_total,
        share: round4(non_read as f64 / non_total.max(1) as f64),
    };
    // The busiest stretches, greedily, none overlapping.
    let mut pre = vec![0u64; n + 2];
    for l in 1..=n {
        pre[l] = pre[l - 1] + hits[l] as u64;
    }
    let (fns, mods) = code_items(text);
    let (fn_own, mod_own) = (owners(&fns, n), owners(&mods, n));
    let width = REGION_LINES.min(n);
    let mut taken = vec![false; n + 2];
    for _ in 0..REGIONS {
        let mut best: Option<(u64, usize)> = None;
        for s in 1..=(n + 1 - width) {
            let e = s + width - 1;
            if taken[s] || taken[e] {
                continue;
            }
            let sum = pre[e] - pre[s - 1];
            if sum > 0 && best.map_or(true, |(b, _)| sum > b) {
                best = Some((sum, s));
            }
        }
        let Some((sum, s)) = best else { break };
        let e = s + width - 1;
        for t in s.saturating_sub(width - 1).max(1)..=e {
            taken[t] = true;
        }
        let mut per_fn: HashMap<u32, u64> = HashMap::new();
        let mut per_mod: HashMap<u32, u64> = HashMap::new();
        for l in s..=e {
            if fn_own[l] > 0 {
                *per_fn.entry(fn_own[l]).or_default() += hits[l] as u64;
            }
            if mod_own[l] > 0 {
                *per_mod.entry(mod_own[l]).or_default() += hits[l] as u64;
            }
        }
        let mut top: Vec<(u32, u64)> = per_fn.into_iter().filter(|(_, h)| *h > 0).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        // Name a module only when it holds most of the stretch's readings;
        // a stretch that merely runs into one is not "in" it.
        let module = per_mod
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .filter(|(_, h)| h * 2 > sum)
            .map(|(k, _)| mods[k as usize - 1].name.clone());
        out.regions.push(Region {
            from_line: s as u32,
            to_line: e as u32,
            line_reads: sum,
            distinct_lines: (s..=e).filter(|&l| hits[l] > 0).count() as u64,
            top_fns: top
                .into_iter()
                .take(3)
                .map(|(k, h)| RegionFn {
                    name: fns[k as usize - 1].name.clone(),
                    line_reads: h,
                })
                .collect(),
            module,
            test: is_test[s] && is_test[e],
        });
    }
    Some(out)
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
    // path -> (provider, session) -> bytes read, main plus subagents.
    let mut file_sessions: HashMap<String, HashMap<(String, String), u64>> = HashMap::new();
    let mut range: Option<CallRange> = None;
    let mut model_acc: HashMap<(String, String), ModelAcc> = HashMap::new();
    let mut stored_attribution = false;
    let mut provider_ranges: BTreeMap<String, ProviderRange> = BTreeMap::new();
    let mut elsewhere = Elsewhere::default();
    let mut elsewhere_paths: std::collections::HashSet<String> = Default::default();
    let mut cwd_unknown_reads = 0u64;
    let mut lib_events: Vec<LibEvent> = Vec::new();

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
            let pr = provider_ranges.entry(pf.provider.to_string()).or_default();
            let slot = if r.via_shell {
                &mut pr.shell
            } else {
                &mut pr.read_tool
            };
            widen(slot, r.at_ms);
            let (resolved, unknown) =
                resolve_read_path(&r.path, r.cwd.as_deref(), input.root, input.home.as_deref());
            let Resolved::Project(path) = resolved else {
                if r.via_shell {
                    elsewhere.shell_reads += 1;
                    elsewhere.via_shell_bytes += r.bytes;
                } else {
                    elsewhere.reads += 1;
                    elsewhere.bytes += r.bytes;
                }
                elsewhere_paths.insert(r.path.clone());
                continue;
            };
            if unknown {
                cwd_unknown_reads += 1;
            }
            let e = file_aggs.entry(path.clone()).or_insert_with(|| FileRow {
                path: path.clone(),
                ..Default::default()
            });
            if path == LIB_RS {
                lib_events.push(LibEvent {
                    at_ms: r.at_ms,
                    provider: pf.provider,
                    via_shell: r.via_shell,
                    bytes: r.bytes,
                    lines: r.lines.clone(),
                    spec: r.shell_lines.clone(),
                });
            }
            if r.via_shell {
                e.via_shell_bytes += r.bytes;
                e.shell_reads += 1;
            } else {
                e.reads += 1;
                e.bytes += r.bytes;
                if thread == Thread::Main {
                    e.main_reads += 1;
                    e.main_bytes += r.bytes;
                } else {
                    e.subagent_reads += 1;
                    e.subagent_bytes += r.bytes;
                }
                *file_sessions
                    .entry(path.clone())
                    .or_default()
                    .entry((pf.provider.to_string(), pf.session_id.clone()))
                    .or_default() += r.bytes;
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
        let lo = calls.iter().map(|c| c.at_ms).min().unwrap_or(0);
        let hi = calls.iter().map(|c| c.at_ms).max().unwrap_or(0);
        widen(
            &mut provider_ranges
                .entry(pf.provider.to_string())
                .or_default()
                .calls,
            lo,
        );
        widen(
            &mut provider_ranges
                .entry(pf.provider.to_string())
                .or_default()
                .calls,
            hi,
        );
        range = Some(match range {
            Some(r) => CallRange {
                from_ms: r.from_ms.min(lo),
                to_ms: r.to_ms.max(hi),
            },
            None => CallRange {
                from_ms: lo,
                to_ms: hi,
            },
        });
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
            let attribution = match &c.attribution {
                Some(a) => {
                    stored_attribution = true;
                    a.clone()
                }
                None if input.windows.is_some() => attribute(&windows, c.at_ms),
                None => Attribution::Chat,
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
    for name in ["claude", "codex", "opencode", "antigravity"] {
        let (s, c) = statuses.get(name).copied().unwrap_or((0, 0));
        providers.insert(
            name.to_string(),
            ProviderStatus::Read {
                sessions: s,
                calls: c,
                model_source: (name == "antigravity").then_some("selection-log"),
            },
        );
    }

    let mut files: Vec<FileRow> = file_aggs.into_values().collect();
    for f in files.iter_mut() {
        if let Some(per) = file_sessions.get(&f.path) {
            f.sessions = per.len() as u64;
            f.max_session_bytes = per.values().copied().max().unwrap_or(0);
        }
    }
    files.sort_by(|a, b| {
        (b.bytes + b.via_shell_bytes, &a.path).cmp(&(a.bytes + a.via_shell_bytes, &b.path))
    });
    let files_total = files.len();
    files.truncate(FILES_CAP);
    let root_dir = Path::new(input.root);
    for f in files.iter_mut() {
        f.size_bytes = std::fs::metadata(root_dir.join(&f.path))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
    }

    elsewhere.paths = elsewhere_paths.len();
    let coverage: Vec<Coverage> = input
        .coverage_now
        .as_ref()
        .or(lib_rs_now.as_ref())
        .and_then(|now| lib_coverage(&lib_events, spans, now))
        .into_iter()
        .collect();

    Report {
        root: input.root.to_string(),
        providers,
        attribution: Attribution_ {
            source: if input.windows.is_some() {
                "state-mirror"
            } else if stored_attribution {
                "ledger"
            } else {
                "unavailable"
            }
            .to_string(),
            windows: windows.len(),
        },
        constrained: Constrained {
            by_model: finish_models(model_acc, &input.native_samples, &input.conventions_bytes),
            instruction_bytes: input.instruction_bytes,
        },
        sessions,
        files,
        files_total,
        range,
        provider_ranges,
        lib_rs_now,
        coverage,
        elsewhere,
        cwd_unknown_reads,
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

/// The agent's own floor: the median first-call context of its sessions in
/// folders with no instruction files. Prefers the same model; falls back to
/// the provider's other models, saying so.
pub fn native_floor(
    samples: &[NativeSample],
    provider: &str,
    model: &str,
) -> (Option<NativeFloor>, &'static str) {
    let median = |mut v: Vec<u64>| -> Option<NativeFloor> {
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        Some(NativeFloor {
            p50: nearest_rank(&v, 50.0),
            sessions: v.len(),
        })
    };
    let same: Vec<u64> = samples
        .iter()
        .filter(|s| s.provider == provider && s.model == model)
        .map(|s| s.context)
        .collect();
    if let Some(n) = median(same) {
        return (Some(n), "same model");
    }
    let any: Vec<u64> = samples
        .iter()
        .filter(|s| s.provider == provider)
        .map(|s| s.context)
        .collect();
    match median(any) {
        Some(n) => (Some(n), "same provider, other models"),
        None => (None, "not measured"),
    }
}

/// Split a measured floor into the agent's own, Bram's conventions (bytes / 4)
/// and the project's remainder.
pub fn floor_parts(
    floor_p50: u64,
    native: Option<NativeFloor>,
    basis: &str,
    conventions_bytes: u64,
) -> FloorParts {
    let approx = (conventions_bytes as f64 / 4.0).round() as u64;
    let project = native
        .as_ref()
        .map(|n| floor_p50.saturating_sub(n.p50 + approx));
    FloorParts {
        native,
        native_basis: basis.to_string(),
        conventions: ConventionsPart {
            bytes: conventions_bytes,
            approx_tokens: approx,
        },
        project,
    }
}

fn finish_models(
    acc: HashMap<(String, String), ModelAcc>,
    native_samples: &[NativeSample],
    conventions_bytes: &BTreeMap<String, u64>,
) -> Vec<ModelRow> {
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
            let floor_p50 = nearest_rank(&floors, 50.0);
            let (native, basis) = native_floor(native_samples, &provider, &model);
            let mut floor_parts = floor_parts(
                floor_p50,
                native,
                basis,
                conventions_bytes.get(&provider).copied().unwrap_or(0),
            );
            if floors.is_empty() {
                // No session started on this model: nothing to split.
                floor_parts.project = None;
            }
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
                floor_parts,
                floor: FloorStats {
                    p50: floor_p50,
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

// --- the floor's parts ------------------------------------------------------

/// Lines read from the head of a transcript while looking for its first call.
const NATIVE_HEAD_LINES: usize = 3000;

/// True when `dir` or any folder above it holds one of `names` (or, for
/// `CLAUDE.md`, `.claude/CLAUDE.md`): the instruction files an agent started
/// there would load.
fn has_instruction_file(dir: &Path, names: &[&str]) -> bool {
    dir.ancestors().any(|a| {
        names.iter().any(|n| a.join(n).is_file())
            || (names.contains(&"CLAUDE.md") && a.join(".claude").join("CLAUDE.md").is_file())
    })
}

/// First main-thread call of a Claude transcript and the folder it ran in,
/// reading only its head.
fn claude_head(path: &Path) -> Option<(String, RawCall)> {
    use std::io::BufRead;
    let reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut parser = ClaudeParser::new(false);
    let mut cwd: Option<String> = None;
    for line in reader.lines().take(NATIVE_HEAD_LINES) {
        let Ok(line) = line else { break };
        if cwd.is_none() {
            cwd = serde_json::from_str::<CwdProbe>(&line)
                .ok()
                .and_then(|p| p.cwd);
        }
        parser.feed_line(&line);
        if !parser.calls.is_empty() {
            break;
        }
    }
    Some((cwd?, parser.calls.into_iter().next()?))
}

/// First call of a Codex main rollout, reading only its head.
fn codex_head(path: &Path) -> Option<RawCall> {
    use std::io::BufRead;
    let reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut parser = CodexParser::new(false);
    for line in reader.lines().take(NATIVE_HEAD_LINES) {
        let Ok(line) = line else { break };
        parser.feed_line(&line);
        if !parser.calls.is_empty() {
            break;
        }
    }
    if parser.is_subagent {
        return None;
    }
    parser.calls.into_iter().next()
}

/// The first call of every main-thread session on this machine whose folder
/// still exists and has no instruction files: the agent's own floor, with
/// no project or Bram text in it. Reads only each transcript's head.
pub fn native_samples(home: &Path) -> Vec<NativeSample> {
    let mut out = Vec::new();
    if let Ok(dirs) = std::fs::read_dir(home.join(".claude").join("projects")) {
        for dir in dirs.flatten() {
            let Ok(files) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                    continue;
                }
                let Some((cwd, call)) = claude_head(&p) else {
                    continue;
                };
                let cwd = Path::new(&cwd);
                if !cwd.is_dir() || has_instruction_file(cwd, &["CLAUDE.md", "CLAUDE.local.md"]) {
                    continue;
                }
                out.push(NativeSample {
                    provider: "claude".to_string(),
                    model: call.model,
                    context: call.context,
                });
            }
        }
    }
    let mut paths = Vec::new();
    let _ = crate::collect_codex_session_paths(&home.join(".codex").join("sessions"), &mut paths);
    for p in paths {
        let Ok(Some((_, cwd))) = crate::codex_session_meta(&p) else {
            continue;
        };
        let cwd = Path::new(&cwd);
        if !cwd.is_dir() || has_instruction_file(cwd, &["AGENTS.md", "AGENTS.override.md"]) {
            continue;
        }
        if let Some(call) = codex_head(&p) {
            out.push(NativeSample {
                provider: "codex".to_string(),
                model: call.model,
                context: call.context,
            });
        }
    }
    out
}

/// The text between Bram's `<!-- bram:start -->` and `<!-- bram:end -->`.
pub fn bram_marker_block(text: &str) -> Option<&str> {
    let start = text.find("<!-- bram:start -->")?;
    let end = text[start..].find("<!-- bram:end -->")? + start;
    Some(&text[start..end + "<!-- bram:end -->".len()])
}

/// Bytes of the seeded Bram conventions each provider loads from `root`.
///
/// - Claude: `.claude/bram-conventions.md`, which `CLAUDE.md` imports; in the
///   Bram source repo (no seeded copy) `app/__shell/conventions.md`.
/// - Codex: the Bram block in `AGENTS.md`. Codex is only pointed at the
///   conventions file, and the gate prose in its `developer_instructions`
///   is already in every Codex session here, so it sits in the agent's part.
pub fn conventions_bytes(root: &Path) -> BTreeMap<String, u64> {
    let size = |rel: &str| {
        std::fs::metadata(root.join(rel))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    };
    let claude = size(".claude/bram-conventions.md").or_else(|| {
        if root.join("src-tauri").join("Cargo.toml").is_file() {
            size("app/__shell/conventions.md")
        } else {
            None
        }
    });
    let codex = std::fs::read_to_string(root.join("AGENTS.md"))
        .ok()
        .and_then(|t| bram_marker_block(&t).map(|b| b.len() as u64));
    let mut out = BTreeMap::new();
    out.insert("claude".to_string(), claude.unwrap_or(0));
    out.insert("codex".to_string(), codex.unwrap_or(0));
    out
}

// --- the ledger -------------------------------------------------------------
//
// issue token-accounting-ledger-in-sqlite: the providers' own files are only
// as durable as their cleanup policies (Claude deletes transcripts after 60
// days, OpenCode's store was recreated). The ledger keeps every call and read
// once seen, in `<app_local_data_dir>/token-ledger/<project key>.db`, beside
// the search index (durable stores leave the cache directory). The report is
// built from the ledger after ingesting whatever is new on disk, so a call
// stays counted when its source is gone.
//
// - `calls` / `reads` are keyed `(source_key, call_key | read_key)`; re-reading
//   a stretch is harmless. A streamed Claude message is revised in place (the
//   last record's usage wins, the first record's time stays).
// - `sources` records how far each file was read (`read_to`, a byte offset;
//   OpenCode: the last `time_updated`) with its size, mtime and inode, plus the
//   parser state a later pass resumes from. A file that shrank or changed
//   identity is re-read from the start.
// - Item attribution is stamped at insert from the claim ledger, but only once
//   a call is `ATTRIBUTION_SETTLE_MS` old, and never for a call at or after the
//   start of the oldest claim still open: an open claim reads as 1 ms wide, so
//   a call stamped against it would be `chat` for good. Those calls stay NULL
//   and attribute from the windows at report time until a later pass stamps
//   them (see `stamp_cutoff`).

/// 2: Codex call keys became `<ordinal>:<at_ms>`; `open` rewrites v1 rows.
/// 4: Windows reads of lib.rs were stored without their lines (backslash
/// paths); `open` re-reads every transcript so the reads gain them.
pub const LEDGER_VERSION: i64 = 4;
const ATTRIBUTION_SETTLE_MS: i64 = 10 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provider {
    Claude,
    Codex,
    OpenCode,
    Antigravity,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::OpenCode => "opencode",
            Provider::Antigravity => "antigravity",
        }
    }
    fn parse(s: &str) -> Option<Provider> {
        match s {
            "claude" => Some(Provider::Claude),
            "codex" => Some(Provider::Codex),
            "opencode" => Some(Provider::OpenCode),
            "antigravity" => Some(Provider::Antigravity),
            _ => None,
        }
    }
}

impl Thread {
    fn as_str(self) -> &'static str {
        match self {
            Thread::Main => "main",
            Thread::Subagent => "subagent",
        }
    }
    fn parse(s: &str) -> Option<Thread> {
        match s {
            "main" => Some(Thread::Main),
            "subagent" => Some(Thread::Subagent),
            _ => None,
        }
    }
}

fn attribution_to_db(a: &Attribution) -> String {
    match a {
        Attribution::Chat => "chat".to_string(),
        Attribution::Items(ids) => serde_json::to_string(ids).unwrap_or_else(|_| "chat".into()),
    }
}

fn attribution_from_db(s: &str) -> Option<Attribution> {
    if s == "chat" {
        return Some(Attribution::Chat);
    }
    serde_json::from_str::<Vec<String>>(s)
        .ok()
        .map(Attribution::Items)
}

/// Who a source file's records belong to.
#[derive(Clone, Debug)]
struct Ident {
    provider: Provider,
    session: String,
    thread: Thread,
    agent_id: Option<String>,
    label: Option<String>,
}

/// One source read this pass, ready to store.
struct Fetched {
    key: String,
    path: Option<String>,
    ident: Ident,
    size: i64,
    mtime_ms: i64,
    inode: i64,
    read_to: i64,
    state: SourceState,
    quota: Option<Quota>,
    calls: Vec<RawCall>,
    reads: Vec<ReadEvent>,
}

/// What a source's row said last time.
struct StoredSource {
    size: i64,
    mtime_ms: i64,
    inode: i64,
    read_to: i64,
    state: SourceState,
}

/// One ingest pass. `inserted` counts rows written or revised.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IngestStats {
    pub calls_inserted: usize,
    pub reads_inserted: usize,
    /// Rows read again and found already stored.
    pub skipped: usize,
    /// Sources actually read this pass.
    pub sources: usize,
    /// Sources whose size and mtime had not moved.
    pub unchanged: usize,
    pub ms: u128,
}

impl IngestStats {
    pub fn inserted(&self) -> usize {
        self.calls_inserted + self.reads_inserted
    }
    pub fn merge(&mut self, o: &IngestStats) {
        self.calls_inserted += o.calls_inserted;
        self.reads_inserted += o.reads_inserted;
        self.skipped += o.skipped;
        self.sources += o.sources;
        self.unchanged += o.unchanged;
        self.ms += o.ms;
    }
    /// The `op=ledger` line, or None for a pass that stored nothing new.
    pub fn trace_line(&self) -> Option<String> {
        (self.inserted() > 0).then(|| self.trace_fields())
    }
    pub fn trace_fields(&self) -> String {
        format!(
            "op=ledger inserted={} skipped={} sources={} ms={} calls={} unchanged={}",
            self.inserted(),
            self.skipped,
            self.sources,
            self.ms,
            self.calls_inserted,
            self.unchanged
        )
    }
}

/// Two passes at once would only fight over the file's write lock.
static INGEST_LOCK: Mutex<()> = Mutex::new(());

pub struct Ledger {
    conn: rusqlite::Connection,
}

/// `<data dir>/token-ledger/<project key>.db`; the key is the search index's.
pub fn ledger_db_path_in(data_dir: &Path, root: &Path) -> PathBuf {
    let canonical = root
        .canonicalize()
        .map(crate::strip_unc_prefix)
        .unwrap_or_else(|_| root.to_path_buf());
    data_dir.join("token-ledger").join(format!(
        "{}.db",
        crate::encode_path_for_filename(&canonical)
    ))
}

/// The app-local data dir without an AppHandle (the CLI): what Tauri's
/// `app_local_data_dir` resolves to for the bundle identifier.
pub fn local_data_dir() -> Option<PathBuf> {
    let home = crate::home_dir()?;
    let base = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local").join("share"))
    };
    Some(base.join("org.xmlui.bram"))
}

fn file_inode(path: &Path) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).map(|m| m.ino() as i64).unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

/// Why a report was built without the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerFallback {
    OpenFailed,
    NewerSchema,
}

impl LedgerFallback {
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerFallback::OpenFailed => "open-failed",
            LedgerFallback::NewerSchema => "newer-schema",
        }
    }
    pub fn trace_fields(self) -> String {
        format!("op=ledger-fallback reason={}", self.as_str())
    }
}

impl Ledger {
    pub fn open(path: &Path) -> Option<Ledger> {
        Self::open_checked(path).ok()
    }

    pub fn open_checked(path: &Path) -> Result<Ledger, LedgerFallback> {
        use LedgerFallback::OpenFailed;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|_| OpenFailed)?;
        }
        let conn = rusqlite::Connection::open(path).map_err(|_| OpenFailed)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|_| OpenFailed)?;
        if version > LEDGER_VERSION {
            // Written by a newer Bram: leave it alone.
            return Err(LedgerFallback::NewerSchema);
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sources (
                source_key TEXT PRIMARY KEY,
                provider TEXT NOT NULL, session TEXT NOT NULL, thread TEXT NOT NULL,
                agent_id TEXT, label TEXT, path TEXT,
                size INTEGER NOT NULL DEFAULT 0, mtime_ms INTEGER NOT NULL DEFAULT 0,
                inode INTEGER NOT NULL DEFAULT 0, read_to INTEGER NOT NULL DEFAULT 0,
                unreadable INTEGER NOT NULL DEFAULT 0, compactions INTEGER NOT NULL DEFAULT 0,
                quota TEXT, state TEXT, updated_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS calls (
                id INTEGER PRIMARY KEY,
                source_key TEXT NOT NULL, call_key TEXT NOT NULL,
                provider TEXT NOT NULL, session TEXT NOT NULL, thread TEXT NOT NULL,
                agent_id TEXT, model TEXT NOT NULL, at_ms INTEGER NOT NULL,
                context INTEGER NOT NULL, input INTEGER NOT NULL,
                cache_read INTEGER NOT NULL, cache_creation INTEGER NOT NULL,
                output INTEGER NOT NULL, reasoning INTEGER,
                attribution TEXT,
                UNIQUE (source_key, call_key)
             );
             CREATE INDEX IF NOT EXISTS calls_at ON calls (at_ms);
             CREATE INDEX IF NOT EXISTS calls_unstamped ON calls (attribution) WHERE attribution IS NULL;
             CREATE TABLE IF NOT EXISTS reads (
                id INTEGER PRIMARY KEY,
                source_key TEXT NOT NULL, read_key TEXT NOT NULL,
                provider TEXT NOT NULL, session TEXT NOT NULL, thread TEXT NOT NULL,
                path TEXT NOT NULL, at_ms INTEGER NOT NULL, bytes INTEGER NOT NULL,
                via_shell INTEGER NOT NULL, lines TEXT,
                cwd TEXT, shell_lines TEXT,
                UNIQUE (source_key, read_key)
             );",
        )
        .map_err(|_| OpenFailed)?;
        if version < LEDGER_VERSION {
            if version == 1 {
                // v1 keyed a Codex call by its ordinal alone. The new key adds
                // the call's own time, which the row already holds, so the
                // rewrite is exact and loses nothing (the rollout may be gone).
                let _ = conn.execute(
                    "UPDATE calls SET call_key = call_key || ':' || at_ms \
                     WHERE provider = 'codex' AND instr(call_key, ':') = 0",
                    [],
                );
            }
            if version < 3 {
                let _ = conn.execute("ALTER TABLE reads ADD COLUMN cwd TEXT", []);
                let _ = conn.execute("ALTER TABLE reads ADD COLUMN shell_lines TEXT", []);
            }
            if version < 4 {
                // v3 stores where each read ran (to resolve a relative path)
                // and which lines a shell read returned; v4 fixes the lines of
                // Windows lib.rs reads. Rows already stored lack them, and the
                // transcripts still hold them, so read every Claude and Codex
                // transcript again from the start, once:
                // reads are upserted on their keys, calls are unchanged, and
                // the counters (unreadable lines, compactions) are recounted by
                // the full pass. A transcript that is gone keeps its rows as
                // they were: relative paths with no folder read as the
                // project's.
                let _ = conn.execute(
                    "UPDATE sources SET read_to = 0, mtime_ms = -1, state = NULL, \
                       unreadable = 0, compactions = 0 \
                     WHERE provider IN ('claude', 'codex')",
                    [],
                );
            }
            let _ = conn.pragma_update(None, "user_version", LEDGER_VERSION);
        }
        Ok(Ledger { conn })
    }

    fn stored_source(&self, key: &str) -> Option<StoredSource> {
        self.conn
            .query_row(
                "SELECT size, mtime_ms, inode, read_to, state FROM sources WHERE source_key = ?1",
                [key],
                |r| {
                    let state: Option<String> = r.get(4)?;
                    Ok(StoredSource {
                        size: r.get(0)?,
                        mtime_ms: r.get(1)?,
                        inode: r.get(2)?,
                        read_to: r.get(3)?,
                        state: state
                            .and_then(|s| serde_json::from_str(&s).ok())
                            .unwrap_or_default(),
                    })
                },
            )
            .ok()
    }

    /// Read what is new in each transcript and store it.
    pub fn ingest_sources(
        &mut self,
        sources: &[Source],
        windows_conn: Option<&rusqlite::Connection>,
    ) -> IngestStats {
        let _guard = INGEST_LOCK.lock();
        let started = std::time::Instant::now();
        let now_ms = now_epoch_ms();
        let mut stats = IngestStats::default();
        let mut agy_models: Option<Vec<ModelSelection>> = None;
        for src in sources {
            let path = src.path();
            let Some((mtime_ms, size)) = file_key(path) else {
                continue;
            };
            let key = path.to_string_lossy().into_owned();
            let stored = self.stored_source(&key);
            let inode = file_inode(path);
            if let Some(s) = &stored {
                if s.size == size as i64 && s.mtime_ms == mtime_ms && s.inode == inode {
                    stats.unchanged += 1;
                    continue;
                }
            }
            // Resume only a file that still holds what was read: not shrunk,
            // not replaced.
            let resume = stored
                .as_ref()
                .filter(|s| s.inode == inode && s.read_to as u64 <= size);
            // A re-read from the start keeps the counters: the calls and reads
            // of the old pass stay stored, so their unreadable lines,
            // compactions and quota stay counted too.
            let (offset, state) = match (resume, &stored) {
                (Some(s), _) => (s.read_to as u64, s.state.clone()),
                (None, Some(old)) => (
                    0,
                    SourceState {
                        unreadable: old.state.unreadable,
                        compactions: old.state.compactions,
                        quota: old.state.quota.clone(),
                        ..Default::default()
                    },
                ),
                (None, None) => (0, SourceState::default()),
            };
            let Some((chunk, read_to, _)) = read_tail(path, offset) else {
                continue;
            };
            let mut fetched = match src {
                Source::ClaudeMain { id, .. } => fetch_claude(
                    &chunk,
                    offset,
                    &state,
                    Ident {
                        provider: Provider::Claude,
                        session: id.clone(),
                        thread: Thread::Main,
                        agent_id: None,
                        label: None,
                    },
                ),
                Source::ClaudeSubagent { id, agent, .. } => fetch_claude(
                    &chunk,
                    offset,
                    &state,
                    Ident {
                        provider: Provider::Claude,
                        session: id.clone(),
                        thread: Thread::Subagent,
                        agent_id: Some(agent.clone()),
                        label: None,
                    },
                ),
                Source::Codex { id, .. } => fetch_codex(&chunk, offset, &state, id),
                Source::Antigravity { id, parent, .. } => {
                    let models = agy_models.get_or_insert_with(agy_model_selections);
                    fetch_antigravity(&chunk, &state, id, parent.as_deref(), models)
                }
            };
            fetched.key = key;
            fetched.path = Some(key_path(path));
            fetched.size = size as i64;
            fetched.mtime_ms = mtime_ms;
            fetched.inode = inode;
            fetched.read_to = read_to as i64;
            if fetched.quota.is_none() {
                fetched.quota = fetched.state.quota.clone();
            }
            stamp_attribution(&mut fetched.calls, windows_conn, now_ms);
            stats.sources += 1;
            self.store(fetched, now_ms, &mut stats);
        }
        stats.ms = started.elapsed().as_millis();
        stats
    }

    /// Read OpenCode messages written or revised since the last pass. With
    /// `only`, just that session (the turn-end sampler's).
    pub fn ingest_opencode(
        &mut self,
        conn: &rusqlite::Connection,
        root: &Path,
        only: Option<&str>,
        windows_conn: Option<&rusqlite::Connection>,
    ) -> IngestStats {
        let _guard = INGEST_LOCK.lock();
        let started = std::time::Instant::now();
        let now_ms = now_epoch_ms();
        let mut stats = IngestStats::default();
        for (sid, parent) in opencode_sessions(conn, root) {
            if only.is_some_and(|o| o != sid && Some(o) != parent.as_deref()) {
                continue;
            }
            let key = format!("opencode:{sid}");
            let stored = self.stored_source(&key);
            let after = stored.as_ref().map(|s| s.read_to).unwrap_or(0);
            let mut state = stored.map(|s| s.state).unwrap_or_default();
            let Ok(mut stmt) = conn.prepare(
                "SELECT id, data, time_created, time_updated FROM session_message \
                 WHERE session_id = ?1 AND type = 'assistant' AND time_updated >= ?2 ORDER BY seq",
            ) else {
                continue;
            };
            let Ok(rows) = stmt.query_map(rusqlite::params![sid, after], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            }) else {
                continue;
            };
            // `>=`: a second row or revision in the saved millisecond would be
            // skipped by `>`. The upsert makes the re-read harmless, and rows
            // in that millisecond that are already stored as they are now are
            // dropped here, so a quiet pass writes nothing.
            let prev_unreadable = state.unreadable;
            let mut calls = Vec::new();
            let mut last = after;
            for (id, data, created, updated) in rows.flatten() {
                last = last.max(updated);
                match opencode_call(&data, created) {
                    Ok(Some(mut c)) => {
                        c.key = id;
                        if updated == after && self.call_is_stored(&key, &c) {
                            continue;
                        }
                        calls.push(c);
                    }
                    Ok(None) => {}
                    // Seen last pass already when it sits on the boundary.
                    Err(()) if updated > after => state.unreadable += 1,
                    Err(()) => {}
                }
            }
            if calls.is_empty() && state.unreadable == prev_unreadable {
                stats.unchanged += 1;
                continue;
            }
            stamp_attribution(&mut calls, windows_conn, now_ms);
            stats.sources += 1;
            let ident = Ident {
                provider: Provider::OpenCode,
                session: parent.clone().unwrap_or_else(|| sid.clone()),
                thread: if parent.is_some() {
                    Thread::Subagent
                } else {
                    Thread::Main
                },
                agent_id: parent.as_ref().map(|_| sid.clone()),
                label: None,
            };
            self.store(
                Fetched {
                    key,
                    path: None,
                    ident,
                    size: 0,
                    mtime_ms: 0,
                    inode: 0,
                    read_to: last,
                    quota: None,
                    state,
                    calls,
                    reads: Vec::new(),
                },
                now_ms,
                &mut stats,
            );
        }
        stats.ms = started.elapsed().as_millis();
        stats
    }

    /// Whether `c` is stored under `source_key` exactly as it is now.
    fn call_is_stored(&self, source_key: &str, c: &RawCall) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM calls WHERE source_key = ?1 AND call_key = ?2 AND model = ?3 \
                   AND context = ?4 AND input = ?5 AND cache_read = ?6 AND cache_creation = ?7 \
                   AND output = ?8 AND reasoning IS ?9",
                rusqlite::params![
                    source_key,
                    c.key,
                    c.model,
                    c.context as i64,
                    c.input as i64,
                    c.cache_read as i64,
                    c.cache_creation as i64,
                    c.output as i64,
                    c.reasoning.map(|r| r as i64),
                ],
                |_| Ok(()),
            )
            .is_ok()
    }

    fn store(&mut self, f: Fetched, now_ms: i64, stats: &mut IngestStats) {
        let Ok(tx) = self.conn.transaction() else {
            return;
        };
        let ident = &f.ident;
        let _ = tx.execute(
            "INSERT INTO sources (source_key, provider, session, thread, agent_id, label, path, \
               size, mtime_ms, inode, read_to, unreadable, compactions, quota, state, updated_ms) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) \
             ON CONFLICT(source_key) DO UPDATE SET provider=excluded.provider, \
               session=excluded.session, thread=excluded.thread, agent_id=excluded.agent_id, \
               label=excluded.label, path=excluded.path, size=excluded.size, \
               mtime_ms=excluded.mtime_ms, inode=excluded.inode, read_to=excluded.read_to, \
               unreadable=excluded.unreadable, compactions=excluded.compactions, \
               quota=excluded.quota, state=excluded.state, updated_ms=excluded.updated_ms",
            rusqlite::params![
                f.key,
                ident.provider.as_str(),
                ident.session,
                ident.thread.as_str(),
                ident.agent_id,
                ident.label,
                f.path,
                f.size,
                f.mtime_ms,
                f.inode,
                f.read_to,
                f.state.unreadable as i64,
                f.state.compactions as i64,
                f.quota.as_ref().and_then(|q| serde_json::to_string(q).ok()),
                serde_json::to_string(&f.state).ok(),
                now_ms,
            ],
        );
        {
            let Ok(mut ins) = tx.prepare_cached(
                "INSERT INTO calls (source_key, call_key, provider, session, thread, agent_id, \
                   model, at_ms, context, input, cache_read, cache_creation, output, reasoning, \
                   attribution) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) \
                 ON CONFLICT(source_key, call_key) DO UPDATE SET model=excluded.model, \
                   context=excluded.context, input=excluded.input, cache_read=excluded.cache_read, \
                   cache_creation=excluded.cache_creation, output=excluded.output, \
                   reasoning=excluded.reasoning \
                 WHERE calls.model IS NOT excluded.model OR calls.context IS NOT excluded.context \
                   OR calls.input IS NOT excluded.input OR calls.cache_read IS NOT excluded.cache_read \
                   OR calls.cache_creation IS NOT excluded.cache_creation \
                   OR calls.output IS NOT excluded.output OR calls.reasoning IS NOT excluded.reasoning",
            ) else {
                return;
            };
            for c in &f.calls {
                let n = ins
                    .execute(rusqlite::params![
                        f.key,
                        c.key,
                        ident.provider.as_str(),
                        ident.session,
                        ident.thread.as_str(),
                        ident.agent_id,
                        c.model,
                        c.at_ms,
                        c.context as i64,
                        c.input as i64,
                        c.cache_read as i64,
                        c.cache_creation as i64,
                        c.output as i64,
                        c.reasoning.map(|r| r as i64),
                        c.attribution.as_ref().map(attribution_to_db),
                    ])
                    .unwrap_or(0);
                stats.calls_inserted += n;
                stats.skipped += 1 - n;
            }
        }
        {
            let Ok(mut ins) = tx.prepare_cached(
                "INSERT INTO reads (source_key, read_key, provider, session, thread, \
                   path, at_ms, bytes, via_shell, lines, cwd, shell_lines) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) \
                 ON CONFLICT(source_key, read_key) DO UPDATE SET cwd=excluded.cwd, \
                   shell_lines=excluded.shell_lines, lines=excluded.lines \
                 WHERE reads.cwd IS NOT excluded.cwd \
                   OR reads.shell_lines IS NOT excluded.shell_lines \
                   OR reads.lines IS NOT excluded.lines",
            ) else {
                return;
            };
            for r in &f.reads {
                let lines = (!r.lines.is_empty())
                    .then(|| serde_json::to_string(&r.lines).ok())
                    .flatten();
                let n = ins
                    .execute(rusqlite::params![
                        f.key,
                        r.key,
                        ident.provider.as_str(),
                        ident.session,
                        ident.thread.as_str(),
                        r.path,
                        r.at_ms,
                        r.bytes as i64,
                        r.via_shell as i64,
                        lines,
                        r.cwd,
                        r.shell_lines
                            .as_ref()
                            .and_then(|l| serde_json::to_string(l).ok()),
                    ])
                    .unwrap_or(0);
                stats.reads_inserted += n;
                stats.skipped += 1 - n;
            }
        }
        let _ = tx.commit();
    }

    /// Stamp the calls that were too young to stamp when stored. A no-op
    /// without the claim ledger.
    pub fn stamp_pending(&mut self, windows_conn: Option<&rusqlite::Connection>) {
        let Some(wc) = windows_conn else { return };
        let _guard = INGEST_LOCK.lock();
        let Some(cutoff) = stamp_cutoff(wc, now_epoch_ms()) else {
            return;
        };
        let rows: Vec<(i64, i64)> = {
            let Ok(mut stmt) = self
                .conn
                .prepare("SELECT id, at_ms FROM calls WHERE attribution IS NULL AND at_ms <= ?1")
            else {
                return;
            };
            let Ok(it) = stmt.query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?))) else {
                return;
            };
            it.flatten().collect()
        };
        if rows.is_empty() {
            return;
        }
        let from = rows.iter().map(|r| r.1).min().unwrap_or(0);
        let to = rows.iter().map(|r| r.1).max().unwrap_or(0);
        let Ok(mut windows) = claims_overlapping(wc, from, to) else {
            return;
        };
        windows.sort_by_key(|w| w.from_ms);
        let Ok(tx) = self.conn.transaction() else {
            return;
        };
        {
            let Ok(mut upd) = tx.prepare_cached("UPDATE calls SET attribution = ?2 WHERE id = ?1")
            else {
                return;
            };
            for (id, at) in rows {
                let _ = upd.execute(rusqlite::params![
                    id,
                    attribution_to_db(&attribute(&windows, at))
                ]);
            }
        }
        let _ = tx.commit();
    }

    /// Every stored source as a `ParsedFile`, calls and reads in the order
    /// they were first stored.
    pub fn load(&self, session_filter: Option<&(String, String)>) -> Vec<Arc<ParsedFile>> {
        let mut by_key: BTreeMap<String, ParsedFile> = BTreeMap::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT source_key, provider, session, thread, agent_id, label, unreadable, \
               compactions, quota FROM sources ORDER BY source_key",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, Option<String>>(8)?,
                ))
            }) {
                for (key, prov, session, thread, agent, label, unread, compact, quota) in
                    rows.flatten()
                {
                    let Some(provider) = Provider::parse(&prov) else {
                        continue;
                    };
                    if let Some((p, id)) = session_filter {
                        if p != provider.as_str() || *id != session {
                            continue;
                        }
                    }
                    by_key.insert(
                        key,
                        ParsedFile {
                            provider: provider.as_str(),
                            session_id: session,
                            thread: Thread::parse(&thread),
                            agent_id: agent,
                            label,
                            unreadable: unread.max(0) as usize,
                            compactions: compact.max(0) as usize,
                            quota: quota.and_then(|q| serde_json::from_str(&q).ok()),
                            ..Default::default()
                        },
                    );
                }
            }
        }
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT source_key, call_key, model, at_ms, context, input, cache_read, \
               cache_creation, output, reasoning, attribution FROM calls ORDER BY id",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    RawCall {
                        key: r.get(1)?,
                        model: r.get(2)?,
                        at_ms: r.get(3)?,
                        context: r.get::<_, i64>(4)? as u64,
                        input: r.get::<_, i64>(5)? as u64,
                        cache_read: r.get::<_, i64>(6)? as u64,
                        cache_creation: r.get::<_, i64>(7)? as u64,
                        output: r.get::<_, i64>(8)? as u64,
                        reasoning: r.get::<_, Option<i64>>(9)?.map(|x| x as u64),
                        attribution: r
                            .get::<_, Option<String>>(10)?
                            .and_then(|a| attribution_from_db(&a)),
                    },
                ))
            }) {
                for (key, call) in rows.flatten() {
                    if let Some(pf) = by_key.get_mut(&key) {
                        pf.calls.push(call);
                    }
                }
            }
        }
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT source_key, read_key, path, at_ms, bytes, via_shell, lines, cwd, \
               shell_lines FROM reads ORDER BY id",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                let lines: Option<String> = r.get(6)?;
                let shell_lines: Option<String> = r.get(8)?;
                Ok((
                    r.get::<_, String>(0)?,
                    ReadEvent {
                        key: r.get(1)?,
                        path: r.get(2)?,
                        at_ms: r.get(3)?,
                        bytes: r.get::<_, i64>(4)? as u64,
                        via_shell: r.get::<_, i64>(5)? != 0,
                        lines: lines
                            .and_then(|l| serde_json::from_str(&l).ok())
                            .unwrap_or_default(),
                        cwd: r.get(7)?,
                        shell_lines: shell_lines.and_then(|l| serde_json::from_str(&l).ok()),
                    },
                ))
            }) {
                for (key, read) in rows.flatten() {
                    if let Some(pf) = by_key.get_mut(&key) {
                        pf.reads.push(read);
                    }
                }
            }
        }
        by_key.into_values().map(Arc::new).collect()
    }
}

fn key_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The latest call time that may be stamped now: `ATTRIBUTION_SETTLE_MS` ago,
/// and before the start of the oldest claim still open. An open claim reads as
/// 1 ms wide (`claims_overlapping`), so a call inside it would be stamped
/// `chat` and never revisited. None when the claims can't be read: stamp
/// nothing rather than guess.
fn stamp_cutoff(windows_conn: &rusqlite::Connection, now_ms: i64) -> Option<i64> {
    let open_from: Option<i64> = windows_conn
        .query_row(
            "SELECT MIN(written_at_ms) FROM claims WHERE cleared_at_ms IS NULL",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let settled = now_ms - ATTRIBUTION_SETTLE_MS;
    Some(match open_from {
        Some(from) => settled.min(from - 1),
        None => settled,
    })
}

/// Stamp the settled calls from the claim ledger; the rest stay unstamped.
fn stamp_attribution(
    calls: &mut [RawCall],
    windows_conn: Option<&rusqlite::Connection>,
    now_ms: i64,
) {
    let Some(wc) = windows_conn else { return };
    let Some(cutoff) = stamp_cutoff(wc, now_ms) else {
        return;
    };
    let (from, to) = calls
        .iter()
        .filter(|c| c.at_ms <= cutoff)
        .fold((i64::MAX, i64::MIN), |(lo, hi), c| {
            (lo.min(c.at_ms), hi.max(c.at_ms))
        });
    if from > to {
        return;
    }
    let Ok(mut windows) = claims_overlapping(wc, from, to) else {
        return;
    };
    windows.sort_by_key(|w| w.from_ms);
    for c in calls.iter_mut().filter(|c| c.at_ms <= cutoff) {
        c.attribution = Some(attribute(&windows, c.at_ms));
    }
}

fn feed_lines(chunk: &str, start: u64, mut feed: impl FnMut(&str, u64)) {
    let mut pos = 0u64;
    for line in chunk.split_inclusive('\n') {
        feed(line.trim_end_matches(['\n', '\r']), start + pos);
        pos += line.len() as u64;
    }
}

fn fetch_claude(chunk: &str, start: u64, state: &SourceState, ident: Ident) -> Fetched {
    let mut p = ClaudeParser::from_state(state);
    feed_lines(chunk, start, |line, off| {
        p.cur_offset = off;
        p.feed_line(line);
    });
    Fetched {
        key: String::new(),
        path: None,
        ident,
        size: 0,
        mtime_ms: 0,
        inode: 0,
        read_to: 0,
        state: p.state(),
        quota: None,
        calls: p.calls,
        reads: p.reads,
    }
}

fn fetch_codex(chunk: &str, start: u64, state: &SourceState, fallback_id: &str) -> Fetched {
    let mut p = CodexParser::from_state(state);
    feed_lines(chunk, start, |line, _| p.feed_line(line));
    let (session, thread, agent_id) = p.identity(fallback_id);
    Fetched {
        key: String::new(),
        path: None,
        ident: Ident {
            provider: Provider::Codex,
            session,
            thread,
            agent_id,
            label: p.nickname.clone(),
        },
        size: 0,
        mtime_ms: 0,
        inode: 0,
        read_to: 0,
        state: p.state(),
        quota: p.quota.clone(),
        calls: std::mem::take(&mut p.calls),
        reads: std::mem::take(&mut p.reads),
    }
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
        source: Some(Arc::new(text)),
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
    ledger_path: Option<&Path>,
) -> (Report, usize, Option<IngestStats>, Option<LedgerFallback>) {
    let started = std::time::Instant::now();
    let sources = discover(root);
    let opencode = crate::opencode_open_db();
    let mut stats = None;
    let mut fallback = None;
    let mut ledger = match ledger_path.map(Ledger::open_checked) {
        Some(Ok(l)) => Some(l),
        Some(Err(why)) => {
            fallback = Some(why);
            None
        }
        None => None,
    };
    let files: Vec<Arc<ParsedFile>> = match ledger.as_mut() {
        Some(l) => {
            let mut st = l.ingest_sources(&sources, windows_conn);
            if let Some(conn) = &opencode {
                st.merge(&l.ingest_opencode(conn, root, None, windows_conn));
            }
            l.stamp_pending(windows_conn);
            stats = Some(st);
            l.load(session_filter.as_ref())
        }
        None => {
            // No ledger (no data dir, or a newer one): read the files, as before.
            let mut files: Vec<Arc<ParsedFile>> = sources.iter().filter_map(parse_source).collect();
            if let Some(conn) = &opencode {
                files.extend(opencode_parse(conn, root).into_iter().map(Arc::new));
            }
            files
        }
    };
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
    if let Some(p) = ledger_path {
        spans = spans.with_store(&p.with_extension("maps.db"));
    }
    let mut report = build_report(
        ReportInput {
            root: &root_s,
            files,
            windows,
            since_ms,
            session_filter,
            include_calls,
            instruction_bytes: instruction_files(root),
            native_samples: crate::home_dir()
                .map(|h| native_samples(&h))
                .unwrap_or_default(),
            conventions_bytes: conventions_bytes(root),
            home: crate::home_dir().map(|h| h.to_string_lossy().into_owned()),
            coverage_now: lib_rs_at_head(root),
        },
        &mut spans,
        lib_rs_now(root),
    );
    report.ms = started.elapsed().as_millis();
    (report, n_files, stats, fallback)
}

fn report_json(report: &Report) -> serde_json::Value {
    serde_json::to_value(report).unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
}

/// `bram --token-accounting <root> [--since <ISO>] [--calls]`.
pub fn run_cli(root: &Path, since: Option<&str>, include_calls: bool) -> serde_json::Value {
    let since_ms = since.and_then(crate::parse_iso_to_epoch_ms);
    let conn = open_state_db_read_only(root);
    let ledger = local_data_dir().map(|d| ledger_db_path_in(&d, root));
    let (report, _, _, _) = run(
        root,
        since_ms,
        None,
        include_calls,
        conn.as_ref(),
        ledger.as_deref(),
    );
    report_json(&report)
}

fn ledger_path_for_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    root: &Path,
) -> Option<PathBuf> {
    use tauri::Manager;
    let data = app.path().app_local_data_dir().ok()?;
    Some(ledger_db_path_in(&data, root))
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
    let ledger = ledger_path_for_app(app, &root);
    let (report, n_files, stats, fallback) = run(
        &root,
        since_ms,
        session_filter,
        include_calls,
        conn.as_ref(),
        ledger.as_deref(),
    );
    if crate::bram_trace_enabled() {
        if let Some(line) = stats.as_ref().and_then(|st| st.trace_line()) {
            crate::append_bram_trace_line(app, "token-accounting", &line);
        }
        if let Some(fb) = fallback {
            crate::append_bram_trace_line(app, "token-accounting", &fb.trace_fields());
        }
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
    let base = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if base.starts_with("agent-") {
        return;
    }
    ledger_session_turn_end(app, provider, path);
    if !crate::bram_trace_enabled() {
        return;
    }
    for s in sample_live(path, provider) {
        crate::append_bram_trace_line(app, "session-context", &format_sample(&s));
    }
}

/// Write the session's new calls (and its subagents') into the ledger.
fn ledger_session_turn_end<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    provider: &str,
    path: &Path,
) {
    let Some(root) = crate::project_root(Some(app)) else {
        return;
    };
    let Some(mut ledger) = ledger_path_for_app(app, &root)
        .as_deref()
        .and_then(Ledger::open)
    else {
        return;
    };
    let conn = crate::worklist_state_open(app);
    let stats = ledger.ingest_sources(&session_sources(provider, path), conn.as_ref());
    if let (true, Some(line)) = (crate::bram_trace_enabled(), stats.trace_line()) {
        crate::append_bram_trace_line(app, "token-accounting", &line);
    }
}

/// The ledger sources for one live session file: a Claude transcript with the
/// subagent files beside it, or a Codex rollout.
pub fn session_sources(provider: &str, path: &Path) -> Vec<Source> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    if provider == "codex" {
        let id = match crate::codex_session_meta(path) {
            Ok(Some((id, _))) => id,
            _ => stem,
        };
        return vec![Source::Codex {
            id,
            path: path.to_path_buf(),
        }];
    }
    let mut out = Vec::new();
    if let Ok(subs) = std::fs::read_dir(path.with_extension("").join("subagents")) {
        for sub in subs.flatten() {
            let sp = sub.path();
            let name = sp.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if sp.extension().and_then(|s| s.to_str()) == Some("jsonl")
                && name.starts_with("agent-")
            {
                out.push(Source::ClaudeSubagent {
                    id: stem.clone(),
                    agent: name.trim_start_matches("agent-").to_string(),
                    path: sp,
                });
            }
        }
    }
    out.push(Source::ClaudeMain {
        id: stem,
        path: path.to_path_buf(),
    });
    out
}

/// OpenCode's turn end: the latest assistant message's tokens, the session's
/// cache reads so far, and the calls since the last user message.
pub fn sample_opencode_turn_end<R: tauri::Runtime>(app: &tauri::AppHandle<R>, sid: &str) {
    let Some(conn) = crate::opencode_open_db() else {
        return;
    };
    if let Some(root) = crate::project_root(Some(app)) {
        if let Some(mut ledger) = ledger_path_for_app(app, &root)
            .as_deref()
            .and_then(Ledger::open)
        {
            let wc = crate::worklist_state_open(app);
            let stats = ledger.ingest_opencode(&conn, &root, Some(sid), wc.as_ref());
            if let (true, Some(line)) = (crate::bram_trace_enabled(), stats.trace_line()) {
                crate::append_bram_trace_line(app, "token-accounting", &line);
            }
        }
    }
    if !crate::bram_trace_enabled() {
        return;
    }
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

/// Antigravity's turn end. The finished cue names no conversation, so this
/// ingests every conversation of the project that changed (sources keep byte
/// offsets, so unchanged ones cost a stat) and samples the most recently
/// modified main-thread one. A turn is its trailing run of calls with gaps
/// under `TURN_GAP_MS`; agy's transcript has no user-message cue to cut on.
pub fn sample_antigravity_turn_end<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(root) = crate::project_root(Some(app)) else {
        return;
    };
    let sources = antigravity_sources(&root);
    if sources.is_empty() {
        return;
    }
    if let Some(mut ledger) = ledger_path_for_app(app, &root)
        .as_deref()
        .and_then(Ledger::open)
    {
        let wc = crate::worklist_state_open(app);
        let stats = ledger.ingest_sources(&sources, wc.as_ref());
        if let (true, Some(line)) = (crate::bram_trace_enabled(), stats.trace_line()) {
            crate::append_bram_trace_line(app, "token-accounting", &line);
        }
    }
    if !crate::bram_trace_enabled() {
        return;
    }
    let newest = sources
        .iter()
        .filter(|s| matches!(s, Source::Antigravity { parent: None, .. }))
        .filter_map(|s| file_key(s.path()).map(|k| (k.0, s)))
        .max_by_key(|(m, _)| *m)
        .map(|(_, s)| s);
    let Some(src) = newest else {
        return;
    };
    let Some(pf) = parse_source(src) else {
        return;
    };
    let Some(last) = pf.calls.last() else {
        return;
    };
    let mut start = pf.calls.len() - 1;
    while start > 0 && pf.calls[start].at_ms - pf.calls[start - 1].at_ms <= TURN_GAP_MS {
        start -= 1;
    }
    let turn = &pf.calls[start..];
    let s = Sample {
        provider: "antigravity",
        session: pf.session_id.clone(),
        model: last.model.clone(),
        tokens: last.context,
        turn_calls: turn.len(),
        turn_output: turn.iter().map(|c| c.output).sum(),
        cache_read: pf.calls.iter().map(|c| c.cache_read).sum(),
        ..Default::default()
    };
    crate::append_bram_trace_line(app, "session-context", &format_sample(&s));
}
