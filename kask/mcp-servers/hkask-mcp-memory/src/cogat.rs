//! Cognitive Atlas (CogAT) ontology bridge for hkask-mcp-memory.
//!
//! Maps hKask memory server concepts to Cognitive Atlas cognitive science
//! concepts. CogAT characterizes the state of current thought in cognitive
//! science with 918 concepts, 857 tasks, and 242 phenotypes.
//!
//! Reference: <https://www.cognitiveatlas.org>
//! Reference: <https://github.com/CognitiveAtlas/cogat-ontology>
//! PI: Russell Poldrack, Stanford University (NIMH Grant RO1MH082795)
//!
//! Note: This is a metaphorical mapping. hKask's "episodic memory" is a
//! software system inspired by but not identical to cognitive episodic memory.
//! The bridge makes the isomorphism level explicit.
//!
//! # Shared Bridge Integration
//!
//! Uses [`hkask_bridge_dublincore`] for entity type classification
//! (e.g., `dctypes:Dataset` for memory stores) and [`hkask_bridge_dublincore`]
//! for process classification (e.g., `pko:Step` for memory operations).

/// A Cognitive Atlas concept identifier.
pub type CogatConcept = &'static str;

// ── Core memory systems ───────────────────────────────────────────────────

/// Memory for personally experienced events, bound to time and place.
/// hKask mapping: episodic_store / episodic_recall / episodic_recall_context
pub const EPISODIC_MEMORY: CogatConcept = "cogat:episodic_memory";

/// Memory for facts, concepts, and general knowledge, detached from context.
/// hKask mapping: semantic_store / semantic_recall / semantic_search
pub const SEMANTIC_MEMORY: CogatConcept = "cogat:semantic_memory";

/// Temporary storage and manipulation of information.
/// hKask mapping: active context window, in-flight tool results
pub const WORKING_MEMORY: CogatConcept = "cogat:working_memory";

// ── Memory processes ──────────────────────────────────────────────────────

/// The process of converting perceived information into a memory trace.
/// hKask mapping: episodic_store, semantic_store (initial write)
pub const ENCODING: CogatConcept = "cogat:encoding";

/// The process of retrieving stored information.
/// hKask mapping: episodic_recall, semantic_recall, memory_recall
pub const RECALL: CogatConcept = "cogat:recall";

/// Cued recall — retrieval triggered by an associated stimulus.
/// hKask mapping: episodic_recall_context (salience-ranked by context)
pub const CUED_RECALL: CogatConcept = "cogat:cued_recall";

/// The process of stabilizing a memory trace after initial encoding.
/// hKask mapping: episodic_consolidate_status, episodic→semantic promotion
pub const MEMORY_CONSOLIDATION: CogatConcept = "cogat:memory_consolidation";

/// The loss of memory over time or due to interference.
/// hKask mapping: semantic_purge, episodic budget eviction
pub const FORGETTING: CogatConcept = "cogat:forgetting";

// ── Organization ──────────────────────────────────────────────────────────

/// Grouping individual items into larger, meaningful units.
/// hKask mapping: semantic_chunk (text → passages)
pub const CHUNKING: CogatConcept = "cogat:chunking";

/// The quality of being particularly noticeable or significant.
/// hKask mapping: episodic_recall_context salience ranking
pub const SALIENCE: CogatConcept = "cogat:salience";

/// Semantic processing — accessing the meaning of information.
/// hKask mapping: semantic_search, semantic_embed
pub const SEMANTIC_PROCESSING: CogatConcept = "cogat:semantic_processing";

/// The formation of a mental representation of a category or concept.
/// hKask mapping: semantic_centroid (mean embedding → prototype)
pub const CONCEPT_FORMATION: CogatConcept = "cogat:concept_formation";

/// Spreading activation across associated concepts in semantic networks.
/// hKask mapping: semantic_search KNN, memory_recall paired recall
pub const SEMANTIC_PRIMING: CogatConcept = "cogat:semantic_priming";

// ── Memory types / distinctions ───────────────────────────────────────────

/// Memory for how to perform tasks and skills.
/// hKask mapping: skill registry, adapter loading, procedural knowledge
pub const PROCEDURAL_MEMORY: CogatConcept = "cogat:procedural_memory";

/// Explicit, conscious memory (contrast with implicit).
/// hKask mapping: all semantically_store'd h_mems
pub const EXPLICIT_MEMORY: CogatConcept = "cogat:explicit_memory";

/// Recognition — identifying previously encountered information.
/// hKask mapping: semantic_search (similarity match against stored embeddings)
pub const RECOGNITION_MEMORY: CogatConcept = "cogat:recognition_memory";

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a memory server operation to its CogAT concept.
pub fn memory_op_to_cogat(op: &str) -> Option<CogatConcept> {
    match op {
        "episodic_store" => Some(ENCODING),
        "episodic_recall" => Some(RECALL),
        "episodic_recall_context" => Some(CUED_RECALL),
        "episodic_consolidate_status" => Some(MEMORY_CONSOLIDATION),
        "semantic_store" => Some(ENCODING),
        "semantic_recall" => Some(RECALL),
        "semantic_search" => Some(SEMANTIC_PRIMING),
        "semantic_embed" => Some(SEMANTIC_PROCESSING),
        "semantic_centroid" => Some(CONCEPT_FORMATION),
        "semantic_chunk" => Some(CHUNKING),
        "semantic_purge" => Some(FORGETTING),
        "memory_recall" => Some(CUED_RECALL),
        _ => None,
    }
}

/// Map a memory type string to its CogAT concept.
pub fn memory_type_to_cogat(memory_type: &str) -> Option<CogatConcept> {
    match memory_type.to_lowercase().as_str() {
        "episodic" => Some(EPISODIC_MEMORY),
        "semantic" => Some(SEMANTIC_MEMORY),
        "procedural" => Some(PROCEDURAL_MEMORY),
        "working" => Some(WORKING_MEMORY),
        "explicit" => Some(EXPLICIT_MEMORY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_ops_map_to_cogat() {
        assert_eq!(memory_op_to_cogat("episodic_store"), Some(ENCODING));
        assert_eq!(memory_op_to_cogat("semantic_recall"), Some(RECALL));
        assert_eq!(
            memory_op_to_cogat("episodic_recall_context"),
            Some(CUED_RECALL)
        );
        assert_eq!(
            memory_op_to_cogat("episodic_consolidate_status"),
            Some(MEMORY_CONSOLIDATION)
        );
        assert_eq!(memory_op_to_cogat("semantic_purge"), Some(FORGETTING));
        assert_eq!(
            memory_op_to_cogat("semantic_centroid"),
            Some(CONCEPT_FORMATION)
        );
        assert_eq!(memory_op_to_cogat("unknown_op"), None);
    }

    #[test]
    fn memory_types_map_to_cogat() {
        assert_eq!(memory_type_to_cogat("episodic"), Some(EPISODIC_MEMORY));
        assert_eq!(memory_type_to_cogat("semantic"), Some(SEMANTIC_MEMORY));
        assert_eq!(memory_type_to_cogat("procedural"), Some(PROCEDURAL_MEMORY));
        assert_eq!(memory_type_to_cogat("nonexistent"), None);
    }
}
