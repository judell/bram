//! Phase A of the mirror-then-migrate strategy for worklist lifecycle state
//! (worklist item `state-mirror-store-and-ledger`): a SQLite shadow of the
//! two agent-reachable state files —
//! `resources/.worklist-authorization.json` and
//! `resources/.inflight-claim.json` — plus an append-only transitions
//! ledger.
//!
//! **Files remain the sole truth this phase.** `lib.rs` calls the
//! `mirror_*` functions in this module AFTER each successful file write, at
//! the same host choke points that write those files
//! (`write_worklist_authorization_record`, `retire_worklist_authorization`,
//! `write_inflight_claim_sentinel`, `shrink_inflight_claim_sentinel`,
//! `clear_inflight_claim_sentinel`). Any error from this module is
//! trace-only — it must never block or alter the file-write path. Callers
//! are expected to wrap every call so a mirror failure degrades to a
//! `[state-mirror] op=error` trace line and nothing else.
//!
//! Modeled on `search_index.rs`: same rusqlite `bundled` crate, same
//! `open`/`open_in_memory` + `ensure_schema` idiom gated by
//! `PRAGMA user_version`. The one deliberate difference: `search_index.rs`
//! is a rebuildable cache (safe to `DROP TABLE` on a version mismatch, since
//! a rebuild restores it); this store's destiny is *authoritative*, so
//! `ensure_schema` here never drops data — a future schema change needs its
//! own migration, not a wipe-and-recreate.
//!
//! Phase-A principle: mirror existing shapes faithfully, no schema
//! redesign. The single-slot claim file stays single-slot in `claims` — a
//! new write while a prior row is still uncleared stamps that prior row
//! cleared with `displaced = 1` rather than modeling a queue.
//!
//! Signatures here take plain data (ints, `&str`, `&[String]`) on purpose —
//! this module knows nothing about `tauri::AppHandle` or any host type, so
//! lib.rs's choke points extract the fields from the struct they just
//! serialized to disk and pass them in directly (mirror what the file write
//! wrote, not a re-derivation).
//!
//! `scripts/state-mirror-check.py` is an on-demand consistency checker that
//! compares the current file contents against the latest matching rows in
//! this db; run it after touching either side of the mirror.

use rusqlite::{params, Connection, Result};

/// Bump when the on-disk schema shape changes. Unlike `search_index.rs`,
/// this is NOT a rebuildable cache — see the module doc comment. A bump
/// here must ship an explicit migration once real rows exist; today
/// `ensure_schema` only uses `CREATE TABLE IF NOT EXISTS`, so a bump alone
/// does nothing but record intent.
const SCHEMA_VERSION: i64 = 1;

/// Open (or create) the state-mirror db at `path`, in WAL mode, with the
/// current schema ensured.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL lets a future reader (e.g. a diagnostic route) run alongside the
    // mirror's writes. No-op on :memory:, so the result is ignored.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    ensure_schema(&conn)?;
    Ok(conn)
}

/// In-memory db — used by tests.
#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    ensure_schema(&conn)?;
    Ok(conn)
}

/// Ensure the three mirror tables exist at the current schema version.
/// Never drops data — see the module doc comment for why this differs from
/// `search_index.rs::ensure_schema`.
fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS auth_records( \
           id INTEGER PRIMARY KEY, \
           issued_at_ms INTEGER NOT NULL UNIQUE, \
           kind TEXT NOT NULL, \
           ids TEXT NOT NULL, \
           commit_too INTEGER NOT NULL DEFAULT 0, \
           interrupted_at_ms INTEGER, \
           consumed_at_ms INTEGER, \
           updated_at_ms INTEGER NOT NULL); \
         CREATE TABLE IF NOT EXISTS claims( \
           id INTEGER PRIMARY KEY, \
           written_at_ms INTEGER NOT NULL UNIQUE, \
           kind TEXT NOT NULL, \
           ids TEXT NOT NULL, \
           cleared_at_ms INTEGER, \
           displaced INTEGER NOT NULL DEFAULT 0, \
           updated_at_ms INTEGER NOT NULL); \
         CREATE TABLE IF NOT EXISTS transitions( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           at_ms INTEGER NOT NULL, \
           item_id TEXT, \
           kind TEXT NOT NULL, \
           detail TEXT, \
           source TEXT); \
         CREATE INDEX IF NOT EXISTS transitions_at_ms ON transitions(at_ms);",
    )?;
    if version != SCHEMA_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    }
    Ok(())
}

/// Serialize a string slice the same way `serde_json::to_string` would for a
/// JSON array, without pulling `serde_json` into this module's public
/// surface — kept local since every mirror fn needs it for `ids`.
fn ids_json(ids: &[String]) -> String {
    let mut out = String::from("[");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for c in id.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                _ => out.push(c),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

fn append_transition(
    conn: &Connection,
    at_ms: i64,
    item_id: Option<&str>,
    kind: &str,
    detail: &str,
    source: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO transitions(at_ms, item_id, kind, detail, source) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![at_ms, item_id, kind, detail, source],
    )?;
    Ok(())
}

// --- auth_records ----------------------------------------------------------

/// Mirror a fresh `.worklist-authorization.json` write (the gate-click
/// writer, `write_worklist_authorization_record` in lib.rs). Upserts on
/// `issued_at_ms` so a retry that reuses the same timestamp does not
/// duplicate a row — normal operation always inserts a new row here since
/// `issued_at_ms` is `unix_now_ms()` at write time.
pub fn mirror_auth_record(
    conn: &Connection,
    issued_at_ms: i64,
    kind: &str,
    ids: &[String],
    commit_too: bool,
    source: &str,
    now_ms: i64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let ids_s = ids_json(ids);
    tx.execute(
        "INSERT INTO auth_records(issued_at_ms, kind, ids, commit_too, updated_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(issued_at_ms) DO UPDATE SET \
           kind = excluded.kind, ids = excluded.ids, commit_too = excluded.commit_too, \
           updated_at_ms = excluded.updated_at_ms",
        params![issued_at_ms, kind, ids_s, commit_too as i64, now_ms],
    )?;
    append_transition(
        &tx,
        now_ms,
        None,
        "auth-record",
        &format!(
            "{{\"kind\":\"{}\",\"ids\":{},\"issuedAtMs\":{},\"commitToo\":{}}}",
            kind, ids_s, issued_at_ms, commit_too
        ),
        source,
    )?;
    tx.commit()
}

/// Mirror a full consume of the active auth record
/// (`consume_worklist_authorization`, the `WorklistAuthRetirement::Consumed`
/// outcome of `retire_worklist_authorization`). Updates the row keyed by
/// `issued_at_ms` — the same identity the original `mirror_auth_record` call
/// upserted — with the final `ids`/`consumed_at_ms`.
pub fn mirror_auth_consume(
    conn: &Connection,
    issued_at_ms: i64,
    kind: &str,
    ids: &[String],
    consumed_at_ms: i64,
    source: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let ids_s = ids_json(ids);
    tx.execute(
        "UPDATE auth_records SET ids = ?2, consumed_at_ms = ?3, updated_at_ms = ?3 \
         WHERE issued_at_ms = ?1",
        params![issued_at_ms, ids_s, consumed_at_ms],
    )?;
    append_transition(
        &tx,
        consumed_at_ms,
        None,
        "auth-consume",
        &format!("{{\"kind\":\"{}\",\"ids\":{}}}", kind, ids_s),
        source,
    )?;
    tx.commit()
}

/// Mirror a partial retirement of the active auth record
/// (`retire_worklist_authorization_ids`, the `WorklistAuthRetirement::Shrunk`
/// outcome). Updates the same row's `ids` to the remaining set; leaves
/// `consumed_at_ms` untouched (the record is still live).
pub fn mirror_auth_consume_shrink(
    conn: &Connection,
    issued_at_ms: i64,
    kind: &str,
    resolved_ids: &[String],
    remaining_ids: &[String],
    at_ms: i64,
    source: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let remaining_s = ids_json(remaining_ids);
    tx.execute(
        "UPDATE auth_records SET ids = ?2, updated_at_ms = ?3 WHERE issued_at_ms = ?1",
        params![issued_at_ms, remaining_s, at_ms],
    )?;
    append_transition(
        &tx,
        at_ms,
        None,
        "auth-consume-shrink",
        &format!(
            "{{\"kind\":\"{}\",\"resolved\":{},\"remaining\":{}}}",
            kind,
            ids_json(resolved_ids),
            remaining_s
        ),
        source,
    )?;
    tx.commit()
}

// --- claims ------------------------------------------------------------

/// Mirror a fresh `.inflight-claim.json` write
/// (`write_inflight_claim_sentinel`). Single-slot semantics: if a prior row
/// is still uncleared, it is stamped cleared here with `displaced = 1`
/// before the new row is inserted — mirroring the file's own displace-on-
/// overwrite behavior.
pub fn mirror_claim_write(
    conn: &Connection,
    written_at_ms: i64,
    kind: &str,
    ids: &[String],
    source: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE claims SET cleared_at_ms = ?1, displaced = 1, updated_at_ms = ?1 \
         WHERE cleared_at_ms IS NULL",
        params![written_at_ms],
    )?;
    let ids_s = ids_json(ids);
    tx.execute(
        "INSERT INTO claims(written_at_ms, kind, ids, updated_at_ms) VALUES (?1, ?2, ?3, ?1) \
         ON CONFLICT(written_at_ms) DO UPDATE SET \
           kind = excluded.kind, ids = excluded.ids, updated_at_ms = excluded.updated_at_ms",
        params![written_at_ms, kind, ids_s],
    )?;
    append_transition(
        &tx,
        written_at_ms,
        None,
        "claim-write",
        &format!("{{\"kind\":\"{}\",\"ids\":{}}}", kind, ids_s),
        source,
    )?;
    tx.commit()
}

/// Mirror an incremental claim shrink (`shrink_inflight_claim_sentinel`,
/// the `ClaimShrink::Shrunk` outcome) — updates the live (uncleared) row's
/// `ids` to the remaining set.
pub fn mirror_claim_shrink(
    conn: &Connection,
    resolved_ids: &[String],
    remaining_ids: &[String],
    at_ms: i64,
    source: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let remaining_s = ids_json(remaining_ids);
    tx.execute(
        "UPDATE claims SET ids = ?1, updated_at_ms = ?2 WHERE cleared_at_ms IS NULL",
        params![remaining_s, at_ms],
    )?;
    append_transition(
        &tx,
        at_ms,
        None,
        "claim-shrink",
        &format!(
            "{{\"resolved\":{},\"remaining\":{}}}",
            ids_json(resolved_ids),
            remaining_s
        ),
        source,
    )?;
    tx.commit()
}

/// Mirror a full claim clear (`clear_inflight_claim_sentinel`, or the
/// `ClaimShrink::Cleared` outcome of `shrink_inflight_claim_sentinel`) —
/// stamps the live (uncleared) row's `cleared_at_ms`. `displaced` stays 0:
/// this is an intentional clear, not an overwrite-by-a-newer-write.
pub fn mirror_claim_clear(conn: &Connection, ids: &[String], at_ms: i64, source: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE claims SET cleared_at_ms = ?1, updated_at_ms = ?1 WHERE cleared_at_ms IS NULL",
        params![at_ms],
    )?;
    append_transition(
        &tx,
        at_ms,
        None,
        "claim-clear",
        &format!("{{\"ids\":{}}}", ids_json(ids)),
        source,
    )?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_three_tables() {
        let conn = open_in_memory().unwrap();
        for table in ["auth_records", "claims", "transitions"] {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }
    }

    #[test]
    fn mirror_auth_record_then_consume_updates_same_row() {
        let conn = open_in_memory().unwrap();
        let ids = vec!["item-a".to_string(), "item-b".to_string()];
        mirror_auth_record(&conn, 1000, "approved", &ids, false, "worklist-action-command", 1000)
            .unwrap();

        let (kind, consumed): (String, Option<i64>) = conn
            .query_row(
                "SELECT kind, consumed_at_ms FROM auth_records WHERE issued_at_ms = 1000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "approved");
        assert_eq!(consumed, None);

        mirror_auth_consume(&conn, 1000, "approved", &ids, 2000, "mutate-advance").unwrap();
        let (row_count, consumed): (i64, Option<i64>) = conn
            .query_row(
                "SELECT count(*), max(consumed_at_ms) FROM auth_records WHERE issued_at_ms = 1000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row_count, 1, "consume must update the same row, not insert a new one");
        assert_eq!(consumed, Some(2000));

        let transitions: i64 = conn
            .query_row("SELECT count(*) FROM transitions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(transitions, 2, "one transition per mirror call");
    }

    #[test]
    fn mirror_claim_write_displaces_prior_uncleared_row() {
        let conn = open_in_memory().unwrap();
        let first = vec!["item-a".to_string()];
        mirror_claim_write(&conn, 100, "approved", &first, "toTurn").unwrap();

        let second = vec!["item-b".to_string()];
        mirror_claim_write(&conn, 200, "drop", &second, "toTurn").unwrap();

        let (displaced, cleared): (i64, Option<i64>) = conn
            .query_row(
                "SELECT displaced, cleared_at_ms FROM claims WHERE written_at_ms = 100",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(displaced, 1);
        assert_eq!(cleared, Some(200));

        let live_count: i64 = conn
            .query_row("SELECT count(*) FROM claims WHERE cleared_at_ms IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(live_count, 1, "exactly one live claim row after a displace");
    }

    #[test]
    fn mirror_claim_shrink_then_clear() {
        let conn = open_in_memory().unwrap();
        let ids = vec!["a".to_string(), "b".to_string()];
        mirror_claim_write(&conn, 10, "approved", &ids, "toTurn").unwrap();

        mirror_claim_shrink(&conn, &["a".to_string()], &["b".to_string()], 20, "mutate-advance")
            .unwrap();
        let stored_ids: String = conn
            .query_row("SELECT ids FROM claims WHERE written_at_ms = 10", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored_ids, "[\"b\"]");

        mirror_claim_clear(&conn, &["b".to_string()], 30, "mutate-advance").unwrap();
        let cleared: Option<i64> = conn
            .query_row("SELECT cleared_at_ms FROM claims WHERE written_at_ms = 10", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cleared, Some(30));

        let transitions: i64 = conn
            .query_row("SELECT count(*) FROM transitions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(transitions, 3);
    }
}
