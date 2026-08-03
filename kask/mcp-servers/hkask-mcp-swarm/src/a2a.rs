//! A2A (Agent2Agent) protocol integration — minimal in-process transport.
//!
//! Uses the `a2a-lf` crate's data model types (AgentCard, Task, Message, Part,
//! Artifact) to wrap the existing `LocalSwarmRuntime::delegate` in A2A-compliant
//! types. No HTTP server is required — the MCP tool dispatch path IS the A2A
//! transport. Agents communicate by calling `swarm_a2a_send` as an MCP tool,
//! which internally creates an A2A Message, delegates to the target agent, and
//! returns an A2A Task with the response as an Artifact.
//!
//! This is the minimal A2A: protocol-compliant types over the existing in-process
//! transport. An HTTP binding can be added later for cross-machine communication —
//! the types are already wire-compatible.

use crate::local_registry::LocalAgentCard;
use a2a::{
    AgentCapabilities, AgentCard, AgentSkill, Artifact, Message, Part, Role, Task, TaskState,
    TaskStatus, new_artifact_id, new_context_id, new_task_id,
};

/// Convert a `LocalAgentCard` to an A2A `AgentCard`. The `url` field in
/// `supported_interfaces` is set to a placeholder (the in-process transport
/// doesn't use HTTP) — when an HTTP binding is added, this becomes the
/// agent's real endpoint URL.
pub(crate) fn to_a2a_card(card: &LocalAgentCard, base_url: &str) -> AgentCard {
    let skills = card
        .accepts
        .iter()
        .enumerate()
        .map(|(i, _accept)| AgentSkill {
            id: format!("{}-{}", card.agent_id, i),
            name: format!("{} capability", card.agent_type),
            description: card.description.clone(),
            tags: vec![card.agent_type.clone()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        })
        .collect::<Vec<_>>();

    AgentCard {
        name: card.agent_id.clone(),
        description: if card.description.is_empty() {
            format!("Local agent: {}", card.agent_id)
        } else {
            card.description.clone()
        },
        version: "1.0.0".to_string(),
        supported_interfaces: vec![a2a::AgentInterface {
            url: format!("{}/{}", base_url.trim_end_matches('/'), card.agent_id),
            protocol_binding: a2a::TRANSPORT_PROTOCOL_HTTP_JSON.to_string(),
            protocol_version: a2a::VERSION.to_string(),
            tenant: None,
        }],
        capabilities: AgentCapabilities {
            streaming: Some(false),
            push_notifications: Some(false),
            extensions: None,
            extended_agent_card: Some(false),
        },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills,
        provider: None,
        documentation_url: None,
        icon_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Create an A2A `Message` from a text string (the `Role::User` perspective —
/// the sender is the caller, not the agent).
pub(crate) fn message_from_text(text: &str, context_id: Option<String>) -> Message {
    Message {
        message_id: a2a::new_message_id(),
        context_id,
        task_id: None,
        role: Role::User,
        parts: vec![Part::text(text)],
        metadata: None,
        extensions: None,
        reference_task_ids: None,
    }
}

/// Wrap a text response in an A2A `Task` with a completed status and a single
/// `Artifact` containing the response as a text `Part`.
pub(crate) fn task_from_response(
    response_text: &str,
    context_id: Option<String>,
    model: &str,
    tokens_used: i64,
    cost: i64,
) -> Task {
    let ctx = context_id.unwrap_or_else(new_context_id);
    Task {
        id: new_task_id(),
        context_id: ctx,
        status: TaskStatus {
            state: TaskState::Completed,
            message: None,
            timestamp: Some(chrono::Utc::now()),
        },
        artifacts: Some(vec![Artifact {
            artifact_id: new_artifact_id(),
            name: Some("response".to_string()),
            description: Some(format!(
                "model={}, tokens={}, cost={}",
                model, tokens_used, cost
            )),
            parts: vec![Part::text(response_text)],
            metadata: None,
            extensions: None,
        }]),
        history: None,
        metadata: None,
    }
}
