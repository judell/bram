#!/usr/bin/env python3
"""Observe-only Codex PermissionRequest payload capture for Bram.

Temporary characterization hook for Codex approval UI parity. It never
approves, denies, or modifies the request; it only records the hook payload
shape so Bram can learn whether Codex PermissionRequest carries enough
structured data to build hook-primary permission menus.
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parent))
import bram_state  # noqa: E402

# Written in the state folder (`.bram/` or `resources/`).
CAPTURE_NAME = "codex-permission-hook-capture.jsonl"


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def compact_shape(value):
    if isinstance(value, dict):
        return {
            key: compact_shape(value[key])
            for key in sorted(value.keys())
        }
    if isinstance(value, list):
        if not value:
            return []
        return {
            "type": "list",
            "len": len(value),
            "first": compact_shape(value[0]),
        }
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


def write_jsonl(path: Path, record: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as f:
        f.write(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n")


def main() -> int:
    raw = sys.stdin.read()
    try:
        payload = json.loads(raw or "{}")
    except Exception as e:
        payload = {"_parse_error": str(e), "_raw_preview": raw[:1000]}

    cwd = Path(payload.get("cwd") or os.getcwd())
    record = {
        "at": utc_now(),
        "cwd": str(cwd),
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

    if bram_state.state_dir(cwd).is_dir():
        write_jsonl(bram_state.state_dir(cwd) / CAPTURE_NAME, record)

    trace_log = os.environ.get("BRAM_TRACE_LOG")
    if os.environ.get("BRAM_TRACE") == "1" and trace_log:
        line = (
            f"[{record['at']}] [hook] script=codex-permission-capture.py "
            f"event={record.get('hook_event_name') or ''} "
            f"tool={record.get('tool_name') or ''} "
            f"cwd={cwd} captured=true"
        )
        try:
            with open(trace_log, "a", encoding="utf-8") as f:
                f.write(line + "\n")
        except Exception:
            pass
    return 0


try:
    raise SystemExit(main())
except Exception:
    # Observe-only: never block Codex approval flow.
    raise SystemExit(0)
