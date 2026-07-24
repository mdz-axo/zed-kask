//! Clustering helpers — shared by dedup and consolidation.
//!
//! `cluster_within_source` groups chunks by cosine similarity within a source
//! file, sorted by salience descending. Used by `docproc_dedup_chunks` and
//! `docproc_consolidate_chunks` in `mod.rs`.

use crate::*;
use hkask_types::corpus::TaggedChunk;

/// Read tagged chunks from a JSONL file. Malformed lines are silently dropped.
pub(crate) fn read_tagged_chunks(path: &str) -> Result<Vec<TaggedChunk>, McpToolError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        McpToolError::invalid_argument(format!("Cannot read tagged_jsonl '{path}': {e}"))
    })?;
    let chunks: Vec<TaggedChunk> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    Ok(chunks)
}

/// Greedy clustering within a source file.
/// Returns clusters as Vecs of indices into `chunks` (sorted by salience desc).
///
/// Vectors in `norm_map` must be pre-normalized (via `normalize_in_place`) so
/// that the dot product equals cosine similarity.
pub(crate) fn cluster_within_source(
    chunk_indices: &[usize],
    chunks: &[TaggedChunk],
    norm_map: &std::collections::HashMap<&str, &[f32]>,
    threshold: f32,
    max_per_cluster: usize,
) -> Vec<Vec<usize>> {
    // Gather (index, embedding) pairs, sorted by salience descending
    let mut indexed_embs: Vec<(usize, &[f32])> = chunk_indices
        .iter()
        .filter_map(|&idx| {
            norm_map
                .get(chunks[idx].entity_ref.as_str())
                .copied()
                .map(|emb| (idx, emb))
        })
        .collect();
    indexed_embs.sort_by(|a, b| {
        chunks[b.0]
            .salience
            .partial_cmp(&chunks[a.0].salience)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut cluster_reps: Vec<&[f32]> = Vec::new();
    let mut cluster_members: Vec<Vec<usize>> = Vec::new();

    for (idx, emb) in &indexed_embs {
        let mut found = None;
        for (ci, rep_emb) in cluster_reps.iter().enumerate() {
            // Vectors are pre-normalized, so the dot product equals cosine similarity.
            let cosine_sim: f32 = emb.iter().zip(rep_emb.iter()).map(|(a, b)| a * b).sum();
            if cosine_sim > threshold {
                found = Some(ci);
                break;
            }
        }
        match found {
            Some(ci) if cluster_members[ci].len() < max_per_cluster => {
                cluster_members[ci].push(*idx);
            }
            _ => {
                cluster_reps.push(emb);
                cluster_members.push(vec![*idx]);
            }
        }
    }

    // Append chunks without embeddings as singletons
    for &idx in chunk_indices {
        if !norm_map.contains_key(chunks[idx].entity_ref.as_str()) {
            cluster_members.push(vec![idx]);
        }
    }

    cluster_members
}
