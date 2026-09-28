//! GOLEM narrative/literary ontology bridge.
//!
//! Maps hKask narrative concepts to the GOLEM ontology (Golem Ontology for
//! Narrative and Fiction), v1.1. GOLEM is an extension of CIDOC-CRM and LRMoo
//! aligned to DOLCE-Lite-Plus: it defines the `gc:` classes and properties
//! below and otherwise reuses `crm:` (CIDOC-CRM), `lrmoo:` (LRMoo), and
//! `dlp:` (DOLCE-Lite-Plus) terms. GOLEM, CIDOC-CRM and LRMoo are loaded in
//! full from `sources/` and resolved through `published`; this module names
//! only the concepts hKask code emits, and `all_terms_are_official` fails the
//! build unless each is published. DOLCE-Lite-Plus is not vendored (its
//! modules state no license): the `dlp:` constants are listed in
//! `DLP_PENDING_SOURCE` and stay unverifiable until it is.
//!
//! Reference: Pianzola, Pannach, Cheng, Yang, Scotti (GOLEM Lab, 2024).
//! <https://ontology.golemlab.eu/> — IRI <https://w3id.org/golem/ontology>,
//! version 1.1, CC BY 4.0, doi:10.5281/zenodo.14911396.
//! Preferred prefix `gc:`, namespace <https://w3id.org/golem/ontology#>.
//!
//! Used by corpus extract_assertions for narrative passages (prose, fiction,
//! memoir, narrative nonfiction) and by the corpus server's ontology_anchor
//! for creative-generation tools.
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors the dc_bibo and pko modules in this
//! crate.

/// A GOLEM concept URI (prefixed canonical form, e.g. `gc:G1_Character`).
pub type GolemConcept = &'static str;

/// Defines the vocabulary constants and registers every one in `ALL_TERMS`,
/// so the publication guard covers each constant by construction.
macro_rules! golem_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: GolemConcept = $uri;)*

        /// Every term in this module, so the publication guard covers each
        /// constant by construction. New terms must go through this macro.
        #[cfg(test)]
        const ALL_TERMS: &[GolemConcept] = &[$($name),*];
    };
}

golem_terms! {
    /// A created intellectual work — the outcome of an intellectual process
    /// of one or more persons (LRMoo F1, reused by GOLEM). GOLEM has no
    /// CreativeWork class; F1_Work is the concept for what corpus_compose
    /// and corpus_rewrite produce.
    WORK = "lrmoo:F1_Work",

    /// A realisation of a work in a specific form — the text itself
    /// (LRMoo F2, reused by GOLEM).
    EXPRESSION = "lrmoo:F2_Expression",

    /// A character in a narrative work — an agent with traits,
    /// relationships, and a narrative role.
    CHARACTER = "gc:G1_Character",

    /// A narrative event — a change of state, process, or state of things
    /// that supports the story.
    NARRATIVE_EVENT = "gc:G5_Narrative_Event",

    /// The narrative universe in which a story unfolds — spatial, cultural,
    /// and social context.
    SETTING = "gc:G12_Setting",

    /// A social relationship between characters within a narrative.
    SOCIAL_RELATIONSHIP = "gc:G4_Social_Relationship",

    /// A narrative sequence — fabula or syuzhet, the ordered events of a
    /// narrative (the GOLEM concept covering plot).
    NARRATIVE_SEQUENCE = "gc:G7_Narrative_Sequence",

    /// A narrative function — a structural role within the story
    /// (e.g., Proppian functions).
    NARRATIVE_FUNCTION = "gc:G10_Narrative_Function",

    /// A narrative role — the functional roles characters play, e.g.
    /// narrator, protagonist, antagonist.
    NARRATIVE_ROLE = "gc:G11_Narrative_Role",

    /// A feature of a narrative or character — style, theme, literary
    /// devices (GOLEM G2; specialized by G17 Character Feature and
    /// G18 Textual Feature).
    FEATURE = "gc:G2_Feature",

    /// A character trait — biographical, physical, or psychological
    /// (GOLEM G17, subclass of G2 Feature).
    CHARACTER_FEATURE = "gc:G17_Character_Feature",

    /// A textual feature — narrative style, tone, point of view, diction
    /// (GOLEM G18, subclass of G2 Feature).
    TEXTUAL_FEATURE = "gc:G18_Textual_Feature",

    /// A work has a character (GOLEM GP1i, inverse of GP1_is_character_in).
    HAS_CHARACTER = "gc:GP1i_has_Character",

    /// A character appears in a work (GOLEM GP1).
    IS_CHARACTER_IN = "gc:GP1_is_character_in",

    /// A narrative or character has a feature — theme, tone, style, motif
    /// (GOLEM GP0). The GOLEM cover for the former invented
    /// hasTheme/hasTone/hasMotif/hasSymbol predicates.
    HAS_FEATURE = "gc:GP0_has_feature",

    /// A feature is a feature of a narrative or character (GOLEM GP0i).
    IS_FEATURE_OF = "gc:GP0i_is_feature_of",

    /// An endurant (character, object) participates in a narrative event
    /// (DOLCE-Lite-Plus, reused by GOLEM).
    PARTICIPANT_IN = "dlp:participant-in",

    /// A narrative event has an endurant participant (DOLCE-Lite-Plus).
    PARTICIPANT = "dlp:participant",

    /// The location of an enduring entity within the narrative
    /// (DOLCE-Lite-Plus, reused by GOLEM).
    GENERIC_LOCATION = "dlp:generic-location",

    /// The setting of an entity — links a character, object, or location to
    /// the narrative setting it is in (DOLCE-Lite-Plus `setting`).
    HAS_SETTING = "dlp:setting",

    /// A psychological state of a character (DOLCE-Lite-Plus, reused by
    /// GOLEM for G3 Psychological State).
    HAS_STATE = "dlp:has-state",

    /// A propositional object (text, narrative unit) makes a statement
    /// about an entity (CIDOC-CRM P67, reused by GOLEM). The honest cover
    /// for interpretive reference — allegory, metaphor, illustration.
    REFERS_TO = "crm:P67_refers_to",

    /// A work is realised in an expression (LRMoo R3, reused by GOLEM).
    REALISED_IN = "lrmoo:R3_is_realised_in",
}

/// Map a predicate prefix from the GOLEM family of namespaces to the
/// chunk-tag namespace key used by the tagging pipeline
/// (`canonicalize_terms` groups resolved GOLEM concepts under `"golem"`).
/// GOLEM's own `gc:` terms and the CIDOC-CRM / LRMoo / DOLCE-Lite-Plus
/// terms it reuses all belong to that one tag family. Returns `None` for
/// prefixes outside the family.
pub fn tag_family(predicate_prefix: &str) -> Option<&'static str> {
    match predicate_prefix.to_lowercase().as_str() {
        "gc" | "crm" | "dlp" | "lrmoo" | "golem" => Some("golem"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_family_covers_golem_reused_namespaces() {
        assert_eq!(tag_family("gc"), Some("golem"));
        assert_eq!(tag_family("crm"), Some("golem"));
        assert_eq!(tag_family("dlp"), Some("golem"));
        assert_eq!(tag_family("lrmoo"), Some("golem"));
        assert_eq!(tag_family("GOLEM"), Some("golem"));
        assert_eq!(tag_family("schema"), None);
        assert_eq!(tag_family("fibo"), None);
    }

    /// The `dlp:` constants: DOLCE-Lite-Plus is not vendored (no stated
    /// license), so these cannot be verified against a pinned source. Listed
    /// explicitly so the gap stays visible and no other term can join it.
    const DLP_PENDING_SOURCE: &[GolemConcept] = &[
        PARTICIPANT_IN,
        PARTICIPANT,
        GENERIC_LOCATION,
        HAS_SETTING,
        HAS_STATE,
    ];

    /// Fabrication guard: every term in this module is published by the
    /// loaded GOLEM, CIDOC-CRM or LRMoo sources, except the explicit
    /// DOLCE-Lite-Plus pending list.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_TERMS {
            if DLP_PENDING_SOURCE.contains(term) {
                assert!(term.starts_with("dlp:"), "{term}");
                continue;
            }
            assert!(
                crate::published::contains(term),
                "{term} is not published by the loaded GOLEM/CIDOC-CRM/LRMoo sources"
            );
        }
    }
}
