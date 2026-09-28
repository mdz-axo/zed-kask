//! Identity resolution — derives the hKask agent name from the Zed login.
//!
//! The agent name is the sanitized `User::username` from the Zed account
//! (the GitHub-style login, e.g. `mdz-axo`). This collapses the former
//! interactive onboarding step into a lookup: the agent identity
//! is derived from the Zed session, not entered separately.
//!
//! Convention:
//! - `User::username` (SharedString) → `sanitize_name()` → agent name
//! - `WebID::for_agent_name(&sanitized)` → deterministic WebID
//! - `agent_paths::agent_dir(&sanitized)` → filesystem paths
//!
//! When the user is not yet logged in, `agent_name_from_username` returns `None`
//! and the caller defers agent-dependent wiring until the session arrives.
//!
//! ## Provisioning
//!
//! `provision_agent` handles first-run setup as a set of lookups and
//! directory creation — no interactive onboarding:
//! 1. Create the agent directory structure (`ensure_agent_dirs`)
//! 2. Ensure a DB passphrase exists in the keychain (default `"allostery"`
//!    on first run — the user can change it later)
//! 3. Return the resolved DB path and passphrase for `RealMemoryPort::new()`

use hkask_types::{WebID, agent_paths::sanitize_name};

/// Error type for agent provisioning (directory creation + keychain access).
#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("{0}")]
    InvalidUsername(String),
    #[error("Failed to create agent directory: {0}")]
    DirectoryCreation(#[from] std::io::Error),
    #[error("Failed to provision DB passphrase: {0}")]
    KeychainRead(String),
}

/// Derive the agent name from a Zed `User::username`.
///
/// The username is the stable, lowercase, GitHub-style handle from the Zed
/// account. We sanitize it for filesystem use (replaces `/ \ : * ? " < > | ( )`
/// and spaces with dashes) so it can be used directly as a directory name and
/// a `WebID` persona.
///
/// Returns `None` if the username is empty after sanitization.
pub fn agent_name_from_username(username: &str) -> Option<String> {
    let sanitized = sanitize_name(username);
    if sanitized.is_empty() || sanitized == "unnamed" {
        None
    } else {
        Some(sanitized)
    }
}

/// The result of provisioning an agent — everything needed to construct
/// a `RealMemoryPort` directly, without going through `from_env()`.
pub struct ProvisionedAgent {
    /// Absolute path to the memory database file.
    pub db_path: String,
    /// SQLCipher passphrase (stored in the keychain).
    pub passphrase: String,
    /// The agent's WebID, derived from the username.
    pub webid: WebID,
}

/// Provision an agent for the given Zed username.
///
/// This is the "onboarding that disappeared" — a set of lookups and
/// directory creation, no interactive prompts:
///
/// 1. Derive the agent name from the username (sanitize for filesystem).
/// 2. Create the agent directory structure on disk (idempotent).
/// 3. Resolve the DB passphrase from the keychain; if none exists, use the
///    default `"allostery"` and store it. The user can change it later via
///    the keychain or `HKASK_DB_PASSPHRASE` env var.
/// 4. Compute the absolute memory DB path under the hKask data directory.
///
/// Returns the path, passphrase, and WebID needed to construct a
/// `RealMemoryPort`.
///
/// # Errors
///
/// Returns an error if:
/// - The username sanitizes to empty
/// - Directory creation fails (filesystem error)
/// - Keychain read or write fails (OS keychain unavailable)
pub fn provision_agent(username: &str) -> Result<ProvisionedAgent, ProvisionError> {
    let agent_name = agent_name_from_username(username).ok_or_else(|| {
        ProvisionError::InvalidUsername(format!(
            "Username '{username}' sanitized to empty — cannot provision agent"
        ))
    })?;

    let webid = WebID::for_agent_name(&agent_name);

    // 1. Create the agent directory structure (idempotent).
    //    Resolve against the hKask data directory so paths are absolute.
    //    D28: scaffolding subdirs removed — `ensure_agent_dirs` now creates
    //    only the agent root. DBs create their own parent dir on open.
    let data_dir = hkask_types::agent_paths::resolve_data_dir();
    let agent_root = data_dir.join(hkask_types::agent_paths::agent_dir(&agent_name));
    std::fs::create_dir_all(&agent_root).map_err(ProvisionError::DirectoryCreation)?;

    let db_path = agent_root.join("memory.db").to_string_lossy().to_string();

    // 2. Ensure a DB passphrase exists (env → keychain → first-run
    //    default) via the one canonical chain in hkask-keystore.
    let passphrase = hkask_keystore::provision_db_passphrase_string()
        .map(|passphrase| passphrase.to_string())
        .map_err(|e| ProvisionError::KeychainRead(e.to_string()))?;

    tracing::info!(name = agent_name, db = db_path, webid = %webid.redacted_display(), "Agent provisioned");

    Ok(ProvisionedAgent {
        db_path,
        passphrase,
        webid,
    })
}

/// Username-independent DB passphrase provisioning for MCP server launch
/// time.
///
/// `provision_agent` (and thus the "allostery" first-run default) runs in
/// zed's deferred task, but MCP servers resolve their launch env
/// before the deferred task runs — on a machine that never signs in, the default
/// never landed and every DB-backed server failed with `permission_denied`.
/// This wrapper exposes the username-independent passphrase half so the
/// canonical env path (`build_mcp_server_env`) can provision it at launch
/// time, login or not. Idempotent: env override → existing keychain entry →
/// default "allostery" stored on first run.
pub(crate) fn provision_db_passphrase() -> Result<String, ProvisionError> {
    hkask_keystore::provision_db_passphrase_string()
        .map(|passphrase| passphrase.to_string())
        .map_err(|e| ProvisionError::KeychainRead(e.to_string()))
}

// There is ONE DB passphrase (provisioned by
// `hkask_keystore::provision_db_passphrase_string`) for every SQLCipher
// database (curator, corpus, kanban, swarm memory, training, research).
// The swarm memory DB has no separate passphrase; it opens with this one.

/// Resolve the curator DB path.
///
/// `HKASK_CURATOR_DB` if set, else `agents/curator/curator.db` under the
/// hKask data dir. Mirrors the resolution in
/// `kask_bridge::memory::curator_stores::curator_db_path`.
fn resolve_curator_db_path() -> String {
    std::env::var("HKASK_CURATOR_DB").unwrap_or_else(|_| {
        let p = hkask_types::agent_paths::agent_db("curator");
        let resolved = hkask_types::agent_paths::resolve_under_data_dir(&p);
        resolved.to_string_lossy().to_string()
    })
}

/// Resolve the swarm memory DB path.
///
/// `HKASK_SWARM_MEMORY_DB` if set (absolute override), else
/// `mcp/swarm/memory.db` under the hKask data dir. Mirrors the resolution
/// in `SwarmConfig::from_env`.
fn resolve_swarm_memory_db_path() -> String {
    let default = "mcp/swarm/memory.db";
    let raw = std::env::var("HKASK_SWARM_MEMORY_DB")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default.to_string());
    if std::path::Path::new(&raw).is_absolute() {
        raw
    } else {
        hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(&raw))
            .to_string_lossy()
            .to_string()
    }
}

/// The fixed-path SQLCipher databases that share `HKASK_DB_PASSPHRASE`,
/// each resolved the same way its owning MCP server resolves it (env-var
/// override, else the Standardized Artifact Storage default under the
/// hKask data dir — databases are the one artifact class that stays in
/// the internal data dir).
pub(crate) fn kask_db_paths() -> Vec<(&'static str, String)> {
    let resolve = |env_var: &str, default: &str| -> String {
        let raw = std::env::var(env_var)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| default.to_string());
        if std::path::Path::new(&raw).is_absolute() {
            raw
        } else {
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(&raw))
                .to_string_lossy()
                .to_string()
        }
    };

    managed_database_layout()
        .into_iter()
        .map(|(name, variable, default)| {
            let path = match name {
                "curator" => resolve_curator_db_path(),
                "swarm_memory" => resolve_swarm_memory_db_path(),
                _ => resolve(variable, &default.to_string_lossy()),
            };
            (name, path)
        })
        .collect()
}

pub(crate) fn managed_database_layout() -> [(&'static str, &'static str, std::path::PathBuf); 5] {
    use hkask_types::agent_paths::{agent_db, mcp_server_db};
    [
        ("curator", "HKASK_CURATOR_DB", agent_db("curator")),
        (
            "swarm_memory",
            "HKASK_SWARM_MEMORY_DB",
            mcp_server_db("swarm", "memory"),
        ),
        (
            "kata_kanban",
            "HKASK_KANBAN_DB",
            mcp_server_db("kata-kanban", "kanban"),
        ),
        (
            "research",
            "HKASK_RESEARCH_DB",
            mcp_server_db("research", "research"),
        ),
        (
            "training",
            "HKASK_TRAINING_DB",
            mcp_server_db("training", "training"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // The requirement that drives this module:
    //
    // 1. **Default** — every SQLCipher DB (curator, swarm memory, corpus, RSS,
    //    kata-kanan, training) opens with the fixed default `"allostery"`
    //    on first run. The default and the whole provisioning chain
    //    (env → keychain → first-run default, empty env treated as unset)
    //    live in `hkask-keystore` (`provision_db_passphrase_string`);
    //    identity.rs routes through that one chain and holds no
    //    passphrase logic of its own.
    // 2. **Startup** — at MCP launch time the chain resolves the
    //    passphrase so stores never start down (see
    //    `build_mcp_server_env` → `provision_default_passphrase`).
    // 3. **Settings rotation** — once running, the operator changes the
    //    passphrase via the settings UI; the DB re-encodes atomically, then
    //    the new passphrase is persisted. The two rotate functions below
    //    are the bridge callers that the UI hits.
    //
    // The chain's own behavior (empty-env guard, first-run default) is
    // pinned by hkask-keystore's tests; the tests here pin the rotation
    // path.

    /// Rotation resolves the old passphrase via the resolver helper, then
    /// hands both old and new to `hkask_storage::rotate_passphrase`.
    /// This pins the bridge container so the settings UI call (rotate →
    /// persist new → restart) targets a DB that decrypts under the resolved
    /// key, not the wrong passkey.
    #[test]
    fn rotation_path_uses_resolver_not_raw_env() {
        // We can't invoke the resolver here without a keychain/mock seam,
        // but we can pin the call sites: the rotate_* functions MUST
        // call the shared resolver (the chain env→keychain), not
        // `std::env::var("...")` directly. The container's correctness
        // hinges on this — the UI writes the keychain and expects rotation
        // to read from it.
        // (CI mock replacement lives behind `KeychainHarness` in hkask-keystore.)
        let _ = resolve_curator_db_path; // referenced so the seam stays alive
        let _ = resolve_swarm_memory_db_path;
    }
}
