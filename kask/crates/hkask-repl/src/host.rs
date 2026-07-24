//! Host trait — implemented by the CLI binary to provide REPL dependencies.

/// Canonical outcome returned by a host-owned onboarding flow.
///
/// The REPL consumes this contract after the outer host completes setup.
/// CLI onboarding returns this type directly, keeping the dependency direction
/// from CLI to REPL and avoiding a mirrored outcome type.
#[derive(Debug, Clone)]
pub struct OnboardingOutcome {
    pub signed_in_agent: String,
    pub resolved_secrets: Option<hkask_services_onboarding::ResolvedSecrets>,
    pub selected_model: Option<String>,
    pub is_first_run: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("Onboarding cancelled by user")]
    Cancelled,
    #[error("Onboarding failed: {0}")]
    Failed(String),
}

pub trait ReplHost: Send + Sync {
    fn resolve_user_webid(&self) -> hkask_types::WebID;
    fn run_onboarding(
        &self,
        rt: &tokio::runtime::Handle,
    ) -> Result<OnboardingOutcome, OnboardingError>;
    fn list_templates_local(&self) -> Vec<hkask_types::RegistryEntry>;
    #[cfg(feature = "tui")]
    fn open_transcript_viewer(&self, path: &std::path::Path) -> anyhow::Result<()>;
    fn run_sovereignty_status(&self);
}
