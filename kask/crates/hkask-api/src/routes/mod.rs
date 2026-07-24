//! HTTP routes — per-domain modules

pub(crate) mod a2a;
pub(crate) mod admin;
pub(crate) mod auth;

pub(crate) mod bundles;
pub(crate) mod chat;
pub(crate) mod chat_ws;
pub(crate) mod consolidation;
pub(crate) mod consolidation_auth;
pub(crate) mod curator;
pub(crate) mod episodic;
pub(crate) mod export;
pub(crate) mod git;
pub(crate) mod goal;
pub(crate) mod health;
pub(crate) mod landing;
pub(crate) mod mcp;
pub(crate) mod memory;
pub(crate) mod models;
pub(crate) mod onboarding;
pub(crate) mod pods;
pub(crate) mod regulation;
pub(crate) mod settings;
pub(crate) mod sovereignty;
pub(crate) mod templates;
pub(crate) mod terminal;
pub(crate) mod userpod;
pub(crate) mod wallet;

// Re-export router functions
pub use a2a::a2a_router;
pub use auth::auth_router;
pub use auth::{CallbackQuery, LoginQuery};

pub use bundles::bundles_router;
pub use chat::chat_router;
pub use consolidation::consolidation_router;
pub use curator::curator_router;
pub use episodic::episodic_router;
pub use export::export_router;
pub use export::{ExportRequest, ExportResponse, UploadRequest};
pub use git::git_router;
pub use goal::goal_router;
pub use health::health_check;
pub use landing::landing_page;
pub use mcp::mcp_router;
pub use memory::memory_router;
pub use models::models_router;
pub use onboarding::invite_required_page;
pub use onboarding::onboarding_page;
pub use pods::pods_router;
pub use settings::settings_router;
pub use sovereignty::sovereignty_router;
pub use templates::templates_router;
pub use terminal::terminal_router;
pub use userpod::userpod_router;
pub use wallet::wallet_router;

// Re-export domain-local types that may be used externally
pub use a2a::{A2AAgentResponse, A2ARegisterRequest, A2ARegisterResponse, AgentListResponse};
pub use admin::InviteResponse;
pub use bundles::{
    ApiBundleSummary, ApplyBundleResponse, BundleListResponse, ComposeBundleRequest,
    ComposeBundleResponse, DeactivateBundleResponse, EvolveBundleResponse,
};
pub use chat::{ApiChatRequest, ApiChatResponse};
pub use curator::{
    DismissEscalationRequest, DismissEscalationResponse, EscalationEntryResponse,
    EscalationStatsResponse, ListEscalationsResponse, MetacognitionStatusResponse,
    ResolveEscalationRequest, ResolveEscalationResponse,
};
pub use git::{ArchiveRequest, ArchiveResponse, ResolveShaResponse};
pub use goal::{CreateGoalRequest, GoalListResponse, GoalResponse, SetGoalStateRequest};
pub use models::{ModelEntry, ModelListResponse, ModelSearchQuery};
pub use pods::{CreatePodRequest, CreatePodResponse, ListPodsResponse, PodStatusResponse};
pub use regulation::regulation_router;
pub use regulation::{LedgerHealthResponse, RegulationVarietyResponse, VarietyCounterResponse};
pub use settings::{SettingsResponse, UpdateSettingsRequest};
pub use sovereignty::{AccessCheckResponse, SovereigntyConsentResponse, SovereigntyStatusResponse};
pub use templates::TemplateResponse;
pub use userpod::{RenameRequest, UserPodInfo, UserPodListResponse};
pub use wallet::{
    ApiKeyCreatedResponse, ApiKeyEntry, ApiKeyListResponse, ApiKeyRevokedResponse,
    CreateKeyRequest, WithdrawalFeeEstimateResponse,
};
