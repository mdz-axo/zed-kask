//! EmbeddingPort — trait boundary for vector embedding storage.
//!
//! Decouples `hkask-memory` from the concrete embedding store. The
//! `kask_bridge` implements this trait over `StorageDriver` with pure-Rust
//! brute-force cosine similarity (no sqlite-vec, no C extension).
//!
//! See: DIVERGENCE.md "Dependency policy" + D6 (MemoryPort) in seam-specs.md.

use crate::{InfrastructureError, NotFound};
use thiserror::Error;

// ── StoredEmbedding ──────────────────────────────────────────────────────────

/// A stored embedding record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEmbedding {
    pub id: String,
    pub entity_ref: String,
    pub vector: Vec<f32>,
    pub model: String,
}

// ── SimilarityResult ─────────────────────────────────────────────────────────

/// A KNN search result — the stored embedding plus its cosine distance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimilarityResult {
    pub embedding: StoredEmbedding,
    pub distance: f64,
}

// ── EmbeddingError ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("Embedding not found: {0}")]
    NotFound(NotFound),
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("Storage error: {0}")]
    Storage(String),
    #[error(transparent)]
    Infrastructure(#[from] InfrastructureError),
    #[error("Corrupt vector data: {0}")]
    Decode(String),
}

// ── EmbeddingPort trait ──────────────────────────────────────────────────────

/// Port trait for embedding storage + KNN similarity search.
///
/// Implementations:
/// - `kask_bridge`: pure-Rust cosine similarity over `StorageDriver` (BLOB
///   column in a regular SQLite table, brute-force KNN in Rust).
pub trait EmbeddingPort: Send + Sync {
    /// Store an embedding for an entity reference. Returns the generated ID.
    fn store(
        &self,
        entity_ref: &str,
        vector: &[f32],
        model: &str,
    ) -> Result<String, EmbeddingError>;

    /// Retrieve an embedding by entity reference.
    fn get(&self, entity_ref: &str) -> Result<StoredEmbedding, EmbeddingError>;

    /// KNN search — return the `limit` nearest embeddings by cosine distance.
    fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SimilarityResult>, EmbeddingError>;

    /// Delete an embedding by entity reference.
    fn delete(&self, entity_ref: &str) -> Result<(), EmbeddingError>;

    /// Count all stored embeddings.
    fn count(&self) -> Result<usize, EmbeddingError>;

    /// Query entity references matching a prefix.
    fn query_by_prefix(&self, prefix: &str) -> Result<Vec<String>, EmbeddingError>;

    /// Get all embeddings whose entity_ref matches a prefix.
    fn get_all_by_prefix(&self, prefix: &str) -> Result<Vec<(String, Vec<f32>)>, EmbeddingError>;
}
