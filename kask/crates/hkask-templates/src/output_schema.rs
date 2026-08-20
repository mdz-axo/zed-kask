//! Structured-output schema pipeline for `select` steps.
//!
//! Extracted from the executor (continues the budget.rs / convergence.rs /
//! compute.rs / input_mapping.rs / condition.rs extraction pattern). Converts
//! a template's `contract.output` frontmatter (or a manifest-declared
//! `output_schema`) into a JSON Schema and a synthetic `ChatToolDefinition`
//! so the model is forced to emit JSON conforming to the contract instead of
//! free-text prose (the LangGraph/Swarm enforce-at-the-API-layer pattern).

use hkask_types::{ChatToolDefinition, ChatToolFunction};
use serde_json::Value;

/// Resolve the output schema for a `select` step.
///
/// Priority:
/// 1. `output_schema` (manifest-declared, if present)
/// 2. `contract.output` from the template frontmatter (parsed at runtime)
///
/// Returns a JSON Schema suitable for tool-calling, or `None` if no schema
/// is available (in which case the executor falls back to text parsing).
pub(crate) fn resolve_output_schema(
    output_schema: Option<&Value>,
    template_content: &str,
) -> Option<Value> {
    // Priority 1: manifest-declared output_schema.
    if let Some(schema) = output_schema
        && schema.is_object()
    {
        return Some(schema.clone());
    }

    // Priority 2: contract.output from the template frontmatter.
    let contract_output = extract_contract_output(template_content)?;
    Some(contract_output_to_schema(&contract_output))
}

/// Extract the `contract.output` block from a `.j2` template's frontmatter.
///
/// The frontmatter is YAML between the start of the file and the `---`
/// separator. The `contract.output` block declares field names and their
/// types as a simple `name: type` mapping (e.g. `convergence_metric: number`).
/// This function parses that block and returns it as a `serde_json::Value`
/// map (field name → type string), or `None` if no contract is found.
///
/// This is the schema source for structured-output tool calling — the
/// executor converts this into a JSON Schema and passes it as a synthetic
/// tool so the model is forced to emit JSON conforming to the contract,
/// instead of emitting prose and hoping `parse_json_response` can extract
/// JSON from it.
fn extract_contract_output(template_content: &str) -> Option<Value> {
    // hKask templates use a frontmatter format where:
    // - The frontmatter starts at the beginning of the file (optionally after
    //   leading Jinja comments `{# ... #}` and a `[inference]` marker line)
    //   and ends at the first `---` separator.
    // - The frontmatter is YAML containing `template_type`,
    //   `contract`, `visibility`, etc.
    // - The body after `---` is the Jinja2 template.
    //
    // We find the `\n---\n` separator and parse everything before it as YAML.
    // Leading Jinja comments (`{# ... #}`) are stripped — they're not valid YAML
    // and would cause the parser to fail. The `[inference]` marker is also
    // stripped for the same reason.
    let separator_pos = template_content.find("\n---\n")?;
    let frontmatter = &template_content[..separator_pos];

    // Strip Jinja comments ({# ... #}) — they can appear anywhere in the
    // frontmatter and are not valid YAML.
    let stripped = strip_jinja_comments(frontmatter);
    let frontmatter = stripped.trim();
    let frontmatter = frontmatter
        .strip_prefix("[inference]")
        .unwrap_or(frontmatter)
        .trim();

    let parsed: Value = serde_yaml_neo::from_str(frontmatter).ok()?;
    let contract = parsed.get("contract")?;
    let output = contract.get("output")?;
    Some(output.clone())
}

/// Strip Jinja comments (`{# ... #}`) from a string. Comments can span
/// multiple lines. Uses a simple state machine rather than regex to avoid
/// the regex dependency.
fn strip_jinja_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'#') {
            // Skip until we find #}
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
                // Unterminated comment — append the rest as-is
                result.push('{');
                result.push('#');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert a `contract.output` block (field name → type string) into a
/// JSON Schema suitable for tool-calling.
///
/// The contract output is a simple mapping like:
/// ```yaml
/// output:
///   convergence_metric: number
///   rationale: string
///   blockers: array
/// ```
///
/// This converts to a JSON Schema object with `type: object`, `properties`
/// mapping each field to its JSON type, and no `required` fields (the model
/// can omit optional fields). The type mapping is:
/// - `string` → `{"type": "string"}`
/// - `number` / `float` / `integer` → `{"type": "number"}`
/// - `boolean` → `{"type": "boolean"}`
/// - `array` → `{"type": "array"}`
/// - `object` → `{"type": "object"}`
/// - any other type → `{"type": "string"}` (safe default)
///
/// If the contract output is already a JSON Schema (has `type` or `properties`
/// at the top level), it's returned as-is.
fn contract_output_to_schema(output: &Value) -> Value {
    // If it's already a JSON Schema object, return as-is.
    if output.is_object() && (output.get("type").is_some() || output.get("properties").is_some()) {
        return output.clone();
    }

    // Otherwise, it's a field-name → type-string mapping.
    let Some(fields) = output.as_object() else {
        return output.clone();
    };

    let mut properties = serde_json::Map::new();
    for (field_name, field_type) in fields {
        let type_str = field_type.as_str().unwrap_or("string");
        let json_type = match type_str {
            "string" | "str" => "string",
            "number" | "float" | "double" => "number",
            "integer" | "int" | "i32" | "i64" | "u32" | "u64" => "number",
            "boolean" | "bool" => "boolean",
            "array" => "array",
            "object" => "object",
            other => {
                // Unknown type string — warn before narrowing to "string".
                // The safe default keeps the cascade running, but the operator
                // must see the drift: a typo'd type (e.g. "intger") silently
                // narrows the schema, permitting strings where the manifest
                // author intended numbers. This is the `.rules` "validation
                // gates must return Undetermined/Skipped, not Ready with empty
                // findings" trap in schema form.
                tracing::warn!(
                    target: "hkask.templates",
                    field = %field_name,
                    declared_type = %other,
                    "contract_output_to_schema: unknown type string — narrowing to \"string\". \
                     A typo'd type silently narrows the output schema; the downstream \
                     step will accept strings where the manifest may have intended \
                     a different type."
                );
                "string"
            }
        };
        properties.insert(field_name.clone(), serde_json::json!({"type": json_type}));
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
    })
}

/// Build a synthetic `ChatToolDefinition` for structured output.
///
/// The tool is named `emit_result` and its parameters are the JSON Schema
/// derived from the contract output. When passed to the inference call,
/// the model is forced to call this tool (emitting JSON conforming to the
/// schema) instead of emitting free-text prose. The executor then extracts
/// the result from `InferenceResult.tool_calls[0].args`.
///
/// This is the LangGraph/Swarm pattern: enforce the output contract at the
/// inference API layer, not the prompt layer. The model physically cannot
/// emit prose when a tool is the only allowed response format.
pub(crate) fn build_structured_output_tool(schema: Value) -> ChatToolDefinition {
    ChatToolDefinition {
        tool_type: "function".to_string(),
        function: ChatToolFunction {
            name: "emit_result".to_string(),
            description: "Emit the structured result for this step. Call this tool with the JSON object matching the schema.".to_string(),
            parameters: schema,
        },
    }
}

