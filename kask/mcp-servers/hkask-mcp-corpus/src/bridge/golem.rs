//! GOLEM narrative/literary ontology bridge.
//!
//! Canonical predicate URIs for narrative concepts — characters, events,
//! themes, literary devices, and interpretive relationships. Used by
//! docproc extract_triples for narrative passages (prose, fiction, memoir,
//! biography, narrative nonfiction) and by replica tools for persona/style
//! ontology mapping.
//!
//! Consolidated from the former duplicated `golem.rs` in hkask-mcp-replica
//! and hkask-mcp-docproc — single owner for the unified corpus server.
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors hkask-bridge-dublincore and
//! hkask-bridge-pko.

/// A GOLEM concept URI.
pub type GolemConcept = &'static str;

// ── Narrative element classes (from replica golem.rs) ─────────────────────

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

// ── Relationships (from replica golem.rs) ─────────────────────────────────

/// Relationship between characters within a narrative.
pub const CHARACTER_RELATIONSHIP: GolemConcept = "golem:G1_Relationship";

/// A character participates in an event.
pub const PARTICIPATES_IN: GolemConcept = "golem:participatesIn";

/// A character is located in a setting.
pub const LOCATED_IN: GolemConcept = "golem:locatedIn";

// ── Work and authorship (from replica golem.rs) ───────────────────────────

/// A creative work — the narrative text itself.
/// Maps to the corpus works that replica_build ingests.
pub const CREATIVE_WORK: GolemConcept = "golem:G1_CreativeWork";

/// The author/creator of a creative work.
pub const AUTHOR: GolemConcept = "golem:G1_Author";

// ── Characters and agents (predicate URIs from docproc golem.rs) ───────────

/// A character or person in the narrative.
pub const HAS_CHARACTER: GolemConcept = "golem:hasCharacter";
/// The narrator or narrative voice.
pub const HAS_NARRATOR: GolemConcept = "golem:hasNarrator";
/// The narrative perspective or point of view.
pub const HAS_PERSPECTIVE: GolemConcept = "golem:hasPerspective";

// ── Plot and structure ────────────────────────────────────────────────────

/// An event or action in the story.
pub const HAS_EVENT: GolemConcept = "golem:hasEvent";
/// A plot element or development.
pub const HAS_PLOT: GolemConcept = "golem:hasPlot";
/// A conflict or tension in the narrative.
pub const HAS_CONFLICT: GolemConcept = "golem:hasConflict";
/// How a conflict is resolved.
pub const HAS_RESOLUTION: GolemConcept = "golem:hasResolution";

// ── Setting and atmosphere ────────────────────────────────────────────────

/// The setting or location of the narrative.
pub const HAS_SETTING: GolemConcept = "golem:hasSetting";
/// The tone or mood of the passage.
pub const HAS_TONE: GolemConcept = "golem:hasTone";

// ── Theme and meaning ─────────────────────────────────────────────────────

/// The central theme or idea.
pub const HAS_THEME: GolemConcept = "golem:hasTheme";
/// A recurring motif or pattern.
pub const HAS_MOTIF: GolemConcept = "golem:hasMotif";
/// A symbol or symbolic element.
pub const HAS_SYMBOL: GolemConcept = "golem:hasSymbol";

// ── Interpretive relationships ───────────────────────────────────────────

/// Allegorical meaning or representation.
pub const ALLEGORY_OF: GolemConcept = "golem:allegoryOf";
/// Metaphorical meaning.
pub const METAPHOR_FOR: GolemConcept = "golem:metaphorFor";
/// What concept or principle the narrative illustrates.
pub const ILLUSTRATES: GolemConcept = "golem:illustrates";
/// What emotion or idea the passage evokes.
pub const EVOKES: GolemConcept = "golem:evokes";

/// All GOLEM predicates, for validation or iteration.
pub const ALL_PREDICATES: &[GolemConcept] = &[
    HAS_CHARACTER,
    HAS_NARRATOR,
    HAS_PERSPECTIVE,
    HAS_EVENT,
    HAS_PLOT,
    HAS_CONFLICT,
    HAS_RESOLUTION,
    HAS_SETTING,
    HAS_TONE,
    HAS_THEME,
    HAS_MOTIF,
    HAS_SYMBOL,
    ALLEGORY_OF,
    METAPHOR_FOR,
    ILLUSTRATES,
    EVOKES,
];

// ── Mapping helpers (from replica golem.rs) ────────────────────────────────

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
