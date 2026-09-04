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

/// Fallback description for an agent with an empty description field.
/// Shared by `to_a2a_card` (per-agent card) and `build_gateway_card`
/// (the HTTP gateway card) so the two don't drift on the fallback shape.
pub fn description_or_fallback(card: &LocalAgentCard) -> String {
    if card.description.is_empty() {
        format!("Local agent: {}", card.agent_id)
    } else {
        card.description.clone()
    }
}

/// A schema ID contains `/` and is not a standard MIME type prefix.
/// Mirrors fermi's `a2a_card::is_schema_id` — e.g. `"scro/bom-query/1"`,
/// `"kask_simops/action_block"`.
pub fn is_schema_id(s: &str) -> bool {
    s.contains('/')
        && !s.starts_with("text/")
        && !s.starts_with("application/")
        && !s.starts_with("image/")
        && !s.starts_with("audio/")
        && !s.starts_with("video/")
}

/// Derive A2A MIME-type input/output modes from port labels, mirroring
/// fermi's `a2a_card::derive_modes`: schema-ID ports (`"scro/bom-query/1"`)
/// mean structured JSON I/O; free-text labels mean plain text; empty ports
/// are permissive (both).
pub fn derive_modes(accepts: &[String], produces: &[String]) -> (Vec<String>, Vec<String>) {
    let input_modes = if accepts.is_empty() {
        vec!["text/plain".to_string(), "application/json".to_string()]
    } else if accepts.iter().any(|a| is_schema_id(a)) {
        vec!["application/json".to_string()]
    } else {
        vec!["text/plain".to_string()]
    };
    let output_modes = if produces.is_empty() {
        vec!["text/plain".to_string(), "application/json".to_string()]
    } else if produces.iter().any(|p| is_schema_id(p)) {
        vec!["application/json".to_string()]
    } else {
        vec!["text/plain".to_string()]
    };
    (input_modes, output_modes)
}

/// Build an A2A `AgentSkill` from a local agent card — ONE skill per agent,
/// mirroring fermi's `a2a_card::build_skill`: human-readable title-cased
/// name, the card's tags, up to 5 `sample_queries` as `examples` (the
/// authoritative input shape for A2A callers), and input/output modes
/// derived from the port labels. Shared by `to_a2a_card` (per-agent card)
/// and `build_gateway_card` (one skill per roster agent).
pub fn to_a2a_skill(card: &LocalAgentCard) -> AgentSkill {
    let (input_modes, output_modes) = derive_modes(&card.accepts, &card.produces);
    AgentSkill {
        id: card.agent_id.clone(),
        name: title_case_slug(&card.agent_id),
        description: description_or_fallback(card),
        tags: card.tags.clone(),
        examples: if card.sample_queries.is_empty() {
            None
        } else {
            Some(card.sample_queries.iter().take(5).cloned().collect())
        },
        input_modes: Some(input_modes),
        output_modes: Some(output_modes),
        security_requirements: None,
    }
}

/// Human-readable name from a snake_case slug: underscores to spaces,
/// title-case each word. Mirrors fermi's `a2a_card::build_skill` naming so a
/// local agent and its ABW original render the same skill name.
fn title_case_slug(slug: &str) -> String {
    slug.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert a `LocalAgentCard` to an A2A `AgentCard`. The `url` field in
/// `supported_interfaces` is set to a placeholder (the in-process transport
/// doesn't use HTTP) — when an HTTP binding is added, this becomes the
/// agent's real endpoint URL. One skill per agent (fermi's shape), with
/// default I/O modes derived from the port labels.
pub fn to_a2a_card(card: &LocalAgentCard, base_url: &str) -> AgentCard {
    let (input_modes, output_modes) = derive_modes(&card.accepts, &card.produces);

    AgentCard {
        name: if card.display_name.is_empty() {
            card.agent_id.clone()
        } else {
            card.display_name.clone()
        },
        description: description_or_fallback(card),
        version: card.version.clone(),
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
        default_input_modes: input_modes,
        default_output_modes: output_modes,
        skills: vec![to_a2a_skill(card)],
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
pub fn message_from_text(text: &str, context_id: Option<String>) -> Message {
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
pub fn task_from_response(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_registry::{LocalAgentCapabilities, LocalAgentCard, LocalAgentDependencies};

    fn local_card(accepts: Vec<String>, produces: Vec<String>) -> LocalAgentCard {
        LocalAgentCard {
            agent_id: "supply_chain_oracle".to_string(),
            agent_type: "research".to_string(),
            description: "Prices bills of materials.".to_string(),
            display_name: "Supply Chain Oracle".to_string(),
            accepts,
            produces,
            dependencies: LocalAgentDependencies::default(),
            capabilities: LocalAgentCapabilities::default(),
            cloud_swarm_id: None,
            tags: vec!["supply-chain".to_string()],
            visibility: "private".to_string(),
            sample_queries: vec!["Price 50g Ashwagandha".to_string()],
            valence: None,
            version: "1.2.0".to_string(),
        }
    }

    #[test]
    fn schema_ids_are_detected() {
        assert!(is_schema_id("scro/bom-query/1"));
        assert!(is_schema_id("kask_simops/action_block"));
        // MIME types are not schema IDs.
        assert!(!is_schema_id("text/plain"));
        assert!(!is_schema_id("application/json"));
        // Free-text labels have no slash.
        assert!(!is_schema_id("forecast-question"));
    }

    /// Mirrors fermi's `derive_modes`: schema-ID ports mean structured JSON
    /// I/O; free-text labels mean plain text; empty ports are permissive.
    #[test]
    fn derive_modes_matches_fermi() {
        let (input, output) = derive_modes(
            &["scro/bom-query/1".to_string()],
            &["scro/bom-response/1".to_string()],
        );
        assert_eq!(input, vec!["application/json".to_string()]);
        assert_eq!(output, vec!["application/json".to_string()]);

        let (input, output) = derive_modes(&["query".to_string()], &["forecast".to_string()]);
        assert_eq!(input, vec!["text/plain".to_string()]);
        assert_eq!(output, vec!["text/plain".to_string()]);

        let (input, output) = derive_modes(&[], &[]);
        assert_eq!(
            input,
            vec!["text/plain".to_string(), "application/json".to_string()]
        );
        assert_eq!(
            output,
            vec!["text/plain".to_string(), "application/json".to_string()]
        );
    }

    /// fermi's A2A card carries ONE skill per agent (with examples from
    /// sample_queries), not one skill per `accepts` entry.
    #[test]
    fn card_has_one_skill_with_examples() {
        let card = local_card(
            vec!["scro/bom-query/1".to_string()],
            vec!["scro/bom-response/1".to_string()],
        );
        let a2a_card = to_a2a_card(&card, "local://swarm/agents");
        assert_eq!(a2a_card.skills.len(), 1);
        let skill = &a2a_card.skills[0];
        assert_eq!(skill.id, "supply_chain_oracle");
        // Title-cased human-readable name, fermi's build_skill naming.
        assert_eq!(skill.name, "Supply Chain Oracle");
        assert_eq!(skill.tags, vec!["supply-chain".to_string()]);
        assert_eq!(
            skill.examples.as_deref(),
            Some(&["Price 50g Ashwagandha".to_string()][..])
        );
        // Schema-ID ports derive application/json modes on both the card
        // and the skill.
        assert_eq!(
            a2a_card.default_input_modes,
            vec!["application/json".to_string()]
        );
        assert_eq!(
            a2a_card.default_output_modes,
            vec!["application/json".to_string()]
        );
        assert_eq!(
            skill.input_modes.as_deref(),
            Some(&["application/json".to_string()][..])
        );
        // The local gateway supports neither streaming nor push — the
        // card must keep saying so (honest capability flags).
        assert_eq!(a2a_card.capabilities.streaming, Some(false));
        assert_eq!(a2a_card.capabilities.push_notifications, Some(false));
        // The card's semver travels to the A2A card (fermi parity — the
        // cloud A2A card carries the agent's version, not a constant).
        assert_eq!(a2a_card.version, "1.2.0");
    }

    #[test]
    fn free_text_ports_derive_text_modes() {
        let card = local_card(vec!["query".to_string()], vec!["forecast".to_string()]);
        let a2a_card = to_a2a_card(&card, "local://swarm/agents");
        assert_eq!(a2a_card.default_input_modes, vec!["text/plain".to_string()]);
        assert_eq!(
            a2a_card.default_output_modes,
            vec!["text/plain".to_string()]
        );
    }
}
