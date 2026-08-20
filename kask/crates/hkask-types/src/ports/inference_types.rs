use serde::{Deserialize, Serialize};

/// A single message in a chat conversation, with an explicit role tag.
///
/// This is the foundation type for multi-turn inference: the message array
/// `[system, user, assistant, user, assistant, ...]` is sent directly to the
/// provider's `/v1/chat/completions` endpoint. Previous assistant responses
/// MUST carry `role: "assistant"` — embedding them inside a `user` message
/// causes the model to mirror/echo user input (the "you responding to
/// yourself" defect).
///
/// `role` is a String (not an enum) for OpenAI wire-format compatibility —
/// providers may introduce new roles (e.g., "tool") without breaking this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "system", "user", "assistant", or "tool"
    pub role: String,
    /// The message content.
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// Inference error types
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Model error: {0}")]
    Model(String),
    #[error("Generation error: {0}")]
    Generation(String),
    #[error("JSON error: {0}")]
    Json(String),
    #[error("Circuit open: {0}")]
    CircuitOpen(String),
    #[error("Vision inference unsupported: {0}")]
    VisionUnsupported(String),
    /// A required provider credential or configuration is missing (e.g. an
    /// API key env var is unset, or no provider is registered for the op).
    /// This is a configuration/authorization failure, not a transient
    /// outage — callers map it to `permission_denied` rather than
    /// `unavailable`. Replaces string-matching on "api key not configured"
    /// / "no provider configured" in downstream MCP servers.
    #[error("Not configured: {0}")]
    NotConfigured(String),
}

/// Token usage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InferenceUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Token probability from LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenProbability {
    pub token: String,
    pub prob: f64,
    pub top_k: Vec<TokenProb>,
}

/// Top-k token probability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenProb {
    pub token: String,
    pub prob: f64,
}

/// Confidence = avg(prob) × (1 - sqrt(variance)). Higher avg + lower variance = higher confidence.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  probs is a slice of [`TokenProbability`] values; may be empty
/// post: returns 0.0 if probs is empty; otherwise returns avg(prob) × (1 - √variance),
///       a value in [0.0, 1.0] where higher average probability and lower variance produce higher confidence
pub fn compute_confidence(probs: &[TokenProbability]) -> f64 {
    if probs.is_empty() {
        return 0.0;
    }

    let avg_prob: f64 = probs.iter().map(|p| p.prob).sum::<f64>() / probs.len() as f64;

    let variance: f64 = probs
        .iter()
        .map(|p| (p.prob - avg_prob).powi(2))
        .sum::<f64>()
        / probs.len() as f64;

    avg_prob * (1.0 - variance.sqrt())
}

/// OpenAI-compatible tool definition sent to models that support native function calling.
///
/// Serialized as `{"type": "function", "function": {"name": ..., "description": ..., "parameters": ...}}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolDefinition {
    /// Always `"function"` for OpenAI-compatible tool calling.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: ChatToolFunction,
}

/// Function definition within a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolFunction {
    /// Tool name (e.g., `"memory/recall"`).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

/// Structured tool call from a model response (OpenAI/Anthropic/Gemini native function calling).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredToolCall {
    pub server: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub call_id: Option<String>,
}

/// Inference result from LLM backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    pub text: String,
    pub model: String,
    pub usage: InferenceUsage,
    pub finish_reason: String,
    pub token_probabilities: Option<Vec<TokenProbability>>,
    /// Populated when `finish_reason == "tool_calls"`. For models without native function calling,
    /// always empty — `parse_tool_calls()` in `tool_augmented` handles `<<tool:...>>` fallback.
    #[serde(default)]
    pub tool_calls: Vec<StructuredToolCall>,
    /// Thinking-mode reasoning trace (Qwen3, GLM-5.2, DeepSeek-R1). Populated
    /// when the model emits a chain-of-thought separate from the final answer.
    /// Surfaced to the REPL as a live "thinking" trace (Zed/Cline pattern);
    /// excluded from context history and episodic storage by default.
    #[serde(default)]
    pub reasoning: Option<String>,
    /// The USD cost of this inference call, observed from the provider's
    /// response (`usage.cost` / `market_cost` / `estimated_cost`), not an
    /// operator-configured price table. Populated by the `InferencePort` impl
    /// that has access to the provider's raw response (the direct-HTTP
    /// backends); `None` when the provider reports no cost (e.g. local Ollama,
    /// or the zed IPC bridge path which surfaces only token counts) —
    /// unpriced calls are free and not charged.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

/// Streaming inference chunk — one piece of a streaming LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceStreamChunk {
    pub text_delta: String,
    pub reasoning_delta: String,
    pub model: String,
    pub finish_reason: Option<String>,
    pub usage: Option<InferenceUsage>,
    pub tool_calls: Vec<StructuredToolCall>,
    pub cost_usd: Option<f64>,
}
