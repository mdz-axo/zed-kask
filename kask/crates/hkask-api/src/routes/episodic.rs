//! Episodic Memory API routes.
//!
//! HTTP surface for episodic memory operations, routed through
//! `EpisodicStoragePort` for OCAP discipline. All requests carry a
//! `DelegationToken` via the HTTP auth middleware (`AuthContext`).

use axum::extract::Extension;
use axum::{Json, extract::Query, extract::State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::AuthContext;
use hkask_memory::{RecallRequest, StorageRequest};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::Confidence;

/// Create episodic memory router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with episodic routes registered
pub fn episodic_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(store_episode))
        .routes(routes!(query_episodes))
        .routes(routes!(storage_usage))
}

/// Request to store an episodic h_mem.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StoreEpisodeRequest {
    /// Entity (subject of the experience).
    pub entity: String,
    /// Attribute (predicate/property).
    pub attribute: String,
    /// Value (object of the h_mem).
    pub value: serde_json::Value,
    /// Confidence (0.0–1.0). Defaults to 1.0.
    pub confidence: Option<f64>,
}

/// Response from storing an episodic h_mem.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StoreEpisodeResponse {
    /// The entity that was stored.
    pub entity: String,
    /// The attribute that was stored.
    pub attribute: String,
}

/// Query parameters for episodic recall.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct QueryEpisodesParams {
    /// Entity to query by.
    pub entity: String,
}

/// A single episodic h_mem as returned over the API.
///
/// Each episode is a (entity, attribute, value) h_mem with confidence,
/// visibility governance (P11), and a temporal validity marker (`valid_from`).
/// `visibility` is "private" for episodic memory.
/// `perspective` is the WebID of the observing agent (P12).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct EpisodeResponse {
    /// Unique episode ID
    pub id: String,
    /// Entity (subject of the experience)
    pub entity: String,
    /// Attribute (predicate / property)
    pub attribute: String,
    /// Value (object of the h_mem)
    pub value: serde_json::Value,
    /// Confidence score (0.0–1.0)
    pub confidence: f64,
    /// WebID of the observing agent (P12)
    pub perspective: Option<String>,
    /// Visibility: "private" (P11)
    pub visibility: String,
    /// ISO 8601 timestamp when this memory was observed
    pub observed_at: String,
}

/// Response from querying episodic memories.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct QueryEpisodesResponse {
    /// Retrieved episodic h_mems
    pub episodes: Vec<EpisodeResponse>,
}

/// Response for episodic storage usage.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct EpisodicUsageResponse {
    /// Current h_mem count for the caller's perspective.
    pub count: usize,
    /// Configured storage budget (max h_mems).
    pub budget: usize,
}

/// Store an episodic h_mem for the authenticated caller.
///
/// Routes through `EpisodicStoragePort` with OCAP discipline:
/// the `DelegationToken` from HTTP auth is verified at the membrane.
#[utoipa::path(
    post,
    path = "/api/episodic/store",
    tag = "episodic",
    request_body = StoreEpisodeRequest,
    responses(
        (status = 200, description = "Episode stored", body = StoreEpisodeResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Storage error"),
    ),
)]
pub(crate) async fn store_episode(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<StoreEpisodeRequest>,
) -> Result<Json<StoreEpisodeResponse>, ServiceErrorResponse> {
    if req.entity.is_empty() || req.attribute.is_empty() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "entity and attribute must not be empty".to_string(),
        }
        .into());
    }

    let confidence = Confidence::new(req.confidence.unwrap_or(1.0));
    let request = StorageRequest::episodic(
        &req.entity,
        &req.attribute,
        req.value,
        confidence,
        auth.webid,
    );
    state
        .agent_service
        .infra()
        .episodic
        .store_episodic(
            request,
            auth.token.as_ref().ok_or_else(|| {
                ServiceError::Infra(hkask_types::InfrastructureError::database(
                    "Session auth not supported for episodic storage".to_string(),
                ))
            })?,
        )
        .map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
        })?;

    tracing::debug!(
        target: "reg.memory.episodic",
        entity = %req.entity,
        attribute = %req.attribute,
        confidence = %confidence,
        "Episodic h_mem stored via API (through port membrane)"
    );

    Ok(Json(StoreEpisodeResponse {
        entity: req.entity,
        attribute: req.attribute,
    }))
}

/// Query episodic memories for the authenticated caller by entity.
///
/// Routes through `EpisodicStoragePort` with OCAP discipline.
/// Applies confidence decay, temporal attention weighting, and deduplication
/// (subloops 2a.2–2a.4) before returning results.
#[utoipa::path(
    get,
    path = "/api/episodic/query",
    tag = "episodic",
    params(
        ("entity" = String, Query, description = "Entity to query"),
    ),
    responses(
        (status = 200, description = "Episodes retrieved", body = QueryEpisodesResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Query error"),
    ),
)]
pub(crate) async fn query_episodes(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Query(params): Query<QueryEpisodesParams>,
) -> Result<Json<QueryEpisodesResponse>, ServiceErrorResponse> {
    if params.entity.is_empty() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "entity query parameter must not be empty".to_string(),
        }
        .into());
    }

    let token = auth
        .token
        .ok_or_else(|| ServiceError::Infra(hkask_types::InfrastructureError::LockPoisoned))?;
    let request = RecallRequest::episodic(&params.entity, auth.webid, token);
    let results = state
        .agent_service
        .infra()
        .episodic
        .recall_episodic(&request)
        .map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
        })?;

    let episodes: Vec<EpisodeResponse> = results
        .into_iter()
        .map(|ep| EpisodeResponse {
            id: ep.id,
            entity: ep.entity,
            attribute: ep.attribute,
            value: ep.value,
            confidence: ep.confidence.value(),
            perspective: ep.perspective.map(|p| p.to_string()),
            visibility: ep.visibility.as_str().to_string(),
            observed_at: ep.observed_at,
        })
        .collect();

    Ok(Json(QueryEpisodesResponse { episodes }))
}

/// Get episodic storage usage for the authenticated caller.
#[utoipa::path(
    get,
    path = "/api/episodic/usage",
    tag = "episodic",
    responses(
        (status = 200, description = "Storage usage", body = EpisodicUsageResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Query error"),
    ),
)]
pub(crate) async fn storage_usage(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<EpisodicUsageResponse>, ServiceErrorResponse> {
    let count = state
        .agent_service
        .infra()
        .episodic
        .episodic_storage_usage(&auth.webid)
        .map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
        })?;
    let budget = state
        .agent_service
        .infra()
        .episodic
        .episodic_storage_budget();

    Ok(Json(EpisodicUsageResponse { count, budget }))
}
