//! EmbeddingPort — trait boundary for vector embedding storage.
//!
//! Decouples agent pods from the concrete `EmbeddingStore` in hkask-storage.

use crate::InfrastructureError;

/// A stored embedding at the port boundary.
#[derive(Debug, Clone)]
pub struct StoredEmbedding {
    pub entity_ref: String,
    pub embedding: Vec<f32>,
    pub dimension: usize,
}

/// Port trait for embedding storage operations.
pub trait EmbeddingPort: Send + Sync {
    /// Store an embedding for an entity reference.
    fn store(&self, entity_ref: &str, embedding: Vec<f32>) -> Result<(), InfrastructureError>;

    /// Retrieve an embedding by entity reference.
    fn get(&self, entity_ref: &str) -> Result<Option<StoredEmbedding>, InfrastructureError>;

    /// Search for similar embeddings by cosine similarity.
    fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<StoredEmbedding>, InfrastructureError>;

    /// Delete an embedding by entity reference.
    fn delete(&self, entity_ref: &str) -> Result<(), InfrastructureError>;
}
