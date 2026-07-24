use rusqlite::Connection;

use crate::research::rss_types::EditTagRequest;
// P4.3: Use the canonical timestamp helper from `hkask-types` rather than
// inlining `chrono::Utc::now().to_rfc3339()` at every call site.
use hkask_types::time::now_rfc3339;

// RSS schema DDL — executed as extensions via Database::open_with_extensions()

/// RSS-specific schema DDL for use with `Database::open_with_extensions()`.
///
/// Creates 4 domain tables (feeds, subscriptions, entries, entry_states),
/// 1 FTS5 virtual table (entries_fts), 4 indexes, and 2 triggers
/// for FTS synchronization.
///
/// PRAGMA ordering invariant: `busy_timeout` MUST be set before
/// `journal_mode = WAL`. See `hkask_storage::database::init_wal_pragmas` for the
/// shared helper (not used here because this is a const DDL string, not
/// a function call, and the PRAGMAs are embedded in the DDL).
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
        read_at    TEXT,
        starred_at TEXT
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
        title, content, summary, content=''
    );

    CREATE INDEX IF NOT EXISTS idx_entries_feed_id ON entries(feed_id);
    CREATE INDEX IF NOT EXISTS idx_entries_published ON entries(published_at);
    CREATE INDEX IF NOT EXISTS idx_subscriptions_label ON subscriptions(label);
    CREATE INDEX IF NOT EXISTS idx_subscriptions_folder ON subscriptions(folder);

    CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
        INSERT INTO entries_fts(rowid, title, content, summary)
            VALUES (new.id, new.title, new.content, new.summary);
    END;

    CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
        INSERT INTO entries_fts(entries_fts, rowid, title, content, summary)
            VALUES ('delete', old.id, old.title, old.content, old.summary);
    END;
";

// Column constants — shared across query functions
const ENTRY_COLS: &str = "e.id, e.entry_id, e.title, e.url, e.author, e.summary, e.published_at, COALESCE(s.is_read, 0) as is_read, COALESCE(s.is_starred, 0) as is_starred";
const ENTRY_FROM_JOIN: &str = "FROM entries e LEFT JOIN entry_states s ON e.id = s.entry_id";
const SUB_QUERY: &str = "SELECT s.stream_id, s.title, s.label, s.folder, s.added_at, f.url, f.title as feed_title FROM subscriptions s JOIN feeds f ON s.feed_id = f.id";

fn feed_text(text: &Option<feed_rs::model::Text>) -> &str {
    text.as_ref().map(|t| t.content.as_str()).unwrap_or("")
}

pub fn upsert_feed(
    conn: &Connection,
    url: &str,
    feed: &feed_rs::model::Feed,
) -> Result<i64, anyhow::Error> {
    let title = feed_text(&feed.title);
    let description = feed_text(&feed.description);
    let site_url = feed.links.first().map(|l| l.href.as_str()).unwrap_or("");

    let existing_id: Option<i64> = conn
        .query_row("SELECT id FROM feeds WHERE url = ?1", [url], |row| {
            row.get(0)
        })
        .ok();

    if let Some(id) = existing_id {
        conn.execute(
            "UPDATE feeds SET title = ?1, description = ?2, site_url = ?3, last_fetched_at = datetime('now') WHERE id = ?4",
            rusqlite::params![title, description, site_url, id],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO feeds (url, title, description, site_url, last_fetched_at) VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            rusqlite::params![url, title, description, site_url],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn insert_entries(
    conn: &Connection,
    feed_id: i64,
    entries: &[feed_rs::model::Entry],
) -> Result<usize, anyhow::Error> {
    let mut new_count = 0;
    for entry in entries {
        let entry_id = entry.id.clone();
        let title = entry
            .title
            .as_ref()
            .map(|t| t.content.as_str())
            .unwrap_or("");
        let url = entry.links.first().map(|l| l.href.as_str()).unwrap_or("");
        let author = entry.authors.first().map(|a| a.name.as_str()).unwrap_or("");
        let content = entry
            .content
            .as_ref()
            .and_then(|c| c.body.as_deref())
            .unwrap_or("");
        let summary = entry
            .summary
            .as_ref()
            .map(|t| t.content.as_str())
            .unwrap_or("");
        let published_at = entry
            .published
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        let updated_at = entry.updated.map(|dt| dt.to_rfc3339()).unwrap_or_default();

        conn.execute(
            "INSERT OR IGNORE INTO entries (feed_id, entry_id, title, url, author, content, summary, published_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                feed_id,
                entry_id,
                title,
                url,
                author,
                content,
                summary,
                published_at,
                updated_at
            ],
        )?;
        if conn.changes() > 0 {
            new_count += 1;
        }
    }
    Ok(new_count)
}

pub fn update_feed_cache_headers(
    conn: &Connection,
    feed_id: i64,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<(), anyhow::Error> {
    conn.execute(
        "UPDATE feeds SET etag = ?1, last_modified = ?2 WHERE id = ?3",
        rusqlite::params![etag, last_modified, feed_id],
    )?;
    Ok(())
}

pub fn resolve_feed_url(conn: &Connection, stream_id: &str) -> Option<String> {
    if let Some(rest) = stream_id.strip_prefix("feed/") {
        Some(rest.to_string())
    } else {
        conn.query_row(
            "SELECT f.url FROM subscriptions s JOIN feeds f ON s.feed_id = f.id WHERE s.stream_id = ?1",
            [stream_id],
            |row| row.get(0),
        )
        .ok()
    }
}

/// Resolve a feed URL from a stream ID and fetch its cached ETag/Last-Modified headers.
///
/// Used by `rss_fetch` to look up the feed URL and conditional-fetch headers in a single
/// blocking call, replacing the manual `spawn_blocking` that duplicated `spawn_db` logic.
pub fn resolve_feed_with_headers(
    conn: &Connection,
    stream_id: &str,
) -> Result<(String, Option<String>, Option<String>), anyhow::Error> {
    let url = resolve_feed_url(conn, stream_id)
        .ok_or_else(|| anyhow::anyhow!("Feed URL not found for stream_id"))?;
    let etag: Option<String> = conn
        .query_row("SELECT etag FROM feeds WHERE url = ?1", [&url], |row| {
            row.get(0)
        })
        .ok();
    let lm: Option<String> = conn
        .query_row(
            "SELECT last_modified FROM feeds WHERE url = ?1",
            [&url],
            |row| row.get(0),
        )
        .ok();
    Ok((url, etag, lm))
}

/// Build (base SQL fragment, params) for a stream. `aux_where` is appended (e.g., "s.is_read = 0").
pub fn build_entry_query(
    stream_id: &str,
    aux_where: &str,
) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let (join, wc) = match stream_id {
        "user/-/state/com.google/reading-list" => {
            ("JOIN subscriptions sub ON e.feed_id = sub.feed_id", "")
        }
        "user/-/state/com.google/starred" => ("", "WHERE s.is_starred = 1"),
        "user/-/state/com.google/read" => ("", "WHERE s.is_read = 1"),
        _ if stream_id.starts_with("user/-/label/") => {
            let label = &stream_id["user/-/label/".len()..];
            params.push(Box::new(label.to_string()));
            (
                "JOIN subscriptions sub ON e.feed_id = sub.feed_id",
                "WHERE sub.label = ?",
            )
        }
        _ if stream_id.starts_with("feed/") => {
            let feed_url = &stream_id["feed/".len()..];
            params.push(Box::new(feed_url.to_string()));
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

fn entry_row_to_json(row: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    let str_col = |n: &str| -> rusqlite::Result<String> {
        row.get::<_, Option<String>>(n)
            .map(|v| v.unwrap_or_default())
    };
    Ok(serde_json::json!({
        "id": row.get::<_, i64>("id")?,
        "entry_id": row.get::<_, String>("entry_id")?,
        "title": str_col("title")?,
        "url": str_col("url")?,
        "author": str_col("author")?,
        "summary": str_col("summary")?,
        "published_at": str_col("published_at")?,
        "is_read": row.get::<_, i64>("is_read")? == 1,
        "is_starred": row.get::<_, i64>("is_starred")? == 1,
    }))
}

fn query_and_collect<T, P, F>(
    conn: &Connection,
    sql: &str,
    params: P,
    mapper: F,
) -> Result<Vec<T>, anyhow::Error>
where
    P: rusqlite::Params,
    F: Fn(&rusqlite::Row) -> rusqlite::Result<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, mapper)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ── Query functions ───────────────────────────────────────────────────────

pub fn query_entries(
    conn: &Connection,
    stream_id: &str,
    unread_only: bool,
    starred_only: bool,
    offset: usize,
    limit: usize,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    let mut extra = Vec::new();
    if unread_only {
        extra.push("(s.is_read = 0 OR s.is_read IS NULL)");
    }
    if starred_only {
        extra.push("s.is_starred = 1");
    }
    let (join_where, mut params) = build_entry_query(stream_id, &extra.join(" AND "));
    let sql = format!(
        "SELECT {ENTRY_COLS} {ENTRY_FROM_JOIN} {join_where} ORDER BY e.published_at DESC LIMIT ? OFFSET ?"
    );
    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    query_and_collect(conn, &sql, param_refs.as_slice(), entry_row_to_json)
}

pub fn count_entries(
    conn: &Connection,
    stream_id: &str,
    unread_only: bool,
) -> Result<usize, anyhow::Error> {
    let aux = if unread_only {
        "(s.is_read = 0 OR s.is_read IS NULL)"
    } else {
        ""
    };
    let (join_where, params) = build_entry_query(stream_id, aux);
    let sql = format!("SELECT COUNT(*) {ENTRY_FROM_JOIN} {join_where}");
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    Ok(conn.query_row(&sql, param_refs.as_slice(), |row| row.get::<_, i64>(0))? as usize)
}

// ── Mutation functions ────────────────────────────────────────────────────

pub fn mark_stream_read(conn: &Connection, stream_id: &str) -> Result<usize, anyhow::Error> {
    let (join_where, params) = build_entry_query(stream_id, "(s.is_read = 0 OR s.is_read IS NULL)");
    let find_sql = format!("SELECT e.id {ENTRY_FROM_JOIN} {join_where}");
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&find_sql)?;
    let entry_ids: Vec<i64> = stmt
        .query_map(param_refs.as_slice(), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let now = now_rfc3339();
    // N3 (panic-safe): use rusqlite's Transaction guard so a panic between
    // BEGIN and COMMIT automatically rolls back. The guard's Drop impl calls
    // finish_() which rolls back if commit() was not called.
    // new_unchecked takes &Connection (not &mut) to avoid changing the
    // function signature; the unchecked variant is safe here because these
    // functions are only called from spawn_db which holds a unique pooled
    // connection for the duration of the closure.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    for id in &entry_ids {
        tx.execute(
            "INSERT INTO entry_states (entry_id, is_read, read_at) VALUES (?1, 1, ?2)\n             ON CONFLICT(entry_id) DO UPDATE SET is_read = 1, read_at = ?2",
            rusqlite::params![id, now],
        )?;
    }
    tx.commit()?;
    Ok(entry_ids.len())
}

pub fn edit_tags(
    conn: &Connection,
    req: &EditTagRequest,
) -> Result<serde_json::Value, anyhow::Error> {
    let now = now_rfc3339();
    let mut updated = 0u64;

    // N2: add_label/remove_label removed. The previous implementation
    // updated the subscription's label based on an entry's feed_id,
    // which silently relabeled every entry in that feed — not just the
    // requested entry. Per-entry labels require a schema change (a
    // labels table keyed by entry_id) and are out of scope for this fix.
    // The fields remain on EditTagRequest for backward-compatible
    // deserialization but are now ignored. Warn so callers know.
    if req.add_label.is_some() || req.remove_label.is_some() {
        tracing::warn!(
            target: "hkask.research.rss",
            entry_ids = ?req.entry_ids,
            "edit_tags: add_label/remove_label are deprecated and ignored — \
             per-entry labels require a schema change. Use rss_list_subscriptions \
             to manage subscription labels."
        );
    }

    // N3 (panic-safe): use rusqlite's Transaction guard so a panic between
    // BEGIN and COMMIT automatically rolls back.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    for id in &req.entry_ids {
        let exists: bool = tx
            .query_row("SELECT COUNT(*) FROM entries WHERE id = ?1", [id], |row| {
                row.get::<_, i64>(0)
            })
            .map(|c| c > 0)?;
        if !exists {
            continue;
        }

        if req.add_read == Some(true) {
            tx.execute(
                "INSERT INTO entry_states (entry_id, is_read, read_at) VALUES (?1, 1, ?2)\n                 ON CONFLICT(entry_id) DO UPDATE SET is_read = 1, read_at = ?2",
                rusqlite::params![id, now],
            )?;
            updated += 1;
        }
        if req.remove_read == Some(true) {
            tx.execute(
                "INSERT INTO entry_states (entry_id, is_read) VALUES (?1, 0)\n                 ON CONFLICT(entry_id) DO UPDATE SET is_read = 0, read_at = NULL",
                rusqlite::params![id],
            )?;
            updated += 1;
        }
        if req.add_starred == Some(true) {
            tx.execute(
                "INSERT INTO entry_states (entry_id, is_starred, starred_at) VALUES (?1, 1, ?2)\n                 ON CONFLICT(entry_id) DO UPDATE SET is_starred = 1, starred_at = ?2",
                rusqlite::params![id, now],
            )?;
            updated += 1;
        }
        if req.remove_starred == Some(true) {
            tx.execute(
                "INSERT INTO entry_states (entry_id, is_starred) VALUES (?1, 0)\n                 ON CONFLICT(entry_id) DO UPDATE SET is_starred = 0, starred_at = NULL",
                rusqlite::params![id],
            )?;
            updated += 1;
        }
    }
    tx.commit()?;
    Ok(serde_json::json!({
        "updated": updated,
        "entry_count": req.entry_ids.len(),
    }))
}

// Search and listing

pub fn search_entries(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    let sql = format!(
        "SELECT {ENTRY_COLS} FROM entries e JOIN entries_fts fts ON e.id = fts.rowid LEFT JOIN entry_states s ON e.id = s.entry_id WHERE entries_fts MATCH ?1 ORDER BY rank LIMIT ?2"
    );
    query_and_collect(
        conn,
        &sql,
        rusqlite::params![query, limit as i64],
        entry_row_to_json,
    )
}

pub fn list_subscriptions(
    conn: &Connection,
    folder: Option<&str>,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    let sql = if folder.is_some() {
        format!("{SUB_QUERY} WHERE s.folder = ?1 ORDER BY s.added_at")
    } else {
        format!("{SUB_QUERY} ORDER BY s.added_at")
    };

    let map_row = |row: &rusqlite::Row| -> Result<serde_json::Value, rusqlite::Error> {
        let feed_title: Option<String> = row.get("feed_title")?;
        let sub_title: Option<String> = row.get("title")?;
        let display_title = sub_title.or(feed_title).unwrap_or_default();
        Ok(serde_json::json!({
            "stream_id": row.get::<_, String>("stream_id")?,
            "title": display_title,
            "url": row.get::<_, String>("url")?,
            "label": row.get::<_, Option<String>>("label")?,
            "folder": row.get::<_, Option<String>>("folder")?,
            "added_at": row.get::<_, Option<String>>("added_at")?.unwrap_or_default(),
        }))
    };

    let mut stmt = conn.prepare(&sql)?;
    let results = if let Some(f) = folder {
        stmt.query_map([f], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    };
    Ok(results)
}

// OPML export

fn opml_outline(indent: &str, url: &str, title: &str) -> String {
    format!(
        "{indent}<outline type=\"rss\" text=\"{}\" title=\"{}\" xmlUrl=\"{}\" />\n",
        xml_escape(title),
        xml_escape(title),
        xml_escape(url)
    )
}

pub fn export_opml(conn: &Connection) -> Result<String, anyhow::Error> {
    let subs = list_subscriptions(conn, None)?;

    let mut folders: std::collections::BTreeMap<String, Vec<&serde_json::Value>> =
        std::collections::BTreeMap::new();
    let mut unfiled: Vec<&serde_json::Value> = Vec::new();

    for sub in &subs {
        if let Some(folder) = sub.get("folder").and_then(|f| f.as_str())
            && !folder.is_empty()
        {
            folders.entry(folder.to_string()).or_default().push(sub);
            continue;
        }
        unfiled.push(sub);
    }

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n  <head>\n    <title>hKask RSS Subscriptions</title>\n  </head>\n  <body>\n",
    );

    for (folder, subs) in &folders {
        let escaped = xml_escape(folder);
        xml.push_str(&format!(
            "    <outline text=\"{escaped}\" title=\"{escaped}\">\n"
        ));
        for sub in subs {
            let url = sub["url"].as_str().unwrap_or("");
            let title = sub["title"].as_str().unwrap_or("");
            xml.push_str(&opml_outline("      ", url, title));
        }
        xml.push_str("    </outline>\n");
    }

    for sub in &unfiled {
        let url = sub["url"].as_str().unwrap_or("");
        let title = sub["title"].as_str().unwrap_or("");
        xml.push_str(&opml_outline("    ", url, title));
    }

    xml.push_str("  </body>\n</opml>");
    Ok(xml)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// OPML import

pub fn import_opml(
    conn: &Connection,
    opml_content: &str,
) -> Result<serde_json::Value, anyhow::Error> {
    let re = regex::Regex::new(r#"<outline[^>]*xmlUrl\s*=\s*\"([^\"]+)\"[^>]*/?\s*>"#)?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;

    let feeds: Vec<String> = re
        .captures_iter(opml_content)
        .filter_map(|cap| Some(cap.get(1)?.as_str().to_string()))
        .collect();

    // N3 (panic-safe): use rusqlite's Transaction guard so a panic between
    // BEGIN and COMMIT automatically rolls back all prior inserts.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    for url in feeds {
        // N11: SSRF defense — validate each OPML-sourced URL before
        // inserting it into the feeds table. A malicious OPML file could
        // otherwise seed the DB with internal URLs that rss_fetch would
        // later fetch (stored-SSRF). Use permissive config (allows
        // localhost/private IPs) because OPML files may legitimately
        // contain self-hosted feeds on local networks. The strict variant
        // (non-http schemes, embedded credentials) is still rejected.
        if let Err(e) = crate::research::providers::validate_provider_url_permissive(&url) {
            tracing::warn!(url = %url, error = %e, "OPML import: rejecting invalid URL");
            errors += 1;
            continue;
        }

        let stream_id = format!("feed/{url}");
        // N6: propagate the COUNT query error instead of swallowing it
        // via unwrap_or(false). A poisoned lock or schema drift should
        // surface, not silently look like "no existing subscription".
        let exists: bool = tx
            .query_row(
                "SELECT COUNT(*) FROM subscriptions WHERE stream_id = ?1",
                [&stream_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)?;

        if exists {
            skipped += 1;
            continue;
        }

        tx.execute(
            "INSERT OR IGNORE INTO feeds (url, last_fetched_at) VALUES (?1, datetime('now'))",
            [&url],
        )?;

        // N6: propagate the feed_id lookup error instead of unwrap_or(0).
        // A 0 sentinel silently counted as an error, masking the real cause.
        let feed_id: i64 = tx.query_row("SELECT id FROM feeds WHERE url = ?1", [&url], |row| {
            row.get(0)
        })?;

        match tx.execute(
            "INSERT INTO subscriptions (feed_id, stream_id) VALUES (?1, ?2)",
            rusqlite::params![feed_id, stream_id],
        ) {
            Ok(_) => imported += 1,
            // Constraint violation (duplicate stream_id from a race) is
            // the only expected error here; treat as skipped, not a hard
            // error.
            Err(rusqlite::Error::SqliteFailure(ref e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                skipped += 1;
            }
            Err(e) => return Err(e.into()),
        }
    }
    tx.commit()?;

    Ok(serde_json::json!({
        "imported": imported,
        "skipped": skipped,
        "errors": errors,
    }))
}
