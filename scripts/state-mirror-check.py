#!/usr/bin/env python3
"""state-mirror-check.py — on-demand consistency check between Bram's
worklist lifecycle *files* (the sole truth) and the phase-A SQLite mirror
built by `worklist_state.rs` (see that module's doc comment and the
`state-mirror-store-and-ledger` worklist item).

Scope: this script checks —

  - `resources/.worklist-authorization.json` vs the matching row in the
    mirror's `auth_records` table (keyed by `issuedAtMs` / `issued_at_ms`).
  - `resources/.inflight-claim.json` vs the live (uncleared) row in the
    mirror's `claims` table.
  - with `--items`, `resources/worklist.json` items vs the mirror's
    `items` table (id/status/begunAtMs/files), un-tombstoned rows only
    (`state-mirror-items-shadow`). Silently downgrades to a SKIPPED note
    (not a failure) when the mirror db predates that item and has no
    `items` table yet.

It does not validate anything about the `transitions` ledger's content
beyond a row count — that's out of this item's scope.

Cold-start note (state-mirror-divergence-tripwire): a freshly created
mirror db necessarily has no row for any auth/claim record whose own
timestamp (`issuedAtMs` / `claimedAt`) predates the db file's own creation
time — the mirror simply didn't exist yet to catch that write. Those cases
print as `PRE-MIRROR (informational)` rather than `MISMATCH` and do not
affect the exit code; a record timestamped AFTER the db was created that
still disagrees with (or is absent from) the mirror is a real `MISMATCH`.

Usage:

    scripts/state-mirror-check.py --project-root /path/to/project --db /path/to/worklist-state.db
    scripts/state-mirror-check.py --db ~/Library/Caches/org.xmlui.bram/worklist-state/-Users-jonudell-bram.db

`--project-root` defaults to the current directory. `--db` defaults to a
best-effort derivation of Bram's own cache-dir convention
(`<app_cache_dir>/worklist-state/<project-key>.db`, the same project-key
encoding `encode_path_for_filename` uses in lib.rs: `/`, `\\`, `:` -> `-`)
for the current platform. That derivation is a guess, not something this
script can verify independently of Bram itself — Tauri's app_cache_dir
depends on the OS and on the app identifier (`org.xmlui.bram`, from
src-tauri/tauri.conf.json), and Windows in particular is unconfirmed.
Pass --db explicitly whenever the default doesn't resolve, or to point at
a specific project's db from anywhere on disk.

Exit status: 0 iff every check reported OK. Prints one line per check.
"""

import argparse
import json
import platform
import sqlite3
import sys
from pathlib import Path

WORKLIST_AUTH_REL = "resources/.worklist-authorization.json"
INFLIGHT_CLAIM_REL = "resources/.inflight-claim.json"
APP_IDENTIFIER = "org.xmlui.bram"


def encode_path_for_filename(p: Path) -> str:
    """Mirror lib.rs's `encode_path_for_filename`: flatten a path into a
    filename-safe identifier by replacing '/', '\\', ':' with '-'. Non-ASCII
    characters pass through unchanged, matching the non-lossy Rust fn
    (search_index_db_path / worklist_state_db_path both use this variant,
    not the ascii-lossy one)."""
    s = str(p)
    return "".join("-" if c in "/\\:" else c for c in s)


def default_cache_dir() -> Path | None:
    """Best-effort guess at Tauri's app_cache_dir() for this app identifier.
    Returns None if the platform isn't recognized — callers should require
    --db in that case."""
    system = platform.system()
    home = Path.home()
    if system == "Darwin":
        return home / "Library" / "Caches" / APP_IDENTIFIER
    if system == "Linux":
        import os

        xdg = os.environ.get("XDG_CACHE_HOME")
        base = Path(xdg) if xdg else home / ".cache"
        return base / APP_IDENTIFIER
    if system == "Windows":
        import os

        local = os.environ.get("LOCALAPPDATA")
        if not local:
            return None
        # Tauri's Windows app_cache_dir layout is unconfirmed from this
        # script; this is a guess (LOCALAPPDATA/<identifier>/cache), not a
        # verified value. Pass --db explicitly on Windows.
        return Path(local) / APP_IDENTIFIER / "cache"
    return None


def derive_default_db_path(project_root: Path) -> Path | None:
    cache_dir = default_cache_dir()
    if cache_dir is None:
        return None
    key = encode_path_for_filename(project_root.resolve())
    return cache_dir / "worklist-state" / f"{key}.db"


def db_creation_ms(db_path: Path) -> int | None:
    """Best-effort creation time of the mirror db file, in Unix ms. Prefers
    `st_birthtime` (true creation time, available on macOS/BSD) and falls
    back to `st_ctime` (metadata-change time on POSIX — not creation, but
    the closest portable proxy) when birthtime isn't reported. Returns None
    only if stat() itself fails, which main() already guards against via
    the earlier `db_path.exists()` check."""
    try:
        st = db_path.stat()
    except OSError:
        return None
    ts = getattr(st, "st_birthtime", None)
    if ts is None:
        ts = st.st_ctime
    return int(ts * 1000)


def load_json(path: Path):
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as e:
        print(f"MISMATCH read {path}: {e}")
        return "ERROR"


def _report(check: str, msg: str, pre_mirror: bool) -> bool:
    """Print one PRE-MIRROR/MISMATCH problem line and return the ok=False
    verdict — PRE-MIRROR is informational only and does not fail the run."""
    if pre_mirror:
        print(f"PRE-MIRROR (informational) {check}: {msg}")
        return True
    print(f"MISMATCH {check}: {msg}")
    return False


def check_auth(conn: sqlite3.Connection, project_root: Path, db_created_ms: int | None) -> bool:
    auth_path = project_root / WORKLIST_AUTH_REL
    record = load_json(auth_path)
    if record == "ERROR":
        return False

    if record is None:
        # No file — consistent only if the db has no live (unconsumed) row.
        row = conn.execute(
            "SELECT issued_at_ms, kind, ids FROM auth_records "
            "WHERE consumed_at_ms IS NULL ORDER BY issued_at_ms DESC LIMIT 1"
        ).fetchone()
        if row is None:
            print("OK auth-record: no file, no live db row")
            return True
        print(
            f"MISMATCH auth-record: file=absent db=live row issued_at_ms={row[0]} "
            f"kind={row[1]} ids={row[2]}"
        )
        return False

    issued_at_ms = record.get("issuedAtMs")
    file_kind = record.get("kind")
    file_ids = set(record.get("ids") or [])
    file_consumed = record.get("consumedAtMs")
    # state-mirror-divergence-tripwire cold-start refinement: this file
    # record predates the mirror db's own existence, so the mirror never had
    # a chance to observe its write — any disagreement below is expected,
    # not a bug.
    pre_mirror = (
        db_created_ms is not None
        and isinstance(issued_at_ms, (int, float))
        and issued_at_ms < db_created_ms
    )

    row = conn.execute(
        "SELECT kind, ids, consumed_at_ms FROM auth_records WHERE issued_at_ms = ?",
        (issued_at_ms,),
    ).fetchone()
    if row is None:
        return _report(
            "auth-record",
            f"file=issuedAtMs={issued_at_ms} kind={file_kind} ids={sorted(file_ids)} "
            f"db=no matching row",
            pre_mirror,
        )

    db_kind, db_ids_json, db_consumed = row
    db_ids = set(json.loads(db_ids_json))

    problems = []
    if db_kind != file_kind:
        problems.append(f"kind file={file_kind} db={db_kind}")
    if db_ids != file_ids:
        problems.append(f"ids file={sorted(file_ids)} db={sorted(db_ids)}")
    # consumedAtMs: the file only ever holds the currently active record
    # (consumed_at_ms is null in practice, since a consumed record is
    # retired/overwritten rather than left on disk with the flag set) — so
    # this compares presence, not exact timestamps, which can legitimately
    # drift by the few ms between the file write and the mirror apply.
    if bool(file_consumed) != bool(db_consumed):
        problems.append(f"consumedAtMs file={file_consumed} db={db_consumed}")

    if problems:
        return _report("auth-record", "; ".join(problems), pre_mirror)
    print(f"OK auth-record: issuedAtMs={issued_at_ms} kind={file_kind} ids={sorted(file_ids)}")
    return True


def check_claim(conn: sqlite3.Connection, project_root: Path, db_created_ms: int | None) -> bool:
    claim_path = project_root / INFLIGHT_CLAIM_REL
    claim = load_json(claim_path)
    if claim == "ERROR":
        return False

    if claim is None:
        row = conn.execute(
            "SELECT written_at_ms, kind, ids FROM claims "
            "WHERE cleared_at_ms IS NULL ORDER BY written_at_ms DESC LIMIT 1"
        ).fetchone()
        if row is None:
            print("OK claim: no file, no live db row")
            return True
        print(
            f"MISMATCH claim: file=absent db=live row written_at_ms={row[0]} "
            f"kind={row[1]} ids={row[2]}"
        )
        return False

    written_at_ms = claim.get("claimedAt")
    file_kind = claim.get("kind")
    file_ids = set(claim.get("ids") or [])
    pre_mirror = (
        db_created_ms is not None
        and isinstance(written_at_ms, (int, float))
        and written_at_ms < db_created_ms
    )

    row = conn.execute(
        "SELECT kind, ids, cleared_at_ms FROM claims WHERE written_at_ms = ?",
        (written_at_ms,),
    ).fetchone()
    if row is None:
        return _report(
            "claim",
            f"file=claimedAt={written_at_ms} kind={file_kind} ids={sorted(file_ids)} "
            f"db=no matching row",
            pre_mirror,
        )

    db_kind, db_ids_json, db_cleared = row
    db_ids = set(json.loads(db_ids_json))

    problems = []
    if db_kind != file_kind:
        problems.append(f"kind file={file_kind} db={db_kind}")
    if db_ids != file_ids:
        problems.append(f"ids file={sorted(file_ids)} db={sorted(db_ids)}")
    if db_cleared is not None:
        problems.append(f"db row already cleared_at_ms={db_cleared} but file is still live")

    if problems:
        return _report("claim", "; ".join(problems), pre_mirror)
    print(f"OK claim: claimedAt={written_at_ms} kind={file_kind} ids={sorted(file_ids)}")
    return True


def table_exists(conn: sqlite3.Connection, name: str) -> bool:
    row = conn.execute(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?", (name,)
    ).fetchone()
    return bool(row and row[0])


def _item_files(item: dict) -> list:
    if isinstance(item.get("files"), list):
        return sorted(f for f in item["files"] if f)
    f = item.get("file")
    return [f] if f else []


def check_items(conn: sqlite3.Connection, project_root: Path) -> bool:
    """Compare worklist.json's items against the mirror's `items` table
    (un-tombstoned rows only) — id/status/begunAtMs/files, the same fields
    `worklist_state::compare_divergence`'s items check covers in the Rust
    tripwire. No PRE-MIRROR carve-out here: unlike auth_records/claims,
    `items` is fully re-synced from the current worklist.json on every
    `/__worklist` read and on every watcher/mutate event
    (`state-mirror-items-shadow`), so a mismatch found once that machinery
    has run is a live disagreement, not a pre-mirror artifact."""
    if not table_exists(conn, "items"):
        print(
            "SKIPPED items: mirror db has no items table yet "
            "(state-mirror-items-shadow not deployed to this db, or no sync has run)"
        )
        return True

    worklist_path = project_root / "resources" / "worklist.json"
    doc = load_json(worklist_path)
    if doc == "ERROR":
        return False
    file_items = (doc or {}).get("items") or []

    file_by_id = {}
    for item in file_items:
        iid = item.get("id")
        if not iid:
            continue
        file_by_id[iid] = {
            "status": item.get("status", "proposed"),
            "begunAtMs": item.get("begunAtMs"),
            "files": _item_files(item),
        }

    rows = conn.execute(
        "SELECT id, status, begun_at_ms, files FROM items WHERE pruned_at_ms IS NULL"
    ).fetchall()
    db_by_id = {}
    for iid, status, begun_at_ms, files_json in rows:
        try:
            files = sorted(json.loads(files_json)) if files_json else []
        except json.JSONDecodeError:
            files = []
        db_by_id[iid] = {"status": status, "begunAtMs": begun_at_ms, "files": files}

    ok = True
    for iid, f in file_by_id.items():
        d = db_by_id.get(iid)
        if d is None:
            print(f"MISMATCH items[{iid}]: file present, db=absent-or-tombstoned")
            ok = False
            continue
        problems = []
        if d["status"] != f["status"]:
            problems.append(f"status file={f['status']} db={d['status']}")
        if d["begunAtMs"] != f["begunAtMs"]:
            problems.append(f"begunAtMs file={f['begunAtMs']} db={d['begunAtMs']}")
        if d["files"] != f["files"]:
            problems.append(f"files file={f['files']} db={d['files']}")
        if problems:
            print(f"MISMATCH items[{iid}]: {'; '.join(problems)}")
            ok = False
        else:
            print(f"OK items[{iid}]: status={f['status']}")

    for iid, d in db_by_id.items():
        if iid not in file_by_id:
            print(f"MISMATCH items[{iid}]: file=absent db=present (status={d['status']})")
            ok = False

    return ok


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Compare Bram's worklist authorization/claim files against the "
            "phase-A SQLite mirror (worklist_state.rs). See this file's "
            "module docstring for scope and the --db derivation caveat."
        )
    )
    parser.add_argument(
        "--project-root",
        default=".",
        help="Bram project root containing resources/.worklist-authorization.json "
        "and resources/.inflight-claim.json (default: current directory)",
    )
    parser.add_argument(
        "--db",
        default=None,
        help="Path to the worklist-state SQLite db. If omitted, a best-effort "
        "default is derived from Bram's cache-dir convention for this "
        "platform and project root — pass this explicitly if that guess is "
        "wrong or the platform is unrecognized.",
    )
    parser.add_argument(
        "--items",
        action="store_true",
        help="Also compare resources/worklist.json items against the mirror's "
        "items table (state-mirror-items-shadow). Silently downgrades to a "
        "SKIPPED note, not a failure, when the mirror db has no items table.",
    )
    args = parser.parse_args()

    project_root = Path(args.project_root).resolve()

    if args.db:
        db_path = Path(args.db).expanduser()
    else:
        derived = derive_default_db_path(project_root)
        if derived is None:
            parser.error(
                "--db is required: could not derive a default cache path for "
                f"platform {platform.system()!r}"
            )
        db_path = derived

    if not db_path.exists():
        print(f"MISMATCH db: {db_path} does not exist")
        return 1

    created_ms = db_creation_ms(db_path)
    if created_ms is not None:
        print(f"db created: {created_ms} ({db_path})")

    conn = sqlite3.connect(str(db_path))
    try:
        ok_auth = check_auth(conn, project_root, created_ms)
        ok_claim = check_claim(conn, project_root, created_ms)
        ok_items = check_items(conn, project_root) if args.items else True
        transitions = conn.execute("SELECT count(*) FROM transitions").fetchone()[0]
        print(f"transitions rows: {transitions}")
    finally:
        conn.close()

    return 0 if (ok_auth and ok_claim and ok_items) else 1


if __name__ == "__main__":
    sys.exit(main())
