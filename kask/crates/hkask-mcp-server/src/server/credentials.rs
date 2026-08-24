//! Credential resolution — keychain-first credential lookup.

use std::collections::HashMap;

use super::error::McpToolError;

/// Routes known credential names through the proper hkask keystore resolvers.
///
/// For unrecognized credential names, falls back to keychain lookup by env var name
/// and then environment variable lookup.
///
/// pre:  env_var is non-empty
/// post: returns credential value from the appropriate resolution chain
#[must_use = "result must be used"]
pub fn resolve_credential(env_var: &str) -> Result<String, hkask_keystore::KeystoreError> {
    match env_var {
        "HKASK_DB_PASSPHRASE" => {
            let passphrase = hkask_keystore::keychain::resolve_db_passphrase_string()?;
            Ok(passphrase.to_string())
        }

        _ => {
            // Unrecognized credential — try keychain, then env var.
            // `retrieve_by_key` returns `Zeroizing<String>` (RR-0063). This
            // function's contract is a plain `String` (the MCP `ServerContext`
            // credential map is not zeroizing), so the wipe guarantee ends here;
            // the keychain read itself no longer leaves a copy behind.
            let val = hkask_keystore::Keychain::default()
                .retrieve_by_key(env_var)
                .map(|secret| secret.to_string())
                .or_else(|_| std::env::var(env_var))
                .map_err(|_| {
                    hkask_keystore::KeystoreError::NotFound(hkask_types::NotFound {
                        entity_type: "credential".to_string(),
                        id: format!(
                            "Credential '{}' not found in keychain or environment",
                            env_var
                        ),
                    })
                })?;
            tracing::debug!(
                credential = env_var,
                "Credential resolved via keychain or environment"
            );
            Ok(val)
        }
    }
}

/// Resolve `HKASK_DB_PASSPHRASE` using the canonical 2-tier chain:
/// 1. `ctx.credentials` (governed launch injection via `build_mcp_server_env`)
/// 2. `resolve_credential("HKASK_DB_PASSPHRASE")` (env var → keychain `hkask-db-passphrase`)
///
/// Returns `Ok(passphrase)` if found in any tier, or
/// `Err(McpToolError::permission_denied(...))` if all tiers miss. The error
/// names the env var and the keychain key so the operator knows what to set.
///
/// Servers that want to fall back to in-memory mode on miss should call this
/// and handle the `Err` with a `tracing::warn!` + in-memory fallback (the
/// `.rules` canonical pattern for degraded-mode servers).
///
/// Servers that require persistence (no in-memory fallback) should propagate
/// the `Err` directly — it's already `McpToolError::permission_denied`.
#[must_use = "result must be used"]
pub fn resolve_db_passphrase(
    credentials: &HashMap<String, String>,
) -> Result<String, McpToolError> {
    if let Some(passphrase) = credentials.get("HKASK_DB_PASSPHRASE") {
        if !passphrase.is_empty() {
            return Ok(passphrase.clone());
        }
    }
    match resolve_credential("HKASK_DB_PASSPHRASE") {
        Ok(passphrase) if !passphrase.is_empty() => Ok(passphrase),
        Ok(_) => Err(McpToolError::permission_denied(
            "HKASK_DB_PASSPHRASE resolved to an empty string. \
             Set HKASK_DB_PASSPHRASE via the keychain or environment variable.",
        )),
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.credentials",
                %error,
                "HKASK_DB_PASSPHRASE not found in credentials map, env, or keychain"
            );
            Err(McpToolError::permission_denied(format!(
                "HKASK_DB_PASSPHRASE not configured. \
                 Set HKASK_DB_PASSPHRASE via the keychain (kask://credentials/hkask_db_passphrase) \
                 or environment variable, or run provision_agent. \
                 Resolution error: {error}"
            )))
        }
    }
}

/// Parse a numeric env var, warning on malformed values.
///
/// A missing env var returns `default` silently ("not configured" is a
/// legitimate state). A present-but-unparsable value also returns `default`
/// but emits a `tracing::warn!` naming the env var and the malformed value so
/// the operator can distinguish "not configured" from "configured but broken"
/// (the silent-fallback trap from `.rules`).
///
/// Reference correct pattern — replaces the `.ok().and_then(|v| v.parse().ok())
/// .unwrap_or(default)` anti-pattern found across MCP servers.
///
/// # Example
/// ```no_run
/// let ttl = hkask_mcp_server::parse_env_warn("HKASK_CACHE_TTL_SECS", 60u64);
/// ```
#[must_use = "the parsed value must be used"]
pub fn parse_env_warn<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr + std::fmt::Debug,
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(raw) => match raw.parse::<T>() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.env",
                    key,
                    raw,
                    error = %e,
                    default = ?default,
                    "env var failed to parse — falling back to default"
                );
                default
            }
        },
        Err(_) => default,
    }
}
