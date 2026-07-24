//! `SecretsPort` adapter over zed's `CredentialsProvider`.
//!
//! hKask MCP servers and the trimmed keystore read API keys and sovereignty secrets
//! through `SecretsPort`. This adapter stores them in the OS keychain via zed's
//! `CredentialsProvider`, namespaced under `kask://credentials/<key>` so they don't
//! collide with zed's own provider keys (which use their provider URLs).

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use futures_util::FutureExt;
use gpui::AsyncApp;
use hkask_types::SecretsPort;

/// The URL prefix for kask-namespaced credentials in the keychain.
pub const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// `SecretsPort` implementation over zed's `CredentialsProvider`.
///
/// Keys are namespaced: a `SecretsPort::read("fmp")` call reads the credential
/// stored at URL `kask://credentials/fmp`. This keeps kask secrets isolated from
/// zed's own provider credentials (which use their provider URLs as keys).
pub struct CredentialsSecretsPort {
    provider: Arc<dyn CredentialsProvider>,
    cx: AsyncApp,
}

impl CredentialsSecretsPort {
    pub fn new(provider: Arc<dyn CredentialsProvider>, cx: AsyncApp) -> Self {
        Self { provider, cx }
    }

    fn url(&self, key: &str) -> String {
        format!("{KASK_CREDENTIAL_NAMESPACE}/{key}")
    }
}

impl SecretsPort for CredentialsSecretsPort {
    fn read<'a>(
        &'a self,
        key: &'a str,
    ) -> hkask_types::ports::SecretsFuture<'a, Result<Option<String>, hkask_types::SecretsError>>
    {
        let url = self.url(key);
        async move {
            let result = self
                .provider
                .read_credentials(&url, &self.cx)
                .await
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))?;
            Ok(result.map(|(_username, password_bytes)| {
                String::from_utf8_lossy(&password_bytes).to_string()
            }))
        }
        .boxed()
    }

    fn write<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> hkask_types::ports::SecretsFuture<'a, Result<(), hkask_types::SecretsError>> {
        let url = self.url(key);
        async move {
            self.provider
                .write_credentials(&url, "kask", value.as_bytes(), &self.cx)
                .await
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))
        }
        .boxed()
    }

    fn delete<'a>(
        &'a self,
        key: &'a str,
    ) -> hkask_types::ports::SecretsFuture<'a, Result<(), hkask_types::SecretsError>> {
        let url = self.url(key);
        async move {
            self.provider
                .delete_credentials(&url, &self.cx)
                .await
                .map_err(|e| hkask_types::SecretsError::Store(e.to_string()))
        }
        .boxed()
    }
}
