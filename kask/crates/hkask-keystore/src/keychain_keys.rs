//! Canonical keychain key constants — single source of truth.
//!
//! All keychain keys used across hKask are defined here. Using bare string
//! literals for keychain keys in call sites is a P5 violation (duplicated
//! source of truth) and a risk vector — a typo in a keychain key silently
//! breaks authentication at runtime with no compiler feedback.
//!
//! Added 2026-06-21 after audit found 21 distinct keychain keys, all bare strings.

/// Keychain key for the database passphrase.
pub const KEY_DB_PASSPHRASE: &str = "hkask-db-passphrase";

/// Keychain key for the swarm memory SQLCipher passphrase.
///
/// Distinct from `KEY_DB_PASSPHRASE`: the swarm memory store is a separate
/// SQLCipher DB (`swarm_memory.db`) shared across all swarms and agents, so
/// it has its own key. Provisioned on first run alongside the DB passphrase
/// via `provision_swarm_memory_passphrase` (see `kask_bridge::identity`).
pub const KEY_SWARM_MEMORY_PASSPHRASE: &str = "hkask-swarm-memory-passphrase";

/// Keychain key for the capability probe (internal diagnostics).
pub const KEY_CAPABILITY_PROBE: &str = "__hkask_capability_probe__";
