//! HMem storage helpers for the embedding pipeline.

use super::passage::TaggedPassage;
use hkask_memory::SemanticMemory;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_storage::HMem;
use hkask_types::Visibility;
use hkask_types::id::WebID;
use serde_json::json;

pub(crate) fn store_passage_h_mems(
    semantic: &SemanticMemory,
    passage: &TaggedPassage,
    author: &str,
    owner: WebID,
) -> Result<(), ServiceError> {
    let store = |entity: &str, attr: &str, value: serde_json::Value| -> Result<(), ServiceError> {
        let h_mem = HMem::new(entity, attr, value, owner).with_visibility(Visibility::Shared);
        semantic.store(h_mem).map_err(|e| {
            let msg = format!("Failed to store h_mem ({entity}, {attr}): {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })
    };

    let er = &passage.entity_ref;

    // Passage text — required for exemplar retrieval in compose
    store(er, "text", json!(passage.text))?;

    // Structural metadata
    store(er, "author", json!(*author))?;
    store(er, "work_title", json!(passage.work_title))?;
    store(er, "work_slug", json!(passage.work_slug))?;
    store(er, "position", json!(passage.position))?;
    store(er, "word_count", json!(passage.signals.word_count))?;
    store(
        er,
        "avg_sentence_length",
        json!(passage.signals.avg_sentence_length),
    )?;

    // Entity tags (who, where, what, why)
    for c in &passage.tags.characters {
        store(er, "mentions_character", json!(c))?;
    }
    for p in &passage.tags.places {
        store(er, "mentions_place", json!(p))?;
    }
    for e in &passage.tags.events {
        store(er, "mentions_event", json!(e))?;
    }
    for c in &passage.tags.concepts {
        store(er, "mentions_concept", json!(c))?;
    }

    // Method tags (how)
    for m in &passage.tags.methods {
        store(er, "exhibits_method", json!(m))?;
    }

    // Method signals
    let s = &passage.signals;
    store(er, "parataxis_ratio", json!(s.parataxis_ratio))?;
    store(er, "adjective_density", json!(s.adjective_density))?;
    store(er, "adverb_density", json!(s.adverb_density))?;
    store(er, "passive_voice_ratio", json!(s.passive_voice_ratio))?;
    store(er, "dialogue_ratio", json!(s.dialogue_ratio))?;
    store(
        er,
        "sentence_length_variance",
        json!(s.sentence_length_variance),
    )?;
    store(er, "hedge_density", json!(s.hedge_density))?;
    store(er, "intensifier_density", json!(s.intensifier_density))?;
    store(er, "concrete_noun_ratio", json!(s.concrete_noun_ratio))?;
    store(er, "sensory_word_ratio", json!(s.sensory_word_ratio))?;

    // Salience
    store(er, "salience", json!(passage.salience))?;

    // Orthogonal tags (Gentle Lovelace dimensions)
    if !passage.dimension.is_empty() {
        store(er, "has_dimension", json!(passage.dimension))?;
    }
    if !passage.document_type.is_empty() {
        store(er, "document_type", json!(passage.document_type))?;
    }
    for cat in &passage.mds_categories {
        store(er, "has_mds_category", json!(cat))?;
    }
    if !passage.section_type.is_empty() {
        store(er, "has_section_type", json!(passage.section_type))?;
    }

    // Classifier-extracted semantic h_mems
    let st = &passage.semantic_triples;
    if !st.topic.is_empty() {
        store(er, "extracted_topic", json!(st.topic))?;
    }
    for concept in &st.concepts {
        store(er, "extracted_concept", json!(concept))?;
    }
    for entity in &st.entities {
        store(er, "extracted_entity", json!(entity))?;
    }
    for rel in &st.relationships {
        store(er, "extracted_relationship", json!(rel))?;
    }
    if !st.primary_dimension.is_empty() {
        store(er, "primary_dimension", json!(st.primary_dimension))?;
    }
    for flag in &st.quality_flags {
        store(er, "has_quality_flag", json!(flag))?;
    }

    // Extra fields from classifier (literary: themes, characters, setting, tone, imagery, etc.)
    for (key, val) in &st.extra {
        store(er, key, val.clone())?;
    }

    Ok(())
}
