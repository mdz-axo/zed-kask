use hkask_types::NotFound;
use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum KeystoreError {
    #[error("Platform keychain error: {0}")]
    Platform(String),

    #[error("Secret not found: {0}")]
    NotFound(NotFound),
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
        }
    }
}
