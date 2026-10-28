//! RDF 1.1 core vocabulary bridge.
//!
//! The `rdf:` namespace terms used by the corpus assertion pipeline.
//! The complete published RDF and RDFS namespace documents are vendored and
//! checksum-pinned in `sources/rdf-11/`; `all_terms_are_official` checks the
//! constants against that indexed source, not a transcribed list.
//!
//! The pipeline uses exactly one term: `rdf:type` (assertion typing in the
//! extraction prompts). Notably, RDF 1.1 publishes **no creator property** —
//! the former `rdf:creator` literal in the corpus dimension mapping was
//! fabricated; the real term is `dcterms:creator` (`dc_bibo::CREATOR`).
//!
//! Reference: https://www.w3.org/1999/02/22-rdf-syntax-ns (RDF 1.1
//! Concepts vocabulary, fetched 2026-08-30).
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies.
//! Mirrors the sepio and schema_org modules in this crate.

/// An RDF 1.1 concept URI (e.g. `rdf:type`).
pub type RdfConcept = &'static str;

/// The subject is an instance of a class — the typing property.
pub const TYPE: RdfConcept = "rdf:type";

/// Bridge constants checked against the complete source index.
#[cfg(test)]
const ALL_TERMS: &[RdfConcept] = &[TYPE];

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: each bridge constant occurs in the publisher's
    /// pinned RDF namespace document.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_TERMS {
            assert!(
                crate::published::contains(term),
                "{term} is not published by RDF"
            );
        }
        let rdf_count = crate::published::terms()
            .iter()
            .filter(|term| term.namespace == "RDF")
            .count();
        assert!(
            rdf_count > ALL_TERMS.len(),
            "complete RDF source, not fragment: {rdf_count}"
        );
        let kind = crate::term_resolution::resolve_term(TYPE);
        assert_eq!(kind.concept, TYPE);
        assert_eq!(kind.namespace, "RDF");
        assert_eq!(
            kind.definition.as_deref(),
            Some("The subject is an instance of a class.")
        );
        assert!(
            kind.source
                .as_deref()
                .is_some_and(|source| source.contains("rdf.ttl"))
        );
        assert!(crate::published::contains("rdfs:subClassOf"));
    }

    /// RDF publishes no creator property (the real term is `dcterms:creator`).
    #[test]
    fn fabricated_creator_stays_absent() {
        assert!(!crate::published::contains("rdf:creator"));
        assert_ne!(
            crate::term_resolution::resolve_term("rdf:creator").concept,
            "rdf:creator"
        );
    }
}
