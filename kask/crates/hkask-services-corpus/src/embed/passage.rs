//! TaggedPassage — fully tagged passage with entity tags, method signals, and salience.

use hkask_memory::salience::{EntityTags, MethodSignals};
use hkask_services_runtime::TripleExtraction;

/// A fully tagged passage: text + entity tags + method signals + salience.
///
/// Carries everything needed for both embedding and h_mem storage.
#[derive(Debug, Clone)]
pub struct TaggedPassage {
    pub(crate) entity_ref: String,
    pub(crate) text: String,
    pub(crate) work_slug: String,
    pub(crate) work_title: String,
    /// Position within the work (0.0 = start, 1.0 = end).
    pub(crate) position: f32,
    /// Whether this is a foundational rule (excluded from centroid).
    pub(crate) is_rule: bool,
    /// Entity tags from config-declared entity matching.
    pub(crate) tags: EntityTags,
    /// Computed stylometric signals.
    pub(crate) signals: MethodSignals,
    /// Salience score (weighted graph degree).
    pub(crate) salience: f32,
    /// Dimension tag for this passage (from work metadata).
    pub(crate) dimension: String,
    /// Document type tag for this passage (from work metadata).
    pub(crate) document_type: String,
    /// MDS category tags for this passage (from work metadata).
    pub(crate) mds_categories: Vec<String>,
    /// Section type tag for this passage (from classifier or work declaration).
    pub(crate) section_type: String,
    /// Classifier-extracted semantic h_mems (topic, concepts, entities, relationships, quality).
    pub(crate) semantic_triples: TripleExtraction,
}

impl TaggedPassage {
    /// Count how many metadata h_mems this passage would consume if stored.
    /// Excludes the `text` h_mem — text is stored for all passages regardless
    /// of budget, since it's required for exemplar retrieval in compose.
    pub(crate) fn metadata_triple_count(&self) -> usize {
        // 6 structural + entity tags + method tags + 1 salience + 10 signals
        // + 4 orthogonal tags (dimension, doc_type, mds_categories, section_type)
        // + semantic h_mems: 1 topic + concepts + entities + relationships + 1 dimension + quality_flags
        6 + self.tags.characters.len()
            + self.tags.places.len()
            + self.tags.events.len()
            + self.tags.concepts.len()
            + self.tags.methods.len()
            + 1
            + 11 // salience + 10 method signals
            + 1 // dimension
            + 1 // document_type
            + self.mds_categories.len() // one per mds_category
            + 1 // section_type
            + if !self.semantic_triples.topic.is_empty() { 1 } else { 0 }
            + self.semantic_triples.concepts.len()
            + self.semantic_triples.entities.len()
            + self.semantic_triples.relationships.len()
            + if !self.semantic_triples.primary_dimension.is_empty() { 1 } else { 0 }
            + self.semantic_triples.quality_flags.len()
            + self.semantic_triples.extra.len()
    }

    /// Total h_mem count including text (for reporting only).
    pub(crate) fn triple_count(&self) -> usize {
        1 + self.metadata_triple_count()
    }
}
