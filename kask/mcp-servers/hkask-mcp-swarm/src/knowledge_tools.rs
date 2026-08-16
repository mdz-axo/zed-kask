//! Local knowledge tools — semantic-memory search and the local analogs of
//! the ABW generate tools (prompt/ontology). Split from `hkask_mcp_swarm.rs`
//! (M2). These read the operator's consolidated `hkask-memory` and use the
//! local `InferencePort`; no ABW calls.
use crate::SwarmServer;
use crate::error::map_local_swarm_error;
use crate::local_knowledge;
use crate::request_types::*;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = knowledge_router, vis = "pub")]
impl SwarmServer {
    /// Search a local agent's prefix-scoped semantic memory (the kask analog of
    /// ABW `swarm_search_knowledge`). Returns matching knowledge fragments
    /// (entity-attribute-value triples) from the operator's consolidated
    /// `hkask-memory`. No ABW calls. Degrades to an empty result with a
    /// `memory_unconfigured` note when the store cannot be opened (e.g., a
    /// passphrase mismatch with an existing DB).
    #[tool(
        description = "Search a local agent's prefix-scoped semantic memory (the local analog of ABW swarm_search_knowledge). Returns matching knowledge fragments (entity-attribute-value triples) from the operator's consolidated hkask-memory. No ABW calls. Degrades to an empty result with a memory_unconfigured note when the store cannot be opened (e.g., a passphrase mismatch with an existing DB)."
    )]
    pub(crate) async fn swarm_search_knowledge_local(
        &self,
        parameters: Parameters<SearchKnowledgeLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_search_knowledge_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.agent_name.trim().is_empty() || req.query.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "agent_name and query must be non-empty".to_string(),
                    ));
                }
                let limit = req.limit.unwrap_or(10).clamp(1, 50);
                match local_knowledge::search_agent_knowledge(
                    &self.local_memory,
                    &req.agent_name,
                    &req.query,
                    limit,
                )
                .await
                {
                    Ok(fragments) => Ok(serde_json::json!({
                        "fragments": fragments,
                        "source": "local_semantic_memory",
                        "agent_name": req.agent_name,
                        "note": "",
                    })),
                    Err(reason) => Ok(serde_json::json!({
                        "fragments": [],
                        "source": "local_semantic_memory",
                        "agent_name": req.agent_name,
                        "note": format!("memory_unconfigured: {reason}"),
                    })),
                }
            },
        )
        .await
    }

    /// Generate a system prompt for a local agent from a description (the kask
    /// analog of ABW `swarm_generate_prompt`). Authoring aid — read-only. Uses
    /// the local `InferencePort` (no ABW); seeded with the agent's consolidated
    /// memory when available.
    #[tool(
        description = "Generate a system prompt for a local agent from a description (the local analog of ABW swarm_generate_prompt). Authoring aid — read-only, spends nothing. Uses the local InferencePort (no ABW); optionally seeded with the agent's consolidated memory."
    )]
    pub(crate) async fn swarm_generate_prompt_local(
        &self,
        parameters: Parameters<GeneratePromptLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_generate_prompt_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.description.trim().is_empty() || req.agent_name.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "description and agent_name must be non-empty".to_string(),
                    ));
                }
                let runtime = self
                    .local_runtime
                    .get_or_init()
                    .await
                    .map_err(map_local_swarm_error)?;
                let agent_type = req.agent_type.unwrap_or_else(|| "research".to_string());
                let seed =
                    local_knowledge::agent_memory_seed(&self.local_memory, &req.agent_name, 20)
                        .await;
                let seeded = !seed.is_empty();
                let seed_block = if seeded {
                    format!("{seed}\n\n")
                } else {
                    String::new()
                };
                let prompt = format!(
                    "You are authoring a system prompt for a new hKask local agent.\n\
                 Agent name: {agent_name}\nAgent type: {agent_type}\n\
                 Description: {description}\n\n{seed_block}\
                 Write a complete, focused system prompt that defines the agent's role, \
                 inputs, outputs, and constraints. Return ONLY the system prompt text, \
                 no preamble or explanation.",
                    agent_name = req.agent_name,
                    agent_type = agent_type,
                    description = req.description,
                    seed_block = seed_block,
                );
                let inference = runtime.inference();
                let text = local_knowledge::one_shot_generate(&inference, &prompt, 0.4)
                    .await
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::json!({
                    "prompt": text,
                    "raw": serde_json::json!({
                        "agent_name": req.agent_name,
                        "agent_type": agent_type,
                        "seeded": seeded,
                    }),
                }))
            },
        )
        .await
    }

    /// Generate a seed ontology (Mermaid ER diagram) for a knowledge domain
    /// (the kask analog of ABW `swarm_generate_ontology`). Authoring aid —
    /// read-only. Uses the local `InferencePort`; optionally seeded with an
    /// agent's semantic-memory graph.
    #[tool(
        description = "Generate a seed ontology (Mermaid ER diagram) for a knowledge domain (the local analog of ABW swarm_generate_ontology). Authoring aid — read-only. Uses the local InferencePort; optionally seeded with an agent's semantic-memory graph."
    )]
    pub(crate) async fn swarm_generate_ontology_local(
        &self,
        parameters: Parameters<GenerateOntologyLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_generate_ontology_local", Some(hkask_bridge_ontology::pko::PROCEDURE), async {
            let req = parameters.0;
            if req.domain_description.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "domain_description must be non-empty".to_string(),
                ));
            }
            let runtime = self.local_runtime.get_or_init().await.map_err(map_local_swarm_error)?;
            let seed = match req.agent_name.as_deref() {
                Some(name) if !name.trim().is_empty() => {
                    local_knowledge::agent_memory_seed(&self.local_memory, name, 30).await
                }
                _ => String::new(),
            };
            let seeded = !seed.is_empty();
            let seed_block = if seeded { format!("{seed}\n\n") } else { String::new() };
            let prompt = format!(
                "You are authoring a seed ontology (entity-relationship model) for a knowledge domain.\n\
                 Domain: {domain}\n\n{seed_block}\
                 Produce a Mermaid erDiagram that captures the core entities, their attributes, \
                 and the relationships between them for this domain. Return ONLY the mermaid block \
                 inside a fenced code block, no preamble.",
                domain = req.domain_description,
                seed_block = seed_block,
            );
            let inference = runtime.inference();
            let text = local_knowledge::one_shot_generate(&inference, &prompt, 0.3)
                .await
                .map_err(map_local_swarm_error)?;
            Ok(serde_json::json!({
                "ontology": text,
                "raw": serde_json::json!({
                    "domain_description": req.domain_description,
                    "seeded": seeded,
                }),
            }))
        })
        .await
    }

    /// Query the grounding trend for an agent (or the whole swarm when
    /// `agent_name` is empty). Reads the stigmergy trail written by
    /// `swarm_delegate_local` — the per-delegation grounding status
    /// annotations. Answers the paper's §4.1 question: "is this getting
    /// better?"
    ///
    /// This is the swarm-server-side trend (stigmergy trail). The
    /// kanban-side trend (`kanban_grounding_trend`) reads
    /// `delegate_result` records from the kanban DB and is the primary
    /// trend for grounded delegations. This tool covers
    /// `swarm_delegate_local` delegations that do not go through
    /// `spawn_via_local_runtime` and therefore have no grounding contract —
    /// they show up as `delegations_without_contract`, which is the
    /// coverage gap signal (paper §6).
    ///
    /// Returns `Err` when the memory store is unavailable (the `.rules`
    /// broken-feedback-loop trap: a DB outage must not collapse to an empty
    /// trend, which would read as "no deviation").
    #[tool(
        description = "Query the grounding trend for an agent (or the whole swarm when agent_name is empty). Reads the stigmergy trail written by swarm_delegate_local. Answers the paper's §4.1 question: is this getting better? Returns Err when the memory store is unavailable (a DB outage must not collapse to an empty trend)."
    )]
    pub(crate) async fn swarm_grounding_trend(
        &self,
        parameters: Parameters<GroundingTrendRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_grounding_trend",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                let agent_name = req.agent_name.as_deref().unwrap_or("").trim();
                let limit = req.limit.unwrap_or(100).clamp(1, 1000);
                match local_knowledge::grounding_trend(
                    &self.local_memory,
                    agent_name,
                    limit,
                )
                .await
                {
                    Ok(trend) => Ok(serde_json::json!({
                        "trend": trend,
                        "agent_name": if agent_name.is_empty() { "*" } else { agent_name },
                        "source": "local_stigmergy_trail",
                        "note": "swarm-server-side trend (stigmergy). For grounded delegations via kanban_task_spawn, use kanban_grounding_trend.",
                    })),
                    Err(reason) => {
                        // The `.rules` broken-feedback-loop trap: a DB
                        // outage must not collapse to an empty trend.
                        // Surface as a typed error, not a silent empty
                        // result.
                        Err(McpToolError::unavailable(format!(
                            "grounding trend query failed (swarm memory unavailable): {reason}"
                        )))
                    }
                }
            },
        )
        .await
    }
}
