//! Inference IPC protocol — shared types for the MCP inference bridge.
//!
//! When zed-kask launches MCP server child processes, it passes a Unix socket
//! path via the `HKASK_INFERENCE_SOCKET` env var. The MCP server connects to
//! this socket and sends inference requests as JSON-RPC messages. Zed handles
//! the requests using its own `LanguageModelRegistry` (with guard, and
//! zed's configured API keys), eliminating the need for MCP servers to have
//! their own API keys.
//!
//! ## Protocol
//!
//! Each request is a single JSON object on one line (newline-delimited JSON):
//!
//! ```json
//! {"id": 1, "method": "generate", "params": {"prompt": "...", "parameters": {...}}}
//! ```
//!
//! The response is also a single JSON object on one line:
//!
//! ```json
//! {"id": 1, "result": {"text": "...", "model": "...", "usage": {...}}}
//! ```
//!
//! or on error:
//!
//! ```json
//! {"id": 1, "error": {"code": "Generation", "message": "..."}}
//! ```
//!
//! ## Methods
//!
//! - `generate` — single prompt → result
//! - `generate_with_model` — prompt + model override → result
//! - `generate_with_messages` — message array → result
//! - `generate_vision` — prompt + images → result
//! - `embed` — model + texts → embedding vectors (OpenAI-compatible `/embeddings`)
//! - `list_models` — list available models from zed's `LanguageModelRegistry`
//! - `tool_invoke` — invoke a governed MCP tool on the zed side (`ToolDispatchPort`);
//!   used by MCP servers that run agent loops (e.g. `hkask-mcp-swarm`'s local
//!   delegate) so a delegated agent can call MCP tools that live in the parent
//!   process. The child carries an opaque parent-issued grant token and a
//!   request allowlist; dispatch intersects them. Default is deny. Tokens are
//!   per server, not PID-bound or OS isolation against arbitrary same-UID code.
//!
//! Streaming methods (`generate_stream*`) are not supported over IPC — the
//! IPC bridge collects the stream server-side and returns a single result.
//! This matches the existing `LanguageModelInferencePort` pattern.

use serde::{Deserialize, Serialize};

use crate::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, LLMParameters};

/// Environment variable name for the Unix socket path.
pub const INFERENCE_SOCKET_ENV: &str = "HKASK_INFERENCE_SOCKET";
/// Opaque parent-owned delegated-tool grant, scoped to a launched MCP child.
pub const TOOL_GRANT_ENV: &str = "HKASK_TOOL_GRANT";

/// Environment variable name for the admission-to-completion timeout in seconds.
///
/// The zed process publishes its configured `inference_timeout_secs`
/// (the deadline the server-side `LanguageModelInferencePort` enforces on
/// queue wait, model resolution, stream establishment and drain) so IPC clients can set
/// their read deadline to `server_timeout + grace` instead of inventing an
/// independent (and shorter) one. Without this, a slow-but-alive provider
/// whose total request lifetime exceeds the client's read timeout produces a
/// storm of `BrokenPipe` warnings: the client gives up first, closes its
/// socket, and the server's later response write hits EPIPE. With this, the
/// client strictly outlasts the server, so a timed-out inference produces one
/// timeout (the server's), not two contradictory warnings.
///
/// Clients that cannot read this var (or find it empty) fall back to a
/// conservative default — never to zero.
pub const INFERENCE_TIMEOUT_ENV: &str = "HKASK_INFERENCE_TIMEOUT_SECS";

/// A single prompt entry for `InferenceMethod::GenerateBatch`.
///
/// Carries the `custom_id` (for matching results to prompts), the system
/// message, and the user message. The zed side formats these as OpenAI
/// Batch API JSONL and submits them to the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPromptEntry {
    /// Unique identifier for this prompt (returned in results for matching).
    pub custom_id: String,
    /// System message content.
    pub system: String,
    /// User message content.
    pub user: String,
}

/// A single result from a batch inference call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultEntry {
    /// The `custom_id` from the corresponding `BatchPromptEntry`.
    pub custom_id: String,
    /// The generated text (on success).
    pub text: Option<String>,
    /// Total tokens used (on success).
    pub total_tokens: u64,
    /// Error message (on failure).
    pub error: Option<String>,
}

/// A request from the MCP server to the zed inference bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    /// Correlation ID — matches the response to the request.
    pub id: u64,
    /// The method to call.
    pub method: InferenceMethod,
    /// Method parameters.
    pub params: InferenceParams,
}

/// The inference method to invoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceMethod {
    Generate,
    GenerateWithModel,
    GenerateWithMessages,
    GenerateVision,
    /// Generate embeddings for a batch of texts. Uses `embed_model` and
    /// `embed_texts` from `InferenceParams`. The result is returned as
    /// `InferenceOutcome::Embeddings`.
    Embed,
    /// List available models from zed's `LanguageModelRegistry`.
    /// The result is returned as `InferenceOutcome::ModelList`.
    ListModels,
    /// Invoke a governed MCP tool on the zed side (`ToolDispatchPort`).
    /// Uses `tool_server`, `tool_name`, `tool_args` from `InferenceParams`.
    /// The result is returned as `InferenceOutcome::ToolResult`.
    ToolInvoke,
    /// Create a sibling agent thread in a new git worktree workspace. Uses
    /// `worktree_prompt`, `worktree_title`, `worktree_name`, `worktree_base_ref`
    /// from `InferenceParams`. The result is returned as
    /// `InferenceOutcome::WorktreeThread`. Used by `kanban_task_spawn` to
    /// isolate spawned agents in a separate worktree (P1: worktree/terminal
    /// model).
    CreateWorktreeThread,
    /// Submit a batch of prompts to the provider's Batch API (OpenRouter
    /// `/api/beta/batches` or DeepInfra `/v1/openai/batches`). The zed side
    /// holds the API keys and handles submission, polling, and download —
    /// the MCP server never sees the credentials. Uses `batch_prompts` and
    /// `model_override` from `InferenceParams`. The result is returned as
    /// `InferenceOutcome::BatchResults`.
    GenerateBatch,
    /// Rerank documents against a query with a dedicated reranker via the
    /// provider's rerank endpoint (OpenRouter `/api/v1/rerank`). Uses
    /// `rerank_model`, `rerank_query`, `rerank_documents` from
    /// `InferenceParams`. The zed side holds the API key and calls the
    /// provider directly — the MCP server never sees the credential. The
    /// result is returned as `InferenceOutcome::RerankScores`.
    Rerank,
}

/// Parameters for an inference request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InferenceParams {
    pub prompt: Option<String>,
    pub messages: Option<Vec<ChatMessage>>,
    pub images: Option<Vec<String>>,
    pub parameters: LLMParameters,
    pub model_override: Option<String>,
    pub tools: Option<Vec<ChatToolDefinition>>,
    /// Embedding model string (provider-prefixed) for `InferenceMethod::Embed`.
    pub embed_model: Option<String>,
    /// Texts to embed for `InferenceMethod::Embed`.
    pub embed_texts: Option<Vec<String>>,
    /// Batch prompts for `InferenceMethod::GenerateBatch`. Each entry is a
    /// `(custom_id, system, user)` tuple. The zed side submits these to the
    /// provider's Batch API and returns results keyed by `custom_id`.
    #[serde(default)]
    pub batch_prompts: Option<Vec<BatchPromptEntry>>,
    /// Max output tokens per prompt for `InferenceMethod::GenerateBatch`.
    #[serde(default)]
    pub batch_max_tokens: Option<u32>,
    // ── Tool dispatch fields (for `InferenceMethod::ToolInvoke`) ──
    pub tool_server: Option<String>,
    /// Tool name to invoke.
    pub tool_name: Option<String>,
    /// Tool arguments (JSON).
    pub tool_args: Option<serde_json::Value>,
    /// Qualified `server/tool` names the child may dispatch (the delegated
    /// agent's declared `mcp_tools` allowlist). The zed side refuses any
    /// tool outside this list **before** minting the panel token — the
    /// allowlist is enforced at the dispatch boundary, not only inside the
    /// child process. Fail closed: a missing or empty allowlist is a
    /// protocol violation, never an implicit grant-all.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    /// Opaque reference to a parent-owned grant; never a caller-defined permission set.
    pub tool_grant: Option<String>,
    // ── Worktree thread fields (for `InferenceMethod::CreateWorktreeThread`) ──
    /// The initial prompt for the new agent thread.
    #[serde(default)]
    pub worktree_prompt: Option<String>,
    /// A short title for the new thread, shown in the sidebar.
    #[serde(default)]
    pub worktree_title: Option<String>,
    /// Optional name for the new worktree directory. When omitted, the
    /// editor generates a random non-colliding name.
    #[serde(default)]
    pub worktree_name: Option<String>,
    /// Git ref (branch, tag, or commit) to base the new worktree on.
    /// Defaults to `HEAD`.
    #[serde(default)]
    pub worktree_base_ref: Option<String>,
    // ── Rerank fields (for `InferenceMethod::Rerank`) ──
    /// Rerank model string (provider-prefixed, e.g.
    /// `OpenRouter/qwen/qwen3-reranker-8b`). The zed side strips the
    /// provider prefix and routes to that provider's rerank endpoint.
    #[serde(default)]
    pub rerank_model: Option<String>,
    /// The search query to rerank documents against.
    #[serde(default)]
    pub rerank_query: Option<String>,
    /// Documents to rerank — plain text per candidate.
    #[serde(default)]
    pub rerank_documents: Option<Vec<String>>,
}

/// A response from the zed inference bridge to the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    /// Correlation ID — matches the request.
    pub id: u64,
    /// The result, or an error.
    #[serde(flatten)]
    pub outcome: InferenceOutcome,
}

/// The outcome of an inference request — either success or error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InferenceOutcome {
    /// Successful result.
    Result {
        #[serde(rename = "result")]
        result: InferenceResult,
    },
    /// Embedding vectors from `InferenceMethod::Embed`.
    Embeddings {
        #[serde(rename = "embeddings")]
        embeddings: Vec<Vec<f32>>,
    },
    /// Model list from `InferenceMethod::ListModels`.
    ModelList {
        #[serde(rename = "models")]
        models: Vec<ModelListEntry>,
    },
    /// Tool dispatch result from `InferenceMethod::ToolInvoke`.
    /// The value is the tool's JSON output. The key is `tool_result` (not
    /// `result`) so the untagged enum cannot confuse a tool output that
    /// happens to carry `result` with the `Result` variant.
    ToolResult {
        #[serde(rename = "tool_result")]
        result: serde_json::Value,
    },
    /// Worktree thread creation result from
    /// `InferenceMethod::CreateWorktreeThread`. The value is the new
    /// thread's id + worktree path.
    WorktreeThread {
        #[serde(rename = "worktree_thread")]
        thread: WorktreeThreadInfo,
    },
    /// Batch inference results from `InferenceMethod::GenerateBatch`.
    /// The zed side submits all prompts to the provider's Batch API,
    /// polls until completion, downloads results, and returns them here.
    /// The MCP server never sees the API keys.
    BatchResults {
        #[serde(rename = "batch_results")]
        results: Vec<BatchResultEntry>,
    },
    /// Rerank scores from `InferenceMethod::Rerank`. One entry per scored
    /// document, sorted by descending relevance by the provider.
    RerankScores {
        #[serde(rename = "rerank_scores")]
        scores: Vec<RerankScoreEntry>,
    },
    /// Error from the inference port.
    Error {
        #[serde(rename = "error")]
        error: InferenceErrorPayload,
    },
}

/// A single reranked document's score — the provider's native relevance
/// judgment, not a parsed LLM generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankScoreEntry {
    /// Index of the document in the original input list.
    pub index: usize,
    /// Relevance score of the document to the query (provider-scaled,
    /// typically 0.0-1.0).
    pub relevance_score: f64,
}

/// A model entry in a `ListModels` response — a serializable subset of
/// zed's `LanguageModel` trait surface, carrying the fields the corpus
/// server's `ModelInfo` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListEntry {
    /// Full model name with provider prefix (e.g. "openrouter/qwen/qwen3-embedding-0.6b").
    pub name: String,
    /// Provider id (e.g. "openrouter", "ollama").
    pub provider: String,
    /// Whether the model supports vision/multimodal input.
    pub supports_vision: bool,
}

/// Info about a worktree-backed agent thread created via
/// `InferenceMethod::CreateWorktreeThread`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeThreadInfo {
    /// A human-readable confirmation message (e.g. "Worktree thread created").
    pub message: String,
}

/// Serializable form of `InferenceError`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceErrorPayload {
    /// The error kind as a string (matches `InferenceError` variant names).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl From<InferenceError> for InferenceErrorPayload {
    fn from(e: InferenceError) -> Self {
        let (code, message) = match e {
            InferenceError::Overloaded(m) => ("Overloaded", m),
            InferenceError::Timeout(m) => ("Timeout", m),
            InferenceError::Connection(m) => ("Connection", m),
            InferenceError::Model(m) => ("Model", m),
            InferenceError::Generation(m) => ("Generation", m),
            InferenceError::Json(m) => ("Json", m),
            InferenceError::CircuitOpen(m) => ("CircuitOpen", m),
            InferenceError::VisionUnsupported(m) => ("VisionUnsupported", m),
            InferenceError::NotConfigured(m) => ("NotConfigured", m),
            InferenceError::Auth(m) => ("Auth", m),
        };
        Self {
            code: code.to_string(),
            message,
        }
    }
}

impl From<InferenceErrorPayload> for InferenceError {
    fn from(e: InferenceErrorPayload) -> Self {
        match e.code.as_str() {
            "Overloaded" => InferenceError::Overloaded(e.message),
            "Timeout" => InferenceError::Timeout(e.message),
            "Connection" => InferenceError::Connection(e.message),
            "Model" => InferenceError::Model(e.message),
            "Generation" => InferenceError::Generation(e.message),
            "Json" => InferenceError::Json(e.message),
            "CircuitOpen" => InferenceError::CircuitOpen(e.message),
            "VisionUnsupported" => InferenceError::VisionUnsupported(e.message),
            "NotConfigured" => InferenceError::NotConfigured(e.message),
            "Auth" => InferenceError::Auth(e.message),
            _ => InferenceError::Generation(e.message),
        }
    }
}
