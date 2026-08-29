use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use credentials_provider::CredentialsProvider;
use futures::FutureExt as _;
use gpui::{App, AppContext, AsyncApp, Global};

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
    // zed-kask: always use the OS keychain via the keystore's `Keychain`,
    // which uses a dedicated async-std thread (`async_std::task::block_on`)
    // to drive oo7's zbus I/O. The platform's
    // `background_executor().spawn(oo7...)` path does not reliably drive
    // the async-std reactor that oo7 requires — writes silently fail and
    // keys disappear on restart. The keystore's dedicated-thread path is
    // the same one that successfully reads `hkask_db_passphrase` at startup.
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
/// the keystore's dedicated async-std thread. This unifies all credential
/// access on one working path and eliminates the silent-write-failure bug
/// where the platform's `background_executor().spawn(oo7...)` couldn't
/// drive oo7's async-std reactor.
struct KeychainCredentialsProvider;

impl CredentialsProvider for KeychainCredentialsProvider {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<Option<(String, Vec<u8>)>>> + 'a>> {
        let url = url.to_string();
        async move {
            let secret = cx
                .background_spawn(async move {
                    match hkask_keystore::Keychain.retrieve_by_url(&url) {
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
                    .filter(|s| !s.is_empty())
                })
                .await;
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
        cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let url = url.to_string();
        let username = username.to_string();
        let password = password.to_vec();
        async move {
            let secret = String::from_utf8_lossy(&password).into_owned();
            cx.background_spawn(async move {
                hkask_keystore::Keychain
                    .store_by_url(&url, &username, &secret)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .await?;
            Ok(())
        }
        .boxed_local()
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let url = url.to_string();
        async move {
            cx.background_spawn(async move {
                hkask_keystore::Keychain
                    .delete_by_url(&url)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .await?;
            Ok(())
        }
        .boxed_local()
    }
}
