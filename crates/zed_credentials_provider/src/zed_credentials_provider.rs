use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use credentials_provider::CredentialsProvider;
use futures::FutureExt as _;
use gpui::{App, AsyncApp, Global};

pub struct ZedCredentialsProvider(pub Arc<dyn CredentialsProvider>);

impl Global for ZedCredentialsProvider {}

/// Returns the global [`CredentialsProvider`].
pub fn init_global(cx: &mut App) {
    // The `CredentialsProvider` trait has `Send + Sync` bounds on it, so it
    // seems like this is a false positive from Clippy.
    #[allow(clippy::arc_with_non_send_sync)]
    let provider = new(cx);
    cx.set_global(ZedCredentialsProvider(provider));
}

pub fn global(cx: &App) -> Arc<dyn CredentialsProvider> {
    cx.try_global::<ZedCredentialsProvider>()
        .map(|provider| provider.0.clone())
        .unwrap_or_else(|| new(cx))
}

fn new(_cx: &App) -> Arc<dyn CredentialsProvider> {
    // zed-kask: always use the OS keychain via the keystore's `Keychain`.
    // Its awaited URL operations run oo7 on async-std's executor; GPUI's
    // background executor does not drive oo7's async-std reactor reliably.
    // Synchronous passphrase bootstrap still uses the same keystore schema.
    //
    // Upstream Zed uses `DevelopmentCredentialsProvider` (JSON file) in dev
    // mode to avoid keychain prompts, but zed-kask is always built from
    // source (release channel is always "dev") and its API keys live in the
    // OS keychain. The JSON file provider was deleted — the keychain is the
    // single source of truth.
    Arc::new(KeychainCredentialsProvider)
}

/// A credentials provider that stores ALL credentials in the OS keychain
/// via `hkask_keystore::Keychain`.
///
/// Every URL — `kask://credentials/*` and `https://*` alike — routes through
/// the keystore's async-std executor. GPUI awaits completion without
/// blocking a background worker or using the platform's oo7 executor.
struct KeychainCredentialsProvider;

impl CredentialsProvider for KeychainCredentialsProvider {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<Option<(String, Vec<u8>)>>> + 'a>> {
        let url = url.to_string();
        async move {
            let secret = match hkask_keystore::Keychain.retrieve_by_url_async(&url).await {
                Ok(secret) => Some(secret),
                Err(hkask_keystore::KeychainError::NotFound(_)) => None,
                Err(hkask_keystore::KeychainError::Platform(error)) => {
                    log::warn!(
                        "Keychain platform error reading credential at {}: {} — \
                         the key may exist but the keychain is inaccessible \
                         (D-Bus, keyring locked, etc.)",
                        url,
                        error
                    );
                    None
                }
            }
            .filter(|s| !s.is_empty());
            if let Some(secret) = secret {
                return Ok(Some(("kask".to_string(), secret.as_bytes().to_vec())));
            }
            Ok(None)
        }
        .boxed_local()
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let url = url.to_string();
        let username = username.to_string();
        let password = password.to_vec();
        async move {
            let secret = String::from_utf8_lossy(&password).into_owned();
            hkask_keystore::Keychain
                .store_by_url_async(&url, &username, &secret)
                .await?;
            Ok(())
        }
        .boxed_local()
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let url = url.to_string();
        async move {
            hkask_keystore::Keychain.delete_by_url_async(&url).await?;
            Ok(())
        }
        .boxed_local()
    }
}

#[cfg(test)]
mod tests {
    /// Pin (zed-kask): the credentials provider is ALWAYS the OS-keychain
    /// one. Upstream Zed uses a file-backed development provider in dev
    /// mode; zed-kask's release channel is always "dev", so an upstream
    /// revert would silently route every `kask://credentials/*` write to a
    /// JSON file while `build_mcp_server_env` keeps reading the OS keychain
    /// — every API key invisible to MCP servers while the UI reports
    /// success. Source-structure pin (mermaid-precedent style): asserts
    /// `new` constructs the keychain provider and that the file-backed
    /// provider appears only in comments.
    #[test]
    fn new_always_constructs_the_keychain_provider() {
        let source = include_str!("zed_credentials_provider.rs");
        let new_body = source
            .split("fn new(")
            .nth(1)
            .expect("fn new must exist in the source");
        assert!(
            new_body.contains("Arc::new(KeychainCredentialsProvider)"),
            "new() must construct the OS-keychain provider"
        );
    }

    #[test]
    fn all_credential_operations_await_the_keystore_without_blocking_gpui_workers() {
        let source = include_str!("zed_credentials_provider.rs");
        let provider = source
            .split("impl CredentialsProvider for KeychainCredentialsProvider {")
            .nth(1)
            .expect("keychain provider implementation exists")
            .split("#[cfg(test)]")
            .next()
            .expect("test boundary exists");
        for operation in [
            "retrieve_by_url_async(",
            "store_by_url_async(",
            "delete_by_url_async(",
        ] {
            assert!(provider.contains(operation), "{operation} must be awaited");
        }
        assert!(!provider.contains("background_spawn("));
    }

    #[test]
    fn no_file_backed_provider_construction() {
        let source = include_str!("zed_credentials_provider.rs");
        // The needle is built by concat! so this test's own source does
        // not match itself on non-comment lines.
        let needle = concat!("DevelopmentCredentials", "Provider");
        for (index, line) in source.lines().enumerate() {
            if line.contains(needle) {
                assert!(
                    line.trim_start().starts_with("//"),
                    "the file-backed provider may only appear in comments, \
                     found a non-comment reference at line {}",
                    index + 1,
                );
            }
        }
    }
}
