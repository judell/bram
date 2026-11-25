//! context-stack-viewer (2026-11-24): the sending view of a session.
//!
//! `/__turns` is the reading view of a session file: a turn per message,
//! tool calls folded in, attachments dropped. The model never saw it that
//! way. On every call it was sent an ordered run of blocks: the harness's
//! own prompt and tools, the instruction files, each user message, its own
//! earlier words and tool calls, each tool result, and the notes the harness
//! slips in between. This module projects a session file into those blocks,
//! each sized in bytes, and scales the sizes to the measured context (the
//! per-call usage the token accounting reads) by a bytes-per-token figure
//! taken from this very session, so the figures total what the footer shows
//! instead of a guessed 4 bytes a token.
//!
//! One reader per provider (`Reader`, `reader_for`); a provider without one
//! answers `supported: false` (Jon, 2026-11-24: "not hardcode Claude and
//! prepare for any provider to do this, even if others are stubs at first").
//! Claude's reader has two strategies:
//!
//! - **requests**: Claude Code records every API request it makes
//!   (`api-request`: a `base` request, how many of its messages to `keep`,
//!   and the `tail` of message hashes appended), the harness-made messages
//!   those hashes name (`api-request-blob`), and the system prompt and tool
//!   list (`api-request-shape`). A hash is the SHA-256 of the message as
//!   canonical JSON (sorted keys), so the ordinary user and assistant
//!   records resolve too: as stored, wrapped in a text block, the parallel
//!   tool results of one turn merged into one user message, an assistant
//!   message with or without its thinking signatures. The last request's
//!   list is exactly what the model was sent. Measured on the session this
//!   was built from: 302 distinct hashes, all resolved.
//! - **records**: for a file without request records (older Claude Code),
//!   the user, assistant and attachment records in file order, with the
//!   latest `prompt_snapshot` attachment as the harness block and a
//!   bytes-per-token figure calibrated on the growth between readings.
//!
//! Everything after a reader (scaling, by-kind totals) is provider-neutral.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::token_accounting::{parse_claude_text, Thread};

/// Used when the file gives nothing to calibrate against: a session with
/// one call, a stretch where the readings did not rise, a request whose
/// shape record is missing.
pub const FALLBACK_BYTES_PER_TOKEN: f64 = 4.0;

/// What an image costs, roughly: the API caps an image at about 1,600
/// tokens, and pasted screenshots are near the cap. Its bytes are base64
/// and say nothing about its tokens.
pub const IMAGE_TOKENS: u64 = 1600;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    /// The harness's own system prompt, or its tool list. Sent first on
    /// every call.
    Harness,
    /// An instruction file the harness sends: CLAUDE.md and its imports.
    Instructions,
    /// The memory index the harness sends beside the instructions.
    Memory,
    /// The user's words.
    User,
    /// The agent's words.
    Agent,
    /// A tool call: its name and its input.
    ToolCall,
    /// What the tool returned.
    ToolResult,
    /// A note the harness slipped in: a reminder, a pasted file, hook
    /// output, a skills list, a tool added mid-session.
    Attachment,
    /// The agent's thinking.
    Thinking,
    /// What a compaction left of everything before it.
    CompactionSummary,
}

const KIND_ORDER: [BlockKind; 10] = [
    BlockKind::Harness,
    BlockKind::Instructions,
    BlockKind::Memory,
    BlockKind::CompactionSummary,
    BlockKind::User,
    BlockKind::Agent,
    BlockKind::ToolCall,
    BlockKind::ToolResult,
    BlockKind::Attachment,
    BlockKind::Thinking,
];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    /// Coordinate in the append-only rollout, retained across stack rebuilds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_key: Option<String>,
    /// Position in the live stack (the blocks the model is sent now).
    pub i: usize,
    pub kind: BlockKind,
    pub label: String,
    /// Text bytes; an image's base64 is not counted here (see `images`).
    pub bytes: u64,
    /// `bytes / bytesPerToken` plus `images * IMAGE_TOKENS`, rounded; 0 for
    /// a block that is not re-sent.
    pub tokens: u64,
    /// `tokens / measured.tokens`.
    pub share: f64,
    pub ts: Option<String>,
    /// The user turn (1-based) this block arrived in; 0 before the first.
    pub turn: usize,
    /// The 0-based call whose request first carried this block.
    pub call: usize,
    /// A file on disk this block is a copy of, when it is one.
    pub on_disk: Option<String>,
    /// Whether later calls carry this block again.
    pub resent: bool,
    pub is_error: bool,
    /// Images in the block, each costed at `IMAGE_TOKENS`.
    pub images: usize,
    /// Of `bytes`, the opaque part: an encrypted thinking signature, which
    /// the model is charged for at its own rate.
    pub opaque_bytes: u64,
    /// The exact text the model was sent; served by the block route, not
    /// in the stack body.
    #[serde(skip)]
    pub text: String,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    /// The last call's context.
    pub tokens: u64,
    /// The first call's context.
    pub floor: u64,
    pub calls: usize,
    pub compactions: usize,
    /// The reading the records strategy calibrates from: the first call
    /// after the last compaction, else the floor. The floor for requests.
    pub base: u64,
}

/// Blocks that were sent on earlier calls and are not sent now: compacted
/// away, or cleared by the harness.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Compacted {
    pub blocks: usize,
    pub bytes: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KindTotal {
    pub kind: BlockKind,
    pub blocks: usize,
    pub bytes: u64,
    pub tokens: u64,
    pub share: f64,
}

/// What the requests strategy saw of the wire.
#[derive(Serialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Wire {
    pub requests: usize,
    /// Messages in the last request.
    pub live_messages: usize,
    /// Messages some earlier request carried and the last does not.
    pub dropped_messages: usize,
    /// Hashes in the last request no record or blob resolved.
    pub unresolved: usize,
    /// Whether the last request's shape (system prompt and tools) was found.
    pub shape_known: bool,
    pub live_bytes: u64,
    pub first_request_bytes: u64,
    pub images: usize,
    /// Requests the two-rate fit had to work with.
    pub fit_rows: usize,
    /// `ok`, or why the fit was refused and one rate serves both:
    /// `too-few-rows`, `no-opaque-bytes`, `degenerate`, `negative-rate`,
    /// `implausible-rate`, `no-requests`.
    pub fit: String,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    Requests,
    Records,
    None,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stack {
    pub session: String,
    pub provider: &'static str,
    pub supported: bool,
    pub reader: Strategy,
    pub model: String,
    pub measured: Measured,
    pub bytes_per_token: f64,
    /// The rate for the opaque bytes (thinking signatures); equal to
    /// `bytesPerToken` when the session gave no fit.
    pub opaque_bytes_per_token: f64,
    /// False when `bytesPerToken` is the fallback, not a measurement.
    pub calibrated: bool,
    pub accounted_tokens: u64,
    /// `measured.tokens - accountedTokens`; negative when the estimate
    /// overshoots.
    pub unaccounted_tokens: i64,
    pub by_kind: Vec<KindTotal>,
    pub compacted: Compacted,
    pub wire: Option<Wire>,
    pub blocks: Vec<Block>,
}

/// The answer for a provider whose files this module does not read, or
/// for no session at all.
pub fn empty(provider: &'static str, supported: bool) -> Stack {
    Stack {
        session: String::new(),
        provider,
        supported,
        reader: Strategy::None,
        model: String::new(),
        measured: Measured::default(),
        bytes_per_token: FALLBACK_BYTES_PER_TOKEN,
        opaque_bytes_per_token: FALLBACK_BYTES_PER_TOKEN,
        calibrated: false,
        accounted_tokens: 0,
        unaccounted_tokens: 0,
        by_kind: Vec::new(),
        compacted: Compacted::default(),
        wire: None,
        blocks: Vec::new(),
    }
}

/// The readers Bram has, one per provider whose session records it turns
/// into blocks. A provider missing here gets `supported: false` from the
/// routes; adding one means a walk over its records and a `Measured` from
/// its accounting parser (`parse_codex_text` already reads Codex rollouts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    ClaudeJsonl,
    CodexRollout,
}

/// The provider labels are the ones `/__turns` and the sessions routes use.
pub fn reader_for(provider: &str) -> Option<Reader> {
    match provider {
        "claude" => Some(Reader::ClaudeJsonl),
        "codex" => Some(Reader::CodexRollout),
        _ => None,
    }
}

/// Project a session file into the stack the model is sent. `provider` is
/// the session's provider label; a provider with no reader gets the empty,
/// unsupported stack.
pub fn build(provider: &'static str, text: &str, session_id: &str, root: Option<&Path>) -> Stack {
    match reader_for(provider) {
        Some(Reader::ClaudeJsonl) => {
            let (measured, model, contexts) = measured_claude(text, session_id);
            let root = root.map(|p| p.to_string_lossy().trim_end_matches('/').to_string());
            match requests::read(text, root.as_deref()) {
                Some(r) => requests::finish(provider, session_id, r, measured, model, &contexts),
                None => records::finish(
                    provider,
                    session_id,
                    records::walk(text, root),
                    measured,
                    model,
                ),
            }
        }
        Some(Reader::CodexRollout) => codex_stack(text, session_id),
        None => empty(provider, false),
    }
}

fn codex_item(item: &Value, mut emit: impl FnMut(BlockKind, &str, &str)) {
    match item["type"].as_str() {
        Some("message") => {
            let (kind, role) = match item["role"].as_str() {
                Some("developer") => (BlockKind::Instructions, "Developer"),
                Some("system") => (BlockKind::Harness, "System"),
                Some("user") => (BlockKind::User, "User"),
                Some("assistant") => (BlockKind::Agent, "Assistant"),
                _ => return,
            };
            for part in item["content"].as_array().into_iter().flatten() {
                if let Some(text) = part["text"].as_str() {
                    emit(kind, &format!("{role} · {}", first_line(text)), text);
                }
            }
        }
        Some("function_call" | "custom_tool_call") => {
            if let Some(input) = item["arguments"]
                .as_str()
                .or_else(|| item["input"].as_str())
            {
                emit(
                    BlockKind::ToolCall,
                    item["name"].as_str().unwrap_or("Tool call"),
                    input,
                );
            }
        }
        Some("function_call_output" | "custom_tool_call_output") => {
            let output = &item["output"];
            let label = item["call_id"].as_str().unwrap_or("Tool result");
            if let Some(text) = output.as_str() {
                emit(BlockKind::ToolResult, label, text);
            } else if let Some(parts) = output.as_array() {
                for part in parts {
                    if let Some(text) = part["text"].as_str() {
                        emit(BlockKind::ToolResult, label, text);
                    }
                }
            }
        }
        _ => {}
    }
}

fn codex_stack(text: &str, fallback_id: &str) -> Stack {
    let usage = crate::token_accounting::parse_codex_text(text, fallback_id);
    let mut stack = empty("codex", true);
    stack.session = format!(
        "codex:{}",
        usage.agent_id.as_ref().unwrap_or(&usage.session_id)
    );
    stack.reader = Strategy::Records;
    stack.model = usage
        .calls
        .last()
        .map(|c| c.model.clone())
        .unwrap_or_default();
    stack.measured.tokens = usage.calls.last().map_or(0, |c| c.context);
    stack.measured.floor = usage.calls.first().map_or(0, |c| c.context);
    stack.measured.calls = usage.calls.len();
    let mut base_len = 0;
    let mut turn = 0;
    for (line, raw) in text.lines().enumerate() {
        let Ok(rec) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        let p = &rec["payload"];
        let kind = rec["type"].as_str();
        if kind == Some("compacted") {
            stack.measured.compactions += 1;
            for old in stack.blocks.drain(base_len..) {
                stack.compacted.blocks += 1;
                stack.compacted.bytes += old.bytes;
            }
        }
        let is_first_block = stack.blocks.is_empty();
        let mut part = 0;
        let mut push = |kind, label: &str, body: &str| {
            let mut b = new_block(
                kind,
                label.to_string(),
                body.to_string(),
                rec["timestamp"].as_str().map(str::to_owned),
                turn,
                0,
            );
            b.record_key = Some(format!("{line}.{part}"));
            part += 1;
            stack.blocks.push(b);
        };
        match kind {
            Some("session_meta") if is_first_block => {
                if let Some(body) = p.pointer("/base_instructions/text").and_then(Value::as_str) {
                    push(BlockKind::Harness, "Base instructions", body);
                    base_len = stack.blocks.len();
                }
            }
            Some("turn_context") => turn += 1,
            Some("response_item") => codex_item(p, push),
            Some("compacted") => {
                if let Some(items) = p["replacement_history"].as_array() {
                    for item in items {
                        codex_item(item, &mut push);
                    }
                } else if let Some(summary) = p["message"].as_str() {
                    push(BlockKind::CompactionSummary, "Compaction summary", summary);
                }
            }
            _ => {}
        }
    }
    let latest = stack.measured.tokens;
    (stack.accounted_tokens, stack.by_kind) = scale(
        &mut stack.blocks,
        FALLBACK_BYTES_PER_TOKEN,
        FALLBACK_BYTES_PER_TOKEN,
        latest,
    );
    stack.unaccounted_tokens = latest as i64 - stack.accounted_tokens as i64;
    stack
}

/// The measured side for Claude, from the accounting's parser so the figures
/// match the footer's.
fn measured_claude(text: &str, session_id: &str) -> (Measured, String, Vec<u64>) {
    let pf = parse_claude_text(text, session_id, Thread::Main, None);
    let contexts: Vec<u64> = pf.calls.iter().map(|c| c.context).collect();
    let measured = Measured {
        tokens: pf.calls.last().map(|c| c.context).unwrap_or(0),
        floor: pf.calls.first().map(|c| c.context).unwrap_or(0),
        calls: pf.calls.len(),
        compactions: pf.compactions,
        base: 0,
    };
    let model = pf.calls.last().map(|c| c.model.clone()).unwrap_or_default();
    (measured, model, contexts)
}

/// Two token rates fitted on a session's own requests: the context each
/// request added (its reading less the previous) regressed on the bytes its
/// tail appended, text apart from the opaque thinking signatures. On the
/// session this was built from the signatures priced at 3.7 bytes a token
/// against 2.5 for text, with residuals within a few hundred tokens, so one
/// rate would overstate thinking's share by a quarter. Rows are requests
/// that kept their whole base, resolved every tail hash, appended no image
/// and grew. None when the rows are too few, the signatures too small to
/// price, or a rate lands outside 1 to 12 bytes a token; the Err names which.
pub fn fit_rates(rows: &[(u64, u64, u64)]) -> Result<(f64, f64), &'static str> {
    let (mut sxx, mut sxy, mut syy, mut sxd, mut syd) = (0f64, 0f64, 0f64, 0f64, 0f64);
    let mut opaque_total = 0u64;
    for &(delta, text, opaque) in rows {
        let (d, x, y) = (delta as f64, text as f64, opaque as f64);
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sxd += x * d;
        syd += y * d;
        opaque_total += opaque;
    }
    if rows.len() < 6 {
        return Err("too-few-rows");
    }
    if opaque_total < 1000 {
        return Err("no-opaque-bytes");
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 {
        return Err("degenerate");
    }
    let a = (sxd * syy - syd * sxy) / det;
    let b = (sxx * syd - sxy * sxd) / det;
    if a <= 0.0 || b <= 0.0 {
        return Err("negative-rate");
    }
    let (text_rate, opaque_rate) = (1.0 / a, 1.0 / b);
    let plausible = |r: f64| (1.0..=12.0).contains(&r);
    if plausible(text_rate) && plausible(opaque_rate) {
        Ok((text_rate, opaque_rate))
    } else {
        Err("implausible-rate")
    }
}

/// The SHA-256 of a message as canonical JSON (sorted keys, no spaces),
/// the way Claude Code names the messages of a request.
pub fn message_hash(message: &Value) -> String {
    let mut s = String::new();
    canonical(message, &mut s);
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                canonical(&m[*k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
    }
}

fn new_block(
    kind: BlockKind,
    label: String,
    text: String,
    ts: Option<String>,
    turn: usize,
    call: usize,
) -> Block {
    Block {
        record_key: None,
        i: 0,
        kind,
        label,
        bytes: text.len() as u64,
        tokens: 0,
        share: 0.0,
        ts,
        turn,
        call,
        on_disk: None,
        resent: true,
        is_error: false,
        images: 0,
        opaque_bytes: 0,
        text,
    }
}

/// Scale the live blocks to the measured total and total them by kind.
fn scale(
    blocks: &mut [Block],
    bytes_per_token: f64,
    opaque_bytes_per_token: f64,
    latest: u64,
) -> (u64, Vec<KindTotal>) {
    let mut accounted: u64 = 0;
    for (i, b) in blocks.iter_mut().enumerate() {
        b.i = i;
        b.tokens = if b.resent {
            let text = b.bytes.saturating_sub(b.opaque_bytes) as f64 / bytes_per_token;
            let opaque = b.opaque_bytes as f64 / opaque_bytes_per_token;
            (text + opaque).round() as u64 + b.images as u64 * IMAGE_TOKENS
        } else {
            0
        };
        b.share = if latest > 0 {
            b.tokens as f64 / latest as f64
        } else {
            0.0
        };
        accounted += b.tokens;
    }
    let mut by_kind = Vec::new();
    for kind in KIND_ORDER {
        let mut t = KindTotal {
            kind,
            blocks: 0,
            bytes: 0,
            tokens: 0,
            share: 0.0,
        };
        for b in blocks.iter().filter(|b| b.kind == kind) {
            t.blocks += 1;
            t.bytes += b.bytes;
            t.tokens += b.tokens;
        }
        if t.blocks > 0 {
            t.share = if latest > 0 {
                t.tokens as f64 / latest as f64
            } else {
                0.0
            };
            by_kind.push(t);
        }
    }
    (accounted, by_kind)
}

fn relative(root: Option<&str>, path: &str) -> String {
    if let Some(root) = root {
        if let Some(rest) = path.strip_prefix(root) {
            let rest = rest.trim_start_matches('/');
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    path.to_string()
}

fn is_memory_path(path: &str) -> bool {
    path.contains("/memory/") || path.ends_with("MEMORY.md")
}

/// A system-reminder text block the harness folds into a user message: the
/// instruction files ("Contents of <path> (...):" sections) become one
/// block each; anything else is one note.
fn reminder_blocks(
    text: &str,
    ts: Option<&String>,
    turn: usize,
    call: usize,
    root: Option<&str>,
) -> Vec<Block> {
    let mut out = Vec::new();
    let marker = "Contents of ";
    let mut starts: Vec<usize> = Vec::new();
    let mut from = 0;
    while let Some(pos) = text[from..].find(marker) {
        let at = from + pos;
        let line_start = at == 0 || text.as_bytes()[at - 1] == b'\n';
        if line_start {
            starts.push(at);
        }
        from = at + marker.len();
    }
    if starts.is_empty() {
        let body = text
            .trim()
            .trim_start_matches("<system-reminder>")
            .trim_end_matches("</system-reminder>")
            .trim();
        let label = format!("Note: {}", first_line(body));
        out.push(new_block(
            BlockKind::Attachment,
            label,
            text.to_string(),
            ts.cloned(),
            turn,
            call,
        ));
        return out;
    }
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(text.len());
        // The preamble before the first header rides with the first file.
        let seg_start = if n == 0 { 0 } else { start };
        let segment = &text[seg_start..end];
        let header = &text[start..end];
        let path: String = header[marker.len()..]
            .split(|c: char| c == ' ' || c == ':' || c == '\n')
            .next()
            .unwrap_or("")
            .to_string();
        let kind = if is_memory_path(&path) {
            BlockKind::Memory
        } else {
            BlockKind::Instructions
        };
        let mut b = new_block(
            kind,
            relative(root, &path),
            segment.to_string(),
            ts.cloned(),
            turn,
            call,
        );
        if !path.is_empty() {
            b.on_disk = Some(path);
        }
        out.push(b);
    }
    out
}

fn tool_result_text(item: &Value) -> (String, usize) {
    let mut text = String::new();
    let mut images = 0;
    match item.get("content") {
        Some(Value::String(s)) => text.push_str(s),
        Some(Value::Array(parts)) => {
            for p in parts {
                match p.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        if let Some(s) = p.get("text").and_then(Value::as_str) {
                            text.push_str(s);
                        }
                    }
                    "image" => images += 1,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    (text, images)
}

fn tool_call_label(name: &str, input: &Value) -> String {
    let detail = [
        "description",
        "command",
        "file_path",
        "pattern",
        "query",
        "url",
        "prompt",
    ]
    .iter()
    .find_map(|k| input.get(*k).and_then(Value::as_str))
    .map(first_line)
    .unwrap_or_default();
    if detail.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {detail}")
    }
}

/// `Skill: <name>` for a harness-inserted skill body, which opens with
/// `Base directory for this skill: <dir>`; the name is the directory's last segment.
fn skill_label(s: &str) -> Option<String> {
    let dir = s
        .lines()
        .next()?
        .strip_prefix("Base directory for this skill:")?
        .trim()
        .trim_end_matches('/');
    let name = dir.rsplit('/').next().unwrap_or(dir);
    Some(format!("Skill: {name}"))
}

/// The first line of a text, trimmed and cut to 80 characters.
pub fn first_line(s: &str) -> String {
    let line = s.trim().lines().next().unwrap_or("").trim();
    let mut out: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        out.push('…');
    }
    out
}

fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn humanize(ty: &str) -> String {
    let mut s = ty.replace('_', " ");
    if let Some(first) = s.get(0..1) {
        let upper = first.to_ascii_uppercase();
        s.replace_range(0..1, &upper);
    }
    s
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The blocks of one wire message (role user, assistant or system), in the
/// order the model reads them. `tool_names` learns tool_use ids so results
/// can be named; `turn` advances on a user message with the user's own text.
fn message_blocks(
    message: &Value,
    ts: Option<&String>,
    turn: &mut usize,
    call: usize,
    tool_names: &mut HashMap<String, String>,
    root: Option<&str>,
) -> Vec<Block> {
    let mut out = Vec::new();
    let role = message.get("role").and_then(Value::as_str).unwrap_or("");
    let content = message.get("content");
    let items: Vec<Value> = match content {
        Some(Value::String(s)) => vec![serde_json::json!({ "type": "text", "text": s })],
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let mut counted_turn = false;
    for item in &items {
        let ty = item.get("type").and_then(Value::as_str).unwrap_or("");
        match (role, ty) {
            ("user", "text") => {
                let s = item.get("text").and_then(Value::as_str).unwrap_or("");
                if s.trim_start().starts_with("<system-reminder>") {
                    out.extend(reminder_blocks(s, ts, *turn, call, root));
                } else if let Some(label) = skill_label(s) {
                    out.push(new_block(
                        BlockKind::Attachment,
                        label,
                        s.to_string(),
                        ts.cloned(),
                        *turn,
                        call,
                    ));
                } else {
                    if !counted_turn {
                        *turn += 1;
                        counted_turn = true;
                    }
                    out.push(new_block(
                        BlockKind::User,
                        first_line(s),
                        s.to_string(),
                        ts.cloned(),
                        *turn,
                        call,
                    ));
                }
            }
            ("user", "tool_result") => {
                let (text, images) = tool_result_text(item);
                let name = item
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .and_then(|id| tool_names.get(id))
                    .cloned();
                let mut label = match name {
                    Some(n) => format!("{n} result"),
                    None => "tool result".to_string(),
                };
                if images > 0 {
                    label.push_str(&format!(" (+{images} image{})", plural(images)));
                }
                let mut b = new_block(BlockKind::ToolResult, label, text, ts.cloned(), *turn, call);
                b.is_error = item.get("is_error").and_then(Value::as_bool) == Some(true);
                b.images = images;
                out.push(b);
            }
            ("user", "image") => {
                let mut b = new_block(
                    BlockKind::User,
                    "Pasted image".to_string(),
                    String::new(),
                    ts.cloned(),
                    *turn,
                    call,
                );
                b.images = 1;
                out.push(b);
            }
            ("assistant", "text") => {
                let s = item.get("text").and_then(Value::as_str).unwrap_or("");
                if s.is_empty() {
                    continue;
                }
                out.push(new_block(
                    BlockKind::Agent,
                    first_line(s),
                    s.to_string(),
                    ts.cloned(),
                    *turn,
                    call,
                ));
            }
            ("assistant", "tool_use") => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                let input = item.get("input").cloned().unwrap_or(Value::Null);
                if let Some(tid) = item.get("id").and_then(Value::as_str) {
                    tool_names.insert(tid.to_string(), name.to_string());
                }
                let compact = serde_json::to_string(&input).unwrap_or_default();
                let pretty = serde_json::to_string_pretty(&input).unwrap_or_default();
                let mut b = new_block(
                    BlockKind::ToolCall,
                    tool_call_label(name, &input),
                    format!("{name}\n{pretty}"),
                    ts.cloned(),
                    *turn,
                    call,
                );
                // The wire carries the compact input, not the pretty one.
                b.bytes = compact.len() as u64;
                out.push(b);
            }
            ("assistant", "thinking") => {
                let s = item.get("thinking").and_then(Value::as_str).unwrap_or("");
                let sig = item.get("signature").and_then(Value::as_str).unwrap_or("");
                if s.is_empty() && sig.is_empty() {
                    continue;
                }
                // Some blocks carry a one-line readable summary beside the
                // encrypted part, most only the encrypted part; the label
                // says which (Jon, 2026-11-24: "here we see a mix").
                let label = if s.is_empty() {
                    "Thinking (encrypted)".to_string()
                } else {
                    format!("Thinking: {}", first_line(s))
                };
                let mut b = new_block(
                    BlockKind::Thinking,
                    label,
                    s.to_string(),
                    ts.cloned(),
                    *turn,
                    call,
                );
                b.bytes = (s.len() + sig.len()) as u64;
                b.opaque_bytes = sig.len() as u64;
                out.push(b);
            }
            ("system", "text") => {
                let s = item.get("text").and_then(Value::as_str).unwrap_or("");
                if s.trim_start().starts_with("<system-reminder>") || s.contains("\nContents of ") {
                    out.extend(reminder_blocks(s, ts, *turn, call, root));
                } else {
                    out.push(new_block(
                        BlockKind::Attachment,
                        format!("Note: {}", first_line(s)),
                        s.to_string(),
                        ts.cloned(),
                        *turn,
                        call,
                    ));
                }
            }
            ("system", "tool_addition") => {
                let tool = item.get("tool").cloned().unwrap_or(Value::Null);
                let name = tool.get("name").and_then(Value::as_str).unwrap_or("tool");
                let compact = serde_json::to_string(&tool).unwrap_or_default();
                let pretty = serde_json::to_string_pretty(&tool).unwrap_or_default();
                let mut b = new_block(
                    BlockKind::Attachment,
                    format!("Tool added: {name}"),
                    pretty,
                    ts.cloned(),
                    *turn,
                    call,
                );
                b.bytes = compact.len() as u64;
                out.push(b);
            }
            _ => {
                let compact = serde_json::to_string(item).unwrap_or_default();
                let pretty = serde_json::to_string_pretty(item).unwrap_or_default();
                let mut b = new_block(
                    BlockKind::Attachment,
                    humanize(ty),
                    pretty,
                    ts.cloned(),
                    *turn,
                    call,
                );
                b.bytes = compact.len() as u64;
                out.push(b);
            }
        }
    }
    out
}

/// The requests strategy: rebuild the last request's message list from the
/// request chain and the hashes' sources.
mod requests {
    use super::*;

    struct Req {
        id: String,
        base: Option<String>,
        keep: usize,
        tail: Vec<String>,
        shape_hash: String,
        ts: Option<String>,
    }

    /// A user or assistant record as the transcript keeps it; assistant
    /// records streamed under one id are one message.
    struct Rec {
        role: &'static str,
        /// The content as stored (a string for a plain user message).
        stored: Value,
        /// The content as blocks.
        blocks: Vec<Value>,
        ts: Option<String>,
    }

    pub struct Read {
        reqs: Vec<Req>,
        shapes: HashMap<String, Value>,
        /// Every hash a record or blob can answer for, with its message.
        index: HashMap<String, (Value, Option<String>)>,
        root: Option<String>,
        /// Each main-thread call in the order `/__token-accounting` counts
        /// them (a non-synthetic assistant record with usage, once per
        /// message id), by the `requestRef` naming the request it answered.
        answered: Vec<Option<String>>,
    }

    /// The call each request's messages count against: the accounting's
    /// index of the call that answered it. A request nothing answered (an
    /// API error, an interrupt before the response; 16 of 893 on session
    /// 15f71ae7) takes the next answered request's call, since that call is
    /// the first the accounting measured carrying its messages; one after
    /// the last answered takes the call not made yet (`answered.len()`).
    /// Numbering by request index instead ran 4 to 15 calls past the Token
    /// growth chart after such requests (context-stack-call-matches-accounting).
    /// A file whose responses carry no `requestRef` keeps the request index;
    /// the bool says whether the link was found.
    fn call_of_request(reqs: &[Req], answered: &[Option<String>]) -> (Vec<usize>, bool) {
        let by_ref: HashMap<&str, usize> = answered
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_deref().map(|r| (r, i)))
            .collect();
        if !reqs.iter().any(|q| by_ref.contains_key(q.id.as_str())) {
            return ((0..reqs.len()).collect(), false);
        }
        let mut out = vec![0; reqs.len()];
        let mut next = answered.len();
        for (k, q) in reqs.iter().enumerate().rev() {
            if let Some(&i) = by_ref.get(q.id.as_str()) {
                next = i;
            }
            out[k] = next;
        }
        (out, true)
    }

    fn strip_signatures(blocks: &[Value]) -> Vec<Value> {
        blocks
            .iter()
            .map(|b| {
                if b.get("type").and_then(Value::as_str) == Some("thinking") {
                    let mut c = b.clone();
                    if let Some(o) = c.as_object_mut() {
                        o.remove("signature");
                    }
                    c
                } else {
                    b.clone()
                }
            })
            .collect()
    }

    fn as_blocks(content: &Value) -> Vec<Value> {
        match content {
            Value::String(s) => vec![serde_json::json!({ "type": "text", "text": s })],
            Value::Array(a) => a.clone(),
            _ => Vec::new(),
        }
    }

    /// None when the file has no request records.
    pub fn read(text: &str, root: Option<&str>) -> Option<Read> {
        let mut reqs: Vec<Req> = Vec::new();
        let mut blobs: HashMap<String, Value> = HashMap::new();
        let mut shapes: HashMap<String, Value> = HashMap::new();
        let mut recs: Vec<Rec> = Vec::new();
        let mut last_asst_id: Option<String> = None;
        let mut answered: Vec<Option<String>> = Vec::new();
        let mut answered_ids: HashSet<String> = HashSet::new();
        for line in text.lines() {
            let Ok(rec) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if rec.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let ts = rec
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string);
            match rec.get("type").and_then(Value::as_str).unwrap_or("") {
                "api-request" => {
                    let Some(id) = rec.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    reqs.push(Req {
                        id: id.to_string(),
                        base: rec.get("base").and_then(Value::as_str).map(str::to_string),
                        keep: rec.get("keep").and_then(Value::as_u64).unwrap_or(0) as usize,
                        tail: rec
                            .get("tail")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_string)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        shape_hash: rec
                            .get("shapeHash")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        ts,
                    });
                    last_asst_id = None;
                }
                "api-request-blob" => {
                    if let (Some(h), Some(m)) =
                        (rec.get("hash").and_then(Value::as_str), rec.get("message"))
                    {
                        blobs.insert(h.to_string(), m.clone());
                    }
                }
                "api-request-shape" => {
                    if let (Some(h), Some(s)) = (
                        rec.get("shapeHash").and_then(Value::as_str),
                        rec.get("shape"),
                    ) {
                        shapes.insert(h.to_string(), s.clone());
                    }
                }
                "user" => {
                    let Some(content) = rec.pointer("/message/content") else {
                        continue;
                    };
                    recs.push(Rec {
                        role: "user",
                        stored: content.clone(),
                        blocks: as_blocks(content),
                        ts,
                    });
                    last_asst_id = None;
                }
                "assistant" => {
                    let id = rec
                        .pointer("/message/id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    // The accounting's call rule (token_accounting.rs,
                    // ClaudeParser::assistant), so the indexes agree.
                    let synthetic = rec.pointer("/message/model").and_then(Value::as_str)
                        == Some("<synthetic>");
                    let has_usage = rec.pointer("/message/usage").is_some_and(|u| !u.is_null());
                    if !synthetic && has_usage {
                        let fresh = match &id {
                            Some(i) => answered_ids.insert(i.clone()),
                            None => true,
                        };
                        if fresh {
                            answered.push(
                                rec.get("requestRef")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                            );
                        }
                    }
                    let blocks = rec
                        .pointer("/message/content")
                        .map(as_blocks)
                        .unwrap_or_default();
                    if id.is_some() && id == last_asst_id {
                        if let Some(last) = recs.last_mut() {
                            last.blocks.extend(blocks);
                            last.stored = Value::Array(last.blocks.clone());
                            continue;
                        }
                    }
                    last_asst_id = id;
                    recs.push(Rec {
                        role: "assistant",
                        stored: Value::Array(blocks.clone()),
                        blocks,
                        ts,
                    });
                }
                _ => {}
            }
        }
        if reqs.is_empty() {
            return None;
        }

        // Every hash a record can answer for. Blobs win on a collision,
        // being the wire form itself.
        let mut index: HashMap<String, (Value, Option<String>)> = HashMap::new();
        let mut group: Vec<&Rec> = Vec::new();
        let flush_group =
            |group: &mut Vec<&Rec>, index: &mut HashMap<String, (Value, Option<String>)>| {
                if group.len() > 1 {
                    let mut blocks: Vec<Value> = Vec::new();
                    for r in group.iter() {
                        blocks.extend(r.blocks.iter().cloned());
                    }
                    let merged = serde_json::json!({ "role": "user", "content": blocks });
                    let ts = group.first().and_then(|r| r.ts.clone());
                    index.entry(message_hash(&merged)).or_insert((merged, ts));
                }
                group.clear();
            };
        for r in &recs {
            if r.role == "user" {
                let stored = serde_json::json!({ "role": "user", "content": r.stored });
                index
                    .entry(message_hash(&stored))
                    .or_insert((stored, r.ts.clone()));
                if r.stored.is_string() {
                    let wrapped = serde_json::json!({ "role": "user", "content": r.blocks });
                    index
                        .entry(message_hash(&wrapped))
                        .or_insert((wrapped, r.ts.clone()));
                }
                group.push(r);
            } else {
                flush_group(&mut group, &mut index);
                let full = serde_json::json!({ "role": "assistant", "content": r.blocks });
                index
                    .entry(message_hash(&full))
                    .or_insert((full, r.ts.clone()));
                let stripped = serde_json::json!({
                    "role": "assistant",
                    "content": strip_signatures(&r.blocks)
                });
                index
                    .entry(message_hash(&stripped))
                    .or_insert((stripped, r.ts.clone()));
            }
        }
        flush_group(&mut group, &mut index);
        for (h, m) in &blobs {
            index.insert(h.clone(), (m.clone(), None));
        }
        Some(Read {
            reqs,
            shapes,
            index,
            root: root.map(str::to_string),
            answered,
        })
    }

    /// The harness blocks of a request's shape: the system prompt and the
    /// tool list.
    fn shape_blocks(shape: &Value, ts: Option<&String>) -> Vec<Block> {
        let mut out = Vec::new();
        if let Some(system) = shape.get("system") {
            let mut text = String::new();
            match system {
                Value::String(s) => text.push_str(s),
                Value::Array(parts) => {
                    for p in parts {
                        if let Some(s) = p.get("text").and_then(Value::as_str) {
                            text.push_str(s);
                            text.push('\n');
                        } else if let Some(s) = p.as_str() {
                            text.push_str(s);
                            text.push('\n');
                        }
                    }
                }
                _ => {}
            }
            if !text.is_empty() {
                out.push(new_block(
                    BlockKind::Harness,
                    "Claude Code's system prompt".to_string(),
                    text,
                    ts.cloned(),
                    0,
                    0,
                ));
            }
        }
        if let Some(tools) = shape.get("tools").and_then(Value::as_array) {
            if !tools.is_empty() {
                let compact = serde_json::to_string(tools).unwrap_or_default();
                let pretty = serde_json::to_string_pretty(tools).unwrap_or_default();
                let mut b = new_block(
                    BlockKind::Harness,
                    format!("Tool definitions ({})", tools.len()),
                    pretty,
                    ts.cloned(),
                    0,
                    0,
                );
                b.bytes = compact.len() as u64;
                out.push(b);
            }
        }
        out
    }

    pub fn finish(
        provider: &'static str,
        session_id: &str,
        r: Read,
        mut measured: Measured,
        model: String,
        contexts: &[u64],
    ) -> Stack {
        // The chain: each request's message list is its base's first `keep`
        // messages plus its tail.
        let mut lists: Vec<Vec<String>> = Vec::with_capacity(r.reqs.len());
        let mut by_id: HashMap<&str, usize> = HashMap::new();
        let mut first_seen: HashMap<&str, usize> = HashMap::new();
        for (k, req) in r.reqs.iter().enumerate() {
            let mut list: Vec<String> = match req.base.as_deref().and_then(|b| by_id.get(b)) {
                Some(&bi) => lists[bi].iter().take(req.keep).cloned().collect(),
                None => Vec::new(),
            };
            list.extend(req.tail.iter().cloned());
            for h in &req.tail {
                first_seen.entry(h.as_str()).or_insert(k);
            }
            by_id.insert(req.id.as_str(), k);
            lists.push(list);
        }
        let last = r.reqs.len() - 1;
        let (call_of, linked) = call_of_request(&r.reqs, &r.answered);
        let live = &lists[last];
        let live_set: HashSet<&str> = live.iter().map(String::as_str).collect();

        let mut wire = Wire {
            requests: r.reqs.len(),
            live_messages: live.len(),
            ..Wire::default()
        };
        let root = r.root.as_deref();

        // The live stack: the shape first, then the messages.
        let mut blocks: Vec<Block> = Vec::new();
        let req_ts = r.reqs[last].ts.as_ref();
        if let Some(shape) = r.shapes.get(&r.reqs[last].shape_hash) {
            blocks.extend(shape_blocks(shape, req_ts));
            wire.shape_known = true;
        }
        let mut tool_names: HashMap<String, String> = HashMap::new();
        let mut turn = 0usize;
        for h in live {
            let call = call_of[first_seen.get(h.as_str()).copied().unwrap_or(last)];
            match r.index.get(h) {
                Some((message, ts)) => {
                    let ts = ts.as_ref().or(req_ts);
                    blocks.extend(message_blocks(
                        message,
                        ts,
                        &mut turn,
                        call,
                        &mut tool_names,
                        root,
                    ));
                }
                None => {
                    wire.unresolved += 1;
                    blocks.push(new_block(
                        BlockKind::Attachment,
                        "Unresolved message".to_string(),
                        String::new(),
                        req_ts.cloned(),
                        turn,
                        call,
                    ));
                }
            }
        }
        // A compaction summary arrives as a user message; name it.
        for b in blocks.iter_mut() {
            if b.kind == BlockKind::User
                && b.text
                    .trim_start()
                    .starts_with("This session is being continued from a previous conversation")
            {
                b.kind = BlockKind::CompactionSummary;
                b.label = "Compaction summary".to_string();
            }
        }

        // Messages some earlier request carried and the last does not.
        let mut dropped_hashes: Vec<&str> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for list in &lists {
            for h in list {
                if !live_set.contains(h.as_str()) && seen.insert(h.as_str()) {
                    dropped_hashes.push(h.as_str());
                }
            }
        }
        let mut compacted = Compacted::default();
        let mut scratch_names: HashMap<String, String> = HashMap::new();
        let mut scratch_turn = 0usize;
        for h in &dropped_hashes {
            if let Some((message, ts)) = r.index.get(*h) {
                for b in message_blocks(
                    message,
                    ts.as_ref(),
                    &mut scratch_turn,
                    0,
                    &mut scratch_names,
                    root,
                ) {
                    compacted.blocks += 1;
                    compacted.bytes += b.bytes;
                }
            }
        }
        wire.dropped_messages = dropped_hashes.len();

        // Bytes of the first request, for the record.
        let mut first_names: HashMap<String, String> = HashMap::new();
        let mut first_turn = 0usize;
        let mut first_bytes: u64 = 0;
        if let Some(shape) = r.shapes.get(&r.reqs[0].shape_hash) {
            first_bytes += shape_blocks(shape, None)
                .iter()
                .map(|b| b.bytes)
                .sum::<u64>();
        }
        for h in &lists[0] {
            if let Some((message, ts)) = r.index.get(h) {
                first_bytes += message_blocks(
                    message,
                    ts.as_ref(),
                    &mut first_turn,
                    0,
                    &mut first_names,
                    root,
                )
                .iter()
                .map(|b| b.bytes)
                .sum::<u64>();
            }
        }
        wire.first_request_bytes = first_bytes;
        wire.live_bytes = blocks.iter().map(|b| b.bytes).sum();
        wire.images = blocks.iter().map(|b| b.images).sum();

        // The session's own rates. First the relative pricing of text and
        // opaque bytes, fitted on the growth between consecutive requests
        // (see `fit_rates`); then one factor on both so the live blocks
        // total the measured figure less what the images cost. Exact
        // content, one tokenizer guess per class; without the shape the
        // harness bytes are unknown and the fallback applies.
        let latest = measured.tokens;
        let image_tokens = wire.images as u64 * IMAGE_TOKENS;
        let mut rows: Vec<(u64, u64, u64)> = Vec::new();
        // A request is recorded before its response, so mid-turn the file
        // holds one request more than it has readings. Where responses name
        // their request (`linked`), each answered request pairs with its own
        // reading, and a row needs the request before it answered by the
        // call before. Without the link, rows pair by position, stop at the
        // last answered request, and more than one unanswered means the two
        // are out of step, so no row is trusted.
        let answers = |k: usize| {
            call_of[k] < contexts.len() && (k + 1 == call_of.len() || call_of[k + 1] != call_of[k])
        };
        if linked || (r.reqs.len() >= contexts.len() && r.reqs.len() - contexts.len() <= 1) {
            let mut prev_total = 0usize;
            for (k, req) in r.reqs.iter().enumerate() {
                let total = req.keep + req.tail.len();
                let c = call_of[k];
                if k >= 1
                    && answers(k)
                    && answers(k - 1)
                    && c == call_of[k - 1] + 1
                    && req.keep == prev_total
                    && contexts[c] > contexts[c - 1]
                {
                    let (mut text_b, mut opaque_b, mut images, mut ok) = (0u64, 0u64, 0usize, true);
                    let mut names: HashMap<String, String> = HashMap::new();
                    let mut t = 0usize;
                    for h in &req.tail {
                        match r.index.get(h) {
                            Some((m, ts)) => {
                                for b in message_blocks(m, ts.as_ref(), &mut t, k, &mut names, root)
                                {
                                    text_b += b.bytes.saturating_sub(b.opaque_bytes);
                                    opaque_b += b.opaque_bytes;
                                    images += b.images;
                                }
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok && images == 0 && text_b + opaque_b > 0 {
                        rows.push((contexts[c] - contexts[c - 1], text_b, opaque_b));
                    }
                }
                prev_total = total;
            }
        }
        let fitted = fit_rates(&rows);
        wire.fit_rows = rows.len();
        wire.fit = match &fitted {
            Ok(_) => "ok".to_string(),
            Err(reason) => reason.to_string(),
        };
        let (mut bytes_per_token, mut opaque_bytes_per_token) = fitted.unwrap_or((1.0, 1.0));
        let pool: f64 = blocks
            .iter()
            .filter(|b| b.resent)
            .map(|b| {
                b.bytes.saturating_sub(b.opaque_bytes) as f64 / bytes_per_token
                    + b.opaque_bytes as f64 / opaque_bytes_per_token
            })
            .sum();
        let calibrated = wire.shape_known && pool > 0.0 && latest > image_tokens;
        if calibrated {
            let factor = pool / (latest - image_tokens) as f64;
            bytes_per_token *= factor;
            opaque_bytes_per_token *= factor;
        } else {
            bytes_per_token = FALLBACK_BYTES_PER_TOKEN;
            opaque_bytes_per_token = FALLBACK_BYTES_PER_TOKEN;
        }
        let (accounted, by_kind) =
            scale(&mut blocks, bytes_per_token, opaque_bytes_per_token, latest);
        measured.base = measured.floor;
        Stack {
            session: if session_id.is_empty() {
                String::new()
            } else {
                format!("{provider}:{session_id}")
            },
            provider,
            supported: true,
            reader: Strategy::Requests,
            model,
            measured,
            bytes_per_token,
            opaque_bytes_per_token,
            calibrated,
            accounted_tokens: accounted,
            unaccounted_tokens: latest as i64 - accounted as i64,
            by_kind,
            compacted,
            wire: Some(wire),
            blocks,
        }
    }
}

/// The records strategy: a file without request records, read record by
/// record, with the harness from the latest prompt snapshot and the ratio
/// calibrated on the growth between readings.
mod records {
    use super::*;

    pub struct Walk {
        raw: Vec<Block>,
        harness: Option<Block>,
        tool_names: HashMap<String, String>,
        seen_calls: HashSet<String>,
        call_ctx: Vec<u64>,
        model: String,
        turn: usize,
        /// Index into `raw` of the latest compaction summary.
        compaction_at: Option<usize>,
        root: Option<String>,
    }

    impl Walk {
        fn push(
            &mut self,
            kind: BlockKind,
            label: String,
            text: String,
            ts: Option<String>,
            on_disk: Option<String>,
            resent: bool,
            is_error: bool,
        ) -> usize {
            let i = self.raw.len();
            let mut b = new_block(kind, label, text, ts, self.turn, self.call_ctx.len());
            b.on_disk = on_disk;
            b.resent = resent;
            b.is_error = is_error;
            self.raw.push(b);
            i
        }

        fn user(&mut self, rec: &Value, ts: Option<String>) {
            let is_compact = rec.get("isCompactSummary").and_then(Value::as_bool) == Some(true);
            let is_meta = rec.get("isMeta").and_then(Value::as_bool) == Some(true);
            let Some(content) = rec.pointer("/message/content") else {
                return;
            };
            let items: Vec<Value> = match content {
                Value::String(s) => vec![serde_json::json!({ "type": "text", "text": s })],
                Value::Array(a) => a.clone(),
                _ => return,
            };
            let mut counted_turn = false;
            for item in &items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        let s = item.get("text").and_then(Value::as_str).unwrap_or("");
                        if !counted_turn && !is_meta && !is_compact {
                            self.turn += 1;
                            counted_turn = true;
                        }
                        let kind = if is_compact {
                            BlockKind::CompactionSummary
                        } else if is_meta {
                            BlockKind::Attachment
                        } else {
                            BlockKind::User
                        };
                        let label = if is_compact {
                            "Compaction summary".to_string()
                        } else if is_meta {
                            skill_label(s).unwrap_or_else(|| first_line(s))
                        } else {
                            first_line(s)
                        };
                        let i =
                            self.push(kind, label, s.to_string(), ts.clone(), None, true, false);
                        if is_compact {
                            self.compaction_at = Some(i);
                        }
                    }
                    "tool_result" => {
                        let (text, images) = tool_result_text(item);
                        let name = item
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .and_then(|id| self.tool_names.get(id))
                            .cloned();
                        let mut label = match name {
                            Some(n) => format!("{n} result"),
                            None => "tool result".to_string(),
                        };
                        if images > 0 {
                            label.push_str(&format!(" (+{images} image{})", plural(images)));
                        }
                        let is_error = item.get("is_error").and_then(Value::as_bool) == Some(true);
                        let i = self.push(
                            BlockKind::ToolResult,
                            label,
                            text,
                            ts.clone(),
                            None,
                            true,
                            is_error,
                        );
                        self.raw[i].images = images;
                    }
                    "image" => {
                        let i = self.push(
                            BlockKind::User,
                            "Pasted image".to_string(),
                            String::new(),
                            ts.clone(),
                            None,
                            true,
                            false,
                        );
                        self.raw[i].images = 1;
                    }
                    _ => {}
                }
            }
        }

        fn assistant(&mut self, rec: &Value, ts: Option<String>) {
            let Some(msg) = rec.get("message") else {
                return;
            };
            let id = msg.get("id").and_then(Value::as_str).unwrap_or("");
            let model = msg.get("model").and_then(Value::as_str).unwrap_or("");
            if let Some(usage) = msg.get("usage") {
                if model != "<synthetic>" {
                    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
                    let ctx = n("input_tokens")
                        + n("cache_read_input_tokens")
                        + n("cache_creation_input_tokens");
                    if ctx > 0 {
                        if self.seen_calls.insert(id.to_string()) {
                            self.call_ctx.push(ctx);
                            if !model.is_empty() {
                                self.model = model.to_string();
                            }
                        } else if let Some(last) = self.call_ctx.last_mut() {
                            // A streamed message repeats its usage under one
                            // id: the last record's numbers.
                            *last = ctx;
                        }
                    }
                }
            }
            let Some(Value::Array(items)) = msg.get("content") else {
                return;
            };
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        let s = item.get("text").and_then(Value::as_str).unwrap_or("");
                        if s.is_empty() {
                            continue;
                        }
                        self.push(
                            BlockKind::Agent,
                            first_line(s),
                            s.to_string(),
                            ts.clone(),
                            None,
                            true,
                            false,
                        );
                    }
                    "tool_use" => {
                        let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                        let input = item.get("input").cloned().unwrap_or(Value::Null);
                        if let Some(tid) = item.get("id").and_then(Value::as_str) {
                            self.tool_names.insert(tid.to_string(), name.to_string());
                        }
                        let compact = serde_json::to_string(&input).unwrap_or_default();
                        let pretty = serde_json::to_string_pretty(&input).unwrap_or_default();
                        let i = self.push(
                            BlockKind::ToolCall,
                            tool_call_label(name, &input),
                            format!("{name}\n{pretty}"),
                            ts.clone(),
                            None,
                            true,
                            false,
                        );
                        self.raw[i].bytes = compact.len() as u64;
                    }
                    "thinking" => {
                        let s = item.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if s.is_empty() {
                            continue;
                        }
                        // Without the request records, assume earlier turns'
                        // thinking is not re-sent.
                        self.push(
                            BlockKind::Thinking,
                            format!("Thinking: {}", first_line(s)),
                            s.to_string(),
                            ts.clone(),
                            None,
                            false,
                            false,
                        );
                    }
                    _ => {}
                }
            }
        }

        fn attachment(&mut self, rec: &Value, ts: Option<String>) {
            let Some(att) = rec.get("attachment") else {
                return;
            };
            let ty = att.get("type").and_then(Value::as_str).unwrap_or("");
            match ty {
                "prompt_snapshot" => {
                    let mut text = String::new();
                    match att.get("systemPrompt") {
                        Some(Value::Array(parts)) => {
                            for p in parts {
                                if let Some(s) = p.as_str() {
                                    text.push_str(s);
                                    text.push('\n');
                                }
                            }
                        }
                        Some(Value::String(s)) => text.push_str(s),
                        _ => {}
                    }
                    for key in ["systemTurns", "tools"] {
                        if let Some(v) = att.get(key) {
                            if !v.is_null() {
                                text.push_str("\n\n# ");
                                text.push_str(key);
                                text.push('\n');
                                text.push_str(&serde_json::to_string_pretty(v).unwrap_or_default());
                            }
                        }
                    }
                    // The latest snapshot is the live one; an earlier one was
                    // replaced, not appended to.
                    let mut b = new_block(
                        BlockKind::Harness,
                        "Claude's own prompt and tools".to_string(),
                        text,
                        ts,
                        self.turn,
                        self.call_ctx.len(),
                    );
                    b.i = 0;
                    self.harness = Some(b);
                }
                "instructions" => {
                    let Some(Value::Array(files)) = att.get("files") else {
                        return;
                    };
                    for f in files {
                        let path = f.get("path").and_then(Value::as_str).unwrap_or("");
                        let content = f.get("content").and_then(Value::as_str).unwrap_or("");
                        let ftype = f.get("type").and_then(Value::as_str).unwrap_or("");
                        let kind = if ftype.to_ascii_lowercase().contains("mem") {
                            BlockKind::Memory
                        } else {
                            BlockKind::Instructions
                        };
                        let label = relative(self.root.as_deref(), path);
                        self.push(
                            kind,
                            label,
                            content.to_string(),
                            ts.clone(),
                            Some(path.to_string()),
                            true,
                            false,
                        );
                    }
                }
                "file" => {
                    let name = att.get("filename").and_then(Value::as_str).unwrap_or("");
                    let content = att.get("content").and_then(Value::as_str).unwrap_or("");
                    let base = name.rsplit('/').next().unwrap_or(name);
                    if is_image_name(name) {
                        let i = self.push(
                            BlockKind::Attachment,
                            "Pasted image".to_string(),
                            String::new(),
                            ts,
                            Some(name.to_string()),
                            true,
                            false,
                        );
                        self.raw[i].images = 1;
                    } else {
                        self.push(
                            BlockKind::Attachment,
                            format!("File: {base}"),
                            content.to_string(),
                            ts,
                            Some(name.to_string()),
                            true,
                            false,
                        );
                    }
                }
                "skill_listing" => {
                    let content = att.get("content").and_then(Value::as_str).unwrap_or("");
                    self.push(
                        BlockKind::Attachment,
                        "Skills list".to_string(),
                        content.to_string(),
                        ts,
                        None,
                        true,
                        false,
                    );
                }
                "hook_success" => {
                    let mut text = String::new();
                    for key in ["content", "stdout", "stderr"] {
                        if let Some(s) = att.get(key).and_then(Value::as_str) {
                            if !s.is_empty() {
                                text.push_str(s);
                                text.push('\n');
                            }
                        }
                    }
                    let hook = att.get("hookName").and_then(Value::as_str).unwrap_or("");
                    let label = if hook.is_empty() {
                        "Hook output".to_string()
                    } else {
                        format!("Hook output: {hook}")
                    };
                    self.push(BlockKind::Attachment, label, text, ts, None, true, false);
                }
                "queued_command" => {
                    let prompt = att.get("prompt").and_then(Value::as_str).unwrap_or("");
                    self.push(
                        BlockKind::Attachment,
                        "Queued message".to_string(),
                        prompt.to_string(),
                        ts,
                        None,
                        true,
                        false,
                    );
                }
                "deferred_tools_delta"
                | "deferred_tools_record"
                | "agent_listing_delta"
                | "mcp_instructions_delta" => {
                    let text = match att.get("addedLines") {
                        Some(Value::Array(lines)) if !lines.is_empty() => lines
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => serde_json::to_string_pretty(att).unwrap_or_default(),
                    };
                    self.push(
                        BlockKind::Attachment,
                        "Tool list change".to_string(),
                        text,
                        ts,
                        None,
                        true,
                        false,
                    );
                }
                "edited_text_file" => {
                    let snippet = att.get("snippet").and_then(Value::as_str).unwrap_or("");
                    self.push(
                        BlockKind::Attachment,
                        "Edited file note".to_string(),
                        snippet.to_string(),
                        ts,
                        None,
                        true,
                        false,
                    );
                }
                "critical_system_reminder"
                | "total_tokens_reminder"
                | "batching_reminder_sent"
                | "silent_turn_reminder"
                | "bash_output_audience_note"
                | "session_context" => {
                    let text = ["text", "content", "context"]
                        .iter()
                        .find_map(|k| att.get(*k).and_then(Value::as_str))
                        .map(str::to_string)
                        .unwrap_or_else(|| serde_json::to_string_pretty(att).unwrap_or_default());
                    self.push(
                        BlockKind::Attachment,
                        "Reminder".to_string(),
                        text,
                        ts,
                        None,
                        true,
                        false,
                    );
                }
                "" => {}
                other => {
                    let text = serde_json::to_string_pretty(att).unwrap_or_default();
                    self.push(
                        BlockKind::Attachment,
                        humanize(other),
                        text,
                        ts,
                        None,
                        true,
                        false,
                    );
                }
            }
        }

        fn system(&mut self, rec: &Value, ts: Option<String>) {
            let subtype = rec.get("subtype").and_then(Value::as_str).unwrap_or("");
            if subtype == "local_command" {
                if let Some(content) = rec.get("content").and_then(Value::as_str) {
                    self.push(
                        BlockKind::Attachment,
                        "Command output".to_string(),
                        content.to_string(),
                        ts,
                        None,
                        true,
                        false,
                    );
                }
            }
            // compact_boundary is carried by the summary record that follows;
            // turn_duration and the rest are bookkeeping the model never sees.
        }
    }

    pub fn walk(text: &str, root: Option<String>) -> Walk {
        let mut w = Walk {
            raw: Vec::new(),
            harness: None,
            tool_names: HashMap::new(),
            seen_calls: HashSet::new(),
            call_ctx: Vec::new(),
            model: String::new(),
            turn: 0,
            compaction_at: None,
            root,
        };
        for line in text.lines() {
            let Ok(rec) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if rec.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let ts = rec
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string);
            match rec.get("type").and_then(Value::as_str).unwrap_or("") {
                "user" => w.user(&rec, ts),
                "assistant" => w.assistant(&rec, ts),
                "attachment" => w.attachment(&rec, ts),
                "system" => w.system(&rec, ts),
                _ => {}
            }
        }
        w
    }

    pub fn finish(
        provider: &'static str,
        session_id: &str,
        mut w: Walk,
        mut measured: Measured,
        model: String,
    ) -> Stack {
        let floor = if measured.calls > 0 {
            measured.floor
        } else {
            w.call_ctx.first().copied().unwrap_or(0)
        };
        let latest = if measured.calls > 0 {
            measured.tokens
        } else {
            w.call_ctx.last().copied().unwrap_or(0)
        };
        let model = if model.is_empty() {
            w.model.clone()
        } else {
            model
        };

        // Calibration window: from the first reading that includes the last
        // compaction's summary (else the floor) to the latest reading.
        let boundary_call = w.compaction_at.map(|i| w.raw[i].call).unwrap_or(0);
        let base = w.call_ctx.get(boundary_call).copied().unwrap_or(floor);
        let appended: u64 = w
            .raw
            .iter()
            .filter(|b| b.resent && b.call > boundary_call)
            .map(|b| b.bytes)
            .sum();
        let growth = latest.saturating_sub(base);
        let (bytes_per_token, calibrated) =
            if growth > 0 && appended > 0 && w.call_ctx.len() > boundary_call + 1 {
                (appended as f64 / growth as f64, true)
            } else {
                (FALLBACK_BYTES_PER_TOKEN, false)
            };

        // The live stack: the harness first, then everything from the last
        // compaction's summary on. Earlier blocks survive only as that summary.
        let cut = w.compaction_at.unwrap_or(0);
        let mut compacted = Compacted::default();
        for b in &w.raw[..cut] {
            if b.resent {
                compacted.blocks += 1;
                compacted.bytes += b.bytes;
            }
        }
        let mut blocks: Vec<Block> = Vec::with_capacity(w.raw.len() - cut + 1);
        if let Some(h) = w.harness.take() {
            blocks.push(h);
        }
        blocks.extend(w.raw.drain(cut..));

        let (accounted, by_kind) = scale(&mut blocks, bytes_per_token, bytes_per_token, latest);
        measured.tokens = latest;
        measured.floor = floor;
        measured.base = base;
        Stack {
            session: if session_id.is_empty() {
                String::new()
            } else {
                format!("{provider}:{session_id}")
            },
            provider,
            supported: true,
            reader: Strategy::Records,
            model,
            measured,
            bytes_per_token,
            opaque_bytes_per_token: bytes_per_token,
            calibrated,
            accounted_tokens: accounted,
            unaccounted_tokens: latest as i64 - accounted as i64,
            by_kind,
            compacted,
            wire: None,
            blocks,
        }
    }
}

#[cfg(test)]
#[path = "context_stack_tests.rs"]
mod tests;
