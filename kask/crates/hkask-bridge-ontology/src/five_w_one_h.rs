//! 5W1H (Five Ws and One H) interrogative ontology bridge.
//!
//! The six universal interrogative pronouns — Who, What, When, Where, Why,
//! How — as a first-class ontology vocabulary. 5W1H is the universal
//! interrogative ground: every chunk, h_mem, and tagged artifact answers at
//! least one interrogative. This module makes the 5W1H vocabulary a typed
//! ontology in the bridge (previously it existed only as the `Dimension` enum
//! in `hkask-types` and as hardcoded strings in the tagging template).
//!
//! 5W1H is not a domain supplement — it is the universal core that every
//! domain supplement layers on top of. The `OntologyAnchor::Core` variant
//! anchors here. The `Dimension` enum in `hkask-types::visibility` is the
//! consumer of this vocabulary; this module is the single source of truth for
//! the canonical concept URIs.
//!
//! Reference: the 5W1H method is a foundational journalism/investigation
//! framework (Kipling's "six honest serving-men"). The interrogative pronouns
//! map to the Dublin Core state axis (Who/What/Where/When) and the PKO
//! process axis (Why/How) — the dual-axis framework's universal ground.

/// A 5W1H concept URI — the canonical identifier for an interrogative dimension.
pub type FiveWOneHConcept = &'static str;

// ── The six interrogative dimensions ────────────────────────────────────────

/// Who — an agent, persona, actor, or entity identity.
/// Maps to Dublin Core `dcterms:creator` / `dcterms:contributor` (state axis).
pub const WHO: FiveWOneHConcept = "5w1h:Who";

/// What — an event, action, occurrence, or state change.
/// Maps to Dublin Core `dcterms:type` / `dcterms:description` (state axis).
pub const WHAT: FiveWOneHConcept = "5w1h:What";

/// When — a temporal fact, timestamp, duration, or ordering.
/// Maps to Dublin Core `dcterms:date` / `dcterms:created` (state axis).
pub const WHEN: FiveWOneHConcept = "5w1h:When";

/// Where — a location, path, address, or spatial context.
/// Maps to Dublin Core `dcterms:coverage` (spatial) (state axis).
pub const WHERE: FiveWOneHConcept = "5w1h:Where";

/// Why — a reason, cause, motivation, or dependency.
/// Maps to PKO `pko:ProcedureTarget` / `pko:requiresAction` (process axis).
pub const WHY: FiveWOneHConcept = "5w1h:Why";

/// How — a method, technique, mechanism, or procedure.
/// Maps to PKO `pko:Procedure` / `pko:Step` (process axis).
pub const HOW: FiveWOneHConcept = "5w1h:How";

/// All 5W1H concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[FiveWOneHConcept] = &[WHO, WHAT, WHEN, WHERE, WHY, HOW];

/// The six interrogative dimension labels (lowercase), matching the
/// `Dimension` enum in `hkask-types::visibility`. Used by the tagging
/// template and the `TaggedChunk.dimensions` field.
pub const DIMENSION_LABELS: &[&str] = &["who", "what", "when", "where", "why", "how"];

/// Map a lowercase dimension label to its canonical 5W1H concept URI.
///
/// Returns `None` for an unknown label. This is the bridge between the
/// `Dimension` enum (used in h_mem storage) and the ontology vocabulary
/// (used in tagging and KG triples).
#[must_use]
pub fn concept_for_label(label: &str) -> Option<FiveWOneHConcept> {
    match label.to_lowercase().as_str() {
        "who" => Some(WHO),
        "what" => Some(WHAT),
        "when" => Some(WHEN),
        "where" => Some(WHERE),
        "why" => Some(WHY),
        "how" => Some(HOW),
        _ => None,
    }
}

/// Map a 5W1H concept URI back to its lowercase dimension label.
#[must_use]
pub fn label_for_concept(concept: &str) -> Option<&'static str> {
    match concept {
        WHO => Some("who"),
        WHAT => Some("what"),
        WHEN => Some("when"),
        WHERE => Some("where"),
        WHY => Some("why"),
        HOW => Some("how"),
        _ => None,
    }
}

/// Which dual-axis this 5W1H dimension maps to.
///
/// Who/What/When/Where are state-axis (Dublin Core); Why/How are process-axis
/// (PKO). This is the dual-axis grounding of the interrogative framework.
#[must_use]
pub fn axis_for_concept(concept: &str) -> Option<crate::axis::OntologyAxis> {
    match concept {
        WHO | WHAT | WHEN | WHERE => Some(crate::axis::OntologyAxis::DcBibo),
        WHY | HOW => Some(crate::axis::OntologyAxis::Pko),
        _ => None,
    }
}
