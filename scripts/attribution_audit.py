#!/usr/bin/env python3
"""Read-only audit of Bram's Worklist line attribution.

Bram's Worklist attributes changed lines to worklist items. Three write-ups on
judell/bram#273 and #419 (2026-10-02) rest on counts produced by throwaway
scripts: the gate adjudication
(https://github.com/judell/bram/issues/419#issuecomment-5959612019), the ruling
on 19 observations
(https://github.com/judell/bram/issues/273#issuecomment-5961109674) and its
follow-up (https://github.com/judell/bram/issues/273#issuecomment-5961457006).
This tool regenerates those counts from the trace log and the boundary trees.
Use it for the soak of the line rule (commit 93830b1), for other repos, and for
re-checking a claim.

Subcommands (all take --trace-dir, --since, --until, --repo and --json):

  gate                    one row per gate attempt, divergences classified
  windows <sha-or-ts>     per-path churn by window owner for one gate commit
  lines [--sha S]         the diff-following line rule over gate commits
  probe <sha>             the superseded reverse-apply engine, for comparison

Things to know:

* Boundary trees. Every claim capture names a tree. The refs that pinned them
  are pruned after a commit, so the trees survive only until `git gc`. A step
  whose tree is gone is reported as missing; the tool never guesses.
* `windows` counts are per-window churn (lines added plus removed between two
  consecutive boundary trees), not surviving lines. A line rewritten three times
  counts three times. `lines` follows each line through the windows and counts
  only what survives into the commit's diff.
* Rotated trace files overlap, and the newest archives can be truncated. Lines
  are de-duplicated before anything is counted (one earlier count was wrong for
  exactly that reason), and a truncated .gz yields whatever decompressed.
* Window ownership: the work between capture A and the next capture belongs to
  the holder of A (the ids on the next inflight-sentinel write after A; a
  `cleared` capture means nobody). The committer is the holder of the last
  claim capture before the commit.
* The `lines` rule is the one in `membership_line_credit` (src-tauri/src/lib.rs),
  including the restart of a path's history at any step whose new blob equals
  the path's HEAD blob.

Strictly read-only: no refs, objects or index of the repo are touched. The
`probe` command applies patches only to a scratch index in a temporary
directory, with a temporary object directory layered over the repo's.
"""

from __future__ import annotations

import argparse
import collections
import glob
import gzip
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import zlib
from typing import Any, Iterable, Sequence

DEFAULT_TRACE_DIR = "resources/bram-traces"
BUCKETS = ("own", "other", "unclaimed", "before", "unexplained")
CLASSES = ("outside-named", "committer-uncredited", "membership-extra")

LINE_RE = re.compile(r"^\[(\S+)\] \[([\w-]+)\] (.*)$")
PREFILTER = (
    "[claim-interval] op=capture ",
    "[claim-interval] op=gate-membership",
    "[inflight-sentinel] op=write",
    "[worklist-commit] op=",
)
CAPTURE_RE = re.compile(r"op=capture ref=(\S+) tree=(\w+) kind=(\w+) ids=(\d+)")
SENTINEL_RE = re.compile(r"op=write kind=\w+ ids=\[(.*?)\]")
GATE_RE = re.compile(
    r"op=gate-membership ids=(\d+) paths=(\d+) ms=(\d+) fresh=(\w+) agrees=(\w+)"
)
UNAVAIL_RE = re.compile(r"op=gate-membership-unavailable ids=(\d+)(?: ms=(\d+))?")
DIVERGE_RE = re.compile(
    r"op=gate-membership-diverges ids=(\S+) path=(\S+) replay=(\S+) membership=(\S+)"
)
COMMIT_RE = re.compile(r"op=(committed|interval-staged) sha=(\w+)")
HUNK_RE = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")
ZEROS_RE = re.compile(r"^0+$")


# ---------------------------------------------------------------- git helpers


class Git:
    """Thin read-only wrapper around the git CLI for one repository."""

    def __init__(self, repo: str = ".") -> None:
        self.repo = repo
        self._tree_cache: dict[str, bool] = {}

    def run(
        self,
        *args: str,
        env: dict[str, str] | None = None,
        input: str | None = None,
    ) -> str | None:
        # Bytes both ways, never text mode: on Windows text mode turns every
        # "\n" written to stdin into "\r\n" (subprocess docs, "Frequently
        # Used Arguments"), so a patch handed to `git apply` no longer
        # matches and probe reversed nothing (v0.7.3 release build).
        proc = subprocess.run(
            ["git", "-c", "core.quotePath=false", "-C", self.repo, *args],
            capture_output=True,
            input=input.encode("utf-8") if input is not None else None,
            env=env,
        )
        if proc.returncode != 0:
            return None
        return proc.stdout.decode("utf-8", errors="replace")

    def has_tree(self, tree: str) -> bool:
        if tree not in self._tree_cache:
            out = self.run("cat-file", "-t", tree)
            self._tree_cache[tree] = out is not None and out.strip() == "tree"
        return self._tree_cache[tree]

    def parent_of(self, sha: str) -> str | None:
        out = self.run("rev-parse", "--verify", "--quiet", sha + "^")
        return out.strip() if out else None

    def commit_paths(self, sha: str) -> list[str]:
        out = self.run(
            "diff-tree", "--no-commit-id", "--name-only", "-r", "--no-renames", sha
        )
        return out.split("\n")[:-1] if out else []

    def numstat(self, a: str, b: str, paths: Sequence[str]) -> dict[str, tuple[int, int]]:
        out = self.run("diff", "--numstat", "--no-renames", a, b, "--", *paths)
        result: dict[str, tuple[int, int]] = {}
        for row in (out or "").splitlines():
            parts = row.split("\t", 2)
            if len(parts) == 3:
                add = int(parts[0]) if parts[0].isdigit() else 0
                rem = int(parts[1]) if parts[1].isdigit() else 0
                result[parts[2]] = (add, rem)
        return result


# --------------------------------------------------------------- trace parsing


def read_trace_file(path: str) -> str:
    """Text of one trace file; a truncated .gz yields what decompressed."""
    if not path.endswith(".gz"):
        with open(path, "rb") as fh:
            return fh.read().decode("utf-8", errors="replace")
    with open(path, "rb") as fh:
        raw = fh.read()
    decomp = zlib.decompressobj(wbits=31)
    out = bytearray()
    try:
        for i in range(0, len(raw), 65536):
            out += decomp.decompress(raw[i : i + 65536])
    except zlib.error:
        pass
    return bytes(out).decode("utf-8", errors="replace")


def trace_files(trace_dir: str) -> list[str]:
    files = glob.glob(os.path.join(trace_dir, "bram-trace*.log"))
    files += glob.glob(os.path.join(trace_dir, "bram-trace*.log.gz"))
    return sorted(files)


class Trace:
    """De-duplicated, timestamp-sorted claim/gate/commit events."""

    def __init__(self, trace_dir: str) -> None:
        events: list[dict[str, Any]] = []
        seen: set[str] = set()
        for path in trace_files(trace_dir):
            for line in read_trace_file(path).split("\n"):
                if not line.startswith("[") or not any(p in line for p in PREFILTER):
                    continue
                if line in seen:
                    continue
                m = LINE_RE.match(line)
                if not m:
                    continue
                seen.add(line)
                events.append(
                    {"ts": m.group(1), "tag": m.group(2), "body": m.group(3)}
                )
        events.sort(key=lambda e: e["ts"])  # stable: file order breaks ties
        self.events = events
        self.captures: list[dict[str, Any]] = []
        self.gates: list[dict[str, Any]] = []
        self.diverges: dict[str, list[dict[str, Any]]] = collections.defaultdict(list)
        self.commits: list[dict[str, Any]] = []
        self.outcome_events: list[dict[str, Any]] = []
        self._parse()

    def _parse(self) -> None:
        for ev in self.events:
            ts, tag, body = ev["ts"], ev["tag"], ev["body"]
            if tag == "claim-interval":
                m = CAPTURE_RE.search(body)
                if m:
                    cleared = m.group(3) == "cleared"
                    self.captures.append(
                        {
                            "ts": ts,
                            "ref": m.group(1),
                            "tree": m.group(2),
                            "kind": m.group(3),
                            "ids": [] if cleared else None,
                        }
                    )
                    continue
                m = DIVERGE_RE.search(body)
                if m:
                    self.diverges[ts].append(
                        {
                            "ids": m.group(1).split(","),
                            "path": m.group(2),
                            "replay": _idset(m.group(3)),
                            "membership": _idset(m.group(4)),
                        }
                    )
                    continue
                m = GATE_RE.search(body)
                if m:
                    self.gates.append(
                        {
                            "ts": ts,
                            "paths": int(m.group(2)),
                            "ms": int(m.group(3)),
                            "fresh": m.group(4) == "true",
                            "agrees": m.group(5) == "true",
                        }
                    )
                    continue
                m = UNAVAIL_RE.search(body)
                if m:
                    self.gates.append(
                        {
                            "ts": ts,
                            "paths": None,
                            "ms": int(m.group(2)) if m.group(2) else None,
                            "fresh": None,
                            "agrees": None,
                        }
                    )
            elif tag == "inflight-sentinel":
                m = SENTINEL_RE.search(body)
                if m and self.captures and self.captures[-1]["ids"] is None:
                    self.captures[-1]["ids"] = _quoted_ids(m.group(1))
            elif tag == "worklist-commit":
                self.outcome_events.append(ev)
                m = COMMIT_RE.search(body)
                if m and all(c["sha"] != m.group(2) for c in self.commits):
                    self.commits.append(
                        {"ts": ts, "sha": m.group(2), "kind": m.group(1)}
                    )

    # -- queries

    def holder_before(self, ts: str) -> dict[str, Any] | None:
        """The last capture before ts that holds ids (a claim)."""
        found = None
        for cap in self.captures:
            if cap["ts"] >= ts:
                break
            if cap["ids"]:
                found = cap
        return found

    def committer(self, ts: str) -> tuple[set[str], int] | None:
        """(committer ids, index of the first capture they claimed) for a commit."""
        holder = self.holder_before(ts)
        if not holder:
            return None
        ids = set(holder["ids"])
        for i, cap in enumerate(self.captures):
            if cap["ts"] >= ts:
                break
            if cap["ids"] and set(cap["ids"]) & ids:
                return ids, i
        return None

    def boundary_chain(self, start: int, ts: str) -> list[tuple[str, tuple]]:
        """Oldest-first (tree, label) steps from the committer's first claim.

        The first step (HEAD -> the first claimed boundary) is labelled
        ('before',). Later, the work between capture a and the next capture b
        is labelled by a's holder: ('held', ids) or ('unclaimed',).
        """
        caps = self.captures
        seq: list[tuple[str, tuple]] = [(caps[start]["tree"], ("before",))]
        j = start
        while j + 1 < len(caps) and caps[j + 1]["ts"] < ts:
            a, b = caps[j], caps[j + 1]
            if b["tree"] != seq[-1][0]:
                if a["ids"]:
                    seq.append((b["tree"], ("held", frozenset(a["ids"]))))
                else:
                    seq.append((b["tree"], ("unclaimed",)))
            j += 1
        return seq

    def in_range(self, ts: str, since: str | None, until: str | None) -> bool:
        return (since is None or ts >= since) and (until is None or ts < until)


def _idset(text: str) -> set[str]:
    return set() if text == "-" else set(text.split(","))


def _quoted_ids(text: str) -> list[str]:
    try:
        return [str(x) for x in json.loads("[" + text + "]")]
    except ValueError:
        return [x.strip().strip('"') for x in text.split(",") if x.strip()]


def bucket(label: tuple, ids: set[str]) -> str:
    if label[0] == "before":
        return "before"
    if label[0] == "unclaimed":
        return "unclaimed"
    return "own" if label[1] & ids else "other"


# ----------------------------------------------------------------------- gate


def classify(requested: set[str], replay: set[str], membership: set[str]) -> list[str]:
    classes = []
    if (replay - requested) - membership:
        classes.append("outside-named")
    if (replay & requested) - membership:
        classes.append("committer-uncredited")
    if membership - replay:
        classes.append("membership-extra")
    return classes


def gate_outcome(trace: Trace, ts: str, next_ts: str | None) -> str:
    for ev in trace.outcome_events:
        if ev["ts"] < ts:
            continue
        if next_ts is not None and ev["ts"] >= next_ts:
            break
        m = COMMIT_RE.search(ev["body"])
        if m:
            return m.group(2)
        m = re.search(r"op=(refuse[\w-]*)", ev["body"])
        if m:
            return "refused:" + m.group(1)
    return "unknown"


def cmd_gate(trace: Trace, args: argparse.Namespace) -> dict[str, Any]:
    rows = []
    gates = trace.gates
    for i, gate in enumerate(gates):
        ts = gate["ts"]
        next_ts = gates[i + 1]["ts"] if i + 1 < len(gates) else None
        if not trace.in_range(ts, args.since, args.until):
            continue
        divs = trace.diverges.get(ts, [])
        if divs:
            requested = set(divs[0]["ids"])
        else:
            holder = trace.holder_before(ts)
            requested = set(holder["ids"]) if holder else set()
        divergences = []
        for d in divs:
            divergences.append(
                {
                    "path": d["path"],
                    "replay": sorted(d["replay"]),
                    "membership": sorted(d["membership"]),
                    "classes": classify(set(d["ids"]), d["replay"], d["membership"]),
                }
            )
        rows.append(
            {
                "ts": ts,
                "ids": sorted(requested),
                "ms": gate["ms"],
                "agrees": gate["agrees"],
                "outcome": gate_outcome(trace, ts, next_ts),
                "divergences": divergences,
            }
        )
    class_counts = collections.Counter()
    outcome_counts = collections.Counter()
    for row in rows:
        outcome = row["outcome"]
        outcome_counts["refused" if outcome.startswith("refused") else outcome if outcome == "unknown" else "committed"] += 1
        for d in row["divergences"]:
            class_counts.update(d["classes"])
    agree = collections.Counter(
        "unavailable" if r["agrees"] is None else "agreed" if r["agrees"] else "disagreed"
        for r in rows
    )
    return {
        "rows": rows,
        "summary": {
            "attempts": len(rows),
            "agreement": dict(agree),
            "classes": {c: class_counts.get(c, 0) for c in CLASSES},
            "outcomes": dict(outcome_counts),
        },
    }


def print_gate(result: dict[str, Any]) -> None:
    for row in result["rows"]:
        agrees = "n/a" if row["agrees"] is None else str(row["agrees"]).lower()
        print(
            f"{row['ts']}  ids={','.join(row['ids']) or '-'}  ms={row['ms']}  "
            f"agrees={agrees}  outcome={row['outcome']}"
        )
        for d in row["divergences"]:
            print(
                f"    {','.join(d['classes']) or 'none':<22} {d['path']}  "
                f"replay={','.join(d['replay']) or '-'}  "
                f"membership={','.join(d['membership']) or '-'}"
            )
    s = result["summary"]
    print()
    print(f"gate attempts: {s['attempts']}")
    for key, n in sorted(s["agreement"].items()):
        print(f"  {key:<24} {n}")
    print("divergent paths by class:")
    for key, n in s["classes"].items():
        print(f"  {key:<24} {n}")
    print("outcomes:")
    for key, n in sorted(s["outcomes"].items()):
        print(f"  {key:<24} {n}")


# -------------------------------------------------------------------- windows


def find_commit(trace: Trace, key: str) -> dict[str, Any] | None:
    for c in trace.commits:
        if c["sha"].startswith(key) or key.startswith(c["sha"]):
            return c
    for c in trace.commits:
        if c["ts"].startswith(key):
            return c
    return None


def cmd_windows(trace: Trace, git: Git, args: argparse.Namespace) -> dict[str, Any]:
    commit = find_commit(trace, args.target)
    if not commit:
        return {"error": f"no gate commit matches {args.target!r} in the trace"}
    who = trace.committer(commit["ts"])
    if not who:
        return {"error": "no claim capture by the committer before this commit", "sha": commit["sha"]}
    ids, start = who
    paths = args.path or git.commit_paths(commit["sha"])
    caps = trace.captures
    totals = {p: {"own": 0, "other": 0, "unclaimed": 0} for p in paths}
    detail: dict[str, list] = {p: [] for p in paths}
    missing: list[str] = []
    for a, b in zip(caps[start:], caps[start + 1 :]):
        if b["ts"] >= commit["ts"]:
            break
        if a["tree"] == b["tree"]:
            continue
        if not (git.has_tree(a["tree"]) and git.has_tree(b["tree"])):
            for t in (a["tree"], b["tree"]):
                if not git.has_tree(t) and t not in missing:
                    missing.append(t)
            continue
        stats = git.numstat(a["tree"], b["tree"], paths)
        owner = a["ids"]
        if owner and set(owner) & ids:
            kind = "own"
        elif owner:
            kind = "other"
        else:
            kind = "unclaimed"
        for path, (add, rem) in stats.items():
            if path in totals:
                totals[path][kind] += add + rem
                detail[path].append(
                    {"window": kind, "holder": owner or [], "from": a["tree"], "to": b["tree"], "lines": add + rem}
                )
    return {
        "sha": commit["sha"],
        "ts": commit["ts"],
        "committer": sorted(ids),
        "first_claim_ts": caps[start]["ts"],
        "paths": [{"path": p, **totals[p], "windows": detail[p]} for p in paths],
        "missing_trees": missing,
    }


def print_windows(result: dict[str, Any]) -> None:
    if "error" in result:
        print("error:", result["error"])
        return
    print(
        f"commit {result['sha']} at {result['ts']}  committer={','.join(result['committer'])}  "
        f"first claim {result['first_claim_ts']}"
    )
    print(f"{'path':<50} {'own':>6} {'other':>6} {'unclaimed':>9}")
    for row in result["paths"]:
        print(f"{row['path']:<50} {row['own']:>6} {row['other']:>6} {row['unclaimed']:>9}")
    if result["missing_trees"]:
        print("MISSING TREES (windows touching them were not counted):", " ".join(result["missing_trees"]))


# ---------------------------------------------------------------------- lines


def parse_line_diff(text: str) -> list[dict[str, Any]]:
    """Parse `git diff -U0 --full-index` into per-file blob ids and hunks."""
    files: list[dict[str, Any]] = []
    cur: dict[str, Any] | None = None
    for line in text.split("\n"):
        if line.startswith("diff --git "):
            m = re.match(r"^diff --git a/(.+) b/(.+)$", line)
            cur = (
                {"path": m.group(2), "old": "", "new": "", "hunks": []} if m else None
            )
            if cur:
                files.append(cur)
        elif cur is not None and line.startswith("index "):
            m = re.match(r"^index (\w+)\.\.(\w+)", line)
            if m:
                cur["old"], cur["new"] = m.group(1), m.group(2)
        elif cur is not None:
            m = HUNK_RE.match(line)
            if m:
                cur["hunks"].append(
                    (
                        int(m.group(1)),
                        int(m.group(2)) if m.group(2) is not None else 1,
                        int(m.group(3)),
                        int(m.group(4)) if m.group(4) is not None else 1,
                    )
                )
    return files


def _blob_lines(git: Git, head: str, path: str) -> int:
    proc = subprocess.run(
        ["git", "-C", git.repo, "cat-file", "blob", f"{head}:{path}"],
        capture_output=True,
    )
    if proc.returncode != 0:
        return 0
    data = proc.stdout
    return data.count(b"\n") + (0 if data == b"" or data.endswith(b"\n") else 1)


def propagate(
    git: Git,
    head: str,
    seq: list[tuple[str, tuple]],
    paths: list[str],
) -> dict[str, tuple[collections.Counter, collections.Counter]] | None:
    """Follow every line of `paths` from `head` through `seq` (oldest first).

    Returns per path (added by step label index, removed by step label index);
    index -1 means unexplained. Port of `membership_line_credit`.
    """
    head_lines = {p: _blob_lines(git, head, p) for p in paths}
    own = {p: [("H", i) for i in range(1, head_lines[p] + 1)] for p in paths}
    removed_by: dict[str, dict[int, int]] = {p: {} for p in paths}
    head_blob: dict[str, str] = {}
    prev = head
    for k, (tree, _label) in enumerate(seq):
        if tree == prev:
            continue
        text = git.run("diff", "-U0", "--no-renames", "--full-index", prev, tree, "--", *paths)
        if text is None:
            return None
        for f in parse_line_diff(text):
            p = f["path"]
            if p not in own:
                continue
            head_blob.setdefault(p, f["old"])
            lines = own[p]
            for a, b, _c, d in sorted(f["hunks"], reverse=True):
                idx = a - 1 if b > 0 else a
                for entry in lines[idx : idx + b]:
                    if entry[0] == "H":
                        removed_by[p][entry[1]] = k
                lines[idx : idx + b] = [("W", k)] * d
            new = f["new"]
            if new and not ZEROS_RE.match(new) and head_blob.get(p) == new:
                own[p] = [("H", i) for i in range(1, head_lines[p] + 1)]
                removed_by[p] = {}
        prev = tree
    text = git.run("diff", "-U0", "--no-renames", "--full-index", head, seq[-1][0], "--", *paths)
    if text is None:
        return None
    result = {p: (collections.Counter(), collections.Counter()) for p in paths}
    for f in parse_line_diff(text):
        p = f["path"]
        if p not in result:
            continue
        add, rem = result[p]
        for a, b, c, d in f["hunks"]:
            for j in range(c, c + d):
                entry = own[p][j - 1] if 0 <= j - 1 < len(own[p]) else ("?", None)
                add[entry[1] if entry[0] == "W" else -1] += 1
            for i in range(a, a + b):
                rem[removed_by[p].get(i, -1)] += 1
    return result


def cmd_lines(trace: Trace, git: Git, args: argparse.Namespace) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    skipped: list[dict[str, Any]] = []
    commits = trace.commits
    if args.sha:
        commits = [c for c in commits if c["sha"].startswith(args.sha) or args.sha.startswith(c["sha"])]
    else:
        commits = [c for c in commits if trace.in_range(c["ts"], args.since, args.until)]
    for commit in commits:
        sha, ts = commit["sha"], commit["ts"]
        who = trace.committer(ts)
        if not who:
            skipped.append({"sha": sha, "reason": "no-claim-before-commit"})
            continue
        ids, start = who
        parent = git.parent_of(sha)
        if not parent:
            skipped.append({"sha": sha, "reason": "commit-or-parent-not-in-repo"})
            continue
        seq = trace.boundary_chain(start, ts)
        missing = [t for t, _ in seq if not git.has_tree(t)]
        if missing:
            skipped.append({"sha": sha, "reason": "trees-missing", "trees": missing})
            continue
        paths = git.commit_paths(sha)
        credit = propagate(git, parent, seq, paths)
        if credit is None:
            skipped.append({"sha": sha, "reason": "git-diff-failed"})
            continue
        stats = git.numstat(parent, seq[-1][0], paths)
        for p in paths:
            add_raw, rem_raw = credit[p]
            add = dict.fromkeys(BUCKETS, 0)
            rem = dict.fromkeys(BUCKETS, 0)
            for target, raw in ((add, add_raw), (rem, rem_raw)):
                for k, n in raw.items():
                    target["unexplained" if k == -1 else bucket(seq[k][1], ids)] += n
            ns = stats.get(p, (0, 0))
            rows.append(
                {
                    "sha": sha,
                    "ts": ts,
                    "committer": sorted(ids),
                    "path": p,
                    "added": add,
                    "removed": rem,
                    "numstat": list(ns),
                    "conserved": sum(add.values()) == ns[0] and sum(rem.values()) == ns[1],
                }
            )
    totals = {
        "added": {b: sum(r["added"][b] for r in rows) for b in BUCKETS},
        "removed": {b: sum(r["removed"][b] for r in rows) for b in BUCKETS},
    }
    return {
        "rows": rows,
        "skipped": skipped,
        "summary": {
            "commits": len({r["sha"] for r in rows}),
            "path_rows": len(rows),
            "not_conserved": sum(1 for r in rows if not r["conserved"]),
            **totals,
        },
    }


def print_lines(result: dict[str, Any]) -> None:
    head = " ".join(f"{b[:5]:>6}" for b in BUCKETS)
    print(f"{'sha':<8} {'path':<44} {'':<3}{head}  numstat  ok")
    for r in result["rows"]:
        for side, label in (("added", "+"), ("removed", "-")):
            nums = " ".join(f"{r[side][b]:>6}" for b in BUCKETS)
            ns = r["numstat"][0 if side == "added" else 1]
            first = (r["sha"], r["path"]) if side == "added" else ("", "")
            print(
                f"{first[0]:<8} {first[1][-44:]:<44} {label:<3}{nums}  {ns:>7}  "
                f"{'' if side == 'added' else ('ok' if r['conserved'] else 'MISMATCH')}"
            )
    for s in result["skipped"]:
        extra = " " + " ".join(s["trees"]) if s.get("trees") else ""
        print(f"skipped {s['sha']}: {s['reason']}{extra}")
    s = result["summary"]
    print()
    print(
        f"{s['commits']} commits, {s['path_rows']} path rows, "
        f"{s['not_conserved']} rows where buckets do not sum to numstat"
    )
    for side in ("added", "removed"):
        parts = "  ".join(f"{b}={s[side][b]}" for b in BUCKETS)
        print(f"  {side:<8} {parts}")


# ---------------------------------------------------------------------- probe


class Scratch:
    """A scratch index plus a temporary object dir layered over the repo's."""

    def __init__(self, git: Git) -> None:
        self.git = git
        self.tmp = tempfile.TemporaryDirectory(prefix="attribution-audit-")
        real = git.run("rev-parse", "--path-format=absolute", "--git-path", "objects")
        objects = Path(self.tmp.name) / "objects"
        (objects / "info").mkdir(parents=True)
        (objects / "info" / "alternates").write_text((real or "").strip() + "\n")
        self.env = dict(os.environ)
        self.env["GIT_INDEX_FILE"] = str(Path(self.tmp.name) / "index")
        self.env["GIT_OBJECT_DIRECTORY"] = str(objects)

    def close(self) -> None:
        self.tmp.cleanup()

    def read_tree(self, tree: str) -> bool:
        return self.git.run("read-tree", tree, env=self.env) is not None

    def apply(self, patch: str, check: bool) -> bool:
        args = ["apply", "--cached", "--reverse"]
        if check:
            args.append("--check")
        return self.git.run(*args, "-", env=self.env, input=patch) is not None


def cmd_probe(trace: Trace, git: Git, args: argparse.Namespace) -> dict[str, Any]:
    commit = find_commit(trace, args.sha)
    if not commit:
        return {"error": f"no gate commit matches {args.sha!r} in the trace"}
    who = trace.committer(commit["ts"])
    if not who:
        return {"error": "no claim capture by the committer before this commit", "sha": commit["sha"]}
    ids, start = who
    caps = trace.captures
    boundary = None
    windows: list[tuple[str, str]] = []
    for a, b in zip(caps[start:], caps[start + 1 :]):
        if b["ts"] >= commit["ts"]:
            break
        boundary = b["tree"]
        if a["tree"] != b["tree"] and a["ids"] and set(a["ids"]) & ids:
            windows.append((a["tree"], b["tree"]))
    if boundary is None:
        return {"error": "no boundary tree before the commit", "sha": commit["sha"]}
    needed = {boundary, *(t for w in windows for t in w)}
    missing = sorted(t for t in needed if not git.has_tree(t))
    if missing:
        return {"error": "boundary trees missing", "sha": commit["sha"], "missing_trees": missing}
    paths = args.path or git.commit_paths(commit["sha"])
    scratch = Scratch(git)
    out_paths = []
    try:
        for path in paths:
            sections = []
            for a, b in windows:
                text = git.run("diff", "--no-renames", a, b, "--", path)
                if text:
                    sections.append({"from": a, "to": b, "patch": text})
            whole_ok = None
            first_fail = None
            ok_before_fail = 0
            if sections:
                scratch.read_tree(boundary)
                whole_ok = scratch.apply("".join(s["patch"] for s in sections), check=True)
                scratch.read_tree(boundary)
                for n, sec in enumerate(reversed(sections), start=1):
                    if scratch.apply(sec["patch"], check=False):
                        ok_before_fail += 1
                    else:
                        first_fail = {"newest_first_index": n, "from": sec["from"], "to": sec["to"]}
                        break
            out_paths.append(
                {
                    "path": path,
                    "sections": len(sections),
                    "whole_patch_reverses": whole_ok,
                    "sections_reversed_before_failure": ok_before_fail,
                    "first_failing_section": first_fail,
                }
            )
    finally:
        scratch.close()
    return {
        "sha": commit["sha"],
        "committer": sorted(ids),
        "boundary_tree": boundary,
        "paths": out_paths,
    }


def print_probe(result: dict[str, Any]) -> None:
    if "error" in result:
        print("error:", result["error"], " ".join(result.get("missing_trees", [])))
        return
    print(f"commit {result['sha']}  committer={','.join(result['committer'])}  boundary={result['boundary_tree']}")
    for r in result["paths"]:
        whole = {None: "n/a (no own sections)", True: "reverses", False: "FAILS"}[r["whole_patch_reverses"]]
        ff = r["first_failing_section"]
        seq = (
            "all sections reverse"
            if ff is None
            else f"section {ff['newest_first_index']} of {r['sections']} (newest first) fails: {ff['from']}..{ff['to']}"
        )
        print(f"{r['path']}\n   sections={r['sections']}  whole patch: {whole}\n   newest-first: {seq}")


# ------------------------------------------------------------------------ main


def build_parser() -> argparse.ArgumentParser:
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--trace-dir", default=DEFAULT_TRACE_DIR)
    common.add_argument("--repo", default=".", help="repository root (default: cwd)")
    common.add_argument("--since", help="keep events at or after this ISO timestamp prefix")
    common.add_argument("--until", help="keep events before this ISO timestamp prefix")
    common.add_argument("--json", action="store_true", help="machine-readable output")
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("gate", parents=[common], help="one row per gate attempt")
    p = sub.add_parser("windows", parents=[common], help="window churn for one gate commit")
    p.add_argument("target", help="commit sha or timestamp prefix")
    p.add_argument("--path", action="append", help="restrict to this path (repeatable)")
    p = sub.add_parser("lines", parents=[common], help="the line rule over gate commits")
    p.add_argument("--sha", help="one commit only")
    p = sub.add_parser("probe", parents=[common], help="the superseded reverse-apply test")
    p.add_argument("sha")
    p.add_argument("--path", action="append", help="restrict to this path (repeatable)")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    trace = Trace(args.trace_dir)
    git = Git(args.repo)
    if args.command == "gate":
        result, printer = cmd_gate(trace, args), print_gate
    elif args.command == "windows":
        result, printer = cmd_windows(trace, git, args), print_windows
    elif args.command == "lines":
        result, printer = cmd_lines(trace, git, args), print_lines
    else:
        result, printer = cmd_probe(trace, git, args), print_probe
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        printer(result)
    return 1 if isinstance(result, dict) and "error" in result else 0


if __name__ == "__main__":
    sys.exit(main())
