//! Curator email interaction — bidirectional via MXroute.
//!
//! Outbound: SMTP API at smtpapi.mxroute.com (invites, alerts, notifications).
//! Inbound: IMAP port 993 SSL (command replies, same credentials as SMTP).
//!
//! Interaction modes (`EmailMode`): Invite, Alert, Notification, Command.
//! Each mode closes a different cybernetic feedback loop.
//!
//! P12 auth: inbound commands require sender allowlist (`HKASK_AUTHORIZED_EMAILS`)
//! and a one-time nonce token (`NonceStore`) issued in outbound alert emails.
//!
//! expect: "The Curator can send and receive email on behalf of the server"

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
    #[error("IMAP error: {0}")]
    Imap(String),
}

/// Result type for email operations.
pub type EmailResult<T> = std::result::Result<T, EmailError>;

/// Interaction mode for curator email — tags each message with its cybernetic purpose.
///
/// Each mode closes a different feedback loop:
/// - `Invite` — outbound, one-way (onboarding)
/// - `Alert` — outbound algedonic, closes S1→S5 when the live channel is dead
/// - `Notification` — outbound periodic digest (escalations + reg status)
/// - `Command` — inbound reply → curator MCP tool call (human-in-the-loop)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Send an email via MXroute's HTTP API.
///
/// Reads credentials from environment variables:
/// - HKASK_MXROUTE_SERVER — MXroute server hostname (e.g., "tuesday.mxrouting.net")
/// - HKASK_SMTP_USERNAME — full email address for auth
/// - HKASK_SMTP_PASSWORD — email account password
/// - HKASK_CURATOR_EMAIL — from address (default: HKASK_SMTP_USERNAME)
///
/// Returns Ok(()) on success, Err(message) on failure.
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

/// Send an invite email to a prospective member.
///
/// Includes the invite code, acceptance link, and server info.
pub async fn send_invite_email(
    to_email: &str,
    to_name: &str,
    invite_code: &str,
) -> EmailResult<()> {
    let domain = std::env::var("HKASK_DOMAIN").unwrap_or_else(|_| "localhost".to_string());
    let scheme = if domain == "localhost" {
        "http"
    } else {
        "https"
    };
    let accept_url = format!("{scheme}://{domain}/api/v1/auth/accept-invite?code={invite_code}");

    let subject = format!("You're invited to hKask — {domain}");
    let body = format!(
        r#"<h2>Welcome to hKask, {to_name}!</h2>
<p>You've been invited to join the hKask server at <strong>{domain}</strong>.</p>

<p>To get started:</p>
<ol>
  <li>Click the link below to accept your invite</li>
  <li>Sign in with your GitHub account</li>
  <li>Create your userpod — your personal AI agent</li>
  <li>Join the team chat and start collaborating</li>
</ol>

<p style="margin: 20px 0;">
  <a href="{accept_url}" style="background:#238636;color:#fff;padding:14px 32px;
     border-radius:8px;text-decoration:none;font-weight:600;display:inline-block;">
     Accept Invitation
  </a>
</p>

<p style="color:#8b949e;font-size:0.9rem;">
  Or copy this link: <code>{accept_url}</code>
</p>

<p style="color:#8b949e;font-size:0.85rem;margin-top:24px;">
  Your invite code: <strong>{invite_code}</strong><br>
  This code expires in 7 days.
</p>

<hr style="border:none;border-top:1px solid #30363d;margin:24px 0;">
<p style="color:#8b949e;font-size:0.8rem;">
  Sent by the hKask Curator · <a href="https://hkask.org">hkask.org</a>
</p>"#,
        to_name = to_name,
        domain = domain,
        accept_url = accept_url,
        invite_code = invite_code,
    );

    send_email(to_email, &subject, &body, EmailMode::Invite).await
}

// ── Inbound (IMAP) ─────────────────────────────────────────────────────

/// A received email fetched from the curator's IMAP inbox.
///
/// `body` is the raw TEXT part (RFC 822 body after headers) as best-effort
/// UTF-8. Full MIME/multipart parsing is deferred — S1 returns the primitive;
/// command parsing (S4) operates on this text.
#[derive(Debug, Clone)]
pub struct InboundEmail {
    pub from: String,
    pub subject: String,
    pub body: String,
    pub uid: u32,
}

/// Fetch unread messages from the curator's IMAP inbox (port 993 SSL).
///
/// Reuses the same env vars as [`send_email`] — no new credentials:
/// - `HKASK_MXROUTE_SERVER` — IMAP server hostname (same as SMTP)
/// - `HKASK_SMTP_USERNAME` — full email address (IMAP login)
/// - `HKASK_SMTP_PASSWORD` — mailbox password
///
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

// ── Alert email sink (S3) ───────────────────────────────────────────────

/// `AlertEmailSink` implementation that sends algedonic alerts via the
/// curator's email channel. Non-blocking — spawns the async send internally
/// so the cybernetics loop is never blocked.
#[derive(Debug)]
pub struct CuratorAlertEmailSink {
    alert_recipient: String,
    nonce_store: Option<std::sync::Arc<NonceStore>>,
}

impl CuratorAlertEmailSink {
    /// Create from env, returning `None` when no recipient is configured.
    pub fn try_from_env() -> Option<std::sync::Arc<dyn hkask_regulation::AlertEmailSink>> {
        let recipient = std::env::var("HKASK_ALERT_EMAIL")
            .or_else(|_| std::env::var("HKASK_SMTP_USERNAME"))
            .ok()?;
        if recipient.is_empty() {
            return None;
        }
        Some(std::sync::Arc::new(Self {
            alert_recipient: recipient,
            nonce_store: None,
        }))
    }

    /// Create from env with a shared nonce store for P12 token auth.
    pub fn try_from_env_with_nonce(
        nonce: std::sync::Arc<NonceStore>,
    ) -> Option<std::sync::Arc<dyn hkask_regulation::AlertEmailSink>> {
        let recipient = std::env::var("HKASK_ALERT_EMAIL")
            .or_else(|_| std::env::var("HKASK_SMTP_USERNAME"))
            .ok()?;
        if recipient.is_empty() {
            return None;
        }
        Some(std::sync::Arc::new(Self {
            alert_recipient: recipient,
            nonce_store: Some(nonce),
        }))
    }
}

impl hkask_regulation::AlertEmailSink for CuratorAlertEmailSink {
    fn send_alert_email(&self, alert: &hkask_regulation::RuntimeAlert) {
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
/// The alert email sink issues a token (included in the alert body). The
/// inbox poller verifies the token from the reply. Tokens are one-time use
/// and expire after `ttl`. This prevents spoofed-email command injection —
/// an attacker would need to intercept the alert email to obtain a valid token.
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

// ── Inbound command parsing (S4) + polling task (S7) ───────────────────────

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
pub fn spawn_inbox_poller<F>(
    interval_secs: u64,
    nonce_store: Option<std::sync::Arc<NonceStore>>,
    handler: F,
) where
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

/// Wire the inbox poller to an `AgentService`, dispatching email commands to
/// the governance layer (resolve/dismiss escalations). The poller is a no-op
/// when IMAP is not configured.
///
/// Pass `Some(nonce_store)` to require P12 nonce-token auth on inbound commands.
/// Call after `AgentService::build_with_email()` returns.
pub fn wire_inbox_poller(
    ctx: &hkask_services_context::AgentService,
    poll_interval_secs: u64,
    nonce_store: Option<std::sync::Arc<NonceStore>>,
) {
    let escalations = std::sync::Arc::clone(&ctx.governance().escalations);
    let events = ctx.governance().events.clone();
    let userpod = ctx.config().user_name.clone();

    spawn_inbox_poller(poll_interval_secs, nonce_store, move |msg, cmd| match cmd {
        EmailCommand::Resolve { escalation_id, .. } => {
            match hkask_services_context::governance::resolve_direct(
                &escalations,
                &events,
                &escalation_id,
                &userpod,
            ) {
                Ok(()) => tracing::info!(
                    target = "reg.email.received",
                    from = %msg.from,
                    id = %escalation_id,
                    "Escalation resolved via email command"
                ),
                Err(e) => tracing::warn!(
                    target = "reg.email.received",
                    error = %e,
                    id = %escalation_id,
                    "Email resolve command failed"
                ),
            }
        }
        EmailCommand::Dismiss { escalation_id, .. } => {
            match hkask_services_context::governance::dismiss_direct(
                &escalations,
                &events,
                &escalation_id,
                &userpod,
            ) {
                Ok(()) => tracing::info!(
                    target = "reg.email.received",
                    from = %msg.from,
                    id = %escalation_id,
                    "Escalation dismissed via email command"
                ),
                Err(e) => tracing::warn!(
                    target = "reg.email.received",
                    error = %e,
                    id = %escalation_id,
                    "Email dismiss command failed"
                ),
            }
        }
        EmailCommand::Unknown => {
            tracing::debug!(
                target = "reg.email.received",
                from = %msg.from,
                "No command parsed from email"
            );
        }
    });
}

// ── Notification/Digest mode (S5) ──────────────────────────────────────────

/// Send a digest email summarizing pending escalations.
async fn send_digest(
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

/// Escape HTML special characters in user-provided text.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Spawn a periodic digest email task.
pub fn spawn_digest_task(
    escalations: std::sync::Arc<hkask_storage::EscalationQueue>,
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

/// Wire the digest email task to an AgentService. Reads
/// (default 86400 = daily). No-op when email is not configured.
pub fn wire_digest_task(ctx: &hkask_services_context::AgentService) {
    let recipient = std::env::var("HKASK_ALERT_EMAIL")
        .or_else(|_| std::env::var("HKASK_SMTP_USERNAME"))
        .unwrap_or_default();
    if recipient.is_empty() {
        return;
    }
    let interval = std::env::var("HKASK_DIGEST_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(86400);
    let escalations = std::sync::Arc::clone(&ctx.governance().escalations);
    spawn_digest_task(escalations, recipient, interval);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_resolve() {
        assert_eq!(
            parse_command("resolve abc-123"),
            EmailCommand::Resolve {
                escalation_id: "abc-123".into(),
                token: None
            }
        );
    }

    #[test]
    fn parse_command_dismiss() {
        assert_eq!(
            parse_command("dismiss xyz-789"),
            EmailCommand::Dismiss {
                escalation_id: "xyz-789".into(),
                token: None
            }
        );
    }

    #[test]
    fn parse_command_case_insensitive() {
        assert_eq!(
            parse_command("RESOLVE ID-1"),
            EmailCommand::Resolve {
                escalation_id: "id-1".into(),
                token: None
            }
        );
    }

    #[test]
    fn parse_command_multiline_body() {
        let body = "Thanks for the alert.\n\nresolve esc-42\nLet me know when done.";
        assert_eq!(
            parse_command(body),
            EmailCommand::Resolve {
                escalation_id: "esc-42".into(),
                token: None
            }
        );
    }

    #[test]
    fn parse_command_unknown() {
        assert_eq!(
            parse_command("Hello, just checking in."),
            EmailCommand::Unknown
        );
        assert_eq!(parse_command(""), EmailCommand::Unknown);
    }

    #[test]
    fn parse_command_with_token() {
        let body = "resolve esc-99\ntoken:550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            parse_command(body),
            EmailCommand::Resolve {
                escalation_id: "esc-99".into(),
                token: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            }
        );
    }

    #[test]
    fn parse_command_token_before_command() {
        let body = "token:abc-123\nresolve esc-1";
        assert_eq!(
            parse_command(body),
            EmailCommand::Resolve {
                escalation_id: "esc-1".into(),
                token: Some("abc-123".into()),
            }
        );
    }

    #[test]
    fn email_mode_display() {
        assert_eq!(EmailMode::Invite.to_string(), "invite");
        assert_eq!(EmailMode::Alert.to_string(), "alert");
        assert_eq!(EmailMode::Notification.to_string(), "notification");
        assert_eq!(EmailMode::Command.to_string(), "command");
    }

    #[test]
    fn nonce_store_issue_and_verify() {
        let store = NonceStore::new(std::time::Duration::from_secs(3600));
        let token = store.issue();
        assert!(store.verify(&token), "freshly issued token should verify");
    }

    #[test]
    fn nonce_store_one_time_use() {
        let store = NonceStore::new(std::time::Duration::from_secs(3600));
        let token = store.issue();
        assert!(store.verify(&token), "first use should succeed");
        assert!(!store.verify(&token), "second use should fail (consumed)");
    }

    #[test]
    fn nonce_store_rejects_unknown() {
        let store = NonceStore::new(std::time::Duration::from_secs(3600));
        assert!(
            !store.verify("nonexistent-token"),
            "unknown token should fail"
        );
    }

    #[test]
    fn nonce_store_rejects_expired() {
        let store = NonceStore::new(std::time::Duration::from_millis(1));
        let token = store.issue();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!store.verify(&token), "expired token should fail");
    }

    #[test]
    fn html_escape_basics() {
        assert_eq!(
            html_escape("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("plain text"), "plain text");
    }

    #[ignore = "requires live MXroute credentials (HKASK_SMTP_USERNAME, HKASK_SMTP_PASSWORD, HKASK_MXROUTE_SERVER)"]
    #[tokio::test]
    async fn imap_round_trip() {
        let to = std::env::var("HKASK_SMTP_USERNAME").expect("HKASK_SMTP_USERNAME set");
        let subject = "hKask integration test - fetch_unread";
        let body = "<p>This is a test email for IMAP round-trip verification.</p>";
        send_email(&to, subject, body, EmailMode::Notification)
            .await
            .expect("send_email");
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let messages = fetch_unread().await.expect("fetch_unread");
        let found = messages
            .iter()
            .any(|m| m.subject.contains("hKask integration test"));
        assert!(found, "test email not found in unread messages");
    }
}
