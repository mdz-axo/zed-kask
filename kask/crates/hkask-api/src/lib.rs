#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask API — HTTP API with OpenAPI
//!
//! **Endpoints:**
//! - `GET /api/templates` — List templates
//! - `GET /api/templates/:id` — Get template
//! - `GET /api/templates/search/:term` — Search templates by lexicon
//! - `GET /api/mcp/servers` — List MCP servers
//! - `GET /api/mcp/tools` — List tools
//! - `GET /api/mcp/tools/:name` — Get tool definition
//! - `POST /api/mcp/invoke` — Invoke an MCP tool
//! - `GET /api/regulation/health` — Regulation health status
//! - `GET /api/regulation/alerts` — Algedonic alerts
//! - `GET /api/regulation/variety` — Regulation variety counters
//! - `GET /api/pods` — List pods
//! - `POST /api/pods` — Create pod
//! - `POST /api/pods/:id/activate` — Activate pod
//! - `POST /api/pods/:id/deactivate` — Deactivate pod
//! - `GET /api/pods/:id/status` — Get pod status
//! - `POST /api/chat` — Curator chat
//! - `GET /api/sovereignty/status` — User sovereignty status
//! - `POST /api/sovereignty/consent/grant` — Grant explicit consent
//! - `POST /api/sovereignty/consent/revoke` — Revoke explicit consent
//! - `GET /api/sovereignty/access/check` — Check data access status
//! - `POST /api/episodic/store` — Store episodic h_mem
//! - `GET /api/episodic/query` — Query episodic memories
//! - `GET /api/episodic/usage` — Episodic storage usage

mod git_cas;

pub mod email;
pub mod error;
pub mod middleware;
pub mod openapi;
pub mod openapi_spec;
pub mod routes;

pub use error::ApiError;

// Re-export route types for OpenAPI schema generation
pub use routes::{A2ARegisterRequest, A2ARegisterResponse};
pub use routes::{
    ApiChatRequest, ApiChatResponse, CreatePodRequest, CreatePodResponse, LedgerHealthResponse,
    ListPodsResponse, ModelEntry, ModelListResponse, ModelSearchQuery, PodStatusResponse,
    RegulationVarietyResponse, TemplateResponse, WithdrawalFeeEstimateResponse,
};

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use std::sync::Arc;

use hkask_services_context::AgentService;
use hkask_services_wallet::WalletService;
use utoipa::OpenApi;

use git_cas::{GitCasBundle, init_git_cas};

use openapi::ApiDoc;

/// API state — composes `AgentService` for all shared infrastructure.
///
/// The `agent_service` field is the single source of truth for domain
/// objects. Surface-specific fields (git CAS, wallet service)
/// are the ONLY fields that don't come from `AgentService`.
#[derive(Clone)]
pub struct ApiState {
    /// Agent service — single source of truth for all shared infrastructure.
    /// All domain objects (registry, escalation queue, consent manager, etc.)
    /// come from here. Surface code derives service types via domain accessors.
    pub agent_service: Arc<AgentService>,

    /// Template-loading adapter — surface-specific
    pub template_adapter: Arc<hkask_templates::TemplateCrateLoader>,
    /// Git CAS port for all CAS operations (hexagonal boundary) — surface-specific
    pub git_cas_port: Arc<dyn hkask_types::git_cas::GitCASPort>,
    /// GixCasAdapter for admin operations (resolve_ref, diff) — surface-specific
    pub gix_cas: Arc<hkask_git_cas::GixCasAdapter>,
    /// Wallet service for rJoule payments and API key management — surface-specific.
    /// Built from config during `ApiState::with_defaults()` or `from_service_context()`.
    pub wallet_service: Option<Arc<WalletService>>,
    /// API key authentication service for Bearer token verification.
    /// Built from the wallet store when a wallet service is available.
    pub api_key_auth_service: Option<Arc<middleware::api_key_auth::ApiKeyAuthService>>,
}

impl ApiState {
    /// Create ApiState with default adapters via `AgentService::build()`.
    ///
    /// Resolves configuration from environment variables and keychain,
    /// builds an `AgentService` with all shared infrastructure, then
    /// constructs `ApiState` from it with surface-specific defaults.
    ///
    /// The API server is headless and cannot run interactive onboarding — the caller
    /// is responsible for ensuring secrets are available via the keystore.
    /// Run `kask chat` interactively first to complete onboarding and store secrets.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  environment variables and keystore are configured
    /// post: if config/secrets available → Ok(ApiState) with full infrastructure
    /// post: if config/secrets missing → Err(ApiError::Internal)
    pub async fn with_defaults() -> Result<Self, ApiError> {
        let config =
            hkask_services_core::ServiceConfig::from_env().map_err(|e| ApiError::Internal {
                message: format!("Failed to resolve service config: {e}"),
            })?;
        let nonce = std::sync::Arc::new(crate::email::NonceStore::new(
            std::time::Duration::from_secs(86400),
        ));
        let email_sink =
            crate::email::CuratorAlertEmailSink::try_from_env_with_nonce(nonce.clone());
        let ctx = hkask_services_context::AgentService::build_with_email(config, email_sink)
            .await
            .map_err(|e| ApiError::Internal {
                message: format!("Failed to build service context: {e}"),
            })?;
        crate::email::wire_inbox_poller(
            &ctx,
            crate::email::inbox_poll_interval_secs(),
            Some(nonce),
        );
        crate::email::wire_digest_task(&ctx);
        Self::from_service_context(ctx).await
    }

    /// Create ApiState from a pre-built `AgentService`.
    ///
    /// This is the canonical construction path for API surfaces that compose
    /// `AgentService::build()`. All shared infrastructure (Regulation, loop system,
    /// governed tool, pod manager, stores) comes from `AgentService`.
    /// Surface-specific fields (git CAS) are constructed from AgentService
    /// fields or initialized to defaults.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  ctx is a fully-built AgentService
    /// post: returns Ok(ApiState) with all shared infra from ctx
    /// post: git_cas initialized from ctx or defaults
    /// post: api_key_auth_service initialized if wallet_store + wallet_service available
    pub async fn from_service_context(ctx: AgentService) -> Result<Self, ApiError> {
        // Surface-specific: Git CAS adapters
        let GitCasBundle {
            template_adapter,
            git_cas_port,
            gix_cas,
        } = init_git_cas()?;

        // Extract wallet service before moving ctx into Arc
        let wallet_service = ctx.infra().wallet.clone();
        // Regulation event sink for API metering spans (reg.api.request).
        let event_sink = ctx.ledger().events.clone();
        // Build API key auth service if wallet store and wallet service are available
        let api_key_auth_service = match (ctx.storage().wallet.clone(), wallet_service.clone()) {
            (Some(store), Some(svc)) => {
                let rate_limit_config = hkask_regulation::api_metering::RateLimitConfig::from_env();
                let api_meter = Arc::new(std::sync::RwLock::new(
                    hkask_regulation::ApiMeter::with_config(rate_limit_config),
                ));
                hkask_regulation::ApiMeter::spawn_learning_loop(Arc::clone(&api_meter));
                Some(Arc::new(
                    middleware::api_key_auth::ApiKeyAuthService::new(store, svc)
                        .with_api_meter(api_meter)
                        .with_event_sink(event_sink),
                ))
            }
            _ => None,
        };

        Ok(Self {
            agent_service: Arc::new(ctx),
            // Surface-specific fields only
            template_adapter,
            git_cas_port,
            gix_cas,
            wallet_service,
            api_key_auth_service,
        })
    }

    /// Attach a wallet service for rJoule payments and API key management.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  svc is a valid `Arc<WalletService>`
    /// post: self.wallet_service = Some(svc); returns self
    pub fn with_wallet_service(mut self, svc: Arc<WalletService>) -> Self {
        self.wallet_service = Some(svc);
        self
    }

    /// Start the loop system (all registered loops begin their tick cycles).
    ///
    /// Call this after the API server starts listening. The loops run in
    /// background tokio tasks until `shutdown_loops()` is called.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  self.agent_service.loop_system() is initialized
    /// post: all registered loops begin tick cycles
    /// post: returns Ok(()) on success, Err(InfrastructureError) on failure
    pub async fn start_loops(&self) {
        let loops = &self.agent_service.ledger().loops;
        tracing::info!(
            target: "hkask.api",
            loops = ?loops.registered_loop_ids().await,
            "Starting loop system"
        );
        loops.start().await;
    }

    /// Signal the loop system to shut down.
    pub fn shutdown_loops(&self) {
        tracing::info!(target: "hkask.api", "Shutting down loop system");
        let loops = &self.agent_service.ledger().loops;
        loops.shutdown();
    }
}

/// Create API router with OpenAPI documentation and authentication
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  state is a valid ApiState
/// post: returns Ok(OpenApiRouter) with all route modules merged
/// post: auth middleware layer applied
/// post: api_key_auth middleware layer applied if available
pub fn create_router(state: ApiState) -> utoipa_axum::router::OpenApiRouter {
    let auth_service = std::sync::Arc::new(middleware::AuthService::from_config(
        state.agent_service.config(),
    ));

    let mut router = utoipa_axum::router::OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(routes::auth_router())
        .route("/", axum::routing::get(routes::landing_page))
        .route("/onboarding", axum::routing::get(routes::onboarding_page))
        .route(
            "/invite-required",
            axum::routing::get(routes::invite_required_page),
        )
        .route("/health", axum::routing::get(routes::health_check))
        .merge(routes::templates_router())
        .merge(routes::terminal_router())
        .merge(routes::pods_router())
        .merge(routes::mcp_router())
        .merge(routes::userpod_router())
        .merge(routes::regulation_router())
        .merge(routes::sovereignty_router())
        .merge(routes::chat_router())
        .merge(routes::models_router())
        .merge(routes::a2a_router())
        .merge(routes::bundles_router())
        .merge(routes::curator_router())
        .merge(routes::episodic_router())
        .merge(routes::export_router())
        .merge(routes::consolidation_router())
        .merge(routes::git_router())
        .merge(routes::goal_router())
        .merge(routes::settings_router())
        .merge(routes::wallet_router())
        .merge(routes::admin::admin_router())
        // Middleware (outermost = last .layer() = runs first):
        // 1. Regulation span — captures all requests
        // 2. Session cookie — injects AuthContext if valid session (DEP-020)
        // 3. Capability token — requires Bearer token if no AuthContext
        .layer(axum::middleware::from_fn_with_state(
            auth_service,
            middleware::auth_middleware,
        ))
        .layer({
            let store = state.agent_service.storage().users.clone();
            axum::middleware::from_fn(move |req: Request<Body>, next: Next| {
                let store = store.clone();
                async move {
                    use middleware::session_middleware_impl;
                    session_middleware_impl(&store, req, next).await
                }
            })
        })
        // Regulation middleware — disabled until regulation_router is wired
        // .layer(axum::middleware::from_fn(middleware::regulation_middleware))
        ;

    // Admin role-gating middleware (runs after session + auth, before routes)
    let admin_store = state.agent_service.storage().users.clone();
    router = router.layer(axum::middleware::from_fn(
        move |req: Request<Body>, next: Next| {
            let store = admin_store.clone();
            async move { middleware::admin_middleware(store, req, next).await }
        },
    ));

    // Apply API key auth middleware if available (allows Bearer token auth on wallet routes)
    if let Some(api_key_auth) = &state.api_key_auth_service {
        router = router.layer(axum::middleware::from_fn_with_state(
            Arc::clone(api_key_auth),
            middleware::api_key_auth::api_key_auth_middleware,
        ));
    }

    router.with_state(state)
}

/// Build OpenAPI spec with all route paths collected from the router.
///
/// Builds the full `OpenApiRouter` (without state or auth middleware) to
/// collect `#[utoipa::path]` metadata from `routes!()` calls, then extracts
/// the complete OpenAPI specification including paths.
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi with all route paths documented
pub fn create_openapi() -> utoipa::openapi::OpenApi {
    use utoipa_axum::router::OpenApiRouter;

    let router: OpenApiRouter<ApiState> = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(routes::auth_router())
        .merge(routes::templates_router())
        .merge(routes::terminal_router())
        .merge(routes::pods_router())
        .route("/", axum::routing::get(routes::landing_page))
        .route("/onboarding", axum::routing::get(routes::onboarding_page))
        .route(
            "/invite-required",
            axum::routing::get(routes::invite_required_page),
        )
        .route("/health", axum::routing::get(routes::health_check))
        .merge(routes::mcp_router())
        .merge(routes::regulation_router())
        .merge(routes::sovereignty_router())
        .merge(routes::chat_router())
        .merge(routes::models_router())
        .merge(routes::userpod_router())
        .merge(routes::a2a_router())
        .merge(routes::bundles_router())
        .merge(routes::curator_router())
        .merge(routes::episodic_router())
        .merge(routes::export_router())
        .merge(routes::consolidation_router())
        .merge(routes::git_router())
        .merge(routes::goal_router())
        .merge(routes::settings_router())
        .merge(routes::wallet_router())
        .merge(routes::admin::admin_router());
    router.into_openapi()
}
