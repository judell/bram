//! issue-230 unified search: embedded SQLite (via `rusqlite` `bundled`, which
//! statically links SQLite and compiles FTS5 in by default) + the combined
//! full-text index for the unified search in #230.
//!
//! This module is the DB layer only: schema, per-file bookkeeping, upsert, and
//! query. The indexer driver (session discovery + text extraction) and the
//! `/__search` route live in `lib.rs`. The combined table generalizes the Go
//! prior art's `quick_search_fts(id, source_type, all_text)` gating index
//! (jonudell/xmlui-mastodon) to the #230 common schema.

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Result};

/// Bump when the on-disk schema shape changes. The index is a rebuildable
/// cache, so a version mismatch just drops and recreates — no migration.
// Bumped to force a full rebuild when stored row values change but the per-doc
// change token (mtime) doesn't — so unchanged docs re-index instead of keeping
// stale values. v3: session/history `link` → internal tab routes. v4: history
// `source` → descriptive item id(s) instead of the "1 applied" summary.
const SCHEMA_VERSION: i64 = 5;

/// A row to index. `content` is the searchable text; `file` is the source
/// file's absolute path (the reindex key); the rest are the #230 common-schema
/// display columns.
#[derive(Debug, Clone)]
pub struct IndexRow {
    /// The `type` column: session | commit | issue | worklist-history.
    pub kind: String,
    pub source: String,
    pub date: String,
    pub link: String,
    pub content: String,
    pub file: String,
}

/// A unified search hit: the common-schema display columns plus the FTS5
/// `snippet()` excerpt and `bm25` relevance rank (lower = better).
#[derive(Debug, Clone)]
pub struct Hit {
    pub kind: String,
    pub source: String,
    pub date: String,
    pub link: String,
    pub snippet: String,
    pub rank: f64,
    /// The doc key (the `file` column) — lets the inline expander fetch the
    /// full stored content via get_doc / the /__search/doc route.
    pub key: String,
}

/// Open (or create) the index at `path`, in WAL mode, with the current schema.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL lets the future read route run while the indexer writes. No-op on
    // :memory:, so we ignore the result.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    ensure_schema(&conn)?;
    Ok(conn)
}

/// In-memory index — used by tests.
#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    ensure_schema(&conn)?;
    Ok(conn)
}

/// Ensure the schema exists at the current version. On a version mismatch,
/// drop and recreate (the index is a rebuildable cache).
fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != SCHEMA_VERSION {
        conn.execute_batch(
            "DROP TABLE IF EXISTS search_index; DROP TABLE IF EXISTS indexed_files;",
        )?;
    }
    // `file` is UNINDEXED: stored (so we could show/debug it) but not
    // tokenized. `CREATE VIRTUAL TABLE … USING fts5` fails unless FTS5 is
    // compiled in — which the smoke test relies on.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS search_index \
           USING fts5(type, source, date, link, content, file UNINDEXED, tokenize='unicode61'); \
         CREATE TABLE IF NOT EXISTS indexed_files( \
           path TEXT PRIMARY KEY, mtime INTEGER, size INTEGER, \
           rowid_ref INTEGER, indexed_at INTEGER);",
    )?;
    if version != SCHEMA_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    }
    Ok(())
}

/// Cheap check (no file read) — has this file changed since it was last
/// indexed? True when unseen or when mtime/size differ.
pub fn needs_index(conn: &Connection, path: &str, mtime: i64, size: i64) -> Result<bool> {
    let seen: Option<(i64, i64)> = conn
        .query_row(
            "SELECT mtime, size FROM indexed_files WHERE path = ?1",
            params![path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match seen {
        Some((m, s)) => m != mtime || s != size,
        None => true,
    })
}

/// Upsert one document as a single index row, keyed by `row.file` (a generic
/// doc key: a session file path, `commit:<sha>`, or `issue:<number>`). Deletes
/// the key's prior row (by stored rowid) before inserting, then records the
/// change token (`mtime`) + `size` so the next pass can skip it unchanged.
pub fn index_doc(conn: &Connection, row: &IndexRow, mtime: i64, size: i64) -> Result<()> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT rowid_ref FROM indexed_files WHERE path = ?1",
            params![row.file],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = existing {
        conn.execute("DELETE FROM search_index WHERE rowid = ?1", params![old])?;
    }
    conn.execute(
        "INSERT INTO search_index(type, source, date, link, content, file) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![row.kind, row.source, row.date, row.link, row.content, row.file],
    )?;
    let new_rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT OR REPLACE INTO indexed_files(path, mtime, size, rowid_ref, indexed_at) \
         VALUES (?1, ?2, ?3, ?4, CAST(strftime('%s','now') AS INTEGER))",
        params![row.file, mtime, size, new_rowid],
    )?;
    Ok(())
}

/// Total indexed rows — for the scan trace.
pub fn row_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT count(*) FROM search_index", [], |r| r.get(0))
}

/// Full-text query across buckets, `bm25`-ranked (best first), with a
/// highlighted `snippet()` of the matched `content` column (index 4). `q` must
/// be a valid FTS5 MATCH expression — callers sanitize raw user input. When
/// `types` is non-empty, results are restricted to those `type` values (the
/// Search page's facet filter).
pub fn query(conn: &Connection, q: &str, limit: usize, types: &[String]) -> Result<Vec<Hit>> {
    let mut sql = String::from(
        "SELECT type, source, date, link, \
                snippet(search_index, 4, '[', ']', '…', 10), \
                bm25(search_index), file \
         FROM search_index WHERE search_index MATCH ?",
    );
    let mut args: Vec<Value> = vec![Value::Text(q.to_string())];
    if !types.is_empty() {
        sql.push_str(" AND type IN (");
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                sql.push(',');
            }
            sql.push('?');
            args.push(Value::Text(t.clone()));
        }
        sql.push(')');
    }
    sql.push_str(" ORDER BY bm25(search_index) LIMIT ?");
    args.push(Value::Integer(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok(Hit {
            kind: r.get(0)?,
            source: r.get(1)?,
            date: r.get(2)?,
            link: r.get(3)?,
            snippet: r.get(4)?,
            rank: r.get(5)?,
            key: r.get(6)?,
        })
    })?;
    rows.collect()
}

/// Fetch one doc's full stored content by its key (the `file` column) — backs
/// the inline expander's /__search/doc route for commit/issue/history.
pub fn get_doc(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT content FROM search_index WHERE file = ?1 LIMIT 1",
        params![key],
        |r| r.get::<_, String>(0),
    )
    .optional()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, file: &str, source: &str, content: &str) -> IndexRow {
        IndexRow {
            kind: kind.into(),
            source: source.into(),
            date: "2026-07-25".into(),
            link: format!("/{kind}"),
            content: content.into(),
            file: file.into(),
        }
    }

    fn seed(conn: &Connection) {
        for (kind, file, source, content) in [
            ("session", "/s/1.jsonl", "Fix drag reorder", "the queue items are now drag reorderable via dnd-list"),
            ("commit", "/c/abc", "abc123", "Make queue items drag-reorderable via vendored xmlui-dnd-list"),
            ("issue", "/i/230", "#230", "unified full text search across sessions commits issues"),
        ] {
            index_doc(conn, &row(kind, file, source, content), 1, 1).unwrap();
        }
    }

    #[test]
    fn fts5_is_compiled_in_and_matches() {
        // CREATE VIRTUAL TABLE … USING fts5 in ensure_schema() fails unless
        // FTS5 is compiled into the bundled SQLite — this is the guardrail.
        let conn = open_in_memory().expect("FTS5 must be compiled into bundled SQLite");
        seed(&conn);

        let hits = query(&conn, "drag", 10, &[]).unwrap();
        assert_eq!(hits.len(), 2, "two rows mention drag");
        assert!(hits.iter().all(|h| h.snippet.contains('[')), "snippet() highlights");
        let ranks: Vec<f64> = hits.iter().map(|h| h.rank).collect();
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "bm25-ordered");

        let issues = query(&conn, "unified", 10, &[]).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, "issue");

        // Facet filter restricts to the requested types.
        let only_commit = query(&conn, "queue", 10, &["commit".to_string()]).unwrap();
        assert!(only_commit.iter().all(|h| h.kind == "commit"), "types filter applied");
        assert!(!only_commit.is_empty(), "commit row matches 'queue'");
    }

    #[test]
    fn skip_unchanged_and_reindex_changed() {
        let conn = open_in_memory().unwrap();
        index_doc(&conn, &row("session", "/s/1.jsonl", "t", "alpha content"), 100, 10).unwrap();
        assert_eq!(row_count(&conn).unwrap(), 1);

        // Same mtime/size → skip (no duplicate).
        assert!(!needs_index(&conn, "/s/1.jsonl", 100, 10).unwrap());

        // Changed mtime → reindex replaces the row, not duplicates it.
        assert!(needs_index(&conn, "/s/1.jsonl", 200, 12).unwrap());
        index_doc(&conn, &row("session", "/s/1.jsonl", "t", "beta content"), 200, 12).unwrap();
        assert_eq!(row_count(&conn).unwrap(), 1, "reindex replaces, not appends");
        assert_eq!(query(&conn, "alpha", 10, &[]).unwrap().len(), 0, "old content gone");
        assert_eq!(query(&conn, "beta", 10, &[]).unwrap().len(), 1, "new content present");
    }
}
