//! Shared OpenAI-compatible & OpenRouter wire protocol, provider descriptors,
//! and response-body redaction across `hkask-inference` and `kask_bridge`.
//!
//! Consolidates:
//! - [`INFERENCE_PROVIDERS`] — the single canonical descriptor table for
//!   OpenAI-compatible / native inference providers (`OpenRouter`, `RunPod`,
//!   `ollama`, `DeepInfra`), replacing the former duplicate
//!   `DIRECT_EMBEDDING_PROVIDERS` table.
//! - [`build_embedding_request_body`], [`parse_embedding_response`], and
//!   [`embed_via_reqwest`] — the single `/embeddings` wire implementation shared
//!   by `LanguageModelEmbeddingPort`, `DirectEmbeddingPort`, and IPC multi-provider
//!   dispatch (enforcing `encoding_format: "float"`, non-empty inputs,
//!   `data[].index` ordering, exact batch count validation, and OpenRouter
//!   post-header HTTP 200 error detection).
//! - [`extract_provider_error_envelope`] / [`extract_provider_error_from_value`] —
//!   detects OpenRouter's post-header `{"error": {"code", "message", "metadata"}}`
//!   error payloads even when delivered under HTTP `200 OK`.
//! - [`format_error_chain`] — walks `std::error::Error::source()` so transport
//!   errors preserve DNS/TLS/timeout/HTTP2-stream root causes instead of masking
//!   them behind `error sending request for url (...)`.
//! - [`sanitize_error_body`] — redacts secret-shaped substrings from raw provider
//!   error bodies before logging or returning across IPC/MCP boundaries.

/// Maximum length of a provider response body embedded in an error string.
pub(crate) const ERROR_BODY_MAX_CHARS: usize = 200;

/// Secret-shaped prefixes that a provider error page or proxy debug dump may
/// echo back (CWE-209). Redaction is a simple prefix scan, not a parser:
/// defense-in-depth before the body reaches IPC/log sinks.
///
/// All prefixes MUST be lowercase — they are matched against the lowercased
/// body in `redact_secret_tokens`.
pub(crate) const SECRET_PREFIXES: &[&str] = &[
    "authorization:",
    "bearer ",
    "sk-",
    "api_key",
    // Common credential prefixes beyond OpenAI's `sk-`:
    // GitHub PATs, AWS keys, Slack tokens, GitLab tokens, JWTs.
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "akaa", // AWS access key id prefix (case-insensitive match)
    "xoxb-",
    "xoxp-",
    "glpat-",
    "eyj", // JWT header base64 prefix (eyJ...)
];

/// Sanitizes a raw provider response body before embedding it in an error
/// string: redacts secret-shaped substrings (prefix through the end of the
/// whitespace-delimited token) and truncates to [`ERROR_BODY_MAX_CHARS`]
/// (char-boundary safe) with a total-length suffix.
///
/// Shared across inference backends, the MCP `classify_http_error` helper,
/// and research providers (the provider error-body redaction class).
#[must_use]
pub fn sanitize_error_body(body: &str) -> String {
    let redacted = redact_secret_tokens(body);
    let total_bytes = body.len();
    if redacted.chars().count() <= ERROR_BODY_MAX_CHARS {
        redacted
    } else {
        let boundary = redacted
            .char_indices()
            .nth(ERROR_BODY_MAX_CHARS)
            .map(|(index, _)| index)
            .unwrap_or(redacted.len());
        format!("{}… ({} bytes total)", &redacted[..boundary], total_bytes)
    }
}

pub(crate) fn redact_secret_tokens(body: &str) -> String {
    // Case-insensitive byte-index scan; prefixes are ASCII so byte matching is exact.
    let lower = body.to_ascii_lowercase();
    let mut output = String::with_capacity(body.len());
    let mut cursor = 0;
    while cursor < body.len() {
        let match_at = SECRET_PREFIXES
            .iter()
            .filter_map(|prefix| lower[cursor..].find(prefix).map(|offset| cursor + offset))
            .min();
        match match_at {
            Some(start) => {
                output.push_str(&body[cursor..start]);
                output.push_str("[REDACTED]");
                let prefix_len = SECRET_PREFIXES
                    .iter()
                    .filter(|prefix| lower[start..].starts_with(**prefix))
                    .map(|prefix| prefix.len())
                    .max()
                    .unwrap_or(0);
                // Skip whitespace after the prefix — for "Authorization: Basic abc123"
                // the token follows a space, and without this the scan stops at
                // that space and the secret survives.
                let token_start = start
                    + prefix_len
                    + (body[start + prefix_len..].len()
                        - body[start + prefix_len..].trim_start().len());
                // Header-style prefixes ("Authorization:", "api_key") may be
                // followed by a scheme word ("Basic", "Bearer") before the
                // secret, so redact to end-of-line. Opaque token prefixes
                // ("Bearer ", "sk-") redact to end-of-token.
                let matched_prefix = SECRET_PREFIXES
                    .iter()
                    .filter(|prefix| lower[start..].starts_with(**prefix))
                    .max_by_key(|prefix| prefix.len());
                let to_end_of_line =
                    matches!(matched_prefix, Some(p) if p.ends_with(':') || *p == "api_key");
                cursor = if to_end_of_line {
                    body[token_start..]
                        .find('\n')
                        .map(|offset| token_start + offset)
                        .unwrap_or(body.len())
                } else {
                    body[token_start..]
                        .find(char::is_whitespace)
                        .map(|offset| token_start + offset)
                        .unwrap_or(body.len())
                };
            }
            None => {
                output.push_str(&body[cursor..]);
                break;
            }
        }
    }
    output
}

// ── Canonical Inference Provider Descriptors ──────────────────────────────

/// A descriptor for an inference provider registered in the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InferenceProviderDescriptor {
    /// The provider ID prefix (e.g. `"OpenRouter"`, `"ollama"`, `"DeepInfra"`).
    pub id: &'static str,
    /// Human-readable name for logging and UI.
    pub name: &'static str,
    /// The OpenAI-compatible API base URL (and keychain slot URL where Zed's
    /// `ApiKeyState` and `kask_bridge` read/write the provider's API key).
    pub api_url: &'static str,
    /// The provider's credential environment variable (`""` for keyless local
    /// services such as `ollama`).
    pub env_var: &'static str,
    /// Stable credential key mapped by `credential_url_for_key`.
    pub credential_key: &'static str,
    /// Dashboard URL where the operator can obtain an API key.
    pub dashboard_url: &'static str,
    /// Whether the provider serves an OpenAI-compatible `{api_url}/embeddings`
    /// endpoint (false for endpoint-discovery providers such as RunPod).
    pub serves_embeddings: bool,
}

/// Canonical inference provider registry shared by `kask_bridge` and
/// `hkask-inference` (eliminating `DIRECT_EMBEDDING_PROVIDERS` drift).
pub static INFERENCE_PROVIDERS: &[InferenceProviderDescriptor] = &[
    InferenceProviderDescriptor {
        id: "OpenRouter",
        name: "OpenRouter",
        api_url: "https://openrouter.ai/api/v1",
        env_var: "OPENROUTER_API_KEY",
        credential_key: "openrouter",
        dashboard_url: "https://openrouter.ai/",
        serves_embeddings: true,
    },
    InferenceProviderDescriptor {
        id: "RunPod",
        name: "RunPod",
        api_url: "https://api.runpod.io",
        env_var: "RUNPOD_API_KEY",
        credential_key: "runpod",
        dashboard_url: "https://www.runpod.io/",
        serves_embeddings: false,
    },
    InferenceProviderDescriptor {
        id: "ollama",
        name: "Ollama",
        api_url: "http://localhost:11434/v1",
        env_var: "",
        credential_key: "ollama",
        dashboard_url: "https://ollama.com/",
        serves_embeddings: true,
    },
    InferenceProviderDescriptor {
        id: "DeepInfra",
        name: "DeepInfra",
        api_url: "https://api.deepinfra.com/v1/openai",
        env_var: "DEEPINFRA_API_KEY",
        credential_key: "deepinfra",
        dashboard_url: "https://deepinfra.com/",
        serves_embeddings: true,
    },
];

/// Case-insensitive ASCII prefix strip (`str::get` guards UTF-8 boundaries).
#[must_use]
pub fn strip_prefix_ascii_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// Look up an inference provider by its `id` (ASCII case-insensitive),
/// including keyless local providers (`ollama`).
#[must_use]
pub fn provider_by_id(id: &str) -> Option<&'static InferenceProviderDescriptor> {
    INFERENCE_PROVIDERS
        .iter()
        .find(|provider| provider.id.eq_ignore_ascii_case(id))
}

/// Split a provider-qualified model string (`"<Provider>/<model_id>"`) and
/// resolve its [`InferenceProviderDescriptor`]. Rejects unqualified strings and
/// empty model suffixes.
#[must_use]
pub fn split_provider_model(model: &str) -> Option<(&'static InferenceProviderDescriptor, &str)> {
    let (prefix, model_id) = model.split_once('/')?;
    if model_id.is_empty() {
        return None;
    }
    let provider = provider_by_id(prefix)?;
    Some((provider, model_id))
}

/// Resolve an embedding-capable provider descriptor and bare `model_id` from a
/// provider-qualified model string (`serves_embeddings == true`).
#[must_use]
pub fn embedding_provider_descriptor(
    embedding_model: &str,
) -> Option<(&'static InferenceProviderDescriptor, &str)> {
    let (provider, model_id) = split_provider_model(embedding_model)?;
    provider.serves_embeddings.then_some((provider, model_id))
}

// ── Transport Error Chain & OpenRouter Error Envelopes ────────────────────

/// Format `err` together with its full `std::error::Error::source()` chain so
/// `reqwest::Error` surfaces underlying hyper, TLS, DNS, or timeout causes
/// instead of masking them behind `error sending request for url (...)`.
#[must_use]
pub fn format_error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut message = err.to_string();
    let mut current = err.source();
    while let Some(cause) = current {
        let cause_text = cause.to_string();
        if !cause_text.is_empty() && !message.contains(&cause_text) {
            message.push_str(": ");
            message.push_str(&cause_text);
        }
        current = cause.source();
    }
    message
}

#[derive(Debug, serde::Deserialize)]
struct ProviderErrorEnvelopeWire {
    error: Option<ProviderErrorBodyWire>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum ProviderErrorBodyWire {
    Structured(ProviderErrorDetailWire),
    Plain(String),
}

#[derive(Debug, serde::Deserialize)]
struct ProviderErrorDetailWire {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

fn parse_error_status_code(code: Option<&serde_json::Value>) -> Option<u16> {
    let value = code?;
    if let Some(num) = value.as_u64() {
        return u16::try_from(num)
            .ok()
            .filter(|status| (400..=599).contains(status));
    }
    if let Some(text) = value.as_str() {
        return text
            .parse::<u16>()
            .ok()
            .filter(|status| (400..=599).contains(status));
    }
    None
}

fn format_provider_error_body(error: ProviderErrorBodyWire, fallback_status: u16) -> (u16, String) {
    match error {
        ProviderErrorBodyWire::Plain(msg) => (fallback_status, sanitize_error_body(&msg)),
        ProviderErrorBodyWire::Structured(detail) => {
            let status = parse_error_status_code(detail.code.as_ref()).unwrap_or(fallback_status);
            let mut parts = Vec::new();
            if let Some(msg) = detail.message.filter(|m| !m.trim().is_empty()) {
                parts.push(msg);
            }
            if let Some(code_val) = detail.code.as_ref()
                && parse_error_status_code(Some(code_val)).is_none()
            {
                parts.push(format!("code={code_val}"));
            }
            if let Some(meta) = detail.metadata.as_ref() {
                if let Some(provider_name) = meta.get("provider_name").and_then(|v| v.as_str()) {
                    parts.push(format!("upstream={provider_name}"));
                }
                if let Some(raw) = meta.get("raw").and_then(|v| v.as_str())
                    && !raw.trim().is_empty()
                {
                    parts.push(format!("raw={raw}"));
                }
            }
            let combined = if parts.is_empty() {
                "provider returned an empty error object".to_string()
            } else {
                parts.join(" | ")
            };
            (status, sanitize_error_body(&combined))
        }
    }
}

/// Detect an OpenRouter / OpenAI `{"error": ...}` JSON envelope in `body_text`
/// (including OpenRouter post-header errors delivered under HTTP `200 OK`).
/// Returns `(effective_status_code, sanitized_message)` when a non-null `error`
/// field is present.
#[must_use]
pub fn extract_provider_error_envelope(
    body_text: &str,
    fallback_status: u16,
) -> Option<(u16, String)> {
    let trimmed = body_text.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }
    let parsed: ProviderErrorEnvelopeWire = serde_json::from_str(trimmed).ok()?;
    let error = parsed.error?;
    Some(format_provider_error_body(error, fallback_status))
}

/// Detect an OpenRouter / OpenAI `{"error": ...}` envelope in an already-parsed
/// [`serde_json::Value`].
#[must_use]
pub fn extract_provider_error_from_value(
    value: &serde_json::Value,
    fallback_status: u16,
) -> Option<(u16, String)> {
    let err_val = value.get("error")?;
    if err_val.is_null() {
        return None;
    }
    let error: ProviderErrorBodyWire = serde_json::from_value(err_val.clone()).ok()?;
    Some(format_provider_error_body(error, fallback_status))
}

// ── Shared OpenAI / OpenRouter `/embeddings` Wire Protocol ────────────────

#[derive(Debug, serde::Deserialize)]
struct OpenAiEmbedResponseWire {
    #[serde(default)]
    data: Option<Vec<OpenAiEmbeddingDataWire>>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<OpenAiEmbedUsageWire>,
    #[serde(default)]
    error: Option<ProviderErrorBodyWire>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAiEmbedUsageWire {
    prompt_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAiEmbeddingDataWire {
    #[serde(default)]
    index: Option<usize>,
    embedding: Vec<f32>,
}

/// Validate inputs and build the canonical OpenAI/OpenRouter `/embeddings`
/// JSON request body.
///
/// Enforces:
/// - non-empty `texts` slice (`EmptyResponse` on `&[]`),
/// - non-blank `model_id` and non-blank `input` strings (`InvalidRequest`,
///   matching OpenRouter's `minLength: 1` item schema),
/// - positive `dimensions` when specified (`InvalidRequest` on `Some(0)`),
/// - explicit `"encoding_format": "float"` so both `LanguageModelEmbeddingPort`
///   and `DirectEmbeddingPort` always receive float arrays rather than base64.
pub fn build_embedding_request_body(
    api_url: &str,
    model_id: &str,
    texts: &[String],
    dimensions: Option<u32>,
) -> Result<serde_json::Value, hkask_types::EmbeddingGenerationError> {
    if texts.is_empty() {
        return Err(hkask_types::EmbeddingGenerationError::EmptyResponse);
    }
    if model_id.trim().is_empty() {
        return Err(hkask_types::EmbeddingGenerationError::InvalidRequest(
            "embedding model id must not be empty".to_string(),
        ));
    }
    if let Some((index, _)) = texts
        .iter()
        .enumerate()
        .find(|(_, text)| text.trim().is_empty())
    {
        return Err(hkask_types::EmbeddingGenerationError::InvalidRequest(
            format!("embedding input at index {index} must not be empty or whitespace-only"),
        ));
    }
    if dimensions == Some(0) {
        return Err(hkask_types::EmbeddingGenerationError::InvalidRequest(
            "embedding dimensions must be greater than zero".to_string(),
        ));
    }

    let mut body = serde_json::json!({
        "model": model_id,
        "input": texts,
        "encoding_format": "float",
    });
    if let Some(dimensions) = dimensions
        && let Some(map) = body.as_object_mut()
    {
        map.insert("dimensions".to_string(), serde_json::json!(dimensions));
    }
    if api_url.to_ascii_lowercase().contains("openrouter.ai")
        && let Some(map) = body.as_object_mut()
    {
        let provider = if model_id.to_ascii_lowercase().starts_with("qwen/") {
            serde_json::json!({
                "only": ["DeepInfra"],
                "allow_fallbacks": false,
            })
        } else {
            serde_json::json!({
                "sort": "latency",
            })
        };
        map.insert("provider".to_string(), provider);
    }
    Ok(body)
}

/// Parse and validate an `/embeddings` HTTP response into an [`hkask_types::EmbeddingBatch`].
///
/// Handles:
/// - Non-2xx status codes with structured error extraction and secret redaction,
/// - OpenRouter HTTP `200 OK` post-header `{"error": ...}` envelopes (classified
///   as `EmbeddingGenerationError::Api(code, msg)` instead of JSON parse errors),
/// - Exact batch count reconciliation (`data.len() == expected_count`),
/// - Deterministic reordering by `data[].index` (with bounds and duplicate checks
///   when `index` is supplied; arrival-order fallback when all `index` fields are
///   omitted in minimal test fixtures),
/// - Rejection of empty `embedding: []` vectors.
pub fn parse_embedding_response(
    status_code: u16,
    body_text: &str,
    requested_model: String,
    expected_count: usize,
) -> Result<hkask_types::EmbeddingBatch, hkask_types::EmbeddingGenerationError> {
    if !(200..300).contains(&status_code) {
        let message = extract_provider_error_envelope(body_text, status_code)
            .map(|(_, msg)| msg)
            .unwrap_or_else(|| sanitize_error_body(body_text));
        return Err(hkask_types::EmbeddingGenerationError::Api(
            status_code,
            message,
        ));
    }

    let parsed: OpenAiEmbedResponseWire = serde_json::from_str(body_text).map_err(|e| {
        hkask_types::EmbeddingGenerationError::Json(format!(
            "failed to parse embedding response: {e} (body: {})",
            sanitize_error_body(body_text)
        ))
    })?;

    if let Some(error) = parsed.error {
        let (code, message) = format_provider_error_body(error, 502);
        return Err(hkask_types::EmbeddingGenerationError::Api(code, message));
    }

    let data = parsed.data.ok_or_else(|| {
        hkask_types::EmbeddingGenerationError::Json(format!(
            "embedding response missing 'data' field (body: {})",
            sanitize_error_body(body_text)
        ))
    })?;

    if data.is_empty() {
        return Err(hkask_types::EmbeddingGenerationError::EmptyResponse);
    }

    if data.len() != expected_count {
        return Err(hkask_types::EmbeddingGenerationError::Json(format!(
            "embedding batch count mismatch: requested {expected_count} inputs, provider returned {} vectors",
            data.len()
        )));
    }

    let has_any_index = data.iter().any(|item| item.index.is_some());
    let has_all_indices = data.iter().all(|item| item.index.is_some());
    let vectors = if has_any_index {
        if !has_all_indices {
            return Err(hkask_types::EmbeddingGenerationError::Json(
                "embedding response mixes indexed and unindexed items".to_string(),
            ));
        }
        let mut slots: Vec<Option<Vec<f32>>> = vec![None; expected_count];
        for item in data {
            let idx = item.index.unwrap_or(usize::MAX);
            if idx >= expected_count {
                return Err(hkask_types::EmbeddingGenerationError::Json(format!(
                    "embedding item index {idx} out of bounds for batch of {expected_count}"
                )));
            }
            if slots[idx].is_some() {
                return Err(hkask_types::EmbeddingGenerationError::Json(format!(
                    "duplicate embedding item index {idx} in provider response"
                )));
            }
            if item.embedding.is_empty() {
                return Err(hkask_types::EmbeddingGenerationError::EmptyResponse);
            }
            slots[idx] = Some(item.embedding);
        }
        slots
            .into_iter()
            .enumerate()
            .map(|(idx, slot)| {
                slot.ok_or_else(|| {
                    hkask_types::EmbeddingGenerationError::Json(format!(
                        "missing embedding vector for batch index {idx}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let mut ordered = Vec::with_capacity(expected_count);
        for item in data {
            if item.embedding.is_empty() {
                return Err(hkask_types::EmbeddingGenerationError::EmptyResponse);
            }
            ordered.push(item.embedding);
        }
        ordered
    };

    Ok(hkask_types::EmbeddingBatch {
        vectors,
        requested_model,
        actual_model: parsed.model,
        usage: match parsed.usage {
            Some(OpenAiEmbedUsageWire {
                prompt_tokens,
                total_tokens,
            }) => hkask_types::InferenceUsage {
                prompt_tokens,
                completion_tokens: 0,
                total_tokens: if total_tokens == 0 {
                    prompt_tokens
                } else {
                    total_tokens
                },
                reported: true,
            },
            None => hkask_types::InferenceUsage::default(),
        },
    })
}

/// Default retrieval instruction for asymmetric Qwen3-Embedding query formatting
/// (Zhang et al., 2025, `arXiv:2506.05176`). Documents embed as raw text;
/// omitting the query-side instruction drops retrieval quality by 1%–5%.
pub const QWEN3_RETRIEVAL_QUERY_INSTRUCTION: &str =
    "Given a technical search query, retrieve relevant reference passages that answer the query";

/// Format a retrieval query for asymmetric embedding models (`arXiv:2506.05176`).
///
/// Wraps queries for `qwen3-embedding*` models in `Instruct: <task>\nQuery: <query>`.
/// Non-Qwen3 models and already-formatted queries pass through unchanged.
#[must_use]
pub fn format_retrieval_query_for_model(model: &str, query: &str) -> String {
    if model.to_ascii_lowercase().contains("qwen3-embedding") && !query.starts_with("Instruct: ") {
        format!("Instruct: {QWEN3_RETRIEVAL_QUERY_INSTRUCTION}\nQuery: {query}")
    } else {
        query.to_string()
    }
}

/// Execute an OpenAI-compatible `/embeddings` HTTP POST over `reqwest` using
/// the shared wire builder and parser. Used by `DirectEmbeddingPort` and by
/// `InferenceIpcServer`'s cross-provider / local-ollama embedding dispatch.
pub async fn embed_via_reqwest(
    client: &reqwest::Client,
    api_url: &str,
    api_key: &str,
    requested_model: &str,
    model_id: &str,
    texts: &[String],
    dimensions: Option<u32>,
) -> Result<hkask_types::EmbeddingBatch, hkask_types::EmbeddingGenerationError> {
    const EMBEDDING_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
    let body = build_embedding_request_body(api_url, model_id, texts, dimensions)?;
    let uri = format!("{}/embeddings", api_url.trim_end_matches('/'));
    let mut request = client
        .post(&uri)
        .header("Content-Type", "application/json")
        .json(&body);
    let trimmed_key = api_key.trim();
    if !trimmed_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {trimmed_key}"));
    }

    let (status, body_text) = tokio::time::timeout(EMBEDDING_HTTP_TIMEOUT, async {
        let response = request.send().await.map_err(|e| {
            hkask_types::EmbeddingGenerationError::Connection(format!(
                "embedding HTTP request failed ({uri}): {}",
                format_error_chain(&e)
            ))
        })?;

        let status = response.status().as_u16();
        let body_text = response.text().await.map_err(|e| {
            hkask_types::EmbeddingGenerationError::Connection(format!(
                "failed to read embedding response body ({uri}): {}",
                format_error_chain(&e)
            ))
        })?;
        Ok::<_, hkask_types::EmbeddingGenerationError>((status, body_text))
    })
    .await
    .map_err(|_| {
        hkask_types::EmbeddingGenerationError::Connection(format!(
            "embedding HTTP request timed out after {}s ({uri})",
            EMBEDDING_HTTP_TIMEOUT.as_secs()
        ))
    })??;

    parse_embedding_response(status, &body_text, requested_model.to_string(), texts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_embedding_request_pins_qwen_to_deepinfra_and_sorts_others_by_latency() {
        let or_qwen = build_embedding_request_body(
            "https://openrouter.ai/api/v1",
            "qwen/qwen3-embedding-8b",
            &["hello".to_string()],
            Some(1024),
        )
        .expect("openrouter qwen body builds");
        assert_eq!(
            or_qwen["provider"]["only"],
            serde_json::json!(["DeepInfra"])
        );
        assert_eq!(
            or_qwen["provider"]["allow_fallbacks"],
            serde_json::json!(false)
        );

        let or_other = build_embedding_request_body(
            "https://openrouter.ai/api/v1",
            "openai/text-embedding-3-small",
            &["hello".to_string()],
            Some(1024),
        )
        .expect("openrouter other body builds");
        assert_eq!(or_other["provider"]["sort"], serde_json::json!("latency"));

        let direct_body = build_embedding_request_body(
            "https://api.deepinfra.com/v1/openai",
            "Qwen/Qwen3-Embedding-8B",
            &["hello".to_string()],
            Some(1024),
        )
        .expect("direct deepinfra body builds");
        assert!(direct_body.get("provider").is_none());
    }

    #[test]
    fn qwen3_retrieval_query_formatting_is_asymmetric_and_idempotent() {
        let formatted = format_retrieval_query_for_model(
            "OpenRouter/qwen/qwen3-embedding-8b",
            "NIP-65 relay list metadata",
        );
        assert!(formatted.starts_with("Instruct: "));
        assert!(formatted.ends_with("\nQuery: NIP-65 relay list metadata"));

        // Idempotent: already-formatted queries are never double-wrapped.
        assert_eq!(
            format_retrieval_query_for_model("OpenRouter/qwen/qwen3-embedding-8b", &formatted),
            formatted
        );

        // Non-Qwen3 models pass through unchanged.
        assert_eq!(
            format_retrieval_query_for_model(
                "OpenRouter/openai/text-embedding-3-small",
                "NIP-65 relay list metadata"
            ),
            "NIP-65 relay list metadata"
        );
    }
    use hkask_types::EmbeddingGenerationError;

    #[test]
    fn build_request_body_includes_float_encoding_and_validates_inputs() {
        let url = "https://api.deepinfra.com/v1/openai";
        let body = build_embedding_request_body(
            url,
            "qwen/qwen3-embedding-8b",
            &["hello".to_string(), "world".to_string()],
            Some(1024),
        )
        .expect("valid request builds");
        assert_eq!(body["model"], "qwen/qwen3-embedding-8b");
        assert_eq!(body["encoding_format"], "float");
        assert_eq!(body["dimensions"], 1024);
        assert_eq!(body["input"], serde_json::json!(["hello", "world"]));

        assert!(matches!(
            build_embedding_request_body(url, "qwen/qwen3-embedding-8b", &[], Some(1024)),
            Err(EmbeddingGenerationError::EmptyResponse)
        ));
        assert!(matches!(
            build_embedding_request_body(
                url,
                "qwen/qwen3-embedding-8b",
                &["ok".to_string(), "   ".to_string()],
                Some(1024)
            ),
            Err(EmbeddingGenerationError::InvalidRequest(_))
        ));
        assert!(matches!(
            build_embedding_request_body(
                url,
                "qwen/qwen3-embedding-8b",
                &["ok".to_string()],
                Some(0)
            ),
            Err(EmbeddingGenerationError::InvalidRequest(_))
        ));
        assert!(matches!(
            build_embedding_request_body(url, "  ", &["ok".to_string()], None),
            Err(EmbeddingGenerationError::InvalidRequest(_))
        ));
    }

    #[test]
    fn parse_response_surfaces_openrouter_http_200_error_envelope_as_api_error() {
        let body = r#"{
            "error": {
                "code": 429,
                "message": "Rate limit exceeded on upstream sk-or-v1-secret123456",
                "metadata": { "provider_name": "DeepInfra", "raw": "upstream 429" }
            }
        }"#;
        let err = parse_embedding_response(
            200,
            body,
            "OpenRouter/qwen/qwen3-embedding-8b".to_string(),
            1,
        )
        .expect_err("HTTP 200 error envelope must not fail as missing field data");
        match err {
            EmbeddingGenerationError::Api(status, msg) => {
                assert_eq!(status, 429);
                assert!(msg.contains("Rate limit exceeded"), "{msg}");
                assert!(msg.contains("upstream=DeepInfra"), "{msg}");
                assert!(
                    !msg.contains("secret123456"),
                    "secret must be redacted: {msg}"
                );
            }
            other => panic!("expected Api(429, ...), got {other:?}"),
        }
    }

    #[test]
    fn parse_response_reorders_by_index_and_rejects_mismatched_or_duplicate_batches() {
        let out_of_order = r#"{
            "object": "list",
            "data": [
                {"object": "embedding", "index": 1, "embedding": [0.0, 2.0]},
                {"object": "embedding", "index": 0, "embedding": [1.0, 0.0]}
            ],
            "model": "qwen/qwen3-embedding-8b",
            "usage": {"prompt_tokens": 9}
        }"#;
        let batch = parse_embedding_response(
            200,
            out_of_order,
            "OpenRouter/qwen/qwen3-embedding-8b".to_string(),
            2,
        )
        .expect("indexed out-of-order batch succeeds");
        assert_eq!(batch.vectors, vec![vec![1.0, 0.0], vec![0.0, 2.0]]);
        assert_eq!(batch.usage.prompt_tokens, 9);
        assert_eq!(
            batch.usage.total_tokens, 9,
            "total_tokens falls back to prompt_tokens"
        );
        assert!(batch.usage.reported);

        // Count mismatch (requested 2, got 1) fails closed.
        let partial = r#"{"data":[{"index":0,"embedding":[1.0,0.0]}]}"#;
        assert!(matches!(
            parse_embedding_response(200, partial, "OpenRouter/m".to_string(), 2),
            Err(EmbeddingGenerationError::Json(_))
        ));

        // Duplicate index fails closed.
        let duplicate_idx =
            r#"{"data":[{"index":0,"embedding":[1.0]},{"index":0,"embedding":[2.0]}]}"#;
        assert!(matches!(
            parse_embedding_response(200, duplicate_idx, "OpenRouter/m".to_string(), 2),
            Err(EmbeddingGenerationError::Json(_))
        ));
    }

    #[test]
    fn format_error_chain_includes_nested_sources() {
        #[derive(Debug, thiserror::Error)]
        #[error("inner TLS handshake reset")]
        struct Inner;

        #[derive(Debug, thiserror::Error)]
        #[error("outer request failed")]
        struct Outer(#[source] Inner);

        let err = Outer(Inner);
        assert_eq!(
            format_error_chain(&err),
            "outer request failed: inner TLS handshake reset"
        );
    }
}
