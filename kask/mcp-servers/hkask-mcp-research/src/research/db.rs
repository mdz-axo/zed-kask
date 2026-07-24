//! RSS database operations — STUB (T0.6 port-ify pending).
//!
//! The original module used `rusqlite::Connection` directly. It is being
//! port-ified to `StorageDriver` (the port from `hkask_types::storage`).
//! Until the port is complete, all functions return an "not yet ported"
//! error so the server compiles and the non-RSS tools work. The RSS tools
//! will return `unavailable` at runtime.
//!
//! See: kask/docs/specs/seam-specs.md "T0.6-storage" + DIVERGENCE.md
//! "Dependency policy".

use crate::research::rss_types::EditTagRequest;
use hkask_types::storage::DbValue;

/// RSS schema DDL — applied by the bridge when opening the database.
pub const RSS_SCHEMA_DDL: &str = "
    PRAGMA busy_timeout=5000;
    PRAGMA journal_mode=WAL;
    PRAGMA foreign_keys=ON;

    CREATE TABLE IF NOT EXISTS feeds (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        url         TEXT NOT NULL UNIQUE,
        title       TEXT,
        description TEXT,
        site_url    TEXT,
        etag        TEXT,
        last_modified TEXT,
        last_fetched_at TEXT,
        created_at  TEXT DEFAULT (datetime('now'))
    );

    CREATE TABLE IF NOT EXISTS subscriptions (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        feed_id   INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
        stream_id TEXT NOT NULL UNIQUE,
        title     TEXT,
        label     TEXT,
        folder    TEXT,
        added_at  TEXT DEFAULT (datetime('now'))
    );

    CREATE TABLE IF NOT EXISTS entries (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        feed_id      INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
        entry_id     TEXT NOT NULL,
        title        TEXT,
        url          TEXT,
        author       TEXT,
        content      TEXT,
        summary      TEXT,
        published_at TEXT,
        updated_at   TEXT,
        fetched_at   TEXT DEFAULT (datetime('now')),
        UNIQUE(feed_id, entry_id)
    );

    CREATE TABLE IF NOT EXISTS entry_states (
        entry_id   INTEGER PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
        is_read    INTEGER NOT NULL DEFAULT 0,
        is_starred INTEGER NOT NULL DEFAULT 0,
        updated_at TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_entries_feed_id ON entries(feed_id);
    CREATE INDEX IF NOT EXISTS idx_entries_published_at ON entries(published_at);
    CREATE INDEX IF NOT EXISTS idx_subscriptions_feed_id ON subscriptions(feed_id);
    CREATE INDEX IF NOT EXISTS idx_subscriptions_stream_id ON subscriptions(stream_id);
";

fn not_ported() -> anyhow::Error {
    anyhow::anyhow!(
        "RSS db operation not yet port-ified to StorageDriver (T0.6). \
         See kask/docs/specs/seam-specs.md."
    )
}

pub fn upsert_feed(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _url: &str,
    _feed: &feed_rs::model::Feed,
) -> Result<i64, anyhow::Error> {
    Err(not_ported())
}

pub fn insert_entries(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _feed_id: i64,
    _entries: &[feed_rs::model::Entry],
) -> Result<usize, anyhow::Error> {
    Err(not_ported())
}

pub fn update_feed_cache_headers(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _feed_id: i64,
    _etag: Option<&str>,
    _last_modified: Option<&str>,
) -> Result<(), anyhow::Error> {
    Err(not_ported())
}

pub fn resolve_feed_url(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _stream_id: &str,
) -> Result<String, anyhow::Error> {
    Err(not_ported())
}

pub fn resolve_feed_with_headers(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _stream_id: &str,
) -> Result<(String, Option<String>, Option<String>), anyhow::Error> {
    Err(not_ported())
}

/// Build the JOIN/WHERE clause and parameter list for an entry query.
/// Returns `(join_where_clause, params)`.
pub fn build_entry_query(
    stream_id: &str,
    aux_where: &str,
) -> (String, Vec<DbValue>) {
    let mut params: Vec<DbValue> = Vec::new();
    let (join, wc) = match stream_id {
        "user/-/state/com.google/reading-list" => {
            ("JOIN subscriptions sub ON e.feed_id = sub.feed_id", "")
        }
        "user/-/state/com.google/starred" => ("", "WHERE s.is_starred = 1"),
        "user/-/state/com.google/read" => ("", "WHERE s.is_read = 1"),
        _ if stream_id.starts_with("user/-/label/") => {
            let label = &stream_id["user/-/label/".len()..];
            params.push(DbValue::Text(label.to_string()));
            (
                "JOIN subscriptions sub ON e.feed_id = sub.feed_id",
                "WHERE sub.label = ?",
            )
        }
        _ if stream_id.starts_with("feed/") => {
            let feed_url = &stream_id["feed/".len()..];
            params.push(DbValue::Text(feed_url.to_string()));
            ("JOIN feeds f ON e.feed_id = f.id", "WHERE f.url = ?")
        }
        _ => ("", "WHERE 1 = 0"),
    };
    let clause = if aux_where.is_empty() {
        format!("{join} {wc}")
    } else if wc.is_empty() {
        format!("{join} WHERE {aux_where}")
    } else {
        format!("{join} {wc} AND {aux_where}")
    };
    (clause, params)
}

pub fn query_entries(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _stream_id: &str,
    _unread_only: bool,
    _starred_only: bool,
    _offset: usize,
    _limit: usize,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    Err(not_ported())
}

pub fn count_entries(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _stream_id: &str,
    _unread_only: bool,
) -> Result<usize, anyhow::Error> {
    Err(not_ported())
}

pub fn mark_stream_read(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _stream_id: &str,
) -> Result<usize, anyhow::Error> {
    Err(not_ported())
}

pub fn edit_tags(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _req: &EditTagRequest,
) -> Result<serde_json::Value, anyhow::Error> {
    Err(not_ported())
}

pub fn search_entries(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _query: &str,
    _limit: usize,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    Err(not_ported())
}

pub fn list_subscriptions(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _folder: Option<&str>,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    Err(not_ported())
}

pub fn export_opml(
    _driver: &dyn hkask_types::storage::StorageDriver,
) -> Result<String, anyhow::Error> {
    Err(not_ported())
}

pub fn import_opml(
    _driver: &dyn hkask_types::storage::StorageDriver,
    _opml_content: &str,
) -> Result<serde_json::Value, anyhow::Error> {
    Err(not_ported())
}
