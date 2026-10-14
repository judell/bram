#!/usr/bin/env python3
"""Build, save, hydrate, and launch full-fidelity synthetic Bram instances.

The generated repository is deliberately disposable: it has a marker, a local
``demo/base`` branch, no remotes, and parked ``scenario/<name>`` branches.  A
scenario branch ends at a pending-work commit whose metadata names its clean
boundary and claim trees.  ``use`` checks out that commit detached, resets it
mixed to the clean boundary, and recreates the scenario's local claim refs.
The parked branch therefore survives while Bram sees the intended dirty tree.
"""

from __future__ import annotations

import argparse
import atexit
import contextlib
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time
from typing import Any, Iterable, Iterator, Mapping, Sequence


SOURCE_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REPO = SOURCE_ROOT.parent / "bram-demo-instance"
MARKER = ".bram-demo.json"
SCENARIO_META = ".bram-demo-scenario.json"
WORKLIST = "resources/worklist.json"
CLAIMS = "resources/.claim-intervals.json"
AUTH = "resources/.worklist-authorization.json"
RUNTIME_DIR = "bram-demo"
FORMAT_VERSION = 1
SCENARIO_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")

AGENT_ENV_KEYS = (
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
)

STARTERS = (
    "disjoint-entanglement",
    "dependency",
    "supersession",
    "unattributed",
    "many-claimants",
    "expired-authorization",
    "ambiguous-duplicate",
    "reedit-own-lines",
    "multi-file",
    "committed-outside",
    "new-file-left-out",
    "chat-edit-in-item-file",
    "joint-owner-committed",
    "entangled-with-unowned",
    "membership-unanswerable",
)

# What membership should answer for each starter, checked by
# scripts/membership-starter-check.py against a live demo's /__worklist.
# Paths are relative to the repo with "{p}" for the starter's prefix. Per path:
# `owners` (the sorted owner ids), and the (added, removed) line counts in
# `single`, `joint` and `unowned`. A path listed under `absent` must have no
# membership entry. `will_commit` pins an item's (added, removed) take where the
# starter exists to pin that count. Each entry restates its starter's own
# comment; when the engine and this table disagree, read the starter first.
EXPECTATIONS: dict[str, dict[str, Any]] = {
    # Two begun items each own one hunk of one file.
    "disjoint-entanglement": {
        "paths": {
            "demo/{p}.txt": {"owners": ["{p}-bottom", "{p}-top"], "single": (2, 2)},
        },
    },
    # prepare rewrote the line (its removal is prepare's); consume rewrote it
    # again (the surviving addition is consume's).
    "dependency": {
        "paths": {
            "demo/{p}.txt": {"owners": ["{p}-consume", "{p}-prepare"], "single": (1, 1)},
        },
    },
    # replace rewrote draft's added line, so the surviving line is replace's and
    # draft owns nothing (attribution-model.md, "Supersession and drift").
    "supersession": {
        "paths": {"demo/{p}.txt": {"owners": ["{p}-replace"], "single": (1, 0)}},
    },
    # Written with no claim open: nobody's.
    "unattributed": {
        "paths": {"demo/{p}.txt": {"owners": [], "unowned": (1, 1)}},
    },
    # Six items approved in one click share one claim: one jointly owned line.
    "many-claimants": {
        "paths": {
            "demo/{p}.txt": {
                "owners": [f"{{p}}-{n}" for n in range(1, 7)],
                "joint": (1, 1),
            },
        },
    },
    # A proposed, never-begun item: nothing to attribute.
    "expired-authorization": {"absent": ["demo/{p}.txt"]},
    # Identical-context duplicates: the line rule is positional, so both edits
    # are the aligning item's, and nothing is ambiguous.
    "ambiguous-duplicate": {
        "paths": {"demo/{p}.txt": {"owners": ["{p}-align"], "single": (2, 2)}},
    },
    # #367: a file created in one window and re-edited in a second counts its
    # five lines once, not once per window.
    "reedit-own-lines": {
        "paths": {"demo/{p}-created.txt": {"owners": ["{p}-author"], "single": (5, 0)}},
        "will_commit": {"{p}-author": (5, 0)},
    },
    # One begun item over five files; the listed-but-untouched one has no entry.
    "multi-file": {
        "paths": {
            "demo/{p}/README.md": {"owners": ["{p}-feature"], "single": (80, 1)},
            "demo/{p}/app.js": {"owners": ["{p}-feature"], "single": (3, 2)},
            "demo/{p}/format.js": {"owners": ["{p}-feature"], "single": (3, 0)},
            "demo/{p}/styles.css": {"owners": ["{p}-feature"], "single": (1, 1)},
        },
        "absent": ["demo/{p}/untouched.txt"],
    },
    # #406: the landed row's work is in HEAD, the reverted row left nothing.
    "committed-outside": {"absent": ["demo/{p}-landed.txt", "demo/{p}-reverted.txt"]},
    # #419: the author owns its new file and its top-line edit. The neighbour's
    # edit to the shared file is unowned, because the neighbour doesn't declare
    # that file (membership credits only begun items that declare the path).
    "new-file-left-out": {
        "paths": {
            "demo/{p}-new.txt": {"owners": ["{p}-author"], "single": (3, 0)},
            "demo/{p}-shared.txt": {
                "owners": ["{p}-author"],
                "single": (1, 1),
                "unowned": (1, 1),
            },
        },
    },
    # #273: the item keeps its surviving lines (2, 3 and two appended, plus its
    # removals of 2, 3 and 5); the chat turn's lines 5 and 8 are unowned.
    "chat-edit-in-item-file": {
        "paths": {
            "demo/{p}-file.txt": {
                "owners": ["{p}-author"],
                "single": (4, 3),
                "unowned": (2, 1),
            },
        },
    },
    # #432: the owner committed and left; the later item owns the shared file.
    "joint-owner-committed": {
        "paths": {"demo/{p}-shared.txt": {"owners": ["{p}-later"], "single": (1, 1)}},
        "absent": ["demo/{p}-vendor.txt"],
    },
    # Two owners of one file plus one hunk nobody owns.
    "entangled-with-unowned": {
        "paths": {
            "demo/{p}.txt": {
                "owners": ["{p}-bottom", "{p}-top"],
                "single": (2, 2),
                "unowned": (1, 1),
            },
        },
        "will_commit": {"{p}-top": (1, 1), "{p}-bottom": (1, 1)},
    },
    # #437: a declared file not yet created and an ignored declared file must
    # not stop the compute; the planner owns its edit.
    "membership-unanswerable": {
        "paths": {"demo/{p}/code.txt": {"owners": ["{p}-planner"], "single": (1, 1)}},
        "will_commit": {"{p}-planner": (1, 1)},
    },
}

# The ambiguous-duplicate fixture's stanza pair. Geometry is load-bearing:
# with git's default 3-line hunk context, the claim-era edit below produces
# two hunks whose new-side block (context + added lines) is the same 7-line
# all-original sequence. Draft stanzas sit at positions 2 and 5 of 6 so that
# BEFORE the edit no phase-aligned 7-line window is all-original (the
# evidence must not reverse-apply against HEAD — that would read as already
# committed), while AFTER the edit the 18-line periodic file contains the
# block at two non-overlapping placements (lines 2-8 and 11-17) — the
# identical-context duplicate the membership engine must declare ambiguous
# rather than resolve by coin flip (docs/attribution-model.md §4).
_AMBIG_ORIGINAL = "alpha\nvalue: original\nomega\n"
_AMBIG_DRAFT = "alpha\nvalue: draft\nomega\n"

_LAUNCHED_PROCESSES: dict[int, subprocess.Popen[bytes]] = {}


def _release_process_handles_at_exit() -> None:
    # A launch is intentionally longer-lived than this short CLI invocation.
    # Mark the detached handle as released so Python does not warn that the
    # deliberately surviving process was abandoned. A same-process test/host
    # uses stop_demo(), which removes and waits for the real Popen first.
    for proc in _LAUNCHED_PROCESSES.values():
        proc.returncode = 0


atexit.register(_release_process_handles_at_exit)


class DemoError(RuntimeError):
    """A safe, user-facing refusal or fixture error."""


# The multi-file fixture's baseline: one begun item whose table needs rows of
# every kind (modified, new, large, untouched) to show the file-row
# disclosure triangle in all its states.
def _multi_file_seeds(prefix: str) -> list[tuple[str, str]]:
    root = f"demo/{prefix}"
    return [
        (
            f"{root}/app.js",
            "".join(f"function step{n}() {{ return {n}; }}\n" for n in range(1, 21)),
        ),
        (
            f"{root}/styles.css",
            "".join(f".row-{n} {{ padding: {n}px; }}\n" for n in range(1, 11)),
        ),
        (f"{root}/README.md", "# Demo\n\nA short readme.\n"),
        (f"{root}/untouched.txt", "Listed by the item, never changed.\n"),
    ]


# The new-file-left-out fixture's shared file: ten lines, with the author's
# region at the top and the neighbour's at the bottom so the two hunks stay
# separable. `edits` is 0 (HEAD), 1 (author's edit), 2 (author's and neighbour's).
def _new_file_left_out_shared(prefix: str, edits: int) -> str:
    lines = ["top: base"] + ["middle"] * 8 + ["bottom: base"]
    if edits >= 1:
        lines[0] = f"top: changed by {prefix}-author"
    if edits >= 2:
        lines[-1] = f"bottom: changed by {prefix}-neighbour"
    return "".join(f"{line}\n" for line in lines)


# The chat-edit-in-item-file fixture's file: ten distinct lines. `stage` is 0
# (HEAD), 1 (the author's first window), 2 (a chat turn, no claim live) or 3
# (the author's second window). Each changed line says who wrote it.
def _chat_edit_file(stage: int) -> str:
    lines = [f"line {n}: base" for n in range(1, 11)]
    if stage >= 1:
        lines[1] = "line 2: changed by the author"
        lines[4] = "line 5: changed by the author"
        lines += ["line 11: appended by the author", "line 12: appended by the author"]
    if stage >= 2:
        lines[4] = "line 5: rewritten in chat"
        lines[7] = "line 8: changed in chat"
    if stage >= 3:
        lines[2] = "line 3: changed by the author, second window"
    return "".join(f"{line}\n" for line in lines)


def _joint_owner_shared(prefix: str, owner_edit: bool, later_edit: bool = False) -> str:
    lines = ["shared: base\n", "middle\n" * 6]
    lines.append(f"{prefix}-owner: committed step URL\n" if owner_edit else "owner: base\n")
    lines.append("middle\n" * 6)
    lines.append(f"{prefix}-later: token terms\n" if later_edit else "later: base\n")
    return "".join(lines)


def _entangled_unowned(top: bool, middle: bool, bottom: bool) -> str:
    # 17 lines: the three changed lines (1, 9, 17) are 7 lines apart, more
    # than git's 2 x 3 lines of context, so each lands in its own hunk.
    lines = ["top: changed by A" if top else "top: base"]
    lines += ["middle"] * 7
    lines += ["middle: changed with no claim" if middle else "middle: base"]
    lines += ["middle"] * 7
    lines += ["bottom: changed by B" if bottom else "bottom: base"]
    return "\n".join(lines) + "\n"


def starter_seeds(starter: str, prefix: str) -> list[tuple[str, str]]:
    if starter == "multi-file":
        return _multi_file_seeds(prefix)
    if starter == "committed-outside":
        return [
            (f"demo/{prefix}-landed.txt", "landed: committed with plain git\n"),
            (f"demo/{prefix}-reverted.txt", "reverted: nothing of the item survives\n"),
        ]
    if starter == "new-file-left-out":
        return [
            (f"demo/{prefix}-shared.txt", _new_file_left_out_shared(prefix, 0)),
            (f"demo/{prefix}-own.txt", "own: never changed by the neighbour\n"),
        ]
    if starter == "chat-edit-in-item-file":
        return [(f"demo/{prefix}-file.txt", _chat_edit_file(0))]
    if starter == "membership-unanswerable":
        # Its own folder, so the ignore rule below reaches no other starter.
        return [
            (f"demo/{prefix}/.gitignore", "/*.md\n"),
            (f"demo/{prefix}/code.txt", "code: base\n"),
        ]
    if starter == "joint-owner-committed":
        # HEAD already holds the owner's edit: it "committed" before the board
        # was captured, so the joint window below replays base -> edited.
        return [
            (f"demo/{prefix}-shared.txt", _joint_owner_shared(prefix, owner_edit=True)),
            (f"demo/{prefix}-vendor.txt", "vendor: runtime 1.0\n"),
        ]
    return [starter_seed(starter, prefix)]


def starter_seed(starter: str, prefix: str) -> tuple[str, str]:
    seeds = {
        "disjoint-entanglement": (
            f"demo/{prefix}.txt",
            "top: base\n" + "middle\n" * 8 + "bottom: base\n",
        ),
        "dependency": (f"demo/{prefix}.txt", "pipeline = base\n"),
        "supersession": (f"demo/{prefix}.txt", "message: stable\n"),
        "unattributed": (f"demo/{prefix}.txt", "owner: none\n"),
        "many-claimants": (f"demo/{prefix}.txt", "shared: base\n"),
        "expired-authorization": (
            f"demo/{prefix}.txt",
            "authorization: waiting\n",
        ),
        "ambiguous-duplicate": (
            f"demo/{prefix}.txt",
            _AMBIG_ORIGINAL
            + _AMBIG_DRAFT
            + _AMBIG_ORIGINAL * 2
            + _AMBIG_DRAFT
            + _AMBIG_ORIGINAL,
        ),
        "reedit-own-lines": (
            f"demo/{prefix}.txt",
            "reedit: the created sibling arrives under two claim windows\n",
        ),
        "entangled-with-unowned": (f"demo/{prefix}.txt", _entangled_unowned(False, False, False)),
    }
    try:
        return seeds[starter]
    except KeyError as exc:
        raise DemoError(f"unknown starter {starter!r}; choose from {', '.join(STARTERS)}") from exc


def _json_bytes(value: Any) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(_json_bytes(value))


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise DemoError(f"cannot read {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise DemoError(f"expected a JSON object in {path}")
    return value


def run_git(
    repo: Path,
    *args: str,
    env: Mapping[str, str] | None = None,
    check: bool = True,
    input_text: str | None = None,
) -> str:
    merged = os.environ.copy()
    if env:
        merged.update(env)
    proc = subprocess.run(
        ["git", "-C", str(repo), *args],
        input=input_text,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=merged,
        check=False,
    )
    if check and proc.returncode:
        detail = proc.stderr.strip() or proc.stdout.strip()
        raise DemoError(f"git {' '.join(args)} failed: {detail}")
    return proc.stdout.strip()


def scenario_ref(name: str) -> str:
    if not SCENARIO_RE.fullmatch(name):
        raise DemoError(
            "scenario names may contain letters, digits, '.', '_', and '-' only"
        )
    return f"refs/heads/scenario/{name}"


def _dangerous_roots() -> set[Path]:
    roots = {Path('/').resolve(), Path.home().resolve(), SOURCE_ROOT.resolve()}
    if os.name == "nt":
        roots.add(Path(Path.cwd().anchor).resolve())
    return roots


def _assert_safe_location(repo: Path) -> Path:
    resolved = repo.expanduser().resolve()
    if resolved in _dangerous_roots():
        raise DemoError(f"refusing unsafe demo target: {resolved}")
    return resolved


def validate_demo(repo: Path) -> Path:
    repo = _assert_safe_location(repo)
    marker_path = repo / MARKER
    if not marker_path.is_file():
        raise DemoError(
            f"refusing to mutate {repo}: missing {MARKER} synthetic-repo marker"
        )
    marker = read_json(marker_path)
    if marker.get("kind") != "bram-synthetic-demo" or marker.get("format") != FORMAT_VERSION:
        raise DemoError(f"refusing {repo}: unrecognized {MARKER}")
    top = run_git(repo, "rev-parse", "--show-toplevel")
    if Path(top).resolve() != repo:
        raise DemoError(f"refusing nested or mismatched git root: {top}")
    remotes = run_git(repo, "remote").splitlines()
    if remotes:
        raise DemoError(
            f"refusing demo repo with upstream/remotes ({', '.join(remotes)})"
        )
    return repo


def _fixed_git_env(step: int = 0) -> dict[str, str]:
    stamp = 1_700_000_000 + step
    return {
        "GIT_AUTHOR_NAME": "Bram demo",
        "GIT_AUTHOR_EMAIL": "demo@bram.invalid",
        "GIT_COMMITTER_NAME": "Bram demo",
        "GIT_COMMITTER_EMAIL": "demo@bram.invalid",
        "GIT_AUTHOR_DATE": f"@{stamp} +0000",
        "GIT_COMMITTER_DATE": f"@{stamp} +0000",
    }


def initialize_repo(repo: Path) -> Path:
    repo = _assert_safe_location(repo)
    if repo.exists() and any(repo.iterdir()):
        if (repo / MARKER).is_file():
            return validate_demo(repo)
        raise DemoError(f"refusing non-empty, unmarked directory: {repo}")
    repo.mkdir(parents=True, exist_ok=True)
    proc = subprocess.run(
        ["git", "init", "-q", "-b", "demo/base", str(repo)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode:
        run_git(repo, "init", "-q")
        run_git(repo, "checkout", "-q", "-b", "demo/base")
    run_git(repo, "config", "user.name", "Bram demo")
    run_git(repo, "config", "user.email", "demo@bram.invalid")

    write_json(
        repo / MARKER,
        {
            "format": FORMAT_VERSION,
            "kind": "bram-synthetic-demo",
            "warning": "Disposable local fixture. Never add a remote.",
        },
    )
    (repo / ".gitignore").write_text(
        "\n".join(
            [
                ".bram-demo-scenario.json",
                "resources/worklist.json",
                "resources/worklist-drafts/",
                "resources/.claim-intervals.json",
                "resources/.worklist-authorization.json",
                "resources/.worklist-direct-edit.json",
                "resources/.inflight-claim.json",
                "resources/.worklist-intent.json",
                "resources/.worklist-result.json",
                "resources/.bram-port",
                "resources/.bram-port.json",
                "resources/bram-traces/",
                "*.log",
                "",
            ]
        ),
        encoding="utf-8",
    )
    write_json(
        repo / ".bram.json",
        {
            "menus": {"hookDriven": True, "parseAndDisplay": True},
            "traces": {"enabled": True},
            "ui": {"showTargetApp": False, "targetAppMinimized": True},
        },
    )
    (repo / "README.md").write_text(
        "# Bram synthetic demo\n\nGenerated by scripts/demo_instance.py.\n",
        encoding="utf-8",
    )
    (repo / "resources").mkdir(exist_ok=True)
    run_git(repo, "add", "-A")
    run_git(repo, "commit", "-q", "-m", "Synthetic demo boundary", env=_fixed_git_env())
    run_git(repo, "update-ref", "refs/heads/demo/base", "HEAD")
    return validate_demo(repo)


@contextlib.contextmanager
def temporary_index(repo: Path, base: str) -> Iterator[dict[str, str]]:
    fd, raw = tempfile.mkstemp(prefix="bram-demo-index-")
    os.close(fd)
    os.unlink(raw)
    env = {"GIT_INDEX_FILE": raw}
    try:
        run_git(repo, "read-tree", base, env=env)
        yield env
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(raw)


def capture_tree(repo: Path, base: str = "HEAD", force_paths: Iterable[str] = ()) -> str:
    with temporary_index(repo, base) as env:
        run_git(repo, "add", "-A", "--", ".", env=env)
        existing = [p for p in force_paths if (repo / p).exists()]
        if existing:
            run_git(repo, "add", "-f", "--", *existing, env=env)
        tree = run_git(repo, "write-tree", env=env)
    if not tree:
        raise DemoError("git produced an empty tree id")
    return tree


def _load_controls(repo: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    worklist = (
        read_json(repo / WORKLIST)
        if (repo / WORKLIST).exists()
        else {"description": "Synthetic Bram worklist", "items": [], "version": 1}
    )
    claims = (
        read_json(repo / CLAIMS)
        if (repo / CLAIMS).exists()
        else {"intervals": [], "version": 1}
    )
    meta = (
        read_json(repo / SCENARIO_META)
        if (repo / SCENARIO_META).exists()
        else {"format": FORMAT_VERSION, "starters": []}
    )
    return worklist, claims, meta


def _write_controls(
    repo: Path, worklist: dict[str, Any], claims: dict[str, Any], meta: dict[str, Any]
) -> None:
    write_json(repo / WORKLIST, worklist)
    write_json(repo / CLAIMS, claims)
    write_json(repo / SCENARIO_META, meta)


class StarterBuilder:
    def __init__(self, repo: Path, scenario: str):
        self.repo = repo
        self.scenario = scenario
        self.worklist, self.claims, self.meta = _load_controls(repo)
        self.meta.setdefault("starters", [])
        digest = int(hashlib.sha256(scenario.encode()).hexdigest()[:10], 16)
        prior = [int(x.get("atMs", 0)) for x in self.claims.get("intervals", [])]
        self.next_at_ms = max(prior, default=1_800_000_000_000 + (digest % 10_000_000) * 100)

    def _occurrence(self, starter: str) -> int:
        return 1 + sum(1 for value in self.meta["starters"] if value == starter)

    def _prefix(self, starter: str, occurrence: int) -> str:
        return starter if occurrence == 1 else f"{starter}-{occurrence}"

    def item(
        self,
        item_id: str,
        path: str | Sequence[str],
        before: str,
        after: str,
        status: str = "applied",
        begun_at_ms: int | None = None,
    ) -> None:
        files = [path] if isinstance(path, str) else list(path)
        entry: dict[str, Any] = {"files": files, "id": item_id, "status": status}
        if begun_at_ms is not None:
            entry["begunAtMs"] = begun_at_ms
        self.worklist.setdefault("items", []).append(entry)
        draft = self.repo / "resources/worklist-drafts" / f"{item_id}.md"
        draft.parent.mkdir(parents=True, exist_ok=True)
        draft.write_text(f"# Before\n\n{before}\n\n# After\n\n{after}\n", encoding="utf-8")

    def boundary(self, ids: Sequence[str], kind: str = "approved") -> None:
        tree = capture_tree(self.repo)
        self.next_at_ms += 1
        at_ms = self.next_at_ms
        ref = f"refs/bram/claims/{at_ms}"
        run_git(self.repo, "update-ref", ref, tree)
        self.claims.setdefault("intervals", []).append(
            {"atMs": at_ms, "ids": list(ids), "kind": kind, "ref": ref, "tree": tree}
        )

    def file(self, rel: str, text: str) -> None:
        path = self.repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def add(self, starter: str) -> None:
        if starter not in STARTERS:
            raise DemoError(f"unknown starter {starter!r}; choose from {', '.join(STARTERS)}")
        occurrence = self._occurrence(starter)
        prefix = self._prefix(starter, occurrence)
        getattr(self, f"starter_{starter.replace('-', '_')}")(prefix)
        self.meta["starters"].append(starter)
        self.worklist["version"] = int(self.worklist.get("version", 0)) + 1
        _write_controls(self.repo, self.worklist, self.claims, self.meta)

    def starter_disjoint_entanglement(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        a, b = f"{prefix}-top", f"{prefix}-bottom"
        self.item(a, path, "The top line is unchanged.", "The top line identifies item A.")
        self.item(b, path, "The bottom line is unchanged.", "The bottom line identifies item B.")
        self.boundary([a])
        self.file(path, "top: changed by A\n" + "middle\n" * 8 + "bottom: base\n")
        self.boundary([b])
        self.file(path, "top: changed by A\n" + "middle\n" * 8 + "bottom: changed by B\n")
        self.boundary([])

    def starter_dependency(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        a, b = f"{prefix}-prepare", f"{prefix}-consume"
        self.item(a, path, "The pipeline is unprepared.", "Prepare the pipeline for a consumer.")
        self.item(b, path, "No consumer is wired.", "Wire a consumer to the prepared pipeline.")
        self.boundary([a])
        self.file(path, "pipeline = prepared\n")
        self.boundary([b])
        self.file(path, "pipeline = prepared + consumer\n")
        self.boundary([])

    def starter_supersession(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        a, b = f"{prefix}-draft", f"{prefix}-replace"
        self.item(a, path, "Only the stable message exists.", "Add an experimental message.")
        self.item(b, path, "The experimental message is visible.", "Replace it with final copy.")
        self.boundary([a])
        self.file(path, "message: stable\nmessage: experimental A\n")
        self.boundary([b])
        self.file(path, "message: stable\nmessage: final B\n")
        self.boundary([])

    def starter_unattributed(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        item_id = f"{prefix}-observer"
        self.item(item_id, path, "The file has no attributed edit.", "Observe an unclaimed edit.")
        self.boundary([])
        self.file(path, "owner: nobody claimed this line\n")
        self.boundary([item_id])
        self.boundary([])

    def starter_many_claimants(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        ids = [f"{prefix}-{n}" for n in range(1, 7)]
        for item_id in ids:
            self.item(item_id, path, "The shared value is at baseline.", f"Contribute {item_id}.")
        self.boundary(ids)
        self.file(path, "shared: " + ", ".join(ids) + "\n")
        self.boundary([])

    def starter_ambiguous_duplicate(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        item_id = f"{prefix}-align"
        self.item(
            item_id,
            path,
            "Stanzas 2 and 5 read `value: draft`; the other four read `value: original`.",
            "Align stanzas 2 and 5 to `value: original`, leaving six byte-identical stanzas "
            "whose duplicate placements the attribution engines must not guess between.",
        )
        self.boundary([item_id])
        self.file(path, _AMBIG_ORIGINAL * 6)
        self.boundary([])

    def starter_reedit_own_lines(self, prefix: str) -> None:
        # judell/bram#367's shape: one item, two claim windows, a file
        # created in the first window and every line re-edited in the second.
        # The concatenated evidence counted each re-touched line once per
        # window (10) while the universe holds 5 - the conservation
        # tripwire's third catch. Post-fix, net normalization reads 5.
        path = f"demo/{prefix}-created.txt"
        item_id = f"{prefix}-author"
        self.item(
            item_id,
            path,
            "The created file does not exist yet.",
            "Create five lines under one claim window, then re-edit all five "
            "under a second window of the same item.",
        )
        self.boundary([item_id])
        self.file(path, "".join(f"line {n}: first pass\n" for n in range(1, 6)))
        self.boundary([])
        self.boundary([item_id])
        self.file(path, "".join(f"line {n}: second pass\n" for n in range(1, 6)))
        self.boundary([])

    def starter_multi_file(self, prefix: str) -> None:
        # One begun item over five files, one of each kind the files table
        # renders: two modified, one new, one large diff, one listed but
        # untouched (no diff, so no disclosure triangle).
        root = f"demo/{prefix}"
        item_id = f"{prefix}-feature"
        files = [
            f"{root}/app.js",
            f"{root}/styles.css",
            f"{root}/format.js",
            f"{root}/README.md",
            f"{root}/untouched.txt",
        ]
        self.item(
            item_id,
            files,
            "A small app with a short readme.",
            "Add a formatter, tweak two steps and a style, and write the readme out properly.",
        )
        self.boundary([item_id])
        app = "".join(f"function step{n}() {{ return {n}; }}\n" for n in range(1, 21))
        app = app.replace("return 3;", "return format(3);").replace("return 12;", "return format(12);")
        self.file(f"{root}/app.js", "import { format } from './format.js';\n" + app)
        css = "".join(f".row-{n} {{ padding: {n}px; }}\n" for n in range(1, 11))
        self.file(f"{root}/styles.css", css.replace("padding: 4px", "padding: 6px"))
        self.file(
            f"{root}/format.js",
            "export function format(n) {\n  return `#${n}`;\n}\n",
        )
        self.file(
            f"{root}/README.md",
            "# Demo\n\n"
            + "".join(f"## Section {n}\n\nParagraph {n} of the longer readme.\n\n" for n in range(1, 21)),
        )
        self.boundary([])

    def starter_new_file_left_out(self, prefix: str) -> None:
        # judell/bram#419's shape: the author item started a NEW, never-tracked
        # file and also changed a tracked file. A neighbour item edited that
        # same file during its own claim window WITHOUT declaring it, as
        # issue-406-chat-commit-everything-ready did with helpers.js. That
        # omission is load-bearing: the pane sees no shared declared path, so
        # it sends no isolate instruction with the Commit click, while the
        # host's replay still credits the neighbour with lines there and
        # routes the author's commit to hunk-by-hunk staging - where a new
        # file was silently left out of commits 30aa4d0 and fe84674. (With
        # the path declared by both, the agent isolates by hand and the
        # commit takes the whole-file path, which stages new files.) The
        # author's second window re-edits the new file so it carries two
        # windows of evidence, as in the real case.
        new = f"demo/{prefix}-new.txt"
        shared = f"demo/{prefix}-shared.txt"
        own = f"demo/{prefix}-own.txt"
        author, neighbour = f"{prefix}-author", f"{prefix}-neighbour"
        self.item(
            author,
            [new, shared],
            "The new file does not exist; the shared file's top line is unchanged.",
            "Create the new file and change the shared file's top line.",
        )
        self.item(
            neighbour,
            own,
            "The neighbour's own file is unchanged.",
            "Declares only its own file, which it never changes; its one edit "
            "lands in the author's shared file instead.",
        )
        self.boundary([author])
        self.file(new, "".join(f"line {n}: first pass\n" for n in range(1, 4)))
        self.file(shared, _new_file_left_out_shared(prefix, 1))
        self.boundary([])
        self.boundary([neighbour])
        self.file(shared, _new_file_left_out_shared(prefix, 2))
        self.boundary([])
        self.boundary([author])
        self.file(new, "".join(f"line {n}: second pass\n" for n in range(1, 4)))
        self.boundary([])

    def starter_chat_edit_in_item_file(self, prefix: str) -> None:
        # judell/bram#273's 2026-10-02 ruling. An item edits a file in its own
        # claim window, a chat turn then edits the same file with no claim
        # live, and the item edits it again in a second window. The membership
        # engine used to credit the item with nothing in such a file, because
        # the item's whole patch no longer reverse-applied; the line-level
        # rule credits the item with its surviving lines (2, 3 and the two
        # appended) and leaves the chat-turn lines (5 and 8) unowned. Each
        # changed line's text names its writer, so the diff checks the board.
        path = f"demo/{prefix}-file.txt"
        author = f"{prefix}-author"
        self.item(
            author,
            path,
            "The file is unchanged.",
            "Change lines 2, 3 and 5 and append two lines, across two claim "
            "windows; a chat turn between them rewrites line 5 and line 8.",
        )
        self.boundary([author])
        self.file(path, _chat_edit_file(1))
        self.boundary([])
        self.file(path, _chat_edit_file(2))
        self.boundary([author])
        self.file(path, _chat_edit_file(3))
        self.boundary([])

    def starter_membership_unanswerable(self, prefix: str) -> None:
        # judell/bram#437: two ordinary board shapes that made the membership
        # engine's scratch-index `add -A -- <declared entries>` fail, so every
        # gate commit on the board was refused as "no membership answer"
        # (`op=membership paths=0 spawns=7`). One begun item declares a file it
        # has not created yet; another declares a new file that the folder's
        # `/*.md` rule ignores, as CodeExam's THIRD-PARTY-NOTICES.md was.
        # Expected since the fix: Commit on the planner goes through, and
        # `op=membership` at the gate reads `fresh=true` with paths > 0. The
        # notices item's own commit still meets the ignore rule at `git add`
        # (#373's refuse-ignored-path) unless the file is force-staged; that is
        # a separate, deliberate refusal. A scenario is stored as git trees,
        # so the ignored NOTICES.md does not survive `use`/`reset`: until it
        # is recreated by hand, the notices item is a second declared file
        # not yet created, which breaks the old seed just as well.
        code = f"demo/{prefix}/code.txt"
        planned = f"demo/{prefix}/planned.txt"
        notices = f"demo/{prefix}/NOTICES.md"
        planner, noticer = f"{prefix}-planner", f"{prefix}-notices"
        self.item(
            planner,
            [code, planned],
            "code.txt reads base; planned.txt does not exist yet.",
            "Change code.txt now; create planned.txt later.",
        )
        self.item(
            noticer,
            notices,
            "There is no NOTICES.md.",
            "Create NOTICES.md, which the folder's /*.md rule ignores.",
        )
        self.boundary([planner])
        self.file(code, "code: changed by the planner\n")
        self.boundary([])
        self.boundary([noticer])
        self.file(notices, "notices: a new file an ignore rule hides\n")
        self.boundary([])

    def starter_entangled_with_unowned(self, prefix: str) -> None:
        # issue-425-retire-replay (Codex review): two begun owners of one file
        # plus a third hunk nobody owns. Each row alone is interval-staged and
        # predicts only its own hunk; committing both together leaves no
        # outside owner, so the gate takes the whole file, unowned hunk
        # included. The per-row numbers then don't add up to the commit.
        path = f"demo/{prefix}.txt"
        a, b = f"{prefix}-top", f"{prefix}-bottom"
        self.item(a, path, "The top line is unchanged.", "The top line identifies item A.")
        self.item(b, path, "The bottom line is unchanged.", "The bottom line identifies item B.")
        self.boundary([a])
        self.file(path, _entangled_unowned(True, False, False))
        self.boundary([])
        self.file(path, _entangled_unowned(True, True, False))
        self.boundary([b])
        self.file(path, _entangled_unowned(True, True, True))
        self.boundary([])

    def starter_joint_owner_committed(self, prefix: str) -> None:
        # judell/bram#432. `owner` and `partner` were approved in one click
        # (one joint claim interval). Their files don't overlap: owner edited
        # the shared file, partner the vendor file. Owner has since committed
        # and been pruned (its edit is in HEAD; it is not on the board), and
        # partner's edit was reverted (no changes, still begun). A later,
        # separately approved item edits the shared file. Committing `later`
        # must succeed: the replay re-resolved the joint interval to partner
        # once owner left the board and refused it (op=refuse-joint-interval
        # ids=[partner]).
        shared = f"demo/{prefix}-shared.txt"
        vendor = f"demo/{prefix}-vendor.txt"
        owner, partner, later = f"{prefix}-owner", f"{prefix}-partner", f"{prefix}-later"
        self.item(
            partner,
            vendor,
            "Approved in one click with the (since committed) owner; its own edit was reverted.",
            "Has no changes; should not block anything. Drop it.",
        )
        self.item(
            later,
            shared,
            "Approved separately, after the owner committed the shared file.",
            "Should commit: it was never approved together with the partner.",
        )
        self.file(shared, _joint_owner_shared(prefix, owner_edit=False))
        self.boundary([owner, partner])
        self.file(shared, _joint_owner_shared(prefix, owner_edit=True))
        self.file(vendor, "vendor: runtime 2.0\n")
        self.file(vendor, "vendor: runtime 1.0\n")
        self.boundary([])
        self.boundary([later])
        self.file(shared, _joint_owner_shared(prefix, owner_edit=True, later_edit=True))
        self.boundary([])

    def starter_committed_outside(self, prefix: str) -> None:
        # judell/bram#406: work committed with plain git, outside the gate.
        # Fixture commits are dated from 1_700_000_000 (_fixed_git_env), so:
        # - landed: begun before the baseline commit that created its file,
        #   which therefore counts as its landing commit;
        # - reverted: begun after every fixture commit, file clean. Also
        #   "clean", but no commit touched it since it began, so it must NOT
        #   read as landed (it's a Drop).
        self.item(
            f"{prefix}-landed",
            f"demo/{prefix}-landed.txt",
            "The change was committed with plain git, outside the Worklist.",
            "Should read as committed outside the Worklist, and clear in one click.",
            begun_at_ms=1_699_999_000_000,
        )
        self.item(
            f"{prefix}-reverted",
            f"demo/{prefix}-reverted.txt",
            "The item began, but its change was reverted before any commit.",
            "Should not read as landed: nothing committed it since it began.",
            status="proposed",
            begun_at_ms=1_700_001_000_000,
        )

    def starter_expired_authorization(self, prefix: str) -> None:
        path = f"demo/{prefix}.txt"
        item_id = f"{prefix}-approval"
        self.item(
            item_id,
            path,
            "The item awaits approval.",
            "Exercise the expired authorization path.",
            status="proposed",
        )
        self.file(path, "authorization: expired\n")
        write_json(
            self.repo / AUTH,
            {
                "commitToo": False,
                "consumedAtMs": None,
                "ids": [item_id],
                "interruptedAtMs": None,
                "issuedAtMs": 1,
                "items": [],
                "kind": "approved",
                "source": "synthetic-expired-authorization",
            },
        )


def _control_paths(repo: Path) -> list[str]:
    paths = [SCENARIO_META, WORKLIST, CLAIMS, AUTH, "resources/worklist-drafts"]
    return [path for path in paths if (repo / path).exists()]


def _claim_records(repo: Path) -> list[dict[str, Any]]:
    if not (repo / CLAIMS).exists():
        return []
    claims = read_json(repo / CLAIMS).get("intervals", [])
    if not isinstance(claims, list):
        raise DemoError(f"expected an intervals array in {CLAIMS}")
    records: list[dict[str, Any]] = []
    for raw in claims:
        if not isinstance(raw, dict) or not isinstance(raw.get("ref"), str):
            raise DemoError(f"invalid claim record in {CLAIMS}")
        record = dict(raw)
        tree = run_git(repo, "rev-parse", f"{record['ref']}^{{tree}}")
        record["tree"] = tree
        records.append(record)
    return records


def _commit_tree(repo: Path, tree: str, parent: str, message: str, step: int) -> str:
    return run_git(
        repo,
        "commit-tree",
        tree,
        "-p",
        parent,
        "-m",
        message,
        env=_fixed_git_env(step),
    )


def capture_scenario(repo: Path, name: str, force: bool = False) -> str:
    repo = validate_demo(repo)
    ref = scenario_ref(name)
    old = run_git(repo, "rev-parse", "--verify", "--quiet", ref, check=False)
    if old and not force:
        raise DemoError(f"scenario {name!r} already exists; pass --force to replace it")
    boundary = run_git(repo, "rev-parse", "HEAD")
    _, _, live_meta = _load_controls(repo)
    records = _claim_records(repo)
    scenario_doc = {
        "boundary": boundary,
        "claimIntervals": records,
        "format": FORMAT_VERSION,
        "name": name,
        "starters": list(live_meta.get("starters", [])),
    }
    write_json(repo / SCENARIO_META, scenario_doc)
    final_tree = capture_tree(repo, boundary, _control_paths(repo))

    parent = boundary
    for index, record in enumerate(records, 1):
        parent = _commit_tree(
            repo,
            str(record["tree"]),
            parent,
            f"Scenario {name}: claim boundary {index}",
            index,
        )
    pending = _commit_tree(
        repo, final_tree, parent, f"Scenario {name}: parked pending work", len(records) + 1
    )
    run_git(repo, "update-ref", ref, pending, *( [old] if old else [] ))
    use_scenario(repo, name)
    return pending


def _clear_claim_refs(repo: Path) -> None:
    refs = run_git(repo, "for-each-ref", "--format=%(refname)", "refs/bram/claims")
    for ref in refs.splitlines():
        if ref:
            run_git(repo, "update-ref", "-d", ref)


def _scenario_doc(repo: Path, name: str) -> tuple[str, dict[str, Any]]:
    ref = scenario_ref(name)
    tip = run_git(repo, "rev-parse", "--verify", ref)
    raw = run_git(repo, "show", f"{ref}:{SCENARIO_META}")
    try:
        doc = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise DemoError(f"scenario {name!r} has invalid metadata: {exc}") from exc
    if not isinstance(doc, dict) or doc.get("name") != name or doc.get("format") != FORMAT_VERSION:
        raise DemoError(f"scenario {name!r} has incompatible metadata")
    return tip, doc


def _runtime_root(repo: Path) -> Path:
    git_dir = Path(run_git(repo, "rev-parse", "--git-dir"))
    if not git_dir.is_absolute():
        git_dir = repo / git_dir
    path = git_dir.resolve() / RUNTIME_DIR
    path.mkdir(parents=True, exist_ok=True)
    return path


def use_scenario(repo: Path, name: str) -> None:
    repo = validate_demo(repo)
    tip, doc = _scenario_doc(repo, name)
    boundary = str(doc.get("boundary", ""))
    run_git(repo, "cat-file", "-e", f"{boundary}^{{commit}}")

    # Destructive by design, but only after the synthetic marker, exact git
    # toplevel, and no-remote checks above have all passed.
    run_git(repo, "reset", "--hard", "-q")
    # Do not clean before checkout: that creates a moment where ignored
    # worklist sidecars disappear, and a running Bram can race the hydrate by
    # restoring its newer cached version. Forced checkout replaces any
    # untracked sidecar that obstructs the parked tree without that gap.
    run_git(repo, "checkout", "--detach", "--force", "-q", tip)
    # Process/session infrastructure is not scenario state. Preserve the live
    # port and trace while a human is driving the second instance, along with
    # Setup's active agent files. Everything else untracked is disposable.
    run_git(
        repo,
        "clean",
        "-fdx",
        "-q",
        "-e",
        ".claude/",
        "-e",
        "AGENTS.md",
        "-e",
        "CLAUDE.md",
        "-e",
        "resources/.bram-port",
        "-e",
        "resources/.bram-port.json",
        "-e",
        "resources/bram-traces/",
    )
    run_git(repo, "reset", "--mixed", "-q", boundary)

    _clear_claim_refs(repo)
    for record in doc.get("claimIntervals", []):
        ref = str(record.get("ref", ""))
        tree = str(record.get("tree", ""))
        if not ref.startswith("refs/bram/claims/"):
            raise DemoError(f"scenario {name!r} contains an unsafe claim ref")
        run_git(repo, "cat-file", "-e", f"{tree}^{{tree}}")
        run_git(repo, "update-ref", ref, tree)

    write_json(
        _runtime_root(repo) / "active.json",
        {"boundary": boundary, "name": name, "ref": scenario_ref(name), "tip": tip},
    )


def reset_scenario(repo: Path) -> str:
    repo = validate_demo(repo)
    active_path = _runtime_root(repo) / "active.json"
    if not active_path.exists():
        raise DemoError("no active scenario; run 'use <name>' first")
    name = str(read_json(active_path).get("name", ""))
    use_scenario(repo, name)
    return name


def add_starter(repo: Path, starter: str) -> None:
    repo = validate_demo(repo)
    active_path = _runtime_root(repo) / "active.json"
    scenario = (
        str(read_json(active_path).get("name", "scratch"))
        if active_path.exists()
        else "scratch"
    )
    symbolic = run_git(repo, "symbolic-ref", "--quiet", "HEAD", check=False)
    if symbolic:
        raise DemoError("live demo HEAD is not detached; run 'use <scenario>' before add")
    builder = StarterBuilder(repo, scenario)
    occurrence = builder._occurrence(starter)
    prefix = builder._prefix(starter, occurrence)
    seeds = starter_seeds(starter, prefix)
    for path, seed in seeds:
        builder.file(path, seed)
    # Extend the clean boundary with only this starter's baseline. Existing
    # dirty scenario work stays out of the new commit and remains in place.
    with temporary_index(repo, "HEAD") as env:
        run_git(repo, "add", "--", *(path for path, _ in seeds), env=env)
        tree = run_git(repo, "write-tree", env=env)
    parent = run_git(repo, "rev-parse", "HEAD")
    boundary = _commit_tree(
        repo, tree, parent, f"Starter {starter}: clean baseline", occurrence + 100
    )
    run_git(repo, "update-ref", "HEAD", boundary)
    # The baseline was committed through a temporary index, so the real index
    # still lacks these paths: git status read them as staged deletions plus
    # untracked files, never clean. Sync just these paths to the new HEAD.
    run_git(repo, "reset", "-q", "--", *(path for path, _ in seeds))
    builder.add(starter)


def new_scenario(repo: Path, name: str, starters: Sequence[str], force: bool = False) -> str:
    scenario_ref(name)
    repo = initialize_repo(repo)
    process_path = _runtime_root(repo) / "process.json"
    if process_path.exists():
        pid = int(read_json(process_path).get("pid", 0) or 0)
        if pid and _process_alive(pid):
            raise DemoError(f"stop the running demo PID {pid} before creating a new scenario")
    base = run_git(repo, "rev-parse", "refs/heads/demo/base")
    run_git(repo, "reset", "--hard", "-q", base)
    run_git(repo, "clean", "-fdx", "-q")
    run_git(repo, "checkout", "--detach", "--force", "-q", base)
    _clear_claim_refs(repo)
    expanded = list(starters)
    if not expanded:
        expanded = list(STARTERS)
    elif "all" in expanded:
        if expanded != ["all"]:
            raise DemoError("use --from all by itself, or list individual starters")
        expanded = list(STARTERS)
    occurrences: dict[str, int] = {}
    for starter in expanded:
        if starter not in STARTERS:
            raise DemoError(f"unknown starter {starter!r}; choose from {', '.join(STARTERS)}")
        occurrences[starter] = occurrences.get(starter, 0) + 1
        n = occurrences[starter]
        prefix = starter if n == 1 else f"{starter}-{n}"
        for path, seed in starter_seeds(starter, prefix):
            target = repo / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(seed, encoding="utf-8")
    run_git(repo, "add", "--", "demo")
    run_git(
        repo,
        "commit",
        "-q",
        "-m",
        f"Scenario {name}: clean starter baseline",
        env=_fixed_git_env(50),
    )
    boundary = run_git(repo, "rev-parse", "HEAD")
    write_json(
        repo / SCENARIO_META,
        {"boundary": boundary, "format": FORMAT_VERSION, "name": name, "starters": []},
    )
    builder = StarterBuilder(repo, name)
    for starter in expanded:
        builder.add(starter)
    return capture_scenario(repo, name, force=force)


def scrub_agent_environment(env: Mapping[str, str]) -> dict[str, str]:
    clean = dict(env)
    for key in AGENT_ENV_KEYS:
        clean.pop(key, None)
    clean["BRAM_TRACE"] = "1"
    clean["BRAM_DEMO_INSTANCE"] = "1"
    return clean


def newest_rust_source(root: Path) -> tuple[float, str] | None:
    """The newest modification time among git-tracked files under src-tauri/,
    and that file. Under the ./bram symlink launch the binary depends only on
    src-tauri/ (app/** is served from disk), so this is the yardstick for a
    stale binary. None when git can't list the files."""
    listing = run_git(root, "ls-files", "-z", "src-tauri", check=False)
    newest: tuple[float, str] | None = None
    for rel in filter(None, listing.split("\0")):
        try:
            mtime = (root / rel).stat().st_mtime
        except OSError:
            continue  # deleted in the worktree
        if newest is None or mtime > newest[0]:
            newest = (mtime, rel)
    return newest


def assert_binary_current(binary: Path) -> None:
    """Refuse evidence from a debug binary older than its Rust sources.

    The test used to be "older than HEAD's commit time", which was wrong both
    ways: any commit tripped it, scripts- and docs-only ones included, and an
    uncommitted Rust edit, which never moves HEAD, slipped through. Comparing
    against the newest tracked file under src-tauri/ catches committed and
    uncommitted Rust changes and ignores everything else."""
    newest = newest_rust_source(SOURCE_ROOT)
    if newest is None:
        return
    newest_mtime, newest_path = newest
    if binary.stat().st_mtime < newest_mtime:
        changed = time.strftime("%Y-%m-%d %H:%M", time.localtime(newest_mtime))
        raise DemoError(
            f"stale Bram binary: {binary} is older than {newest_path} "
            f"(changed {changed}); rebuild with cargo build"
        )


def _process_alive(pid: int) -> bool:
    """Whether `pid` names a live process.

    True when the process exists, including when it exists but cannot be
    queried; False when it does not. Both branches keep that contract, because
    callers treat False as "safe to proceed without stopping anything".

    demo-instance-liveness-on-windows: signal 0 has no meaning on Windows, so
    the POSIX probe below raised OSError [WinError 87] there — neither of the
    exceptions it catches — and propagated. Both `stop` and `launch` call this
    before doing anything useful, so each died before acting; `stop` failed
    *without stopping anything*, which read as a broken verb rather than a
    broken check. The rest of the file already branches on os.name (the SIGKILL
    escalation in _stop_recorded is guarded that way for the same reason); this
    call site was missed.
    """
    if os.name == "nt":
        import ctypes

        PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
        STILL_ACTIVE = 259
        ERROR_ACCESS_DENIED = 5

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
        if not handle:
            # Exists but not queryable → alive, matching the PermissionError
            # arm below. Any other failure (notably ERROR_INVALID_PARAMETER,
            # which is what a dead PID gives) means gone.
            return ctypes.get_last_error() == ERROR_ACCESS_DENIED
        try:
            code = ctypes.c_ulong()
            if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
                return True
            # A process that exited with code 259 is indistinguishable from a
            # running one here — the standard STILL_ACTIVE wart. For a launcher
            # of one known binary that is not worth a WaitForSingleObject call;
            # swap to that if a real 259 ever shows up.
            return code.value == STILL_ACTIVE
        finally:
            kernel32.CloseHandle(handle)

    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def launch_demo(repo: Path, binary: Path) -> dict[str, Any]:
    repo = validate_demo(repo)
    binary = binary.expanduser().absolute()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise DemoError(f"Bram binary is missing or not executable: {binary}")
    assert_binary_current(binary)
    runtime = _runtime_root(repo)
    pid_path = runtime / "process.json"
    if pid_path.exists():
        prior = read_json(pid_path)
        prior_pid = int(prior.get("pid", 0))
        if prior_pid and _process_alive(prior_pid):
            raise DemoError(f"demo Bram is already running as PID {prior_pid}")
        pid_path.unlink()
    log_path = runtime / "bram.log"
    log = log_path.open("ab", buffering=0)
    flags = 0
    if os.name == "nt":
        flags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    try:
        proc = subprocess.Popen(
            [str(binary), str(repo)],
            cwd=str(SOURCE_ROOT),
            env=scrub_agent_environment(os.environ),
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=(os.name != "nt"),
            creationflags=flags,
        )
    finally:
        log.close()
    time.sleep(0.15)
    code = proc.poll()
    if code is not None:
        tail = log_path.read_text(encoding="utf-8", errors="replace")[-2000:]
        raise DemoError(f"Bram exited during launch ({code}); log tail:\n{tail}")
    record = {
        "binary": str(binary),
        "log": str(log_path),
        "pid": proc.pid,
        "repo": str(repo),
        "startedAtMs": int(time.time() * 1000),
    }
    _LAUNCHED_PROCESSES[proc.pid] = proc
    write_json(pid_path, record)
    return record


def stop_demo(repo: Path, timeout: float = 5.0) -> int:
    repo = validate_demo(repo)
    pid_path = _runtime_root(repo) / "process.json"
    if not pid_path.exists():
        raise DemoError("no recorded demo process")
    record = read_json(pid_path)
    if Path(str(record.get("repo", ""))).resolve() != repo:
        raise DemoError("recorded process belongs to a different repository")
    pid = int(record.get("pid", 0))
    if pid <= 0:
        raise DemoError("invalid recorded demo PID")
    proc = _LAUNCHED_PROCESSES.pop(pid, None)
    if _process_alive(pid):
        try:
            os.kill(pid, signal.SIGTERM)
        except PermissionError as exc:
            raise DemoError(f"permission denied stopping recorded PID {pid}") from exc
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if proc is not None and proc.poll() is not None:
                break
            if proc is None and not _process_alive(pid):
                break
            time.sleep(0.05)
        still_alive = proc.poll() is None if proc is not None else _process_alive(pid)
        if still_alive and os.name != "nt":
            try:
                os.kill(pid, signal.SIGKILL)
            except PermissionError as exc:
                raise DemoError(f"permission denied force-stopping recorded PID {pid}") from exc
        if proc is not None:
            with contextlib.suppress(subprocess.TimeoutExpired):
                proc.wait(timeout=1)
    pid_path.unlink(missing_ok=True)
    return pid


def status(repo: Path) -> dict[str, Any]:
    repo = validate_demo(repo)
    runtime = _runtime_root(repo)
    active_path = runtime / "active.json"
    active = read_json(active_path) if active_path.exists() else {}
    branches = run_git(
        repo, "for-each-ref", "--format=%(refname:strip=3)", "refs/heads/scenario"
    ).splitlines()
    refs = run_git(
        repo, "for-each-ref", "--format=%(refname) %(objectname)", "refs/bram/claims"
    ).splitlines()
    worklist = read_json(repo / WORKLIST) if (repo / WORKLIST).exists() else {"items": []}
    process_path = runtime / "process.json"
    process = read_json(process_path) if process_path.exists() else {}
    pid = int(process.get("pid", 0) or 0)
    return {
        "activeScenario": active.get("name"),
        "claimRefs": refs,
        "dirty": run_git(repo, "status", "--short").splitlines(),
        "items": len(worklist.get("items", [])),
        "log": str(runtime / "bram.log"),
        "pid": pid if pid and _process_alive(pid) else None,
        "repo": str(repo),
        "scenarios": branches,
    }


def prune_scenarios(
    repo: Path, matches: Sequence[str], keep: int, dry_run: bool, gc: bool
) -> tuple[list[str], list[str]]:
    """Remove scenario branches matching any glob in `matches`, keeping the
    newest `keep` of them and never the active scenario. Returns (removed,
    skipped). "Newest" is by name, not commit date: scenario commits carry
    fixed dates (`_fixed_git_env`) so their trees are reproducible, and the
    starter check's `check-YYYYMMDD-HHMMSS` names sort in time order."""
    repo = validate_demo(repo)
    active_path = _runtime_root(repo) / "active.json"
    active = (read_json(active_path) if active_path.exists() else {}).get("name")
    names = run_git(
        repo, "for-each-ref", "--format=%(refname:strip=3)", "refs/heads/scenario"
    ).splitlines()
    matched = sorted(
        (n for n in names if any(fnmatch.fnmatchcase(n, m) for m in matches)), reverse=True
    )
    removed: list[str] = []
    skipped: list[str] = []
    for i, name in enumerate(matched):
        if name == active:
            skipped.append(f"{name} (active)")
            continue
        if i < keep:
            skipped.append(f"{name} (kept)")
            continue
        if not dry_run:
            run_git(repo, "branch", "-D", f"scenario/{name}")
        removed.append(name)
    if gc and not dry_run:
        run_git(repo, "gc", "--prune=now", "--quiet")
    return removed, skipped


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo",
        type=Path,
        default=Path(os.environ.get("BRAM_DEMO_REPO", DEFAULT_REPO)),
        help=f"synthetic repository (default: {DEFAULT_REPO})",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    new = sub.add_parser("new", help="create and hydrate a scenario")
    new.add_argument("name")
    new.add_argument("--from", dest="starters", action="append", choices=(*STARTERS, "all"))
    new.add_argument("--force", action="store_true")

    use = sub.add_parser("use", help="hydrate a parked scenario exactly")
    use.add_argument("name")

    capture = sub.add_parser("capture", help="park the current stumbled-into state")
    capture.add_argument("name")
    capture.add_argument("--force", action="store_true")

    sub.add_parser("reset", help="rehydrate the active scenario")

    add = sub.add_parser("add", help="add a starter to the live dirty fixture")
    add.add_argument("starter", choices=STARTERS)

    launch = sub.add_parser("launch", help="launch the real Bram and record its exact PID")
    launch.add_argument(
        "--binary",
        type=Path,
        default=Path(os.environ.get("BRAM_DEMO_DEFAULT_BINARY", SOURCE_ROOT / "bram")),
    )

    stop = sub.add_parser("stop", help="stop only the recorded demo PID")
    stop.add_argument("--timeout", type=float, default=5.0)

    status_parser = sub.add_parser("status", help="show scenarios and live fixture state")
    status_parser.add_argument("--json", action="store_true")

    prune = sub.add_parser(
        "prune",
        help="remove scenario branches (default: the starter check's check-* boards)",
    )
    prune.add_argument(
        "--match",
        action="append",
        help="glob over scenario names; repeatable (default: check-*)",
    )
    prune.add_argument(
        "--keep", type=int, default=0, help="keep the newest N matches, newest by name"
    )
    prune.add_argument("--dry-run", action="store_true", help="list, change nothing")
    prune.add_argument(
        "--gc", action="store_true", help="then git gc --prune=now to reclaim the space"
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "new":
            tip = new_scenario(args.repo, args.name, args.starters or ["all"], args.force)
            print(f"created and hydrated scenario/{args.name} ({tip[:12]})")
        elif args.command == "use":
            use_scenario(args.repo, args.name)
            print(f"hydrated scenario/{args.name}")
        elif args.command == "capture":
            tip = capture_scenario(args.repo, args.name, args.force)
            print(f"captured and hydrated scenario/{args.name} ({tip[:12]})")
        elif args.command == "reset":
            print(f"reset scenario/{reset_scenario(args.repo)}")
        elif args.command == "add":
            add_starter(args.repo, args.starter)
            print(f"added {args.starter}; capture it to make it durable")
        elif args.command == "launch":
            record = launch_demo(args.repo, args.binary)
            print(f"launched PID {record['pid']}")
            print(f"log: {record['log']}")
        elif args.command == "stop":
            print(f"stopped PID {stop_demo(args.repo, args.timeout)}")
        elif args.command == "status":
            value = status(args.repo)
            if args.json:
                print(json.dumps(value, indent=2, sort_keys=True))
            else:
                print(f"repo: {value['repo']}")
                print(f"active: {value['activeScenario'] or '-'}")
                print(f"scenarios: {', '.join(value['scenarios']) or '-'}")
                print(f"items: {value['items']}  claims: {len(value['claimRefs'])}")
                print(f"dirty paths: {len(value['dirty'])}  pid: {value['pid'] or '-'}")
                print(f"log: {value['log']}")
        elif args.command == "prune":
            removed, skipped = prune_scenarios(
                args.repo, args.match or ["check-*"], args.keep, args.dry_run, args.gc
            )
            verb = "would remove" if args.dry_run else "removed"
            for name in removed:
                print(f"{verb} scenario/{name}")
            for note in skipped:
                print(f"kept scenario/{note}")
            print(f"{verb} {len(removed)} scenario(s)" + (" and ran gc" if args.gc and not args.dry_run else ""))
        return 0
    except DemoError as exc:
        print(f"demo-instance: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
