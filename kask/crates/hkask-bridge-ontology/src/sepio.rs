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
//! The full release is loaded from `sources/sepio/` and resolved through
//! `published` (labels and IAO definitions included); this module names only
//! the concepts hKask code emits, and `all_terms_are_official` fails the
//! build unless each is published.
//!
//! Reference: https://github.com/monarch-initiative/SEPIO-ontology
//! (OWL release 2023-06-13). NOTE: the SEPIO project's *current*
//! information model is the linkML specification at
//! https://github.com/sepio-framework/sepio-linkml
//! (https://w3id.org/sepio-model) — a re-conceptualization that does not
//! use the OBO `SEPIO_nnnnnnn` CURIEs (it models evidence via
//! `Statement.hasEvidence`, `EvidenceLine.directionOfEvidenceProvided`,
//! `specifiedBy`, etc.). This module anchors on the OWL release because
//! its OBO CURIEs are the published ontological terms; migrating to the
//! linkML attribute names is an open ontology-selection choice for the
//! operator. Re-verify against the linkML model before adding terms
//! beyond this list.
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
/// so the publication guard covers each constant by construction.
macro_rules! sepio_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: SepioConcept = $uri;)*

        /// Every term in this module, so the publication guard covers each
        /// constant by construction. New terms must go through this macro.
        #[cfg(test)]
        const ALL_TERMS: &[SepioConcept] = &[$($name),*];
    };
}

sepio_terms! {
    /// An assertion — a statement made by an agent that a proposition is
    /// true. The state-axis type for extracted assertion h_mems.
    ASSERTION = "SEPIO:0000001",

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

    /// Fabrication guard: every term in this module is published in the
    /// loaded SEPIO release.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_TERMS {
            assert!(
                crate::published::contains(term),
                "{term} is not published in the loaded SEPIO release"
            );
        }
    }
}
