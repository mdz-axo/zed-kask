//! Integration tests for the grounding contract (Rung 3).
//!
//! These tests verify the grounding contract shape and the `enforce_grounding`
//! function's behavior from the integration test perspective — confirming
//! the contract declared in `grounding::task_agent_contract()` matches the
//! fields that `build_task_agent_card`'s system prompt instructs the agent
//! to produce. The unit tests in `grounding.rs` test the function; these
//! tests test the contract's alignment with the system prompt (paper Rule
//! 5.1: a check that has never failed in its real path has not been tested).

#![cfg(test)]

use hkask_mcp_swarm::LocalAgentCard;
use hkask_verification::grounding::{self, ProvenanceTag};

// ── Grounding contract wiring tests ─────────────────────────────────────

/// The grounding contract for "task" agent_type declares the expected
/// fields and their source tools. This pins the contract shape so a
/// future change that drops a field or changes a source tool is caught.
#[test]
fn task_agent_contract_declares_expected_fields() {
    let contract = grounding::task_agent_contract();
    assert_eq!(contract.agent_type, "task");
    assert!(contract.field_sources.contains_key("deliverable_path"));
    assert!(contract.field_sources.contains_key("test_verdict"));
    assert!(contract.field_sources.contains_key("summary"));
    assert!(contract.field_sources.contains_key("approach"));

    // deliverable_path must be sourced from file-writing tools.
    let dp_sources = contract.field_sources["deliverable_path"]
        .sources
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>();
    assert!(dp_sources.contains(&"zed/edit_file"));
    assert!(dp_sources.contains(&"zed/write_file"));
    assert!(dp_sources.contains(&"zed/terminal"));

    // test_verdict must be sourced from terminal.
    let tv_sources = &contract.field_sources["test_verdict"].sources;
    assert!(tv_sources.contains(&"zed/terminal".to_string()));

    // summary and approach are Inferred (empty source lists).
    assert!(contract.field_sources["summary"].sources.is_empty());
    assert!(contract.field_sources["approach"].sources.is_empty());
}

/// When the agent produces JSON with a deliverable_path but no file-writing
/// tool was called, the grounding check nulls the field.
#[test]
fn grounding_nulls_deliverable_path_when_no_tool_called() {
    let contract = grounding::task_agent_contract();
    let output = serde_json::json!({
        "deliverable_path": "/src/main.rs",
        "summary": "I wrote the file."
    });
    let tool_calls: Vec<serde_json::Value> = vec![];

    let (result, cleaned) = grounding::enforce_grounding(&contract, &output, &tool_calls, "");
    assert!(
        result
            .nulled_fields
            .contains(&"deliverable_path".to_string())
    );
    assert!(cleaned["deliverable_path"].is_null());
    // summary is Inferred — kept.
    assert_eq!(cleaned["summary"], "I wrote the file.");
}

/// When the agent produces JSON with a deliverable_path AND a file-writing
/// tool was called successfully, the field is kept and marked Sourced.
#[test]
fn grounding_keeps_deliverable_path_when_tool_succeeded() {
    let contract = grounding::task_agent_contract();
    let output = serde_json::json!({
        "deliverable_path": "/src/main.rs",
        "summary": "I wrote the file."
    });
    let tool_calls = vec![serde_json::json!({
        "tool": "zed/write_file",
        "ok": true,
        "result": {"path": "/src/main.rs"}
    })];

    let (result, cleaned) = grounding::enforce_grounding(&contract, &output, &tool_calls, "");
    assert!(result.nulled_fields.is_empty());
    assert_eq!(cleaned["deliverable_path"], "/src/main.rs");
    match &result.provenance["deliverable_path"] {
        ProvenanceTag::Sourced { tool } => {
            assert_eq!(tool, "zed/write_file");
        }
        other => panic!("expected Sourced, got {other:?}"),
    }
}

/// When the agent produces JSON with a test_verdict but no terminal tool
/// was called, the field is nulled. This is the paper's headline defect:
/// an agent that claims tests passed without running them.
#[test]
fn grounding_nulls_test_verdict_when_no_terminal_call() {
    let contract = grounding::task_agent_contract();
    let output = serde_json::json!({
        "test_verdict": "pass: all tests passed",
        "summary": "All tests pass."
    });
    // Only write_file was called — terminal was not.
    let tool_calls = vec![serde_json::json!({
        "tool": "zed/write_file",
        "ok": true
    })];

    let (result, cleaned) = grounding::enforce_grounding(&contract, &output, &tool_calls, "");
    assert!(result.nulled_fields.contains(&"test_verdict".to_string()));
    assert!(cleaned["test_verdict"].is_null());
    // summary is Inferred — kept.
    assert_eq!(cleaned["summary"], "All tests pass.");
}

/// When the agent restates a nulled value in the narrative, the leak is
/// detected.
#[test]
fn grounding_detects_narrative_leak_of_nulled_value() {
    let contract = grounding::task_agent_contract();
    let output = serde_json::json!({
        "deliverable_path": "/src/very/long/path/to/main.rs"
    });
    let tool_calls: Vec<serde_json::Value> = vec![];
    let narrative = "I wrote the file at /src/very/long/path/to/main.rs and it works.";

    let (result, _cleaned) =
        grounding::enforce_grounding(&contract, &output, &tool_calls, narrative);
    assert_eq!(result.narrative_leaks.len(), 1);
    assert_eq!(result.narrative_leaks[0].1, "deliverable_path");
}

/// When the agent produces a field not in the contract, it is marked
/// UncommissionedInference but kept (not nulled).
#[test]
fn grounding_marks_uncommissioned_inference_but_keeps() {
    let contract = grounding::task_agent_contract();
    let output = serde_json::json!({
        "author_name": "Jane Doe",
        "summary": "Done."
    });
    let tool_calls: Vec<serde_json::Value> = vec![];

    let (result, cleaned) = grounding::enforce_grounding(&contract, &output, &tool_calls, "");
    assert!(result.nulled_fields.is_empty());
    assert_eq!(cleaned["author_name"], "Jane Doe");
    assert_eq!(
        result.provenance["author_name"],
        ProvenanceTag::UncommissionedInference
    );
}

/// When the output is not JSON (prose), the grounding check is a no-op —
/// no fields to ground. This is the paper's §6 limit: prose output is not
/// covered by the grounding contract.
#[test]
fn grounding_noop_for_prose_output() {
    let contract = grounding::task_agent_contract();
    let output = serde_json::Value::String("just prose".to_string());
    let tool_calls: Vec<serde_json::Value> = vec![];

    let (result, cleaned) = grounding::enforce_grounding(&contract, &output, &tool_calls, "");
    assert!(result.provenance.is_empty());
    assert!(result.nulled_fields.is_empty());
    assert_eq!(cleaned, output);
}

/// The grounding contract applies only to agent_type "task". Other types
/// get no grounding (None = not checked, paper Rule 5.3).
#[test]
fn grounding_contract_only_applies_to_task_agent_type() {
    let contract = grounding::task_agent_contract();
    assert_eq!(contract.agent_type, "task");
    // A card with agent_type "narrator" would not match — the spawn path
    // checks `agent.agent_type == grounding_contract.agent_type` before
    // calling enforce_grounding.
    let narrator_card = LocalAgentCard {
        agent_id: "narrator".to_string(),
        agent_type: "narrator".to_string(),
        ..Default::default()
    };
    assert_ne!(narrator_card.agent_type, contract.agent_type);
}
