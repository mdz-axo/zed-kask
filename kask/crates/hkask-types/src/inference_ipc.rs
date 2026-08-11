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
//! - `media_generate` — generate media (image, video, speech, transcription) via fal.ai/DeepInfra
//! - `tool_invoke` — invoke a governed MCP tool on the zed side (`ToolDispatchPort`);
//!   used by MCP servers that run agent loops (e.g. `hkask-mcp-swarm`'s local
//!   delegate) so a delegated agent can call MCP tools that live in the parent
//!   process. The zed side mints the OCAP panel token — the child never holds
//!   token material.
//! - `skill_execute` — run an hKask skill cascade on the zed side
//!   (`SkillExecPort`, backed by the global `ManifestExecutor`); used by MCP
//!   servers so a delegated agent's declared `skills` execute with the
//!   executor's own gas/OCAP enforcement.
//!
//! Streaming methods (`generate_stream*`) are not supported over IPC — the
//! IPC bridge collects the stream server-side and returns a single result.
//! This matches the existing `LanguageModelInferencePort` pattern.

use serde::{Deserialize, Serialize};

use crate::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, LLMParameters};

/// Environment variable name for the Unix socket path.
pub const INFERENCE_SOCKET_ENV: &str = "HKASK_INFERENCE_SOCKET";

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
    /// Generate media (image, video, speech, transcription, etc.) via
    /// fal.ai/DeepInfra backends. Uses `media_op`, `media_prompt`,
    /// `media_image_url`, `media_text`, `media_size`, `media_count`,
    /// `media_strength`, `media_duration`, `media_workflow` from
    /// `InferenceParams`. The result is returned as
    /// `InferenceOutcome::Media`.
    MediaGenerate,
    /// Invoke a governed MCP tool on the zed side (`ToolDispatchPort`).
    /// Uses `tool_server`, `tool_name`, `tool_args` from `InferenceParams`.
    /// The result is returned as `InferenceOutcome::ToolResult`.
    ToolInvoke,
    /// Execute an hKask skill cascade on the zed side (`SkillExecPort`).
    /// Uses `skill_name`, `skill_task` from `InferenceParams`. The result is
    /// returned as `InferenceOutcome::SkillResult`.
    SkillExecute,
    /// Create a sibling agent thread in a new git worktree workspace. Uses
    /// `worktree_prompt`, `worktree_title`, `worktree_name`, `worktree_base_ref`
    /// from `InferenceParams`. The result is returned as
    /// `InferenceOutcome::WorktreeThread`. Used by `kanban_task_spawn` to
    /// isolate spawned agents in a separate worktree (P1: worktree/terminal
    /// model).
    CreateWorktreeThread,
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
    // ── Media generation fields (for `InferenceMethod::MediaGenerate`) ──
    /// The media operation to perform (e.g. "generate_image", "transcribe").
    pub media_op: Option<String>,
    /// Text prompt for image/video generation.
    pub media_prompt: Option<String>,
    /// Image URL for image-to-image, image-to-video, upscale, etc.
    pub media_image_url: Option<String>,
    /// Audio URL for transcription.
    pub media_audio_url: Option<String>,
    /// Text for speech synthesis.
    pub media_text: Option<String>,
    /// Voice name for speech synthesis.
    pub media_voice: Option<String>,
    /// Image size for image generation.
    pub media_size: Option<String>,
    /// Number of images to generate.
    pub media_count: Option<u32>,
    /// Strength for image-to-image.
    pub media_strength: Option<f32>,
    /// Scale factor for upscaling.
    pub media_scale: Option<u32>,
    /// Duration for video generation.
    pub media_duration: Option<f32>,
    /// Object description for segmentation.
    pub media_object_description: Option<String>,
    /// Language hint for transcription.
    pub media_language: Option<String>,
    /// Workflow JSON for `execute_workflow`.
    pub media_workflow: Option<serde_json::Value>,
    // ── Tool dispatch fields (for `InferenceMethod::ToolInvoke`) ──
    /// MCP server id the tool lives on (e.g. "codegraph").
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
    // ── Skill execution fields (for `InferenceMethod::SkillExecute`) ──
    /// Skill id to execute (e.g. "grill-me").
    pub skill_name: Option<String>,
    /// Task text the skill cascade acts on.
    pub skill_task: Option<String>,
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
    /// Media generation result from `InferenceMethod::MediaGenerate`.
    /// The value is the raw JSON returned by fal.ai/DeepInfra.
    Media {
        #[serde(rename = "media")]
        media: serde_json::Value,
    },
    /// Tool dispatch result from `InferenceMethod::ToolInvoke`.
    /// The value is the tool's JSON output. The key is `tool_result` (not
    /// `result`) so the untagged enum cannot confuse a tool output that
    /// happens to carry `result` with the `Result` variant.
    ToolResult {
        #[serde(rename = "tool_result")]
        result: serde_json::Value,
    },
    /// Skill execution result from `InferenceMethod::SkillExecute`.
    /// The value is the cascade's final output text. The key is
    /// `skill_result` (distinct from `result`/`tool_result` for the same
    /// untagged-enum reason).
    SkillResult {
        #[serde(rename = "skill_result")]
        result: String,
    },
    /// Worktree thread creation result from
    /// `InferenceMethod::CreateWorktreeThread`. The value is the new
    /// thread's id + worktree path. The key is `worktree_thread` (distinct
    /// from `result`/`tool_result`/`skill_result` for the untagged-enum
    /// reason).
    WorktreeThread {
        #[serde(rename = "worktree_thread")]
        thread: WorktreeThreadInfo,
    },
    /// Error from the inference port.
    Error {
        #[serde(rename = "error")]
        error: InferenceErrorPayload,
    },
}

/// A model entry in a `ListModels` response — a serializable subset of
/// zed's `LanguageModel` trait surface, carrying the fields the corpus
/// server's `ModelInfo` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListEntry {
    /// Full model name with provider prefix (e.g. "deepinfra/qwen/qwen3-embedding-0.6b").
    pub name: String,
    /// Provider id (e.g. "deepinfra", "openrouter").
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
            InferenceError::Connection(m) => ("Connection", m),
            InferenceError::Model(m) => ("Model", m),
            InferenceError::Generation(m) => ("Generation", m),
            InferenceError::Json(m) => ("Json", m),
            InferenceError::CircuitOpen(m) => ("CircuitOpen", m),
            InferenceError::VisionUnsupported(m) => ("VisionUnsupported", m),
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
            "Connection" => InferenceError::Connection(e.message),
            "Model" => InferenceError::Model(e.message),
            "Generation" => InferenceError::Generation(e.message),
            "Json" => InferenceError::Json(e.message),
            "CircuitOpen" => InferenceError::CircuitOpen(e.message),
            "VisionUnsupported" => InferenceError::VisionUnsupported(e.message),
            _ => InferenceError::Generation(e.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The untagged `InferenceOutcome` must not confuse a tool output that
    // happens to carry a `result` key with the `Result` variant — the
    // `ToolResult` payload uses the distinct `tool_result` key. Pins the
    // serde contract for the swarm tool-dispatch path.
    #[test]
    fn tool_result_roundtrip_uses_distinct_key() {
        let response = InferenceResponse {
            id: 7,
            outcome: InferenceOutcome::ToolResult {
                result: serde_json::json!({ "rows": 42, "result": "inner" }),
            },
        };
        let json = serde_json::to_string(&response).expect("serialize");
        assert!(
            json.contains("\"tool_result\""),
            "ToolResult must serialize under the distinct tool_result key: {json}"
        );
        let parsed: InferenceResponse = serde_json::from_str(&json).expect("deserialize");
        match parsed.outcome {
            InferenceOutcome::ToolResult { result } => {
                assert_eq!(result["rows"], 42);
                // The inner `result` key of the tool's own output must not
                // be mistaken for the Result variant.
                assert_eq!(result["result"], "inner");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_invoke_request_roundtrip() {
        let request = InferenceRequest {
            id: 3,
            method: InferenceMethod::ToolInvoke,
            params: InferenceParams {
                prompt: None,
                messages: None,
                images: None,
                parameters: LLMParameters::default(),
                model_override: None,
                tools: None,
                embed_model: None,
                embed_texts: None,
                media_op: None,
                media_prompt: None,
                media_image_url: None,
                media_audio_url: None,
                media_text: None,
                media_voice: None,
                media_size: None,
                media_count: None,
                media_strength: None,
                media_scale: None,
                media_duration: None,
                media_object_description: None,
                media_language: None,
                media_workflow: None,
                tool_server: Some("codegraph".to_string()),
                tool_name: Some("codegraph_query".to_string()),
                tool_args: Some(serde_json::json!({ "q": "x" })),
                tool_allowlist: Some(vec!["codegraph/codegraph_query".to_string()]),
                skill_name: None,
                skill_task: None,
                worktree_prompt: None,
                worktree_title: None,
                worktree_name: None,
                worktree_base_ref: None,
            },
        };
        let json = serde_json::to_string(&request).expect("serialize");
        let parsed: InferenceRequest = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(parsed.method, InferenceMethod::ToolInvoke));
        assert_eq!(parsed.params.tool_server.as_deref(), Some("codegraph"));
        assert_eq!(parsed.params.tool_name.as_deref(), Some("codegraph_query"));
        assert_eq!(
            parsed.params.tool_args,
            Some(serde_json::json!({ "q": "x" }))
        );
        assert_eq!(
            parsed.params.tool_allowlist,
            Some(vec!["codegraph/codegraph_query".to_string()])
        );
    }

    #[test]
    fn skill_execute_request_roundtrip() {
        let request = InferenceRequest {
            id: 4,
            method: InferenceMethod::SkillExecute,
            params: InferenceParams {
                prompt: None,
                messages: None,
                images: None,
                parameters: LLMParameters::default(),
                model_override: None,
                tools: None,
                embed_model: None,
                embed_texts: None,
                media_op: None,
                media_prompt: None,
                media_image_url: None,
                media_audio_url: None,
                media_text: None,
                media_voice: None,
                media_size: None,
                media_count: None,
                media_strength: None,
                media_scale: None,
                media_duration: None,
                media_object_description: None,
                media_language: None,
                media_workflow: None,
                tool_server: None,
                tool_name: None,
                tool_args: None,
                tool_allowlist: None,
                skill_name: Some("grill-me".to_string()),
                skill_task: Some("probe the delegate".to_string()),
                worktree_prompt: None,
                worktree_title: None,
                worktree_name: None,
                worktree_base_ref: None,
            },
        };
        let json = serde_json::to_string(&request).expect("serialize");
        let parsed: InferenceRequest = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(parsed.method, InferenceMethod::SkillExecute));
        assert_eq!(parsed.params.skill_name.as_deref(), Some("grill-me"));
        assert_eq!(
            parsed.params.skill_task.as_deref(),
            Some("probe the delegate")
        );

        // The skill output serializes under the distinct `skill_result` key.
        let response = InferenceResponse {
            id: 4,
            outcome: InferenceOutcome::SkillResult {
                result: "gap analysis".to_string(),
            },
        };
        let json = serde_json::to_string(&response).expect("serialize");
        assert!(json.contains("\"skill_result\""), "{json}");
        let parsed: InferenceResponse = serde_json::from_str(&json).expect("deserialize");
        match parsed.outcome {
            InferenceOutcome::SkillResult { result } => assert_eq!(result, "gap analysis"),
            other => panic!("expected SkillResult, got {other:?}"),
        }
    }
}
