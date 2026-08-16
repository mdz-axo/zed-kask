//! Delegation envelope — making provenance survive a hop (N2).
//!
//! Fermi's `envelope.rs` pattern: when one agent delegates to another, the
//! response carries an additive `envelope` key with the enforced payload
//! (grounding applied), provenance stamps, and violations. This means
//! grounding survives agent-to-agent composition — the composition path
//! (the one that matters for a fleet) is the protected one.
//!
//! The envelope is additive on purpose: every existing key on the
//! `LocalDelegateResult` is preserved byte-for-byte, and the envelope is
//! added under its own `envelope` key. Consumers that don't know about the
//! envelope ignore it; consumers that do can read grounding status without
//! parsing the `GroundingResult`.
//!
//! What an envelope asserts, and what it does not:
//! - It asserts: *this is the producer's own document, enforced against its
//!   declared grounding contract, and here is what was stripped.*
//! - It does not assert the document is correct, or that it validates
//!   against a JSON Schema — schema validation is the next increment.

use serde_json::{Value, json};

/// Build the additive `envelope` value for a delegated execution.
///
/// The payload is the enforced document: ungrounded fields already nulled,
/// provenance already stamped. A consumer receives data that has been
/// through the grounding contract rather than data it must trust.
///
/// `agent_name` is the producer's agent_id.
/// `payload` is the cleaned JSON output (after grounding).
/// `grounding_result` is the `GroundingResult` from `enforce_grounding`.
/// `has_contract` is whether a grounding contract exists for this producer.
pub fn build(
    agent_name: &str,
    payload: Option<&Value>,
    grounding_result: &crate::grounding::GroundingResult,
    has_contract: bool,
) -> Value {
    json!({
        "producer": agent_name,
        // Whether a grounding contract exists for this producer at all.
        // False is not a pass — it means nobody has written one, which
        // the coverage metric reports and this must not disguise.
        "grounding_enforced": has_contract,
        "payload": payload,
        "blocks": grounding_result
            .provenance
            .iter()
            .map(|(field, tag)| {
                json!({
                    "field": field,
                    "provenance": crate::grounding::provenance_stamp(tag),
                })
            })
            .collect::<Vec<_>>(),
        "violations": grounding_result
            .nulled_fields
            .iter()
            .map(|path| {
                json!({
                    "path": path,
                    "kind": "ungrounded_field",
                })
            })
            .chain(grounding_result.narrative_leaks.iter().map(|(needle, field)| {
                json!({
                    "path": field,
                    "kind": "narrative_leak",
                    "needle": needle,
                })
            }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::{GroundingResult, ProvenanceTag};

    #[test]
    fn envelope_carries_producer_and_grounding_status() {
        let result = GroundingResult::default();
        let env = build(
            "test_agent",
            Some(&json!({"field": "value"})),
            &result,
            true,
        );
        assert_eq!(env["producer"], "test_agent");
        assert_eq!(env["grounding_enforced"], true);
    }

    #[test]
    fn envelope_carries_payload() {
        let result = GroundingResult::default();
        let payload = json!({"deliverable_path": "/src/main.rs"});
        let env = build("test_agent", Some(&payload), &result, true);
        assert_eq!(env["payload"]["deliverable_path"], "/src/main.rs");
    }

    #[test]
    fn envelope_carries_none_payload_when_absent() {
        let result = GroundingResult::default();
        let env = build("test_agent", None, &result, false);
        assert!(env["payload"].is_null());
        assert_eq!(env["grounding_enforced"], false);
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
        let env = build("test_agent", Some(&json!({})), &result, true);
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
        let env = build("test_agent", Some(&json!({})), &result, true);
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
        let env = build("test_agent", Some(&json!({})), &result, true);
        let blocks = env["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        // Find the deliverable_path block.
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
        let env = build("test_agent", Some(&json!({})), &result, true);
        assert!(env["violations"].as_array().unwrap().is_empty());
        assert!(env["blocks"].as_array().unwrap().is_empty());
    }
}
