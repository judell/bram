#!/usr/bin/env python3
"""Codex PreToolUse hook: enforce the Bram two-stage worklist flow.

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
resources/.worklist-authorization.json, this is not a Bram-managed
repo and the guard exits 0 (allow everything). Bram's setup writes that file
the first time it's run in a project.
"""

import json
import os
import re
import sys
import tempfile
import urllib.request
import time
from datetime import datetime, timezone
from pathlib import Path


WORKLIST_REL = "resources/worklist.json"
WORKLIST_DRAFTS_PREFIX = "resources/worklist-drafts/"
AUTH_REL = "resources/.worklist-authorization.json"
# Codex filesystem lifecycle channel (#130). Writing the intent file is how
# Codex drives the worklist lifecycle (the loopback-curl equivalent), so it is
# never an uncovered repo mutation. The result file is host-written, exempted
# for symmetry.
WORKLIST_INTENT_REL = "resources/.worklist-intent.json"
WORKLIST_RESULT_REL = "resources/.worklist-result.json"
BYPASS_TTL_SECONDS = 60 * 60  # an authorization record is fresh for 1h


# Issue #49 [hook] trace + issue #95 phantom-write diagnostic.
# - Always emits one `[worklist-guard]` line to stderr, including cwd,
#   so the hook's decision is visible to the agent / user without
#   tracing enabled. Refs #95.
# - Additionally POSTs the line to the host's /__hook-trace route, which
#   appends to resources/bram-traces/bram-trace.log iff the live Traces
#   setting is on (hook-trace-follow-settings).
def _post_hook_trace(event, tool, target, decision, reason, cwd):
    """Ship the [hook] decision line to the host's /__hook-trace route, which
    appends to bram-trace.log iff the LIVE Traces setting is on
    (hook-trace-follow-settings). Replaces spawn-time BRAM_TRACE env gating,
    which desynced from the Settings toggle for running sessions. Failures
    are swallowed; tracing must never block a tool call."""
    try:
        cur = os.path.abspath(cwd or os.getcwd())
        port = None
        while True:
            candidate = os.path.join(cur, "resources", ".bram-port")
            if os.path.exists(candidate):
                with open(candidate) as f:
                    port = int(f.read().strip())
                break
            parent = os.path.dirname(cur)
            if parent == cur:
                break
            cur = parent
        if not port:
            return
        body = json.dumps({
            "script": "codex-worklist-guard.py",
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
    _post_hook_trace(event, tool, target, decision, reason, cwd)


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
        # full message goes to stderr below for codex to surface.
        (reason or "").splitlines()[0][:120] if reason else "blocked",
    )
    # Codex PreToolUse contract: exit code 2 + reason on stderr.
    # The flat {"permissionDecision":"deny",...} JSON the Claude hook
    # emits is invalid PreToolUse output for codex — codex rejects
    # it, logs "PreToolUse hook (failed)", and the tool call proceeds
    # instead of being blocked. exit(2)+stderr is universally
    # supported and avoids any JSON nesting ambiguity.
    sys.stderr.write((reason or "blocked") + "\n")
    sys.stderr.flush()
    sys.exit(2)


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


def is_coordination_file(rel):
    """Codex lifecycle channel files (#130) — always allowed, like a curl to
    the loopback lifecycle routes was. Not a tracked repo mutation."""
    return rel in (WORKLIST_INTENT_REL, WORKLIST_RESULT_REL)


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


def is_worklist_draft(rel):
    return (
        isinstance(rel, str)
        and rel.startswith(WORKLIST_DRAFTS_PREFIX)
        and rel.endswith(".md")
        and "/" not in rel[len(WORKLIST_DRAFTS_PREFIX):]
    )


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


def _draft_exists(cwd, item_id):
    if not isinstance(item_id, str) or not item_id.strip():
        return False
    if "/" in item_id or "\\" in item_id:
        return False
    return os.path.exists(
        os.path.join(cwd, WORKLIST_DRAFTS_PREFIX, f"{item_id}.md")
    )


def _worklist_items_violating_draft_only(content, cwd=None):
    """Parse a full worklist.json content string and return a list of
    (id, violations) tuples for proposed items that break the draft-only
    rule. Violations: inline `before` (non-empty), inline `after`
    (non-empty), missing `id`, missing `file`/`files`. Draft prose must
    live in resources/worklist-drafts/<id>.md; inline keys are rejected.
    Returns None if the content can't be parsed as JSON (caller decides
    how to handle); cwd is accepted for call-site parity but not used by
    the JSON-content path (no draft-existence check at hook time — let
    the server's _draftMissing diagnostic surface missing drafts)."""
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
        if it.get("status", "proposed") != "proposed":
            continue
        violations = []
        item_id = it.get("id")
        if not isinstance(item_id, str) or not item_id.strip():
            violations.append("id")
        if not _item_has_file(it):
            violations.append("file (or non-empty files array)")
        before = it.get("before")
        if isinstance(before, str) and before.strip():
            violations.append("inline `before` (move to resources/worklist-drafts/<id>.md)")
        after = it.get("after")
        if isinstance(after, str) and after.strip():
            violations.append("inline `after` (move to resources/worklist-drafts/<id>.md)")
        if violations:
            label = item_id if (isinstance(item_id, str) and item_id.strip()) else "<no-id>"
            bad.append((label, violations))
    return bad


def _patch_adds_violating_draft_only(patch_text, cwd=None):
    """Heuristic for apply_patch on worklist.json: scan the patch's added
    lines for new proposed items that break the draft-only rule.
    Violations: presence of `"before":` / `"after":` with non-empty
    values, or missing `id`/`file`/`files`. Returns a list of
    (label, violations) tuples matching _worklist_items_violating_draft_only's
    shape, or [] if everything looks OK or we can't tell. cwd accepted
    for call-site parity but unused."""
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
    # Item count is the max of id/file count — before/after presence is
    # itself a violation, so we don't infer item count from it.
    item_count = max(len(ids), len(files))
    violations = []
    if item_count > 0:
        if len(ids) < item_count:
            violations.append(f"id (saw {len(ids)} of {item_count})")
        if len(files) < item_count:
            violations.append(f"file or files (saw {len(files)} of {item_count})")
    if befores:
        violations.append(f"inline `before` (move to resources/worklist-drafts/<id>.md; saw {len(befores)})")
    if afters:
        violations.append(f"inline `after` (move to resources/worklist-drafts/<id>.md; saw {len(afters)})")
    if violations:
        label = ids[0] if ids else "<missing-id>"
        return [(label, violations)]
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


def worklist_version_from_text(text):
    """Return (present, version) for a worklist.json text. `present` is
    True iff JSON parsed and the top-level `version` field was an int.
    `version` defaults to 0 when absent or malformed. Mirrors the Claude
    hook's helper; keep in sync."""
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


def _stale_worklist_version_error(old_version, new_version, new_present, tool_name):
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
    return (
        f"{tool_name} blocked: stale base on resources/worklist.json. "
        + detail
        + " Re-read the file, base your edit on the current contents, "
        "and include the bumped version field. This guards against "
        "concurrent-writer races between your propose / apply_patch "
        "and other agents or the /__worklist/mutate route."
    )


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


def _worklist_content_after_apply_patch(cwd, patch_text):
    """Best-effort apply_patch interpreter for resources/worklist.json.

    Codex patches carry enough context to reconstruct the post-edit file for
    ordinary Update File hunks. Do that so worklist state validation can use
    item-id-aware old/new JSON comparison instead of textual status heuristics.
    Return None when the patch shape is too unusual to apply confidently.
    """
    old_content = current_worklist_text(cwd)
    if not old_content:
        return None

    lines = patch_text.splitlines()
    i = 0
    content = old_content
    saw_worklist_update = False
    while i < len(lines):
        line = lines[i]
        if line != f"*** Update File: {WORKLIST_REL}":
            i += 1
            continue
        saw_worklist_update = True
        i += 1
        while i < len(lines):
            line = lines[i]
            if line.startswith("*** ") and line != "*** End of File":
                break
            if not line.startswith("@@"):
                i += 1
                continue
            i += 1
            old_parts = []
            new_parts = []
            while i < len(lines):
                hline = lines[i]
                if hline.startswith("@@") or hline.startswith("*** "):
                    break
                if hline.startswith(" "):
                    old_parts.append(hline[1:])
                    new_parts.append(hline[1:])
                elif hline.startswith("-"):
                    old_parts.append(hline[1:])
                elif hline.startswith("+"):
                    new_parts.append(hline[1:])
                else:
                    return None
                i += 1
            old_text = "\n".join(old_parts)
            new_text = "\n".join(new_parts)
            if old_text:
                if old_text not in content:
                    return None
                content = content.replace(old_text, new_text, 1)
            elif new_text:
                return None
        continue

    return content if saw_worklist_update else None


def _self_test_replacement_patch(old_content, new_content):
    return (
        "*** Begin Patch\n"
        f"*** Update File: {WORKLIST_REL}\n"
        "@@\n"
        + "".join("-" + line for line in old_content.splitlines(True))
        + "".join("+" + line for line in new_content.splitlines(True))
        + "*** End Patch\n"
    )


def self_test():
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        (root / "resources").mkdir()
        old_doc = {
            "description": "",
            "items": [
                {
                    "id": "existing",
                    "status": "proposed",
                    "file": "a.txt",
                }
            ],
        }
        old_content = json.dumps(old_doc, indent=2) + "\n"
        (root / WORKLIST_REL).write_text(old_content)

        new_item_doc = {
            "description": "",
            "items": old_doc["items"]
            + [
                {
                    "id": "new-item",
                    "status": "proposed",
                    "file": "b.txt",
                }
            ],
        }
        new_item_content = json.dumps(new_item_doc, indent=2) + "\n"
        applied = _worklist_content_after_apply_patch(
            str(root),
            _self_test_replacement_patch(old_content, new_item_content),
        )
        assert applied == new_item_content
        assert _worklist_items_violating_draft_only(applied, str(root)) == []
        assert worklist_state_changes(old_content, applied) == ([], [])

        # Inline prose on a new proposed item is rejected.
        inline_doc = {
            "description": "",
            "items": old_doc["items"]
            + [
                {
                    "id": "inline-item",
                    "status": "proposed",
                    "file": "b.txt",
                    "before": "old",
                    "after": "new",
                }
            ],
        }
        inline_content = json.dumps(inline_doc, indent=2) + "\n"
        inline_bad = _worklist_items_violating_draft_only(inline_content, str(root))
        assert inline_bad and inline_bad[0][0] == "inline-item"
        # Patch-text validator also flags an apply_patch that adds inline prose.
        inline_patch_bad = _patch_adds_violating_draft_only(
            _self_test_replacement_patch(old_content, inline_content),
            str(root),
        )
        assert inline_patch_bad and "inline" in " ".join(inline_patch_bad[0][1])

        transitioned = old_content.replace('"status": "proposed"', '"status": "applied"')
        assert worklist_state_changes(old_content, transitioned) == (
            [],
            [("existing", "proposed", "applied")],
        )

        removed = json.dumps({"description": "", "items": []}, indent=2) + "\n"
        assert worklist_state_changes(old_content, removed) == (
            [("existing", "proposed")],
            [],
        )

        # Opt-out parity (#186): matching phrases write a fresh
        # kind:"direct-edit" record at AUTH_REL, others don't.
        assert has_opt_out("just do it")
        assert has_opt_out("Skip the worklist")
        assert has_opt_out("commit this directly")
        assert not has_opt_out("looks good")
        assert not has_opt_out("go ahead and merge")
        assert not has_opt_out("")

        write_direct_edit_record(str(root))
        wrote = json.loads((root / AUTH_REL).read_text())
        assert wrote["kind"] == "direct-edit"
        assert wrote["paths"] == ["*"]
        assert isinstance(wrote["issued_at_ms"], int)
        assert wrote["issued_at_ms"] > 0


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
    for label, violations in bad:
        lines.append(f"  - item {label}: {', '.join(violations)}")
    detail = "\n".join(lines)
    return (
        f"{tool_name} blocked: proposed worklist item(s) violate draft-only "
        f"rule.\n{detail}\n"
        f"Required for every proposed item: \"id\" (kebab-case identifier) and "
        f"\"file\" (or \"files\" array for multi-file items). Prose must live "
        f"in resources/worklist-drafts/<id>.md (Markdown with `# Before` and "
        f"`# After` sections); inline \"before\"/\"after\" keys in "
        f"worklist.json are rejected. Write the draft file first, then add "
        f"the metadata-only item to worklist.json."
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
        "curl -4 -sS -X POST -d '{\"op\":\"advance\",\"ids\":[\"item-id\"],\"status\":\"applied\"}' "
        "http://127.0.0.1:$(cat resources/.bram-port)/__worklist/mutate"
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
# The full conventions live in .claude/bram-conventions.md (or the
# legacy .claude/xmlui-desktop-conventions.md if Setup has not yet
# migrated this project), which codex reads on first prompt; this stays
# short to avoid drowning the model in repeated boilerplate.
GATE_REMINDER = (
    "bram worklist gate. First response to a change request must be "
    "(a) clarify, (b) propose items via resources/worklist-drafts/<id>.md "
    "plus resources/worklist.json metadata (or inline before/after), or "
    "(c) read-only investigation prefaced "
    "\"I don't yet have enough context to propose\". Mutations outside approved "
    "items are blocked at runtime. Full convention: "
    ".claude/bram-conventions.md (legacy: .claude/xmlui-desktop-conventions.md)"
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


# Opt-out phrases that authorize a one-turn direct edit. Mirrors Claude's
# _OPT_OUT_PATTERNS verbatim so the user-facing contract is identical
# regardless of agent. Anything ambiguous ("looks good", "go ahead") is
# deliberately NOT here — matches the conventions' "Don't infer
# commit/drop/advance from feedback" rule.
_OPT_OUT_PATTERNS = [
    re.compile(r"\bjust do it\b", re.IGNORECASE),
    re.compile(r"\bcommit\s+(this|that|it)\s+directly\b", re.IGNORECASE),
    re.compile(r"\bcommit directly[,\.\s]+no worklist\b", re.IGNORECASE),
    re.compile(r"\bno worklist\s+(for\s+(this|that)|here)\b", re.IGNORECASE),
    re.compile(r"\bskip the worklist\b", re.IGNORECASE),
    re.compile(r"\binline (the )?fix\b", re.IGNORECASE),
    re.compile(r"\bdon'?t bother with the worklist\b", re.IGNORECASE),
]


def has_opt_out(msg):
    if not isinstance(msg, str):
        return False
    return any(rx.search(msg) for rx in _OPT_OUT_PATTERNS)


def write_direct_edit_record(cwd):
    """Write a fresh kind:'direct-edit' record so the next PreToolUse's
    fresh_bypass() returns True. Uses snake_case issued_at_ms to match
    Bram's host canon and Codex's fresh_bypass reader."""
    path = os.path.join(cwd, AUTH_REL)
    rec = {
        "kind": "direct-edit",
        "issued_at_ms": int(time.time() * 1000),
        "paths": ["*"],
    }
    directory = os.path.dirname(path) or "."
    fd, tmp = tempfile.mkstemp(prefix=".auth-", dir=directory)
    try:
        with os.fdopen(fd, "w") as f:
            json.dump(rec, f)
        os.replace(tmp, path)
    except Exception:
        try:
            os.unlink(tmp)
        except Exception:
            pass


def handle_user_prompt_submit(payload, cwd):
    # Only inject in Bram-managed repos (presence of auth file).
    if not os.path.exists(os.path.join(cwd, AUTH_REL)):
        allow()
    prompt = payload.get("prompt", "")
    if has_opt_out(prompt):
        write_direct_edit_record(cwd)
        allow()
    if not looks_like_change_request(prompt):
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
        targets = patch_targets(tool_input) if isinstance(tool_input, dict) else set()
        if targets:
            first = next(iter(targets))
            t0 = normalize_target(cwd, first) or first
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
            new_content = _worklist_content_after_apply_patch(cwd, patch_body)
            if new_content is not None:
                bad_ids = _worklist_items_violating_draft_only(new_content, cwd)
                if bad_ids:
                    deny(_worklist_validation_error(bad_ids, "apply_patch"))
                removed, status_changed = worklist_state_changes(
                    current_worklist_text(cwd),
                    new_content,
                )
                if removed or status_changed:
                    deny(_mechanical_worklist_change_error(removed, status_changed, "apply_patch"))
                # Optimistic-concurrency check on the version field.
                old_text = current_worklist_text(cwd)
                old_has_version, old_version = worklist_version_from_text(old_text)
                new_has_version, new_version = worklist_version_from_text(new_content)
                if old_has_version and (
                    not new_has_version or new_version != old_version + 1
                ):
                    deny(_stale_worklist_version_error(
                        old_version, new_version, new_has_version, "apply_patch"
                    ))
            else:
                removed = _patch_removes_worklist_items(cwd, patch_body)
                if removed:
                    deny(_mechanical_worklist_change_error(removed, [], "apply_patch"))
                bad_ids = _patch_adds_violating_draft_only(patch_body, cwd)
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
            if is_worklist_draft(rel):
                continue  # draft prose files are proposal-authoring inputs
            if is_coordination_file(rel):
                continue  # Codex lifecycle channel (#130)
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
        # Issue #176: gh --body @ antipattern check runs before bash_writes,
        # so it catches read-only-looking gh commands that would silently
        # mangle a comment.
        if is_gh_body_at_antipattern(cmd):
            m = _GH_BODY_AT_RX.search(cmd or "")
            matched = m.group(0).strip() if m else ""
            _trace_hook(
                "PreToolUse",
                "Bash",
                (cmd or "")[:200],
                "deny",
                "gh-body-at-antipattern",
                cwd,
            )
            deny(
                "gh --body takes a literal string, not stdin or a file reference.\n"
                "Use --body-file - (stdin) or --body-file <path> instead.\n"
                f"Detected: {matched}"
            )
        if not bash_writes(cmd):
            allow()
        if "resources/worklist-drafts/" in (cmd or ""):
            allow()
        if ".worklist-intent.json" in (cmd or ""):
            allow()  # Codex lifecycle channel (#130)
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
                f"codex-worklist-guard.py to recognize this MCP tool's input shape."
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
                bad_ids = _worklist_items_violating_draft_only(new_content, cwd)
                if bad_ids:
                    deny(_worklist_validation_error(bad_ids, tool_name))
                removed, status_changed = worklist_state_changes(
                    current_worklist_text(cwd),
                    new_content,
                )
                if removed or status_changed:
                    deny(_mechanical_worklist_change_error(removed, status_changed, tool_name))
                # Optimistic-concurrency check on the version field
                # (parallel to the apply_patch branch above).
                old_text = current_worklist_text(cwd)
                old_has_version, old_version = worklist_version_from_text(old_text)
                new_has_version, new_version = worklist_version_from_text(new_content)
                if old_has_version and (
                    not new_has_version or new_version != old_version + 1
                ):
                    deny(_stale_worklist_version_error(
                        old_version, new_version, new_has_version, tool_name
                    ))
        violations = []
        for t in candidate_paths:
            rel = normalize_target(cwd, t)
            if rel is None:
                continue  # outside the project tree
            if rel == WORKLIST_REL:
                continue
            if is_worklist_draft(rel):
                continue
            if is_coordination_file(rel):
                continue  # Codex lifecycle channel (#130)
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
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        self_test()
        print("codex-worklist-guard self-test passed")
        sys.exit(0)
    main()
