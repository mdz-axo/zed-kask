//! Assertion extraction helper — RDF predicate → 5W1H dimension mapping.
//!
//! Used by `corpus_extract_assertions` in `mod.rs`.

use hkask_bridge_ontology::golem;
use hkask_bridge_ontology::sepio;

/// Map an abstract-namespace predicate prefix to the chunk-tag namespace
/// key it must have been tagged with to bypass the subject/object-in-text
/// check. GOLEM-family prefixes (`gc:`, `crm:`, `dlp:`, `lrmoo:` — GOLEM
/// reuses CIDOC-CRM, LRMoo, and DOLCE-Lite-Plus terms) all map to the
/// `"golem"` tag key emitted by `tag-chunks-batch.j2`. Returns `None` for
/// non-abstract namespaces (schema, rdf, dcterms, ...), which never bypass.
pub(crate) fn abstract_namespace_tag_key(pred_ns: &str) -> Option<&'static str> {
    if let Some(family) = golem::tag_family(pred_ns) {
        return Some(family);
    }
    match pred_ns {
        "sepio" => Some("sepio"),
        "pko" => Some("pko"),
        "epistemic" => Some("epistemic"),
        "other" => Some("other"),
        _ => None,
    }
}

/// Map an RDF predicate to a 5W1H dimension.
///
/// Migrated from the CLI binary's `predicate_to_dimension` function.
/// Used by `corpus_extract_assertions` to assign a Dimension to each stored h_mem.
pub(crate) fn predicate_to_dimension(predicate: &str) -> hkask_types::Dimension {
    use hkask_types::Dimension::*;
    let p = predicate.to_lowercase();

    // Ontology-bridge predicates carry mixed-case canonical local names
    // (e.g. `gc:GP1i_has_Character`) — compare case-insensitively against
    // the canonical constants so the mapping cannot drift from the
    // vocabulary module.
    for (canonical, dimension) in [
        (golem::HAS_CHARACTER, Who),
        (golem::HAS_SETTING, Where),
        (golem::GENERIC_LOCATION, Where),
        (golem::REFERS_TO, Why),
        (golem::HAS_FEATURE, What),
    ] {
        if p == canonical.to_lowercase() {
            return dimension;
        }
    }

    // Curated mapping — exact or prefix match on known predicates
    match p.as_str() {
        // Who — agents, authors, characters, creators
        "schema:author" | "schema:creator" | "schema:contributor" | "schema:actor"
        | "rdf:creator" => Who,

        // Who — SEPIO epistemic agents (disputing evidence lines are
        // independent arguments — agents of the debate)
        sepio::HAS_DISPUTING_EVIDENCE_LINE => Who,

        // When — temporal
        "schema:datecreated"
        | "schema:datemodified"
        | "schema:datepublished"
        | "dcterms:created"
        | "dcterms:issued" => When,

        // When — SEPIO temporal epistemic
        sepio::HAS_CONFIDENCE_LEVEL => When,

        // Where — spatial
        "schema:location" | "dcterms:spatial" => Where,

        // Why — causation, motivation, interpretive reference
        "schema:causes" | "schema:resultof" => Why,

        // Why — SEPIO epistemic causation
        sepio::CONTRADICTS
        | sepio::HAS_DISPUTING_EVIDENCE
        | sepio::HAS_SUPPORTING_EVIDENCE
        | sepio::ASSERTS_PROPOSITION => Why,

        // How — methods, processes
        "schema:uses" | "schema:method" => How,

        // How — SEPIO methods and evidence
        sepio::WAS_SPECIFIED_BY | sepio::HAS_EVIDENCE => How,

        // What — default for everything else
        _ => What,
    }
}

/// Hallucination guard for LLM-extracted assertions (RR-0018).
///
/// Returns the confidence to store for an assertion: the LLM-reported confidence,
/// or 0.5 (capped) when the assertion fails verification. Verification:
///
/// - Abstract-namespace predicates (GOLEM family — `gc`/`crm`/`dlp`/`lrmoo`,
///   plus `sepio`/`pko`/`epistemic`/`other`) bypass the
///   subject/object-in-text check ONLY if the predicate's tag family was
///   actually tagged for this chunk. Without that cross-check, the LLM could
///   emit any `gc:`/`SEPIO:` predicate to bypass the guard for chunks where
///   that ontology was never detected — admitting hallucinated assertions
///   at full LLM-reported confidence (the M4 fix).
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
    let tag_key = abstract_namespace_tag_key(&pred_ns);
    let namespace_tagged = match tag_key {
        Some(key) => !chunk_namespaces.is_empty() && chunk_namespaces.contains(key),
        None => false,
    };
    if tag_key.is_some() && namespace_tagged {
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
