//! Pure helpers for the ABW cloud swarm tools — the agent-card builder and the
//! execute-response extractor. Extracted from `cloud_swarm_tools.rs` so the tool
//! methods file stays focused on the `#[tool_router]` impl, and the pure
//! functions (which are property-tested without a live ABW connection) live
//! alongside their tests.
//!
//! Both helpers are `pub` so `cloud_swarm_tools.rs` and the `test_utils` module can
//! re-export them.

use crate::request_types::CreateAgentRequest;

/// Build the ABW agent-create JSON payload for `POST /api/agents` from a
/// `CreateAgentRequest`. Pure function — no HTTP, no auth, no side effects —
/// so it can be property-tested without a live ABW connection.
///
/// The payload is FLAT, mirroring fermi's `CreateAgentRequest`
/// (`src/handlers/agents.rs`): `agent_name`, `agent_type`, `description`,
/// `system_prompt`, `model`, `temperature`, `executor_type`, `tags`,
/// `visibility`, `accepts`, `produces`, and `mcp_tools` as
/// `[{name, description}]` objects. fermi's serde struct ignores unknown
/// fields, so the previous nested shape (`capabilities.model`,
/// `metadata.description`) was silently dropped — agents were created with
/// fermi's default model, no description, and no tools. Flat is the only
/// shape fermi's create reads.
///
/// Fields fermi's create does not accept but its update (`PUT /api/agents/:id`,
/// `AgentUpdate` in `agent-bestiary/memory/src/types.rs`) does — `mcp_servers`,
/// `valence`, `model_ladder`, `capability_gates` — are collected by
/// [`build_agent_update_payload`] for the follow-up PUT `swarm_create_agent`
/// issues after the create succeeds. Fields neither endpoint accepts are
/// reported by [`unsupported_create_fields`].
pub fn build_create_agent_card(
    req: &CreateAgentRequest,
    default_agent_model: &str,
) -> serde_json::Value {
    serde_json::json!({
        "agent_name": req.agent_name,
        "agent_type": req.agent_type,
        "description": req.description,
        "system_prompt": req.system_prompt,
        "model": req.model.clone().unwrap_or_else(|| default_agent_model.to_string()),
        "temperature": req.temperature.unwrap_or(0.3),
        // The panel only authors LLM-backed agents; fermi's default is "llm"
        // but the old payload declared it under a nested key fermi ignored.
        "executor_type": "llm",
        "tags": req.tags.clone().unwrap_or_default(),
        "visibility": req.visibility.clone().unwrap_or_else(|| "private".to_string()),
        "accepts": req.accepts.clone().unwrap_or_default(),
        "produces": req.produces.clone().unwrap_or_default(),
        // fermi's create validates each name against its dispatch table (or
        // a `server__tool` from a declared mcp_server) and rejects the whole
        // request otherwise — fail-closed with fermi's actionable error
        // beats the old silent drop under the ignored nested key.
        "mcp_tools": req
            .mcp_tools
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|name| serde_json::json!({ "name": name, "description": "" }))
            .collect::<Vec<_>>(),
    })
}

/// Build the follow-up `PUT /api/agents/:id` payload carrying the fields
/// fermi's create does not accept but its update does (`AgentUpdate`):
/// `mcp_servers`, `valence`, `model_ladder`, `capability_gates`. Returns an
/// empty object when the request supplied none of them — the caller skips
/// the PUT entirely.
///
/// `mcp_servers` keeps the None/Some([])/Some([...]) precedence contract
/// from `CreateAgentRequest`: `None` omits the field (fermi inherits from the
/// filesystem card via NULL column); `Some([])` is authoritative "no
/// servers"; `Some([...])` is authoritative replacement. Secrets are
/// referenced by `auth.secret_key` (agent owner's scoped secret store) —
/// never inlined in the card.
/// Map a `ValenceInput` to fermi's `AgentValence` object. fermi's struct
/// requires all four fields; fill neutral defaults for what the caller
/// omitted so fermi's card resolution never sees a partial object.
pub fn valence_payload(valence: &crate::request_types::ValenceInput) -> serde_json::Value {
    serde_json::json!({
        "primary_affect": valence
            .primary_affect
            .clone()
            .unwrap_or_else(|| "neutral".to_string()),
        "arousal": valence.arousal.unwrap_or(0.5),
        "valence": valence.valence.unwrap_or(0.5),
        "personality_traits": valence.personality_traits.clone().unwrap_or_default(),
    })
}

pub fn build_agent_update_payload(req: &CreateAgentRequest) -> serde_json::Value {
    let mut payload = serde_json::json!({});
    let obj = payload.as_object_mut().expect("just constructed object");
    if let Some(mcp_servers) = &req.mcp_servers {
        obj.insert(
            "mcp_servers".to_string(),
            serde_json::to_value(mcp_servers).unwrap_or_else(|_| serde_json::json!([])),
        );
    }
    if let Some(valence) = &req.valence {
        obj.insert("valence".to_string(), valence_payload(valence));
    }
    if let Some(ladder) = &req.model_ladder {
        // fermi ADR-011: per-tier model resolution. `ModelLadderRung`
        // serializes field-compatible with fermi's `ModelRung`
        // (tier/model/provider/note).
        obj.insert(
            "model_ladder".to_string(),
            serde_json::to_value(ladder).unwrap_or_else(|_| serde_json::json!([])),
        );
    }
    if let Some(gates) = &req.capability_gates {
        // fermi ADR-011: the card field is a map (tool → min tier); the tool
        // input is a list of `{tool, min_tier}` — convert.
        let map = serde_json::Map::from_iter(
            gates
                .iter()
                .map(|g| (g.tool.clone(), serde_json::json!(g.min_tier))),
        );
        obj.insert(
            "capability_gates".to_string(),
            serde_json::Value::Object(map),
        );
    }
    payload
}

/// Names of request fields the ABW API cannot store on a non-curated agent
/// at all — they live only on fermi's filesystem cards for curated agents;
/// neither `CreateAgentRequest` nor `AgentUpdate` accepts them. Returned so
/// `swarm_create_agent` can tell the caller their value was dropped instead
/// of losing it silently.
pub fn unsupported_create_fields(req: &CreateAgentRequest) -> Vec<&'static str> {
    let mut unsupported = Vec::new();
    if req.skills.as_ref().is_some_and(|s| !s.is_empty()) {
        unsupported.push("skills");
    }
    if req.sample_queries.as_ref().is_some_and(|s| !s.is_empty()) {
        unsupported.push("sample_queries");
    }
    if req.dependencies_required.is_some() || req.dependencies_optional.is_some() {
        unsupported.push("dependencies");
    }
    unsupported
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
    use super::*;
    use crate::request_types::{
        CapabilityGate, CreateAgentRequest, McpServerSpec, ModelLadderRung, ValenceInput,
    };

    fn base_request() -> CreateAgentRequest {
        CreateAgentRequest {
            agent_name: "test_agent".to_string(),
            agent_type: "research".to_string(),
            system_prompt: "You test.".to_string(),
            description: "A test agent".to_string(),
            model: Some("claude-haiku-4-5-20251001".to_string()),
            temperature: Some(0.2),
            tags: Some(vec!["test".to_string()]),
            sample_queries: Some(vec!["query".to_string()]),
            accepts: Some(vec!["text".to_string()]),
            produces: Some(vec!["analysis".to_string()]),
            dependencies_required: None,
            dependencies_optional: None,
            mcp_tools: Some(vec!["web_search".to_string()]),
            mcp_servers: None,
            skills: None,
            visibility: Some("private".to_string()),
            valence: None,
            model_ladder: None,
            capability_gates: None,
        }
    }

    /// fermi's `CreateAgentRequest` is FLAT — serde ignores unknown fields,
    /// so a nested `capabilities`/`metadata` payload silently drops every
    /// rich field. The built card must carry description/tags/model/
    /// temperature/mcp_tools at the top level and contain no nested keys.
    #[test]
    fn create_card_is_flat_fermi_shape() {
        let card = build_create_agent_card(&base_request(), "fallback-model");
        assert_eq!(card["agent_name"], "test_agent");
        assert_eq!(card["description"], "A test agent");
        assert_eq!(card["tags"], serde_json::json!(["test"]));
        assert_eq!(card["model"], "claude-haiku-4-5-20251001");
        assert_eq!(card["temperature"], 0.2);
        assert_eq!(card["executor_type"], "llm");
        assert_eq!(card["visibility"], "private");
        assert_eq!(card["accepts"], serde_json::json!(["text"]));
        assert_eq!(card["produces"], serde_json::json!(["analysis"]));
        // fermi's create validates mcp_tools as [{name, description,
        // input_schema}] objects — a bare string list fails its parse.
        assert_eq!(
            card["mcp_tools"],
            serde_json::json!([{ "name": "web_search", "description": "" }])
        );
        // The nested keys fermi ignores must not be present at all — their
        // presence is how the silent-drop regression reappears.
        assert!(card.get("capabilities").is_none());
        assert!(card.get("metadata").is_none());
    }

    #[test]
    fn create_card_defaults_model_and_temperature() {
        let mut req = base_request();
        req.model = None;
        req.temperature = None;
        let card = build_create_agent_card(&req, "default-model");
        assert_eq!(card["model"], "default-model");
        assert_eq!(card["temperature"], 0.3);
    }

    /// The PUT-only fields (mcp_servers, valence, model_ladder,
    /// capability_gates) go to the follow-up PUT, never the create POST.
    #[test]
    fn update_payload_carries_put_only_fields() {
        let mut req = base_request();
        req.mcp_servers = Some(vec![McpServerSpec {
            name: "research".to_string(),
            endpoint: "https://mcp.example.com".to_string(),
            tool_allowlist: vec![],
            timeout_secs: None,
            auth: None,
        }]);
        req.valence = Some(ValenceInput {
            arousal: Some(0.7),
            valence: None,
            primary_affect: None,
            personality_traits: None,
        });
        req.model_ladder = Some(vec![ModelLadderRung {
            tier: "free".to_string(),
            model: "small-model".to_string(),
            provider: "anthropic".to_string(),
            note: None,
        }]);
        req.capability_gates = Some(vec![CapabilityGate {
            tool: "web_search".to_string(),
            min_tier: "standard".to_string(),
        }]);
        let update = build_agent_update_payload(&req);
        assert!(update["mcp_servers"].is_array());
        // fermi's AgentValence requires all four fields — omitted caller
        // fields are filled with neutral defaults, not left partial.
        assert_eq!(update["valence"]["arousal"], 0.7);
        assert_eq!(update["valence"]["valence"], 0.5);
        assert_eq!(update["valence"]["primary_affect"], "neutral");
        assert_eq!(
            update["valence"]["personality_traits"],
            serde_json::json!([])
        );
        assert!(update["model_ladder"].is_array());
        // fermi's card field is a map (tool → min tier), not a list.
        assert_eq!(
            update["capability_gates"],
            serde_json::json!({ "web_search": "standard" })
        );
        // The create payload must not carry any of these.
        let card = build_create_agent_card(&req, "fallback-model");
        assert!(card.get("mcp_servers").is_none());
        assert!(card.get("valence").is_none());
        assert!(card.get("model_ladder").is_none());
        assert!(card.get("capability_gates").is_none());
    }

    #[test]
    fn update_payload_empty_when_no_put_only_fields() {
        let update = build_agent_update_payload(&base_request());
        assert!(update.as_object().unwrap().is_empty());
    }

    /// Fields the ABW API cannot store at all must be reported, not lost.
    #[test]
    fn unsupported_fields_are_listed() {
        let mut req = base_request();
        req.skills = Some(vec!["some_skill".to_string()]);
        req.dependencies_required = Some(vec!["dep".to_string()]);
        // base_request carries sample_queries, so all three are reported.
        assert_eq!(
            unsupported_create_fields(&req),
            vec!["skills", "sample_queries", "dependencies"]
        );
        // A request with none of the unsupported fields reports none.
        let mut clean = base_request();
        clean.sample_queries = None;
        assert!(unsupported_create_fields(&clean).is_empty());
    }
}
