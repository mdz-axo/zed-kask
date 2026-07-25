//! Identity resolution — derives the hKask userpod name from the Zed login.
//!
//! The userpod name is the sanitized `User::username` from the Zed account
//! (the GitHub-style login, e.g. `mdz-axo`). This collapses the former
//! interactive onboarding step into a lookup: the userpod identity
//! is derived from the Zed session, not entered separately.
//!
//! Convention:
//! - `User::username` (SharedString) → `sanitize_name()` → userpod name
//! - `WebID::for_userpod_name(&sanitized)` → deterministic WebID
//! - `agent_paths::userpod_dir(&sanitized)` → filesystem paths
//!
//! When the user is not yet logged in, `resolve_userpod_name` returns `None`
//! and the caller defers userpod-dependent wiring until the session arrives.
//!
//! ## Provisioning
//!
//! `provision_userpod` handles first-run setup as a set of lookups and
//! directory creation — no interactive onboarding:
//! 1. Create the userpod directory structure (`ensure_userpod_dirs`)
//! 2. Ensure a DB passphrase exists in the keychain (auto-generate a random
//!    English word if none exists — the user can change it later)
//! 3. Return the resolved DB path and passphrase for `RealMemoryPort::new()`

use hkask_keystore::Keychain;
use hkask_types::keychain_keys::KEY_DB_PASSPHRASE;
use hkask_types::{WebID, agent_paths::sanitize_name};

/// Derive the userpod name from a Zed `User::username`.
///
/// The username is the stable, lowercase, GitHub-style handle from the Zed
/// account. We sanitize it for filesystem use (replaces `/ \ : * ? " < > | ( )`
/// and spaces with dashes) so it can be used directly as a directory name and
/// a `WebID` persona.
///
/// Returns `None` if the username is empty after sanitization.
pub fn userpod_name_from_username(username: &str) -> Option<String> {
    let sanitized = sanitize_name(username);
    if sanitized.is_empty() || sanitized == "unnamed" {
        None
    } else {
        Some(sanitized)
    }
}

/// Derive the `WebID` for a Zed username.
///
/// Deterministic: the same username always produces the same WebID
/// (via `WebID::for_userpod_name` in the `"hkask"` namespace).
pub fn webid_from_username(username: &str) -> Option<WebID> {
    userpod_name_from_username(username).map(|name| WebID::for_userpod_name(&name))
}

/// A curated list of common English words, each 8+ letters.
/// Used to generate a human-readable DB passphrase on first run.
/// The user can change it later via the keychain or settings.
const PASSPHRASE_WORDS: &[&str] = &[
    "absolute",
    "adventure",
    "amplitude",
    "architect",
    "asteroid",
    "atmosphere",
    "backbone",
    "blueprint",
    "boundary",
    "butterfly",
    "calendar",
    "catalyst",
    "cathedral",
    "champion",
    "chandelier",
    "cheesecake",
    "cinnamon",
    "composer",
    "computer",
    "constellation",
    "corridor",
    "courtyard",
    "daffodil",
    "daybreak",
    "dinosaur",
    "directory",
    "driftwood",
    "elephant",
    "epiphany",
    "eternity",
    "festival",
    "flamingo",
    "fountain",
    "gossamer",
    "helicopter",
    "hospital",
    "hummingbird",
    "identity",
    "infinity",
    "inspiration",
    "kaleidoscope",
    "lavender",
    "lemonade",
    "lighthouse",
    "limousine",
    "magnolia",
    "manuscript",
    "marigold",
    "meridian",
    "midnight",
    "mountain",
    "mushroom",
    "mystique",
    "nightingale",
    "novelette",
    "oblivion",
    "opulence",
    "orchestra",
    "palindrome",
    "panorama",
    "paradise",
    "parchment",
    "passenger",
    "pavilion",
    "peppermint",
    "pinnacle",
    "platinum",
    "pomegranate",
    "porcelain",
    "primrose",
    "propeller",
    "quicksilver",
    "radiance",
    "reflection",
    "refrigerator",
    "renaissance",
    "resonance",
    "rhinoceros",
    "riverbed",
    "rosewood",
    "sapphire",
    "satellite",
    "scintilla",
    "seashell",
    "serenity",
    "silhouette",
    "snowfall",
    "solstice",
    "spectrum",
    "stardust",
    "starlight",
    "sunflower",
    "tapestry",
    "tortoise",
    "tradition",
    "tranquility",
    "turbulence",
    "umbrella",
    "undertow",
    "universe",
    "upholstery",
    "vanguard",
    "waterfall",
    "whimsical",
    "wildflower",
    "windmill",
    "yesterday",
];

/// Pick a random word from the passphrase word list.
///
/// Uses `rand::thread_rng` for cryptographic randomness — the passphrase
/// protects an encrypted database, so we don't want a predictable seed.
fn random_passphrase_word() -> String {
    use rand::seq::IndexedRandom;
    let mut rng = rand::rng();
    PASSPHRASE_WORDS
        .choose(&mut rng)
        .expect("PASSPHRASE_WORDS is non-empty")
        .to_string()
}

/// The result of provisioning a userpod — everything needed to construct
/// a `RealMemoryPort` directly, without going through `from_env()`.
pub struct ProvisionedUserpod {
    /// Absolute path to the memory database file.
    pub db_path: String,
    /// SQLCipher passphrase (stored in the keychain).
    pub passphrase: String,
    /// The userpod's WebID, derived from the username.
    pub webid: WebID,
}

/// Provision a userpod for the given Zed username.
///
/// This is the "onboarding that disappeared" — a set of lookups and
/// directory creation, no interactive prompts:
///
/// 1. Derive the userpod name from the username (sanitize for filesystem).
/// 2. Create the userpod directory structure on disk (idempotent).
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
pub fn provision_userpod(username: &str) -> Result<ProvisionedUserpod, String> {
    let userpod_name = userpod_name_from_username(username).ok_or_else(|| {
        format!("Username '{username}' sanitized to empty — cannot provision userpod")
    })?;

    let webid = WebID::for_userpod_name(&userpod_name);

    // 1. Create the userpod directory structure (idempotent).
    //    Resolve against the hKask data directory so paths are absolute.
    let data_dir = hkask_services_core::config::resolve_data_dir();
    let userpod_root = data_dir.join(hkask_types::agent_paths::userpod_dir(&userpod_name));
    std::fs::create_dir_all(&userpod_root)
        .map_err(|e| format!("Failed to create userpod dir {userpod_root:?}: {e}"))?;
    for sub in hkask_types::agent_paths::USERPOD_SUBDIRS {
        std::fs::create_dir_all(userpod_root.join(sub))
            .map_err(|e| format!("Failed to create userpod subdir {sub}: {e}"))?;
    }

    let db_path = userpod_root.join("memory.db").to_string_lossy().to_string();

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

    tracing::info!("Userpod provisioned: name={userpod_name}, db={db_path}, webid={webid:?}");

    Ok(ProvisionedUserpod {
        db_path,
        passphrase,
        webid,
    })
}

/// Resolve the DB passphrase from the keychain, or create one if none exists.
///
/// On first run, generates a random English word (8+ letters) and stores it
/// in the OS keychain under `KEY_DB_PASSPHRASE`. The user can change it later
/// by updating the keychain entry or setting `HKASK_DB_PASSPHRASE`.
fn resolve_or_create_passphrase() -> Result<String, String> {
    // Try to read an existing passphrase from the keychain.
    match hkask_keystore::keychain::resolve_db_passphrase_string() {
        Ok(passphrase) => Ok(passphrase.to_string()),
        Err(hkask_keystore::keychain::KeychainError::NotFound(_)) => {
            // First run — generate a random English word and store it.
            let word = random_passphrase_word();
            let keychain = Keychain::default();
            keychain
                .store_by_key(KEY_DB_PASSPHRASE, &word)
                .map_err(|e| format!("Failed to store generated DB passphrase in keychain: {e}"))?;
            tracing::info!(
                "Generated DB passphrase (random English word) and stored in keychain. \
                 The user can change it via the keychain or HKASK_DB_PASSPHRASE env var."
            );
            Ok(word)
        }
        Err(e) => Err(format!(
            "Failed to resolve DB passphrase from keychain: {e}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_style_username_passes_through() {
        assert_eq!(
            userpod_name_from_username("mdz-axo").as_deref(),
            Some("mdz-axo")
        );
        assert_eq!(
            userpod_name_from_username("octocat").as_deref(),
            Some("octocat")
        );
    }

    #[test]
    fn spaces_become_dashes() {
        assert_eq!(
            userpod_name_from_username("Jacques Zuck").as_deref(),
            Some("Jacques-Zuck")
        );
    }

    #[test]
    fn path_traversal_rejected() {
        assert_eq!(userpod_name_from_username(".."), None);
        assert_eq!(userpod_name_from_username("."), None);
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(userpod_name_from_username(""), None);
        assert_eq!(userpod_name_from_username("   "), None);
    }

    #[test]
    fn webid_is_deterministic() {
        let w1 = webid_from_username("mdz-axo").unwrap();
        let w2 = webid_from_username("mdz-axo").unwrap();
        assert_eq!(w1, w2);
    }

    #[test]
    fn different_users_get_different_webids() {
        let w1 = webid_from_username("alice").unwrap();
        let w2 = webid_from_username("bob").unwrap();
        assert_ne!(w1, w2);
    }

    #[test]
    fn all_passphrase_words_are_8_plus_letters() {
        for word in PASSPHRASE_WORDS {
            assert!(
                word.len() >= 8,
                "Word '{word}' is only {} chars — must be 8+",
                word.len()
            );
        }
    }

    #[test]
    fn random_passphrase_word_returns_a_valid_word() {
        let word = random_passphrase_word();
        assert!(word.len() >= 8);
        assert!(PASSPHRASE_WORDS.contains(&word.as_str()));
    }

    #[test]
    fn random_passphrase_word_varies_across_calls() {
        // With 170+ words, the chance of 10 identical draws is negligible.
        let words: Vec<String> = (0..10).map(|_| random_passphrase_word()).collect();
        let unique: std::collections::HashSet<_> = words.iter().collect();
        assert!(unique.len() > 1, "random_passphrase_word should vary");
    }
}
