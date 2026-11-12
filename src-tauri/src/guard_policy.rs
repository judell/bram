// The Claude worklist guard's policy (see guard.rs). It began as a shadow port
// (bram-guard-worklist-policy-shadow) of a Python guard, since retired, with
// `bram guard claude-worklist` writing one breadcrumb per invocation so the two
// could be compared. The decision pipeline here is now the implementation. The
// shadow path still runs (guard.rs), so the discipline below, written against
// the Python guard, still describes it until that path is removed.
//
// Shadow discipline, contractual:
//
// - No stdout. The Python guard signals `deny` by exit code 2 plus stderr, and
//   `allow` for lifecycle paths by a JSON permissionDecision on stdout. The
//   shadow prints neither; `run_guard_mode` exits 0 unconditionally.
// - No network. The Python guard POSTs `/__hook-trace` and, on a prose opt-out,
//   `/__audit/direct-edit`. The shadow makes no HTTP call at all.
// - No writes except the breadcrumb line guard.rs appends.
// - Filesystem reads and `git cat-file` are ported as-is: they are what the
//   decisions are made of.
//
// Deliberate divergences from the Python guard are all judell/bram#299 and are
// each marked `#299` at their site: pure forge reads, inline `python -c` whose
// script does not write, and comparison operators inside shell quotes.
//
// Fidelity notes that are NOT divergences, recorded so the grep-join is
// readable:
//
// - The Python guard emits several NON-TERMINAL trace lines per invocation
//   (crossboundary-signed/unparsed, forge-sha-ok, subagent-gate-no-agent-id,
//   the payload-keys observe line). The shadow records only the terminal
//   decision, because the breadcrumb contract is one line per invocation.
// - The breadcrumb's `target` field must not contain whitespace, so where the
//   Python trace target is a 200-char command preview or a comma-joined path
//   list the shadow writes `-`.
// - Python exits 0 silently (no trace at all) on four Write/Edit paths. Those
//   surface here as `no-trace:*` reasons; a `no-trace:` prefix means "the
//   Python guard allows here and emits nothing", so the join expects no twin.

use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct ShadowVerdict {
    pub decision: String,
    pub reason: String,
    pub target: String,
    // Authority mode (bram-guard-authority-flip): the user-facing deny
    // message the Python guard would print to stderr, verbatim. Empty for
    // allows and for deny sites whose message transcription is pending —
    // authority mode then falls back to the no-coverage template, the
    // dominant deny class. Shadow mode ignores this field entirely.
    pub message: String,
    // Set only on a prose-opt-out allow: a stable hash of the matched last
    // user message, for the /__audit/direct-edit breadcrumb authority mode
    // POSTs (the shadow never POSTs). Host dedup keys on the value being
    // stable per turn, not on its algorithm.
    pub audit_turn_key: Option<String>,
}

// Authority-mode fallback deny text, used while a deny site's verbatim
// message transcription is pending. The no-coverage template is transcribed
// from the Python guard's dominant deny (live specimens 2026-08-28/29);
// other reasons get an honest generic that names the reason rather than
// borrowing the wrong template.
pub fn fallback_deny_message(tool: &str, target: &str, root: &Path, reason: &str) -> String {
    let header = format!(
        "[worklist-guard] tool={} target={} cwd={} decision=deny reason={}",
        tool,
        target,
        root.display(),
        reason
    );
    if reason.starts_with("no-coverage") || reason.starts_with("bash-write-no-coverage") {
        return format!(
            "{}\nBlocked: writing to {} requires either a proposed/applied item in {sd}/worklist.json covering this path, or an explicit opt-out phrase in your last message.\n  - Propose the change in {sd}/worklist.json first (item with file=\"{}\", non-empty before and after, status proposed). Wait for the user's approved: payload, then retry.\n  - The user can authorize a direct edit by ending their message with \"just do it\", or by clicking the Skip worklist button.\nresolved_project_root={} (marker: {sd}/.worklist-authorization.json; if this is not the repo root, a stray marker file captured this subtree — remove that {sd}/.worklist-authorization.json)",
            header,
            target,
            target,
            root.display(),
            sd = state_dir_name(root)
        );
    }
    format!(
        "{}\nBlocked by the Bram worklist guard ({}). See .claude/bram-conventions.md for the governing convention; address the named reason and retry.",
        header, reason
    )
}

fn allow(reason: impl Into<String>, target: impl Into<String>) -> ShadowVerdict {
    ShadowVerdict {
        decision: "allow".into(),
        reason: reason.into(),
        target: target.into(),
        message: String::new(),
        audit_turn_key: None,
    }
}

fn deny(reason: impl Into<String>, target: impl Into<String>) -> ShadowVerdict {
    ShadowVerdict {
        decision: "deny".into(),
        reason: reason.into(),
        target: target.into(),
        message: String::new(),
        audit_turn_key: None,
    }
}

// --- authority-mode deny messages -------------------------------------------
//
// The Python guard's stderr on a deny is TWO parts: `_trace_hook` writes the
// `[worklist-guard] …` diagnostic line first (always, tracing on or off), then
// the site's own `print(msg, file=sys.stderr)`. `authority_worklist_hook`
// eprintln!s `ShadowVerdict::message` and nothing else, so the transcribed
// message carries both halves, joined by a newline.
//
// Three header fields vary per site and are NOT the verdict's own fields:
//
// - `tool` — "Bash" on the Bash branch, the mcp__ tool name on the MCP branch,
//   and the Write/Edit tool name elsewhere (Python stashes it in
//   `__BRAM_TRACE_TOOL` for the deny_* helpers).
// - `target` — the Python trace target, which on Bash is the 200-char command
//   preview and on `mcp-unrecognized-input` is the empty string. The verdict's
//   own `target` stays whitespace-free for the breadcrumb, so the two differ.
// - `cwd` — the payload cwd on the Bash and MCP branches (passed explicitly),
//   and the process cwd everywhere else, where Python lets `_trace_hook`
//   default to `os.getcwd()`.
fn hook_stderr_header(tool: &str, target: &str, cwd: &str, decision: &str, reason: &str) -> String {
    format!(
        "[worklist-guard] tool={} target={} cwd={} decision={} reason={}",
        tool, target, cwd, decision, reason
    )
}

/// Python `os.getcwd()`, with its `except` branch's empty string.
fn process_cwd() -> String {
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// A deny carrying the Python guard's verbatim stderr text.
/// `verdict_target` is the breadcrumb's target; `header_*` are the trace
/// line's fields (see above); `body` is the site's `print(...)` text.
fn deny_msg(
    reason: impl Into<String>,
    verdict_target: impl Into<String>,
    header_tool: &str,
    header_target: &str,
    header_cwd: &str,
    body: &str,
) -> ShadowVerdict {
    let reason = reason.into();
    let header = hook_stderr_header(header_tool, header_target, header_cwd, "deny", &reason);
    let mut v = deny(reason, verdict_target);
    v.message = format!("{}\n{}", header, body);
    v
}

/// `deny_coverage` (python claude-worklist-guard.py:1183-1245). Returns the
/// (reason, message-body) pair; the caller owns the trace header fields.
fn deny_coverage_parts(
    target_rel: &str,
    opt_out_attempted: bool,
    project_root: Option<&Path>,
    session_root: Option<&Path>,
    session_bypass_detail: Option<&str>,
) -> (String, String) {
    let mut msg = format!(
        "Blocked: writing to {target} requires either a proposed/applied item in {sd}/worklist.json covering this path, or an explicit opt-out phrase in your last message.\n  - Propose the change in {sd}/worklist.json first (item with file=\"{target}\", non-empty before and after, status proposed). Wait for the user's approved: payload, then retry.\n  - The user can authorize a direct edit by ending their message with \"just do it\", or by clicking the Skip worklist button.",
        target = target_rel,
        sd = STATE_DIR_NEW
    );
    if opt_out_attempted {
        msg.push_str("\n  - (Detected what looked like opt-out language, but it didn't match the expected phrasing. The verbal opt-out is exactly \"just do it\"; otherwise use the Skip worklist button.)");
    }
    let mut reason = "no-coverage-no-opt-out".to_string();
    let (_, worktree) = worktree_coverage_target(target_rel);
    if let Some(worktree) = worktree {
        msg.push_str(&format!(
            "\nworktree={} (coverage checked against the corresponding real-tree path; this denial is still authoritative)",
            worktree
        ));
        reason.push_str(&format!(":worktree={}", worktree));
    }
    if let Some(root) = project_root {
        msg.push_str(&claude_coverage_root_line(root));
        reason.push_str(&format!(":root={}", root.display()));
    }
    if let (Some(sr), Some(detail)) = (session_root, session_bypass_detail) {
        msg.push_str(&format!(
            "\nAlso checked the session project's direct-edit bypass (cwd-derived root: {}): {}.",
            sr.display(),
            bypass_detail_label(sr, detail)
        ));
        reason.push_str(&format!(":session_root={}:{}", sr.display(), detail));
    }
    (reason, msg)
}

/// The `resolved_project_root=` line shared by `deny_coverage` and
/// `deny_bash_coverage` (issue-280).
fn claude_coverage_root_line(project_root: &Path) -> String {
    format!(
        "\nresolved_project_root={} (marker: {}; if this is not the repo root, a stray marker file captured this subtree — remove that {})",
        project_root.display(),
        state_rel(project_root, AUTH_LEAF),
        state_rel(project_root, AUTH_LEAF)
    )
}

/// judell/bram#287's deny text as the Bash-redirect (python:2087-2096) and MCP
/// (python:2179-2187) surfaces print it. The Write/Edit surface's copy carries
/// an extra clause and is spelled out at that site instead. A template: the
/// call sites print it as is.
const SUBAGENT_WORKLIST_WRITE_BODY: &str = "Worklist proposals and claim edits (.bram/worklist.json, .bram/worklist-drafts/*) stay in the orchestrator's own turn. A delegated subagent cannot write them directly. See 'Delegating worklist items to subagents' in conventions.md and judell/bram#287.";

/// `_BYPASS_DETAIL_LABELS` (python:1175-1180), with its `.get(detail, detail)`
/// fallback to the raw detail token.
fn bypass_detail_label(root: &Path, detail: &str) -> String {
    match detail {
        "absent" => format!("no {} there", state_rel(root, AUTH_LEAF)),
        "wrong-kind" => "a record exists there but isn't a direct-edit bypass".to_string(),
        "stale" => "a direct-edit record there has expired (>1h old)".to_string(),
        "path-not-covered" => {
            "a fresh direct-edit record there doesn't cover this path".to_string()
        }
        other => other.to_string(),
    }
}

// --- constants (mirror of the Python module's) ------------------------------

// Every state path below is a LEAF inside the project's state folder, joined
// onto the folder resolved for the root in question (`state_rel` /
// `state_path`), never a literal folder prefix (issue-469).
const WORKLIST_LEAF: &str = "worklist.json";
const WORKLIST_DRAFTS_LEAF: &str = "worklist-drafts/";
const WORKTREE_PREFIX: &str = ".claude/worktrees/";
const AUTH_LEAF: &str = ".worklist-authorization.json";
const BYPASS_TTL_MS: f64 = 60.0 * 60.0 * 1000.0;
const POST_COMMIT_PUSH_GRACE_MS: f64 = 10.0 * 60.0 * 1000.0;

const LIFECYCLE_LEAVES_EXACT: &[&str] = &[
    "worklist.json",
    ".worklist-authorization.json",
    ".inflight-claim.json",
    ".pty-intent.jsonl",
    ".worklist-intent.json",
    ".worklist-result.json",
    ".bram-port",
    ".bram-port.json",
    // guard-link-per-project: the host records its own guard here.
    ".bram-guard-target",
];
const LIFECYCLE_LEAVES_PREFIXES: &[&str] = &[
    "worklist-drafts/",
    "worklist-citations/",
    "feedback-drafts/",
    "feedback-history/",
    "bram-traces/",
];

// --- state folder resolution (issue-469) -------------------------------------
//
// Bram's per-project state folder is `.bram/` (judell/bram#469). Since
// issue-469-retire-resources the guard reads nothing from the pre-0.8.4
// `resources/`: a 0.8.7 host either moved a project's state at startup or
// held the start, and a held start runs no agent. A project with no Bram
// `.bram/` is unmanaged here.
//
// `~/.bram` is the machine folder (it holds `bram-guard`), never a project's
// state folder, and a `.bram/` without Bram's marker belongs to another tool:
// neither is a state folder (`has_state_dir`), so no breadcrumb lands beside
// the guard binary or in another tool's folder, and no walk-up stops there.
pub const STATE_DIR_NEW: &str = ".bram";

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn is_home(dir: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else {
        return false;
    };
    if dir == home {
        return true;
    }
    match (std::fs::canonicalize(dir), std::fs::canonicalize(home)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether `dir/.bram` is Bram's (judell/bram#469): it carries one of the two
/// markers every Bram-made state folder has, `.migrated-from` (the mover) or a
/// `.gitignore` whose content is exactly `*` (`ensure_new_state_dir`, the
/// demo hydrator). A `.bram/` without either belongs to something else (a
/// CodeExam index, say) and is never adopted. This is the one rule: the
/// host's `is_bram_state_dir` calls it.
pub fn is_bram_state_dir(root: &Path) -> bool {
    let dir = root.join(STATE_DIR_NEW);
    dir.join(".migrated-from").exists()
        || std::fs::read_to_string(dir.join(".gitignore")).is_ok_and(|s| s.trim() == "*")
}

/// Whether `dir/.bram` is a project state folder (it is Bram's, and isn't
/// `~/.bram`).
fn has_project_bram_dir(dir: &Path, home: Option<&Path>) -> bool {
    dir.join(STATE_DIR_NEW).is_dir() && !is_home(dir, home) && is_bram_state_dir(dir)
}

/// Whether `root` has a state folder: Bram's `.bram/`, not `~/.bram`. The test
/// before any write into it (the breadcrumb).
pub fn has_state_dir(root: &Path) -> bool {
    has_project_bram_dir(root, home_dir().as_deref())
}

/// One line for the `bash-write-no-coverage` denial when a `.bram/` that isn't
/// Bram's sits at the root: the host holds that start, so there is no board
/// to cover the write. Deny path only.
fn foreign_dot_bram_hint(root: &Path) -> Option<String> {
    let dot = root.join(STATE_DIR_NEW);
    if !dot.is_dir() || is_bram_state_dir(root) || is_home(root, home_dir().as_deref()) {
        return None;
    }
    Some("\n  - A folder named .bram/ that isn't Bram's is in the way, so Bram hasn't started on this project and there is no worklist to cover this. The user can rename that folder and relaunch Bram (judell/bram#469).".to_string())
}

/// The state folder's name: `.bram`. Kept as a function of the root for its
/// callers, which ask per root.
pub fn state_dir_name(_root: &Path) -> &'static str {
    STATE_DIR_NEW
}

/// `<root>/.bram`.
pub fn state_dir(root: &Path) -> PathBuf {
    root.join(STATE_DIR_NEW)
}

/// `leaf` inside `root`'s state folder, as an absolute-ish path.
pub fn state_path(root: &Path, leaf: &str) -> PathBuf {
    state_dir(root).join(leaf)
}

/// `leaf` inside `root`'s state folder, root-relative with `/` separators —
/// the shape `normalize_target` produces.
pub fn state_rel(root: &Path, leaf: &str) -> String {
    format!("{}/{}", state_dir_name(root), leaf)
}

fn state_dir_candidates_with_home(dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    if has_project_bram_dir(dir, home) {
        vec![dir.join(STATE_DIR_NEW)]
    } else {
        Vec::new()
    }
}

/// For walk-ups: the state folder to try at one level (Bram's `.bram/`, or
/// none), so a nested instance is found before its parent.
pub fn state_dir_candidates(dir: &Path) -> Vec<PathBuf> {
    state_dir_candidates_with_home(dir, home_dir().as_deref())
}

/// `rel`'s path inside `root`'s state folder, if it is inside it.
fn state_leaf<'a>(root: &Path, rel: &'a str) -> Option<&'a str> {
    rel.strip_prefix(state_dir_name(root))?.strip_prefix('/')
}

fn is_worklist_rel(root: &Path, rel: &str) -> bool {
    state_leaf(root, rel) == Some(WORKLIST_LEAF)
}

// The state paths the tests build fixtures with.
#[cfg(test)]
const WORKLIST_REL: &str = ".bram/worklist.json";
#[cfg(test)]
const AUTH_REL: &str = ".bram/.worklist-authorization.json";
#[cfg(test)]
const DIRECT_EDIT_REL: &str = ".bram/.worklist-direct-edit.json";
#[cfg(test)]
const WORKLIST_INTENT_REL: &str = ".bram/.worklist-intent.json";
#[cfg(test)]
const WORKLIST_RESULT_REL: &str = ".bram/.worklist-result.json";

/// A test fixture's state folder as Bram makes it: `<root>/.bram` with a
/// `.gitignore` of `*` (the marker `is_bram_state_dir` reads).
#[cfg(test)]
pub(crate) fn test_make_state_dir(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root.join(STATE_DIR_NEW))?;
    std::fs::write(root.join(STATE_DIR_NEW).join(".gitignore"), "*\n")
}

const MCP_WRITE_TOKENS: &[&str] = &[
    "write", "edit", "create", "delete", "remove", "rename", "move", "copy", "patch", "append",
    "truncate", "mkdir", "rmdir", "modify", "replace", "save", "set_",
];

// judell/bram#384 retired the sibling `MCP_PATH_KEYS` enumeration (thirteen
// hardcoded key names, top-level only) that used to stand in for "does this
// call name a repo path". See `mcp_candidate_paths` below for the structural
// replacement -- it is a property test over every string in `tool_input`,
// not a membership test against a list, so there is no key list to keep here
// at all. `MCP_WRITE_TOKENS` above is untouched: it still separates reads
// from writes, which the structural test is not trying to replace (see the
// known-limit note at `mcp_candidate_paths`).

const WORKLIST_LIFECYCLE_ROUTES: &[&str] = &[
    "/__worklist/resolve",
    "/__worklist/mutate",
    "/__worklist/commit",
];

const NONREPO_REDIRECT_EXACT: &[&str] = &["/dev/null", "/dev/zero", "/tmp", "/private/tmp"];
const NONREPO_REDIRECT_PREFIXES: &[&str] = &["/tmp/", "/private/tmp/"];

// --- character-scanning primitives ------------------------------------------
//
// The Python guard's classifiers are regexes; guard mode is stdlib-only, so
// each one is hand-ported as a scanner over `Vec<char>`. Every helper below
// encodes one regex construct, so a reader can check the port against the
// pattern it replaces rather than against prose.

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// `(^|[\s;&|`(])` — the command-position boundary every verb pattern opens with.
fn at_boundary(c: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    matches!(
        c[i - 1],
        ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | ';' | '&' | '|' | '`' | '('
    )
}

/// Python `\w`: unicode alphanumerics plus underscore.
fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Trailing `\b`: end of input, or the next char is not a word char.
fn word_end(c: &[char], i: usize) -> bool {
    i >= c.len() || !is_word(c[i])
}

/// Leading `\b` before a word char at `i`.
fn word_start(c: &[char], i: usize) -> bool {
    i == 0 || !is_word(c[i - 1])
}

/// Literal match at `i`; returns the index just past it.
fn lit(c: &[char], i: usize, s: &str) -> Option<usize> {
    let mut j = i;
    for ch in s.chars() {
        if j >= c.len() || c[j] != ch {
            return None;
        }
        j += 1;
    }
    Some(j)
}

/// `\s+`
fn ws1(c: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    while j < c.len() && c[j].is_whitespace() {
        j += 1;
    }
    if j == i {
        None
    } else {
        Some(j)
    }
}

/// `\s*`
fn ws0(c: &[char], i: usize) -> usize {
    let mut j = i;
    while j < c.len() && c[j].is_whitespace() {
        j += 1;
    }
    j
}

/// `(^|[\s;&|`(])<head>\s+[<sub>\s+]<verb>\b` — the shape of every
/// `gh issue <verb>` / `git <verb>` pattern in `_BASH_WRITE_PATTERNS`.
fn cmd_verb(c: &[char], heads: &[&str], subs: Option<&[&str]>, verbs: &[&str]) -> bool {
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        for head in heads {
            let Some(j) = lit(c, i, head) else { continue };
            let Some(mut j) = ws1(c, j) else { continue };
            if let Some(subs) = subs {
                let mut matched = false;
                for s in subs {
                    if let Some(k) = lit(c, j, s) {
                        if let Some(k2) = ws1(c, k) {
                            j = k2;
                            matched = true;
                            break;
                        }
                    }
                }
                if !matched {
                    continue;
                }
            }
            for v in verbs {
                if let Some(k) = lit(c, j, v) {
                    if word_end(c, k) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `(^|[\s;&|`(])<word>\b` — bare-command patterns (`tee`, `rm`, `mv`, …).
fn cmd_word(c: &[char], words: &[&str]) -> bool {
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        for w in words {
            if let Some(k) = lit(c, i, w) {
                if word_end(c, k) {
                    return true;
                }
            }
        }
    }
    false
}

/// `(^|[\s;&|`(])<head>\s+<flag>\b` — `node -e`, `bash -c`, `sh -c`.
fn cmd_flag(c: &[char], head: &str, flag: &str) -> bool {
    cmd_flag_at(c, head, flag).is_some()
}

fn cmd_flag_at(c: &[char], head: &str, flag: &str) -> Option<usize> {
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        let Some(j) = lit(c, i, head) else { continue };
        let Some(j) = ws1(c, j) else { continue };
        if let Some(k) = lit(c, j, flag) {
            if word_end(c, k) {
                return Some(k);
            }
        }
    }
    None
}

/// `(^|[\s;&|`(])<head>\s+[^|;&]*<flag>\b` — `sed -i`, `perl -i`, where the
/// in-place flag may sit anywhere before the next pipeline separator.
fn cmd_inplace(c: &[char], head: &str, flag: &str) -> bool {
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        let Some(j) = lit(c, i, head) else { continue };
        let Some(j) = ws1(c, j) else { continue };
        let mut k = j;
        while k < c.len() && !matches!(c[k], '|' | ';' | '&') {
            if let Some(e) = lit(c, k, flag) {
                if word_end(c, e) {
                    return true;
                }
            }
            k += 1;
        }
    }
    false
}

/// `(^|[\s;&|`(])python[0-9.]*\s+-c\b`. Returns the index just past `-c`.
fn python_dash_c_positions(c: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        let Some(mut j) = lit(c, i, "python") else {
            continue;
        };
        while j < c.len() && (c[j].is_ascii_digit() || c[j] == '.') {
            j += 1;
        }
        let Some(j) = ws1(c, j) else { continue };
        if let Some(k) = lit(c, j, "-c") {
            if word_end(c, k) {
                out.push(k);
            }
        }
    }
    out
}

/// `open\s*\(\s*['"][^'"]+['"]\s*,\s*['"][wax]` — an inline file open in write,
/// append, or exclusive-create mode. No leading boundary in the Python pattern.
fn open_write_mode(c: &[char]) -> bool {
    for i in 0..c.len() {
        let Some(j) = lit(c, i, "open") else { continue };
        let j = ws0(c, j);
        if j >= c.len() || c[j] != '(' {
            continue;
        }
        let j = ws0(c, j + 1);
        if j >= c.len() || !matches!(c[j], '\'' | '"') {
            continue;
        }
        let mut k = j + 1;
        let start = k;
        while k < c.len() && !matches!(c[k], '\'' | '"') {
            k += 1;
        }
        if k == start || k >= c.len() {
            continue;
        }
        let j = ws0(c, k + 1);
        if j >= c.len() || c[j] != ',' {
            continue;
        }
        let j = ws0(c, j + 1);
        if j >= c.len() || !matches!(c[j], '\'' | '"') {
            continue;
        }
        if j + 1 < c.len() && matches!(c[j + 1], 'w' | 'a' | 'x') {
            return true;
        }
    }
    false
}

/// `(^|[\s;&|`(])git\s+(-C\s+\S+\s+)?push\b`
fn push_cmd(command: &str) -> bool {
    let c = chars(command);
    for i in 0..c.len() {
        if !at_boundary(c.as_slice(), i) {
            continue;
        }
        let Some(j) = lit(&c, i, "git") else { continue };
        let Some(j) = ws1(&c, j) else { continue };
        // Optional `-C <path> `.
        let mut candidates = vec![j];
        if let Some(k) = lit(&c, j, "-C") {
            if let Some(k) = ws1(&c, k) {
                let mut e = k;
                while e < c.len() && !c[e].is_whitespace() {
                    e += 1;
                }
                if e > k {
                    if let Some(k2) = ws1(&c, e) {
                        candidates.push(k2);
                    }
                }
            }
        }
        for cand in candidates {
            if let Some(k) = lit(&c, cand, "push") {
                if word_end(&c, k) {
                    return true;
                }
            }
        }
    }
    false
}

// --- redirect scanning (quote-aware, #299 case 3) ---------------------------

/// Every `> x` / `>> x` redirect target, quotes stripped.
///
/// #299 (case 3): the Python guard's `_REDIRECT_TARGET_RX` is quote-blind, so
/// `awk '$1 >= "2026-08-28"'` reads as a shell redirect and the command is
/// denied `bash-write-no-coverage`. This scanner tracks single- and
/// double-quote state (and backslash escapes) and only reports redirects that
/// are actually unquoted. A `>` inside quotes is a comparison operator, JSON,
/// or jq syntax — never a redirect.
fn redirect_targets(command: &str) -> Vec<String> {
    let c = chars(command);
    let n = c.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    while i < n {
        let ch = c[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match ch {
            '\'' => {
                in_single = true;
                i += 1;
            }
            '"' => {
                in_double = true;
                i += 1;
            }
            '\\' => i += 2,
            '>' => {
                let boundary_ok = at_boundary(&c, i);
                let mut j = i;
                while j < n && c[j] == '>' {
                    j += 1;
                }
                if boundary_ok {
                    let k = ws0(&c, j);
                    if k < n && c[k] != '>' && c[k] != '&' {
                        if matches!(c[k], '\'' | '"') {
                            // Quoted target: consume to the matching quote so
                            // quote state stays coherent for the remainder.
                            let q = c[k];
                            let mut e = k + 1;
                            let mut buf = String::new();
                            while e < n && c[e] != q {
                                buf.push(c[e]);
                                e += 1;
                            }
                            out.push(buf);
                            i = if e < n { e + 1 } else { e };
                            continue;
                        }
                        let mut e = k;
                        while e < n && !c[e].is_whitespace() && c[e] != '>' && c[e] != '&' {
                            e += 1;
                        }
                        let raw: String = c[k..e].iter().collect();
                        out.push(strip_matching_quotes(&raw));
                        i = e;
                        continue;
                    }
                }
                i = j;
            }
            _ => i += 1,
        }
    }
    out
}

fn strip_matching_quotes(s: &str) -> String {
    let v: Vec<char> = s.chars().collect();
    if v.len() >= 2 && v[0] == v[v.len() - 1] && matches!(v[0], '\'' | '"') {
        v[1..v.len() - 1].iter().collect()
    } else {
        s.to_string()
    }
}

fn has_redirect(command: &str) -> bool {
    !redirect_targets(command).is_empty()
}

fn is_nonrepo_redirect_target(t: &str) -> bool {
    NONREPO_REDIRECT_EXACT.contains(&t)
        || NONREPO_REDIRECT_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// True iff a redirect targets somewhere other than a no-op sink or temp
/// scratch space.
fn redirect_is_write(command: &str) -> bool {
    redirect_targets(command)
        .iter()
        .any(|t| !is_nonrepo_redirect_target(t))
}

// --- inline `python -c` content test (#299 case 2) --------------------------

/// Write indicators inside an inline Python script. Deliberately conservative
/// per #299: `.write` catches `sys.stdout.write` too, and any `open(` counts,
/// so the divergence only covers scripts that plainly touch nothing.
const PY_WRITE_HINTS: &[&str] = &[
    "open(",
    ".write",
    "os.remove",
    "os.unlink",
    "os.rename",
    "os.replace",
    "os.rmdir",
    "os.mkdir",
    "os.makedirs",
    "os.system",
    "os.chmod",
    "os.chown",
    "os.truncate",
    "os.link",
    "os.symlink",
    "shutil",
    "subprocess",
    "json.dump(",
    "pickle.dump",
    "csv.writer",
    ".unlink(",
    ".mkdir(",
    ".touch(",
    ".rmdir(",
    ".rename(",
    "urllib",
    "requests.",
    "socket.",
];

/// Extract the argument that follows `-c` at `i`, honouring shell quoting.
/// None means "could not read it" — the conservative branch.
fn shell_arg_after(c: &[char], i: usize) -> Option<String> {
    let j = ws0(c, i);
    if j >= c.len() {
        return None;
    }
    if matches!(c[j], '\'' | '"') {
        let q = c[j];
        let mut e = j + 1;
        let mut buf = String::new();
        while e < c.len() {
            if q == '"' && c[e] == '\\' && e + 1 < c.len() {
                buf.push(c[e + 1]);
                e += 2;
                continue;
            }
            if c[e] == q {
                return Some(buf);
            }
            buf.push(c[e]);
            e += 1;
        }
        return None; // unterminated quote: unreadable
    }
    let mut e = j;
    let mut buf = String::new();
    while e < c.len() && !c[e].is_whitespace() {
        buf.push(c[e]);
        e += 1;
    }
    if buf.is_empty() {
        None
    } else {
        Some(buf)
    }
}

// --- inline `node -e` content test (#362) -----------------------------------

/// Write/side-effect indicators inside an inline Node script. Same
/// deliberately-conservative posture as PY_WRITE_HINTS: `require(` and
/// `import` catch every module that could reach the filesystem or spawn a
/// process, network clients count as side effects (matching python's
/// urllib/requests/socket entries), so the carve-out covers only scripts
/// that plainly touch nothing — Walt's `console.log(1+1)` class (#362).
const NODE_WRITE_HINTS: &[&str] = &[
    "require(",
    "import(",
    "import ",
    "fs.",
    "child_process",
    "writeFile",
    "appendFile",
    "createWriteStream",
    ".unlink",
    ".rmdir",
    ".mkdir",
    ".rename",
    ".truncate",
    ".chmod",
    ".symlink",
    ".copyFile",
    "execSync",
    "spawnSync",
    "spawn(",
    "exec(",
    "fetch(",
    "http.",
    "https.",
    "net.",
    "WebSocket",
    "process.binding",
    "Deno.",
    "Bun.",
];

/// `(^|[\s;&|`(])node\s+-e\b`. Returns the index just past `-e`.
fn node_dash_e_positions(c: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        let Some(j) = lit(c, i, "node") else { continue };
        let Some(j) = ws1(c, j) else { continue };
        if let Some(k) = lit(c, j, "-e") {
            if word_end(c, k) {
                out.push(k);
            }
        }
    }
    out
}

/// #362: `node -e` used to classify as a write by the FLAG alone, denying
/// pure-expression one-liners (`node -e "console.log(1+1)"` — Walt's
/// confirmed false positive). Mirror of python_dash_c_writes: read the
/// inline script; no write indicator → not a write via this pattern.
/// Unreadable, expansion-bearing, or hint-carrying scripts stay writes —
/// ambiguity degrades to over-denying, never under-denying.
fn node_dash_e_writes(command: &str) -> bool {
    let c = chars(command);
    let positions = node_dash_e_positions(&c);
    if positions.is_empty() {
        return false;
    }
    for p in positions {
        match shell_arg_after(&c, p) {
            None => return true,
            Some(script) => {
                if script.contains('$') || script.contains('`') {
                    return true;
                }
                if NODE_WRITE_HINTS.iter().any(|h| script.contains(h)) {
                    return true;
                }
            }
        }
    }
    false
}

/// unowned-lines-postmortem-fixes: a script fed to `python`/`node` on stdin
/// through a heredoc (`python3 - <<'EOF'`, `python3 -I -`, `node - <<EOF`) is
/// a script, so it gets the same body inspection as `python -c` / `node -e`.
/// The general `open(write-mode)` scan runs on quote-MASKED text, so in a
/// heredoc `open(p,'w')` became `open(p,   )` and a file-writing script
/// classified `bash-read-only` (2026-10-09 06:09:03Z, an unauthorized edit of
/// app/__shell/helpers.js). Only an interpreter line whose remaining words
/// are all flags (or `-`) counts: `python3 tool.py <<EOF` feeds DATA, not a
/// script. An unquoted heredoc expands `$`/backticks, so a body carrying
/// either is read conservatively, as for `-c`.
fn interpreter_heredoc_writes(command: &str) -> bool {
    let lines: Vec<&str> = command.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some(at) = line.find("<<") else { continue };
        let Some(lang) = heredoc_interpreter(&line[..at]) else {
            continue;
        };
        let rest = line[at + 2..].trim_start_matches('-').trim_start();
        let quoted = rest.starts_with('\'') || rest.starts_with('"');
        let tag: String = rest
            .trim_start_matches(|c| c == '\'' || c == '"')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if tag.is_empty() {
            return true;
        }
        let mut body = String::new();
        while i < lines.len() && lines[i].trim() != tag {
            body.push_str(lines[i]);
            body.push('\n');
            i += 1;
        }
        if !quoted && (body.contains('$') || body.contains('`')) {
            return true;
        }
        let hints = if lang == "node" {
            NODE_WRITE_HINTS
        } else {
            PY_WRITE_HINTS
        };
        if hints.iter().any(|h| body.contains(h)) {
            return true;
        }
    }
    false
}

/// The interpreter a heredoc line runs as a stdin script: "python" (any
/// `python`, `python3`, `python3.12`) or "node", when every word after it
/// before the `<<` is a flag or `-`. The interpreter may follow `;`, `&&`,
/// `|` or `(` on the same line.
fn heredoc_interpreter(prefix: &str) -> Option<&'static str> {
    let segment = prefix
        .rsplit(|c| c == ';' || c == '|' || c == '&' || c == '(')
        .next()
        .unwrap_or(prefix);
    let mut words = segment.split_whitespace();
    let first = words.next()?;
    let base = first.rsplit('/').next().unwrap_or(first);
    let lang = if base == "node" {
        "node"
    } else if base.starts_with("python")
        && base["python".len()..]
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
    {
        "python"
    } else {
        return None;
    };
    if words.all(|w| w.starts_with('-')) {
        Some(lang)
    } else {
        None
    }
}

/// #299 (case 2): the Python guard treats the mere presence of `python -c` as
/// a write. Here the inline script is read: if it can be extracted and carries
/// no write indicator, the command is not a write via this pattern. When the
/// script cannot be extracted (unquoted, unterminated, built by expansion) the
/// port stays conservative and matches Python.
fn python_dash_c_writes(command: &str) -> bool {
    let c = chars(command);
    let positions = python_dash_c_positions(&c);
    if positions.is_empty() {
        return false;
    }
    for p in positions {
        match shell_arg_after(&c, p) {
            None => return true,
            Some(script) => {
                // Shell expansion means the text the guard reads is not the
                // text python runs, so the content test cannot conclude
                // anything — stay conservative and match the Python guard.
                if script.contains('$') || script.contains('`') {
                    return true;
                }
                if PY_WRITE_HINTS.iter().any(|h| script.contains(h)) {
                    return true;
                }
            }
        }
    }
    false
}

// --- forge read classification (#299 case 1) --------------------------------

/// `(gh|glab) (issue|pr|mr) (list|view|status|diff|checks)` — pure forge reads.
///
/// #299 (case 1): `gh issue list --state all --json …` was denied while
/// `gh issue view N --json …` passed. Both are reads. In the current Python
/// source neither verb is in `_GH_ISSUE_WRITE_RX` or `_FORGE_WRITE_RX`, so a
/// bare `gh issue list` is already classified read; the observed denial came
/// from the quote-blind redirect scan firing on a `--jq '… > …'` filter (case 3
/// above). This predicate makes the read classification explicit and testable
/// so a future widening of the forge verb lists cannot silently recapture it.
fn forge_read_command(command: &str) -> bool {
    let c = chars(command);
    cmd_verb(
        &c,
        &["gh", "glab"],
        Some(&["issue", "pr", "mr"]),
        &["list", "view", "status", "diff", "checks"],
    )
}

// --- bash write classifier ---------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum SkipPattern {
    None,
    GhIssueWrite,
}

const GIT_WRITE_VERBS: &[&str] = &[
    "add",
    "commit",
    "push",
    "rm",
    "mv",
    "reset",
    "checkout",
    "restore",
    "stash",
    "am",
    "apply",
    "cherry-pick",
    "rebase",
    "revert",
    "tag",
    "branch",
];

const GH_ISSUE_WRITE_VERBS: &[&str] = &[
    "close", "reopen", "edit", "comment", "create", "delete", "transfer", "pin", "unpin", "lock",
    "unlock",
];

const FORGE_ISSUE_ONLY_VERBS: &[&str] = &["comment", "create", "edit", "close", "reopen"];

fn gh_issue_write(c: &[char]) -> bool {
    cmd_verb(c, &["gh"], Some(&["issue"]), GH_ISSUE_WRITE_VERBS)
}

// #333: quoted spans are argument DATA for shell-token classification — a
// grep pattern containing `git apply` must not classify the grep as a write.
// Interior characters of single-/double-quoted spans become spaces; the
// delimiters stay. Backslash escapes are honoured outside single quotes.
// Returns None on an unterminated quote: the caller falls back to the flat
// scan, so ambiguity degrades to over-denying, never under-denying.
//
// The interpreter trap is handled structurally, not here: `bash -c` / `sh -c`
// / `node -e` classify as writes by the FLAG, which sits outside the quotes,
// and the python/open inspectors below deliberately receive the RAW command
// because the quoted script body is exactly what they inspect.
fn mask_quoted_spans(command: &str) -> Option<String> {
    let mut out = String::with_capacity(command.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in command.chars() {
        match quote {
            None => {
                if escaped {
                    escaped = false;
                    out.push(ch);
                } else if ch == '\\' {
                    escaped = true;
                    out.push(ch);
                } else if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    out.push(ch);
                } else {
                    out.push(ch);
                }
            }
            Some(q) => {
                if escaped {
                    escaped = false;
                    out.push(' ');
                } else if q == '"' && ch == '\\' {
                    escaped = true;
                    out.push(' ');
                } else if ch == q {
                    quote = None;
                    out.push(ch);
                } else {
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                }
            }
        }
    }
    (quote.is_none() && !escaped).then_some(out)
}

// #333: every `git <write-verb>` invocation with its argument tokens, up to a
// command separator — so listing forms can be judged per invocation and a
// compound like `git tag --list && git push` still classifies as a write.
fn git_verb_segments(c: &[char]) -> Vec<(&'static str, Vec<String>)> {
    let mut out = Vec::new();
    for i in 0..c.len() {
        if !at_boundary(c, i) {
            continue;
        }
        let Some(j) = lit(c, i, "git") else { continue };
        let Some(j) = ws1(c, j) else { continue };
        for verb in GIT_WRITE_VERBS {
            let Some(k) = lit(c, j, verb) else { continue };
            if !word_end(c, k) {
                continue;
            }
            let mut args = Vec::new();
            let mut p = k;
            loop {
                while p < c.len() && matches!(c[p], ' ' | '\t') {
                    p += 1;
                }
                if p >= c.len() || matches!(c[p], ';' | '&' | '|' | '`' | '(' | ')' | '\n') {
                    break;
                }
                let start = p;
                while p < c.len()
                    && !c[p].is_whitespace()
                    && !matches!(c[p], ';' | '&' | '|' | '`' | '(' | ')')
                {
                    p += 1;
                }
                let tok: String = c[start..p].iter().collect();
                let tok = tok.trim_matches(['\'', '"']).to_string();
                if !tok.is_empty() {
                    args.push(tok);
                }
            }
            out.push((*verb, args));
            break;
        }
    }
    out
}

// #333: the enumerated read-only shapes of mutation-capable git verbs.
// Anything outside them stays a write — `git tag v1` must keep denying.
fn git_invocation_is_readonly(verb: &str, args: &[String]) -> bool {
    match verb {
        // With -l/--list, tag is a listing whatever else follows (patterns
        // are matched, never created).
        "tag" => args.iter().any(|a| a == "-l" || a == "--list"),
        // Bare `git branch` lists. Otherwise: only known listing flags, plus
        // bare pattern tokens when --list is present. NOT `-l`, which
        // historically meant create-with-reflog.
        "branch" => {
            let allowed = [
                "--list",
                "-a",
                "-r",
                "-v",
                "-vv",
                "--all",
                "--remotes",
                "--show-current",
            ];
            let has_list = args.iter().any(|a| a == "--list");
            args.iter()
                .all(|a| allowed.contains(&a.as_str()) || (has_list && !a.starts_with('-')))
        }
        "stash" => matches!(
            args.first().map(String::as_str),
            Some("list") | Some("show")
        ),
        _ => false,
    }
}

/// #333: the classifier names its evidence. `None` = not a write; `Some(tok)`
/// = a write, where `tok` is the matched token — surfaced in the deny reason
/// so a misclassification is a one-line diagnosis instead of a guessing game.
fn write_patterns_token(command: &str, skip: SkipPattern) -> Option<String> {
    // Quote-blind checks first, on the RAW command: redirects have their own
    // quote-awareness (#299 case 3), and the python/open inspectors read
    // quoted script bodies on purpose.
    if redirect_is_write(command) {
        return Some("redirect".to_string());
    }
    if python_dash_c_writes(command) {
        return Some("python -c".to_string());
    }
    // #362: node -e joins python -c as body-inspected (raw command — the
    // quoted script is exactly what it reads), replacing the flag-alone
    // classification below that denied pure-expression one-liners.
    if node_dash_e_writes(command) {
        return Some("node -e".to_string());
    }
    if interpreter_heredoc_writes(command) {
        return Some("interpreter heredoc".to_string());
    }
    let masked_owned = mask_quoted_spans(command);
    let scan: &str = masked_owned.as_deref().unwrap_or(command);
    let c = chars(scan);
    if open_write_mode(&c) {
        return Some("open(write-mode)".to_string());
    }
    if cmd_word(&c, &["tee"]) {
        return Some("tee".to_string());
    }
    if cmd_inplace(&c, "sed", "-i") {
        return Some("sed -i".to_string());
    }
    if cmd_inplace(&c, "perl", "-i") {
        return Some("perl -i".to_string());
    }
    for w in ["rm", "mv", "cp", "truncate", "install"] {
        if cmd_word(&c, &[w]) {
            return Some(w.to_string());
        }
    }
    for (verb, args) in git_verb_segments(&c) {
        if !git_invocation_is_readonly(verb, &args) {
            return Some(format!("git {}", verb));
        }
    }
    if skip != SkipPattern::GhIssueWrite && gh_issue_write(&c) {
        return Some("gh issue <write-verb>".to_string());
    }
    if cmd_flag(&c, "bash", "-c") {
        return Some("bash -c".to_string());
    }
    if cmd_flag(&c, "sh", "-c") {
        return Some("sh -c".to_string());
    }
    None
}

fn write_patterns_match(command: &str, skip: SkipPattern) -> bool {
    write_patterns_token(command, skip).is_some()
}

fn bash_writes(command: &str) -> bool {
    write_patterns_match(command, SkipPattern::None)
}

/// conventions.md, "Issue-only forge work with no repo diff".
fn forge_issue_only_write(command: &str) -> bool {
    let c = chars(command);
    if !cmd_verb(&c, &["gh"], Some(&["issue"]), FORGE_ISSUE_ONLY_VERBS) {
        return false;
    }
    // Any redirect at all disqualifies, regardless of target — the refusal
    // that forces "write the body file in its own step".
    if has_redirect(command) {
        return false;
    }
    !write_patterns_match(command, SkipPattern::GhIssueWrite)
}

/// Issue #176: `gh … --body @…`. `(?:^|\s|;|&&|\|\||\|)gh\s.*?--body\s+@\S`,
/// MULTILINE — ported per line because `.` never crosses a newline.
fn is_gh_body_at_antipattern(command: &str) -> bool {
    gh_body_at_match(command).is_some()
}

/// The Python deny prints `Detected: {m.group(0).strip()}`, so authority mode
/// needs the matched span, not just the boolean. `group(0)` opens at the
/// boundary token the alternation consumed — `^` (zero-width), one whitespace
/// / `;` / `|` character, or the two-character `&&` / `||` — and closes one
/// character past the `@`. `.strip()` then removes a whitespace opener but
/// leaves `;` / `|` / `&&` in place.
fn gh_body_at_match(command: &str) -> Option<String> {
    for line in command.split('\n') {
        let c = chars(line);
        for i in 0..c.len() {
            let boundary = i == 0
                || matches!(c[i - 1], ' ' | '\t' | '\r' | '\u{b}' | '\u{c}' | ';' | '|')
                || (c[i - 1] == '&' && i >= 2 && c[i - 2] == '&');
            if !boundary {
                continue;
            }
            let Some(j) = lit(&c, i, "gh") else { continue };
            if j >= c.len() || !c[j].is_whitespace() {
                continue;
            }
            for k in j..c.len() {
                let Some(m) = lit(&c, k, "--body") else {
                    continue;
                };
                let Some(m) = ws1(&c, m) else { continue };
                if m < c.len() && c[m] == '@' && m + 1 < c.len() && !c[m + 1].is_whitespace() {
                    // Leftmost start: the two-character alternatives (`&&`,
                    // `||`) begin one position earlier than the single-char
                    // ones, and re.search takes the earliest start that
                    // matches at all.
                    let start = if i == 0 {
                        0
                    } else if i >= 2 && c[i - 1] == c[i - 2] && matches!(c[i - 1], '&' | '|') {
                        i - 2
                    } else {
                        i - 1
                    };
                    let matched: String = c[start..m + 2].iter().collect();
                    return Some(matched.trim().to_string());
                }
            }
        }
    }
    None
}

// --- cross-boundary signature + SHA verification ----------------------------

// #363: blank heredoc BODIES before the publishing screens (signature,
// session-url, sha) look for forge-write shapes. Heredoc content is text
// being written into a file, not command text — Walt's case wrote a draft
// whose prose mentioned `gh issue create … --body-file <file>`, and the
// signature scan matched it, then denied on the not-yet-existing body file
// (#331's unreadable-body rule, correctly load-bearing, aimed at a
// misparse). Line-based: introducers (`<<TAG`, `<<-TAG`, quoted tags) queue
// in order, body lines blank until the terminator line, the terminator and
// all command lines survive. A heredoc that never terminates returns the
// command UNCHANGED — ambiguity degrades to today's over-denying, never to
// under-screening. The Bash write classifier's view is deliberately
// untouched: a heredoc that writes a file still classifies as that write.
fn mask_heredoc_bodies(command: &str) -> String {
    if !command.contains("<<") {
        return command.to_string();
    }
    fn introducers(line: &str) -> Vec<(String, bool)> {
        let c: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < c.len() {
            if c[i] == '<' && c[i + 1] == '<' {
                // Not a here-string (<<<) and not the tail of one.
                if (i + 2 < c.len() && c[i + 2] == '<') || (i > 0 && c[i - 1] == '<') {
                    i += 1;
                    continue;
                }
                let mut j = i + 2;
                let dash = j < c.len() && c[j] == '-';
                if dash {
                    j += 1;
                }
                while j < c.len() && c[j] == ' ' {
                    j += 1;
                }
                let quote = if j < c.len() && (c[j] == '\'' || c[j] == '"') {
                    let q = c[j];
                    j += 1;
                    Some(q)
                } else {
                    None
                };
                let start = j;
                while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                    j += 1;
                }
                if j > start {
                    if let Some(q) = quote {
                        if j < c.len() && c[j] == q {
                            j += 1;
                        } else {
                            i += 1;
                            continue; // unterminated quoted tag: not an introducer
                        }
                    }
                    out.push((
                        c[start..]
                            .iter()
                            .take(j - start - quote.map_or(0, |_| 1))
                            .collect::<String>(),
                        dash,
                    ));
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        out
    }
    let mut masked_lines: Vec<String> = Vec::new();
    let mut queue: std::collections::VecDeque<(String, bool)> = std::collections::VecDeque::new();
    for line in command.lines() {
        if let Some((tag, dash)) = queue.front().cloned() {
            let candidate = if dash {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if candidate == tag {
                queue.pop_front();
                masked_lines.push(line.to_string());
            } else {
                masked_lines.push(" ".repeat(line.chars().count()));
            }
            continue;
        }
        for intro in introducers(line) {
            queue.push_back(intro);
        }
        masked_lines.push(line.to_string());
    }
    if !queue.is_empty() {
        // Unterminated heredoc: can't tell body from command — fall back.
        return command.to_string();
    }
    masked_lines.join("\n")
}

// --- Bash write-target extraction (issue-387) --------------------------------
//
// `bash_writes` / `codex_bash_writes` already answer "is this command a
// write"; the functions below answer "to what path", for the tractable
// subset named in the item: `>`, `>>`, `tee`, and heredoc redirects (the
// heredoc's own `>` target, not its body). They exist because neither Bash
// branch calls `trace_would_deny_unaddressed` today -- the guard denies
// uncovered Bash writes correctly (enforcement is not the gap), but it can
// only ask "does ANY item cover the worklist", never "does an item cover
// THIS path", so #368's question is unanswerable for the majority of writes
// in this repo (197 Bash vs. 33 Write + 11 Edit, tool calls since
// 2026-09-13).
//
// Same invariant `mask_heredoc_bodies` above documents for its neighbouring
// parser, carried over verbatim: ambiguity degrades to declining, never to a
// guessed path. A wrong path is a confidently false observation; "could not
// determine" is not. Command substitution, variable expansion, and `eval`
// make the general case undecidable, so this does not attempt it -- it
// extracts the tractable shapes and says "unknown" about everything else.

/// A target token that embeds shell expansion cannot be trusted as the
/// literal path the shell will actually write -- `$VAR` or `` `cmd` `` might
/// resolve to any path at all, including one outside the extracted token's
/// apparent shape. Reporting it as-is would be exactly the "guessed path"
/// the invariant above forbids.
fn bash_target_is_untrustworthy(token: &str) -> bool {
    token.contains('$') || token.contains('`')
}

/// `redirect_targets`'s unquoted-token scan stops at whitespace, `>`, and
/// `&`, but not at a bare `;` or `|` -- so a target directly followed by a
/// separator with no space (`> out.txt;`, `>out.txt|wc`) reads the
/// separator as part of the path. An unescaped `;`/`|`/`&` outside quotes
/// always ends a shell word, so cutting the token there is not a guess --
/// it is what the shell itself would do. Applied uniformly to every
/// extracted token (quoted targets containing these characters literally
/// are rare enough, and outside this project's file names entirely, that
/// the tradeoff favors the common unquoted case).
fn tractable_target_token(raw: &str) -> Option<String> {
    let end = raw.find([';', '|', '&']).unwrap_or(raw.len());
    let trimmed = raw[..end].trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `tee`'s positional file arguments: everything after the bare word `tee`
/// up to the next command separator, skipping flags (`-a`, `--append`, ...)
/// and the lone `-` stdin/stdout marker, which names no file. Quote-aware in
/// the same style as `redirect_targets` below.
fn tee_targets(command: &str) -> Vec<String> {
    let c: Vec<char> = command.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        if !at_boundary(&c, i) {
            i += 1;
            continue;
        }
        let Some(j) = lit(&c, i, "tee") else {
            i += 1;
            continue;
        };
        if !word_end(&c, j) {
            i += 1;
            continue;
        }
        let mut p = j;
        loop {
            p = ws0(&c, p);
            if p >= c.len() || matches!(c[p], ';' | '&' | '|' | '`' | '(' | ')' | '\n') {
                break;
            }
            if matches!(c[p], '\'' | '"') {
                let q = c[p];
                let start = p + 1;
                let mut e = start;
                while e < c.len() && c[e] != q {
                    e += 1;
                }
                let tok: String = c[start..e].iter().collect();
                p = if e < c.len() { e + 1 } else { e };
                if !tok.is_empty() {
                    out.push(tok);
                }
                continue;
            }
            let start = p;
            while p < c.len()
                && !c[p].is_whitespace()
                && !matches!(c[p], ';' | '&' | '|' | '`' | '(' | ')')
            {
                p += 1;
            }
            let tok: String = c[start..p].iter().collect();
            if tok.is_empty() {
                break;
            }
            if tok != "-" && !tok.starts_with('-') {
                out.push(tok);
            }
        }
        i = if p > j { p } else { j + 1 };
    }
    out
}

/// The outcome of trying to extract Bash write targets from a command that
/// already classifies as a write. `Unknown` is not an error -- it is the
/// honest answer for every shape outside the tractable subset (`sed -i`,
/// `rm`/`mv`/`cp`, `git`, `python -c`, ...) and for any tractable shape whose
/// target could not be trusted (expansion-bearing).
enum BashWriteTarget {
    /// One or more concrete, literal write targets, as written in the
    /// command -- not yet resolved against a project root.
    Paths(Vec<String>),
    /// The command is a write; no target could be safely determined.
    Unknown,
}

fn bash_write_targets(command: &str) -> BashWriteTarget {
    // Heredoc BODY text is masked first so a `>` (or the word `tee`) that
    // merely appears inside a heredoc's payload -- example shell snippets in
    // a commit message draft, say -- is never misread as a real redirect.
    // Same body/command split `mask_heredoc_bodies` already performs for the
    // signature/SHA scanners; see its own comment above.
    let masked = mask_heredoc_bodies(command);
    let mut out: Vec<String> = Vec::new();
    let mut saw_untrustworthy = false;
    for raw in redirect_targets(&masked)
        .into_iter()
        .chain(tee_targets(&masked))
    {
        let Some(t) = tractable_target_token(&raw) else {
            continue;
        };
        if bash_target_is_untrustworthy(&t) {
            saw_untrustworthy = true;
            continue;
        }
        out.push(t);
    }
    if saw_untrustworthy || out.is_empty() {
        return BashWriteTarget::Unknown;
    }
    let mut seen: HashSet<String> = HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    BashWriteTarget::Paths(out)
}

const FORGE_WRITE_VERBS: &[&str] = &["create", "comment", "note", "edit", "update", "review"];

fn is_forge_write(command: &str) -> bool {
    let c = chars(command);
    if cmd_verb(
        &c,
        &["gh", "glab"],
        Some(&["issue", "pr", "mr"]),
        FORGE_WRITE_VERBS,
    ) {
        return true;
    }
    if cmd_verb(&c, &["gh"], Some(&["gist"]), &["create", "edit"])
        || cmd_verb(&c, &["glab"], Some(&["snippet"]), &["create", "update"])
    {
        return true;
    }
    let lower = command.to_ascii_lowercase();
    lower.contains("gh api")
        && (lower.contains("--method post")
            || lower.contains("--method patch")
            || lower.contains("-x post")
            || lower.contains("-x patch"))
        && (lower.contains("/comments") || lower.contains("gists/"))
}

fn forge_positional_body_file(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    let file_backed = lower.contains("gh gist edit")
        || lower.contains("gh gist create")
        || lower.contains("glab snippet create")
        || lower.contains("glab snippet update");
    if !file_backed {
        return None;
    }
    let token = command.split_whitespace().last()?.trim_matches(['\'', '"']);
    (!token.is_empty() && !token.starts_with('-')).then(|| normalize_body_file_path(token))
}

fn forge_api_body(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    let body_pos = lower.find("body=")?;
    let start = body_pos + "body=".len();
    let bytes = command.as_bytes();
    let quote = body_pos
        .checked_sub(1)
        .and_then(|i| bytes.get(i).copied())
        .filter(|b| matches!(*b, b'\'' | b'"'));
    let mut end = start;
    while end < bytes.len() {
        if quote
            .map(|q| bytes[end] == q)
            .unwrap_or(bytes[end].is_ascii_whitespace())
        {
            break;
        }
        end += 1;
    }
    Some(command[start..end].replace("\\n", "\n"))
}

/// `(?:--body-file|-F)(?:=|\s+)(value)`, with two #347 corrections to the
/// original naive positional scan:
/// - Flags are matched only OUTSIDE quoted regions and at a token boundary —
///   a `-F` inside a quoted `--title` argument is prose, not a flag (filing
///   #347 itself was denied on exactly that).
/// - `-F` in gh api carries field syntax: `-F key=@path` is file-backed and
///   the file is the part after the `@` (the prefix was previously left on
///   the path, so a readable signed body was denied as unreadable);
///   `-F key=value` with no `@` names no file and the scan continues.
// guard-no-session-urls: session URLs are private telemetry; a commit or
// forge body carrying one publishes it irreversibly, and no opt-in exists.
// The prose rule (conventions, 'No session URLs in public artifacts')
// demonstrably does not bind a session whose harness default says to append
// the trailer — every ~/budget commit on 2026-09-06 carried one despite the
// seeded rule — so this is enforcement, the cargo-fmt-at-the-gate shape.
// The matcher is deliberately narrow: the session-URL host path and the
// trailer key, case-insensitive. It is applied only to publishing-shaped
// commands (git commit, forge writes) so a grep or trace inspection that
// mentions the URL stays an ordinary read.
pub(crate) fn contains_session_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("claude.ai/code/") || lower.contains("claude-session:")
}

// A command that runs `git … commit` in any of its segments. Segment-scoped
// so `git log | grep commit` doesn't read as a commit — and even a false
// positive only matters when the same command also carries a session URL.
fn is_git_commit_command(command: &str) -> bool {
    for line in command.split(['\n', ';', '|']) {
        for sub in line.split("&&") {
            let toks: Vec<&str> = sub.split_whitespace().collect();
            if let Some(gi) = toks.iter().position(|t| *t == "git") {
                if toks[gi + 1..].iter().any(|t| *t == "commit") {
                    return true;
                }
            }
        }
    }
    false
}

// `git commit -F <path>` / `--file <path>` — the message file the guard must
// scan, since the violation rides the message, not the command line.
fn git_commit_message_file_arg(command: &str) -> Option<String> {
    let toks: Vec<&str> = command.split_whitespace().collect();
    for (i, t) in toks.iter().enumerate() {
        if (*t == "-F" || *t == "--file") && i + 1 < toks.len() {
            let v = toks[i + 1].trim_matches(|ch: char| ch == '"' || ch == '\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
        if let Some(v) = t.strip_prefix("--file=") {
            return Some(
                v.trim_matches(|ch: char| ch == '"' || ch == '\'')
                    .to_string(),
            );
        }
    }
    None
}

// deny when a publishing-shaped command carries a session URL in its command
// text or in a readable message/body file. Returns ("deny", where) or
// ("skip", ""). An unreadable named file is left to the signature check's
// existing unreadable-body denial — this check never fails open on text it
// DID read.
fn session_url_verdict(command: &str, cwd: &Path) -> (&'static str, String) {
    let masked = mask_heredoc_bodies(command); // #363
    let command = masked.as_str();
    if !is_git_commit_command(command) && !is_forge_write(command) {
        return ("skip", String::new());
    }
    if contains_session_url(command) {
        return ("deny", "command-text".to_string());
    }
    let mut paths: Vec<String> = Vec::new();
    if let Some(p) = git_commit_message_file_arg(command) {
        paths.push(p);
    }
    if let Some(p) = body_file_arg(command) {
        paths.push(p);
    }
    if let Some(p) = input_file_arg(command) {
        paths.push(p);
    }
    for p in paths {
        if p == "-" {
            continue;
        }
        let candidate = Path::new(&p);
        let text = std::fs::read_to_string(candidate)
            .or_else(|_| std::fs::read_to_string(cwd.join(candidate)));
        if let Ok(text) = text {
            if contains_session_url(&text) {
                return ("deny", p);
            }
        }
    }
    ("skip", String::new())
}

fn body_file_arg(command: &str) -> Option<String> {
    let c = chars(command);
    let mut quote: Option<char> = None;
    let mut i = 0usize;
    while i < c.len() {
        if let Some(q) = quote {
            if c[i] == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if matches!(c[i], '\'' | '"') {
            quote = Some(c[i]);
            i += 1;
            continue;
        }
        let mut matched_end: Option<usize> = None;
        for flag in ["--body-file", "-F"] {
            if !at_boundary(&c, i) {
                break;
            }
            let Some(j) = lit(&c, i, flag) else { continue };
            let k = if j < c.len() && c[j] == '=' {
                j + 1
            } else if let Some(k) = ws1(&c, j) {
                k
            } else {
                continue;
            };
            // Value runs to the matching quote when it opens with one
            // (quoted paths may contain spaces), else to whitespace.
            let mut e = k;
            if e < c.len() && matches!(c[e], '\'' | '"') {
                let q = c[e];
                e += 1;
                while e < c.len() && c[e] != q {
                    e += 1;
                }
                if e < c.len() {
                    e += 1;
                }
            } else {
                while e < c.len() && !c[e].is_whitespace() {
                    e += 1;
                }
            }
            if e > k {
                let raw: String = c[k..e].iter().collect();
                if flag == "-F" {
                    if let Some((_, value)) = raw.split_once('=') {
                        let value = strip_surrounding_quotes(value);
                        match value.strip_prefix('@') {
                            Some(path) => return Some(normalize_body_file_path(path)),
                            // Inline field, not a file: skip the value and
                            // keep scanning (a real --body-file may follow).
                            None => {
                                matched_end = Some(e);
                                break;
                            }
                        }
                    }
                }
                return Some(normalize_body_file_path(&raw));
            }
        }
        i = matched_end.unwrap_or(i + 1);
    }
    None
}

/// `--input(?:=|\s+)(\S+)` — gh api's JSON payload file. Recognized (#347) so
/// the file-backed `--input` route is signature-verified like `-F body=@`,
/// instead of falling open as no-inspectable-body while the equivalent
/// field-syntax write is checked.
fn input_file_arg(command: &str) -> Option<String> {
    if !command.to_ascii_lowercase().contains("gh api") {
        return None;
    }
    let c = chars(command);
    for i in 0..c.len() {
        let Some(j) = lit(&c, i, "--input") else {
            continue;
        };
        if !at_boundary(&c, i) {
            continue;
        }
        let k = if j < c.len() && c[j] == '=' {
            j + 1
        } else if let Some(k) = ws1(&c, j) {
            k
        } else {
            continue;
        };
        let mut e = k;
        while e < c.len() && !c[e].is_whitespace() {
            e += 1;
        }
        if e > k {
            let raw: String = c[k..e].iter().collect();
            return Some(normalize_body_file_path(&raw));
        }
    }
    None
}

/// `(?:--body|-b|--message|-m|--description)(?:=|\s+)("…"|'…'|\S+)`
fn body_literal_arg(command: &str) -> Option<String> {
    let c = chars(command);
    for i in 0..c.len() {
        for flag in ["--body", "-b", "--message", "-m", "--description"] {
            let Some(j) = lit(&c, i, flag) else { continue };
            let k = if j < c.len() && c[j] == '=' {
                j + 1
            } else if let Some(k) = ws1(&c, j) {
                k
            } else {
                continue;
            };
            if k >= c.len() {
                continue;
            }
            if c[k] == '"' {
                let mut e = k + 1;
                while e < c.len() {
                    if c[e] == '\\' && e + 1 < c.len() {
                        e += 2;
                        continue;
                    }
                    if c[e] == '"' {
                        return Some(c[k..=e].iter().collect());
                    }
                    e += 1;
                }
                // Unterminated: fall through to the \S+ arm, as the regex would.
            } else if c[k] == '\'' {
                let mut e = k + 1;
                while e < c.len() && c[e] != '\'' {
                    e += 1;
                }
                if e < c.len() {
                    return Some(c[k..=e].iter().collect());
                }
            }
            let mut e = k;
            while e < c.len() && !c[e].is_whitespace() {
                e += 1;
            }
            if e > k {
                return Some(c[k..e].iter().collect());
            }
        }
    }
    None
}

/// #331: strip exactly one pair of matching surrounding quotes, nothing else.
/// Deliberately NOT unquote_shell: its `\n` unescaping would corrupt a Windows
/// path like `C:\Users\jon\new\body.md`, turning `\new` into a newline. A
/// path token needs its bytes preserved.
fn strip_surrounding_quotes(value: &str) -> &str {
    let b = value.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// #331: an MSYS absolute path (`/c/Users/...`) mapped to its drive form
/// (`C:/Users/...`). Pure and platform-independent so it can be tested
/// anywhere; call sites gate on Windows, where the Bash tool is a Git Bash
/// shell and the MSYS spelling is the natural one. On POSIX `/c/...` is a
/// legitimate real path and must not be rewritten.
fn msys_drive_path(value: &str) -> Option<String> {
    let c: Vec<char> = value.chars().collect();
    if c.len() >= 2 && c[0] == '/' && c[1].is_ascii_alphabetic() && (c.len() == 2 || c[2] == '/') {
        let rest: String = c[2..].iter().collect();
        return Some(format!("{}:{}", c[1].to_ascii_uppercase(), rest));
    }
    None
}

/// #331: what body_file_arg and the positional variant hand onward. Quoted
/// paths are the idiomatic Windows spelling and were previously taken
/// verbatim — the file then never opened, and the guard blamed the signature.
fn normalize_body_file_path(raw: &str) -> String {
    let stripped = strip_surrounding_quotes(raw);
    if cfg!(windows) {
        if let Some(mapped) = msys_drive_path(stripped) {
            return mapped;
        }
    }
    stripped.to_string()
}

fn unquote_shell(value: &str) -> String {
    let v: Vec<char> = value.chars().collect();
    if v.len() >= 2 && v[0] == v[v.len() - 1] && matches!(v[0], '\'' | '"') {
        let inner: String = v[1..v.len() - 1].iter().collect();
        return inner.replace("\\\"", "\"").replace("\\n", "\n");
    }
    value.to_string()
}

/// (body_text, reason). None body means "unreadable", with reason naming why.
fn crossboundary_body(command: &str, cwd: &Path) -> (Option<String>, &'static str) {
    if let Some(path) = body_file_arg(command) {
        if path == "-" {
            return (None, "body-file-stdin");
        }
        let candidate = if Path::new(&path).is_absolute() {
            PathBuf::from(&path)
        } else {
            cwd.join(&path)
        };
        return match std::fs::read(&candidate) {
            Ok(bytes) => (
                Some(String::from_utf8_lossy(&bytes).into_owned()),
                "body-file",
            ),
            Err(_) => (None, "body-file-unreadable"),
        };
    }
    // #347: gh api --input <file> is a JSON payload; its `body` field is the
    // inspectable prose. A payload with no body field is metadata-only
    // (input-json-no-body fails open at the verdict sites); an unreadable or
    // unparseable named file is denied, per #331's named-file principle.
    if let Some(path) = input_file_arg(command) {
        if path == "-" {
            return (None, "input-file-stdin");
        }
        let candidate = if Path::new(&path).is_absolute() {
            PathBuf::from(&path)
        } else {
            cwd.join(&path)
        };
        return match std::fs::read(&candidate) {
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(v) => match v.get("body").and_then(|b| b.as_str()) {
                    Some(b) => (Some(b.to_string()), "input-json-body"),
                    None => (None, "input-json-no-body"),
                },
                Err(_) => (None, "input-json-unparseable"),
            },
            Err(_) => (None, "input-file-unreadable"),
        };
    }
    if let Some(lit) = body_literal_arg(command) {
        return (Some(unquote_shell(&lit)), "body-literal");
    }
    if let Some(body) = forge_api_body(command) {
        return (Some(body), "api-body-field");
    }
    if let Some(path) = forge_positional_body_file(command) {
        let candidate = if Path::new(&path).is_absolute() {
            PathBuf::from(&path)
        } else {
            cwd.join(&path)
        };
        return match std::fs::read(&candidate) {
            Ok(bytes) => (
                Some(String::from_utf8_lossy(&bytes).into_owned()),
                "positional-body-file",
            ),
            Err(_) => (None, "positional-body-file-unreadable"),
        };
    }
    (None, "no-body-flag")
}

fn normalize_repo_locator(remote: &str) -> Option<String> {
    let mut value = remote.trim().trim_end_matches('/').to_string();
    if let Some(rest) = value.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        value = format!("{host}/{path}");
    } else if let Some((_, rest)) = value.split_once("://") {
        value = rest.to_string();
        if let Some((_, tail)) = value.split_once('@') {
            value = tail.to_string();
        }
    } else {
        return None;
    }
    value = value
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();
    let (host, path) = value.split_once('/')?;
    if host.is_empty() || path.is_empty() || !host.contains('.') {
        return None;
    }
    Some(format!("{}/{}", host.to_ascii_lowercase(), path))
}

/// guard-git-no-window: a `git` command that opens no console window on
/// Windows. The guard is GUI-subsystem (`bin/bram-guard.rs`), so it has no
/// console for a console-subsystem child to inherit; without
/// CREATE_NO_WINDOW every `git` it runs gets a new, visible console. On
/// 2026-11-09 that popped 3–4 windows per `guard_selftest` case at every
/// Windows launch (judell/bram#469, Windows recovery acceptance).
fn guard_git_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub(crate) fn expected_repo_locator(cwd: &Path) -> Option<String> {
    let safe_directory = format!("safe.directory={}", cwd.display());
    let output = guard_git_command("git")
        .args(["-c", &safe_directory, "remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_repo_locator(&String::from_utf8_lossy(&output.stdout))
}

// awaiting-cross-machine-agent-moves: the parsed agent signature, shared
// with the Awaiting You sweep in lib.rs — one grammar, two consumers. The
// standing slots after thread and model are optional and positional:
// `<os>` (added 2026-09-05, #346 legibility) then `<machine>` (the short
// hostname, added same day so two same-OS machines are distinguishable and
// the sweep can classify same-account comments exactly). Older two- and
// three-slot signatures parse with the trailing fields None.
//
// An optional `version` slot (added 2026-09-07) leads the parenthetical when
// present: a first slot of the literal form "Bram <version>" (a non-empty
// remainder after the "Bram " prefix) is captured as `version` and the
// standing slots shift over by one — thread becomes the second slot rather
// than the first. A first slot that merely starts with letters spelling
// "Bram" without the trailing space plus content (e.g. "Bramble 5") is not a
// version: it falls through to ordinary thread validation and fails it.
// Signatures with no version slot at all parse exactly as before, with
// `version: None`.
pub(crate) struct AgentSignature<'a> {
    // The thread slot ("main thread" | "subagent") is validated by the parser
    // but not stored: no consumer reads it yet. Re-add the field when one does.
    // The version slot ("Bram 0.6.5", added 2026-09-07) went unread for four
    // days; `signature_version_verdict` now compares it against the guard's own
    // compiled version, so a slot that is present and WRONG is denied rather
    // than published. The Awaiting You display consumer is still a follow-up.
    pub(crate) version: Option<&'a str>,
    pub(crate) model: &'a str,
    pub(crate) os: Option<&'a str>,
    pub(crate) machine: Option<&'a str>,
    // awaiting-cross-project-agent-moves: the human-familiar project name
    // ("Bram", "Budget") — display material for the Awaiting surface; the
    // locator is what identity comparisons use.
    pub(crate) project: &'a str,
    pub(crate) locator: &'a str,
    // "<owner>'s <Agent>", the speaker label the Awaiting You sweep shows.
    pub(crate) speaker: &'a str,
    // Expander-only optional bullets (the one-line form carries none).
    // `commit` is the HEAD short sha with any `+dirty` suffix stripped.
    pub(crate) commit: Option<&'a str>,
    pub(crate) item: Option<&'a str>,
    pub(crate) agent_cli: Option<&'a str>,
}

pub(crate) fn parse_agent_signature(line: &str) -> Option<AgentSignature<'_>> {
    let line = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '_' | '*' | '>' | '#' | '-' | ' '))
        .trim_end_matches(|ch| matches!(ch, '_' | '*' | ' '));
    let (owner, rest) = line.split_once("'s ")?;
    if owner.trim().is_empty() {
        return None;
    }
    let (agent, rest) = rest.split_once(" (")?;
    let speaker = line[..owner.len() + 3 + agent.len()].trim();
    if !matches!(agent, "Claude" | "Codex" | "OpenCode" | "Antigravity") {
        return None;
    }
    let (standing, rest) = rest.split_once(") speaking from the ")?;
    let mut slots = standing.split(", ");
    let first = slots.next()?.trim();
    let (version, thread) = match first.strip_prefix("Bram ") {
        Some(v) if !v.trim().is_empty() => (Some(v.trim()), slots.next()?.trim()),
        _ => (None, first),
    };
    let model = slots.next()?.trim();
    if !matches!(thread, "main thread" | "subagent") || model.is_empty() {
        return None;
    }
    let os = slots.next().map(str::trim).filter(|s| !s.is_empty());
    let machine = slots.next().map(str::trim).filter(|s| !s.is_empty());
    let rest = rest.strip_suffix("):")?;
    let (project, locator) = rest.rsplit_once(" project (")?;
    if project.trim().is_empty() || locator.trim().is_empty() {
        return None;
    }
    Some(AgentSignature {
        version,
        model,
        os,
        machine,
        project: project.trim(),
        locator: locator.trim(),
        speaker,
        commit: None,
        item: None,
        agent_cli: None,
    })
}

/// forge-signature-as-expander: the summary of the expander form,
/// `<owner>'s <Agent> speaking from <locator>`. The fields come from the
/// block's bullets (`parse_expander_signature`).
fn parse_agent_summary(summary: &str) -> Option<AgentSignature<'_>> {
    let summary = summary.trim();
    if summary.contains('\n') {
        return None;
    }
    let (owner, rest) = summary.split_once("'s ")?;
    if owner.trim().is_empty() {
        return None;
    }
    let (agent, locator) = rest.split_once(" speaking from ")?;
    if !matches!(agent, "Claude" | "Codex" | "OpenCode" | "Antigravity") {
        return None;
    }
    let locator = locator.trim();
    if locator.is_empty() {
        return None;
    }
    let project = locator
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(locator);
    Some(AgentSignature {
        version: None,
        model: "",
        os: None,
        machine: None,
        project,
        locator,
        speaker: &summary[..owner.len() + 3 + agent.len()],
        commit: None,
        item: None,
        agent_cli: None,
    })
}

/// The expander form starting at `body[at..]` (which begins with `<details`):
/// summary grammar plus the labeled bullets. Returns the signature and the
/// end offset (in `body`) of the closing `</details>`.
fn parse_expander_signature(body: &str, at: usize) -> Option<(AgentSignature<'_>, usize)> {
    let after = &body[at..];
    let sum_open = after.find("<summary>")? + "<summary>".len();
    let sum_close = sum_open + after[sum_open..].find("</summary>")?;
    let mut sig = parse_agent_summary(&after[sum_open..sum_close])?;
    let details_close = sum_close + after[sum_close..].find("</details>")?;
    for raw in after[sum_close + "</summary>".len()..details_close].lines() {
        let line = raw
            .trim()
            .trim_start_matches(|ch| matches!(ch, '-' | '*' | ' '));
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match label.trim().to_ascii_lowercase().as_str() {
            "bram" => {
                let v = value
                    .strip_prefix(['v', 'V'])
                    .filter(|r| r.starts_with(|c: char| c.is_ascii_digit()))
                    .unwrap_or(value);
                sig.version = Some(v);
            }
            "model" => sig.model = value,
            "os" => sig.os = Some(value),
            "machine" => sig.machine = Some(value),
            "commit" => {
                let c = value.strip_suffix("+dirty").unwrap_or(value).trim();
                sig.commit = Some(c).filter(|c| !c.is_empty());
            }
            "item" => sig.item = Some(value),
            "agent cli" => sig.agent_cli = Some(value),
            _ => {}
        }
    }
    if sig.model.is_empty() {
        return None;
    }
    Some((sig, at + details_close + "</details>".len()))
}

/// issue-484-details-summary-prs: the most text a details-summary body may
/// carry above its fold. Willison's opener is a sentence or two of the
/// human's own judgement; the cap keeps a whole agent-written description
/// from sitting unsigned above it.
const DETAILS_SUMMARY_OPENER_MAX: usize = 600;

/// forge-body-fold-enforced: a forge body longer than either limit, measured
/// after its signature line, must carry at least one `<details>` fold with a
/// `<summary>`. The rule (environment.md §Forge posts in details-summary
/// form) was missed on 2026-11-09 when it lived only in prose: the Windows
/// reply on #488 went up as ~70 expanded lines and was refolded by hand.
const FORGE_BODY_UNFOLDED_MAX_LINES: usize = 30;
const FORGE_BODY_UNFOLDED_MAX_CHARS: usize = 2_500;

/// The signature of `body` and its byte span. Forms, in order:
///
/// 1. The one-line form as the first non-empty line (span: that line).
/// 2. The expander form (forge-signature-as-expander): the first `<details>`
///    whose `<summary>` is `<owner>'s <Agent> speaking from <locator>`, first
///    in the body or under an opener of at most `DETAILS_SUMMARY_OPENER_MAX`
///    characters (span: the whole `<details>...</details>` block).
/// 3. The details-summary form (issue-484): the one-line form as the
///    `<summary>` of the first `<details>` block, same opener cap (span: the
///    summary text, so the block still counts as the post's fold).
///
/// None when no shape holds. Every consumer reads the signature here, so
/// there is one grammar.
pub(crate) fn parse_body_signature(
    body: &str,
) -> Option<(AgentSignature<'_>, std::ops::Range<usize>)> {
    let first = body.split('\n').find(|l| !l.trim().is_empty())?;
    if let Some(sig) = parse_agent_signature(first) {
        // `first` is a subslice of `body`, so its offset is exact.
        let start = first.as_ptr() as usize - body.as_ptr() as usize;
        return Some((sig, start..start + first.len()));
    }
    let mut from = 0usize;
    let mut nth = 0usize;
    while let Some(rel) = body[from..].find("<details") {
        let at = from + rel;
        if body[..at].trim().chars().count() > DETAILS_SUMMARY_OPENER_MAX {
            return None;
        }
        if let Some((sig, end)) = parse_expander_signature(body, at) {
            return Some((sig, at..end));
        }
        if nth == 0 {
            let after = &body[at..];
            let open = after.find("<summary>")? + "<summary>".len();
            let close = after[open..].find("</summary>")?;
            let raw = &after[open..open + close];
            let summary = raw.trim();
            if !summary.contains('\n') {
                if let Some(sig) = parse_agent_signature(summary) {
                    let start = at + open + (raw.len() - raw.trim_start().len());
                    return Some((sig, start..start + summary.len()));
                }
            }
        }
        nth += 1;
        from = at + "<details".len();
    }
    None
}

/// True iff the body is signed (see `parse_body_signature`) and the signature
/// names this checkout's normalized origin when one is discoverable.
fn body_is_signed(body: &str, cwd: &Path) -> bool {
    let expected = expected_repo_locator(cwd);
    let Some((sig, _)) = parse_body_signature(body) else {
        return false;
    };
    expected
        .as_deref()
        .map(|want| sig.locator.eq_ignore_ascii_case(want))
        .unwrap_or(true)
}

/// The `crossboundary-unsigned` deny text, shared by the Claude and Codex
/// deny sites so they cannot drift (issue-365). The abstract template alone
/// left a field agent guessing at its own repo's locator (`~/budget`, five
/// blind retries) even though `body_is_signed` had already computed the
/// exact expectation and discarded it — this appends that expectation (or,
/// for a genuinely remoteless checkout, states that any host/path-shaped
/// locator is accepted) so the deny teaches instead of only refusing.
fn crossboundary_unsigned_message(cwd: &Path) -> String {
    let expectation = match expected_repo_locator(cwd) {
        Some(want) => format!(
            "This checkout's origin is {want} — the signature's locator must be exactly that."
        ),
        None => {
            "No origin remote found here; any host/path-shaped locator is accepted.".to_string()
        }
    };
    format!(
        "This agent-authored forge artifact lacks the full repository-qualified signature.\nSign it with this expander, first in the body or right after a short opener:\n<details>\n<summary><owner>'s <Agent> speaking from <origin-host>/<path></summary>\n\n- Bram: v<version>\n- Model: <model>\n- Subagent: none (or the task name)\n- OS: <os>\n- Machine: <hostname -s>\n</details>\n{expectation}\nThe older one-line form is still accepted.\nSee conventions.md, 'Signing agent-authored forge artifacts' — every artifact, every comment, not just the first in a thread."
    )
}

fn crossboundary_signature_verdict(command: &str, cwd: &Path) -> (&'static str, &'static str) {
    if command.is_empty() {
        return ("skip", "no-command");
    }
    let masked = mask_heredoc_bodies(command); // #363
    let command = masked.as_str();
    if !is_forge_write(command) {
        return ("skip", "not-forge-write");
    }
    let (body, reason) = crossboundary_body(command, cwd);
    match body {
        None => ("unparsed", reason),
        Some(b) => {
            if body_is_signed(&b, cwd) {
                ("signed", reason)
            } else {
                ("unsigned", reason)
            }
        }
    }
}

/// guard-checks-the-signature-version: compare the version slot the signature
/// parser has been returning since 2026-09-07 against the build this guard was
/// compiled into. Nothing read it until now, so a signature whose version is
/// present and WRONG passed every existing check.
///
/// The value goes stale in exactly the sessions where it matters most.
/// `.claude/bram-conventions.md` carries the `<!-- bram vX.Y.Z -->` marker and is
/// `@`-imported at session start, so an agent signing from what it can already
/// see quotes the version that was true when it booted. The longer a session has
/// been working the likelier its slot is a lie, and long sessions are the ones
/// filing substantive issues — the failure selects for the reports whose triage
/// the slot exists to serve (#343, #362).
///
/// Measured, on the whole population of signed artifacts since the slot shipped
/// (49 of them, 4 days): one wrong. tau#1, signed `Bram 0.6.7` and filed
/// **10h26m after that project's own on-disk marker had been rewritten to
/// 0.6.8**. Both live sources were correct; the stale value was in context. It
/// was caught by a person reading the line, which is the only mechanism there
/// was.
///
/// Compared against the version the RUNNING instance recorded in its
/// `.bram-port.json` sidecar (a plain file read), falling back to
/// `env!("CARGO_PKG_VERSION")`, this guard's own build, when no instance has
/// recorded one. Not a loopback call to `/__app-info`: no port lookup, nothing
/// to fail while the route is busy.
///
/// The earlier premise was that `~/.bram/bram-guard` points at the binary built
/// beside the app, so the guard's own compiled version IS the running build's.
/// That breaks whenever the binary is rebuilt under a running instance: the
/// guard link picks up the new build at once while the app, and its
/// `/__app-info`, keeps the version it launched with. On 2026-11-10 the running
/// instance reported 0.8.5, the guard was rebuilt to 0.8.6, and the deny for
/// `Bram 0.8.5` told the agent to read `/__app-info`, which said 0.8.5. The
/// signature describes the instance, so the instance's version is the one to
/// compare against.
///
/// The slot stays OPTIONAL — `version: None` is allowed exactly as before. This
/// catches present-and-wrong, which is strictly narrower than requiring one, and
/// leaves the soft rollout and older threads alone.
fn signature_version_verdict(command: &str, cwd: &Path) -> (&'static str, String, ExpectedVersion) {
    // A skip never reaches a message, so it does no sidecar read: this runs on
    // every Bash call the guard sees.
    let skip = || {
        (
            "skip",
            String::new(),
            ExpectedVersion {
                version: String::new(),
                source: "guard build",
            },
        )
    };
    if command.is_empty() {
        return skip();
    }
    let masked = mask_heredoc_bodies(command); // #363, as the sibling checks do
    let command = masked.as_str();
    if !is_forge_write(command) {
        return skip();
    }
    let Some(body) = crossboundary_body(command, cwd).0 else {
        // Unreadable/stdin bodies are the unparsed case, already handled (and
        // denied) by the signature check ahead of this one.
        return skip();
    };
    // An unsigned body is denied ahead of this. The signature may be the
    // first line, an expander block, or a details-summary `<summary>`.
    let Some((sig, _)) = parse_body_signature(&body) else {
        return skip();
    };
    let expected = expected_signature_version(cwd);
    match sig.version {
        Some(v) if v != expected.version => ("mismatch", v.to_string(), expected),
        _ => ("ok", String::new(), expected),
    }
}

/// The version a signature is compared against, and where it came from.
struct ExpectedVersion {
    version: String,
    /// `"running instance"` (from `.bram-port.json`) or `"guard build"`.
    source: &'static str,
}

/// The running instance's recorded version, found by walking up from `cwd`
/// through each level's state-folder candidates for `.bram-port.json`; the
/// guard's compiled version when none is readable (older host, no instance).
fn expected_signature_version(cwd: &Path) -> ExpectedVersion {
    let mut cur = abspath_of(cwd);
    loop {
        for dir in state_dir_candidates(&cur) {
            let Ok(text) = std::fs::read_to_string(dir.join(".bram-port.json")) else {
                continue;
            };
            let version = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("version")?.as_str().map(str::to_string))
                .filter(|v| !v.is_empty());
            if let Some(version) = version {
                return ExpectedVersion {
                    version,
                    source: "running instance",
                };
            }
        }
        match cur.parent() {
            Some(parent) if parent != cur => cur = parent.to_path_buf(),
            _ => break,
        }
    }
    ExpectedVersion {
        version: env!("CARGO_PKG_VERSION").to_string(),
        source: "guard build",
    }
}

/// The stale-version deny text, shared by the Claude and Codex deny sites so
/// they cannot drift — the same discipline issue-365 imposed on the unsigned
/// message.
fn stale_signature_version_message(signed: &str, expected: &ExpectedVersion) -> String {
    let basis = if expected.source == "running instance" {
        format!("the running instance is {}", expected.version)
    } else {
        format!(
            "this guard's build is {} (no running-instance version recorded)",
            expected.version
        )
    };
    format!(
        "This forge write is signed \"Bram {}\" but {}.\n\
A wrong version is worse than an absent one: it looks like provenance and \
sends triage at the wrong build (judell/bram#343, #362).\n\
Usually the value came from context, the `<!-- bram vX.Y.Z -->` marker in the \
`@`-imported `.claude/bram-conventions.md` or memory, rather than from the live \
instance.\n\
Re-read `GET /__app-info` on the loopback port for the live version, fix the \
signature, and retry.",
        signed, basis
    )
}

/// True iff `body` contains a `<details>` block with a `<summary>` inside it.
fn body_has_fold(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let Some(at) = lower.find("<details") else {
        return false;
    };
    lower[at..].contains("<summary")
}

/// `(non-blank lines, chars)` of `body` with its signature span removed (see
/// `parse_body_signature`). An unsigned body, which the signature check has
/// already denied, is measured whole.
fn forge_body_extent(body: &str) -> (usize, usize) {
    let rest = body_without_signature(body);
    let lines = rest.lines().filter(|l| !l.trim().is_empty()).count();
    (lines, rest.trim().chars().count())
}

/// `body` with the signature span cut out. For the expander form that removes
/// the signature's own `<details>` block, so it is not mistaken for the
/// post's fold.
pub(crate) fn body_without_signature(body: &str) -> String {
    match parse_body_signature(body) {
        Some((_, span)) => format!("{}{}", &body[..span.start], &body[span.end..]),
        None => body.to_string(),
    }
}

/// forge-body-fold-enforced: `("unfolded", lines, chars)` when an inspectable
/// forge body is over `FORGE_BODY_UNFOLDED_MAX_LINES` non-blank lines or
/// `FORGE_BODY_UNFOLDED_MAX_CHARS` characters after its signature and has no
/// `<details>`/`<summary>` fold; `("ok", ..)` or `("skip", ..)` otherwise.
///
/// Reads the same body the signature check reads, so it covers exactly the
/// writes that check inspects, except gists and snippets: those carry file
/// content (code, logs), where a fold means nothing.
fn forge_body_fold_verdict(command: &str, cwd: &Path) -> (&'static str, usize, usize) {
    let skip = ("skip", 0, 0);
    if command.is_empty() {
        return skip;
    }
    let masked = mask_heredoc_bodies(command); // #363, as the sibling checks do
    let command = masked.as_str();
    if !is_forge_write(command) {
        return skip;
    }
    let c = chars(command);
    if cmd_verb(&c, &["gh"], Some(&["gist"]), &["create", "edit"])
        || cmd_verb(&c, &["glab"], Some(&["snippet"]), &["create", "update"])
    {
        return skip;
    }
    let lower = command.to_ascii_lowercase();
    if lower.contains("gh api") && lower.contains("gists/") {
        return skip;
    }
    let (body, reason) = crossboundary_body(command, cwd);
    let Some(body) = body else {
        return skip;
    };
    if reason == "positional-body-file" {
        return skip;
    }
    let (lines, chars) = forge_body_extent(&body);
    let long = lines > FORGE_BODY_UNFOLDED_MAX_LINES || chars > FORGE_BODY_UNFOLDED_MAX_CHARS;
    if long && !body_has_fold(&body_without_signature(&body)) {
        ("unfolded", lines, chars)
    } else {
        ("ok", lines, chars)
    }
}

/// The `long-forge-body-unfolded` deny text, shared by the Claude and Codex
/// deny sites so they cannot drift.
fn long_forge_body_unfolded_message(lines: usize, chars: usize) -> String {
    format!(
        "This forge post is long and has no fold: {lines} non-blank lines and {chars} \
characters after the signature (limits: {FORGE_BODY_UNFOLDED_MAX_LINES} lines, \
{FORGE_BODY_UNFOLDED_MAX_CHARS} characters).\n\
A long agent-written post keeps what a person reads visible and folds the evidence. \
Use the outline:\n\
  - the signature block (the expander)\n\
  - one bold sentence: **What this post is:** …, with its key numbers\n\
  - bullets, each leading with a bold claim\n\
  - a `Data:` line naming the route or command, with the Bram version\n\
  - one numbered fold per topic: <details><summary><b>1. …</b></summary> … </details>\n\
See `.claude/bram-reference/environment.md`, §Forge posts in details-summary form. \
Refold the body file and retry."
    )
}

/// `\b[0-9a-f]{40}\b`
fn full_shas(body: &str) -> Vec<String> {
    let c = chars(body);
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        if !word_start(&c, i) {
            i += 1;
            continue;
        }
        let mut e = i;
        while e < c.len() && (c[e].is_ascii_digit() || matches!(c[e], 'a'..='f')) {
            e += 1;
        }
        if e - i == 40 && word_end(&c, e) {
            out.push(c[i..e].iter().collect());
        }
        i = if e > i { e } else { i + 1 };
    }
    out.sort();
    out.dedup();
    out
}

/// issue-277 B / #391. Check full SHAs locally and against remote-tracking
/// refs. This is local evidence of a push, not a network/URL reachability
/// check: any remote counts. Repos with no tracking refs skip the push check;
/// unavailable git and failed remote-ref probes fail open.
fn forge_sha_verdict(command: &str, cwd: &Path) -> (&'static str, String) {
    forge_sha_verdict_with_git(command, cwd, Path::new("git"))
}

// An explicit executable lets tests exercise unavailable/failed git without
// mutating the process-wide PATH while other guard tests run in parallel.
fn forge_sha_verdict_with_git(command: &str, cwd: &Path, git: &Path) -> (&'static str, String) {
    if command.is_empty() {
        return ("skip", "no-command".into());
    }
    let masked = mask_heredoc_bodies(command); // #363
    let command = masked.as_str();
    if !is_forge_write(command) {
        return ("skip", "not-forge-write".into());
    }
    let (body, reason) = crossboundary_body(command, cwd);
    let Some(body) = body else {
        return ("unparsed", reason.into());
    };
    let shas = full_shas(&body);
    // The expander's optional `Commit:` bullet must resolve locally too.
    // It is usually a short sha, so it gets the resolve probe only, not the
    // pushed-to-a-remote check below.
    let sig_commit = parse_body_signature(&body).and_then(|(s, _)| s.commit.map(str::to_string));
    if shas.is_empty() && sig_commit.is_none() {
        return ("ok", "no-full-shas".into());
    }
    if let Some(c) = &sig_commit {
        if c.len() < 4 || c.len() > 40 || !c.bytes().all(|b| b.is_ascii_hexdigit()) {
            return ("bad", c.clone());
        }
    }
    for sha in shas.iter().chain(sig_commit.iter()) {
        let probe = guard_git_command(git)
            .arg("cat-file")
            .arg("-e")
            .arg(format!("{}^{{commit}}", sha))
            .current_dir(cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match probe {
            Err(_) => return ("unparsed", "git-unavailable".into()),
            Ok(st) => {
                if !st.success() {
                    return ("bad", sha.clone());
                }
            }
        }
    }
    let refs = guard_git_command(git)
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "--count=1",
            "refs/remotes",
        ])
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output();
    let has_remote_refs = match refs {
        Err(_) => return ("unparsed", "git-unavailable".into()),
        Ok(output) if !output.status.success() => {
            return ("unparsed", "git-remote-refs-error".into());
        }
        Ok(output) => !output.stdout.iter().all(u8::is_ascii_whitespace),
    };
    if has_remote_refs {
        for sha in &shas {
            let contains = guard_git_command(git)
                .args([
                    "for-each-ref",
                    "--format=%(refname)",
                    "--count=1",
                    "--contains",
                    sha,
                    "refs/remotes",
                ])
                .current_dir(cwd)
                .stderr(std::process::Stdio::null())
                .output();
            match contains {
                Err(_) => return ("unparsed", "git-unavailable".into()),
                Ok(output) if !output.status.success() => {
                    return ("unparsed", "git-remote-contains-error".into());
                }
                Ok(output) if output.stdout.iter().all(u8::is_ascii_whitespace) => {
                    return ("unpushed", sha.clone());
                }
                Ok(_) => {}
            }
        }
    }
    ("ok", format!("verified:{}", shas.len()))
}

fn forge_sha_unpushed_message(sha: &str) -> String {
    format!(
        "The body contains a full commit SHA that is not contained in any remote-tracking ref:\n    {}\n\
A local-only commit link can 404 for readers. Push the commit first, then confirm \
`git branch -r --contains <sha>` lists a branch; or cite only the short SHA in \
backticks, without a commit link. This check uses local tracking refs and accepts \
any remote; it does not verify the link's destination. If you expect the commit \
to be pushed already, run `git fetch` and retry to refresh those refs. See judell/bram#391.",
        sha
    )
}

// --- subagent gates ----------------------------------------------------------

fn subagent_lifecycle_verdict(
    command: &str,
    agent_id: &str,
) -> (&'static str, Option<&'static str>) {
    if !WORKLIST_LIFECYCLE_ROUTES
        .iter()
        .any(|r| command.contains(r))
    {
        return ("skip", None);
    }
    if !agent_id.trim().is_empty() {
        return ("deny", None);
    }
    ("allow", Some("no-agent-id"))
}

/// judell/bram#287. `rel` is None when the target is outside the project tree.
fn subagent_worklist_write_verdict(
    root: &Path,
    rel: Option<&str>,
    agent_id: &str,
) -> (&'static str, Option<&'static str>) {
    let is_worklist_surface = match rel.and_then(|r| state_leaf(root, r)) {
        Some(leaf) => leaf == WORKLIST_LEAF || leaf.starts_with(WORKLIST_DRAFTS_LEAF),
        None => false,
    };
    if is_worklist_surface {
        if !agent_id.trim().is_empty() {
            return ("deny", None);
        }
        return ("skip", Some("no-agent-id"));
    }
    ("skip", None)
}

// --- MCP surface -------------------------------------------------------------

fn mcp_is_mutation(tool_name: &str) -> bool {
    let name = tool_name.to_lowercase();
    MCP_WRITE_TOKENS.iter().any(|t| name.contains(t))
}

/// Recursively collect every string value in `value` -- any key, any depth,
/// including array elements -- for `mcp_candidate_paths` below. Depth-first,
/// so callers that care about order see first-seen order.
fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => {
            for it in items {
                collect_strings(it, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_strings(v, out);
            }
        }
        _ => {}
    }
}

/// judell/bram#384: the structural replacement for `MCP_PATH_KEYS`. The old
/// extraction asked "does a known key hold a string at the top level of
/// tool_input" -- an enumeration standing in for a property, wrong in both
/// directions (false positives on connectors with no path key at all; false
/// negatives on a real filesystem tool whose path sits under an unlisted key
/// or nested inside an object). The question that actually matters is
/// structural and needs no list:
///
///   Can this call name a path inside the project root?
///
/// So this walks every string value in `tool_input`, any key, any depth, and
/// resolves each one as a whole path via `normalize`, the caller's own
/// provider-specific join+abspath pair (`normalize_target` for Claude,
/// `codex_normalize_target` for Codex -- see their call sites). A string
/// becomes a *candidate* -- return value is its project-relative path -- only
/// when all of:
///
/// - it contains no whitespace. A sentence like "see src/main.rs for
///   details" is one string with a path-shaped substring in the middle, not
///   a path; resolving the WHOLE string (never a substring) as one path is
///   what keeps prose out, and prose is essentially always multi-word.
/// - `normalize` places it inside the project root (its provider-specific
///   join already refuses anything outside root, by returning `None`).
/// - it exists on disk, OR it is *creatable*: its relative form contains a
///   `/` and its resolved parent directory exists. The parent leg is what
///   admits a legitimate file creation, whose target does not exist yet.
///
/// The `/`-in-the-relative-path requirement on the creatable leg is a
/// deliberate narrowing of the literal "exists or has an existing parent"
/// rule, added because that literal rule is otherwise nearly vacuous: a
/// bare, slash-free token's filesystem parent is always the call's own cwd,
/// which trivially exists -- so without this, ANY short single-word string
/// with no whitespace (an email address, a calendar summary, an opaque
/// connector id, a version string) would qualify as a "creatable" candidate
/// purely because it does not yet exist as a file in the cwd. That would
/// reintroduce false positives for exactly the connector calls this fix
/// exists to stop denying (verified against the required Gmail / Calendar /
/// Drive cases -- see the guard_policy_tests module and the item's synth
/// receipts). An EXISTING single-segment name (`"path": "README.md"`) is
/// still recognized on the `exists` leg, since existence is real evidence
/// about the repository, not an artifact of how many components the string
/// happens to have.
///
/// Known residual, named rather than hidden (see `mcp_branch` /
/// `codex_mcp_branch` for where the two consequences of this function
/// returning empty are handled): a single top-level bare filename with no
/// existing sibling of that name (`"NEWFILE.md"` dropped directly in the
/// project root, never seen before) is not recognized as a creatable
/// candidate under this narrowing, though it would be under the literal
/// draft rule. And a short token that coincidentally matches something
/// that already exists in the repo (an event summary that happens to be
/// spelled "LICENSE") still resolves as a candidate via the `exists` leg --
/// rare, but not impossible, and worth knowing about when reading a trace.
fn mcp_candidate_paths(
    tool_input: &Value,
    root: &Path,
    normalize: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut raw = Vec::new();
    collect_strings(tool_input, &mut raw);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in raw {
        if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
            continue;
        }
        let Some(rel) = normalize(&s) else { continue };
        if rel.is_empty() {
            continue;
        }
        let abs = root.join(&rel);
        let exists = abs.exists();
        let creatable = rel.contains('/') && abs.parent().is_some_and(|p| p.exists());
        if !exists && !creatable {
            continue;
        }
        if seen.insert(rel.clone()) {
            out.push(rel);
        }
    }
    out
}

/// judell/bram#384's one accepted loosening, made observable rather than
/// silent. `mcp_candidate_paths` returning empty for a write-verb-named tool
/// now means ALLOW (out of scope) instead of the old fail-closed deny -- and
/// the near-total majority of such calls are external connectors that
/// cannot touch a repo file regardless (Gmail, Calendar, Drive: see the
/// item's synth receipts). The one case this loosens for real is a tool
/// that DOES write a repo file but whose target is ambient or
/// session-remembered rather than present anywhere in this call's own
/// input -- ordinary per-path coverage cannot see it because there is no
/// path to check. This breadcrumb is that residue's tripwire: per the
/// observe-first discipline in docs/developing-bram.md, a fire here is the
/// evidence a server of that shape exists, recorded before it can do any
/// damage. Zero fires is the expected, healthy reading, not a sign the
/// instrument is dead -- see "Distinguish soak observers from tripwires" in
/// that doc.
fn trace_mcp_write_verb_no_candidates(project_root: &Path, provider: &str, tool: &str) {
    crate::guard::append_breadcrumb(
        project_root,
        provider,
        "mcp-write-verb-no-candidate-paths",
        tool,
        "reason=write-verb-tool-naming-no-repo-path",
    );
}

// --- path helpers ------------------------------------------------------------

fn normalize_components(p: &Path) -> PathBuf {
    use std::path::Component::*;
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Prefix(_) | RootDir => out.push(comp.as_os_str()),
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            Normal(s) => out.push(s),
        }
    }
    out
}

/// Lexical `os.path.abspath`: no filesystem access, relative paths resolved
/// against the process cwd exactly as the Python guard's does.
fn abspath(p: &str) -> PathBuf {
    let pb = Path::new(p);
    let joined = if pb.is_absolute() {
        pb.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(pb)
    };
    normalize_components(&joined)
}

fn abspath_of(p: &Path) -> PathBuf {
    abspath(&p.to_string_lossy())
}

fn find_project_root(start: &str) -> Option<PathBuf> {
    let mut cur = abspath(start);
    loop {
        // issue-469: Bram's `.bram/` at each level, so a nested instance's
        // marker is found before its parent's.
        if state_dir_candidates(&cur)
            .iter()
            .any(|d| d.join(AUTH_LEAF).exists())
        {
            return Some(cur);
        }
        match cur.parent() {
            Some(parent) if parent != cur => cur = parent.to_path_buf(),
            _ => return None,
        }
    }
}

fn normalize_target(project_root: &Path, target: &str) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let abs_target = abspath(target);
    let abs_root = abspath_of(project_root);
    if abs_target == abs_root {
        return Some(String::new());
    }
    let root_s = abs_root.to_string_lossy().into_owned();
    let sep = std::path::MAIN_SEPARATOR;
    let prefix = format!("{}{}", root_s, sep);
    let target_s = abs_target.to_string_lossy().into_owned();
    if target_s.starts_with(&prefix) {
        return Some(target_s[prefix.len()..].replace(sep, "/"));
    }
    None
}

/// Map a managed Claude worktree target to its real-tree path for item
/// coverage only (#309). Lifecycle classification, bypass lookup, traces, and
/// deny targets continue to use the original normalized path.
fn worktree_coverage_target(rel: &str) -> (&str, Option<&str>) {
    let Some(tail) = rel.strip_prefix(WORKTREE_PREFIX) else {
        return (rel, None);
    };
    let Some((worktree, mapped)) = tail.split_once('/') else {
        return (rel, None);
    };
    if worktree.is_empty() || mapped.is_empty() {
        return (rel, None);
    }
    (mapped, Some(worktree))
}

/// Whether `rel` is covered by a declared file/directory entry, plus the
/// managed worktree name when one was mapped. Directory matching mirrors the
/// commit gate's separator-anchored expansion from #295 and happens after the
/// #309 worktree mapping.
fn coverage_verdict<'a>(covered: &HashSet<String>, rel: &'a str) -> (bool, Option<&'a str>) {
    let (mapped, worktree) = worktree_coverage_target(rel);
    let is_covered = covered.iter().any(|declared| {
        let entry = declared.trim_end_matches('/');
        !entry.is_empty()
            && (mapped == entry
                || mapped
                    .strip_prefix(entry)
                    .map_or(false, |rest| rest.starts_with('/')))
    });
    (is_covered, worktree)
}

fn with_worktree_marker(mut verdict: ShadowVerdict, worktree: Option<&str>) -> ShadowVerdict {
    if let Some(name) = worktree {
        verdict.reason.push_str(":worktree=");
        verdict.reason.push_str(name);
    }
    verdict
}

fn is_lifecycle_path(root: &Path, rel: &str) -> bool {
    let Some(leaf) = state_leaf(root, rel) else {
        return false;
    };
    LIFECYCLE_LEAVES_EXACT.contains(&leaf)
        || LIFECYCLE_LEAVES_PREFIXES
            .iter()
            .any(|p| leaf.starts_with(p))
}

fn is_worklist_draft(root: &Path, rel: &str) -> bool {
    let Some(leaf) = state_leaf(root, rel) else {
        return false;
    };
    leaf.starts_with(WORKLIST_DRAFTS_LEAF)
        && leaf.ends_with(".md")
        && !leaf[WORKLIST_DRAFTS_LEAF.len()..].contains('/')
}

// --- worklist.json readers ---------------------------------------------------

/// Mirrors `items_by_id`: a single malformed item collapses the whole map to
/// empty, because the Python comprehension raises and the except swallows it.
fn items_by_id(text: &str) -> Option<Vec<(String, Value)>> {
    let doc: Value = serde_json::from_str(text).ok()?;
    let obj = doc.as_object()?;
    let items = match obj.get("items") {
        None => return Some(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return None,
    };
    let mut out = Vec::new();
    for it in items {
        let o = it.as_object()?;
        let id = o.get("id")?;
        let key = match id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out.push((key, it.clone()));
    }
    Some(out)
}

fn items_map(text: &str) -> Vec<(String, Value)> {
    items_by_id(text).unwrap_or_default()
}

fn item_status(item: &Value) -> String {
    item.get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("proposed")
        .to_string()
}

type StateChanges = (Vec<(String, String)>, Vec<(String, String, String)>);

fn worklist_state_changes(
    old_items: &[(String, Value)],
    new_items: &[(String, Value)],
) -> StateChanges {
    let mut removed = Vec::new();
    let mut status_changed = Vec::new();
    for (id, old_item) in old_items {
        match new_items.iter().find(|(nid, _)| nid == id) {
            None => removed.push((id.clone(), item_status(old_item))),
            Some((_, new_item)) => {
                let (o, n) = (item_status(old_item), item_status(new_item));
                if o != n {
                    status_changed.push((id.clone(), o, n));
                }
            }
        }
    }
    (removed, status_changed)
}

fn worklist_items_with_inline_prose(content: &str) -> Vec<String> {
    let Ok(doc) = serde_json::from_str::<Value>(content) else {
        return Vec::new();
    };
    let Some(items) = doc.get("items").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut bad = Vec::new();
    for it in items {
        let Some(o) = it.as_object() else { continue };
        if o.get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("proposed")
            != "proposed"
        {
            continue;
        }
        let nonempty = |k: &str| {
            o.get(k)
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        };
        if nonempty("before") || nonempty("after") {
            let label = o
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("<no-id>");
            bad.push(label.to_string());
        }
    }
    bad
}

fn worklist_version_from_text(text: &str) -> (bool, i64) {
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return (false, 0);
    };
    if !doc.is_object() {
        return (false, 0);
    }
    match doc.get("version") {
        Some(Value::Number(n)) if n.is_i64() => (true, n.as_i64().unwrap_or(0)),
        _ => (false, 0),
    }
}

fn worklist_covered_files(project_root: &Path) -> HashSet<String> {
    worklist_covered_files_filtered(project_root, None)
}

// issue-368-addressed-turn-context: the same walk, optionally restricted to
// named item ids — the would-deny observer asks whether a path is covered by
// the items the ACTIVE TURN is addressed to, not by the whole board.
fn worklist_covered_files_filtered(
    project_root: &Path,
    only_ids: Option<&HashSet<String>>,
) -> HashSet<String> {
    let mut covered = HashSet::new();
    let Ok(text) = std::fs::read_to_string(state_path(project_root, WORKLIST_LEAF)) else {
        return covered;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return covered;
    };
    let Some(items) = doc.get("items").and_then(|v| v.as_array()) else {
        return covered;
    };
    for it in items {
        let Some(o) = it.as_object() else { continue };
        let st = o
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("proposed");
        if st != "proposed" && st != "applied" {
            continue;
        }
        if let Some(only) = only_ids {
            let id = o.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !only.contains(id) {
                continue;
            }
        }
        if let Some(f) = o.get("file").and_then(|v| v.as_str()) {
            covered.insert(f.to_string());
        }
        if let Some(fs) = o.get("files").and_then(|v| v.as_array()) {
            for p in fs {
                if let Some(s) = p.as_str() {
                    covered.insert(s.to_string());
                }
            }
        }
    }
    covered
}

// --- issue-368-addressed-turn-context (observe-only) -------------------------
// The host stamps `.bram/.turn-context.json` while an ADDRESSED turn
// (approved:/iterate:/drop: payload naming ids) is in flight, and removes it
// at turn end or for general turns. When a file-mutating call is allowed
// purely by worklist coverage but the active turn does not address a covering
// item, this emits a `would-deny` breadcrumb into hook-events.log and allows
// unchanged — the soak evidence for the #368 policy question (should general
// turns be denied item-file mutation?). ctx=none cannot distinguish a
// terminal-typed general turn from a pre-368 host, so the vocabulary reads it
// as the weaker signal; ctx=addressed fires only when the addressed items'
// own declared files do not cover the target.
const TURN_CONTEXT_LEAF: &str = ".turn-context.json";

fn turn_context_addressed_ids(project_root: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(state_path(project_root, TURN_CONTEXT_LEAF)).ok()?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    Some(
        doc.get("addressedIds")?
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect(),
    )
}

// addressed-turn-edits-stay-in-item-files: after a file tool edits a path the
// items this turn is addressed to don't declare, tell the AGENT (the #368
// breadcrumb only reaches hook-events.log). Lines written in an addressed turn
// are credited to the addressed items, so a misrouted edit gets committed
// under the wrong id or left behind (2026-09-27: a423886 / 6b22c82). Returned
// as PostToolUse additionalContext, which the hooks docs say reaches the
// model; allow-time reasons aren't documented to. Advisory only: nothing is
// blocked. None for general turns, lifecycle and draft paths, and covered
// targets. Claude reads the target from `file_path`; Codex's apply_patch
// carries a patch, whose targets `patch_targets` extracts. Codex also
// documents PostToolUse `hookSpecificOutput.additionalContext` as "added as
// extra developer context" (https://learn.chatgpt.com/docs/hooks).
pub(crate) fn addressed_turn_advisory(payload: &Value) -> Option<String> {
    let ti = payload.get("tool_input")?;
    let target = ti
        .get("file_path")
        .or_else(|| ti.get("notebook_path"))?
        .as_str()?;
    let start = Path::new(target)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let project_root = find_project_root(&start)?;
    let rel = normalize_target(&project_root, target)?;
    addressed_advisory_for(&project_root, "claude-rs", &[rel])
}

pub(crate) fn codex_addressed_turn_advisory(payload: &Value) -> Option<String> {
    let ti = payload.get("tool_input")?;
    let cwd = payload.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
    let project_root = find_project_root(cwd)?;
    let rels: Vec<String> = patch_targets(ti)
        .iter()
        .filter_map(|t| codex_normalize_target(&project_root, t))
        .collect();
    addressed_advisory_for(&project_root, "codex-rs", &rels)
}

// opencode-edit-advisory: an OpenCode edit as `opencode_policy_payload`
// (guard.rs) translates it, `edit` / `write` to Claude's Edit / Write and
// `patch` to Codex's apply_patch, so each takes its own target reading. The
// plugin sends the project root as `cwd`.
pub(crate) fn opencode_addressed_turn_advisory(payload: &Value) -> Option<String> {
    let ti = payload.get("tool_input")?;
    let project_root = find_project_root(payload.get("cwd")?.as_str()?)?;
    let rels: Vec<String> = match str_field(payload, "tool_name").as_str() {
        "apply_patch" => patch_targets(ti)
            .iter()
            .filter_map(|t| codex_normalize_target(&project_root, t))
            .collect(),
        "Edit" | "Write" => ti
            .get("file_path")
            .and_then(|p| p.as_str())
            .and_then(|p| normalize_target(&project_root, p))
            .into_iter()
            .collect(),
        _ => return None,
    };
    addressed_advisory_for(&project_root, "opencode-rs", &rels)
}

// The shared check: which of these targets the addressed items don't cover.
fn addressed_advisory_for(project_root: &Path, provider: &str, rels: &[String]) -> Option<String> {
    let ids = turn_context_addressed_ids(project_root)?;
    if ids.is_empty() {
        return None;
    }
    let set: HashSet<String> = ids.iter().cloned().collect();
    let addressed_cov = worklist_covered_files_filtered(project_root, Some(&set));
    let stray: Vec<&String> = rels
        .iter()
        .filter(|rel| {
            !is_lifecycle_path(project_root, rel) && !is_worklist_draft(project_root, rel)
        })
        .filter(|rel| !coverage_verdict(&addressed_cov, rel).0)
        .collect();
    if stray.is_empty() {
        return None;
    }
    let paths: Vec<String> = stray.iter().map(|r| format!("`{}`", r)).collect();
    crate::guard::append_breadcrumb(
        project_root,
        provider,
        "addressed-turn-advisory",
        "-",
        &format!(
            "target={} addressed={}",
            stray
                .iter()
                .map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(","),
            ids.join(",")
        ),
    );
    Some(format!(
        "Bram: {} {} in the files of the item(s) this turn is addressed to ({}). \
         Lines written in this turn are credited to them, so this change will be \
         committed under their id or left behind when the right item commits. If it \
         belongs to another item, say so and route it there (or add the file to this \
         item's `files`) before editing further.",
        paths.join(", "),
        if paths.len() == 1 { "isn't" } else { "aren't" },
        ids.join(", ")
    ))
}

// would-deny-keys-on-authorization: the durable grant the eventual rule
// should key on. Mirrors the host's own liveness test
// (worklist_active_authorization_summary): an `approved` record that is
// unconsumed, uninterrupted, and inside the TTL. Read fresh each call — the
// guard is a short-lived process and the record changes under it.
fn live_authorization_ids(project_root: &Path) -> Option<HashSet<String>> {
    let text = std::fs::read_to_string(state_path(project_root, AUTH_LEAF)).ok()?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    if doc.get("kind").and_then(|v| v.as_str()) != Some("approved") {
        return None;
    }
    if doc.get("consumedAtMs").and_then(|v| v.as_f64()).is_some() {
        return None;
    }
    if doc
        .get("interruptedAtMs")
        .and_then(|v| v.as_f64())
        .is_some()
    {
        return None;
    }
    let issued = doc
        .get("issuedAtMs")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if issued <= 0.0 || now_ms() - issued > crate::WORKLIST_AUTH_TTL_MS as f64 {
        return None;
    }
    let ids: HashSet<String> = doc
        .get("ids")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    (!ids.is_empty()).then_some(ids)
}

fn trace_would_deny_unaddressed(project_root: &Path, provider: &str, tool: &str, rel: &str) {
    // A live grant is an authorized general-turn edit, not the observed class.
    let bypassed = if provider.starts_with("codex") {
        codex_fresh_bypass(project_root, rel)
    } else {
        fresh_bypass(project_root, rel)
    };
    if bypassed {
        return;
    }
    // would-deny-keys-on-authorization: the decision input is AUTHORIZATION,
    // not turn addressing. Addressing was only ever a proxy for "the user
    // asked for this"; the authorization is the actual grant, and it is what
    // #368's rule should refuse against. Keying on addressing made this fire
    // on two ordinary states — a legitimate resume (since the claim now ends
    // with the turn, a multi-turn apply's later turns are general turns) and
    // a "just do it" edit whose 1h grant had aged out. 48 fires in one
    // morning in tau-extractor-sandbox, several of them Jon's own deliberate
    // direct commits: inflated by construction, and a soak that cannot be
    // read is worse than none because it looks like evidence.
    if let Some(auth_ids) = live_authorization_ids(project_root) {
        let auth_cov = worklist_covered_files_filtered(project_root, Some(&auth_ids));
        let (covered_by_auth, _) = coverage_verdict(&auth_cov, rel);
        if covered_by_auth {
            return;
        }
    }
    let ctx = match turn_context_addressed_ids(project_root) {
        None => "none",
        Some(ids) => {
            let set: HashSet<String> = ids.into_iter().collect();
            if set.is_empty() {
                "empty"
            } else {
                let addressed_cov = worklist_covered_files_filtered(project_root, Some(&set));
                let (ok, _) = coverage_verdict(&addressed_cov, rel);
                if ok {
                    return;
                }
                "addressed"
            }
        }
    };
    crate::guard::append_breadcrumb(
        project_root,
        provider,
        "would-deny",
        tool,
        // `auth=` is the decision input; `ctx=` stays for diagnosis so the two
        // suppression reasons remain distinguishable when reading a soak.
        &format!(
            "target={} auth=none ctx={} reason={}",
            rel,
            ctx,
            would_deny_reason(project_root, rel)
        ),
    );
}

// guard-coverage-requires-begun-item: split the soak by WHAT covered the
// path. `general-turn-item-edit` is #368's class: a begun item (applied, or
// `begunAtMs` stamped by the host at its first approval) covers it.
// `proposed-only-coverage` is the narrower, harmful class: only items nobody
// has started cover it (2026-10-09 06:09:03Z: app/__shell/helpers.js, named
// only by the unstarted issue-425-retire-replay, edited in a chat turn).
// Observe-only; the eventual rule keys on this reason.
fn would_deny_reason(project_root: &Path, rel: &str) -> &'static str {
    let begun = begun_item_ids(project_root);
    let begun_cov = worklist_covered_files_filtered(project_root, Some(&begun));
    if coverage_verdict(&begun_cov, rel).0 {
        "general-turn-item-edit"
    } else {
        "proposed-only-coverage"
    }
}

/// Items that have begun: `applied`, or `proposed` with the host-written
/// `begunAtMs` (conventions: "the durable answer to 'has work on this item
/// begun?'").
fn begun_item_ids(project_root: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(text) = std::fs::read_to_string(state_path(project_root, WORKLIST_LEAF)) else {
        return ids;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return ids;
    };
    for it in doc
        .get("items")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(o) = it.as_object() else { continue };
        let applied = o.get("status").and_then(|v| v.as_str()) == Some("applied");
        let begun = o.get("begunAtMs").and_then(|v| v.as_f64()).is_some();
        if applied || begun {
            if let Some(id) = o.get("id").and_then(|v| v.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

/// issue-387: refines a Bash branch's coarse "some begun item covers
/// something, so allow" leg into per-target observation -- the same
/// refinement `trace_would_deny_unaddressed` already gives the file-tool and
/// MCP branches. Call this ONLY from the branch that is about to allow for
/// that coarse reason; the allow itself never changes.
///
/// `normalize` resolves one extracted (still request-relative) token against
/// the command's actual working directory into a project-relative path, or
/// `None` when the token is outside the project -- callers pass their own
/// provider's join+normalize pair (`normalize_target` needs a manual cwd
/// join; `codex_normalize_target` does its own).
fn trace_bash_would_deny(
    project_root: &Path,
    provider: &str,
    command: &str,
    covered: &HashSet<String>,
    normalize: impl Fn(&str) -> Option<String>,
) {
    match bash_write_targets(command) {
        BashWriteTarget::Unknown => {
            // Distinguishable from `would-deny`: this says "the guard could
            // not tell", not "the guard would have denied". Without it, a
            // soak of this signal cannot tell "no fires" from "could not
            // look" -- the same denominator problem `mask_heredoc_bodies`'s
            // neighbourhood and the census's damaged-archive counting both
            // already take seriously.
            crate::guard::append_breadcrumb(
                project_root,
                provider,
                "would-deny-unknown-target",
                "Bash",
                "reason=bash-target-unextractable",
            );
        }
        BashWriteTarget::Paths(paths) => {
            for raw in paths {
                let Some(rel) = normalize(&raw) else {
                    continue;
                };
                if is_lifecycle_path(project_root, &rel) {
                    continue;
                }
                let (is_covered, _) = coverage_verdict(covered, &rel);
                if is_covered {
                    // would-deny-keys-on-authorization: this delegates to the
                    // SAME function the file-tool and MCP branches call, so
                    // the decision input stays authorization, not turn
                    // addressing, exactly as `b64d81f` corrected it.
                    trace_would_deny_unaddressed(project_root, provider, "Bash", &rel);
                }
            }
        }
    }
}

// --- authorization record ----------------------------------------------------

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn num_field(rec: &serde_json::Map<String, Value>, keys: &[&str]) -> f64 {
    for k in keys {
        if let Some(v) = rec.get(*k).and_then(|v| v.as_f64()) {
            if v != 0.0 {
                return v;
            }
        }
    }
    0.0
}

// issue-352: direct-edit grants moved to their own sidecar so a gate click
// (which replaces the single-slot shared authorization file) can no longer
// destroy a live grant mid-TTL — the Budget-project incident: a Drop click
// clobbered a 20s-old grant and the just-authorized edit was denied. The
// sidecar is authoritative; the legacy shared-file location stays honored
// for one release so a grant written by an old binary survives the upgrade
// mid-window.
const DIRECT_EDIT_LEAF: &str = ".worklist-direct-edit.json";

fn bypass_lookup_detail(project_root: &Path, path_rel: &str) -> (bool, &'static str) {
    let primary = bypass_lookup_in(&state_path(project_root, DIRECT_EDIT_LEAF), path_rel);
    if primary.0 {
        return primary;
    }
    let legacy = bypass_lookup_in(&state_path(project_root, AUTH_LEAF), path_rel);
    if legacy.0 {
        return legacy;
    }
    // Neither matched: report the more informative failure for the deny
    // message (a stale/path-not-covered beats a bare absent).
    if primary.1 != "absent" {
        primary
    } else {
        legacy
    }
}

fn bypass_lookup_in(file: &Path, path_rel: &str) -> (bool, &'static str) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return (false, "absent");
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return (false, "absent");
    };
    let Some(rec) = doc.as_object() else {
        return (false, "wrong-kind");
    };
    if rec.get("kind").and_then(|v| v.as_str()) != Some("direct-edit") {
        return (false, "wrong-kind");
    }
    let issued = num_field(rec, &["issuedAtMs", "issued_at_ms"]);
    if now_ms() - issued > BYPASS_TTL_MS {
        return (false, "stale");
    }
    let covered = rec
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == path_rel || s == "*")
        })
        .unwrap_or(false);
    if covered {
        (true, "ok")
    } else {
        (false, "path-not-covered")
    }
}

fn fresh_bypass(project_root: &Path, path_rel: &str) -> bool {
    bypass_lookup_detail(project_root, path_rel).0
}

type CrossRootVerdict = (
    bool,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
);

/// issue-262: consult the target project's bypass record and, only when it
/// differs, the session (cwd) project's.
fn cross_root_bypass_verdict(
    target_root: &Path,
    path_rel: &str,
    session_root: Option<&Path>,
) -> CrossRootVerdict {
    let (matched, target_detail) = bypass_lookup_detail(target_root, path_rel);
    if matched {
        return (true, Some("target"), target_detail, None);
    }
    let Some(session_root) = session_root else {
        return (false, None, target_detail, None);
    };
    if abspath_of(session_root) == abspath_of(target_root) {
        return (false, None, target_detail, None);
    }
    let (session_matched, session_detail) = bypass_lookup_detail(session_root, path_rel);
    if session_matched {
        (true, Some("session"), target_detail, Some(session_detail))
    } else {
        (false, None, target_detail, Some(session_detail))
    }
}

fn post_commit_push_grace(project_root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(state_path(project_root, AUTH_LEAF)) else {
        return false;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let Some(rec) = doc.as_object() else {
        return false;
    };
    if rec.get("kind").and_then(|v| v.as_str()) != Some("approved") {
        return false;
    }
    let consumed = num_field(rec, &["consumedAtMs", "consumed_at_ms"]);
    if consumed == 0.0 {
        return false;
    }
    now_ms() - consumed <= POST_COMMIT_PUSH_GRACE_MS
}

// --- transcript / opt-out ----------------------------------------------------

fn last_user_text(transcript_path: &str) -> String {
    if transcript_path.is_empty() {
        return String::new();
    }
    // ci-retire-python-hook-checks: read LOSSILY. `read_to_string` errors on
    // invalid UTF-8, and this is untrusted input read only for opt-out phrase
    // matching — so one undecodable byte anywhere in a transcript used to
    // yield empty text, silently disabling "just do it" and denying the edit.
    // The Python guard opened it with errors="replace" for exactly this, and
    // its test_undecodable_transcript_bytes pinned it ("even a corrupt
    // transcript must not take the guard down"). #249 in mirror image: Python
    // crashed loudly, this failed quiet.
    let Ok(bytes) = std::fs::read(transcript_path) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    // The Python guard scans forward, parsing EVERY line to keep only the
    // last non-empty user text; on a 30 MB session that is ~10k JSON parses
    // per guard invocation, and at debug opt-level (the shipping profile)
    // the authority-mode deny measured ~3x slower than the Python guard on
    // the identical payload (1.16 s vs 0.40 s, 2026-08-29). Scanning in
    // REVERSE parses a handful of tail lines instead: the first qualifying
    // record from the end is the forward scan's last, by construction —
    // semantics unchanged, shape fixed.
    for line in text.lines().rev() {
        let Ok(m) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if m.get("type").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let content = m.get("message").and_then(|v| v.get("content"));
        let c = match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("text"))
                .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(""),
            _ => continue,
        };
        // A trailing tool_result-only record must not clobber a real
        // message — in reverse, that means skip empties and keep looking.
        if !c.trim().is_empty() {
            return c;
        }
    }
    String::new()
}

/// `\bjust do it\b`, case-insensitive — the single documented opt-out phrase.
fn has_opt_out(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    let c: Vec<char> = lower.chars().collect();
    let needle: Vec<char> = "just do it".chars().collect();
    if c.len() < needle.len() {
        return false;
    }
    for i in 0..=(c.len() - needle.len()) {
        if c[i..i + needle.len()] == needle[..]
            && word_start(&c, i)
            && word_end(&c, i + needle.len())
        {
            return true;
        }
    }
    false
}

/// issue-171: an Iterate turn's real text lives in the feedback drafts.
fn iterate_feedback_text(project_root: &Path, last_msg: &str) -> String {
    let stripped = last_msg.trim();
    let Some(rest) = stripped.strip_prefix("iterate:") else {
        return String::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(rest.trim()) else {
        return String::new();
    };
    let Some(items) = doc.get("items").and_then(|v| v.as_array()) else {
        return String::new();
    };
    let mut out: Vec<String> = Vec::new();
    for it in items {
        let Some(r) = it.get("feedbackRef").and_then(|v| v.as_str()) else {
            continue;
        };
        if r.is_empty() || r.contains('/') || r.contains('\\') || r.contains("..") {
            continue;
        }
        let path = state_dir(project_root)
            .join("feedback-drafts")
            .join(format!("{}.md", r));
        if let Ok(bytes) = std::fs::read(&path) {
            out.push(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    out.join("\n")
}

/// issue-431-opt-out-in-outbound-turn: a turn Bram delivers through an
/// outbound-turn file reaches the transcript only as its frame,
/// `Read and follow this Bram turn: @.bram/outbound-turns/<id>-turn.json
/// …`, so the user's words, "just do it" included, live in the file's `text`.
/// Since 9b40e82 (2026-10-02, #416) every message with a line break takes
/// that route; before it, short ones went inline and the phrase was visible.
/// Same indirection as `iterate_feedback_text`, same name hygiene.
fn outbound_turn_text(project_root: &Path, last_msg: &str) -> String {
    // issue-469: the frame names the state folder the host wrote the turn
    // into, `.bram/`; nothing else is honored.
    let folder = STATE_DIR_NEW;
    let mark = format!("@{}/outbound-turns/", folder);
    let Some(at) = last_msg.find(&mark) else {
        return String::new();
    };
    let name: String = last_msg[at + mark.len()..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    if !name.ends_with("-turn.json")
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return String::new();
    }
    let path = project_root.join(folder).join("outbound-turns").join(&name);
    let Ok(bytes) = std::fs::read(&path) else {
        return String::new();
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_default()
}

/// Last-chance authorization shared by every write surface (judell/bram#263).
/// Returns the last user message on a match so authority mode can compute
/// the audit-breadcrumb turn key; the shadow makes no network call, so for
/// it this remains read-only (Some = would-allow).
fn opt_out_clears(project_root: &Path, payload: &Value) -> Option<String> {
    let transcript = str_field(payload, "transcript_path");
    let last_msg = last_user_text(&transcript);
    if has_opt_out(&last_msg) {
        return Some(last_msg);
    }
    let draft = iterate_feedback_text(project_root, &last_msg);
    if !draft.is_empty() && has_opt_out(&draft) {
        return Some(last_msg);
    }
    // issue-431: the turn's words may live in its outbound-turn file. The
    // audit turn key stays the frame (`last_msg`), so the one-turn scope
    // and the host's dedupe are unchanged.
    let wrapped = outbound_turn_text(project_root, &last_msg);
    if !wrapped.is_empty() && has_opt_out(&wrapped) {
        return Some(last_msg);
    }
    None
}

// Stable per-turn key for the direct-edit audit breadcrumb (FNV-1a over the
// matched message; see the ShadowVerdict field for why sha256 parity with
// the Python guard is not required).
fn opt_out_turn_key(msg: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in msg.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn opt_out_allow(reason: &str, target: impl Into<String>, matched_msg: &str) -> ShadowVerdict {
    let mut v = allow(reason, target);
    v.audit_turn_key = Some(opt_out_turn_key(matched_msg));
    v
}

// --- payload accessors --------------------------------------------------------

fn str_field(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn tool_input(payload: &Value) -> Value {
    payload
        .get("tool_input")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

fn payload_cwd(payload: &Value) -> PathBuf {
    let cwd = str_field(payload, "cwd");
    if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(cwd)
    }
}

fn resolve_session_root(payload: &Value) -> Option<PathBuf> {
    let cwd = payload.get("cwd").and_then(|v| v.as_str())?;
    if cwd.is_empty() {
        return None;
    }
    find_project_root(cwd)
}

// --- the pipeline -------------------------------------------------------------

/// Shadow verdict for one PreToolUse payload. `None` means this provider has no
/// ported policy.
pub fn shadow_worklist_decision(provider: &str, payload: &Value) -> Option<ShadowVerdict> {
    match provider {
        "claude-rs" => Some(claude_decision(payload)),
        "codex-rs" => Some(codex_decision(payload)),
        _ => None,
    }
}

/// Per-repo pause of Worklist coverage enforcement for Claude. Reads
/// `worklist.enforcement` from `.bram.json` (legacy `.xmlui-desktop.json`);
/// any error or other value means enforcement stays on.
pub fn claude_enforcement_paused(cwd: &Path) -> bool {
    let Some(root) = find_project_root(&cwd.to_string_lossy()) else {
        return false;
    };
    for name in [".bram.json", ".xmlui-desktop.json"] {
        let path = root.join(name);
        if path.exists() {
            let Ok(bytes) = std::fs::read(&path) else {
                return false;
            };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                return false;
            };
            return value
                .pointer("/worklist/enforcement")
                .and_then(|v| v.as_str())
                == Some("paused");
        }
    }
    false
}

/// Context text handed to Claude at SessionStart / UserPromptSubmit while
/// enforcement is paused. Factual phrasing on purpose.
pub fn enforcement_paused_context() -> &'static str {
    "Worklist enforcement is paused for Claude in this repository: the user turned it off in Bram's Settings (.bram.json worklist.enforcement = \"paused\"). While it is paused, Bram's guard does not deny edits that no Worklist item covers, and the user manages the work and its commits directly, without Worklist items. Bram's conventions about proposing an item before editing, and the Start / Commit gate, do not apply while this is in effect. The guard's other checks (session URLs in commits, signatures on forge writes, subagent lifecycle calls) still apply."
}

fn claude_decision(payload: &Value) -> ShadowVerdict {
    let v = claude_decision_inner(payload);
    if v.decision == "deny"
        && (v.reason.starts_with("no-coverage") || v.reason.starts_with("bash-write-no-coverage"))
        && claude_enforcement_paused(&payload_cwd(payload))
    {
        return allow(format!("enforcement-paused:{}", v.reason), v.target);
    }
    v
}

fn claude_decision_inner(payload: &Value) -> ShadowVerdict {
    let tool_name = str_field(payload, "tool_name");
    if tool_name == "Bash" {
        return bash_branch(payload);
    }
    if tool_name.starts_with("mcp__") {
        return mcp_branch(payload, &tool_name);
    }
    if tool_name != "Write" && tool_name != "Edit" {
        return allow("no-trace:not-write-edit", "-");
    }
    write_edit_branch(payload, &tool_name)
}

fn bash_branch(payload: &Value) -> ShadowVerdict {
    let ti = tool_input(payload);
    let command = ti
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cwd = payload_cwd(payload);
    // Python's `preview = command[:200]` — the trace target on every Bash-branch
    // decision, and so the `target=` field of the transcribed header.
    let preview: String = command.chars().take(200).collect();
    let cwd_s = cwd.to_string_lossy().to_string();

    if let Some(matched) = gh_body_at_match(&command) {
        // python:1892-1909
        return deny_msg(
            "gh-body-at-antipattern",
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &format!(
                "gh --body takes a literal string, not stdin or a file reference.\nUse --body-file - (stdin) or --body-file <path> instead.\nDetected: {}",
                matched
            ),
        );
    }
    let (cb_verdict, cb_detail) = crossboundary_signature_verdict(&command, &cwd);
    // #331: an unreadable named body is still denied (fail-open would defeat
    // the check), but it is a DIFFERENT failure from an unsigned body and
    // must say so — the flat "crossboundary-unsigned" cost the Windows filer
    // three turns of re-verifying a signature the guard never read.
    if cb_verdict == "unparsed"
        && !matches!(
            cb_detail,
            "body-file-stdin" | "no-body-flag" | "input-file-stdin" | "input-json-no-body"
        )
    {
        let path = body_file_arg(&command)
            .or_else(|| input_file_arg(&command))
            .or_else(|| forge_positional_body_file(&command))
            .unwrap_or_else(|| "<unresolved path>".to_string());
        return deny_msg(
            format!("crossboundary-unparsed:{}", cb_detail),
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &format!(
                "The body file for this forge write could not be read, so its signature could not be verified:\n    {}\nAn unreadable named body is denied rather than silently bypassed (conventions.md, 'Signing agent-authored forge artifacts'). Check the path and retry — quoted and MSYS-style paths are accepted.",
                path
            ),
        );
    }
    if cb_verdict == "unsigned" {
        // python:1916-1934, message built by crossboundary_unsigned_message
        // (issue-365) so it names this checkout's expected locator.
        return deny_msg(
            "crossboundary-unsigned",
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &crossboundary_unsigned_message(&cwd),
        );
    }
    // guard-checks-the-signature-version: only reachable once the body parsed
    // AND the signature is well-formed, so "signed but stale" is a distinct
    // verdict from unsigned rather than a subset of it.
    let (ver_verdict, ver_signed, ver_expected) = signature_version_verdict(&command, &cwd);
    if ver_verdict == "mismatch" {
        return deny_msg(
            format!("crossboundary-stale-version:{}", ver_signed),
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &stale_signature_version_message(&ver_signed, &ver_expected),
        );
    }
    // forge-body-fold-enforced: a signed, current body that is long must fold.
    let (fold_verdict, fold_lines, fold_chars) = forge_body_fold_verdict(&command, &cwd);
    if fold_verdict == "unfolded" {
        return deny_msg(
            "long-forge-body-unfolded",
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &long_forge_body_unfolded_message(fold_lines, fold_chars),
        );
    }
    let (sha_verdict, sha_detail) = forge_sha_verdict(&command, &cwd);
    if sha_verdict == "bad" {
        // python:1947-1965
        return deny_msg(
            format!("forge-sha-unresolved:{}", sha_detail),
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &format!(
                "The body contains a full commit SHA that does not resolve in this repository:\n    {}\nA fabricated or rebase-orphaned SHA renders as an ordinary link and 404s silently. Resolve the real hash with `git rev-parse <short-sha>` (or `git log --oneline`) and retry. See judell/bram#277.",
                sha_detail
            ),
        );
    }
    if sha_verdict == "unpushed" {
        return deny_msg(
            format!("forge-sha-unpushed:{}", sha_detail),
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &forge_sha_unpushed_message(&sha_detail),
        );
    }
    let (su_verdict, su_where) = session_url_verdict(&command, &cwd);
    if su_verdict == "deny" {
        return deny_msg(
            "no-session-url",
            "-",
            "Bash",
            &preview,
            &cwd_s,
            &format!(
                "This commit or forge write carries an agent session URL or Claude-Session \
                 trailer (found in: {}). Session links are private telemetry; publishing \
                 them is irreversible and no opt-in exists. Remove the trailer/URL and \
                 retry. See conventions.md, 'No session URLs in public artifacts'.",
                su_where
            ),
        );
    }
    let agent_id = str_field(payload, "agent_id");
    let (sl_verdict, _) = subagent_lifecycle_verdict(&command, &agent_id);
    if sl_verdict == "deny" {
        // python:2007-2025
        return deny_msg(
            "subagent-lifecycle-call",
            "-",
            "Bash",
            &preview,
            &cwd_s,
            "Worklist lifecycle calls (/__worklist/resolve, /__worklist/mutate, worklist-commit) stay in the orchestrator's own turn, after delegated subagents return. A subagent transcript cannot make them directly. See 'Delegating worklist items to subagents' in conventions.md.",
        );
    }
    if forge_issue_only_write(&command) {
        return allow("forge-issue-only", "-");
    }
    // #299 (case 1): a pure forge read is a read. Asserted explicitly here so
    // the classification is a tested property, not an accident of which verbs
    // the write regexes happen to list.
    if forge_read_command(&command) && !bash_writes(&command) {
        return allow("bash-read-only", "-");
    }
    if !bash_writes(&command) {
        let reason = if has_redirect(&command) && !redirect_is_write(&command) {
            "bash-write-nonrepo-target"
        } else {
            "bash-read-only"
        };
        return allow(reason, "-");
    }
    let Some(project_root) = find_project_root(&cwd.to_string_lossy()) else {
        return allow("unmanaged-repo", "-");
    };
    if !agent_id.trim().is_empty() {
        for raw_target in redirect_targets(&command) {
            let candidate = if Path::new(&raw_target).is_absolute() {
                PathBuf::from(&raw_target)
            } else {
                cwd.join(&raw_target)
            };
            let rel = normalize_target(&project_root, &candidate.to_string_lossy());
            let (sw, _) = subagent_worklist_write_verdict(&project_root, rel.as_deref(), &agent_id);
            if sw == "deny" {
                // python:2082-2097. Note the Bash-branch trace target is the
                // command preview, not the redirect target.
                return deny_msg(
                    "subagent-worklist-write",
                    rel.unwrap_or_else(|| "-".into()),
                    "Bash",
                    &preview,
                    &cwd_s,
                    &String::from(SUBAGENT_WORKLIST_WRITE_BODY),
                );
            }
        }
    }
    if command.contains(&state_rel(&project_root, WORKLIST_DRAFTS_LEAF))
        || command.contains(".worklist-intent.json")
    {
        return allow("bram-lifecycle-channel", "-");
    }
    let covered = worklist_covered_files(&project_root);
    if !covered.is_empty() || fresh_bypass(&project_root, "*") {
        // issue-387: this is the coarse "does ANY item cover the worklist"
        // allow -- the gap `trace_would_deny_unaddressed` never reaches on
        // Bash. Refine it into a per-target observation without changing
        // the allow itself.
        trace_bash_would_deny(&project_root, "claude-rs", &command, &covered, |raw| {
            let candidate = if Path::new(raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                cwd.join(raw)
            };
            normalize_target(&project_root, &candidate.to_string_lossy())
        });
        return allow("covered-by-worklist-item", "-");
    }
    if let Some(msg) = opt_out_clears(&project_root, payload) {
        return opt_out_allow("opt-out-phrase", "-", &msg);
    }
    if push_cmd(&command) && post_commit_push_grace(&project_root) {
        return allow("post-commit-push-grace", "-");
    }
    // `deny_bash_coverage` (python:1284-1323). The root line is unconditional
    // here because this call site always passes a project_root.
    let mut body = "Bash blocked: this command writes to the filesystem or performs a sensitive side effect, and .bram/worklist.json has no proposed or applied items covering active work.\n  - Propose the work in the worklist first, wait for the user's approved: payload, then retry.\n  - The user can authorize a direct edit by ending their message with \"just do it\", or by clicking the Skip worklist button.".to_string();
    body.push_str(&claude_coverage_root_line(&project_root));
    if let Some(hint) = foreign_dot_bram_hint(&project_root) {
        body.push_str(&hint);
    }
    if push_cmd(&command) {
        body.push_str("\n  - This looks like a push. The user can click Push in the Commits tab; an agent push is allowed only in the 10-minute window after a gate commit (see judell/bram#283).");
    }
    // #333: name the evidence. The classifier knows which token made this a
    // write; saying so turns a misclassification report into a one-line
    // diagnosis instead of three lost turns.
    let token =
        write_patterns_token(&command, SkipPattern::None).unwrap_or_else(|| "?".to_string());
    deny_msg(
        format!(
            "bash-write-no-coverage:token={}:root={}",
            token,
            project_root.display()
        ),
        "-",
        "Bash",
        &preview,
        &cwd_s,
        &body,
    )
}

fn mcp_branch(payload: &Value, tool_name: &str) -> ShadowVerdict {
    let ti = tool_input(payload);
    let cwd = payload_cwd(payload);
    if !mcp_is_mutation(tool_name) {
        return allow("mcp-read-only", "-");
    }
    let Some(project_root) = find_project_root(&cwd.to_string_lossy()) else {
        return allow("unmanaged-repo", "-");
    };
    let cwd_s = cwd.to_string_lossy().to_string();
    let candidates =
        mcp_candidate_paths(&ti, &project_root, |s| normalize_target(&project_root, s));
    if candidates.is_empty() {
        // judell/bram#384: a write-verb NAME with nothing in tool_input that
        // structurally resolves to a repo path is out of scope, regardless
        // of the name -- most such calls are external connectors (Gmail,
        // Calendar, Drive) that cannot touch a repository file at all (see
        // the item's synth receipts; every real `mcp-unrecognized-input`
        // denial in this project's trace history was one of exactly these
        // three shapes, never a genuine filesystem write). This replaces
        // #360's fail-closed-with-bypass-first default: there is nothing
        // here to authorize, because the call cannot name a repo file.
        //
        // The one thing this loosens for real: a tool that DOES write a
        // repo file but whose target is ambient/session-remembered rather
        // than present anywhere in its own input. That case is
        // indistinguishable from a harmless connector call at this point,
        // so it is traced rather than silently allowed -- see
        // `trace_mcp_write_verb_no_candidates`.
        trace_mcp_write_verb_no_candidates(&project_root, "claude-rs", tool_name);
        return allow("mcp-no-candidate-paths", "-");
    }
    let agent_id = str_field(payload, "agent_id");
    let covered = worklist_covered_files(&project_root);
    let mut violations: Vec<String> = Vec::new();
    for rel in candidates.iter().cloned() {
        if is_worklist_rel(&project_root, &rel) {
            // python:2151-2165
            let body = format!(
                "{tool} blocked: edits to {worklist} must use Write/Edit (so the guard can validate prunes, the version bump, and inline prose) or /__worklist/mutate for mechanical transitions.",
                tool = tool_name,
                worklist = state_rel(&project_root, WORKLIST_LEAF)
            );
            return deny_msg(
                "mcp-worklist-write",
                rel.clone(),
                tool_name,
                &rel,
                &cwd_s,
                &body,
            );
        }
        let (sw, _) = subagent_worklist_write_verdict(&project_root, Some(&rel), &agent_id);
        if sw == "deny" {
            // python:2174-2188
            return deny_msg(
                "subagent-worklist-write",
                rel.clone(),
                tool_name,
                &rel,
                &cwd_s,
                &String::from(SUBAGENT_WORKLIST_WRITE_BODY),
            );
        }
        let (is_covered, _) = coverage_verdict(&covered, &rel);
        if is_lifecycle_path(&project_root, &rel) || is_covered || fresh_bypass(&project_root, &rel)
        {
            if is_covered && !is_lifecycle_path(&project_root, &rel) {
                trace_would_deny_unaddressed(&project_root, "claude-rs", tool_name, &rel);
            }
            continue;
        }
        violations.push(rel);
    }
    if let Some(first) = violations.first() {
        if let Some(msg) = opt_out_clears(&project_root, payload) {
            return opt_out_allow("opt-out-phrase", first.clone(), &msg);
        }
        // `deny_coverage(violations[0], opt_out_attempted=False, project_root=…)`
        // (python:2200). No session-root leg on this surface, and
        // `deny_coverage`'s own `_trace_hook` passes no cwd, so the header
        // carries the process cwd rather than the payload's.
        let (reason, body) = deny_coverage_parts(first, false, Some(&project_root), None, None);
        return deny_msg(
            reason.clone(),
            first.clone(),
            tool_name,
            first,
            &process_cwd(),
            &body,
        );
    }
    // Python traces the comma-joined candidate list here; the breadcrumb's
    // target field must stay whitespace-free, so the shadow writes `-`.
    allow("covered-by-worklist-item", "-")
}

fn write_edit_branch(payload: &Value, tool_name: &str) -> ShadowVerdict {
    let ti = tool_input(payload);
    let fp = ti.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
    if fp.is_empty() {
        return allow("no-trace:no-file-path", "-");
    }
    let start = Path::new(fp)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".to_string());
    let Some(project_root) = find_project_root(&start) else {
        return allow("no-trace:unmanaged-repo", "-");
    };
    let Some(rel) = normalize_target(&project_root, fp) else {
        return allow("no-trace:outside-project", "-");
    };

    // Every deny below is reached through Python's `deny_*` helpers or an
    // inline `print`, none of which pass a cwd to `_trace_hook` — so the
    // header's `cwd=` is the guard process's own, not the payload's.
    let hook_cwd = process_cwd();

    let agent_id = str_field(payload, "agent_id");
    let (sw, _) = subagent_worklist_write_verdict(&project_root, Some(&rel), &agent_id);
    if sw == "deny" {
        // python:2239-2256. This surface's wording carries an extra clause the
        // Bash / MCP twins do not ("-- it can widen its own item's coverage…").
        return deny_msg(
            "subagent-worklist-write",
            rel.clone(),
            tool_name,
            &rel,
            &hook_cwd,
            &"Worklist proposals and claim edits (.bram/worklist.json, .bram/worklist-drafts/*) stay in the orchestrator's own turn. A delegated subagent cannot write them directly -- it can widen its own item's coverage, author new items, or edit claim state with no denial. See 'Delegating worklist items to subagents' in conventions.md and judell/bram#287.".to_string(),
        );
    }

    let worklist_rel = state_rel(&project_root, WORKLIST_LEAF);
    if rel != worklist_rel && is_lifecycle_path(&project_root, &rel) {
        return allow("bram-lifecycle-channel", rel);
    }

    if rel == worklist_rel {
        if !Path::new(fp).exists() {
            return allow("worklist-bootstrap", rel);
        }
        let old = match std::fs::read_to_string(fp) {
            Ok(old) => old,
            Err(err) => {
                // Python raises here; the top-level `except BaseException`
                // (python:2350-2382) traces `decision=error reason=<summary>`
                // and prints the guard-bug text. The summary is a Python
                // exception repr — its Rust twin below is the closest reading,
                // never byte-identical.
                let summary: String = format!("OSError: {}", err).chars().take(500).collect();
                let header = hook_stderr_header(
                    tool_name,
                    "",
                    &hook_cwd,
                    "error",
                    &summary.chars().take(200).collect::<String>(),
                );
                let mut v = deny("guard-error:worklist-unreadable", rel);
                v.message = format!(
                    "{}\nBlocked: the Bram worklist guard failed and denied by default.\n  - {}\n  - This is a guard bug, not a policy decision. Please report it.",
                    header, summary
                );
                return v;
            }
        };
        let new = if tool_name == "Write" {
            ti.get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            let o = ti.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let n = ti.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            let replace_all = ti
                .get("replace_all")
                .map(|v| !matches!(v, Value::Null | Value::Bool(false)))
                .unwrap_or(false);
            if replace_all {
                old.replace(o, n)
            } else {
                old.replacen(o, n, 1)
            }
        };
        let old_items = items_map(&old);
        let new_items = items_map(&new);
        let (removed, status_changed) = worklist_state_changes(&old_items, &new_items);
        if removed.is_empty() && status_changed.is_empty() {
            let inline_bad = worklist_items_with_inline_prose(&new);
            if !inline_bad.is_empty() {
                // `deny_inline_prose` (python:1388-1409)
                let mut lines = vec!["Blocked: inline `before` / `after` on proposed worklist item(s) — prose must live in .bram/worklist-drafts/<id>.md.".to_string()];
                for label in &inline_bad {
                    lines.push(format!(
                        "  - item {label}: remove inline before/after; write prose to {sd}/worklist-drafts/{label}.md instead",
                        label = label,
                        sd = state_dir_name(&project_root)
                    ));
                }
                lines.push("Draft file format: `# Before` section then `# After` section, both in Markdown. The server merge surfaces the draft prose alongside the metadata in worklist.json.".to_string());
                return deny_msg(
                    "worklist-inline-prose",
                    rel,
                    tool_name,
                    &worklist_rel,
                    &hook_cwd,
                    &lines.join("\n"),
                );
            }
            let (old_has, old_version) = worklist_version_from_text(&old);
            let (new_has, new_version) = worklist_version_from_text(&new);
            if old_has && (!new_has || new_version != old_version + 1) {
                // `deny_stale_worklist_version` (python:1342-1371)
                let detail = if new_has {
                    format!(
                        "You set version={}, but on-disk version is {}. Expected version={}.",
                        new_version,
                        old_version,
                        old_version + 1
                    )
                } else {
                    format!(
                        "Your write is missing the `version` field. On-disk version is {}; the new content must set version={}.",
                        old_version,
                        old_version + 1
                    )
                };
                let body = format!(
                    "Blocked: stale base on {wl}. {}\n  - Re-read {wl}, base your edit on the current contents, and include the bumped version field on the write. This guards against concurrent-writer races between your propose / Edit and other agents or the /__worklist/mutate route.",
                    detail,
                    wl = worklist_rel
                );
                return deny_msg(
                    "stale-worklist-version",
                    rel,
                    tool_name,
                    &worklist_rel,
                    &hook_cwd,
                    &body,
                );
            }
            return allow("worklist-author", rel);
        }
        // `deny_mechanical_worklist_change` (python:1412-1444)
        let mut lines = vec![
            format!("Blocked: mechanical worklist state changes must go through `POST /__worklist/mutate`, not a direct edit to `{}`.", worklist_rel),
            "  - Direct worklist edits are for proposing items or refining their prose during iterate.".to_string(),
            "  - Use mutate for `prune` and `advance` after a verified `drop:` / `approved:` turn.".to_string(),
        ];
        if !removed.is_empty() {
            let detail = removed
                .iter()
                .map(|(id, status)| format!("\"{}\" (status={})", id, status))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  - Removed item ids: {}", detail));
        }
        if !status_changed.is_empty() {
            let detail = status_changed
                .iter()
                .map(|(id, old_status, new_status)| {
                    format!("\"{}\" ({}->{})", id, old_status, new_status)
                })
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  - Status changes: {}", detail));
        }
        lines.push("  - Example: curl -4 -sS -X POST -d '{\"op\":\"prune\",\"ids\":[\"item-id\"]}' http://127.0.0.1:$(cat .bram/.bram-port)/__worklist/mutate".to_string());
        return deny_msg(
            "mechanical-worklist-change",
            rel,
            tool_name,
            &worklist_rel,
            &hook_cwd,
            &lines.join("\n"),
        );
    }

    if is_worklist_draft(&project_root, &rel) {
        return allow("worklist-draft", rel);
    }

    let covered = worklist_covered_files(&project_root);
    // Python's `deny_coverage` recomputes the worktree from the target, so the
    // coverage verdict's own copy is only consulted for the allow leg.
    let (is_covered, _worktree) = coverage_verdict(&covered, &rel);
    if is_covered {
        trace_would_deny_unaddressed(&project_root, "claude-rs", tool_name, &rel);
        return allow("covered-by-worklist-item", rel);
    }
    let session_root = resolve_session_root(payload);
    let (bypass_ok, bypass_via, _target_detail, session_detail) =
        cross_root_bypass_verdict(&project_root, &rel, session_root.as_deref());
    if bypass_ok {
        let reason = if bypass_via == Some("target") {
            "fresh-bypass".to_string()
        } else {
            format!(
                "fresh-bypass:session-root={}",
                session_root
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            )
        };
        return allow(reason, rel);
    }
    if let Some(msg) = opt_out_clears(&project_root, payload) {
        return opt_out_allow("opt-out-phrase", rel, &msg);
    }
    // `deny_coverage` (python:2330-2337). `opt_out_attempted` is the near-miss
    // heuristic computed at the call site from the last user message.
    let last_msg = last_user_text(&str_field(payload, "transcript_path")).to_lowercase();
    let opt_out_attempted = last_msg.contains("worklist") && last_msg.contains("no");
    let (reason, body) = deny_coverage_parts(
        &rel,
        opt_out_attempted,
        Some(&project_root),
        session_root.as_deref(),
        session_detail,
    );
    deny_msg(reason, rel.clone(), tool_name, &rel, &hook_cwd, &body)
}

// =============================================================================
// Codex adapter
// =============================================================================
//
// Authoritative source: app/provider-hooks/codex-worklist-guard.py. The shared
// core above is reused wherever the two Python guards are literally the same
// code (the write-pattern scanners, the signature / SHA gates, the MCP surface,
// push-grace, draft-path shapes); everything below is where Codex genuinely
// differs. The differences, verified against the Python rather than assumed:
//
// - **No project-root walk.** `cwd` IS the root (`claude_coverage_root_line`'s
//   "anchored at cwd"), so there is no `find_project_root`, no `resolve_
//   session_root`, and no issue-262 cross-root bypass. A cwd with no
//   `.bram/.worklist-authorization.json` allows unconditionally.
// - **No transcript, no prose opt-out at PreToolUse.** `has_opt_out` runs on
//   the UserPromptSubmit surface only, where the Python guard WRITES a
//   `direct-edit` record; PreToolUse then reads that record through
//   `fresh_bypass()`. The shadow never writes, so the write half is
//   deliberately not ported (see `codex_user_prompt_branch`).
// - **`apply_patch`, not Write/Edit.** Targets come out of the patch body
//   (`*** Update File:` / `+++ b/`), and the worklist-json validators work on
//   a reconstructed post-patch file.
// - **The `.worklist-intent.json` coordination channel** (#130) plus its
//   pending/stale gate, which has no Claude counterpart.
// - **`.bram/worklist-citations/` is an exemption of its own** here; the
//   Claude guard folds it into `_LIFECYCLE_PATHS_PREFIXES`.
// - **No `gh issue <verb>` entry in `_BASH_WRITE_PATTERNS`,** hence no
//   `forge_issue_only_write` exemption — `gh issue close 5` is simply not a
//   write on this side.
// - **No subagent gates.** The Codex hook payload carries no agent identity
//   (the Python says so at length at both sites), so #287 has nothing to key
//   on.
// - **Reason strings are derived, not named.** Codex's `allow()` defaults to
//   `passed-checks` and its `deny()` traces `message.splitlines()[0][:120]`.
//   So the shadow builds the Python message verbatim and truncates it the same
//   way, instead of carrying the Claude guard's short named reasons.
//
// judell/bram#299 inheritance: the Codex guard carries byte-identical copies of
// `_REDIRECT_TARGET_RX` / `_REDIRECT_WRITE_RX` and of the
// `python[0-9.]*\s+-c\b` pattern, so cases 2 and 3 misclassify here exactly as
// they do on the Claude side and the shared core's fixes apply unchanged. Case 1
// is inherited as an assertion only: with no `gh` entry in this guard's write
// patterns, a pure forge read was never captured here, so `forge_read_command`
// changes no verdict on this side — it is pinned by test so a future widening
// cannot silently recapture it.

const WORKLIST_CITATIONS_LEAF: &str = "worklist-citations/";
const WORKLIST_INTENT_LEAF: &str = ".worklist-intent.json";
const WORKLIST_RESULT_LEAF: &str = ".worklist-result.json";
const INTENT_STALE_SECONDS: f64 = 120.0;

/// Codex `deny()`: the traced reason is the message's first line, 120 chars.
fn codex_deny_reason(message: &str) -> String {
    if message.is_empty() {
        return "blocked".to_string();
    }
    // Python `.splitlines()[0]` — our messages only ever carry `\n`, but the
    // other Python line terminators are cheap to honour.
    let first = message
        .split(|c| {
            matches!(
                c,
                '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
        })
        .next()
        .unwrap_or("");
    first.chars().take(120).collect()
}

fn codex_deny(message: &str, target: &str) -> ShadowVerdict {
    let mut v = deny(codex_deny_reason(message), target);
    // The full Python-verbatim message was always constructed here and then
    // truncated into the reason; authority mode prints it, so keep it.
    v.message = message.to_string();
    v
}

fn codex_allow(reason: &str, target: &str) -> ShadowVerdict {
    allow(reason, target)
}

/// Python `str.splitlines()` for the terminators a patch body can carry.
fn py_splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push(&s[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                out.push(&s[start..i]);
                i += if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    2
                } else {
                    1
                };
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

// --- codex path / authorization helpers --------------------------------------

/// Codex `normalize_target`: `os.path.abspath(os.path.join(cwd, target))`.
/// The join against the PROJECT cwd is the difference from the Claude core's
/// `normalize_target`, which abspaths the target against the PROCESS cwd.
fn codex_normalize_target(cwd: &Path, target: &str) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let joined = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        cwd.join(target)
    };
    let abs_target = abspath_of(&joined);
    let abs_cwd = abspath_of(cwd);
    if abs_target == abs_cwd {
        return Some(String::new());
    }
    let sep = std::path::MAIN_SEPARATOR;
    let prefix = format!("{}{}", abs_cwd.to_string_lossy(), sep);
    let t = abs_target.to_string_lossy().into_owned();
    if t.starts_with(&prefix) {
        Some(t[prefix.len()..].replace(sep, "/"))
    } else {
        None
    }
}

/// Codex `fresh_bypass`: reads ONLY snake_case `issued_at_ms` (the Claude
/// guard accepts `issuedAtMs` first), and has no per-root detail vocabulary.
/// issue-352: sidecar first, legacy shared file honored for one release —
/// the same precedence as the Claude arm's bypass_lookup_detail.
fn codex_fresh_bypass(cwd: &Path, path_rel: &str) -> bool {
    codex_fresh_bypass_in(&state_path(cwd, DIRECT_EDIT_LEAF), path_rel)
        || codex_fresh_bypass_in(&state_path(cwd, AUTH_LEAF), path_rel)
}

fn codex_fresh_bypass_in(file: &Path, path_rel: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let Some(rec) = doc.as_object() else {
        return false;
    };
    if rec.get("kind").and_then(|v| v.as_str()) != Some("direct-edit") {
        return false;
    }
    let issued = rec
        .get("issued_at_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if now_ms() - issued > BYPASS_TTL_MS {
        return false;
    }
    rec.get("paths")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == path_rel || s == "*")
        })
        .unwrap_or(false)
}

fn is_worklist_citation(root: &Path, rel: &str) -> bool {
    let Some(leaf) = state_leaf(root, rel) else {
        return false;
    };
    leaf.starts_with(WORKLIST_CITATIONS_LEAF)
        && leaf.ends_with(".json")
        && !leaf[WORKLIST_CITATIONS_LEAF.len()..].contains('/')
}

fn is_coordination_file(root: &Path, rel: &str) -> bool {
    matches!(
        state_leaf(root, rel),
        Some(WORKLIST_INTENT_LEAF) | Some(WORKLIST_RESULT_LEAF)
    )
}

fn current_worklist_text(cwd: &Path) -> String {
    // Lossy for the same reason as last_user_text: an undecodable byte would
    // otherwise read as an empty worklist, i.e. no coverage for any path, and
    // deny every edit with a misleading reason. Our own file, so likelier to
    // carry an em-dash than a corrupt byte — but the failure mode is the same.
    std::fs::read(state_path(cwd, WORKLIST_LEAF))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// Codex `worklist_items` — the raw item array, for `_patch_removes_worklist_items`.
fn codex_worklist_items(cwd: &Path) -> Vec<Value> {
    let Ok(doc) = serde_json::from_str::<Value>(&current_worklist_text(cwd)) else {
        return Vec::new();
    };
    let Some(obj) = doc.as_object() else {
        return Vec::new();
    };
    match obj.get("items") {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// Codex `items_by_id_from_content`: skips non-dict items and non-string /
/// blank ids instead of collapsing the whole map (the Claude `items_by_id`
/// comprehension raises on a missing `id` and the except returns `{}`).
fn codex_items_by_id(content: &str) -> Vec<(String, Value)> {
    let Ok(doc) = serde_json::from_str::<Value>(content) else {
        return Vec::new();
    };
    let Some(items) = doc.get("items").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Value)> = Vec::new();
    for it in items {
        let Some(o) = it.as_object() else { continue };
        let Some(id) = o.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if id.trim().is_empty() {
            continue;
        }
        // Python dict semantics: a repeated id keeps the last item.
        match out.iter_mut().find(|(k, _)| k == id) {
            Some(slot) => slot.1 = it.clone(),
            None => out.push((id.to_string(), it.clone())),
        }
    }
    out
}

/// Codex `bash_writes`: identical to the Claude core minus the
/// `gh issue <write-verb>` pattern, which this guard's
/// `_BASH_WRITE_PATTERNS` does not carry at all.
fn codex_bash_writes(command: &str) -> bool {
    write_patterns_match(command, SkipPattern::GhIssueWrite)
}

// --- pending-intent gate (#130, no Claude counterpart) -----------------------

#[derive(Debug, Clone, PartialEq)]
enum IntentDetail {
    NoPending,
    StaleOverwrite,
    Pending {
        nonce: Option<String>,
        route: Option<String>,
        age_seconds: i64,
    },
}

fn intent_write_verdict(cwd: &Path) -> (&'static str, IntentDetail) {
    let path = state_path(cwd, WORKLIST_INTENT_LEAF);
    let Ok(meta) = std::fs::metadata(&path) else {
        return ("allow", IntentDetail::NoPending);
    };
    let Ok(modified) = meta.modified() else {
        return ("allow", IntentDetail::NoPending);
    };
    let mtime_ms = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let age_seconds = (now_ms() - mtime_ms) / 1000.0;
    if age_seconds > INTENT_STALE_SECONDS {
        return ("allow", IntentDetail::StaleOverwrite);
    }
    let doc = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let field = |k: &str| {
        doc.as_ref()
            .and_then(|d| d.as_object())
            .and_then(|o| o.get(k))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    (
        "deny",
        IntentDetail::Pending {
            nonce: field("nonce"),
            route: field("route"),
            // Python `int(age_seconds)` truncates toward zero.
            age_seconds: age_seconds as i64,
        },
    )
}

fn opt_str(v: &Option<String>) -> String {
    match v {
        Some(s) => s.clone(),
        None => "None".to_string(),
    }
}

fn intent_pending_message(tool_label: &str, detail: &IntentDetail) -> String {
    let (nonce, route, age) = match detail {
        IntentDetail::Pending {
            nonce,
            route,
            age_seconds,
        } => (opt_str(nonce), opt_str(route), *age_seconds),
        _ => ("None".to_string(), "None".to_string(), 0),
    };
    format!(
        "{tool} blocked: .bram/.worklist-intent.json already holds a pending \
request (route={route}, nonce={nonce}, age={age}s) that has not drained yet. \
Wait for .bram/.worklist-result.json to appear, or retry once the prior \
request clears.",
        tool = tool_label,
        route = route,
        nonce = nonce,
        age = age
    )
}

// --- codex patch parsing ------------------------------------------------------

/// Concatenation of the string-valued `input` / `patch` / `content` / `command`
/// keys, each prefixed with a newline (Codex `patch_text`).
fn patch_text(tool_input: &Value) -> String {
    if let Some(s) = tool_input.as_str() {
        return s.to_string();
    }
    let Some(obj) = tool_input.as_object() else {
        return String::new();
    };
    let mut text = String::new();
    for key in ["input", "patch", "content", "command"] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            text.push('\n');
            text.push_str(v);
        }
    }
    text
}

/// `^\*\*\* (?:Update|Add|Delete) File: (.+?)\s*$` plus the `^\+\+\+ b/(.+?)\s*$`
/// unified-diff fallback. Python collects these into a SET; the shadow keeps
/// first-seen order so the violation list (and therefore the deny message the
/// reason is cut from) is deterministic.
fn patch_targets(tool_input: &Value) -> Vec<String> {
    fn push(out: &mut Vec<String>, v: &str) {
        // `(.+?)\s*$`: trailing whitespace is not part of the path. A
        // whitespace-only remainder yields no target here where Python's lazy
        // `.+?` would capture one space; no real patch produces that shape.
        let trimmed = v.trim_end();
        if !trimmed.is_empty() && !out.iter().any(|e| e == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    let text = patch_text(tool_input);
    let mut out: Vec<String> = Vec::new();
    for line in py_splitlines(&text) {
        for head in ["*** Update File: ", "*** Add File: ", "*** Delete File: "] {
            if let Some(rest) = line.strip_prefix(head) {
                push(&mut out, rest);
            }
        }
        if let Some(rest) = line.strip_prefix("+++ b/") {
            push(&mut out, rest);
        }
    }
    out
}

fn added_block_text(patch: &str) -> String {
    patch
        .split('\n')
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| &l[1..])
        .collect::<Vec<_>>()
        .join("\n")
}

fn removed_block_text(patch: &str) -> String {
    patch
        .split('\n')
        .filter(|l| l.starts_with('-') && !l.starts_with("---"))
        .map(|l| &l[1..])
        .collect::<Vec<_>>()
        .join("\n")
}

/// Codex `_worklist_content_after_apply_patch`: a best-effort interpreter for
/// `*** Update File: .bram/worklist.json` hunks. None means "too unusual to
/// apply confidently", which routes the caller to the patch-text heuristics.
fn worklist_content_after_apply_patch(cwd: &Path, patch: &str) -> Option<String> {
    let old_content = current_worklist_text(cwd);
    if old_content.is_empty() {
        return None;
    }
    let lines = py_splitlines(patch);
    let header = format!("*** Update File: {}", state_rel(cwd, WORKLIST_LEAF));
    let mut content = old_content;
    let mut saw_worklist_update = false;
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i] != header {
            i += 1;
            continue;
        }
        saw_worklist_update = true;
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("*** ") && line != "*** End of File" {
                break;
            }
            if !line.starts_with("@@") {
                i += 1;
                continue;
            }
            i += 1;
            let mut old_parts: Vec<&str> = Vec::new();
            let mut new_parts: Vec<&str> = Vec::new();
            while i < lines.len() {
                let hline = lines[i];
                if hline.starts_with("@@") || hline.starts_with("*** ") {
                    break;
                }
                if let Some(rest) = hline.strip_prefix(' ') {
                    old_parts.push(rest);
                    new_parts.push(rest);
                } else if let Some(rest) = hline.strip_prefix('-') {
                    old_parts.push(rest);
                } else if let Some(rest) = hline.strip_prefix('+') {
                    new_parts.push(rest);
                } else {
                    return None;
                }
                i += 1;
            }
            let old_text = old_parts.join("\n");
            let new_text = new_parts.join("\n");
            if !old_text.is_empty() {
                if !content.contains(&old_text) {
                    return None;
                }
                content = content.replacen(&old_text, &new_text, 1);
            } else if !new_text.is_empty() {
                return None;
            }
        }
    }
    if saw_worklist_update {
        Some(content)
    } else {
        None
    }
}

// --- codex worklist validators -----------------------------------------------

/// `"<key>"\s*:\s*"([^"]*)"` over a text blob. `require_nonempty` selects the
/// `[^"]+` form `_ID_RE` uses.
fn json_key_string_values(text: &str, key: &str, require_nonempty: bool) -> Vec<String> {
    let c = chars(text);
    let needle = format!("\"{}\"", key);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        let Some(j) = lit(&c, i, &needle) else {
            i += 1;
            continue;
        };
        let j = ws0(&c, j);
        if j >= c.len() || c[j] != ':' {
            i += 1;
            continue;
        }
        let j = ws0(&c, j + 1);
        if j >= c.len() || c[j] != '"' {
            i += 1;
            continue;
        }
        let mut e = j + 1;
        while e < c.len() && c[e] != '"' {
            e += 1;
        }
        if e >= c.len() || (require_nonempty && e == j + 1) {
            i += 1;
            continue;
        }
        out.push(c[j + 1..e].iter().collect::<String>());
        i = e + 1;
    }
    out
}

/// `"files"\s*:\s*\[\s*"` — the files-array opener `_patch_adds_violating_
/// draft_only` counts alongside the `"file"` hits.
fn files_array_opener_count(text: &str) -> usize {
    let c = chars(text);
    let mut n = 0usize;
    let mut i = 0usize;
    while i < c.len() {
        let Some(j) = lit(&c, i, "\"files\"") else {
            i += 1;
            continue;
        };
        let j = ws0(&c, j);
        if j >= c.len() || c[j] != ':' {
            i += 1;
            continue;
        }
        let j = ws0(&c, j + 1);
        if j >= c.len() || c[j] != '[' {
            i += 1;
            continue;
        }
        let j = ws0(&c, j + 1);
        if j >= c.len() || c[j] != '"' {
            i += 1;
            continue;
        }
        n += 1;
        i = j + 1;
    }
    n
}

fn item_has_file(o: &serde_json::Map<String, Value>) -> bool {
    if let Some(f) = o.get("file").and_then(|v| v.as_str()) {
        if !f.trim().is_empty() {
            return true;
        }
    }
    if let Some(fs) = o.get("files").and_then(|v| v.as_array()) {
        for e in fs {
            if let Some(s) = e.as_str() {
                if !s.trim().is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

type DraftViolations = Vec<(String, Vec<String>)>;

/// Codex `_worklist_items_violating_draft_only`. Wider than the Claude guard's
/// `worklist_items_with_inline_prose`: missing `id` and missing `file`/`files`
/// are violations here too. None means the content did not parse as JSON, which
/// the Python callers treat as falsy (no deny).
fn worklist_items_violating_draft_only(content: &str) -> Option<DraftViolations> {
    let doc: Value = serde_json::from_str(content).ok()?;
    let Some(items) = doc.get("items").and_then(|v| v.as_array()) else {
        return Some(Vec::new());
    };
    let mut bad: DraftViolations = Vec::new();
    for it in items {
        let Some(o) = it.as_object() else { continue };
        if o.get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("proposed")
            != "proposed"
        {
            continue;
        }
        let mut violations: Vec<String> = Vec::new();
        let item_id = o.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if item_id.trim().is_empty() {
            violations.push("id".to_string());
        }
        if !item_has_file(o) {
            violations.push("file (or non-empty files array)".to_string());
        }
        let nonempty = |k: &str| {
            o.get(k)
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        };
        if nonempty("before") {
            violations.push("inline `before` (move to .bram/worklist-drafts/<id>.md)".to_string());
        }
        if nonempty("after") {
            violations.push("inline `after` (move to .bram/worklist-drafts/<id>.md)".to_string());
        }
        if !violations.is_empty() {
            let label = if item_id.trim().is_empty() {
                "<no-id>"
            } else {
                item_id
            };
            bad.push((label.to_string(), violations));
        }
    }
    Some(bad)
}

/// Codex `_patch_adds_violating_draft_only`: the same rule applied to a patch's
/// added lines, for the case where the post-patch file could not be rebuilt.
fn patch_adds_violating_draft_only(patch: &str) -> DraftViolations {
    let added = added_block_text(patch);
    let ids: Vec<String> = json_key_string_values(&added, "id", true)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .collect();
    let file_hits = json_key_string_values(&added, "file", false)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .count();
    let files = file_hits + files_array_opener_count(&added);
    let befores = json_key_string_values(&added, "before", false)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .count();
    let afters = json_key_string_values(&added, "after", false)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .count();
    let item_count = ids.len().max(files);
    let mut violations: Vec<String> = Vec::new();
    if item_count > 0 {
        if ids.len() < item_count {
            violations.push(format!("id (saw {} of {})", ids.len(), item_count));
        }
        if files < item_count {
            violations.push(format!("file or files (saw {} of {})", files, item_count));
        }
    }
    if befores > 0 {
        violations.push(format!(
            "inline `before` (move to .bram/worklist-drafts/<id>.md; saw {})",
            befores
        ));
    }
    if afters > 0 {
        violations.push(format!(
            "inline `after` (move to .bram/worklist-drafts/<id>.md; saw {})",
            afters
        ));
    }
    if violations.is_empty() {
        return Vec::new();
    }
    let label = ids
        .first()
        .cloned()
        .unwrap_or_else(|| "<missing-id>".into());
    vec![(label, violations)]
}

/// Codex `_patch_removes_worklist_items`.
fn patch_removes_worklist_items(cwd: &Path, patch: &str) -> Vec<(String, String)> {
    let mut old_items: Vec<(String, Value)> = Vec::new();
    for item in codex_worklist_items(cwd) {
        if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
            let id = id.to_string();
            match old_items.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = item.clone(),
                None => old_items.push((id, item.clone())),
            }
        }
    }
    let removed_ids = json_key_string_values(&removed_block_text(patch), "id", true);
    let readded_ids = json_key_string_values(&added_block_text(patch), "id", true);
    let mut ids: Vec<String> = removed_ids
        .into_iter()
        .filter(|id| !id.is_empty() && !readded_ids.contains(id))
        .collect();
    ids.sort();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        if let Some((_, item)) = old_items.iter().find(|(k, _)| *k == id) {
            out.push((id, item_status(item)));
        }
    }
    out
}

/// Codex `_worklist_new_content_from_tool_input` (the MCP surface).
fn worklist_new_content_from_tool_input(cwd: &Path, tool_input: &Value) -> Option<String> {
    let old_content = current_worklist_text(cwd);
    let obj = tool_input.as_object()?;
    for key in ["content", "text"] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    if let Some(edits) = obj.get("edits").and_then(|v| v.as_array()) {
        let mut new_content = old_content;
        for edit in edits {
            let e = edit.as_object()?;
            let old_text = e.get("oldText").and_then(|v| v.as_str())?;
            let new_text = e.get("newText").and_then(|v| v.as_str())?;
            new_content = new_content.replacen(old_text, new_text, 1);
        }
        return Some(new_content);
    }
    let old_text = obj.get("old_string").and_then(|v| v.as_str());
    let new_text = obj.get("new_string").and_then(|v| v.as_str());
    if let (Some(o), Some(n)) = (old_text, new_text) {
        return Some(old_content.replacen(o, n, 1));
    }
    None
}

// --- codex deny messages (verbatim; the reason is cut from line 1) ------------

fn coverage_root_line(cwd: &Path) -> String {
    format!(
        "\nresolved_project_root={} (marker: {}, anchored at cwd)",
        abspath_of(cwd).display(),
        state_rel(cwd, AUTH_LEAF)
    )
}

fn worklist_validation_error(bad: &DraftViolations, tool_name: &str) -> String {
    if bad.is_empty() {
        return format!("{} blocked: worklist validation failed.", tool_name);
    }
    let detail = bad
        .iter()
        .map(|(label, v)| format!("  - item {}: {}", label, v.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{tool} blocked: proposed worklist item(s) violate draft-only rule.\n{detail}\n\
Required for every proposed item: \"id\" (kebab-case identifier) and \"file\" \
(or \"files\" array for multi-file items). Prose must live in \
.bram/worklist-drafts/<id>.md (Markdown with `# Before` and `# After` \
sections); inline \"before\"/\"after\" keys in worklist.json are rejected. \
Write the draft file first, then add the metadata-only item to worklist.json.",
        tool = tool_name,
        detail = detail
    )
}

fn mechanical_worklist_change_error(
    removed: &[(String, String)],
    status_changed: &[(String, String, String)],
    tool_name: &str,
) -> String {
    let mut lines = vec![
        format!(
            "{} blocked: mechanical worklist state changes must go through \
`POST /__worklist/mutate`, not a direct edit to `.bram/worklist.json`.",
            tool_name
        ),
        "Direct worklist edits are for proposing items or refining prose during iterate."
            .to_string(),
        "Use mutate for `prune` and `advance` after a verified `drop:` / `approved:` turn."
            .to_string(),
    ];
    if !removed.is_empty() {
        let detail = removed
            .iter()
            .map(|(id, st)| format!("\"{}\" (status={})", id, st))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Removed item ids: {}", detail));
    }
    if !status_changed.is_empty() {
        let detail = status_changed
            .iter()
            .map(|(id, o, n)| format!("\"{}\" ({}->{})", id, o, n))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Status changes: {}", detail));
    }
    lines.push(
        "Example: curl -4 -sS -X POST -d \
'{\"op\":\"advance\",\"ids\":[\"item-id\"],\"status\":\"applied\"}' \
http://127.0.0.1:$(cat .bram/.bram-port)/__worklist/mutate"
            .to_string(),
    );
    lines.join("\n")
}

fn stale_worklist_version_error(
    old_version: i64,
    new_version: i64,
    new_present: bool,
    tool_name: &str,
) -> String {
    let detail = if new_present {
        format!(
            "You set version={}, but on-disk version is {}. Expected version={}.",
            new_version,
            old_version,
            old_version + 1
        )
    } else {
        format!(
            "Your write is missing the `version` field. On-disk version is {}; \
the new content must set version={}.",
            old_version,
            old_version + 1
        )
    };
    format!(
        "{} blocked: stale base on .bram/worklist.json. {} Re-read the file, \
base your edit on the current contents, and include the bumped version field. \
This guards against concurrent-writer races between your propose / apply_patch \
and other agents or the /__worklist/mutate route.",
        tool_name, detail
    )
}

// --- UserPromptSubmit --------------------------------------------------------

/// `_CHANGE_KEYWORDS`, expanded. The Python regex is one `\b(a|b|...)\b`
/// alternation, so a per-alternative `\b<literal>\b` scan is equivalent.
const CHANGE_KEYWORDS: &[&str] = &[
    "fix",
    "fixes",
    "fixed",
    "fixing",
    "add",
    "adds",
    "added",
    "adding",
    "change",
    "changes",
    "changed",
    "changing",
    "update",
    "updates",
    "updated",
    "updating",
    "modify",
    "modifies",
    "modified",
    "modifying",
    "implement",
    "implements",
    "implemented",
    "implementing",
    "create",
    "creates",
    "created",
    "creating",
    "build",
    "builds",
    "building",
    "buildt",
    "rewrite",
    "rewrites",
    "rewriting",
    "refactor",
    "refactors",
    "refactored",
    "refactoring",
    "edit",
    "edits",
    "edited",
    "editing",
    "delete",
    "deletes",
    "deleted",
    "deleting",
    "remove",
    "removes",
    "removed",
    "removing",
    "rename",
    "renames",
    "renamed",
    "renaming",
    "patch",
    "patches",
    "patched",
    "patching",
    "improve",
    "improves",
    "improved",
    "improving",
    "convert",
    "converts",
    "converted",
    "converting",
    "migrate",
    "migrates",
    "migrated",
    "migrating",
    "extend",
    "extends",
    "extended",
    "extending",
    "integrate",
    "integrates",
    "integrated",
    "integrating",
    "replace",
    "replaces",
    "replaced",
    "replacing",
    "tweak",
    "tweaks",
    "tweaked",
    "tweaking",
    "adjust",
    "adjusts",
    "adjusted",
    "adjusting",
    "broken",
    "missing",
    "wrong",
    "lets",
    "let's",
    "please",
    "i want",
    "id like",
    "i'd like",
    "can you",
    "could you",
    "make it",
    "make the",
    "make a",
    "make an",
    "should be",
    "should have",
    "should use",
];

fn looks_like_change_request(prompt: &str) -> bool {
    if prompt.trim().chars().count() < 30 {
        return false;
    }
    let lower = prompt.to_lowercase();
    let c: Vec<char> = lower.chars().collect();
    for i in 0..c.len() {
        if !word_start(&c, i) {
            continue;
        }
        for kw in CHANGE_KEYWORDS {
            if let Some(e) = lit(&c, i, kw) {
                if word_end(&c, e) {
                    return true;
                }
            }
        }
    }
    false
}

// --- the codex pipeline -------------------------------------------------------

fn codex_decision(payload: &Value) -> ShadowVerdict {
    let cwd = payload_cwd(payload);
    let event = str_field(payload, "hook_event_name");

    if event == "UserPromptSubmit" {
        return codex_user_prompt_branch(payload, &cwd);
    }

    // PreToolUse path: the managed-repo marker is required, and unlike the
    // Claude guard there is no walk — cwd is the root or nothing is.
    if !state_path(&cwd, AUTH_LEAF).exists() {
        return codex_allow("passed-checks", "-");
    }

    let tool_name = str_field(payload, "tool_name");
    let ti = tool_input(payload);
    let covered = worklist_covered_files(&cwd);

    if tool_name == "apply_patch" {
        return codex_apply_patch_branch(&cwd, &ti, &covered);
    }
    if tool_name == "Bash" {
        return codex_bash_branch(&cwd, &ti, &covered);
    }
    if tool_name.starts_with("mcp__") {
        return codex_mcp_branch(&cwd, &tool_name, &ti, &covered);
    }
    codex_allow("passed-checks", "-")
}

/// The Python guard WRITES a `direct-edit` authorization record on the opt-out
/// path (`write_direct_edit_record`). The shadow is read-only by contract, so
/// only the decision is reproduced; the record is deliberately not written and
/// the reason is unchanged either way (`passed-checks`). The reminder-injection
/// ending emits no `[worklist-guard]` line at all, so it records here with the
/// `no-trace:` prefix the Claude port uses for the same situation.
fn codex_user_prompt_branch(payload: &Value, cwd: &Path) -> ShadowVerdict {
    if !state_path(cwd, AUTH_LEAF).exists() {
        return codex_allow("passed-checks", "-");
    }
    let prompt = str_field(payload, "prompt");
    if has_opt_out(&prompt) {
        return codex_allow("passed-checks", "-");
    }
    if !looks_like_change_request(&prompt) {
        return codex_allow("passed-checks", "-");
    }
    codex_allow("no-trace:gate-reminder", "-")
}

/// Best-effort trace target, mirroring `_HOOK_CTX["target"]`. The breadcrumb's
/// target field must stay whitespace-free, so a path with a space writes `-`.
fn codex_target(rel: &str) -> String {
    if rel.is_empty() || rel.chars().any(|c| c.is_whitespace()) {
        "-".to_string()
    } else {
        rel.to_string()
    }
}

fn codex_apply_patch_branch(
    cwd: &Path,
    tool_input: &Value,
    covered: &HashSet<String>,
) -> ShadowVerdict {
    let raw_targets = patch_targets(tool_input);
    let trace_target = raw_targets
        .first()
        .map(|t| codex_normalize_target(cwd, t).unwrap_or_else(|| t.clone()))
        .map(|t| codex_target(&t))
        .unwrap_or_else(|| "-".to_string());

    if raw_targets.is_empty() {
        return codex_deny(
            &"apply_patch blocked: could not parse target file(s) from the patch \
payload. Propose the change in .bram/worklist.json first so the guard can \
verify coverage."
                .to_string(),
            "-",
        );
    }

    let touches_worklist = raw_targets
        .iter()
        .any(|t| codex_normalize_target(cwd, t).is_some_and(|r| is_worklist_rel(cwd, &r)));
    if touches_worklist {
        let patch_body = patch_text(tool_input);
        match worklist_content_after_apply_patch(cwd, &patch_body) {
            Some(new_content) => {
                if let Some(bad) = worklist_items_violating_draft_only(&new_content) {
                    if !bad.is_empty() {
                        return codex_deny(
                            &String::from(worklist_validation_error(&bad, "apply_patch")),
                            &trace_target,
                        );
                    }
                }
                let old_text = current_worklist_text(cwd);
                let (removed, status_changed) = worklist_state_changes(
                    &codex_items_by_id(&old_text),
                    &codex_items_by_id(&new_content),
                );
                if !removed.is_empty() || !status_changed.is_empty() {
                    return codex_deny(
                        &String::from(mechanical_worklist_change_error(
                            &removed,
                            &status_changed,
                            "apply_patch",
                        )),
                        &trace_target,
                    );
                }
                let (old_has, old_version) = worklist_version_from_text(&old_text);
                let (new_has, new_version) = worklist_version_from_text(&new_content);
                if old_has && (!new_has || new_version != old_version + 1) {
                    return codex_deny(
                        &String::from(stale_worklist_version_error(
                            old_version,
                            new_version,
                            new_has,
                            "apply_patch",
                        )),
                        &trace_target,
                    );
                }
            }
            None => {
                let removed = patch_removes_worklist_items(cwd, &patch_body);
                if !removed.is_empty() {
                    return codex_deny(
                        &String::from(mechanical_worklist_change_error(
                            &removed,
                            &[],
                            "apply_patch",
                        )),
                        &trace_target,
                    );
                }
                let bad = patch_adds_violating_draft_only(&patch_body);
                if !bad.is_empty() {
                    return codex_deny(
                        &String::from(worklist_validation_error(&bad, "apply_patch")),
                        &trace_target,
                    );
                }
            }
        }
    }

    let mut violations: Vec<String> = Vec::new();
    let mut denied_worktree: Option<String> = None;
    for t in &raw_targets {
        let Some(rel) = codex_normalize_target(cwd, t) else {
            continue; // outside the project tree
        };
        if is_worklist_rel(cwd, &rel)
            || is_worklist_draft(cwd, &rel)
            || is_worklist_citation(cwd, &rel)
        {
            continue;
        }
        if state_leaf(cwd, &rel) == Some(WORKLIST_RESULT_LEAF) {
            continue;
        }
        if state_leaf(cwd, &rel) == Some(WORKLIST_INTENT_LEAF) {
            let (verdict, detail) = intent_write_verdict(cwd);
            if verdict == "deny" {
                return codex_deny(
                    &String::from(intent_pending_message("apply_patch", &detail)),
                    &trace_target,
                );
            }
            // `intent-stale-overwrite` traces here in Python but is
            // non-terminal, so it is not the breadcrumb's decision.
            continue;
        }
        let (is_covered, worktree) = coverage_verdict(covered, &rel);
        if is_covered || codex_fresh_bypass(cwd, &rel) {
            if is_covered {
                trace_would_deny_unaddressed(cwd, "codex-rs", "apply_patch", &rel);
            }
            continue;
        }
        if denied_worktree.is_none() {
            denied_worktree = worktree.map(str::to_string);
        }
        violations.push(rel);
    }
    if !violations.is_empty() {
        let bad = violations.join(", ");
        let message = format!(
            "apply_patch blocked: {} is not covered by any proposed or applied \
item in {}, and no fresh direct-edit authorization covers \
it. Propose the change in the worklist first (status: 'proposed'), wait for the \
user's approved: payload, then retry.{}",
            bad,
            state_rel(cwd, WORKLIST_LEAF),
            coverage_root_line(cwd)
        );
        return with_worktree_marker(
            codex_deny(&message, &trace_target),
            denied_worktree.as_deref(),
        );
    }
    codex_allow("passed-checks", &trace_target)
}

fn codex_bash_branch(cwd: &Path, tool_input: &Value, covered: &HashSet<String>) -> ShadowVerdict {
    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if is_gh_body_at_antipattern(&command) {
        // Python appends `Detected: <regex match>` as a third line. The traced
        // reason is line 1 only, so the match text is not reconstructed here.
        return codex_deny(
            "gh --body takes a literal string, not stdin or a file reference.\n\
Use --body-file - (stdin) or --body-file <path> instead.\nDetected: <match>",
            "-",
        );
    }
    let (cb_verdict, cb_detail) = crossboundary_signature_verdict(&command, cwd);
    // #331: same split as the Claude site — codex_deny's reason is the first
    // message line, so the distinct first line is what makes the trace
    // distinguish unreadable from unsigned.
    if cb_verdict == "unparsed"
        && !matches!(
            cb_detail,
            "body-file-stdin" | "no-body-flag" | "input-file-stdin" | "input-json-no-body"
        )
    {
        let path = body_file_arg(&command)
            .or_else(|| input_file_arg(&command))
            .or_else(|| forge_positional_body_file(&command))
            .unwrap_or_else(|| "<unresolved path>".to_string());
        return codex_deny(
            &format!(
                "The body file for this forge write could not be read, so its signature could not be verified.\n    {}\nAn unreadable named body is denied rather than silently bypassed (conventions.md, 'Signing agent-authored forge artifacts'). Check the path and retry — quoted and MSYS-style paths are accepted.",
                path
            ),
            "-",
        );
    }
    if cb_verdict == "unsigned" {
        // Same split as the Claude site — shared builder (issue-365) so the
        // two deny sites cannot drift on the locator-teaching line.
        return codex_deny(&crossboundary_unsigned_message(cwd), "-");
    }
    // Same placement as the Claude site, shared message builder so the two
    // cannot drift.
    let (ver_verdict, ver_signed, ver_expected) = signature_version_verdict(&command, cwd);
    if ver_verdict == "mismatch" {
        return codex_deny(
            &stale_signature_version_message(&ver_signed, &ver_expected),
            "-",
        );
    }
    // forge-body-fold-enforced: same placement and message as the Claude site.
    // The traced reason is the stable code, not the message's first line, so
    // `reason=long-forge-body-unfolded` greps the same on both providers.
    let (fold_verdict, fold_lines, fold_chars) = forge_body_fold_verdict(&command, cwd);
    if fold_verdict == "unfolded" {
        let mut v = deny("long-forge-body-unfolded", "-");
        v.message = long_forge_body_unfolded_message(fold_lines, fold_chars);
        return v;
    }
    let (sha_verdict, sha_detail) = forge_sha_verdict(&command, cwd);
    if sha_verdict == "bad" {
        return codex_deny(
            &format!(
                "The body contains a full commit SHA that does not resolve in this \
repository:\n    {}\nA fabricated or rebase-orphaned SHA renders as an ordinary \
link and 404s silently. Resolve the real hash with `git rev-parse <short-sha>` \
and retry. See judell/bram#277.",
                sha_detail
            ),
            "-",
        );
    }
    if sha_verdict == "unpushed" {
        return codex_deny(&forge_sha_unpushed_message(&sha_detail), "-");
    }

    let (su_verdict, su_where) = session_url_verdict(&command, cwd);
    if su_verdict == "deny" {
        return codex_deny(
            &format!(
                "This commit or forge write carries an agent session URL or Claude-Session \
                 trailer (found in: {}). Session links are private telemetry; publishing \
                 them is irreversible and no opt-in exists. Remove the trailer/URL and \
                 retry. See conventions.md, 'No session URLs in public artifacts'.",
                su_where
            ),
            "-",
        );
    }

    // #299 (case 1): a pure forge read is a read. Unlike the Claude guard this
    // side has no `gh issue <verb>` write pattern, so the forge verbs were
    // never captured here and this changes no verdict — it is pinned so a
    // future widening of the write patterns cannot silently recapture them.
    if forge_read_command(&command) && !codex_bash_writes(&command) {
        return codex_allow("passed-checks", "-");
    }
    if !codex_bash_writes(&command) {
        // #299 (case 3) rides in through the quote-aware `has_redirect`.
        if has_redirect(&command) && !redirect_is_write(&command) {
            return codex_allow("bash-write-nonrepo-target", "-");
        }
        return codex_allow("passed-checks", "-");
    }
    if command.contains(&state_rel(cwd, WORKLIST_DRAFTS_LEAF)) {
        return codex_allow("passed-checks", "-");
    }
    if command.contains(".worklist-intent.json") {
        let (verdict, detail) = intent_write_verdict(cwd);
        if verdict == "deny" {
            return codex_deny(&String::from(intent_pending_message("Bash", &detail)), "-");
        }
        if detail == IntentDetail::StaleOverwrite {
            return codex_allow("intent-stale-overwrite", "-");
        }
        return codex_allow("passed-checks", "-");
    }
    if !covered.is_empty() || codex_fresh_bypass(cwd, "*") {
        // issue-387: same coarse-allow refinement as the Claude Bash branch.
        trace_bash_would_deny(cwd, "codex-rs", &command, covered, |raw| {
            codex_normalize_target(cwd, raw)
        });
        return codex_allow("passed-checks", "-");
    }
    if push_cmd(&command) && post_commit_push_grace(cwd) {
        return codex_allow("post-commit-push-grace", "-");
    }
    let mut message = format!(
        "Bash blocked: this command writes to the filesystem, and \
{} has no proposed or applied items covering the change. \
Propose the work in the worklist first, or have the user issue a direct-edit \
authorization.{}",
        state_rel(cwd, WORKLIST_LEAF),
        coverage_root_line(cwd)
    );
    if push_cmd(&command) {
        message.push_str(
            "\n  - This looks like a push. The user can click Push in the Commits \
tab; an agent push is allowed only in the 10-minute window after a gate commit \
(see judell/bram#283).",
        );
    }
    codex_deny(&message, "-")
}

fn codex_mcp_branch(
    cwd: &Path,
    tool_name: &str,
    tool_input: &Value,
    covered: &HashSet<String>,
) -> ShadowVerdict {
    if !mcp_is_mutation(tool_name) {
        return codex_allow("passed-checks", "-");
    }
    let candidates = mcp_candidate_paths(tool_input, cwd, |s| codex_normalize_target(cwd, s));
    if candidates.is_empty() {
        // judell/bram#384: same fix as the Claude branch above -- a
        // write-verb NAME with nothing in tool_input that structurally
        // resolves to a repo path is out of scope regardless of the name.
        // This replaces #360's fail-closed-with-bypass-first default (Codex
        // has no transcript opt-out at PreToolUse by design, so that side
        // never applied here anyway). See `trace_mcp_write_verb_no_candidates`
        // for the one residual this accepts and how it stays observable.
        trace_mcp_write_verb_no_candidates(cwd, "codex-rs", tool_name);
        return codex_allow("mcp-no-candidate-paths", "-");
    }
    let touches_worklist = candidates.iter().any(|rel| is_worklist_rel(cwd, rel));
    if touches_worklist && tool_input.is_object() {
        let Some(new_content) = worklist_new_content_from_tool_input(cwd, tool_input) else {
            return codex_deny(
                &format!(
                    "{} blocked: worklist edits that advance status or prune items must \
use `/__worklist/mutate`. For direct authoring/refinement edits, use a write/edit \
shape whose resulting content the guard can inspect.",
                    tool_name
                ),
                "-",
            );
        };
        if !new_content.trim().is_empty() {
            if let Some(bad) = worklist_items_violating_draft_only(&new_content) {
                if !bad.is_empty() {
                    return codex_deny(
                        &String::from(worklist_validation_error(&bad, tool_name)),
                        "-",
                    );
                }
            }
            let old_text = current_worklist_text(cwd);
            let (removed, status_changed) = worklist_state_changes(
                &codex_items_by_id(&old_text),
                &codex_items_by_id(&new_content),
            );
            if !removed.is_empty() || !status_changed.is_empty() {
                return codex_deny(
                    &String::from(mechanical_worklist_change_error(
                        &removed,
                        &status_changed,
                        tool_name,
                    )),
                    "-",
                );
            }
            let (old_has, old_version) = worklist_version_from_text(&old_text);
            let (new_has, new_version) = worklist_version_from_text(&new_content);
            if old_has && (!new_has || new_version != old_version + 1) {
                return codex_deny(
                    &String::from(stale_worklist_version_error(
                        old_version,
                        new_version,
                        new_has,
                        tool_name,
                    )),
                    "-",
                );
            }
        }
    }
    let mut violations: Vec<String> = Vec::new();
    let mut denied_worktree: Option<String> = None;
    for rel in candidates.iter().cloned() {
        if is_worklist_rel(cwd, &rel)
            || is_worklist_draft(cwd, &rel)
            || is_worklist_citation(cwd, &rel)
            || is_coordination_file(cwd, &rel)
        {
            continue;
        }
        let (is_covered, worktree) = coverage_verdict(covered, &rel);
        if is_covered || codex_fresh_bypass(cwd, &rel) {
            if is_covered {
                trace_would_deny_unaddressed(cwd, "codex-rs", tool_name, &rel);
            }
            continue;
        }
        if denied_worktree.is_none() {
            denied_worktree = worktree.map(str::to_string);
        }
        violations.push(rel);
    }
    if !violations.is_empty() {
        let message = format!(
            "{} blocked: {} is not covered by any proposed or applied item in \
{}, and no fresh direct-edit authorization covers it. \
Propose the change in the worklist first, wait for the user's approved: \
payload, then retry.{}",
            tool_name,
            violations.join(", "),
            state_rel(cwd, WORKLIST_LEAF),
            coverage_root_line(cwd)
        );
        return with_worktree_marker(codex_deny(&message, "-"), denied_worktree.as_deref());
    }
    codex_allow("passed-checks", "-")
}

// --- parity suite -------------------------------------------------------------
//
// A port of `self_test()` in app/provider-hooks/claude-worklist-guard.py
// (lines 1204-1574). Every Python assertion has a twin here, in source order.
// The judell/bram#299 cases assert the NEW classification and are labelled.

#[cfg(test)]
mod guard_policy_tests {
    use super::*;

    // --- ported from the retired scripts/tests/test_provider_hook_encoding.py,
    // whose subject (the Python guards) went away with them. The behaviours it
    // pinned are still ours to keep. ---

    #[test]
    fn session_urls_deny_in_publishing_commands_only() {
        let cwd = std::env::temp_dir();
        // Inline -m trailer: the ~/budget shape.
        let (v, w) = session_url_verdict(
            "git commit -m \"Fix\n\nClaude-Session: https://claude.ai/code/session_01Eau\"",
            &cwd,
        );
        assert_eq!((v, w.as_str()), ("deny", "command-text"));
        // Forge write with the URL in the command line.
        let (v, _) = session_url_verdict(
            "gh issue comment 5 --body \"see https://claude.ai/code/session_abc\"",
            &cwd,
        );
        assert_eq!(v, "deny");
        // A grep mentioning the URL is a read — never captured.
        let (v, _) = session_url_verdict("grep -r claude.ai/code/ .bram/", &cwd);
        assert_eq!(v, "skip");
        // Clean commit passes.
        let (v, _) = session_url_verdict("git commit -m \"Honest message\"", &cwd);
        assert_eq!(v, "skip");
        // Message file carrying the trailer: read and denied, naming the file.
        let f = cwd.join(format!("bram-su-test-{}.txt", std::process::id()));
        std::fs::write(
            &f,
            "Subject\n\nClaude-Session: https://claude.ai/code/session_x\n",
        )
        .unwrap();
        let (v, w) = session_url_verdict(&format!("git commit -F {}", f.to_string_lossy()), &cwd);
        assert_eq!(v, "deny");
        assert!(w.ends_with(".txt"));
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn git_commit_detection_is_segment_scoped() {
        assert!(is_git_commit_command("git commit -m x"));
        assert!(is_git_commit_command("cd /x && git -C /y commit -F msg"));
        assert!(!is_git_commit_command("git log --oneline | head"));
        assert!(!is_git_commit_command("echo commit"));
    }

    #[test]
    fn opt_out_survives_an_undecodable_transcript() {
        // The Python guard read the transcript with errors="replace" because a
        // corrupt byte must not take the guard down. Rust's read_to_string
        // returns Err on invalid UTF-8, which silently yielded empty text and
        // disabled opt-out matching entirely.
        let dir = std::env::temp_dir().join(format!("bram-enc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let tp = dir.join("corrupt.jsonl");
        let mut bytes = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": "please fix it just do it"}
        })
        .to_string()
        .into_bytes();
        bytes.push(b'\n');
        // A lone 0xFF is valid in no UTF-8 sequence.
        bytes.extend_from_slice(&[0xFF, 0xFE, b'\n']);
        std::fs::write(&tp, &bytes).unwrap();

        let text = last_user_text(tp.to_str().unwrap());
        assert!(
            has_opt_out(&text),
            "opt-out must survive undecodable bytes; got {text:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_ascii_transcript_reads_like_its_ascii_equivalent() {
        // Drafts are full of em-dashes; a non-ASCII transcript must behave
        // exactly as an ASCII one does.
        let dir = std::env::temp_dir().join(format!("bram-enc2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let tp = dir.join("spicy.jsonl");
        std::fs::write(
            &tp,
            serde_json::json!({
                "type": "user",
                "message": {"role": "user", "content": "café — naïve 日本語 just do it"}
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        assert!(has_opt_out(&last_user_text(tp.to_str().unwrap())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worklist_text_survives_undecodable_bytes() {
        // An unreadable worklist would otherwise present as "no items", i.e.
        // no coverage for any path, denying every edit with a wrong reason.
        let dir = std::env::temp_dir().join(format!("bram-enc3-{}", std::process::id()));
        let _ = crate::guard_policy::test_make_state_dir(&dir);
        let mut bytes =
            br#"{"items":[{"id":"a","status":"proposed","files":["src/x.rs"]}],"#.to_vec();
        bytes.extend_from_slice(&[0xFF]);
        bytes.extend_from_slice(br#""version":1}"#);
        std::fs::write(dir.join(WORKLIST_REL), &bytes).unwrap();
        let text = current_worklist_text(&dir);
        assert!(!text.is_empty(), "must not read as an empty worklist");
        assert!(text.contains("src/x.rs"), "content must survive: {text:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // issue-431-opt-out-in-outbound-turn
    #[test]
    fn opt_out_in_an_outbound_turn_file_counts() {
        let root = scratch("opt-out-outbound");
        let turns = root.join(".bram").join("outbound-turns");
        std::fs::create_dir_all(&turns).unwrap();
        let write_turn = |id: &str, text: &str| {
            std::fs::write(
                turns.join(format!("{}.json", id)),
                serde_json::json!({ "schema": 1, "id": id, "kind": "message", "mode": "", "text": text })
                    .to_string(),
            )
            .unwrap();
        };
        let transcript = root.join("t.jsonl");
        let payload_for = |frame: &str| {
            std::fs::write(
                &transcript,
                serde_json::json!({ "type": "user", "message": { "content": frame } }).to_string(),
            )
            .unwrap();
            serde_json::json!({ "transcript_path": transcript.to_string_lossy() })
        };
        // #431's message, verbatim.
        write_turn(
            "1791677856169-turn",
            "number the arrows so we see a record of the sequence\n\n7 and 8 should collapse into one step.\n\njust do it.",
        );
        let frame = "Read and follow this Bram turn: @.bram/outbound-turns/1791677856169-turn.json (end of Bram turn)";
        assert_eq!(
            opt_out_clears(&root, &payload_for(frame)).as_deref(),
            Some(frame)
        );
        // A wrapped message without the phrase does not opt out.
        write_turn("2-turn", "number the arrows\n\nthanks");
        assert!(opt_out_clears(
            &root,
            &payload_for("Read and follow this Bram turn: @.bram/outbound-turns/2-turn.json (end of Bram turn)")
        )
        .is_none());
        // A frame that tries to leave the directory is ignored.
        assert_eq!(
            outbound_turn_text(
                &root,
                "Read and follow this Bram turn: @.bram/outbound-turns/../x-turn.json"
            ),
            ""
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // guard-coverage-requires-begun-item
    #[test]
    fn would_deny_reason_separates_unstarted_proposals_from_begun_items() {
        let root = scratch("would-deny-reason");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::write(
            root.join(WORKLIST_REL),
            serde_json::json!({ "items": [
                { "id": "unstarted", "status": "proposed", "files": ["a.js"] },
                { "id": "begun", "status": "proposed", "begunAtMs": 1, "files": ["b.js"] },
                { "id": "done", "status": "applied", "files": ["c.js"] },
            ]})
            .to_string(),
        )
        .unwrap();
        assert_eq!(would_deny_reason(&root, "a.js"), "proposed-only-coverage");
        assert_eq!(would_deny_reason(&root, "b.js"), "general-turn-item-edit");
        assert_eq!(would_deny_reason(&root, "c.js"), "general-turn-item-edit");
        let ids = begun_item_ids(&root);
        assert!(ids.contains("begun") && ids.contains("done") && !ids.contains("unstarted"));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bram-guard-policy-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    // --- bash write / read corpus (python lines 1205-1249) ------------------

    #[test]
    fn bash_write_commands_are_writes() {
        let write_commands = [
            "echo x > out.txt",
            "printf x >> out.txt",
            "printf x | tee out.txt",
            "sed -i '' s/a/b/ file.txt",
            "perl -i -pe s/a/b/ file.txt",
            "python -c \"open('x', 'w').write('x')\"",
            "node -e \"require('fs').writeFileSync('x','x')\"",
            // #362: the conservative refusals the body inspector keeps —
            // expansion means the guard's text is not what node runs.
            "node -e \"console.log($HOME)\"",
            "node -e 'fetch(\"https://x.test\")'",
            "git commit -m test",
            "git push",
            "gh issue close 119 --repo judell/bram",
            "rm stale.txt",
            "mv a b",
            "cp a b",
            "truncate -s 0 file.txt",
            "bash -c 'echo x > out.txt'",
            "sh -c 'echo x > out.txt'",
            "echo hi > /dev/stdout",
            "cat > src-tauri/src/lib.rs",
            "cat > /tmp/b.md && git commit -m x",
        ];
        for c in write_commands {
            assert!(bash_writes(c), "{c}");
        }
    }

    #[test]
    fn bash_read_commands_are_reads() {
        let read_commands = [
            "ls -la",
            "rg Bash app/provider-hooks/claude-worklist-guard.py",
            "git status --short",
            // #362: Walt's confirmed false-positive class — pure-expression
            // node -e one-liners read nothing and print.
            "node -e \"console.log(1+1)\"",
            "node -e 'console.log(JSON.stringify({a: 1}))'",
            "git stash list",
            "git status --porcelain | grep modified",
            "gh issue view 119 --repo judell/bram",
            "python --version",
            "curl -I https://example.com",
            "git ls-files x >/dev/null 2>&1",
            "echo hi > /dev/null",
            "echo hi >> /dev/zero",
            "cat > /tmp/body.md",
            "cat > /private/tmp/x/body.md",
        ];
        for c in read_commands {
            assert!(!bash_writes(c), "{c}");
        }
    }

    // --- judell/bram#299: three deliberate divergences ----------------------

    #[test]
    fn issue_299_pure_forge_reads_are_reads() {
        // #299 case 1. Python's own classifier already lets the bare forms
        // through; these pin it, and forge_read_command makes it explicit.
        for c in [
            "gh issue list",
            "gh issue list --state all --json number,title,state,updatedAt --limit 200",
            "gh issue view 299 --json title,body",
            "glab issue list --state opened",
            "gh pr list --json number,title",
        ] {
            assert!(!bash_writes(c), "{c}");
            assert!(forge_read_command(c), "{c}");
        }
        // A jq comparison filter is the shape that actually got denied.
        let jq = "gh issue list --state all --json number --jq '.[]|select(.number > 200)'";
        assert!(!bash_writes(jq), "{jq}");
        // Still not a blanket exemption: a chained write is still a write.
        assert!(bash_writes("gh issue list && git commit -m x"));
    }

    #[test]
    fn issue_299_inline_python_c_reads_are_reads() {
        // #299 case 2: keyed on what the inline script does, not on `-c`.
        for c in [
            "python -c \"print(1)\"",
            "python3 -c 'import sys; print(sys.version)'",
            "python3 -c \"import json,sys; d=json.loads(sys.stdin.read()); print(len(d))\"",
        ] {
            assert!(!bash_writes(c), "{c}");
        }
        // Anything that touches a file, or that cannot be read, stays a write.
        for c in [
            "python -c \"open('x', 'w').write('x')\"",
            "python3 -c \"import shutil; shutil.rmtree('x')\"",
            "python3 -c \"import os; os.remove('x')\"",
            "python3 -c $SCRIPT",
        ] {
            assert!(bash_writes(c), "{c}");
        }
    }

    #[test]
    fn issue_299_quoted_comparison_is_not_a_redirect() {
        // #299 case 3: `>` / `>=` inside quotes is a comparison, not a redirect.
        for c in [
            "awk '$1 >= \"2026-08-28T06:16\"' file.txt",
            "sort -k1 f | awk '$2 > \"3\"'",
            "jq '.[] | select(.n > 5)' data.json",
            "grep -E \"a > b\" file",
        ] {
            assert!(!bash_writes(c), "{c}");
            assert!(!has_redirect(c), "{c}");
        }
        // Unquoted redirects still register.
        assert!(has_redirect("echo x > out.txt"));
        assert_eq!(redirect_targets("echo x > out.txt"), vec!["out.txt"]);
        assert_eq!(
            redirect_targets("cat > \"my file.txt\""),
            vec!["my file.txt"]
        );
    }

    // --- Bash write-target extraction (issue-387) ----------------------------

    fn bash_paths(command: &str) -> Vec<String> {
        match bash_write_targets(command) {
            BashWriteTarget::Paths(p) => p,
            BashWriteTarget::Unknown => panic!("expected Paths for: {command}"),
        }
    }

    fn bash_unknown(command: &str) -> bool {
        matches!(bash_write_targets(command), BashWriteTarget::Unknown)
    }

    #[test]
    fn bash_write_targets_redirects() {
        assert_eq!(bash_paths("echo hi > out.txt"), vec!["out.txt"]);
        assert_eq!(bash_paths("echo hi >> out.txt"), vec!["out.txt"]);
        assert_eq!(
            bash_paths("cat file1 > app/covered.js"),
            vec!["app/covered.js"]
        );
        assert_eq!(bash_paths("echo hi > \"my file.txt\""), vec!["my file.txt"]);
    }

    #[test]
    fn bash_write_targets_heredoc_masks_body_but_keeps_intro_target() {
        // The `>` on the introducer line is a real target; a `>` mentioned
        // inside the heredoc BODY (e.g. example shell text in a drafted
        // commit message) must not also be read as one.
        let cmd = "cat > app/covered.js <<'EOF'\nsee: echo hi > somewhere-else.txt\nEOF\n";
        assert_eq!(bash_paths(cmd), vec!["app/covered.js"]);
    }

    #[test]
    fn bash_write_targets_tee() {
        assert_eq!(bash_paths("echo hi | tee out.txt"), vec!["out.txt"]);
        assert_eq!(
            bash_paths("echo hi | tee -a out.txt"),
            vec!["out.txt"],
            "the -a append flag is not a target"
        );
        assert_eq!(
            bash_paths("echo hi | tee a.txt b.txt"),
            vec!["a.txt", "b.txt"]
        );
        assert!(bash_unknown("echo hi | tee -"), "lone '-' names no file");
    }

    #[test]
    fn bash_write_targets_unknown_on_expansion() {
        // A variable or command-substitution target cannot be trusted as the
        // literal path the shell will write -- decline rather than guess.
        assert!(bash_unknown("echo hi > \"$OUT\""));
        assert!(bash_unknown("echo hi > $OUT"));
        assert!(bash_unknown("echo hi > \"$(mktemp)\""));
        assert!(bash_unknown("echo hi | tee \"$OUT\""));
    }

    #[test]
    fn bash_write_targets_unknown_for_shapes_outside_tractable_subset() {
        // These classify as writes (via write_patterns_token) but use shapes
        // this extractor does not attempt -- sed -i, rm, git, python -c.
        for c in [
            "sed -i 's/a/b/' file.txt",
            "rm file.txt",
            "git commit -am x",
            "python -c \"open('f','w').write('x')\"",
        ] {
            assert!(bash_writes(c), "expected a write: {c}");
            assert!(bash_unknown(c), "expected Unknown: {c}");
        }
    }

    #[test]
    fn bash_write_targets_dedupes() {
        assert_eq!(
            bash_paths("echo a > out.txt; echo b >> out.txt"),
            vec!["out.txt"]
        );
    }

    // --- forge_issue_only_write (python lines 1251-1272) --------------------

    #[test]
    fn forge_issue_only_exemption() {
        assert!(forge_issue_only_write("gh issue comment 5 --body-file f"));
        assert!(forge_issue_only_write("gh issue close 5"));
        assert!(forge_issue_only_write("gh issue reopen 5"));
        assert!(forge_issue_only_write(
            "gh issue edit 5 --title \"new title\""
        ));
        assert!(forge_issue_only_write("gh issue create --title x --body y"));
        assert!(!forge_issue_only_write(
            "gh issue comment 5 --body-file f && git commit -m x"
        ));
        assert!(!forge_issue_only_write("gh issue delete 5"));
        assert!(!forge_issue_only_write("gh issue transfer 5 other/repo"));
        assert!(!forge_issue_only_write("gh issue pin 5"));
        assert!(!forge_issue_only_write("gh issue lock 5"));
        assert!(!forge_issue_only_write("gh issue view 5"));
        assert!(!forge_issue_only_write("git commit -m x"));
    }

    // --- MCP surface (python lines 1274-1291) -------------------------------

    #[test]
    fn mcp_mutation_recognition() {
        for name in [
            "mcp__filesystem__write_file",
            "mcp__filesystem__edit_file",
            "mcp__filesystem__move_file",
            "mcp__filesystem__create_directory",
        ] {
            assert!(mcp_is_mutation(name), "{name}");
        }
        for name in [
            "mcp__filesystem__read_text_file",
            "mcp__filesystem__list_directory",
            "mcp__xmlui__xmlui_search_howto",
            "mcp__xmlui__xmlui_component_docs",
        ] {
            assert!(!mcp_is_mutation(name), "{name}");
        }
    }

    // --- opt-out reachability (python lines 1293-1328) ----------------------

    fn transcript(dir: &Path, tag: &str, text: &str) -> String {
        let p = dir.join(format!("t-{}.jsonl", tag));
        let mut body = serde_json::json!({
            "type": "user",
            "message": {"content": [{"type": "text", "text": text}]},
        })
        .to_string();
        body.push('\n');
        body.push_str(
            &serde_json::json!({
                "type": "user",
                "message": {"content": [{"type": "tool_result", "text": ""}]},
            })
            .to_string(),
        );
        body.push('\n');
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn opt_out_phrase_is_reachable_from_every_surface() {
        let td = scratch("optout");
        let t1 = transcript(&td, "a", "patch it, just do it");
        assert_eq!(last_user_text(&t1), "patch it, just do it");
        assert!(has_opt_out(&last_user_text(&t1)));
        let t2 = transcript(&td, "b", "skip the worklist");
        assert!(!has_opt_out(&last_user_text(&t2)));
        assert!(!has_opt_out(&last_user_text("")));

        let t3 = transcript(&td, "c", "do the build, just do it");
        assert!(opt_out_clears(&td, &serde_json::json!({"transcript_path": t3})).is_some());
        let t4 = transcript(&td, "d", "please do the build");
        assert!(opt_out_clears(&td, &serde_json::json!({"transcript_path": t4})).is_none());
        assert!(opt_out_clears(&td, &serde_json::json!({})).is_none());
        let _ = std::fs::remove_dir_all(&td);
    }

    // --- signature gates (python lines 1330-1381) ---------------------------

    // #333: read-only listing forms of mutation-capable git verbs, and write
    // tokens inside quoted argument data. The adversarial half is the point:
    // every mutating neighbour must keep denying, and quoted-but-executed
    // interpreter bodies must stay classified as writes.
    #[test]
    fn bash_classifier_git_listing_forms_and_quoting() {
        // Case A: listing forms are read-only…
        for cmd in [
            "git tag --list",
            "git tag -l \"v0.6.*\"",
            "git tag --list | tail -20 && git status -sb && git log --oneline",
            "git stash list",
            "git stash show",
            "git branch",
            "git branch -a",
            "git branch --list \"issue-*\"",
        ] {
            assert!(!bash_writes(cmd), "should be read-only: {}", cmd);
        }
        // …and every mutating neighbour still denies.
        for cmd in [
            "git tag v1.0",
            "git tag -a v1.0 -m x",
            "git stash",
            "git stash pop",
            "git branch -d old",
            "git branch new-branch",
            "git branch --list && git push",
            "git commit -m \"x\"",
        ] {
            assert!(bash_writes(cmd), "must stay a write: {}", cmd);
        }

        // Case B: write tokens inside quoted argument data are data.
        for cmd in [
            "grep -n \"GIT_INDEX_FILE\\|git apply\\|--check\\|temp_dir\" src-tauri/src/lib.rs | head -40",
            "grep 'rm -rf' notes.md",
            "echo \"git push origin\"",
            "rg \"git checkout\" docs/",
        ] {
            assert!(!bash_writes(cmd), "quoted pattern is data: {}", cmd);
        }
        // Quoted-but-executed stays a write: the interpreter FLAG classifies.
        for cmd in [
            "sh -c \"rm -rf x\"",
            "bash -c 'git tag v1'",
            "python3 -c 'open(\"f\",\"w\").write(\"x\")'",
            "rm \"some file.txt\"",
            "git commit -m \"just listing things: git tag --list\"",
        ] {
            assert!(bash_writes(cmd), "must stay a write: {}", cmd);
        }
        // Unterminated quote: masking declines, flat scan rules — over-deny.
        // (`|` is a word boundary; a bare `"` never was, which is why the
        // token needs the pipe in front to demonstrate the fallback.)
        assert!(bash_writes("grep \"x\\|rm -rf notes.md"));
        assert!(!bash_writes("grep \"x\\|rm -rf\" notes.md"));

        // The classifier names its evidence.
        assert_eq!(
            write_patterns_token("rm x", SkipPattern::None).as_deref(),
            Some("rm")
        );
        assert_eq!(
            write_patterns_token("git tag v1", SkipPattern::None).as_deref(),
            Some("git tag")
        );
        assert_eq!(
            write_patterns_token("git tag --list", SkipPattern::None),
            None
        );
    }

    // #331: the fixture gap that shipped the bug — every prior test passed a
    // bare path. Quoted paths (the idiomatic Windows spelling) and MSYS paths
    // never parsed; the guard then blamed the signature it had never read.
    #[test]
    fn body_file_quoted_and_msys_paths() {
        let td = scratch("sig331");
        let signed = "Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        let body_path = td.join("signed.md");
        std::fs::write(&body_path, signed).unwrap();

        // Double-quoted absolute path reads and verifies.
        let quoted_abs = format!("gh issue comment 5 --body-file \"{}\"", body_path.display());
        assert_eq!(
            crossboundary_signature_verdict(&quoted_abs, &td).0,
            "signed"
        );

        // Single-quoted relative path resolves against cwd.
        assert_eq!(
            crossboundary_signature_verdict("gh issue comment 5 --body-file 'signed.md'", &td).0,
            "signed"
        );

        // An unreadable named file is unparsed WITH the distinguishing
        // detail — the deny site formats this as
        // crossboundary-unparsed:body-file-unreadable, never as unsigned.
        assert_eq!(
            crossboundary_signature_verdict(
                "gh issue comment 5 --body-file \"/nope/missing.md\"",
                &td
            ),
            ("unparsed", "body-file-unreadable")
        );

        // MSYS mapping is pure and testable everywhere; call sites gate on
        // Windows. /tmp must not read as a drive.
        assert_eq!(
            msys_drive_path("/c/Users/jon/body.md").as_deref(),
            Some("C:/Users/jon/body.md")
        );
        assert_eq!(msys_drive_path("/tmp/body.md"), None);
        assert_eq!(msys_drive_path("relative/path.md"), None);

        // Quote stripping preserves backslashes byte-for-byte: unquote_shell's
        // \n unescaping would corrupt C:\Users\jon\new\b.md.
        assert_eq!(
            strip_surrounding_quotes("\"C:\\Users\\jon\\new\\b.md\""),
            "C:\\Users\\jon\\new\\b.md"
        );
        assert_eq!(strip_surrounding_quotes("'x.md'"), "x.md");
        assert_eq!(strip_surrounding_quotes("\"mismatched'"), "\"mismatched'");

        let _ = std::fs::remove_dir_all(&td);
    }

    // issue-352: a gate click writing the shared single-slot file must not
    // revoke a live direct-edit grant — the grant lives in its own sidecar.
    // Replays the Budget-project timeline: grant written, unrelated Drop
    // recorded 20s later, the authorized edit must still pass.
    #[test]
    fn direct_edit_grant_survives_gate_click_clobber() {
        let td = scratch("sig352");
        crate::guard_policy::test_make_state_dir(&td).unwrap();
        let now = now_ms() as i64;
        // The grant, in its sidecar (new-binary shape).
        std::fs::write(
            td.join(DIRECT_EDIT_REL),
            format!(
                "{{\"kind\":\"direct-edit\",\"issued_at_ms\":{},\"issuedAtMs\":{},\"paths\":[\"*\"]}}",
                now, now
            ),
        )
        .unwrap();
        // The unrelated gate click, replacing the shared slot entirely.
        std::fs::write(
            td.join(AUTH_REL),
            format!(
                "{{\"kind\":\"drop\",\"ids\":[\"belt-tightening-log\"],\"issued_at_ms\":{}}}",
                now
            ),
        )
        .unwrap();
        assert!(
            fresh_bypass(&td, "README.md"),
            "Claude arm: grant must survive"
        );
        assert!(
            codex_fresh_bypass(&td, "README.md"),
            "Codex arm: grant must survive"
        );
        // Legacy compat: a grant still in the shared file (old binary) is
        // honored when no sidecar exists.
        let _ = std::fs::remove_file(td.join(DIRECT_EDIT_REL));
        std::fs::write(
            td.join(AUTH_REL),
            format!(
                "{{\"kind\":\"direct-edit\",\"issued_at_ms\":{},\"issuedAtMs\":{},\"paths\":[\"*\"]}}",
                now, now
            ),
        )
        .unwrap();
        assert!(
            fresh_bypass(&td, "README.md"),
            "legacy location still honored"
        );
        assert!(codex_fresh_bypass(&td, "README.md"));
        // A stale sidecar grant reports stale, not absent, and does not pass.
        std::fs::write(
            td.join(DIRECT_EDIT_REL),
            format!(
                "{{\"kind\":\"direct-edit\",\"issued_at_ms\":{},\"issuedAtMs\":{},\"paths\":[\"*\"]}}",
                now - 2 * 3_600_000,
                now - 2 * 3_600_000
            ),
        )
        .unwrap();
        std::fs::write(td.join(AUTH_REL), "{\"kind\":\"none\"}").unwrap();
        assert_eq!(bypass_lookup_detail(&td, "README.md"), (false, "stale"));
        let _ = std::fs::remove_dir_all(&td);
    }

    // #347: the two extractor defects found retrofitting signatures, plus the
    // --input route they forced as a workaround.
    #[test]
    fn body_file_field_syntax_quotes_and_input() {
        let td = scratch("sig347");
        let signed = "Jon's Claude (main thread, Fable 5, macOS) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        let body_path = td.join("signed.md");
        std::fs::write(&body_path, signed).unwrap();

        // gh api field syntax: the file is AFTER the @ — previously the
        // whole `body=@/path` token was opened as the path and a readable
        // signed body was denied unreadable.
        let api_patch = format!(
            "gh api repos/judell/bram/issues/comments/1 -X PATCH -F body=@{}",
            body_path.display()
        );
        assert_eq!(crossboundary_signature_verdict(&api_patch, &td).0, "signed");

        // -F key=value with no @ names no file; the real --body-file later
        // in the command is the one verified.
        let inline_field = format!(
            "gh api repos/judell/bram/issues/comments/1 -X PATCH -F state=open --body-file {}",
            body_path.display()
        );
        assert_eq!(
            crossboundary_signature_verdict(&inline_field, &td).0,
            "signed"
        );

        // A flag-shaped substring inside a quoted argument is prose, not a
        // flag — filing #347 itself was denied on its own --title.
        let quoted_title = format!(
            "gh issue create --title \"Guard mis-parses gh api -F body=@file: prefix left\" --body-file {}",
            body_path.display()
        );
        assert_eq!(
            crossboundary_signature_verdict(&quoted_title, &td).0,
            "signed"
        );

        // Unreadable field-syntax file: denied naming the BARE path.
        assert_eq!(
            crossboundary_body(
                "gh api repos/judell/bram/issues/comments/1 -X PATCH -F body=@/nope/missing.md",
                &td
            ),
            (None, "body-file-unreadable")
        );
        assert_eq!(
            body_file_arg("gh api x -X PATCH -F body=@/nope/missing.md").as_deref(),
            Some("/nope/missing.md")
        );

        // --input: signed JSON body verifies; missing body field fails open
        // as metadata-only; unreadable and unparseable stay denied.
        let payload = td.join("payload.json");
        std::fs::write(&payload, serde_json::json!({ "body": signed }).to_string()).unwrap();
        let input_cmd = format!(
            "gh api repos/judell/bram/issues/comments/1 -X PATCH --input {}",
            payload.display()
        );
        assert_eq!(
            crossboundary_signature_verdict(&input_cmd, &td),
            ("signed", "input-json-body")
        );
        let meta_only = td.join("meta.json");
        std::fs::write(&meta_only, "{\"state\":\"closed\"}").unwrap();
        let meta_cmd = format!(
            "gh api repos/judell/bram/issues/comments/1 -X PATCH --input {}",
            meta_only.display()
        );
        assert_eq!(
            crossboundary_signature_verdict(&meta_cmd, &td),
            ("unparsed", "input-json-no-body")
        );
        assert_eq!(
            crossboundary_signature_verdict(
                "gh api repos/judell/bram/issues/comments/1 -X PATCH --input /nope/missing.json",
                &td
            ),
            ("unparsed", "input-file-unreadable")
        );
        let not_json = td.join("not.json");
        std::fs::write(&not_json, "not json at all").unwrap();
        let bad_cmd = format!(
            "gh api repos/judell/bram/issues/comments/1 -X PATCH --input {}",
            not_json.display()
        );
        assert_eq!(
            crossboundary_signature_verdict(&bad_cmd, &td),
            ("unparsed", "input-json-unparseable")
        );

        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn crossboundary_signature_gates() {
        let td = scratch("sig");
        let signed = "Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        let unsigned = "Vendored the build; the tooltip renders.";
        let verdict = |cmd: &str| crossboundary_signature_verdict(cmd, &td).0;

        assert_eq!(verdict("gh issue view 12 --repo xmlui-org/xmlui"), "skip");
        assert_eq!(verdict("ls -la"), "skip");
        // #363: forge-write text INSIDE a heredoc body is content, not a
        // command — Walt's exact shape (a local draft whose prose says how
        // to file it later) must not be screened as a forge write.
        assert_eq!(
            verdict(
                "cat > some-draft.md <<'EOF'\n# Draft\n\nTo file later: gh issue create --repo judell/bram --body-file some-draft.md\nEOF"
            ),
            "skip"
        );
        // The compound write-then-post shape stays covered: the gh half
        // sits OUTSIDE the heredoc mask, so #331's screening still applies.
        assert_eq!(
            verdict(
                "cat > b.md <<'EOF'\ndraft body\nEOF\ngh issue create --repo judell/bram --body-file b.md"
            ),
            "unparsed"
        );
        // Unterminated heredoc: body and command can't be separated —
        // masking falls back to raw text and today's conservative screen.
        assert_eq!(
            verdict("cat > c.md <<'EOF'\ngh issue create --repo judell/bram --body-file c.md"),
            "unparsed"
        );
        assert_eq!(
            verdict(&format!("gh issue comment 5 --body \"{}\"", unsigned)),
            "unsigned"
        );
        assert_eq!(
            verdict(&format!("gh issue comment 5 --body \"{}\"", signed)),
            "signed"
        );
        assert_eq!(
            verdict(&format!(
                "gh issue comment 5 --repo judell/bram --body \"{}\"",
                unsigned
            )),
            "unsigned"
        );
        assert!(!td.join(".git").join("config").exists());
        assert_eq!(
            verdict(&format!(
                "gh issue comment 5 --repo xmlui-org/xmlui --body \"{}\"",
                signed
            )),
            "signed"
        );
        assert_eq!(
            verdict(&format!(
                "gh issue comment 5 --repo xmlui-org/xmlui --body \"{}\"",
                unsigned
            )),
            "unsigned"
        );
        assert_eq!(
            verdict(&format!("gh pr comment 9 --body \"{}\"", unsigned)),
            "unsigned"
        );
        assert_eq!(
            verdict(&format!("glab issue note 3 -m \"{}\"", unsigned)),
            "unsigned"
        );
        assert_eq!(verdict("gh issue comment 5 --body-file -"), "unparsed");
        assert_eq!(
            verdict("gh issue comment 5 --body-file /nope/missing.md"),
            "unparsed"
        );
        assert_eq!(verdict("gh issue comment 5"), "unparsed");
        assert_eq!(verdict("gh issue close 5"), "skip");

        let body_path = td.join("comment.md");
        std::fs::write(&body_path, signed).unwrap();
        assert_eq!(
            crossboundary_signature_verdict(
                &format!("gh issue comment 5 --body-file {}", body_path.display()),
                &td
            ),
            ("signed", "body-file")
        );

        // Version-first canonical form (added 2026-09-07) is signed too.
        let versioned = "Jon's Claude (Bram 0.6.5, main thread, Fable 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        assert_eq!(
            verdict(&format!("gh issue comment 5 --body \"{}\"", versioned)),
            "signed"
        );

        // guard-checks-the-signature-version: the version slot is checked
        // SEPARATELY from signedness, so a stale one is still "signed" here —
        // signature_version_verdict is what denies it.
        assert_eq!(
            verdict(&format!("gh issue comment 5 --body \"{}\"", versioned)),
            "signed"
        );

        let _ = std::fs::remove_dir_all(&td);
    }

    // guard-checks-the-signature-version: the three cases the item's
    // acceptance names — present-and-wrong denied, correct allowed, absent
    // allowed. The live failure this reproduces is tau#1, signed "Bram 0.6.7"
    // and filed 10h26m after that project's own marker said 0.6.8.
    #[test]
    fn signature_version_mismatch_is_denied_but_absence_is_not() {
        let td = scratch("sigver");
        let current = env!("CARGO_PKG_VERSION");
        let sig = |v: &str| {
            format!(
                "Jon's Claude (Bram {}, main thread, Opus 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):\n\nBody.",
                v
            )
        };
        let v = |cmd: &str| signature_version_verdict(cmd, &td);

        // Present and WRONG -> denied, and the detail carries the SIGNED
        // version (not the running one) so the trace names what was claimed.
        let stale = v(&format!(
            "gh issue comment 5 --repo judell/bram --body \"{}\"",
            sig("0.6.7-not-a-real-build")
        ));
        assert_eq!(stale.0, "mismatch");
        assert_eq!(stale.1, "0.6.7-not-a-real-build");

        // Present and correct -> allowed.
        assert_eq!(
            v(&format!(
                "gh issue comment 5 --repo judell/bram --body \"{}\"",
                sig(current)
            ))
            .0,
            "ok"
        );

        // ABSENT -> allowed. The soft rollout is deliberate; this check is
        // strictly narrower than requiring a version slot, and older threads
        // remain valid text.
        let unversioned = "Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        assert_eq!(
            v(&format!(
                "gh issue comment 5 --repo judell/bram --body \"{}\"",
                unversioned
            ))
            .0,
            "ok"
        );

        // Not a forge write, and an unsigned body, are both somebody else's
        // verdict — this check must not claim them.
        assert_eq!(v("ls -la").0, "skip");
        assert_eq!(
            v("gh issue comment 5 --body \"no signature here\"").0,
            "skip"
        );
        assert_eq!(
            v("gh issue comment 5 --body-file /nope/missing.md").0,
            "skip"
        );

        // The deny text names both versions, so the agent does not have to
        // guess which one the guard thinks is right.
        let msg = stale_signature_version_message("0.6.7", &stale.2);
        assert!(msg.contains("0.6.7"), "{msg}");
        assert!(msg.contains(current), "{msg}");
        assert!(msg.contains("guard's build"), "{msg}");
        assert!(msg.contains("/__app-info"), "{msg}");

        let _ = std::fs::remove_dir_all(&td);
    }

    // The running instance's version, recorded in `.bram-port.json`, decides
    // the verdict; the guard's compiled version is only the fallback (the
    // 2026-11-10 incident: instance 0.8.5, guard rebuilt to 0.8.6).
    #[test]
    fn signature_version_prefers_the_running_instance_sidecar() {
        let td = scratch("sigver-sidecar");
        let current = env!("CARGO_PKG_VERSION");
        let recorded = "9.9.9-running";
        let sig = |v: &str| {
            format!(
                "gh issue comment 5 --repo judell/bram --body \"Jon's Claude (Bram {}, main thread, Opus 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):\n\nBody.\"",
                v
            )
        };

        // No sidecar: today's behavior, compared against the guard build.
        let none = signature_version_verdict(&sig(recorded), &td);
        assert_eq!(none.0, "mismatch");
        assert_eq!(none.2.source, "guard build");
        assert_eq!(signature_version_verdict(&sig(current), &td).0, "ok");

        // Sidecar without a usable version: still the guard build.
        crate::guard_policy::test_make_state_dir(&td).unwrap();
        let sidecar = td.join(".bram").join(".bram-port.json");
        std::fs::write(&sidecar, "{\"schema\":1,\"version\":\"\"}").unwrap();
        assert_eq!(signature_version_verdict(&sig(current), &td).0, "ok");

        // Sidecar with a version that differs from the compiled one.
        std::fs::write(
            &sidecar,
            format!("{{\"schema\":1,\"version\":\"{recorded}\"}}"),
        )
        .unwrap();
        let ok = signature_version_verdict(&sig(recorded), &td);
        assert_eq!(ok.0, "ok");
        assert_eq!(ok.2.source, "running instance");
        let bad = signature_version_verdict(&sig(current), &td);
        assert_eq!(bad.0, "mismatch");
        assert_eq!(bad.1, current);
        let msg = stale_signature_version_message(&bad.1, &bad.2);
        assert!(
            msg.contains("the running instance is 9.9.9-running"),
            "{msg}"
        );

        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn parse_agent_signature_version_slot() {
        // (a) new canonical form: version leads the parenthetical.
        let sig = parse_agent_signature(
            "Jon's Claude (Bram 0.6.5, main thread, Fable 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):",
        )
        .expect("versioned signature should parse");
        assert_eq!(sig.version, Some("0.6.5"));
        assert_eq!(sig.model, "Fable 5");
        assert_eq!(sig.os, Some("macOS"));
        assert_eq!(sig.machine, Some("Tuck"));
        assert_eq!(sig.project, "Bram");
        assert_eq!(sig.locator, "github.com/judell/bram");

        // (b) old five-slot form (no version) still parses, version None.
        let sig = parse_agent_signature(
            "Jon's Claude (main thread, Fable 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):",
        )
        .expect("five-slot signature should parse");
        assert_eq!(sig.version, None);
        assert_eq!(sig.model, "Fable 5");
        assert_eq!(sig.os, Some("macOS"));
        assert_eq!(sig.machine, Some("Tuck"));

        // (c) old two-slot form still parses, version None.
        let sig = parse_agent_signature(
            "Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):",
        )
        .expect("two-slot signature should parse");
        assert_eq!(sig.version, None);
        assert_eq!(sig.model, "Opus 5");
        assert_eq!(sig.os, None);
        assert_eq!(sig.machine, None);

        // (d) "Bramble 5" is not a version — the exact "Bram " prefix is
        // required — so it's read as the thread slot, fails thread
        // validation, and the whole parse fails.
        assert!(parse_agent_signature(
            "Jon's Claude (Bramble 5, main thread, Fable 5) speaking from the Bram project (github.com/judell/bram):",
        )
        .is_none());

        // (e) opencode-signature-and-conventions: OpenCode signs as itself,
        // with the model it is actually running in the model slot.
        let sig = parse_agent_signature(
            "Asish's OpenCode (Bram 0.7.7, main thread, deepseek-v4.1-flash, macOS, Mac) speaking from the Bram project (github.com/judell/bram):",
        )
        .expect("OpenCode signature should parse");
        assert_eq!(sig.model, "deepseek-v4.1-flash");
        assert_eq!(sig.locator, "github.com/judell/bram");
        // Any other agent name still fails, so a typo can't pass as signed.
        assert!(parse_agent_signature(
            "Asish's Opencode (Bram 0.7.7, main thread, x, macOS, Mac) speaking from the Bram project (github.com/judell/bram):",
        )
        .is_none());

        // (f) antigravity-signature-and-conventions: agy signs as
        // `Antigravity`, the product name, not its `agy` command. Miscased
        // or abbreviated names still fail.
        let sig = parse_agent_signature(
            "Asish's Antigravity (Bram 0.8.1, main thread, Gemini 3.8 Flash, macOS, Mac) speaking from the Bram project (github.com/judell/bram):",
        )
        .expect("Antigravity signature should parse");
        assert_eq!(sig.model, "Gemini 3.8 Flash");
        assert_eq!(sig.locator, "github.com/judell/bram");
        for wrong in ["AntiGravity", "antigravity", "agy"] {
            assert!(
                parse_agent_signature(&format!(
                    "Asish's {wrong} (Bram 0.8.1, main thread, x, macOS, Mac) speaking from the Bram project (github.com/judell/bram):",
                ))
                .is_none(),
                "{wrong} must not parse as signed"
            );
        }
    }

    // forge-signature-as-expander: the expander form and the one-line form
    // parse to the same fields; every consumer reads through one parser.
    const EXPANDER: &str = "<details>\n<summary>Jon's Claude speaking from github.com/judell/bram</summary>\n\n- Bram: v0.8.6\n- Model: Opus 5.5\n- Subagent: none\n- OS: macOS\n- Machine: Tuck\n</details>\n";

    fn git_scratch(name: &str) -> std::path::PathBuf {
        let td = scratch(name);
        std::fs::create_dir_all(td.join(".git").join("refs")).unwrap();
        std::fs::create_dir_all(td.join(".git").join("objects")).unwrap();
        std::fs::write(td.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            td.join(".git").join("config"),
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[remote \"origin\"]\n\turl = https://github.com/judell/bram.git\n",
        )
        .unwrap();
        td
    }

    #[test]
    fn expander_and_one_line_forms_parse_equal() {
        let one = "Jon's Claude (Bram 0.8.6, main thread, Opus 5.5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):";
        let (a, span_a) = parse_body_signature(one).expect("one-line");
        let (b, span_b) = parse_body_signature(EXPANDER).expect("expander");
        assert_eq!(span_a, 0..one.len());
        assert_eq!(&EXPANDER[span_b.clone()], EXPANDER.trim_end());
        assert_eq!(a.version, Some("0.8.6"));
        assert_eq!(b.version, Some("0.8.6"));
        assert_eq!(a.model, b.model);
        assert_eq!(a.os, b.os);
        assert_eq!(a.machine, b.machine);
        assert_eq!(a.project, "Bram");
        assert_eq!(b.project, "bram");
        assert_eq!(a.locator, b.locator);
        assert_eq!(a.speaker, "Jon's Claude");
        assert_eq!(b.speaker, "Jon's Claude");

        // No bullet marker, extra whitespace, no v prefix, optional fields.
        let loose = "<details>\n<summary>  Asish's Codex speaking from gitlab.com/g/s/p </summary>\n\n Bram:   0.8.6 \nModel:gpt-5.2\n</details>";
        let (c, _) = parse_body_signature(loose).expect("loose");
        assert_eq!(c.version, Some("0.8.6"));
        assert_eq!(c.model, "gpt-5.2");
        assert_eq!(c.machine, None);
        assert_eq!(c.project, "p");
        assert_eq!(c.locator, "gitlab.com/g/s/p");

        // Model is required; the agent name is checked.
        let no_model = EXPANDER.replace("- Model: Opus 5.5\n", "");
        assert!(parse_body_signature(&no_model).is_none());
        let bad_agent = EXPANDER.replace("Claude", "Bot");
        assert!(parse_body_signature(&bad_agent).is_none());
    }

    #[test]
    fn expander_signature_locator_and_version_are_checked() {
        let td = git_scratch("signature-expander");
        assert!(body_is_signed(&format!("{EXPANDER}\nBody."), &td));
        let wrong = EXPANDER.replace("github.com/judell/bram", "github.com/xmlui-org/xmlui");
        assert!(!body_is_signed(&format!("{wrong}\nBody."), &td));

        // Opener + expander (the PR form) is signed; an over-cap opener is not.
        let opener = "Lets Bram file PRs.\n\n";
        assert!(body_is_signed(&format!("{opener}{EXPANDER}"), &td));
        let long = format!(
            "{}\n\n{EXPANDER}",
            "word ".repeat(DETAILS_SUMMARY_OPENER_MAX / 4)
        );
        assert!(!body_is_signed(&long, &td));

        // Version mismatch against the recorded running instance.
        let state = td.join(".bram");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(".gitignore"), "*\n").unwrap();
        std::fs::write(state.join(".bram-port.json"), r#"{"version":"0.9.0"}"#).unwrap();
        let p = td.join("c.md");
        std::fs::write(&p, EXPANDER).unwrap();
        let cmd = format!("gh issue comment 5 --body-file {}", p.display());
        let (verdict, signed, _) = signature_version_verdict(&cmd, &td);
        assert_eq!(verdict, "mismatch");
        assert_eq!(signed, "0.8.6");
        std::fs::write(state.join(".bram-port.json"), r#"{"version":"0.8.6"}"#).unwrap();
        assert_eq!(signature_version_verdict(&cmd, &td).0, "ok");
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn expander_block_is_not_the_posts_fold() {
        let td = git_scratch("signature-expander-fold");
        let lines: String = (1..=40).map(|n| format!("- finding {n}\n")).collect();
        let verdict = |name: &str, body: &str| {
            let p = td.join(name);
            std::fs::write(&p, body).unwrap();
            forge_body_fold_verdict(
                &format!("gh issue comment 5 --body-file {}", p.display()),
                &td,
            )
        };
        // Only fold is the signature block: still unfolded.
        let only_sig = format!("{EXPANDER}\n{lines}");
        assert_eq!(verdict("a.md", &only_sig).0, "unfolded");
        assert_eq!(verdict("a.md", &only_sig).1, 40);
        // A second, content fold passes.
        let folded = format!(
            "{EXPANDER}\n**What this post is:** x.\n\n<details>\n<summary><b>1. Evidence</b></summary>\n\n{lines}</details>\n"
        );
        assert_eq!(verdict("b.md", &folded).0, "ok");
        // PR form: opener, signature block, then content fold.
        let pr = format!(
            "Short opener.\n\n{EXPANDER}\n<details>\n<summary>Details</summary>\n\n{lines}</details>\n"
        );
        assert_eq!(verdict("c.md", &pr).0, "ok");
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn body_is_signed_tolerates_markdown_emphasis() {
        let td = scratch("signature-markdown");
        assert!(body_is_signed(
            "_Jon's Codex (main thread, GPT-5.6-sol) speaking from the XMLUI project (github.com/xmlui-org/xmlui):_",
            &td
        ));
        assert!(body_is_signed(
            "> **Jon's Claude (subagent, Opus 5) speaking from the Bram project (github.com/judell/bram):**",
            &td
        ));
        assert!(!body_is_signed(
            "Bram side — a correction to my green light.",
            &td
        ));
        assert!(!body_is_signed("", &td));
        let _ = std::fs::remove_dir_all(&td);
    }

    // issue-484-details-summary-prs: a human opener above the fold, the
    // signature as the first <details> block's <summary>.
    #[test]
    fn details_summary_body_is_signed_by_its_summary() {
        let td = scratch("signature-details-summary");
        std::fs::create_dir_all(td.join(".git").join("refs")).unwrap();
        std::fs::create_dir_all(td.join(".git").join("objects")).unwrap();
        std::fs::write(td.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            td.join(".git").join("config"),
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[remote \"origin\"]\n\turl = https://github.com/judell/bram.git\n",
        )
        .unwrap();
        let sig = "Jon's Claude (Bram 0.8.5, main thread, Opus 5.5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):";
        let wrong = "Jon's Claude (main thread, Opus 5.5) speaking from the Bram project (github.com/xmlui-org/xmlui):";
        let opener = "Lets Bram file PRs with a human opener and the agent's text folded.\n\n";

        // One line, and split across lines.
        let one = format!("{opener}<details><summary>{sig}</summary>\n\nThe hook.\n</details>\n");
        let split =
            format!("{opener}<details>\n  <summary>{sig}</summary>\n\nThe hook.\n</details>\n");
        assert!(body_is_signed(&one, &td));
        assert!(body_is_signed(&split, &td));
        assert_eq!(
            parse_body_signature(&split).map(|(s, _)| s.locator),
            Some("github.com/judell/bram")
        );

        // A wrong locator in the summary is refused, as on a top line.
        let wrong_repo = format!("{opener}<details><summary>{wrong}</summary>\n\nx\n</details>");
        assert!(!body_is_signed(&wrong_repo, &td));

        // A summary that isn't a signature is refused.
        let model_only =
            format!("{opener}<details><summary>Claude Opus 5.5</summary>\n\nx\n</details>");
        assert!(!body_is_signed(&model_only, &td));

        // An opener over the cap is refused.
        let long = format!(
            "{}\n\n<details><summary>{sig}</summary>\n\nx\n</details>",
            "word ".repeat(DETAILS_SUMMARY_OPENER_MAX / 4)
        );
        assert!(!body_is_signed(&long, &td));

        // The plain top-line form is unchanged, and wins when present.
        assert!(body_is_signed(&format!("{sig}\n\nBody."), &td));
        let both = format!("{wrong}\n\n<details><summary>{sig}</summary>\n</details>");
        assert!(!body_is_signed(&both, &td));
        let _ = std::fs::remove_dir_all(&td);
    }

    // forge-body-fold-enforced: a long forge body with no <details> fold is
    // refused; a fold, a short body, or the details-summary form passes.
    #[test]
    fn long_forge_body_must_fold() {
        let td = scratch("forge-body-fold");
        let sig = "Jon's Claude (main thread, Opus 5.5) speaking from the Bram project (github.com/judell/bram):";
        let lines: String = (1..=40).map(|n| format!("- finding {n}\n")).collect();
        let write = |name: &str, body: &str| {
            let p = td.join(name);
            std::fs::write(&p, body).unwrap();
            format!("gh issue comment 5 --body-file {}", p.display())
        };
        let verdict = |cmd: &str| forge_body_fold_verdict(cmd, &td);

        // Long, unfolded: 40 non-blank lines after the signature.
        let long = format!("{sig}\n\n{lines}");
        assert_eq!(
            verdict(&write("long.md", &long)),
            ("unfolded", 40, long[sig.len()..].trim().chars().count())
        );

        // The same body with one fold passes.
        let folded = format!(
            "{sig}\n\n**What this post is:** x.\n\n<details>\n<summary><b>1. Evidence</b></summary>\n\n{lines}</details>\n"
        );
        assert_eq!(verdict(&write("folded.md", &folded)).0, "ok");

        // Short, unfolded passes; the signature line itself is not counted.
        let short_lines: String = (1..=FORGE_BODY_UNFOLDED_MAX_LINES)
            .map(|n| format!("line {n}\n"))
            .collect();
        let short = format!("{sig}\n\n{short_lines}");
        assert_eq!(
            verdict(&write("short.md", &short)),
            (
                "ok",
                FORGE_BODY_UNFOLDED_MAX_LINES,
                short[sig.len()..].trim().chars().count()
            )
        );

        // Few lines but over the character limit is long too.
        let wide = format!(
            "{sig}\n\n{}",
            "word ".repeat(FORGE_BODY_UNFOLDED_MAX_CHARS / 4)
        );
        assert_eq!(verdict(&write("wide.md", &wide)).0, "unfolded");

        // The details-summary PR form: a short opener, the signature as the
        // <summary>, everything else folded.
        let pr = format!(
            "Lets Bram file PRs with a human opener.\n\n<details>\n<summary>{sig}</summary>\n\n{lines}</details>\n"
        );
        std::fs::write(td.join("pr.md"), &pr).unwrap();
        let pr_cmd = format!(
            "gh pr create --title t --body-file {}",
            td.join("pr.md").display()
        );
        assert_eq!(verdict(&pr_cmd).0, "ok");

        // Not a body-bearing forge write: no check.
        assert_eq!(verdict("gh issue view 5").0, "skip");
        assert_eq!(verdict("gh issue close 5").0, "skip");
        assert_eq!(verdict("gh issue edit 5 --title t").0, "skip");
        assert_eq!(
            verdict(&format!("cat {}", td.join("long.md").display())).0,
            "skip"
        );
        // Gists and snippets carry file content, not a post.
        assert_eq!(
            verdict(&format!("gh gist create {}", td.join("long.md").display())).0,
            "skip"
        );

        // The deny site: the traced reason and the agent-facing body.
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": write("long.md", &long)},
            "cwd": "/",
        }));
        assert_eq!(v.reason, "long-forge-body-unfolded");
        let body = body_of(&v);
        assert!(body.contains("40 non-blank lines"), "{body}");
        assert!(body.contains("limits: 30 lines, 2500 characters"), "{body}");
        assert!(body.contains("**What this post is:**"), "{body}");
        assert!(
            body.contains("§Forge posts in details-summary form"),
            "{body}"
        );
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": write("folded.md", &folded)},
            "cwd": "/",
        }));
        assert_ne!(v.reason, "long-forge-body-unfolded");
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn repository_qualified_signatures_cover_gist_api_and_origin_match() {
        let td = scratch("signature-origin");
        std::fs::create_dir_all(td.join(".git").join("refs")).unwrap();
        std::fs::create_dir_all(td.join(".git").join("objects")).unwrap();
        std::fs::write(td.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            td.join(".git").join("config"),
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[remote \"origin\"]\n\turl = git@gitlab.com:group/subgroup/project.git\n",
        )
        .unwrap();

        let signed = "Jon's Codex (main thread, GPT-5.6-sol) speaking from the Bram project (gitlab.com/group/subgroup/project):\n\nBody.";
        let wrong_repo = "Jon's Codex (main thread, GPT-5.6-sol) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        let old_form = "Jon's Codex speaking from the Bram project:\n\nBody.";
        assert_eq!(
            expected_repo_locator(&td),
            Some("gitlab.com/group/subgroup/project".to_string())
        );
        assert!(body_is_signed(signed, &td));
        assert!(!body_is_signed(wrong_repo, &td));
        assert!(!body_is_signed(old_form, &td));
        assert_eq!(
            normalize_repo_locator("https://GitLab.com/group/subgroup/project.git\n"),
            Some("gitlab.com/group/subgroup/project".to_string())
        );

        let gist_body = td.join("gist.md");
        std::fs::write(&gist_body, signed).unwrap();
        assert_eq!(
            crossboundary_signature_verdict(
                &format!(
                    "gh gist edit deadbeef --filename gist.md {}",
                    gist_body.display()
                ),
                &td
            ),
            ("signed", "positional-body-file")
        );
        assert_eq!(
            crossboundary_signature_verdict(
                &format!(
                    "gh api --method POST gists/deadbeef/comments -f \"body={}\"",
                    signed.replace('\n', "\\n")
                ),
                &td
            ),
            ("signed", "api-body-field")
        );
        assert_eq!(
            crossboundary_signature_verdict(
                "gh gist edit deadbeef --filename gist.md missing.md",
                &td
            ),
            ("unparsed", "positional-body-file-unreadable")
        );
        let _ = std::fs::remove_dir_all(&td);
    }

    // --- mcp_candidate_paths (judell/bram#384) -------------------------------
    //
    // Replaces the retired `mcp_path_extraction` test, which pinned
    // `MCP_PATH_KEYS`'s top-level-only, thirteen-key extraction -- exactly
    // the enumeration this item replaces with a structural walk. `norm`
    // below is `codex_normalize_target`, reused here only because it joins
    // relative strings against an explicit root instead of the process cwd
    // (unlike the Claude arm's `normalize_target`), which is what lets this
    // test use plain relative strings instead of every candidate needing to
    // be spelled out as an absolute path under `root`.

    #[test]
    fn mcp_candidate_paths_structural() {
        let root = scratch("mcp-candidates");
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app/x.xmlui"), "").unwrap();
        let norm = |s: &str| codex_normalize_target(&root, s);
        let empty: Vec<String> = Vec::new();

        // Found via a key MCP_PATH_KEYS always recognized (existing file).
        assert_eq!(
            mcp_candidate_paths(&serde_json::json!({"path": "app/x.xmlui"}), &root, norm),
            vec!["app/x.xmlui".to_string()]
        );

        // Found via a key MCP_PATH_KEYS never knew about, nested one level
        // inside an object -- the structural fix's whole point (the #6 synth
        // case in the item: an "output_path"-shaped write tool).
        assert_eq!(
            mcp_candidate_paths(
                &serde_json::json!({"options": {"output_path": "app/x.xmlui"}}),
                &root,
                norm
            ),
            vec!["app/x.xmlui".to_string()]
        );

        // Found nested inside an array of objects.
        assert_eq!(
            mcp_candidate_paths(
                &serde_json::json!({"edits": [{"path": "app/x.xmlui", "oldText": "a"}]}),
                &root,
                norm
            ),
            vec!["app/x.xmlui".to_string()]
        );

        // Prose containing a path-shaped substring is NOT a candidate: the
        // whole string is resolved as one path, never a substring, and this
        // one has spaces (the over-matching guard's primary target).
        assert_eq!(
            mcp_candidate_paths(
                &serde_json::json!({"body": "see app/x.xmlui for details"}),
                &root,
                norm
            ),
            empty
        );

        // A bare, slash-free token that does not exist is NOT a candidate:
        // its filesystem parent (the project root) always exists, so
        // without this narrowing almost any short connector field (an
        // email address, an id, a one-word title) would qualify as
        // "creatable" purely by not yet being a file. This is what keeps
        // realistic Gmail/Calendar/Drive args from becoming false
        // positives again under the new design.
        assert_eq!(
            mcp_candidate_paths(
                &serde_json::json!({"to": "someone@example.com"}),
                &root,
                norm
            ),
            empty
        );

        // The same shape of bare token IS a candidate once it names
        // something that actually exists -- existence is real evidence
        // about the repository, not an artifact of how many path segments
        // the string happens to have.
        std::fs::write(root.join("NOTES"), "").unwrap();
        assert_eq!(
            mcp_candidate_paths(&serde_json::json!({"summary": "NOTES"}), &root, norm),
            vec!["NOTES".to_string()]
        );

        // A multi-segment string whose parent doesn't exist (a fabricated
        // token, such as a base64 blob that happens to contain a stray
        // '/') is not a candidate: nonsense intermediate directories don't
        // exist either.
        assert_eq!(
            mcp_candidate_paths(
                &serde_json::json!({"content": "QUJDREVGRw/f9J3ha+kL=="}),
                &root,
                norm
            ),
            empty
        );

        // A multi-segment path whose parent DOES exist is a creatable
        // candidate even though the file itself doesn't exist yet -- the
        // legitimate file-creation case the parent test exists to admit.
        assert_eq!(
            mcp_candidate_paths(
                &serde_json::json!({"output_path": "app/new.txt"}),
                &root,
                norm
            ),
            vec!["app/new.txt".to_string()]
        );

        // Non-object top-level input: still scanned (the bare top-level
        // string), filtered out here for the same reason as the bare-token
        // case above.
        assert_eq!(
            mcp_candidate_paths(&serde_json::json!("not-a-dict"), &root, norm),
            empty
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // --- subagent lifecycle gate (python lines 1389-1440) -------------------

    #[test]
    fn subagent_lifecycle_gate_keys_on_agent_id() {
        let lifecycle_cmd = "curl -4 -sS -X POST -H \"Content-Type: application/json\" \
             --data @/tmp/body.json http://127.0.0.1:61455/__worklist/mutate";
        assert_eq!(
            subagent_lifecycle_verdict(lifecycle_cmd, "b1iz1vf6f"),
            ("deny", None)
        );
        assert_eq!(
            subagent_lifecycle_verdict(lifecycle_cmd, ""),
            ("allow", Some("no-agent-id"))
        );
        assert_eq!(
            subagent_lifecycle_verdict(lifecycle_cmd, "   "),
            ("allow", Some("no-agent-id"))
        );
        assert_eq!(
            subagent_lifecycle_verdict("git status", "b1iz1vf6f"),
            ("skip", None)
        );
        // A subagent transcript path in the agent_id slot must not deny.
        assert_eq!(
            subagent_lifecycle_verdict("git status", "/tmp/subagents/agent-1.jsonl"),
            ("skip", None)
        );
    }

    // --- subagent worklist-write gate (python lines 1442-1464) --------------

    #[test]
    fn subagent_worklist_write_gate() {
        assert_eq!(
            subagent_worklist_write_verdict(
                Path::new("/nonexistent-issue-469"),
                Some(WORKLIST_REL),
                "b1iz1vf6f"
            ),
            ("deny", None)
        );
        assert_eq!(
            subagent_worklist_write_verdict(
                Path::new("/nonexistent-issue-469"),
                Some(".bram/worklist-drafts/some-item.md"),
                "b1iz1vf6f"
            ),
            ("deny", None)
        );
        assert_eq!(
            subagent_worklist_write_verdict(
                Path::new("/nonexistent-issue-469"),
                Some(WORKLIST_REL),
                ""
            ),
            ("skip", Some("no-agent-id"))
        );
        assert_eq!(
            subagent_worklist_write_verdict(Path::new("/nonexistent-issue-469"), None, ""),
            ("skip", None)
        );
        assert_eq!(
            subagent_worklist_write_verdict(
                Path::new("/nonexistent-issue-469"),
                Some(WORKLIST_REL),
                "   "
            ),
            ("skip", Some("no-agent-id"))
        );
        assert_eq!(
            subagent_worklist_write_verdict(
                Path::new("/nonexistent-issue-469"),
                Some("app/tools/Main.xmlui"),
                "b1iz1vf6f"
            ),
            ("skip", None)
        );
    }

    #[test]
    fn issue_309_worktree_coverage_mapping() {
        let covered: HashSet<String> = ["app/x.txt", "components"]
            .into_iter()
            .map(str::to_string)
            .collect();

        assert_eq!(
            coverage_verdict(&covered, ".claude/worktrees/agent-a/app/x.txt"),
            (true, Some("agent-a"))
        );
        // #295 directory-entry coverage is evaluated after #309 mapping.
        assert_eq!(
            coverage_verdict(
                &covered,
                ".claude/worktrees/agent-a/components/nested/y.xmlui"
            ),
            (true, Some("agent-a"))
        );
        assert_eq!(
            coverage_verdict(&covered, ".claude/worktrees/agent-a/app/y.txt"),
            (false, Some("agent-a"))
        );

        let claude_hook: HashSet<String> = [".claude/hooks/guard.py"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(
            coverage_verdict(&claude_hook, ".claude/hooks/guard.py"),
            (true, None)
        );
        assert_eq!(
            coverage_verdict(&covered, "scratch/.claude/worktrees/agent-a/app/x.txt"),
            (false, None)
        );
    }

    // --- cross-root direct-edit bypass (python lines 1466-1574) -------------

    fn write_auth(root: &Path, kind: &str, paths: &[&str], age_ms: f64) {
        let rec = serde_json::json!({
            "kind": kind,
            "paths": paths,
            "issuedAtMs": now_ms() - age_ms,
        });
        std::fs::write(root.join(AUTH_REL), rec.to_string()).unwrap();
    }

    fn clear_auth(root: &Path) {
        let _ = std::fs::remove_file(root.join(AUTH_REL));
    }

    #[test]
    fn cross_root_bypass_consults_both_roots() {
        let base = scratch("crossroot");
        let target_root = base.join("target-project");
        let session_root = base.join("session-project");
        crate::guard_policy::test_make_state_dir(&target_root).unwrap();
        crate::guard_policy::test_make_state_dir(&session_root).unwrap();

        // resolve_session_root walks up to the marker, and fails quietly.
        write_auth(&session_root, "direct-edit", &["*"], 0.0);
        assert_eq!(
            resolve_session_root(&serde_json::json!({"cwd": session_root.to_string_lossy()})),
            Some(abspath_of(&session_root))
        );
        assert_eq!(
            resolve_session_root(&serde_json::json!({
                "cwd": session_root.join("nested").join("dir").to_string_lossy()
            })),
            Some(abspath_of(&session_root))
        );
        assert_eq!(
            resolve_session_root(&serde_json::json!({"cwd": base.to_string_lossy()})),
            None
        );
        assert_eq!(resolve_session_root(&serde_json::json!({})), None);
        assert_eq!(resolve_session_root(&serde_json::json!({"cwd": 123})), None);
        clear_auth(&session_root);

        // (a) target root only -> matched via target, session never consulted.
        write_auth(&target_root, "direct-edit", &["*"], 0.0);
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", Some(&session_root)),
            (true, Some("target"), "ok", None)
        );
        assert!(fresh_bypass(&target_root, "some/file.txt"));
        clear_auth(&target_root);

        // (b) session root only -> matched via session.
        write_auth(&session_root, "direct-edit", &["*"], 0.0);
        assert!(!fresh_bypass(&target_root, "some/file.txt"));
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", Some(&session_root)),
            (true, Some("session"), "absent", Some("ok"))
        );
        clear_auth(&session_root);

        // (c) neither -> both lookups run, both absent.
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", Some(&session_root)),
            (false, None, "absent", Some("absent"))
        );

        // (d) stale in the session root -> named as stale, not a false ok.
        write_auth(
            &session_root,
            "direct-edit",
            &["*"],
            BYPASS_TTL_MS + 60_000.0,
        );
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", Some(&session_root)),
            (false, None, "absent", Some("stale"))
        );
        clear_auth(&session_root);

        // (e) session root == target root -> the second lookup never runs.
        write_auth(&target_root, "direct-edit", &["*"], 0.0);
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", Some(&target_root)),
            (true, Some("target"), "ok", None)
        );
        clear_auth(&target_root);
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", Some(&target_root)),
            (false, None, "absent", None)
        );

        // No session root at all -> single lookup.
        assert_eq!(
            cross_root_bypass_verdict(&target_root, "some/file.txt", None),
            (false, None, "absent", None)
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // --- pipeline-level checks (not in the Python self-test, which exercises
    // main() only through the live hook) ------------------------------------

    #[test]
    fn wrong_provider_returns_none() {
        let payload = serde_json::json!({"tool_name": "Bash"});
        assert!(shadow_worklist_decision("gemini-rs", &payload).is_none());
        assert!(shadow_worklist_decision("claude-rs", &payload).is_some());
        assert!(shadow_worklist_decision("codex-rs", &payload).is_some());
    }

    // unowned-lines-postmortem-fixes: a stdin script through a heredoc is
    // inspected like `python -c` / `node -e`.
    #[test]
    fn interpreter_heredoc_writes_classify_as_writes() {
        // The 2026-10-09 06:09:03Z command's shape: quote-masking hid 'w'.
        let incident = "python3 - <<'EOF'\np='/x/helpers.js'\ns=open(p).read()\nopen(p,'w').write(s)\nEOF\nnode --check helpers.js";
        assert!(interpreter_heredoc_writes(incident));
        assert_eq!(
            write_patterns_token(incident, SkipPattern::None).as_deref(),
            Some("interpreter heredoc")
        );
        assert!(interpreter_heredoc_writes(
            "python3 -I - <<'EOF'\nimport os\nos.replace('a','b')\nEOF"
        ));
        assert!(interpreter_heredoc_writes(
            "cd x && python3.12 - <<EOF\nshutil.copy('a','b')\nEOF"
        ));
        assert!(interpreter_heredoc_writes(
            "node - <<'JS'\nrequire('fs').writeFileSync('a','b')\nJS"
        ));
        // Unquoted tag: the shell expands the body, so `$` is conservative.
        assert!(interpreter_heredoc_writes(
            "python3 - <<EOF\nprint(\"$HOME\")\nEOF"
        ));
    }

    #[test]
    fn interpreter_heredoc_reads_stay_reads() {
        let read = "python3 -I - <<'EOF'\nimport json,sys\nprint(len(sys.argv))\nEOF";
        assert!(!interpreter_heredoc_writes(read));
        assert_eq!(write_patterns_token(read, SkipPattern::None), None);
        // A script FILE reading heredoc data is not a stdin script.
        assert!(!interpreter_heredoc_writes(
            "python3 tool.py <<'EOF'\nopen(x,'w')\nEOF"
        ));
        // Not an interpreter at all.
        assert!(!interpreter_heredoc_writes("cat <<'EOF'\nopen(x,'w')\nEOF"));
        assert_eq!(heredoc_interpreter("pythonista -"), None);
        assert_eq!(heredoc_interpreter("/usr/bin/python3 -I -"), Some("python"));
    }

    #[test]
    fn bash_read_only_pipeline_allows() {
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "ls -la"},
                "cwd": "/",
            }),
        )
        .unwrap();
        assert_eq!(v.decision, "allow");
        assert_eq!(v.reason, "bash-read-only");
    }

    #[test]
    fn gh_body_at_antipattern_denies() {
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "gh issue comment 5 --body @body.md"},
                "cwd": "/",
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "gh-body-at-antipattern")
        );
        assert!(!is_gh_body_at_antipattern(
            "gh issue comment 5 --body \"x\""
        ));
    }

    #[test]
    fn write_edit_pipeline_covers_worklist_surfaces() {
        let root = scratch("we");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();

        // A lifecycle path is implicitly authorized.
        let inflight = root.join(".bram/.inflight-claim.json");
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": inflight.to_string_lossy(), "content": "{}"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "bram-lifecycle-channel")
        );

        // An uncovered project file is denied and the root is named.
        let target = root.join("app/x.txt");
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": target.to_string_lossy(), "content": "x"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(v.decision, "deny");
        assert_eq!(v.target, "app/x.txt");
        assert!(
            v.reason.starts_with("no-coverage-no-opt-out:root="),
            "reason: {}",
            v.reason
        );

        // Covered by a proposed item -> allowed.
        std::fs::write(
            root.join(WORKLIST_REL),
            serde_json::json!({
                "version": 1,
                "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
            })
            .to_string(),
        )
        .unwrap();
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": target.to_string_lossy(), "content": "x"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "covered-by-worklist-item")
        );

        // A stale version bump on worklist.json itself is denied.
        let wl = root.join(WORKLIST_REL);
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {
                    "file_path": wl.to_string_lossy(),
                    "content": serde_json::json!({
                        "version": 1,
                        "items": [
                            {"id": "i1", "status": "proposed", "files": ["app/x.txt"]},
                            {"id": "i2", "status": "proposed", "files": ["app/y.txt"]}
                        ]
                    }).to_string(),
                },
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "stale-worklist-version")
        );

        // A correct bump authors cleanly.
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {
                    "file_path": wl.to_string_lossy(),
                    "content": serde_json::json!({
                        "version": 2,
                        "items": [
                            {"id": "i1", "status": "proposed", "files": ["app/x.txt"]},
                            {"id": "i2", "status": "proposed", "files": ["app/y.txt"]}
                        ]
                    }).to_string(),
                },
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "worklist-author")
        );

        // Removing an item is a mechanical change.
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {
                    "file_path": wl.to_string_lossy(),
                    "content": serde_json::json!({"version": 2, "items": []}).to_string(),
                },
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "mechanical-worklist-change")
        );

        // Inline prose on a proposed item is denied.
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {
                    "file_path": wl.to_string_lossy(),
                    "content": serde_json::json!({
                        "version": 2,
                        "items": [
                            {"id": "i1", "status": "proposed", "files": ["app/x.txt"], "before": "x"}
                        ]
                    }).to_string(),
                },
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "worklist-inline-prose")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn issue_309_claude_worktree_pipeline_maps_only_coverage() {
        let root = scratch("worktree-claude");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();
        std::fs::write(
            root.join(WORKLIST_REL),
            serde_json::json!({
                "version": 1,
                "items": [{
                    "id": "i1",
                    "status": "proposed",
                    "files": ["app/x.txt", "components"]
                }]
            })
            .to_string(),
        )
        .unwrap();

        let covered = root.join(".claude/worktrees/agent-a/app/x.txt");
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": covered.to_string_lossy(), "content": "x"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "covered-by-worklist-item")
        );
        assert_eq!(v.target, ".claude/worktrees/agent-a/app/x.txt");

        let covered_dir = root.join(".claude/worktrees/agent-a/components/nested/y.xmlui");
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": covered_dir.to_string_lossy(), "content": "x"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "covered-by-worklist-item")
        );

        let uncovered = root.join(".claude/worktrees/agent-a/app/y.txt");
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": uncovered.to_string_lossy(), "content": "x"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("no-coverage-no-opt-out:worktree=agent-a:root="),
            "reason: {}",
            v.reason
        );
        assert_eq!(v.target, ".claude/worktrees/agent-a/app/y.txt");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_pipeline_branches() {
        let root = scratch("mcp");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();

        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "mcp__filesystem__read_text_file",
                "tool_input": {"path": "a"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-read-only")
        );

        // judell/bram#384: "contents" is a genuinely unrecognized key (it was
        // never in the old MCP_PATH_KEYS list either), and its value "x" is a
        // bare, slash-free token that resolves to zero structural candidates
        // -- so this is now the accepted residual (allow + observe-only
        // trace), not a fail-closed deny. Previously this asserted
        // ("deny", "mcp-unrecognized-input"); the design deliberately
        // inverts this case, so the assertion changes with it.
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "mcp__filesystem__write_file",
                "tool_input": {"contents": "x"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-no-candidate-paths")
        );
        let events = std::fs::read_to_string(root.join(".bram/bram-traces/hook-events.log"))
            .unwrap_or_default();
        assert!(
            events.contains("mcp-write-verb-no-candidate-paths")
                && events.contains("mcp__filesystem__write_file"),
            "events: {events}"
        );

        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "mcp__filesystem__write_file",
                "tool_input": {"path": root.join(WORKLIST_REL).to_string_lossy()},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "mcp-worklist-write")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn helpers_match_the_python_predicates() {
        assert!(is_lifecycle_path(
            Path::new("/nonexistent-issue-469"),
            ".bram/.bram-port"
        ));
        assert!(is_lifecycle_path(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-drafts/x.md"
        ));
        assert!(!is_lifecycle_path(Path::new("/nonexistent-issue-469"), ""));
        assert!(!is_lifecycle_path(
            Path::new("/nonexistent-issue-469"),
            "app/tools/Main.xmlui"
        ));
        assert!(is_worklist_draft(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-drafts/x.md"
        ));
        assert!(!is_worklist_draft(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-drafts/sub/x.md"
        ));
        assert!(!is_worklist_draft(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-drafts/x.json"
        ));
        assert!(push_cmd("git push"));
        assert!(push_cmd("git -C /tmp/repo push origin main"));
        assert!(push_cmd("git commit -m x && git push"));
        assert!(!push_cmd("pushd /tmp"));
        assert_eq!(worklist_version_from_text("{\"version\": 3}"), (true, 3));
        assert_eq!(worklist_version_from_text("{}"), (false, 0));
        assert_eq!(worklist_version_from_text("nope"), (false, 0));
        assert_eq!(
            full_shas("see 0123456789abcdef0123456789abcdef01234567 here"),
            vec!["0123456789abcdef0123456789abcdef01234567"]
        );
        assert!(full_shas("deadbeef").is_empty());
    }

    // judell/bram#384 supersedes #360's design for this exact case. #360 kept
    // the fail-closed deny for a name-classified MCP "mutation" with no
    // extractable path, releasing it only through an explicit grant or
    // opt-out phrase (the block this test used to exercise, and the very
    // shape #384's report showed denying `tabs_create_mcp`, Gmail
    // `create_draft`, Calendar `create_event`/`delete_event`, and 8 real
    // Google_Drive `create_file` denials in this project's own trace
    // history). Under the structural design, zero candidate paths means the
    // call cannot name a repo file at all, so it is out of scope
    // regardless of name -- no grant or opt-out needed, and none of them
    // change the verdict any more. The one residual (an ambient/
    // session-remembered write target) is traced, not silently allowed.
    #[test]
    fn mcp_zero_candidates_allowed_regardless_of_authorization() {
        let root = scratch("mcp360");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();

        let payload = serde_json::json!({
            "tool_name": "mcp__claude_ai_Google_Drive__create_file",
            "tool_input": {"title": "x", "textContent": "y"},
            "cwd": root.to_string_lossy(),
        });

        // No grant, no opt-out, no authorization file at all beyond the bare
        // managed-repo marker: still allowed, because there is nothing here
        // that could name a repo path.
        let v = shadow_worklist_decision("claude-rs", &payload).unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-no-candidate-paths")
        );
        let events = std::fs::read_to_string(root.join(".bram/bram-traces/hook-events.log"))
            .unwrap_or_default();
        assert!(
            events.contains("mcp-write-verb-no-candidate-paths")
                && events.contains("mcp__claude_ai_Google_Drive__create_file"),
            "events: {events}"
        );

        // A stale/absent grant changes nothing either -- the allow no longer
        // routes through the bypass/opt-out machinery at all.
        write_auth(&root, "direct-edit", &["*"], BYPASS_TTL_MS + 60_000.0);
        let v = shadow_worklist_decision("claude-rs", &payload).unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-no-candidate-paths")
        );

        // Same shape, second connector (Calendar create_event), no
        // transcript and no "just do it" phrase anywhere -- still allowed.
        let v = shadow_worklist_decision(
            "claude-rs",
            &serde_json::json!({
                "tool_name": "mcp__claude_ai_Google_Calendar__create_event",
                "tool_input": {"summary": "lunch"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-no-candidate-paths")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // --- authority-mode deny-message parity (bram-guard-authority-flip) -----
    //
    // Shadow mode ignores ShadowVerdict::message; authority mode prints it as
    // the whole of stderr, so these assert the transcribed text against the
    // Python guard's `print(msg, file=sys.stderr)` bodies, verbatim, with the
    // authoritative line numbers cited beside each. The `[worklist-guard] …`
    // header `_trace_hook` writes first is line 1 of every message; `body_of`
    // strips it so each assertion reads as the Python source does.

    fn claude_decide(payload: Value) -> ShadowVerdict {
        shadow_worklist_decision("claude-rs", &payload).unwrap()
    }

    fn body_of(v: &ShadowVerdict) -> &str {
        v.message
            .split_once('\n')
            .map(|(_, body)| body)
            .unwrap_or("")
    }

    fn header_of(v: &ShadowVerdict) -> &str {
        v.message.split('\n').next().unwrap_or("")
    }

    fn managed_claude(name: &str) -> PathBuf {
        let root = scratch(name);
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();
        root
    }

    #[test]
    fn message_parity_no_coverage_write() {
        // python claude-worklist-guard.py:1183-1245 (deny_coverage), reached
        // from :2330. opt_out_attempted is false (no transcript), and the
        // session-root leg is skipped because cwd resolves to the same root.
        let root = managed_claude("msg-no-coverage");
        std::fs::write(root.join("app/x.txt"), "hello").unwrap();
        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": root.join("app/x.txt").to_string_lossy(),
                "content": "new",
            },
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.decision, "deny");
        let expected = format!(
            "Blocked: writing to app/x.txt requires either a proposed/applied item in .bram/worklist.json covering this path, or an explicit opt-out phrase in your last message.\n  - Propose the change in .bram/worklist.json first (item with file=\"app/x.txt\", non-empty before and after, status proposed). Wait for the user's approved: payload, then retry.\n  - The user can authorize a direct edit by ending their message with \"just do it\", or by clicking the Skip worklist button.\nresolved_project_root={root} (marker: .bram/.worklist-authorization.json; if this is not the repo root, a stray marker file captured this subtree — remove that .bram/.worklist-authorization.json)",
            root = root.display()
        );
        assert_eq!(body_of(&v), expected);
        // The header is `_trace_hook`'s stderr diagnostic: Write/Edit denials
        // pass no cwd, so it carries the guard process's own.
        assert_eq!(
            header_of(&v),
            format!(
                "[worklist-guard] tool=Write target=app/x.txt cwd={} decision=deny reason=no-coverage-no-opt-out:root={}",
                process_cwd(),
                root.display()
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_no_coverage_worktree_line() {
        // python:1211-1217 — the worktree marker paragraph, appended before
        // the resolved_project_root line (#309).
        let root = managed_claude("msg-worktree");
        let target = root.join(".claude/worktrees/agent-a/app/x.txt");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "hello").unwrap();
        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {"file_path": target.to_string_lossy(), "content": "new"},
            "cwd": root.to_string_lossy(),
        }));
        assert!(body_of(&v).contains(
            "\nworktree=agent-a (coverage checked against the corresponding real-tree path; this denial is still authoritative)\nresolved_project_root="
        ), "body: {}", body_of(&v));
        assert!(v.reason.contains(":worktree=agent-a:root="), "{}", v.reason);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn paused_claude(name: &str, enforcement: &str) -> PathBuf {
        let root = managed_claude(name);
        std::fs::write(
            root.join(".bram.json"),
            format!("{{\"worklist\":{{\"enforcement\":\"{}\"}}}}", enforcement),
        )
        .unwrap();
        root
    }

    fn write_uncovered(root: &Path) -> ShadowVerdict {
        claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": root.join("app/x.txt").to_string_lossy(),
                "content": "new",
            },
            "cwd": root.to_string_lossy(),
        }))
    }

    #[test]
    fn enforcement_default_denies_uncovered_write() {
        let root = managed_claude("enf-default");
        let v = write_uncovered(&root);
        assert_eq!(v.decision, "deny");
        assert!(v.reason.starts_with("no-coverage"), "{}", v.reason);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enforcement_paused_allows_uncovered_write() {
        let root = paused_claude("enf-paused", "paused");
        let v = write_uncovered(&root);
        assert_eq!(v.decision, "allow");
        assert!(
            v.reason.starts_with("enforcement-paused:no-coverage"),
            "{}",
            v.reason
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enforcement_on_still_denies_uncovered_write() {
        let root = paused_claude("enf-on", "on");
        let v = write_uncovered(&root);
        assert_eq!(v.decision, "deny");
        assert!(v.reason.starts_with("no-coverage"), "{}", v.reason);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enforcement_paused_keeps_non_coverage_denies() {
        let root = paused_claude("enf-paused-other", "paused");
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh issue comment 5 --body @body.md"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "gh-body-at-antipattern")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enforcement_paused_allows_bash_write_no_coverage() {
        let root = paused_claude("enf-paused-bash", "paused");
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "rm stale.txt"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.decision, "allow");
        assert!(
            v.reason
                .starts_with("enforcement-paused:bash-write-no-coverage"),
            "{}",
            v.reason
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_bash_write_no_coverage() {
        // python:1284-1323 (deny_bash_coverage), reached from :2117. The push
        // bullet (judell/bram#283) rides on _PUSH_CMD_RX only.
        let root = managed_claude("msg-bash-coverage");
        let base = "Bash blocked: this command writes to the filesystem or performs a sensitive side effect, and .bram/worklist.json has no proposed or applied items covering active work.\n  - Propose the work in the worklist first, wait for the user's approved: payload, then retry.\n  - The user can authorize a direct edit by ending their message with \"just do it\", or by clicking the Skip worklist button.";
        let root_line = format!(
            "\nresolved_project_root={} (marker: .bram/.worklist-authorization.json; if this is not the repo root, a stray marker file captured this subtree — remove that .bram/.worklist-authorization.json)",
            root.display()
        );

        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "rm stale.txt"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.decision, "deny");
        assert_eq!(body_of(&v), format!("{}{}", base, root_line));
        // Bash denials DO pass cwd to _trace_hook, and the trace target is the
        // 200-char command preview rather than the breadcrumb's `-`.
        assert_eq!(
            header_of(&v),
            format!(
                "[worklist-guard] tool=Bash target=rm stale.txt cwd={root} decision=deny reason=bash-write-no-coverage:token=rm:root={root}",
                root = root.display()
            )
        );

        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git push origin main"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(
            body_of(&v),
            format!(
                "{}{}\n  - This looks like a push. The user can click Push in the Commits tab; an agent push is allowed only in the 10-minute window after a gate commit (see judell/bram#283).",
                base, root_line
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_stale_worklist_version() {
        // python:1342-1371 (deny_stale_worklist_version), both `detail` arms.
        let root = managed_claude("msg-stale-version");
        let wl = root.join(WORKLIST_REL);
        std::fs::write(&wl, r#"{"version":1,"items":[]}"#).unwrap();

        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": wl.to_string_lossy(),
                "content": r#"{"version":1,"items":[]}"#,
            },
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "stale-worklist-version");
        assert_eq!(
            body_of(&v),
            "Blocked: stale base on .bram/worklist.json. You set version=1, but on-disk version is 1. Expected version=2.\n  - Re-read .bram/worklist.json, base your edit on the current contents, and include the bumped version field on the write. This guards against concurrent-writer races between your propose / Edit and other agents or the /__worklist/mutate route."
        );

        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {"file_path": wl.to_string_lossy(), "content": r#"{"items":[]}"#},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(
            body_of(&v),
            "Blocked: stale base on .bram/worklist.json. Your write is missing the `version` field. On-disk version is 1; the new content must set version=2.\n  - Re-read .bram/worklist.json, base your edit on the current contents, and include the bumped version field on the write. This guards against concurrent-writer races between your propose / Edit and other agents or the /__worklist/mutate route."
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_inline_prose() {
        // python:1388-1409 (deny_inline_prose), including the `<no-id>` label
        // for an item with no usable id.
        let root = managed_claude("msg-inline-prose");
        let wl = root.join(WORKLIST_REL);
        std::fs::write(&wl, r#"{"version":1,"items":[]}"#).unwrap();
        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": wl.to_string_lossy(),
                "content": r#"{"version":2,"items":[{"id":"a","status":"proposed","before":"x"},{"status":"proposed","after":"y"}]}"#,
            },
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "worklist-inline-prose");
        assert_eq!(
            body_of(&v),
            "Blocked: inline `before` / `after` on proposed worklist item(s) — prose must live in .bram/worklist-drafts/<id>.md.\n  - item a: remove inline before/after; write prose to .bram/worklist-drafts/a.md instead\n  - item <no-id>: remove inline before/after; write prose to .bram/worklist-drafts/<no-id>.md instead\nDraft file format: `# Before` section then `# After` section, both in Markdown. The server merge surfaces the draft prose alongside the metadata in worklist.json."
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_mechanical_worklist_change() {
        // python:1412-1444 (deny_mechanical_worklist_change), with both the
        // Removed and Status-changes detail lines present.
        let root = managed_claude("msg-mechanical");
        let wl = root.join(WORKLIST_REL);
        std::fs::write(
            &wl,
            r#"{"version":1,"items":[{"id":"a","status":"proposed"},{"id":"b","status":"proposed"}]}"#,
        )
        .unwrap();
        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": wl.to_string_lossy(),
                "content": r#"{"version":2,"items":[{"id":"b","status":"applied"}]}"#,
            },
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "mechanical-worklist-change");
        assert_eq!(
            body_of(&v),
            "Blocked: mechanical worklist state changes must go through `POST /__worklist/mutate`, not a direct edit to `.bram/worklist.json`.\n  - Direct worklist edits are for proposing items or refining their prose during iterate.\n  - Use mutate for `prune` and `advance` after a verified `drop:` / `approved:` turn.\n  - Removed item ids: \"a\" (status=proposed)\n  - Status changes: \"b\" (proposed->applied)\n  - Example: curl -4 -sS -X POST -d '{\"op\":\"prune\",\"ids\":[\"item-id\"]}' http://127.0.0.1:$(cat .bram/.bram-port)/__worklist/mutate"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // guard-checks-the-signature-version: this is the test that matters, and it
    // is deliberately at the DENY SITE rather than at signature_version_verdict.
    // xmlui-org/xmlui-mcp#33 is this project's standing lesson: the
    // subagent-lifecycle check was tested at the wrong layer, keyed on a field
    // its payloads never carried, and was therefore inert from the day it
    // shipped without a single test failing. An enforcement claim needs a FIRE
    // behind it, not an inspection — so this drives claude_decide end to end and
    // asserts the traced reason and the agent-facing body.
    #[test]
    fn stale_signature_version_is_denied_at_the_deny_site() {
        let current = env!("CARGO_PKG_VERSION");
        let sig = |v: &str| {
            format!(
                "Jon's Claude (Bram {}, main thread, Opus 5, macOS, Tuck) speaking from the Bram project (github.com/judell/bram):\n\nBody.",
                v
            )
        };

        // The live shape: a version that is real but behind. tau#1 signed
        // "Bram 0.6.7" 10h26m after its own marker said 0.6.8.
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": format!("gh issue comment 5 --body \"{}\"", sig("0.0.1-stale"))},
            "cwd": "/",
        }));
        assert_eq!(v.reason, "crossboundary-stale-version:0.0.1-stale");
        let body = body_of(&v);
        assert!(body.contains("0.0.1-stale"), "{body}");
        assert!(body.contains(current), "{body}");
        assert!(body.contains("/__app-info"), "{body}");

        // The correct version is NOT denied for this reason — proving the
        // tripwire discriminates rather than firing on every signed write.
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": format!("gh issue comment 5 --body \"{}\"", sig(current))},
            "cwd": "/",
        }));
        assert_ne!(v.reason, "crossboundary-stale-version");
        assert!(
            !v.reason.starts_with("crossboundary-stale-version"),
            "{}",
            v.reason
        );

        // Absent slot: allowed, so the soft rollout survives.
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh issue comment 5 --body \"Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\n\nBody.\""},
            "cwd": "/",
        }));
        assert!(
            !v.reason.starts_with("crossboundary-stale-version"),
            "{}",
            v.reason
        );

        // Unsigned still reports UNSIGNED, not stale — the two denials must not
        // collapse, which is #331's lesson about naming the right failure.
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh issue comment 5 --body 'plain unsigned text'"},
            "cwd": "/",
        }));
        assert_eq!(v.reason, "crossboundary-unsigned");
    }

    #[test]
    fn message_parity_crossboundary_unsigned() {
        // python:1916-1934 — the signature deny, and python:1892-1909 for the
        // #176 antipattern, whose `Detected:` tail is `m.group(0).strip()`:
        // a whitespace opener is stripped, a `;` / `&&` / `||` opener is not.
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh issue comment 5 --body 'plain unsigned text'"},
            "cwd": "/",
        }));
        assert_eq!(v.reason, "crossboundary-unsigned");
        assert_eq!(
            body_of(&v),
            "This agent-authored forge artifact lacks the full repository-qualified signature.\nSign it with this expander, first in the body or right after a short opener:\n<details>\n<summary><owner>'s <Agent> speaking from <origin-host>/<path></summary>\n\n- Bram: v<version>\n- Model: <model>\n- Subagent: none (or the task name)\n- OS: <os>\n- Machine: <hostname -s>\n</details>\nNo origin remote found here; any host/path-shaped locator is accepted.\nThe older one-line form is still accepted.\nSee conventions.md, 'Signing agent-authored forge artifacts' — every artifact, every comment, not just the first in a thread."
        );

        let at = '@';
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": format!("echo hi && gh issue comment 5 --body {at}-")},
            "cwd": "/",
        }));
        assert_eq!(v.reason, "gh-body-at-antipattern");
        assert_eq!(
            body_of(&v),
            format!(
                "gh --body takes a literal string, not stdin or a file reference.\nUse --body-file - (stdin) or --body-file <path> instead.\nDetected: gh issue comment 5 --body {at}-"
            )
        );
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": format!("echo hi;gh issue comment 5 --body {at}x.md")},
            "cwd": "/",
        }));
        // `@\S` consumes exactly one character, so the reported span stops at
        // `@x` — and the `;` opener survives `.strip()`.
        assert_eq!(
            body_of(&v).lines().last().unwrap(),
            format!("Detected: ;gh issue comment 5 --body {at}x")
        );
    }

    #[test]
    fn message_parity_crossboundary_unsigned_names_expected_locator() {
        // issue-365: the deny used to show only the abstract template even
        // when expected_repo_locator(cwd) had computed an exact expectation
        // — the field cost was five blind signature retries in ~/budget. The
        // deny now names the checkout's own origin so there's something to
        // converge on.
        let td = scratch("signature-deny-with-origin");
        std::fs::create_dir_all(td.join(".git").join("refs")).unwrap();
        std::fs::create_dir_all(td.join(".git").join("objects")).unwrap();
        std::fs::write(td.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            td.join(".git").join("config"),
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[remote \"origin\"]\n\turl = https://github.com/judell/budget.git\n",
        )
        .unwrap();

        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh issue comment 5 --body 'plain unsigned text'"},
            "cwd": td.to_string_lossy(),
        }));
        assert_eq!(v.reason, "crossboundary-unsigned");
        assert!(
            body_of(&v).contains(
                "This checkout's origin is github.com/judell/budget — the signature's locator must be exactly that."
            ),
            "body: {}",
            body_of(&v)
        );
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn message_parity_crossboundary_unsigned_remoteless() {
        // issue-365, the other axis: a genuinely remoteless checkout states
        // the acceptance rule explicitly instead of leaving it implicit.
        let td = scratch("signature-deny-no-origin");
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh issue comment 5 --body 'plain unsigned text'"},
            "cwd": td.to_string_lossy(),
        }));
        assert_eq!(v.reason, "crossboundary-unsigned");
        assert!(
            body_of(&v)
                .contains("No origin remote found here; any host/path-shaped locator is accepted."),
            "body: {}",
            body_of(&v)
        );
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn message_parity_subagent_and_mcp_denials() {
        // python:2247-2255 (Write/Edit wording, with the extra clause),
        // python:2087-2096 + 2179-2187 (the shorter Bash / MCP wording),
        // python:2137-2143 and python:2158-2164 (the two MCP-only denials).
        let root = managed_claude("msg-subagent");
        let wl = root.join(WORKLIST_REL);
        std::fs::write(&wl, r#"{"version":1,"items":[]}"#).unwrap();

        let v = claude_decide(serde_json::json!({
            "tool_name": "Write",
            "agent_id": "agent-7",
            "tool_input": {"file_path": wl.to_string_lossy(), "content": "{}"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "subagent-worklist-write");
        assert_eq!(
            body_of(&v),
            "Worklist proposals and claim edits (.bram/worklist.json, .bram/worklist-drafts/*) stay in the orchestrator's own turn. A delegated subagent cannot write them directly -- it can widen its own item's coverage, author new items, or edit claim state with no denial. See 'Delegating worklist items to subagents' in conventions.md and judell/bram#287."
        );

        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "agent_id": "agent-7",
            "tool_input": {"command": "echo x > .bram/worklist.json"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "subagent-worklist-write");
        assert_eq!(body_of(&v), SUBAGENT_WORKLIST_WRITE_BODY);

        // judell/bram#384 retires this deny site: "blob" is an unrecognized
        // key holding a bare, slash-free token ("x") that resolves to zero
        // structural candidates, so this is now the accepted residual --
        // allow, plus the observe-only trace line -- rather than the old
        // `mcp-unrecognized-input` fail-closed deny whose message pointed
        // at input-shape recognition (the wrong lever; see the item). There
        // is no deny message left to transcribe for this case.
        let v = claude_decide(serde_json::json!({
            "tool_name": "mcp__fs__write_file",
            "tool_input": {"blob": "x"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-no-candidate-paths")
        );
        let events = std::fs::read_to_string(root.join(".bram/bram-traces/hook-events.log"))
            .unwrap_or_default();
        assert!(
            events.contains("mcp-write-verb-no-candidate-paths")
                && events.contains("mcp__fs__write_file"),
            "events: {events}"
        );

        let v = claude_decide(serde_json::json!({
            "tool_name": "mcp__fs__write_file",
            "tool_input": {"path": wl.to_string_lossy()},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "mcp-worklist-write");
        assert_eq!(
            body_of(&v),
            "mcp__fs__write_file blocked: edits to .bram/worklist.json must use Write/Edit (so the guard can validate prunes, the version bump, and inline prose) or /__worklist/mutate for mechanical transitions."
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_subagent_lifecycle_call() {
        // python:2016-2024
        let root = managed_claude("msg-lifecycle");
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "agent_id": "agent-7",
            "tool_input": {"command": "curl -sS http://127.0.0.1:1/__worklist/mutate"},
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, "subagent-lifecycle-call");
        assert_eq!(
            body_of(&v),
            "Worklist lifecycle calls (/__worklist/resolve, /__worklist/mutate, worklist-commit) stay in the orchestrator's own turn, after delegated subagents return. A subagent transcript cannot make them directly. See 'Delegating worklist items to subagents' in conventions.md."
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn message_parity_forge_sha_unresolved() {
        // python:1956-1964. cwd is a non-repo scratch dir, so `git cat-file`
        // exits nonzero and the verdict is `bad` with the SHA as its detail.
        let root = managed_claude("msg-forge-sha");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let v = claude_decide(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": format!(
                    "gh issue comment 5 --body \"Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\\n\\nSee {sha}\""
                ),
            },
            "cwd": root.to_string_lossy(),
        }));
        assert_eq!(v.reason, format!("forge-sha-unresolved:{sha}"));
        assert_eq!(
            body_of(&v),
            format!("The body contains a full commit SHA that does not resolve in this repository:\n    {sha}\nA fabricated or rebase-orphaned SHA renders as an ordinary link and 404s silently. Resolve the real hash with `git rev-parse <short-sha>` (or `git log --oneline`) and retry. See judell/bram#277.")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn forge_sha_test_git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Guard test",
                "-c",
                "user.email=guard@example.test",
            ])
            .args(args)
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn forge_sha_remote_tracking_refs_and_provider_denials() {
        let root = managed_claude("forge-sha-remote");
        forge_sha_test_git(&root, &["init", "--initial-branch=main"]);
        let tree = forge_sha_test_git(&root, &["hash-object", "-w", "-t", "tree", "--stdin"]);
        let pushed = forge_sha_test_git(&root, &["commit-tree", &tree, "-m", "base"]);
        forge_sha_test_git(&root, &["update-ref", "refs/heads/main", &pushed]);
        let command = "gh issue comment 5 --body-file comment.md";
        let write_body = |text: &str| {
            std::fs::write(
                root.join("comment.md"),
                format!("Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\n\n{text}\n"),
            ).unwrap();
        };
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": root.to_string_lossy(),
        });
        let assert_allowed = || {
            assert_eq!(forge_sha_verdict(command, &root).0, "ok");
            for provider in ["claude-rs", "codex-rs"] {
                let v = shadow_worklist_decision(provider, &payload).unwrap();
                assert_eq!(v.decision, "allow", "{provider}: {}", v.message);
            }
        };

        // A local commit in a repo with no tracking refs is still allowed,
        // including a configured remote that has never been pushed/fetched.
        write_body(&format!(
            "See https://github.com/judell/bram/commit/{pushed}"
        ));
        assert_allowed();
        forge_sha_test_git(&root, &["init", "--bare", "remote.git"]);
        forge_sha_test_git(&root, &["remote", "add", "origin", "./remote.git"]);
        assert_allowed();
        forge_sha_test_git(
            &root,
            &["push", "origin", "refs/heads/main:refs/heads/main"],
        );
        assert_allowed();

        // A descendant exists locally (including a local tag), but no remote
        // contains it. Check a body with both SHAs, not only a lone citation.
        let local =
            forge_sha_test_git(&root, &["commit-tree", &tree, "-p", &pushed, "-m", "local"]);
        forge_sha_test_git(&root, &["update-ref", "refs/heads/main", &local]);
        forge_sha_test_git(&root, &["update-ref", "refs/tags/local-only", &local]);
        write_body(&format!(
            "Base {pushed}; see https://github.com/judell/bram/commit/{local}"
        ));
        assert_eq!(
            forge_sha_verdict(command, &root),
            ("unpushed", local.clone())
        );
        let claude = claude_decide(payload.clone());
        assert_eq!(claude.decision, "deny");
        assert_eq!(claude.reason, format!("forge-sha-unpushed:{local}"));
        assert_eq!(body_of(&claude), forge_sha_unpushed_message(&local));
        let codex = shadow_worklist_decision("codex-rs", &payload).unwrap();
        assert_eq!(codex.decision, "deny");
        assert!(codex.message.contains(&forge_sha_unpushed_message(&local)));
        assert!(codex.message.contains("Push the commit first"));
        assert!(codex.message.contains("short SHA in backticks"));

        // The suggested non-link citation works without pushing.
        write_body(&format!("Local commit `{}`", &local[..8]));
        assert_allowed();

        // Any remote counts (URL matching is deliberately outside #391).
        forge_sha_test_git(&root, &["remote", "rename", "origin", "fork"]);
        forge_sha_test_git(&root, &["push", "fork", "refs/heads/main:refs/heads/main"]);
        write_body(&format!("Base {pushed}; now pushed {local}"));
        assert_allowed();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn expander_optional_fields_parse_and_commit_is_checked() {
        let with = "<details>\n<summary>Jon's Claude speaking from github.com/judell/bram</summary>\n\n- Bram: v0.8.6\n- Model: Opus 5.5\n- Subagent: none\n- OS: macOS\n- Machine: Tuck\n- Commit: COMMIT+dirty\n- Item: forge-signature-as-expander\n- Agent CLI: Claude Code 2.1.322\n</details>\n";
        let first = with.replace("COMMIT", "0b2ce8f3");
        let (sig, _) = parse_body_signature(&first).unwrap();
        assert_eq!(sig.commit, Some("0b2ce8f3"));
        assert_eq!(sig.item, Some("forge-signature-as-expander"));
        assert_eq!(sig.agent_cli, Some("Claude Code 2.1.322"));
        let (plain, _) = parse_body_signature(EXPANDER).unwrap();
        assert_eq!(
            (plain.commit, plain.item, plain.agent_cli),
            (None, None, None)
        );
        let (one, _) = parse_body_signature(
            "Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):",
        )
        .unwrap();
        assert_eq!((one.commit, one.item, one.agent_cli), (None, None, None));

        let root = scratch("expander-commit");
        forge_sha_test_git(&root, &["init", "--initial-branch=main"]);
        let tree = forge_sha_test_git(&root, &["hash-object", "-w", "-t", "tree", "--stdin"]);
        let head = forge_sha_test_git(&root, &["commit-tree", &tree, "-m", "base"]);
        forge_sha_test_git(&root, &["update-ref", "refs/heads/main", &head]);
        let command = "gh issue comment 5 --body-file comment.md";
        let verdict = |body: &str| {
            std::fs::write(root.join("comment.md"), body).unwrap();
            forge_sha_verdict(command, &root)
        };
        // Resolvable (short, with +dirty) is allowed; none is allowed.
        let good = with.replace("COMMIT", &head[..8]);
        assert_eq!(verdict(&good).0, "ok");
        assert_eq!(verdict(EXPANDER).0, "ok");
        // Unresolvable is denied with the unresolved-sha reason.
        let bad = with.replace("COMMIT", "deadbeef");
        assert_eq!(verdict(&bad), ("bad", "deadbeef".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn forge_sha_missing_git_fails_open() {
        let root = scratch("forge-sha-missing-git");
        let command = "gh issue comment 5 --body '0123456789abcdef0123456789abcdef01234567'";
        assert_eq!(
            forge_sha_verdict_with_git(command, &root, &root.join("missing-git")),
            ("unparsed", "git-unavailable".into())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn forge_sha_failed_remote_probes_fail_open() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("forge-sha-failed-probes");
        let git = root.join("fake-git");
        let command = "gh issue comment 5 --body '0123456789abcdef0123456789abcdef01234567'";
        for (script, detail) in [
            (
                "#!/bin/sh\nif [ \"$1\" = cat-file ]; then exit 0; fi\nexit 2\n",
                "git-remote-refs-error",
            ),
            (
                "#!/bin/sh\nif [ \"$1\" = cat-file ]; then exit 0; fi\ncase \"$*\" in *--contains*) exit 2;; esac\nprintf 'refs/remotes/origin/main\\n'\n",
                "git-remote-contains-error",
            ),
        ] {
            std::fs::write(&git, script).unwrap();
            std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                forge_sha_verdict_with_git(command, &root, &git),
                ("unparsed", detail.into())
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

// --- codex parity suite -------------------------------------------------------
//
// A port of `self_test()` in app/provider-hooks/codex-worklist-guard.py
// (lines 973-1198). Every Python assertion has a twin here, in source order,
// except the four covering `write_direct_edit_record` (lines 1098-1103): the
// shadow never writes, so that surface is replaced by a test asserting the
// decision is reproduced AND the record is NOT written. The judell/bram#299
// cases assert the NEW classification and are labelled.

#[cfg(test)]
mod codex_guard_policy_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bram-codex-policy-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Python `_self_test_replacement_patch` (lines 962-970).
    fn replacement_patch(old_content: &str, new_content: &str) -> String {
        let mut out = format!("*** Begin Patch\n*** Update File: {}\n@@\n", WORKLIST_REL);
        for line in old_content.split_inclusive('\n') {
            out.push('-');
            out.push_str(line);
        }
        for line in new_content.split_inclusive('\n') {
            out.push('+');
            out.push_str(line);
        }
        out.push_str("*** End Patch\n");
        out
    }

    fn pretty(v: &Value) -> String {
        serde_json::to_string_pretty(v).unwrap() + "\n"
    }

    // --- deny-reason derivation (python lines 974-981 + deny(), lines 172-190) --
    //
    // The Python `pre_tool_use_deny_response` shape is a stdout contract the
    // shadow deliberately does not emit; what carries over is `deny()`'s traced
    // reason, which is `message.splitlines()[0][:120]`.

    #[test]
    fn deny_reason_is_first_line_truncated_to_120() {
        assert_eq!(codex_deny_reason("one line"), "one line");
        assert_eq!(codex_deny_reason("first\nsecond\nthird"), "first");
        let long = "x".repeat(200);
        assert_eq!(codex_deny_reason(&long).chars().count(), 120);
        assert_eq!(codex_deny_reason(""), "blocked");
        // The four canonical first lines, pinned against the Python messages.
        assert_eq!(
            codex_deny_reason(&worklist_validation_error(
                &vec![("i1".to_string(), vec!["id".to_string()])],
                "apply_patch"
            )),
            "apply_patch blocked: proposed worklist item(s) violate draft-only rule."
        );
        assert!(codex_deny_reason(&mechanical_worklist_change_error(
            &[("i1".to_string(), "proposed".to_string())],
            &[],
            "apply_patch"
        ))
        .starts_with("apply_patch blocked: mechanical worklist state changes must go through"));
        assert!(
            codex_deny_reason(&stale_worklist_version_error(1, 1, true, "apply_patch"))
                .starts_with(
                    "apply_patch blocked: stale base on .bram/worklist.json. You set version=1,"
                )
        );
        // coverage_root_line opens with a newline, so it never reaches a reason.
        assert!(coverage_root_line(Path::new("/tmp")).starts_with('\n'));
    }

    // --- bash write / read corpus (python lines 983-1013) ----------------------

    #[test]
    fn codex_bash_write_commands_are_writes() {
        for c in [
            "echo x > out.txt",
            "printf x >> out.txt",
            "git commit -m test",
            "rm stale.txt",
            "echo hi > /dev/stdout",
            "cat > src-tauri/src/lib.rs",
            "cat > /tmp/b.md && git commit -m x",
        ] {
            assert!(codex_bash_writes(c), "{c}");
        }
    }

    #[test]
    fn codex_bash_read_commands_are_reads() {
        for c in [
            "ls -la",
            "git status --short",
            "curl -I https://example.com",
            "git ls-files x >/dev/null 2>&1",
            "echo hi > /dev/null",
            "echo hi >> /dev/zero",
            "cat > /tmp/body.md",
            "cat > /private/tmp/x/body.md",
        ] {
            assert!(!codex_bash_writes(c), "{c}");
        }
    }

    /// Not in the Python self-test, but the load-bearing difference between the
    /// two guards' `_BASH_WRITE_PATTERNS`: Codex carries no `gh issue <verb>`
    /// entry, so forge issue writes are simply not writes on this side (and
    /// there is no `forge_issue_only_write` exemption to compensate).
    #[test]
    fn codex_has_no_gh_issue_write_pattern() {
        for c in [
            "gh issue close 119 --repo judell/bram",
            "gh issue delete 5",
            "gh issue transfer 5 other/repo",
            "gh issue pin 5",
        ] {
            assert!(!codex_bash_writes(c), "{c}");
            assert!(bash_writes(c), "claude side still writes: {c}");
        }
    }

    // --- apply_patch reconstruction + validators (python lines 1015-1085) ------

    #[test]
    fn apply_patch_worklist_reconstruction_and_validators() {
        let root = scratch("patch");
        crate::guard_policy::test_make_state_dir(&root).unwrap();

        let old_doc = serde_json::json!({
            "description": "",
            "items": [{"id": "existing", "status": "proposed", "file": "a.txt"}]
        });
        let old_content = pretty(&old_doc);
        std::fs::write(root.join(WORKLIST_REL), &old_content).unwrap();

        let new_item_doc = serde_json::json!({
            "description": "",
            "items": [
                {"id": "existing", "status": "proposed", "file": "a.txt"},
                {"id": "new-item", "status": "proposed", "file": "b.txt"}
            ]
        });
        let new_item_content = pretty(&new_item_doc);
        let applied = worklist_content_after_apply_patch(
            &root,
            &replacement_patch(&old_content, &new_item_content),
        );
        assert_eq!(applied.as_deref(), Some(new_item_content.as_str()));
        let applied = applied.unwrap();
        assert_eq!(
            worklist_items_violating_draft_only(&applied),
            Some(Vec::new())
        );
        assert_eq!(
            worklist_state_changes(
                &codex_items_by_id(&old_content),
                &codex_items_by_id(&applied)
            ),
            (Vec::new(), Vec::new())
        );

        // Inline prose on a new proposed item is rejected.
        let inline_doc = serde_json::json!({
            "description": "",
            "items": [
                {"id": "existing", "status": "proposed", "file": "a.txt"},
                {"id": "inline-item", "status": "proposed", "file": "b.txt",
                 "before": "old", "after": "new"}
            ]
        });
        let inline_content = pretty(&inline_doc);
        let inline_bad = worklist_items_violating_draft_only(&inline_content).unwrap();
        assert!(!inline_bad.is_empty());
        assert_eq!(inline_bad[0].0, "inline-item");
        // The patch-text validator also flags an apply_patch adding inline prose.
        let inline_patch_bad =
            patch_adds_violating_draft_only(&replacement_patch(&old_content, &inline_content));
        assert!(!inline_patch_bad.is_empty());
        assert!(inline_patch_bad[0].1.join(" ").contains("inline"));

        // A status transition is a mechanical change.
        let transitioned =
            old_content.replace("\"status\": \"proposed\"", "\"status\": \"applied\"");
        assert_eq!(
            worklist_state_changes(
                &codex_items_by_id(&old_content),
                &codex_items_by_id(&transitioned)
            ),
            (
                Vec::new(),
                vec![(
                    "existing".to_string(),
                    "proposed".to_string(),
                    "applied".to_string()
                )]
            )
        );

        let removed = pretty(&serde_json::json!({"description": "", "items": []}));
        assert_eq!(
            worklist_state_changes(
                &codex_items_by_id(&old_content),
                &codex_items_by_id(&removed)
            ),
            (
                vec![("existing".to_string(), "proposed".to_string())],
                Vec::new()
            )
        );

        // `_patch_removes_worklist_items` is the fallback when the post-patch
        // file cannot be rebuilt (not asserted in the Python self-test, but it
        // is the other half of the touches-worklist branch).
        let prune_patch = format!(
            "*** Begin Patch\n*** Update File: {}\n@@\n-    \"id\": \"existing\",\n*** End Patch\n",
            WORKLIST_REL
        );
        assert_eq!(
            patch_removes_worklist_items(&root, &prune_patch),
            vec![("existing".to_string(), "proposed".to_string())]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_targets_reads_both_patch_dialects() {
        let ti = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: app/a.txt\n*** Add File: app/b.txt\n\
        *** Delete File: app/c.txt   \n*** End Patch\n"
        });
        assert_eq!(
            patch_targets(&ti),
            vec!["app/a.txt", "app/b.txt", "app/c.txt"]
        );
        assert_eq!(
            patch_targets(&serde_json::json!({"patch": "--- a/x\n+++ b/app/d.txt\n"})),
            vec!["app/d.txt"]
        );
        assert!(patch_targets(&serde_json::json!({"content": "no patch here"})).is_empty());
        assert!(patch_targets(&serde_json::json!({})).is_empty());
    }

    // --- opt-out parity (python lines 1087-1103) -------------------------------

    #[test]
    fn codex_opt_out_phrase_parity() {
        assert!(has_opt_out("just do it"));
        assert!(!has_opt_out("Skip the worklist"));
        assert!(!has_opt_out("commit this directly"));
        assert!(!has_opt_out("looks good"));
        assert!(!has_opt_out("go ahead and merge"));
        assert!(!has_opt_out(""));
    }

    /// Replaces the Python self-test's four `write_direct_edit_record`
    /// assertions (lines 1098-1103). The shadow reproduces the DECISION on the
    /// opt-out path and writes nothing: the record the Python guard would have
    /// created is the one side effect this port must not have.
    #[test]
    fn user_prompt_opt_out_decides_without_writing_the_record() {
        let root = scratch("prompt");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();

        let v = shadow_worklist_decision(
            "codex-rs",
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": "patch the tooltip, just do it",
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );
        assert_eq!(
            std::fs::read_to_string(root.join(AUTH_REL)).unwrap(),
            "{}",
            "shadow wrote a direct-edit record"
        );

        // A change-shaped prompt injects the reminder, which the Python guard
        // emits with NO trace line at all.
        let v = shadow_worklist_decision(
            "codex-rs",
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": "please fix the tooltip so it stops clipping in the footer",
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "no-trace:gate-reminder")
        );

        // Short / non-change prompts pass through.
        let v = shadow_worklist_decision(
            "codex-rs",
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": "what is in the worklist",
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        // Unmanaged cwd: never inspected.
        let bare = scratch("prompt-bare");
        let v = shadow_worklist_decision(
            "codex-rs",
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": "please fix the tooltip so it stops clipping in the footer",
                "cwd": bare.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn change_request_heuristic_mirrors_the_keyword_regex() {
        assert!(!looks_like_change_request("fix it")); // < 30 chars
        assert!(looks_like_change_request(
            "could you take a look at the footer alignment there"
        ));
        assert!(looks_like_change_request(
            "the tooltip is broken on the second row of the table"
        ));
        assert!(!looks_like_change_request(
            "where does the projection broadcast happen in this codebase"
        ));
        // `\b` on both ends: an embedded stem does not match.
        assert!(!looks_like_change_request(
            "the address book listing shows nothing at all for me here"
        ));
    }

    // --- signature gates (python lines 1105-1158) ------------------------------
    //
    // Byte-identical to the Claude guard's, and reused from the shared core;
    // pinned here so a change to either Python copy shows up on both sides.

    // forge-body-fold-enforced: the Codex deny site traces the stable reason
    // code, not the message's first line, and shares the Claude message.
    #[test]
    fn codex_long_forge_body_unfolded_is_denied() {
        let td = scratch("codex-forge-body-fold");
        let sig = "Jon's Codex (main thread, GPT-5.6-sol) speaking from the Bram project (github.com/judell/bram):";
        let lines: String = (1..=40).map(|n| format!("- finding {n}\n")).collect();
        let body = format!("{sig}\n\n{lines}");
        let p = td.join("long.md");
        std::fs::write(&p, &body).unwrap();
        let cmd = format!("gh issue comment 5 --body-file {}", p.display());
        let v = codex_bash_branch(&td, &serde_json::json!({ "command": cmd }), &HashSet::new());
        assert_eq!(v.decision, "deny");
        assert_eq!(v.reason, "long-forge-body-unfolded");
        let chars = body[sig.len()..].trim().chars().count();
        assert_eq!(v.message, long_forge_body_unfolded_message(40, chars));
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn codex_crossboundary_signature_gates() {
        let td = scratch("codex-sig");
        let signed = "Jon's Claude (main thread, Opus 5) speaking from the Bram project (github.com/judell/bram):\n\nBody.";
        let unsigned = "Vendored the build; the tooltip renders.";
        let verdict = |cmd: &str| crossboundary_signature_verdict(cmd, &td).0;

        assert_eq!(verdict("gh issue view 12 --repo xmlui-org/xmlui"), "skip");
        assert_eq!(verdict("ls -la"), "skip");
        assert_eq!(
            verdict(&format!("gh issue comment 5 --body \"{}\"", unsigned)),
            "unsigned"
        );
        assert_eq!(
            verdict(&format!("gh issue comment 5 --body \"{}\"", signed)),
            "signed"
        );
        assert_eq!(
            verdict(&format!(
                "gh issue comment 5 --repo judell/bram --body \"{}\"",
                unsigned
            )),
            "unsigned"
        );
        assert!(!td.join(".git").join("config").exists());
        assert_eq!(
            verdict(&format!(
                "gh issue comment 5 --repo xmlui-org/xmlui --body \"{}\"",
                signed
            )),
            "signed"
        );
        assert_eq!(
            verdict(&format!(
                "gh issue comment 5 --repo xmlui-org/xmlui --body \"{}\"",
                unsigned
            )),
            "unsigned"
        );
        assert_eq!(
            verdict(&format!("gh pr comment 9 --body \"{}\"", unsigned)),
            "unsigned"
        );
        assert_eq!(
            verdict(&format!("glab issue note 3 -m \"{}\"", unsigned)),
            "unsigned"
        );
        assert_eq!(verdict("gh issue comment 5 --body-file -"), "unparsed");
        assert_eq!(
            verdict("gh issue comment 5 --body-file /nope/missing.md"),
            "unparsed"
        );
        assert_eq!(verdict("gh issue comment 5"), "unparsed");
        assert_eq!(verdict("gh issue close 5"), "skip");

        let body_path = td.join("comment.md");
        std::fs::write(&body_path, signed).unwrap();
        assert_eq!(
            crossboundary_signature_verdict(
                &format!("gh issue comment 5 --body-file {}", body_path.display()),
                &td
            ),
            ("signed", "body-file")
        );

        assert!(body_is_signed(
            "_Jon's Codex (main thread, GPT-5.6-sol) speaking from the XMLUI project (github.com/xmlui-org/xmlui):_",
            &td
        ));
        assert!(body_is_signed(
            "> **Jon's Claude (subagent, Opus 5) speaking from the Bram project (github.com/judell/bram):**",
            &td
        ));
        assert!(!body_is_signed(
            "Bram side — a correction to my green light.",
            &td
        ));
        assert!(!body_is_signed("", &td));
        let _ = std::fs::remove_dir_all(&td);
    }

    // --- pending-intent gate (python lines 1160-1198) --------------------------

    #[test]
    fn intent_write_verdict_gate() {
        let root = scratch("intent");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        let intent_path = root.join(WORKLIST_INTENT_REL);

        assert_eq!(
            intent_write_verdict(&root),
            ("allow", IntentDetail::NoPending)
        );

        std::fs::write(
            &intent_path,
            serde_json::json!({"nonce": "n1", "route": "worklist-mutate", "body": {}}).to_string(),
        )
        .unwrap();
        let (verdict, detail) = intent_write_verdict(&root);
        assert_eq!(verdict, "deny");
        match &detail {
            IntentDetail::Pending {
                nonce,
                route,
                age_seconds,
            } => {
                assert_eq!(nonce.as_deref(), Some("n1"));
                assert_eq!(route.as_deref(), Some("worklist-mutate"));
                assert!((*age_seconds as f64) < INTENT_STALE_SECONDS);
            }
            other => panic!("expected Pending, got {:?}", other),
        }

        // Backdated mtime -> allow, traced as an overwrite rather than a
        // collision. `os.utime` has no std equivalent; `touch -t` is the port.
        let touched = std::process::Command::new("touch")
            .arg("-t")
            .arg("202001010000")
            .arg(&intent_path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if touched {
            assert_eq!(
                intent_write_verdict(&root),
                ("allow", IntentDetail::StaleOverwrite)
            );
        }

        // Unparsable but FRESH -> deny, with nonce/route reported as None.
        std::fs::write(&intent_path, "not json").unwrap();
        let (verdict, detail) = intent_write_verdict(&root);
        assert_eq!(verdict, "deny");
        match &detail {
            IntentDetail::Pending { nonce, route, .. } => {
                assert!(nonce.is_none() && route.is_none());
            }
            other => panic!("expected Pending, got {:?}", other),
        }
        let reason = codex_deny_reason(&intent_pending_message("Bash", &detail));
        assert!(
            reason.starts_with(
                "Bash blocked: .bram/.worklist-intent.json already holds a pending \
request (route=None, nonce=None, age=0s)"
            ),
            "reason: {reason}"
        );
        assert_eq!(reason.chars().count(), 120);
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- judell/bram#299 inheritance ------------------------------------------

    #[test]
    fn issue_299_codex_inherits_all_three_divergences() {
        // Case 1: pure forge reads. This guard never had a `gh` write pattern,
        // so the predicate changes no verdict here — pinned so a future
        // widening cannot silently recapture it.
        for c in [
            "gh issue list",
            "gh issue list --state all --json number,title,state,updatedAt --limit 200",
            "gh issue view 299 --json title,body",
            "glab issue list --state opened",
        ] {
            assert!(forge_read_command(c), "{c}");
            assert!(!codex_bash_writes(c), "{c}");
        }
        assert!(codex_bash_writes("gh issue list && git commit -m x"));

        // Case 2: inline `python -c` is classified on what the script does.
        for c in [
            "python -c \"print(1)\"",
            "python3 -c 'import sys; print(sys.version)'",
        ] {
            assert!(!codex_bash_writes(c), "{c}");
        }
        for c in [
            "python -c \"open('x', 'w').write('x')\"",
            "python3 -c \"import shutil; shutil.rmtree('x')\"",
            "python3 -c $SCRIPT",
        ] {
            assert!(codex_bash_writes(c), "{c}");
        }

        // Case 3: a `>` inside quotes is a comparison, not a redirect.
        for c in [
            "awk '$1 >= \"2026-08-28T06:16\"' file.txt",
            "jq '.[] | select(.n > 5)' data.json",
            "grep -E \"a > b\" file",
        ] {
            assert!(!codex_bash_writes(c), "{c}");
            assert!(!has_redirect(c), "{c}");
        }
        assert!(codex_bash_writes("echo x > out.txt"));
    }

    // --- codex-specific helpers ------------------------------------------------

    #[test]
    fn codex_helpers_match_the_python_predicates() {
        assert!(is_worklist_citation(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-citations/x.json"
        ));
        assert!(!is_worklist_citation(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-citations/sub/x.json"
        ));
        assert!(!is_worklist_citation(
            Path::new("/nonexistent-issue-469"),
            ".bram/worklist-citations/x.md"
        ));
        assert!(is_coordination_file(
            Path::new("/nonexistent-issue-469"),
            WORKLIST_INTENT_REL
        ));
        assert!(is_coordination_file(
            Path::new("/nonexistent-issue-469"),
            WORKLIST_RESULT_REL
        ));
        assert!(!is_coordination_file(
            Path::new("/nonexistent-issue-469"),
            WORKLIST_REL
        ));
        // Codex normalize_target joins against the PROJECT cwd, not the process cwd.
        let root = Path::new("/tmp/proj");
        assert_eq!(
            codex_normalize_target(root, "app/x.txt").as_deref(),
            Some("app/x.txt")
        );
        assert_eq!(
            codex_normalize_target(root, "/tmp/proj/app/x.txt").as_deref(),
            Some("app/x.txt")
        );
        assert_eq!(codex_normalize_target(root, "/etc/hosts"), None);
        assert_eq!(
            codex_normalize_target(root, "/tmp/proj").as_deref(),
            Some("")
        );
        assert_eq!(codex_normalize_target(root, ""), None);
        // Python `.splitlines()` parity for the patch reader.
        assert_eq!(py_splitlines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(py_splitlines("a\n"), vec!["a"]);
        assert!(py_splitlines("").is_empty());
        // items_by_id skips blank/non-string ids instead of collapsing the map.
        let text = r#"{"items":[{"id":"a"},{"no":"id"},{"id":""},{"id":"b"}]}"#;
        let m = codex_items_by_id(text);
        assert_eq!(
            m.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!(items_by_id(text).is_none(), "claude side collapses");
    }

    #[test]
    fn codex_fresh_bypass_reads_snake_case_only() {
        let root = scratch("bypass");
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        // camelCase issuedAtMs is NOT read by this guard: with no snake_case
        // field the record reads as issued at epoch 0, hence stale.
        std::fs::write(
            root.join(AUTH_REL),
            serde_json::json!({"kind": "direct-edit", "paths": ["*"], "issuedAtMs": now_ms()})
                .to_string(),
        )
        .unwrap();
        assert!(!codex_fresh_bypass(&root, "any/path"));
        assert!(fresh_bypass(&root, "any/path"), "claude side accepts it");

        std::fs::write(
            root.join(AUTH_REL),
            serde_json::json!({"kind": "direct-edit", "paths": ["*"], "issued_at_ms": now_ms()})
                .to_string(),
        )
        .unwrap();
        assert!(codex_fresh_bypass(&root, "any/path"));

        std::fs::write(
            root.join(AUTH_REL),
            serde_json::json!({
                "kind": "direct-edit", "paths": ["app/x.txt"], "issued_at_ms": now_ms()
            })
            .to_string(),
        )
        .unwrap();
        assert!(codex_fresh_bypass(&root, "app/x.txt"));
        assert!(!codex_fresh_bypass(&root, "app/y.txt"));

        std::fs::write(
            root.join(AUTH_REL),
            serde_json::json!({
                "kind": "direct-edit", "paths": ["*"],
                "issued_at_ms": now_ms() - BYPASS_TTL_MS - 60_000.0
            })
            .to_string(),
        )
        .unwrap();
        assert!(!codex_fresh_bypass(&root, "any/path"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- pipeline-level checks -------------------------------------------------

    fn managed(name: &str) -> PathBuf {
        let root = scratch(name);
        crate::guard_policy::test_make_state_dir(&root).unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();
        root
    }

    fn decide(root: &Path, tool: &str, ti: Value) -> ShadowVerdict {
        shadow_worklist_decision(
            "codex-rs",
            &serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": tool,
                "tool_input": ti,
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap()
    }

    #[test]
    fn unmanaged_cwd_allows_everything() {
        let root = scratch("unmanaged");
        let v = decide(&root, "Bash", serde_json::json!({"command": "rm -rf x"}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_bash_pipeline_branches() {
        let root = managed("codex-bash");

        let v = decide(&root, "Bash", serde_json::json!({"command": "ls -la"}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "git ls-files x >/dev/null 2>&1"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "bash-write-nonrepo-target")
        );

        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "rm stale.txt"}),
        );
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason.starts_with(
                "Bash blocked: this command writes to the filesystem, and \
.bram/worklist.json has no proposed"
            ),
            "reason: {}",
            v.reason
        );
        assert_eq!(v.reason.chars().count(), 120);

        // Any coverage at all opens the Bash gate.
        std::fs::write(
            root.join(WORKLIST_REL),
            serde_json::json!({
                "version": 1,
                "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
            })
            .to_string(),
        )
        .unwrap();
        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "rm stale.txt"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );
        std::fs::remove_file(root.join(WORKLIST_REL)).unwrap();

        // The drafts substring and the intent channel are lifecycle writes.
        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "echo x > .bram/worklist-drafts/a.md"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "echo '{}' > .bram/.worklist-intent.json"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        std::fs::write(root.join(WORKLIST_INTENT_REL), "{\"nonce\":\"n9\"}").unwrap();
        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "echo '{}' > .bram/.worklist-intent.json"}),
        );
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("Bash blocked: .bram/.worklist-intent.json already holds a pending"),
            "reason: {}",
            v.reason
        );
        std::fs::remove_file(root.join(WORKLIST_INTENT_REL)).unwrap();

        // gh --body @ still denies, ahead of everything else.
        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "gh issue comment 5 --body @body.md"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            (
                "deny",
                "gh --body takes a literal string, not stdin or a file reference."
            )
        );

        // Unsigned forge write.
        let v = decide(
            &root,
            "Bash",
            serde_json::json!({"command": "gh issue comment 5 --body \"no signature here\""}),
        );
        assert_eq!(v.decision, "deny");
        assert!(v.reason.starts_with(
            "This agent-authored forge artifact lacks the full repository-qualified signature"
        ));

        // A push with a fresh consumed approval rides the grace.
        std::fs::write(
            root.join(AUTH_REL),
            serde_json::json!({"kind": "approved", "consumedAtMs": now_ms()}).to_string(),
        )
        .unwrap();
        let v = decide(&root, "Bash", serde_json::json!({"command": "git push"}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "post-commit-push-grace")
        );
        std::fs::write(root.join(AUTH_REL), "{}").unwrap();

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_apply_patch_pipeline_branches() {
        let root = managed("codex-patch-pipe");

        // Unparseable patch payload.
        let v = decide(
            &root,
            "apply_patch",
            serde_json::json!({"input": "nothing"}),
        );
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("apply_patch blocked: could not parse target file(s)"),
            "reason: {}",
            v.reason
        );

        // Uncovered project file.
        let patch = "*** Begin Patch\n*** Update File: app/x.txt\n@@\n-a\n+b\n*** End Patch\n";
        let v = decide(&root, "apply_patch", serde_json::json!({"input": patch}));
        assert_eq!(v.decision, "deny");
        assert_eq!(v.target, "app/x.txt");
        assert!(
            v.reason
                .starts_with("apply_patch blocked: app/x.txt is not covered by any proposed"),
            "reason: {}",
            v.reason
        );

        // Covered by a proposed item. The worklist file is newline-terminated
        // because `replacement_patch` is a faithful port of the Python helper,
        // which builds one patch line per `splitlines(True)` chunk.
        std::fs::write(
            root.join(WORKLIST_REL),
            serde_json::json!({
                "version": 1,
                "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        let v = decide(&root, "apply_patch", serde_json::json!({"input": patch}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        // Drafts, citations, and the result file are exempt without coverage.
        for (rel, label) in [
            (".bram/worklist-drafts/i1.md", "draft"),
            (".bram/worklist-citations/i1.json", "citation"),
            (WORKLIST_RESULT_REL, "result"),
        ] {
            let p = format!(
                "*** Begin Patch\n*** Update File: {}\n@@\n-a\n+b\n*** End Patch\n",
                rel
            );
            let v = decide(&root, "apply_patch", serde_json::json!({"input": p}));
            assert_eq!(
                (v.decision.as_str(), v.reason.as_str()),
                ("allow", "passed-checks"),
                "{label}"
            );
        }

        // A patch that prunes an item is a mechanical change.
        let old = std::fs::read_to_string(root.join(WORKLIST_REL)).unwrap();
        let prune = replacement_patch(
            &old,
            &(serde_json::json!({"version": 2, "items": []}).to_string() + "\n"),
        );
        let v = decide(&root, "apply_patch", serde_json::json!({"input": prune}));
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("apply_patch blocked: mechanical worklist state changes"),
            "reason: {}",
            v.reason
        );

        // A stale version bump.
        let stale = replacement_patch(
            &old,
            &(serde_json::json!({
                "version": 1,
                "items": [
                    {"id": "i1", "status": "proposed", "files": ["app/x.txt"]},
                    {"id": "i2", "status": "proposed", "files": ["app/y.txt"]}
                ]
            })
            .to_string()
                + "\n"),
        );
        let v = decide(&root, "apply_patch", serde_json::json!({"input": stale}));
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("apply_patch blocked: stale base on .bram/worklist.json."),
            "reason: {}",
            v.reason
        );

        // A correct bump authors cleanly.
        let good = replacement_patch(
            &old,
            &(serde_json::json!({
                "version": 2,
                "items": [
                    {"id": "i1", "status": "proposed", "files": ["app/x.txt"]},
                    {"id": "i2", "status": "proposed", "files": ["app/y.txt"]}
                ]
            })
            .to_string()
                + "\n"),
        );
        let v = decide(&root, "apply_patch", serde_json::json!({"input": good}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        // Inline prose on a proposed item.
        let prose = replacement_patch(
            &old,
            &(serde_json::json!({
                "version": 2,
                "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"], "before": "x"}]
            })
            .to_string()
                + "\n"),
        );
        let v = decide(&root, "apply_patch", serde_json::json!({"input": prose}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            (
                "deny",
                "apply_patch blocked: proposed worklist item(s) violate draft-only rule."
            )
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn issue_309_codex_worktree_pipeline_maps_only_coverage() {
        let root = managed("codex-worktree");
        std::fs::write(
            root.join(WORKLIST_REL),
            serde_json::json!({
                "version": 1,
                "items": [{
                    "id": "i1",
                    "status": "proposed",
                    "files": ["app/x.txt", "components"]
                }]
            })
            .to_string()
                + "\n",
        )
        .unwrap();

        let patch = |rel: &str| {
            format!(
                "*** Begin Patch\n*** Update File: {}\n@@\n-a\n+b\n*** End Patch\n",
                rel
            )
        };

        let v = decide(
            &root,
            "apply_patch",
            serde_json::json!({
                "input": patch(".claude/worktrees/agent-a/app/x.txt")
            }),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );
        assert_eq!(v.target, ".claude/worktrees/agent-a/app/x.txt");

        let v = decide(
            &root,
            "apply_patch",
            serde_json::json!({
                "input": patch(
                    ".claude/worktrees/agent-a/components/nested/y.xmlui"
                )
            }),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        let v = decide(
            &root,
            "apply_patch",
            serde_json::json!({
                "input": patch(".claude/worktrees/agent-a/app/y.txt")
            }),
        );
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason.ends_with(":worktree=agent-a"),
            "reason: {}",
            v.reason
        );
        assert_eq!(v.target, ".claude/worktrees/agent-a/app/y.txt");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_mcp_pipeline_branches() {
        let root = managed("codex-mcp");

        let v = decide(
            &root,
            "mcp__filesystem__read_text_file",
            serde_json::json!({"path": "app/x.txt"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        // judell/bram#384 retires #360's fail-closed-then-bypass design for
        // this exact shape: "contents" was never a recognized key, and its
        // value "x" is a bare, slash-free token resolving to zero structural
        // candidates, so this call cannot name a repo file at all. It is now
        // allowed outright -- no direct-edit grant required -- with the
        // observe-only residue traced.
        let v = decide(
            &root,
            "mcp__filesystem__write_file",
            serde_json::json!({"contents": "x"}),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "mcp-no-candidate-paths")
        );
        let events = std::fs::read_to_string(root.join(".bram/bram-traces/hook-events.log"))
            .unwrap_or_default();
        assert!(
            events.contains("mcp-write-verb-no-candidate-paths")
                && events.contains("mcp__filesystem__write_file"),
            "events: {events}"
        );

        let v = decide(
            &root,
            "mcp__filesystem__write_file",
            serde_json::json!({"path": root.join("app/y.txt").to_string_lossy()}),
        );
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("mcp__filesystem__write_file blocked: app/y.txt is not covered"),
            "reason: {}",
            v.reason
        );

        // Unlike the Claude guard, a worklist.json write via MCP is validated
        // rather than denied outright.
        let v = decide(
            &root,
            "mcp__filesystem__write_file",
            serde_json::json!({
                "path": root.join(WORKLIST_REL).to_string_lossy(),
                "content": serde_json::json!({
                    "version": 1,
                    "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
                }).to_string(),
            }),
        );
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        // No inspectable content -> the mutate-route denial.
        let v = decide(
            &root,
            "mcp__filesystem__write_file",
            serde_json::json!({"path": root.join(WORKLIST_REL).to_string_lossy()}),
        );
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason.starts_with(
                "mcp__filesystem__write_file blocked: worklist edits that advance status"
            ),
            "reason: {}",
            v.reason
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_tool_names_pass_through() {
        let root = managed("codex-other");
        let v = decide(&root, "Read", serde_json::json!({"file_path": "x"}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

// issue-469: the state folder's own cases. Since issue-469-retire-resources
// the coverage above builds its fixtures under `.bram/` too; these check what
// counts as a state folder, and that the pre-0.8.4 `resources/` is an
// ordinary project folder to the guard.
#[cfg(test)]
mod state_dir_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bram-state-dir-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// A `.bram/` as Bram makes it: `.gitignore` of `*` (judell/bram#469).
    fn mark_dot_bram(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(".gitignore"), "*\n").unwrap();
    }

    #[test]
    fn foreign_dot_bram_is_not_adopted() {
        let root = scratch("foreign");
        // Index-style files, no marker: not Bram's, so not a state folder.
        std::fs::create_dir_all(root.join(".bram")).unwrap();
        std::fs::write(root.join(".bram/index.json"), "{}").unwrap();
        assert!(!is_bram_state_dir(&root));
        assert!(!has_state_dir(&root));
        assert!(state_dir_candidates(&root).is_empty());
        // A `.gitignore` that is not exactly `*` is not the marker.
        std::fs::write(root.join(".bram/.gitignore"), "node_modules\n").unwrap();
        assert!(!has_state_dir(&root));
        std::fs::write(root.join(".bram/.gitignore"), "*\n").unwrap();
        assert!(has_state_dir(&root));
        assert_eq!(state_dir_candidates(&root), vec![root.join(".bram")]);
        std::fs::remove_file(root.join(".bram/.gitignore")).unwrap();
        std::fs::write(root.join(".bram/.migrated-from"), "x 2026-11-11T00:00:00Z").unwrap();
        assert!(has_state_dir(&root));
    }

    #[test]
    fn foreign_dot_bram_hint_names_a_foreign_folder_only() {
        let root = scratch("foreign-hint");
        assert!(foreign_dot_bram_hint(&root).is_none());
        std::fs::create_dir_all(root.join(".bram")).unwrap();
        assert!(foreign_dot_bram_hint(&root)
            .unwrap()
            .contains("isn't Bram's"));
        mark_dot_bram(&root.join(".bram"));
        assert!(foreign_dot_bram_hint(&root).is_none());
    }

    fn managed(name: &str, folder: &str) -> PathBuf {
        let root = scratch(name);
        std::fs::create_dir_all(root.join(folder)).unwrap();
        if folder == ".bram" {
            mark_dot_bram(&root.join(folder));
        }
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join(folder).join(AUTH_LEAF), "{}").unwrap();
        root
    }

    fn write_payload(provider: &str, root: &Path, target: &Path, content: &str) -> ShadowVerdict {
        shadow_worklist_decision(
            provider,
            &serde_json::json!({
                "tool_name": "Write",
                "tool_input": {"file_path": target.to_string_lossy(), "content": content},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap()
    }

    #[test]
    fn resolver_names_dot_bram_and_skips_the_home_folder() {
        let root = scratch("resolver");
        assert_eq!(state_dir_name(&root), ".bram");
        assert_eq!(state_rel(&root, WORKLIST_LEAF), ".bram/worklist.json");
        assert_eq!(
            state_path(&root, AUTH_LEAF),
            root.join(".bram/.worklist-authorization.json")
        );
        // No `.bram/` yet: nothing to walk to, nothing to write into.
        assert!(state_dir_candidates(&root).is_empty());
        assert!(!has_state_dir(&root));
        // The pre-0.8.4 folder is never a candidate.
        std::fs::create_dir_all(root.join("resources")).unwrap(); // state-dir: literal ok (the retired name, under test)
        std::fs::write(root.join("resources").join(AUTH_LEAF), "{}").unwrap(); // state-dir: literal ok (the retired name, under test)
        assert!(state_dir_candidates(&root).is_empty());
        mark_dot_bram(&root.join(".bram"));
        assert_eq!(state_dir(&root), root.join(".bram"));
        assert_eq!(state_dir_candidates(&root), vec![root.join(".bram")]);
        // `~/.bram` holds the guard binary: never a project's state folder.
        assert!(!has_project_bram_dir(&root, Some(&root)));
        assert!(state_dir_candidates_with_home(&root, Some(&root)).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn predicates_follow_the_resolved_folder() {
        let root = scratch("predicates");
        mark_dot_bram(&root.join(".bram"));
        assert!(is_lifecycle_path(&root, ".bram/.bram-port"));
        assert!(is_lifecycle_path(&root, ".bram/worklist-drafts/x.md"));
        assert!(is_lifecycle_path(
            &root,
            ".bram/bram-traces/hook-events.log"
        ));
        assert!(!is_lifecycle_path(&root, "resources/.bram-port")); // state-dir: literal ok (the retired name, under test)
        assert!(!is_lifecycle_path(&root, ".bram"));
        assert!(!is_lifecycle_path(&root, ".bramx/.bram-port"));
        assert!(is_worklist_draft(&root, ".bram/worklist-drafts/x.md"));
        assert!(!is_worklist_draft(&root, ".bram/worklist-drafts/sub/x.md"));
        assert!(!is_worklist_draft(&root, "resources/worklist-drafts/x.md")); // state-dir: literal ok (the retired name, under test)
        assert!(is_worklist_citation(
            &root,
            ".bram/worklist-citations/x.json"
        ));
        assert!(is_coordination_file(&root, ".bram/.worklist-intent.json"));
        assert!(is_coordination_file(&root, ".bram/.worklist-result.json"));
        // The retired name, under test.
        let old_intent = concat!("resources", "/.worklist-intent.json");
        assert!(!is_coordination_file(&root, old_intent));
        assert!(is_worklist_rel(&root, ".bram/worklist.json"));
        assert!(!is_worklist_rel(&root, "resources/worklist.json")); // state-dir: literal ok (the retired name, under test)
        assert_eq!(
            subagent_worklist_write_verdict(&root, Some(".bram/worklist.json"), "a1"),
            ("deny", None)
        );
        assert_eq!(
            subagent_worklist_write_verdict(&root, Some("resources/worklist.json"), "a1"), // state-dir: literal ok (the retired name, under test)
            ("skip", None)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_root_walk_finds_the_nearest_dot_bram() {
        let root = managed("root-walk", ".bram");
        let sub = root.join("sub");
        let x = sub.join("x");
        std::fs::create_dir_all(&x).unwrap();
        // Only the parent is managed.
        assert_eq!(find_project_root(&x.to_string_lossy()), Some(root.clone()));
        // A pre-0.8.4 marker in the child manages nothing.
        std::fs::create_dir_all(sub.join("resources")).unwrap(); // state-dir: literal ok (the retired name, under test)
        std::fs::write(sub.join("resources").join(AUTH_LEAF), "{}").unwrap(); // state-dir: literal ok (the retired name, under test)
        assert_eq!(find_project_root(&x.to_string_lossy()), Some(root.clone()));
        // A nested `.bram/` instance is found before its parent.
        mark_dot_bram(&sub.join(".bram"));
        std::fs::write(sub.join(".bram").join(AUTH_LEAF), "{}").unwrap();
        assert_eq!(find_project_root(&x.to_string_lossy()), Some(sub.clone()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn claude_write_edit_coverage_under_dot_bram() {
        let root = managed("claude-dot-bram", ".bram");

        let inflight = root.join(".bram/.inflight-claim.json");
        let v = write_payload("claude-rs", &root, &inflight, "{}");
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "bram-lifecycle-channel")
        );

        let target = root.join("app/x.txt");
        let v = write_payload("claude-rs", &root, &target, "x");
        assert_eq!(v.decision, "deny");
        assert!(v.reason.starts_with("no-coverage-no-opt-out:root="));
        assert!(
            v.message
                .contains("item in .bram/worklist.json covering this path"),
            "message: {}",
            v.message
        );
        assert!(
            v.message
                .contains("marker: .bram/.worklist-authorization.json"),
            "message: {}",
            v.message
        );
        assert!(!v.message.contains("resources/"), "message: {}", v.message); // state-dir: literal ok (the retired name, under test)

        let wl = root.join(".bram/worklist.json");
        std::fs::write(
            &wl,
            serde_json::json!({
                "version": 1,
                "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
            })
            .to_string(),
        )
        .unwrap();
        let v = write_payload("claude-rs", &root, &target, "x");
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "covered-by-worklist-item")
        );

        // worklist.json under `.bram/` gets the worklist validators.
        let stale = serde_json::json!({
            "version": 1,
            "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
        })
        .to_string();
        let v = write_payload("claude-rs", &root, &wl, &stale);
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("deny", "stale-worklist-version")
        );
        assert!(
            v.message.contains("stale base on .bram/worklist.json"),
            "message: {}",
            v.message
        );

        // Drafts sit under the lifecycle prefixes.
        let draft = root.join(".bram/worklist-drafts/i1.md");
        let v = write_payload("claude-rs", &root, &draft, "# Before\n");
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "bram-lifecycle-channel")
        );

        // `resources/` is an ordinary project folder.
        let legacy = root.join("resources/worklist.json"); // state-dir: literal ok (the retired name, under test)
        let v = write_payload("claude-rs", &root, &legacy, "{}");
        assert_eq!(v.decision, "deny");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_apply_patch_coverage_under_dot_bram() {
        let root = managed("codex-dot-bram", ".bram");
        let decide = |ti: Value| {
            shadow_worklist_decision(
                "codex-rs",
                &serde_json::json!({
                    "hook_event_name": "PreToolUse",
                    "tool_name": "apply_patch",
                    "tool_input": ti,
                    "cwd": root.to_string_lossy(),
                }),
            )
            .unwrap()
        };
        let patch = "*** Begin Patch\n*** Update File: app/x.txt\n@@\n-a\n+b\n*** End Patch\n";
        let v = decide(serde_json::json!({"input": patch}));
        assert_eq!(v.decision, "deny");
        assert!(
            v.reason
                .starts_with("apply_patch blocked: app/x.txt is not covered by any proposed"),
            "reason: {}",
            v.reason
        );
        assert!(
            v.message.contains(".bram/worklist.json"),
            "message: {}",
            v.message
        );

        std::fs::write(
            root.join(".bram/worklist.json"),
            serde_json::json!({
                "version": 1,
                "items": [{"id": "i1", "status": "proposed", "files": ["app/x.txt"]}]
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        let v = decide(serde_json::json!({"input": patch}));
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );

        let draft = "*** Begin Patch\n*** Add File: .bram/worklist-drafts/i1.md\n+# Before\n*** End Patch\n";
        let v = decide(serde_json::json!({"input": draft}));
        assert_eq!(v.decision, "allow", "reason: {}", v.reason);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_unmanaged_without_the_marker() {
        let root = scratch("codex-unmanaged");
        mark_dot_bram(&root.join(".bram"));
        // The pre-0.8.4 marker manages nothing.
        std::fs::create_dir_all(root.join("resources")).unwrap(); // state-dir: literal ok (the retired name, under test)
        std::fs::write(root.join("resources").join(AUTH_LEAF), "{}").unwrap(); // state-dir: literal ok (the retired name, under test)
        let v = shadow_worklist_decision(
            "codex-rs",
            &serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "apply_patch",
                "tool_input": {"input": "*** Begin Patch\n*** Update File: app/x.txt\n@@\n-a\n+b\n*** End Patch\n"},
                "cwd": root.to_string_lossy(),
            }),
        )
        .unwrap();
        assert_eq!(
            (v.decision.as_str(), v.reason.as_str()),
            ("allow", "passed-checks")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn outbound_turn_frame_names_the_state_folder() {
        let root = scratch("outbound-dot-bram");
        let turns = root.join(".bram").join("outbound-turns");
        std::fs::create_dir_all(&turns).unwrap();
        std::fs::write(
            turns.join("7-turn.json"),
            serde_json::json!({"text": "fix it\n\njust do it."}).to_string(),
        )
        .unwrap();
        let frame =
            "Read and follow this Bram turn: @.bram/outbound-turns/7-turn.json (end of Bram turn)";
        assert_eq!(outbound_turn_text(&root, frame), "fix it\n\njust do it.");
        // A frame naming a folder that isn't the state folder reads nothing,
        // the pre-0.8.4 one included.
        let other = "Read and follow this Bram turn: @other/outbound-turns/7-turn.json";
        assert_eq!(outbound_turn_text(&root, other), "");
        let old_turns = root.join("resources").join("outbound-turns"); // state-dir: literal ok (the retired name, under test)
        std::fs::create_dir_all(&old_turns).unwrap();
        std::fs::copy(turns.join("7-turn.json"), old_turns.join("7-turn.json")).unwrap();
        let old = "Read and follow this Bram turn: @resources/outbound-turns/7-turn.json"; // state-dir: literal ok (the retired name, under test)
        assert_eq!(outbound_turn_text(&root, old), "");
        let _ = std::fs::remove_dir_all(&root);
    }
}
