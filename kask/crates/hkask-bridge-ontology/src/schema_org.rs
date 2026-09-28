//! schema.org predicate bridge — the expository-passage vocabulary.
//!
//! Canonical predicate URIs for the corpus assertion pipeline's expository
//! passages (concepts, analysis, arguments). schema.org is the general-purpose
//! vocabulary the extraction prompts offer alongside the domain ontologies
//! (GOLEM for narrative, SEPIO for epistemic, FIBO for financial).
//!
//! The full schema.org release (every layer of the pinned version) is loaded
//! from `sources/schema-org/` and resolved through `published`. This module is
//! the curated predicate menu the extraction prompts offer, not the
//! vocabulary: `all_terms_are_official` fails the build if a menu entry is
//! not a published property of the loaded release.
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
//! Reference: https://schema.org/docs/developers.html (release pinned in
//! `sources/SOURCES.lock`).
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

    /// Fabrication guard: every menu predicate is a published property of the
    /// loaded schema.org release. A plausible-looking invented URI fails here.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_TERMS {
            let published = crate::published::get(term).unwrap_or_else(|| {
                panic!("{term} is not published in the loaded schema.org release")
            });
            assert_eq!(published.kind, "property", "{term} must be a property");
        }
    }

    /// The fabricated predicates this module replaced must stay absent —
    /// if a future schema.org release adds one of them, re-verify before
    /// admitting it to the menu.
    #[test]
    fn fabricated_predicates_stay_absent() {
        for fabricated in [
            "schema:causes",
            "schema:resultOf",
            "schema:uses",
            "schema:method",
            "schema:subject",
        ] {
            assert!(
                !crate::published::contains(fabricated),
                "{fabricated} is now published — it was fabricated at the 2026-08-30 \
                 verification; re-verify against the release before using it"
            );
        }
    }
}
