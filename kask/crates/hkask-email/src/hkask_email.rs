#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! Curator email — outbound via MXroute SMTP API.
//!
//! Outbound: SMTP API at smtpapi.mxroute.com (alerts, notifications, test).
//!
//! Interaction modes (`EmailMode`): Invite, Alert, Notification, Command.
//! Each mode closes a different cybernetic feedback loop.
//!
//! Credentials are read from environment variables (set by the composition
//! root from kask settings + keychain):
//! - `HKASK_MXROUTE_SERVER` — MXroute server hostname (e.g. "tuesday.mxrouting.net")
//! - `HKASK_SMTP_USERNAME` — full email address for auth
//! - `HKASK_SMTP_PASSWORD` — email account password (from keychain)
//! - `HKASK_CURATOR_EMAIL` — from address (default: `HKASK_SMTP_USERNAME`)
//! - `HKASK_ALERT_EMAIL` — alert recipient (default: `HKASK_SMTP_USERNAME`)

use std::sync::Arc;

use hkask_regulation::{AlertEmailSink, RuntimeAlert};
use serde::{Deserialize, Serialize};

// ── Errors ───────────────────────────────────────────────────────────────

/// Errors that can occur during email delivery.
#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("Email delivery is not configured: {0}")]
    NotConfigured(String),
    #[error("MXroute API request failed: {0}")]
    ApiRequest(String),
    #[error("MXroute API returned HTTP {0}")]
    ApiStatus(u16),
    #[error("MXroute API error: {0}")]
    ApiError(String),
}

/// Result type for email operations.
pub type EmailResult<T> = std::result::Result<T, EmailError>;

// ── Email mode ───────────────────────────────────────────────────────────

/// Interaction mode for curator email — tags each message with its cybernetic purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmailMode {
    Invite,
    Alert,
    Notification,
    Command,
}

impl std::fmt::Display for EmailMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invite => write!(f, "invite"),
            Self::Alert => write!(f, "alert"),
            Self::Notification => write!(f, "notification"),
            Self::Command => write!(f, "command"),
        }
    }
}

// ── Shared HTTP client ───────────────────────────────────────────────────

/// Shared HTTP client for MXroute API calls.
fn email_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

// ── Outbound: send_email ─────────────────────────────────────────────────

/// Send an email via MXroute's HTTP API.
///
/// Reads credentials from environment variables (see crate docs). Returns
/// `Ok(())` on success, `Err(EmailError::NotConfigured)` when the required
/// env vars are not set.
pub async fn send_email(to: &str, subject: &str, body: &str, mode: EmailMode) -> EmailResult<()> {
    let server = std::env::var("HKASK_MXROUTE_SERVER")
        .map_err(|_| EmailError::NotConfigured("HKASK_MXROUTE_SERVER not set".into()))?;
    let username = std::env::var("HKASK_SMTP_USERNAME")
        .map_err(|_| EmailError::NotConfigured("HKASK_SMTP_USERNAME not set".into()))?;
    let password = std::env::var("HKASK_SMTP_PASSWORD")
        .map_err(|_| EmailError::NotConfigured("HKASK_SMTP_PASSWORD not set".into()))?;
    let from = std::env::var("HKASK_CURATOR_EMAIL").unwrap_or_else(|_| username.clone());

    let payload = serde_json::json!({
        "server": server,
        "username": username,
        "password": password,
        "from": from,
        "to": to,
        "subject": subject,
        "body": body,
    });

    let response = email_client()
        .post("https://smtpapi.mxroute.com/")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| EmailError::ApiRequest(e.to_string()))?;

    if !response.status().is_success() {
        return Err(EmailError::ApiStatus(response.status().as_u16()));
    }

    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| EmailError::ApiRequest(format!("parse error: {e}")))?;

    if result["success"].as_bool() != Some(true) {
        let msg = result["message"].as_str().unwrap_or("unknown error");
        return Err(EmailError::ApiError(msg.to_string()));
    }

    tracing::info!(
        target = "reg.email.sent",
        to = %to,
        subject = %subject,
        mode = %mode,
        "REG"
    );
    Ok(())
}

/// Send a test email to verify MXroute credentials are working.
///
/// Used by the Settings → Kask → Curator Email "Send Test Email" button.
/// Returns `Ok(())` on success so the UI can show a success toast.
pub async fn send_test_email(to: &str) -> EmailResult<()> {
    let subject = "[hKask] Test Email — configuration verified";
    let body = "<h2>Test Email</h2>\n<p>This email confirms that your hKask curator email configuration is working correctly.</p>\n<p>The MXroute SMTP API accepted the send request with your configured credentials.</p>\n<p style='color:#8b949e;font-size:0.8rem'>Sent from Settings → Kask → Curator Email → Send Test Email</p>";
    send_email(to, subject, body, EmailMode::Notification).await
}

// ── Alert email sink (S3) ───────────────────────────────────────────────

/// `AlertEmailSink` implementation that sends algedonic alerts via the
/// curator's email channel. Non-blocking — spawns the async send on the
/// kask tokio runtime so the cybernetics loop is never blocked.
///
/// Stores a `tokio::runtime::Handle` rather than relying on the caller's
/// thread having a tokio context active. This makes `send_alert_email` safe
/// to call from any thread — the handle dispatches the spawn to the kask
/// runtime regardless of the caller's executor.
#[derive(Debug)]
pub struct CuratorAlertEmailSink {
    alert_recipient: String,
    tokio_handle: tokio::runtime::Handle,
}

impl CuratorAlertEmailSink {
    /// Create from env, returning `None` when no recipient is configured.
    ///
    /// `tokio_handle` is the kask tokio runtime handle (obtained via
    /// `gpui_tokio::Tokio::handle(cx)` at the composition root). The handle
    /// lets `send_alert_email` spawn the async send without relying on the
    /// caller's thread having a tokio context active.
    pub fn try_from_env(tokio_handle: tokio::runtime::Handle) -> Option<Arc<dyn AlertEmailSink>> {
        let recipient = std::env::var("HKASK_ALERT_EMAIL")
            .or_else(|_| std::env::var("HKASK_SMTP_USERNAME"))
            .ok()?;
        if recipient.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            alert_recipient: recipient,
            tokio_handle,
        }))
    }

    /// Create from explicit settings, returning `None` when no recipient is
    /// configured. This is the primary constructor for the composition root.
    ///
    /// `tokio_handle` is the kask tokio runtime handle (obtained via
    /// `gpui_tokio::Tokio::handle(cx)` at the composition root).
    pub fn try_from_settings(
        smtp_username: &str,
        alert_email: &str,
        tokio_handle: tokio::runtime::Handle,
    ) -> Option<Arc<dyn AlertEmailSink>> {
        let recipient = if !alert_email.is_empty() {
            alert_email
        } else if !smtp_username.is_empty() {
            smtp_username
        } else {
            return None;
        };
        Some(Arc::new(Self {
            alert_recipient: recipient.to_string(),
            tokio_handle,
        }))
    }
}

impl AlertEmailSink for CuratorAlertEmailSink {
    fn send_alert_email(&self, alert: &RuntimeAlert) {
        if self.alert_recipient.is_empty() {
            tracing::debug!(target: "reg.alert", "Alert email sink has no recipient — skipping");
            return;
        }
        let recipient = self.alert_recipient.clone();
        let domain = alert.domain.clone();
        let deficit = alert.deficit;
        let threshold = alert.threshold;
        let message = alert.message.clone();
        let subject = format!("[hKask Alert] {domain} variety deficit {deficit}/{threshold}");
        let body = format!(
            "<h2>Algedonic Alert</h2>\n<p><b>Domain:</b> {domain}</p>\n<p><b>Deficit:</b> {deficit} / {threshold}</p>\n<p><b>Message:</b> {message}</p>\n<p style='color:#8b949e;font-size:0.8rem'>Sent by the hKask Curator cybernetics loop</p>"
        );
        self.tokio_handle.spawn(async move {
            if let Err(e) = send_email(&recipient, &subject, &body, EmailMode::Alert).await {
                tracing::warn!(target: "reg.alert", error = %e, "Failed to send alert email");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests mutate process-global env vars (HKASK_ALERT_EMAIL,
    // HKASK_SMTP_USERNAME, …). Parallel execution races a `set_var` in one
    // test against a `remove_var` in another, producing flaky verdicts. The
    // lock serializes only the env-mutating tests; `try_from_settings_*`
    // and `email_mode_display` touch no env and run unlocked.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn email_mode_display() {
        assert_eq!(EmailMode::Alert.to_string(), "alert");
        assert_eq!(EmailMode::Invite.to_string(), "invite");
        assert_eq!(EmailMode::Notification.to_string(), "notification");
        assert_eq!(EmailMode::Command.to_string(), "command");
    }

    #[tokio::test]
    async fn send_email_returns_not_configured_when_env_missing() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::remove_var("HKASK_MXROUTE_SERVER");
            std::env::remove_var("HKASK_SMTP_USERNAME");
            std::env::remove_var("HKASK_SMTP_PASSWORD");
        }
        let result = send_email("recipient@example.com", "subject", "body", EmailMode::Alert).await;
        assert!(matches!(result, Err(EmailError::NotConfigured(_))));
    }

    #[tokio::test]
    async fn try_from_env_returns_none_when_no_env_var() {
        // Env vars are not set in the test environment (and the
        // send_email_returns_not_configured test above removes them), so
        // try_from_env should return None.
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::remove_var("HKASK_ALERT_EMAIL");
            std::env::remove_var("HKASK_SMTP_USERNAME");
        }
        let handle = tokio::runtime::Handle::current();
        assert!(CuratorAlertEmailSink::try_from_env(handle).is_none());
    }

    #[tokio::test]
    async fn try_from_env_uses_alert_email_when_set() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::set_var("HKASK_ALERT_EMAIL", "ops@example.com");
        }
        let handle = tokio::runtime::Handle::current();
        let sink = CuratorAlertEmailSink::try_from_env(handle);
        assert!(sink.is_some());
        unsafe {
            std::env::remove_var("HKASK_ALERT_EMAIL");
        }
    }

    #[tokio::test]
    async fn try_from_env_falls_back_to_smtp_username() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::remove_var("HKASK_ALERT_EMAIL");
            std::env::set_var("HKASK_SMTP_USERNAME", "curator@example.com");
        }
        let handle = tokio::runtime::Handle::current();
        let sink = CuratorAlertEmailSink::try_from_env(handle);
        assert!(sink.is_some());
        unsafe {
            std::env::remove_var("HKASK_SMTP_USERNAME");
        }
    }

    #[tokio::test]
    async fn try_from_settings_returns_none_when_both_empty() {
        let handle = tokio::runtime::Handle::current();
        assert!(CuratorAlertEmailSink::try_from_settings("", "", handle).is_none());
    }

    #[tokio::test]
    async fn try_from_settings_uses_alert_email_when_set() {
        let handle = tokio::runtime::Handle::current();
        let sink = CuratorAlertEmailSink::try_from_settings(
            "curator@example.com",
            "ops@example.com",
            handle,
        );
        assert!(sink.is_some());
    }

    #[tokio::test]
    async fn try_from_settings_falls_back_to_smtp_username() {
        let handle = tokio::runtime::Handle::current();
        let sink = CuratorAlertEmailSink::try_from_settings("curator@example.com", "", handle);
        assert!(sink.is_some());
    }
}
