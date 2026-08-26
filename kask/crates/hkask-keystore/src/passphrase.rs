//! Default DB passphrase for first-run provisioning.
//!
//! All SQLCipher databases (curator, corpus, kata-kanban, swarm memory,
//! research RSS, training) default to `"allostery"` on first run. The user
//! can change it later via the settings UI (which triggers atomic DB
//! re-encryption) or the `HKASK_DB_PASSPHRASE` / `HKASK_SWARM_MEMORY_PASSPHRASE`
//! env vars. There is no random generation — a fixed default eliminates the
//! passphrase/DB desync class of bugs where the keychain loses a generated
//! word and the DB becomes unrecoverable.

/// The default passphrase for all SQLCipher databases.
///
/// Chosen as a memorable, 9-letter English word that satisfies the >=8 char
/// minimum enforced by the storage layer. Not a security boundary — the
/// keychain is the security boundary. This default exists so first-run
/// provisioning always produces a DB the user can open, change, and recover.
pub const DEFAULT_PASSPHRASE: &str = "allostery";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_passphrase_is_at_least_eight_letters() {
        assert!(
            DEFAULT_PASSPHRASE.len() >= 8,
            "default passphrase must be >= 8 chars for SQLCipher"
        );
    }
}
