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

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::corpus::TaggedChunk;

    fn chunk(entity_ref: &str, source: &str, salience: f32) -> TaggedChunk {
        TaggedChunk {
            entity_ref: entity_ref.to_string(),
            source: source.to_string(),
            salience,
            ..Default::default()
        }
    }

    fn input(chunks: Vec<TaggedChunk>, embeddings: Vec<(&str, Vec<f32>)>) -> ClusterInput {
        let norm_map = embeddings
            .into_iter()
            .map(|(entity_ref, vector)| (entity_ref.to_string(), vector))
            .collect();
        ClusterInput { chunks, norm_map }
    }

    #[test]
    fn identical_vectors_cluster_together() {
        // Two chunks from the same source with identical (parallel) embeddings
        // must land in one cluster; the higher-salience chunk is the survivor.
        let chunks = vec![chunk("a", "doc.txt", 0.9), chunk("b", "doc.txt", 0.5)];
        let embeddings = vec![("a", vec![1.0, 0.0]), ("b", vec![1.0, 0.0])];
        let cluster_input = input(chunks, embeddings);
        let clusters = cluster_input.cluster_by_source(0.85, usize::MAX);
        assert_eq!(clusters.len(), 1);
        // Both members in one cluster, salience-descending: the survivor
        // (first element) is the higher-salience chunk.
        assert_eq!(clusters[0], vec![0, 1]);
    }

    #[test]
    fn orthogonal_vectors_stay_separate() {
        let chunks = vec![chunk("a", "doc.txt", 0.9), chunk("b", "doc.txt", 0.5)];
        let embeddings = vec![("a", vec![1.0, 0.0]), ("b", vec![0.0, 1.0])];
        let cluster_input = input(chunks, embeddings);
        let clusters = cluster_input.cluster_by_source(0.85, usize::MAX);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn different_sources_never_cluster_together() {
        // Identical embeddings but different sources — grouping is per-source.
        let chunks = vec![chunk("a", "one.txt", 0.9), chunk("b", "two.txt", 0.5)];
        let embeddings = vec![("a", vec![1.0, 0.0]), ("b", vec![1.0, 0.0])];
        let cluster_input = input(chunks, embeddings);
        let clusters = cluster_input.cluster_by_source(0.85, usize::MAX);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn max_per_cluster_caps_cluster_size() {
        let chunks = vec![
            chunk("a", "doc.txt", 0.9),
            chunk("b", "doc.txt", 0.8),
            chunk("c", "doc.txt", 0.7),
        ];
        let embeddings = vec![
            ("a", vec![1.0, 0.0]),
            ("b", vec![1.0, 0.0]),
            ("c", vec![1.0, 0.0]),
        ];
        let cluster_input = input(chunks, embeddings);
        // Cap of 2: first two (highest salience) cluster, third starts a new one.
        let clusters = cluster_input.cluster_by_source(0.85, 2);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].len(), 2);
        assert_eq!(clusters[1], vec![2]);
    }

    #[test]
    fn missing_embedding_becomes_singleton() {
        // A chunk with no stored embedding cannot match anything — it must
        // still appear as its own cluster rather than being silently dropped.
        let chunks = vec![chunk("a", "doc.txt", 0.9), chunk("orphan", "doc.txt", 0.5)];
        let embeddings = vec![("a", vec![1.0, 0.0])];
        let cluster_input = input(chunks, embeddings);
        let clusters = cluster_input.cluster_by_source(0.85, usize::MAX);
        assert_eq!(clusters.len(), 2);
        let flat: Vec<usize> = clusters.iter().flatten().copied().collect();
        assert!(flat.contains(&1)); // orphan present
    }
}
