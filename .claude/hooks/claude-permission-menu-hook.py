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
# Installed copy lives at .claude/hooks/claude-permission-menu-hook.py and is refreshed
# from this canonical source by Setup / build.rs. Do not edit the installed copy.
import sys, os, json, time, urllib.request

PORT_REL = os.path.join("resources", ".bram-port")

# Observe-only event breadcrumb (command-substitution-menu-shape): one line
# per hook invocation so a pane miss self-classifies — a breadcrumb with no
# host-side op=permission means the POST was lost; no breadcrumb means
# Claude Code fired no event and the grid path is the designed fallback.
# Fail-silent; capped so it can't grow unbounded.
_EVENTS_LOG_CAP_BYTES = 5 * 1024 * 1024


def _breadcrumb(root, event, tool, extra=""):
    # `extra` (menu-no-claim-signature-gap phase 1): POST outcome suffix —
    # "post=<path> ok ms=N" / "post=<path> err=<ExcClass> ms=N" /
    # "post=skipped port=none" — so a fired-but-claimless prompt
    # self-classifies (lost in transit vs host-side) instead of conflating
    # with "hook never ran". One extra line per POSTing invocation.
    try:
        d = os.path.join(root, "resources", "bram-traces")
        if not os.path.isdir(os.path.join(root, "resources")):
            return
        os.makedirs(d, exist_ok=True)
        path = os.path.join(d, "hook-events.log")
        try:
            if os.path.getsize(path) > _EVENTS_LOG_CAP_BYTES:
                return
        except OSError:
            pass
        with open(path, "a") as f:
            f.write(
                "%s claude %s %s%s\n"
                % (
                    time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime())
                    + (".%03dZ" % (int(time.time() * 1000) % 1000)),
                    event or "?",
                    tool or "?",
                    (" " + extra) if extra else "",
                )
            )
    except Exception:
        pass


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
    # Returns an outcome string for the breadcrumb (phase-1 attribution:
    # 42/59 menus surfaced claimless on 2026-07-30 while breadcrumbs showed
    # the hook firing — the loss is in this POST or on the host, and
    # fail-silent meant the two were indistinguishable).
    started = time.time()
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
        # it must not stall. Fire-and-forget; ignore the response body.
        urllib.request.urlopen(req, timeout=0.4).read()
        return "post=%s ok ms=%d" % (path, int((time.time() - started) * 1000))
    except Exception as e:
        return "post=%s err=%s ms=%d" % (
            path,
            type(e).__name__,
            int((time.time() - started) * 1000),
        )


def main():
    try:
        payload = json.loads(sys.stdin.read() or "{}")
    except Exception:
        payload = {}
    event = payload.get("hook_event_name") or ""
    root = _project_root(payload)
    tool = payload.get("tool_name")
    _breadcrumb(root, event, tool)
    port = _port(root)
    if not port:
        if event in ("PermissionRequest", "PreToolUse", "PostToolUse", "PermissionDenied"):
            _breadcrumb(root, event, tool, "post=skipped port=none")
        return
    outcome = None
    if event == "PermissionRequest":
        outcome = _post(port, "/__menu/permission", {
            "tool_name": tool,
            "tool_input": payload.get("tool_input") or {},
            "permission_suggestions": payload.get("permission_suggestions") or [],
            "tool_use_id": payload.get("tool_use_id"),
        })
    elif event == "PreToolUse" and tool == "AskUserQuestion":
        # Family B: no PermissionRequest fires; choices live in tool_input.
        # questions. Surface the same way (host builds the menu from tool_input).
        outcome = _post(port, "/__menu/permission", {
            "tool_name": tool,
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
        outcome = _post(port, "/__menu/permission/clear", {
            "tool_use_id": payload.get("tool_use_id"),
            "tool_name": tool,
            "tool_input": payload.get("tool_input") or {},
        })
    if outcome:
        _breadcrumb(root, event, tool, outcome)


try:
    main()
except Exception:
    pass
sys.exit(0)
