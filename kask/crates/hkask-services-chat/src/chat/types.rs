//! Chat service types — request/response structs, token accounting, message sources.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use hkask_capability::{AuthContext, DelegationToken};
use hkask_memory::{EpisodicStoragePort, SemanticStoragePort};
use hkask_types::WebID;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferencePort, StructuredToolCall};

/// Token usage breakdown for gas accounting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    /// Total tokens as energy cost. Uses a 1:1 mapping — one gas unit per token.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self.total_tokens must be set
    /// post: returns total_tokens as u64 gas cost
    pub fn gas_cost(&self) -> u64 {
        self.total_tokens as u64
    }
}

/// Response from a chat inference call.
///
/// Carries the response text, token usage, and structured tool calls
/// (from native function calling) alongside the finish reason so the
/// calling surface can detect tool-call completions.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChatTurnResponse {
    /// The agent's response text
    pub text: String,
    /// Token usage from the inference call (prompt + completion tokens)
    pub usage: Option<TokenUsage>,
    /// Why the model stopped generating ("stop", "tool_calls", etc.)
    pub finish_reason: String,
    /// Structured tool calls when the model supports native function calling.
    pub tool_calls: Vec<StructuredToolCall>,
    /// Thinking-mode reasoning trace, surfaced separately from `text` so the
    /// REPL can render a live "thinking" section (Zed/Cline pattern). `None`
    /// when the provider emits no chain-of-thought.
    #[serde(default)]
    pub reasoning: Option<String>,
    /// The message array used for this inference call.
    /// On the first iteration this is the array built by `prepare_chat`;
    /// on subsequent iterations it is the pre-built array passed in.
    pub messages: Vec<hkask_types::ChatMessage>,
}

/// Request for a single chat turn.
///
/// Both CLI and API construct this from their surface-specific inputs,
/// then delegate to `ChatService::chat()`.
pub struct ChatTurnRequest {
    /// User input message
    pub input: String,
    /// Agent name (defaults to "Curator")
    pub userpod_name: Option<String>,
    /// Model override (defaults to agent-kind-specific model)
    pub model_override: Option<String>,
    /// Pre-formatted tool-call section of the system prompt from MCP discovery
    pub tool_section: Option<String>,
    /// Condensed API reference for answering API questions (from hkask_api::openapi_spec)
    pub api_spec: Option<String>,
    /// Override inference port — when provided, takes precedence over AgentService's shared port.
    /// The REPL uses this to pass its long-lived inference port.
    pub inference_port_override: Option<Arc<dyn InferencePort>>,
    /// Override episodic storage — when provided, takes precedence over AgentService's default.
    /// The REPL uses this to pass its per-agent persistent storage.
    pub episodic_storage_override: Option<Arc<dyn EpisodicStoragePort>>,
    /// Override semantic storage — when provided, takes precedence over AgentService's default.
    /// The REPL uses this to pass its per-agent persistent storage.
    pub semantic_storage_override: Option<Arc<dyn SemanticStoragePort>>,
    /// Verified authentication context from the caller. When provided, the service
    /// uses the caller's identity to derive operation-specific capability tokens
    /// instead of minting ad-hoc system-level tokens. API routes extract this from
    /// middleware-verified request extensions; CLI paths construct it from keystore secrets.
    pub auth_context: Option<AuthContext>,
    pub params_override: Option<LLMParameters>,
    /// OpenAI-compatible tool definitions for native function calling.
    /// When present, tools are included in the inference request so the model
    /// can return structured tool calls via `finish_reason == "tool_calls"`.
    /// The REPL passes these from `state.tool_definitions` during turn processing
    /// including the `/ask` handler which routes through `single_agent_turn`.
    pub tools: Option<Vec<ChatToolDefinition>>,
    /// Typed message array from thread history — when present, used to build
    /// multi-turn `[system, user, assistant, ...]` message arrays for inference.
    /// This preserves role tags so the provider sees proper conversation structure.
    /// None for single-turn API/CLI calls.
    pub thread_messages: Option<Vec<hkask_types::ChatMessage>>,
    /// Pre-built message array — when present, `chat()` skips `prepare_chat()`
    /// and calls `generate_with_messages` directly with these messages.
    /// Used by the turn loop for iterations 2+ (tool-use continuations) where
    /// the growing message array already contains the system prompt, thread
    /// history, assistant responses, and tool results with proper roles.
    /// None for the first iteration (prepare_chat builds the initial array).
    pub prebuilt_messages: Option<Vec<hkask_types::ChatMessage>>,
    /// Active improv mode — when set, prepends mode-specific instructions
    /// to the system prompt so the model adopts the interaction posture.
    /// None means no improv posture (default agent behavior).
    pub improv_mode: Option<super::improv::ImprovMode>,
}

/// Prepared chat context
/// Prepared chat context — the result of prompt composition before inference.
///
/// Returned by `ChatService::prepare_chat()` so that CLI/API surfaces can
/// stream inference output incrementally while still using the service layer
/// for agent lookup, prompt composition, and semantic recall.
pub struct PreparedChat {
    /// Typed message array for inference (system + thread history + user).
    pub messages: Vec<hkask_types::ChatMessage>,
    /// The resolved model name.
    pub model: String,
    /// The agent's WebID (for episodic storage).
    pub agent_webid: WebID,
    /// Capability token for memory operations.
    pub capability_token: DelegationToken,
    /// The resolved inference port.
    pub inference_port: Arc<dyn InferencePort>,
    /// The resolved episodic storage port.
    pub episodic_port: Arc<dyn EpisodicStoragePort>,
    /// The agent name (for episodic storage).
    pub userpod_name: String,
}

/// Request for a single-agent turn through `ChatService::execute_turn()`.
pub struct TurnRequest {
    /// User input message
    pub input: String,
    /// Agent name for registry lookup and memory operations
    pub userpod_name: String,
    /// Model name (e.g., "deepseek-v4-flash")
    pub model: String,
    /// Inference port override (REPL passes its long-lived port)
    pub inference_port: Arc<dyn InferencePort>,
    /// Episodic storage port (per-agent, for history and storage)
    pub episodic_storage: Arc<dyn EpisodicStoragePort>,
    /// Semantic storage port (per-agent, for recall)
    pub semantic_storage: Arc<dyn SemanticStoragePort>,
    /// Agent WebID for memory operations
    pub agent_webid: WebID,
    /// Persona constraints for output filtering
    /// Pre-formatted tool section of the system prompt
    pub tool_section: String,
    /// Condensed API reference for answering API questions
    pub api_spec: Option<String>,
    /// LLM parameters from user settings
    pub llm_params: LLMParameters,
    /// Capability checker for minting memory access tokens
    pub capability_checker: Arc<hkask_capability::CapabilityChecker>,
    /// System WebID for token minting
    pub system_webid: WebID,
    /// Iteration counter (0 = first iteration, incremented by caller for continuations)
    pub iteration: usize,
    /// Whether to auto-condense conversation history when approaching context limits.
    /// When true and context exceeds 87.5% of `context_window`, older messages
    /// are condensed. The threshold (0.875), saliency window (5), and pre-compress
    /// (true) are constants — not per-request parameters.
    pub auto_condense: bool,
    /// Model context window size in tokens. None disables condensation.
    pub context_window: Option<u32>,
    /// Typed message array from thread history. When present, used to build
    /// multi-turn `[system, user, assistant, ...]` message arrays for inference.
    pub thread_messages: Option<Vec<hkask_types::ChatMessage>>,
    /// Active improv mode — when set, prepends mode-specific instructions
    /// to the system prompt so the model adopts the interaction posture.
    /// None means no improv posture (default agent behavior).
    pub improv_mode: Option<super::improv::ImprovMode>,

    /// OpenAI-compatible tool definitions for native function calling.
    /// Built from MCP-discovered tools by the REPL at init time.
    /// When present, the model may return structured tool calls.
    pub tools: Option<Vec<ChatToolDefinition>>,
    /// Pre-built message array for iterations 2+ — when present, skips
    /// prepare_chat and calls inference directly with these messages.
    pub prebuilt_messages: Option<Vec<hkask_types::ChatMessage>>,
}

/// Result of a single-agent turn from `ChatService::execute_turn()`.
#[derive(Debug, Clone)]
pub struct TurnResult {
    /// The final response text (after persona filtering)
    pub text: String,
    /// Token usage for this iteration
    pub usage: TokenUsage,

    /// Structured tool calls when the model requests tools.
    /// Empty if finish_reason != "tool_calls".
    pub structured_tool_calls: Vec<StructuredToolCall>,
    /// Thinking-mode reasoning trace for this iteration, surfaced separately
    /// from `text` so the REPL can render a "thinking" section. `None` when
    /// the provider emits no chain-of-thought.
    pub reasoning: Option<String>,
    /// The message array used for this inference call.
    /// The turn loop grows this array across iterations by appending
    /// `assistant(response)` and `user(tool_results)` messages.
    /// On iteration 1, this is the array built by `prepare_chat`.
    /// On subsequent iterations, this is the pre-built array passed in.
    pub messages: Vec<hkask_types::ChatMessage>,
}

/// Event emitted during a streaming chat turn.
///
/// Callers (WSS handler, future SSE handler) consume this stream
/// and map each event to their transport-specific framing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatStreamEvent {
    /// A token delta from the streaming inference backend.
    Token { text_delta: String, model: String },
    /// Turn complete — all tokens emitted, episodic stored, Regulation spans written.
    /// `memory_stored` is false if the sovereignty gate blocked episodic storage.
    Done {
        finish_reason: String,
        usage: Option<TokenUsage>,
        memory_stored: bool,
    },
    /// An error occurred during the turn.
    Error { message: String },
}
