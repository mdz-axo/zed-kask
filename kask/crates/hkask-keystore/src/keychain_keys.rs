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

/// Keychain key for the capability probe (internal diagnostics).
pub const KEY_CAPABILITY_PROBE: &str = "__hkask_capability_probe__";
