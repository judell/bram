#!/usr/bin/env python3
"""Codex PreToolUse hook: enforce the xmlui-desktop two-stage worklist flow.

Stdin payload (from codex):
  {session_id, turn_id, cwd, hook_event_name, model, permission_mode,
   tool_name, tool_input, tool_use_id}

Canonical tool_name is `apply_patch` for file edits (Write/Edit are matcher
aliases on the codex side but the stdin always says apply_patch). `Bash`
is the other surface — codex can sed -i / tee / python -c around the
apply_patch gate, so we cover it too. MCP tools (tool_name starts with
`mcp__`) are the third surface: any user with `[mcp_servers.filesystem]`
in ~/.codex/config.toml can route writes through mcp__filesystem__write_text_file
/ edit_file / move_file etc., which would otherwise bypass apply_patch
entirely.

Response on block (stdout JSON):
  {"permissionDecision":"deny","permissionDecisionReason":"<non-empty>"}

Default (allow): exit 0 with no output.

Project-awareness: if the cwd does not contain
resources/.worklist-authorization.json, this is not an xmlui-desktop-managed
repo and the guard exits 0 (allow everything). xmlui-desktop's setup writes
that file the first time it's run in a project.
"""

import json
import os
import re
import sys
import time
from datetime import datetime, timezone


WORKLIST_REL = "resources/worklist.json"
AUTH_REL = "resources/.worklist-authorization.json"
BYPASS_TTL_SECONDS = 60 * 60  # an authorization record is fresh for 1h


# Issue #49 [hook] trace + issue #95 phantom-write diagnostic.
# - Always emits one `[worklist-guard]` line to stderr, including cwd,
#   so the hook's decision is visible to the agent / user without
#   BRAM_TRACE being enabled. Refs #95.
# - Additionally appends to resources/bram-trace.log when BRAM_TRACE=1
#   and BRAM_TRACE_LOG is set on the agent's PTY child env (existing
#   issue #49 behavior).
def _trace_hook(event, tool, target, decision, reason, cwd=None):
    if cwd is None:
        cwd = _HOOK_CTX.get("cwd", "")
    diagnostic = (
        f"[worklist-guard] tool={tool} target={target} cwd={cwd} "
        f"decision={decision} reason={reason}"
    )
    try:
        sys.stderr.write(diagnostic + "\n")
        sys.stderr.flush()
    except Exception:
        pass
    try:
        if os.environ.get("BRAM_TRACE") != "1":
            return
        log_path = os.environ.get("BRAM_TRACE_LOG")
        if not log_path:
            return
        now = datetime.now(timezone.utc)
        ts = now.strftime("%Y-%m-%dT%H:%M:%S.") + f"{now.microsecond // 1000:03d}Z"
        line = (
            f"[{ts}] [hook] script=worklist-guard-codex.py event={event} "
            f"tool={tool} target={target} cwd={cwd} "
            f"decision={decision} reason={reason}\n"
        )
        with open(log_path, "a") as f:
            f.write(line)
    except Exception:
        pass


# Module-level context for [hook] trace records. main() populates these
# once the inbound payload is parsed; allow() / deny() read them on exit.
_HOOK_CTX = {"event": "", "tool": "", "target": "", "cwd": ""}


def allow(reason="passed-checks"):
    _trace_hook(
        _HOOK_CTX["event"] or "PreToolUse",
        _HOOK_CTX["tool"] or "",
        _HOOK_CTX["target"] or "",
        "allow",
        reason,
    )
    sys.exit(0)


def deny(reason):
    _trace_hook(
        _HOOK_CTX["event"] or "PreToolUse",
        _HOOK_CTX["tool"] or "",
        _HOOK_CTX["target"] or "",
        "deny",
        # Trim the deny message to a short reason for the trace; the
        # full message still goes through the permissionDecisionReason
        # field below for codex to surface to the user/agent.
        (reason or "").splitlines()[0][:120] if reason else "blocked",
    )
    print(json.dumps({
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }))
    sys.exit(0)


def load_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except Exception:
        return None


def worklist_items(cwd):
    data = load_json(os.path.join(cwd, WORKLIST_REL))
    if not isinstance(data, dict):
        return []
    items = data.get("items")
    return items if isinstance(items, list) else []


def items_by_id_from_content(content):
    try:
        doc = json.loads(content)
    except Exception:
        return {}
    if not isinstance(doc, dict):
        return {}
    items = doc.get("items")
    if not isinstance(items, list):
        return {}
    out = {}
    for item in items:
        if not isinstance(item, dict):
            continue
        item_id = item.get("id")
        if isinstance(item_id, str) and item_id.strip():
            out[item_id] = item
    return out


def current_worklist_text(cwd):
    try:
        with open(os.path.join(cwd, WORKLIST_REL)) as f:
            return f.read()
    except Exception:
        return ""


def covered_files(items):
    """Return set of project-relative paths covered by any proposed/applied item."""
    covered = set()
    for it in items:
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


def fresh_bypass(cwd, path_rel):
    """True iff resources/.worklist-authorization.json carries a recent
    direct-edit bypass authorization covering path_rel."""
    rec = load_json(os.path.join(cwd, AUTH_REL))
    if not isinstance(rec, dict):
        return False
    if rec.get("kind") != "direct-edit":
        return False
    issued = rec.get("issued_at_ms") or 0
    if (time.time() * 1000 - issued) > BYPASS_TTL_SECONDS * 1000:
        return False
    paths = rec.get("paths") or []
    return path_rel in paths or "*" in paths


def normalize_target(cwd, target):
    """Return project-relative path for target if it's inside cwd, else None."""
    if not isinstance(target, str) or not target:
        return None
    abs_target = os.path.abspath(os.path.join(cwd, target))
    abs_cwd = os.path.abspath(cwd)
    if abs_target == abs_cwd:
        return ""
    prefix = abs_cwd + os.sep
    if abs_target.startswith(prefix):
        return abs_target[len(prefix):].replace(os.sep, "/")
    return None


# codex apply_patch format:
#   *** Begin Patch
#   *** Update File: path/to/file
#   *** Add File: path/to/new
#   *** Delete File: path/to/old
#   *** End Patch
_PATCH_PATH_RE = re.compile(
    r"^\*\*\* (?:Update|Add|Delete) File: (.+?)\s*$", re.MULTILINE
)

# Unified diff fallback (in case codex ever wraps in standard diff):
#   --- a/path
#   +++ b/path
_UNIFIED_PATH_RE = re.compile(r"^\+\+\+ b/(.+?)\s*$", re.MULTILINE)


def patch_targets(tool_input):
    """Extract project-relative paths that an apply_patch would mutate."""
    text = ""
    if isinstance(tool_input, dict):
        for key in ("input", "patch", "content", "command"):
            v = tool_input.get(key)
            if isinstance(v, str):
                text += "\n" + v
    elif isinstance(tool_input, str):
        text = tool_input
    targets = set()
    for m in _PATCH_PATH_RE.finditer(text):
        targets.add(m.group(1))
    for m in _UNIFIED_PATH_RE.finditer(text):
        targets.add(m.group(1))
    return targets


def patch_text(tool_input):
    text = ""
    if isinstance(tool_input, dict):
        for key in ("input", "patch", "content", "command"):
            v = tool_input.get(key)
            if isinstance(v, str):
                text += "\n" + v
    elif isinstance(tool_input, str):
        text = tool_input
    return text


# Bash commands we deny without worklist coverage. The list is intentionally
# narrow: codex needs to run plenty of read-only shell during investigation,
# so we don't gate ls/grep/cat/curl/find/git-status. We only catch the
# patterns codex would use to bypass apply_patch.
_BASH_WRITE_PATTERNS = [
    re.compile(r"(^|[\s;&|`(])>+\s*[^\s>&]"),         # > file or >> file
    re.compile(r"(^|[\s;&|`(])tee\b"),                # tee
    re.compile(r"(^|[\s;&|`(])sed\s+[^|;&]*-i\b"),    # sed -i
    re.compile(r"(^|[\s;&|`(])perl\s+[^|;&]*-i\b"),   # perl -i
    re.compile(r"(^|[\s;&|`(])(rm|mv|cp|truncate|install)\b"),
    re.compile(r"(^|[\s;&|`(])git\s+(add|commit|push|rm|mv|reset|checkout|restore|stash|am|apply|cherry-pick|rebase|revert|tag|branch)\b"),
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


# MCP tool naming convention is `mcp__<server>__<tool>` (verified in
# codex-rs/core/tests/suite/hooks_mcp.rs). We treat MCP tools as mutation
# candidates only when the tool-name suffix contains a recognized mutation
# token. Read-only MCP tools (read_, list_, search_, get_, info, stat) pass
# through unconditionally so the guard doesn't break non-filesystem MCP
# servers (databases, search backends, etc.).
_MCP_WRITE_TOKENS = (
    "write", "edit", "create", "delete", "remove", "rename",
    "move", "copy", "patch", "append", "truncate", "mkdir", "rmdir",
    "modify", "replace", "save", "set_",
)

# Common path-shaped keys in MCP tool_input payloads. The standard
# @modelcontextprotocol/server-filesystem uses `path`, `source`, `destination`;
# other servers may use `file_path`, `target_path`, etc. We check all of them.
_MCP_PATH_KEYS = (
    "path", "file_path", "filepath", "filename",
    "source", "src", "destination", "dest", "dst",
    "target", "target_path", "to", "from",
)


def mcp_is_mutation(tool_name):
    """True iff an mcp__-prefixed tool name signals filesystem mutation."""
    name = tool_name.lower()
    return any(tok in name for tok in _MCP_WRITE_TOKENS)


# Validators for worklist.json mutations. The PreToolUse hook lets writes to
# the worklist file pass the coverage check (it's how proposing works), but
# we still want to require that each proposed item carries non-empty
# before/after content. Title-only proposals are useless for review and
# defeat the audit-trail purpose of the two-stage flow.

_ID_RE = re.compile(r'"id"\s*:\s*"([^"]+)"')
_NONEMPTY_STRING_VAL = re.compile(r'"\s*([^"]+?)\s*"')  # generic helper


def _added_block_text(patch_text):
    """Extract just the '+' added lines from a unified-diff or codex patch.
    Returns a single string with the leading '+' removed from each line."""
    out = []
    for line in patch_text.split("\n"):
        if line.startswith("+") and not line.startswith("+++"):
            out.append(line[1:])
    return "\n".join(out)


def _removed_block_text(patch_text):
    out = []
    for line in patch_text.split("\n"):
        if line.startswith("-") and not line.startswith("---"):
            out.append(line[1:])
    return "\n".join(out)


def _item_has_file(it):
    """A worklist item must name what it touches: either non-empty `file` or
    non-empty `files` array (at least one non-empty string entry)."""
    f = it.get("file")
    if isinstance(f, str) and f.strip():
        return True
    fs = it.get("files")
    if isinstance(fs, list):
        for entry in fs:
            if isinstance(entry, str) and entry.strip():
                return True
    return False


def _worklist_items_with_empty_body(content):
    """Parse a full worklist.json content string and return a list of
    (id, missing_fields) tuples for proposed items missing any required
    field. Required: id (non-empty), file or files (non-empty), before
    (non-empty), after (non-empty). Returns None if the content can't be
    parsed as JSON (caller decides how to handle)."""
    try:
        doc = json.loads(content)
    except Exception:
        return None
    items = doc.get("items") if isinstance(doc, dict) else None
    if not isinstance(items, list):
        return []
    bad = []
    for it in items:
        if not isinstance(it, dict):
            continue
        status = it.get("status", "proposed")
        if status != "proposed":
            continue  # only enforce the schema requirement on still-pending items
        missing = []
        item_id = it.get("id")
        if not isinstance(item_id, str) or not item_id.strip():
            missing.append("id")
        if not _item_has_file(it):
            missing.append("file (or non-empty files array)")
        before = it.get("before")
        if not isinstance(before, str) or not before.strip():
            missing.append("before")
        after = it.get("after")
        if not isinstance(after, str) or not after.strip():
            missing.append("after")
        if missing:
            label = item_id if (isinstance(item_id, str) and item_id.strip()) else "<no-id>"
            bad.append((label, missing))
    return bad


def _patch_adds_have_empty_bodies(patch_text):
    """Heuristic for apply_patch on worklist.json: scan the patch's added
    lines for new proposed items and verify each has all four required
    fields (id, file or files, before, after) with non-empty values.
    Returns a list of (label, missing_fields) tuples matching the JSON-path
    validator's shape, or [] if everything looks OK or we can't tell."""
    added = _added_block_text(patch_text)
    id_re = _ID_RE
    file_re = re.compile(r'"file"\s*:\s*"([^"]*?)"')
    files_re = re.compile(r'"files"\s*:\s*\[\s*"')  # files array opener
    before_re = re.compile(r'"before"\s*:\s*"([^"]*?)"')
    after_re = re.compile(r'"after"\s*:\s*"([^"]*?)"')
    ids = [v for v in id_re.findall(added) if v.strip()]
    files = [v for v in file_re.findall(added) if v.strip()] + files_re.findall(added)
    befores = [b for b in before_re.findall(added) if b.strip()]
    afters = [a for a in after_re.findall(added) if a.strip()]
    # Item count is the max of any field count — covers the case where
    # codex adds before/after pairs without an accompanying id (ids=0 but
    # before/after count > 0 still means new items being added).
    item_count = max(len(ids), len(befores), len(afters))
    if item_count == 0:
        return []  # no new items being added (status change, etc.)
    missing = []
    if len(ids) < item_count:
        missing.append(f"id (saw {len(ids)} of {item_count})")
    if len(files) < item_count:
        missing.append(f"file or files (saw {len(files)} of {item_count})")
    if len(befores) < item_count:
        missing.append(f"before (saw {len(befores)} of {item_count})")
    if len(afters) < item_count:
        missing.append(f"after (saw {len(afters)} of {item_count})")
    if missing:
        label = ids[0] if ids else "<missing-id>"
        return [(label, missing)]
    return []


def worklist_state_changes(old_content, new_content):
    old_items = items_by_id_from_content(old_content)
    new_items = items_by_id_from_content(new_content)
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


def _patch_removes_worklist_items(cwd, patch_text):
    old_items = {
        item["id"]: item
        for item in worklist_items(cwd)
        if isinstance(item, dict) and isinstance(item.get("id"), str)
    }
    removed_ids = {item_id for item_id in _ID_RE.findall(_removed_block_text(patch_text)) if item_id}
    readded_ids = {item_id for item_id in _ID_RE.findall(_added_block_text(patch_text)) if item_id}
    out = []
    for item_id in sorted(removed_ids - readded_ids):
        item = old_items.get(item_id)
        if item is None:
            continue
        out.append((item_id, item.get("status", "proposed")))
    return out


_STATUS_RE = re.compile(r'"status"\s*:\s*"([^"]+)"')


def _patch_changes_worklist_status(patch_text):
    removed = set(_STATUS_RE.findall(_removed_block_text(patch_text)))
    added = set(_STATUS_RE.findall(_added_block_text(patch_text)))
    if not removed and not added:
        return False
    return removed != added


def _worklist_new_content_from_tool_input(cwd, tool_input):
    old_content = current_worklist_text(cwd)
    if not isinstance(tool_input, dict):
        return None
    for key in ("content", "text"):
        value = tool_input.get(key)
        if isinstance(value, str):
            return value
    edits = tool_input.get("edits")
    if isinstance(edits, list):
        new_content = old_content
        for edit in edits:
            if not isinstance(edit, dict):
                return None
            old_text = edit.get("oldText")
            new_text = edit.get("newText")
            if not isinstance(old_text, str) or not isinstance(new_text, str):
                return None
            new_content = new_content.replace(old_text, new_text, 1)
        return new_content
    old_text = tool_input.get("old_string")
    new_text = tool_input.get("new_string")
    if isinstance(old_text, str) and isinstance(new_text, str):
        return old_content.replace(old_text, new_text, 1)
    return None


def _worklist_validation_error(bad, tool_name):
    if not bad:
        return f"{tool_name} blocked: worklist validation failed."
    lines = []
    for label, missing in bad:
        lines.append(f"  - item {label}: missing {', '.join(missing)}")
    detail = "\n".join(lines)
    return (
        f"{tool_name} blocked: proposed worklist item(s) are missing required "
        f"fields.\n{detail}\n"
        f"Required for every proposed item: \"id\" (kebab-case identifier), "
        f"\"file\" (or \"files\" array for multi-file items), "
        f"\"before\" (current state + alternatives considered + why rejected), "
        f"and \"after\" (the planned change). Title-only or body-only items "
        f"are not acceptable. Rewrite the worklist with complete items and try "
        f"again."
    )


def _mechanical_worklist_change_error(removed, status_changed, tool_name):
    lines = [
        f"{tool_name} blocked: mechanical worklist state changes must go through "
        f"`POST /__worklist/mutate`, not a direct edit to `resources/worklist.json`.",
        "Direct worklist edits are for proposing items or refining prose during iterate.",
        "Use mutate for `prune` and `advance` after a verified `drop:` / `approved:` turn.",
    ]
    if removed:
        detail = ", ".join(f'"{item_id}" (status={status})' for item_id, status in removed)
        lines.append(f"Removed item ids: {detail}")
    if status_changed:
        detail = ", ".join(
            f'"{item_id}" ({old_status}->{new_status})'
            for item_id, old_status, new_status in status_changed
        )
        lines.append(f"Status changes: {detail}")
    lines.append(
        "Example: "
        "curl -X POST -d '{\"op\":\"advance\",\"ids\":[\"item-id\"],\"status\":\"applied\"}' "
        "http://localhost:${BRAM_PORT:-$XMLUI_DESKTOP_PORT}/__worklist/mutate"
    )
    return "\n".join(lines)


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


def emit_additional_context(text):
    """UserPromptSubmit response that injects context into the model turn."""
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": text,
        }
    }))
    sys.exit(0)


# Compact reminder injected only on prompts that look like change requests in
# managed repos. The PreToolUse hook is the runtime backstop; this is the
# pre-emptive nudge that aims to head off codex's inspect-narrate-edit reflex.
# The full conventions live in .claude/xmlui-desktop-conventions.md, which
# codex reads on first prompt; this stays short to avoid drowning the model
# in repeated boilerplate.
GATE_REMINDER = (
    "xmlui-desktop worklist gate. First response to a change request must be "
    "(a) clarify, (b) propose items to resources/worklist.json (each with "
    "non-empty before/after), or (c) read-only investigation prefaced "
    "\"I don't yet have enough context to propose\". Mutations outside approved "
    "items are blocked at runtime. Full convention: "
    ".claude/xmlui-desktop-conventions.md"
)

# Heuristic for "this prompt is asking for a change." Keep permissive (better
# to inject one extra reminder than miss a real change request), but skip
# short greetings, questions, and status checks where the gate doesn't apply.
_CHANGE_KEYWORDS = re.compile(
    r"\b("
    r"fix(?:es|ed|ing)?|add(?:s|ed|ing)?|chang(?:e|es|ed|ing)|"
    r"updat(?:e|es|ed|ing)|modif(?:y|ies|ied|ying)|implement(?:s|ed|ing)?|"
    r"creat(?:e|es|ed|ing)|build(?:s|ing|t)?|rewrit(?:e|es|ing)|"
    r"refactor(?:s|ed|ing)?|edit(?:s|ed|ing)?|delet(?:e|es|ed|ing)|"
    r"remov(?:e|es|ed|ing)|renam(?:e|es|ed|ing)|"
    r"patch(?:es|ed|ing)?|improv(?:e|es|ed|ing)|convert(?:s|ed|ing)?|"
    r"migrat(?:e|es|ed|ing)|extend(?:s|ed|ing)?|integrat(?:e|es|ed|ing)|"
    r"replac(?:e|es|ed|ing)|tweak(?:s|ed|ing)?|adjust(?:s|ed|ing)?|"
    r"broken|missing|wrong|"
    r"let'?s|please|i want|i'?d like|can you|could you|make (?:it|the|a|an)|"
    r"should (?:be|have|use)"
    r")\b",
    re.IGNORECASE,
)


def looks_like_change_request(prompt):
    if not isinstance(prompt, str):
        return False
    # Sub-30-char prompts are usually navigation or chit-chat; skip them.
    if len(prompt.strip()) < 30:
        return False
    return bool(_CHANGE_KEYWORDS.search(prompt))


def handle_user_prompt_submit(payload, cwd):
    # Only inject in xmlui-desktop-managed repos (presence of auth file).
    if not os.path.exists(os.path.join(cwd, AUTH_REL)):
        allow()
    if not looks_like_change_request(payload.get("prompt", "")):
        allow()
    emit_additional_context(GATE_REMINDER)


def main():
    try:
        payload = json.load(sys.stdin)
    except Exception:
        allow()

    cwd = payload.get("cwd") or os.getcwd()
    event_name = payload.get("hook_event_name") or ""
    _HOOK_CTX["event"] = event_name or "PreToolUse"
    _HOOK_CTX["cwd"] = cwd

    if event_name == "UserPromptSubmit":
        handle_user_prompt_submit(payload, cwd)

    # PreToolUse path: also require the managed-repo marker.
    if not os.path.exists(os.path.join(cwd, AUTH_REL)):
        allow()

    tool_name = payload.get("tool_name") or ""
    tool_input = payload.get("tool_input") or {}
    _HOOK_CTX["tool"] = tool_name
    # Best-effort target derivation for the trace. apply_patch gets the
    # first patch target; Bash gets the command preview; anything else
    # leaves target empty.
    if tool_name == "apply_patch":
        targets = patch_targets(tool_input) if isinstance(tool_input, dict) else []
        if targets:
            t0 = normalize_target(cwd, targets[0]) or targets[0]
            _HOOK_CTX["target"] = t0
    elif tool_name == "Bash" and isinstance(tool_input, dict):
        cmd = tool_input.get("command") or ""
        _HOOK_CTX["target"] = (cmd or "")[:80]

    items = worklist_items(cwd)
    covered = covered_files(items)

    if tool_name == "apply_patch":
        raw_targets = patch_targets(tool_input)
        if not raw_targets:
            deny("apply_patch blocked: could not parse target file(s) from "
                 "the patch payload. Propose the change in resources/worklist.json "
                 "first so the guard can verify coverage.")
        # If the patch touches the worklist file, validate item bodies.
        touches_worklist = any(
            normalize_target(cwd, t) == WORKLIST_REL for t in raw_targets
        )
        if touches_worklist:
            patch_body = patch_text(tool_input)
            removed = _patch_removes_worklist_items(cwd, patch_body)
            if removed or _patch_changes_worklist_status(patch_body):
                deny(_mechanical_worklist_change_error(removed, [], "apply_patch"))
            bad_ids = _patch_adds_have_empty_bodies(patch_body)
            if bad_ids:
                deny(_worklist_validation_error(bad_ids, "apply_patch"))
        violations = []
        for t in raw_targets:
            rel = normalize_target(cwd, t)
            if rel is None:
                # Outside the project tree — let codex handle, hook isn't
                # the gate for non-project paths.
                continue
            if rel == WORKLIST_REL:
                continue  # writing to the worklist itself is how proposing works
            if rel in covered:
                continue
            if fresh_bypass(cwd, rel):
                continue
            violations.append(rel)
        if violations:
            bad = ", ".join(violations)
            deny(
                f"apply_patch blocked: {bad} is not covered by any proposed "
                f"or applied item in resources/worklist.json, and no fresh "
                f"direct-edit authorization covers it. Propose the change in "
                f"the worklist first (status: 'proposed'), wait for the user's "
                f"approved: payload, then retry."
            )
        allow()

    if tool_name == "Bash":
        cmd = tool_input.get("command") if isinstance(tool_input, dict) else ""
        if not bash_writes(cmd):
            allow()
        # Mutating shell command — require any worklist coverage OR a "*"
        # bypass. We don't try to map shell commands to specific paths
        # (too fragile); presence of any pending/applied work is the gate.
        if covered or fresh_bypass(cwd, "*"):
            allow()
        deny(
            "Bash blocked: this command writes to the filesystem, and "
            "resources/worklist.json has no proposed or applied items "
            "covering the change. Propose the work in the worklist first, "
            "or have the user issue a direct-edit authorization."
        )

    if tool_name.startswith("mcp__"):
        if not mcp_is_mutation(tool_name):
            allow()
        candidate_paths = mcp_paths(tool_input)
        if not candidate_paths:
            # Mutation-shaped MCP tool with no path-shaped fields we
            # recognize. Don't try to be clever — block by default and
            # name the surface so the user can extend the guard if it's
            # a legitimate non-filesystem mutation.
            deny(
                f"{tool_name} blocked: looks like a mutation but the guard "
                f"could not extract any file path from tool_input. Propose "
                f"the change in resources/worklist.json first, or extend "
                f"worklist-guard-codex.py to recognize this MCP tool's input shape."
            )
        # If any candidate path is the worklist file and tool_input provides
        # the full new content, validate item bodies.
        touches_worklist = any(
            normalize_target(cwd, t) == WORKLIST_REL for t in candidate_paths
        )
        if touches_worklist and isinstance(tool_input, dict):
            new_content = _worklist_new_content_from_tool_input(cwd, tool_input)
            if new_content is None:
                deny(
                    f"{tool_name} blocked: worklist edits that advance status or prune items "
                    f"must use `/__worklist/mutate`. For direct authoring/refinement edits, "
                    f"use a write/edit shape whose resulting content the guard can inspect."
                )
            if isinstance(new_content, str) and new_content.strip():
                bad_ids = _worklist_items_with_empty_body(new_content)
                if bad_ids:
                    deny(_worklist_validation_error(bad_ids, tool_name))
                removed, status_changed = worklist_state_changes(
                    current_worklist_text(cwd),
                    new_content,
                )
                if removed or status_changed:
                    deny(_mechanical_worklist_change_error(removed, status_changed, tool_name))
        violations = []
        for t in candidate_paths:
            rel = normalize_target(cwd, t)
            if rel is None:
                continue  # outside the project tree
            if rel == WORKLIST_REL:
                continue
            if rel in covered:
                continue
            if fresh_bypass(cwd, rel):
                continue
            violations.append(rel)
        if violations:
            bad = ", ".join(violations)
            deny(
                f"{tool_name} blocked: {bad} is not covered by any proposed "
                f"or applied item in resources/worklist.json, and no fresh "
                f"direct-edit authorization covers it. Propose the change in "
                f"the worklist first, wait for the user's approved: payload, "
                f"then retry."
            )
        allow()

    allow()


if __name__ == "__main__":
    main()
