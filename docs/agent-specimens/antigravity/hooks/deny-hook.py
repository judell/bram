#!/usr/bin/env python3
"""Record one PreToolUse payload and return a deterministic deny."""
import json
from pathlib import Path
import sys

payload = sys.stdin.read()
out = Path(__file__).resolve().parent / "payloads" / "PreToolUse-denied.jsonl"
out.parent.mkdir(parents=True, exist_ok=True)
with out.open("a", encoding="utf-8") as stream:
    stream.write(payload.rstrip("\n") + "\n")
print(json.dumps({"decision": "deny", "reason": "SPECIMEN-DENY-7731: blocked by the capture hook."}))
