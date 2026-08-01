//! Triple extraction helper — RDF predicate → 5W1H dimension mapping.
//!
//! Used by `corpus_extract_triples` in `mod.rs`.

use crate::bridge::eso;
use crate::bridge::fibo;
use crate::bridge::golem;

/// Map an RDF predicate to a 5W1H dimension.
///
/// Migrated from the CLI binary's `predicate_to_dimension` function.
/// Used by `corpus_extract_triples` to assign a Dimension to each stored h_mem.
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

/// Hallucination guard for LLM-extracted triples (RR-0018).
///
/// Returns the confidence to store for a triple: the LLM-reported confidence,
/// or 0.5 (capped) when the triple fails verification. Verification:
///
/// - Abstract-namespace predicates (golem/eso/fibo/pko/epistemic/omc/other)
///   bypass the subject/object-in-text check ONLY if the predicate's
///   namespace was actually tagged for this chunk. Without that cross-check,
///   the LLM could emit any `golem:`/`eso:` predicate to bypass the guard
///   for chunks where that ontology was never detected — admitting
///   hallucinated triples at full LLM-reported confidence (the M4 fix).
/// - All other triples: subject and object strings must appear in the chunk
///   text, or confidence is capped at 0.5 (not 0.3 — too aggressive).
pub(crate) fn triple_confidence(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// RR-0018: an abstract-namespace predicate must NOT bypass the guard
    /// when the chunk was never tagged with that namespace — the LLM cannot
    /// launder hallucinated triples through `golem:` prefixes.
    #[test]
    fn abstract_namespace_without_chunk_tag_is_capped() {
        let confidence = triple_confidence(
            "doc:hero",
            "golem:hasCharacter",
            &serde_json::json!("hero"),
            0.95,
            "the chunk text mentions nothing relevant",
            &["pko".to_string()].into_iter().collect(),
        );
        assert_eq!(confidence, 0.5);
    }

    #[test]
    fn abstract_namespace_with_chunk_tag_passes_through() {
        let confidence = triple_confidence(
            "doc:hero",
            "golem:hasCharacter",
            &serde_json::json!("hero"),
            0.95,
            "interpretive chunk",
            &["golem".to_string()].into_iter().collect(),
        );
        assert_eq!(confidence, 0.95);
    }

    #[test]
    fn concrete_triple_missing_from_text_is_capped() {
        let confidence = triple_confidence(
            "doc:zebra",
            "schema:author",
            &serde_json::json!("zebra"),
            0.9,
            "this chunk is about architecture",
            &std::collections::HashSet::new(),
        );
        assert_eq!(confidence, 0.5);
    }

    #[test]
    fn concrete_triple_present_in_text_keeps_confidence() {
        let confidence = triple_confidence(
            "doc:zebra",
            "schema:author",
            &serde_json::json!("zebra"),
            0.9,
            "the zebra appears in this chunk",
            &std::collections::HashSet::new(),
        );
        assert_eq!(confidence, 0.9);
    }
}
