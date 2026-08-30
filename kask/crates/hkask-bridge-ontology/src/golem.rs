//! GOLEM narrative/literary ontology bridge.
//!
//! Maps hKask narrative concepts to the GOLEM ontology (Golem Ontology for
//! Narrative and Fiction), v1.1. GOLEM is an extension of CIDOC-CRM and LRMoo
//! aligned to DOLCE-Lite-Plus: it defines the `gc:` classes and properties
//! below and otherwise reuses `crm:` (CIDOC-CRM), `lrmoo:` (LRMoo), and
//! `dlp:` (DOLCE-Lite-Plus) terms. Every URI in this module is verified
//! against the official publication — `fixtures/golem-v1.1-terms.txt` pins
//! the term list, and `all_terms_are_official` fails the build if a term
//! drifts from it. Do not add a term that is not in that fixture.
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
/// so the fixture test covers each constant by construction.
macro_rules! golem_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: GolemConcept = $uri;)*

        /// Every term in this module. The fixture test asserts each appears
        /// in the official GOLEM v1.1 term list — a fabricated URI cannot
        /// pass. New terms must go through this macro.
        pub const ALL_TERMS: &[GolemConcept] = &[$($name),*];
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
    SOCIAL_RELATIONSHIP = "gc:G4_Social_Relationships",

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
/// (`tag-chunks-batch.j2` emits `ontology_tags` keyed by `"golem"`).
/// GOLEM's own `gc:` terms and the CIDOC-CRM / LRMoo / DOLCE-Lite-Plus
/// terms it reuses all belong to that one tag family. Returns `None` for
/// prefixes outside the family.
pub fn tag_family(predicate_prefix: &str) -> Option<&'static str> {
    match predicate_prefix.to_lowercase().as_str() {
        "gc" | "crm" | "dlp" | "lrmoo" | "golem" => Some("golem"),
        _ => None,
    }
}

// ── Mapping helpers ────────────────────────────────────────

/// Map a corpus creative operation to its GOLEM concept.
///
/// Takes the bare operation name — the corpus tool name minus its `corpus_`
/// prefix (`corpus_compose` → `compose`). Only creative generation anchors on
/// GOLEM: compose and rewrite produce narrative prose (works). Discovery is
/// deliberately NOT here — it is a search action on the process axis
/// (`corpus_stage_to_pko_step`), not a creative work.
pub fn corpus_op_to_golem(op: &str) -> Option<GolemConcept> {
    match op {
        "compose" | "rewrite" => Some(WORK),
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
        assert_eq!(corpus_op_to_golem("compose"), Some(WORK));
        assert_eq!(corpus_op_to_golem("rewrite"), Some(WORK));
        assert_eq!(corpus_op_to_golem("discover"), None);
        assert_eq!(corpus_op_to_golem("convert"), None);
    }

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

    /// Fabrication guard: every term in this module must appear in the
    /// official GOLEM v1.1 term list checked in as a fixture (source URL
    /// and fetch date in the fixture header). A term that is not in the
    /// published ontology fails here — pin tests on the constants alone
    /// cannot catch a plausible-looking invented URI.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/golem-v1.1-terms.txt");
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        assert!(
            !official.is_empty(),
            "fixture {fixture_path} contains no terms"
        );
        for term in ALL_TERMS {
            assert!(
                official.contains(term),
                "{term} is not in the official GOLEM v1.1 term list ({fixture_path}) — \
                 it must be verified against https://ontology.golemlab.eu/ before use"
            );
        }
    }
}
