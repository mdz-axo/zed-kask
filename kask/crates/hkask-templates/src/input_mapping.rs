//! Input-mapping resolution — bind `input_mapping` values into context.
//!
//! Extracted from the executor (continues the budget.rs / convergence.rs /
//! compute.rs extraction pattern). Three binding forms are supported:
//! `{{ expr }}` strings (rendered via minijinja with `| tojson`), `$ref`
//! objects (context references, with dot-path fallback), and literals.
//!
//! `resolve_mapping_value` is the single resolver — it handles all three
//! forms. The legacy `bind_parameters`/`bind_single_parameter` pair was
//! removed: it only handled `$ref` + literals and passed `{{ }}` strings
//! through verbatim, which silently broke populate steps whose input_mapping
//! used inline Jinja (12 manifests in the registry). `resolve_mapping_value`
//! is a strict superset (same `$ref` + dot-path resolution, plus `{{ }}`
//! rendering), so the last `bind_parameters` call site (execute_populate)
//! switched to it and the dead pair was deleted.

use crate::template_renderer::render_minijinja;
use serde_json::Value;
use std::collections::HashMap;

/// Resolve a dot-path like "step_1_result.field" from the context.
pub(crate) fn resolve_dot_path(path: &str, context: &HashMap<String, Value>) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    let first = context.get(parts[0])?.clone();
    let mut current = first;
    for part in &parts[1..] {
        match current {
            Value::Object(map) => {
                current = map.get(*part)?.clone();
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Resolve an input_mapping value into a concrete JSON value for template binding.
///
/// Handles three forms used in manifests:
/// - `{{ expr }}` string → rendered through minijinja with `| tojson` and parsed back
///   to a JSON value (so `{{ tasks }}` in a template receives the real array/object,
///   not a stringified repr that would double-encode under `| tojson`).
/// - `{"$ref": "dot.path"}` object → the referenced context value (populate-style).
/// - literal (string/number/bool/array) → as-is, recursing into containers.
pub(crate) fn resolve_mapping_value(
    value: &Value,
    context: &HashMap<String, Value>,
    base: &std::path::Path,
) -> Value {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
                let inner = trimmed[2..trimmed.len() - 2].trim();
                let wrapped = format!("{{{{ ({inner}) | tojson }}}}");
                match render_minijinja(&wrapped, context, base) {
                    Ok(json_str) => {
                        serde_json::from_str(json_str.trim()).unwrap_or_else(|_| value.clone())
                    }
                    Err(_) => value.clone(),
                }
            } else if trimmed.contains("{{") {
                render_minijinja(s, context, base)
                    .map(Value::String)
                    .unwrap_or_else(|_| value.clone())
            } else {
                value.clone()
            }
        }
        Value::Object(map) => {
            if let Some(Value::String(ref_path)) = map.get("$ref") {
                if let Some(v) = context.get(ref_path.as_str()) {
                    return v.clone();
                }
                if let Some(v) = resolve_dot_path(ref_path, context) {
                    return v;
                }
            }
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), resolve_mapping_value(v, context, base));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| resolve_mapping_value(v, context, base))
                .collect(),
        ),
        other => other.clone(),
    }
}
