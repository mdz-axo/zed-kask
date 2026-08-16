//! Grounding contract — Rung 3 (Grounding) of the verification ladder.
//!
//! Stops a fully-typed agent ecology from being a fully-fabricated one.
//! Declares which output fields of a `kanban-task-*` agent must be sourced
//! from actual tool calls vs. inferred by the LLM. Per invocation, checks
//! which fields could have come from available tools. Nulls unsourced
//! fields, retains the removed value (paper §5.5: "tag, do not delete"),
//! scans narrative for leaked claims.
//!
//! The five-valued vocabulary (paper §5.5 extended):
//! - Sourced: a named tool returned it. Keep, mark verified.
//! - Inferred: judgement over sourced inputs, by design (commissioned).
//!   Keep, mark as inference.
//! - UncommissionedInference: the model produced a judgment that was not
//!   explicitly commissioned but is plausibly within the agent's scope.
//!   Keep, mark as uncommissioned inference, scan for unsupported claims.
//! - Narrative: prose. Keep, scan for claims it cannot support.
//! - Unsourced: no tool could supply it. Null it, record what was removed.
//!
//! The contract is hand-declared and therefore incomplete (paper §6).
//! Coverage is itself a metric.

use std::collections::HashMap;

/// The five-valued grounding vocabulary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProvenanceTag {
    /// A named tool returned it.
    Sourced { tool: String },
    /// Judgement over sourced inputs, by design (commissioned by the
    /// system prompt).
    Inferred,
    /// The model produced a judgment that was not explicitly commissioned
    /// but is plausibly within the agent's scope. Distinct from Unsourced
    /// because the agent was implicitly authorized to reason, not to
    /// fabricate facts.
    UncommissionedInference,
    /// Prose — kept, scanned for claims it cannot support.
    Narrative,
    /// No tool could supply it. Nulled, value retained for calibration.
    Unsourced {
        /// Truncated preview of the removed value (first 200 chars).
        /// The full value goes to the audit log, not the API response.
        removed_preview: String,
    },
}

/// A field → tool map for one agent type's structured output.
/// Hand-declared, therefore incomplete (paper §6). Coverage is itself
/// a metric.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroundingContract {
    /// The agent_type this contract applies to (e.g. "task").
    pub agent_type: String,
    /// Map of output field name → list of tools that can source it.
    /// Empty list = the field is Inferred (commissioned judgment),
    /// not Unsourced.
    pub field_sources: HashMap<String, Vec<String>>,
}

/// The result of grounding enforcement on one delegation output.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GroundingResult {
    /// Per-field provenance tags.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub provenance: HashMap<String, ProvenanceTag>,
    /// Fields that were nulled (Unsourced).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nulled_fields: Vec<String>,
    /// Narrative claims that leaked unsourced values. Each entry is a
    /// (substring_found, field_it_leaked) pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrative_leaks: Vec<(String, String)>,
}

/// The built-in grounding contract for `kanban-task-*` agents.
///
/// These agents are spawned by `kanban_task_spawn` to execute a task using
/// declared skills. Their output may include:
/// - `deliverable_path`: a file path the agent claims to have written.
///   Must be sourced from an `edit_file`, `write_file`, or `terminal` tool
///   call that succeeded.
/// - `test_verdict`: a pass/fail claim about tests. Must be sourced from a
///   `terminal` tool call that succeeded (the test runner).
/// - `summary`: a prose summary of what the agent did. Inferred — the
///   agent was commissioned to summarize.
/// - `approach`: a description of the approach taken. Inferred.
///
/// Any other field in the output is treated as UncommissionedInference
/// (kept, marked) unless it matches a tool's output (Sourced) or has no
/// possible source (Unsourced, nulled).
pub fn task_agent_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    field_sources.insert(
        "deliverable_path".to_string(),
        vec![
            "zed/edit_file".to_string(),
            "zed/write_file".to_string(),
            "zed/terminal".to_string(),
        ],
    );
    field_sources.insert(
        "test_verdict".to_string(),
        vec!["zed/terminal".to_string()],
    );
    // Commissioned judgments — empty source list = Inferred, not Unsourced.
    field_sources.insert("summary".to_string(), vec![]);
    field_sources.insert("approach".to_string(), vec![]);
    GroundingContract {
        agent_type: "task".to_string(),
        field_sources,
    }
}

/// Extract the set of tools that successfully returned data from the
/// `tool_calls` summary on a `LocalDelegateResult`.
///
/// The `tool_calls` entries have shape `{"tool": "server/tool_name", "ok": true/false}`.
/// Only successful calls count — a tool that errored did not supply data.
fn successful_tools(tool_calls: &[serde_json::Value]) -> std::collections::HashSet<String> {
    tool_calls
        .iter()
        .filter_map(|tc| {
            let ok = tc.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if !ok {
                return None;
            }
            tc.get("tool")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Check whether a field's value could have been sourced from any of the
/// declared tools. Returns true if at least one declared tool was called
/// successfully.
fn is_sourced(field_tools: &[String], successful: &std::collections::HashSet<String>) -> bool {
    field_tools.iter().any(|t| successful.contains(t))
}

/// Truncate a value to a preview string for the Unsourced tag.
fn truncate_preview(value: &serde_json::Value) -> String {
    let s = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s
    }
}

/// Scan narrative text for mentions of a removed value. Returns the
/// matching substring if found.
///
/// This is deliberately conservative — it checks whether the removed
/// value's string representation appears as a substring of the narrative.
/// Over-reach (paper Rule 5.2) is the main risk: a short removed value
/// like "0" would match any narrative containing "0". We mitigate by
/// requiring the preview to be at least 10 characters before scanning.
fn scan_narrative_for_leak(
    narrative: &str,
    removed_preview: &str,
    field_name: &str,
) -> Option<(String, String)> {
    if removed_preview.len() < 10 {
        return None;
    }
    if narrative.contains(removed_preview) {
        Some((removed_preview.to_string(), field_name.to_string()))
    } else {
        None
    }
}

/// Run the grounding contract against a delegation output.
///
/// - Sourced fields: keep, mark verified.
/// - Inferred fields (empty source list): keep, mark as inference.
/// - Fields not in the contract: mark as UncommissionedInference.
/// - Unsourced fields (in contract, no matching tool call): null,
///   retain a truncated preview.
/// - Narrative: scan for leaked removed values.
///
/// Returns the grounding result and a cleaned output with unsourced
/// fields nulled.
pub fn enforce_grounding(
    contract: &GroundingContract,
    output: &serde_json::Value,
    tool_calls: &[serde_json::Value],
    narrative: &str,
) -> (GroundingResult, serde_json::Value) {
    let successful = successful_tools(tool_calls);
    let mut result = GroundingResult::default();
    let mut cleaned = output.clone();

    if let serde_json::Value::Object(ref map) = output {
        for (field, value) in map {
            let tag = match contract.field_sources.get(field) {
                Some(sources) if sources.is_empty() => {
                    // Commissioned judgment — Inferred.
                    ProvenanceTag::Inferred
                }
                Some(sources) => {
                    // Has declared tools — check if any was called successfully.
                    if is_sourced(sources, &successful) {
                        ProvenanceTag::Sourced {
                            tool: sources
                                .iter()
                                .find(|t| successful.contains(*t))
                                .cloned()
                                .unwrap_or_default(),
                        }
                    } else {
                        // Declared tools exist but none were called successfully.
                        // The field claims a value no tool supplied — null it.
                        let preview = truncate_preview(value);
                        result.nulled_fields.push(field.clone());
                        // Null the field in the cleaned output.
                        if let serde_json::Value::Object(ref mut clean_map) = cleaned {
                            clean_map.insert(field.clone(), serde_json::Value::Null);
                        }
                        // Scan narrative for the leaked value.
                        if let Some(leak) =
                            scan_narrative_for_leak(narrative, &preview, field)
                        {
                            result.narrative_leaks.push(leak);
                        }
                        ProvenanceTag::Unsourced {
                            removed_preview: preview,
                        }
                    }
                }
                None => {
                    // Field not in the contract — UncommissionedInference.
                    // The model produced something the contract didn't
                    // declare. Keep it, but mark it so the caller knows
                    // it wasn't checked.
                    ProvenanceTag::UncommissionedInference
                }
            };
            result.provenance.insert(field.clone(), tag);
        }
    }

    (result, cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_call(tool: &str, ok: bool) -> serde_json::Value {
        json!({ "tool": tool, "ok": ok })
    }

    #[test]
    fn sourced_field_kept_when_tool_succeeded() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "I wrote the file."
        });
        let tool_calls = vec![tool_call("zed/write_file", true)];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["deliverable_path"], "/src/main.rs");
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Sourced { tool } => {
                assert_eq!(tool, "zed/write_file");
            }
            other => panic!("expected Sourced, got {other:?}"),
        }
    }

    #[test]
    fn unsourced_field_nulled_when_no_tool_called() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "I wrote the file."
        });
        // No tool calls — deliverable_path is unsourced.
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(cleaned["deliverable_path"].is_null());
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Unsourced { removed_preview } => {
                assert_eq!(removed_preview, "/src/main.rs");
            }
            other => panic!("expected Unsourced, got {other:?}"),
        }
    }

    #[test]
    fn unsourced_field_nulled_when_tool_failed() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs"
        });
        // Tool was called but failed — did not supply data.
        let tool_calls = vec![tool_call("zed/write_file", false)];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(cleaned["deliverable_path"].is_null());
    }

    #[test]
    fn inferred_field_kept() {
        let contract = task_agent_contract();
        let output = json!({
            "summary": "I completed the task by writing a new module."
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(
            cleaned["summary"],
            "I completed the task by writing a new module."
        );
        assert_eq!(
            result.provenance["summary"],
            ProvenanceTag::Inferred
        );
    }

    #[test]
    fn uncommissioned_field_marked_but_kept() {
        let contract = task_agent_contract();
        // "author_name" is not in the contract — UncommissionedInference.
        let output = json!({
            "author_name": "Jane Doe"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["author_name"], "Jane Doe");
        assert_eq!(
            result.provenance["author_name"],
            ProvenanceTag::UncommissionedInference
        );
    }

    #[test]
    fn narrative_leak_detected() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/very/long/path/to/main.rs"
        });
        // No tool calls — deliverable_path is nulled.
        let tool_calls: Vec<serde_json::Value> = vec![];
        // The narrative restates the nulled value.
        let narrative = "I wrote the file at /src/very/long/path/to/main.rs and it works.";

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, narrative);
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert_eq!(result.narrative_leaks.len(), 1);
        assert_eq!(result.narrative_leaks[0].1, "deliverable_path");
    }

    #[test]
    fn narrative_leak_not_detected_for_short_values() {
        // Short values (< 10 chars) are not scanned — over-reach mitigation
        // (paper Rule 5.2).
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/x"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];
        let narrative = "The path /x is correct.";

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, narrative);
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(result.narrative_leaks.is_empty());
    }

    #[test]
    fn no_contract_fields_means_all_uncommissioned() {
        // An output with no fields in the contract — all UncommissionedInference.
        let contract = GroundingContract {
            agent_type: "task".to_string(),
            field_sources: HashMap::new(),
        };
        let output = json!({
            "random_field": "value",
            "another": 42
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned, output); // unchanged
        assert_eq!(
            result.provenance["random_field"],
            ProvenanceTag::UncommissionedInference
        );
    }

    #[test]
    fn non_object_output_no_grounding() {
        // Prose output (not JSON) — no fields to ground.
        let contract = task_agent_contract();
        let output = serde_json::Value::String("just prose".to_string());
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.provenance.is_empty());
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned, output);
    }

    #[test]
    fn test_verdict_sourced_from_terminal() {
        let contract = task_agent_contract();
        let output = json!({
            "test_verdict": "pass: 3 tests ran, 0 failed"
        });
        let tool_calls = vec![tool_call("zed/terminal", true)];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(
            cleaned["test_verdict"],
            "pass: 3 tests ran, 0 failed"
        );
        match &result.provenance["test_verdict"] {
            ProvenanceTag::Sourced { tool } => {
                assert_eq!(tool, "zed/terminal");
            }
            other => panic!("expected Sourced, got {other:?}"),
        }
    }

    #[test]
    fn test_verdict_nulled_when_no_terminal_call() {
        // The agent claims tests passed but never ran them.
        let contract = task_agent_contract();
        let output = json!({
            "test_verdict": "pass: all tests passed"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(result.nulled_fields, vec!["test_verdict"]);
        assert!(cleaned["test_verdict"].is_null());
    }

    #[test]
    fn multiple_fields_mixed_grounding() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "test_verdict": "pass",
            "summary": "Done.",
            "unknown_field": "surprise"
        });
        // Only write_file succeeded — terminal was not called.
        let tool_calls = vec![tool_call("zed/write_file", true)];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        // deliverable_path: Sourced (write_file succeeded)
        assert!(cleaned["deliverable_path"].is_string());
        // test_verdict: Unsourced (terminal not called)
        assert!(cleaned["test_verdict"].is_null());
        assert!(result.nulled_fields.contains(&"test_verdict".to_string()));
        // summary: Inferred
        assert_eq!(result.provenance["summary"], ProvenanceTag::Inferred);
        // unknown_field: UncommissionedInference
        assert_eq!(
            result.provenance["unknown_field"],
            ProvenanceTag::UncommissionedInference
        );
    }

    #[test]
    fn successful_tools_filters_failed_calls() {
        let tool_calls = vec![
            tool_call("zed/terminal", true),
            tool_call("zed/write_file", false),
            tool_call("zed/edit_file", true),
        ];
        let successful = successful_tools(&tool_calls);
        assert!(successful.contains("zed/terminal"));
        assert!(successful.contains("zed/edit_file"));
        assert!(!successful.contains("zed/write_file"));
    }

    #[test]
    fn truncate_preview_long_values() {
        let long = serde_json::Value::String("x".repeat(300));
        let preview = truncate_preview(&long);
        assert!(preview.ends_with("..."));
        assert_eq!(preview.len(), 203); // 200 + "..."
    }

    #[test]
    fn truncate_preview_short_values_unchanged() {
        let short = serde_json::Value::String("short".to_string());
        let preview = truncate_preview(&short);
        assert_eq!(preview, "short");
    }
}
