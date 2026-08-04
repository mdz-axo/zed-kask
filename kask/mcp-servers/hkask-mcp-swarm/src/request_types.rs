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
pub struct ListAgentsRequest {
    /// Filter by agent type (e.g. "research", "creative", "meta"). Optional.
    pub agent_type: Option<String>,
    /// Filter by tag. Optional.
    pub tag: Option<String>,
    /// Maximum number of agents to return. Default 50.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSwarmRequest {
    /// Workspace ID (UUID) or slug. Lists workspaces when omitted.
    pub workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteAgentRequest {
    /// Agent name (e.g. "market_analyst").
    pub agent_name: String,
    /// The query or task for the agent.
    pub query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAgentRequest {
    /// Agent name or id.
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListAppsRequest {
    /// Max apps to return. Default 50.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OntologyTemplatesRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HireCostRequest {
    /// Agent name (e.g. "social_media_studio").
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RequestConsentRequest {
    /// The action to authorize: "hire" or "delegate".
    pub action: String,
    /// The target: agent name (hire) or workspace id (delegate).
    pub target: String,
    /// The credit ceiling the operator is authorizing.
    pub credits_authorized: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HireRequest {
    /// Workspace (swarm) id to hire into.
    pub workspace_id: String,
    /// Agent name to hire.
    pub agent_name: String,
    /// Whether to also hire the agent's optional dependency team.
    pub include_optional: Option<bool>,
    /// The consent token from `swarm_request_consent` (action "hire",
    /// target = agent_name). Single-use; mutually exclusive with
    /// `session_token` - provide exactly one.
    pub consent_token: Option<String>,
    /// A pre-authorized session token from `swarm_authorize_session`.
    /// Reusable across multiple hires/delegates; deducts from the session's
    /// credit budget. Mutually exclusive with `consent_token`.
    pub session_token: Option<String>,
    /// The credit cost the operator authorized (from `swarm_hire_cost`).
    pub credits_authorized: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateRequest {
    /// Workspace (swarm) id containing the agent.
    pub workspace_id: String,
    /// Agent name to delegate to (the @mention target).
    pub agent_name: String,
    /// The task for the agent.
    pub task: String,
    /// The consent token from `swarm_request_consent` (action "delegate",
    /// target = workspace_id). Single-use; mutually exclusive with
    /// `session_token` - provide exactly one.
    pub consent_token: Option<String>,
    /// A pre-authorized session token from `swarm_authorize_session`.
    /// Reusable; deducts from the session's credit budget. Mutually exclusive
    /// with `consent_token`.
    pub session_token: Option<String>,
    /// The credit cost the operator authorized.
    pub credits_authorized: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SwarmRunRequest {
    /// Workspace (swarm) id to read the run status from.
    pub workspace_id: String,
    /// Max messages to return. Default 50.
    pub limit: Option<usize>,
}

// ── Authoring & composition ────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GeneratePromptRequest {
    /// Natural-language description of what the agent should do.
    pub description: String,
    /// Agent name (lowercase_with_underscores).
    pub agent_name: String,
    /// Agent type (e.g. "research", "creative", "meta").
    pub agent_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateOntologyRequest {
    /// Natural-language description of the agent's knowledge domain.
    pub domain_description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateAgentRequest {
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
pub struct CreateSwarmRequest {
    /// Workspace (swarm) name.
    pub name: String,
    /// Mission / description. Optional.
    pub mission: Option<String>,
    /// Agent names to hire into the new swarm. Each hire is consent-gated
    /// separately — pass `consent_tokens` aligned with `agents`.
    pub agents: Option<Vec<String>>,
    /// Consent tokens for the hires (action "hire", target = agent name).
    /// Per-agent single-use tokens, aligned with `agents`. Mutually exclusive
    /// with `session_token` - provide exactly one of the two.
    pub consent_tokens: Option<Vec<String>>,
    /// A pre-authorized session token from `swarm_authorize_session` that
    /// funds ALL hires in `agents` from one reusable budget. Mutually exclusive
    /// with `consent_tokens`.
    pub session_token: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct XamanRequest {
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
pub struct CreateAppRequest {
    /// The Xaman Ek session id to turn into an App.
    pub session_id: String,
}

// ── Local mode request types (v2 §15 Slice 9) ──────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FundLocalRequest {
    /// Number of local credits to deposit into the operator's ledger
    /// account. Must be positive.
    pub credits: i64,
}

/// Read-only balance query — no fields.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BalanceLocalRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateLocalRequest {
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
pub struct ListLocalAgentsRequest {
    /// Optional filter by agent_type. When empty, returns all local agents.
    #[serde(default)]
    pub agent_type: Option<String>,
    /// Maximum number of agents to return (default 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloneToLocalRequest {
    /// The ABW agent id to clone to the local registry. The server fetches
    /// the agent card from ABW, sets `min_provider_class: local`, writes it
    /// to `agents/local/curated/<id>/agent_card.json`, and sets `cloud_id`
    /// to the ABW agent id (marking it as synced).
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PushToCloudRequest {
    /// The local agent id to push to ABW. The server reads the local card,
    /// creates or updates the ABW agent via `swarm_create_agent`, and sets
    /// `cloud_id` on the local card to the ABW agent id.
    pub agent_name: String,
}

/// Read-only local ledger history query.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LocalHistoryRequest {
    /// Max transactions to return (default 50, capped at 500).
    pub limit: Option<u32>,
}

/// Remove a local agent card.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveLocalRequest {
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
pub struct CreateLocalAgentRequest {
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
pub struct FanoutLocalRequest {
    pub delegations: Vec<FanoutEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanoutEntry {
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
pub struct ReconfigureLocalAgentRequest {
    pub agent_name: String,
    pub system_prompt: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

// ── Local swarm membership (local replica of an ABW workspace) ───────────────

/// Create a local swarm — the local replica of an ABW workspace/team. A named
/// grouping of local agent ids with a mission. No cost, no consent token (the
/// local ledger gates delegation, not roster edits). Optionally seed members.
/// Returns the new swarm with its generated `swarm_id`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateLocalSwarmRequest {
    /// Human-readable swarm name. A path-safe slug id is derived from this.
    pub name: String,
    /// Mission / description for the swarm.
    #[serde(default)]
    pub mission: String,
    /// Optional initial member agent ids to seed the roster with. Each id SHOULD
    /// exist in `LocalAgentRegistry`, but this is not enforced at create time
    /// (the roster is ids; resolution happens at delegation time, mirroring ABW
    /// workspaces).
    #[serde(default)]
    pub agents: Vec<String>,
}

/// List all local swarms. Each entry has `swarm_id`, `name`, `mission`,
/// `members`, `created_at`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListLocalSwarmsRequest {}

/// Get a single local swarm by id, including its roster.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetLocalSwarmRequest {
    /// The `swarm_id` returned by `swarm_create_local_swarm`.
    pub swarm_id: String,
}

/// Permanently delete a local swarm. The roster is dropped with the swarm;
/// member agents are NOT touched (they stay in `LocalAgentRegistry`).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteLocalSwarmRequest {
    /// The `swarm_id` to delete.
    pub swarm_id: String,
}

/// Add a local agent to a local swarm's roster (idempotent).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddAgentToLocalSwarmRequest {
    /// The swarm to add the agent to.
    pub swarm_id: String,
    /// The agent id (from `LocalAgentRegistry`) to add.
    pub agent_name: String,
}

/// Remove a local agent from a local swarm's roster (idempotent).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveAgentFromLocalSwarmRequest {
    /// The swarm to remove the agent from.
    pub swarm_id: String,
    /// The agent id to remove.
    pub agent_name: String,
}

/// Fire (un-hire) an agent from an ABW workspace.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FireRequest {
    /// The workspace (swarm) id.
    pub workspace_id: String,
    /// The agent to fire — the roster's `agent_name` or `agent_id` (ABW
    /// resolves both; verified live 2026-08-02).
    pub agent_name: String,
}

/// Permanently delete an ABW agent.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteAgentRequest {
    /// The agent to delete — the `agent_id` or `agent_name` from
    /// `swarm_list_agents` (for owned agents the catalogue carries a uuid in
    /// `agent_id` and the slug in `agent_name`; the tool resolves either).
    pub agent_name: String,
}

/// Permanently delete an ABW workspace (swarm).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteSwarmRequest {
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
pub struct SearchKnowledgeRequest {
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
pub struct PublishChecksRequest {
    /// Agent name (slug) or UUID.
    pub agent_name: String,
}

/// Publish an agent to the public catalogue — `POST /api/agents/{id}/publish`.
/// With `force=true` (admin only), failing checks are bypassed and `reason` is
/// audited to `admin_bypass_events` (mig-164, wired in fermi v0.10.5/v0.10.15).
/// Requires API key.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PublishAgentRequest {
    /// Agent name (slug) or UUID to publish.
    pub agent_name: String,
    /// Force-publish past failing checks (admin only). When `true`, `reason` is
    /// required and audited.
    pub force: Option<bool>,
    /// Justification for force-publish. Required when `force` is `true`;
    /// ignored otherwise.
    pub reason: Option<String>,
}

/// Fork an ABW agent into a derivative — `POST /api/agents/{id}/fork`
/// (fermi v0.10.16 fixed the fork path, which 500'd for everyone since
/// mig-006 due to an `agents.owner_id` column reference). Creates
/// `{source}_fork_{n}` with author-royalty tracking; the derived name is
/// slug-validated (a legacy-name source containing `-` or `/` is refused with
/// a detailed 400 — those need an admin rename via `/api/admin/agents/legacy-slugs`
/// first). Requires API key.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForkAgentRequest {
    /// Source agent name (slug) or UUID to fork.
    pub agent_name: String,
    /// Carry the source's ontology into the fork. Default false.
    pub include_ontology: Option<bool>,
    /// Carry the source's embeddings into the fork. Default false.
    pub include_embeddings: Option<bool>,
}

// ── Fan-out / pipeline / delegate-and-wait / session ──────────────────────────

/// Fan-out delegation to multiple agents in an ABW workspace. Each entry is
/// a separate `@mention` delegation, each gated by its own consent token. ABW
/// delegation is fire-and-forget (posts the @mention, returns immediately) —
/// the tool posts all messages and returns per-agent status. Responses arrive
/// via `swarm_run_status` polling. Capped at `MAX_FANOUT` (10).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanoutRequest {
    /// Workspace (swarm) id containing the agents.
    pub workspace_id: String,
    /// Delegations to post. Each must have its own consent token.
    pub delegations: Vec<FanoutAbwEntry>,
}

/// A single fan-out delegation entry for ABW.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanoutAbwEntry {
    /// Agent name to delegate to (the @mention target).
    pub agent_name: String,
    /// The task for the agent.
    pub task: String,
    /// The credit cost the operator authorized.
    pub credits_authorized: u32,
    /// Consent token from `swarm_request_consent` (action "delegate",
    /// target = workspace_id). Single-use; mutually exclusive with
    /// `session_token` - provide exactly one per entry.
    pub consent_token: Option<String>,
    /// A pre-authorized session token from `swarm_authorize_session`.
    /// Reusable; the same session can fund multiple entries. Mutually
    /// exclusive with `consent_token`.
    pub session_token: Option<String>,
}

/// Sequential pipeline: run N local agents in order, passing each agent's
/// output as context to the next via `{prev_output}` substitution. Each step
/// runs via `swarm_delegate_local`. Capped at `MAX_PIPELINE_STEPS` (10).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PipelineLocalRequest {
    /// Pipeline steps, executed in order.
    pub steps: Vec<PipelineStep>,
}

/// A single step in a local pipeline.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PipelineStep {
    /// Agent id to delegate to. Must exist in the local registry.
    pub agent_name: String,
    /// Task text. May contain `{prev_output}` which is replaced with the
    /// previous step's response text. For the first step, `{prev_output}`
    /// is left as-is (there is no previous output).
    pub task: String,
    /// Maximum credits authorized for this step.
    pub credits_authorized: u32,
}

/// Delegate a task to an ABW agent and poll `swarm_run_status` until the agent
/// responds or the timeout is reached. Wraps `swarm_delegate` +
/// `swarm_run_status` into a single call.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateAndWaitRequest {
    /// Workspace (swarm) id containing the agent.
    pub workspace_id: String,
    /// Agent name to delegate to (the @mention target).
    pub agent_name: String,
    /// The task for the agent.
    pub task: String,
    /// Consent token from `swarm_request_consent` (action "delegate",
    /// target = workspace_id). Single-use; mutually exclusive with
    /// `session_token` - provide exactly one.
    pub consent_token: Option<String>,
    /// A pre-authorized session token from `swarm_authorize_session`.
    /// Reusable; deducts from the session's credit budget. Mutually exclusive
    /// with `consent_token`.
    pub session_token: Option<String>,
    /// The credit cost the operator authorized.
    pub credits_authorized: u32,
    /// Maximum seconds to wait for the agent's response. Default 60, max 300.
    /// The tool polls `swarm_run_status` every 2 seconds until a response from
    /// the delegated agent appears or the timeout is reached.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Open a pre-authorized spend session for headless ABW pipelines. Returns a
/// session token that can be used in place of per-spend consent tokens for
/// `swarm_hire`, `swarm_delegate`, and `swarm_create_swarm`. Each spend
/// deducts from the session's total credits; when exhausted, the next spend
/// requires a new session. The per-dispatch ceiling
/// (`HKASK_ABW_MAX_CREDITS`) still gates individual spends.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuthorizeSessionRequest {
    /// Total credits to pre-authorize for the session. Each spend deducts
    /// from this total. Must be positive.
    pub total_credits: u32,
    /// Actions this session may authorize. Each must be "hire" or "delegate".
    /// An empty list authorizes both.
    #[serde(default)]
    pub actions: Vec<String>,
}

// ── A2A protocol tools ───────────────────────────────────────────────────────

/// Send an A2A (Agent2Agent) protocol message to a local agent. The message is
/// wrapped in A2A types (Message → Task → Artifact) and dispatched through the
/// existing in-process `LocalSwarmRuntime::delegate`. The response is returned
/// as an A2A Task with the agent's output as a text Artifact. No HTTP server —
/// the MCP tool dispatch IS the A2A transport.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct A2aSendRequest {
    /// Agent id to send the message to. Must exist in the local registry.
    pub agent_name: String,
    /// The message text to send to the agent.
    pub message: String,
    /// The maximum credits the operator authorizes for this call.
    pub credits_authorized: u32,
    /// Optional A2A context ID for grouping related tasks. If omitted, a new
    /// context is generated. Pass the same context_id across multiple
    /// `swarm_a2a_send` calls to group them in a conversation.
    #[serde(default)]
    pub context_id: Option<String>,
}

/// Get the A2A Agent Card for a local agent. The card describes the agent's
/// capabilities, skills, and supported interface (in-process transport).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct A2aCardRequest {
    /// Agent id to get the card for. If omitted, returns cards for all local
    /// agents.
    #[serde(default)]
    pub agent_name: Option<String>,
}

// ── Local knowledge tools (kask analogs of ABW knowledge/prompt/ontology) ─────

/// Vector-search an agent's prefix-scoped semantic memory (the local analog of
/// ABW `swarm_search_knowledge`). Returns matching knowledge fragments
/// (entity-attribute-value triples) from the operator's consolidated
/// `hkask-memory`. No ABW calls. Degrades to an empty result with a
/// `memory_unconfigured` note when `HKASK_SWARM_MEMORY_PASSPHRASE` is unset.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchKnowledgeLocalRequest {
    /// Agent id whose prefix-scoped memory (`agent:<id>:`) to search.
    pub agent_name: String,
    /// The search query. Matched case-insensitively against each triple's
    /// entity, attribute, and value.
    pub query: String,
    /// Maximum fragments to return. Default 10.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Generate a system prompt for a local agent from a description (the local
/// analog of ABW `swarm_generate_prompt`). Authoring aid — read-only, spends
/// nothing. Uses the local `InferencePort` (no ABW); optionally seeded with
/// the agent's consolidated memory. Output is guard-scanned.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GeneratePromptLocalRequest {
    /// Natural-language description of what the agent should do.
    pub description: String,
    /// Agent name (used to seed the prompt from the agent's memory and as the
    /// generated prompt's identifier).
    pub agent_name: String,
    /// Agent type hint (e.g. "research", "creative", "meta"). Default "research".
    #[serde(default)]
    pub agent_type: Option<String>,
}

/// Generate a seed ontology (Mermaid ER diagram) for a domain (the local analog
/// of ABW `swarm_generate_ontology`). Authoring aid — read-only. Uses the local
/// `InferencePort`; optionally seeded with an agent's semantic-memory graph.
/// Output is guard-scanned.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateOntologyLocalRequest {
    /// Natural-language description of the knowledge domain.
    pub domain_description: String,
    /// Optional agent id — when set, the ontology is seeded from that agent's
    /// prefix-scoped semantic memory (memory-as-graph).
    #[serde(default)]
    pub agent_name: Option<String>,
}
