//! Local knowledge tools — semantic-memory search and the local analogs of
//! the ABW generate tools (prompt/ontology). Split from `hkask_mcp_swarm.rs`
//! (M2). These read the operator's consolidated `hkask-memory` and use the
//! local `InferencePort`; no ABW calls.
use crate::SwarmServer;
use crate::error::map_local_swarm_error;
use crate::local_knowledge;
use crate::request_types::*;
use hkask_mcp_server::server::{McpToolError, execute_tool};
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "swarm_search_knowledge_local", async {
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
        })
        .await
    }

    /// Recall prior swarm turns from the shared knowledgebase by semantic
    /// similarity (the episodic-memory complement to `swarm_search_knowledge_local`,
    /// which searches the EAV graph). By default spans ALL agents and ALL
    /// swarms — a turn any agent produced is retrievable. Pass `agent_name`
    /// to scope the recall to one agent (fermi parity: its per-agent KG is
    /// searched per-agent). Degrades to a `memory_unconfigured`
    /// note when the store cannot be opened or the query cannot be embedded.
    #[tool(
        description = "Recall prior swarm turns from the shared swarm memory by semantic similarity to a query. Spans all agents and all swarms by default (one shared knowledgebase); pass agent_name to scope the recall to one agent's turns (the per-agent analog of fermi's per-agent KG search). Returns the most similar past turns (task + response + model + producing agent). The episodic-memory complement to swarm_search_knowledge_local (which searches the EAV graph). Degrades to an empty result with a memory_unconfigured note when the store cannot be opened or the query cannot be embedded."
    )]
    pub(crate) async fn swarm_recall_local(
        &self,
        parameters: Parameters<RecallLocalRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "swarm_recall_local", async {
            let req = parameters.0;
            if req.query.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must be non-empty".to_string(),
                ));
            }
            let limit = req.limit.unwrap_or(10).clamp(1, 50);
            let agent_scope = req
                .agent_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty());
            let runtime = self
                .local_runtime
                .get_or_init()
                .await
                .map_err(map_local_swarm_error)?;
            let inference = runtime.inference();
            match local_knowledge::recall_turns(
                &self.local_memory,
                &inference,
                &req.query,
                limit,
                agent_scope,
                hkask_inference::model_constants::embedding_model().as_deref(),
            )
            .await
            {
                Ok(turns) => Ok(serde_json::json!({
                    "turns": turns,
                    "source": "local_episodic_memory",
                    "scope": agent_scope.unwrap_or("all_agents"),
                    "count": turns.len(),
                    "note": "",
                })),
                Err(reason) => Ok(serde_json::json!({
                    "turns": [],
                    "source": "local_episodic_memory",
                    "scope": agent_scope.unwrap_or("all_agents"),
                    "count": 0,
                    "note": format!("memory_unconfigured: {reason}"),
                })),
            }
        })
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "swarm_generate_prompt_local", async {
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
                local_knowledge::agent_memory_seed(&self.local_memory, &req.agent_name, 20).await;
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
        })
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "swarm_generate_ontology_local", async {
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
}
