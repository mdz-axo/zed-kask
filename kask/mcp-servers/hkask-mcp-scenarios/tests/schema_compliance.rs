//! Schema-compliance tests for hkask-mcp-scenarios tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! The scan runs over the live combined router (all 22 registered tools),
//! not a stale hand-maintained type list, so a tool added without its
//! strictness check cannot degrade silently.

use hkask_mcp_scenarios::ScenariosServer;
use hkask_types::find_boolean_schema_positions;
use schemars::schema_for;

/// expect: every registered tool's live input schema is strict — no bare
/// boolean in any schema-valued position — and the arbitrary-JSON inputs
/// render as JSON-text strings, never as open schemas.
#[test]
fn live_router_schemas_are_strict_for_all_registered_tools() {
    let tools = ScenariosServer::combined_router().list_all();
    assert_eq!(
        tools.len(),
        22,
        "approved surface: 22 registered tools (pinned by tool_surface_is_exactly_22_registered_tools)"
    );
    for tool in tools {
        let schema = serde_json::to_value(&tool.input_schema).expect("router schema serializes");
        let violations = find_boolean_schema_positions(&schema);
        assert!(
            violations.is_empty(),
            "{} live schema contains bare-boolean positions: {violations:?}",
            tool.name
        );
    }
}

/// expect: the arbitrary-JSON request fields render as JSON-text strings
/// (the `AnyJsonValue` contract), never as bare booleans or open objects.
/// Optional fields render as the `["string", "null"]` union; required
/// fields as the bare `"string"`.
#[test]
fn arbitrary_json_inputs_render_as_json_text_strings() {
    let by_name: std::collections::BTreeMap<_, _> = ScenariosServer::combined_router()
        .list_all()
        .into_iter()
        .map(|tool| {
            (
                tool.name.clone(),
                serde_json::to_value(&tool.input_schema).unwrap(),
            )
        })
        .collect();

    let is_json_text = |schema: &serde_json::Value| -> bool {
        matches!(schema["type"].as_str(), Some("string"))
            || schema["type"].as_array().is_some_and(|types| {
                types.iter().any(|t| t == "string")
                    && types.iter().all(|t| t == "string" || t == "null")
            })
    };

    let frame = by_name
        .get("scenario_frame")
        .expect("scenario_frame registered")
        .clone();
    assert!(
        is_json_text(&frame["properties"]["prior_answers"]),
        "prior_answers must be JSON text, not an open schema: {frame}"
    );

    let frame_document = by_name
        .get("scenario_frame_document")
        .expect("scenario_frame_document registered")
        .clone();
    assert!(
        is_json_text(&frame_document["properties"]["answers"]),
        "answers must be JSON text, not an open schema: {frame_document}"
    );

    let markets = by_name
        .get("scenario_from_markets_set")
        .expect("scenario_from_markets_set registered")
        .clone();
    assert!(
        is_json_text(&markets["properties"]["market_records"]),
        "market_records must be JSON text, not an open schema: {markets}"
    );

    let persist = by_name
        .get("forecast_persist")
        .expect("forecast_persist registered")
        .clone();
    assert!(
        is_json_text(&persist["properties"]["scenario_tree"]),
        "scenario_tree must be JSON text, not an open schema: {persist}"
    );
}

/// expect: the typed request structs carry no bare-boolean schema positions
/// either — the check holds at both the type and the live-router level.
#[test]
fn typed_request_structs_are_schema_clean() {
    use hkask_mcp_scenarios::requests::*;

    for (name, schema) in [
        ("BuildEventsRequest", schema_for!(BuildEventsRequest)),
        ("BrainstormRequest", schema_for!(BrainstormRequest)),
        ("FrameRequest", schema_for!(FrameRequest)),
        ("FrameDocumentRequest", schema_for!(FrameDocumentRequest)),
        ("PropagateRequest", schema_for!(PropagateRequest)),
        ("PosteriorsRequest", schema_for!(PosteriorsRequest)),
        (
            "PosteriorEvidenceEntry",
            schema_for!(PosteriorEvidenceEntry),
        ),
        ("CrossValidateRequest", schema_for!(CrossValidateRequest)),
        ("QuantifyRequest", schema_for!(QuantifyRequest)),
        ("UpdateRequest", schema_for!(UpdateRequest)),
        ("OutcomeEntry", schema_for!(OutcomeEntry)),
        ("DeclareRequest", schema_for!(DeclareRequest)),
        ("ScoreRequest", schema_for!(ScoreRequest)),
        ("CalibrateRequest", schema_for!(CalibrateRequest)),
        ("SynthesizeRequest", schema_for!(SynthesizeRequest)),
        ("CalibrationRequest", schema_for!(CalibrationRequest)),
        ("TriageRequest", schema_for!(TriageRequest)),
        ("AssessRequest", schema_for!(AssessRequest)),
        ("StatusRequest", schema_for!(StatusRequest)),
    ] {
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let violations = find_boolean_schema_positions(&value);
        assert!(
            violations.is_empty(),
            "{name} schema has bare-boolean schema positions (Ollama/Gemini would reject): {violations:?}"
        );
    }
}
