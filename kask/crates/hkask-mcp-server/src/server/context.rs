//! Server context — credential requirements, capability detection, and construction context.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::credentials::resolve_credential;
use super::error::McpError;

/// A credential that an MCP server requires to function.
///
/// Servers declare these; the runtime resolves them from `hkask-keystore`
/// and passes them into the `ServerContext` at server construction time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialRequirement {
    /// Environment variable name the server expects (e.g., `"HKASK_GITHUB_TOKEN"`).
    pub env_var: String,
    /// Human-readable description of what this credential is for.
    pub description: String,
    /// Whether the server cannot function without this credential.
    /// Optional credentials allow degraded operation.
    pub required: bool,
}

impl CredentialRequirement {
    /// Declare a required credential.
    /// Create a required credential declaration.
    ///
    /// expect: "The system provides authenticated tool execution context for MCP servers"
    /// pre:  env_var and description are non-empty
    /// post: returns CredentialRequirement with required=true
    #[must_use]
    pub fn required(env_var: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            env_var: env_var.into(),
            description: description.into(),
            required: true,
        }
    }

    /// Declare an optional credential (allows degraded operation).
    /// Create an optional credential declaration.
    ///
    /// expect: "The system provides authenticated tool execution context for MCP servers"
    /// pre:  env_var and description are non-empty
    /// post: returns CredentialRequirement with required=false
    #[must_use]
    pub fn optional(env_var: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            env_var: env_var.into(),
            description: description.into(),
            required: false,
        }
    }
}

/// Infrastructure capabilities detected at server startup.
///
/// Computed from environment and credential resolution results — not configured.
/// Servers use this to advertise available tools and report their operating mode.
///
/// Two operating modes emerge from capability detection:
/// - **Embedded** (hKask runtime): WebID is non-anonymous, keystore reachable,
///   persistence available, Regulation consumes spans.
/// - **Standalone** (IDE): WebID is anonymous, keystore may be unavailable,
///   persistence unavailable, Regulation spans go to stderr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityTier {
    /// Running as part of an hKask installation (vs standalone in an IDE).
    pub embedded: bool,
    /// OS keychain is reachable for secret resolution.
    pub keystore_available: bool,
    /// Persistent storage (database) is configured.
    pub persistence_available: bool,
}

impl CapabilityTier {
    /// Detect capabilities from resolved credentials and environment.
    /// Detect which credentials are available from resolved values.
    ///
    /// expect: "The system provides authenticated tool execution context for MCP servers"
    /// pre:  resolved_credentials is a valid map
    /// post: returns CapabilityTier with embedded, keystore_available, persistence_available fields set
    #[must_use]
    pub fn detect(resolved_credentials: &HashMap<String, String>) -> Self {
        let embedded = resolved_credentials.contains_key("HKASK_WEBID")
            || resolved_credentials.contains_key("HKASK_USERPOD_PERSONA");
        let persistence_available = resolved_credentials.contains_key("HKASK_DB_PATH");
        let keystore_available = Self::probe_keystore();
        Self {
            embedded,
            keystore_available,
            persistence_available,
        }
    }

    /// Probe whether the OS keychain is reachable.
    ///
    /// Attempts a lightweight keychain read with a sentinel key.
    /// Returns `true` if the keychain responds (even with "not found"),
    /// `false` only if the platform keychain itself is broken/unavailable.
    fn probe_keystore() -> bool {
        match hkask_keystore::Keychain::default()
            .retrieve_by_key(hkask_types::keychain_keys::KEY_CAPABILITY_PROBE)
        {
            Ok(_) => true,
            Err(hkask_keystore::KeychainError::NotFound(_)) => true,
            Err(hkask_keystore::KeychainError::Platform(_)) => false,
        }
    }

    /// Regulation spans are meaningful only in embedded mode (consumed by hKask Regulation).
    /// In standalone mode, spans go to stderr via the tracing subscriber.
    /// Check if Regulation is available (all required credentials present).
    ///
    /// expect: "The system provides authenticated tool execution context for MCP servers"
    /// post: returns true iff embedded (Regulation spans consumed by runtime, not stderr)
    #[must_use]
    pub fn reg_available(&self) -> bool {
        self.embedded
    }
}

/// Server construction context. No ambient authority — all deps injected here.
pub struct ServerContext {
    pub credentials: HashMap<String, String>,

    /// Resolved from HKASK_WEBID → HKASK_USERPOD_PERSONA → anonymous.
    pub webid: hkask_types::WebID,

    /// Infrastructure capabilities detected at startup.
    pub capability_tier: CapabilityTier,
}

impl ServerContext {
    /// Resolve the DB passphrase from the credentials map or the hkask keystore chain.
    ///
    /// Tries the pre-resolved credentials map first, then falls back to
    /// `resolve_credential` which routes through the proper hkask keystore
    /// chain (env var → keychain `hkask-db-passphrase`).
    fn resolve_db_credential(&self) -> Result<String, McpError> {
        if let Some(passphrase) = self.credentials.get("HKASK_DB_PASSPHRASE") {
            return Ok(passphrase.clone());
        }
        resolve_credential("HKASK_DB_PASSPHRASE").map_err(|e| {
            McpError::DatabasePassphrase(format!("Failed to resolve DB passphrase: {e}"))
        })
    }

    /// Looks up `db_env_var` and resolves the passphrase. Falls back to in-memory DB.
    ///
    /// expect: "The system provides authenticated tool execution context for MCP servers"
    /// pre:  db_env_var is set and contains a valid path in credentials map
    /// post: returns opened Database with passphrase from credentials or keystore chain, or in-memory fallback
    #[must_use = "result must be used"]
    pub fn open_database(&self, db_env_var: &str) -> Result<hkask_storage::Database, McpError> {
        use hkask_storage::open_database;
        match self.credentials.get(db_env_var) {
            Some(path) => {
                let passphrase = self.resolve_db_credential()?;
                Ok(open_database(path, &passphrase)?)
            }
            None => Ok(hkask_storage::Database::in_memory()?),
        }
    }

    /// Like `open_database`, but passes DDL for custom tables (e.g. FTS5).
    ///
    /// expect: "The system provides authenticated tool execution context for MCP servers"
    /// pre:  db_env_var is set, extensions is valid SQL DDL
    /// post: returns opened Database with extensions applied, or in-memory fallback
    #[must_use = "result must be used"]
    pub fn open_database_with_extensions(
        &self,
        db_env_var: &str,
        extensions: &str,
    ) -> Result<hkask_storage::Database, McpError> {
        match self.credentials.get(db_env_var) {
            Some(path) => {
                let passphrase = self.resolve_db_credential()?;
                Ok(hkask_storage::Database::open_with_extensions(
                    path,
                    &passphrase,
                    extensions,
                )?)
            }
            None => Ok(hkask_storage::Database::in_memory_with_extensions(
                extensions,
            )?),
        }
    }
}
