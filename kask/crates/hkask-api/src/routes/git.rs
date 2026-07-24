//! Git CAS archival and resolution routes
//!
//! Routes all git operations through the `GitCASPort` hexagonal boundary.
//! The archive route still uses `TemplateCrateLoader::load_template_crate` for
//! template loading (a domain operation, not a CAS operation), while
//! SHA resolution uses `GixCasAdapter::resolve_ref`.

use axum::extract::Extension;
use axum::{Json, extract::Path, extract::State};
use hkask_services_core::ServiceError;
use hkask_types::InfrastructureError;
use hkask_types::git_cas::RepoId;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::AuthContext;

/// Archive repository request
#[derive(Debug, Deserialize, ToSchema)]
pub struct ArchiveRequest {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub path: String,
}

/// Archive entry response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ArchiveEntry {
    pub name: String,
    pub path: String,
    pub content: String,
    pub template_type: String,
}

/// Archive response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ArchiveResponse {
    pub crate_name: String,
    pub git_sha: String,
    pub templates: Vec<ArchiveEntry>,
}

/// SHA resolve response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ResolveShaResponse {
    pub sha: String,
}

/// Create git router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with git routes registered
pub fn git_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(archive))
        .routes(routes!(resolve_sha))
}

/// Archive a repository template crate
///
/// Uses `TemplateCrateLoader::load_template_crate` for template loading (domain
/// operation) and `GixCasAdapter::resolve_ref` for SHA resolution.
#[utoipa::path(
    post,
    path = "/api/v1/git/archive",
    tag = "git",
    request_body = ArchiveRequest,
    responses(
        (status = 200, description = "Template crate archived", body = ArchiveResponse),
        (status = 400, description = "Invalid path or missing crate"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn archive(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Json(req): Json<ArchiveRequest>,
) -> Result<Json<ArchiveResponse>, ServiceErrorResponse> {
    // Construct crate name from owner/repo
    let crate_name = format!("{}/{}", req.owner, req.repo);

    // Template loading uses the disk-based adapter (not CAS)
    let template_adapter = state.template_adapter.clone();
    let template_crate = template_adapter
        .load_template_crate(&crate_name)
        .map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
        })?;

    // SHA resolution uses GixCasAdapter directly (admin operation, not backup)
    let sha = state
        .gix_cas
        .resolve_ref(&RepoId::Registry, "HEAD")
        .await
        .map(|commit| commit.to_string())
        .map_err(|e| {
            ServiceErrorResponse(ServiceError::Infra(InfrastructureError::NotFound(format!(
                "Failed to resolve git HEAD: {e}"
            ))))
        })?;

    let templates: Vec<ArchiveEntry> = template_crate
        .templates
        .iter()
        .map(|t| ArchiveEntry {
            name: t.path.clone(),
            path: t.path.clone(),
            content: t.content.clone(),
            template_type: t.template_type.clone(),
        })
        .collect();

    Ok(Json(ArchiveResponse {
        crate_name: template_crate.name,
        git_sha: sha,
        templates,
    }))
}

/// Resolve a git reference to a SHA using GixCasAdapter
#[utoipa::path(
    get,
    path = "/api/v1/git/resolve/{sha}",
    tag = "git",
    params(
        ("sha" = String, Path, description = "Git reference to resolve (branch, tag, or commit prefix)"),
    ),
    responses(
        (status = 200, description = "SHA resolved", body = ResolveShaResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn resolve_sha(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Path(reference): Path<String>,
) -> Result<Json<ResolveShaResponse>, ServiceErrorResponse> {
    let commit = state
        .gix_cas
        .resolve_ref(&RepoId::Registry, &reference)
        .await
        .map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::database(format!(
                "Failed to resolve ref '{}': {}",
                reference, e
            )))
        })?;

    Ok(Json(ResolveShaResponse {
        sha: commit.to_string(),
    }))
}
