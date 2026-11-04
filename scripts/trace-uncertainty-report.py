#!/usr/bin/env python3
"""Report trace evidence for agent-side uncertainty / gear shifting.

This is a heuristic reporter for Bram traces. It groups Bash tool
commands by user turn, then scores each turn for agent-side exploration:

- many broad searches/reads
- many command categories in one turn
- repeated searches with different terms
- tool-choice probes (`command -v ...`)
- read-only command bursts with no obvious edit/check phase
- category switching (`search -> read -> search -> script`, etc.)

It does not infer intent. It surfaces turns where the tool sequence looks
like the agent was building or changing its model of the situation.

Usage:
  scripts/trace-uncertainty-report.py [trace-file-or-glob ...] [--window-seconds N]
"""

from __future__ import annotations

import argparse
import glob
import json
import re
import sys
from collections import Counter
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parent))
import bram_state  # noqa: E402

# The state folder (`.bram/` or `resources/`) under the current directory.
DEFAULT_TRACE = bram_state.trace_log(Path("."))

ROUTE_RE = re.compile(r"^\[(?P<ts>[^]]+)\] \[(?P<cat>[^]]+)\] (?P<body>.*)$")
HOOK_TARGET_RE = re.compile(r"tool=Bash target=(?P<target>.*?) cwd=")


@dataclass
class Command:
    ts: datetime
    line_no: int
    target: str

    @property
    def category(self) -> str:
        return command_category(self.target)


@dataclass
class UserTurn:
    trace: Path
    ts: datetime
    line_no: int
    preview: str
    commands: list[Command] = field(default_factory=list)

    @property
    def turn_type(self) -> str:
        preview = self.preview.lstrip()
        if preview.startswith("approved:"):
            return "approved"
        if preview.startswith("iterate:"):
            return "iterate"
        if preview.startswith("drop:"):
            return "drop"
        if preview.startswith("skip-worklist:"):
            return "skip-worklist"
        return "conversation"

    def broad_commands(self) -> list[Command]:
        return [cmd for cmd in self.commands if is_broad_command(cmd.target)]

    def categories(self) -> list[str]:
        return [cmd.category for cmd in self.commands]

    def category_switches(self) -> int:
        cats = self.categories()
        return sum(1 for a, b in zip(cats, cats[1:]) if a != b)

    def repeated_searches(self) -> int:
        search_like = [
            normalize_search_target(cmd.target)
            for cmd in self.commands
            if cmd.category in {"search", "find", "git-list"}
        ]
        return max(0, len(search_like) - len(set(search_like)))

    def exploratory_tool_checks(self) -> int:
        return sum(1 for cmd in self.commands if cmd.category == "tool-check")

    def has_edit_or_check(self) -> bool:
        return any(cmd.category in {"edit", "build-check", "test"} for cmd in self.commands)

    def uncertainty_score(self) -> int:
        broad = len(self.broad_commands())
        distinct_categories = len(set(self.categories()))
        score = 0
        score += broad * 2
        score += max(0, distinct_categories - 2) * 2
        score += self.category_switches()
        score += self.repeated_searches() * 2
        score += self.exploratory_tool_checks() * 3
        if len(self.commands) >= 4 and not self.has_edit_or_check():
            score += 4
        return score


def parse_ts(raw: str) -> datetime | None:
    try:
        return datetime.fromisoformat(raw.replace("Z", "+00:00")).astimezone(timezone.utc)
    except ValueError:
        return None


def parse_iframe_json(body: str) -> dict | None:
    idx = body.find("{")
    if idx < 0:
        return None
    try:
        return json.loads(body[idx:])
    except json.JSONDecodeError:
        return None


def command_category(target: str) -> str:
    s = target.strip()
    lower = s.lower()
    if lower.startswith("rg "):
        return "search"
    if lower.startswith("find "):
        return "find"
    if lower.startswith("sed -n") or lower.startswith("cat ") or lower.startswith("nl "):
        return "read"
    if lower.startswith("git ls-files"):
        return "git-list"
    if lower.startswith("git status") or lower.startswith("git diff") or lower.startswith("git log"):
        return "git-inspect"
    if lower.startswith("command -v ") or lower.startswith("which "):
        return "tool-check"
    if lower.startswith("python3 - <<") or lower.startswith("python3 -c"):
        return "script"
    if lower.startswith("wc -l"):
        return "count"
    if lower.startswith("cargo check") or lower.startswith("npm run") or lower.startswith("pnpm ") or lower.startswith("yarn "):
        return "build-check"
    if lower.startswith("cargo test") or "pytest" in lower or lower.startswith("npm test"):
        return "test"
    if lower.startswith("chmod ") or lower.startswith("apply_patch") or lower.startswith("mv ") or lower.startswith("cp "):
        return "edit"
    return "other"


def is_broad_command(target: str) -> bool:
    return command_category(target) in {
        "search",
        "find",
        "read",
        "git-list",
        "script",
        "count",
        "tool-check",
    }


def normalize_search_target(target: str) -> str:
    s = target.strip()
    # Keep enough shape to distinguish repeated exact searches, but drop
    # common output caps that don't change the question.
    s = re.sub(r"\s+--max-count\s+\d+", "", s)
    s = re.sub(r"\s+--max_output_tokens[= ]\d+", "", s)
    return s[:200]


def load_trace(path: Path) -> tuple[list[UserTurn], list[Command]]:
    turns: list[UserTurn] = []
    commands: list[Command] = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        m = ROUTE_RE.match(line)
        if not m:
            continue
        ts = parse_ts(m.group("ts"))
        if ts is None:
            continue
        cat = m.group("cat")
        body = m.group("body")
        if cat == "iframe" and "subkind=to-turn" in body and '"stage":"source"' in body:
            payload = parse_iframe_json(body)
            if not payload:
                continue
            turns.append(
                UserTurn(
                    trace=path,
                    ts=ts,
                    line_no=line_no,
                    preview=str(payload.get("textPreview") or ""),
                )
            )
        elif cat == "hook" and "tool=Bash" in body:
            hm = HOOK_TARGET_RE.search(body)
            if hm:
                commands.append(Command(ts=ts, line_no=line_no, target=hm.group("target")))
    return turns, commands


def attach_commands(turns: list[UserTurn], commands: list[Command], window_seconds: int) -> None:
    for idx, turn in enumerate(turns):
        next_turn_ts = turns[idx + 1].ts if idx + 1 < len(turns) else None
        for cmd in commands:
            delta = (cmd.ts - turn.ts).total_seconds()
            before_next_turn = next_turn_ts is None or cmd.ts < next_turn_ts
            if before_next_turn and 0 <= delta <= window_seconds:
                turn.commands.append(cmd)


def expand_trace_paths(patterns: list[str]) -> tuple[list[Path], list[Path]]:
    trace_paths: list[Path] = []
    for pattern in patterns:
        matches = [Path(p) for p in glob.glob(pattern)]
        trace_paths.extend(matches or [Path(pattern)])
    trace_paths = sorted(set(trace_paths))
    missing = [p for p in trace_paths if not p.is_file()]
    return [p for p in trace_paths if p.is_file()], missing


def print_turn(turn: UserTurn) -> None:
    cats = Counter(turn.categories())
    broad = turn.broad_commands()
    print(
        f"- {turn.trace.name}:{turn.line_no} {turn.ts.isoformat()} "
        f"type={turn.turn_type} score={turn.uncertainty_score()} commands={len(turn.commands)} "
        f"broad={len(broad)} categories={dict(cats)} switches={turn.category_switches()}"
    )
    print(f"  preview: {turn.preview}")
    for cmd in broad[:10]:
        print(f"    {cmd.category:11s} line {cmd.line_no}: {cmd.target[:150]}")
    if len(broad) > 10:
        print(f"    ... {len(broad) - 10} more broad commands")
    print()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("traces", nargs="*", default=[str(DEFAULT_TRACE)], help="Trace files or glob patterns")
    parser.add_argument("--window-seconds", type=int, default=180)
    parser.add_argument("--top", type=int, default=25, help="Maximum turns to print")
    parser.add_argument("--min-score", type=int, default=8, help="Minimum agent-side uncertainty score to list")
    args = parser.parse_args()

    trace_paths, missing = expand_trace_paths(args.traces)

    all_turns: list[UserTurn] = []
    total_commands = 0
    all_category_counts: Counter[str] = Counter()
    for trace in trace_paths:
        turns, commands = load_trace(trace)
        attach_commands(turns, commands, args.window_seconds)
        all_turns.extend(turns)
        total_commands += len(commands)
        all_category_counts.update(cmd.category for cmd in commands)

    turns = sorted(all_turns, key=lambda t: (t.ts, str(t.trace), t.line_no))
    turns_with_commands = [t for t in turns if t.commands]
    scored = [t for t in turns_with_commands if t.uncertainty_score() >= args.min_score]
    ranked = sorted(scored, key=lambda t: (t.uncertainty_score(), len(t.commands), t.ts), reverse=True)
    scored_by_type: dict[str, list[UserTurn]] = {}
    for turn in scored:
        scored_by_type.setdefault(turn.turn_type, []).append(turn)
    for typed_turns in scored_by_type.values():
        typed_turns.sort(key=lambda t: (t.uncertainty_score(), len(t.commands), t.ts), reverse=True)

    print(f"Trace files requested: {len(trace_paths) + len(missing)}")
    print(f"Trace files read: {len(trace_paths)}")
    if missing:
        print(f"Missing files: {len(missing)}")
    print(f"User turns with previews: {len(turns)}")
    print(f"Bash tool commands: {total_commands}")
    print(f"Turns followed by tool commands within {args.window_seconds}s: {len(turns_with_commands)}")
    print(f"Command categories: {dict(all_category_counts)}")
    print(f"Turns with agent-side uncertainty score >= {args.min_score}: {len(scored)}")
    print(f"High-score turns by type: {dict(Counter(t.turn_type for t in scored))}")
    print()

    print("Highest Agent-Side Uncertainty Scores")
    print("=====================================")
    if not ranked:
        print("None.")
        return 0
    for turn in ranked[: args.top]:
        print_turn(turn)
    if len(ranked) > args.top:
        print(f"... {len(ranked) - args.top} more turns omitted")

    non_implementation = [
        t
        for t in scored
        if t.turn_type in {"conversation", "skip-worklist"}
    ]
    non_implementation.sort(key=lambda t: (t.uncertainty_score(), len(t.commands), t.ts), reverse=True)
    print()
    print("Highest Non-Implementation Scores")
    print("=================================")
    if not non_implementation:
        print("None.")
    else:
        for turn in non_implementation[: args.top]:
            print_turn(turn)
        if len(non_implementation) > args.top:
            print(f"... {len(non_implementation) - args.top} more turns omitted")

    print()
    print("Highest Scores By Turn Type")
    print("===========================")
    if not scored_by_type:
        print("None.")
    else:
        for turn_type in sorted(scored_by_type):
            print(f"{turn_type}")
            print("-" * len(turn_type))
            for turn in scored_by_type[turn_type][: min(5, args.top)]:
                print_turn(turn)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
