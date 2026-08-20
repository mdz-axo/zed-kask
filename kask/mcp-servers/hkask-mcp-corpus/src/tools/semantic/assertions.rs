//! Assertion extraction helper — RDF predicate → 5W1H dimension mapping.
//!
//! Used by `corpus_extract_assertions` in `mod.rs`.

use hkask_bridge_ontology::eso;
use hkask_bridge_ontology::fibo;
use hkask_bridge_ontology::golem;

/// Map an RDF predicate to a 5W1H dimension.
///
/// Migrated from the CLI binary's `predicate_to_dimension` function.
/// Used by `corpus_extract_assertions` to assign a Dimension to each stored h_mem.
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

/// Hallucination guard for LLM-extracted assertions (RR-0018).
///
/// Returns the confidence to store for an assertion: the LLM-reported confidence,
/// or 0.5 (capped) when the assertion fails verification. Verification:
///
/// - Abstract-namespace predicates (golem/eso/fibo/pko/epistemic/omc/other)
///   bypass the subject/object-in-text check ONLY if the predicate's
///   namespace was actually tagged for this chunk. Without that cross-check,
///   the LLM could emit any `golem:`/`eso:` predicate to bypass the guard
///   for chunks where that ontology was never detected — admitting
///   hallucinated assertions at full LLM-reported confidence (the M4 fix).
/// - All other assertions: subject and object strings must appear in the chunk
///   text, or confidence is capped at 0.5 (not 0.3 — too aggressive).
pub(crate) fn assertion_confidence(
    subject: &str,
    predicate: &str,
    object: &serde_json::Value,
    raw_confidence: f64,
    chunk_text: &str,
    chunk_namespaces: &std::collections::HashSet<String>,
) -> f64 {
    let pred_ns = predicate.split(':').next().unwrap_or("").to_lowercase();
    let is_abstract_ns = matches!(
        pred_ns.as_str(),
        "golem" | "eso" | "fibo" | "pko" | "epistemic" | "omc" | "other"
    );
    let namespace_tagged = !chunk_namespaces.is_empty() && chunk_namespaces.contains(&pred_ns);
    if is_abstract_ns && namespace_tagged {
        return raw_confidence;
    }

    let text_lower = chunk_text.to_lowercase();
    let subj_clean = subject
        .strip_prefix("doc:")
        .unwrap_or(subject)
        .to_lowercase();
    let subj_in_text = !subj_clean.is_empty() && text_lower.contains(&subj_clean);
    let obj_str = match object {
        serde_json::Value::String(s) => s.to_lowercase(),
        _ => String::new(),
    };
    let obj_in_text = obj_str.is_empty() || text_lower.contains(&obj_str);
    if (!subj_in_text || !obj_in_text) && raw_confidence > 0.5 {
        0.5
    } else {
        raw_confidence
    }
}
