//! Embedding-cluster service — shared fetch → normalize → cluster pipeline.
//!
//! `corpus_dedup_chunks` and `ConsolidationService::consolidate` previously
//! each hand-rolled the same ~40-line sequence: open the memory store, query
//! embeddings by prefix, pre-normalize vectors, group chunks by source, and
//! run greedy clustering per source. This module is that sequence, once.

use hkask_mcp_server::server::McpToolError;
use hkask_types::corpus::TaggedChunk;

use crate::helpers::{map_memory_store_error, open_memory_store};
use crate::tools::corpus::clustering::cluster_within_source;

/// Chunks plus their pre-normalized embeddings, grouped for clustering.
///
/// `norm_map` keys are chunk entity-refs; vectors are unit-length so the dot
/// product inside `cluster_within_source` equals cosine similarity.
pub(crate) struct ClusterInput {
    pub chunks: Vec<TaggedChunk>,
    pub norm_map: std::collections::HashMap<String, Vec<f32>>,
}

/// Load tagged chunks and their stored embeddings, pre-normalized.
///
/// Fails with `invalid_argument` when the JSONL is empty (both callers
/// treat that as a caller error) and maps store errors via the canonical
/// `map_memory_store_error`.
pub(crate) fn load_clusters(
    tagged_jsonl: &str,
    db_path: &str,
    passphrase: &str,
    prefix: &str,
) -> Result<ClusterInput, McpToolError> {
    let chunks = crate::tools::corpus::clustering::read_tagged_chunks(tagged_jsonl)?;
    if chunks.is_empty() {
        return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
    }

    let store = open_memory_store(db_path, passphrase)?;
    let embeddings = store
        .embeddings_by_prefix(prefix)
        .map_err(|e| map_memory_store_error(e, "Embedding query failed"))?;

    let norm_map: std::collections::HashMap<String, Vec<f32>> = embeddings
        .into_iter()
        .map(|(entity_ref, mut vector)| {
            crate::normalize_in_place(&mut vector);
            (entity_ref, vector)
        })
        .collect();

    Ok(ClusterInput { chunks, norm_map })
}

impl ClusterInput {
    /// Group chunks by source file and greedily cluster each group.
    ///
    /// Returns clusters as index vectors into `self.chunks`, ordered by
    /// salience descending within each cluster. `max_per_cluster` caps
    /// cluster size (`usize::MAX` for dedup, which keeps one survivor anyway).
    pub(crate) fn cluster_by_source(
        &self,
        threshold: f32,
        max_per_cluster: usize,
    ) -> Vec<Vec<usize>> {
        let borrowed: std::collections::HashMap<&str, &[f32]> = self
            .norm_map
            .iter()
            .map(|(entity_ref, vector)| (entity_ref.as_str(), vector.as_slice()))
            .collect();

        let mut by_source: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (index, chunk) in self.chunks.iter().enumerate() {
            by_source
                .entry(chunk.source.as_str())
                .or_default()
                .push(index);
        }

        let mut all_clusters = Vec::new();
        for indices in by_source.values() {
            all_clusters.extend(cluster_within_source(
                indices,
                &self.chunks,
                &borrowed,
                threshold,
                max_per_cluster,
            ));
        }
        all_clusters
    }
}
