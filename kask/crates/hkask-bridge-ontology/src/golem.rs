//! GOLEM narrative/literary ontology bridge.
//!
//! Canonical predicate URIs for narrative concepts — characters, events,
//! themes, literary devices, and interpretive relationships. Used by
//! docproc extract_assertions for narrative passages (prose, fiction, memoir,
//! biography, narrative nonfiction) and by corpus tools for style
//! ontology mapping.
//!
//! Consolidated from the former duplicated `golem.rs` in the corpus server
//! and hkask-mcp-docproc — single owner for the unified corpus server.
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors the dc_bibo and pko modules in this
//! crate.

/// A GOLEM concept URI.
pub type GolemConcept = &'static str;

// ── Narrative element classes ─────────────────────

/// A character in a narrative work — an agent with traits, relationships,
/// and a narrative role. Maps to authorial style exemplars in the corpus.
pub const CHARACTER: GolemConcept = "golem:G1_Character";

/// An event or happening within a narrative — a plot point, a scene,
/// a significant occurrence. Maps to narrative arcs in an author corpus.
pub const EVENT: GolemConcept = "golem:G1_Event";

/// The setting of a narrative — temporal and spatial context.
pub const SETTING: GolemConcept = "golem:G1_Setting";

/// A narrative function — a structural role within the story
/// (e.g., Proppian functions, motifs, archetypes).
pub const NARRATIVE_FUNCTION: GolemConcept = "golem:G10_Narrative_Function";

// ── Relationships ─────────────────────────────────

/// Relationship between characters within a narrative.
pub const CHARACTER_RELATIONSHIP: GolemConcept = "golem:G1_Relationship";

/// A character participates in an event.
pub const PARTICIPATES_IN: GolemConcept = "golem:participatesIn";

/// A character is located in a setting.
pub const LOCATED_IN: GolemConcept = "golem:locatedIn";

// ── Work and authorship ───────────────────────────

/// A creative work — the narrative text itself.
/// Maps to the corpus works that corpus_compose ingests.
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

// ── Mapping helpers ────────────────────────────────────────

/// Map a corpus creative operation to its GOLEM concept.
///
/// Takes the bare operation name — the corpus tool name minus its `corpus_`
/// prefix (`corpus_compose` → `compose`). Only creative generation anchors on
/// GOLEM: compose and rewrite produce narrative prose (creative works).
/// Discovery is deliberately NOT here — it is a search action on the process
/// axis (`corpus_stage_to_pko_step`), not a creative work.
pub fn corpus_op_to_golem(op: &str) -> Option<GolemConcept> {
    match op {
        "compose" | "rewrite" => Some(CREATIVE_WORK),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_op_mapper_covers_creative_generation_only() {
        // Creative generation anchors on GOLEM; discovery is a process action
        // (corpus_stage_to_pko_step), not a creative work.
        assert_eq!(corpus_op_to_golem("compose"), Some(CREATIVE_WORK));
        assert_eq!(corpus_op_to_golem("rewrite"), Some(CREATIVE_WORK));
        assert_eq!(corpus_op_to_golem("discover"), None);
        assert_eq!(corpus_op_to_golem("convert"), None);
    }
}
