//! OpenAPI specification

use utoipa::OpenApi;

use crate::{
    ApiChatRequest, ApiChatResponse, CreatePodRequest, CreatePodResponse, LedgerHealthResponse,
    ListPodsResponse, ModelEntry, ModelListResponse, ModelSearchQuery, PodStatusResponse,
    RegulationVarietyResponse, TemplateResponse,
};

use crate::routes::{
    A2AAgentResponse, A2ARegisterRequest, A2ARegisterResponse, AgentListResponse, ApiBundleSummary,
    ApplyBundleResponse, ArchiveRequest, ArchiveResponse, BundleListResponse, CallbackQuery,
    ComposeBundleRequest, ComposeBundleResponse, CreateGoalRequest, DeactivateBundleResponse,
    DismissEscalationRequest, DismissEscalationResponse, EscalationEntryResponse,
    EscalationStatsResponse, EvolveBundleResponse, ExportRequest, ExportResponse, GoalListResponse,
    GoalResponse, InviteResponse, ListEscalationsResponse, LoginQuery, MetacognitionStatusResponse,
    RenameRequest, ResolveEscalationRequest, ResolveEscalationResponse, ResolveShaResponse,
    SetGoalStateRequest, SettingsResponse, UpdateSettingsRequest, UploadRequest, UserPodInfo,
    UserPodListResponse,
};

// Handler-local types needed in schemas
// TODO: Regulation subscribe params — regulation.rs was not committed; needs sovereignty.rs migration

/// API documentation
///
/// # Architectural Context
///
/// hKask is grounded in the **Principle of Least Action** (P0). Every endpoint
/// and schema exists because it reduces total system action — there is no
/// speculative generality.
///
/// ## Core Patterns (see docs/architecture/hKask-architecture-master.md)
///
/// - **Pattern A — Skills Model:** Templates in WordAct / FlowDef / KnowAct taxonomy.
///   Endpoints: `/api/templates`, `/api/v1/bundles`, `/api/v1/git/archive`.
/// - **Pattern B — Regulation Feedback Loop:** Cybernetic self-regulation via variety
///   counters, algedonic alerts, and homeostatic backpressure (P9).
///   Endpoints: `/api/regulation/health`, `/api/regulation/variety`, `/api/regulation/subscribe`.
/// - **Pattern C — Agentic AI Mediation:** Curator agent + escalation queue +
///   metacognition (Pattern C). Endpoints: `/api/chat`, `/api/v1/curator/*`.
/// - **Pattern D — Agent Creation:** Pod lifecycle with sovereign memory, per-agent
///   databases, and consent-governed data boundaries (P1, P2, P4).
///   Endpoints: `/api/bots`, `/api/episodic`, `/api/sovereignty`.
///
/// ## Magna Carta Principles (see docs/architecture/core/PRINCIPLES.md)
///
/// - **P1 — User Sovereignty:** Users own their data and delegation boundaries.
/// - **P2 — Affirmative Consent:** Default is deny; access requires explicit,
///   scoped, revocable consent.
/// - **P3 — Generative Space:** All settings are equally exposed across
///   CLI/API/REPL — no hidden or engineer-only controls.
/// - **P4 — Clear Boundaries (OCAP):** Every request carries a DelegationToken;
///   no ambient authority, no admin bypass.
/// - **P5 — Essentialism:** Every endpoint must earn its existence. Prefer
///   deletion over deprecation.
/// - **P8 — Semantic Grounding:** Responses carry provenance-aware representations.
/// - **P12 — Authenticated Host Mandate:** Every action has an accountable host identity
///   (WebID). No anonymous agency.
///
/// ## Authentication
///
/// All endpoints use **Bearer token** authentication. The token is a
/// DelegationToken — an unforgeable, attenuating OCAP capability token
/// carrying the authenticated WebID and scoped permissions.
///
/// ## Vocabulary
///
/// All domain terms used in request/response schemas are grounded in the
/// canonical WordAct / FlowDef / KnowAct taxonomy.
#[derive(OpenApi)]
#[openapi(
    components(schemas(
        TemplateResponse,
        LedgerHealthResponse,
        RegulationVarietyResponse,
        CreatePodRequest,
        CreatePodResponse,
        PodStatusResponse,
        ListPodsResponse,
        ApiChatRequest,
        ApiChatResponse,
        ModelEntry,
        ModelListResponse,
        ModelSearchQuery,
        // Curator schemas
        ListEscalationsResponse,
        EscalationEntryResponse,
        ResolveEscalationRequest,
        ResolveEscalationResponse,
        DismissEscalationRequest,
        DismissEscalationResponse,
        EscalationStatsResponse,
        MetacognitionStatusResponse,
        // Git schemas
        ArchiveRequest,
        ArchiveResponse,
        ResolveShaResponse,
        // Goal schemas
        CreateGoalRequest,
        SetGoalStateRequest,
        GoalResponse,
        GoalListResponse,
        // Regulation subscribe params
        // RegSubscribeParams  // TODO: Regulation route migration,
        // Auth schemas
        LoginQuery,
        CallbackQuery,
        // Admin schemas
        InviteResponse,
        // Export schemas
        ExportRequest,
        ExportResponse,
        UploadRequest,
        // UserPod schemas
        UserPodInfo,
        UserPodListResponse,
        RenameRequest,
        // Settings schemas
        SettingsResponse,
        UpdateSettingsRequest,
        // A2A schemas
        A2AAgentResponse,
        A2ARegisterRequest,
        A2ARegisterResponse,
        AgentListResponse,
        // Bundle schemas
        ApiBundleSummary,
        ApplyBundleResponse,
        BundleListResponse,
        ComposeBundleRequest,
        ComposeBundleResponse,
        DeactivateBundleResponse,
        EvolveBundleResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "templates", description = "Template registry — WordAct / FlowDef / KnowAct skills (Pattern A)"),
        (name = "mcp", description = "MCP servers and tools — tool discovery and invocation across out-of-process MCP servers"),
        (name = "regulation", description = "Regulation Ledger — variety tracking, algedonic alerts, and homeostatic self-regulation (P9, Pattern B)"),
        (name = "chat", description = "Curator chat interface — inference with model switching and streaming (Pattern C)"),
                (name = "chat-ws", description = "Chat WebSocket — persistent bidirectional streaming agent chat with MCP tool support (P3)"),
        (name = "models", description = "Multi-provider model catalog (DeepInfra, fal.ai, Together AI, OpenRouter, KiloCode) — discover and search available LLMs"),
        (name = "curator", description = "Curator escalation and metacognition — health reports and pending escalation queue (Pattern C, P12)"),
        (name = "git", description = "Git archival and resolution — template crate loading and SHA resolution via GitCASPort hexagonal boundary"),
        (name = "a2a", description = "A2A agent registration — register, list, and unregister agents with capability delegation (P4 OCAP)"),
        (name = "goals", description = "Goal coordination substrate — creation, listing, and state transitions with OCAP authority gating (P4)"),
        (name = "bundles", description = "Bundle composition and evolution — inference-driven skill bundling with apply/deactivate lifecycle (Pattern A)"),
        (name = "episodic", description = "Episodic memory — store and query bitemporal h_mems with OCAP-gated access (P1, P4, P11)"),
        (name = "sovereignty", description = "Sovereignty governance — consent grant/revoke and access checks under Magna Carta P1–P4"),
        (name = "consolidation", description = "Context consolidation — episodic→semantic memory condensation with passphrase-gated authorization"),
        (name = "admin", description = "Admin — invite creation, listing, session management, and server config (FUNCTIONAL_SPECIFICATION.md §3.16)"),
        (name = "auth", description = "Authentication — OAuth sign-in with GitHub/Google, session management, and invite acceptance (P1, P12)"),
        (name = "export", description = "Export — sovereignty archive creation, upload, and download for data portability (P1)"),
        (name = "landing", description = "Landing page — static HTML welcome page with OAuth sign-in (P3)"),
        (name = "pods", description = "Pod lifecycle — create, list, activate, deactivate, and status (Pattern D)"),
        (name = "userpods", description = "UserPod management — list, rename, and delete userpods (P1)"),
        (name = "settings", description = "Settings — read/write REPL inference settings with P3 equal surface exposure"),
        (name = "terminal", description = "Terminal — browser-based xterm.js WebSocket terminal (P3, P12)"),
        (name = "wallet", description = "Wallet — API key management, withdrawal fee estimation (P9)"),
    ),
    info(
        title = "hKask API",
        version = "0.31.0",
        description = "A Minimal Viable Container for UserPods — HTTP API.\n\nhKask is an agent runtime grounded in 12 architectural principles\n(P0–P12) expressed through four composable patterns: Skills Model,\nRegulation Feedback Loop, Agentic AI Mediation, and Agent Creation with\nSovereign Memory. This API exposes all capabilities equally across\nCLI, API, and MCP surfaces under P3 (Equal Surface Exposure).\n\nAll endpoints carry OCAP DelegationToken authentication (P4).\nData access is governed by user sovereignty and affirmative\nconsent (P1, P2)."
    ),
    servers(
        (url = "/api", description = "hKask API server"),
    ),
)]
pub struct ApiDoc;

/// Security addon — injects Bearer token security scheme into the OpenAPI spec.
///
/// hKask uses DelegationTokens (OCAP capability tokens carrying the authenticated
/// WebID and scoped permissions) transmitted as Bearer tokens in the Authorization
/// header. Every endpoint is gated by the auth middleware.
struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_token",
                utoipa::openapi::security::SecurityScheme::Http(
                    utoipa::openapi::security::HttpBuilder::new()
                        .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                        .description(Some(
                            "DelegationToken — an OCAP capability token carrying the authenticated WebID and scoped permissions (P4).\n\nObtain via the REPL onboarding flow (`kask secret`) or agent registration (A2A)."
                        ))
                        .bearer_format("DelegationToken")
                        .build(),
                ),
            );
        }
        // Apply bearer_token security to all operations
        openapi.security = Some(vec![utoipa::openapi::security::SecurityRequirement::new(
            "bearer_token",
            Vec::<String>::new(),
        )]);
    }
}
