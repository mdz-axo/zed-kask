//! Canonical ontology annotation I/O for downstream corpus consumers.

use hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL;
use hkask_types::corpus::TaggedChunk;

use crate::{McpToolError, read_jsonl_stream};

fn read_current_tagged_chunks(path: &str) -> Result<Vec<TaggedChunk>, McpToolError> {
    let chunks = read_jsonl_stream::<TaggedChunk>(path, "tagged_jsonl")?;
    for chunk in &chunks {
        if !chunk.has_current_canonical_terms() {
            return Err(McpToolError::invalid_argument(format!(
                "Chunk '{}' is not reconciled under ontology protocol '{}'",
                chunk.entity_ref, TERM_RESOLUTION_PROTOCOL
            )));
        }
    }
    Ok(chunks)
}

/// Read resolver-derived ontology context for assertion extraction.
pub(crate) fn read_ontology_tags(
    path: &str,
) -> Result<std::collections::HashMap<String, String>, McpToolError> {
    let chunks = read_current_tagged_chunks(path)?;
    let mut map = std::collections::HashMap::new();
    for chunk in chunks {
        let mut tags = chunk.ontology_tags.into_iter().collect::<Vec<_>>();
        tags.sort_by(|left, right| left.0.cmp(&right.0));
        let parts = tags
            .into_iter()
            .map(|(namespace, concepts)| format!("{namespace}: {}", concepts.join(", ")))
            .collect::<Vec<_>>();
        if !parts.is_empty() {
            map.insert(chunk.entity_ref, parts.join(" | "));
        }
    }
    Ok(map)
}

/// Read ontology tags as bracketed prefixes for ontology-anchored embedding.
pub(crate) fn read_ontology_tags_annotated(
    path: &str,
) -> Result<std::collections::HashMap<String, String>, McpToolError> {
    Ok(read_ontology_tags(path)?
        .into_iter()
        .map(|(entity_ref, tags)| (entity_ref, format!("[{tags}] ")))
        .collect())
}

/// Read resolver-selected namespace keys per classified chunk.
pub(crate) fn read_ontology_namespaces(
    path: &str,
) -> Result<std::collections::HashMap<String, std::collections::HashSet<String>>, McpToolError> {
    let chunks = read_current_tagged_chunks(path)?;
    let mut map = std::collections::HashMap::new();
    for chunk in chunks {
        let namespaces = chunk.ontology_tags.into_keys().collect();
        map.insert(chunk.entity_ref, namespaces);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn downstream_readers_reject_stale_and_inconsistent_tags() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("tagged.jsonl");
        let base = json!({
            "entity_ref": "corpus:test:1",
            "source": "source.txt",
            "text": "A corporation is an organization.",
            "candidate_terms": ["corporation"],
            "ontology_tags": {"fibo": [hkask_bridge_ontology::fibo::CORPORATION]},
            "concepts": [hkask_bridge_ontology::fibo::CORPORATION]
        });

        let mut stale = base.clone();
        stale["classification"] = json!({"status": "classified"});
        std::fs::write(&path, stale.to_string()).expect("stale fixture");
        assert!(read_ontology_tags(&path.to_string_lossy()).is_err());

        let mut inconsistent = base;
        inconsistent["classification"] = json!({
            "status": "classified",
            "ontology_protocol": TERM_RESOLUTION_PROTOCOL
        });
        inconsistent["ontology_tags"] = json!({});
        std::fs::write(&path, inconsistent.to_string()).expect("inconsistent fixture");
        assert!(read_ontology_namespaces(&path.to_string_lossy()).is_err());
    }
}
