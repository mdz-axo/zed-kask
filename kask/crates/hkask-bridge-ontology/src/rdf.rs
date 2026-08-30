//! RDF 1.1 core vocabulary bridge.
//!
//! The `rdf:` namespace terms used by the corpus assertion pipeline.
//! RDF 1.1 publishes a small closed vocabulary (22 terms — properties,
//! classes, datatypes, and the `nil` instance); the complete official list
//! is pinned in `fixtures/rdf-11-terms.txt`, and `all_terms_are_official`
//! fails the build if a term drifts from it.
//!
//! The pipeline uses exactly one term: `rdf:type` (assertion typing in the
//! extraction prompts). Notably, RDF 1.1 publishes **no creator property** —
//! the former `rdf:creator` literal in the corpus dimension mapping was
//! fabricated; the real term is `dcterms:creator`
//! (`dc_bibo::CREATOR`, fixture-verified).
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

/// Every term in this module. The fixture test asserts each appears in the
/// official RDF 1.1 term list — a fabricated URI cannot pass. New terms
/// must be added alongside a fixture entry.
pub const ALL_TERMS: &[RdfConcept] = &[TYPE];

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every term in this module must appear in the
    /// official RDF 1.1 vocabulary checked in as a fixture (source URL and
    /// fetch date in the fixture header).
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/rdf-11-terms.txt");
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
                "{term} is not in the official RDF 1.1 vocabulary ({fixture_path})"
            );
        }
    }

    /// The fabricated `rdf:creator` must stay absent from the official list —
    /// RDF 1.1 publishes no creator property (the real term is
    /// `dcterms:creator`).
    #[test]
    fn fabricated_creator_stays_absent() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/rdf-11-terms.txt");
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        assert!(
            !official.contains("rdf:creator"),
            "rdf:creator was added to the fixture — RDF 1.1 publishes no creator \
             property; the real term is dcterms:creator"
        );
    }
}
