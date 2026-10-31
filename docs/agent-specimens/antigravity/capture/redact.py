"""Render snapshots and redact a scratch capture before it is kept."""
import os
from pathlib import Path
import re
import sys

from render import render

pack = Path(__file__).resolve().parents[1]
home = os.environ.get("ANTIGRAVITY_SPECIMEN_HOME", str(Path.home()))
workspace = os.environ.get("ANTIGRAVITY_SPECIMEN_WORKSPACE", "")
workspace_home = workspace.replace(home, "~", 1) if workspace.startswith(home) else ""

replacements = [(workspace, b"<workspace>"), (workspace_home, b"<workspace>"), (home, b"~")]
replacements = [(old.encode(), new) for old, new in replacements if old]

def redact(data):
    for old, new in replacements:
        data = data.replace(old, new)
    return re.sub(rb"[\w.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", b"<user-email>", data)

for raw in sorted(pack.glob("*.raw")):
    if raw.name == "_all.raw":
        raw.unlink()
        continue
    data = redact(raw.read_bytes())
    raw.write_bytes(data)
    rendered = render(data)
    (pack / (raw.stem + ".txt")).write_bytes(redact(rendered.encode()))

for payload in pack.rglob("*.jsonl"):
    payload.write_bytes(redact(payload.read_bytes()))

