//! Curator email — bidirectional via MXroute.
//!
//! Outbound: SMTP API at smtpapi.mxroute.com (alerts, notifications, test).
//! Inbound: IMAP port 993 SSL (command replies, same credentials as SMTP).
//!
//! Interaction modes (`EmailMode`): Invite, Alert, Notification, Command.
//! Each mode closes a different cybernetic feedback loop.
//!
//! P12 auth: inbound commands require sender allowlist (`HKASK_AUTHORIZED_EMAILS`)
//! and a one-time nonce token (`NonceStore`) issued in outbound alert emails.
//!
//! Credentials are read from environment variables (set by the composition
//! root from kask settings + keychain):
//! - `HKASK_MXROUTE_SERVER` — MXroute server hostname (e.g. "tuesday.mxrouting.net")
//! - `HKASK_SMTP_USERNAME` — full email address for auth
//! - `HKASK_SMTP_PASSWORD` — email account password (from keychain)
//! - `HKASK_CURATOR_EMAIL` — from address (default: `HKASK_SMTP_USERNAME`)
//! - `HKASK_ALERT_EMAIL` — alert recipient (default: `HKASK_SMTP_USERNAME`)
//! - `HKASK_AUTHORIZED_EMAILS` — comma-separated sender allowlist (P12)
//! - `HKASK_INBOX_POLL_INTERVAL_SECS` — IMAP poll interval (default 60, 0 = disabled)
//! - `HKASK_DIGEST_INTERVAL_SECS` — digest interval (default 86400, 0 = disabled)

use std::sync::Arc;

use hkask_regulation::{AlertEmailSink, RuntimeAlert};
use serde::{Deserialize, Serialize};

// ── Errors ───────────────────────────────────────────────────────────────

/// Errors that can occur during email delivery or inbound fetch.
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
    #[error("IMAP error: {0}")]
    Imap(String),
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

// ── Shared HTTP client + IMAP TLS connector ──────────────────────────────

/// Shared HTTP client for MXroute API calls.
fn email_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Cached IMAP TLS connector — avoids rebuilding the root cert store on every poll.
fn imap_tls_connector() -> tokio_rustls::TlsConnector {
    use std::sync::OnceLock;
    static CONNECTOR: OnceLock<tokio_rustls::TlsConnector> = OnceLock::new();
    CONNECTOR
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
        })
        .clone()
}

// ── MIME body extraction ─────────────────────────────────────────────────

/// Extract the text/plain body from a raw RFC 822 message.
/// Falls back to raw UTF-8 lossy conversion if parsing fails.
fn extract_text_body(raw: &[u8]) -> String {
    match mailparse::parse_mail(raw) {
        Ok(parsed) => extract_text_plain_from_mime(&parsed),
        Err(_) => String::from_utf8_lossy(raw).into_owned(),
    }
}

/// Recursively walk the MIME tree to find the first text/plain part.
fn extract_text_plain_from_mime(msg: &mailparse::ParsedMail) -> String {
    if msg.ctype.mimetype == "text/plain" {
        return msg.get_body().unwrap_or_default();
    }
    for part in &msg.subparts {
        let text = extract_text_plain_from_mime(part);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
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
/// curator's email channel. Non-blocking — spawns the async send internally
/// so the cybernetics loop is never blocked.
#[derive(Debug)]
pub struct CuratorAlertEmailSink {
    alert_recipient: String,
    nonce_store: Option<Arc<NonceStore>>,
}

impl CuratorAlertEmailSink {
    /// Create from env, returning `None` when no recipient is configured.
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

    /// Create from explicit settings, returning `None` when no recipient is
    /// configured. This is the primary constructor for the composition root.
    pub fn try_from_settings(
        smtp_username: &str,
        alert_email: &str,
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
            nonce_store: None,
        }))
    }

    /// Create from explicit settings with a shared nonce store.
    pub fn try_from_settings_with_nonce(
        smtp_username: &str,
        alert_email: &str,
        nonce: Arc<NonceStore>,
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
            nonce_store: Some(nonce),
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
            tokens.remove(token);
            return valid;
        }
        false
    }

    fn cleanup_expired(&self) {
        let ttl = self.ttl;
        self.tokens
            .lock()
            .expect("nonce store not poisoned")
            .retain(|_, issued_at| issued_at.elapsed() < ttl);
    }
}

// ── Inbound: IMAP fetch (S4) ─────────────────────────────────────────────

/// A fetched inbound email message.
#[derive(Debug, Clone)]
pub struct InboundEmail {
    pub from: String,
    pub subject: String,
    pub body: String,
    pub uid: u32,
}

/// Fetch unread messages from the curator's IMAP inbox (port 993 SSL).
///
/// Reuses the same env vars as [`send_email`] — no new credentials.
/// Returns unread messages and marks them `\Seen`. Emits `reg.email.received`
/// per message. This closes the inbound half of the email feedback loop.
pub async fn fetch_unread() -> EmailResult<Vec<InboundEmail>> {
    use futures_util::StreamExt;

    let server = std::env::var("HKASK_MXROUTE_SERVER")
        .map_err(|_| EmailError::NotConfigured("HKASK_MXROUTE_SERVER not set".into()))?;
    let username = std::env::var("HKASK_SMTP_USERNAME")
        .map_err(|_| EmailError::NotConfigured("HKASK_SMTP_USERNAME not set".into()))?;
    let password = std::env::var("HKASK_SMTP_PASSWORD")
        .map_err(|_| EmailError::NotConfigured("HKASK_SMTP_PASSWORD not set".into()))?;

    let connector = imap_tls_connector();

    let tcp = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::net::TcpStream::connect((server.as_str(), 993)),
    )
    .await
    .map_err(|_| EmailError::Imap("imap connect timeout (30s)".into()))?
    .map_err(|e| EmailError::Imap(format!("tcp connect: {e}")))?;
    let server_name = rustls::pki_types::ServerName::try_from(server.clone())
        .map_err(|e| EmailError::Imap(format!("server name: {e}")))?;
    let tls_stream = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        connector.connect(server_name, tcp),
    )
    .await
    .map_err(|_| EmailError::Imap("tls handshake timeout (15s)".into()))?
    .map_err(|e| EmailError::Imap(format!("tls handshake: {e}")))?;

    // async-imap uses futures-ecosystem AsyncRead/AsyncWrite; tokio-rustls
    // provides tokio-ecosystem traits. The compat adapter bridges them.
    use tokio_util::compat::TokioAsyncReadCompatExt;
    let client = async_imap::Client::new(tls_stream.compat());
    let mut session = client
        .login(&username, &password)
        .await
        .map_err(|(e, _)| EmailError::Imap(format!("imap login: {e}")))?;

    session
        .select("INBOX")
        .await
        .map_err(|e| EmailError::Imap(format!("select inbox: {e}")))?;

    let uids: Vec<u32> = session
        .uid_search("UNSEEN")
        .await
        .map_err(|e| EmailError::Imap(format!("search unseen: {e}")))?
        .into_iter()
        .collect();

    let mut messages = Vec::new();
    if !uids.is_empty() {
        let uid_set = uids
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut fetch_stream = session
            .uid_fetch(&uid_set, "(UID ENVELOPE BODY.PEEK[])")
            .await
            .map_err(|e| EmailError::Imap(format!("fetch: {e}")))?;

        while let Some(msg) = fetch_stream
            .next()
            .await
            .transpose()
            .map_err(|e| EmailError::Imap(format!("fetch iter: {e}")))?
        {
            let uid = msg.uid.unwrap_or(0);
            let from = msg
                .envelope()
                .and_then(|env| env.from.as_ref())
                .and_then(|addrs| addrs.first())
                .map(|a| {
                    let mailbox = a
                        .mailbox
                        .as_deref()
                        .map(|m| String::from_utf8_lossy(m).into_owned())
                        .unwrap_or_default();
                    let host = a
                        .host
                        .as_deref()
                        .map(|h| String::from_utf8_lossy(h).into_owned())
                        .unwrap_or_default();
                    format!("{mailbox}@{host}")
                })
                .unwrap_or_default();
            let subject = msg
                .envelope()
                .and_then(|env| env.subject.as_deref())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .unwrap_or_default();
            let body = msg.body().map(extract_text_body).unwrap_or_default();

            tracing::info!(
                target = "reg.email.received",
                from = %from,
                subject = %subject,
                "REG"
            );
            messages.push(InboundEmail {
                from,
                subject,
                body,
                uid,
            });
        }
        drop(fetch_stream);

        // Mark fetched messages as seen (PEEK avoided auto-marking during fetch).
        let _ = session
            .uid_store(&uid_set, "+FLAGS (\\Seen)")
            .await
            .map_err(|e| EmailError::Imap(format!("store seen: {e}")))?;
    }

    if let Err(e) = session.logout().await {
        tracing::debug!(target: "reg.email.received", error = %e, "IMAP logout failed (non-critical)");
    }
    Ok(messages)
}

// ── Inbound command parsing (S4) ────────────────────────────────────────

/// A parsed command from an inbound email reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailCommand {
    Resolve {
        escalation_id: String,
        token: Option<String>,
    },
    Dismiss {
        escalation_id: String,
        token: Option<String>,
    },
    Unknown,
}

/// Parse an email body for a command verb and optional nonce token.
///
/// Supported verbs (case-insensitive, first match wins):
/// - `resolve <id>` — resolve an escalation
/// - `dismiss <id>` — dismiss an escalation as not actionable
/// - `token:<uuid>` — nonce token for P12 auth (may appear on any line)
#[must_use]
pub fn parse_command(body: &str) -> EmailCommand {
    let mut token = None;
    let mut found_command = false;
    let mut escalation_id = String::new();
    let mut is_resolve = false;

    for line in body.lines() {
        let line = line.trim().to_lowercase();
        if let Some(rest) = line.strip_prefix("token:") {
            token = Some(rest.trim().to_string());
        }
        if !found_command {
            if let Some(rest) = line.strip_prefix("resolve ") {
                escalation_id = rest.trim().to_string();
                is_resolve = true;
                found_command = true;
            } else if let Some(rest) = line.strip_prefix("dismiss ") {
                escalation_id = rest.trim().to_string();
                found_command = true;
            }
        }
    }

    if !found_command {
        EmailCommand::Unknown
    } else if is_resolve {
        EmailCommand::Resolve {
            escalation_id,
            token,
        }
    } else {
        EmailCommand::Dismiss {
            escalation_id,
            token,
        }
    }
}

/// Check if a sender is authorized (P12). Reads `HKASK_AUTHORIZED_EMAILS`.
#[must_use]
pub fn is_authorized_sender(from: &str) -> bool {
    let allowlist = std::env::var("HKASK_AUTHORIZED_EMAILS").unwrap_or_default();
    !allowlist.is_empty() && allowlist.split(',').any(|e| e.trim() == from)
}

/// Read the inbox poll interval from `HKASK_INBOX_POLL_INTERVAL_SECS` (default 60).
#[must_use]
pub fn inbox_poll_interval_secs() -> u64 {
    std::env::var("HKASK_INBOX_POLL_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
}

/// Spawn a background IMAP poller that calls `handler` per received email.
/// When `nonce_store` is provided, commands without a valid token are rejected.
pub fn spawn_inbox_poller<F>(interval_secs: u64, nonce_store: Option<Arc<NonceStore>>, handler: F)
where
    F: Fn(InboundEmail, EmailCommand) + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(interval_secs);
        loop {
            tokio::time::sleep(interval).await;
            match fetch_unread().await {
                Ok(messages) => {
                    for msg in messages {
                        let authorized = is_authorized_sender(&msg.from);
                        let cmd = parse_command(&msg.body);
                        let nonce_ok = match (&nonce_store, &cmd) {
                            (Some(store), EmailCommand::Resolve { token: Some(t), .. })
                            | (Some(store), EmailCommand::Dismiss { token: Some(t), .. }) => {
                                store.verify(t)
                            }
                            (Some(_), EmailCommand::Resolve { token: None, .. })
                            | (Some(_), EmailCommand::Dismiss { token: None, .. }) => false,
                            _ => true,
                        };
                        if authorized && nonce_ok {
                            handler(msg, cmd);
                        } else if !authorized {
                            tracing::warn!(target = "reg.email.received", from = %msg.from, "Unauthorized sender - command ignored (P12)");
                        } else if !nonce_ok {
                            tracing::warn!(target = "reg.email.received", from = %msg.from, "Invalid or missing nonce token - command rejected (P12)");
                        }
                    }
                }
                Err(EmailError::NotConfigured(_)) => {}
                Err(e) => {
                    tracing::warn!(target = "reg.email.received", error = %e, "IMAP poll failed")
                }
            }
        }
    });
}

// ── Notification/Digest mode (S5) ──────────────────────────────────────────

/// Escape HTML special characters in user-provided text.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Send a digest email summarizing pending escalations.
pub async fn send_digest(
    escalations: &hkask_storage::EscalationQueue,
    recipient: &str,
) -> EmailResult<()> {
    let pending = match escalations.list_pending() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "reg.email.sent", error = %e, "Digest: failed to list pending escalations");
            return Ok(());
        }
    };
    let count = pending.len();
    if pending.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");

    let mut rows = String::new();
    for entry in pending.iter().take(10) {
        let id = entry.id.to_string();
        let output = html_escape(&entry.output);
        let created = entry.created_at.format("%Y-%m-%d %H:%M").to_string();
        rows.push_str(&format!(
            "<tr><td><code>{id}</code></td><td>{output}</td><td>{created}</td></tr>"
        ));
    }
    let truncated = if count > 10 {
        format!("<p style='color:#8b949e'>Showing 10 of {count}.</p>")
    } else {
        String::new()
    };

    let subject = format!("[hKask Digest] {count} pending escalation(s)");
    let body = format!(
        "<h2>hKask Escalation Digest</h2><p><b>{count}</b> pending escalation(s) as of {now}</p><table border='1' cellpadding='6' style='border-collapse:collapse'><tr><th>ID</th><th>Output</th><th>Created</th></tr>{rows}</table>{truncated}<p style='color:#8b949e;font-size:0.8rem'>To resolve an escalation, reply to an alert email with: resolve &lt;id&gt;</p>"
    );
    send_email(recipient, &subject, &body, EmailMode::Notification).await
}

/// Spawn a periodic digest email task.
pub fn spawn_digest_task(
    escalations: Arc<hkask_storage::EscalationQueue>,
    recipient: String,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(interval_secs);
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = send_digest(&escalations, &recipient).await {
                tracing::warn!(target: "reg.email.sent", error = %e, "Digest email failed");
            }
        }
    });
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
        assert_eq!(EmailMode::Command.to_string(), "command");
    }

    #[tokio::test]
    async fn send_email_returns_not_configured_when_env_missing() {
        unsafe {
            std::env::remove_var("HKASK_MXROUTE_SERVER");
            std::env::remove_var("HKASK_SMTP_USERNAME");
            std::env::remove_var("HKASK_SMTP_PASSWORD");
        }
        let result = send_email("recipient@example.com", "subject", "body", EmailMode::Alert).await;
        assert!(matches!(result, Err(EmailError::NotConfigured(_))));
    }

    #[test]
    fn try_from_settings_returns_none_when_both_empty() {
        assert!(CuratorAlertEmailSink::try_from_settings("", "").is_none());
    }

    #[test]
    fn try_from_settings_uses_alert_email_when_set() {
        let sink =
            CuratorAlertEmailSink::try_from_settings("curator@example.com", "ops@example.com");
        assert!(sink.is_some());
    }

    #[test]
    fn try_from_settings_falls_back_to_smtp_username() {
        let sink = CuratorAlertEmailSink::try_from_settings("curator@example.com", "");
        assert!(sink.is_some());
    }

    #[test]
    fn parse_command_resolve() {
        let cmd = parse_command("resolve abc-123\ntoken:xyz");
        assert_eq!(
            cmd,
            EmailCommand::Resolve {
                escalation_id: "abc-123".to_string(),
                token: Some("xyz".to_string()),
            }
        );
    }

    #[test]
    fn parse_command_dismiss() {
        let cmd = parse_command("dismiss def-456");
        assert_eq!(
            cmd,
            EmailCommand::Dismiss {
                escalation_id: "def-456".to_string(),
                token: None,
            }
        );
    }

    #[test]
    fn parse_command_unknown() {
        let cmd = parse_command("hello world");
        assert_eq!(cmd, EmailCommand::Unknown);
    }

    #[test]
    fn parse_command_case_insensitive() {
        let cmd = parse_command("RESOLVE abc-123");
        assert_eq!(
            cmd,
            EmailCommand::Resolve {
                escalation_id: "abc-123".to_string(),
                token: None,
            }
        );
    }

    #[test]
    fn html_escape_escapes_special_chars() {
        assert_eq!(html_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
    }
}
