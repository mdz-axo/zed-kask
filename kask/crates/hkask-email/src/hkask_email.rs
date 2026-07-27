//! Curator outbound email via MXroute's SMTP API.
//!
//! Outbound only: algedonic alerts, invites, notifications. Inbound IMAP
//! polling (command replies) lives in `hkask-api` in the upstream hKask
//! codebase and is intentionally not ported here — zed-kask closes the
//! algedonic loop via the live GPUI toast channel and this email sink as
//! the last-resort fallback when that channel is down.
//!
//! P12 auth: the alert email sink issues a one-time nonce token
//! (`NonceStore`) included in the alert body. The upstream hKask inbox
//! poller verifies the token from the reply. Tokens are one-time use and
//! expire after `ttl`. This prevents spoofed-email command injection —
//! an attacker would need to intercept the alert email to obtain a valid
//! token. The token is included in outbound alerts even when no inbound
//! poller is wired, so a future inbound path can verify replies without
//! re-issuing tokens.
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

/// Interaction mode for curator email — tags each message with its cybernetic purpose.
///
/// Each mode closes a different feedback loop:
/// - `Invite` — outbound, one-way (onboarding)
/// - `Alert` — outbound algedonic, closes S1→S5 when the live channel is dead
/// - `Notification` — outbound periodic digest (escalations + reg status)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmailMode {
    Invite,
    Alert,
    Notification,
}

impl std::fmt::Display for EmailMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invite => write!(f, "invite"),
            Self::Alert => write!(f, "alert"),
            Self::Notification => write!(f, "notification"),
        }
    }
}

/// Shared HTTP client for MXroute API calls.
fn email_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

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

// ── Alert email sink (S3) ───────────────────────────────────────────────

/// `AlertEmailSink` implementation that sends algedonic alerts via the
/// curator's email channel. Non-blocking — spawns the async send internally
/// so the cybernetics loop is never blocked.
#[derive(Debug)]
pub struct CuratorAlertEmailSink {
    alert_recipient: String,
    nonce_store: Option<Arc<NonceStore>>,
}

impl CuratorAlertEmailSink {
    /// Create from env, returning `None` when no recipient is configured.
    ///
    /// Reads `HKASK_ALERT_EMAIL`, falling back to `HKASK_SMTP_USERNAME`.
    /// Returns `None` if neither is set or empty — the caller should fall
    /// back to a logging sink in that case.
    pub fn try_from_env() -> Option<Arc<dyn AlertEmailSink>> {
        let recipient = std::env::var("HKASK_ALERT_EMAIL")
            .or_else(|_| std::env::var("HKASK_SMTP_USERNAME"))
            .ok()?;
        if recipient.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            alert_recipient: recipient,
            nonce_store: None,
        }))
    }

    /// Create from env with a shared nonce store for P12 token auth.
    pub fn try_from_env_with_nonce(nonce: Arc<NonceStore>) -> Option<Arc<dyn AlertEmailSink>> {
        let recipient = std::env::var("HKASK_ALERT_EMAIL")
            .or_else(|_| std::env::var("HKASK_SMTP_USERNAME"))
            .ok()?;
        if recipient.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            alert_recipient: recipient,
            nonce_store: Some(nonce),
        }))
    }
}

impl AlertEmailSink for CuratorAlertEmailSink {
    fn send_alert_email(&self, alert: &RuntimeAlert) {
        if self.alert_recipient.is_empty() {
            tracing::warn!(target: "reg.alert", "Alert email sink has no recipient");
            return;
        }
        let recipient = self.alert_recipient.clone();
        let domain = alert.domain.clone();
        let deficit = alert.deficit;
        let threshold = alert.threshold;
        let message = alert.message.clone();
        let subject = format!("[hKask Alert] {domain} variety deficit {deficit}/{threshold}");
        let token = self.nonce_store.as_ref().map(|s| s.issue());
        let token_line = token
            .as_ref()
            .map(|t| format!("<p style='margin-top:16px'><b>To respond, reply with your command and:</b> token:{t}</p>"))
            .unwrap_or_default();
        let body = format!(
            "<h2>Algedonic Alert</h2>\n<p><b>Domain:</b> {domain}</p>\n<p><b>Deficit:</b> {deficit} / {threshold}</p>\n<p><b>Message:</b> {message}</p>{token_line}\n<p style='color:#8b949e;font-size:0.8rem'>Sent by the hKask Curator cybernetics loop</p>"
        );
        tokio::spawn(async move {
            if let Err(e) = send_email(&recipient, &subject, &body, EmailMode::Alert).await {
                tracing::warn!(target: "reg.alert", error = %e, "Failed to send alert email");
            }
        });
    }
}

// ── Nonce store (P12 token auth) ──────────────────────────────────────────

/// In-memory one-time token store for P12 email command auth.
///
/// The alert email sink issues a token (included in the alert body). A
/// future inbound poller verifies the token from the reply. Tokens are
/// one-time use and expire after `ttl`. This prevents spoofed-email
/// command injection — an attacker would need to intercept the alert
/// email to obtain a valid token.
pub struct NonceStore {
    tokens: std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    ttl: std::time::Duration,
}

impl std::fmt::Debug for NonceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NonceStore")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl NonceStore {
    /// Create a store with a token TTL (e.g. 24h).
    pub fn new(ttl: std::time::Duration) -> Self {
        Self {
            tokens: std::sync::Mutex::new(std::collections::HashMap::new()),
            ttl,
        }
    }

    /// Issue a one-time token. Returns the token string to include in an email.
    pub fn issue(&self) -> String {
        self.cleanup_expired();
        let token = uuid::Uuid::new_v4().to_string();
        self.tokens
            .lock()
            .expect("nonce store not poisoned")
            .insert(token.clone(), std::time::Instant::now());
        token
    }

    /// Verify and consume a token. Returns `true` if valid and not expired.
    pub fn verify(&self, token: &str) -> bool {
        let mut tokens = self.tokens.lock().expect("nonce store not poisoned");
        if let Some(issued_at) = tokens.get(token) {
            let valid = issued_at.elapsed() < self.ttl;
            tokens.remove(token); // one-time use
            return valid;
        }
        false
    }

    /// Remove expired tokens. Called automatically by `issue()` to prevent
    /// unbounded growth from tokens that are never verified (e.g. alert emails
    /// that the recipient ignores).
    fn cleanup_expired(&self) {
        let ttl = self.ttl;
        self.tokens
            .lock()
            .expect("nonce store not poisoned")
            .retain(|_, issued_at| issued_at.elapsed() < ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_store_issues_and_verifies() {
        let store = NonceStore::new(std::time::Duration::from_secs(60));
        let token = store.issue();
        assert!(store.verify(&token), "fresh token should verify");
        assert!(
            !store.verify(&token),
            "token should be one-time use (consumed)"
        );
    }

    #[test]
    fn nonce_store_rejects_unknown_token() {
        let store = NonceStore::new(std::time::Duration::from_secs(60));
        assert!(!store.verify("nonexistent-token"));
    }

    #[test]
    fn nonce_store_expires_tokens() {
        let store = NonceStore::new(std::time::Duration::from_millis(1));
        let token = store.issue();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!store.verify(&token), "expired token should not verify");
    }

    #[test]
    fn email_mode_display() {
        assert_eq!(EmailMode::Alert.to_string(), "alert");
        assert_eq!(EmailMode::Invite.to_string(), "invite");
        assert_eq!(EmailMode::Notification.to_string(), "notification");
    }

    #[tokio::test]
    async fn send_email_returns_not_configured_when_env_missing() {
        // Clear env to ensure deterministic test (the test runner may have
        // these set in its environment. `set_var`/`remove_var` are unsafe
        // in Rust 2024 because they mutate process-global state outside
        // the borrow checker's view.
        unsafe {
            std::env::remove_var("HKASK_MXROUTE_SERVER");
            std::env::remove_var("HKASK_SMTP_USERNAME");
            std::env::remove_var("HKASK_SMTP_PASSWORD");
        }

        let result = send_email("recipient@example.com", "subject", "body", EmailMode::Alert).await;
        assert!(matches!(result, Err(EmailError::NotConfigured(_))));
    }

    #[test]
    fn alert_sink_returns_none_when_no_env() {
        unsafe {
            std::env::remove_var("HKASK_ALERT_EMAIL");
            std::env::remove_var("HKASK_SMTP_USERNAME");
        }
        assert!(CuratorAlertEmailSink::try_from_env().is_none());
    }

    #[test]
    fn alert_sink_returns_some_when_recipient_configured() {
        unsafe {
            std::env::set_var("HKASK_ALERT_EMAIL", "ops@example.com");
        }
        let sink = CuratorAlertEmailSink::try_from_env();
        assert!(sink.is_some());
        unsafe {
            std::env::remove_var("HKASK_ALERT_EMAIL");
        }
    }
}
