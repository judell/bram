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

// --- item sessions (item-last-session-switch-there) -------------------------
//
// The ledger's first real reader. Bram records which session received an
// item's Start / Commit / Drop / feedback turn, as a `kind='session'`
// transitions row, so the Worklist can offer "Last worked on in '…' ·
// Switch there". Written only when the session differs from the item's
// latest one, so ordinary turns add no rows. Sessions and this db are both
// per machine, so the fact lives where it's true. A failed write is a missed
// stamp, healed by the item's next delivery (the latest row still differs).

fn session_id_is_plain(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The item's most recent recorded session: (provider, session id, at_ms).
pub fn latest_item_session(
    conn: &Connection,
    item_id: &str,
) -> Result<Option<(String, String, i64)>> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT detail, at_ms FROM transitions WHERE item_id = ?1 AND kind = 'session' \
             ORDER BY id DESC LIMIT 1",
            params![item_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(detail, at)| {
        let v: serde_json::Value = serde_json::from_str(&detail).ok()?;
        let provider = v.get("provider")?.as_str()?.to_string();
        let sid = v.get("sessionId")?.as_str()?.to_string();
        Some((provider, sid, at))
    }))
}

/// Record that `item_id`'s turn was delivered to (provider, session). Returns
/// Ok(true) when a row was written, Ok(false) when it matched the latest
/// row (or the ids weren't plain, which stores nothing rather than guess).
pub fn record_item_session(
    conn: &Connection,
    item_id: &str,
    provider: &str,
    session_id: &str,
    at_ms: i64,
) -> Result<bool> {
    if !session_id_is_plain(provider) || !session_id_is_plain(session_id) {
        return Ok(false);
    }
    if let Some((p, sid, _)) = latest_item_session(conn, item_id)? {
        if p == provider && sid == session_id {
            return Ok(false);
        }
    }
    let detail = format!(
        "{{\"provider\":\"{}\",\"sessionId\":\"{}\"}}",
        provider, session_id
    );
    append_transition(conn, at_ms, Some(item_id), "session", &detail, "delivery")?;
    Ok(true)
}

// --- session groups (session-groups) ----------------------------------------
//
// A group is a line of work: at most one session per agent, any number of
// agents, and a session is in at most one group. Switching agents from a
// group member resumes that agent's member. Groups replaced pairs on
// 2026-10-28: with three agents a team was three pairs that could disagree
// with each other (Codex and OpenCode "v0.8.0 burndown" both paired with
// Claude "v0.8.0 burndown" but not with each other, and the offer had to
// explain an Oct 23 pairing no screen showed).
//
// Stored as `kind='group'` transitions rows, append-only and replayed in
// order:
//   {"op":"join","group":<g>,"provider":<p>,"id":<id>}  moves (p, id) out of
//       any other group into g, replacing g's current member for p
//   {"op":"leave","provider":<p>,"id":<id>}
//   {"op":"migrated"}  the old pair rows were converted (once)
// A group left with fewer than two members dissolves.
//
// The old `kind='pair'` rows ({"claude": <id>, "codex": <id>}, an empty side
// for an unpair) stay as history; `migrate_pairs_to_groups` converts the
// pairs still holding when it first runs, and `session_never_paired` still
// reads them.

const PAIR_PROVIDERS: [&str; 3] = ["claude", "codex", "opencode"];

fn is_pair_provider(p: &str) -> bool {
    PAIR_PROVIDERS.contains(&p)
}

fn provider_rank(p: &str) -> usize {
    PAIR_PROVIDERS
        .iter()
        .position(|q| *q == p)
        .unwrap_or(usize::MAX)
}

type SessionKey = (String, String);

/// A group as callers see it: its id, its members in PAIR_PROVIDERS order,
/// and the member it started with (its name is that session's title).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionGroup {
    pub id: String,
    pub members: Vec<SessionKey>,
    pub origin: SessionKey,
}

struct GroupRec {
    members: std::collections::BTreeMap<usize, SessionKey>,
    origin: SessionKey,
}

/// Every group, replayed once. Build it with `group_snapshot` when asking
/// about many sessions (the Sessions list).
#[derive(Default)]
pub struct GroupSnapshot {
    groups: std::collections::HashMap<String, GroupRec>,
    of: std::collections::HashMap<SessionKey, String>,
    // (provider, id, other provider): ever in one group with a session of
    // the other provider. session_never_paired reads it.
    ever: std::collections::HashSet<(String, String, String)>,
}

impl GroupSnapshot {
    fn remove(&mut self, key: &SessionKey) {
        let Some(gid) = self.of.remove(key) else {
            return;
        };
        let dissolve = match self.groups.get_mut(&gid) {
            Some(g) => {
                g.members.retain(|_, k| k != key);
                g.members.len() < 2
            }
            None => false,
        };
        if dissolve {
            if let Some(g) = self.groups.remove(&gid) {
                for k in g.members.values() {
                    self.of.remove(k);
                }
            }
        }
    }

    fn join(&mut self, gid: &str, key: SessionKey) {
        if self.of.get(&key).map(|g| g.as_str()) == Some(gid) {
            return;
        }
        self.remove(&key);
        let rank = provider_rank(&key.0);
        let g = self
            .groups
            .entry(gid.to_string())
            .or_insert_with(|| GroupRec {
                members: Default::default(),
                origin: key.clone(),
            });
        if let Some(old) = g.members.insert(rank, key.clone()) {
            self.of.remove(&old);
        }
        let others: Vec<SessionKey> = g.members.values().filter(|k| **k != key).cloned().collect();
        self.of.insert(key.clone(), gid.to_string());
        for (q, qid) in others {
            self.ever.insert((key.0.clone(), key.1.clone(), q.clone()));
            self.ever.insert((q, qid, key.0.clone()));
        }
    }

    /// Every group of at least two.
    pub fn groups(&self) -> Vec<SessionGroup> {
        let mut ids: Vec<&String> = self.groups.keys().collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|gid| {
                let g = &self.groups[gid];
                (g.members.len() >= 2).then(|| SessionGroup {
                    id: gid.clone(),
                    members: g.members.values().cloned().collect(),
                    origin: g.origin.clone(),
                })
            })
            .collect()
    }

    /// The group (of at least two) that (`provider`, `id`) is in.
    pub fn group_of(&self, provider: &str, id: &str) -> Option<SessionGroup> {
        let gid = self.of.get(&(provider.to_string(), id.to_string()))?;
        let g = self.groups.get(gid)?;
        if g.members.len() < 2 {
            return None;
        }
        Some(SessionGroup {
            id: gid.clone(),
            members: g.members.values().cloned().collect(),
            origin: g.origin.clone(),
        })
    }
}

pub fn group_snapshot(conn: &Connection) -> Result<GroupSnapshot> {
    let mut snap = GroupSnapshot::default();
    let mut stmt = conn.prepare(
        "SELECT detail FROM transitions WHERE kind = 'group' AND item_id IS NULL ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for detail in rows {
        let detail = detail?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&detail) else {
            continue;
        };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        match s("op").as_str() {
            "join" => snap.join(&s("group"), (s("provider"), s("id"))),
            "leave" => snap.remove(&(s("provider"), s("id"))),
            _ => {}
        }
    }
    Ok(snap)
}

/// The group (provider, sid) is in, if any.
pub fn session_group(conn: &Connection, provider: &str, sid: &str) -> Result<Option<SessionGroup>> {
    if !session_id_is_plain(sid) || !is_pair_provider(provider) {
        return Ok(None);
    }
    Ok(group_snapshot(conn)?.group_of(provider, sid))
}

fn append_group_row(
    conn: &Connection,
    detail: serde_json::Value,
    at_ms: i64,
    source: &str,
) -> Result<()> {
    append_transition(conn, at_ms, None, "group", &detail.to_string(), source)
}

fn new_group_id(at_ms: i64, key: &SessionKey) -> String {
    format!(
        "g{}-{}-{}",
        at_ms,
        key.0,
        key.1.chars().take(8).collect::<String>()
    )
}

/// Put `joiner` in a group with `anchor`. When `anchor` is in a group,
/// `joiner` moves into it (replacing that group's member on joiner's agent);
/// otherwise, when `joiner` is in one, `anchor` joins that; otherwise the two
/// start a group, named for `anchor`. Ok(false) when they already share a
/// group, or a provider or id isn't plain.
pub fn join_group(
    conn: &Connection,
    anchor: (&str, &str),
    joiner: (&str, &str),
    at_ms: i64,
    source: &str,
) -> Result<bool> {
    if !is_pair_provider(anchor.0)
        || !is_pair_provider(joiner.0)
        || anchor.0 == joiner.0
        || !session_id_is_plain(anchor.1)
        || !session_id_is_plain(joiner.1)
    {
        return Ok(false);
    }
    let snap = group_snapshot(conn)?;
    let ga = snap.group_of(anchor.0, anchor.1);
    let gj = snap.group_of(joiner.0, joiner.1);
    let row = |gid: &str, (p, id): (&str, &str)| serde_json::json!({ "op": "join", "group": gid, "provider": p, "id": id });
    match (ga, gj) {
        (Some(a), Some(j)) if a.id == j.id => Ok(false),
        (Some(a), _) => {
            append_group_row(conn, row(&a.id, joiner), at_ms, source)?;
            Ok(true)
        }
        (None, Some(j)) => {
            append_group_row(conn, row(&j.id, anchor), at_ms, source)?;
            Ok(true)
        }
        (None, None) => {
            let gid = new_group_id(at_ms, &(anchor.0.to_string(), anchor.1.to_string()));
            append_group_row(conn, row(&gid, anchor), at_ms, source)?;
            append_group_row(conn, row(&gid, joiner), at_ms, source)?;
            Ok(true)
        }
    }
}

/// Take (provider, sid) out of its group. Ok(false) when it isn't in one.
pub fn leave_group(
    conn: &Connection,
    provider: &str,
    sid: &str,
    at_ms: i64,
    source: &str,
) -> Result<bool> {
    if session_group(conn, provider, sid)?.is_none() {
        return Ok(false);
    }
    append_group_row(
        conn,
        serde_json::json!({ "op": "leave", "provider": provider, "id": sid }),
        at_ms,
        source,
    )?;
    Ok(true)
}

/// The `other`-provider member of (`provider`, `sid`)'s group.
pub fn session_partner(
    conn: &Connection,
    provider: &str,
    sid: &str,
    other: &str,
) -> Result<Option<String>> {
    if provider == other || !is_pair_provider(other) {
        return Ok(None);
    }
    Ok(session_group(conn, provider, sid)?.and_then(|g| {
        g.members
            .into_iter()
            .find(|(p, _)| p == other)
            .map(|(_, id)| id)
    }))
}

/// Every other member (provider, id) of (`provider`, `sid`)'s group, in
/// PAIR_PROVIDERS order.
#[cfg(test)]
pub fn session_partners(
    conn: &Connection,
    provider: &str,
    sid: &str,
) -> Result<Vec<(&'static str, String)>> {
    let Some(g) = session_group(conn, provider, sid)? else {
        return Ok(Vec::new());
    };
    Ok(g.members
        .into_iter()
        .filter(|(p, _)| p != provider)
        .filter_map(|(p, id)| PAIR_PROVIDERS.iter().find(|q| **q == p).map(|q| (*q, id)))
        .collect())
}

/// Pair-shaped entry point, kept for the switch paths that think in pairs.
/// Both ids: `join_group` with `a` as the anchor. One id empty: the other
/// agent's member of that session's group leaves it (the old per-pair
/// unpair). Ok(false) when nothing changed.
pub fn record_session_pair(
    conn: &Connection,
    a: &str,
    a_id: &str,
    b: &str,
    b_id: &str,
    at_ms: i64,
    source: &str,
) -> Result<bool> {
    if !is_pair_provider(a) || !is_pair_provider(b) || a == b {
        return Ok(false);
    }
    let plain_or_empty = |s: &str| s.is_empty() || session_id_is_plain(s);
    if !plain_or_empty(a_id) || !plain_or_empty(b_id) || (a_id.is_empty() && b_id.is_empty()) {
        return Ok(false);
    }
    if !a_id.is_empty() && !b_id.is_empty() {
        return join_group(conn, (a, a_id), (b, b_id), at_ms, source);
    }
    let ((kept, kept_id), gone) = if b_id.is_empty() {
        ((a, a_id), b)
    } else {
        ((b, b_id), a)
    };
    match session_partner(conn, kept, kept_id, gone)? {
        Some(id) => leave_group(conn, gone, &id, at_ms, source),
        None => Ok(false),
    }
}

// The `other` side of the latest legacy pair row naming (`side`, `sid`)
// together with `other`: Some("") for an unpair, None when no such row.
fn latest_pair_row_for(
    conn: &Connection,
    side: &str,
    sid: &str,
    other: &str,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT detail FROM transitions WHERE kind = 'pair' AND item_id IS NULL ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for detail in rows {
        let detail = detail?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&detail) else {
            continue;
        };
        if v.get(side).and_then(|x| x.as_str()) != Some(sid) {
            continue;
        }
        if let Some(theirs) = v.get(other) {
            return Ok(Some(theirs.as_str().unwrap_or("").to_string()));
        }
    }
    Ok(None)
}

// A legacy pair that still held: the partner's own latest row names it back.
fn legacy_pair_holds(conn: &Connection, a: &str, a_id: &str, b: &str, b_id: &str) -> Result<bool> {
    Ok(
        latest_pair_row_for(conn, a, a_id, b)?.as_deref() == Some(b_id)
            && latest_pair_row_for(conn, b, b_id, a)?.as_deref() == Some(a_id),
    )
}

/// Convert the legacy pairs still holding into groups, once (no group rows
/// yet). Pairs that share a session merge into one group; when a merge would
/// put two sessions of one agent in a group, the more recent pair wins and
/// the older one is returned as dropped ("<p>=<id>+<q>=<id>").
///
/// group-migration-atomic (OpenCode's review of 116cded): the check and every
/// write run in one IMMEDIATE transaction. A failure partway rolls back, so
/// "any group row exists" means the migration finished and the next launch
/// retries from scratch; a second Bram process on the same project blocks on
/// the write lock, then sees the committed rows and skips (bounded by the
/// connection's busy_timeout; on timeout it errors and retries next launch).
pub fn migrate_pairs_to_groups(conn: &Connection, at_ms: i64) -> Result<Option<Vec<String>>> {
    migrate_pairs_to_groups_tx(conn, at_ms, None)
}

fn migrate_pairs_to_groups_tx(
    conn: &Connection,
    at_ms: i64,
    fail_after_rows: Option<usize>,
) -> Result<Option<Vec<String>>> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match migrate_pairs_to_groups_in_tx(conn, at_ms, fail_after_rows) {
        Ok(out) => {
            conn.execute_batch("COMMIT")?;
            Ok(out)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// The migration body, run inside migrate_pairs_to_groups_tx's transaction.
// `fail_after_rows` is a test hook: fail after writing that many rows.
fn migrate_pairs_to_groups_in_tx(
    conn: &Connection,
    at_ms: i64,
    fail_after_rows: Option<usize>,
) -> Result<Option<Vec<String>>> {
    let mut written = 0usize;
    let wrote = |written: &mut usize| -> Result<()> {
        *written += 1;
        if fail_after_rows.is_some_and(|n| *written >= n) {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    };
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transitions WHERE kind = 'group' AND item_id IS NULL",
        [],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Ok(None);
    }
    let pairs: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT detail FROM transitions WHERE kind = 'pair' AND item_id IS NULL ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_>>()?
    };
    if pairs.is_empty() {
        return Ok(None);
    }
    // Newest first, so a more recent pair claims its agent slots first.
    let mut edges: Vec<(SessionKey, SessionKey)> = Vec::new();
    for detail in &pairs {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(detail) else {
            continue;
        };
        let sides: Vec<SessionKey> = PAIR_PROVIDERS
            .iter()
            .filter_map(|p| {
                v.get(*p)
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| (p.to_string(), s.to_string()))
            })
            .collect();
        let [a, b] = sides.as_slice() else {
            continue;
        };
        if edges
            .iter()
            .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
        {
            continue;
        }
        if legacy_pair_holds(conn, &a.0, &a.1, &b.0, &b.1)? {
            edges.push((a.clone(), b.clone()));
        }
    }
    // Components with at most one session per agent.
    let mut comps: Vec<std::collections::BTreeMap<usize, SessionKey>> = Vec::new();
    let mut comp_of: std::collections::HashMap<SessionKey, usize> = Default::default();
    let mut dropped = Vec::new();
    for (a, b) in edges {
        let desc = format!("{}={}+{}={}", a.0, a.1, b.0, b.1);
        match (comp_of.get(&a).copied(), comp_of.get(&b).copied()) {
            (Some(x), Some(y)) if x == y => {}
            (Some(x), Some(y)) => {
                if comps[y].keys().any(|r| comps[x].contains_key(r)) {
                    dropped.push(desc);
                    continue;
                }
                let moved = std::mem::take(&mut comps[y]);
                for (r, k) in moved {
                    comp_of.insert(k.clone(), x);
                    comps[x].insert(r, k);
                }
            }
            (Some(i), None) | (None, Some(i)) => {
                let k = if comp_of.contains_key(&a) { b } else { a };
                let r = provider_rank(&k.0);
                if comps[i].contains_key(&r) {
                    dropped.push(desc);
                    continue;
                }
                comps[i].insert(r, k.clone());
                comp_of.insert(k, i);
            }
            (None, None) => {
                let i = comps.len();
                comps.push(Default::default());
                comps[i].insert(provider_rank(&a.0), a.clone());
                comps[i].insert(provider_rank(&b.0), b.clone());
                comp_of.insert(a, i);
                comp_of.insert(b, i);
            }
        }
    }
    for (n, comp) in comps.iter().enumerate() {
        if comp.len() < 2 {
            continue;
        }
        // Named for its Claude member when it has one (the first agent).
        let origin = comp.values().next().cloned().unwrap_or_default();
        let gid = format!("{}-m{}", new_group_id(at_ms, &origin), n);
        for k in std::iter::once(&origin).chain(comp.values().filter(|k| **k != origin)) {
            append_group_row(
                conn,
                serde_json::json!({ "op": "join", "group": gid, "provider": k.0, "id": k.1 }),
                at_ms,
                "migrate-pairs",
            )?;
            wrote(&mut written)?;
        }
    }
    append_group_row(
        conn,
        serde_json::json!({ "op": "migrated", "dropped": dropped }),
        at_ms,
        "migrate-pairs",
    )?;
    wrote(&mut written)?;
    Ok(Some(dropped))
}

/// Whether (`provider`, `sid`) was never grouped (or, before groups, paired)
/// with a session of `other`, and so never left one either.
/// first-switch-to-unused-provider-starts-named-partner gates its fresh
/// partner on this alone.
pub fn session_never_paired(
    conn: &Connection,
    provider: &str,
    sid: &str,
    other: &str,
) -> Result<bool> {
    if !session_id_is_plain(sid) || !is_pair_provider(provider) || !is_pair_provider(other) {
        return Ok(false);
    }
    if latest_pair_row_for(conn, provider, sid, other)?.is_some() {
        return Ok(false);
    }
    let snap = group_snapshot(conn)?;
    Ok(!snap
        .ever
        .contains(&(provider.to_string(), sid.to_string(), other.to_string())))
}

// new-session-switch-starts-fresh-partner: a session started with New session
// is a new line of work, so the first switch away from it starts a fresh
// partner instead of resuming the other side's old pin. Stored as a
// `kind='session-born'` transitions row with detail {"provider", "id"}.

/// Record that (`provider`, `sid`) was started with New session. Ok(false)
/// when the id isn't plain or the row already exists.
pub fn record_session_born(
    conn: &Connection,
    provider: &str,
    sid: &str,
    at_ms: i64,
    source: &str,
) -> Result<bool> {
    if !session_id_is_plain(sid) || !is_pair_provider(provider) {
        return Ok(false);
    }
    if session_born_from_new_session(conn, provider, sid)? {
        return Ok(false);
    }
    let detail = format!("{{\"provider\":\"{}\",\"id\":\"{}\"}}", provider, sid);
    append_transition(conn, at_ms, None, "session-born", &detail, source)?;
    Ok(true)
}

/// Whether (`provider`, `sid`) was started with New session.
pub fn session_born_from_new_session(conn: &Connection, provider: &str, sid: &str) -> Result<bool> {
    if !session_id_is_plain(sid) || !is_pair_provider(provider) {
        return Ok(false);
    }
    let detail = format!("{{\"provider\":\"{}\",\"id\":\"{}\"}}", provider, sid);
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transitions WHERE kind = 'session-born' AND item_id IS NULL AND detail = ?1",
        params![detail],
        |r| r.get(0),
    )?;
    Ok(n > 0)
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
/// When the mirror first saw `item_id` on the board (`items.first_seen_ms`),
/// or None if it never did. clear-landed-never-begun-rows: the anchor for a
/// row that was never started, so has no `begunAtMs`. For an item older than
/// this database it is when the mirror first saw it, later than the true
/// proposal, which errs safe (an earlier commit is refused, never accepted).
pub fn item_first_seen_ms(conn: &Connection, item_id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT first_seen_ms FROM items WHERE id = ?1",
        params![item_id],
        |r| r.get(0),
    )
    .optional()
}

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
    fn item_first_seen_ms_is_the_first_sync_and_none_when_unseen() {
        // clear-landed-never-begun-rows anchors a never-started row on this.
        let conn = open_in_memory().unwrap();
        assert_eq!(item_first_seen_ms(&conn, "item-a").unwrap(), None);
        let items = vec![snap("item-a", "proposed", None, "[\"a.rs\"]", "[]")];
        mirror_items_sync(&conn, 1000, &items, "watcher").unwrap();
        // A later sync of the same item must not move the anchor forward.
        mirror_items_sync(&conn, 5000, &items, "watcher").unwrap();
        assert_eq!(item_first_seen_ms(&conn, "item-a").unwrap(), Some(1000));
        assert_eq!(item_first_seen_ms(&conn, "item-b").unwrap(), None);
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

    // item-last-session-switch-there
    #[test]
    fn item_session_is_recorded_only_when_it_changes() {
        let conn = open_in_memory().unwrap();
        assert_eq!(latest_item_session(&conn, "item-a").unwrap(), None);
        assert!(record_item_session(&conn, "item-a", "claude", "s-1", 100).unwrap());
        assert!(!record_item_session(&conn, "item-a", "claude", "s-1", 200).unwrap());
        assert_eq!(
            latest_item_session(&conn, "item-a").unwrap(),
            Some(("claude".to_string(), "s-1".to_string(), 100))
        );
        assert!(record_item_session(&conn, "item-a", "codex", "01a0-x", 300).unwrap());
        assert_eq!(
            latest_item_session(&conn, "item-a").unwrap(),
            Some(("codex".to_string(), "01a0-x".to_string(), 300))
        );
        assert_eq!(latest_item_session(&conn, "item-b").unwrap(), None);
    }

    #[test]
    fn session_pairs_follow_the_latest_row_on_both_sides() {
        let conn = open_in_memory().unwrap();
        let cx =
            |c: &str, x: &str, at| record_session_pair(&conn, "claude", c, "codex", x, at, "test");
        assert_eq!(
            session_partner(&conn, "claude", "c1", "codex").unwrap(),
            None
        );
        assert!(cx("c1", "x1", 1).unwrap());
        assert_eq!(
            session_partner(&conn, "claude", "c1", "codex")
                .unwrap()
                .as_deref(),
            Some("x1")
        );
        assert_eq!(
            session_partner(&conn, "codex", "x1", "claude")
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        // Re-recording the same pair writes nothing.
        assert!(!cx("c1", "x1", 2).unwrap());
        // x1 re-pairs with c2: c1 no longer has a partner (one per side).
        assert!(cx("c2", "x1", 3).unwrap());
        assert_eq!(
            session_partner(&conn, "claude", "c1", "codex").unwrap(),
            None
        );
        assert_eq!(
            session_partner(&conn, "codex", "x1", "claude")
                .unwrap()
                .as_deref(),
            Some("c2")
        );
        // Unpair c2.
        assert!(cx("c2", "", 4).unwrap());
        assert_eq!(
            session_partner(&conn, "claude", "c2", "codex").unwrap(),
            None
        );
        assert_eq!(
            session_partner(&conn, "codex", "x1", "claude").unwrap(),
            None
        );
    }

    // opencode-partner-handoff
    #[test]
    fn a_session_keeps_one_partner_per_other_provider() {
        let conn = open_in_memory().unwrap();
        record_session_pair(&conn, "claude", "c1", "codex", "x1", 1, "test").unwrap();
        // Argument order doesn't matter; the row is written claude-first.
        assert!(
            record_session_pair(&conn, "opencode", "ses_o1", "claude", "c1", 2, "test").unwrap()
        );
        assert_eq!(
            session_partners(&conn, "claude", "c1").unwrap(),
            vec![
                ("codex", "x1".to_string()),
                ("opencode", "ses_o1".to_string())
            ]
        );
        assert_eq!(
            session_partner(&conn, "opencode", "ses_o1", "claude")
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        // The OpenCode pair leaves Claude<->Codex alone, and vice versa.
        assert_eq!(
            session_partner(&conn, "codex", "x1", "claude")
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        record_session_pair(&conn, "claude", "c1", "opencode", "", 3, "test").unwrap();
        assert_eq!(
            session_partners(&conn, "claude", "c1").unwrap(),
            vec![("codex", "x1".to_string())]
        );
        // Never paired is per side. session-groups: x1 shared a group with
        // ses_o1, so it has been "paired" with OpenCode.
        assert!(!session_never_paired(&conn, "claude", "c1", "codex").unwrap());
        assert!(!session_never_paired(&conn, "codex", "x1", "opencode").unwrap());
        assert!(session_never_paired(&conn, "codex", "x9", "opencode").unwrap());
        // Same-provider and unknown-provider pairs are refused.
        assert!(!record_session_pair(&conn, "claude", "c1", "claude", "c2", 4, "test").unwrap());
        assert!(!record_session_pair(&conn, "claude", "c1", "other", "z", 4, "test").unwrap());
    }

    #[test]
    fn session_pairs_refuse_ids_they_would_have_to_escape() {
        let conn = open_in_memory().unwrap();
        assert!(!record_session_pair(&conn, "claude", "a\"b", "codex", "x1", 1, "test").unwrap());
        assert!(!record_session_pair(&conn, "claude", "", "codex", "", 1, "test").unwrap());
        assert_eq!(
            session_partner(&conn, "claude", "a\"b", "codex").unwrap(),
            None
        );
    }

    #[test]
    fn session_born_is_per_provider_and_written_once() {
        let conn = open_in_memory().unwrap();
        assert!(!session_born_from_new_session(&conn, "claude", "c1").unwrap());
        assert!(record_session_born(&conn, "claude", "c1", 1, "test").unwrap());
        assert!(!record_session_born(&conn, "claude", "c1", 2, "test").unwrap());
        assert!(session_born_from_new_session(&conn, "claude", "c1").unwrap());
        assert!(!session_born_from_new_session(&conn, "codex", "c1").unwrap());
        assert!(!record_session_born(&conn, "claude", "a\"b", 3, "test").unwrap());
        assert!(!record_session_born(&conn, "other", "c2", 3, "test").unwrap());
        // opencode-partner-handoff: OpenCode sessions can be born too.
        assert!(record_session_born(&conn, "opencode", "ses_o1", 4, "test").unwrap());
    }

    #[test]
    fn fresh_partner_only_for_a_new_session_never_paired_or_unpaired() {
        let conn = open_in_memory().unwrap();
        // What switch_agent requires for the new-session reason.
        let session_wants_fresh_partner = |conn: &Connection, p: &str, sid: &str| {
            Ok::<bool, rusqlite::Error>(
                session_born_from_new_session(conn, p, sid)?
                    && session_never_paired(conn, p, sid, "codex")?,
            )
        };
        // Not started with New session.
        assert!(!session_wants_fresh_partner(&conn, "claude", "c0").unwrap());
        // Started with New session, never paired.
        record_session_born(&conn, "claude", "c1", 1, "test").unwrap();
        assert!(session_wants_fresh_partner(&conn, "claude", "c1").unwrap());
        // Paired, then unpaired: an Unpair must survive the next switch.
        record_session_pair(&conn, "claude", "c1", "codex", "x1", 2, "test").unwrap();
        assert!(!session_wants_fresh_partner(&conn, "claude", "c1").unwrap());
        record_session_pair(&conn, "claude", "c1", "codex", "", 3, "test").unwrap();
        assert!(!session_wants_fresh_partner(&conn, "claude", "c1").unwrap());
        assert!(session_never_paired(&conn, "claude", "c0", "codex").unwrap());
        assert!(!session_never_paired(&conn, "claude", "c1", "codex").unwrap());
        // Unpaired from the other side leaves the earlier row naming it.
        record_session_born(&conn, "claude", "c2", 4, "test").unwrap();
        record_session_pair(&conn, "claude", "c2", "codex", "x2", 5, "test").unwrap();
        record_session_pair(&conn, "claude", "", "codex", "x2", 6, "test").unwrap();
        assert!(!session_wants_fresh_partner(&conn, "claude", "c2").unwrap());
    }

    // session-groups
    #[test]
    fn a_group_holds_one_session_per_agent_and_any_member_can_join() {
        let conn = open_in_memory().unwrap();
        // Claude c1 + OpenCode o1, then Codex x1 joins from either side.
        assert!(join_group(&conn, ("claude", "c1"), ("opencode", "o1"), 1, "t").unwrap());
        assert!(join_group(&conn, ("codex", "x1"), ("opencode", "o1"), 2, "t").unwrap());
        let g = session_group(&conn, "codex", "x1").unwrap().unwrap();
        assert_eq!(
            g.members,
            vec![
                ("claude".to_string(), "c1".to_string()),
                ("codex".to_string(), "x1".to_string()),
                ("opencode".to_string(), "o1".to_string()),
            ]
        );
        assert_eq!(g.origin, ("claude".to_string(), "c1".to_string()));
        assert!(!join_group(&conn, ("claude", "c1"), ("codex", "x1"), 3, "t").unwrap());
        // A second Codex session joining replaces x1, which leaves.
        assert!(join_group(&conn, ("claude", "c1"), ("codex", "x2"), 4, "t").unwrap());
        assert_eq!(session_group(&conn, "codex", "x1").unwrap(), None);
        assert_eq!(
            session_partner(&conn, "opencode", "o1", "codex")
                .unwrap()
                .as_deref(),
            Some("x2")
        );
    }

    #[test]
    fn joining_moves_a_session_out_of_its_old_group_which_dissolves_below_two() {
        let conn = open_in_memory().unwrap();
        join_group(&conn, ("claude", "old"), ("codex", "x1"), 1, "t").unwrap();
        join_group(&conn, ("claude", "new"), ("opencode", "o1"), 2, "t").unwrap();
        // x1 moves to the new line of work; "old" is left alone, so its group goes.
        assert!(join_group(&conn, ("claude", "new"), ("codex", "x1"), 3, "t").unwrap());
        assert_eq!(session_group(&conn, "claude", "old").unwrap(), None);
        assert_eq!(
            session_group(&conn, "codex", "x1")
                .unwrap()
                .unwrap()
                .members
                .len(),
            3
        );
        // Leave: the rest stay together.
        assert!(leave_group(&conn, "codex", "x1", 4, "t").unwrap());
        assert!(!leave_group(&conn, "codex", "x1", 5, "t").unwrap());
        assert_eq!(
            session_partners(&conn, "claude", "new").unwrap(),
            vec![("opencode", "o1".to_string())]
        );
        // Leaving a group of two dissolves it.
        leave_group(&conn, "opencode", "o1", 6, "t").unwrap();
        assert_eq!(session_group(&conn, "claude", "new").unwrap(), None);
        // Having been grouped still counts against a fresh partner.
        assert!(!session_never_paired(&conn, "claude", "new", "codex").unwrap());
    }

    // group-migration-atomic: a migration that fails after its first row
    // leaves nothing, and a rerun completes it; a rerun after success writes
    // nothing.
    #[test]
    fn a_failed_migration_rolls_back_and_a_rerun_completes_it() {
        let conn = open_in_memory().unwrap();
        let pair = |a: &str, ai: &str, b: &str, bi: &str, at: i64| {
            let detail = serde_json::json!({ a: ai, b: bi }).to_string();
            append_transition(&conn, at, None, "pair", &detail, "test").unwrap();
        };
        pair("claude", "c1", "opencode", "o1", 1);
        pair("claude", "c1", "codex", "x1", 2);
        let groups = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM transitions WHERE kind = 'group'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(migrate_pairs_to_groups_tx(&conn, 10, Some(1)).is_err());
        assert_eq!(
            groups(&conn),
            0,
            "a failed migration must leave no group rows"
        );
        assert!(migrate_pairs_to_groups(&conn, 11).unwrap().is_some());
        let after = groups(&conn);
        assert_eq!(after, 4, "three joins and the migrated marker");
        assert_eq!(
            session_group(&conn, "codex", "x1")
                .unwrap()
                .unwrap()
                .members
                .len(),
            3
        );
        assert_eq!(migrate_pairs_to_groups(&conn, 12).unwrap(), None);
        assert_eq!(groups(&conn), after, "a second migration writes nothing");
    }

    #[test]
    fn migration_merges_holding_pairs_and_the_newer_pair_wins_a_slot() {
        let conn = open_in_memory().unwrap();
        let pair = |a: &str, ai: &str, b: &str, bi: &str, at: i64| {
            let detail = serde_json::json!({ a: ai, b: bi }).to_string();
            append_transition(&conn, at, None, "pair", &detail, "test").unwrap();
        };
        // 2026-10-28's ledger, simplified: Claude c1 with OpenCode o1 and with
        // Codex x1; OpenCode o1 also still with an old Codex x0 (older).
        pair("codex", "x0", "opencode", "o1", 1);
        pair("claude", "c1", "opencode", "o1", 2);
        pair("claude", "c1", "codex", "x1", 3);
        // A pair that was later unpaired doesn't migrate.
        pair("claude", "c9", "codex", "x9", 4);
        pair("claude", "c9", "codex", "", 5);
        let dropped = migrate_pairs_to_groups(&conn, 10).unwrap().unwrap();
        assert_eq!(dropped, vec!["codex=x0+opencode=o1".to_string()]);
        let g = session_group(&conn, "opencode", "o1").unwrap().unwrap();
        assert_eq!(
            g.members,
            vec![
                ("claude".to_string(), "c1".to_string()),
                ("codex".to_string(), "x1".to_string()),
                ("opencode".to_string(), "o1".to_string()),
            ]
        );
        assert_eq!(session_group(&conn, "codex", "x0").unwrap(), None);
        assert_eq!(session_group(&conn, "claude", "c9").unwrap(), None);
        // Once only.
        assert_eq!(migrate_pairs_to_groups(&conn, 11).unwrap(), None);
    }

    #[test]
    fn item_session_refuses_ids_it_would_have_to_escape() {
        let conn = open_in_memory().unwrap();
        assert!(!record_item_session(&conn, "item-a", "claude", "a\"b", 1).unwrap());
        assert_eq!(latest_item_session(&conn, "item-a").unwrap(), None);
    }
}
