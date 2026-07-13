#!/usr/bin/env python3
"""PreToolUse hook: enforce the worklist flow for Write/Edit on this project.

Two responsibilities:

1. Writes/Edits to resources/worklist.json — validate that any removed item
   was authorized. status="applied" items can be pruned freely (commit-then-
   prune); status="proposed" items can only be pruned when the last user
   message was `drop: {"ids":[...]}` listing that id.

2. Writes/Edits to any OTHER file in the project — require the target file
   to be covered by a proposed/applied item in resources/worklist.json, OR
   a fresh direct-edit bypass record in resources/.worklist-authorization.json,
   OR an explicit opt-out phrase in the last user message ("just do it",
   "commit directly, no worklist", "inline the fix", "skip the worklist",
   "no worklist for this/that"). Mirrors the coverage check the codex-side
   hook (app/shell/worklist-guard-codex.py) does for apply_patch.

If the project lacks resources/.worklist-authorization.json (the managed-repo
marker Bram Setup writes), the hook exits 0 (allow) — Claude
sessions in unmanaged repos run as if no hook were installed.
"""

import json
import os
import re
import sys
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
    _post_hook_trace("worklist-guard.py", event, tool, target, decision, reason, cwd)


# Opt-out phrases that authorize a one-turn direct edit. Matched
# case-insensitively against the last user message. Kept narrow and
# explicit — anything ambiguous ("looks good", "go ahead") is NOT here,
# matching the conventions' "Don't infer commit/drop/advance from feedback"
# rule. Each pattern requires the user to type something obviously about
# bypassing the worklist; passive approval doesn't count.
_OPT_OUT_PATTERNS = [
    re.compile(r"\bjust do it\b", re.IGNORECASE),
    re.compile(r"\bcommit\s+(this|that|it)\s+directly\b", re.IGNORECASE),
    re.compile(r"\bcommit directly[,\.\s]+no worklist\b", re.IGNORECASE),
    re.compile(r"\bno worklist\s+(for\s+(this|that)|here)\b", re.IGNORECASE),
    re.compile(r"\bskip the worklist\b", re.IGNORECASE),
    re.compile(r"\binline (the )?fix\b", re.IGNORECASE),
    re.compile(r"\bdon'?t bother with the worklist\b", re.IGNORECASE),
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
    with open(transcript_path) as f:
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
        with open(os.path.join(project_root, WORKLIST_REL)) as f:
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
        with open(os.path.join(project_root, AUTH_REL)) as f:
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
        f"  - Opt-out phrases the user can type to authorize a direct edit: "
        f"\"just do it\", \"commit this directly\", \"no worklist for this\", "
        f"\"skip the worklist\", \"inline the fix\"."
    )
    if opt_out_attempted:
        msg += (
            "\n  - (Detected what looked like opt-out language, but it didn't "
            "match the expected phrasing. Be explicit.)"
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
        "or applied items covering active work. Propose the work in the "
        "worklist first, or have the user issue a direct-edit authorization.",
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
        "rg Bash app/__shell/worklist-guard.py",
        "git status --short",
        "gh issue view 119 --repo judell/bram",
        "python --version",
        "curl -I https://example.com",
    ]
    for command in write_commands:
        assert bash_writes(command), command
    for command in read_commands:
        assert not bash_writes(command), command


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
        deny_bash_coverage(command, cwd)

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
        with open(fp) as f:
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
    last_msg = last_user_text(payload.get("transcript_path", ""))
    if has_opt_out(last_msg):
        _trace_hook("PreToolUse", tool_name, rel, "allow", "opt-out-phrase")
        sys.exit(0)
    deny_coverage(rel, opt_out_attempted=("worklist" in (last_msg or "").lower()
                                          and "no" in (last_msg or "").lower()))


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        self_test()
        print("worklist-guard self-test passed")
        sys.exit(0)
    main()
