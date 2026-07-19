#!/usr/bin/env python3
"""Bram Codex permission-menu surfacing hook.

Observe-only: records Codex permission payload shape and notifies Bram's
existing permission-menu route. It never approves or denies a request.
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


PORT_REL = Path("resources/.bram-port")
CAPTURE_REL = Path("resources/codex-permission-hook-capture.jsonl")


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def compact_shape(value):
    if isinstance(value, dict):
        return {key: compact_shape(value[key]) for key in sorted(value.keys())}
    if isinstance(value, list):
        if not value:
            return []
        return {"type": "list", "len": len(value), "first": compact_shape(value[0])}
    if isinstance(value, str):
        return {
            "type": "str",
            "len": len(value),
            "preview": value[:240],
            "sha256": hashlib.sha256(value.encode("utf-8", errors="replace")).hexdigest()[:16],
        }
    if value is None:
        return None
    return type(value).__name__


def project_root(payload: dict) -> Path:
    return Path(payload.get("cwd") or os.getcwd())


def loopback_port(root: Path) -> int | None:
    try:
        return int((root / PORT_REL).read_text(encoding="utf-8").strip())
    except Exception:
        return None


def post(port: int, path: str, body: dict) -> None:
    try:
        # Echo the per-session token Bram set in this agent's env so the route
        # can distinguish our POSTs from a foreign agent's. Absent -> rejected.
        token = os.environ.get("BRAM_MENU_TOKEN")
        if token:
            body = dict(body)
            body["bram_token"] = token
        data = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            f"http://127.0.0.1:{port}{path}",
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req, timeout=0.4).read()
    except Exception:
        pass


def append_capture(root: Path, payload: dict) -> None:
    if not (root / "resources").is_dir():
        return
    record = {
        "at": utc_now(),
        "cwd": str(root),
        "hook_event_name": payload.get("hook_event_name"),
        "tool_name": payload.get("tool_name"),
        "tool_use_id": payload.get("tool_use_id"),
        "permission_mode": payload.get("permission_mode"),
        "model": payload.get("model"),
        "permission_mode_field_present": "permission_mode" in payload,
        "top_level_keys": sorted(payload.keys()),
        "tool_input_shape": compact_shape(payload.get("tool_input")),
        "payload_shape": compact_shape(payload),
    }
    try:
        capture = root / CAPTURE_REL
        capture.parent.mkdir(parents=True, exist_ok=True)
        with capture.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n")
    except Exception:
        pass


def main() -> int:
    try:
        payload = json.loads(sys.stdin.read() or "{}")
    except Exception:
        payload = {}

    root = project_root(payload)
    event = payload.get("hook_event_name") or ""
    tool_name = payload.get("tool_name")

    if event == "PermissionRequest":
        append_capture(root, payload)

    port = loopback_port(root)
    if not port:
        return 0

    body = {
        "provider": "codex",
        "hook_event_name": event,
        "tool_name": tool_name,
        "tool_input": payload.get("tool_input") or {},
        "permission_mode": payload.get("permission_mode"),
        "session_id": payload.get("session_id"),
        "turn_id": payload.get("turn_id"),
        "transcript_path": payload.get("transcript_path"),
    }
    if event == "PermissionRequest":
        post(port, "/__menu/permission", body)
    elif event == "PostToolUse":
        post(port, "/__menu/permission/clear", body)
    return 0


try:
    raise SystemExit(main())
except Exception:
    raise SystemExit(0)
