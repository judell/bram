#!/usr/bin/env python3
"""Recompute the #368 would-deny predicate retroactively from primary data.

judell/bram#368 asked whether a GENERAL chat turn (no `approved:`/`iterate:`
authorization live) should be DENIED from editing a file that belongs to a
"begun" worklist item. `trace_would_deny_unaddressed` in
`src-tauri/src/guard_policy.rs` was built to OBSERVE the answer instead of
guessing it: whenever a general-turn edit lands on a path some board item
already covers, and the live authorization record (`.worklist-authorization.json`)
does not itself cover that path, the guard emits a `would-deny` breadcrumb to
`resources/bram-traces/hook-events.log` and allows the edit anyway. #368 was
closed on 3 qualifying post-fix fires, all judged benign, concluding "do not
deny."

THAT CONCLUSION IS UNTESTED IN ONE SPECIFIC WAY: an empty (or near-empty)
dangerous pile is consistent with two different worlds -- "the dangerous case
does not occur" and "the observer cannot see it" -- and a grep cannot tell
them apart. `docs/developing-bram.md` names this trap explicitly: a
tripwire's zero and a dead instrument's zero read identically. The live
observer is also structurally thin evidence on its own terms even setting
that worry aside: it only exists post-fix (the pre-fix code, before commit
b64d81f, keyed on TURN ADDRESSING rather than authorization and produced an
unusably different signal -- see the b64d81f commit message), so its
usable history is a few days at roughly one fire per two days. A meaningful
sample is months away by construction.

WHY RECOMPUTE INSTEAD OF JUST READING THE OBSERVER'S OWN LOG:

The observer's OUTPUT (`would-deny` lines in hook-events.log) is exactly what
is in question -- if the observer is blind to the dangerous shape, its own
log cannot prove or disprove that blindness. But every input the observer's
decision is actually made from is durable, independent, primary data that
predates the observer and was recorded for other reasons:

  - every Edit/Write/MultiEdit tool call the agent ever made is in the Claude
    session JSONLs (~2,750 calls, each carrying a timestamp and a target path)
  - every worklist claim open/close is logged as an [inflight-sentinel] line
    in resources/bram-traces/bram-trace.log (live + rotated archives) --
    `op=write` / `op=rearm-at-turn-start` open, `op=clear` and its several
    named-id variants close
  - every worklist item's `begunAtMs` and declared `files`, at every point in
    time, are recoverable from the periodic full-board snapshots in
    resources/worklist-history/*.json (median ~4.8 minutes apart) plus the
    live resources/worklist.json for "now"

This script replays that primary data and asks the #368 question directly,
for every edit the agent ever made, not just the ones a live process happened
to be watching for. It reaches BEFORE b64d81f, where the real observer has no
usable signal at all, and it is falsifiable against the observer's own 3
known post-fix fires -- every run cross-checks its population against them
and prints "found"/"NOT FOUND" for each (see KNOWN_POST_FIX_FIRES below): if
this script does not recover them, that is a bug in this script, not
evidence about the predicate.

WHAT THIS SCRIPT DOES NOT DO: it does not replicate
`trace_would_deny_unaddressed` field-for-field. That function keys
specifically on `.worklist-authorization.json`, a file that is overwritten in
place and has no history -- there is nothing to replay retroactively. This
script substitutes the nearest available durable proxy for "authorization was
live": an open worklist CLAIM (the [inflight-sentinel] interval) covering the
SAME item that declares the edited path. A claim and an authorization are
related but not identical (a claim can outlive a consumed authorization by a
few log lines, an authorization can in principle exist a moment before its
claim is written) -- this is a deliberate, named approximation, not the exact
mechanism. See CAVEATS below and in-line comments at each classification
heuristic for where it can be wrong.

THE PREDICATE, as specified for this census (deliberately narrower than the
raw guard's own ALLOW gate, which additionally allows edits to files any
merely-PROPOSED, never-begun item happens to declare -- this script only
counts paths a BEGUN item declares, because that is the shape #368 actually
worried about):

    For each file edit: was the target path declared by a worklist item that
    was BEGUN (begunAtMs set and <= edit time) and not yet pruned (still
    present in the nearest board snapshot at-or-before edit time), at a
    moment when NO CLAIM covering that item's id was open?

CLASSIFICATION (the substantive output, and the part to audit, not trust):

  - same-turn-correction: the agent had already edited this same file earlier
    in the SAME turn (a turn starts at a genuine user message in the same
    session). This is "correcting code you just wrote", which
    app/__shell/conventions.md explicitly permits outside the worklist gate.
    CAN BE WRONG: it only looks one session back to the last genuine user
    message; a very short user message that is itself part of the same
    back-and-forth (voice transcription artifacts, a stray "yes") still
    counts as a fresh turn boundary by this heuristic, which could split what
    a human would call one continuous exchange into two turns and misclassify
    the second half as cold-resume.
  - cold-resume: the covering item's begunAtMs predates the START of the
    turn that produced this edit -- i.e. the work was begun in an earlier
    turn (possibly an earlier session entirely) and a later, unauthorized
    turn touched it with no claim live. This is the shape #368 worried about.
  - other: the covering item was begun THIS turn (so a claim WAS live earlier
    in the turn) but this specific edit is, by this script's reckoning, the
    first touch of that file in the turn with no claim open -- e.g. the claim
    closed (commit, advance) partway through a turn that then kept editing a
    declared-but-not-yet-touched file before the turn ended. Deliberately not
    forced into either bucket; a large `other` count is itself a finding
    about the classifier or about a genuine third shape, not a defect to hide.

Read-only, standard library only. Prints text or JSON; never writes.
"""

from __future__ import annotations

import argparse
import bisect
import datetime as dt
import gzip
import json
import re
import sys
from pathlib import Path, PurePosixPath

COMMIT_BOUNDARY = "2026-09-09T19:39:39Z"  # b64d81f: would-deny keys on authorization

KNOWN_POST_FIX_FIRES = (
    ("2026-09-12T03:57:34Z", "src-tauri/src/lib.rs"),
    ("2026-09-12T03:57:44Z", "src-tauri/src/lib.rs"),
    ("2026-09-12T05:06:59Z", "app/__shell/helpers.js"),
)
FIRE_MATCH_TOLERANCE_S = 5  # Edit-call ts vs would-deny-trace ts are concurrent, not identical

ENVELOPE = "<task-notification>"
EDIT_TOOLS = ("Edit", "Write", "MultiEdit")

TRACE_LINE = re.compile(
    r"^\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?)Z\]\s\[inflight-sentinel\]\s(op=\S+.*)$"
)
FIELD = re.compile(r"(\w+)=(\[[^\]]*\]|\S+)")

# Ops that OPEN a claim for the named ids (fresh start ts).
OPEN_OPS = {"write", "rearm-at-turn-start"}
# Ops that CLOSE specific named ids, field carrying the id list varies by op.
CLOSE_OPS = {
    "clear": "ids",
    "clear-at-turn-end": "ids",
    "clear-on-commit-refusal": "ids",
}
# op=clear-shrink closes only `resolved`, leaves `remaining` open untouched.
SHRINK_OP = "clear-shrink"
# op=stale-startup-clear carries no ids at all -- closes whatever is open.
FULL_CLEAR_OP = "stale-startup-clear"
# No-ops for interval purposes: a request/skip/refusal that changes nothing,
# or a diagnostic (`reconcile-no-claim`) whose semantics are "the host looked
# and found no claim for these ids" -- informational, not a state transition.
NOOP_OPS = {"skip-clear", "clear-request", "reconcile-no-claim", "clear-partial"}


def parse_ts(text) -> dt.datetime | None:
    """Parse an ISO timestamp, or a bare `YYYY-MM-DD` as that day's midnight."""
    if not isinstance(text, str):
        return None
    text = text.strip()
    if len(text) == 10:
        text += "T00:00:00"
    try:
        return dt.datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S").replace(
            tzinfo=dt.timezone.utc
        )
    except ValueError:
        return None


def to_ms(ts: dt.datetime) -> float:
    return ts.timestamp() * 1000.0


def session_dir_for(project_root: Path) -> Path:
    slug = str(project_root.resolve()).replace("/", "-")
    return Path("~/.claude/projects").expanduser() / slug


def open_log(path: Path):
    if path.suffix == ".gz":
        return gzip.open(path, "rt", errors="replace")
    return path.open("r", errors="replace")


def parse_field_value(raw: str):
    if raw.startswith("["):
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return []
    return raw.strip('"')


# --- claim intervals ---------------------------------------------------------


def read_claim_events(trace_dir: Path):
    """Every [inflight-sentinel] line across live + rotated logs, globally
    time-sorted (filenames like `bram-trace-2026-07-20-10.log.gz` do NOT sort
    correctly as strings against `-3.log.gz`, so file processing order is not
    trusted for anything -- every line is parsed independently and the whole
    set is sorted by its own embedded timestamp before replay).

    Returns (events, scanned, damaged). A truncated `.log.gz` raises EOFError
    partway through decompression; whatever lines were read before the break
    are kept and the archive is counted+named as damaged, never silently
    dropped -- a census that quietly drops archives misreports its own
    denominator.
    """
    events, scanned, damaged = [], 0, []
    for path in sorted(trace_dir.glob("*.log*")):
        if path.name.endswith(".tmp") or ".tmp-" in path.name:
            continue
        scanned += 1
        try:
            with open_log(path) as fh:
                for line in fh:
                    m = TRACE_LINE.match(line)
                    if not m:
                        continue
                    ts_text, rest = m.groups()
                    ts = parse_ts(ts_text)
                    if ts is None:
                        continue
                    op_match = re.match(r"op=([a-z-]+)", rest)
                    if not op_match:
                        continue  # truncated line, e.g. "op=wri" -- skip, not crash
                    op = op_match.group(1)
                    fields = {k: parse_field_value(v) for k, v in FIELD.findall(rest)}
                    events.append((to_ms(ts), op, fields, path.name))
        except (OSError, EOFError, gzip.BadGzipFile):
            damaged.append(path.name)
    events.sort(key=lambda e: e[0])
    return events, scanned, damaged


def build_claim_intervals(events, last_ts_ms: float):
    """Replay claim open/close events into per-id [(start_ms, end_ms_or_None, kind)].

    `kind` (approved / iterate / drop / direct, from `op=write kind=...`)
    matters as much as the interval bounds. The REAL guard check
    (`live_authorization_ids` in guard_policy.rs) only treats an `approved`
    authorization record as a suppressing grant -- an `iterate` (Refine)
    claim is open in the inflight-sentinel sense while a Refine turn edits an
    already-begun item's files, but it is NOT an "authorization" in the
    guard's vocabulary, and a would-deny fire found DURING an open `iterate`
    claim in the field data (2026-09-12T03:57:34Z/44Z, both under an
    `op=write kind=iterate` window) is what exposed this: treating "any
    claim open" as "authorized" produces a false suppression the real
    observer did not apply. `direct` (the `skip-worklist:` opt-out) is kept
    as suppressing too, as the closest available proxy for the guard's
    separate `fresh_bypass()` TTL check on `.worklist-direct-edit.json`,
    which this script has no retroactive access to. `drop` never suppresses.

    An id still open at the end of the observed log (no matching close ever
    seen) is recorded with end=None and reported as "unmatched-open" rather
    than silently treated either way. For containment checks such an interval
    is extended through `last_ts_ms` (the latest timestamp seen anywhere in
    the trace data) -- the conservative choice: an edit landing inside an
    unmatched-open, approved/direct span is treated as claim-covered (NOT
    flagged), which can only make the reported dangerous population an
    UNDERcount, never an overcount. The unmatched-open count is reported so
    this bound is visible rather than assumed.

    The sentinel file is SINGLE-SLOT (conventions.md: "writing a second
    [claim] overwrites the first"), so a fresh `op=write`/`op=rearm` can
    silently retire whatever was open before it, with no separate close event
    ever emitted for the retired ids. Confirmed in this data: distinct
    `drop`/`approved` writes for the SAME id 9-20s apart with no intervening
    close. Every write therefore closes (at its own timestamp) any
    currently-open id not part of the new write -- including a same-id
    "reopen", which closes the old span and starts a fresh one rather than
    silently discarding it (discarding would erase real approved/direct
    coverage and manufacture false would-deny positives in this script's
    own output).
    """
    open_since: dict[str, tuple[float, str]] = {}
    closed: dict[str, list[tuple[float, float | None, str]]] = {}
    anomalies = {
        "reopen-same-id": 0,
        "superseded-by-write": 0,
        "close-without-open": 0,
        "stale-clears": 0,
    }

    def retire(item_id: str, ts_ms: float, anomaly_key: str | None):
        start, kind = open_since.pop(item_id)
        closed.setdefault(item_id, []).append((start, ts_ms, kind))
        if anomaly_key:
            anomalies[anomaly_key] += 1

    def close_id(item_id: str, ts_ms: float):
        if item_id not in open_since:
            # Usually benign: the trace shows clear-at-turn-end and clear
            # firing back-to-back for the same ids (a redundant double
            # close), which lands here on the second call. Counted, not
            # treated as evidence of a tracking bug.
            anomalies["close-without-open"] += 1
            return
        retire(item_id, ts_ms, None)

    for ts_ms, op, fields, _src in events:
        if op in OPEN_OPS:
            ids = fields.get("ids", [])
            if isinstance(ids, str):
                ids = [ids]
            kind = fields.get("kind", "?")
            new_id_set = set(ids)
            for old_id in [i for i in open_since if i not in new_id_set]:
                retire(old_id, ts_ms, "superseded-by-write")
            for item_id in ids:
                if item_id in open_since:
                    retire(item_id, ts_ms, "reopen-same-id")
                open_since[item_id] = (ts_ms, kind)
        elif op in CLOSE_OPS:
            ids = fields.get(CLOSE_OPS[op], [])
            if isinstance(ids, str):
                ids = [ids]
            for item_id in ids:
                close_id(item_id, ts_ms)
        elif op == SHRINK_OP:
            resolved = fields.get("resolved", [])
            if isinstance(resolved, str):
                resolved = [resolved]
            for item_id in resolved:
                close_id(item_id, ts_ms)
            # `remaining` ids stay open untouched -- nothing to do for them.
        elif op == FULL_CLEAR_OP:
            for item_id in list(open_since.keys()):
                close_id(item_id, ts_ms)
                anomalies["stale-clears"] += 1
        elif op in NOOP_OPS:
            continue
        # any other/unrecognized op: ignore

    unmatched = dict(open_since)
    for item_id, (start, kind) in unmatched.items():
        closed.setdefault(item_id, []).append((start, None, kind))
    return closed, anomalies, len(unmatched)


AUTHORIZING_KINDS = {"approved", "direct"}


def claim_covers(intervals: dict, item_id: str, ts_ms: float, last_ts_ms: float) -> bool:
    """True if an AUTHORIZING claim (kind approved or direct -- see
    build_claim_intervals docstring) covering item_id was open at ts_ms.
    An `iterate` or `drop` claim being open does not suppress."""
    for start, end, kind in intervals.get(item_id, ()):
        if kind not in AUTHORIZING_KINDS:
            continue
        hi = last_ts_ms if end is None else end
        if start <= ts_ms < hi:
            return True
    return False


# --- worklist item snapshots --------------------------------------------------


def items_from_doc(doc) -> dict:
    """id -> {files: frozenset[str], begunAtMs: float|None} for one worklist
    (or worklist-history) document. `file` (singular, legacy) and `files`
    (list) are merged; directory entries are kept as-is and matched by
    prefix at lookup time (Field notes: "A directory entry ... covers
    everything under it"). A handful of early worklist-history files are a
    bare JSON list of items rather than the {description, items} wrapper --
    both shapes are accepted.
    """
    out = {}
    items = doc if isinstance(doc, list) else doc.get("items", [])
    for it in items:
        if not isinstance(it, dict):
            continue
        item_id = it.get("id")
        if not item_id:
            continue
        files = set()
        f = it.get("file")
        if isinstance(f, str):
            files.add(f)
        fs = it.get("files")
        if isinstance(fs, list):
            files.update(s for s in fs if isinstance(s, str))
        begun = it.get("begunAtMs")
        begun_ms = float(begun) if isinstance(begun, (int, float)) else None
        out[item_id] = {"files": frozenset(files), "begunAtMs": begun_ms}
    return out


def build_snapshots(history_dir: Path, worklist_path: Path):
    """[(ts_ms, {id: {...}}), ...] sorted ascending, ending with the CURRENT
    resources/worklist.json (stamped at its mtime, standing for "now" --
    valid from that point forward). Board membership at any edit time is read
    from the nearest snapshot AT OR BEFORE that time; an item absent from
    that snapshot is treated as not-yet-proposed or already-pruned, with no
    need for separate prune-interval bookkeeping.
    """
    snaps = []
    for path in history_dir.glob("*.json"):
        stem = path.stem
        if not stem.isdigit():
            continue
        try:
            doc = json.loads(path.read_text(errors="replace"))
        except (OSError, json.JSONDecodeError):
            continue
        snaps.append((int(stem), items_from_doc(doc)))
    if worklist_path.is_file():
        try:
            doc = json.loads(worklist_path.read_text(errors="replace"))
            mtime_ms = int(worklist_path.stat().st_mtime * 1000)
            snaps.append((mtime_ms, items_from_doc(doc)))
        except (OSError, json.JSONDecodeError):
            pass
    snaps.sort(key=lambda s: s[0])
    return snaps


def covering_items(snapshots, ts_ms: float, rel_path: str):
    """Begun, not-yet-pruned items (as of the nearest snapshot <= ts_ms) whose
    declared files cover rel_path. Returns (items_dict_subset, snapshot_ts)
    or (None, None) if ts_ms predates every snapshot (no data)."""
    idx = bisect.bisect_right([s[0] for s in snapshots], ts_ms) - 1
    if idx < 0:
        return None, None
    snap_ts, items = snapshots[idx]
    hits = {}
    rel = PurePosixPath(rel_path)
    for item_id, info in items.items():
        begun = info["begunAtMs"]
        if begun is None or begun > ts_ms:
            continue
        for f in info["files"]:
            fp = f.rstrip("/")
            if rel_path == fp or str(rel).startswith(fp + "/"):
                hits[item_id] = info
                break
    return hits, snap_ts


# --- session scan --------------------------------------------------------------


def record_text(rec: dict):
    content = rec.get("message", {}).get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        if any(isinstance(b, dict) and b.get("type") == "tool_result" for b in content):
            return None
        return "\n\n".join(
            b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text"
        )
    return None


def scan_sessions(session_dir: Path, project_root: Path, since_ms: float | None):
    """Per session: chronological edit list [(ts_ms, rel_path, tool)] and
    sorted genuine-user-message timestamps (turn starts). Edits outside
    project_root are dropped (excluded from population -- they can never be
    covered by a worklist item's declared paths) and counted separately.
    """
    root_prefix = str(project_root) + "/"
    edits_by_session: dict[str, list[tuple[float, str, str]]] = {}
    users_by_session: dict[str, list[float]] = {}
    outside_root = 0
    total_edit_calls = 0

    for path in sorted(session_dir.glob("*.jsonl")):
        edits, users = [], []
        try:
            with path.open("r", errors="replace") as fh:
                for line in fh:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        rec = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    ts = parse_ts(rec.get("timestamp") or "")
                    if ts is None:
                        continue
                    ts_ms = to_ms(ts)
                    rtype = rec.get("type")
                    if rtype == "user":
                        text = record_text(rec)
                        if text is None or rec.get("isMeta"):
                            continue
                        if not text.lstrip().startswith(ENVELOPE):
                            users.append(ts_ms)
                    elif rtype == "assistant":
                        content = rec.get("message", {}).get("content")
                        if not isinstance(content, list):
                            continue
                        for b in content:
                            if not (isinstance(b, dict) and b.get("type") == "tool_use"):
                                continue
                            if b.get("name") not in EDIT_TOOLS:
                                continue
                            fp = (b.get("input") or {}).get("file_path")
                            if not isinstance(fp, str):
                                continue
                            total_edit_calls += 1
                            if fp.startswith(root_prefix):
                                rel = fp[len(root_prefix):]
                                edits.append((ts_ms, rel, b.get("name")))
                            else:
                                outside_root += 1
        except OSError:
            continue
        edits.sort(key=lambda e: e[0])
        users.sort()
        if since_ms is not None:
            edits = [e for e in edits if e[0] >= since_ms]
        edits_by_session[path.stem] = edits
        users_by_session[path.stem] = users

    return edits_by_session, users_by_session, outside_root, total_edit_calls


# --- classification ------------------------------------------------------------


def turn_start(users: list[float], ts_ms: float) -> float:
    i = bisect.bisect_right(users, ts_ms)
    return users[i - 1] if i > 0 else 0.0


def classify(session_edits, idx_in_session, users, rel_path, item_begun_min):
    ts_ms = session_edits[idx_in_session][0]
    t_start = turn_start(users, ts_ms)
    for j in range(idx_in_session - 1, -1, -1):
        prior_ts, prior_path, _tool = session_edits[j]
        if prior_ts < t_start:
            break
        if prior_path == rel_path:
            return "same-turn-correction"
    if item_begun_min < t_start:
        return "cold-resume"
    return "other"


# --- main ------------------------------------------------------------------


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument(
        "--project-root", type=Path, default=Path.cwd(),
        help="project whose sessions, traces, and worklist history to read (default: cwd)",
    )
    ap.add_argument("--session-dir", type=Path, help="override the Claude session directory")
    ap.add_argument("--trace-dir", type=Path, help="override the bram-traces directory")
    ap.add_argument("--history-dir", type=Path, help="override the worklist-history directory")
    ap.add_argument("--since", help="ISO date or timestamp; ignore edits before it")
    ap.add_argument("--json", action="store_true", help="emit JSON instead of text")
    ap.add_argument(
        "--samples", type=int, default=5, help="sample pairs to print per class (default 5)"
    )
    args = ap.parse_args(argv)

    root = args.project_root.expanduser().resolve()
    session_dir = (args.session_dir or session_dir_for(root)).expanduser()
    trace_dir = (args.trace_dir or root / "resources" / "bram-traces").expanduser()
    history_dir = (args.history_dir or root / "resources" / "worklist-history").expanduser()
    worklist_path = root / "resources" / "worklist.json"

    since = parse_ts(args.since) if args.since else None
    if args.since and since is None:
        print(f"could not parse --since {args.since!r}", file=sys.stderr)
        return 2
    since_ms = to_ms(since) if since else None

    for label, path in (("session", session_dir), ("trace", trace_dir), ("history", history_dir)):
        if not path.is_dir():
            print(f"{label} directory not found: {path}", file=sys.stderr)
            return 2

    events, scanned, damaged = read_claim_events(trace_dir)
    last_ts_ms = events[-1][0] if events else 0.0
    intervals, anomalies, unmatched_open = build_claim_intervals(events, last_ts_ms)

    snapshots = build_snapshots(history_dir, worklist_path)
    if not snapshots:
        print(f"no worklist-history snapshots found under {history_dir}", file=sys.stderr)
        return 2

    edits_by_session, users_by_session, outside_root, total_edit_calls = scan_sessions(
        session_dir, root, since_ms
    )

    boundary_ms = to_ms(parse_ts(COMMIT_BOUNDARY))

    population = []  # each: dict with ts_ms, session, rel_path, item_ids, klass
    considered = 0
    no_snapshot_data = 0

    for session, edits in edits_by_session.items():
        users = users_by_session.get(session, [])
        for idx, (ts_ms, rel_path, tool) in enumerate(edits):
            considered += 1
            hits, snap_ts = covering_items(snapshots, ts_ms, rel_path)
            if hits is None:
                no_snapshot_data += 1
                continue
            if not hits:
                continue  # not covered by any begun, unpruned item -- not in scope
            covering_ids = set(hits.keys())
            if any(claim_covers(intervals, iid, ts_ms, last_ts_ms) for iid in covering_ids):
                continue  # a claim covering the declaring item was live -- authorized
            item_begun_min = min(info["begunAtMs"] for info in hits.values())
            klass = classify(edits, idx, users, rel_path, item_begun_min)
            population.append(
                {
                    "ts_ms": ts_ms,
                    "ts": dt.datetime.fromtimestamp(ts_ms / 1000, tz=dt.timezone.utc).isoformat(),
                    "session": session,
                    "path": rel_path,
                    "tool": tool,
                    "item_ids": sorted(covering_ids),
                    "class": klass,
                }
            )

    population.sort(key=lambda p: p["ts_ms"])
    before = [p for p in population if p["ts_ms"] < boundary_ms]
    after = [p for p in population if p["ts_ms"] >= boundary_ms]

    class_counts = {"same-turn-correction": 0, "cold-resume": 0, "other": 0}
    for p in population:
        class_counts[p["class"]] += 1

    rate_per_1000 = (len(population) / considered * 1000.0) if considered else 0.0

    # Cross-check against the 3 known post-fix would-deny fires.
    recovered = []
    for fire_ts_text, fire_path in KNOWN_POST_FIX_FIRES:
        fire_ms = to_ms(parse_ts(fire_ts_text))
        match = None
        for p in after:
            if p["path"] == fire_path and abs(p["ts_ms"] - fire_ms) <= FIRE_MATCH_TOLERANCE_S * 1000:
                match = p
                break
        recovered.append(
            {"expected": fire_ts_text, "path": fire_path, "found": match["ts"] if match else None}
        )

    result = {
        "project_root": str(root),
        "since": since.isoformat() if since else None,
        "trace_logs_scanned": scanned,
        "trace_logs_damaged": damaged,
        "claim_events": len(events),
        "claim_anomalies": anomalies,
        "claim_ids_unmatched_open": unmatched_open,
        "worklist_snapshots": len(snapshots),
        "edit_calls_total": total_edit_calls,
        "edit_calls_outside_project_root": outside_root,
        "edit_calls_considered": considered,
        "edit_calls_no_snapshot_data": no_snapshot_data,
        "population_size": len(population),
        "rate_per_1000_edits": round(rate_per_1000, 3),
        "classification": class_counts,
        "before_b64d81f": len(before),
        "after_b64d81f": len(after),
        "known_fires_recovered": recovered,
        "samples": {
            k: [
                {"ts": p["ts"], "path": p["path"], "item_ids": p["item_ids"], "session": p["session"]}
                for p in population
                if p["class"] == k
            ][: args.samples]
            for k in class_counts
        },
    }

    if args.json:
        print(json.dumps(result, indent=2))
        return 0

    print(f"project              {root}")
    if since:
        print(f"since                {since.isoformat()}")
    print(f"trace logs scanned   {scanned} (live + rotated)")
    if damaged:
        print(
            f"    WARNING {len(damaged)} log(s) truncated; partial reads counted:"
            f" {', '.join(damaged[:5])}{' ...' if len(damaged) > 5 else ''}"
        )
    print(f"claim events         {len(events)}")
    print(f"claim anomalies      {anomalies}")
    print(f"claim ids unmatched (never closed in observed data): {unmatched_open}")
    print(f"worklist snapshots   {len(snapshots)}")
    print()
    print(f"edit calls total                 {total_edit_calls}")
    print(f"  outside project root (excluded) {outside_root}")
    print(f"  considered                      {considered}")
    print(f"    no snapshot data (excluded)   {no_snapshot_data}")
    print()
    print(f"POPULATION (would-deny predicate true)  {len(population)}")
    print(f"    rate per 1,000 edits considered     {result['rate_per_1000_edits']}")
    print(f"    before b64d81f (2026-09-09 19:39:39Z)  {len(before)}")
    print(f"    after  b64d81f                          {len(after)}")
    print()
    print("classification:")
    for k, v in class_counts.items():
        print(f"    {k:<22} {v}")
    print()
    print("known post-fix fires recovered?")
    for r in recovered:
        status = r["found"] or "NOT FOUND"
        print(f"    expected {r['expected']} {r['path']:<28} -> {status}")
    print()
    for k in class_counts:
        rows = result["samples"][k]
        print(f"sample: {k} ({class_counts[k]} total, showing {len(rows)})")
        for row in rows:
            print(f"    {row['ts'][:19]}  {row['path']:<40} item={row['item_ids']} session={row['session']}")
        print()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
