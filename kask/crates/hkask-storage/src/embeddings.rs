//! Embedding store — sqlite-vec or pgvector backed KNN similarity search.
//!
//! Two tables: `embeddings` (metadata + vector BLOB) + `vec_embeddings`
//! (sqlite-vec virtual table for KNN, keyed on implicit integer rowid).
//!
//! The vector BLOB is intentionally stored in both tables. vec0 requires
//! the vector for KNN MATCH; `embeddings.vector` provides uniform retrieval
//! via the backend-agnostic `DatabaseDriver` query path (get/get_all_by_prefix
//! work without branching on SqliteVec vs PgVector). Deduplicating would
//! require backend-conditional retrieval (join vec0 for SQLite, read column
//! for PgVector) — more complexity for ~4 KB/embedding savings. The
//! redundancy earns its keep by preserving the uniform retrieval abstraction.
//! If per-pod storage becomes a concern at scale, the escape hatch is a vec0
//! auxiliary column (`+vector BLOB`) to eliminate the `embeddings.vector` copy.
use hkask_types::InfrastructureError;
use hkask_types::NotFound;

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
use sqlx::Row;
use std::sync::Arc;

/// Vector search backend — sqlite-vec or pgvector.
enum VectorBackend {
    SqliteVec {
        pool: Pool<SqliteConnectionManager>,
        dim: usize,
    },
    PgVector {
        pool: sqlx::PgPool,
        handle: tokio::runtime::Handle,
        dim: usize,
    },
}

impl VectorBackend {
    fn dim(&self) -> usize {
        match self {
            Self::SqliteVec { dim, .. } | Self::PgVector { dim, .. } => *dim,
        }
    }

    fn search_sql(&self) -> &'static str {
        match self {
            Self::SqliteVec { .. } => {
                // vec0 is keyed on its implicit integer rowid; the UUID lives
                // only in the embeddings table. Join on rowid (integer B-tree
                // lookup) instead of a TEXT metadata column.
                "SELECT e.id, v.distance, e.entity_ref, e.vector, e.model
                 FROM vec_embeddings v
                 JOIN embeddings e ON v.rowid = e.rowid
                 WHERE v.embedding MATCH ?1 AND v.k = ?2
                 ORDER BY v.distance"
            }
            Self::PgVector { .. } => {
                "SELECT id, entity_ref, vector, model,
                        embedding <-> ?1::vector AS distance
                 FROM embeddings ORDER BY distance LIMIT ?2"
            }
        }
    }
}

/// EmbeddingStore — vector embedding storage.
pub struct EmbeddingStore {
    backend: VectorBackend,
    driver: Arc<dyn crate::database::driver::DatabaseDriver>,
}
impl EmbeddingStore {
    /// Create from a DatabaseDriver.
    ///
    /// Dispatches to the correct backend based on the driver's provider.
    /// - SqliteDriver → SqliteVec (raw conn for sqlite-vec)
    /// - PostgresDriver → PgVector (driver-based pgvector)
    pub fn from_driver(
        driver: Arc<dyn crate::database::driver::DatabaseDriver>,
        dim: usize,
    ) -> Self {
        let backend = match driver.provider() {
            crate::database::types::DbProvider::Sqlite => {
                let pool = driver
                    .sqlite_pool()
                    .cloned()
                    .expect("SqliteDriver must provide sqlite_pool()");
                VectorBackend::SqliteVec { pool, dim }
            }
            crate::database::types::DbProvider::Postgres => {
                let pool = driver
                    .postgres_pool()
                    .cloned()
                    .expect("PostgresDriver must provide postgres_pool()");
                let handle = tokio::runtime::Handle::try_current()
                    .expect("PostgresDriver requires a tokio runtime");
                VectorBackend::PgVector { pool, handle, dim }
            }
        };
        Self { backend, driver }
    }

    fn dim(&self) -> usize {
        self.backend.dim()
    }

    /// Execute SQL through the driver, mapping errors to EmbeddingError.
    fn exec(
        &self,
        sql: &str,
        params: &[crate::database::value::DbValue],
    ) -> Result<usize, EmbeddingError> {
        Ok(self.driver.execute(sql, params)?)
    }

    /// Query rows through the driver.
    fn query_driver(
        &self,
        sql: &str,
        params: &[crate::database::value::DbValue],
    ) -> Result<Vec<crate::database::value::DbRow>, EmbeddingError> {
        Ok(self.driver.query(sql, params)?)
    }
    /// Encode f32 vector as pgvector string literal: "[1.0,2.0,3.0]".
    fn pgvector_encode(vector: &[f32]) -> String {
        let mut s = String::with_capacity(vector.len() * 6 + 2);
        s.push('[');
        for (i, v) in vector.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&v.to_string());
        }
        s.push(']');
        s
    }

    /// Decode pgvector text format back to f32 vector: "[1.0,2.0]" → Vec<f32>.
    fn pgvector_decode(text: &str, expected_dim: usize) -> Result<Vec<f32>, EmbeddingError> {
        let inner = text.trim_start_matches('[').trim_end_matches(']');
        if inner.is_empty() {
            return Ok(Vec::new());
        }
        let floats: Result<Vec<f32>, _> =
            inner.split(',').map(|s| s.trim().parse::<f32>()).collect();
        let vector = floats.map_err(|e| EmbeddingError::Decode(format!("pgvector decode: {e}")))?;
        if vector.len() != expected_dim {
            return Err(EmbeddingError::DimensionMismatch {
                expected: expected_dim,
                actual: vector.len(),
            });
        }
        Ok(vector)
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
    ) -> Result<String, EmbeddingError> {
        self.validate_dim(vector)?;
        let id = hkask_types::EmbeddingID::new().to_string();
        let blob = Self::encode_vector(vector);
        let dim = vector.len() as i32;

        match &self.backend {
            VectorBackend::SqliteVec { pool, .. } => {
                let conn = pool
                    .get()
                    .map_err(|e| InfrastructureError::database(e.to_string()))?;
                conn.execute_batch("BEGIN TRANSACTION;")?;
                let result = conn.execute(
                    "INSERT INTO embeddings (id, entity_ref, vector, dimensions, model) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id, entity_ref, blob, dim, model],
                );
                if let Err(e) = result {
                    let _ = conn.execute_batch("ROLLBACK;");
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
                    let _ = conn.execute_batch("ROLLBACK;");
                    return Err(EmbeddingError::Storage(e));
                }
                conn.execute_batch("COMMIT;")?;
            }
            VectorBackend::PgVector { pool, handle, .. } => {
                let pg_vector = Self::pgvector_encode(vector);
                let h = handle.clone();
                let id_c = id.clone();
                let er = entity_ref.to_string();
                let m = model.to_string();
                h.block_on(async move {
                    let mut q = sqlx::query(
                        "INSERT INTO embeddings (id, entity_ref, vector, dimensions, model) VALUES ($1, $2, $3::vector, $4, $5)"
                    );
                    q = q.bind(&id_c).bind(&er).bind(&pg_vector).bind(dim).bind(&m);
                    q.execute(pool).await.map_err(|e| EmbeddingError::Infrastructure(
                        InfrastructureError::database(e.to_string())
                    ))
                })?;
            }
        }
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
            "SELECT id, entity_ref, vector, dimensions, model FROM embeddings WHERE entity_ref = ?",
            &[DbValue::Text(entity_ref.to_string())],
        )?;
        match rows.first() {
            Some(row) => {
                let id = row.get(0)?.as_text()?.to_string();
                let er = row.get(1)?.as_text()?.to_string();
                let blob = row.get(2)?.as_blob()?.to_vec();
                let model = row.get(4)?.as_text()?.to_string();
                let vector = Self::decode_vector(&blob, self.dim())?;
                Ok(StoredEmbedding {
                    id,
                    entity_ref: er,
                    vector,
                    model,
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

        match &self.backend {
            VectorBackend::SqliteVec { pool, .. } => {
                let query_blob = Self::encode_vector(query_vector);
                let conn = pool
                    .get()
                    .map_err(|e| InfrastructureError::database(e.to_string()))?;
                let sql = self.backend.search_sql();
                let mut stmt = conn.prepare(sql)?;
                let rows = stmt.query_map(rusqlite::params![&query_blob, limit as i64], |row| {
                    let id: String = row.get(0)?;
                    let distance: f64 = row.get(1)?;
                    let entity_ref: String = row.get(2)?;
                    let vector_blob: Vec<u8> = row.get(3)?;
                    let model: String = row.get(4)?;
                    Ok((id, distance, entity_ref, vector_blob, model))
                })?;
                let mut results = Vec::new();
                for row in rows {
                    let (id, distance, entity_ref, blob, model) =
                        row.map_err(EmbeddingError::Storage)?;
                    let vector = Self::decode_vector(&blob, self.dim())?;
                    results.push(SimilarityResult {
                        embedding: StoredEmbedding {
                            id,
                            entity_ref,
                            vector,
                            model,
                        },
                        distance,
                    });
                }
                Ok(results)
            }
            VectorBackend::PgVector { pool, handle, .. } => {
                let pg_vec = Self::pgvector_encode(query_vector);
                let h = handle.clone();
                let dim = self.dim();
                let l = limit;
                h.block_on(async move {
                    let mut q = sqlx::query(
                        "SELECT id, entity_ref, vector::text, model, embedding <-> $1::vector AS distance FROM embeddings ORDER BY distance LIMIT $2"
                    );
                    q = q.bind(&pg_vec).bind(l as i64);
                    let rows = q.fetch_all(pool).await.map_err(|e| {
                        EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string()))
                    })?;
                    let mut results = Vec::new();
                    for row in &rows {
                        let id: String = row.try_get(0).map_err(|e| EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string())))?;
                        let entity_ref: String = row.try_get(1).map_err(|e| EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string())))?;
                        let vector_text: String = row.try_get(2).map_err(|e| EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string())))?;
                        let model: String = row.try_get(3).map_err(|e| EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string())))?;
                        let distance: f64 = row.try_get(4).map_err(|e| EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string())))?;
                        let vector = Self::pgvector_decode(&vector_text, dim)?;
                        results.push(SimilarityResult {
                            embedding: StoredEmbedding { id, entity_ref, vector, model },
                            distance,
                        });
                    }
                    Ok(results)
                })
            }
        }
    }
    /// Delete embedding from both tables (single transaction).
    /// Delete an embedding by entity_ref.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — delete embedding
    /// pre:  entity_ref is non-empty
    /// post: embedding deleted if existed
    pub fn delete(&self, entity_ref: &str) -> Result<(), EmbeddingError> {
        use crate::database::value::DbValue;
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
        match &self.backend {
            VectorBackend::SqliteVec { pool, .. } => {
                let conn = pool
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
                    let _ = conn.execute_batch("ROLLBACK;");
                    return Err(EmbeddingError::Storage(e));
                }
                // Delete from embeddings on the SAME connection — not via self.exec,
                // which would acquire a second pool connection and self-deadlock
                // on SQLite's single-writer lock (busy_timeout=5000 → SQLITE_BUSY).
                if let Err(e) = conn.execute(
                    "DELETE FROM embeddings WHERE id = ?1",
                    rusqlite::params![id],
                ) {
                    let _ = conn.execute_batch("ROLLBACK;");
                    return Err(EmbeddingError::Storage(e));
                }
                conn.execute_batch("COMMIT;")?;
            }
            VectorBackend::PgVector { .. } => {
                // PgVector: single table, no separate vec table
                self.exec("DELETE FROM embeddings WHERE id = ?", &[DbValue::Text(id)])?;
            }
        }
        Ok(())
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
        match &self.backend {
            VectorBackend::SqliteVec { pool, .. } => {
                let pattern = format!("{}%", prefix);
                let conn = pool
                    .get()
                    .map_err(|e| InfrastructureError::database(e.to_string()))?;
                let mut stmt = conn.prepare(
                    "SELECT entity_ref, vector FROM embeddings WHERE entity_ref LIKE ?1",
                )?;
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
            VectorBackend::PgVector { pool, handle, .. } => {
                let pattern = format!("{}%", prefix);
                let h = handle.clone();
                h.block_on(async move {
                    let rows = sqlx::query(
                        "SELECT entity_ref, vector::text FROM embeddings WHERE entity_ref LIKE $1",
                    )
                    .bind(&pattern)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| {
                        EmbeddingError::Infrastructure(InfrastructureError::database(e.to_string()))
                    })?;
                    let mut results = Vec::new();
                    for row in &rows {
                        let entity_ref: String = row.try_get(0).map_err(|e| {
                            EmbeddingError::Infrastructure(InfrastructureError::database(
                                e.to_string(),
                            ))
                        })?;
                        let vector_text: String = row.try_get(1).map_err(|e| {
                            EmbeddingError::Infrastructure(InfrastructureError::database(
                                e.to_string(),
                            ))
                        })?;
                        let vector = Self::pgvector_decode(&vector_text, dim)?;
                        results.push((entity_ref, vector));
                    }
                    Ok(results)
                })
            }
        }
    }

    /// Query entity_refs matching a prefix.
    /// Query entity_refs by prefix.
    ///
    /// expect: "The system provides durable storage for embedding data"
    /// \[P3\] Motivating: Generative Space — query entity refs by prefix
    /// pre:  prefix is non-empty
    /// post: returns Vec of entity_refs matching prefix
    pub fn query_by_prefix(&self, prefix: &str) -> Result<Vec<String>, EmbeddingError> {
        match &self.backend {
            VectorBackend::SqliteVec { pool, .. } => {
                let pattern = format!("{}%", prefix);
                let conn = pool
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
            VectorBackend::PgVector { pool, handle, .. } => {
                let pattern = format!("{}%", prefix);
                let h = handle.clone();
                h.block_on(async move {
                    let rows =
                        sqlx::query("SELECT entity_ref FROM embeddings WHERE entity_ref LIKE $1")
                            .bind(&pattern)
                            .fetch_all(pool)
                            .await
                            .map_err(|e| {
                                EmbeddingError::Infrastructure(InfrastructureError::database(
                                    e.to_string(),
                                ))
                            })?;
                    let mut refs = Vec::new();
                    for row in &rows {
                        let s: String = row.try_get(0).map_err(|e| {
                            EmbeddingError::Infrastructure(InfrastructureError::database(
                                e.to_string(),
                            ))
                        })?;
                        refs.push(s);
                    }
                    Ok(refs)
                })
            }
        }
    }
}

// ── EmbeddingPort implementation ──────────────────────────────────────

impl hkask_types::embedding_port::EmbeddingPort for EmbeddingStore {
    fn store(&self, entity_ref: &str, embedding: Vec<f32>) -> Result<(), InfrastructureError> {
        self.store(entity_ref, &embedding, "default")
            .map(|_| ())
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn get(
        &self,
        entity_ref: &str,
    ) -> Result<Option<hkask_types::embedding_port::StoredEmbedding>, InfrastructureError> {
        match self.get(entity_ref) {
            Ok(se) => Ok(Some(hkask_types::embedding_port::StoredEmbedding {
                entity_ref: se.entity_ref.clone(),
                embedding: se.vector.clone(),
                dimension: se.vector.len(),
            })),
            Err(EmbeddingError::NotFound(_)) => Ok(None),
            Err(e) => Err(InfrastructureError::database(e.to_string())),
        }
    }

    fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<hkask_types::embedding_port::StoredEmbedding>, InfrastructureError> {
        self.search(query_embedding, limit)
            .map(|results| {
                results
                    .into_iter()
                    .map(|sr| hkask_types::embedding_port::StoredEmbedding {
                        entity_ref: sr.embedding.entity_ref.clone(),
                        embedding: sr.embedding.vector.clone(),
                        dimension: sr.embedding.vector.len(),
                    })
                    .collect()
            })
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn delete(&self, entity_ref: &str) -> Result<(), InfrastructureError> {
        self.delete(entity_ref)
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Database;
    use crate::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    /// Regression test for the self-deadlock in `delete`.
    ///
    /// Previously, `delete` held a raw pool connection with an open
    /// transaction (BEGIN), then called `self.exec` for the second DELETE,
    /// which acquired a *separate* pool connection. SQLite allows only one
    /// writer at a time, so the second connection blocked on the write
    /// lock held by the first, hit `busy_timeout=5000`, and returned
    /// SQLITE_BUSY. This test stores an embedding then deletes it; under
    /// the bug, the delete would take 5s (file pool) or hang (in-memory
    /// pool with max_size=1). Under the fix, both DELETEs run on the same
    /// connection and complete instantly.
    #[test]
    fn delete_does_not_self_deadlock() {
        let db = Database::in_memory().expect("in-memory database");
        let pool = db.sqlite_pool().expect("pool");
        let driver = Arc::new(SqliteDriver::new(pool));
        let dim = 1024;
        let store = EmbeddingStore::from_driver(driver as Arc<_>, dim);

        let entity_ref = "test:delete:deadlock:0";
        let vector = vec![0.1; dim];
        store
            .store(entity_ref, &vector, "test-model")
            .expect("store");

        // Under the bug, this hangs (in-memory pool max_size=1) or takes
        // ~5s (file pool, busy_timeout). Under the fix, microseconds.
        let start = std::time::Instant::now();
        store.delete(entity_ref).expect("delete should succeed");
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "delete took {:?} — likely self-deadlock on busy_timeout",
            elapsed
        );

        // Verify the embedding is gone from both tables.
        assert!(
            store.get(entity_ref).is_err(),
            "embedding should be deleted"
        );
    }
}
