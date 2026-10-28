//! Synthetic feed fetching, item extraction, and DB commit helpers.

use hkask_mcp_server::server::{McpToolError, map_join_error};

use crate::research::db::{insert_entries, lookup_synthetic_by_feed_url, update_synthetic_status};
use crate::research::synthetic::{
    ExtractedItem, ExtractorKind, ExtractorSpec, build_diff_hash_feed, build_synthetic_feed,
    content_hash, extract, extract_llm_schema, extract_pdf_ocr, items_to_entries,
};
use crate::{MAX_RESPONSE_BYTES, ResearchServer, map_db_error, spawn_db};

/// Build the synthetic feed from extracted items — the shared tail of the
/// item-producing extractor arms (llm_schema; pdf_ocr with items; css and
/// json_path). Returns the feed with its entries assigned and the entry
/// count. The diff-hash arms keep their own shape (a single hash entry).
pub(crate) fn items_synthetic_feed(
    items: Vec<ExtractedItem>,
    source_url: &str,
    title: &str,
    description: &str,
) -> (feed_rs::model::Feed, usize) {
    let entries = items_to_entries(items, title);
    let count = entries.len();
    let mut feed = build_synthetic_feed(source_url, title, description);
    feed.entries = entries;
    (feed, count)
}

/// Insert extracted entries and update the synthetic feed's status in one
/// transaction — the shared commit tail of the two fetch paths (diff-hash
/// with its new content hash; item extraction without). Returns the
/// stream's fetch summary, or maps the DB/join failure.
pub(crate) async fn commit_synthetic_entries(
    db: r2d2::Pool<hkask_storage::SqliteConnectionManager>,
    feed_id: i64,
    entries: Vec<feed_rs::model::Entry>,
    extract_hash: Option<String>,
    stream_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    let result = spawn_db(db, move |conn| {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
        let new_count = insert_entries(&tx, feed_id, &entries)?;
        update_synthetic_status(&tx, feed_id, new_count, extract_hash.as_deref(), None)?;
        tx.commit()?;
        Ok::<usize, anyhow::Error>(new_count)
    })
    .await;
    match result {
        Ok(Ok(new_count)) => Ok(serde_json::json!({
            "stream_id": stream_id,
            "new_entries": new_count,
            "fetched": true,
        })),
        Ok(Err(e)) => Err(map_db_error(e)),
        Err(e) => Err(map_join_error(e, "db task failed")),
    }
}

impl ResearchServer {
    /// Validate `source_url` and fetch its response body up to `MAX_RESPONSE_BYTES`.
    /// On non-2xx HTTP status, returns `Err((Some(status), McpToolError))` so callers
    /// that track feed status in the DB can record the HTTP status failure.
    pub(crate) async fn fetch_synthetic_source_bytes(
        &self,
        source_url: &str,
    ) -> Result<Vec<u8>, (Option<reqwest::StatusCode>, McpToolError)> {
        hkask_mcp_server::server::validate_tool_url_permissive(source_url)
            .map_err(|e| (None, e))?;
        let response = self.rss_client.get(source_url).send().await.map_err(|e| {
            (
                None,
                McpToolError::unavailable(format!("fetch source: {e}")),
            )
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err((
                Some(status),
                McpToolError::unavailable(format!("source returned HTTP {status}")),
            ));
        }
        let body = response.bytes().await.map_err(|e| {
            (
                None,
                McpToolError::unavailable(format!("read source body: {e}")),
            )
        })?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err((
                None,
                McpToolError::unavailable(format!(
                    "source body is {} bytes ({:.1} MiB) — exceeds the {} byte limit",
                    body.len(),
                    body.len() as f64 / (1024.0 * 1024.0),
                    MAX_RESPONSE_BYTES
                )),
            ));
        }
        Ok(body.to_vec())
    }

    /// Run the item-producing synthetic extractor (`LlmSchema`, `PdfOcr`, `Css`, `JsonPath`).
    pub(crate) async fn extract_synthetic_items(
        &self,
        kind: ExtractorKind,
        spec: &ExtractorSpec,
        source_url: &str,
        body: &[u8],
    ) -> Result<Vec<ExtractedItem>, McpToolError> {
        match kind {
            ExtractorKind::LlmSchema => extract_llm_schema(self.pool.as_ref(), spec, source_url)
                .await
                .map_err(McpToolError::from),
            ExtractorKind::PdfOcr => extract_pdf_ocr(self.pool.as_ref(), spec, source_url, body)
                .await
                .map_err(McpToolError::from),
            _ => extract(kind, spec, source_url, body).map_err(McpToolError::from),
        }
    }

    /// Shared synthetic feed fetch logic. Called by `rss_fetch` when it
    /// detects a `synthetic://` URL.
    pub(crate) async fn fetch_synthetic_inner(
        &self,
        stream_id: &str,
        feed_url: String,
    ) -> Result<serde_json::Value, McpToolError> {
        let Some(db) = self.research_db.clone() else {
            return Err(McpToolError::permission_denied(
                "Research database not available — set HKASK_DB_PASSPHRASE \
                 (the DB opens at its default path; set HKASK_RESEARCH_DB only for a \
                 custom location) and check the server log for an open failure.",
            ));
        };

        let feed_url_for_lookup = feed_url.clone();
        let synth_row = spawn_db(db.clone(), move |conn| {
            lookup_synthetic_by_feed_url(conn, &feed_url_for_lookup)
        })
        .await;
        let synth = match synth_row {
            Ok(Ok(Some(row))) => row,
            Ok(Ok(None)) => {
                return Err(McpToolError::not_found("synthetic feed binding not found"));
            }
            Ok(Err(e)) => return Err(map_db_error(e)),
            Err(e) => return Err(map_join_error(e, "db lookup failed")),
        };

        let kind: ExtractorKind = synth.extractor_kind.parse().map_err(McpToolError::from)?;
        let spec: ExtractorSpec = serde_json::from_str(&synth.extractor_spec)
            .map_err(|e| McpToolError::internal(format!("invalid stored extractor_spec: {e}")))?;

        let body = match self.fetch_synthetic_source_bytes(&synth.source_url).await {
            Ok(body) => body,
            Err((Some(status), err)) => {
                let err_msg = format!("source returned HTTP {status}");
                let feed_id = synth.feed_id;
                match spawn_db(db.clone(), move |conn| {
                    update_synthetic_status(conn, feed_id, 0, None, Some(&err_msg))
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(db_error)) => tracing::warn!(
                        target: "hkask.web",
                        feed_id,
                        error = %db_error,
                        "synthetic-feed error-status write failed — the feed record will show a stale status"
                    ),
                    Err(join_error) => tracing::warn!(
                        target: "hkask.web",
                        feed_id,
                        error = %join_error,
                        "synthetic-feed error-status write was lost — the feed record will show a stale status"
                    ),
                }
                return Err(err);
            }
            Err((None, err)) => return Err(err),
        };

        if kind == ExtractorKind::DiffHash {
            let new_hash = content_hash(&body);
            if Some(new_hash.as_str()) == synth.last_extract_hash.as_deref() {
                return Ok(serde_json::json!({
                    "stream_id": stream_id,
                    "new_entries": 0,
                    "fetched": true,
                    "not_modified": true,
                }));
            }
            let (feed, hash) = build_diff_hash_feed(&body, &synth.source_url, &synth.source_url);
            commit_synthetic_entries(db, synth.feed_id, feed.entries, Some(hash), stream_id).await
        } else {
            let items = self
                .extract_synthetic_items(kind, &spec, &synth.source_url, &body)
                .await?;
            let entries = items_to_entries(items, &synth.source_url);
            commit_synthetic_entries(db, synth.feed_id, entries, None, stream_id).await
        }
    }
}
