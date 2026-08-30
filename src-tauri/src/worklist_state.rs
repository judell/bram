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

use rusqlite::{params, Connection, OptionalExtension, Result};

/// Bump when the on-disk schema shape changes. Unlike `search_index.rs`,
/// this is NOT a rebuildable cache — see the module doc comment. A bump
/// here must ship an explicit migration once real rows exist; today
/// `ensure_schema` only uses `CREATE TABLE IF NOT EXISTS`, so a bump alone
/// does nothing but record intent.
///
/// v2 (state-mirror-items-shadow): additive migration adding the `items`
/// table. `auth_records`, `claims`, and `transitions` are untouched —
/// `ensure_schema` never drops data, so existing v1 dbs pick up the new
/// table on next open with their rows intact.
const SCHEMA_VERSION: i64 = 2;

/// Open (or create) the state-mirror db at `path`, in WAL mode, with the
/// current schema ensured.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL lets a future reader (e.g. a diagnostic route) run alongside the
    // mirror's writes. No-op on :memory:, so the result is ignored.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    // mirror-busy-timeout-and-vocab: without a busy_timeout, a reader
    // (Status route, the checker) overlapping a write fails the write
    // instantly with `database is locked` (wave 3's one mirror error,
    // judell/bram#291). 250ms of patience retires that class; the
    // layered re-syncs behind it remain the correctness backstop.
    let _ = conn.pragma_update(None, "busy_timeout", 250);
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
         CREATE INDEX IF NOT EXISTS transitions_at_ms ON transitions(at_ms); \
         CREATE TABLE IF NOT EXISTS items( \
           id TEXT PRIMARY KEY, \
           status TEXT, \
           begun_at_ms INTEGER, \
           files TEXT, \
           closes_issues TEXT, \
           first_seen_ms INTEGER NOT NULL, \
           last_synced_ms INTEGER NOT NULL, \
           pruned_at_ms INTEGER); \
         CREATE INDEX IF NOT EXISTS items_last_synced_ms ON items(last_synced_ms);",
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
pub fn mirror_claim_clear(
    conn: &Connection,
    ids: &[String],
    at_ms: i64,
    source: &str,
) -> Result<()> {
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

// --- items ---------------------------------------------------------------
//
// state-mirror-items-shadow, the second slice of the mirror: the other
// half of worklist lifecycle state — status, begunAtMs, files,
// closesIssues, item existence itself — lives in worklist.json, written
// directly by agents rather than through a host choke point the way
// auth_records/claims are. So instead of one mirror_* call per write, this
// is a full-file re-sync driven from wherever the current contents are
// known: the filesystem watcher (after `maybe_enforce_worklist_policy`'s
// revert logic runs, so a reverted write mirrors the restored truth), the
// mutate advance/prune completion point (which `worklist-commit`'s prune
// also reaches by delegation), and a reconcile-on-read backstop for a
// request that lands before either has run.

/// Escape a string for embedding in the hand-built JSON `detail` blobs
/// below — same rule set as `ids_json`'s per-character escape, factored out
/// since `mirror_items_sync` needs it standalone (not always inside a JSON
/// array).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

/// Plain-data snapshot of one worklist.json item, passed into
/// `mirror_items_sync`. `files_json` and `closes_issues_json` are already
/// serialized (the caller's job — this module takes no `serde_json`
/// dependency, per the module's existing convention of plain data over the
/// FFI-ish boundary between lib.rs's host types and this store).
pub struct WorklistItemSnapshot<'a> {
    pub id: &'a str,
    pub status: &'a str,
    pub begun_at_ms: Option<i64>,
    /// Pre-serialized JSON array of file paths, e.g. `["a.rs","b.rs"]`.
    pub files_json: &'a str,
    /// Pre-serialized JSON value for `closesIssues`, e.g. `[]` or
    /// `[{"number":42,"title":"..."}]`.
    pub closes_issues_json: &'a str,
}

/// Counts from one `mirror_items_sync` pass, folded into the caller's
/// `[state-mirror]` trace line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ItemsSyncCounts {
    pub upserts: usize,
    pub tombstones: usize,
    pub transitions: usize,
}

/// Full-file re-sync of the `items` table from a parsed worklist.json
/// snapshot. Upserts every item present in `items` (clearing
/// `pruned_at_ms` if it had been tombstoned and reappeared); soft-
/// tombstones (never `DELETE`s, so the transitions ledger keeps its
/// referent) any previously-live row whose id is absent from the
/// snapshot.
///
/// Appends one `transitions` row per observed CHANGE: `item-seen` (new
/// id), `item-status` (status differs from the stored row; detail carries
/// old/new), `item-begun` (`begun_at_ms` newly set, i.e. None -> Some —
/// mirrors the file-side rule that the stamp is never moved once set),
/// `item-files` (files list differs; detail carries old/new), `item-
/// pruned` (tombstoned this pass). `closes_issues` and a bare tombstone
/// reappearance are kept in sync silently — phase-A mirrors existing
/// shapes; neither is one of the tracked transition kinds. An item with no
/// observed change appends nothing.
///
/// Every present row's `last_synced_ms` is bumped to `now_ms` regardless
/// of whether anything else changed, so the reconcile-on-read backstop's
/// `MAX(last_synced_ms)` check advances even on a no-op pass.
pub fn mirror_items_sync(
    conn: &Connection,
    now_ms: i64,
    items: &[WorklistItemSnapshot],
    source: &str,
) -> Result<ItemsSyncCounts> {
    let tx = conn.unchecked_transaction()?;
    let mut counts = ItemsSyncCounts::default();
    let mut seen_ids: Vec<&str> = Vec::with_capacity(items.len());

    for item in items {
        seen_ids.push(item.id);
        let existing: Option<(String, Option<i64>, String, String, Option<i64>)> = tx
            .query_row(
                "SELECT status, begun_at_ms, files, closes_issues, pruned_at_ms \
                 FROM items WHERE id = ?1",
                params![item.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;

        match existing {
            None => {
                tx.execute(
                    "INSERT INTO items(id, status, begun_at_ms, files, closes_issues, \
                       first_seen_ms, last_synced_ms, pruned_at_ms) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, NULL)",
                    params![
                        item.id,
                        item.status,
                        item.begun_at_ms,
                        item.files_json,
                        item.closes_issues_json,
                        now_ms
                    ],
                )?;
                append_transition(
                    &tx,
                    now_ms,
                    Some(item.id),
                    "item-seen",
                    &format!(
                        "{{\"status\":\"{}\",\"files\":{}}}",
                        json_escape(item.status),
                        item.files_json
                    ),
                    source,
                )?;
                counts.upserts += 1;
                counts.transitions += 1;
            }
            Some((old_status, old_begun, old_files, old_closes, old_pruned)) => {
                let status_changed = old_status != item.status;
                let begun_changed = old_begun != item.begun_at_ms;
                let begun_newly_set = old_begun.is_none() && item.begun_at_ms.is_some();
                let files_changed = old_files != item.files_json;
                let closes_changed = old_closes != item.closes_issues_json;
                let was_pruned = old_pruned.is_some();
                let content_changed = status_changed
                    || begun_changed
                    || files_changed
                    || closes_changed
                    || was_pruned;

                if content_changed {
                    tx.execute(
                        "UPDATE items SET status = ?2, begun_at_ms = ?3, files = ?4, \
                           closes_issues = ?5, last_synced_ms = ?6, pruned_at_ms = NULL \
                         WHERE id = ?1",
                        params![
                            item.id,
                            item.status,
                            item.begun_at_ms,
                            item.files_json,
                            item.closes_issues_json,
                            now_ms
                        ],
                    )?;
                    if status_changed {
                        append_transition(
                            &tx,
                            now_ms,
                            Some(item.id),
                            "item-status",
                            &format!(
                                "{{\"old\":\"{}\",\"new\":\"{}\"}}",
                                json_escape(&old_status),
                                json_escape(item.status)
                            ),
                            source,
                        )?;
                        counts.transitions += 1;
                    }
                    if begun_newly_set {
                        append_transition(
                            &tx,
                            now_ms,
                            Some(item.id),
                            "item-begun",
                            &format!("{{\"beganAtMs\":{}}}", item.begun_at_ms.unwrap_or(now_ms)),
                            source,
                        )?;
                        counts.transitions += 1;
                    }
                    if files_changed {
                        append_transition(
                            &tx,
                            now_ms,
                            Some(item.id),
                            "item-files",
                            &format!("{{\"old\":{},\"new\":{}}}", old_files, item.files_json),
                            source,
                        )?;
                        counts.transitions += 1;
                    }
                    counts.upserts += 1;
                } else {
                    tx.execute(
                        "UPDATE items SET last_synced_ms = ?2 WHERE id = ?1",
                        params![item.id, now_ms],
                    )?;
                }
            }
        }
    }

    // Soft-tombstone any previously-live row absent from this snapshot.
    let mut stmt = tx.prepare("SELECT id, status FROM items WHERE pruned_at_ms IS NULL")?;
    let live_rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>>>()?;
    drop(stmt);
    for (id, status) in live_rows {
        if seen_ids.iter().any(|s| *s == id) {
            continue;
        }
        tx.execute(
            "UPDATE items SET pruned_at_ms = ?2, last_synced_ms = ?2 WHERE id = ?1",
            params![id, now_ms],
        )?;
        append_transition(
            &tx,
            now_ms,
            Some(id.as_str()),
            "item-pruned",
            &format!("{{\"status\":\"{}\"}}", json_escape(&status)),
            source,
        )?;
        counts.tombstones += 1;
        counts.transitions += 1;
    }

    tx.commit()?;
    Ok(counts)
}

// --- divergence tripwire ---------------------------------------------------
//
// state-mirror-divergence-tripwire: with both mirror halves in place
// (auth_records/claims written at host choke points, items full-resynced),
// the question that gates the first phase-B reader flip is whether the db
// view ever disagrees with the file view. Every `/__worklist` build derives
// both views and calls `compare_divergence` to diff them. This is a
// TRIPWIRE, not a soak observer: zero `Divergence`s is the success
// condition, and its deny-path reachability is proven by a deliberate fire
// (the tests below write the db a lie and assert this function catches it),
// never by waiting — see the `deliberate fire` tests at the bottom of this
// module.
//
// The comparison lives here, not in lib.rs, specifically so it is a pure
// function over plain data + a `Connection` — testable without an
// `AppHandle`, matching this module's existing convention (see the module
// doc comment).

/// One point of disagreement between the file-derived view and the db
/// view. `field` is `"items"`, `"claim"`, or `"auth"`; `item` is the item id
/// for an `"items"` mismatch and `None` otherwise; `detail` is a compact,
/// human-readable rendering of both sides, ready to append to a
/// `[state-mirror] op=divergence` trace line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    pub field: &'static str,
    pub item: Option<String>,
    pub detail: String,
}

/// File-derived claim, or `None` when `.inflight-claim.json` is absent
/// (absent = no live claim).
pub struct FileClaimSnapshot<'a> {
    pub kind: &'a str,
    pub ids: &'a [String],
}

/// File-derived authorization record. Callers pass `None` when
/// `.worklist-authorization.json` itself is absent — nothing to compare
/// against in that case, so the auth check is skipped entirely rather than
/// asserting "no db row" (a fresh project may have plenty of retired
/// auth_records rows from before this record's issued_at_ms).
pub struct FileAuthSnapshot<'a> {
    pub issued_at_ms: i64,
    pub kind: &'a str,
    pub ids: &'a [String],
    pub consumed: bool,
}

/// The file-derived view of worklist lifecycle state, assembled by the
/// `/__worklist` route from the same sources it already reads (the built
/// response doc for items, `.inflight-claim.json` for the claim,
/// `.worklist-authorization.json` for the auth record).
pub struct FileStateSnapshot<'a> {
    pub items: &'a [WorklistItemSnapshot<'a>],
    pub claim: Option<FileClaimSnapshot<'a>>,
    pub auth: Option<FileAuthSnapshot<'a>>,
}

/// Diff `snapshot` (the file-derived view) against the db view read from
/// `conn`. Returns one `Divergence` per disagreement; an empty vec means
/// the two views agree completely.
///
/// Cold-db tolerance: items are compared against `items` rows with
/// `pruned_at_ms IS NULL`, and the caller is expected to have run the
/// reconcile-on-read backstop (`mirror_items_sync`) immediately before
/// building `snapshot` — by the time this runs, a fresh db has already
/// caught up to the current file contents, so there is nothing here that
/// special-cases "empty tables" for items. `claim`/`auth` are naturally
/// cold-tolerant the same way: `None` on both sides (nothing ever written)
/// produces no divergence.
pub fn compare_divergence(
    conn: &Connection,
    snapshot: &FileStateSnapshot,
) -> Result<Vec<Divergence>> {
    let mut out = Vec::new();

    // --- items ---
    let mut stmt = conn
        .prepare("SELECT id, status, begun_at_ms, files FROM items WHERE pruned_at_ms IS NULL")?;
    let db_items: std::collections::HashMap<String, (Option<String>, Option<i64>, Option<String>)> =
        stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
        })?
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect();
    drop(stmt);

    let mut file_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for item in snapshot.items {
        file_ids.insert(item.id);
        match db_items.get(item.id) {
            None => {
                out.push(Divergence {
                    field: "items",
                    item: Some(item.id.to_string()),
                    detail: format!(
                        "file={{status:{},begun:{:?},files:{}}} db=absent-or-tombstoned",
                        item.status, item.begun_at_ms, item.files_json
                    ),
                });
            }
            Some((db_status, db_begun, db_files)) => {
                let status_match = db_status.as_deref() == Some(item.status);
                let begun_match = *db_begun == item.begun_at_ms;
                let files_match = db_files.as_deref() == Some(item.files_json);
                if !status_match || !begun_match || !files_match {
                    out.push(Divergence {
                        field: "items",
                        item: Some(item.id.to_string()),
                        detail: format!(
                            "file={{status:{},begun:{:?},files:{}}} db={{status:{:?},begun:{:?},files:{:?}}}",
                            item.status, item.begun_at_ms, item.files_json,
                            db_status, db_begun, db_files
                        ),
                    });
                }
            }
        }
    }
    for (id, (status, begun, files)) in &db_items {
        if !file_ids.contains(id.as_str()) {
            out.push(Divergence {
                field: "items",
                item: Some(id.clone()),
                detail: format!(
                    "file=absent db={{status:{:?},begun:{:?},files:{:?}}}",
                    status, begun, files
                ),
            });
        }
    }

    // --- claim ---
    let db_claim: Option<(String, String)> = conn
        .query_row(
            "SELECT kind, ids FROM claims WHERE cleared_at_ms IS NULL \
             ORDER BY written_at_ms DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match (&snapshot.claim, &db_claim) {
        (None, None) => {}
        (Some(f), None) => out.push(Divergence {
            field: "claim",
            item: None,
            detail: format!("file={{kind:{},ids:{}}} db=none", f.kind, ids_json(f.ids)),
        }),
        (None, Some((db_kind, db_ids))) => out.push(Divergence {
            field: "claim",
            item: None,
            detail: format!("file=none db={{kind:{},ids:{}}}", db_kind, db_ids),
        }),
        (Some(f), Some((db_kind, db_ids))) => {
            if f.kind != db_kind || &ids_json(f.ids) != db_ids {
                out.push(Divergence {
                    field: "claim",
                    item: None,
                    detail: format!(
                        "file={{kind:{},ids:{}}} db={{kind:{},ids:{}}}",
                        f.kind,
                        ids_json(f.ids),
                        db_kind,
                        db_ids
                    ),
                });
            }
        }
    }

    // --- auth ---
    if let Some(f) = &snapshot.auth {
        let db_auth: Option<(String, String, Option<i64>)> = conn
            .query_row(
                "SELECT kind, ids, consumed_at_ms FROM auth_records WHERE issued_at_ms = ?1",
                params![f.issued_at_ms],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match db_auth {
            None => out.push(Divergence {
                field: "auth",
                item: None,
                detail: format!(
                    "file={{kind:{},ids:{},consumed:{}}} db=absent issued_at_ms={}",
                    f.kind,
                    ids_json(f.ids),
                    f.consumed,
                    f.issued_at_ms
                ),
            }),
            Some((db_kind, db_ids, db_consumed_at_ms)) => {
                let db_consumed = db_consumed_at_ms.is_some();
                if f.kind != db_kind || ids_json(f.ids) != db_ids || f.consumed != db_consumed {
                    out.push(Divergence {
                        field: "auth",
                        item: None,
                        detail: format!(
                            "file={{kind:{},ids:{},consumed:{}}} db={{kind:{},ids:{},consumed:{}}}",
                            f.kind,
                            ids_json(f.ids),
                            f.consumed,
                            db_kind,
                            db_ids,
                            db_consumed
                        ),
                    });
                }
            }
        }
    }

    Ok(out)
}

// --- Status-tab read helpers ------------------------------------------------
//
// state-mirror-divergence-tripwire, dual-source conversion: plain read-only
// queries backing the Status tab's "State Mirror" section and the
// dual-sourced "Current claim" / "Claim pairs" / "Auth history (mirror)"
// rows. No writes here — these run on every Status tab fetch.

/// The single un-cleared (live) claim row, if any: `(kind, ids_json,
/// written_at_ms)`. Mirrors the single-slot semantics of
/// `.inflight-claim.json` — at most one row should ever match, but the
/// `ORDER BY ... LIMIT 1` is a defensive tie-break, not a claim that more
/// than one can exist under normal operation.
pub fn live_claim(conn: &Connection) -> Result<Option<(String, String, i64)>> {
    conn.query_row(
        "SELECT kind, ids, written_at_ms FROM claims WHERE cleared_at_ms IS NULL \
         ORDER BY written_at_ms DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
}

/// `(writes, clears, last_at_ms)` from the transitions ledger: the "Claim
/// pairs" row's replacement for the old `bram-trace.log` grep of
/// `[inflight-sentinel] op=write` / `op=clear`. `clears` counts
/// `claim-clear` transitions only — that kind already covers a shrink that
/// terminated a claim (`mirror_claim_shrink`'s `ClaimShrink::Cleared`
/// outcome is mirrored via `mirror_claim_clear`, not `mirror_claim_shrink`),
/// so no separate shrink-count is needed to get the full termination
/// picture.
pub fn claim_pair_counts(conn: &Connection) -> Result<(i64, i64, Option<i64>)> {
    let writes: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transitions WHERE kind = 'claim-write'",
        [],
        |r| r.get(0),
    )?;
    let clears: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transitions WHERE kind = 'claim-clear'",
        [],
        |r| r.get(0),
    )?;
    let last_ms: Option<i64> = conn.query_row(
        "SELECT MAX(at_ms) FROM transitions WHERE kind IN ('claim-write', 'claim-clear')",
        [],
        |r| r.get(0),
    )?;
    Ok((writes, clears, last_ms))
}

/// `(transitions_count, last_at_ms)` — the "State Mirror" section's
/// "Transitions" / "Last apply" rows.
pub fn mirror_health(conn: &Connection) -> Result<(i64, Option<i64>)> {
    let transitions: i64 = conn.query_row("SELECT COUNT(*) FROM transitions", [], |r| r.get(0))?;
    let last_ms: Option<i64> =
        conn.query_row("SELECT MAX(at_ms) FROM transitions", [], |r| r.get(0))?;
    Ok((transitions, last_ms))
}

/// Last `limit` `auth_records` rows, newest first:
/// `(issued_at_ms, kind, ids_json, consumed_at_ms)`. Backs the
/// Authorization section's "Auth history (mirror)" row — history the
/// single-slot `.worklist-authorization.json` file cannot show.
pub fn recent_auth_records(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<(i64, String, String, Option<i64>)>> {
    let mut stmt = conn.prepare(
        "SELECT issued_at_ms, kind, ids, consumed_at_ms FROM auth_records \
         ORDER BY issued_at_ms DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_four_tables() {
        let conn = open_in_memory().unwrap();
        for table in ["auth_records", "claims", "transitions", "items"] {
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
        mirror_auth_record(
            &conn,
            1000,
            "approved",
            &ids,
            false,
            "worklist-action-command",
            1000,
        )
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
        assert_eq!(
            row_count, 1,
            "consume must update the same row, not insert a new one"
        );
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
            .query_row(
                "SELECT count(*) FROM claims WHERE cleared_at_ms IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(live_count, 1, "exactly one live claim row after a displace");
    }

    #[test]
    fn mirror_claim_shrink_then_clear() {
        let conn = open_in_memory().unwrap();
        let ids = vec!["a".to_string(), "b".to_string()];
        mirror_claim_write(&conn, 10, "approved", &ids, "toTurn").unwrap();

        mirror_claim_shrink(
            &conn,
            &["a".to_string()],
            &["b".to_string()],
            20,
            "mutate-advance",
        )
        .unwrap();
        let stored_ids: String = conn
            .query_row("SELECT ids FROM claims WHERE written_at_ms = 10", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored_ids, "[\"b\"]");

        mirror_claim_clear(&conn, &["b".to_string()], 30, "mutate-advance").unwrap();
        let cleared: Option<i64> = conn
            .query_row(
                "SELECT cleared_at_ms FROM claims WHERE written_at_ms = 10",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cleared, Some(30));

        let transitions: i64 = conn
            .query_row("SELECT count(*) FROM transitions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(transitions, 3);
    }

    // --- items -------------------------------------------------------------

    fn snap<'a>(
        id: &'a str,
        status: &'a str,
        begun_at_ms: Option<i64>,
        files_json: &'a str,
        closes_issues_json: &'a str,
    ) -> WorklistItemSnapshot<'a> {
        WorklistItemSnapshot {
            id,
            status,
            begun_at_ms,
            files_json,
            closes_issues_json,
        }
    }

    #[test]
    fn mirror_items_sync_new_item_is_seen() {
        let conn = open_in_memory().unwrap();
        let items = vec![snap("item-a", "proposed", None, "[\"a.rs\"]", "[]")];
        let counts = mirror_items_sync(&conn, 1000, &items, "watcher").unwrap();

        assert_eq!(counts.upserts, 1);
        assert_eq!(counts.tombstones, 0);
        assert_eq!(counts.transitions, 1);

        let (status, first_seen, last_synced, pruned): (String, i64, i64, Option<i64>) = conn
            .query_row(
                "SELECT status, first_seen_ms, last_synced_ms, pruned_at_ms FROM items WHERE id = 'item-a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(status, "proposed");
        assert_eq!(first_seen, 1000);
        assert_eq!(last_synced, 1000);
        assert_eq!(pruned, None);

        let (kind, item_id): (String, Option<String>) = conn
            .query_row("SELECT kind, item_id FROM transitions", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(kind, "item-seen");
        assert_eq!(item_id.as_deref(), Some("item-a"));
    }

    #[test]
    fn mirror_items_sync_status_change_appends_transition() {
        let conn = open_in_memory().unwrap();
        let first = vec![snap("item-a", "proposed", None, "[]", "[]")];
        mirror_items_sync(&conn, 1000, &first, "watcher").unwrap();

        let second = vec![snap("item-a", "applied", None, "[]", "[]")];
        let counts = mirror_items_sync(&conn, 2000, &second, "mutate").unwrap();

        assert_eq!(counts.upserts, 1);
        assert_eq!(counts.transitions, 1);

        let status: String = conn
            .query_row("SELECT status FROM items WHERE id = 'item-a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "applied");

        let (kind, detail): (String, String) = conn
            .query_row(
                "SELECT kind, detail FROM transitions WHERE at_ms = 2000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "item-status");
        assert_eq!(detail, "{\"old\":\"proposed\",\"new\":\"applied\"}");
    }

    #[test]
    fn mirror_items_sync_files_change_appends_transition() {
        let conn = open_in_memory().unwrap();
        let first = vec![snap("item-a", "proposed", None, "[\"a.rs\"]", "[]")];
        mirror_items_sync(&conn, 1000, &first, "watcher").unwrap();

        let second = vec![snap(
            "item-a",
            "proposed",
            None,
            "[\"a.rs\",\"b.rs\"]",
            "[]",
        )];
        let counts = mirror_items_sync(&conn, 2000, &second, "watcher").unwrap();

        assert_eq!(counts.upserts, 1);
        assert_eq!(counts.transitions, 1);

        let files: String = conn
            .query_row("SELECT files FROM items WHERE id = 'item-a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(files, "[\"a.rs\",\"b.rs\"]");

        let kind: String = conn
            .query_row("SELECT kind FROM transitions WHERE at_ms = 2000", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, "item-files");
    }

    #[test]
    fn mirror_items_sync_tombstone_then_reappear_clears_tombstone() {
        let conn = open_in_memory().unwrap();
        let present = vec![snap("item-a", "proposed", None, "[]", "[]")];
        mirror_items_sync(&conn, 1000, &present, "watcher").unwrap();

        // item-a is missing from this pass -> tombstoned.
        let empty: Vec<WorklistItemSnapshot<'_>> = vec![];
        let counts = mirror_items_sync(&conn, 2000, &empty, "watcher").unwrap();
        assert_eq!(counts.tombstones, 1);
        assert_eq!(counts.transitions, 1);

        let pruned: Option<i64> = conn
            .query_row(
                "SELECT pruned_at_ms FROM items WHERE id = 'item-a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pruned, Some(2000));
        let kind: String = conn
            .query_row("SELECT kind FROM transitions WHERE at_ms = 2000", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, "item-pruned");

        // item-a reappears -> tombstone cleared.
        let reappeared = vec![snap("item-a", "proposed", None, "[]", "[]")];
        mirror_items_sync(&conn, 3000, &reappeared, "watcher").unwrap();
        let pruned: Option<i64> = conn
            .query_row(
                "SELECT pruned_at_ms FROM items WHERE id = 'item-a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pruned, None, "reappearance must clear the tombstone");
    }

    #[test]
    fn mirror_items_sync_is_idempotent() {
        let conn = open_in_memory().unwrap();
        let items = vec![
            snap("item-a", "proposed", None, "[\"a.rs\"]", "[]"),
            snap(
                "item-b",
                "applied",
                Some(500),
                "[\"b.rs\"]",
                "[{\"number\":42}]",
            ),
        ];
        let first = mirror_items_sync(&conn, 1000, &items, "watcher").unwrap();
        assert_eq!(first.upserts, 2);
        // Both are brand-new ids -> one item-seen each. A begun_at_ms already
        // set at first sighting is not "newly set" (that transition only
        // fires when a later sync observes None -> Some on a row this store
        // already knew about), so item-b's Some(500) does not also fire
        // item-begun here.
        assert_eq!(first.transitions, 2, "item-a seen + item-b seen");

        // Identical snapshot, later timestamp: nothing changed.
        let second = mirror_items_sync(&conn, 2000, &items, "watcher").unwrap();
        assert_eq!(second.upserts, 0);
        assert_eq!(second.tombstones, 0);
        assert_eq!(
            second.transitions, 0,
            "second sync of unchanged items appends no transitions"
        );

        // last_synced_ms still advances on the no-op pass.
        let last_synced: i64 = conn
            .query_row(
                "SELECT last_synced_ms FROM items WHERE id = 'item-a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(last_synced, 2000);

        let total_transitions: i64 = conn
            .query_row("SELECT count(*) FROM transitions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_transitions, 2);
    }

    // --- compare_divergence: deliberate-fire tripwire provenance ----------
    //
    // Per the tripwire-provenance rule (conventions.md, Log-first
    // development): a tripwire's zero is meaningless without a test proving
    // the deny path is reachable. Each test below writes the db a lie the
    // file-derived snapshot disagrees with, and asserts `compare_divergence`
    // catches it — never a soak, never a wait.

    #[test]
    fn compare_divergence_agrees_on_matching_state() {
        let conn = open_in_memory().unwrap();
        let items = vec![snap("item-a", "proposed", None, "[\"a.rs\"]", "[]")];
        mirror_items_sync(&conn, 1000, &items, "watcher").unwrap();
        mirror_claim_write(&conn, 1000, "approved", &["item-a".to_string()], "toTurn").unwrap();
        mirror_auth_record(
            &conn,
            1000,
            "approved",
            &["item-a".to_string()],
            false,
            "worklist-action-command",
            1000,
        )
        .unwrap();

        let claim_ids = vec!["item-a".to_string()];
        let auth_ids = vec!["item-a".to_string()];
        let snapshot = FileStateSnapshot {
            items: &items,
            claim: Some(FileClaimSnapshot {
                kind: "approved",
                ids: &claim_ids,
            }),
            auth: Some(FileAuthSnapshot {
                issued_at_ms: 1000,
                kind: "approved",
                ids: &auth_ids,
                consumed: false,
            }),
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();
        assert_eq!(
            divergences,
            Vec::new(),
            "matching file/db state must report no divergence"
        );
    }

    #[test]
    fn compare_divergence_catches_item_status_lie() {
        let conn = open_in_memory().unwrap();
        // The db mirror thinks item-a is still "proposed" ...
        let synced = vec![snap("item-a", "proposed", None, "[]", "[]")];
        mirror_items_sync(&conn, 1000, &synced, "watcher").unwrap();

        // ... but the file-derived snapshot (what /__worklist just read) says
        // "applied" -- a deliberate lie the mirror never saw.
        let lying = vec![snap("item-a", "applied", None, "[]", "[]")];
        let snapshot = FileStateSnapshot {
            items: &lying,
            claim: None,
            auth: None,
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].field, "items");
        assert_eq!(divergences[0].item.as_deref(), Some("item-a"));
        assert!(divergences[0].detail.contains("proposed"));
        assert!(divergences[0].detail.contains("applied"));
    }

    #[test]
    fn compare_divergence_catches_missing_db_item() {
        let conn = open_in_memory().unwrap();
        // Nothing ever mirrored -- an empty db lying by omission against a
        // file that has a real item.
        let file_items = vec![snap("item-a", "proposed", None, "[]", "[]")];
        let snapshot = FileStateSnapshot {
            items: &file_items,
            claim: None,
            auth: None,
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].field, "items");
        assert_eq!(divergences[0].item.as_deref(), Some("item-a"));
        assert!(divergences[0].detail.contains("db=absent-or-tombstoned"));
    }

    #[test]
    fn compare_divergence_catches_claim_kind_lie() {
        let conn = open_in_memory().unwrap();
        mirror_claim_write(&conn, 100, "approved", &["item-a".to_string()], "toTurn").unwrap();

        // The file sentinel says "drop" -- a lie against the mirrored "approved".
        let ids = vec!["item-a".to_string()];
        let snapshot = FileStateSnapshot {
            items: &[],
            claim: Some(FileClaimSnapshot {
                kind: "drop",
                ids: &ids,
            }),
            auth: None,
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].field, "claim");
        assert_eq!(divergences[0].item, None);
        assert!(divergences[0].detail.contains("drop"));
        assert!(divergences[0].detail.contains("approved"));
    }

    #[test]
    fn compare_divergence_catches_claim_present_in_db_but_absent_in_file() {
        let conn = open_in_memory().unwrap();
        mirror_claim_write(&conn, 100, "approved", &["item-a".to_string()], "toTurn").unwrap();

        // The file has no sentinel at all -- as if .inflight-claim.json were
        // deleted out from under a live db row.
        let snapshot = FileStateSnapshot {
            items: &[],
            claim: None,
            auth: None,
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].field, "claim");
        assert!(divergences[0].detail.contains("file=none"));
    }

    #[test]
    fn compare_divergence_catches_auth_consumed_lie() {
        let conn = open_in_memory().unwrap();
        let ids = vec!["item-a".to_string()];
        mirror_auth_record(
            &conn,
            5000,
            "approved",
            &ids,
            false,
            "worklist-action-command",
            5000,
        )
        .unwrap();

        // The file says this record is still pending (not consumed), but
        // the mirror was told it was consumed -- a deliberate lie on the
        // "consumed state" the item's spec calls out explicitly.
        mirror_auth_consume(&conn, 5000, "approved", &ids, 6000, "mutate-advance").unwrap();

        let snapshot = FileStateSnapshot {
            items: &[],
            claim: None,
            auth: Some(FileAuthSnapshot {
                issued_at_ms: 5000,
                kind: "approved",
                ids: &ids,
                consumed: false,
            }),
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].field, "auth");
        assert!(divergences[0].detail.contains("consumed:false"));
        assert!(divergences[0].detail.contains("consumed:true"));
    }

    #[test]
    fn compare_divergence_catches_auth_record_missing_from_db() {
        let conn = open_in_memory().unwrap();
        // The mirror never saw this write at all.
        let ids = vec!["item-a".to_string()];
        let snapshot = FileStateSnapshot {
            items: &[],
            claim: None,
            auth: Some(FileAuthSnapshot {
                issued_at_ms: 9999,
                kind: "approved",
                ids: &ids,
                consumed: false,
            }),
        };
        let divergences = compare_divergence(&conn, &snapshot).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].field, "auth");
        assert!(divergences[0].detail.contains("db=absent"));
    }

    #[test]
    fn live_claim_returns_the_uncleared_row() {
        let conn = open_in_memory().unwrap();
        assert_eq!(live_claim(&conn).unwrap(), None);
        mirror_claim_write(&conn, 10, "approved", &["a".to_string()], "toTurn").unwrap();
        let (kind, ids, written_at_ms) = live_claim(&conn).unwrap().unwrap();
        assert_eq!(kind, "approved");
        assert_eq!(ids, "[\"a\"]");
        assert_eq!(written_at_ms, 10);
        mirror_claim_clear(&conn, &["a".to_string()], 20, "mutate-advance").unwrap();
        assert_eq!(live_claim(&conn).unwrap(), None);
    }

    #[test]
    fn claim_pair_counts_counts_writes_and_clears() {
        let conn = open_in_memory().unwrap();
        mirror_claim_write(&conn, 10, "approved", &["a".to_string()], "toTurn").unwrap();
        mirror_claim_clear(&conn, &["a".to_string()], 20, "mutate-advance").unwrap();
        mirror_claim_write(&conn, 30, "drop", &["b".to_string()], "toTurn").unwrap();
        let (writes, clears, last_ms) = claim_pair_counts(&conn).unwrap();
        assert_eq!(writes, 2);
        assert_eq!(clears, 1);
        assert_eq!(last_ms, Some(30));
    }

    #[test]
    fn recent_auth_records_orders_newest_first() {
        let conn = open_in_memory().unwrap();
        let ids = vec!["a".to_string()];
        mirror_auth_record(&conn, 100, "approved", &ids, false, "src", 100).unwrap();
        mirror_auth_record(&conn, 200, "drop", &ids, false, "src", 200).unwrap();
        let rows = recent_auth_records(&conn, 5).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 200);
        assert_eq!(rows[1].0, 100);
    }
}
