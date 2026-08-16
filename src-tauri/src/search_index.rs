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
// v8: add the `extra` column carrying a hit's self-contained structured detail
// (the serialized WorklistHistoryGroup for history rows), so the expander needs
// no recency-limited re-fetch.
// v9: issue rows now store the fully-enriched issue payload in `extra` (for
// cache-first /__issue serving) plus a synthetic "issues:list" row. Existing
// issue rows carry empty `extra` at the old token, so force a rebuild to
// backfill them (indexed=0/skipped=230 otherwise).
// v10: Claude session extraction includes tool_result / tool_use / thinking
// text (search-index-tool-output-coverage). v11: Codex extraction includes its
// tool records too (custom_tool_call[_output], function_call, patch_apply_end —
// search-index-codex-tool-output-coverage). v12 adds a lower-weight `intent`
// column for already-cached Haiku descriptions plus a `session_tools` routing
// table for targeted refreshes. Each bump forces a rebuild that back-indexes
// all sessions with the wider coverage.
const SCHEMA_VERSION: i64 = 12;

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
    /// Supplemental generated intent text, weighted below primary content.
    pub intent: String,
    pub file: String,
    /// Tool-call ids found in this session. `None` preserves an existing
    /// mapping during an intent-only refresh; non-session rows also use None.
    pub tool_ids: Option<Vec<String>>,
    /// Self-contained structured detail for the hit (e.g. the serialized
    /// WorklistHistoryGroup), so the expander needs no re-fetch. Empty for
    /// buckets whose detail is fetched live.
    pub extra: String,
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
    /// Self-contained structured detail (serialized JSON), empty when the
    /// bucket fetches detail live. History rows carry the WorklistHistoryGroup.
    pub extra: String,
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
            "DROP TABLE IF EXISTS search_index; \
             DROP TABLE IF EXISTS indexed_files; \
             DROP TABLE IF EXISTS session_tools;",
        )?;
    }
    // `file` is UNINDEXED: stored (so we could show/debug it) but not
    // tokenized. `CREATE VIRTUAL TABLE … USING fts5` fails unless FTS5 is
    // compiled in — which the smoke test relies on.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS search_index \
           USING fts5(type, source, date, link, content, intent, file UNINDEXED, extra UNINDEXED, tokenize='unicode61'); \
         CREATE TABLE IF NOT EXISTS indexed_files( \
           path TEXT PRIMARY KEY, mtime INTEGER, size INTEGER, \
           rowid_ref INTEGER, indexed_at INTEGER); \
         CREATE TABLE IF NOT EXISTS session_tools( \
           tool_id TEXT NOT NULL, session_path TEXT NOT NULL, \
           PRIMARY KEY(tool_id, session_path)); \
         CREATE INDEX IF NOT EXISTS session_tools_path \
           ON session_tools(session_path);",
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
    let tx = conn.unchecked_transaction()?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT rowid_ref FROM indexed_files WHERE path = ?1",
            params![row.file],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = existing {
        tx.execute("DELETE FROM search_index WHERE rowid = ?1", params![old])?;
    }
    tx.execute(
        "INSERT INTO search_index(type, source, date, link, content, intent, file, extra) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.kind,
            row.source,
            row.date,
            row.link,
            row.content,
            row.intent,
            row.file,
            row.extra
        ],
    )?;
    let new_rowid = tx.last_insert_rowid();
    tx.execute(
        "INSERT OR REPLACE INTO indexed_files(path, mtime, size, rowid_ref, indexed_at) \
         VALUES (?1, ?2, ?3, ?4, CAST(strftime('%s','now') AS INTEGER))",
        params![row.file, mtime, size, new_rowid],
    )?;
    if row.kind == "session" {
        if let Some(tool_ids) = &row.tool_ids {
            tx.execute(
                "DELETE FROM session_tools WHERE session_path = ?1",
                params![row.file],
            )?;
            {
                let mut insert_tool = tx.prepare_cached(
                    "INSERT OR IGNORE INTO session_tools(tool_id, session_path) VALUES (?1, ?2)",
                )?;
                for tool_id in tool_ids {
                    insert_tool.execute(params![tool_id, row.file])?;
                }
            }
        }
    }
    tx.commit()
}

/// Resolve cached-description tool ids to the session documents that contain
/// them. This is read by the single indexer thread before a targeted refresh.
pub fn session_paths_for_tools(conn: &Connection, tool_ids: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT session_path FROM session_tools WHERE tool_id = ?1 ORDER BY session_path",
    )?;
    for tool_id in tool_ids {
        let paths = stmt
            .query_map(params![tool_id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>>>()?;
        for path in paths {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Reconcile the index against the filesystem: remove rows whose file-backed
/// source no longer exists. Only `indexed_files.path` values that are absolute
/// filesystem paths (start with '/') are file-backed — session transcripts
/// (Claude + Codex). Synthetic keys (`commit:<sha>`, `issue:<n>`,
/// `history:<ts>`) never start with '/' and are always exempt. Returns the
/// number of docs pruned.
///
/// Needed because the incremental passes only walk *existing* files, so a
/// deleted transcript (e.g. Claude Code's 30-day cleanup sweep, which a Bram
/// relaunch triggers) would otherwise leave an orphaned row that surfaces as a
/// phantom search hit whose detail view is empty (search-index-prune-orphaned-rows).
pub fn prune_missing_files(conn: &Connection) -> Result<usize> {
    let mut stmt =
        conn.prepare("SELECT path, rowid_ref FROM indexed_files WHERE path LIKE '/%'")?;
    let candidates = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<(String, i64)>>>()?;
    drop(stmt);
    let mut removed = 0usize;
    for (path, rowid) in candidates {
        if std::path::Path::new(&path).exists() {
            continue;
        }
        conn.execute("DELETE FROM search_index WHERE rowid = ?1", params![rowid])?;
        conn.execute("DELETE FROM indexed_files WHERE path = ?1", params![path])?;
        conn.execute(
            "DELETE FROM session_tools WHERE session_path = ?1",
            params![path],
        )?;
        removed += 1;
    }
    Ok(removed)
}

/// Total indexed rows — for the scan trace.
/// Newest issue number present in the issues bucket (0 when none) — the
/// Search page's coverage line (issue #251): "issues through #N" tells a user
/// whether an absent result could be un-indexed vs. simply not a keyword
/// match. Keys are `issue:<number>`, so numeric max over the suffix.
pub fn newest_issue_number(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(CAST(SUBSTR(file, 7) AS INTEGER)), 0) FROM search_index \
         WHERE file LIKE 'issue:%'",
        [],
        |r| r.get(0),
    )
}

pub fn row_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT count(*) FROM search_index", [], |r| r.get(0))
}

/// Indexed row count per `type` bucket, ordered by type. Powers the Status
/// tab's Search-indexer section.
pub fn counts_by_type(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt =
        conn.prepare("SELECT type, count(*) FROM search_index GROUP BY type ORDER BY type")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

/// List rows of one `type` bucket, newest-first (no full-text match) — for a
/// reverse-chron browse like the History tab. `date` is epoch seconds stored as
/// text, so sort it numerically. Cheap: an indexed ORDER BY / LIMIT, not a file
/// scan. `snippet`/`rank` are unused here (empty / 0).
pub fn list_by_type(conn: &Connection, kind: &str, limit: usize) -> Result<Vec<Hit>> {
    let mut stmt = conn.prepare(
        "SELECT type, source, date, link, '', 0.0, file, extra \
         FROM search_index WHERE type = ?1 \
         ORDER BY CAST(date AS INTEGER) DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![kind, limit as i64], |r| {
            Ok(Hit {
                kind: r.get(0)?,
                source: r.get(1)?,
                date: r.get(2)?,
                link: r.get(3)?,
                snippet: r.get(4)?,
                rank: r.get(5)?,
                key: r.get(6)?,
                extra: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

/// Full-text query across buckets, `bm25`-ranked (best first), with a
/// highlighted `snippet()` from the best matching column. Existing columns
/// retain weight 1.0; generated intent is deliberately supplemental at 0.35.
/// `q` must be a valid FTS5 MATCH expression — callers sanitize raw user input.
/// When `types` is non-empty, results are restricted to those `type` values.
pub fn query(conn: &Connection, q: &str, limit: usize, types: &[String]) -> Result<Vec<Hit>> {
    let mut sql = String::from(
        "SELECT type, source, date, link, \
                snippet(search_index, -1, '[', ']', '…', 40), \
                bm25(search_index, 1.0, 1.0, 1.0, 1.0, 1.0, 0.35), file, extra \
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
    sql.push_str(
        " ORDER BY bm25(search_index, 1.0, 1.0, 1.0, 1.0, 1.0, 0.35) LIMIT ?",
    );
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
            extra: r.get(7)?,
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

/// Read the `extra` column for a doc key (the cached structured payload).
pub fn get_extra(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT extra FROM search_index WHERE file = ?1 LIMIT 1",
        params![key],
        |r| r.get::<_, String>(0),
    )
    .optional()
}

// --- Bounded commit-patch capture (issue #250) ---------------------------
//
// The commit indexer feeds `git show` output through these types so that a
// commit's raw patch is sanitized WHILE it is read: retained bytes per commit
// are bounded by PATCH_MAX_TOTAL (+ one line buffer) no matter how large the
// raw patch is. The v0.3.14 cold rebuild accumulated every raw patch before
// sanitizing and drove Bram to ~6 GB RSS on a heavily rewritten repo; one
// commit's raw patch alone was 41.8 MB.

/// Lines longer than this collapse to a marker (minified vendor bumps ride on
/// one line).
pub const PATCH_MAX_LINE: usize = 2000;
/// Retained sanitized text per commit caps here.
pub const PATCH_MAX_TOTAL: usize = 256 * 1024;

/// Incremental, bounded sanitizer for one commit patch. Byte-feed; line
/// buffering stops at PATCH_MAX_LINE + 1 (enough to know the line is long),
/// so a multi-megabyte single line never materializes. Semantics match the
/// historical batch sanitize: long lines become "[long line elided]\n",
/// retention stops at PATCH_MAX_TOTAL with dropped bytes counted as
/// stripped-line length + 1, and a final "[patch truncated]\n" marker lands
/// when anything was dropped.
pub struct PatchSanitizer {
    out: String,
    line: Vec<u8>,
    line_len: usize, // full length of the current line (bytes), even when not buffered
    last_byte_cr: bool,
    elided: usize,
    dropped: usize,
}

impl PatchSanitizer {
    pub fn new() -> Self {
        Self {
            out: String::new(),
            line: Vec::new(),
            line_len: 0,
            last_byte_cr: false,
            elided: 0,
            dropped: 0,
        }
    }

    fn end_line(&mut self) {
        // `str::lines()` parity: a trailing \r is part of the terminator.
        let mut len = self.line_len;
        if self.last_byte_cr {
            len -= 1;
            if self.line.len() == self.line_len {
                self.line.pop();
            }
        }
        if self.out.len() >= PATCH_MAX_TOTAL {
            self.dropped += len + 1;
        } else if len > PATCH_MAX_LINE {
            self.elided += 1;
            self.out.push_str("[long line elided]\n");
        } else {
            self.out
                .push_str(&String::from_utf8_lossy(&self.line[..len.min(self.line.len())]));
            self.out.push('\n');
        }
        self.line.clear();
        self.line_len = 0;
        self.last_byte_cr = false;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' {
                self.end_line();
                continue;
            }
            self.line_len += 1;
            self.last_byte_cr = b == b'\r';
            // Buffer one byte past the elision threshold so end_line can tell
            // long from short; the rest of a long line is counted, not stored.
            if self.line.len() <= PATCH_MAX_LINE {
                self.line.push(b);
            }
        }
    }

    /// Retained bytes right now (sanitized text + line buffer) — the bound
    /// the regression test asserts.
    pub fn retained_bytes(&self) -> usize {
        self.out.len() + self.line.len()
    }

    pub fn finish(mut self) -> (String, usize, usize) {
        if self.line_len > 0 {
            self.end_line();
        }
        if self.dropped > 0 {
            self.out.push_str("[patch truncated]\n");
        }
        (self.out, self.elided, self.dropped)
    }
}

/// Batch form, retained for parity testing: identical output to feeding the
/// bytes through PatchSanitizer.
#[cfg(test)]
pub fn sanitize_commit_patch(patch: &str) -> (String, usize, usize) {
    let mut s = PatchSanitizer::new();
    s.feed(patch.as_bytes());
    s.finish()
}

/// Streaming parser for `git show --format=%x1e%H%x1f --patch` output:
/// splits records on \x1e, the sha header on \x1f, and runs each record's
/// patch body through a PatchSanitizer as bytes arrive. Completed records
/// are returned from feed()/finish() as (sha, (sanitized, elided, dropped)).
pub struct PatchStream {
    header: Vec<u8>,
    in_body: bool,
    sanitizer: PatchSanitizer,
    max_retained: usize,
}

impl PatchStream {
    pub fn new() -> Self {
        Self {
            header: Vec::new(),
            in_body: false,
            sanitizer: PatchSanitizer::new(),
            max_retained: 0,
        }
    }

    fn take_record(&mut self) -> Option<(String, (String, usize, usize))> {
        let sha = String::from_utf8_lossy(&self.header).trim().to_string();
        self.header.clear();
        let sanitizer = std::mem::replace(&mut self.sanitizer, PatchSanitizer::new());
        self.in_body = false;
        if sha.is_empty() {
            return None;
        }
        Some((sha, sanitizer.finish()))
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(String, (String, usize, usize))> {
        let mut done = Vec::new();
        let mut rest = chunk;
        while !rest.is_empty() {
            if !self.in_body {
                // Header: bytes up to \x1f are the sha; \x1e closes a record
                // that had no body (or leading noise before the first record).
                match rest.iter().position(|&b| b == 0x1f || b == 0x1e) {
                    Some(pos) => {
                        self.header.extend_from_slice(&rest[..pos]);
                        let delim = rest[pos];
                        rest = &rest[pos + 1..];
                        if delim == 0x1f {
                            self.in_body = true;
                        } else if let Some(rec) = self.take_record() {
                            done.push(rec);
                        }
                    }
                    None => {
                        self.header.extend_from_slice(rest);
                        rest = &[];
                    }
                }
            } else {
                match rest.iter().position(|&b| b == 0x1e) {
                    Some(pos) => {
                        self.sanitizer.feed(&rest[..pos]);
                        self.max_retained = self.max_retained.max(self.sanitizer.retained_bytes());
                        rest = &rest[pos + 1..];
                        if let Some(rec) = self.take_record() {
                            done.push(rec);
                        }
                    }
                    None => {
                        self.sanitizer.feed(rest);
                        self.max_retained = self.max_retained.max(self.sanitizer.retained_bytes());
                        rest = &[];
                    }
                }
            }
        }
        done
    }

    pub fn finish(mut self) -> (Vec<(String, (String, usize, usize))>, usize) {
        let max = self.max_retained;
        let mut done = Vec::new();
        if let Some(rec) = self.take_record() {
            done.push(rec);
        }
        (done, max)
    }
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
            intent: String::new(),
            file: file.into(),
            tool_ids: None,
            extra: String::new(),
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

    #[test]
    fn generated_intent_is_searchable_but_ranked_below_primary_content() {
        let conn = open_in_memory().unwrap();
        let raw = row("session", "/s/raw.jsonl", "raw", "inspect durable cache");
        index_doc(&conn, &raw, 1, 1).unwrap();
        let mut enriched = row(
            "session",
            "/s/intent.jsonl",
            "intent",
            "unrelated primary words",
        );
        enriched.intent = "inspect durable cache".to_string();
        index_doc(&conn, &enriched, 1, 1).unwrap();

        let hits = query(&conn, "inspect", 10, &["session".to_string()]).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].key, "/s/raw.jsonl",
            "raw transcript text should rank first"
        );
        assert!(
            hits.iter()
                .find(|h| h.key == "/s/intent.jsonl")
                .unwrap()
                .snippet
                .contains("[inspect]"),
            "an intent-only match should provide a visible highlighted snippet"
        );
    }

    #[test]
    fn session_tool_mapping_tracks_replacement_and_supports_targeting() {
        let conn = open_in_memory().unwrap();
        let mut first = row("session", "/s/1.jsonl", "one", "alpha");
        first.tool_ids = Some(vec!["tool-a".into(), "tool-b".into()]);
        index_doc(&conn, &first, 1, 1).unwrap();
        assert_eq!(
            session_paths_for_tools(&conn, &["tool-b".into()]).unwrap(),
            vec!["/s/1.jsonl"]
        );

        let mut replacement = row("session", "/s/1.jsonl", "one", "beta");
        replacement.tool_ids = Some(vec!["tool-c".into()]);
        index_doc(&conn, &replacement, 2, 2).unwrap();
        assert!(session_paths_for_tools(&conn, &["tool-b".into()])
            .unwrap()
            .is_empty());
        assert_eq!(
            session_paths_for_tools(&conn, &["tool-c".into()]).unwrap(),
            vec!["/s/1.jsonl"]
        );

        let intent_only = row("session", "/s/1.jsonl", "one", "gamma");
        index_doc(&conn, &intent_only, 2, 2).unwrap();
        assert_eq!(
            session_paths_for_tools(&conn, &["tool-c".into()]).unwrap(),
            vec!["/s/1.jsonl"],
            "intent-only row replacement must preserve the routing map"
        );
    }

    // issue-250 regression: multi-megabyte historical patches must index with
    // truncation AND with bounded retained bytes during capture.
    #[test]
    fn patch_stream_bounds_retention_and_matches_batch_sanitize() {
        // Three commits: a 5 MB many-line patch, a single 4 MB line, a small one.
        let big_lines = format!("{}\n", "x".repeat(200));
        let big = big_lines.repeat(25_000); // ~5 MB
        let one_line = format!("diff --git a b\n{}\n", "y".repeat(4 * 1024 * 1024));
        let small = "diff --git a b\n+hello\n".to_string();
        let raw = format!(
            "\u{1e}aaa\u{1f}{}\u{1e}bbb\u{1f}{}\u{1e}ccc\u{1f}{}",
            big, one_line, small
        );

        let mut stream = PatchStream::new();
        let mut records = Vec::new();
        for chunk in raw.as_bytes().chunks(8 * 1024) {
            records.extend(stream.feed(chunk));
        }
        let (tail, max_retained) = stream.finish();
        records.extend(tail);

        assert_eq!(
            records.iter().map(|(sha, _)| sha.as_str()).collect::<Vec<_>>(),
            vec!["aaa", "bbb", "ccc"]
        );
        // Streaming output is byte-identical to the historical batch sanitize.
        for (sha, raw_patch) in [("aaa", &big), ("bbb", &one_line), ("ccc", &small)] {
            let batch = sanitize_commit_patch(raw_patch);
            let streamed = &records.iter().find(|(s, _)| s == sha).unwrap().1;
            assert_eq!(streamed, &batch, "parity for {}", sha);
        }
        // Truncation happened where it should…
        assert!(records[0].1 .0.ends_with("[patch truncated]\n"));
        assert!(records[0].1 .2 > 4 * 1024 * 1024, "big patch dropped bytes");
        assert!(records[1].1 .0.contains("[long line elided]"));
        assert_eq!(records[1].1 .1, 1, "one elided line");
        assert_eq!(records[2].1 .2, 0, "small patch intact");
        // …and capture never retained more than the cap + one line buffer.
        assert!(
            max_retained <= PATCH_MAX_TOTAL + PATCH_MAX_LINE + 2,
            "retained {} exceeds bound",
            max_retained
        );
    }

    #[test]
    fn prune_removes_missing_file_rows_only() {
        use std::io::Write;
        let conn = open_in_memory().unwrap();

        // A real, existing transcript path → its row must survive.
        let mut present = std::env::temp_dir();
        present.push("bram-prune-present.jsonl");
        writeln!(std::fs::File::create(&present).unwrap(), "x").unwrap();
        let present_key = present.to_string_lossy().to_string();

        index_doc(&conn, &row("session", &present_key, "present", "alpha"), 1, 1).unwrap();
        index_doc(&conn, &row("session", "/no/such/gone-session.jsonl", "gone", "beta"), 1, 1)
            .unwrap();
        // Synthetic key (real commit-row format) — no leading '/', must be exempt.
        index_doc(&conn, &row("commit", "commit:abc123", "abc", "gamma"), 1, 1).unwrap();
        assert_eq!(row_count(&conn).unwrap(), 3);

        let removed = prune_missing_files(&conn).unwrap();
        assert_eq!(removed, 1, "only the missing-file row is pruned");
        assert_eq!(row_count(&conn).unwrap(), 2);
        assert_eq!(query(&conn, "beta", 10, &[]).unwrap().len(), 0, "gone transcript pruned");
        assert_eq!(query(&conn, "alpha", 10, &[]).unwrap().len(), 1, "present transcript kept");
        assert_eq!(query(&conn, "gamma", 10, &[]).unwrap().len(), 1, "synthetic key exempt");

        let _ = std::fs::remove_file(&present);
    }
}
