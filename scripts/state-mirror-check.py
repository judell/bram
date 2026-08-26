#!/usr/bin/env python3
"""state-mirror-check.py — on-demand consistency check between Bram's
worklist lifecycle *files* (the sole truth) and the phase-A SQLite mirror
built by `worklist_state.rs` (see that module's doc comment and the
`state-mirror-store-and-ledger` worklist item).

Scope: this script checks only what that item mirrors —

  - `resources/.worklist-authorization.json` vs the matching row in the
    mirror's `auth_records` table (keyed by `issuedAtMs` / `issued_at_ms`).
  - `resources/.inflight-claim.json` vs the live (uncleared) row in the
    mirror's `claims` table.

It does not validate `worklist.json` itself, or anything about the
`transitions` ledger's content beyond a row count — those are out of this
item's scope.

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


def load_json(path: Path):
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as e:
        print(f"MISMATCH read {path}: {e}")
        return "ERROR"


def check_auth(conn: sqlite3.Connection, project_root: Path) -> bool:
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

    row = conn.execute(
        "SELECT kind, ids, consumed_at_ms FROM auth_records WHERE issued_at_ms = ?",
        (issued_at_ms,),
    ).fetchone()
    if row is None:
        print(
            f"MISMATCH auth-record: file=issuedAtMs={issued_at_ms} kind={file_kind} "
            f"ids={sorted(file_ids)} db=no matching row"
        )
        return False

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
        print(f"MISMATCH auth-record: {'; '.join(problems)}")
        return False
    print(f"OK auth-record: issuedAtMs={issued_at_ms} kind={file_kind} ids={sorted(file_ids)}")
    return True


def check_claim(conn: sqlite3.Connection, project_root: Path) -> bool:
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

    row = conn.execute(
        "SELECT kind, ids, cleared_at_ms FROM claims WHERE written_at_ms = ?",
        (written_at_ms,),
    ).fetchone()
    if row is None:
        print(
            f"MISMATCH claim: file=claimedAt={written_at_ms} kind={file_kind} "
            f"ids={sorted(file_ids)} db=no matching row"
        )
        return False

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
        print(f"MISMATCH claim: {'; '.join(problems)}")
        return False
    print(f"OK claim: claimedAt={written_at_ms} kind={file_kind} ids={sorted(file_ids)}")
    return True


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

    conn = sqlite3.connect(str(db_path))
    try:
        ok_auth = check_auth(conn, project_root)
        ok_claim = check_claim(conn, project_root)
        transitions = conn.execute("SELECT count(*) FROM transitions").fetchone()[0]
        print(f"transitions rows: {transitions}")
    finally:
        conn.close()

    return 0 if (ok_auth and ok_claim) else 1


if __name__ == "__main__":
    sys.exit(main())
