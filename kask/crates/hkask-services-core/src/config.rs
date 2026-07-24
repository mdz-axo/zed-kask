//! Service-level configuration resolved once at startup.
//! # REQ: P1 (User Sovereignty) — secrets from OS keychain, never hardcoded.
//! expect: "Service configuration resolves secrets from the OS keychain"
//!
//! `ServiceConfig` holds all configuration that varies per deployment:
//! database paths, secrets, thresholds, and feature flags. Both CLI and API
//! surfaces construct a `ServiceConfig` from environment variables, keychain
//! secrets, or explicit parameters, then pass it to `AgentService::build()`.

use crate::error::{DomainKind, ErrorKind, ServiceError};
use hkask_inference::InferenceConfig;
use hkask_storage::database::types::DbProvider;
use hkask_types::WalletConfig;

// ── Default values ──────────────────────────────────────────────────────────
// Centralized here so all three constructors share the same defaults.
// Changing a default means changing it once.
// Public so standalone CLI commands (without a ServiceConfig) can use the
// same defaults instead of duplicating string literals.

const DEFAULT_ENERGY_BUDGET_CAP: u64 = 10_000;
const DEFAULT_GAS_REPLENISH_RATE: u64 = 1_000;
const DEFAULT_REG_THRESHOLD: u64 = 100;
const DEFAULT_TEMPLATE_CACHE_PATH: &str = "/tmp/hkask-templates";
const DEFAULT_USER_NAME: &str = "curator";
const TEST_USER_NAME: &str = "test-user";

/// Default path for the primary database file.
/// Resolved relative to `resolve_data_dir()` unless overridden via `HKASK_DB_PATH`.
pub const DEFAULT_DB_PATH: &str = "hkask.db";

/// Resolve the hKask data directory.
///
/// Order of precedence:
/// 1. `HKASK_DATA_DIR` environment variable
/// 2. `$XDG_DATA_HOME/hkask`
/// 3. `$HOME/.local/share/hkask`
/// 4. Current working directory (fallback)
///
/// All relative database paths in `ServiceConfig` are resolved against
/// this directory, ensuring agent databases stay in a predictable location
/// regardless of where `kask` is invoked from.
#[must_use]
pub fn resolve_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HKASK_DATA_DIR") {
        let p = std::path::PathBuf::from(&dir);
        if p.is_absolute() || p.starts_with(".") {
            return p;
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("hkask");
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("hkask");
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Configuration resolved once at startup and shared across all services.
///
/// This consolidates the construction paths that previously existed in
/// `ReplState`, `ApiState`, and the loop wiring modules.
///
/// Construction methods:
/// - `from_env()` — resolves secrets from environment variables and keychain
/// - `from_secrets()` — accepts pre-resolved secrets (from onboarding)
/// - `in_memory()` — synthetic config for tests
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Path to the primary database file (hkask.db).
    pub db_path: String,

    /// Passphrase for encrypted database access.
    pub db_passphrase: String,

    /// Database provider — `sqlite` (default) or `postgres` (future).
    /// Set via `HKASK_DB_PROVIDER` env var.
    pub db_provider: DbProvider,

    /// Secret for the A2A root authority and manifest delegation tokens.
    pub a2a_secret: Vec<u8>,

    /// Inference configuration for the multi-provider router.
    pub inference_config: InferenceConfig,

    /// Regulation variety threshold for algedonic alerts.
    pub reg_threshold: u64,

    /// Gas budget cap per session (units).
    pub energy_budget_cap: u64,

    /// Gas replenish rate per turn (units).
    pub gas_replenish_rate: u64,

    /// Whether to use in-memory databases (for tests).
    pub in_memory: bool,

    /// Default inference model name.
    pub default_model: String,

    /// User name (from onboarding or config). The 1:1 userpod name.
    pub user_name: String,

    /// Path for the template cache (Git CAS storage).
    pub template_cache_path: String,

    /// Path for the memory database (episodic + semantic stores).
    ///
    /// When `in_memory: false`, memory stores persist to this file.
    /// Defaults to `{db_path}-memory.db` (e.g., `hkask.db` → `hkask-memory.db`)
    /// when not explicitly set. Ignored when `in_memory: true`.
    pub memory_db_path: Option<String>,

    /// Wallet configuration for rJoule payments and multi-chain deposits.
    pub wallet_config: WalletConfig,

    /// Episodic memory life in days — configurable, default 180 (6 months × 30).
    ///
    /// Sets S in Wozniak & Gorzelanczyk (1995) forgetting curve: R(t) = exp(-t/S).
    /// After S days without recall, confidence decays to exp(-1) ≈ 36.8%.
    /// Recalling a memory resets its decay clock (t goes back to 0).
    /// Override via HKASK_MEMORY_LIFE_DAYS env var.
    pub memory_life_days: f64,

    /// Whether the Curator daemon may auto-consolidate memory when escalations exist.
    ///
    /// This is an opt-in flag (default `false`) gated by P2 affirmative consent.
    /// Even when enabled, the Curator checks consent for both `EpisodicMemory`
    /// and `SemanticMemory` before running, and posts an escalation entry describing
    /// the event. Override via `HKASK_CURATOR_AUTO_CONSOLIDATION=1`.
    pub curator_auto_consolidation_enabled: bool,
}

impl ServiceConfig {
    /// Resolve configuration from environment variables and keychain.
    ///
    /// Reads `HKASK_DB_PATH`, `HKASK_TEMPLATE_CACHE_PATH`,
    /// and `HKASK_MEMORY_DB_PATH` from environment. The A2A authority secret
    /// and database passphrase are resolved via `hkask-keystore`.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  keystore must have a2a_secret and db_passphrase configured
    /// post: returns ServiceConfig with env-derived values and keystore secrets; Err(Keystore) on secret resolution failure
    #[must_use = "result must be used"]
    pub fn from_env() -> Result<Self, ServiceError> {
        let data_dir = resolve_data_dir();
        let db_path = std::env::var("HKASK_DB_PATH")
            .unwrap_or_else(|_| data_dir.join(DEFAULT_DB_PATH).to_string_lossy().to_string());
        let db_provider =
            parse_db_provider(&std::env::var("HKASK_DB_PROVIDER").unwrap_or_default());
        let inference_config = InferenceConfig::from_env();
        let default_model = inference_config.default_model.clone();
        let template_cache_path = std::env::var("HKASK_TEMPLATE_CACHE_PATH")
            .unwrap_or_else(|_| DEFAULT_TEMPLATE_CACHE_PATH.to_string());
        let memory_db_path = std::env::var("HKASK_MEMORY_DB_PATH").ok();

        // Resolve secrets from keystore. If keystore resolution fails,
        // fall back to empty secrets (in-memory mode will be used).
        let a2a_secret = hkask_keystore::keychain::resolve_a2a_secret()
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Infrastructure,
                source: Some(Box::new(e)),
                message: "Failed to resolve A2A secret".into(),
            })?
            .to_vec();

        let db_passphrase =
            hkask_keystore::keychain::resolve_db_passphrase_string().map_err(|e| {
                ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Infrastructure,
                    source: Some(Box::new(e)),
                    message: "Failed to resolve DB passphrase".into(),
                }
            })?;
        let db_passphrase = db_passphrase.to_string();

        let memory_life_days = std::env::var("HKASK_MEMORY_LIFE_DAYS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(180.0);
        let curator_auto_consolidation_enabled = read_curator_auto_consolidation_env();

        Ok(Self {
            db_path,
            db_passphrase,
            db_provider,
            a2a_secret,
            default_model,
            inference_config,
            reg_threshold: DEFAULT_REG_THRESHOLD,
            energy_budget_cap: DEFAULT_ENERGY_BUDGET_CAP,
            gas_replenish_rate: DEFAULT_GAS_REPLENISH_RATE,
            in_memory: false,
            user_name: DEFAULT_USER_NAME.to_string(),
            template_cache_path,
            memory_db_path,
            wallet_config: WalletConfig::default(),
            memory_life_days,
            curator_auto_consolidation_enabled,
        })
    }

    /// Create a config from pre-resolved secrets (e.g., from onboarding).
    ///
    /// This avoids re-resolving from the keychain, which is important
    /// for the REPL's interactive onboarding flow.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  a2a_secret, db_passphrase, and user_name must be non-empty
    /// post: returns ServiceConfig with provided secrets and env-derived or default values
    #[must_use]
    pub fn from_secrets(a2a_secret: String, db_passphrase: String, user_name: String) -> Self {
        let data_dir = resolve_data_dir();
        let db_path = std::env::var("HKASK_DB_PATH")
            .unwrap_or_else(|_| data_dir.join(DEFAULT_DB_PATH).to_string_lossy().to_string());
        let inference_config = InferenceConfig::from_env();
        let template_cache_path = std::env::var("HKASK_TEMPLATE_CACHE_PATH")
            .unwrap_or_else(|_| DEFAULT_TEMPLATE_CACHE_PATH.to_string());
        let memory_db_path = std::env::var("HKASK_MEMORY_DB_PATH").ok();
        let memory_life_days = std::env::var("HKASK_MEMORY_LIFE_DAYS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(180.0);
        let curator_auto_consolidation_enabled = read_curator_auto_consolidation_env();

        Self {
            db_path,
            db_passphrase,
            db_provider: parse_db_provider(&std::env::var("HKASK_DB_PROVIDER").unwrap_or_default()),
            a2a_secret: a2a_secret.into_bytes(),
            inference_config: inference_config.clone(),
            reg_threshold: DEFAULT_REG_THRESHOLD,
            energy_budget_cap: DEFAULT_ENERGY_BUDGET_CAP,
            gas_replenish_rate: DEFAULT_GAS_REPLENISH_RATE,
            in_memory: false,
            default_model: inference_config.default_model.clone(),
            user_name,
            template_cache_path,
            memory_db_path,
            wallet_config: WalletConfig::default(),
            memory_life_days,
            curator_auto_consolidation_enabled,
        }
    }

    /// Create a config suitable for integration tests.
    ///
    /// Uses in-memory databases and synthetic secrets. Never use in production.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  none (always succeeds)
    /// post: returns ServiceConfig with :memory: DB, zeroed secrets, and test agent name
    #[must_use]
    pub fn in_memory() -> Self {
        let inference_config = InferenceConfig::default();
        Self {
            db_path: ":memory:".to_string(),
            db_passphrase: String::new(),
            db_provider: DbProvider::Sqlite,
            a2a_secret: vec![0u8; 32],
            inference_config: inference_config.clone(),
            reg_threshold: DEFAULT_REG_THRESHOLD,
            energy_budget_cap: DEFAULT_ENERGY_BUDGET_CAP,
            gas_replenish_rate: DEFAULT_GAS_REPLENISH_RATE,
            in_memory: true,
            default_model: inference_config.default_model.clone(),
            user_name: TEST_USER_NAME.to_string(),
            template_cache_path: DEFAULT_TEMPLATE_CACHE_PATH.to_string(),
            memory_db_path: None,
            wallet_config: WalletConfig::default(),
            memory_life_days: 180.0,
            curator_auto_consolidation_enabled: false,
        }
    }
}

/// Read `HKASK_CURATOR_AUTO_CONSOLIDATION` env var into a bool.
///
/// Returns `true` when set to `"1"`, `false` otherwise (including unset).
/// Centralized here to avoid duplicating the env-var read across constructors.
fn read_curator_auto_consolidation_env() -> bool {
    std::env::var("HKASK_CURATOR_AUTO_CONSOLIDATION").as_deref() == Ok("1")
}

/// Parse the `HKASK_DB_PROVIDER` env var into a `DbProvider`.
/// Defaults to `Sqlite` for unknown or empty values.
fn parse_db_provider(raw: &str) -> DbProvider {
    match raw.to_lowercase().as_str() {
        "" | "sqlite" => DbProvider::Sqlite,
        "postgres" | "postgresql" | "pg" => {
            tracing::warn!(
                "HKASK_DB_PROVIDER=postgres selected but PostgresDriver is not yet implemented. Setups using postgres will fail at startup with a clear error. Use sqlite until v0.32."
            );
            DbProvider::Postgres
        }
        other => {
            tracing::warn!("Unknown HKASK_DB_PROVIDER='{other}' — falling back to sqlite");
            DbProvider::Sqlite
        }
    }
}

impl ServiceConfig {
    /// Returns the effective memory DB path when `in_memory: false`.
    ///
    /// Uses the standard userpod directory layout: `userpods/{user_name}/memory.db`.
    /// This puts the agent's memory database alongside its pod database in the
    /// same directory, so all of an agent's data is self-contained in one folder.
    ///
    /// Returns `None` when `in_memory: true` (memory stores are ephemeral).
    ///
    /// pre:  none (always succeeds)
    /// post: returns Some(path) using agent dir layout if not in_memory; None if in_memory
    #[must_use]
    pub fn effective_memory_db_path(&self) -> Option<String> {
        if self.in_memory {
            return None;
        }
        Some(
            hkask_types::agent_paths::userpod_memory_db(&self.user_name)
                .to_string_lossy()
                .to_string(),
        )
    }
}
