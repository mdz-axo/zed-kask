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
//! 2. Ensure a DB passphrase exists in the keychain (auto-generate a random
//!    English word if none exists — the user can change it later)
//! 3. Return the resolved DB path and passphrase for `RealMemoryPort::new()`

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{App, Task};
use hkask_keystore::Keychain;
use hkask_keystore::keychain_keys::{KEY_DB_PASSPHRASE, KEY_SWARM_MEMORY_PASSPHRASE};
use hkask_types::{WebID, agent_paths::sanitize_name};

/// Error type for agent provisioning (directory creation + keychain access).
#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("{0}")]
    InvalidUsername(String),
    #[error("Failed to create agent directory: {0}")]
    DirectoryCreation(#[from] std::io::Error),
    #[error("Failed to store DB passphrase in keychain: {0}")]
    KeychainStore(String),
    #[error("Failed to resolve DB passphrase from keychain: {0}")]
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
/// 3. Resolve the DB passphrase from the keychain; if none exists, generate
///    a random English word (8+ letters) and store it. The user can change
///    it later via the keychain or `HKASK_DB_PASSPHRASE` env var.
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
    let data_dir = hkask_services_core::config::resolve_data_dir();
    let agent_root = data_dir.join(hkask_types::agent_paths::agent_dir(&agent_name));
    std::fs::create_dir_all(&agent_root).map_err(ProvisionError::DirectoryCreation)?;

    let db_path = agent_root.join("memory.db").to_string_lossy().to_string();

    // 2. Ensure a DB passphrase exists in the keychain.
    //    If the env var is set, use that (user override).
    //    If the keychain has one, use that (returning user).
    //    Otherwise, generate a random English word and store it.
    let passphrase = if let Ok(p) = std::env::var("HKASK_DB_PASSPHRASE") {
        if !p.trim().is_empty() {
            p
        } else {
            resolve_or_create_passphrase()?
        }
    } else {
        resolve_or_create_passphrase()?
    };

    tracing::info!(name = agent_name, db = db_path, webid = %webid.redacted_display(), "Agent provisioned");

    Ok(ProvisionedAgent {
        db_path,
        passphrase,
        webid,
    })
}

/// Mirror the provisioned DB passphrase from the hkask-keystore keychain
/// (`hkask-db-passphrase`, written by `provision_agent`) into zed's
/// `CredentialsProvider` under `kask://credentials/hkask_db_passphrase`.
///
/// `build_mcp_server_env` reads the passphrase via the primary
/// `ctx.credentials` tier of `resolve_db_passphrase`, which looks up this URL.
/// Without the mirror, first-run provisioning only writes to the keystore
/// keychain, so MCP servers reach the passphrase only via the fallback tier
/// (`resolve_credential` → `hkask-db-passphrase`) — and `nudge_mcp_servers`
/// never fires because `provision_agent` bypasses `write_credential`.
///
/// This bridges the two keychain backends at write time, reusing the
/// canonical write path so the ordering dependency (provision before server
/// launch) is explicit rather than implicit.
///
/// Failure modes (per the `.rules` trap on startup-failure signals):
/// - Passphrase read fails → `tracing::warn!` naming the error; MCP servers
///   fall back to the keystore tier of `resolve_db_passphrase`.
/// - CredentialsProvider write fails → `tracing::warn!` naming the URL and
///   error; same fallback applies.
///
/// The returned `Task` must be awaited (or detached) by the caller. It does
/// NOT fire `nudge_mcp_servers` — at the call site in the deferred post-login
/// task, the governed MCP servers have not launched yet, so they pick up the
/// mirrored credential at launch via `build_mcp_server_env`.
pub fn mirror_provisioned_db_passphrase(
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    // Read the provisioned passphrase from the keystore chain (env → keychain
    // `hkask-db-passphrase`). `provision_agent` has already run by the time
    // this is called, so the keychain entry exists on first run.
    let passphrase = match hkask_keystore::keychain::resolve_db_passphrase_string() {
        Ok(passphrase) => passphrase,
        Err(error) => {
            tracing::warn!(
                target: "hkask.identity",
                %error,
                "Could not read provisioned DB passphrase for mirror — \
                 MCP servers will rely on the fallback tier of resolve_db_passphrase"
            );
            return Task::ready(());
        }
    };

    let credentials_provider = credentials_provider.clone();
    cx.spawn(async move |cx| {
        // Write to zed's CredentialsProvider so `build_mcp_server_env` finds
        // the passphrase via the primary `ctx.credentials` tier.
        let url = format!(
            "{}/hkask_db_passphrase",
            crate::credentials::KASK_CREDENTIAL_NAMESPACE
        );
        match credentials_provider
            .write_credentials(&url, "kask", passphrase.as_bytes(), cx)
            .await
        {
            Ok(()) => tracing::info!(
                target: "hkask.identity",
                credential_url = %url,
                "Mirrored provisioned DB passphrase to CredentialsProvider \
                 so build_mcp_server_env picks it up via the primary ctx.credentials tier"
            ),
            Err(error) => tracing::warn!(
                target: "hkask.identity",
                %error,
                credential_url = %url,
                "Failed to mirror provisioned DB passphrase to CredentialsProvider — \
                 MCP servers will rely on the fallback tier of resolve_db_passphrase"
            ),
        }
    })
}

/// Resolve the DB passphrase from the keychain, or create one if none exists.
///
/// On first run, generates a random English word (8+ letters) and stores it
/// in the OS keychain under `KEY_DB_PASSPHRASE`. The user can change it later
/// by updating the keychain entry or setting `HKASK_DB_PASSPHRASE`.
fn resolve_or_create_passphrase() -> Result<String, ProvisionError> {
    // Try to read an existing passphrase from the keychain.
    match hkask_keystore::keychain::resolve_db_passphrase_string() {
        Ok(passphrase) => Ok(passphrase.to_string()),
        Err(hkask_keystore::keychain::KeychainError::NotFound(_)) => {
            // First run — generate a random English word and store it.
            let word = hkask_keystore::generate_random_passphrase();
            let keychain = Keychain::default();
            keychain
                .store_by_key(KEY_DB_PASSPHRASE, &word)
                .map_err(|e| ProvisionError::KeychainStore(e.to_string()))?;
            tracing::info!(
                "Generated DB passphrase (random English word) and stored in keychain. \
                 The user can change it via the keychain or HKASK_DB_PASSPHRASE env var."
            );
            Ok(word)
        }
        Err(e) => Err(ProvisionError::KeychainRead(e.to_string())),
    }
}

/// Provision the swarm memory SQLCipher passphrase.
///
/// Mirrors the resolve-or-create half of [`provision_agent`] but for the
/// swarm memory store (`swarm_memory.db`), which is a separate SQLCipher DB
/// shared across all swarms and agents. Without this, the swarm server falls
/// back to the compiled-in pre-release default `"allostery"`
/// (`SwarmConfig::default().memory_passphrase`) — a constant that ships in
/// the source tree, which the `mcp_servers.rs` RR-0061 comment explicitly
/// flags as the bug the allowlist was supposed to fix. The allowlist fix let
/// the operator override it, but nothing generated an override on first
/// run, so `build_mcp_server_env` warned on every launch.
///
/// Resolution order:
/// 1. `HKASK_SWARM_MEMORY_PASSPHRASE` env var (user override).
/// 2. Existing keychain entry `hkask-swarm-memory-passphrase` (returning user).
/// 3. Generate a random English word and store it (first run).
///
/// Returns the resolved passphrase so the caller can mirror it into zed's
/// `CredentialsProvider` (see [`mirror_provisioned_swarm_memory_passphrase`]).
///
/// # Errors
///
/// Returns `ProvisionError::KeychainStore` if the generated passphrase cannot
/// be stored, or `ProvisionError::KeychainRead` if the keychain read fails
/// for a reason other than "not found."
pub fn provision_swarm_memory_passphrase() -> Result<String, ProvisionError> {
    // 1. Env var override.
    if let Ok(p) = std::env::var("HKASK_SWARM_MEMORY_PASSPHRASE") {
        if !p.trim().is_empty() {
            return Ok(p);
        }
    }

    // 2. Existing keychain entry.
    match hkask_keystore::keychain::resolve_swarm_memory_passphrase_string() {
        Ok(passphrase) => Ok(passphrase.to_string()),
        Err(hkask_keystore::keychain::KeychainError::NotFound(_)) => {
            // 3. First run — generate a random English word and store it.
            let word = hkask_keystore::generate_random_passphrase();
            let keychain = Keychain::default();
            keychain
                .store_by_key(KEY_SWARM_MEMORY_PASSPHRASE, &word)
                .map_err(|e| ProvisionError::KeychainStore(e.to_string()))?;
            tracing::info!(
                "Generated swarm memory passphrase (random English word) and stored \
                 in keychain. The user can change it via the keychain or \
                 HKASK_SWARM_MEMORY_PASSPHRASE env var."
            );
            Ok(word)
        }
        Err(e) => Err(ProvisionError::KeychainRead(e.to_string())),
    }
}

/// Mirror the provisioned swarm memory passphrase from the hkask-keystore
/// keychain (`hkask-swarm-memory-passphrase`, written by
/// [`provision_swarm_memory_passphrase`]) into zed's `CredentialsProvider`
/// under `kask://credentials/hkask_swarm_memory_passphrase`.
///
/// Mirrors [`mirror_provisioned_db_passphrase`] for the swarm memory store.
/// `build_mcp_server_env` reads the passphrase via the primary
/// `ctx.credentials` tier, which looks up this URL. Without the mirror, the
/// swarm server reaches the passphrase only via the fallback tier
/// (`resolve_credential` → `hkask-swarm-memory-passphrase`), and
/// `nudge_mcp_servers` never fires because the provisioning bypasses
/// `write_credential`.
///
/// Must be `.await`ed in the deferred post-login task before governed MCP
/// server launch, alongside `mirror_provisioned_db_passphrase`, so the
/// swarm server picks up the mirrored credential at launch via
/// `build_mcp_server_env`.
///
/// Failure modes (per the `.rules` trap on startup-failure signals):
/// - Passphrase read fails → `tracing::warn!` naming the error; the swarm
///   server falls back to the keystore tier of `resolve_credential`.
/// - CredentialsProvider write fails → `tracing::warn!` naming the URL and
///   error; same fallback applies.
pub fn mirror_provisioned_swarm_memory_passphrase(
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let passphrase = match hkask_keystore::keychain::resolve_swarm_memory_passphrase_string() {
        Ok(passphrase) => passphrase,
        Err(error) => {
            tracing::warn!(
                target: "hkask.identity",
                %error,
                "Could not read provisioned swarm memory passphrase for mirror — \
                 the swarm server will rely on the fallback tier of resolve_credential \
                 or the compiled-in default 'allostery'"
            );
            return Task::ready(());
        }
    };

    let credentials_provider = credentials_provider.clone();
    cx.spawn(async move |cx| {
        let url = format!(
            "{}/hkask_swarm_memory_passphrase",
            crate::credentials::KASK_CREDENTIAL_NAMESPACE
        );
        match credentials_provider
            .write_credentials(&url, "kask", passphrase.as_bytes(), cx)
            .await
        {
            Ok(()) => tracing::info!(
                target: "hkask.identity",
                credential_url = %url,
                "Mirrored provisioned swarm memory passphrase to CredentialsProvider \
                 so build_mcp_server_env picks it up via the primary ctx.credentials tier"
            ),
            Err(error) => tracing::warn!(
                target: "hkask.identity",
                %error,
                credential_url = %url,
                "Failed to mirror provisioned swarm memory passphrase to \
                 CredentialsProvider — the swarm server will rely on the fallback \
                 tier of resolve_credential or the compiled-in default 'allostery'"
            ),
        }
    })
}
