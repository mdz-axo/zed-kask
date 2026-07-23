//! Taint-tracking context map for FIDES information flow control.
//!
//! Source: Microsoft Research FIDES (arXiv:2505.23643)
//!
//! Wraps `HashMap<String, Value>` with a parallel taint label map. When a Source
//! tool writes its result, the entry is marked tainted. When `bind_parameters`
//! resolves a `$ref` to a tainted entry, the bound value carries the taint.
//! When a Sink tool's input contains any tainted value, `has_untrusted_input`
//! is true — exactly, not heuristically.
//!
//! Limitation: taint is tracked at the context-entry level, not the field level.
//! If a Source tool returns `{"clean": "safe", "dirty": "untrusted"}`, the entire
//! entry is marked tainted. A Sink tool reading only `clean` would still see
//! `has_untrusted_input = true`. This is over-approximate (safe — no false
//! negatives) but may produce false positives on blocking.

use hkask_types::ToolTaint;
use serde_json::Value;
use std::collections::HashMap;

/// A context map that tracks taint labels alongside JSON values.
///
/// Entries inserted via `insert_tainted` are marked with a `ToolTaint` label.
/// Entries inserted via the standard `insert` are implicitly `Pure`.
/// The `has_untrusted` method checks whether any of the given value's
/// `$ref` references resolve to tainted entries.
#[derive(Debug, Clone, Default)]
pub struct TaintContext {
    values: HashMap<String, Value>,
    taint_labels: HashMap<String, ToolTaint>,
}

impl TaintContext {
    /// Create an empty taint context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a value with the default `Pure` taint label.
    pub fn insert(&mut self, key: String, value: Value) {
        self.values.insert(key, value);
        // Don't overwrite an existing taint label if the key already exists.
        // taint_labels.entry(key).or_insert(ToolTaint::Pure);
    }

    /// Insert a value with an explicit taint label.
    /// Source tool results should use `ToolTaint::Source`.
    pub fn insert_tainted(&mut self, key: String, value: Value, taint: ToolTaint) {
        self.values.insert(key.clone(), value);
        self.taint_labels.insert(key, taint);
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Get the taint label for a key. Returns `Pure` if not explicitly labeled.
    pub fn taint_of(&self, key: &str) -> ToolTaint {
        self.taint_labels
            .get(key)
            .copied()
            .unwrap_or(ToolTaint::Pure)
    }

    /// Check whether a value bound from the context carries untrusted data.
    ///
    /// This recursively checks the value for `{"$ref": "step_N_result"}` patterns
    /// and determines whether any referenced entry is tainted (Source or Endorser).
    /// A Sink tool whose input contains any tainted reference should be blocked
    /// by the runtime policy (FIDES Source→Sink rule).
    pub fn has_untrusted_input(&self, value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                // Check for $ref pattern: {"$ref": "step_1_result.field"}
                if let Some(Value::String(ref_path)) = map.get("$ref") {
                    // Extract the context key (first segment before any dot).
                    let context_key = ref_path.split('.').next().unwrap_or("");
                    return self.taint_of(context_key) == ToolTaint::Source;
                }
                // Recurse into object fields.
                map.values().any(|v| self.has_untrusted_input(v))
            }
            Value::Array(arr) => arr.iter().any(|v| self.has_untrusted_input(v)),
            _ => false,
        }
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Check if the context is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Iterate over (key, value) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.values.iter()
    }

    /// Convert to a plain `HashMap<String, Value>` for compatibility with
    /// functions that don't need taint tracking (e.g., minijinja rendering).
    pub fn into_inner(self) -> HashMap<String, Value> {
        self.values
    }

    /// Get a reference to the inner values map.
    pub fn values(&self) -> &HashMap<String, Value> {
        &self.values
    }

    /// Get a mutable reference to the inner values map.
    pub fn values_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.values
    }
}

impl From<HashMap<String, Value>> for TaintContext {
    fn from(values: HashMap<String, Value>) -> Self {
        Self {
            values,
            taint_labels: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_by_default() {
        let ctx = TaintContext::new();
        assert_eq!(ctx.taint_of("anything"), ToolTaint::Pure);
    }

    #[test]
    fn source_taint_tracked() {
        let mut ctx = TaintContext::new();
        ctx.insert_tainted(
            "step_1_result".to_string(),
            Value::String("untrusted data".to_string()),
            ToolTaint::Source,
        );
        assert_eq!(ctx.taint_of("step_1_result"), ToolTaint::Source);
    }

    #[test]
    fn has_untrusted_detects_source_ref() {
        let mut ctx = TaintContext::new();
        ctx.insert_tainted(
            "step_1_result".to_string(),
            Value::String("untrusted".to_string()),
            ToolTaint::Source,
        );
        // Input references a tainted entry.
        let input = serde_json::json!({"data": {"$ref": "step_1_result.field"}});
        assert!(ctx.has_untrusted_input(&input));
    }

    #[test]
    fn no_untrusted_for_pure_ref() {
        let mut ctx = TaintContext::new();
        ctx.insert(
            "step_1_result".to_string(),
            Value::String("trusted".to_string()),
        );
        let input = serde_json::json!({"data": {"$ref": "step_1_result.field"}});
        assert!(!ctx.has_untrusted_input(&input));
    }

    #[test]
    fn has_untrusted_recursive() {
        let mut ctx = TaintContext::new();
        ctx.insert_tainted(
            "step_2_result".to_string(),
            Value::String("untrusted".to_string()),
            ToolTaint::Source,
        );
        // Nested reference in an array.
        let input = serde_json::json!({
            "items": [
                {"safe": "yes"},
                {"$ref": "step_2_result.data"}
            ]
        });
        assert!(ctx.has_untrusted_input(&input));
    }

    #[test]
    fn endorser_not_untrusted() {
        let mut ctx = TaintContext::new();
        ctx.insert_tainted(
            "step_1_result".to_string(),
            Value::String("endorsed".to_string()),
            ToolTaint::Endorser,
        );
        let input = serde_json::json!({"data": {"$ref": "step_1_result.field"}});
        // Endorser output is trusted — it was extracted via quarantined LLM.
        assert!(!ctx.has_untrusted_input(&input));
    }

    #[test]
    fn from_hashmap_preserves_values() {
        let mut map = HashMap::new();
        map.insert("key".to_string(), Value::String("value".to_string()));
        let ctx: TaintContext = map.into();
        assert_eq!(ctx.get("key").unwrap(), &Value::String("value".to_string()));
        assert_eq!(ctx.taint_of("key"), ToolTaint::Pure);
    }
}
