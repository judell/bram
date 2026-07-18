#!/usr/bin/env python3
# Bram permission-menu surfacing hook (canonical source).
#
# Fires as a Claude Code PreToolUse/PermissionRequest/PostToolUse hook and
# POSTs the structured menu to Bram's loopback so the agent pane can render
# permission menus from data instead of screen-scraping the xterm grid.
#
# - PermissionRequest: a permission dialog is about to show -> POST /__menu/permission
#   with {tool_name, tool_input, permission_suggestions}. (Fires ONLY when a
#   dialog will appear, so it is a false-positive-free "a menu is up" signal.)
# - PreToolUse (AskUserQuestion only): Family B has no PermissionRequest; its
#   choices live in tool_input.questions -> POST /__menu/permission the same way
#   (the host builds the menu from tool_input, not permission_suggestions).
# - PostToolUse / PermissionDenied: the prompt was answered (tool ran) or
#   declined (No/Esc) -> POST /__menu/permission/clear so it doesn't linger.
#   A declined prompt fires PermissionDenied, NOT PostToolUse, so both clear.
#
# OBSERVE-ONLY: never returns an allow/deny/ask decision; the user answers in
# the terminal or via the pane (which injects keystrokes). Fully defensive and
# fast: short timeout, fire-and-forget, ALWAYS exits 0 so it can never block or
# delay the prompt.
#
# Installed copy lives at .claude/hooks/permission-menu-hook.py and is refreshed
# from this canonical source by Setup / build.rs. Do not edit the installed copy.
import sys, os, json, urllib.request

PORT_REL = os.path.join("resources", ".bram-port")


def _project_root(payload):
    # CLAUDE_PROJECT_DIR first: it is pinned to the session's project root.
    # The payload cwd tracks the shell's persistent `cd`, so a session
    # working outside the repo (e.g. a scratchpad) resolved a root with no
    # resources/.bram-port and every POST — claims AND clears — silently
    # vanished. A stale claim then muzzled grid menus for its 300s timeout
    # (2026-07-18 pane-blind windows; hook-claim-stale-grid-defer).
    return os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()


def _port(root):
    try:
        with open(os.path.join(root, PORT_REL)) as f:
            return int(f.read().strip())
    except Exception:
        return None


def _post(port, path, body):
    try:
        # Echo the per-session token Bram set in this agent's env so the route
        # can tell our POSTs from a foreign agent's. Absent (agent not launched
        # by Bram) -> route rejects, which is the intended behavior.
        token = os.environ.get("BRAM_MENU_TOKEN")
        if token:
            body = dict(body)
            body["bram_token"] = token
        data = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            "http://127.0.0.1:%d%s" % (port, path),
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        # Short timeout: the hook is synchronous before the prompt renders, so
        # it must not stall. Fire-and-forget; ignore the response.
        urllib.request.urlopen(req, timeout=0.4).read()
    except Exception:
        pass


def main():
    try:
        payload = json.loads(sys.stdin.read() or "{}")
    except Exception:
        payload = {}
    event = payload.get("hook_event_name") or ""
    root = _project_root(payload)
    port = _port(root)
    if not port:
        return
    if event == "PermissionRequest":
        _post(port, "/__menu/permission", {
            "tool_name": payload.get("tool_name"),
            "tool_input": payload.get("tool_input") or {},
            "permission_suggestions": payload.get("permission_suggestions") or [],
            "tool_use_id": payload.get("tool_use_id"),
        })
    elif event == "PreToolUse" and payload.get("tool_name") == "AskUserQuestion":
        # Family B: no PermissionRequest fires; choices live in tool_input.
        # questions. Surface the same way (host builds the menu from tool_input).
        _post(port, "/__menu/permission", {
            "tool_name": payload.get("tool_name"),
            "tool_input": payload.get("tool_input") or {},
            "permission_suggestions": [],
            "tool_use_id": payload.get("tool_use_id"),
        })
    elif event in ("PostToolUse", "PermissionDenied"):
        # Answered (PostToolUse) or declined via No/Esc (PermissionDenied) —
        # either way the prompt is resolved, so clear any surfaced menu.
        # tool_name + tool_input let the host key the removal by signature:
        # PermissionRequest claims carry no tool_use_id, so id-only clears
        # matched nothing (parallel-menu-claim-queue soak, 2026-07-18).
        _post(port, "/__menu/permission/clear", {
            "tool_use_id": payload.get("tool_use_id"),
            "tool_name": payload.get("tool_name"),
            "tool_input": payload.get("tool_input") or {},
        })


try:
    main()
except Exception:
    pass
sys.exit(0)
