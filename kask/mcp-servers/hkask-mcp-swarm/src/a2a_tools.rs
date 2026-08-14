//! A2A (Agent2Agent) protocol tools — in-process message dispatch and Agent
//! Card discovery. Split from `hkask_mcp_swarm.rs` (M2). No HTTP server — the
//! MCP tool dispatch IS the A2A transport; agents declare these tools in
//! `capabilities.mcp_tools` to communicate with each other.
use crate::SwarmServer;
use crate::a2a;
use crate::error::map_local_swarm_error;
use crate::request_types::*;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = a2a_router, vis = "pub")]
impl SwarmServer {
    /// Send an A2A (Agent2Agent) protocol message to a local agent. Wraps the
    /// message in A2A types (Message → Task → Artifact) and dispatches through
    /// the existing in-process `LocalSwarmRuntime::delegate`. The response is
    /// returned as an A2A Task with the agent's output as a text Artifact. No
    /// HTTP server — the MCP tool dispatch IS the A2A transport. Agents can
    /// communicate with each other by declaring this tool in their
    /// `capabilities.mcp_tools`.
    #[tool(
        description = "Send an A2A (Agent2Agent) protocol message to a local agent. Wraps in A2A types (Message/Task/Artifact) and dispatches in-process. Returns an A2A Task with the agent's response as a text Artifact. No HTTP — MCP tool dispatch is the transport. Agents declare this tool in mcp_tools to communicate with each other."
    )]
    pub(crate) async fn swarm_a2a_send(&self, parameters: Parameters<A2aSendRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_a2a_send",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.agent_name.trim().is_empty() || req.message.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "agent_name and message must be non-empty".to_string(),
                    ));
                }
                let runtime = self
                    .local_runtime
                    .get_or_init()
                    .await
                    .map_err(map_local_swarm_error)?;
                let agent = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "agent '{}' not found in local registry",
                        req.agent_name
                    ))
                })?;
                let ceiling = self.client.config().max_credits_per_dispatch;
                let result = runtime
                    .delegate(&agent, &req.message, req.credits_authorized, ceiling)
                    .await
                    .map_err(map_local_swarm_error)?;
                let mut task = a2a::task_from_response(
                    &result.response,
                    req.context_id.clone(),
                    &result.model,
                    result.tokens_used,
                    result.cost,
                );
                // Record the inbound user message in the task history. This is the
                // consumer of `a2a::message_from_text` (the in-process counterpart
                // of the HTTP gateway's inbound `Message`) — without it the helper
                // is dead code.
                task.history = Some(vec![a2a::message_from_text(
                    &req.message,
                    req.context_id.clone(),
                )]);
                Ok(serde_json::to_value(&task).unwrap_or_else(
                    |_| serde_json::json!({ "error": "failed to serialize A2A task" }),
                ))
            },
        )
        .await
    }

    /// Get the A2A Agent Card for a local agent (or all local agents).
    #[tool(
        description = "Get the A2A (Agent2Agent) Agent Card for a local agent, or all local agents if agent_name is omitted. The card describes the agent's capabilities, skills, and supported interface. A2A-compliant discovery."
    )]
    pub(crate) async fn swarm_a2a_card(&self, parameters: Parameters<A2aCardRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_a2a_card",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                let base_url = "local://swarm/agents".to_string();
                match req.agent_name {
                    Some(name) if !name.trim().is_empty() => {
                        let card = self.local_registry.get(&name).ok_or_else(|| {
                            McpToolError::not_found(format!(
                                "agent '{}' not found in local registry",
                                name
                            ))
                        })?;
                        let a2a_card = a2a::to_a2a_card(&card, &base_url);
                        Ok(serde_json::to_value(&a2a_card).unwrap_or_else(
                            |_| serde_json::json!({ "error": "failed to serialize agent card" }),
                        ))
                    }
                    _ => {
                        let cards = self.local_registry.list();
                        let a2a_cards: Vec<_> = cards
                            .iter()
                            .map(|c| a2a::to_a2a_card(c, &base_url))
                            .collect();
                        Ok(serde_json::json!({
                            "count": a2a_cards.len(),
                            "agent_cards": a2a_cards,
                        }))
                    }
                }
            },
        )
        .await
    }
}
