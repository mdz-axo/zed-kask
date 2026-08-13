//! Caller-supplied context validation and discovery against a manifest's
//! declared `inputs`.
//!
//! Two capabilities, both operating on the manifest's `inputs:` block (an
//! `Option<serde_json::Value>` array of `{name, type, required, default,
//! description}` objects):
//!
//! - [`validate_inputs`] (Layer A): enforce `required` and JSON `type` at the
//!   skill-execution boundary, turning silent wrong-params (missing required
//!   inputs, wrong-typed values) into a structured error. Opt-in via
//!   `BundleManifest::enforce_inputs == Some(true)` so existing skills are
//!   unaffected. Unknown keys are warned, not rejected (manifests may declare
//!   inputs sparsely).
//! - [`render_input_param_spec`] (Layer B): render a concise typed parameter
//!   spec from the declared `inputs`, for inclusion in a skill's catalog
//!   description so the model discovers which `context` keys to pass (the
//!   interactive `skill` tool surfaces only the description for manifested
//!   skills).
//!
//! Both are pure functions over the manifest's raw `inputs` JSON — no I/O, no
//! side effects beyond `tracing::warn!` for unknown keys — so they are unit-
//! testable in this crate. The execution wiring lives in the skill executor /
//! bridge (see `BridgeManifestExecutor::execute_skill`).
//!
//! Why this exists: the manifest's `inputs` is an advertised contract, but
//! historically nothing enforced it on the interactive path — the executor
//! never applied `default` and never checked `required`/`type`, so a typo'd or
//! missing param silently produced a misleading result (an advertised
//! invariant with no enforcement point). These functions provide the
//! enforcement point.

use serde_json::Value;
use std::collections::HashMap;

/// Validate a caller-supplied `context` against the manifest's declared
/// `inputs`. Returns `Ok(())` if the skill did not opt in (`enforce_inputs` is
/// not `Some(true)`) or declared no inputs; otherwise returns
/// `Err(human_readable)` listing every violation (missing required, type
/// mismatch).
///
/// `enforce_inputs` is the manifest's opt-in flag (`BundleManifest::enforce_inputs`);
/// `inputs` is the manifest's declared inputs block (`manifest.inputs.as_ref()`).
/// `system_keys` are runtime-injected keys (e.g. `"task"`, `"embedding_model"`)
/// that are excluded from the unknown-key warning — they are not user params.
///
/// Unknown user keys (present in `context`, not declared in `inputs`, not a
/// system key) are logged via `tracing::warn!` but do NOT fail validation —
/// manifests may declare inputs sparsely, and rejecting unknown keys would
/// break skills that read keys they forgot to declare.
/// Error from manifest input validation — a semicolon-joined list of
/// missing-required and wrong-type messages.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct InputValidationError(pub String);

/// Validate the manifest's declared `inputs` against the runtime `context`.
pub fn validate_inputs(
    enforce_inputs: Option<bool>,
    inputs: Option<&Value>,
    context: &HashMap<String, Value>,
    system_keys: &[&str],
) -> Result<(), InputValidationError> {
    // Opt-in gate: skills must explicitly enable enforcement so existing skills
    // (whose required inputs may be supplied programmatically, not via the
    // interactive `context` map) are not broken.
    if enforce_inputs != Some(true) {
        return Ok(());
    }
    let Some(Value::Array(declared)) = inputs else {
        // No declared inputs → nothing to validate.
        return Ok(());
    };

    // Parse declared inputs into (name, type, required).
    let mut specs: Vec<(&str, &str, bool)> = Vec::new();
    for entry in declared {
        let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
            tracing::warn!(
                target: "hkask.templates.inputs",
                "manifest `inputs` entry has no string `name`; skipped: {entry}"
            );
            continue;
        };
        let typ = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let required = entry
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        specs.push((name, typ, required));
    }

    let mut errors: Vec<String> = Vec::new();
    let declared_names: Vec<&str> = specs.iter().map(|(n, _, _)| *n).collect();

    for (name, typ, required) in &specs {
        match context.get(*name) {
            None => {
                if *required {
                    errors.push(format!(
                        "missing required input `{name}` (type: {typ}); pass it via the skill tool's `context` map"
                    ));
                }
            }
            Some(v) => {
                if !typ.is_empty() && !json_type_matches(v, typ) {
                    errors.push(format!(
                        "input `{name}` expects type `{typ}` but got `{}`",
                        json_type_name(v)
                    ));
                }
            }
        }
    }

    // Unknown user keys: warn only (manifests may declare inputs sparsely).
    for key in context.keys() {
        if !declared_names.contains(&key.as_str()) && !system_keys.contains(&key.as_str()) {
            tracing::warn!(
                target: "hkask.templates.inputs",
                key = %key,
                "unknown `context` key not declared in manifest `inputs` (possible typo); \
                 not rejected because manifests may declare inputs sparsely"
            );
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(InputValidationError(errors.join("; ")))
    }
}

/// Render a concise typed parameter spec from the manifest's declared `inputs`,
/// suitable for inclusion in a skill's catalog description (so the model
/// discovers which `context` keys to pass). Returns an empty string if there
/// are no declared inputs.
///
/// Example output:
/// `change_spec (string, required); diff_base (string, required); focus (array); fix_mode (string); delegate_security (bool)`
pub fn render_input_param_spec(manifest_inputs: Option<&Value>) -> String {
    let Some(Value::Array(declared)) = manifest_inputs else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    for entry in declared {
        let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let typ = entry.get("type").and_then(|v| v.as_str()).unwrap_or("any");
        let typ_short = match typ {
            "boolean" => "bool",
            "integer" | "number" => "number",
            other => other,
        };
        let required = entry
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if required {
            parts.push(format!("{name} ({typ_short}, required)"));
        } else {
            parts.push(format!("{name} ({typ_short})"));
        }
    }
    parts.join("; ")
}

/// Extract the set of input keys declared in a `.j2` template's `contract.input`
/// frontmatter block.
///
/// hKask templates use a YAML frontmatter (between the file start and the first
/// `---` separator) that may declare a `contract:` block with an `input:`
/// sub-mapping of `name: type` pairs. This parses that block and returns the
/// declared input key names, or an empty set if the template has no
/// `contract.input` block.
///
/// This is the contract side of the input_mapping ↔ contract.input cross-check
/// (see `manifest_compliance::input_mapping_matches_template_contract`): the
/// manifest's `input_mapping` provides keys to the template, and the
/// template's `contract.input` declares which keys it consumes. Mismatches are
/// either typos in the mapping (mapping has a key the template doesn't declare)
/// or stale contracts (template declares a key the mapping doesn't provide —
/// often intentional for agent-coordinated context, so this is informational).
///
/// Mirrors `output_schema::extract_contract_output` but for the `input` block.
/// Kept here (not in `output_schema`) because it is about *inputs*, and
/// `output_schema` is `pub(crate)`.
pub fn extract_contract_input_keys(template_content: &str) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    // Find the frontmatter: everything before the first `\n---\n` separator.
    let Some(separator_pos) = template_content.find("\n---\n") else {
        return keys;
    };
    let frontmatter = &template_content[..separator_pos];
    // Strip Jinja comments ({# ... #}) — they are not valid YAML.
    let stripped = strip_jinja_comments_inputs(frontmatter);
    let frontmatter = stripped.trim();
    let frontmatter = frontmatter
        .strip_prefix("[inference]")
        .unwrap_or(frontmatter)
        .trim();
    let Ok(parsed) = serde_yaml_neo::from_str::<Value>(frontmatter) else {
        return keys;
    };
    let Some(contract) = parsed.get("contract") else {
        return keys;
    };
    let Some(input) = contract.get("input") else {
        return keys;
    };
    if let Some(obj) = input.as_object() {
        for k in obj.keys() {
            keys.insert(k.clone());
        }
    }
    keys
}

/// Strip Jinja comments (`{# ... #}`) from a string. Mirrors
/// `output_schema::strip_jinja_comments` (which is `pub(crate)`).
fn strip_jinja_comments_inputs(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'#') {
            chars.next(); // consume '#'
            let mut found_close = false;
            while let Some(c) = chars.next() {
                if c == '#' && chars.peek() == Some(&'}') {
                    chars.next(); // consume '}'
                    found_close = true;
                    break;
                }
            }
            if !found_close {
                result.push('{');
                result.push('#');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Map a declared type string to whether a JSON value matches it. Unknown
/// declared types are treated as "anything matches" (don't fail on a type the
/// validator doesn't recognize — the manifest may use a custom type tag).
fn json_type_matches(v: &Value, type_str: &str) -> bool {
    match type_str {
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "integer" => v.as_i64().is_some(),
        "number" => v.is_number(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        // Unrecognized declared type → accept (don't fail on unknown type tags).
        _ => true,
    }
}

/// Human-readable JSON type name for error messages.
fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest_loader::load_manifest_from_yaml;

    fn ctx(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn validate_no_op_when_not_opted_in() {
        let inputs = Some(serde_json::json!([
            { "name": "a", "type": "string", "required": true }
        ]));
        // Missing required input `a`, but not opted in → no error.
        assert!(validate_inputs(None, inputs.as_ref(), &HashMap::new(), &["task"]).is_ok());
    }

    #[test]
    fn validate_missing_required_errors_when_opted_in() {
        let inputs = Some(serde_json::json!([
            { "name": "change_spec", "type": "string", "required": true },
            { "name": "diff_base", "type": "string", "required": true }
        ]));
        let err =
            validate_inputs(Some(true), inputs.as_ref(), &HashMap::new(), &["task"]).unwrap_err();
        assert!(err.0.contains("missing required input `change_spec`"));
        assert!(err.0.contains("missing required input `diff_base`"));
    }

    #[test]
    fn validate_type_mismatch_errors() {
        let inputs = Some(serde_json::json!([
            { "name": "fix_mode", "type": "string", "required": false },
            { "name": "delegate_security", "type": "boolean", "required": false }
        ]));
        // fix_mode passed as bool (should be string), delegate_security as string (should be bool).
        let c = ctx(&[
            ("fix_mode", Value::Bool(true)),
            ("delegate_security", Value::String("true".into())),
        ]);
        let err = validate_inputs(Some(true), inputs.as_ref(), &c, &["task"]).unwrap_err();
        assert!(
            err.0
                .contains("`fix_mode` expects type `string` but got `boolean`")
        );
        assert!(
            err.0
                .contains("`delegate_security` expects type `boolean` but got `string`")
        );
    }

    #[test]
    fn validate_correct_types_pass() {
        let inputs = Some(serde_json::json!([
            { "name": "change_spec", "type": "string", "required": true },
            { "name": "focus", "type": "array", "required": false },
            { "name": "delegate_security", "type": "boolean", "required": false },
            { "name": "fix_mode", "type": "string", "required": false }
        ]));
        let c = ctx(&[
            ("change_spec", Value::String("do X".into())),
            ("focus", Value::Array(vec![])),
            ("delegate_security", Value::Bool(true)),
            ("fix_mode", Value::String("blockers".into())),
        ]);
        assert!(validate_inputs(Some(true), inputs.as_ref(), &c, &["task"]).is_ok());
    }

    #[test]
    fn validate_system_keys_excluded_from_unknown_check() {
        let inputs = Some(serde_json::json!([
            { "name": "change_spec", "type": "string", "required": true }
        ]));
        // `task` is a system key; not declared, must not error or warn-fail.
        let c = ctx(&[
            ("change_spec", Value::String("do X".into())),
            ("task", Value::String("review this".into())),
        ]);
        assert!(validate_inputs(Some(true), inputs.as_ref(), &c, &["task"]).is_ok());
    }

    #[test]
    fn validate_unknown_user_key_does_not_fail() {
        // Unknown user key (typo) is warned, not rejected — must not error.
        let inputs = Some(serde_json::json!([
            { "name": "change_spec", "type": "string", "required": true }
        ]));
        let c = ctx(&[
            ("change_spec", Value::String("do X".into())),
            ("fxi_mode", Value::String("blockers".into())), // typo of fix_mode
        ]);
        assert!(validate_inputs(Some(true), inputs.as_ref(), &c, &["task"]).is_ok());
    }

    #[test]
    fn validate_no_declared_inputs_is_ok() {
        let c = ctx(&[("anything", Value::String("x".into()))]);
        assert!(validate_inputs(Some(true), None, &c, &["task"]).is_ok());
    }

    #[test]
    fn render_param_spec_lists_typed_params() {
        let inputs = Some(serde_json::json!([
            { "name": "change_spec", "type": "string", "required": true },
            { "name": "focus", "type": "array", "required": false },
            { "name": "delegate_security", "type": "boolean", "required": false },
            { "name": "fix_mode", "type": "string", "required": false }
        ]));
        let spec = render_input_param_spec(inputs.as_ref());
        assert!(spec.contains("change_spec (string, required)"));
        assert!(spec.contains("focus (array)"));
        assert!(spec.contains("delegate_security (bool)"));
        assert!(spec.contains("fix_mode (string)"));
    }

    #[test]
    fn render_param_spec_empty_when_no_inputs() {
        assert_eq!(render_input_param_spec(None), "");
        assert_eq!(render_input_param_spec(Some(&Value::Array(vec![]))), "");
    }

    /// End-to-end: load_manifest_from_yaml accepts `enforce_inputs` (the
    /// deny_unknown_fields gate on ManifestHeader) and wires it through to
    /// validate_inputs.
    #[test]
    fn load_manifest_parses_enforce_inputs_and_validates() {
        let yaml = r#"
manifest:
  id: test-skill
  enforce_inputs: true
inputs:
  - name: change_spec
    type: string
    required: true
  - name: fix_mode
    type: string
    required: false
"#;
        let manifest = load_manifest_from_yaml(yaml).expect("manifest parses");
        assert_eq!(manifest.enforce_inputs, Some(true));

        // Missing required `change_spec` → error when opted in.
        let err = validate_inputs(
            manifest.enforce_inputs,
            manifest.inputs.as_ref(),
            &HashMap::new(),
            &["task"],
        )
        .unwrap_err();
        assert!(err.0.contains("missing required input `change_spec`"));

        // Well-formed context → ok.
        let c = ctx(&[
            ("change_spec", Value::String("do X".into())),
            ("fix_mode", Value::String("blockers".into())),
        ]);
        assert!(
            validate_inputs(
                manifest.enforce_inputs,
                manifest.inputs.as_ref(),
                &c,
                &["task"]
            )
            .is_ok()
        );
    }
}
