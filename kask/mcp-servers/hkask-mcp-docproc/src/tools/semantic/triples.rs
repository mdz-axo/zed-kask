//! Triple extraction helper — RDF predicate → 5W1H dimension mapping.
//!
//! Used by `docproc_extract_triples` in `mod.rs`.

use crate::bridge::eso;
use crate::bridge::fibo;
use crate::bridge::golem;

/// Map an RDF predicate to a 5W1H dimension.
///
/// Migrated from the CLI binary's `predicate_to_dimension` function.
/// Used by `docproc_extract_triples` to assign a Dimension to each stored h_mem.
pub(crate) fn predicate_to_dimension(predicate: &str) -> hkask_types::Dimension {
    use hkask_types::Dimension::*;
    let p = predicate.to_lowercase();

    // Curated mapping — exact or prefix match on known predicates
    match p.as_str() {
        // Who — agents, authors, characters, creators
        "schema:author"
        | "schema:creator"
        | "schema:contributor"
        | "schema:actor"
        | golem::HAS_CHARACTER
        | golem::HAS_NARRATOR
        | "rdf:creator" => Who,

        // Who — ESO epistemic agents
        eso::HAS_COUNTERARGUMENT => Who,

        // When — temporal
        "schema:datecreated"
        | "schema:datemodified"
        | "schema:datepublished"
        | "dcterms:created"
        | "dcterms:issued" => When,

        // When — ESO temporal epistemic
        eso::HAS_CONFIDENCE => When,

        // Where — spatial
        "schema:location" | golem::HAS_SETTING | "dcterms:spatial" => Where,

        // Why — causation, motivation, theme
        "schema:causes"
        | "schema:resultof"
        | golem::HAS_CONFLICT
        | golem::ALLEGORY_OF
        | fibo::HAS_RISK => Why,

        // Why — ESO epistemic causation
        eso::IMPLIES
        | eso::CONTRADICTS
        | eso::FALSIFIED_BY
        | eso::CORROBORATED_BY
        | eso::GENERALIZES_TO => Why,

        // How — methods, processes, resolution
        "schema:uses"
        | "schema:method"
        | golem::HAS_RESOLUTION
        | golem::METAPHOR_FOR
        | golem::ILLUSTRATES
        | golem::EVOKES => How,

        // How — ESO methods and evidence
        eso::USES_METHOD | eso::HAS_EVIDENCE | eso::HAS_LIMITATION => How,

        // What — default for everything else
        _ => What,
    }
}
