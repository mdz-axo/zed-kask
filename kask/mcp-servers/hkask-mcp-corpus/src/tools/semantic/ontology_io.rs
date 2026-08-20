//! Ontology tag I/O — reading tagged chunks JSONL for downstream consumers.
//!
//! Three readers with different output shapes:
//! - `read_ontology_tags` → formatted string (for LLM prompt injection)
//! - `read_ontology_tags_annotated` → bracketed prefix (for embedding annotation)
//! - `read_ontology_namespaces` → namespace set (for M4 predicate cross-check)

use crate::{McpToolError, normalize_concept, read_jsonl_lenient};

/// Read ontology tags from a tagged chunks JSONL file.
///
/// Returns a map of `entity_ref` → formatted ontology context string
/// (e.g. `"golem: metaphor, character development | fibo: ROIC"`).
/// Used by `extract_passages_batch` to inject pre-classified ontology tags
/// into the extraction prompt so the LLM uses the right predicates.
pub(crate) fn read_ontology_tags(
    path: &str,
) -> Result<std::collections::HashMap<String, String>, McpToolError> {
    let (values, _dropped) = read_jsonl_lenient::<serde_json::Value>(path, "tagged_jsonl")?;
    let mut map = std::collections::HashMap::new();
    for v in values {
        let entity_ref = v.get("entity_ref").and_then(|v| v.as_str()).unwrap_or("");
        if entity_ref.is_empty() {
            continue;
        }
        if let Some(tags) = v.get("ontology_tags").and_then(|t| t.as_object()) {
            let parts: Vec<String> = tags
                .iter()
                .map(|(ns, concepts)| {
                    let list: Vec<String> = concepts
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|c| c.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("{ns}: {}", list.join(", "))
                })
                .collect();
            if !parts.is_empty() {
                map.insert(entity_ref.to_string(), parts.join(" | "));
            }
        }
    }
    Ok(map)
}

/// Read ontology tags and format as bracketed annotation prefixes for embedding.
///
/// Wraps `read_ontology_tags` with `[]` brackets and trailing space.
/// Used by `embed_batch_from_jsonl` to prepend ontology annotations
/// to chunk text before embedding.
pub(crate) fn read_ontology_tags_annotated(
    path: &str,
) -> Result<std::collections::HashMap<String, String>, McpToolError> {
    let map = read_ontology_tags(path)?;
    Ok(map
        .into_iter()
        .map(|(k, v)| (k, format!("[{}] ", v)))
        .collect())
}

/// Read ontology namespace keys per chunk from a tagged chunks JSONL file.
///
/// Returns a map of `entity_ref` → set of normalized namespace keys
/// (e.g. `{"fibo", "golem"}`). Used by `extract_passages_batch` to cross-check
/// that a assertion's predicate namespace was actually tagged for the chunk
/// before bypassing the text-containment hallucination guard (M4 fix).
///
/// Namespace keys are normalized via `normalize_concept` (lowercase + trim +
/// collapse whitespace) so they match the form produced by
/// `validate_ontology_tags` in the tagging phase.
pub(crate) fn read_ontology_namespaces(
    path: &str,
) -> Result<std::collections::HashMap<String, std::collections::HashSet<String>>, McpToolError> {
    let (values, _dropped) = read_jsonl_lenient::<serde_json::Value>(path, "tagged_jsonl")?;
    let mut map: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for v in values {
        let entity_ref = v.get("entity_ref").and_then(|v| v.as_str()).unwrap_or("");
        if entity_ref.is_empty() {
            continue;
        }
        if let Some(tags) = v.get("ontology_tags").and_then(|t| t.as_object()) {
            let namespaces: std::collections::HashSet<String> = tags
                .keys()
                .map(|ns| normalize_concept(ns))
                .filter(|ns| !ns.is_empty())
                .collect();
            if !namespaces.is_empty() {
                map.insert(entity_ref.to_string(), namespaces);
            }
        }
    }
    Ok(map)
}
