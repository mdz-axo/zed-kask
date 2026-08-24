//! Credential namespace constants and credential-verification utilities.
//!
//! The `kask://credentials` prefix is the single source of truth for
//! kask-namespaced keychain entries. Every credential URL written by kask
//! (data-service API keys, the curator SMTP password, the swarm ABW key, the
//! mirrored DB passphrase, etc.) is built by formatting this constant with a
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
