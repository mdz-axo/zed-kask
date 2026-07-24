//! Shared math and text helpers used across docproc tool modules.

/// Cosine similarity between two vectors. Consolidated from ocr/semantic.rs (C4).
/// Returns 0.0 if either vector is empty or dimensions mismatch.
pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
}

/// Approximate token-to-word conversion: 1 word ≈ 1.33 tokens.
/// So tokens ÷ 1.33 = words. This is the standard BPE ratio for English text.
pub(crate) fn tokens_to_words(tokens: usize) -> usize {
    ((tokens as f64) / 1.33) as usize
}

/// Compute (max_words, min_words) from (max_tokens, overlap_tokens).
/// `overlap_tokens` determines the minimum chunk size (hard floor below which
/// a buffer won't flush). Falls back to HkaskSettings::chunk_max_tokens() when
/// max_tokens is None.
pub(crate) fn chunk_word_bounds(
    max_tokens: Option<usize>,
    overlap_tokens: Option<usize>,
) -> (usize, usize) {
    let default_max = HkaskSettings::load().chunk_max_tokens();
    let max_w = tokens_to_words(max_tokens.unwrap_or(default_max));
    let min_w = tokens_to_words(overlap_tokens.unwrap_or(64)).max(max_w / 4);
    (max_w, min_w)
}

/// Serialize (entity_ref, text) pair slice into json.
pub(crate) fn serialize_passages(passages: &[(String, String)]) -> Vec<serde_json::Value> {
    passages
        .iter()
        .map(|(entity_ref, passage_text)| json!({"entity_ref": entity_ref, "text": passage_text}))
        .collect()
}

/// Chunk a `DocStructure` into passages, respecting heading boundaries.
///
/// Groups blocks under their nearest preceding heading. Each group becomes
/// one or more passages via `SemanticMemory::chunk_text`. When a group exceeds
/// `max_words`, it is split at sentence boundaries within the group. When a
/// group is smaller than `min_words`, it is merged with the next group if
/// possible (to avoid tiny chunks).
///
/// Falls back to flat `chunk_text` when the structure has no headings.
pub(crate) fn chunk_structure(
    structure: &hkask_types::document::DocStructure,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    boundary: &str,
) -> Vec<(String, String)> {
    use hkask_types::document::Block;

    // Collect all blocks across pages, tracking heading starts.
    let blocks: Vec<&Block> = structure.iter_blocks().collect();

    // If no headings, flatten to text and use the standard chunker.
    let has_headings = blocks.iter().any(|b| b.is_heading());
    if !has_headings {
        let flat_text = structure.text();
        return SemanticMemory::chunk_text(
            &flat_text,
            entity_ref_prefix,
            min_words,
            max_words,
            boundary,
        );
    }

    // Group blocks by section (each heading starts a new section).
    let mut sections: Vec<(String, String)> = Vec::new(); // (heading_text, body_text)
    let mut current_heading = String::new();
    let mut current_body = String::new();

    for block in &blocks {
        match block {
            Block::Heading { text, .. } => {
                // Flush previous section
                if !current_body.trim().is_empty() || !current_heading.is_empty() {
                    sections.push((current_heading.clone(), current_body.clone()));
                }
                current_heading = text.clone();
                current_body.clear();
            }
            _ => {
                let block_text = block.text();
                if !current_body.is_empty() {
                    current_body.push_str("\n\n");
                }
                current_body.push_str(&block_text);
            }
        }
    }
    // Flush final section
    if !current_body.trim().is_empty() || !current_heading.is_empty() {
        sections.push((current_heading.clone(), current_body.clone()));
    }

    // Chunk each section, prepending the heading as context.
    let mut passages = Vec::new();
    for (idx, (heading, body)) in sections.iter().enumerate() {
        if body.trim().is_empty() {
            continue;
        }
        // Prepend heading to body so each chunk knows its section.
        let section_text = if heading.is_empty() {
            body.clone()
        } else {
            format!("{heading}\n\n{body}")
        };
        let section_ref = format!("{entity_ref_prefix}:sec{idx}");
        let section_passages =
            SemanticMemory::chunk_text(&section_text, &section_ref, min_words, max_words, boundary);
        passages.extend(section_passages);
    }
    passages
}

use crate::*;

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::document::{Block, DocStructure, Page};

    #[test]
    fn cosine_similarity_zero_for_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_similarity_identical() {
        let sim = cosine_similarity(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]);
        assert!((sim - 1.0).abs() < 1e-5, "expected ~1.0, got {sim}");
    }

    #[test]
    fn tokens_to_words_approximate() {
        assert_eq!(tokens_to_words(133), 100);
        assert_eq!(tokens_to_words(0), 0);
    }

    fn sample_structure_with_headings() -> DocStructure {
        DocStructure {
            source_format: "docx".to_string(),
            pages: vec![Page {
                page_number: 1,
                blocks: vec![
                    Block::Heading {
                        level: 1,
                        text: "Introduction".to_string(),
                    },
                    Block::Paragraph {
                        text: "This is the intro paragraph. It has two sentences.".to_string(),
                    },
                    Block::Heading {
                        level: 2,
                        text: "Methods".to_string(),
                    },
                    Block::Paragraph {
                        text: "We used Rust. It was fast.".to_string(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn chunk_structure_respects_heading_boundaries() {
        let structure = sample_structure_with_headings();
        // Large max_words so each section fits in one chunk.
        let passages = chunk_structure(&structure, "doc", 1, 1000, ".!?");
        // Two sections → at least two passages (one per section).
        assert!(
            passages.len() >= 2,
            "expected >= 2 passages, got {}",
            passages.len()
        );
        // Each passage should contain its heading text.
        assert!(
            passages
                .iter()
                .any(|(_, text)| text.contains("Introduction")),
            "no passage contains Introduction heading"
        );
        assert!(
            passages.iter().any(|(_, text)| text.contains("Methods")),
            "no passage contains Methods heading"
        );
    }

    #[test]
    fn chunk_structure_falls_back_when_no_headings() {
        let structure = DocStructure::from_plain_text("Just a paragraph. With text.", "plain");
        let passages = chunk_structure(&structure, "doc", 1, 1000, ".!?");
        assert!(!passages.is_empty());
    }
}
