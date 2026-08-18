//! Delegation envelope — making provenance survive a hop (N2).
//!
//! The envelope carries the enforced payload (grounding applied), provenance
//! stamps, violations, and schema validation status. This means grounding
//! survives agent-to-agent composition — the composition path (the one that
//! matters for a fleet) is the protected one.
//!
//! The envelope is additive: every existing key on the
//! `LocalDelegateResult` is preserved byte-for-byte, and the envelope is
//! added under its own `envelope` key. Consumers that don't know about the
//! envelope ignore it; consumers that do can read grounding status without
//! parsing the `GroundingResult`.

use serde_json::{Value, json};

/// Three-valued grounding status. Distinguishes "ran" from "couldn't run"
/// from "no contract." A bool cannot — `false` conflates "no contract"
/// with "contract existed but output wasn't JSON."
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundingStatus {
    /// Contract existed and grounding ran.
    Enforced,
    /// Contract existed but output was not a JSON object.
    Unenforceable,
    /// No contract for this agent_type.
    NoContract,
}

/// Four-valued payload status. Distinguishes "no response" from "prose"
/// from "empty" from "document." A `bool` (is_object) cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadStatus {
    NoResponse,
    EmptyResponse,
    Document,
    ProseOnly,
}

/// Five-valued validation status. `Unverified*` is never a pass — a
/// consumer that treats it as one has reintroduced the defect this
/// envelope exists to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Invalid,
    UnsupportedSchema,
    NoSchema,
    NoPayload,
}

/// Schema validation result carried in the envelope.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValidationResult {
    pub status: ValidationStatus,
    pub violations: Vec<SchemaViolation>,
    pub unsupported: Vec<String>,
}

/// One schema violation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemaViolation {
    pub path: String,
    pub message: String,
}

/// Build the additive `envelope` value for a delegated execution.
///
/// The payload is the enforced document: ungrounded fields already nulled,
/// provenance already stamped. A consumer receives data that has been
/// through the grounding contract rather than data it must trust.
///
/// - `agent_name` is the producer's agent_id.
/// - `payload` is the cleaned JSON output (after grounding), if any.
/// - `grounding_status` distinguishes "ran" from "couldn't run" from "no contract."
/// - `payload_status` distinguishes "no response" from "prose" from "document."
/// - `grounding_result` is the `GroundingResult` from `enforce_grounding`.
///   `None` when grounding didn't run (no contract or unenforceable).
/// - `validation` is the schema validation result, if any.
pub fn build(
    agent_name: &str,
    payload: Option<&Value>,
    grounding_status: GroundingStatus,
    payload_status: PayloadStatus,
    grounding_result: Option<&crate::grounding::GroundingResult>,
    validation: Option<&ValidationResult>,
) -> Value {
    let blocks = grounding_result
        .map(|gr| {
            gr.provenance
                .iter()
                .map(|(field, tag)| {
                    json!({
                        "field": field,
                        "provenance": crate::grounding::provenance_stamp(tag),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let violations = grounding_result
        .map(|gr| {
            gr.nulled_fields
                .iter()
                .map(|path| {
                    json!({
                        "path": path,
                        "kind": "ungrounded_field",
                    })
                })
                .chain(gr.narrative_leaks.iter().map(|(needle, field)| {
                    json!({
                        "path": field,
                        "kind": "narrative_leak",
                        "needle": needle,
                    })
                }))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let validation_json = validation
        .map(|v| {
            json!({
                "status": v.status,
                "violations": v.violations,
                "unsupported": v.unsupported,
            })
        })
        .unwrap_or(json!({
            "status": ValidationStatus::NoSchema,
            "violations": [],
            "unsupported": [],
        }));

    json!({
        "producer": agent_name,
        "grounding_status": grounding_status,
        "payload_status": payload_status,
        "payload": payload,
        "blocks": blocks,
        "violations": violations,
        "validation": validation_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::{GroundingResult, ProvenanceTag};

    #[test]
    fn envelope_carries_enforced_status_when_grounding_ran() {
        let result = GroundingResult::default();
        let env = build(
            "test_agent",
            Some(&json!({"field": "value"})),
            GroundingStatus::Enforced,
            PayloadStatus::Document,
            Some(&result),
            None,
        );
        assert_eq!(env["producer"], "test_agent");
        assert_eq!(env["grounding_status"], "enforced");
    }

    #[test]
    fn envelope_carries_no_contract_status_when_no_contract() {
        let env = build(
            "test_agent",
            None,
            GroundingStatus::NoContract,
            PayloadStatus::NoResponse,
            None,
            None,
        );
        assert_eq!(env["grounding_status"], "no_contract");
        assert!(env["payload"].is_null());
    }

    #[test]
    fn envelope_carries_unenforceable_status_when_output_not_object() {
        let env = build(
            "test_agent",
            Some(&json!("just prose")),
            GroundingStatus::Unenforceable,
            PayloadStatus::ProseOnly,
            None,
            None,
        );
        assert_eq!(env["grounding_status"], "unenforceable");
        assert_eq!(env["payload_status"], "prose_only");
    }

    #[test]
    fn envelope_distinguishes_document_from_prose_only() {
        let doc_env = build(
            "test_agent",
            Some(&json!({"key": "value"})),
            GroundingStatus::Enforced,
            PayloadStatus::Document,
            Some(&GroundingResult::default()),
            None,
        );
        assert_eq!(doc_env["payload_status"], "document");

        let prose_env = build(
            "test_agent",
            Some(&json!("a string, not an object")),
            GroundingStatus::Unenforceable,
            PayloadStatus::ProseOnly,
            None,
            None,
        );
        assert_eq!(prose_env["payload_status"], "prose_only");
    }

    #[test]
    fn envelope_distinguishes_no_response_from_empty() {
        let no_resp = build(
            "test_agent",
            None,
            GroundingStatus::NoContract,
            PayloadStatus::NoResponse,
            None,
            None,
        );
        assert_eq!(no_resp["payload_status"], "no_response");
        assert!(no_resp["payload"].is_null());

        let empty = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&GroundingResult::default()),
            None,
        );
        assert_eq!(empty["payload_status"], "empty_response");
        assert!(empty["payload"].is_object());
    }

    #[test]
    fn envelope_reports_validation_status_valid() {
        let validation = ValidationResult {
            status: ValidationStatus::Valid,
            violations: vec![],
            unsupported: vec![],
        };
        let env = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&GroundingResult::default()),
            Some(&validation),
        );
        assert_eq!(env["validation"]["status"], "valid");
        assert!(
            env["validation"]["violations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn envelope_reports_validation_status_no_schema_when_absent() {
        let env = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&GroundingResult::default()),
            None,
        );
        assert_eq!(env["validation"]["status"], "no_schema");
        assert!(
            env["validation"]["violations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            env["validation"]["unsupported"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn envelope_reports_nulled_fields_as_violations() {
        let mut result = GroundingResult::default();
        result.nulled_fields.push("deliverable_path".to_string());
        result.provenance.insert(
            "deliverable_path".to_string(),
            ProvenanceTag::Unsourced {
                removed_preview: "/src/main.rs".to_string(),
                tool_failed: false,
            },
        );
        let env = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&result),
            None,
        );
        let violations = env["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["path"], "deliverable_path");
        assert_eq!(violations[0]["kind"], "ungrounded_field");
    }

    #[test]
    fn envelope_reports_narrative_leaks_as_violations() {
        let mut result = GroundingResult::default();
        result
            .narrative_leaks
            .push(("/src/main.rs".to_string(), "deliverable_path".to_string()));
        let env = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&result),
            None,
        );
        let violations = env["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["kind"], "narrative_leak");
        assert_eq!(violations[0]["needle"], "/src/main.rs");
    }

    #[test]
    fn envelope_carries_provenance_blocks() {
        let mut result = GroundingResult::default();
        result.provenance.insert(
            "deliverable_path".to_string(),
            ProvenanceTag::Sourced {
                tool: "zed/write_file".to_string(),
            },
        );
        result
            .provenance
            .insert("summary".to_string(), ProvenanceTag::Inferred);
        let env = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&result),
            None,
        );
        let blocks = env["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        let dp = blocks
            .iter()
            .find(|b| b["field"] == "deliverable_path")
            .expect("deliverable_path block missing");
        assert_eq!(dp["provenance"], "tool_verified");
        let summary = blocks
            .iter()
            .find(|b| b["field"] == "summary")
            .expect("summary block missing");
        assert_eq!(summary["provenance"], "model_inference");
    }

    #[test]
    fn envelope_with_no_violations_is_clean() {
        let result = GroundingResult::default();
        let env = build(
            "test_agent",
            Some(&json!({})),
            GroundingStatus::Enforced,
            PayloadStatus::EmptyResponse,
            Some(&result),
            None,
        );
        assert!(env["violations"].as_array().unwrap().is_empty());
        assert!(env["blocks"].as_array().unwrap().is_empty());
    }
}
