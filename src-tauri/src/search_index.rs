//! issue-230-sqlite-fts-foundation: embedded SQLite (via `rusqlite` with the
//! `bundled` feature, which statically links SQLite and compiles FTS5 in by
//! default) plus the combined full-text index shape for the unified search in
//! #230. This module is the load-bearing foundation ONLY — there is no
//! background indexer, host route, or UI here yet; those are later #230
//! phases. See resources/worklist-drafts/issue-230-sqlite-fts-foundation.md.
//!
//! The combined table generalizes the "fast gating" index from the Go prior
//! art (jonudell/xmlui-mastodon `quick_search_fts(id, source_type, all_text)`)
//! to the #230 common schema: type / source / date / link / content.

// The whole module is scaffolding until the indexer + route land in later
// phases; unused-until-wired functions are expected.
#![allow(dead_code)]

use rusqlite::{params, Connection, Result};

/// A row to index. `content` is the searchable text; the other fields are the
/// common-schema display columns carried alongside it.
#[derive(Debug, Clone)]
pub struct IndexRow {
    /// The `type` column: session | commit | issue | worklist-history.
    pub kind: String,
    pub source: String,
    pub date: String,
    pub link: String,
    pub content: String,
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
}

/// Open (or create) the index at `path` and ensure the schema exists. WAL so
/// later indexing writes don't block concurrent reads.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // journal_mode is a no-op for :memory:; only meaningful for a file db.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    init_schema(&conn)?;
    Ok(conn)
}

/// In-memory index — used by the smoke test and handy for future unit tests.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init_schema(&conn)?;
    Ok(conn)
}

/// The combined FTS5 table on the #230 common schema. `CREATE VIRTUAL TABLE …
/// USING fts5` fails unless FTS5 is compiled into the linked SQLite — which is
/// exactly what the smoke test relies on to prove the bundled build.
fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS search_index \
         USING fts5(type, source, date, link, content, tokenize='unicode61');",
    )
}

/// Insert one row into the index.
pub fn insert(conn: &Connection, row: &IndexRow) -> Result<()> {
    conn.execute(
        "INSERT INTO search_index(type, source, date, link, content) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![row.kind, row.source, row.date, row.link, row.content],
    )?;
    Ok(())
}

/// Full-text query across all buckets, `bm25`-ranked (best first), with a
/// highlighted `snippet()` of the matched `content` column. This replaces the
/// hand-rolled `find_snippets()` ±40-char windows once the indexer is wired.
pub fn query(conn: &Connection, q: &str, limit: usize) -> Result<Vec<Hit>> {
    // content is column index 4 (type=0, source=1, date=2, link=3, content=4).
    let mut stmt = conn.prepare(
        "SELECT type, source, date, link, \
                snippet(search_index, 4, '[', ']', '…', 10), \
                bm25(search_index) \
         FROM search_index WHERE search_index MATCH ?1 \
         ORDER BY bm25(search_index) LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![q, limit as i64], |r| {
        Ok(Hit {
            kind: r.get(0)?,
            source: r.get(1)?,
            date: r.get(2)?,
            link: r.get(3)?,
            snippet: r.get(4)?,
            rank: r.get(5)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(conn: &Connection) {
        for (kind, source, content) in [
            (
                "session",
                "sess-1 Fix drag reorder",
                "the queue items are now drag reorderable via the dnd-list extension",
            ),
            (
                "commit",
                "abc123",
                "Make queue items drag-reorderable via vendored xmlui-dnd-list",
            ),
            (
                "issue",
                "#230 unified search",
                "unified full text search across sessions commits issues and history",
            ),
        ] {
            insert(
                conn,
                &IndexRow {
                    kind: kind.into(),
                    source: source.into(),
                    date: "2026-07-25".into(),
                    link: format!("/{kind}"),
                    content: content.into(),
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn fts5_is_compiled_in_and_matches() {
        // If FTS5 were not compiled into the bundled SQLite, open_in_memory()
        // would error here on CREATE VIRTUAL TABLE … USING fts5 — so this line
        // is the guardrail that the bundled build includes FTS5.
        let conn = open_in_memory().expect("FTS5 must be compiled into bundled SQLite");
        seed(&conn);

        // "drag" appears in the session and commit rows (unicode61 splits the
        // hyphen in "drag-reorderable"), not the issue row.
        let hits = query(&conn, "drag", 10).unwrap();
        assert_eq!(hits.len(), 2, "two rows mention drag");
        assert!(
            hits.iter().all(|h| h.snippet.contains('[')),
            "snippet() wraps the matched term"
        );

        // bm25 returns lower-is-better; ORDER BY bm25 yields ascending ranks.
        let ranks: Vec<f64> = hits.iter().map(|h| h.rank).collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "results are bm25-ordered"
        );

        // The type discriminator supports faceting across buckets.
        let issues = query(&conn, "unified", 10).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, "issue");
    }
}
