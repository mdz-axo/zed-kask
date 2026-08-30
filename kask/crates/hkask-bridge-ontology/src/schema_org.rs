//! schema.org predicate bridge — the expository-passage vocabulary.
//!
//! Canonical predicate URIs for the corpus assertion pipeline's expository
//! passages (concepts, analysis, arguments). schema.org is the general-purpose
//! vocabulary the extraction prompts offer alongside the domain ontologies
//! (GOLEM for narrative, SEPIO for epistemic, FIBO for financial).
//!
//! Every URI in this module is verified against the official machine-readable
//! release — `fixtures/schema-org-terms.txt` pins the term list (source URL
//! and fetch date in the fixture header), and `all_terms_are_official` fails
//! the build if a term drifts from it. Do not add a term that is not in that
//! fixture.
//!
//! The fabricated predicates this module replaces were emitted by the corpus
//! pipeline for years before verification: `schema:causes`, `schema:resultOf`,
//! `schema:uses`, `schema:method`, and `schema:subject` exist nowhere in the
//! published vocabulary. schema.org publishes no general causation or
//! method predicate — those functional roles are carried by the SEPIO
//! constants (`CONTRADICTS`, `HAS_SUPPORTING_EVIDENCE`, `WAS_SPECIFIED_BY`,
//! `HAS_EVIDENCE`) or fall to the dimension mapping's default `What` arm.
//! `schema:subject`'s real counterpart is `SUBJECT_OF` (the inverse of
//! `about`).
//!
//! Reference: https://schema.org/docs/developers.html (release v30.0,
//! verified 2026-08-30 against schemaorg-all-https.jsonld).
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies.
//! Mirrors the sepio and dc_bibo modules in this crate.

/// A schema.org concept URI (e.g. `schema:author`).
pub type SchemaConcept = &'static str;

/// Defines the vocabulary constants and registers every one in `ALL_TERMS`,
/// so the fixture test covers each constant by construction.
macro_rules! schema_org_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: SchemaConcept = $uri;)*

        /// Every term in this module. The fixture test asserts each appears
        /// in the official schema.org term list — a fabricated URI cannot
        /// pass. New terms must go through this macro.
        pub const ALL_TERMS: &[SchemaConcept] = &[$($name),*];
    };
}

schema_org_terms! {
    /// Who — the author of a creative work.
    AUTHOR = "schema:author",

    /// Who — the creator of a creative work.
    CREATOR = "schema:creator",

    /// Who — a contributor to a creative work.
    CONTRIBUTOR = "schema:contributor",

    /// Who — a performer in a work (cast member).
    ACTOR = "schema:actor",

    /// What — the name of a thing.
    NAME = "schema:name",

    /// What — the work contains a reference to (but is not necessarily
    /// about) a concept.
    MENTIONS = "schema:mentions",

    /// What — the most generic relation between two things (schema.org
    /// scopes this to familial relations between persons; the extraction
    /// pipeline treats it as the generic relatedness arm).
    RELATED_TO = "schema:relatedTo",

    /// What — the subject matter of a work.
    ABOUT = "schema:about",

    /// What — a work has this work as a part.
    HAS_PART = "schema:hasPart",

    /// What — this work is a part of another work.
    IS_PART_OF = "schema:isPartOf",

    /// When — the creation date of a work.
    DATE_CREATED = "schema:dateCreated",

    /// When — the last modification date of a work.
    DATE_MODIFIED = "schema:dateModified",

    /// When — the publication date of a work.
    DATE_PUBLISHED = "schema:datePublished",

    /// Where — the location of an event or place the work relates to.
    LOCATION = "schema:location",

    /// What — a CreativeWork or Event about this Thing (the inverse of
    /// `about`). The real counterpart of the fabricated `schema:subject`.
    SUBJECT_OF = "schema:subjectOf",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every term in this module must appear in the
    /// official schema.org term list checked in as a fixture (source URL
    /// and fetch date in the fixture header). A term that is not in the
    /// published vocabulary fails here — pin tests on the constants alone
    /// cannot catch a plausible-looking invented URI.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/schema-org-terms.txt");
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
                "{term} is not in the official schema.org term list ({fixture_path}) — \
                 it must be verified against the schema.org release before use"
            );
        }
    }

    /// The fabricated predicates this module replaced must stay absent —
    /// if a future schema.org release adds one of them, re-verify and move
    /// it into the fixture deliberately.
    #[test]
    fn fabricated_predicates_stay_absent() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/schema-org-terms.txt");
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        for fabricated in [
            "schema:causes",
            "schema:resultOf",
            "schema:uses",
            "schema:method",
            "schema:subject",
        ] {
            assert!(
                !official.contains(fabricated),
                "{fabricated} was added to the fixture — it was fabricated at the \
                 2026-08-30 verification; re-verify against the current schema.org \
                 release before admitting it"
            );
        }
    }
}
