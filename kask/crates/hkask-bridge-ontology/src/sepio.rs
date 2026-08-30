//! SEPIO (Scientific Evidence and Provenance Information Ontology) bridge.
//!
//! Canonical predicate URIs for epistemic and evidential reasoning —
//! evidence, support, dispute, contradiction, confidence, and method
//! provenance. Used by docproc extract_assertions for expository passages
//! on science, systems thinking, forecasting, complexity, and research
//! methodology.
//!
//! SEPIO is the Monarch Initiative's ontology for evidence and provenance
//! (namespace `http://purl.obolibrary.org/obo/SEPIO_`, OBO prefix `SEPIO`).
//! Every URI in this module is verified against the official release —
//! `fixtures/sepio-2023-06-13-terms.txt` pins the term list, and
//! `all_terms_are_official` fails the build if a term drifts from it. Do
//! not add a term that is not in that fixture.
//!
//! Reference: https://github.com/monarch-initiative/SEPIO-ontology
//! (OWL release 2023-06-13; the README notes the OWL lags the newer linkML
//! information model — re-verify against the linkML models before relying
//! on terms beyond this list).
//!
//! This module replaces the former fabricated "Epistemic Science Ontology"
//! (`eso:`), which never existed as a published vocabulary. Only former ESO
//! functions with a real SEPIO equivalent survived the migration; the rest
//! (hasTheory, hasModel, hasClaim, hasAssumption, hasLimitation, implies,
//! generalizesTo, hasUncertainty, hasHypothesis) were dropped — SEPIO
//! publishes no such properties, and no plausible-looking URI may be
//! invented to cover them.
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors the dc_bibo, pko, and golem modules
//! in this crate.

/// A SEPIO concept URI (OBO CURIE form, e.g. `SEPIO:0000189`).
pub type SepioConcept = &'static str;

/// Defines the vocabulary constants and registers every one in `ALL_TERMS`,
/// so the fixture test covers each constant by construction.
macro_rules! sepio_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: SepioConcept = $uri;)*

        /// Every term in this module. The fixture test asserts each appears
        /// in the official SEPIO term list — a fabricated URI cannot pass.
        /// New terms must go through this macro.
        pub const ALL_TERMS: &[SepioConcept] = &[$($name),*];
    };
}

sepio_terms! {
    /// An agent asserts a proposition (a claim's content).
    ASSERTS_PROPOSITION = "SEPIO:0000030",

    /// An artifact or process was specified by a plan specification
    /// (e.g. an assertion method) — the method provenance link.
    WAS_SPECIFIED_BY = "SEPIO:0000041",

    /// An independent argument against a proposition — a counterargument
    /// or line of disputing evidence.
    HAS_DISPUTING_EVIDENCE_LINE = "SEPIO:0000008",

    /// One proposition contradicts another.
    CONTRADICTS = "SEPIO:0000101",

    /// An assertion or agent has a confidence level.
    HAS_CONFIDENCE_LEVEL = "SEPIO:0000167",

    /// An assertion has evidence supporting it.
    HAS_EVIDENCE = "SEPIO:0000189",

    /// Evidence supports a proposition (corroboration).
    HAS_SUPPORTING_EVIDENCE = "SEPIO:0000440",

    /// Evidence disputes a proposition (falsification pressure).
    HAS_DISPUTING_EVIDENCE = "SEPIO:0000441",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every term in this module must appear in the
    /// official SEPIO term list checked in as a fixture (source URL and
    /// fetch date in the fixture header). A term that is not in the
    /// published ontology fails here — pin tests on the constants alone
    /// cannot catch a plausible-looking invented URI.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/sepio-2023-06-13-terms.txt"
        );
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(|line| line.split('\t').next().unwrap_or("").trim())
            .filter(|term| !term.is_empty() && !term.starts_with('#'))
            .collect();
        assert!(
            !official.is_empty(),
            "fixture {fixture_path} contains no terms"
        );
        for term in ALL_TERMS {
            assert!(
                official.contains(term),
                "{term} is not in the official SEPIO term list ({fixture_path}) — \
                 it must be verified against the SEPIO OWL release before use"
            );
        }
    }
}
