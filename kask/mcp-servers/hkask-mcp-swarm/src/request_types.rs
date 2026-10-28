//! Request types for the swarm MCP tools — extracted from the server
//! root so the tool-handler file holds only the `#[tool]` impl block.
//!
//! These are plain `#[derive(Debug, Deserialize, JsonSchema)]` data types
//! with no `#[tool]` or `#[tool_router]` macro involvement, so they are
//! safe to relocate (unlike the tool handlers, which rmcp requires in a
//! single `impl` block — see the `tool_router` macro in `rmcp-macros`).

use hkask_types::AnyJsonValue;
use schemars::JsonSchema;
use serde::Deserialize;

// ── Request types ──────────────────────────────────────────────────────────

/// Valence parameters for agent personality encoding: arousal (0–1),
/// valence (0–1), primary_affect
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

// ── Local mode request types (v2 §15 Slice 9) ──────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateLocalRequest {
    /// The agent id to delegate to. Must exist in the local agent registry
    /// (`agents/local/curated/<id>/agent_card.json`).
    pub agent_name: String,
    /// The task text to send to the agent. Leading @mentions are stripped
    /// (defense-in-depth).
    pub task: String,
}

/// Delegate within a local swarm's durable conversation thread.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateInThreadLocalRequest {
    pub swarm_id: String,
    pub agent_name: String,
    pub task: String,
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

/// Get a single local agent's full card.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetLocalAgentRequest {
    /// The local agent id whose card to fetch
    /// (`agents/local/curated/<id>/agent_card.json`).
    pub agent_name: String,
}

/// Validate a local agent's declared `workflow_template` — the scaffold
/// for the run-the-declared-workflow pattern. Resolves each stage's slot
/// against the local registry and seam-checks consecutive stages
/// (upstream `produces` must overlap downstream `accepts`). Advisory — a
/// violation describes what a runner would trip over, it blocks nothing.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowCheckLocalRequest {
    /// The local agent whose `workflow_template` to check.
    pub agent_name: String,
}

/// Run a local agent's declared `workflow_template` end-to-end.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunWorkflowLocalRequest {
    /// The local agent whose `workflow_template` to run.
    pub agent_name: String,
    /// The task — stage 1's input. Each later stage receives its
    /// predecessor's response verbatim (the artifact flows).
    pub task: String,
    /// When supplied, dispatch each stage into this swarm's ordered thread.
    pub swarm_id: Option<String>,
}

/// Report the observed delegation topology — which agents' outputs have
/// actually fed which agents' inputs — and compare it against the declared
/// ports.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ObservedSeamsLocalRequest {
    /// Max edges to return, most-recent-first by last observation.
    /// Default 50.
    pub limit: Option<usize>,
}

/// The local fleet's shape — the prompt-tier digest.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FleetDigestLocalRequest {
    /// Include the rendered prompt-tier text alongside the structured map.
    /// Default true — the rendered form is what a meta agent injects.
    #[serde(default = "default_true")]
    pub include_rendered: bool,
}

fn default_true() -> bool {
    true
}

/// Who else answers the same ask — the cohort query.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WhoAnswersLocalRequest {
    /// The `accepts` label to query (e.g. "text", "analysis").
    pub label: String,
}

/// Remove a local agent card.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveLocalRequest {
    /// The local agent id to remove. The server deletes its card directory
    /// (`agents/local/curated/<id>/`) after path-safety checks. Nothing
    /// outside the registry is touched.
    pub agent_name: String,
}

/// Create a new local agent card programmatically (Cybernetic Swarm Plan —
/// `reconfigure_agent` / fan-out composition needs a programmatic create
/// path). Writes
/// `agents/local/curated/<id>/agent_card.json` and reloads the registry. No
/// consent token — local mode has no consent gate and no budget gate (no
/// local budget exists; card creation is free).
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
    pub output_contract: Option<AnyJsonValue>,
    /// Optional input contract — what callers must send to invoke this
    /// agent (fermi A2A `capabilities.input_contract`). Symmetric to
    /// `output_contract`.
    #[serde(default)]
    pub input_contract: Option<AnyJsonValue>,
    /// Sampling temperature (fermi `agents.temperature`, 0.0–1.0). `None`
    /// means the executor's default preset applies — same semantics as
    /// fermi's create, which defaults to 0.3.
    #[serde(default)]
    pub temperature: Option<f64>,
    /// Provider-agnostic sampling overrides. Must be a JSON object whose known
    /// fields deserialize as `LLMParameters`; partial objects are allowed.
    #[serde(default)]
    pub model_params: Option<AnyJsonValue>,
    /// Per-card declared evaluators (the evaluator contract). When present,
    /// every `swarm_delegate_local` call to this agent runs them against the
    /// response and stamps its match result under `task_success`.
    #[serde(default)]
    pub evaluators: Option<Vec<crate::local_registry::DeclaredEvaluator>>,
    /// Opt-in structured reasoning trace. When true, the agent's executor
    /// registers a `reasoning/think` tool the model may call to record
    /// reasoning steps. See `LocalAgentCapabilities.reasoning`.
    #[serde(default)]
    pub reasoning: Option<bool>,
}

/// Parallel multi-agent fan-out (Cybernetic Swarm Plan — PSO social term).
/// Dispatch N local agents in one call and aggregate. By default each
/// delegation runs sequentially; when `parallel` is true, the inference
/// calls run concurrently via `tokio::join_all` for speed, and the
/// per-delegation side effects (validate_produces + ingest_turn) run
/// sequentially after all delegations complete.
/// Capped at `MAX_FANOUT`. No consent token — local mode.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanoutLocalRequest {
    pub delegations: Vec<FanoutEntry>,
    /// When true, run the inference calls concurrently and process the
    /// per-delegation side effects after all complete. When false (default),
    /// run sequentially. Parallel mode is faster for independent, read-heavy
    /// delegations but uses more concurrent inference resources.
    #[serde(default)]
    pub parallel: bool,
    /// When supplied, dispatch sequentially into this swarm's ordered thread.
    /// Scoped parallel fanout is rejected.
    pub swarm_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanoutEntry {
    pub agent_name: String,
    pub task: String,
}

/// Reconfigure an existing local agent's prompt in place (Cybernetic Swarm Plan
/// C6 — the Modify-Block / MASS prompt axis). Updates ONLY the `system_prompt`
/// (and optionally `model`/`mcp_tools`/`skills`/`model_params`); preserves `agent_id`,
/// `agent_type`, `description`, `accepts`, `produces`, `dependencies`, and
/// provenance, then reloads the registry. The DECIDE `reconfigure_agent` action seeds
/// `swarm_generate_prompt_local` with the blamed agent's failure log to produce the
/// new prompt, then this tool writes it. No consent
/// token — re-prompting is generation (an LLM producing a new
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
    /// Optional replacement for provider-agnostic sampling overrides. When
    /// absent, the existing card value is preserved.
    #[serde(default)]
    pub model_params: Option<AnyJsonValue>,
}

// ── Local swarm membership ───────────────────────────────────────────

/// Create a local swarm — a named grouping of local agent ids with a mission.
/// No cost, no consent token (no
/// local budget exists; nothing gates delegation or roster edits). Optionally seed members.
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
    /// (the roster is ids; resolution happens at delegation time).
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

/// Update a local swarm's display name and mission in place. The `swarm_id`
/// (on-disk identity) is preserved — only the human-readable `name` and
/// `mission` change.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateLocalSwarmRequest {
    /// The `swarm_id` of the swarm to edit.
    pub swarm_id: String,
    /// New display name (non-empty).
    pub name: String,
    /// New mission / description.
    #[serde(default)]
    pub mission: String,
}

/// Clone a local swarm: create a new swarm with a fresh slug id, copying the
/// source's mission and roster. The new name is suffixed with " (copy)".
/// Member ids are preserved as-is — a member whose card no longer exists is
/// not an error (the roster is ids; resolution happens at delegation time).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloneLocalSwarmRequest {
    /// The `swarm_id` of the source swarm to clone.
    pub swarm_id: String,
}

/// Sequential pipeline: run N local agents in order, passing each agent's
/// output as context to the next via `{prev_output}` substitution. Each step
/// runs through the local runtime or, when scoped, the swarm thread.
/// Capped at `MAX_PIPELINE_STEPS` (10).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PipelineLocalRequest {
    /// Pipeline steps, executed in order.
    pub steps: Vec<PipelineStep>,
    /// When supplied, dispatch each step into this swarm's ordered thread.
    pub swarm_id: Option<String>,
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
}

// ── A2A protocol tools ───────────────────────────────────────────────────────

/// Send an A2A (Agent2Agent) protocol message to a local agent. The message is
/// wrapped in A2A types (Message → Task → Artifact) and dispatched to the
/// optional swarm thread or standalone runtime. The response is returned
/// as an A2A Task with the agent's output as a text Artifact. No HTTP server —
/// the MCP tool dispatch IS the A2A transport.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct A2aSendRequest {
    /// Agent id to send the message to. Must exist in the local registry.
    pub agent_name: String,
    /// The message text to send to the agent.
    pub message: String,
    /// Optional local swarm thread. Omitted sends remain standalone.
    #[serde(default)]
    pub swarm_id: Option<String>,
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
/// swarm. Each member receives the ordered prior turns; successful responses
/// are stored, then collected as an array of A2A Tasks. This is the
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
    /// Optional A2A context ID for grouping related tasks. If omitted, a new
    /// context is generated and shared across all member dispatches in this
    /// broadcast.
    #[serde(default)]
    pub context_id: Option<String>,
}

// ── Local knowledge tools ────────────────────────────────────────────

/// Vector-search an agent's prefix-scoped semantic memory. Returns matching
/// knowledge fragments
/// (entity-attribute-value triples) from the operator's consolidated
/// `hkask-memory`. Degrades to an empty result with a
/// `memory_unconfigured` note when `HKASK_DB_PASSPHRASE` is unset.
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

/// Generate a system prompt for a local agent from a description.
/// Authoring aid — read-only, spends
/// nothing. Uses the local `InferencePort`; optionally seeded with an
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

/// Generate a seed ontology (Mermaid ER diagram) for a domain.
/// Authoring aid — read-only. Uses the local
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
    /// Optional per-agent scope (fermi parity: its per-agent KG/episodes
    /// are searched per-agent). When set, only turns produced by this
    /// agent are returned; when absent, recall spans ALL agents — the
    /// shared-knowledgebase default (one store, many namespaces).
    #[serde(default)]
    pub agent_name: Option<String>,
}

/// AI assist for the swarm panel authoring forms — suggests completions for
/// partial inputs or validates well-formedness. Authoring aid — read-only,
/// spends nothing. Uses the local `InferencePort` (one-shot LLM generate).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AiAssistRequest {
    /// "suggest" (complete empty/partial fields) or "validate" (check
    /// well-formedness).
    pub action: String,
    /// "agent" (author form) or "swarm" (compose form).
    pub surface: String,
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

/// Request for a deterministic response-predicate check. Its result does not
/// establish task accomplishment, factual truth, or evaluator independence.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvaluateLocalRequest {
    /// The agent's response text (the `response` field from
    /// `swarm_delegate_local`'s result).
    pub response: String,
    /// The evaluator type: "contains" (response contains the spec string),
    /// "regex" (response matches the spec regex), "not_contains" (response
    /// does NOT contain the spec string — for verifying absence of an error).
    /// Checks are pure response scoring, not independent ground truth.
    pub evaluator: String,
    /// The spec: substring (contains/not_contains) or regex pattern. Case-sensitive.
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
    /// Optional response predicate. When provided, the tool runs the
    /// check after the delegation and stamps `task_success` onto the result.
    /// When absent, `task_success` is omitted and the board remains Pending.
    pub evaluator: Option<PlanEvaluator>,
    /// Optional stable task identifier for the task board. When omitted,
    /// a synthetic id is derived from (agent_name, task) so the same task
    /// accumulates attempts across invocations rather than growing
    /// unboundedly.
    #[serde(default)]
    pub task_id: Option<String>,
}

/// An evaluator spec within a plan delegation.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanEvaluator {
    /// The evaluator type: "contains", "not_contains", or "regex".
    pub evaluator: String,
    /// The spec: substring (contains/not_contains) or regex pattern. Case-sensitive.
    pub spec: String,
}

/// Request for `swarm_execute_plan_local` — runs a swarm-intelligence plan
/// (a list of delegations), evaluates each result, and returns the collected
/// `LocalDelegateResult` array ready to feed back to swarm-intelligence. These
/// response-predicate results do not certify task accomplishment.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecutePlanLocalRequest {
    /// The delegations to execute, in order. Capped at 10 (same as fanout).
    pub delegations: Vec<PlanDelegation>,
    /// Optional swarm id. When set, delegations require membership and share
    /// the durable ordered thread; task progress (status, attempt count,
    /// fail count, last result) is recorded to the swarm's task board
    /// (`<swarm_dir>/<swarm_id>/task_board.json`) so the Curator's ORIENT
    /// phase can query durable task progress via `swarm_task_board`.
    #[serde(default)]
    pub swarm_id: Option<String>,
}

/// Request for `swarm_task_board` — query a swarm's persistent task board.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskBoardRequest {
    /// The swarm id whose task board to query.
    pub swarm_id: String,
}

/// Request for `swarm_eval_suite_local` — regression-test a swarm composition
/// across a dataset of cases. Each case is a plan (list of delegations with
/// evaluators). The suite runs each case via `swarm_execute_plan_local`,
/// aggregates pass/fail, and returns a suite-level result. There is no
/// improve-then-re-evaluate outer loop — the suite is measure-and-report,
/// not measure-improve-remeasure.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvalSuiteLocalRequest {
    /// Optional swarm id. When set, task progress for each case's delegations
    /// is recorded to the swarm's task board so regression runs are durable
    /// across invocations.
    #[serde(default)]
    pub swarm_id: Option<String>,
    /// The test cases to run, in order. Capped at 10.
    pub cases: Vec<EvalSuiteCase>,
}

/// A single test case in a swarm eval suite. Each case is a plan: a list of
/// delegations with deterministic evaluators. The case passes when ALL
/// delegations pass their evaluators.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvalSuiteCase {
    /// A short name for the case (for reporting).
    pub name: String,
    /// The delegations to execute for this case. Same shape as
    /// `swarm_execute_plan_local` delegations.
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
