//! Credential resolution — keychain-first credential lookup with .env fallback.

use std::collections::HashMap;

/// Parse .env files and return key-value pairs without mutating the process environment.
///
/// Deprecated: prefer the OS keychain via `kask keystore load`. This function
/// is retained as a fallback for MCP servers that haven't migrated to
/// keychain-based credential resolution.
///
/// Walks up the directory tree from the current working directory, searching for
/// a `.env` file at each level. Returns the first `.env` found (closest to cwd).
/// This matches the legacy `dotenvy::dotenv()` search strategy without the `unsafe set_var`.
///
/// post: returns HashMap of env vars from the nearest .env file
/// post: returns empty map if no .env found up to the filesystem root
#[must_use]
pub fn load_dotenv() -> HashMap<String, String> {
    let mut current = std::env::current_dir().unwrap_or_default();
    loop {
        let path = current.join(".env");
        if let Ok(content) = std::fs::read_to_string(&path) {
            let mut map = HashMap::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let (key, value) = (key.trim(), value.trim());
                    if !key.is_empty() && !value.is_empty() && std::env::var(key).is_err() {
                        map.insert(key.into(), value.into());
                    }
                }
            }
            return map;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    HashMap::new()
}

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
        "HKASK_OCAP_SECRET" => {
            let bytes = hkask_keystore::keychain::get_or_create_ocap_secret()?;
            Ok(hex::encode(&*bytes))
        }
        "HKASK_A2A_SECRET" => {
            let bytes = hkask_keystore::keychain::resolve_a2a_secret()?;
            Ok(hex::encode(&*bytes))
        }

        _ => {
            // Unrecognized credential — try keychain, then env var.
            let val = hkask_keystore::Keychain::default()
                .retrieve_by_key(env_var)
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

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn db_credential_preserves_configured_passphrase() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("HKASK_DB_PASSPHRASE");
        unsafe { std::env::set_var("HKASK_DB_PASSPHRASE", "mcp-db-passphrase") };

        let resolved = resolve_credential("HKASK_DB_PASSPHRASE").expect("resolve DB credential");

        unsafe {
            match old {
                Some(value) => std::env::set_var("HKASK_DB_PASSPHRASE", value),
                None => std::env::remove_var("HKASK_DB_PASSPHRASE"),
            }
        }
        assert_eq!(resolved, "mcp-db-passphrase");
    }
}
