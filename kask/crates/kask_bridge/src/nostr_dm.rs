//! Editor-only NIP-17 Private Direct Messages (<https://nostr-nips.com/nip-17>)
//! using NIP-44 v2 encryption (<https://nostr-nips.com/nip-44>) and NIP-59 Gift
//! Wrap (<https://nostr-nips.com/nip-59>).
//!
//! Security & privacy boundaries:
//! - Strictly NIP-17 (`kind: 14` rumor → `kind: 13` seal → `kind: 1059` gift
//!   wrap). Legacy NIP-04 (`kind: 4`) is intentionally not implemented.
//! - Runs strictly inside the editor process: never exposed as an MCP tool,
//!   never stored in the unencrypted loopback LMDB relay (`hkask-mcp-nostr-relay`),
//!   and never injected into Steer/LLM context.
//! - Local persistence uses a SQLCipher-encrypted SQLite database
//!   (`{data_dir}/nostr-dm/messages.db`) keyed with the operator's keychain
//!   password (`NOSTR_PASSWORD_CREDENTIAL_URL`).
//! - Sending a DM always requires explicit operator confirmation via a native
//!   editor prompt (never bypassed by `kask.nostr.autopublish`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{AppContext as _, AsyncApp, PromptLevel};
use hkask_ipc_wire::nostr_wire::MAX_CONTENT_CHARS;
use nostr::event::{FinalizeEvent, FinalizeUnsignedEvent};
use nostr::nips::nip59;
use nostr_sdk::prelude::{
    Client, Event, EventBuilder, Filter, Keys, Kind, PublicKey, SecretKey as SdkSecretKey, Tag,
    ToBech32,
};
use rusqlite::{Connection, params};
use settings::Settings as _;
use zeroize::Zeroizing;

use crate::credentials::{NOSTR_NCRYPTSEC_CREDENTIAL_URL, NOSTR_PASSWORD_CREDENTIAL_URL};
use crate::settings::KaskSettings;

/// One decrypted NIP-17 direct message stored in the local SQLCipher database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmMessage {
    pub rumor_id: String,
    pub peer_pubkey_hex: String,
    pub sender_pubkey_hex: String,
    pub created_at: u64,
    pub content: String,
    pub outgoing: bool,
}

/// Summary of one DM conversation partner for the panel's thread list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmThreadSummary {
    pub peer_pubkey_hex: String,
    pub peer_npub: String,
    pub last_message: String,
    pub last_created_at: u64,
    pub message_count: usize,
}

/// Result of syncing `kind: 1059` gift wraps from configured `wss://` relays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmSyncReport {
    pub fetched_wraps: usize,
    pub newly_stored: usize,
    pub failed_relays: Vec<String>,
    pub threads: Vec<DmThreadSummary>,
}

/// Result of sending a NIP-17 DM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmSendReport {
    pub message: DmMessage,
    pub relays_accepted: usize,
    pub relays_failed: usize,
}

/// Pure SQLCipher store for decrypted NIP-17 direct messages.
pub struct DmStore {
    conn: Connection,
}

impl DmStore {
    /// Open (or create) the SQLCipher-encrypted DM database at `path`.
    pub fn open(path: &Path, passphrase: &str) -> Result<Self, String> {
        if passphrase.is_empty() {
            return Err("DM store passphrase cannot be empty".to_string());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create DM store directory: {err}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                if let Err(err) =
                    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                {
                    tracing::warn!(%err, "failed to set 0700 permissions on DM store directory");
                }
            }
        }
        let conn = Connection::open(path)
            .map_err(|err| format!("failed to open DM SQLCipher database: {err}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if let Err(err) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            {
                tracing::warn!(%err, "failed to set 0600 permissions on DM SQLCipher database");
            }
        }
        let escaped = passphrase.replace('\'', "''");
        conn.execute_batch(&format!(
            "PRAGMA key = '{escaped}';
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS dm_messages (
                 rumor_id TEXT PRIMARY KEY,
                 wrap_id TEXT,
                 peer_pubkey TEXT NOT NULL,
                 sender_pubkey TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 content TEXT NOT NULL,
                 outgoing INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_dm_peer_time
                 ON dm_messages(peer_pubkey, created_at ASC);"
        ))
        .map_err(|err| format!("failed to unlock or initialize DM SQLCipher database: {err}"))?;
        Ok(Self { conn })
    }

    /// Insert a decrypted message idempotently by `rumor_id`. Returns `true` if newly inserted.
    pub fn insert(&self, msg: &DmMessage, wrap_id: Option<&str>) -> Result<bool, String> {
        let rows = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO dm_messages
                 (rumor_id, wrap_id, peer_pubkey, sender_pubkey, created_at, content, outgoing)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    msg.rumor_id,
                    wrap_id,
                    msg.peer_pubkey_hex,
                    msg.sender_pubkey_hex,
                    msg.created_at as i64,
                    msg.content,
                    if msg.outgoing { 1i64 } else { 0i64 },
                ],
            )
            .map_err(|err| format!("failed to store DM message: {err}"))?;
        Ok(rows > 0)
    }

    /// List all DM threads ordered by most recent message first.
    pub fn list_threads(&self) -> Result<Vec<DmThreadSummary>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT peer_pubkey,
                        (SELECT content FROM dm_messages m2
                         WHERE m2.peer_pubkey = m1.peer_pubkey
                         ORDER BY m2.created_at DESC, m2.rumor_id DESC LIMIT 1) AS last_msg,
                        MAX(created_at) AS last_at,
                        COUNT(*) AS cnt
                 FROM dm_messages m1
                 GROUP BY peer_pubkey
                 ORDER BY last_at DESC",
            )
            .map_err(|err| format!("failed to query DM threads: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                let peer_hex: String = row.get(0)?;
                let last_message: String = row.get(1)?;
                let last_at: i64 = row.get(2)?;
                let count: i64 = row.get(3)?;
                Ok((
                    peer_hex,
                    last_message,
                    last_at.max(0) as u64,
                    count.max(0) as usize,
                ))
            })
            .map_err(|err| format!("failed to read DM threads: {err}"))?;

        let mut threads = Vec::new();
        for row in rows {
            let (peer_pubkey_hex, last_message, last_created_at, message_count) =
                row.map_err(|err| format!("failed to decode DM thread row: {err}"))?;
            let peer_npub = PublicKey::from_hex(&peer_pubkey_hex)
                .ok()
                .and_then(|pk| pk.to_bech32().ok())
                .unwrap_or_else(|| peer_pubkey_hex.clone());
            threads.push(DmThreadSummary {
                peer_pubkey_hex,
                peer_npub,
                last_message,
                last_created_at,
                message_count,
            });
        }
        Ok(threads)
    }

    /// List all messages for one peer in chronological reading order.
    pub fn messages_for_peer(&self, peer_pubkey_hex: &str) -> Result<Vec<DmMessage>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rumor_id, peer_pubkey, sender_pubkey, created_at, content, outgoing
                 FROM dm_messages
                 WHERE peer_pubkey = ?1
                 ORDER BY created_at ASC, rumor_id ASC",
            )
            .map_err(|err| format!("failed to query DM messages: {err}"))?;
        let rows = stmt
            .query_map(params![peer_pubkey_hex], |row| {
                let created_at: i64 = row.get(3)?;
                let outgoing: i64 = row.get(5)?;
                Ok(DmMessage {
                    rumor_id: row.get(0)?,
                    peer_pubkey_hex: row.get(1)?,
                    sender_pubkey_hex: row.get(2)?,
                    created_at: created_at.max(0) as u64,
                    content: row.get(4)?,
                    outgoing: outgoing != 0,
                })
            })
            .map_err(|err| format!("failed to read DM messages: {err}"))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| format!("failed to decode DM message row: {err}"))?);
        }
        Ok(out)
    }
}

/// Parse a recipient public key (`npub1…` or 64-char hex) into a `PublicKey`.
pub fn parse_recipient_pubkey(raw: &str) -> Result<PublicKey, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter a recipient npub or 64-character hex public key.".to_string());
    }
    if trimmed.starts_with("nsec1") || trimmed.starts_with("ncryptsec1") {
        return Err(
            "That looks like a secret key, not a public key. Nothing was sent.".to_string(),
        );
    }
    PublicKey::parse(trimmed)
        .map_err(|_| "Recipient must be a valid npub1… or 64-character hex pubkey.".to_string())
}

/// Build a NIP-17 `kind: 14` rumor and wrap it as a NIP-59 `kind: 1059` Gift Wrap
/// for `receiver` using NIP-44 v2 encryption.
pub fn wrap_nip17_dm(
    sender_keys: &Keys,
    receiver: &PublicKey,
    content: &str,
) -> Result<(Event, String, u64), String> {
    let mut rumor = EventBuilder::new(Kind::PrivateDirectMessage, content)
        .tag(Tag::public_key(*receiver))
        .finalize_unsigned(sender_keys.public_key());
    rumor.ensure_id();
    let rumor_id = rumor.id.map(|id| id.to_hex()).unwrap_or_default();
    let created_at = rumor.created_at.as_secs();
    let wrap = nip59::GiftWrapBuilder::new(*receiver, rumor)
        .finalize(sender_keys)
        .map_err(|err| format!("NIP-59 gift wrap failed: {err}"))?;
    Ok((wrap, rumor_id, created_at))
}

/// Unwrap one NIP-59 `kind: 1059` Gift Wrap using `my_keys` and extract a
/// verified NIP-17 `kind: 14` direct message.
pub fn unwrap_nip17_dm(my_keys: &Keys, wrap: &Event) -> Result<Option<DmMessage>, String> {
    if wrap.kind != Kind::GiftWrap {
        return Ok(None);
    }
    let unwrapped = nip59::extract_rumor(my_keys, wrap)
        .map_err(|err| format!("NIP-59 unwrap failed: {err}"))?;
    let rumor = unwrapped.rumor;
    if rumor.kind != Kind::PrivateDirectMessage {
        return Ok(None);
    }
    let my_hex = my_keys.public_key().to_hex();
    let sender_hex = unwrapped.sender.to_hex();
    let outgoing = sender_hex == my_hex;
    let peer_pubkey_hex = if outgoing {
        rumor
            .tags
            .public_keys()
            .next()
            .map(|pk| pk.to_hex())
            .unwrap_or_else(|| my_hex.clone())
    } else {
        sender_hex.clone()
    };
    let rumor_id = rumor
        .id
        .map(|id| id.to_hex())
        .unwrap_or_else(|| wrap.id.to_hex());
    Ok(Some(DmMessage {
        rumor_id,
        peer_pubkey_hex,
        sender_pubkey_hex: sender_hex,
        created_at: rumor.created_at.as_secs(),
        content: rumor.content,
        outgoing,
    }))
}

fn dm_db_path() -> PathBuf {
    crate::resolve_data_dir()
        .join("nostr-dm")
        .join("messages.db")
}

fn is_public_wss_relay(url: &str) -> bool {
    let trimmed = url.trim();
    if !trimmed.starts_with("wss://") {
        return false;
    }
    let rest = &trimmed["wss://".len()..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host_raw = if let Some(after_open) = authority.strip_prefix('[') {
        after_open.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host_raw.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                !(v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast())
            }
            std::net::IpAddr::V6(v6) => {
                let seg0 = v6.segments()[0];
                let is_ula = (seg0 & 0xfe00) == 0xfc00;
                let is_link_local = (seg0 & 0xffc0) == 0xfe80;
                !(v6.is_loopback() || v6.is_unspecified() || is_ula || is_link_local)
            }
        };
    }
    true
}

async fn load_keys_and_passphrase() -> Result<(Keys, Zeroizing<String>), String> {
    let ncryptsec_bytes = hkask_keystore::Keychain
        .retrieve_by_url_async(NOSTR_NCRYPTSEC_CREDENTIAL_URL)
        .await
        .map_err(|_| {
            "No Nostr secret key is saved in the keychain. Save your nsec in Settings → Kask → Nostr."
                .to_string()
        })?;
    let password_bytes = hkask_keystore::Keychain
        .retrieve_by_url_async(NOSTR_PASSWORD_CREDENTIAL_URL)
        .await
        .map_err(|_| {
            "No Nostr encryption password is saved in the keychain. Save your key in Settings → Kask → Nostr."
                .to_string()
        })?;
    let ncryptsec = Zeroizing::new(
        String::from_utf8(ncryptsec_bytes.to_vec())
            .map_err(|_| "Stored Nostr ncryptsec is not valid UTF-8.".to_string())?,
    );
    let password = Zeroizing::new(
        String::from_utf8(password_bytes.to_vec())
            .map_err(|_| "Stored Nostr password is not valid UTF-8.".to_string())?,
    );
    if ncryptsec.is_empty() || password.is_empty() {
        return Err(
            "Nostr keychain entries are empty. Save your key in Settings → Kask → Nostr."
                .to_string(),
        );
    }
    let signer_key =
        hkask_nostr_signer::decrypt_ncryptsec(&ncryptsec, &password).map_err(|_| {
            "Could not decrypt the saved Nostr key with the saved password.".to_string()
        })?;
    let raw_bytes = signer_key.to_zeroizing_bytes();
    let sdk_secret = SdkSecretKey::from_slice(&*raw_bytes)
        .map_err(|_| "Decrypted Nostr key was not valid secp256k1 bytes.".to_string())?;
    drop(raw_bytes);
    drop(signer_key);
    Ok((Keys::new(sdk_secret), password))
}

/// Load locally stored DM threads from the SQLCipher database (zero network calls).
pub async fn load_local_threads() -> Result<Vec<DmThreadSummary>, String> {
    let (_, password) = load_keys_and_passphrase().await?;
    let path = dm_db_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let store = DmStore::open(&path, &password)?;
    store.list_threads()
}

/// Load locally stored messages for a specific peer (zero network calls).
pub async fn load_local_messages(peer_pubkey_hex: String) -> Result<Vec<DmMessage>, String> {
    let (_, password) = load_keys_and_passphrase().await?;
    let path = dm_db_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let store = DmStore::open(&path, &password)?;
    store.messages_for_peer(&peer_pubkey_hex)
}

/// Fetch `kind: 1059` NIP-59 gift wraps for the configured identity from
/// `kask.nostr.seed_relays`, unwrap NIP-17 `kind: 14` messages inside the editor,
/// and store them in the SQLCipher database.
pub async fn sync_dms(cx: &mut AsyncApp) -> Result<DmSyncReport, String> {
    let seed_relays = cx.update(|cx| KaskSettings::get_global(cx).nostr.seed_relays.clone());
    let relays: Vec<String> = seed_relays
        .into_iter()
        .filter(|r| is_public_wss_relay(r))
        .take(8)
        .collect();
    if relays.is_empty() {
        return Err(
            "No usable public wss:// seed relays configured in Settings → Kask → Nostr."
                .to_string(),
        );
    }

    let (keys, password) = load_keys_and_passphrase().await?;
    let client = Client::new();
    let mut failed_relays = Vec::new();
    for relay in &relays {
        if let Err(err) = client.add_relay(relay.as_str()).await {
            failed_relays.push(format!("{relay}: {err}"));
        }
    }
    client.connect().await;

    let filter = Filter::new()
        .kind(Kind::GiftWrap)
        .pubkey(keys.public_key())
        .limit(100);
    let events = client
        .fetch_events(filter)
        .timeout(Duration::from_secs(10))
        .await
        .map_err(|err| format!("Failed to fetch NIP-17 gift wraps: {err}"))?;
    client.disconnect().await;

    let path = dm_db_path();
    let store = DmStore::open(&path, &password)?;
    let fetched_wraps = events.len();
    let mut newly_stored = 0usize;
    for wrap in events.iter() {
        if let Ok(Some(msg)) = unwrap_nip17_dm(&keys, wrap) {
            if store.insert(&msg, Some(&wrap.id.to_hex()))? {
                newly_stored += 1;
            }
        }
    }
    let threads = store.list_threads()?;
    Ok(DmSyncReport {
        fetched_wraps,
        newly_stored,
        failed_relays,
        threads,
    })
}

/// NIP-17 preferred DM inbox relay list kind (`kind: 10050`).
pub const DM_INBOX_RELAYS_KIND: u16 = 10_050;

/// Maximum recipient `kind: 10050` relays contacted per DM send (NIP-17 recommends 1–3).
const MAX_RECIPIENT_DM_RELAYS: usize = 3;

/// Extract up to [`MAX_RECIPIENT_DM_RELAYS`] SSRF-validated public `wss://`
/// relays from the newest NIP-17 `kind: 10050` event in `events`.
pub fn extract_dm_inbox_relays(events: &[Event]) -> Vec<String> {
    let Some(newest) = events
        .iter()
        .filter(|ev| ev.kind == Kind::Custom(DM_INBOX_RELAYS_KIND))
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        })
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for tag in newest.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("relay") {
            continue;
        }
        let Some(raw) = parts.get(1).map(|s| s.trim()) else {
            continue;
        };
        if !is_public_wss_relay(raw) {
            continue;
        }
        let normalized = raw.trim_end_matches('/').to_string();
        if !out
            .iter()
            .any(|seen: &String| seen.eq_ignore_ascii_case(&normalized))
        {
            out.push(normalized);
            if out.len() == MAX_RECIPIENT_DM_RELAYS {
                break;
            }
        }
    }
    out
}

/// Look up a recipient's NIP-17 `kind: 10050` DM inbox relays using the
/// operator's configured `seed_relays` (bounded 5s read; SSRF-filtered).
async fn discover_recipient_dm_relays(recipient: PublicKey, seed_relays: &[String]) -> Vec<String> {
    let client = Client::new();
    for relay in seed_relays {
        let _ = client.add_relay(relay.as_str()).await;
    }
    client.connect().await;
    let filter = Filter::new()
        .kind(Kind::Custom(DM_INBOX_RELAYS_KIND))
        .author(recipient)
        .limit(4);
    let events = client
        .fetch_events(filter)
        .timeout(Duration::from_secs(5))
        .await;
    client.disconnect().await;
    match events {
        Ok(list) => {
            let collected: Vec<Event> = list.into_iter().collect();
            extract_dm_inbox_relays(&collected)
        }
        Err(err) => {
            tracing::debug!(%err, "NIP-17 kind:10050 discovery failed; falling back to seed relays");
            Vec::new()
        }
    }
}

/// Send a NIP-17 encrypted DM (NIP-44 v2 + NIP-59 Gift Wrap) after mandatory
/// native operator approval (`window.prompt`, never bypassed by `autopublish`).
pub async fn send_dm(
    recipient_raw: String,
    content: String,
    cx: &mut AsyncApp,
) -> Result<DmSendReport, String> {
    let recipient = parse_recipient_pubkey(&recipient_raw)?;
    if content.trim().is_empty() {
        return Err("Write a message before sending.".to_string());
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(format!(
            "Message is {} characters; maximum is {MAX_CONTENT_CHARS}.",
            content.chars().count()
        ));
    }

    let seed_relays = cx.update(|cx| KaskSettings::get_global(cx).nostr.seed_relays.clone());
    let relays: Vec<String> = seed_relays
        .into_iter()
        .filter(|r| is_public_wss_relay(r))
        .take(8)
        .collect();
    if relays.is_empty() {
        return Err(
            "No usable public wss:// seed relays configured in Settings → Kask → Nostr."
                .to_string(),
        );
    }

    // Discover recipient's NIP-17 kind:10050 DM inbox relays before prompting
    // so the operator sees and approves every exact delivery target.
    let inbox_relays = discover_recipient_dm_relays(recipient, &relays).await;
    let (recipient_relays, relay_label) = if !inbox_relays.is_empty() {
        (inbox_relays, "Recipient NIP-17 inbox relays (kind:10050)")
    } else {
        (
            relays.clone(),
            "Seed relays (fallback — recipient has no kind:10050 list on seed relays)",
        )
    };

    // Mandatory operator confirmation before touching the keychain or signing.
    let window = cx
        .update(|cx| {
            cx.active_window()
                .or_else(|| cx.windows().into_iter().next())
        })
        .ok_or_else(|| {
            "Sending a NIP-17 DM requires an open editor window for approval.".to_string()
        })?;
    let recipient_npub = recipient.to_bech32().unwrap_or_else(|_| recipient.to_hex());
    let prompt_msg =
        format!("Send encrypted NIP-17 DM (NIP-44 + NIP-59 Gift Wrap) to {recipient_npub}?");
    let detail = format!(
        "{relay_label}: {}\n\n{}",
        recipient_relays.join(", "),
        content
    );
    let answer = cx.update_window(window, |_, window, cx| {
        window.prompt(
            PromptLevel::Warning,
            &prompt_msg,
            Some(&detail),
            &["Send DM", "Cancel"],
            cx,
        )
    });
    let approved = match answer {
        Ok(rx) => matches!(rx.await, Ok(0)),
        Err(_) => false,
    };
    if !approved {
        return Err("Cancelled: encrypted DM was not signed or sent.".to_string());
    }

    let (keys, password) = load_keys_and_passphrase().await?;
    let (recipient_wrap, rumor_id, created_at) = wrap_nip17_dm(&keys, &recipient, &content)?;
    // Self-copy wrap so outgoing messages also sync back across devices per NIP-17.
    let self_rumor = EventBuilder::new(Kind::PrivateDirectMessage, &content)
        .tag(Tag::public_key(recipient))
        .finalize_unsigned(keys.public_key());
    let self_wrap = nip59::GiftWrapBuilder::new(keys.public_key(), self_rumor)
        .finalize(&keys)
        .map_err(|err| format!("NIP-59 self-copy gift wrap failed: {err}"))?;

    let recipient_client = Client::new();
    for relay in &recipient_relays {
        if let Err(err) = recipient_client.add_relay(relay.as_str()).await {
            tracing::warn!(%relay, %err, "NIP-17 DM: failed to add recipient relay");
        }
    }
    recipient_client.connect().await;
    let send_out = recipient_client
        .send_event(&recipient_wrap)
        .await
        .map_err(|err| format!("Failed to send NIP-17 gift wrap: {err}"))?;
    recipient_client.disconnect().await;

    // Publish self-copy wrap to the operator's own seed relays.
    let self_client = Client::new();
    for relay in &relays {
        let _ = self_client.add_relay(relay.as_str()).await;
    }
    self_client.connect().await;
    if let Err(err) = self_client.send_event(&self_wrap).await {
        tracing::warn!(%err, "NIP-17 DM: self-copy gift wrap send failed");
    }
    self_client.disconnect().await;

    let relays_accepted = send_out.success.len();
    let relays_failed = send_out.failed.len();
    if relays_accepted == 0 {
        return Err(format!(
            "No relay accepted the encrypted DM ({relays_failed} failed)."
        ));
    }

    let message = DmMessage {
        rumor_id,
        peer_pubkey_hex: recipient.to_hex(),
        sender_pubkey_hex: keys.public_key().to_hex(),
        created_at,
        content,
        outgoing: true,
    };
    let store = DmStore::open(&dm_db_path(), &password)?;
    store.insert(&message, Some(&recipient_wrap.id.to_hex()))?;

    Ok(DmSendReport {
        message,
        relays_accepted,
        relays_failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nip17_gift_wrap_round_trips_for_recipient_and_fails_for_third_party() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let eve = Keys::generate();

        let (wrap, rumor_id, _created_at) =
            wrap_nip17_dm(&alice, &bob.public_key(), "Meet at dawn (NIP-17)")
                .expect("gift wrap succeeds");
        assert_eq!(wrap.kind, Kind::GiftWrap);
        assert_ne!(
            wrap.pubkey,
            alice.public_key(),
            "ephemeral wrap pubkey hides sender"
        );

        let bob_msg = unwrap_nip17_dm(&bob, &wrap)
            .expect("bob unwraps")
            .expect("yields kind:14 DM");
        assert_eq!(bob_msg.rumor_id, rumor_id);
        assert_eq!(bob_msg.content, "Meet at dawn (NIP-17)");
        assert_eq!(bob_msg.sender_pubkey_hex, alice.public_key().to_hex());
        assert_eq!(bob_msg.peer_pubkey_hex, alice.public_key().to_hex());
        assert!(!bob_msg.outgoing);

        assert!(
            unwrap_nip17_dm(&eve, &wrap).is_err(),
            "unrelated third party must not decrypt NIP-44/NIP-59 gift wrap"
        );
    }

    #[test]
    fn dm_store_encrypts_at_rest_with_sqlcipher_and_lists_threads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("messages.db");
        let store = DmStore::open(&db_path, "strong-test-passphrase").expect("opens sqlcipher");
        let peer = Keys::generate().public_key().to_hex();
        let me = Keys::generate().public_key().to_hex();

        let m1 = DmMessage {
            rumor_id: "r1".into(),
            peer_pubkey_hex: peer.clone(),
            sender_pubkey_hex: peer.clone(),
            created_at: 100,
            content: "hello".into(),
            outgoing: false,
        };
        let m2 = DmMessage {
            rumor_id: "r2".into(),
            peer_pubkey_hex: peer.clone(),
            sender_pubkey_hex: me,
            created_at: 200,
            content: "hi back".into(),
            outgoing: true,
        };
        assert!(store.insert(&m1, Some("w1")).expect("insert m1"));
        assert!(!store.insert(&m1, Some("w1")).expect("idempotent m1"));
        assert!(store.insert(&m2, Some("w2")).expect("insert m2"));

        let threads = store.list_threads().expect("threads");
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].message_count, 2);
        assert_eq!(threads[0].last_message, "hi back");

        let msgs = store.messages_for_peer(&peer).expect("messages");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].content, "hi back");
        drop(store);

        // Opening with the wrong passphrase must fail.
        assert!(DmStore::open(&db_path, "wrong-passphrase").is_err());
    }

    #[test]
    fn parse_recipient_pubkey_rejects_secret_keys() {
        let err =
            parse_recipient_pubkey("nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq")
                .expect_err("nsec must be refused");
        assert!(err.contains("secret key"));
    }

    #[test]
    fn is_public_wss_relay_blocks_private_loopback_and_non_wss() {
        assert!(is_public_wss_relay("wss://relay.primal.net"));
        assert!(!is_public_wss_relay("ws://relay.primal.net"));
        assert!(!is_public_wss_relay("wss://127.0.0.1:43301"));
        assert!(!is_public_wss_relay("wss://10.0.0.1"));
        assert!(!is_public_wss_relay("wss://192.168.1.5"));
        assert!(!is_public_wss_relay("wss://169.254.169.254"));
        assert!(!is_public_wss_relay("wss://[fd00::1]"));
        assert!(!is_public_wss_relay("wss://user:pass@relay.primal.net"));
    }

    #[test]
    fn extract_dm_inbox_relays_picks_newest_filters_ssrf_dedups_and_caps_at_three() {
        let keys = Keys::generate();
        let older = EventBuilder::new(Kind::Custom(DM_INBOX_RELAYS_KIND), "")
            .custom_created_at(nostr_sdk::prelude::Timestamp::from_secs(1_700_000_000))
            .tag(Tag::parse(["relay", "wss://old.example.com"]).expect("tag"))
            .finalize(&keys)
            .expect("sign older");
        let newer = EventBuilder::new(Kind::Custom(DM_INBOX_RELAYS_KIND), "")
            .custom_created_at(nostr_sdk::prelude::Timestamp::from_secs(1_700_000_500))
            .tag(Tag::parse(["relay", "ws://insecure.example.com"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://127.0.0.1:7447"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://10.0.0.5"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://inbox1.example.com/"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://INBOX1.example.com"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://inbox2.example.com"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://inbox3.example.com"]).expect("tag"))
            .tag(Tag::parse(["relay", "wss://inbox4.example.com"]).expect("tag"))
            .finalize(&keys)
            .expect("sign newer");

        let extracted = extract_dm_inbox_relays(&[older, newer]);
        assert_eq!(
            extracted,
            vec![
                "wss://inbox1.example.com".to_string(),
                "wss://inbox2.example.com".to_string(),
                "wss://inbox3.example.com".to_string(),
            ]
        );
    }
}
