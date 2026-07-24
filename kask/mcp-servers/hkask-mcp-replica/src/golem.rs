//! GOLEM ontology bridge for hkask-mcp-replica.
//!
//! Maps hKask replica server concepts to the GOLEM ontology for narrative
//! and fiction. GOLEM models characters, relationships, events, settings,
//! and narrative functions — extending CIDOC-CRM and LRMoo, aligned with DOLCE.
//!
//! Reference: Pianzola et al. (2024), GOLEM Ontology for Narrative and Fiction
//! Repository: <https://github.com/GOLEM-lab/golem-ontology>
//! Project: <https://golemlab.eu/> (ERC StG, 2023–2027)
//!
//! Pattern: thin mapping layer — canonical URI constants, mapping functions,
//! no dependencies, no reasoners, no overhead ≤100 lines.
//!
//! # Shared Bridge Integration
//!
//! Uses [`hkask_bridge_dublincore`] for creative work classification
//! (e.g., `dctypes:Text`, `bibo:Book`) and [`hkask_bridge_dublincore`] for
//! narrative procedure classification.

/// A GOLEM concept URI.
pub type GolemConcept = &'static str;

// ── Narrative elements ────────────────────────────────────────────────────

/// A character in a narrative work — an agent with traits, relationships,
/// and a narrative role. Maps to authorial personas in replica.
pub const CHARACTER: GolemConcept = "golem:G1_Character";

/// An event or happening within a narrative — a plot point, a scene,
/// a significant occurrence. Maps to narrative arcs in an author corpus.
pub const EVENT: GolemConcept = "golem:G1_Event";

/// The setting of a narrative — temporal and spatial context.
pub const SETTING: GolemConcept = "golem:G1_Setting";

/// A narrative function — a structural role within the story
/// (e.g., Proppian functions, motifs, archetypes).
pub const NARRATIVE_FUNCTION: GolemConcept = "golem:G10_Narrative_Function";

// ── Relationships ─────────────────────────────────────────────────────────

/// Relationship between characters within a narrative.
pub const CHARACTER_RELATIONSHIP: GolemConcept = "golem:G1_Relationship";

/// A character participates in an event.
pub const PARTICIPATES_IN: GolemConcept = "golem:participatesIn";

/// A character is located in a setting.
pub const LOCATED_IN: GolemConcept = "golem:locatedIn";

// ── Work and authorship ───────────────────────────────────────────────────

/// A creative work — the narrative text itself.
/// Maps to the corpus works that replica_build ingests.
pub const CREATIVE_WORK: GolemConcept = "golem:G1_CreativeWork";

/// The author/creator of a creative work.
pub const AUTHOR: GolemConcept = "golem:G1_Author";

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a replica server operation to its GOLEM concept.
pub fn replica_op_to_golem(op: &str) -> Option<GolemConcept> {
    match op {
        "replica_build" => Some(AUTHOR),
        "replica_compose" => Some(CREATIVE_WORK),
        "replica_mashup" => Some(NARRATIVE_FUNCTION),
        "replica_discover" => Some(CREATIVE_WORK),
        "replica_compare" => Some(CHARACTER),
        _ => None,
    }
}

/// Map a style attribute to a narrative concept.
pub fn style_dimension_to_golem(dim: &str) -> Option<GolemConcept> {
    match dim.to_lowercase().as_str() {
        "voice" | "tone" | "persona" => Some(CHARACTER),
        "setting" | "atmosphere" | "place" => Some(SETTING),
        "plot" | "structure" | "arc" => Some(NARRATIVE_FUNCTION),
        "character" | "protagonist" => Some(CHARACTER),
        "event" | "scene" | "action" => Some(EVENT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replica_ops_map_to_golem() {
        assert_eq!(replica_op_to_golem("replica_build"), Some(AUTHOR));
        assert_eq!(replica_op_to_golem("replica_compose"), Some(CREATIVE_WORK));
        assert_eq!(
            replica_op_to_golem("replica_mashup"),
            Some(NARRATIVE_FUNCTION)
        );
        assert_eq!(replica_op_to_golem("unknown_op"), None);
    }

    #[test]
    fn style_dimensions_map_to_golem() {
        assert_eq!(style_dimension_to_golem("voice"), Some(CHARACTER));
        assert_eq!(style_dimension_to_golem("setting"), Some(SETTING));
        assert_eq!(style_dimension_to_golem("plot"), Some(NARRATIVE_FUNCTION));
        assert_eq!(style_dimension_to_golem("rhyme_scheme"), None);
    }
}
