//! Ontology tag I/O — reading tagged chunks JSONL for downstream consumers.
//!
//! Three readers with different output shapes:
//! - `read_ontology_tags` → formatted string (for LLM prompt injection)
//! - `read_ontology_tags_annotated` → bracketed prefix (for embedding annotation)
//! - `read_ontology_namespaces` → namespace set (for M4 predicate cross-check)

use crate::*;

/// Read ontology tags from a tagged chunks JSONL file.
///
/// Returns a map of `entity_ref` → formatted ontology context string
/// (e.g. `"golem: metaphor, character development | fibo: ROIC"`).
/// Used by `extract_triples_batch` to inject pre-classified ontology tags
/// into the extraction prompt so the LLM uses the right predicates.
pub(crate) fn read_ontology_tags(
    path: &str,
) -> Result<std::collections::HashMap<String, String>, McpToolError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        McpToolError::invalid_argument(format!("Cannot read tagged_jsonl '{path}': {e}"))
    })?;
    let mut map = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
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
/// (e.g. `{"fibo", "golem"}`). Used by `extract_triples_batch` to cross-check
/// that a triple's predicate namespace was actually tagged for the chunk
/// before bypassing the text-containment hallucination guard (M4 fix).
///
/// Namespace keys are normalized via `normalize_concept` (lowercase + trim +
/// collapse whitespace) so they match the form produced by
/// `validate_ontology_tags` in the tagging phase.
pub(crate) fn read_ontology_namespaces(
    path: &str,
) -> Result<std::collections::HashMap<String, std::collections::HashSet<String>>, McpToolError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        McpToolError::invalid_argument(format!("Cannot read tagged_jsonl '{path}': {e}"))
    })?;
    let mut map: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_ontology_namespaces_extracts_normalized_keys() {
        // M4 fix: namespace keys must be normalized (lowercase + trim) so they
        // match the form produced by validate_ontology_tags in the tagging phase.
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("tagged.jsonl");
        let content = r#"{"entity_ref":"corpus:researcher:doc:1","ontology_tags":{"FIBO":["ROIC"],"golem":["metaphor"]}}
{"entity_ref":"corpus:researcher:doc:2","ontology_tags":{"pko":["analysis"]}}
{"entity_ref":"corpus:researcher:doc:3","dimensions":["what"]}
"#;
        std::fs::write(&path, content).expect("write");

        let map = read_ontology_namespaces(path.to_str().unwrap()).expect("read");

        let ns1 = map
            .get("corpus:researcher:doc:1")
            .expect("chunk 1 must have namespaces");
        assert!(ns1.contains("fibo"), "FIBO must be normalized to fibo");
        assert!(ns1.contains("golem"));
        assert!(!ns1.contains("FIBO"), "original casing must not survive");

        let ns2 = map
            .get("corpus:researcher:doc:2")
            .expect("chunk 2 must have namespaces");
        assert!(ns2.contains("pko"));

        // Chunk 3 has no ontology_tags — must not appear in the map.
        assert!(!map.contains_key("corpus:researcher:doc:3"));
    }

    #[test]
    fn read_ontology_namespaces_empty_file_returns_empty_map() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("empty.jsonl");
        std::fs::write(&path, "").expect("write");
        let map = read_ontology_namespaces(path.to_str().unwrap()).expect("read");
        assert!(map.is_empty());
    }

    #[test]
    fn read_ontology_namespaces_skips_malformed_lines() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("mixed.jsonl");
        let content = "not json at all\n{\"entity_ref\":\"ok\",\"ontology_tags\":{\"fibo\":[\"roic\"]}}\n{\"entity_ref\":\"\",\"ontology_tags\":{}}\n";
        std::fs::write(&path, content).expect("write");
        let map = read_ontology_namespaces(path.to_str().unwrap()).expect("read");
        assert_eq!(
            map.len(),
            1,
            "only the valid line with non-empty namespaces must be kept"
        );
        assert!(map.contains_key("ok"));
    }
}
