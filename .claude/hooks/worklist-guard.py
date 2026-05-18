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
marker xmlui-desktop's Setup writes), the hook exits 0 (allow) — Claude
sessions in unmanaged repos run as if no hook were installed.
"""

import json
import os
import re
import sys
import time


WORKLIST_REL = "resources/worklist.json"
AUTH_REL = "resources/.worklist-authorization.json"
BYPASS_TTL_SECONDS = 60 * 60  # direct-edit auth records are fresh for 1h


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


def parse_auth(msg):
    """Return (kind, ids) for `approved:` or `drop:` prefixed messages."""
    msg = msg.strip()
    for prefix, kind in (("approved:", "approved"), ("drop:", "drop")):
        if msg.startswith(prefix):
            try:
                data = json.loads(msg[len(prefix):].strip())
            except Exception:
                return kind, set()
            items = data.get("items")
            if isinstance(items, list):
                return kind, {
                    it.get("id") for it in items
                    if isinstance(it, dict) and it.get("id")
                }
            # Legacy fallback: drop: {"ids":[...]}
            return kind, set(data.get("ids", []))
    return None, set()


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
    print(msg, file=sys.stderr)
    sys.exit(2)


def main():
    payload = json.load(sys.stdin)
    if payload.get("tool_name") not in ("Write", "Edit"):
        sys.exit(0)

    ti = payload.get("tool_input", {})
    fp = ti.get("file_path", "")
    if not isinstance(fp, str) or not fp:
        sys.exit(0)

    # Locate the project root via the managed-repo marker. If the file isn't
    # inside an xmlui-desktop-managed project at all, exit cleanly — this
    # hook is a no-op for Claude sessions in unmanaged repos.
    project_root = find_project_root(os.path.dirname(fp) or ".")
    if project_root is None:
        sys.exit(0)

    rel = normalize_target(project_root, fp)
    if rel is None:
        # Target is outside the project tree (e.g., editing files in
        # ~/.codex/ or /tmp/). The worklist gate doesn't apply.
        sys.exit(0)

    # Branch 1: writes to resources/worklist.json — existing prune validation.
    if rel == WORKLIST_REL:
        if not os.path.exists(fp):
            sys.exit(0)
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
        removed = set(old_items) - set(new_items)
        if not removed:
            sys.exit(0)
        kind, ids = parse_auth(last_user_text(payload.get("transcript_path", "")))
        violations = []
        for rid in removed:
            st = old_items[rid].get("status", "proposed")
            if st == "applied":
                continue
            if kind == "drop" and rid in ids:
                continue
            violations.append((rid, st))
        if violations:
            bad = ", ".join(f'"{r}" (status={s})' for r, s in violations)
            print(
                f"Blocked: removing {bad} from resources/worklist.json without "
                f"going through the two-stage flow.\n"
                f"  - 'proposed' must transition to 'applied' (re-add the item) "
                f"before pruning.\n"
                f"  - Direct removal is allowed only when the user's last "
                f"message was `drop: {{\"items\":[{{\"id\":\"...\"}}]}}`.\n"
                f"Last user authorization kind: {kind or 'none'}.",
                file=sys.stderr,
            )
            sys.exit(2)
        sys.exit(0)

    # Branch 2: writes to any other project file — require worklist coverage,
    # fresh bypass, or explicit opt-out language in the last user message.
    covered = worklist_covered_files(project_root)
    if rel in covered:
        sys.exit(0)
    if fresh_bypass(project_root, rel):
        sys.exit(0)
    last_msg = last_user_text(payload.get("transcript_path", ""))
    if has_opt_out(last_msg):
        sys.exit(0)
    deny_coverage(rel, opt_out_attempted=("worklist" in (last_msg or "").lower()
                                          and "no" in (last_msg or "").lower()))


if __name__ == "__main__":
    main()
