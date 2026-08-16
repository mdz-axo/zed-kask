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

// ── Default values ──────────────────────────────────────────────────────────
// Centralized here so all three constructors share the same defaults.
// Changing a default means changing it once.
// Public so standalone CLI commands (without a ServiceConfig) can use the
// same defaults instead of duplicating string literals.

const DEFAULT_ENERGY_BUDGET_CAP: u64 = 10_000;
const DEFAULT_GAS_REPLENISH_RATE: u64 = 1_000;
const DEFAULT_REG_THRESHOLD: u64 = 100;
/// Default template cache path, resolved from the platform cache directory.
///
/// Uses `$XDG_CACHE_HOME/hkask/templates` on Linux, `~/Library/Caches/hkask/templates`
/// on macOS, and `%LOCALAPPDATA%/hkask/templates` on Windows via `dirs::cache_dir()`.
/// Falls back to `/tmp/hkask-templates` only if `dirs` cannot determine a cache dir.
/// Override: `HKASK_TEMPLATE_CACHE_PATH` env var.
fn default_template_cache_path() -> String {
    dirs::cache_dir()
        .map(|d| {
            d.join("hkask")
                .join("templates")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| DEFAULT_TEMPLATE_CACHE_PATH_FALLBACK.to_string())
}

/// Fallback when `dirs::cache_dir()` returns `None` (rare: no HOME set).
const DEFAULT_TEMPLATE_CACHE_PATH_FALLBACK: &str = "/tmp/hkask-templates";
/// Fallback agent name when `HKASK_AGENT_NAME` is not set.
///
/// In zed-kask, the composition root sets `HKASK_AGENT_NAME` from the
/// Zed login username. This constant is only used for standalone CLI usage
/// (admin commands, repair tools) where no Zed session is available.
const DEFAULT_USER_NAME: &str = "curator";
const TEST_USER_NAME: &str = "test-user";

/// Default memory retention in days (≈6 months).
///
/// Controls how long episodic and semantic memory entries persist before
/// decay. Override: `HKASK_MEMORY_LIFE_DAYS` env var.
const DEFAULT_MEMORY_LIFE_DAYS: f64 = 180.0;

// Default DB filename and data-dir resolution are path primitives owned by
// `hkask_types::agent_paths`; re-exported here so this crate's historical
// public API (`hkask_services_core::DEFAULT_DB_PATH`, `::config::resolve_data_dir`)
// keeps resolving for existing consumers.
pub use hkask_types::agent_paths::{DEFAULT_DB_PATH, resolve_data_dir};

/// Configuration resolved once at startup and shared across all services.
///
/// This consolidates the construction paths that previously existed in
/// `ReplState`, `ApiState`, and the loop wiring modules.
///
/// Construction methods:
/// - `from_env()` — resolves secrets from environment variables and keychain
/// - `from_secrets()` — accepts pre-resolved secrets (from the composition root)
/// - `in_memory()` — synthetic config for tests
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Path to the primary database file (hkask.db).
    pub db_path: String,

    /// Passphrase for encrypted database access.
    pub db_passphrase: String,

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

    /// User name (from the Zed login via `HKASK_AGENT_NAME`, or `DEFAULT_USER_NAME`
    /// for standalone CLI). The 1:1 agent name.
    pub user_name: String,

    /// Path for the template cache (Git CAS storage).
    pub template_cache_path: String,

    /// Path for the memory database (episodic + semantic stores).
    ///
    /// When `in_memory: false`, memory stores persist to this file.
    /// Defaults to `{db_path}-memory.db` (e.g., `hkask.db` → `hkask-memory.db`)
    /// when not explicitly set. Ignored when `in_memory: true`.
    pub memory_db_path: Option<String>,

    /// Episodic memory life in days — configurable, default 180 (6 months × 30).
    ///
    /// Sets S in Wozniak & Gorzelanczyk (1995) forgetting curve: R(t) = exp(-t/S).
    /// After S days without recall, confidence decays to exp(-1) ≈ 36.8%.
    /// Recalling a memory resets its decay clock (t goes back to 0).
    /// Override via HKASK_MEMORY_LIFE_DAYS env var.
    pub memory_life_days: f64,
}

impl ServiceConfig {
    /// Resolve configuration from environment variables and keychain.
    ///
    /// Reads `HKASK_DB_PATH`, `HKASK_TEMPLATE_CACHE_PATH`,
    /// `HKASK_MEMORY_DB_PATH`, and `HKASK_AGENT_NAME` from environment.
    /// The database passphrase is resolved via `hkask-keystore`.
    ///
    /// The agent name defaults to `HKASK_AGENT_NAME` env var (set by the
    /// zed-kask composition root from the Zed login username), falling back to
    /// `DEFAULT_USER_NAME` ("curator") for standalone CLI usage.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  keystore must have db_passphrase configured
    /// post: returns ServiceConfig with env-derived values and keystore secrets; Err(Keystore) on secret resolution failure
    #[must_use = "result must be used"]
    pub fn from_env() -> Result<Self, ServiceError> {
        let data_dir = resolve_data_dir();
        let db_path = std::env::var("HKASK_DB_PATH")
            .unwrap_or_else(|_| data_dir.join(DEFAULT_DB_PATH).to_string_lossy().to_string());
        let inference_config = InferenceConfig::from_env();
        let default_model = inference_config.default_model.clone();
        let template_cache_path = std::env::var("HKASK_TEMPLATE_CACHE_PATH")
            .unwrap_or_else(|_| default_template_cache_path());
        let memory_db_path = std::env::var("HKASK_MEMORY_DB_PATH").ok();
        let user_name = std::env::var("HKASK_AGENT_NAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_USER_NAME.to_string());

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

        // A malformed numeric env var must warn, not silently fall back — an
        // operator cannot distinguish "not configured" from "configured but
        // broken" otherwise (`.rules` failure-signal trap). Mirrors the
        // canonical `parse_memory_life_days` in `kask_bridge/src/memory.rs`.
        let memory_life_days = match std::env::var("HKASK_MEMORY_LIFE_DAYS") {
            Ok(raw) => match raw.parse::<f64>() {
                Ok(days) => days,
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.services.config",
                        value = %raw,
                        error = %e,
                        "HKASK_MEMORY_LIFE_DAYS malformed — falling back to default {DEFAULT_MEMORY_LIFE_DAYS}"
                    );
                    DEFAULT_MEMORY_LIFE_DAYS
                }
            },
            Err(_) => DEFAULT_MEMORY_LIFE_DAYS,
        };
        Ok(Self {
            db_path,
            db_passphrase,
            default_model,
            inference_config,
            reg_threshold: DEFAULT_REG_THRESHOLD,
            energy_budget_cap: DEFAULT_ENERGY_BUDGET_CAP,
            gas_replenish_rate: DEFAULT_GAS_REPLENISH_RATE,
            in_memory: false,
            user_name,
            template_cache_path,
            memory_db_path,
            memory_life_days,
        })
    }

    /// Create a config from pre-resolved secrets.
    ///
    /// This avoids re-resolving from the keychain, which is useful when the
    /// caller has already resolved secrets (e.g., the zed-kask composition root
    /// which derives identity from the Zed login).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  db_passphrase and user_name must be non-empty
    /// post: returns ServiceConfig with provided secrets and env-derived or default values
    #[must_use]
    pub fn from_secrets(db_passphrase: String, user_name: String) -> Self {
        let data_dir = resolve_data_dir();
        let db_path = std::env::var("HKASK_DB_PATH")
            .unwrap_or_else(|_| data_dir.join(DEFAULT_DB_PATH).to_string_lossy().to_string());
        let inference_config = InferenceConfig::from_env();
        let template_cache_path = std::env::var("HKASK_TEMPLATE_CACHE_PATH")
            .unwrap_or_else(|_| default_template_cache_path());
        let memory_db_path = std::env::var("HKASK_MEMORY_DB_PATH").ok();
        // A malformed numeric env var must warn, not silently fall back (`.rules`
        // failure-signal trap). Mirrors `from_env` and the canonical
        // `parse_memory_life_days` in `kask_bridge/src/memory.rs`.
        let memory_life_days = match std::env::var("HKASK_MEMORY_LIFE_DAYS") {
            Ok(raw) => match raw.parse::<f64>() {
                Ok(days) => days,
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.services.config",
                        value = %raw,
                        error = %e,
                        "HKASK_MEMORY_LIFE_DAYS malformed — falling back to default {DEFAULT_MEMORY_LIFE_DAYS}"
                    );
                    DEFAULT_MEMORY_LIFE_DAYS
                }
            },
            Err(_) => DEFAULT_MEMORY_LIFE_DAYS,
        };
        Self {
            db_path,
            db_passphrase,
            inference_config: inference_config.clone(),
            reg_threshold: DEFAULT_REG_THRESHOLD,
            energy_budget_cap: DEFAULT_ENERGY_BUDGET_CAP,
            gas_replenish_rate: DEFAULT_GAS_REPLENISH_RATE,
            in_memory: false,
            default_model: inference_config.default_model,
            user_name,
            template_cache_path,
            memory_db_path,
            memory_life_days,
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
            inference_config: inference_config.clone(),
            reg_threshold: DEFAULT_REG_THRESHOLD,
            energy_budget_cap: DEFAULT_ENERGY_BUDGET_CAP,
            gas_replenish_rate: DEFAULT_GAS_REPLENISH_RATE,
            in_memory: true,
            default_model: inference_config.default_model,
            user_name: TEST_USER_NAME.to_string(),
            template_cache_path: default_template_cache_path(),
            memory_db_path: None,
            memory_life_days: DEFAULT_MEMORY_LIFE_DAYS,
        }
    }
}

impl ServiceConfig {
    /// Returns the effective memory DB path when `in_memory: false`.
    ///
    /// Uses the standard agent directory layout: `agents/{user_name}/memory.db`.
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
            hkask_types::agent_paths::agent_memory_db(&self.user_name)
                .to_string_lossy()
                .to_string(),
        )
    }

    /// Open a SQLite database driver.
    ///
    /// Opens a SQLCipher database at `db_path` with `db_passphrase`.
    /// Returns an `Arc<dyn DatabaseDriver>` ready for store construction.
    ///
    /// pre:  `db_path` is a valid SQLite file path.
    /// post: returns a connected driver with schema initialized.
    pub fn open_driver(
        &self,
    ) -> Result<std::sync::Arc<dyn hkask_storage::DatabaseDriver>, ServiceError> {
        let db = hkask_storage::open_database(&self.db_path, &self.db_passphrase).map_err(|e| {
            ServiceError::Domain {
                kind: ErrorKind::ServiceUnavailable,
                domain: DomainKind::Storage,
                message: e.to_string(),
                source: Some(Box::new(e)),
            }
        })?;
        let pool = db.sqlite_pool().map_err(|e| ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Storage,
            message: e.to_string(),
            source: Some(Box::new(e)),
        })?;
        Ok(std::sync::Arc::new(
            hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, self.db_path.as_str()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{DomainKind, ErrorKind, ServiceError};

    fn sqlite_config(path: &str) -> ServiceConfig {
        let mut config = ServiceConfig::from_secrets(
            "test-db-passphrase".to_string(),
            TEST_USER_NAME.to_string(),
        );
        config.db_path = path.to_string();
        config
    }

    /// Sqlite open failure must surface as `ServiceError::Domain` over
    /// `DomainKind::Storage`, preserving the `DatabaseError` source chain
    /// so callers can inspect the specific failure mode.
    #[test]
    fn open_driver_sqlite_error_is_typed_storage_domain() {
        // A path under a non-existent directory fails at open time.
        let config = sqlite_config("/nonexistent-dir/should-fail.db");
        let err = match config.open_driver() {
            Ok(_) => panic!("expected open_driver to fail on a non-existent path"),
            Err(e) => e,
        };

        match err {
            ServiceError::Domain {
                kind,
                domain,
                source,
                ..
            } => {
                assert_eq!(domain, DomainKind::Storage);
                assert_eq!(kind, ErrorKind::ServiceUnavailable);
                // The source chain must carry the underlying DatabaseError.
                let source = source.expect("source chain must preserve DatabaseError");
                let db_err = source
                    .downcast_ref::<hkask_storage::DatabaseError>()
                    .expect("source must be a DatabaseError");
                assert!(
                    !db_err.to_string().is_empty(),
                    "source DatabaseError must carry a message, got: {db_err}"
                );
            }
            other => panic!("expected ServiceError::Domain, got {other:?}"),
        }
    }
}
