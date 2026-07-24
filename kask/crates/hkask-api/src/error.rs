//! API error type with Axum IntoResponse — maps to HTTP status codes per variant.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    NotFound { resource: String, id: String },
    Unauthorized { reason: String },
    Forbidden { reason: String },
    BadRequest { message: String },
    Conflict { message: String },
    ServiceUnavailable { reason: String },
    Internal { message: String },
}

impl ApiError {
    fn status_and_message(self) -> (StatusCode, String) {
        match self {
            ApiError::NotFound { resource, id } => {
                (StatusCode::NOT_FOUND, format!("{resource} not found: {id}"))
            }
            ApiError::Unauthorized { reason } => {
                (StatusCode::UNAUTHORIZED, format!("Unauthorized: {reason}"))
            }
            ApiError::Forbidden { reason } => {
                (StatusCode::FORBIDDEN, format!("Forbidden: {reason}"))
            }
            ApiError::BadRequest { message } => {
                (StatusCode::BAD_REQUEST, format!("Bad request: {message}"))
            }
            ApiError::Conflict { message } => {
                (StatusCode::CONFLICT, format!("Conflict: {message}"))
            }
            ApiError::ServiceUnavailable { reason } => (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Service unavailable: {reason}"),
            ),
            ApiError::Internal { message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal error: {message}"),
            ),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NotFound { resource, id } => write!(f, "{resource} not found: {id}"),
            ApiError::Unauthorized { reason } => write!(f, "Unauthorized: {reason}"),
            ApiError::Forbidden { reason } => write!(f, "Forbidden: {reason}"),
            ApiError::BadRequest { message } => write!(f, "Bad request: {message}"),
            ApiError::Conflict { message } => write!(f, "Conflict: {message}"),
            ApiError::ServiceUnavailable { reason } => write!(f, "Service unavailable: {reason}"),
            ApiError::Internal { message } => write!(f, "Internal error: {message}"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

// ── ServiceError newtype for Axum IntoResponse ───────────────────────
//
// Route handlers return `Result<Json<T>, ServiceErrorResponse>`.
// `ServiceError` cannot implement `IntoResponse` directly (orphan rule:
// both the trait and type are foreign). This newtype bridges the gap,
// delegating to `ApiError` for HTTP status code mapping.

/// Newtype wrapper that implements `IntoResponse` for `ServiceError`.
///
/// Route handlers return ``Result<Json<T>, ServiceErrorResponse>``.
/// The `?` operator auto-converts `ServiceError` via the `From` impl.
#[derive(Debug)]
pub struct ServiceErrorResponse(pub hkask_services_core::ServiceError);

impl From<hkask_services_core::ServiceError> for ServiceErrorResponse {
    fn from(e: hkask_services_core::ServiceError) -> Self {
        ServiceErrorResponse(e)
    }
}

impl IntoResponse for ServiceErrorResponse {
    fn into_response(self) -> Response {
        ApiError::from(self.0).into_response()
    }
}

// ── Domain error → ServiceErrorResponse bridges ───────────────────────
//
// The `?` operator needs direct `From<E> for ServiceErrorResponse`.
// These impls bridge domain errors that route handlers propagate with `?`.
// They delegate to `ServiceError`'s `#[from]` wrappers.

impl From<hkask_pods::a2a::A2AError> for ServiceErrorResponse {
    fn from(e: hkask_pods::a2a::A2AError) -> Self {
        ServiceErrorResponse(hkask_services_core::ServiceError::Domain {
            kind: hkask_services_core::ErrorKind::Forbidden,
            domain: hkask_services_core::DomainKind::Agent,
            source: None,
            message: e.to_string(),
        })
    }
}

impl From<hkask_storage::EscalationError> for ServiceErrorResponse {
    fn from(e: hkask_storage::EscalationError) -> Self {
        ServiceErrorResponse(hkask_services_core::ServiceError::Domain {
            kind: hkask_services_core::ErrorKind::BadRequest,
            domain: hkask_services_core::DomainKind::Curator,
            source: None,
            message: e.to_string(),
        })
    }
}

impl From<uuid::Error> for ServiceErrorResponse {
    fn from(e: uuid::Error) -> Self {
        let msg = e.to_string();
        ServiceErrorResponse(hkask_services_core::ServiceError::InvalidWebID {
            source: Some(e),
            message: msg,
        })
    }
}

impl From<hkask_pods::pod::AgentPodError> for ServiceErrorResponse {
    fn from(e: hkask_pods::pod::AgentPodError) -> Self {
        use hkask_pods::pod::AgentPodError as PE;
        use hkask_services_core::DomainKind;
        use hkask_services_core::ErrorKind;
        let kind = match &e {
            PE::PodNotActive | PE::PodNotFound(_) => ErrorKind::NotFound,
            PE::CapabilityDenied { .. }
            | PE::SovereigntyDenied { .. }
            | PE::AttenuationLimitExceeded => ErrorKind::Forbidden,
            PE::InvalidStateTransition(..) | PE::ModeError(_) => ErrorKind::BadRequest,
            PE::InferenceUnavailable(_) => ErrorKind::ServiceUnavailable,
            _ => ErrorKind::BadRequest,
        };
        ServiceErrorResponse(hkask_services_core::ServiceError::Domain {
            kind,
            domain: DomainKind::Pod,
            source: None,
            message: e.to_string(),
        })
    }
}

impl From<hkask_types::RegistryError> for ServiceErrorResponse {
    fn from(e: hkask_types::RegistryError) -> Self {
        ServiceErrorResponse(hkask_services_core::ServiceError::Domain {
            kind: hkask_services_core::ErrorKind::BadRequest,
            domain: hkask_services_core::DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })
    }
}

// ── Service layer adapter ────────────────────────────────────────────

impl From<hkask_services_core::ServiceError> for ApiError {
    fn from(e: hkask_services_core::ServiceError) -> Self {
        use hkask_services_core::ErrorKind;
        use hkask_services_core::ServiceError as SE;
        match e {
            SE::Domain {
                kind,
                domain,
                message,
                ..
            } => match kind {
                ErrorKind::NotFound => ApiError::NotFound {
                    resource: format!("{:?}", domain).to_lowercase(),
                    id: message,
                },
                ErrorKind::Forbidden => ApiError::Forbidden { reason: message },
                ErrorKind::Conflict => ApiError::Conflict { message },
                ErrorKind::BadRequest => ApiError::BadRequest { message },
                ErrorKind::ServiceUnavailable => ApiError::ServiceUnavailable { reason: message },
            },
            SE::ModelService {
                kind,
                retryable,
                message,
                ..
            } => {
                if retryable || kind == ErrorKind::ServiceUnavailable {
                    ApiError::ServiceUnavailable { reason: message }
                } else {
                    match kind {
                        ErrorKind::NotFound => ApiError::NotFound {
                            resource: "model".into(),
                            id: message,
                        },
                        ErrorKind::Forbidden => ApiError::Forbidden { reason: message },
                        ErrorKind::Conflict => ApiError::Conflict { message },
                        ErrorKind::BadRequest => ApiError::BadRequest { message },
                        ErrorKind::ServiceUnavailable => {
                            ApiError::ServiceUnavailable { reason: message }
                        }
                    }
                }
            }
            SE::McpTool { message, .. } => ApiError::Internal { message },
            SE::Infra(err) => ApiError::Internal {
                message: err.to_string(),
            },
            SE::InvalidWebID { message, .. } => ApiError::BadRequest { message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn apierror_maps_to_correct_status_codes() {
        let (status, _) = ApiError::NotFound {
            resource: "agent".into(),
            id: "test".into(),
        }
        .status_and_message();
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = ApiError::Unauthorized {
            reason: "bad token".into(),
        }
        .status_and_message();
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _) = ApiError::Forbidden {
            reason: "no access".into(),
        }
        .status_and_message();
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = ApiError::BadRequest {
            message: "invalid".into(),
        }
        .status_and_message();
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = ApiError::Internal {
            message: "boom".into(),
        }
        .status_and_message();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn apierror_display_is_readable() {
        let err = ApiError::NotFound {
            resource: "pod".into(),
            id: "p1".into(),
        };
        assert_eq!(err.to_string(), "pod not found: p1");

        let err = ApiError::Unauthorized {
            reason: "expired".into(),
        };
        assert_eq!(err.to_string(), "Unauthorized: expired");

        let err = ApiError::BadRequest {
            message: "missing field".into(),
        };
        assert_eq!(err.to_string(), "Bad request: missing field");
    }

    #[test]
    fn apierror_into_response_produces_correct_status() {
        let err = ApiError::NotFound {
            resource: "agent".into(),
            id: "alice".into(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let err = ApiError::Internal {
            message: "boom".into(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn serviceerror_kind_discrimination_produces_correct_http_status() {
        use axum::response::IntoResponse;
        use hkask_services_core::{DomainKind, ErrorKind, ServiceError as SE};

        // Domain not found → 404
        let err = SE::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Pod,
            source: None,
            message: "pod-123".into(),
        };
        let status = ApiError::from(err).into_response().status();
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Domain forbidden → 403
        let err = SE::Domain {
            kind: ErrorKind::Forbidden,
            domain: DomainKind::User,
            source: None,
            message: "denied".into(),
        };
        let status = ApiError::from(err).into_response().status();
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Domain conflict → 409
        let err = SE::Domain {
            kind: ErrorKind::Conflict,
            domain: DomainKind::Agent,
            source: None,
            message: "conflict".into(),
        };
        let status = ApiError::from(err).into_response().status();
        assert_eq!(status, StatusCode::CONFLICT);

        // Domain bad request → 400
        let err = SE::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "bad".into(),
        };
        let status = ApiError::from(err).into_response().status();
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Domain service unavailable → 503
        let err = SE::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Infrastructure,
            source: None,
            message: "inference down".into(),
        };
        let status = ApiError::from(err).into_response().status();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        // InvalidWebID → 400
        let err = SE::InvalidWebID {
            source: None,
            message: "bad-webid".into(),
        };
        let status = ApiError::from(err).into_response().status();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
