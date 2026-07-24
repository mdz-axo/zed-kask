//! Template management routes
//!
//! # Service layer depth test
//!
//! TemplateService was considered but **rejected** as shallow: every handler is a
//! thin delegation to a `SqliteRegistry` method plus HTTP response mapping. No
//! cross-surface business logic duplication exists (CLI template commands take
//! `&mut SqliteRegistry` directly and do terminal formatting). A TemplateService
//! would just be `self.storage().registry.list()` / `self.storage().registry.get()` / etc. — pure
//! pass-throughs that increase interface cost without adding behavior.
//!
//! Decision: Guideline — keep direct `service_context.storage().registry` access.
//! Revisit if template matching logic grows beyond name/skill/polarity queries.

use axum::Json;
use axum::extract::{Path, State};
use hkask_types::RegistryIndex;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Template response — a registered skill template in the WordAct / FlowDef / KnowAct taxonomy (Pattern A).
///
/// `template_type` is one of: WordAct, FlowDef, KnowAct.
/// `lexicon_terms` maps the template to canonical vocabulary terms.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TemplateResponse {
    /// Unique template identifier
    pub id: String,
    /// Template type: WordAct, FlowDef, or KnowAct
    pub template_type: String,
    /// Human-readable template name
    pub name: String,
    /// Template description
    pub description: String,
    /// Source file path within the registry
    pub source_path: String,
    /// Canonical vocabulary terms this template implements
    pub lexicon_terms: Vec<String>,
}

/// Create templates router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with template routes registered
pub fn templates_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_templates))
        .routes(routes!(get_template))
        .route(
            "/api/templates/search/{term}",
            axum::routing::get(search_templates),
        )
}

/// List templates
#[utoipa::path(
    get,
    path = "/api/templates",
    tag = "templates",
    responses(
        (status = 200, description = "List of templates", body = Vec<TemplateResponse>),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn list_templates(State(state): State<ApiState>) -> Json<Vec<TemplateResponse>> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "templates_list", "REG");
    let registry = state.agent_service.storage().registry.lock().await;
    let entries = registry.list(None);

    let templates = entries
        .iter()
        .map(|e| TemplateResponse {
            id: e.id.clone(),
            template_type: e.template_type.as_str().to_string(),
            name: e.name.clone(),
            description: e.description.clone(),
            source_path: e.source_path.clone(),
            lexicon_terms: e.lexicon_terms.clone(),
        })
        .collect();

    Json(templates)
}

/// Get template by ID
#[utoipa::path(
    get,
    path = "/api/templates/{id}",
    tag = "templates",
    params(
        ("id" = String, Path, description = "Template ID"),
    ),
    responses(
        (status = 200, description = "Template details", body = TemplateResponse),
        (status = 404, description = "Template not found"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn get_template(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<TemplateResponse>, ServiceErrorResponse> {
    let registry = state.agent_service.storage().registry.lock().await;

    let entry = registry.get(&id)?;

    Ok(Json(TemplateResponse {
        id: entry.id.clone(),
        template_type: entry.template_type.as_str().to_string(),
        name: entry.name.clone(),
        description: entry.description.clone(),
        source_path: entry.source_path.clone(),
        lexicon_terms: entry.lexicon_terms.clone(),
    }))
}

/// Search templates by lexicon term
async fn search_templates(
    State(state): State<ApiState>,
    Path(term): Path<String>,
) -> Json<Vec<TemplateResponse>> {
    let registry = state.agent_service.storage().registry.lock().await;
    let results = registry.search_by_lexicon(&term).unwrap_or_default();

    let templates = results
        .iter()
        .map(|e| TemplateResponse {
            id: e.id.clone(),
            template_type: e.template_type.as_str().to_string(),
            name: e.name.clone(),
            description: e.description.clone(),
            source_path: e.source_path.clone(),
            lexicon_terms: e.lexicon_terms.clone(),
        })
        .collect();

    Json(templates)
}
