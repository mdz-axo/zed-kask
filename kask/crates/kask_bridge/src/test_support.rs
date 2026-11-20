//! Shared cfg(test) fixtures for kask_bridge's test modules (R5 batch-2).
//! One canonical copy — test modules import from here instead of
//! re-declaring.

use hkask_keystore::KeychainError;

/// A mock for the typed keychain-read seam (`typed_keychain_read` in
/// mcp_servers.rs) — canned outcomes per URL, classified over the SAME
/// `KeychainError` taxonomy production reads return. Replaces the
/// provider-mock failure injection the A-4 pin used before MCP-credential
/// reads dropped below the provider trait: transient states (Timeout,
/// Platform) are injected as typed keystore errors, exactly as a wedged
/// or failing keychain would produce them.
///
/// An installed mock serves EVERY keychain read in the process: a URL
/// with no canned outcome reads as typed `NotFound` (a keychain that
/// answered and holds no entry), never the real keychain. `install()`
/// returns a guard that uninstalls the mock on drop — a leaked mock would
/// poison sibling tests with canned reads, so `install()` refuses to
/// overwrite an already-installed mock.
pub(crate) struct MockKeychain {
    outcomes: std::collections::HashMap<String, MockRead>,
}

/// The cloneable recipe behind a canned mock read — reconstructed into the
/// typed `KeychainError` on every read, so repeated reads of one URL serve
/// the same outcome (the composed-path tests call `build_mcp_server_env`
/// several times per test).
enum MockRead {
    Secret(Vec<u8>),
    Platform(String),
    Timeout { timeout_secs: u64 },
}

impl MockKeychain {
    pub(crate) fn new() -> Self {
        Self {
            outcomes: std::collections::HashMap::new(),
        }
    }

    /// Seed from a plain secrets map — the seeding shape the composed-path
    /// tests used with the provider mock.
    pub(crate) fn from_secrets(secrets: std::collections::HashMap<String, Vec<u8>>) -> Self {
        Self {
            outcomes: secrets
                .into_iter()
                .map(|(url, bytes)| (url, MockRead::Secret(bytes)))
                .collect(),
        }
    }

    pub(crate) fn with_secret(mut self, url: &str, secret: impl AsRef<[u8]>) -> Self {
        self.outcomes
            .insert(url.to_string(), MockRead::Secret(secret.as_ref().to_vec()));
        self
    }

    /// A platform-layer keychain failure (locked keyring, D-Bus) — the
    /// typed `KeychainError::Platform` a failing keychain returns.
    pub(crate) fn with_platform_error(mut self, url: &str, message: &str) -> Self {
        self.outcomes
            .insert(url.to_string(), MockRead::Platform(message.to_string()));
        self
    }

    /// A keychain read that does not answer within its deadline — the
    /// typed `KeychainError::Timeout` a wedged secret portal produces.
    pub(crate) fn with_timeout(mut self, url: &str, timeout_secs: u64) -> Self {
        self.outcomes
            .insert(url.to_string(), MockRead::Timeout { timeout_secs });
        self
    }

    pub(crate) fn install(self) -> KeychainMockGuard {
        let mut installed = INJECTED_KEYCHAIN_READS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert!(
            installed.is_none(),
            "a MockKeychain is already installed — the previous test leaked its guard"
        );
        *installed = Some(self.outcomes);
        KeychainMockGuard
    }
}

/// RAII uninstall for an installed [`MockKeychain`] — dropped at test end
/// (or on panic), restoring the real-keystore path for sibling tests.
pub(crate) struct KeychainMockGuard;

impl Drop for KeychainMockGuard {
    fn drop(&mut self) {
        *INJECTED_KEYCHAIN_READS
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// The installed-mock store behind the typed keychain-read seam. `None` —
/// the production state, and every test that has not installed a mock —
/// reads the real keystore.
static INJECTED_KEYCHAIN_READS: std::sync::Mutex<
    Option<std::collections::HashMap<String, MockRead>>,
> = std::sync::Mutex::new(None);

/// Serve one typed read from the installed mock, if any. `None` — no mock
/// installed — falls through to the real keystore. A mock with no entry for
/// `url` reads as typed `NotFound`, like a keychain that answered and
/// holds no entry.
pub(crate) fn injected_keychain_read(url: &str) -> Option<Result<Vec<u8>, KeychainError>> {
    let installed = INJECTED_KEYCHAIN_READS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let map = installed.as_ref()?;
    Some(match map.get(url) {
        Some(MockRead::Secret(bytes)) => Ok(bytes.clone()),
        Some(MockRead::Platform(message)) => Err(KeychainError::Platform(message.clone())),
        Some(MockRead::Timeout { timeout_secs }) => Err(KeychainError::Timeout {
            key: url.to_string(),
            timeout_secs: *timeout_secs,
        }),
        None => Err(not_found(url)),
    })
}

fn not_found(url: &str) -> KeychainError {
    KeychainError::NotFound(hkask_types::NotFound {
        entity_type: "credential".to_string(),
        id: url.to_string(),
    })
}
