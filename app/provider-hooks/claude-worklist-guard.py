#!/usr/bin/env python3
"""PreToolUse hook: enforce the worklist flow on this project.

Three write surfaces, registered via CLAUDE_GUARD_MATCHERS in
src-tauri/src/lib.rs — the registration is what makes each branch below
reachable, and a branch whose matcher is missing is dead code that still
reads as coverage (judell/bram#261):

- `Write` / `Edit` — the canonical Claude edit tools.
- `Bash` — the agent can `sed -i` / `tee` / `python -c` / heredoc around
  the Write/Edit gate, so mutating commands need coverage too. Also
  where the #176 `gh --body @` check and the cross-boundary signature
  check live.
- `mcp__*` — a configured filesystem MCP server exposes write_file /
  edit_file / move_file, which would otherwise bypass Write/Edit
  entirely.

Two responsibilities:

1. Writes/Edits to resources/worklist.json — validate that any removed item
   was authorized. status="applied" items can be pruned freely (commit-then-
   prune); status="proposed" items can only be pruned when the last user
   message was `drop: {"ids":[...]}` listing that id.

2. Writes/Edits to any OTHER file in the project — require the target file
   to be covered by a proposed/applied item in resources/worklist.json, OR
   a fresh direct-edit bypass record in resources/.worklist-authorization.json,
   OR the single explicit opt-out phrase "just do it" in the last user
   message. Mirrors the coverage check the codex-side hook
   (app/provider-hooks/codex-worklist-guard.py) does for apply_patch.

If the project lacks resources/.worklist-authorization.json (the managed-repo
marker Bram Setup writes), the hook exits 0 (allow) — Claude
sessions in unmanaged repos run as if no hook were installed.
"""

import hashlib
import json
import os
import re
import sys

NL = chr(10)
import time
import urllib.request
from datetime import datetime, timezone


WORKLIST_REL = "resources/worklist.json"
WORKLIST_DRAFTS_PREFIX = "resources/worklist-drafts/"
AUTH_REL = "resources/.worklist-authorization.json"
BYPASS_TTL_SECONDS = 60 * 60  # direct-edit auth records are fresh for 1h


_LIFECYCLE_PATHS_EXACT = {
    "resources/worklist.json",
    "resources/.worklist-authorization.json",
    "resources/.inflight-claim.json",
    "resources/.pty-intent.jsonl",
    "resources/.worklist-intent.json",
    "resources/.worklist-result.json",
    "resources/.bram-port",
    "resources/.bram-port.json",
}
_LIFECYCLE_PATHS_PREFIXES = (
    "resources/worklist-drafts/",
    "resources/worklist-citations/",
    "resources/feedback-drafts/",
    "resources/feedback-history/",
    "resources/bram-traces/",
)


def is_lifecycle_path(rel):
    """True iff rel names a pure-coordination path whose writes are
    implicitly authorized by the worklist lifecycle."""
    if not isinstance(rel, str) or not rel:
        return False
    if rel in _LIFECYCLE_PATHS_EXACT:
        return True
    return any(rel.startswith(p) for p in _LIFECYCLE_PATHS_PREFIXES)


def emit_allow_for_lifecycle(target_rel, reason="bram-lifecycle-channel"):
    """Emit Claude's PreToolUse 'allow' decision so the user is not
    prompted for permission on lifecycle bookkeeping writes.
    Reference: https://docs.claude.com/en/docs/claude-code/hooks#pretooluse-decision-control
    """
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        target_rel,
        "allow",
        reason,
    )
    output = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": (
                f"Bram lifecycle channel ({target_rel}): implicitly authorized "
                f"by the worklist flow, no per-write confirmation needed."
            ),
        }
    }
    print(json.dumps(output))
    sys.exit(0)


# Issue #176: detect the `gh ... --body @<...>` antipattern. `gh`'s --body
# takes a literal string, but `@-` and `@path` look like stdin/file refs
# (curl convention) and silently stash the literal text — comments render
# as a placeholder smiley. Deny before the malformed command runs.
_GH_BODY_AT_RX = re.compile(
    r"(?:^|\s|;|&&|\|\||\|)gh\s.*?--body\s+@\S",
    re.MULTILINE,
)


def is_gh_body_at_antipattern(command):
    if not isinstance(command, str):
        return False
    return bool(_GH_BODY_AT_RX.search(command))


# Bash commands we deny without worklist coverage. This intentionally matches
# the Codex guard's H3-level classifier: broad enough to stop common write and
# external side-effect bypasses, but narrow enough to keep read-only shell
# investigation usable.
_BASH_WRITE_PATTERNS = [
    re.compile(r"(^|[\s;&|`(])>+\s*[^\s>&]"),         # > file or >> file
    re.compile(r"(^|[\s;&|`(])tee\b"),                # tee
    re.compile(r"(^|[\s;&|`(])sed\s+[^|;&]*-i\b"),    # sed -i
    re.compile(r"(^|[\s;&|`(])perl\s+[^|;&]*-i\b"),   # perl -i
    re.compile(r"(^|[\s;&|`(])(rm|mv|cp|truncate|install)\b"),
    re.compile(r"(^|[\s;&|`(])git\s+(add|commit|push|rm|mv|reset|checkout|restore|stash|am|apply|cherry-pick|rebase|revert|tag|branch)\b"),
    re.compile(r"(^|[\s;&|`(])gh\s+issue\s+(close|reopen|edit|comment|create|delete|transfer|pin|unpin|lock|unlock)\b"),
    re.compile(r"open\s*\(\s*['\"][^'\"]+['\"]\s*,\s*['\"][wax]"),
    re.compile(r"(^|[\s;&|`(])python[0-9.]*\s+-c\b"),  # python -c can write
    re.compile(r"(^|[\s;&|`(])node\s+-e\b"),
    re.compile(r"(^|[\s;&|`(])bash\s+-c\b"),
    re.compile(r"(^|[\s;&|`(])sh\s+-c\b"),
]


def bash_writes(command):
    if not isinstance(command, str):
        return False
    for rx in _BASH_WRITE_PATTERNS:
        if rx.search(command):
            return True
    return False


# Cross-boundary signature check. conventions.md ("Name the boundary and
# sign your side") requires every artifact crossing a project boundary to
# open with `<owner>'s <Agent> speaking from the <Project> project:`.
# Measured drift on both sides is what motivates enforcing it here rather
# than trusting prose alone.
#
# A parallel implementation lives in the Codex guard; the two scripts
# install to different roots as standalone files, so this logic is
# deliberately duplicated rather than shared (same reasoning as
# _BASH_WRITE_PATTERNS). Keep them in step.
#
# Three gates, all of which must pass before denying. Any shape we cannot
# read with confidence FAILS OPEN — a guard that blocks real work over a
# formatting rule is worse than the drift it prevents.
_FORGE_WRITE_RX = re.compile(
    r"(?:^|[\s;&|`(])(?:gh|glab)\s+(?:issue|pr|mr)\s+"
    r"(?:create|comment|note|edit|update|review)\b"
)
# Body sources. The alternation deliberately requires `=` or whitespace
# after the flag so `--body-file` is not swallowed by the `--body` arm.
_BODY_FILE_RX = re.compile(r"(?:--body-file|-F)(?:=|\s+)(\S+)")
_BODY_LITERAL_RX = re.compile(
    r"(?:--body|-b|--message|-m|--description)(?:=|\s+)"
    r"(\"(?:[^\"\\]|\\.)*\"|'[^']*'|\S+)"
)
# `<owner>'s <Agent> speaking from the <Project> project:` — the shape, not
# the instance. The script ships to every managed project, so no owner or
# project name is hardcoded; conventions.md supplies the instances.
_SIGNATURE_RX = re.compile(
    r"speaking\s+from\s+the\s+.{1,60}?\bproject\b",
    re.IGNORECASE,
)


def _unquote_shell(value):
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
        inner = value[1:-1]
        return inner.replace('\\"', '"').replace("\\n", NL)
    return value


def crossboundary_body(command, cwd):
    """Return (body_text, reason). body_text is None when the body cannot be
    read, with reason naming why so the fail-open is greppable."""
    m = _BODY_FILE_RX.search(command)
    if m:
        path = m.group(1)
        if path == "-":
            return None, "body-file-stdin"
        try:
            candidate = path if os.path.isabs(path) else os.path.join(cwd, path)
            with open(candidate, encoding="utf-8", errors="replace") as f:
                return f.read(), "body-file"
        except Exception:
            return None, "body-file-unreadable"
    m = _BODY_LITERAL_RX.search(command)
    if m:
        return _unquote_shell(m.group(1)), "body-literal"
    return None, "no-body-flag"


def body_is_signed(body):
    """True iff the first non-empty line carries the canonical shape."""
    if not isinstance(body, str):
        return False
    for line in body.splitlines():
        stripped = line.strip().lstrip("_*>#- ").strip()
        if not stripped:
            continue
        return bool(_SIGNATURE_RX.search(stripped))
    return False


def crossboundary_signature_verdict(command, cwd):
    """('skip'|'signed'|'unparsed'|'unsigned', detail).

    'skip' means the command is not a forge write with a body at all.

    The signature is required on EVERY agent-authored forge artifact, not
    only ones that cross a project boundary. The risk it exists to prevent
    is not "which project is this from" but "who is speaking": on a shared
    account an agent comment is indistinguishable from a human one, and
    that is true in every repo. The cross-boundary case was a subset of the
    problem mistaken for the whole of it (judell/bram#253, where unsigned
    agent comments sat beside Jon's own replies while a cross-boundary
    issue in the same period was correctly signed).

    So there is no origin comparison here any more, and the hook no longer
    parses .git/config. That the correction REMOVES a gate is the tell that
    the scoping was the accidental part.
    """
    if not isinstance(command, str) or not command:
        return "skip", "no-command"
    if not _FORGE_WRITE_RX.search(command):
        return "skip", "not-forge-write"
    body, reason = crossboundary_body(command, cwd)
    if body is None:
        return "unparsed", reason
    if body_is_signed(body):
        return "signed", reason
    return "unsigned", reason

# MCP is the third write surface, at parity with the Codex guard: a user
# with a filesystem MCP server configured can route writes through
# mcp__filesystem__write_file / edit_file / move_file, bypassing the
# Write/Edit gate entirely. This project has a project-scoped filesystem
# server whose allowed roots include the repo (judell/bram#261).
#
# Mutation candidates are recognized by a token in the tool-name suffix, so
# read-only MCP tools (search, docs, list) pass untouched and non-filesystem
# servers are not broken. Mirrors _MCP_WRITE_TOKENS in the Codex guard.
_MCP_WRITE_TOKENS = (
    "write", "edit", "create", "delete", "remove", "rename",
    "move", "copy", "patch", "append", "truncate", "mkdir", "rmdir",
    "modify", "replace", "save", "set_",
)

# Path-shaped keys in MCP tool_input payloads. The standard
# @modelcontextprotocol/server-filesystem uses path/source/destination;
# other servers vary. Mirrors _MCP_PATH_KEYS in the Codex guard.
_MCP_PATH_KEYS = (
    "path", "file_path", "filepath", "filename",
    "source", "src", "destination", "dest", "dst",
    "target", "target_path", "to", "from",
)


def mcp_is_mutation(tool_name):
    """True iff an mcp__-prefixed tool name signals filesystem mutation."""
    name = (tool_name or "").lower()
    return any(tok in name for tok in _MCP_WRITE_TOKENS)


def mcp_paths(tool_input):
    """Extract path-shaped values from an MCP tool_input dict."""
    if not isinstance(tool_input, dict):
        return []
    out = []
    for key in _MCP_PATH_KEYS:
        v = tool_input.get(key)
        if isinstance(v, str) and v:
            out.append(v)
        elif isinstance(v, list):
            for item in v:
                if isinstance(item, str) and item:
                    out.append(item)
    return out


def _post_hook_trace(script, event, tool, target, decision, reason, cwd):
    """Ship the [hook] decision line to the host, which writes it through the
    standard bram-trace path — gated on the LIVE Traces setting, not on
    spawn-time env (hook-trace-follow-settings). BRAM_TRACE/BRAM_TRACE_LOG
    env gating is gone: env is inherited when the agent process spawns, so
    it silently desynced from the Settings toggle for the lifetime of a
    session. Failures are swallowed; tracing must never block a tool call."""
    try:
        cur = os.path.abspath(cwd or os.getcwd())
        port = None
        while True:
            candidate = os.path.join(cur, "resources", ".bram-port")
            if os.path.exists(candidate):
                with open(candidate, encoding="utf-8") as f:
                    port = int(f.read().strip())
                break
            parent = os.path.dirname(cur)
            if parent == cur:
                break
            cur = parent
        if not port:
            return
        body = json.dumps({
            "script": script,
            "event": event,
            "tool": tool,
            "target": str(target)[:300],
            "cwd": cwd or "",
            "decision": decision,
            "reason": reason,
        }).encode()
        req = urllib.request.Request(
            "http://127.0.0.1:%d/__hook-trace" % port,
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req, timeout=0.3).read()
    except Exception:
        pass


def _trace_hook(event, tool, target, decision, reason, cwd=None):
    """Issue #49 [hook] trace + issue #95 phantom-write diagnostic.

    - Always emits one `[worklist-guard]` line to stderr, including cwd,
      so the hook's decision is visible to the agent / user without
      tracing enabled. Refs #95 — phantom worklist writes need this
      signal to distinguish hook-block from cwd-mismatch from
      watcher-revert.
    - Additionally POSTs the line to the host's /__hook-trace route, which
      appends it to resources/bram-traces/bram-trace.log iff the live
      Traces setting is on (hook-trace-follow-settings).
    """
    if cwd is None:
        try:
            cwd = os.getcwd()
        except Exception:
            cwd = ""
    diagnostic = (
        f"[worklist-guard] tool={tool} target={target} cwd={cwd} "
        f"decision={decision} reason={reason}"
    )
    try:
        sys.stderr.write(diagnostic + "\n")
        sys.stderr.flush()
    except Exception:
        pass
    _post_hook_trace("claude-worklist-guard.py", event, tool, target, decision, reason, cwd)


# The single opt-out phrase that authorizes a one-turn direct edit. Matched
# case-insensitively against the last user message. Narrowed from a
# seven-regex list (opt-out-single-phrase-and-audit) to one explicit phrase
# — anything ambiguous ("looks good", "go ahead") is NOT here, matching the
# conventions' "Don't infer commit/drop/advance from feedback" rule. The
# non-verbal route (the Skip worklist button / `skip-worklist:` prefix)
# is unaffected and lives in the host, not here.
_OPT_OUT_PATTERNS = [
    re.compile(r"\bjust do it\b", re.IGNORECASE),
]


def items_by_id(text):
    try:
        return {it["id"]: it for it in json.loads(text).get("items", [])}
    except Exception:
        return {}


def last_user_text(transcript_path):
    if not transcript_path or not os.path.exists(transcript_path):
        return ""
    last = ""
    with open(transcript_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            try:
                m = json.loads(line)
            except Exception:
                continue
            if m.get("type") != "user":
                continue
            c = m.get("message", {}).get("content", "")
            if isinstance(c, list):
                c = "".join(
                    p.get("text", "") for p in c
                    if isinstance(p, dict) and p.get("type") == "text"
                )
            # Only update `last` when c has actual text. tool_result-only user
            # records collapse to an empty string in the list comprehension above;
            # overwriting `last` with that would lose a real `approved:`/`drop:`
            # message typed in an earlier turn whenever any tool call followed it.
            if isinstance(c, str) and c.strip():
                last = c
    return last


def has_opt_out(msg):
    if not isinstance(msg, str):
        return False
    return any(rx.search(msg) for rx in _OPT_OUT_PATTERNS)


def _post_direct_edit_audit_breadcrumb(project_root, last_msg):
    """Best-effort breadcrumb POST to /__audit/direct-edit so a Claude
    prose opt-out lands in the audit ledger the same way a Codex prose
    opt-out or the Skip worklist button already does (opt-out-single-
    phrase-and-audit). Mirrors the fire-and-forget pattern in
    claude-permission-menu-hook.py: short timeout, silent failure — the
    guard must never block or fail a tool call because Bram is down."""
    try:
        with open(
            os.path.join(project_root, "resources", ".bram-port"), encoding="utf-8"
        ) as f:
            port = int(f.read().strip())
        turn_key = hashlib.sha256(last_msg.encode("utf-8")).hexdigest()
        body = json.dumps(
            {
                "provider": "claude",
                "source": "claude-guard-opt-out",
                "turnKey": turn_key,
            }
        ).encode("utf-8")
        req = urllib.request.Request(
            "http://127.0.0.1:%d/__audit/direct-edit" % port,
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req, timeout=0.4).read()
    except Exception:
        pass


def find_project_root(start):
    """Walk up from `start` until we find the AUTH_REL marker. Returns the
    project root path, or None if the marker isn't anywhere above."""
    cur = os.path.abspath(start)
    while True:
        if os.path.exists(os.path.join(cur, AUTH_REL)):
            return cur
        parent = os.path.dirname(cur)
        if parent == cur:
            return None
        cur = parent


def normalize_target(project_root, target):
    """Return project-relative path for target if it's inside project_root,
    else None."""
    if not isinstance(target, str) or not target:
        return None
    abs_target = os.path.abspath(target)
    abs_root = os.path.abspath(project_root)
    if abs_target == abs_root:
        return ""
    prefix = abs_root + os.sep
    if abs_target.startswith(prefix):
        return abs_target[len(prefix):].replace(os.sep, "/")
    return None


def is_worklist_draft(rel):
    return (
        isinstance(rel, str)
        and rel.startswith(WORKLIST_DRAFTS_PREFIX)
        and rel.endswith(".md")
        and "/" not in rel[len(WORKLIST_DRAFTS_PREFIX):]
    )


def worklist_items_with_inline_prose(content):
    """Parse worklist content; return list of ids that carry non-empty
    inline `before` or `after`. Draft-only: prose belongs in
    resources/worklist-drafts/<id>.md, not in the metadata index."""
    try:
        doc = json.loads(content)
    except Exception:
        return []
    items = doc.get("items") if isinstance(doc, dict) else None
    if not isinstance(items, list):
        return []
    bad = []
    for it in items:
        if not isinstance(it, dict):
            continue
        if it.get("status", "proposed") != "proposed":
            continue
        before = it.get("before")
        after = it.get("after")
        has_inline = (
            (isinstance(before, str) and before.strip())
            or (isinstance(after, str) and after.strip())
        )
        if has_inline:
            item_id = it.get("id")
            label = item_id if (isinstance(item_id, str) and item_id.strip()) else "<no-id>"
            bad.append(label)
    return bad


def worklist_covered_files(project_root):
    """Set of project-relative paths covered by proposed/applied items."""
    try:
        with open(os.path.join(project_root, WORKLIST_REL), encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        return set()
    covered = set()
    for it in data.get("items") or []:
        if not isinstance(it, dict):
            continue
        st = it.get("status", "proposed")
        if st not in ("proposed", "applied"):
            continue
        if isinstance(it.get("file"), str):
            covered.add(it["file"])
        for p in it.get("files", []) or []:
            if isinstance(p, str):
                covered.add(p)
    return covered


def fresh_bypass(project_root, path_rel):
    """True iff the auth record carries a recent direct-edit bypass
    covering path_rel."""
    try:
        with open(os.path.join(project_root, AUTH_REL), encoding="utf-8") as f:
            rec = json.load(f)
    except Exception:
        return False
    if not isinstance(rec, dict) or rec.get("kind") != "direct-edit":
        return False
    issued = rec.get("issuedAtMs") or rec.get("issued_at_ms") or 0
    if (time.time() * 1000 - issued) > BYPASS_TTL_SECONDS * 1000:
        return False
    paths = rec.get("paths") or []
    return path_rel in paths or "*" in paths


def deny_coverage(target_rel, opt_out_attempted):
    msg = (
        f"Blocked: writing to {target_rel} requires either a proposed/applied "
        f"item in resources/worklist.json covering this path, or an explicit "
        f"opt-out phrase in your last message.\n"
        f"  - Propose the change in resources/worklist.json first (item with "
        f"file=\"{target_rel}\", non-empty before and after, status proposed). "
        f"Wait for the user's approved: payload, then retry.\n"
        f"  - The user can authorize a direct edit by ending their message "
        f"with \"just do it\", or by clicking the Skip worklist button."
    )
    if opt_out_attempted:
        msg += (
            "\n  - (Detected what looked like opt-out language, but it didn't "
            "match the expected phrasing. The verbal opt-out is exactly "
            "\"just do it\"; otherwise use the Skip worklist button.)"
        )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        target_rel,
        "deny",
        "no-coverage-no-opt-out",
    )
    print(msg, file=sys.stderr)
    sys.exit(2)


def opt_out_clears(project_root, payload, tool_name, target_rel):
    """Last-chance authorization shared by EVERY write surface: the documented
    "just do it" phrase in the user's last message.

    Every branch that is about to deny for lack of coverage must call this
    first. It lives in one place deliberately (judell/bram#263): the policy
    used to be inline in the Write/Edit branch only, so the Bash branch
    (#217) and then the MCP branch (#261) were each written without it, and
    `conventions.md`'s promise that the phrase is matched "on every
    PreToolUse" was false on two of the three surfaces for a month.

    Posting the audit breadcrumb is part of this function, not of its
    callers, for the same reason: it is the only record that a prose opt-out
    authorized a change, and a surface that honored the phrase without
    recording it would authorize unaudited writes — worse than
    over-denying.
    """
    last_msg = last_user_text(payload.get("transcript_path", ""))
    if not has_opt_out(last_msg):
        return False
    _post_direct_edit_audit_breadcrumb(project_root, last_msg)
    # Pass project_root as cwd: it is the project the decision applies to, and
    # it is where _post_hook_trace finds resources/.bram-port. Defaulting to
    # os.getcwd() named the wrong project on the Bash/MCP paths, where the
    # target can live outside the session's directory.
    _trace_hook("PreToolUse", tool_name, target_rel, "allow", "opt-out-phrase",
                project_root)
    return True


def deny_bash_coverage(command, cwd=None):
    preview = (command or "")[:200]
    _trace_hook(
        "PreToolUse",
        "Bash",
        preview,
        "deny",
        "bash-write-no-coverage",
        cwd,
    )
    print(
        "Bash blocked: this command writes to the filesystem or performs a "
        "sensitive side effect, and resources/worklist.json has no proposed "
        "or applied items covering active work.\n"
        "  - Propose the work in the worklist first, wait for the user's "
        "approved: payload, then retry.\n"
        "  - The user can authorize a direct edit by ending their message "
        "with \"just do it\", or by clicking the Skip worklist button.",
        file=sys.stderr,
    )
    sys.exit(2)


def worklist_version_from_text(text):
    """Return (present, version) for a worklist.json text. `present` is
    True iff the JSON parsed and the top-level `version` field was an
    integer. `version` defaults to 0 when absent or malformed."""
    try:
        doc = json.loads(text)
    except Exception:
        return (False, 0)
    if not isinstance(doc, dict):
        return (False, 0)
    v = doc.get("version")
    if isinstance(v, int):
        return (True, v)
    return (False, 0)


def deny_stale_worklist_version(old_version, new_version, new_present):
    if new_present:
        detail = (
            f"You set version={new_version}, but on-disk version is "
            f"{old_version}. Expected version={old_version + 1}."
        )
    else:
        detail = (
            f"Your write is missing the `version` field. On-disk version "
            f"is {old_version}; the new content must set version="
            f"{old_version + 1}."
        )
    msg = (
        "Blocked: stale base on resources/worklist.json. "
        + detail
        + "\n  - Re-read resources/worklist.json, base your edit on the "
        "current contents, and include the bumped version field on the "
        "write. This guards against concurrent-writer races between "
        "your propose / Edit and other agents or the /__worklist/mutate "
        "route."
    )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        WORKLIST_REL,
        "deny",
        "stale-worklist-version",
    )
    print(msg, file=sys.stderr)
    sys.exit(2)


def worklist_state_changes(old_items, new_items):
    removed = []
    status_changed = []
    for item_id, old_item in old_items.items():
        if item_id not in new_items:
            removed.append((item_id, old_item.get("status", "proposed")))
            continue
        old_status = old_item.get("status", "proposed")
        new_status = new_items[item_id].get("status", "proposed")
        if old_status != new_status:
            status_changed.append((item_id, old_status, new_status))
    return removed, status_changed


def deny_inline_prose(bad):
    lines = [
        "Blocked: inline `before` / `after` on proposed worklist item(s) — "
        "prose must live in resources/worklist-drafts/<id>.md."
    ]
    for label in bad:
        lines.append(f"  - item {label}: remove inline before/after; write prose to "
                     f"resources/worklist-drafts/{label}.md instead")
    lines.append(
        "Draft file format: `# Before` section then `# After` section, both "
        "in Markdown. The server merge surfaces the draft prose alongside "
        "the metadata in worklist.json."
    )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        WORKLIST_REL,
        "deny",
        "worklist-inline-prose",
    )
    print("\n".join(lines), file=sys.stderr)
    sys.exit(2)


def deny_mechanical_worklist_change(removed, status_changed):
    lines = [
        "Blocked: mechanical worklist state changes must go through "
        "`POST /__worklist/mutate`, not a direct edit to "
        "`resources/worklist.json`.",
        "  - Direct worklist edits are for proposing items or refining "
        "their prose during iterate.",
        "  - Use mutate for `prune` and `advance` after a verified "
        "`drop:` / `approved:` turn.",
    ]
    if removed:
        detail = ", ".join(f'"{item_id}" (status={status})' for item_id, status in removed)
        lines.append(f"  - Removed item ids: {detail}")
    if status_changed:
        detail = ", ".join(
            f'"{item_id}" ({old_status}->{new_status})'
            for item_id, old_status, new_status in status_changed
        )
        lines.append(f"  - Status changes: {detail}")
    lines.append(
        "  - Example: "
        "curl -4 -sS -X POST -d '{\"op\":\"prune\",\"ids\":[\"item-id\"]}' "
        "http://127.0.0.1:$(cat resources/.bram-port)/__worklist/mutate"
    )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        WORKLIST_REL,
        "deny",
        "mechanical-worklist-change",
    )
    print("\n".join(lines), file=sys.stderr)
    sys.exit(2)


def self_test():
    write_commands = [
        "echo x > out.txt",
        "printf x >> out.txt",
        "printf x | tee out.txt",
        "sed -i '' s/a/b/ file.txt",
        "perl -i -pe s/a/b/ file.txt",
        "python -c \"open('x', 'w').write('x')\"",
        "node -e \"require('fs').writeFileSync('x','x')\"",
        "git commit -m test",
        "git push",
        "gh issue close 119 --repo judell/bram",
        "rm stale.txt",
        "mv a b",
        "cp a b",
        "truncate -s 0 file.txt",
        "bash -c 'echo x > out.txt'",
        "sh -c 'echo x > out.txt'",
    ]
    read_commands = [
        "ls -la",
        "rg Bash app/provider-hooks/claude-worklist-guard.py",
        "git status --short",
        "gh issue view 119 --repo judell/bram",
        "python --version",
        "curl -I https://example.com",
    ]
    for command in write_commands:
        assert bash_writes(command), command
    for command in read_commands:
        assert not bash_writes(command), command

    # MCP surface (judell/bram#261). Mutation recognition is by tool-name
    # token, so read-only servers pass untouched.
    mcp_mutations = [
        "mcp__filesystem__write_file",
        "mcp__filesystem__edit_file",
        "mcp__filesystem__move_file",
        "mcp__filesystem__create_directory",
    ]
    mcp_reads = [
        "mcp__filesystem__read_text_file",
        "mcp__filesystem__list_directory",
        "mcp__xmlui__xmlui_search_howto",
        "mcp__xmlui__xmlui_component_docs",
    ]
    for name in mcp_mutations:
        assert mcp_is_mutation(name), name
    for name in mcp_reads:
        assert not mcp_is_mutation(name), name

    # judell/bram#263: the opt-out must be reachable from every write surface,
    # so it is tested as one function rather than per-branch.
    import tempfile as _tempfile

    with _tempfile.TemporaryDirectory() as td:
        def _transcript(text):
            p = os.path.join(td, "t-%d.jsonl" % abs(hash(text)))
            with open(p, "w", encoding="utf-8") as f:
                f.write(json.dumps({
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": text}]},
                }) + NL)
                # A trailing tool_result-only record must not clobber the
                # real user message (the reason last_user_text skips blanks).
                f.write(json.dumps({
                    "type": "user",
                    "message": {"content": [{"type": "tool_result", "text": ""}]},
                }) + NL)
            return p

        assert last_user_text(_transcript("patch it, just do it")) == "patch it, just do it"
        assert has_opt_out(last_user_text(_transcript("patch it, just do it")))
        assert not has_opt_out(last_user_text(_transcript("skip the worklist")))
        assert not has_opt_out(last_user_text(""))

        # The helper is what every branch calls; project_root is only used for
        # the breadcrumb POST, which is best-effort and silent with no port.
        assert opt_out_clears(
            td, {"transcript_path": _transcript("do the build, just do it")},
            "Bash", "git pull",
        )
        assert not opt_out_clears(
            td, {"transcript_path": _transcript("please do the build")},
            "Bash", "git pull",
        )
        assert not opt_out_clears(td, {}, "mcp__filesystem__write_file", "a.txt")

    # Signature gates. Mirrors the Codex guard's cases.

    with _tempfile.TemporaryDirectory() as td:
        signed = "Jon's Claude speaking from the Bram project:" + NL + NL + "Body."
        unsigned = "Vendored the build; the tooltip renders."

        def verdict(cmd):
            return crossboundary_signature_verdict(cmd, td)[0]

        # Gate 1: not a forge write.
        assert verdict("gh issue view 12 --repo xmlui-org/xmlui") == "skip"
        assert verdict("ls -la") == "skip"
        # Same repo is now inspected exactly like any other — the regression
        # test for the scoping this item removed.
        assert verdict('gh issue comment 5 --body "%s"' % unsigned) == "unsigned"
        assert verdict('gh issue comment 5 --body "%s"' % signed) == "signed"
        assert verdict(
            'gh issue comment 5 --repo judell/bram --body "%s"' % unsigned
        ) == "unsigned"
        # No .git/config anywhere in this tempdir: the verdict must not
        # depend on one being readable.
        assert not os.path.exists(os.path.join(td, ".git", "config"))
        # Gate 3: cross-repo behaviour unchanged in both directions.
        assert verdict(
            'gh issue comment 5 --repo xmlui-org/xmlui --body "%s"' % signed
        ) == "signed"
        assert verdict(
            'gh issue comment 5 --repo xmlui-org/xmlui --body "%s"' % unsigned
        ) == "unsigned"
        # Surfaces _BASH_WRITE_PATTERNS does not match.
        assert verdict('gh pr comment 9 --body "%s"' % unsigned) == "unsigned"
        assert verdict('glab issue note 3 -m "%s"' % unsigned) == "unsigned"
        # Fail-open shapes.
        assert verdict("gh issue comment 5 --body-file -") == "unparsed"
        assert verdict("gh issue comment 5 --body-file /nope/missing.md") == "unparsed"
        # No body to inspect at all (e.g. `gh issue close 5`) still fails open.
        assert verdict("gh issue comment 5") == "unparsed"
        assert verdict("gh issue close 5") == "skip"

        # --body-file must not be swallowed by the --body arm.
        body_path = os.path.join(td, "comment.md")
        with open(body_path, "w") as f:
            f.write(signed)
        assert crossboundary_signature_verdict(
            "gh issue comment 5 --body-file %s" % body_path, td
        ) == ("signed", "body-file")

    # Leading Markdown emphasis on the signature line still counts.
    assert body_is_signed("_Jon's Codex speaking from the XMLUI project:_")
    assert body_is_signed("> **Jon's Claude speaking from the Bram project:**")
    assert not body_is_signed("Bram side — a correction to my green light.")
    assert not body_is_signed("")

    assert mcp_paths({"path": "app/x.xmlui"}) == ["app/x.xmlui"]
    assert mcp_paths({"source": "a", "destination": "b"}) == ["a", "b"]
    assert mcp_paths({"paths": ["a", "b"]}) == []  # unrecognized key -> deny path
    assert mcp_paths({"edits": [{"oldText": "x"}]}) == []
    assert mcp_paths("not-a-dict") == []


def main():
    payload = json.load(sys.stdin)
    tool_name = payload.get("tool_name", "")

    if tool_name == "Bash":
        ti = payload.get("tool_input", {})
        command = ti.get("command", "") if isinstance(ti, dict) else ""
        preview = command[:200]
        cwd = payload.get("cwd") or os.getcwd()
        if is_gh_body_at_antipattern(command):
            m = _GH_BODY_AT_RX.search(command)
            matched = m.group(0).strip() if m else ""
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "deny",
                "gh-body-at-antipattern",
                cwd,
            )
            print(
                "gh --body takes a literal string, not stdin or a file reference.\n"
                "Use --body-file - (stdin) or --body-file <path> instead.\n"
                f"Detected: {matched}",
                file=sys.stderr,
            )
            sys.exit(2)
        # Cross-boundary signature. Runs before bash_writes for the same
        # reason the #176 check does: _BASH_WRITE_PATTERNS covers
        # `gh issue ...` but not `gh pr ...` or glab at all, so a later
        # position would miss those surfaces entirely. Non-terminal on
        # pass — the command still faces the worklist gate below.
        cb_verdict, cb_detail = crossboundary_signature_verdict(command, cwd)
        if cb_verdict == "unsigned":
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "deny",
                "crossboundary-unsigned",
                cwd,
            )
            print(
                "This posts to a repo other than this project's origin, and the "
                "body does not open with the cross-boundary signature.\n"
                "Open the first line with:\n"
                "    <owner>'s <Agent> speaking from the <Project> project:\n"
                "See conventions.md, 'Name the boundary and sign your side' — "
                "every artifact, every comment, not just the first in a thread.",
                file=sys.stderr,
            )
            sys.exit(2)
        elif cb_verdict in ("signed", "unparsed"):
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "allow",
                "crossboundary-" + cb_verdict + ":" + cb_detail,
                cwd,
            )
        if not bash_writes(command):
            _trace_hook("PreToolUse", "Bash", preview, "allow", "bash-read-only", cwd)
            sys.exit(0)
        project_root = find_project_root(cwd)
        if project_root is None:
            _trace_hook("PreToolUse", "Bash", preview, "allow", "unmanaged-repo", cwd)
            sys.exit(0)
        if WORKLIST_DRAFTS_PREFIX in command or ".worklist-intent.json" in command:
            _trace_hook("PreToolUse", "Bash", preview, "allow", "bram-lifecycle-channel", cwd)
            sys.exit(0)
        covered = worklist_covered_files(project_root)
        if covered or fresh_bypass(project_root, "*"):
            _trace_hook("PreToolUse", "Bash", preview, "allow", "covered-by-worklist-item", cwd)
            sys.exit(0)
        if opt_out_clears(project_root, payload, "Bash", preview):
            sys.exit(0)
        deny_bash_coverage(command, cwd)

    if tool_name.startswith("mcp__"):
        os.environ["__BRAM_TRACE_TOOL"] = tool_name
        ti = payload.get("tool_input", {})
        cwd = payload.get("cwd") or os.getcwd()
        if not mcp_is_mutation(tool_name):
            _trace_hook("PreToolUse", tool_name, "", "allow", "mcp-read-only", cwd)
            sys.exit(0)
        project_root = find_project_root(cwd)
        if project_root is None:
            _trace_hook("PreToolUse", tool_name, "", "allow", "unmanaged-repo", cwd)
            sys.exit(0)
        candidates = mcp_paths(ti)
        if not candidates:
            # Mutation-shaped with no path we recognize. Don't guess: deny and
            # name the surface so the guard can be extended deliberately.
            _trace_hook(
                "PreToolUse", tool_name, "", "deny", "mcp-unrecognized-input", cwd
            )
            print(
                f"{tool_name} blocked: this looks like a mutation, but the guard "
                f"could not extract any file path from tool_input.\n"
                f"Propose the change in resources/worklist.json first, or extend "
                f"claude-worklist-guard.py to recognize this tool's input shape.",
                file=sys.stderr,
            )
            sys.exit(2)
        covered = worklist_covered_files(project_root)
        violations = []
        for target in candidates:
            rel = normalize_target(project_root, target)
            if rel is None:
                continue  # outside the project tree
            if rel == WORKLIST_REL:
                # worklist.json mutations must go through a shape the guard can
                # validate (Write/Edit) or through /__worklist/mutate. An MCP
                # write would skip the prune/version/inline-prose checks.
                _trace_hook(
                    "PreToolUse", tool_name, rel, "deny", "mcp-worklist-write", cwd
                )
                print(
                    f"{tool_name} blocked: edits to {WORKLIST_REL} must use "
                    f"Write/Edit (so the guard can validate prunes, the version "
                    f"bump, and inline prose) or /__worklist/mutate for "
                    f"mechanical transitions.",
                    file=sys.stderr,
                )
                sys.exit(2)
            if is_lifecycle_path(rel):
                continue
            if rel in covered:
                continue
            if fresh_bypass(project_root, rel):
                continue
            violations.append(rel)
        if violations:
            if opt_out_clears(project_root, payload, tool_name, violations[0]):
                sys.exit(0)
            deny_coverage(violations[0], opt_out_attempted=False)
        _trace_hook(
            "PreToolUse", tool_name, ",".join(candidates)[:120], "allow",
            "covered-by-worklist-item", cwd,
        )
        sys.exit(0)

    if tool_name not in ("Write", "Edit"):
        sys.exit(0)
    # Stash for downstream deny-path trace calls (deny_coverage and
    # deny_mechanical_worklist_change don't otherwise have tool_name).
    os.environ["__BRAM_TRACE_TOOL"] = tool_name

    ti = payload.get("tool_input", {})
    fp = ti.get("file_path", "")
    if not isinstance(fp, str) or not fp:
        sys.exit(0)

    # Locate the project root via the managed-repo marker. If the file isn't
    # inside a Bram-managed project at all, exit cleanly — this
    # hook is a no-op for Claude sessions in unmanaged repos.
    project_root = find_project_root(os.path.dirname(fp) or ".")
    if project_root is None:
        sys.exit(0)

    rel = normalize_target(project_root, fp)
    if rel is None:
        # Target is outside the project tree (e.g., editing files in
        # ~/.codex/ or /tmp/). The worklist gate doesn't apply.
        sys.exit(0)

    # Pre-Branch-1: lifecycle coordination paths the user already authorized
    # implicitly. Emit allow-via-JSON so Claude does not surface a native
    # permission menu on these bookkeeping writes. worklist.json itself
    # is excluded so Branch 1 can still validate mechanical state changes.
    if rel != WORKLIST_REL and is_lifecycle_path(rel):
        emit_allow_for_lifecycle(rel, "bram-lifecycle-channel")

    # Branch 1: writes to resources/worklist.json — existing prune validation.
    if rel == WORKLIST_REL:
        if not os.path.exists(fp):
            emit_allow_for_lifecycle(rel, "worklist-bootstrap")
        with open(fp, encoding="utf-8") as f:
            old = f.read()
        if payload["tool_name"] == "Write":
            new = ti.get("content", "")
        else:
            o = ti.get("old_string", "")
            n = ti.get("new_string", "")
            new = old.replace(o, n) if ti.get("replace_all") else old.replace(o, n, 1)
        old_items = items_by_id(old)
        new_items = items_by_id(new)
        removed, status_changed = worklist_state_changes(old_items, new_items)
        if not removed and not status_changed:
            inline_bad = worklist_items_with_inline_prose(new)
            if inline_bad:
                deny_inline_prose(inline_bad)
            # Optimistic-concurrency check: once the on-disk file carries
            # a `version` field, every write must bump it by exactly 1.
            # Skip the check when the on-disk file has no version yet
            # (legacy file, pre-migration); a new write that introduces
            # the field at version=1 is the natural migration path.
            old_has_version, old_version = worklist_version_from_text(old)
            new_has_version, new_version = worklist_version_from_text(new)
            if old_has_version and (
                not new_has_version or new_version != old_version + 1
            ):
                deny_stale_worklist_version(
                    old_version, new_version, new_has_version
                )
            emit_allow_for_lifecycle(rel, "worklist-author")
        deny_mechanical_worklist_change(removed, status_changed)

    # Branch 2: worklist draft prose files (already covered by the
    # pre-Branch-1 lifecycle short-circuit; left as a guardrail).
    if is_worklist_draft(rel):
        emit_allow_for_lifecycle(rel, "worklist-draft")

    # Branch 2: writes to any other project file — require worklist coverage,
    # fresh bypass, or explicit opt-out language in the last user message.
    covered = worklist_covered_files(project_root)
    if rel in covered:
        _trace_hook("PreToolUse", tool_name, rel, "allow", "covered-by-worklist-item")
        sys.exit(0)
    if fresh_bypass(project_root, rel):
        _trace_hook("PreToolUse", tool_name, rel, "allow", "fresh-bypass")
        sys.exit(0)
    if opt_out_clears(project_root, payload, tool_name, rel):
        sys.exit(0)
    last_msg = last_user_text(payload.get("transcript_path", ""))
    deny_coverage(rel, opt_out_attempted=("worklist" in (last_msg or "").lower()
                                          and "no" in (last_msg or "").lower()))


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        self_test()
        print("worklist-guard self-test passed")
        sys.exit(0)
    try:
        main()
    except SystemExit:
        # main() signals its decision by exiting; never convert that.
        raise
    except BaseException as exc:  # noqa: BLE001 - deliberate catch-all
        # Fail closed. A crashing guard exits non-zero-but-not-2, which the
        # provider reads as "hook failed, proceed" -- an enforcement bypass
        # that leaves no receipt, because the crash happens upstream of the
        # guard's own instrumentation. That is exactly how the cp1252
        # transcript crash hid on Windows (judell/bram#249): registered,
        # reported healthy by Setup and Status, never once enforcing.
        #
        # Blocking on a bug is a visible nuisance reported in minutes;
        # allowing on a bug is an invisible hole. Prefer the nuisance. The
        # permission-menu hook deliberately stays fail-open: it is
        # observe-only and must never gate a tool call.
        summary = "%s: %s" % (type(exc).__name__, exc)
        try:
            _trace_hook(
                os.environ.get("__BRAM_TRACE_EVENT", "PreToolUse"),
                os.environ.get("__BRAM_TRACE_TOOL", ""),
                "",
                "error",
                summary[:200],
            )
        except BaseException:
            pass
        sys.stderr.write(
            "Blocked: the Bram worklist guard failed and denied by default."
            + NL
            + "  - "
            + summary[:500]
            + NL
            + "  - This is a guard bug, not a policy decision. Please report it."
            + NL
        )
        sys.exit(2)
