#!/usr/bin/env python3
"""Summarize Bram PTY menu detection traces.

Reads [pty-menu-scan] lines from bram-trace.log, classifies detected
menus and high-signal skips against the current catalog anchors, and
optionally writes missed/unknown excerpts as specimen drafts.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import bram_state  # noqa: E402

DEFAULT_TRACE = bram_state.trace_log(ROOT)
SPECIMEN_DIR = ROOT / "docs" / "pty-menu-specimens"


@dataclass(frozen=True)
class Shape:
    shape_id: str
    family: str
    anchors: tuple[str, ...]


SHAPES = (
    Shape("edit", "claude-tool-permission", ("do you want to make this edit to",)),
    Shape("create", "claude-tool-permission", ("do you want to create",)),
    Shape("overwrite", "claude-tool-permission", ("do you want to overwrite",)),
    Shape("notebook-insert", "claude-tool-permission", ("do you want to insert this cell into",)),
    Shape("notebook-delete", "claude-tool-permission", ("do you want to delete this cell from",)),
    Shape("proceed", "claude-tool-permission", ("do you want to proceed?",)),
    Shape("connection", "claude-tool-permission", ("do you want to allow this connection?",)),
    Shape("api-key", "claude-tool-permission", ("do you want to use this api key?",)),
    Shape("skill", "claude-tool-permission", ("use skill", "from this skill")),
    Shape("codex-action-required", "codex-tool-permission", ("action required",)),
    Shape("askuserquestion", "claude-question", ("type something", "chat about this")),
)


SCAN_RE = re.compile(r"\[pty-menu-scan\]\s+(?P<body>.*)$")
FIELD_RE = re.compile(r"(\w+)=('(?:[^']|’)*'|\S+)")


def parse_fields(body: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for key, value in FIELD_RE.findall(body):
        if value.startswith("'") and value.endswith("'"):
            value = value[1:-1]
        fields[key] = value
    return fields


def parse_scan_lines(trace_path: Path) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    with trace_path.open("r", encoding="utf-8", errors="replace") as f:
        for line_no, line in enumerate(f, 1):
            match = SCAN_RE.search(line)
            if not match:
                continue
            fields = parse_fields(match.group("body"))
            fields["line"] = str(line_no)
            rows.append(fields)
    return rows


def classify_excerpt(excerpt: str) -> str:
    lower = excerpt.lower()
    for shape in SHAPES:
        if any(anchor in lower for anchor in shape.anchors):
            return shape.shape_id
    return "unknown"


def compact_excerpt(excerpt: str, limit: int = 140) -> str:
    text = " ".join(excerpt.split())
    if len(text) <= limit:
        return text
    return text[: limit - 1].rstrip() + "..."


def truthy(row: dict[str, str], key: str) -> bool:
    return row.get(key) == "true"


def looks_like_menu_candidate(row: dict[str, str]) -> bool:
    return (
        truthy(row, "cursor")
        or truthy(row, "numbered")
        or truthy(row, "header")
        or truthy(row, "codex_action")
        or truthy(row, "needle2_after_anchor")
    )


def collect(rows: list[dict[str, str]]) -> tuple[Counter[str], Counter[str], Counter[str], dict[str, list[dict[str, str]]], int, int]:
    covered: Counter[str] = Counter()
    missed: Counter[str] = Counter()
    unknown: Counter[str] = Counter()
    examples: dict[str, list[dict[str, str]]] = defaultdict(list)
    noisy_skips = 0
    weak_menu_bearing_skips = 0

    for row in rows:
        op = row.get("op", "")
        menu_bearing = row.get("menu_bearing") == "true"
        candidate = looks_like_menu_candidate(row)
        excerpt = row.get("excerpt", "")
        shape = classify_excerpt(excerpt)
        row["shape"] = shape

        if op == "fire":
            covered[shape] += 1
            if len(examples[f"covered:{shape}"]) < 3:
                examples[f"covered:{shape}"].append(row)
        elif op == "skip" and menu_bearing and candidate:
            if shape == "unknown":
                unknown[shape] += 1
                key = "unknown:unknown"
            else:
                missed[shape] += 1
                key = f"missed:{shape}"
            if len(examples[key]) < 5:
                examples[key].append(row)
        elif op == "skip" and menu_bearing:
            weak_menu_bearing_skips += 1
        elif op == "skip":
            noisy_skips += 1

    return covered, missed, unknown, examples, noisy_skips, weak_menu_bearing_skips


def print_counter(title: str, counter: Counter[str], examples: dict[str, list[dict[str, str]]], prefix: str) -> None:
    print(f"\n{title}")
    if not counter:
        print("  none")
        return
    for shape, count in counter.most_common():
        print(f"  {shape}: {count}")
        for row in examples.get(f"{prefix}:{shape}", [])[:2]:
            print(f"    line {row.get('line')}: {compact_excerpt(row.get('excerpt', ''))}")


def specimen_slug(row: dict[str, str], index: int) -> str:
    today = dt.date.today().isoformat()
    shape = row.get("shape") or "unknown"
    return f"{today}-trace-{shape}-{index:02d}.md"


def write_specimens(rows: list[dict[str, str]], specimen_dir: Path, limit: int) -> list[Path]:
    specimen_dir.mkdir(parents=True, exist_ok=True)
    candidates = [
        r
        for r in rows
        if r.get("op") == "skip"
        and r.get("menu_bearing") == "true"
        and looks_like_menu_candidate(r)
        and r.get("excerpt")
    ]
    written: list[Path] = []
    for index, row in enumerate(candidates[:limit], 1):
        path = specimen_dir / specimen_slug(row, index)
        content = (
            "---\n"
            f"observed: {dt.date.today().isoformat()}\n"
            "provider: unknown\n"
            "cli_version: unknown\n"
            f"shape: {row.get('shape', 'unknown')}\n"
            "source: trace-excerpt\n"
            f"trace_line: {row.get('line')}\n"
            "---\n\n"
            "## Menu text\n\n"
            f"{row.get('excerpt', '')}\n\n"
            "## Notes\n\n"
            "- Filed automatically from `scripts/pty-menu-scan-report.py --write-specimens`.\n"
            "- Confirm provider, CLI version, and catalog classification before committing.\n"
        )
        path.write_text(content, encoding="utf-8")
        written.append(path)
    return written


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", nargs="?", default=str(DEFAULT_TRACE), help="trace log path")
    parser.add_argument("--write-specimens", action="store_true", help="write missed/unknown trace excerpts as specimen drafts")
    parser.add_argument("--specimen-limit", type=int, default=10, help="maximum specimen drafts to write")
    args = parser.parse_args(argv)

    trace_path = Path(args.trace)
    if not trace_path.exists():
        print(f"trace not found: {trace_path}", file=sys.stderr)
        return 2

    rows = parse_scan_lines(trace_path)
    covered, missed, unknown, examples, noisy_skips, weak_menu_bearing_skips = collect(rows)

    print(f"Trace: {trace_path}")
    print(f"PTY menu scan lines: {len(rows)}")
    print(f"Noisy non-menu skips: {noisy_skips}")
    print(f"Weak menu-bearing skips suppressed: {weak_menu_bearing_skips}")
    print_counter("Covered catalog shapes (op=fire)", covered, examples, "covered")
    print_counter("Missed catalog shapes (op=skip menu_bearing=true)", missed, examples, "missed")
    print_counter("Unknown menu-bearing skips", unknown, examples, "unknown")

    if args.write_specimens:
        written = write_specimens(rows, SPECIMEN_DIR, args.specimen_limit)
        print(f"\nSpecimen drafts written: {len(written)}")
        for path in written:
            print(f"  {path.relative_to(ROOT)}")

    if missed or unknown:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
