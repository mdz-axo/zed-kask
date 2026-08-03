//! Request types for the swarm MCP tools — extracted from the server
//! root so the tool-handler file holds only the `#[tool]` impl block.
//!
//! These are plain `#[derive(Debug, Deserialize, JsonSchema)]` data types
//! with no `#[tool]` or `#[tool_router]` macro involvement, so they are
//! safe to relocate (unlike the tool handlers, which rmcp requires in a
//! single `impl` block — see the `tool_router` macro in `rmcp-macros`).

use schemars::JsonSchema;
use serde::Deserialize;

// ── Request types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListAgentsRequest {
    /// Filter by agent type (e.g. "research", "creative", "meta"). Optional.
    pub agent_type: Option<String>,
    /// Filter by tag. Optional.
    pub tag: Option<String>,
    /// Maximum number of agents to return. Default 50.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetSwarmRequest {
    /// Workspace ID (UUID) or slug. Lists workspaces when omitted.
    pub workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExecuteAgentRequest {
    /// Agent name (e.g. "market_analyst").
    pub agent_name: String,
    /// The query or task for the agent.
    pub query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetAgentRequest {
    /// Agent name or id.
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListAppsRequest {
    /// Max apps to return. Default 50.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct OntologyTemplatesRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct HireCostRequest {
    /// Agent name (e.g. "social_media_studio").
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RequestConsentRequest {
    /// The action to authorize: "hire" or "delegate".
    pub action: String,
    /// The target: agent name (hire) or workspace id (delegate).
    pub target: String,
    /// The credit ceiling the operator is authorizing.
    pub credits_authorized: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct HireRequest {
    /// Workspace (swarm) id to hire into.
    pub workspace_id: String,
    /// Agent name to hire.
    pub agent_name: String,
    /// Whether to also hire the agent's optional dependency team.
    pub include_optional: Option<bool>,
    /// The consent token from `swarm_request_consent` (action "hire",
    /// target = agent_name). Required — the spend is refused without it.
    pub consent_token: String,
    /// The credit cost the operator authorized (from `swarm_hire_cost`).
    pub credits_authorized: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DelegateRequest {
    /// Workspace (swarm) id containing the agent.
    pub workspace_id: String,
    /// Agent name to delegate to (the @mention target).
    pub agent_name: String,
    /// The task for the agent.
    pub task: String,
    /// The consent token from `swarm_request_consent` (action "delegate",
    /// target = workspace_id). Required.
    pub consent_token: String,
    /// The credit cost the operator authorized.
    pub credits_authorized: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SwarmRunRequest {
    /// Workspace (swarm) id to read the run status from.
    pub workspace_id: String,
    /// Max messages to return. Default 50.
    pub limit: Option<usize>,
}

// ── Authoring & composition ────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GeneratePromptRequest {
    /// Natural-language description of what the agent should do.
    pub description: String,
    /// Agent name (lowercase_with_underscores).
    pub agent_name: String,
    /// Agent type (e.g. "research", "creative", "meta").
    pub agent_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GenerateOntologyRequest {
    /// Natural-language description of the agent's knowledge domain.
    pub domain_description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateAgentRequest {
    /// Agent name (lowercase_with_underscores) — becomes the system identifier.
    pub agent_name: String,
    /// Agent type (e.g. "research", "creative", "meta").
    pub agent_type: String,
    /// The agent's system prompt (its instructions).
    pub system_prompt: String,
    /// One-sentence description for the catalogue.
    pub description: String,
    /// Model id. Default: the server's `default_agent_model` (operator-
    /// configurable via `HKASK_ABW_DEFAULT_AGENT_MODEL`).
    pub model: Option<String>,
    /// Temperature (0.1–0.3 factual, 0.5–0.8 creative). Default 0.3.
    pub temperature: Option<f64>,
    /// Tags for catalogue discovery.
    pub tags: Option<Vec<String>>,
    /// Sample queries to help users understand what to ask.
    pub sample_queries: Option<Vec<String>>,
    /// Required dependency agent names (for compound agents).
    pub dependencies_required: Option<Vec<String>>,
    /// Optional dependency agent names (for compound agents).
    pub dependencies_optional: Option<Vec<String>>,
    /// MCP tools the agent may call (ABW-side capabilities, e.g.
    /// `["codegraph/codegraph_query"]`). Passed through to the ABW card's
    /// `capabilities.mcp_tools`. The local-mode analog is the local card's
    /// `capabilities.mcp_tools` (executed by `swarm_delegate_local`).
    pub mcp_tools: Option<Vec<String>>,
    /// Skill ids the agent declares (ABW-side capabilities). Passed through
    /// to the ABW card's `capabilities.skills`.
    pub skills: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateSwarmRequest {
    /// Workspace (swarm) name.
    pub name: String,
    /// Mission / description. Optional.
    pub mission: Option<String>,
    /// Agent names to hire into the new swarm. Each hire is consent-gated
    /// separately — pass `consent_tokens` aligned with `agents`.
    pub agents: Option<Vec<String>>,
    /// Consent tokens for the hires (action "hire", target = agent name).
    /// Required when `agents` is non-empty; the swarm itself is free to create.
    pub consent_tokens: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct XamanRequest {
    /// Message for Xaman Ek.
    pub message: String,
    /// Session type: "composition_design" (team planning), "workspace_help",
    /// or "free". Defaults to "free" (or server-side detection).
    pub session_type: Option<String>,
    /// Existing session id to continue. Optional.
    pub session_id: Option<String>,
    /// Consent token authorizing this curator call (action "curate",
    /// target = session_id or "xaman"). Required when `curator_consent_default`
    /// is `false` (the default) — Xaman Ek is a third-party curator that reads
    /// user task content, so sending content to it requires explicit opt-in
    /// per the plan's §3.7. When `curator_consent_default` is `true`, this
    /// field is optional.
    pub consent_token: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateAppRequest {
    /// The Xaman Ek session id to turn into an App.
    pub session_id: String,
}

// ── Local mode request types (v2 §15 Slice 9) ──────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FundLocalRequest {
    /// Number of local credits to deposit into the operator's ledger
    /// account. Must be positive.
    pub credits: i64,
}

/// Read-only balance query — no fields.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BalanceLocalRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DelegateLocalRequest {
    /// The agent id to delegate to. Must exist in the local agent registry
    /// (`agents/local/curated/<id>/agent_card.json`).
    pub agent_name: String,
    /// The task text to send to the agent. Leading @mentions are stripped
    /// (defense-in-depth, mirrors ABW delegate).
    pub task: String,
    /// The maximum credits the operator authorizes for this call. The actual
    /// cost is `min(1 credit per 1000 tokens, credits_authorized)`. Must not
    /// exceed the per-dispatch ceiling (`HKASK_ABW_MAX_CREDITS`, default 50).
    pub credits_authorized: u32,
}

// ── Local mode request types (v2 §15 Slice 11) ─────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListLocalAgentsRequest {
    /// Optional filter by agent_type. When empty, returns all local agents.
    #[serde(default)]
    pub agent_type: Option<String>,
    /// Maximum number of agents to return (default 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CloneToLocalRequest {
    /// The ABW agent id to clone to the local registry. The server fetches
    /// the agent card from ABW, sets `min_provider_class: local`, writes it
    /// to `agents/local/curated/<id>/agent_card.json`, and sets `cloud_id`
    /// to the ABW agent id (marking it as synced).
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PushToCloudRequest {
    /// The local agent id to push to ABW. The server reads the local card,
    /// creates or updates the ABW agent via `swarm_create_agent`, and sets
    /// `cloud_id` on the local card to the ABW agent id.
    pub agent_name: String,
}

/// Read-only local ledger history query.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LocalHistoryRequest {
    /// Max transactions to return (default 50, capped at 500).
    pub limit: Option<u32>,
}

/// Remove a local agent card.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RemoveLocalRequest {
    /// The local agent id to remove. The server deletes its card directory
    /// (`agents/local/curated/<id>/`) after path-safety checks. A synced
    /// card's ABW agent is NOT touched.
    pub agent_name: String,
}

/// Create a new local agent card programmatically (Cybernetic Swarm Plan —
/// `reconfigure_agent` / fan-out composition needs a programmatic create
/// path; `swarm_clone_to_local` only copies from ABW). Writes
/// `agents/local/curated/<id>/agent_card.json` and reloads the registry. No
/// consent token — local mode has no consent gate (the ledger balance is the
/// gate for *execution*, but card creation is free).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateLocalAgentRequest {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub system_prompt: String,
    #[serde(default)]
    pub accepts: Vec<String>,
    #[serde(default)]
    pub produces: Vec<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// Parallel multi-agent fan-out (Cybernetic Swarm Plan — PSO social term).
/// Dispatch N local agents in one call and aggregate. Each delegation runs
/// sequentially to avoid ledger TOCTOU (the local ledger is single-writer;
/// concurrent debits would race the balance read). Capped at `MAX_FANOUT`.
/// No consent token — local mode.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FanoutLocalRequest {
    pub delegations: Vec<FanoutEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FanoutEntry {
    pub agent_name: String,
    pub task: String,
    pub credits_authorized: u32,
}

/// Reconfigure an existing local agent's prompt in place (Cybernetic Swarm Plan
/// C6 — the Modify-Block / MASS prompt axis). Updates ONLY the `system_prompt`
/// (and optionally `model`/`mcp_tools`/`skills`); preserves `agent_id`,
/// `agent_type`, `description`, `accepts`, `produces`, `dependencies`, and the
/// `cloud_id` sync link. The DECIDE `reconfigure_agent` action seeds
/// `swarm_generate_prompt` with the blamed agent's failure log to produce the
/// new prompt, then this tool writes it and reloads the registry. No consent
/// token — local mode; re-prompting is generation (an LLM producing a new
/// prompt), not judging, so it is admissible under the determinism constraint.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReconfigureLocalAgentRequest {
    pub agent_name: String,
    pub system_prompt: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// Fire (un-hire) an agent from an ABW workspace.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FireRequest {
    /// The workspace (swarm) id.
    pub workspace_id: String,
    /// The agent to fire — the roster's `agent_name` or `agent_id` (ABW
    /// resolves both; verified live 2026-08-02).
    pub agent_name: String,
}

/// Permanently delete an ABW agent.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteAgentRequest {
    /// The agent to delete — the `agent_id` or `agent_name` from
    /// `swarm_list_agents` (for owned agents the catalogue carries a uuid in
    /// `agent_id` and the slug in `agent_name`; the tool resolves either).
    pub agent_name: String,
}

/// Permanently delete an ABW workspace (swarm).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteSwarmRequest {
    /// The workspace (swarm) id to delete.
    pub workspace_id: String,
}

// ── Knowledge search (fermi v0.10.26 embedder fix) ───────────────────────────

/// Search an agent's consolidated dreaming-memory knowledge graph via ABW's
/// vector search (`GET /api/agents/{id}/knowledge/search?q=`). The embedder was
/// broken platform-wide for 6 weeks (an Anthropic embeddings endpoint that does
/// not exist); v0.10.26 fixed it to OpenAI `text-embedding-3-large` @ 1024,
/// matching the existing pgvector column. Requires API key.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SearchKnowledgeRequest {
    /// Agent name (slug) or UUID.
    pub agent_name: String,
    /// Natural-language query to vector-search the agent's knowledge graph.
    pub query: String,
}

// ── Publish (fermi v0.10.15 admin force-publish) ───────────────────────────

/// Preflight an agent publish — `GET /api/agents/{id}/publish-checks`. Returns
/// `can_publish` and the list of failing checks (name/description/system_prompt/
/// tags). Requires API key.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PublishChecksRequest {
    /// Agent name (slug) or UUID.
    pub agent_name: String,
}

/// Publish an agent to the public catalogue — `POST /api/agents/{id}/publish`.
/// With `force=true` (admin only), failing checks are bypassed and `reason` is
/// audited to `admin_bypass_events` (mig-164, wired in fermi v0.10.5/v0.10.15).
/// Requires API key.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PublishAgentRequest {
    /// Agent name (slug) or UUID to publish.
    pub agent_name: String,
    /// Force-publish past failing checks (admin only). When `true`, `reason` is
    /// required and audited.
    pub force: Option<bool>,
    /// Justification for force-publish. Required when `force` is `true`;
    /// ignored otherwise.
    pub reason: Option<String>,
}
