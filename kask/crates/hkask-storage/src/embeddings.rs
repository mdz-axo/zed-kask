//! Embedding store — sqlite-vec backed KNN similarity search.
//!
//! Two tables: `embeddings` (metadata + vector BLOB) + `vec_embeddings`
//! (sqlite-vec virtual table for KNN, keyed on implicit integer rowid).
//!
//! The vector BLOB is intentionally stored in both tables. vec0 requires
//! the vector for KNN MATCH; `embeddings.vector` provides uniform retrieval
//! via the backend-agnostic `DatabaseDriver` query path (get/get_all_by_prefix).
//! Deduplicating would require backend-conditional retrieval (join vec0 for
//! the KNN path, read the column for the metadata path) — more complexity
//! for ~4 KB/embedding savings. The redundancy earns its keep by preserving
//! the uniform retrieval abstraction. If per-pod storage becomes a concern
//! at scale, the escape hatch is a vec0 auxiliary column (`+vector BLOB`)
//! to eliminate the `embeddings.vector` copy.
use hkask_types::InfrastructureError;
use hkask_types::NotFound;

use crate::database::value::DbValue;

impl From<crate::database::types::DbError> for EmbeddingError {
    fn from(e: crate::database::types::DbError) -> Self {
        // Preserve error kind via InfrastructureError::from(DbError)
        EmbeddingError::Infrastructure(InfrastructureError::from(e))
    }
}
/// Stored embedding record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEmbedding {
    pub id: String,
    pub entity_ref: String,
    pub vector: Vec<f32>,
    pub model: String,
    pub passage_text: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimilarityResult {
    pub embedding: StoredEmbedding,
    pub distance: f64,
}
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("Embedding not found: {0}")]
    NotFound(NotFound),
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("Storage error: {0}")]
    Storage(#[source] rusqlite::Error),
    #[error(transparent)]
    Infrastructure(#[from] hkask_types::InfrastructureError),
    #[error("Corrupt vector data: {0}")]
    Decode(String),
}
impl From<rusqlite::Error> for EmbeddingError {
    fn from(e: rusqlite::Error) -> Self {
        EmbeddingError::Storage(e)
    }
}
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use std::sync::Arc;

/// EmbeddingStore — vector embedding storage backed by sqlite-vec.
pub struct EmbeddingStore {
    pool: Pool<SqliteConnectionManager>,
    dim: usize,
    driver: Arc<dyn crate::database::driver::DatabaseDriver>,
}
impl EmbeddingStore {
    /// Create from a DatabaseDriver.
    ///
    /// The driver must be a `SqliteDriver` (the only supported backend).
    ///
    /// `dim == 0` is clamped to 1024 (with a `log::warn!`) because a
    /// zero-dimensional store can never accept any vector — every `store`
    /// call would fail with `DimensionMismatch { expected: 0, actual: N }`,
    /// silently disabling embedding-based recall. This has been observed in
    /// production when a user's settings file explicitly sets `embedding_dim:
    /// 0`; the `unwrap_or(1024)` default only fires for `None`, not for
    /// `Some(0)`. Clamping keeps the system functional (degraded) instead of
    /// panicking, per the `.rules` trap "Process-global hooks set at runtime
    /// need a startup-failure signal".
    pub fn from_driver(
        driver: Arc<dyn crate::database::driver::DatabaseDriver>,
        dim: usize,
    ) -> Result<Self, EmbeddingError> {
        let dim = if dim == 0 {
            tracing::warn!(
                target: "reg.storage",
                embedding_dim = dim,
                "EmbeddingStore::from_driver called with dim == 0 — \
                 clamping to 1024 to avoid a zero-dimensional store. \
                 Set kask_settings.corpus.embedding_dim (or HKASK_EMBEDDING_DIM) \
                 to match the embedding model's output (default 1024 for
                 `DEFAULT_EMBEDDING_MODEL`)."
            );
            1024
        } else {
            dim
        };
        // SqliteDriver always provides a pool (constructed with one in
        // `SqliteDriver::new`). A `None` return here means the driver
        // is not a SqliteDriver — a logic error that cannot be recovered from.
        let pool = driver.sqlite_pool().cloned().ok_or_else(|| {
            EmbeddingError::Infrastructure(hkask_types::InfrastructureError::database(
                "EmbeddingStore requires a SqliteDriver, but sqlite_pool() returned None",
            ))
        })?;
        Ok(Self { pool, dim, driver })
    }

    fn dim(&self) -> usize {
        self.dim
    }

    /// Query rows through the driver.
    fn query_driver(
        &self,
        sql: &str,
        params: &[crate::database::value::DbValue],
    ) -> Result<Vec<crate::database::value::DbRow>, EmbeddingError> {
        Ok(self.driver.query(sql, params)?)
    }

    /// Encode f32 vector as binary blob for sqlite-vec.
    fn encode_vector(vector: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(vector.len() * 4);
        for &f in vector {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes
    }
    /// Decode binary blob back into f32 vector.
    fn decode_vector(blob: &[u8], expected_dim: usize) -> Result<Vec<f32>, EmbeddingError> {
        if blob.len() != expected_dim * 4 {
            return Err(EmbeddingError::DimensionMismatch {
                expected: expected_dim * 4,
                actual: blob.len(),
            });
        }
        let mut vector = Vec::with_capacity(expected_dim);
        for chunk in blob.chunks_exact(4) {
            let f = f32::from_le_bytes(chunk.try_into().map_err(|_| {
                EmbeddingError::Decode("corrupt vector blob: chunk not 4 bytes".into())
            })?);
            vector.push(f);
        }
        Ok(vector)
    }
    /// Validate vector dimension.
    fn validate_dim(&self, vector: &[f32]) -> Result<(), EmbeddingError> {
        if vector.len() != self.dim() {
            return Err(EmbeddingError::DimensionMismatch {
                expected: self.dim(),
                actual: vector.len(),
            });
        }
        Ok(())
    }
}
impl EmbeddingStore {
    /// Store embedding in both tables (single transaction). Returns the embedding ID.
    /// Store an embedding vector.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — store an embedding vector
    /// pre:  entity_ref is non-empty, vector matches store dimension, model is non-empty
    /// post: embedding stored and indexed by entity_ref
    /// post: returns embedding ID
    pub fn store(
        &self,
        entity_ref: &str,
        vector: &[f32],
        model: &str,
        passage_text: Option<&str>,
    ) -> Result<String, EmbeddingError> {
        self.validate_dim(vector)?;
        let id = hkask_types::EmbeddingID::new().to_string();
        let blob = Self::encode_vector(vector);
        let dim = vector.len() as i32;

        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        conn.execute_batch("BEGIN TRANSACTION;")?;
        let result = conn.execute(
            "INSERT INTO embeddings (id, entity_ref, vector, dimensions, model, passage_text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, entity_ref, blob, dim, model, passage_text],
        );
        if let Err(e) = result {
            if let Err(rb_err) = conn.execute_batch("ROLLBACK;") {
                tracing::warn!(target: "reg.storage", error = %rb_err, "ROLLBACK failed after embeddings INSERT error");
            }
            return Err(EmbeddingError::Storage(e));
        }
        // vec0 is keyed on its implicit integer rowid, which mirrors
        // embeddings.rowid. Link the vector to the metadata row by
        // reusing the rowid SQLite just assigned.
        // SAFETY (connection affinity): last_insert_rowid() is per-
        // connection state. This is safe because the INSERT above and
        // this call happen on the same PooledConnection within the
        // same transaction. Refactoring to self.exec() (which acquires
        // a different pool connection) would break this — the rowid
        // would be from the wrong connection.
        let rowid: i64 = conn.last_insert_rowid();
        let vec_result = conn.execute(
            "INSERT INTO vec_embeddings (rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![rowid, &blob],
        );
        if let Err(e) = vec_result {
            if let Err(rb_err) = conn.execute_batch("ROLLBACK;") {
                tracing::warn!(target: "reg.storage", error = %rb_err, "ROLLBACK failed after vec_embeddings INSERT error");
            }
            return Err(EmbeddingError::Storage(e));
        }
        conn.execute_batch("COMMIT;")?;
        tracing::debug!(
            target: "storage.embedding",
            id = %id, entity_ref = %entity_ref, model = %model, dimensions = dim,
            "Embedding stored"
        );
        Ok(id)
    }
    /// Retrieve an embedding by entity reference.
    /// Retrieve an embedding by entity_ref.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — retrieve embedding by entity
    /// pre:  entity_ref is non-empty
    /// post: returns StoredEmbedding if found
    /// post: returns Err(NotFound) if not found
    #[must_use = "result must be used"]
    pub fn get(&self, entity_ref: &str) -> Result<StoredEmbedding, EmbeddingError> {
        use crate::database::value::DbValue;
        let rows = self.query_driver(
            "SELECT id, entity_ref, vector, dimensions, model, passage_text FROM embeddings WHERE entity_ref = ?",
            &[DbValue::Text(entity_ref.to_string())],
        )?;
        match rows.first() {
            Some(row) => {
                let id = row.get(0)?.as_text()?.to_string();
                let er = row.get(1)?.as_text()?.to_string();
                let blob = row.get(2)?.as_blob()?.to_vec();
                let model = row.get(4)?.as_text()?.to_string();
                let passage_text = row
                    .get(5)
                    .ok()
                    .and_then(|v| v.as_text().ok())
                    .map(|s| s.to_string());
                let vector = Self::decode_vector(&blob, self.dim())?;
                Ok(StoredEmbedding {
                    id,
                    entity_ref: er,
                    vector,
                    model,
                    passage_text,
                })
            }
            None => Err(EmbeddingError::NotFound(NotFound {
                entity_type: "embedding".to_string(),
                id: entity_ref.to_string(),
            })),
        }
    }
    /// KNN search using sqlite-vec MATCH operator.
    ///
    /// Returns the `limit` nearest embeddings by cosine distance. vec0 v0.1.x
    /// does not support WHERE constraints on the distance column for
    /// threshold-based filtering (sqlite-vec#165). If the caller needs a
    /// distance threshold (e.g. "all neighbors within cosine 0.3"), over-fetch
    /// a larger `limit` and post-filter the returned `SimilarityResult` list
    /// by the `distance` field in Rust.
    /// Search for similar embeddings by vector distance.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — vector similarity search
    /// pre:  query_vector matches store dimension, limit > 0
    /// post: returns `Vec<SimilarityResult>` ordered by ascending distance
    #[must_use = "result must be used"]
    pub fn search(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<SimilarityResult>, EmbeddingError> {
        self.validate_dim(query_vector)?;

        let query_blob = Self::encode_vector(query_vector);
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        // vec0 is keyed on its implicit integer rowid; the UUID lives
        // only in the embeddings table. Join on rowid (integer B-tree
        // lookup) instead of a TEXT metadata column.
        let mut stmt = conn.prepare(
            "SELECT e.id, v.distance, e.entity_ref, e.vector, e.model, e.passage_text
             FROM vec_embeddings v
             JOIN embeddings e ON v.rowid = e.rowid
             WHERE v.embedding MATCH ?1 AND v.k = ?2
             ORDER BY v.distance",
        )?;
        let rows = stmt.query_map(rusqlite::params![&query_blob, limit as i64], |row| {
            let id: String = row.get(0)?;
            let distance: f64 = row.get(1)?;
            let entity_ref: String = row.get(2)?;
            let vector_blob: Vec<u8> = row.get(3)?;
            let model: String = row.get(4)?;
            let passage_text: Option<String> = row.get(5)?;
            Ok((id, distance, entity_ref, vector_blob, model, passage_text))
        })?;
        let mut results = Vec::new();
        for row in rows {
            let (id, distance, entity_ref, blob, model, passage_text) =
                row.map_err(EmbeddingError::Storage)?;
            let vector = Self::decode_vector(&blob, self.dim())?;
            results.push(SimilarityResult {
                embedding: StoredEmbedding {
                    id,
                    entity_ref,
                    vector,
                    model,
                    passage_text,
                },
                distance,
            });
        }
        Ok(results)
    }
    /// Delete embedding from both tables (single transaction).
    /// Delete an embedding by entity_ref.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — delete embedding
    /// pre:  entity_ref is non-empty
    /// post: embedding deleted if existed
    pub fn delete(&self, entity_ref: &str) -> Result<(), EmbeddingError> {
        let rows = self.query_driver(
            "SELECT id FROM embeddings WHERE entity_ref = ?",
            &[DbValue::Text(entity_ref.to_string())],
        )?;
        let id = match rows.first() {
            Some(row) => row.get(0)?.as_text()?.to_string(),
            None => {
                return Err(EmbeddingError::NotFound(NotFound {
                    entity_type: "embedding".to_string(),
                    id: entity_ref.to_string(),
                }));
            }
        };
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        conn.execute_batch("BEGIN TRANSACTION;")?;
        // vec0 is rowid-keyed; resolve the UUID to the embeddings rowid
        // and delete the vector by integer key (fast B-tree lookup,
        // avoids the inefficient >12-char TEXT metadata scan).
        if let Err(e) = conn.execute(
            "DELETE FROM vec_embeddings WHERE rowid = (SELECT rowid FROM embeddings WHERE id = ?1)",
            rusqlite::params![id],
        ) {
            if let Err(rb_err) = conn.execute_batch("ROLLBACK;") {
                tracing::warn!(target: "reg.storage", error = %rb_err, "ROLLBACK failed after vec_embeddings DELETE error");
            }
            return Err(EmbeddingError::Storage(e));
        }
        // Delete from embeddings on the SAME connection — not via self.exec,
        // which would acquire a second pool connection and self-deadlock
        // on SQLite's single-writer lock (busy_timeout=5000 → SQLITE_BUSY).
        if let Err(e) = conn.execute(
            "DELETE FROM embeddings WHERE id = ?1",
            rusqlite::params![id],
        ) {
            if let Err(rb_err) = conn.execute_batch("ROLLBACK;") {
                tracing::warn!(target: "reg.storage", error = %rb_err, "ROLLBACK failed after embeddings DELETE error");
            }
            return Err(EmbeddingError::Storage(e));
        }
        conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    /// Delete every embedding under an entity — the vector rows AND the
    /// metadata rows, transactionally. `delete` resolves only the first
    /// id for an entity; a thread entity holds one embedding per turn,
    /// and the retirement pass must remove all of them.
    pub fn delete_all_by_entity_ref(&self, entity_ref: &str) -> Result<usize, EmbeddingError> {
        let rows = self.query_driver(
            "SELECT id FROM embeddings WHERE entity_ref = ?",
            &[DbValue::Text(entity_ref.to_string())],
        )?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let id = row.get(0)?.as_text()?.to_string();
            ids.push(id);
        }
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        conn.execute_batch("BEGIN TRANSACTION;")?;
        for id in &ids {
            // vec0 is rowid-keyed; resolve the UUID to the embeddings
            // rowid and delete the vector by integer key (same pattern
            // as delete).
            if let Err(e) = conn.execute(
                "DELETE FROM vec_embeddings WHERE rowid = (SELECT rowid FROM embeddings WHERE id = ?1)",
                rusqlite::params![id],
            ) {
                if let Err(rb_err) = conn.execute_batch("ROLLBACK;") {
                    tracing::warn!(target: "reg.storage", error = %rb_err, "ROLLBACK failed after vec_embeddings DELETE error");
                }
                return Err(EmbeddingError::Storage(e));
            }
            // Delete from embeddings on the SAME connection — not via
            // self.exec, which would acquire a second pool connection and
            // self-deadlock on SQLite's single-writer lock.
            if let Err(e) = conn.execute(
                "DELETE FROM embeddings WHERE id = ?1",
                rusqlite::params![id],
            ) {
                if let Err(rb_err) = conn.execute_batch("ROLLBACK;") {
                    tracing::warn!(target: "reg.storage", error = %rb_err, "ROLLBACK failed after embeddings DELETE error");
                }
                return Err(EmbeddingError::Storage(e));
            }
        }
        conn.execute_batch("COMMIT;")?;
        Ok(ids.len())
    }

    /// Delete vector rows whose rowid has no embeddings metadata row —
    /// orphans left when metadata was deleted without vec access (e.g.
    /// a therapy SQL pass). KNN's inner join already ignores them; this
    /// reclaims the shadow-table space.
    pub fn delete_orphaned_vectors(&self) -> Result<usize, EmbeddingError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        conn.execute(
            "DELETE FROM vec_embeddings WHERE rowid NOT IN (SELECT rowid FROM embeddings)",
            rusqlite::params![],
        )
        .map_err(EmbeddingError::Storage)
    }
    /// Count total embeddings stored.
    /// Count stored embeddings.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P8\] Motivating: Semantic Grounding — count embeddings
    /// post: returns total count of embeddings
    pub fn count(&self) -> Result<usize, EmbeddingError> {
        let rows = self.query_driver("SELECT COUNT(*) FROM embeddings", &[])?;
        let count: i64 = rows
            .first()
            .ok_or_else(|| {
                EmbeddingError::Infrastructure(InfrastructureError::database(
                    "COUNT query returned no rows",
                ))
            })?
            .get(0)?
            .as_int()?;
        Ok(count as usize)
    }
    /// Bulk-load all (entity_ref, vector) pairs matching a prefix.
    ///
    /// Returns entity_ref + decoded vector for every embedding whose entity_ref
    /// starts with `prefix`. Used by corpus dedup to load all chunk embeddings
    /// in a single query instead of N individual `get()` calls.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — bulk vector retrieval by prefix
    /// pre:  prefix is non-empty
    /// post: returns Vec of (entity_ref, vector) pairs matching prefix
    pub fn get_all_by_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, Vec<f32>)>, EmbeddingError> {
        let dim = self.dim();
        let pattern = format!("{}%", prefix);
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        let mut stmt =
            conn.prepare("SELECT entity_ref, vector FROM embeddings WHERE entity_ref LIKE ?1")?;
        let rows = stmt.query_map(rusqlite::params![pattern], |row| {
            let entity_ref: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((entity_ref, blob))
        })?;
        let mut results = Vec::new();
        for row in rows {
            let (entity_ref, blob) = row.map_err(EmbeddingError::Storage)?;
            let vector = Self::decode_vector(&blob, dim)?;
            results.push((entity_ref, vector));
        }
        Ok(results)
    }

    /// Query entity_refs matching a prefix.
    /// Query entity_refs by prefix.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — query entity refs by prefix
    /// pre:  prefix is non-empty
    /// post: returns Vec of entity_refs matching prefix
    pub fn query_by_prefix(&self, prefix: &str) -> Result<Vec<String>, EmbeddingError> {
        let pattern = format!("{}%", prefix);
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        let mut stmt =
            conn.prepare("SELECT entity_ref FROM embeddings WHERE entity_ref LIKE ?1")?;
        let rows = stmt.query_map(rusqlite::params![pattern], |row| row.get(0))?;
        let mut refs = Vec::new();
        for row in rows {
            let entity_ref: String = row.map_err(EmbeddingError::Storage)?;
            refs.push(entity_ref);
        }
        Ok(refs)
    }

    /// Load all embeddings with their passage text for in-memory index hydration.
    ///
    /// Returns `(entity_ref, vector, passage_text)` for every stored embedding.
    /// Used by the corpus server to rebuild the in-memory vector index after a
    /// restart, so `corpus_query` returns full passage text without requiring
    /// a re-embed from the source JSONL.
    pub fn all_with_text(&self) -> Result<Vec<(String, Vec<f32>, Option<String>)>, EmbeddingError> {
        let dim = self.dim();
        let conn = self
            .pool
            .get()
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT entity_ref, vector, passage_text FROM embeddings")?;
        let rows = stmt.query_map([], |row| {
            let entity_ref: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let passage_text: Option<String> = row.get(2)?;
            Ok((entity_ref, blob, passage_text))
        })?;
        let mut results = Vec::new();
        for row in rows {
            let (entity_ref, blob, passage_text) = row.map_err(EmbeddingError::Storage)?;
            let vector = Self::decode_vector(&blob, dim)?;
            results.push((entity_ref, vector, passage_text));
        }
        Ok(results)
    }
}
