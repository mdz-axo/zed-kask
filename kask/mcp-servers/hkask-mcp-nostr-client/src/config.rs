//! Operator configuration, read once at startup.
//!
//! Public data only: the identity's `npub`, the seed relays, and where the
//! local relay server announces its listener. The agent cannot change any of
//! them; no tool writes them. A value that does not parse is never guessed
//! at: it is reported as invalid by `nostr_account`.

use std::path::PathBuf;

use hkask_nostr_core::{IdentifierError, decode_npub, encode_npub};

/// Environment variable carrying the identity's `npub`.
pub const NPUB_ENV: &str = "HKASK_NOSTR_CLIENT_NPUB";
/// Environment variable carrying the seed relays, comma-separated.
pub const SEED_RELAYS_ENV: &str = "HKASK_NOSTR_CLIENT_SEED_RELAYS";

/// The Nostr identity this server reads for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// No `npub` is set.
    NotConfigured,
    /// A valid `npub`, in canonical text form, with its 32-byte key.
    Configured { npub: String, public_key: [u8; 32] },
    /// A value is set and is not an `npub`. The reason never repeats the
    /// value: it may be a secret key pasted into the wrong field.
    Invalid { reason: String },
}

/// What the server runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub identity: Identity,
    /// Seed relays as the operator wrote them, trimmed, first-seen order.
    pub seed_relays: Vec<String>,
    /// The file the local relay server announces its listener in. `None`:
    /// this server was not told where to look, and reads no local relay.
    pub local_relay_announcement: Option<PathBuf>,
}

/// Why the configured value is not an `npub`, in words that repeat none of
/// it. A prefix is named only when it is one of the known Nostr prefixes:
/// that tells the operator which kind of value was pasted. Any other prefix
/// is text the operator typed, so it is not repeated.
fn invalid_reason(error: &IdentifierError) -> String {
    const KNOWN_PREFIXES: [&str; 5] = ["note", "nprofile", "nevent", "naddr", "nrelay"];
    match error {
        IdentifierError::NotBech32 => "the configured value is not a bech32 npub".to_string(),
        IdentifierError::WrongPrefix { found, .. } if found == "nsec" || found == "ncryptsec" => {
            "the configured value is a secret key, not a public npub; remove it from \
             kask.nostr.npub in settings"
                .to_string()
        }
        IdentifierError::WrongPrefix { found, .. } if KNOWN_PREFIXES.contains(&found.as_str()) => {
            format!("the configured value is a `{found}` identifier; an npub is required")
        }
        IdentifierError::WrongPrefix { .. } => {
            "the configured value is bech32 but is not an npub".to_string()
        }
        IdentifierError::WrongLength { found } => {
            format!("the configured npub holds {found} bytes; a public key is 32")
        }
    }
}

impl Config {
    /// Parse the two settings. Pure, so every rule is tested without
    /// touching the process environment.
    pub fn from_values(npub: Option<&str>, seed_relays: Option<&str>) -> Self {
        let identity = match npub.map(str::trim).filter(|raw| !raw.is_empty()) {
            None => Identity::NotConfigured,
            Some(raw) => match decode_npub(raw) {
                // Re-encoded, so the reported form is canonical lower case
                // whichever case the operator typed.
                Ok(public_key) => Identity::Configured {
                    npub: encode_npub(&public_key),
                    public_key,
                },
                Err(error) => Identity::Invalid {
                    reason: invalid_reason(&error),
                },
            },
        };

        let mut relays: Vec<String> = Vec::new();
        let entries = seed_relays
            .unwrap_or_default()
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|entry| !entry.is_empty());
        for entry in entries {
            if !relays.iter().any(|seen| seen == entry) {
                relays.push(entry.to_string());
            }
        }

        Config {
            identity,
            seed_relays: relays,
            local_relay_announcement: None,
        }
    }

    /// Look for the local relay's announcement at `path`.
    #[must_use]
    pub fn with_local_relay_announcement(mut self, path: PathBuf) -> Self {
        self.local_relay_announcement = Some(path);
        self
    }

    /// Read the settings the editor passes to this process. The local
    /// relay's announcement is looked for under the data directory, where
    /// the relay server writes it.
    pub fn from_env() -> Self {
        let npub = std::env::var("HKASK_NOSTR_CLIENT_NPUB").ok();
        let seed_relays = std::env::var("HKASK_NOSTR_CLIENT_SEED_RELAYS").ok();
        Self::from_values(npub.as_deref(), seed_relays.as_deref()).with_local_relay_announcement(
            hkask_types::agent_paths::resolve_under_data_dir(
                &hkask_types::nostr_wire::local_relay_announcement_path(),
            ),
        )
    }
}
