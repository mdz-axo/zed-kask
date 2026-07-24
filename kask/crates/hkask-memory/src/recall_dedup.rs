//! Recall deduplication — entity-attribute-value hash strategy
//!
//! Filters duplicate h_mems at recall time by computing a BLAKE3 hash
//! of the canonical EAV content. This is the single dedup layer in hKask;
//! rendering of recalled memories (prompt strings, JSON payloads, typed
//! response structs) is each consuming surface's responsibility. See
//! ADR-060 for the decision and rationale.

use hkask_storage::HMem;
use std::collections::HashSet;

/// Compute a canonical content hash for a h_mem using the EAV strategy.
///
/// The hash covers entity + attribute + canonical value, intentionally
/// excluding metadata (timestamps, confidence, perspective, visibility)
/// so that the same factual content stored at different times or with
/// different confidence levels is recognized as a duplicate.
///
/// expect: "The system deduplicates h_mems to preserve generative storage budget"
/// \[P3\] Motivating: Generative Space — canonical recall dedup enables reuse of factual content across memory
/// \[P8\] Constraining: Semantic Grounding — deterministic BLAKE3 hash over canonical EAV content
/// pre:  h_mem is a valid HMem with entity, attribute, value
/// post: returns deterministic 32-byte BLAKE3 hash of canonical EAV content
/// post: same EAV content → same hash (metadata-independent)
pub fn eav_hash(h_mem: &HMem) -> [u8; 32] {
    let canonical = format!(
        "{}\x00{}\x00{}",
        h_mem.entity,
        h_mem.attribute,
        canonical_value(&h_mem.value)
    );
    *blake3::hash(canonical.as_bytes()).as_bytes()
}

/// Produce a deterministic string representation of a JSON value for hashing.
fn canonical_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canonical_value).collect();
            format!("[{}]", parts.join(","))
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", k, canonical_value(&map[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// Filter duplicate h_mems from a recall result set.
///
/// Returns only the first occurrence of each unique EAV content.
/// Preserves the original ordering (first-seen wins).
///
/// expect: "The system deduplicates h_mems to preserve generative storage budget"
/// \[P3\] Motivating: Generative Space — deduplication preserves generative storage budget
/// \[P5\] Constraining: Essentialism — first-seen wins, no speculative retention policy
/// pre:  h_mems is a Vec of valid Triples
/// post: returns Vec with duplicates removed (by EAV hash)
/// post: preserves original ordering (first occurrence kept)
/// post: result.len() ≤ h_mems.len()
pub fn dedup_h_mems(h_mems: Vec<HMem>) -> Vec<HMem> {
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(h_mems.len());

    for h_mem in h_mems {
        let hash = eav_hash(&h_mem);
        if seen.insert(hash) {
            result.push(h_mem);
        }
    }

    result
}
