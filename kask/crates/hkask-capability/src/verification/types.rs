//! Token verification types — outcome enum, error constants, and error message helpers.
//!
//! Centralised here so that all MCP servers and adapters reference the same
//! strings, avoiding duplication and drift.

// ── Token error constants (P2.8) ──────────────────────────────────────────
// Centralised here so that all MCP servers and adapters reference the same
// strings, avoiding duplication and drift.

/// Token HMAC/signature verification failed.
pub const TOKEN_ERR_INVALID_SIGNATURE: &str = "Token signature verification failed";
/// Token has expired.
pub const TOKEN_ERR_EXPIRED: &str = "Token is expired";
/// No capability checker was available to validate the token.
pub const TOKEN_ERR_NO_CHECKER: &str = "No capability checker configured";

/// Outcome of verifying a delegation token.
///
/// Provides structured, granular failure modes so call sites can map each
/// failure to a specific error response instead of a generic boolean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// Token passed all verification checks.
    Valid,
    /// Token signature is invalid or tampered.
    InvalidSignature,
    /// Token has expired.
    Expired,
    /// Token does not grant the requested access.
    InsufficientAccess { resource_id: String, action: String },
    /// No capability checker was provided — access denied.
    NoChecker,
}

// ── Token error message helpers (P2.8) ──────────────────────────────────────
// Thin wrappers around the constants that produce the correct error type
// for each consumer, keeping message text in one place.

/// Format an "insufficient access" error message.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  resource_id is any &str; action is any &str
/// post: returns "Token does not authorize access to {resource_id} ({action})"
pub fn token_err_insufficient_access(resource_id: &str, action: &str) -> String {
    format!("Token does not authorize access to {resource_id} ({action})")
}

/// Format an "insufficient access for tool" error message.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  tool_name is any &str
/// post: returns "Token does not authorize tool: {tool_name}"
pub fn token_err_tool_access_denied(tool_name: &str) -> String {
    format!("Token does not authorize tool: {tool_name}")
}
