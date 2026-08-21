//! Request types for the swarm MCP tools — extracted from the server
//! root so the tool-handler file holds only the `#[tool]` impl block.
//!
//! These are plain `#[derive(Debug, Deserialize, JsonSchema)]` data types
//! with no `#[tool]` or `#[tool_router]` macro involvement, so they are
//! safe to relocate (unlike the tool handlers, which rmcp requires in a
//! single `impl` block — see the `tool_router` macro in `rmcp-macros`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Request types ──────────────────────────────────────────────────────────

/// A single rung on a model ladder — a (tier → model, provider) mapping.
/// fermi's ADR-011 cognition-tier system resolves the highest rung at or
/// below the creature's tier when executing an agent. When `model_ladder`
/// is `None` on the card, fermi uses the card's single `model` field for
/// all tiers (the pre-ADR-011 behavior).
#[derive(Debug, Default, Clone, PartialEq, Deserialize, JsonSchema, Serialize)]
pub struct ModelLadderRung {
    /// Cognition tier for this rung: `"free"`, `"standard"`, or `"premium"`.
    pub tier: String,
    /// Model id for this tier (e.g. `"claude-haiku-4-5-20251001"`).
    pub model: String,
    /// Provider: `"anthropic"`, `"openai"`, `"openrouter"`, `"ollama"`.
    pub provider: String,
    /// Optional note explaining why this model is at this tier.
    pub note: Option<String>,
}

/// Per-tool cognition-tier gates. Maps a tool name (the qualified
/// `server/tool` from `mcp_tools`) to the minimum tier required to invoke
/// it. fermi enforces this at execution time — a creature below the tier
/// cannot invoke the tool. When `None` on the card, no per-tool gates apply
/// (all tools available at all tiers).
#[derive(Debug, Default, Clone, PartialEq, Deserialize, JsonSchema, Serialize)]
pub struct CapabilityGate {
    /// The tool name this gate applies to (qualified `server/tool`).
    pub tool: String,
    /// Minimum cognition tier: `"free"`, `"standard"`, or `"premium"`.
    pub min_tier: String,
}

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

/// Valence parameters for agent personality encoding. Mirrors the ABW
/// `metadata.valence` object: arousal (0–1), valence (0–1), primary_affect
/// (a word like "curiosity" or "precision"), and personality_traits (free-form
/// descriptors). All fields optional — the caller fills in what they have.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ValenceInput {
    /// Arousal level (0.0 = calm, 1.0 = highly activated).
    pub arousal: Option<f64>,
    /// Valence polarity (0.0 = negative/serious, 1.0 = positive/enthusiastic).
    pub valence: Option<f64>,
    /// One-word primary affect label (e.g. "curiosity", "precision", "vigilance").
    pub primary_affect: Option<String>,
    /// Personality trait descriptors (e.g. ["analytical", "cautious", "pragmatic"]).
    pub personality_traits: Option<Vec<String>>,
}

/// Inbound MCP server declaration — a third-party MCP server an ABW agent
/// may *consume* (call as a client). Mirrors fermi's `RemoteMcpServer` shape
/// (fermi v0.16.1, mig-177 (fermi v0.16.1); `src/agent_backend/mcp_client.rs`). fermi accepts
/// both a sequence and a map form; zed-kask sends the sequence form because
/// the map form's keys are server namespaces that the model cannot
/// authoritatively choose.
///
/// This is the inbound direction (what the agent can call). The outbound
/// direction (what the agent exposes over `/mcp/agents/:id`) is `mcp_tools`
/// — a separate field on the same card. The two are easy to conflate; fermi
/// v0.11.8 release notes pin the mnemonic: `mcp_servers` = inbound,
/// `mcp_tools` = outbound.
///
/// Secrets are never inlined in the card. `secret_key` names an entry in
/// the agent owner's scoped secret store (resolved fermi-side via
/// `fermi_auth::get_secrets_for_agent`); `env` is a process-level fallback
/// for platform-owned integrations. zed-kask never sees the secret value.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, JsonSchema, Serialize)]
pub struct McpServerAuthSpec {
    /// Authentication scheme. `"bearer"` (default) sends the credential as
    /// `Authorization: Bearer <secret>`. `"header"` sends it as a raw
    /// header value on `header`.
    #[serde(default = "default_auth_scheme")]
    pub scheme: String,
    /// Header carrying the credential. Defaults to `Authorization`.
    pub header: Option<String>,
    /// Key into the agent owner's scoped secret store. fermi resolves this
    /// to the plaintext secret at execution time; zed-kask never handles
    /// the value.
    pub secret_key: Option<String>,
    /// Environment-variable fallback when the secret store has no entry.
    /// Platform-owned integrations use this.
    pub env: Option<String>,
}

fn default_auth_scheme() -> String {
    "bearer".to_string()
}

/// A remote MCP server an ABW agent is authorised to call as a client.
///
/// Field names follow the MCP ecosystem convention (Claude Desktop,
/// Cursor, `mcp.json`) so a card authored in that style transfers
/// directly. Only `name` and `endpoint` are load-bearing for discovery;
/// the rest narrow or authenticate the connection.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, JsonSchema, Serialize)]
pub struct McpServerSpec {
    /// Namespace prefix for this server's tools. Sanitised to
    /// `[a-zA-Z0-9_-]` fermi-side; becomes part of the qualified tool
    /// name the model sees (`<name>__<tool>`).
    pub name: String,
    /// Streamable-HTTP JSON-RPC endpoint. Must be an `http://` or
    /// `https://` URL — fermi refuses stdio-only servers with an
    /// actionable error (ABW does not spawn MCP processes).
    pub endpoint: String,
    /// If non-empty, only these remote tool names are exposed. Narrows a
    /// broad third-party surface to what the agent actually needs.
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    /// Per-call timeout in seconds. Default 30 (fermi-side).
    pub timeout_secs: Option<u64>,
    /// Authentication. When `None`, the server is treated as open (no
    /// credential attached). An `auth` block with an unset `secret_key`
    /// and `env` is a hard failure fermi-side — do not use it to mean
    /// "open".
    #[serde(default)]
    pub auth: Option<McpServerAuthSpec>,
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
    /// Declared input types (fermi `declares_accepts` — composition planning
    /// routes on these). Passed through to the ABW card's top-level
    /// `accepts`.
    pub accepts: Option<Vec<String>>,
    /// Declared output types (fermi `declares_produces` — downstream agents
    /// match on these). Passed through to the ABW card's top-level
    /// `produces`.
    pub produces: Option<Vec<String>>,
    /// Required dependency agent names (for compound agents).
    pub dependencies_required: Option<Vec<String>>,
    /// Optional dependency agent names (for compound agents).
    pub dependencies_optional: Option<Vec<String>>,
    /// Outbound MCP tool allowlist — what this agent *exposes* over
    /// `/mcp/agents/:id` to external MCP clients (Claude Desktop, Cursor,
    /// Zed). Qualified `server/tool` names (e.g.
    /// `["research/web_search"]`). Passed through to the ABW card's
    /// `capabilities.mcp_tools`. The local-mode analog is the local
    /// card's `capabilities.mcp_tools` (executed by
    /// `swarm_delegate_local`).
    ///
    /// Distinct from `mcp_servers` (inbound). fermi v0.16.1 release notes
    /// pin the mnemonic: `mcp_tools` = outbound (what I expose),
    /// `mcp_servers` = inbound (who I can call).
    pub mcp_tools: Option<Vec<String>>,
    /// Inbound MCP server declarations — third-party MCP servers this
    /// agent may *call* as a client at execution time. fermi v0.16.1
    /// (mig-177 (fermi v0.16.1)) added the `agents.mcp_servers` JSONB column; v0.16.1
    /// (mig-178 (fermi v0.16.1)) wired `resolve_agent_card` to bridge it over the file
    /// card. fermi discovers each server's `tools/list` (TTL-cached),
    /// namespaces the results as `<name>__<tool>`, and routes
    /// `tools/call` back out. Builtins win on name collisions.
    ///
    /// Secrets are referenced by `auth.secret_key` (agent owner's scoped
    /// secret store) — never inlined in the card. zed-kask never handles
    /// the secret value.
    ///
    /// When `None` (the default), the card omits `mcp_servers` and fermi
    /// inherits from the filesystem card (NULL column). When `Some([])`,
    /// the card publishes "explicitly no servers", overriding a file
    /// card. When `Some([...])`, the list is authoritative.
    pub mcp_servers: Option<Vec<McpServerSpec>>,
    /// Skill ids the agent declares (ABW-side capabilities). Passed through
    /// to the ABW card's `capabilities.skills`.
    pub skills: Option<Vec<String>>,
    /// Visibility level for the agent card: "public", "private", or "unlisted".
    /// Default: "private" (draft — not visible in the public catalogue until
    /// published via `swarm_publish_agent`).
    pub visibility: Option<String>,
    /// Valence / personality parameters stored under `metadata.valence` on the
    /// ABW agent card. Drives valence-homophily detection in Xaman Ek
    /// composition sessions.
    pub valence: Option<ValenceInput>,
    /// Per-tier model ladder (fermi ADR-011). When `None`, the card uses the
    /// single `model` field for all tiers. When `Some`, fermi resolves the
    /// highest rung at or below the creature's cognition tier.
    pub model_ladder: Option<Vec<ModelLadderRung>>,
    /// Per-tool cognition-tier gates (fermi ADR-011). When `None`, no
    /// per-tool gates apply. When `Some`, each entry maps a tool name to its
    /// minimum required tier.
    pub capability_gates: Option<Vec<CapabilityGate>>,
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
    /// to `agents/local/curated/<id>/agent_card.json`, and sets `cloud_swarm_id`
    /// to the ABW agent id (marking it as synced).
    pub agent_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PushToCloudSwarmRequest {
    /// The local agent id to push to ABW. The server reads the local card,
    /// creates or updates the ABW agent via `swarm_create_agent`, and sets
    /// `cloud_swarm_id` on the local card to the ABW agent id.
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
/// consent token — local mode has no consent gate and no funding gate (the
/// ledger records spend rather than authorizing it; card creation is free).
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
    /// Tags for local catalogue discovery. Optional.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Sample queries (fermi `has_sample_queries`) — one per entry.
    #[serde(default)]
    pub sample_queries: Vec<String>,
    /// Visibility level ("public", "private", "unlisted"). Default "private".
    #[serde(default)]
    pub visibility: String,
    /// Valence / personality parameters. Optional.
    pub valence: Option<ValenceInput>,
    /// Optional output contract for the agent's structured output.
    #[serde(default)]
    pub output_contract: Option<serde_json::Value>,
    /// Per-card declared evaluators (the evaluator contract). When present,
    /// every `swarm_delegate_local` call to this agent runs them against the
    /// response and stamps a deterministic `task_success` verdict.
    #[serde(default)]
    pub evaluators: Option<Vec<crate::local_registry::DeclaredEvaluator>>,
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
/// `cloud_swarm_id` sync link. The DECIDE `reconfigure_agent` action seeds
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
/// local ledger records spend; it gates neither delegation nor roster edits). Optionally seed members.
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
    /// resolves both; verified live 2026-08-13).
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

// ── Knowledge search (fermi v0.16.1 embedder fix) ───────────────────────────

/// Search an agent's consolidated dreaming-memory knowledge graph. fermi
/// does not expose a vector-search HTTP endpoint — the `MemoryStore` has
/// the capability (`search_similar_episodes`, semantic entity/rule search
/// via pgvector `<=>` cosine distance) but it's not wired to a route. The
/// tool fetches `GET /api/agents/{id}/kg/rules` + `GET /api/agents/{id}/kg/entities`
/// and does client-side text matching against the query. The v0.16.1
/// embedder fix (OpenAI `text-embedding-3-large` @ 1024) is still
/// load-bearing — without it, consolidation never runs and the KG tables
/// stay empty. Requires API key.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchKnowledgeRequest {
    /// Agent name (slug) or UUID.
    pub agent_name: String,
    /// Natural-language query to search the agent's knowledge graph
    /// (rules + entities). Matched client-side against `rule_content`,
    /// `rule_description`, `entity_name`, and `entity_summary`.
    pub query: String,
}

// ── Publish (fermi v0.16.1 admin force-publish) ───────────────────────────

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
/// audited to `admin_bypass_events` (mig-164 (fermi v0.16.1), wired in fermi v0.16.1).
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
/// (fermi v0.16.1 fixed the fork path, which 500'd for everyone since
/// mig-006 (fermi v0.16.1) due to an `agents.owner_id` column reference). Creates
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

/// Broadcast an A2A (Agent2Agent) protocol message to all members of a local
/// swarm. Each member receives the message via `LocalSwarmRuntime::delegate`,
/// and the responses are collected as an array of A2A Tasks. This is the
/// shared-channel analog of fermi's workspace-message broadcast — agents that
/// declare `swarm/swarm_a2a_broadcast` in their `mcp_tools` can address their
/// entire swarm in one call, rather than calling `swarm_a2a_send` per member.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct A2aBroadcastRequest {
    /// The local swarm id to broadcast to. All members of this swarm receive
    /// the message.
    pub swarm_id: String,
    /// The message text to broadcast to every member.
    pub message: String,
    /// The maximum credits the operator authorizes per-member dispatch.
    pub credits_authorized: u32,
    /// Optional A2A context ID for grouping related tasks. If omitted, a new
    /// context is generated and shared across all member dispatches in this
    /// broadcast.
    #[serde(default)]
    pub context_id: Option<String>,
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
/// nothing. Uses the local `InferencePort` (no ABW); optionally seeded with an
/// agent's consolidated memory.
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
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateOntologyLocalRequest {
    /// Natural-language description of the knowledge domain.
    pub domain_description: String,
    /// Optional agent id — when set, the ontology is seeded from that agent's
    /// prefix-scoped semantic memory (memory-as-graph).
    #[serde(default)]
    pub agent_name: Option<String>,
}

/// Recall prior swarm turns from the shared knowledgebase by semantic
/// similarity (the episodic-memory analog of `swarm_search_knowledge_local`,
/// which searches the EAV graph). Spans ALL agents and ALL swarms — the
/// single shared `swarm_memory.db`. Degrades to a `memory_unconfigured` note
/// when the store cannot be opened or the query cannot be embedded.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallLocalRequest {
    /// The natural-language query. Embedded and matched against every prior
    /// turn's task embedding via KNN — returns the most similar past turns.
    pub query: String,
    /// Maximum turns to return. Default 10.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// AI assist for the swarm panel authoring forms — suggests completions for
/// partial inputs or validates well-formedness. Authoring aid — read-only,
/// spends nothing. Uses the local `InferencePort` (one-shot LLM generate).
/// The `mode` field only tailors the guidance text; no ABW
/// calls in either mode.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AiAssistRequest {
    /// "suggest" (complete empty/partial fields) or "validate" (check
    /// well-formedness).
    pub action: String,
    /// "agent" (author form) or "swarm" (compose form).
    pub surface: String,
    /// "abw" or "local" — tailors the guidance to the selected backend.
    pub mode: String,
    /// Agent name / swarm name (the form's Name field).
    #[serde(default)]
    pub name: String,
    /// Agent type (agent surface only): research/creative/meta.
    #[serde(default)]
    pub agent_type: String,
    /// Agent description (agent surface) / unused for swarm.
    #[serde(default)]
    pub description: String,
    /// Agent system prompt (agent surface).
    #[serde(default)]
    pub system_prompt: String,
    /// Swarm mission (swarm surface).
    #[serde(default)]
    pub mission: String,
    /// Swarm agents, comma-separated (swarm surface).
    #[serde(default)]
    pub agents: String,
    /// Tags, comma-separated (agent surface).
    #[serde(default)]
    pub tags: String,
    /// Sample queries, one per line (agent surface). Newline-separated
    /// because queries contain commas.
    #[serde(default)]
    pub sample_queries: String,
    /// Declared input types, comma-separated (agent surface).
    #[serde(default)]
    pub accepts: String,
    /// Declared output types, comma-separated (agent surface).
    #[serde(default)]
    pub produces: String,
    /// Whether any valence field is set (agent surface).
    #[serde(default)]
    pub has_valence: bool,
}

/// Request for `swarm_evaluate_local` — a deterministic task-success evaluator.
/// The Curator (or a human) calls this after a `swarm_delegate_local` to stamp
/// a `TaskSuccessVerdict` with `provenance: DeterministicEvaluator` onto the delegation
/// result. This is the enforcement point for the C5/C6 fault-attribution loop:
/// without it, `task_success` is always `None` and ORIENT's highest-fidelity
/// fault signal is inert.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvaluateLocalRequest {
    /// The agent's response text (the `response` field from
    /// `swarm_delegate_local`'s result).
    pub response: String,
    /// The evaluator type: "contains" (response contains the spec string),
    /// "regex" (response matches the spec regex), "not_contains" (response
    /// does NOT contain the spec string — for verifying absence of an error),
    /// "exit_code" (run the spec as a shell command with $RESPONSE set to
    /// the response text; pass if exit code is 0 — external ground truth),
    /// or "file_exists" (pass if the spec file path exists — external ground
    /// truth). The exit_code and file_exists evaluators mitigate the Goodhart
    /// risk of string-match oracles in a training loop: they check real-world
    /// effects, not response text, so gaming requires actually doing the work.
    pub evaluator: String,
    /// The spec: substring (contains/not_contains), regex pattern, shell
    /// command (exit_code), or file path (file_exists). Case-sensitive.
    pub spec: String,
}

/// A single delegation within an execute-plan request. The tool runs each
/// delegation via `swarm_delegate_local`, then (when an evaluator is provided)
/// stamps a deterministic `TaskSuccessVerdict` onto the result.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanDelegation {
    /// The agent id to delegate to. Must exist in the local registry.
    pub agent_name: String,
    /// The task text to send to the agent.
    pub task: String,
    /// Maximum credits authorized for this delegation.
    pub credits_authorized: u32,
    /// Optional deterministic evaluator. When provided, the tool runs the
    /// check after the delegation and stamps `task_success` onto the result.
    /// When absent, `task_success` is left null (open task, no oracle).
    pub evaluator: Option<PlanEvaluator>,
}

/// An evaluator spec within a plan delegation.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanEvaluator {
    /// The evaluator type: "contains", "not_contains", "regex", "exit_code",
    /// or "file_exists".
    pub evaluator: String,
    /// The spec: substring (contains/not_contains), regex pattern, shell
    /// command (exit_code), or file path (file_exists). Case-sensitive.
    pub spec: String,
}

/// Request for `swarm_execute_plan_local` — runs a swarm-intelligence plan
/// (a list of delegations), evaluates each result, and returns the collected
/// `LocalDelegateResult` array ready to feed back to swarm-intelligence. This
/// closes the loop deterministically: the caller passes the plan, the tool
/// executes it and stamps verdicts, the caller passes the results back. Works
/// in any context — chat, autonomous pipeline, or API.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecutePlanLocalRequest {
    /// The delegations to execute, in order. Capped at 10 (same as fanout).
    pub delegations: Vec<PlanDelegation>,
}

/// A single task in a `swarm_eval_agent_local` run. The evaluator is required:
/// the harness exists to measure pass rates, so a task without an oracle is
/// unmeasurable and is rejected upfront rather than silently counted as
/// neither pass nor fail.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvalAgentTask {
    /// The task text to send to the agent.
    pub task: String,
    /// Maximum credits authorized per rollout of this task.
    pub credits_authorized: u32,
    /// The deterministic evaluator run against each rollout's response.
    pub evaluator: PlanEvaluator,
}

/// Request for `swarm_eval_agent_local` — the rollout harness. Runs one agent
/// card against a task set N times each, stamps deterministic verdicts, and
/// reports per-task pass rates with standard error (the sampled part is the
/// rollout, not the evaluator — repeat counts exist because local inference
/// is sampled and identical tasks diverge).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvalAgentLocalRequest {
    /// The agent id to evaluate. Must exist in the local registry.
    pub agent_name: String,
    /// The task set. Capped at 10 tasks.
    pub tasks: Vec<EvalAgentTask>,
    /// Repeats per task (default 3, cap 10). The total rollout count
    /// (tasks × repeats) is capped at 50 — each rollout is a real inference
    /// call with real token cost.
    pub repeats: Option<u32>,
}
