use hkask_types::NotFound;
use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum KeystoreError {
    #[error("Platform keychain error: {0}")]
    Platform(String),

    #[error("Secret not found: {0}")]
    NotFound(NotFound),

    /// The keychain read did not answer within its deadline — the secret
    /// portal may be wedged. Distinct from `Platform`: a wedged portal is
    /// a stall to outlive, not a break to report.
    #[error(
        "Keychain read for '{key}' did not answer within {timeout_secs}s — the secret portal may be wedged; set the credential via its env var to bypass"
    )]
    Timeout { key: String, timeout_secs: u64 },
}

impl From<NotFound> for KeystoreError {
    fn from(nf: NotFound) -> Self {
        KeystoreError::NotFound(nf)
    }
}

impl From<crate::keychain::KeychainError> for KeystoreError {
    fn from(err: crate::keychain::KeychainError) -> Self {
        match err {
            crate::keychain::KeychainError::Platform(msg) => KeystoreError::Platform(msg),
            crate::keychain::KeychainError::NotFound(nf) => KeystoreError::NotFound(nf),
            crate::keychain::KeychainError::Timeout { key, timeout_secs } => {
                KeystoreError::Timeout { key, timeout_secs }
            }
        }
    }
}
