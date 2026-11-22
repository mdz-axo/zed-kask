//! NIP-46 Nostr Connect (`bunker://`) remote signer client (`kind: 24133`, NIP-44).
//!
//! Runs exclusively inside the editor process (`kask_bridge`) behind the exact
//! same native operator approval prompt as local NIP-49 keychain signing.
//!
//! Security invariants:
//! - The `bunker://` URI (which may contain a `secret=` token) lives exclusively
//!   in `NOSTR_BUNKER_URI_CREDENTIAL_URL` in the OS keychain, never in
//!   `settings.json` and never delivered to any MCP server.
//! - Relay URLs inside the `bunker://` URI are strictly validated as public
//!   `wss://` endpoints with no embedded credentials, loopback, or private IPs.
//! - Every signed event returned by the remote bunker is verified locally via
//!   `hkask_nostr_core::Event::from_json` (NIP-01 ID + BIP-340 Schnorr signature)
//!   AND checked for exact `pubkey`, `kind`, `created_at`, `tags`, and `content`
//!   equality against the operator-approved unsigned event so a compromised
//!   bunker cannot substitute an unapproved payload.

use std::net::IpAddr;
use std::time::Duration;

use futures_util::StreamExt as _;
use nostr::event::FinalizeEvent as _;
use nostr::nips::nip44::{self, Version as Nip44Version};
use nostr_sdk::prelude::{
    Client, ClientNotification, EventBuilder, Filter, Keys, Kind, PublicKey, Tag, Timestamp,
};
use serde_json::{Value, json};
use zeroize::Zeroizing;

/// NIP-46 Nostr Connect RPC event kind.
pub(crate) const NIP46_KIND: u16 = 24_133;

/// Maximum time to wait for the remote bunker to reply to `sign_event`.
const BUNKER_RPC_TIMEOUT: Duration = Duration::from_secs(20);

/// Parsed, SSRF-validated NIP-46 `bunker://` connection configuration.
#[derive(Debug, Clone)]
pub(crate) struct BunkerConfig {
    pub(crate) remote_signer_pubkey: PublicKey,
    pub(crate) relays: Vec<String>,
    pub(crate) secret: Option<Zeroizing<String>>,
}

/// Unsigned Nostr event approved by the operator for remote signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnsignedApprovedEvent {
    pub(crate) pubkey_hex: String,
    pub(crate) created_at: u64,
    pub(crate) kind: u16,
    pub(crate) tags: Vec<Vec<String>>,
    pub(crate) content: String,
}

impl UnsignedApprovedEvent {
    pub(crate) fn to_unsigned_json(&self) -> String {
        json!({
            "pubkey": self.pubkey_hex,
            "created_at": self.created_at,
            "kind": self.kind,
            "tags": self.tags,
            "content": self.content,
        })
        .to_string()
    }
}

fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Validate a bunker relay URL as a clean public `wss://` endpoint without
/// credentials or private/loopback addresses.
pub(crate) fn validate_public_wss_relay(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("wss://") || trimmed.chars().any(char::is_whitespace) {
        return Err("bunker relay must be a clean wss:// URL".to_string());
    }
    let after_scheme = &trimmed["wss://".len()..];
    let authority = after_scheme.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err("bunker relay must not contain credentials or an empty host".to_string());
    }
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .map(|s| s.trim_start_matches('['))
            .unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let lower_host = host.to_ascii_lowercase();
    if lower_host.is_empty()
        || lower_host == "localhost"
        || lower_host.ends_with(".localhost")
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".internal")
    {
        return Err("bunker relay host must be a public hostname".to_string());
    }
    if let Ok(ip) = lower_host.parse::<IpAddr>()
        && is_disallowed_ip(ip)
    {
        return Err("bunker relay IP must be publicly routable".to_string());
    }
    Ok(trimmed.to_string())
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse and SSRF-validate a NIP-46 `bunker://<remote-pubkey>?relay=wss://...&secret=...` URI.
pub(crate) fn parse_bunker_uri(raw: &str) -> Result<BunkerConfig, String> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix("bunker://")
        .ok_or_else(|| "bunker URI must start with bunker://".to_string())?;
    let (pubkey_part, query_part) = rest.split_once('?').ok_or_else(|| {
        "bunker URI must include at least one ?relay=wss://... parameter".to_string()
    })?;
    let pk_str = pubkey_part.trim().trim_end_matches('/');
    if pk_str.len() != 64 || !pk_str.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("bunker URI remote signer pubkey must be 64 hex characters".to_string());
    }
    let remote_signer_pubkey = PublicKey::from_hex(pk_str)
        .map_err(|_| "bunker URI remote signer pubkey is invalid".to_string())?;

    let mut relays = Vec::new();
    let mut secret = None;
    for pair in query_part.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let decoded = percent_decode(v);
        match k {
            "relay" => {
                let valid = validate_public_wss_relay(&decoded)?;
                if !relays.contains(&valid) && relays.len() < 4 {
                    relays.push(valid);
                }
            }
            "secret" if !decoded.trim().is_empty() => {
                secret = Some(Zeroizing::new(decoded.trim().to_string()));
            }
            _ => {}
        }
    }
    if relays.is_empty() {
        return Err("bunker URI must specify at least one valid public wss:// relay".to_string());
    }
    Ok(BunkerConfig {
        remote_signer_pubkey,
        relays,
        secret,
    })
}

/// Build a NIP-46 JSON-RPC `sign_event` request payload.
pub(crate) fn build_sign_event_rpc(req_id: &str, unsigned: &UnsignedApprovedEvent) -> String {
    json!({
        "id": req_id,
        "method": "sign_event",
        "params": [unsigned.to_unsigned_json()],
    })
    .to_string()
}

/// Parse and cryptographically verify a NIP-46 `sign_event` response against
/// the operator-approved unsigned event.
pub(crate) fn parse_and_verify_sign_event_response(
    expected_req_id: &str,
    plaintext_json: &str,
    approved: &UnsignedApprovedEvent,
) -> Result<hkask_nostr_core::Event, String> {
    let resp: Value = serde_json::from_str(plaintext_json)
        .map_err(|err| format!("bunker returned invalid JSON-RPC payload: {err}"))?;
    let id = resp.get("id").and_then(Value::as_str).unwrap_or("");
    if id != expected_req_id {
        return Err("bunker response id did not match request id".to_string());
    }
    if let Some(err) = resp.get("error").and_then(Value::as_str)
        && !err.trim().is_empty()
    {
        return Err(format!("remote bunker refused signing: {}", err.trim()));
    }
    let result_str = resp
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| "bunker response missing signed event result".to_string())?;
    let signed = hkask_nostr_core::Event::from_json(result_str)
        .map_err(|err| format!("bunker returned an invalidly signed Nostr event: {err}"))?;

    // Strict payload-integrity check: a remote bunker MUST NOT alter pubkey,
    // kind, created_at, tags, or content from what the operator approved.
    let actual_pubkey = signed
        .pubkey()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if !actual_pubkey.eq_ignore_ascii_case(&approved.pubkey_hex) {
        return Err("bunker signed with a pubkey that does not match kask.nostr.npub".to_string());
    }
    if signed.kind() != approved.kind
        || signed.created_at() != approved.created_at
        || signed.tags() != approved.tags.as_slice()
        || signed.content() != approved.content
    {
        return Err(
            "bunker returned a signed event whose fields do not match the approved payload"
                .to_string(),
        );
    }
    Ok(signed)
}

/// Sign an operator-approved unsigned event through a NIP-46 remote bunker
/// (`kind: 24133` NIP-44 encrypted RPC over the bunker's public `wss://` relays).
pub(crate) async fn sign_event_via_bunker(
    bunker_uri: &str,
    approved: &UnsignedApprovedEvent,
) -> Result<hkask_nostr_core::Event, String> {
    let config = parse_bunker_uri(bunker_uri)?;
    let client_keys = Keys::generate();
    let client = Client::new();
    for relay in &config.relays {
        client
            .add_relay(relay.as_str())
            .await
            .map_err(|err| format!("failed to add bunker relay: {err}"))?;
    }
    client.connect().await;

    let filter = Filter::new()
        .kind(Kind::Custom(NIP46_KIND))
        .author(config.remote_signer_pubkey)
        .pubkey(client_keys.public_key())
        .since(Timestamp::now() - Duration::from_secs(10));
    client
        .subscribe(filter)
        .await
        .map_err(|err| format!("failed to subscribe to bunker relay: {err}"))?;

    let mut notifications = client.notifications();

    // Send `connect` first when the bunker URI carries a secret token.
    if let Some(secret) = &config.secret {
        let connect_id = uuid::Uuid::new_v4().to_string();
        let connect_rpc = json!({
            "id": connect_id,
            "method": "connect",
            "params": [config.remote_signer_pubkey.to_hex(), secret.as_str()],
        })
        .to_string();
        let encrypted = nip44::encrypt(
            client_keys.secret_key(),
            &config.remote_signer_pubkey,
            connect_rpc,
            Nip44Version::V2,
        )
        .map_err(|err| format!("NIP-44 encrypt failed for bunker connect: {err}"))?;
        let ev = EventBuilder::new(Kind::Custom(NIP46_KIND), encrypted)
            .tag(Tag::public_key(config.remote_signer_pubkey))
            .finalize(&client_keys)
            .map_err(|err| format!("failed to sign NIP-46 connect event: {err}"))?;
        client
            .send_event(&ev)
            .await
            .map_err(|err| format!("failed to send NIP-46 connect event: {err}"))?;
    }

    let req_id = uuid::Uuid::new_v4().to_string();
    let sign_rpc = build_sign_event_rpc(&req_id, approved);
    let encrypted = nip44::encrypt(
        client_keys.secret_key(),
        &config.remote_signer_pubkey,
        sign_rpc,
        Nip44Version::V2,
    )
    .map_err(|err| format!("NIP-44 encrypt failed for bunker sign_event: {err}"))?;
    let req_event = EventBuilder::new(Kind::Custom(NIP46_KIND), encrypted)
        .tag(Tag::public_key(config.remote_signer_pubkey))
        .finalize(&client_keys)
        .map_err(|err| format!("failed to sign NIP-46 request event: {err}"))?;
    client
        .send_event(&req_event)
        .await
        .map_err(|err| format!("failed to send NIP-46 sign_event request: {err}"))?;

    let outcome = tokio::time::timeout(BUNKER_RPC_TIMEOUT, async {
        while let Some(notification) = notifications.next().await {
            if let ClientNotification::Event { event, .. } = notification
                && event.kind == Kind::Custom(NIP46_KIND)
                && event.pubkey == config.remote_signer_pubkey
            {
                let Ok(plaintext) = nip44::decrypt(
                    client_keys.secret_key(),
                    &config.remote_signer_pubkey,
                    &event.content,
                ) else {
                    continue;
                };
                if let Ok(v) = serde_json::from_str::<Value>(&plaintext)
                    && v.get("id").and_then(Value::as_str) == Some(req_id.as_str())
                {
                    return parse_and_verify_sign_event_response(&req_id, &plaintext, approved);
                }
            }
        }
        Err("bunker relay stream closed before returning a signed event".to_string())
    })
    .await;

    client.disconnect().await;

    match outcome {
        Ok(res) => res,
        Err(_) => Err("remote bunker did not respond within 20s".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bunker_uri_valid_and_ssrf_refusals() {
        let pk = "a".repeat(64);
        let uri = format!("bunker://{pk}?relay=wss%3A%2F%2Frelay.nsec.app&secret=tok123");
        let parsed = parse_bunker_uri(&uri).expect("valid bunker URI");
        assert_eq!(parsed.remote_signer_pubkey.to_hex(), pk);
        assert_eq!(parsed.relays, vec!["wss://relay.nsec.app"]);
        assert_eq!(parsed.secret.as_deref().map(String::as_str), Some("tok123"));

        // Refuse ws://, loopback, private IP, credentials, and missing relay
        for bad in [
            format!("https://{pk}?relay=wss://relay.nsec.app"),
            format!("bunker://{pk}?relay=ws://relay.nsec.app"),
            format!("bunker://{pk}?relay=wss://127.0.0.1:7777"),
            format!("bunker://{pk}?relay=wss://localhost:7777"),
            format!("bunker://{pk}?relay=wss://10.0.0.1"),
            format!("bunker://{pk}?relay=wss://user:pass@relay.nsec.app"),
            format!("bunker://{pk}"),
        ] {
            assert!(parse_bunker_uri(&bad).is_err(), "must refuse: {bad}");
        }
    }

    #[test]
    fn nip46_sign_event_round_trip_verifies_signature_and_refuses_tampering() {
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        let remote_key = hkask_nostr_signer::SecretKey::from_nsec(TEST_NSEC).expect("key");
        let pubkey_hex: String = remote_key
            .x_only_public_key()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let approved = UnsignedApprovedEvent {
            pubkey_hex,
            created_at: 1_700_000_123,
            kind: 1,
            tags: Vec::new(),
            content: "hello via nip46".to_string(),
        };
        let signed =
            hkask_nostr_signer::sign_event(&remote_key, 1, approved.created_at, &approved.content)
                .expect("signs");

        // Valid response passes signature + field equality checks
        let ok_resp = json!({
            "id": "req-1",
            "result": signed.to_json(),
        })
        .to_string();
        let verified =
            parse_and_verify_sign_event_response("req-1", &ok_resp, &approved).expect("verifies");
        assert_eq!(verified.content(), "hello via nip46");

        // Tampered content signed by the bunker MUST be refused
        let tampered = hkask_nostr_signer::sign_event(&remote_key, 1, approved.created_at, "evil")
            .expect("signs");
        let bad_resp = json!({
            "id": "req-1",
            "result": tampered.to_json(),
        })
        .to_string();
        assert!(parse_and_verify_sign_event_response("req-1", &bad_resp, &approved).is_err());

        // Mismatched request id MUST be refused
        assert!(parse_and_verify_sign_event_response("req-other", &ok_resp, &approved).is_err());
    }
}
