//! Credential namespace constants and credential-verification utilities.
//!
//! The `kask://credentials` prefix is the single source of truth for
//! kask-namespaced keychain entries. Every credential URL written by kask
//! (data-service API keys, the curator SMTP password, the mirrored DB
//! passphrase, etc.) is built by formatting this constant with a
//! per-credential suffix. Keeping the constant here — rather than re-declaring
//! it in each consumer — prevents the silent drift that occurred when the
//! former private duplicate in `inference_providers` diverged from the
//! crate-root value.

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Used by the settings UI to read/write API keys via zed's CredentialsProvider.
pub const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// Send a test email to verify MXroute credentials are working.
///
/// Spawns the send on the kask tokio runtime (reqwest needs tokio for I/O).
/// Returns immediately — the caller (settings UI) can't observe the result
/// synchronously, but the `reg.email.sent` / `reg.alert` tracing spans surface
/// success/failure in the logs.
///
/// No-op when email is not configured (`send_test_email` returns
/// `Err(NotConfigured)` which is logged at `warn` level by the spawned task).
pub fn spawn_test_email(recipient: String, cx: &gpui::App) {
    gpui_tokio::Tokio::spawn(cx, async move {
        match hkask_email::send_test_email(&recipient).await {
            Ok(()) => tracing::info!(
                target: "reg.email.sent",
                recipient = %recipient,
                "Test email sent successfully"
            ),
            Err(e) => tracing::warn!(
                target: "reg.email.sent",
                error = %e,
                recipient = %recipient,
                "Test email failed"
            ),
        }
    })
    .detach();
}

/// One-time startup purge of the removed ABW API key (ABW-removal plan
/// Decision A; recorded in DIVERGENCE.md D5).
///
/// The ABW cloud swarm integration was removed; its keychain entry
/// `kask://credentials/hkask_abw_api_key` is a dormant third-party secret
/// with no remaining read path or management surface — worse to keep than
/// to delete once, loudly. Safe to run on every startup:
/// `delete_by_url_async` on an absent entry is a no-op, so the steady state
/// logs at debug and the actual purge logs at info. A failed delete of a
/// live key warns naming manual remediation.
pub fn purge_removed_abw_api_key(cx: &gpui::App) {
    use gpui::AppContext as _;

    let url = format!("{KASK_CREDENTIAL_NAMESPACE}/hkask_abw_api_key");
    cx.background_spawn(async move {
        let keychain = hkask_keystore::keychain::Keychain;
        match keychain.retrieve_by_url_async(&url).await {
            Ok(_) => match keychain.delete_by_url_async(&url).await {
                Ok(()) => tracing::info!(
                    target: "reg.keystore",
                    "Purged the removed ABW API key at {url} — the ABW cloud swarm \
                     integration no longer exists; the orphaned third-party secret \
                     is deleted (D5, ABW-removal plan Decision A)"
                ),
                Err(e) => tracing::warn!(
                    target: "reg.keystore",
                    "Found the removed ABW API key at {url} but the purge delete \
                     failed: {e} — delete the keychain entry manually"
                ),
            },
            Err(hkask_keystore::KeychainError::NotFound(_)) => {
                tracing::debug!(
                    target: "reg.keystore",
                    "No ABW API key at {url} — keychain already clean (the ABW \
                     integration is removed)"
                );
            }
            Err(e) => tracing::warn!(
                target: "reg.keystore",
                "Could not check for the removed ABW API key at {url}: {e} — the \
                 key may still exist; delete it manually if present"
            ),
        }
    })
    .detach();
}

/// One-time startup purge of the removed Reduct.cloud API key
/// (Reduct-removal plan, Decision A mirrored from the ABW precedent;
/// recorded in DIVERGENCE.md D5/D9).
///
/// The Reduct.cloud integration was removed from the media MCP server
/// (2026-10-22); its keychain entry `kask://credentials/reduct_api_key` is a
/// dormant third-party secret with no remaining read path or management
/// surface — worse to keep than to delete once, loudly. Safe to run on every
/// startup: `delete_by_url_async` on an absent entry is a no-op, so the steady
/// state logs at debug and the actual purge logs at info. A failed delete of a
/// live key warns naming manual remediation.
pub fn purge_removed_reduct_api_key(cx: &gpui::App) {
    use gpui::AppContext as _;

    let url = format!("{KASK_CREDENTIAL_NAMESPACE}/reduct_api_key");
    cx.background_spawn(async move {
        let keychain = hkask_keystore::keychain::Keychain;
        match keychain.retrieve_by_url_async(&url).await {
            Ok(_) => match keychain.delete_by_url_async(&url).await {
                Ok(()) => tracing::info!(
                    target: "reg.keystore",
                    "Purged the removed Reduct.cloud API key at {url} — the media \
                     server's Reduct cloud connection no longer exists; the orphaned \
                     third-party secret is deleted (D5/D9, Reduct-removal plan)"
                ),
                Err(e) => tracing::warn!(
                    target: "reg.keystore",
                    "Found the removed Reduct.cloud API key at {url} but the purge \
                     delete failed: {e} — delete the keychain entry manually"
                ),
            },
            Err(hkask_keystore::KeychainError::NotFound(_)) => {
                tracing::debug!(
                    target: "reg.keystore",
                    "No Reduct.cloud API key at {url} — keychain already clean (the \
                     Reduct.cloud integration is removed)"
                );
            }
            Err(e) => tracing::warn!(
                target: "reg.keystore",
                "Could not check for the removed Reduct.cloud API key at {url}: {e} — \
                 the key may still exist; delete it manually if present"
            ),
        }
    })
    .detach();
}
