//! Onboarding — secret derivation, keychain, A2A registration, sign-in.
use hkask_keystore::{Keychain, master_key::derive_all_internal_secrets};
use hkask_pods::A2ARuntime;
use hkask_services_core::{DomainKind, ErrorKind, ServiceConfig, ServiceError};
use hkask_types::WebID;
use std::sync::Arc;

pub mod matrix;

#[derive(Debug, Clone)]
pub struct ResolvedSecrets {
    pub master_key_hex: String,
    pub a2a_secret: String,
    pub db_passphrase: String,
}

#[derive(Debug)]
pub struct SignInOutcome {
    pub agent_name: String,
    pub resolved_secrets: ResolvedSecrets,
}

pub struct OnboardingService;

impl OnboardingService {
    pub fn derive_secrets(passphrase: &str) -> Result<ResolvedSecrets, ServiceError> {
        let secrets = derive_all_internal_secrets(passphrase);
        // P9: Regulation span
        tracing::info!(
            target: "hkask.onboarding",
            operation = "secrets_derived",
            "REG"
        );
        Ok(ResolvedSecrets {
            master_key_hex: secrets.master_key_hex,
            a2a_secret: secrets.a2a_secret,
            db_passphrase: passphrase.to_string(),
        })
    }

    pub fn init_a2a(config: &ServiceConfig) -> Arc<A2ARuntime> {
        Arc::new(A2ARuntime::new(&config.a2a_secret))
    }

    pub async fn register_userpod(a2a: &Arc<A2ARuntime>, name: &str) -> Result<(), ServiceError> {
        let webid = WebID::from_persona(name.as_bytes());
        let caps = vec![
            "tool:inference:call".to_string(),
            "tool:mcp:invoke".to_string(),
        ];
        a2a.register_agent(webid, caps)
            .await
            .map_err(|e| ServiceError::Domain {
                domain: DomainKind::Agent,
                kind: ErrorKind::Forbidden,
                source: None,
                message: e.to_string(),
            })?;
        tracing::info!(target: "hkask.onboarding", operation = "userpod_registered", name = %name, "REG");
        Ok(())
    }

    pub async fn try_sign_in(
        config: &ServiceConfig,
        agent_name: &str,
        resolved: &ResolvedSecrets,
    ) -> Result<SignInOutcome, ServiceError> {
        let db_path = &config.db_path;
        if db_path != ":memory:" && std::path::Path::new(db_path).exists() {
            hkask_storage::Database::open(db_path, &resolved.db_passphrase).map_err(|_| {
                ServiceError::Domain {
                    domain: DomainKind::Agent,
                    kind: ErrorKind::NotFound,
                    source: None,
                    message: "DB passphrase mismatch".into(),
                }
            })?;
        }
        let kc = Keychain::default();
        let _ = kc.store_by_key(
            hkask_types::keychain_keys::KEY_A2A_SECRET,
            &resolved.a2a_secret,
        );
        let _ = kc.store_by_key(
            hkask_types::keychain_keys::KEY_DB_PASSPHRASE,
            &resolved.db_passphrase,
        );
        Ok(SignInOutcome {
            agent_name: agent_name.to_string(),
            resolved_secrets: resolved.clone(),
        })
    }

    pub fn has_orphaned_db(config: &ServiceConfig) -> bool {
        let p = &config.db_path;
        p != ":memory:"
            && !p.is_empty()
            && std::path::Path::new(p).exists()
            && !config.db_passphrase.is_empty()
            && hkask_storage::Database::open(p, &config.db_passphrase).is_err()
    }

    pub fn remove_orphaned_db_unchecked(config: &ServiceConfig) -> bool {
        let p = &config.db_path;
        if p == ":memory:" || !std::path::Path::new(p).exists() {
            return false;
        }
        let _ = std::fs::remove_file(p);
        !std::path::Path::new(p).exists()
    }

    pub fn cleanup_failed_onboarding(config: &ServiceConfig) {
        if config.db_path != ":memory:" {
            let _ = std::fs::remove_file(&config.db_path);
        }
    }
}
