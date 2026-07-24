//! Model listing routes
//!
//! Two endpoints for discovering and switching LLM models across providers:
//!
//! - `GET /api/models` — List all available models (DeepInfra, fal.ai, Together AI, OpenRouter, KiloCode)
//! - `GET /api/models/search?q=...` — Fuzzy search models by name
//!
//! Model names use a 2-letter provider prefix (DI/, FA/, TG/, OR/).
//! Returned names can be passed as the `model` field in
//! `POST /api/chat` requests to select which LLM the Curator or agent uses.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use utoipa::IntoParams;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;

/// Create models router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with model routes registered
pub fn models_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_models))
        .routes(routes!(search_models))
}

/// A model available through the inference router.
///
/// Includes metadata from the provider's model listing endpoint:
/// model family, parameter count, quantization level, and disk size.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelEntry {
    /// Model identifier (e.g., "qwen3:8b", "llama3.1:70b")
    pub name: String,
    /// Model family (e.g., "llama", "qwen2")
    pub family: Option<String>,
    /// Parameter count (e.g., "8B", "70B")
    pub parameter_size: Option<String>,
    /// Quantization level (e.g., "Q4_0", "Q5_K_M")
    pub quantization_level: Option<String>,
    /// Model size in gigabytes
    pub size_gb: Option<f64>,
}

/// Response containing available models.
///
/// Returned by `GET /api/models` and `GET /api/models/search`.
/// Model names from this response can be used as the `model` field
/// in `POST /api/chat` requests.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelListResponse {
    /// List of available models
    pub models: Vec<ModelEntry>,
    /// Total number of models in the list
    pub count: usize,
}

/// Query parameters for fuzzy model search.
///
/// The `q` parameter performs case-insensitive substring matching
/// against model names. For example, `q=llama` matches "llama3.1:8b",
/// "llama3.1:70b", etc.
#[derive(Debug, Clone, Deserialize, IntoParams, ToSchema)]
pub struct ModelSearchQuery {
    /// Fuzzy search query — matches model name (case-insensitive substring)
    pub q: String,
}

/// List all available models from all configured providers.
///
/// Queries DeepInfra, fal.ai, Together AI, OpenRouter, and KiloCode and returns metadata for each
/// available model with provider prefix applied. Returns an empty list if
/// no providers are reachable (graceful degradation).
#[utoipa::path(
    get,
    path = "/api/models",
    tag = "models",
    responses(
        (status = 200, description = "List of available models", body = ModelListResponse),
        (status = 503, description = "No providers reachable (returns empty list)"),
    ),
)]
pub(crate) async fn list_models(State(state): State<ApiState>) -> Json<ModelListResponse> {
    use hkask_services_inference::InferenceService;

    let ctx = hkask_services_inference::InferenceContext::from(&*state.agent_service);
    let models = InferenceService::list_models(&ctx)
        .await
        .unwrap_or_default();

    let models: Vec<ModelEntry> = models
        .into_iter()
        .map(|m| ModelEntry {
            name: m.name,
            family: m.family,
            parameter_size: m.parameter_size,
            quantization_level: m.quantization_level,
            size_gb: m.size_bytes.map(|s| s as f64 / 1_073_741_824.0),
        })
        .collect();

    let count = models.len();
    Json(ModelListResponse { models, count })
}

/// Search available models by name (fuzzy matching).
///
/// Performs case-insensitive substring matching against model names
/// across all providers. Use this endpoint to discover valid model
/// identifiers before passing them to `POST /api/chat` via the `model` field.
#[utoipa::path(
    get,
    path = "/api/models/search",
    tag = "models",
    params(
        ("q" = String, Query, description = "Fuzzy search query — matches model name (case-insensitive substring)")
    ),
    responses(
        (status = 200, description = "Matching models", body = ModelListResponse),
    ),
)]
pub(crate) async fn search_models(
    State(state): State<ApiState>,
    axum::extract::Query(query): axum::extract::Query<ModelSearchQuery>,
) -> Json<ModelListResponse> {
    use hkask_services_inference::InferenceService;

    let ctx = hkask_services_inference::InferenceContext::from(&*state.agent_service);
    let models = InferenceService::search_models(&ctx, &query.q)
        .await
        .unwrap_or_default();

    let models: Vec<ModelEntry> = models
        .into_iter()
        .map(|m| ModelEntry {
            name: m.name,
            family: m.family,
            parameter_size: m.parameter_size,
            quantization_level: m.quantization_level,
            size_gb: m.size_bytes.map(|s| s as f64 / 1_073_741_824.0),
        })
        .collect();

    let count = models.len();
    Json(ModelListResponse { models, count })
}
