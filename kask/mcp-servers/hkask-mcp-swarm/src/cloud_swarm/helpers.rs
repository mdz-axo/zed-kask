//! Pure helpers for the ABW cloud swarm tools — the agent-card builder and the
//! execute-response extractor. Extracted from `cloud_swarm_tools.rs` so the tool
//! methods file stays focused on the `#[tool_router]` impl, and the pure
//! functions (which are property-tested without a live ABW connection) live
//! alongside their tests.
//!
//! Both helpers are `pub` so `cloud_swarm_tools.rs` and the `test_utils` module can
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
///   may call as a client, fermi v0.16.1 / mig-177 (fermi v0.16.1)). Injected only when the
///   caller supplies a value: `None` omits the field (fermi inherits from the
///   filesystem card via NULL column); `Some([])` is authoritative "no
///   servers"; `Some([...])` is authoritative replacement. Secrets are
///   referenced by `auth.secret_key` (agent owner's scoped secret store) —
///   never inlined in the card.
/// - `metadata.valence` (personality encoding, fermi v0.16.x).
/// - `dependencies` (compound agent team, fermi v0.16.x).
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
    // fermi v0.16.1 (mig-177 (fermi v0.16.1)): inbound MCP servers. Inject only when the
    // caller supplied a value. See the struct doc on `CreateAgentRequest`
    // for the None/Some([])/Some([...]) precedence contract.
    if let Some(mcp_servers) = &req.mcp_servers {
        card["capabilities"]["mcp_servers"] =
            serde_json::to_value(mcp_servers).unwrap_or_else(|_| serde_json::json!([]));
    }
    // Valence (personality encoding) goes under metadata.valence, matching
    // the ABW agent card shape (verified live 2026-08-13).
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
    // Model ladder (fermi ADR-011): per-tier model resolution. Injected only
    // when the caller supplies a value — `None` omits the field so fermi
    // falls back to the single `model` field.
    if let Some(ladder) = &req.model_ladder {
        card["model_ladder"] =
            serde_json::to_value(ladder).unwrap_or_else(|_| serde_json::json!([]));
    }
    // Capability gates (fermi ADR-011): per-tool minimum tier. Injected only
    // when the caller supplies a value.
    if let Some(gates) = &req.capability_gates {
        card["capability_gates"] =
            serde_json::to_value(gates).unwrap_or_else(|_| serde_json::json!({}));
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
