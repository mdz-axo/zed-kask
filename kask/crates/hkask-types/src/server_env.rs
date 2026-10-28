//! `ServerEnv` — the environment map for an MCP server child process.
//!
//! Invariant (I2 of the MCP server lifecycle review, 2026-08-29): every kask
//! MCP server process must be spawned with an environment composed by the
//! single canonical path (`kask_bridge::build_mcp_server_env`), which applies
//! the per-server config allowlist, resolves keychain credentials, and
//! injects the inference socket. The keyless-server defect shipped because a
//! raw settings entry bypassed that path entirely.
//!
//! This type makes the invariant visible in every signature: spawn functions
//! take `ServerEnv`, and the only construction path is
//! [`ServerEnv::from_canonical`], called by `build_mcp_server_env`. The
//! constructor is `#[doc(hidden)]` rather than `pub(crate)` because the type
//! crosses crate boundaries (hkask-types ← kask_bridge ← hkask-mcp) — the
//! enforcement is convention-plus-review, not the type system. The structural
//! fix (deleting the bypassing spawn paths) is the single-supervisor
//! migration; this type is the signature-level half.

use std::collections::HashMap;

/// An MCP server child-process environment, composed only by
/// `kask_bridge::build_mcp_server_env`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerEnv(HashMap<String, String>);

impl ServerEnv {
    /// Canonical construction — for `build_mcp_server_env` only. Every other
    /// construction of a server env is a review flag: it bypasses the config
    /// allowlist, the credential resolution, or the inference socket.
    #[doc(hidden)]
    pub fn from_canonical(env: HashMap<String, String>) -> Self {
        Self(env)
    }

    /// Read access for spawn sites and env-diff observers.
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, String, String> {
        self.0.iter()
    }

    /// Read a single variable (assertions, logging, tests).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|value| value.as_str())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The composed map, for boundaries that must hand env to non-kask types
    /// (zed's `ContextServerCommand`). Each call site documents why the
    /// escape hatch is legitimate.
    #[doc(hidden)]
    pub fn into_inner(self) -> HashMap<String, String> {
        self.0
    }
}
