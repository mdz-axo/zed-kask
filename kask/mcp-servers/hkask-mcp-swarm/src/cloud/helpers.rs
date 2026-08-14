//! Pure helpers for the ABW cloud tools — the agent-card builder and the
//! execute-response extractor. Extracted from `cloud_tools.rs` so the tool
//! methods file stays focused on the `#[tool_router]` impl, and the pure
//! functions (which are property-tested without a live ABW connection) live
//! alongside their tests.
//!
//! Both helpers are `pub` so `cloud_tools.rs` and the `test_utils` module can
//! re-export them.

use crate::request_types::CreateAgentRequest;

/// Build the ABW agent-card JSON payload for `POST /api/agents` from a
/// `CreateAgentRequest`. Pure function — no HTTP, no auth, no side effects —
/// so it can be property-tested without a live ABW connection.
///
/// The card shape mirrors fermi's `resolve_agent_card` expectations:
/// - `capabilities.mcp_tools` (outbound — what the agent exposes over
///   `/mcp/agents/:id`). Always present, defaulting to `[]`.
/// - `capabilities.mcp_servers` (inbound — third-party MCP servers the agent
///   may call as a client, fermi v0.11.6 / mig-177). Injected only when the
///   caller supplies a value: `None` omits the field (fermi inherits from the
///   filesystem card via NULL column); `Some([])` is authoritative "no
///   servers"; `Some([...])` is authoritative replacement. Secrets are
///   referenced by `auth.secret_key` (agent owner's scoped secret store) —
///   never inlined in the card.
/// - `metadata.valence` (personality encoding, fermi v0.10.x).
/// - `dependencies` (compound agent team, fermi v0.10.x).
pub fn build_create_agent_card(
    req: &CreateAgentRequest,
    default_agent_model: &str,
) -> serde_json::Value {
    let mut card = serde_json::json!({
        "agent_name": req.agent_name,
        "agent_type": req.agent_type,
        "system_prompt": req.system_prompt,
        "capabilities": {
            "executor": "llm",
            "model": req.model.clone().unwrap_or_else(|| default_agent_model.to_string()),
            "temperature": req.temperature.unwrap_or(0.3),
            "provider": "anthropic",
            "mcp_tools": req.mcp_tools.clone().unwrap_or_default(),
            "skills": req.skills.clone().unwrap_or_default(),
        },
        "metadata": {
            "description": req.description,
            "tags": req.tags.clone().unwrap_or_default(),
            "sample_queries": req.sample_queries.clone().unwrap_or_default(),
        },
        "visibility": req.visibility.clone().unwrap_or_else(|| "private".to_string()),
    });
    // fermi v0.11.6 (mig-177): inbound MCP servers. Inject only when the
    // caller supplied a value. See the struct doc on `CreateAgentRequest`
    // for the None/Some([])/Some([...]) precedence contract.
    if let Some(mcp_servers) = &req.mcp_servers {
        card["capabilities"]["mcp_servers"] =
            serde_json::to_value(mcp_servers).unwrap_or_else(|_| serde_json::json!([]));
    }
    // Valence (personality encoding) goes under metadata.valence, matching
    // the ABW agent card shape (verified live 2026-08-04).
    if let Some(valence) = &req.valence {
        card["metadata"]["valence"] = serde_json::json!({
            "arousal": valence.arousal,
            "valence": valence.valence,
            "primary_affect": valence.primary_affect,
            "personality_traits": valence.personality_traits.clone().unwrap_or_default(),
        });
    }
    // Compound agents declare their dependency team.
    if req.dependencies_required.is_some() || req.dependencies_optional.is_some() {
        card["dependencies"] = serde_json::json!({
            "required": req.dependencies_required.clone().unwrap_or_default(),
            "optional": req.dependencies_optional.clone().unwrap_or_default(),
        });
    }
    card
}

/// Extract the agent's textual output from an ABW execute-agent response.
///
/// Fermi's `execute_agent_handler` returns the agent's narrative in
/// `metadata.reasoning` and structured findings in `evidence[]`; it does
/// not emit a top-level `response` field. Older ABW deploys used a
/// top-level `response` string. This helper tries the current shape first,
/// falls back to evidence summaries, then to the legacy `response` field,
/// so the extraction works against both current `main` and older deploys.
pub fn extract_execute_response(data: &serde_json::Value) -> Option<String> {
    // Current fermi shape: metadata.reasoning (the LLM's narrative output).
    if let Some(reasoning) = data
        .get("metadata")
        .and_then(|m| m.get("reasoning"))
        .and_then(|r| r.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(reasoning.to_string());
    }
    // Fallback: join evidence summaries + key findings (structured output).
    if let Some(evidence) = data.get("evidence").and_then(|e| e.as_array()) {
        let parts: Vec<String> = evidence
            .iter()
            .filter_map(|e| {
                let mut bits: Vec<String> = Vec::new();
                if let Some(s) = e
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    bits.push(s.to_string());
                }
                if let Some(kf) = e.get("key_findings").and_then(|v| v.as_array()) {
                    for f in kf {
                        if let Some(t) = f.as_str().filter(|s| !s.is_empty()) {
                            bits.push(t.to_string());
                        }
                    }
                }
                if bits.is_empty() {
                    None
                } else {
                    Some(bits.join("\n"))
                }
            })
            .collect();
        if !parts.is_empty() {
            return Some(parts.join("\n\n"));
        }
    }
    // Legacy: top-level `response` string (older ABW deploys).
    if let Some(resp) = data
        .get("response")
        .and_then(|r| r.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(resp.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{build_create_agent_card, extract_execute_response};
    use serde_json::json;

    #[test]
    fn extract_execute_response_prefers_metadata_reasoning() {
        let data = json!({
            "metadata": { "reasoning": "The market is trending up." },
            "evidence": [{ "summary": "s1", "key_findings": ["f1"] }],
            "status": "Success",
        });
        assert_eq!(
            extract_execute_response(&data),
            Some("The market is trending up.".to_string())
        );
    }

    #[test]
    fn extract_execute_response_falls_back_to_evidence() {
        let data = json!({
            "metadata": { "reasoning": null },
            "evidence": [
                { "summary": "Chip supply tightening", "key_findings": ["TSMC capacity full", "Price up 12%"] },
                { "summary": "Demand strong", "key_findings": [] },
            ],
        });
        let result = extract_execute_response(&data).unwrap();
        assert!(result.contains("Chip supply tightening"));
        assert!(result.contains("TSMC capacity full"));
        assert!(result.contains("Price up 12%"));
        assert!(result.contains("Demand strong"));
    }

    #[test]
    fn extract_execute_response_uses_legacy_response_field() {
        let data = json!({
            "response": "Legacy narrative output",
        });
        assert_eq!(
            extract_execute_response(&data),
            Some("Legacy narrative output".to_string())
        );
    }

    #[test]
    fn extract_execute_response_returns_none_when_empty() {
        let data = json!({
            "metadata": { "reasoning": "" },
            "evidence": [],
            "response": null,
        });
        assert_eq!(extract_execute_response(&data), None);
    }

    #[test]
    fn extract_execute_response_skips_empty_evidence_entries() {
        let data = json!({
            "evidence": [
                { "summary": "", "key_findings": [] },
                { "summary": "Real finding", "key_findings": [] },
            ],
        });
        assert_eq!(
            extract_execute_response(&data),
            Some("Real finding".to_string())
        );
    }

    #[test]
    fn extract_execute_response_prefers_reasoning_over_legacy_response() {
        let data = json!({
            "metadata": { "reasoning": "Current shape" },
            "response": "Legacy shape",
        });
        assert_eq!(
            extract_execute_response(&data),
            Some("Current shape".to_string())
        );
    }

    // ── Property-based tests ───────────────────────────────────────────
    //
    // These exercise the full input space (via proptest + hkask-test-harness)
    // rather than individual hand-picked inputs. They complement the
    // example-based tests above by verifying universal invariants.

    use hkask_test_harness::arb_json_value;
    use proptest::prelude::*;

    use crate::request_types::CreateAgentRequest;

    // P4 (panic_freedom): `extract_execute_response` must never panic on
    // arbitrary JSON — ABW responses are untrusted third-party payloads and
    // a panic would crash the MCP server process. No `prop_assume!` — every
    // generated JSON is accepted.
    proptest! {
        #[test]
        fn extract_execute_response_never_panics(data in arb_json_value()) {
            let _ = extract_execute_response(&data);
        }

        // P1 (invariant): when the function returns Some, the string is
        // never empty — an empty response is indistinguishable from no
        // response and would mislead the caller.
        #[test]
        fn extract_execute_response_never_returns_empty(data in arb_json_value()) {
            if let Some(text) = extract_execute_response(&data) {
                prop_assert!(!text.is_empty(), "extracted response must be non-empty, got: {:?}", text);
            }
        }
    }

    prop_compose! {
        fn arb_create_agent_request()
            (agent_name in any::<String>(),
             agent_type in any::<String>(),
             system_prompt in any::<String>(),
             description in any::<String>(),
             model in prop::option::of(any::<String>()),
             temperature in prop::option::of(any::<f64>()),
             tags in prop::option::of(prop::collection::vec(any::<String>(), 0..4)),
             sample_queries in prop::option::of(prop::collection::vec(any::<String>(), 0..4)),
             dependencies_required in prop::option::of(prop::collection::vec(any::<String>(), 0..4)),
             dependencies_optional in prop::option::of(prop::collection::vec(any::<String>(), 0..4)),
             mcp_tools in prop::option::of(prop::collection::vec(any::<String>(), 0..4)),
             skills in prop::option::of(prop::collection::vec(any::<String>(), 0..4)),
             visibility in prop::option::of(any::<String>()))
            -> CreateAgentRequest {
            CreateAgentRequest {
                agent_name,
                agent_type,
                system_prompt,
                description,
                model,
                temperature,
                tags,
                sample_queries,
                dependencies_required,
                dependencies_optional,
                mcp_tools,
                mcp_servers: None,
                skills,
                visibility,
                valence: None,
            }
        }
    }

    proptest! {
        // P4 (panic_freedom): the card builder must never panic on any
        // combination of request fields — it is the boundary between
        // operator input and the ABW REST API.
        #[test]
        fn build_create_agent_card_never_panics(req in arb_create_agent_request()) {
            let _ = build_create_agent_card(&req, "default-model");
        }

        // P1 (round_trip): scalar request fields appear verbatim in the
        // output card.
        #[test]
        fn build_create_agent_card_round_trips_scalars(req in arb_create_agent_request()) {
            let card = build_create_agent_card(&req, "default-model");
            prop_assert_eq!(card.get("agent_name").and_then(|v| v.as_str()), Some(req.agent_name.as_str()));
            prop_assert_eq!(card.get("agent_type").and_then(|v| v.as_str()), Some(req.agent_type.as_str()));
            prop_assert_eq!(card.get("system_prompt").and_then(|v| v.as_str()), Some(req.system_prompt.as_str()));
        }

        // P1 (invariant): capabilities.model is the request's model when
        // supplied, else the default.
        #[test]
        fn build_create_agent_card_model_default_fallback(req in arb_create_agent_request()) {
            let card = build_create_agent_card(&req, "default-model");
            let expected = req.model.as_deref().unwrap_or("default-model");
            let got = card.get("capabilities").and_then(|c| c.get("model")).and_then(|v| v.as_str());
            prop_assert_eq!(got, Some(expected));
        }

        // P1 (invariant): capabilities.mcp_tools defaults to an empty array
        // when the caller supplies None.
        #[test]
        fn build_create_agent_card_mcp_tools_defaults_empty(req in arb_create_agent_request()) {
            let card = build_create_agent_card(&req, "default-model");
            let tools = card.get("capabilities").and_then(|c| c.get("mcp_tools")).and_then(|v| v.as_array());
            let expected = req.mcp_tools.as_deref().unwrap_or(&[]);
            prop_assert_eq!(tools.map(|a| a.len()).unwrap_or(0), expected.len());
        }

        // P1 (invariant): the `dependencies` object is present iff the
        // caller supplied required or optional deps.
        #[test]
        fn build_create_agent_card_dependencies_presence(req in arb_create_agent_request()) {
            let card = build_create_agent_card(&req, "default-model");
            let has_deps = req.dependencies_required.is_some() || req.dependencies_optional.is_some();
            prop_assert_eq!(card.get("dependencies").is_some(), has_deps);
        }
    }
}
