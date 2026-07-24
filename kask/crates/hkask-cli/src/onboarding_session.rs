//! Onboarding state machine — resumable userpod creation.
//!
//! Simplified from the userpod-creation state machine: no agent registry,
//! no Matrix account creation, no UserProfile. The flow is linear:
//! derive secrets → create the userpod (SQLCipher DB) → register in A2A.
//! Each step is independently callable and carries its own recovery logic.

use hkask_identity::RegistrationRequest;
use hkask_pods::A2ARuntime;
use hkask_services_core::{DomainKind, ErrorKind, ServiceConfig, ServiceError};
use hkask_services_onboarding::{OnboardingService, ResolvedSecrets};
use hkask_storage::user_store::UserStore;
use std::sync::Arc;

use crate::onboarding::OnboardingError;

/// Accumulated state across onboarding steps.
/// Each field is populated as its corresponding step completes.
pub struct OnboardingSession {
    // ── Collected before the state machine starts ──
    pub userpod_name: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,

    // ── Accumulated during state machine execution ──
    selected_model: Option<String>,
    passphrase: Option<String>,
    resolved_secrets: Option<ResolvedSecrets>,
}

impl OnboardingSession {
    /// Create a new session with identity already collected.
    pub fn new(userpod_name: String, email: String, first_name: String, last_name: String) -> Self {
        Self {
            userpod_name,
            email,
            first_name,
            last_name,
            selected_model: None,
            passphrase: None,
            resolved_secrets: None,
        }
    }

    /// Run all remaining steps to completion. Returns the completed session.
    /// Interactive callbacks (`get_model`, `get_passphrase`) are injected so
    /// this state machine has no stdio dependencies.
    pub async fn run(
        mut self,
        get_model: impl FnOnce() -> Result<String, OnboardingError>,
        get_passphrase: impl FnOnce() -> Result<String, OnboardingError>,
    ) -> Result<CompletedSession, (Self, OnboardingError)> {
        if let Err(e) = self.advance_model(get_model()) {
            return Err((self, e));
        }
        if let Err(e) = self.advance_passphrase(get_passphrase()) {
            return Err((self, e));
        }
        if let Err(e) = self.advance_secrets().await {
            return Err((self, e));
        }
        if let Err(e) = self.advance_userpod().await {
            return Err((self, e));
        }
        if let Err(e) = self.advance_a2a().await {
            return Err((self, e));
        }
        Ok(CompletedSession {
            userpod_name: self.userpod_name,
            selected_model: self.selected_model.unwrap_or_default(),
            resolved_secrets: self.resolved_secrets,
        })
    }

    // ── Step implementations ─────────────────────────────────────────────

    fn advance_model(
        &mut self,
        model_result: Result<String, OnboardingError>,
    ) -> Result<(), OnboardingError> {
        let model = model_result?;
        self.selected_model = Some(model);
        Ok(())
    }

    fn advance_passphrase(
        &mut self,
        passphrase_result: Result<String, OnboardingError>,
    ) -> Result<(), OnboardingError> {
        let passphrase = passphrase_result?;
        self.passphrase = Some(passphrase);
        Ok(())
    }

    async fn advance_secrets(&mut self) -> Result<(), OnboardingError> {
        let passphrase = self.passphrase.as_ref().ok_or_else(|| {
            OnboardingError::Service(ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Infrastructure,
                source: None,
                message: "Passphrase not set before secret derivation".into(),
            })
        })?;

        // Remove orphaned DB from a previous failed attempt.
        if let Ok(pre_config) = ServiceConfig::from_env()
            && OnboardingService::has_orphaned_db(&pre_config)
        {
            eprintln!("  A database from a previous failed setup was found.");
            eprint!("  Remove it? [y/N] ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let confirm = crate::onboarding::read_line().unwrap_or_default();
            if confirm.trim().to_lowercase().starts_with('y') {
                if OnboardingService::remove_orphaned_db_unchecked(&pre_config) {
                    eprintln!("  Removed orphaned database.");
                } else {
                    eprintln!("  ⚠ Database was not removed (cleanup failed).");
                }
            } else {
                eprintln!("  Keeping existing database. Setup will use it if compatible.");
            }
        }

        // Derive secrets (no keychain store here — the caller stores them
        // after the userpod is created so a failed userpod step doesn't leave
        // orphaned keychain entries).
        let resolved = OnboardingService::derive_secrets(passphrase).map_err(|e| {
            eprintln!("  \x1b[31m✗\x1b[0m Failed to derive security keys: {}", e);
            OnboardingError::Service(e)
        })?;
        self.resolved_secrets = Some(resolved);
        Ok(())
    }

    async fn advance_userpod(&mut self) -> Result<(), OnboardingError> {
        let resolved = self.resolved_secrets.as_ref().ok_or_else(|| {
            OnboardingError::Service(ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Infrastructure,
                source: None,
                message: "Secrets not derived before userpod creation".into(),
            })
        })?;
        let passphrase = self.passphrase.as_deref().unwrap_or("");

        // Build a config from the resolved secrets so we can open the DB.
        let config = ServiceConfig::from_secrets(
            resolved.a2a_secret.clone(),
            resolved.db_passphrase.clone(),
            self.userpod_name.clone(),
        );

        // Create the userpod in the SQLCipher DB. Idempotent: if the userpod
        // already exists (re-onboarding after keychain clear), log and continue.
        let store = UserStore::open(&config.db_path, &config.db_passphrase).map_err(|e| {
            OnboardingError::Service(ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: format!("Failed to open user DB: {e}"),
            })
        })?;
        if store
            .get_userpod(&self.userpod_name)
            .map_err(|e| {
                OnboardingError::Service(ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Storage,
                    source: None,
                    message: format!("Failed to look up userpod: {e}"),
                })
            })?
            .is_some()
        {
            tracing::info!(
                target: "hkask.onboarding",
                userpod = %self.userpod_name,
                "Userpod already exists in UserStore — skipping creation"
            );
        } else {
            store
                .register_userpod(&RegistrationRequest {
                    userpod_name: self.userpod_name.clone(),
                    first_name: self.first_name.clone(),
                    last_name: self.last_name.clone(),
                    email: self.email.clone(),
                    phone: None,
                    passphrase: passphrase.to_string(),
                    capabilities: hkask_identity::UserPod::DEFAULT_CAPABILITIES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                })
                .map_err(|e| {
                    eprintln!("  \x1b[31m✗\x1b[0m Failed to create userpod: {}", e);
                    OnboardingError::Service(ServiceError::Domain {
                        kind: ErrorKind::BadRequest,
                        domain: DomainKind::Storage,
                        source: None,
                        message: format!("Failed to register userpod: {e}"),
                    })
                })?;
        }

        // Persist secrets to the keychain now that the userpod exists.
        let keychain = hkask_keystore::Keychain::default();
        let _ = keychain.store_by_key(
            hkask_types::keychain_keys::KEY_MASTER_KEY,
            &resolved.master_key_hex,
        );
        let _ = keychain.store_by_key(
            hkask_types::keychain_keys::KEY_A2A_SECRET,
            &resolved.a2a_secret,
        );
        let _ = keychain.store_by_key(hkask_types::keychain_keys::KEY_DB_PASSPHRASE, passphrase);

        tracing::info!(
            target: "hkask.onboarding",
            userpod = %self.userpod_name,
            "Userpod created and secrets stored in keychain"
        );
        Ok(())
    }

    async fn advance_a2a(&mut self) -> Result<(), OnboardingError> {
        let resolved = self.resolved_secrets.as_ref().ok_or_else(|| {
            OnboardingError::Service(ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Infrastructure,
                source: None,
                message: "Secrets not derived before A2A registration".into(),
            })
        })?;
        let config = ServiceConfig::from_secrets(
            resolved.a2a_secret.clone(),
            resolved.db_passphrase.clone(),
            self.userpod_name.clone(),
        );
        let a2a = Arc::new(A2ARuntime::new(&config.a2a_secret));
        OnboardingService::register_userpod(&a2a, &self.userpod_name)
            .await
            .map_err(|e| {
                eprintln!(
                    "  \x1b[31m✗\x1b[0m Failed to register userpod in A2A: {}",
                    e
                );
                OnboardingError::Service(e)
            })?;
        Ok(())
    }
}

/// The completed session, ready for post-onboarding summary.
pub struct CompletedSession {
    pub userpod_name: String,
    pub selected_model: String,
    pub resolved_secrets: Option<ResolvedSecrets>,
}
