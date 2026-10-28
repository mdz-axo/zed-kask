//! Server construction context and credential requirements.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Server construction context. No ambient authority — all deps injected here.
pub struct ServerContext {
    pub credentials: HashMap<String, String>,

    /// Resolved from HKASK_WEBID → anonymous.
    pub webid: hkask_types::WebID,
}
