//! User sovereignty routes — call consent manager directly.

use axum::extract::Extension;
use axum::{Json, extract::Query, extract::State};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::curation::{BoundaryClassification, DataSovereigntyBoundary};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::AuthContext;

fn consent_name(value: bool) -> &'static str {
    if value { "required" } else { "open" }
}

/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with sovereignty routes registered
pub fn sovereignty_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(sovereignty_status))
        .routes(routes!(sovereignty_grant_consent))
        .routes(routes!(sovereignty_revoke_consent))
        .routes(routes!(sovereignty_check_access))
}

/// Sovereignty status response — P1 (User Sovereignty) and P2 (Affirmative Consent).
///
/// Reflects the authenticated agent's current data sovereignty boundary:
/// which categories are sovereign (agent-only), shared (consent-gated), and
/// public (always accessible). `granted_categories` lists categories where
/// explicit consent has been given.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SovereigntyStatusResponse {
    /// Whether any explicit consent has been granted
    pub explicit_consent: bool,
    /// "required" if the boundary demands affirmative consent; "open" otherwise
    pub requires_affirmative_consent: String,
    /// Data categories that are agent-sovereign (never shared without consent)
    pub sovereign_data: Vec<String>,
    /// Data categories eligible for sharing after explicit consent
    pub shared_data: Vec<String>,
    /// Data categories that are always publicly accessible
    pub public_data: Vec<String>,
    /// Categories for which explicit consent has been granted
    pub granted_categories: Vec<String>,
}

/// Sovereignty consent request — P2 (Affirmative Consent).
///
/// Grant explicit consent for a data category. Valid categories:
/// "memory", "inference", "replica", "all".
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SovereigntyConsentRequest {
    /// Data category to grant consent for (e.g. "memory", "inference", "all")
    pub category: String,
}

/// Sovereignty consent response — P2 (Affirmative Consent).
///
/// `consent: true` means the grant was accepted and data sharing is enabled
/// for the listed categories. `consent: false` means consent was revoked and
/// only public data remains accessible.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SovereigntyConsentResponse {
    /// Whether consent is currently granted
    pub consent: bool,
    /// Human-readable status message
    pub message: String,
    /// Categories currently granted consent
    pub categories: Vec<String>,
}

/// Access check response — P4 (OCAP) membrane result.
///
/// Reports whether the authenticated agent has access to a specific data
/// category. `classification` is one of: PUBLIC, SHARED, SOVEREIGN.
/// `access_required` is the OCAP-level gate (e.g., "capability:memory",
/// "consent:required").
#[derive(Serialize, Deserialize, ToSchema)]
pub struct AccessCheckResponse {
    /// Data category checked
    pub category: String,
    /// Classification: PUBLIC, SHARED, or SOVEREIGN
    pub classification: String,
    /// OCAP-level access gate description
    pub access_required: String,
}

/// Get sovereignty status for the authenticated agent — consent state,
/// data category classifications, and granted sharing categories.
#[utoipa::path(
    get, path = "/api/sovereignty/status", tag = "sovereignty",
    responses(
        (status = 200, description = "Sovereignty status", body = SovereigntyStatusResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn sovereignty_status(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<SovereigntyStatusResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "sovereignty_status", "REG");
    let cm = &state.agent_service.governance().consent;
    let webid_str = auth.webid.to_string();
    let boundary = DataSovereigntyBoundary::load_or_default();
    let granted = cm
        .get_granted_categories(&webid_str)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Consent,
            source: None,
            message: e.to_string(),
        })?;

    Ok(Json(SovereigntyStatusResponse {
        explicit_consent: !granted.is_empty(),
        requires_affirmative_consent: consent_name(boundary.requires_affirmative_consent())
            .to_string(),
        sovereign_data: boundary
            .sovereign_data
            .iter()
            .map(|c| c.as_str().to_string())
            .collect(),
        shared_data: boundary
            .shared_data
            .iter()
            .map(|c| c.as_str().to_string())
            .collect(),
        public_data: boundary
            .public_data
            .iter()
            .map(|c| c.as_str().to_string())
            .collect(),
        granted_categories: granted,
    }))
}

/// Grant explicit consent for a data category, enabling data sharing.
#[utoipa::path(
    post, path = "/api/sovereignty/consent/grant", tag = "sovereignty",
    responses(
        (status = 200, description = "Consent granted", body = SovereigntyConsentResponse),
        (status = 400, description = "Invalid category"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn sovereignty_grant_consent(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<SovereigntyConsentRequest>,
) -> Result<Json<SovereigntyConsentResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "sovereignty_grant", category = %req.category, "REG");
    let webid_str = auth.webid.to_string();
    let cat_str = req.category;
    let cat = hkask_types::DataCategory::parse(&cat_str);
    let cm = &state.agent_service.governance().consent;
    cm.grant_consent(&webid_str, &cat)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Consent,
            source: None,
            message: e.to_string(),
        })?;
    let granted = cm
        .get_granted_categories(&webid_str)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Consent,
            source: None,
            message: e.to_string(),
        })?;
    Ok(Json(SovereigntyConsentResponse {
        consent: true,
        message: format!("Explicit consent granted for '{cat_str}'. Data sharing enabled."),
        categories: granted,
    }))
}

/// Revoke all explicit consent — only public data remains accessible.
#[utoipa::path(
    post, path = "/api/sovereignty/consent/revoke", tag = "sovereignty",
    responses(
        (status = 200, description = "Consent revoked", body = SovereigntyConsentResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Consent not found"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn sovereignty_revoke_consent(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<SovereigntyConsentResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "sovereignty_revoke", "REG");
    let webid_str = auth.webid.to_string();
    let cm = &state.agent_service.governance().consent;
    cm.revoke_consent(&webid_str)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Consent,
            source: None,
            message: e.to_string(),
        })?;
    Ok(Json(SovereigntyConsentResponse {
        consent: false,
        message: "Explicit consent revoked. Only public data is accessible.".into(),
        categories: vec![],
    }))
}

/// Check whether the authenticated agent has access to a specific data category.
#[utoipa::path(
    get, path = "/api/sovereignty/access/check", tag = "sovereignty",
    params(("category" = String, Query, description = "Data category to check")),
    responses(
        (status = 200, description = "Access check result", body = AccessCheckResponse),
        (status = 401, description = "Unauthorized"),
        (status = 400, description = "Missing category parameter"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn sovereignty_check_access(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<AccessCheckResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "sovereignty_check_access", "REG");
    let cat_str = params.get("category").map(|s| s.as_str()).unwrap_or("");
    if cat_str.is_empty() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Missing required query parameter: category".into(),
        }
        .into());
    }
    let cat = hkask_types::DataCategory::parse(cat_str);
    let cat_name = cat.as_str();
    let webid_str = auth.webid.to_string();
    let boundary = DataSovereigntyBoundary::load_or_default();
    let cm = &state.agent_service.governance().consent;

    let class = boundary.classify(&cat);
    let classification = class.label();
    let access_required = class.access_required();
    let has_consent = match class {
        BoundaryClassification::Public => true,
        BoundaryClassification::Sovereign | BoundaryClassification::Shared => {
            cm.has_consent(&webid_str, &cat).unwrap_or(false)
        }
        BoundaryClassification::Unknown => false, // Magna Carta P2: default deny
    };

    if !has_consent {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Agent,
            source: None,
            message: hkask_pods::a2a::A2AError::CapabilityDenied(
                auth.webid,
                format!("No consent for category '{cat_name}' (class {classification})"),
            )
            .to_string(),
        }
        .into());
    }
    Ok(Json(AccessCheckResponse {
        category: cat_name.to_string(),
        classification: classification.to_string(),
        access_required: access_required.to_string(),
    }))
}
