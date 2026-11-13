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
//! - `create_worktree_thread` — create a sibling agent thread in a new git
//!   worktree workspace on the zed side (`kanban_task_spawn`'s delegation
//!   path). The child carries an opaque parent-issued grant token and a
//!   request allowlist; dispatch intersects them. Default is deny. Tokens are
//!   per server, not PID-bound or OS isolation against arbitrary same-UID code.
//! - `sign_nostr_note` — sign one untagged kind 1 Nostr note with the
//!   operator's key, in the editor (D106). The child carries the same opaque
//!   grant token; the grant must name `host/sign_nostr_note`. The editor
//!   reads `kask.nostr.autopublish` and, when it is off, shows the note and
//!   waits for the operator before any key is read. The key never crosses
//!   the wire in either direction.
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

/// The host effect a grant must name for `InferenceMethod::SignNostrNote`
/// to be admitted. Set per child in `kask.mcp.delegated_tools`; default
/// settings grant it to no server.
pub const NOSTR_SIGN_GRANT: &str = "host/sign_nostr_note";

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
    /// Create a sibling agent thread in a new git worktree workspace. Uses
    /// `worktree_prompt`, `worktree_title`, `worktree_name`, `worktree_base_ref`
    /// from `InferenceParams`. The result is returned as
    /// `InferenceOutcome::WorktreeThread`. Used by `kanban_task_spawn` to
    /// isolate spawned agents in a separate worktree (P1: worktree/terminal
    /// model).
    CreateWorktreeThread,

    /// Rerank documents against a query with a dedicated reranker via the
    /// provider's rerank endpoint (OpenRouter `/api/v1/rerank`). Uses
    /// `rerank_model`, `rerank_query`, `rerank_documents` from
    /// `InferenceParams`. The zed side holds the API key and calls the
    /// provider directly — the MCP server never sees the credential. The
    /// result is returned as `InferenceOutcome::RerankScores`.
    Rerank,

    /// Ask a decision model (a System One model served over the provider's
    /// Decisions API — e.g. OpenRouter `/api/alpha/decisions`,
    /// `typesafe/jev-1.14`) typed questions about a state: state in, typed
    /// answers with calibrated per-level probabilities out — never text.
    /// Uses `decision_model`, `decision_state`, `decision_questions` from
    /// `InferenceParams`. The zed side holds the API key and calls the
    /// provider directly — the MCP server never sees the credential. The
    /// result is returned as `InferenceOutcome::DecisionAnswers`.
    Decision,

    /// Sign one untagged kind 1 Nostr note with the operator's key, in the
    /// editor. Uses `nostr_content`, `nostr_created_at` and `nostr_relays`
    /// from `InferenceParams` and the grant in `tool_grant`, which must name
    /// [`NOSTR_SIGN_GRANT`]. The editor applies `kask.nostr.autopublish`:
    /// off, it shows the note, the signing identity and the relays and signs
    /// only when the operator approves; on, it signs. The result is returned
    /// as `InferenceOutcome::NostrSignedEvent`. The key is read, decrypted
    /// and used on the zed side only.
    SignNostrNote,
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
    /// Output width to request from MRL-capable embedding models (the
    /// OpenAI-compatible `dimensions` body field — e.g. Qwen3 Embedding's
    /// Matryoshka truncation). `None` lets the model emit its native width;
    /// callers whose vector stores are width-bound (sqlite-vec `float[N]`
    /// and `int8[N]` tables) must pass the store's width so returned vectors always fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_dimensions: Option<u32>,

    /// Qualified `server/tool` names the spawned child may use. The zed side
    /// refuses any tool outside this list and the independent parent grant.
    /// CreateWorktreeThread requires this field; an empty list deliberately
    /// creates an inference-only child, never grant-all.
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
    /// Rerank model string. Only `OpenRouter/`-prefixed models are
    /// rerank-eligible: rerank routes to the OpenRouter endpoint only
    /// (`hkask-inference/src/rerank.rs`); any other prefix degrades to
    /// heuristic rerank with the reason surfaced.
    #[serde(default)]
    pub rerank_model: Option<String>,
    /// The search query to rerank documents against.
    #[serde(default)]
    pub rerank_query: Option<String>,
    /// Documents to rerank — plain text per candidate.
    #[serde(default)]
    pub rerank_documents: Option<Vec<String>>,
    // ── Decision fields (for `InferenceMethod::Decision`) ──
    /// Decision model string (provider-prefixed; only `OpenRouter/`-prefixed
    /// models are decision-eligible — the Decisions API routes to the
    /// OpenRouter endpoint only, `hkask-inference/src/decisions.rs`).
    #[serde(default)]
    pub decision_model: Option<String>,
    /// The state to evaluate — the text (or JSON) the questions are about.
    #[serde(default)]
    pub decision_state: Option<String>,
    /// Typed score questions: an instruction plus ordered level
    /// descriptions per question (`criteria`); the provider answers with a
    /// probability distribution over those levels.
    #[serde(default)]
    pub decision_questions: Option<Vec<DecisionQuestion>>,
    // ── Nostr fields (for `InferenceMethod::SignNostrNote`) ──
    /// The note's text, as it will be published.
    #[serde(default)]
    pub nostr_content: Option<String>,
    /// The note's `created_at`, seconds since the Unix epoch. Signed into
    /// the event, so the caller fixes it and the read-back can find it.
    #[serde(default)]
    pub nostr_created_at: Option<u64>,
    /// The relays the caller will send the signed note to. Shown to the
    /// operator in the approval; the editor sends nothing.
    #[serde(default)]
    pub nostr_relays: Option<Vec<String>>,
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
        /// Exact model identity supplied by the embedding caller.
        #[serde(default)]
        requested_model: String,
        /// Exact model identity returned by the provider, when present.
        #[serde(default)]
        actual_model: Option<String>,
        /// Token usage the provider reported for the embedding call.
        /// `#[serde(default)]` for cross-binary wire compat: the zed-side
        /// server and the MCP-server client are different binaries and can
        /// be different versions across a rolling restart — an old
        /// server's response deserializes with zeroed usage and
        /// `reported: false` (honest absence, never a fabricated zero).
        #[serde(default)]
        usage: crate::ports::InferenceUsage,
    },
    /// Model list from `InferenceMethod::ListModels`.
    ModelList {
        #[serde(rename = "models")]
        models: Vec<ModelListEntry>,
    },
    /// Create a sibling agent thread in a new git worktree workspace.
    WorktreeThread {
        #[serde(rename = "worktree_thread")]
        thread: WorktreeThreadInfo,
    },

    /// Rerank scores from `InferenceMethod::Rerank`. One entry per scored
    /// document, sorted by descending relevance by the provider.
    RerankScores {
        #[serde(rename = "rerank_scores")]
        scores: Vec<RerankScoreEntry>,
        /// Token usage the provider reported for the rerank call
        /// (`#[serde(default)]` for cross-binary wire compat — see the
        /// `Embeddings` variant).
        #[serde(default)]
        usage: crate::ports::InferenceUsage,
    },
    /// Typed answers from `InferenceMethod::Decision`. One entry per
    /// question the provider answered, keyed by the caller's question id.
    DecisionAnswers {
        #[serde(rename = "decision_answers")]
        answers: Vec<DecisionAnswer>,
        /// The exact model identity the provider served (the snapshot id,
        /// e.g. `typesafe/jev-1.14-20261014`).
        #[serde(rename = "decision_model")]
        model: String,
        /// Token usage the provider reported for the decision call
        /// (`#[serde(default)]` for cross-binary wire compat — see the
        /// `Embeddings` variant).
        #[serde(default)]
        usage: crate::ports::InferenceUsage,
    },
    /// The signed event from `InferenceMethod::SignNostrNote`: the NIP-01
    /// event object (`id`, `pubkey`, `created_at`, `kind`, `tags`,
    /// `content`, `sig`), verified before it was returned.
    NostrSignedEvent {
        #[serde(rename = "nostr_event")]
        event: serde_json::Value,
    },
    /// Error from the inference port.
    Error {
        #[serde(rename = "error")]
        error: InferenceErrorPayload,
    },
}

/// A single reranked document's score — the provider's native relevance
/// judgment, not a parsed LLM generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankScoreEntry {
    /// Index of the document in the original input list.
    pub index: usize,
    /// Relevance score of the document to the query (provider-scaled,
    /// typically 0.0-1.0).
    pub relevance_score: f64,
}

/// A typed score question for `InferenceMethod::Decision` — the Jev-class
/// "Score" primitive: an instruction plus an ordered array of level
/// descriptions (least to most), answered with a probability distribution
/// over the levels plus a confidence. The question id is never sent to the
/// model (the provider's contract — all meaning lives in the instructions
/// and the level descriptions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionQuestion {
    /// Caller-side question key — the answer returns under this key.
    pub id: String,
    /// The question text, including any criteria context the levels need.
    pub instructions: String,
    /// Ordered level descriptions, least to most (the score scale).
    pub criteria: Vec<String>,
}

/// One level's probability from a decision answer — the parsed form of the
/// provider's string-keyed `probabilities` map (`{"0": 0.79, ...}`),
/// decoded once at the zed side (the one decode home).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionLevelProbability {
    /// The level's index into the question's `criteria`.
    pub level: u8,
    /// The calibrated probability the model assigns to that level.
    pub probability: f64,
}

/// A typed answer to one `DecisionQuestion` — the provider's native
/// judgment (a probability distribution over the caller's own levels),
/// not a parsed LLM generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionAnswer {
    /// The question key the answer belongs to.
    pub id: String,
    /// The probability-weighted score over the levels (the provider's
    /// `score` field — e.g. 1.21 for mostly-level-1 with some level-2).
    pub score: f64,
    /// The probability distribution over the question's criteria levels.
    pub level_probabilities: Vec<DecisionLevelProbability>,
    /// The concentration of the distribution (0.0-1.0) — how unambiguous
    /// the model's judgment is (not its correctness).
    pub confidence: f64,
}

/// A model entry in a `ListModels` response — a serializable subset of
/// zed's `LanguageModel` trait surface, carrying the fields the corpus
/// server's `ModelInfo` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListEntry {
    /// Full model name with provider prefix (e.g. "openrouter/qwen/qwen3-embedding-8b").
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
    /// The provider's original HTTP status for `Api` errors, carried
    /// structurally so the client reconstructs
    /// `EmbeddingGenerationError::Api(status, _)` instead of re-parsing the
    /// message string. `None` for every other error kind and for status-less
    /// `Api`-coded payloads (`EmptyResponse`, `DimensionMismatch`) —
    /// absence, never a fabricated status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
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
            status: None,
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

#[cfg(test)]
mod tests {
    use super::{
        InferenceErrorPayload, InferenceMethod, InferenceOutcome, InferenceParams,
        InferenceRequest, InferenceResponse, NOSTR_SIGN_GRANT,
    };

    /// expect: the sign method and its outcome survive the wire, the method
    /// is spelled `sign_nostr_note`, and a request from a client that knows
    /// nothing of the Nostr fields still parses (they default).
    #[test]
    fn sign_nostr_note_round_trips_and_its_fields_default() {
        let request = InferenceRequest {
            id: 7,
            method: InferenceMethod::SignNostrNote,
            params: InferenceParams {
                tool_grant: Some("token".into()),
                nostr_content: Some("hello".into()),
                nostr_created_at: Some(1_700_000_000),
                nostr_relays: Some(vec!["wss://relay.example".into()]),
                ..Default::default()
            },
        };
        let wire = serde_json::to_string(&request).expect("serializes");
        assert!(wire.contains(r#""method":"sign_nostr_note""#));
        let back: InferenceRequest = serde_json::from_str(&wire).expect("parses");
        assert!(matches!(back.method, InferenceMethod::SignNostrNote));
        assert_eq!(back.params.nostr_content.as_deref(), Some("hello"));
        assert_eq!(back.params.nostr_created_at, Some(1_700_000_000));

        // A client built before these fields existed sends none of them.
        let mut legacy_wire = serde_json::to_value(InferenceParams::default()).expect("serializes");
        let legacy_object = legacy_wire.as_object_mut().expect("an object");
        for field in ["nostr_content", "nostr_created_at", "nostr_relays"] {
            legacy_object.remove(field);
        }
        let legacy: InferenceParams =
            serde_json::from_value(legacy_wire).expect("old params parse");
        assert_eq!(legacy.nostr_content, None);
        assert_eq!(legacy.nostr_created_at, None);
        assert_eq!(legacy.nostr_relays, None);

        let response = InferenceResponse {
            id: 7,
            outcome: InferenceOutcome::NostrSignedEvent {
                event: serde_json::json!({"kind": 1, "content": "hello"}),
            },
        };
        let wire = serde_json::to_string(&response).expect("serializes");
        let back: InferenceResponse = serde_json::from_str(&wire).expect("parses");
        match back.outcome {
            InferenceOutcome::NostrSignedEvent { event } => assert_eq!(event["kind"], 1),
            other => panic!("wrong outcome: {other:?}"),
        }
        assert_eq!(NOSTR_SIGN_GRANT, "host/sign_nostr_note");
    }

    /// The `status` field is wire-optional: a payload from before the field
    /// existed still parses (absent → `None`), and `None` serializes without
    /// the field, so the old wire shape is preserved for non-Api errors.
    #[test]
    fn error_payload_status_is_wire_optional() {
        let legacy = serde_json::from_str::<InferenceErrorPayload>(
            r#"{"code":"Api","message":"rate limited"}"#,
        )
        .expect("legacy payload without status parses");
        assert_eq!(legacy.status, None);
        assert_eq!(legacy.code, "Api");

        let wire = serde_json::to_string(&InferenceErrorPayload {
            code: "Api".to_string(),
            message: "rate limited".to_string(),
            status: Some(429),
        })
        .expect("serializes");
        assert_eq!(
            wire,
            r#"{"code":"Api","message":"rate limited","status":429}"#
        );

        let wire = serde_json::to_string(&InferenceErrorPayload {
            code: "Json".to_string(),
            message: "malformed".to_string(),
            status: None,
        })
        .expect("serializes");
        assert_eq!(wire, r#"{"code":"Json","message":"malformed"}"#);
    }
}
