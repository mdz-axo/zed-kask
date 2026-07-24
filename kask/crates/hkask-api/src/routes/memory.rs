//! Paired memory API routes — combined episodic + semantic recall.
//!
//! Mirrors the dual-recall circuit in ChatService::prepare_chat where
//! both memory types are recalled together and merged into context.

use axum::extract::Extension;
use axum::{Json, extract::Query, extract::State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::AuthContext;
use crate::routes::episodic::EpisodeResponse;
use hkask_memory::RecallRequest;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct QueryMemoryParams {
    pub entity: String,
    pub include_semantic: Option<bool>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SemanticTripleResponse {
    pub entity: String,
    pub attribute: String,
    pub value: serde_json::Value,
    pub confidence: f64,
    pub visibility: String,
    pub observed_at: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct MemoryRecallResponse {
    pub entity: String,
    pub episodic: Vec<EpisodeResponse>,
    pub semantic: Option<Vec<SemanticTripleResponse>>,
}

pub fn memory_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(memory_recall))
}

#[utoipa::path(
    get,
    path = "/api/memory/recall",
    tag = "memory",
    params(
        ("entity" = String, Query, description = "Entity to recall across both stores"),
        ("include_semantic" = Option<bool>, Query, description = "Include semantic results"),
    ),
    responses(
        (status = 200, description = "Memory recalled", body = MemoryRecallResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Query error"),
    ),
)]
pub(crate) async fn memory_recall(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Query(params): Query<QueryMemoryParams>,
) -> Result<Json<MemoryRecallResponse>, ServiceErrorResponse> {
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

    let episodic_request = RecallRequest::episodic(&params.entity, auth.webid, token.clone());
    let episodic_results = state
        .agent_service
        .infra()
        .episodic
        .recall_episodic(&episodic_request)
        .map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
        })?;
    let episodic: Vec<EpisodeResponse> = episodic_results
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

    let semantic = if params.include_semantic.unwrap_or(false) {
        let semantic_port = &state.agent_service.infra().semantic;
        let semantic_request = RecallRequest::semantic(&params.entity, token.clone());
        let semantic_results = semantic_port
            .recall_semantic(&semantic_request)
            .map_err(|e| {
                ServiceError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
            })?;
        Some(
            semantic_results
                .into_iter()
                .map(|t| SemanticTripleResponse {
                    entity: t.entity,
                    attribute: t.attribute,
                    value: t.value,
                    confidence: t.confidence.value(),
                    visibility: t.visibility.as_str().to_string(),
                    observed_at: t.observed_at,
                })
                .collect(),
        )
    } else {
        None
    };

    Ok(Json(MemoryRecallResponse {
        entity: params.entity,
        episodic,
        semantic,
    }))
}
