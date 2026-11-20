//! BUD-11 `Authorization: Nostr <base64>` verification over `kind: 24242`
//! events using `hkask-nostr-core` (BIP-340 Schnorr verification only; never
//! holds or links any signing key).

use std::collections::BTreeSet;

use base64::Engine as _;
use hkask_nostr_core::Event;

/// Maximum clock skew allowed for `created_at` in the future (60 seconds).
pub const CLOCK_SKEW_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthAction {
    Upload,
    Delete,
}

impl AuthAction {
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedAuth {
    pub pubkey: [u8; 32],
    pub pubkey_hex: String,
    pub x_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingHeader,
    InvalidScheme,
    InvalidBase64,
    InvalidEvent,
    WrongKind,
    FutureCreatedAt,
    MissingExpiration,
    InvalidExpiration,
    Expired,
    WrongAction,
    MissingSha256Tag,
    Sha256Mismatch,
    ServerMismatch,
    AuthorNotAllowed,
}

impl AuthError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::MissingHeader => "missing Authorization header",
            Self::InvalidScheme => "Authorization header must use Nostr scheme",
            Self::InvalidBase64 => "Authorization Nostr payload is not valid base64",
            Self::InvalidEvent => "Authorization event failed NIP-01 or Schnorr verification",
            Self::WrongKind => "Authorization event must be kind 24242",
            Self::FutureCreatedAt => "Authorization event created_at is in the future",
            Self::MissingExpiration => "Authorization event is missing an expiration tag",
            Self::InvalidExpiration => {
                "Authorization event has a malformed or duplicate expiration tag"
            }
            Self::Expired => "Authorization event has expired",
            Self::WrongAction => "Authorization event t tag does not match this endpoint",
            Self::MissingSha256Tag => "Authorization event is missing an x sha256 tag",
            Self::Sha256Mismatch => "Authorization event x tag does not match the blob sha256",
            Self::ServerMismatch => "Authorization event server tag does not match this server",
            Self::AuthorNotAllowed => "author is not on the operator's allowed_authors list",
        }
    }
}

fn decode_base64_any(encoded: &str) -> Option<Vec<u8>> {
    let trimmed = encoded.trim();
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(trimmed))
        .ok()
}

pub fn is_valid_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn normalize_server_domain(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    authority.trim_end_matches('.').to_string()
}

/// Verify a BUD-11 `Authorization` header against `action`, `now_secs`,
/// `allowed_authors`, an optional `expected_sha256` (required for `Delete`
/// and `HEAD /upload`), and `admitted_servers` (matched whenever the auth
/// event carries one or more `["server", ...]` tags).
pub fn verify_blossom_auth(
    auth_header: Option<&str>,
    action: AuthAction,
    now_secs: u64,
    allowed_authors: &BTreeSet<[u8; 32]>,
    expected_sha256: Option<&str>,
    admitted_servers: &[&str],
) -> Result<VerifiedAuth, AuthError> {
    let raw = auth_header.ok_or(AuthError::MissingHeader)?;
    let encoded = raw
        .trim()
        .strip_prefix("Nostr ")
        .ok_or(AuthError::InvalidScheme)?;
    let decoded = decode_base64_any(encoded).ok_or(AuthError::InvalidBase64)?;
    let json_text = String::from_utf8(decoded).map_err(|_| AuthError::InvalidEvent)?;
    let event = Event::from_json(&json_text).map_err(|_| AuthError::InvalidEvent)?;

    if event.kind() != 24_242 {
        return Err(AuthError::WrongKind);
    }
    if event.created_at() > now_secs.saturating_add(CLOCK_SKEW_SECS) {
        return Err(AuthError::FutureCreatedAt);
    }

    let mut has_action = false;
    let mut expiration: Option<u64> = None;
    let mut x_hashes: Vec<String> = Vec::new();
    let mut server_tags: Vec<String> = Vec::new();

    for tag in event.tags() {
        if tag.len() < 2 {
            continue;
        }
        match tag[0].as_str() {
            "t" => {
                if tag[1] != action.as_tag() {
                    return Err(AuthError::WrongAction);
                }
                has_action = true;
            }
            "expiration" => {
                if expiration.is_some() {
                    return Err(AuthError::InvalidExpiration);
                }
                let exp = tag[1]
                    .parse::<u64>()
                    .map_err(|_| AuthError::InvalidExpiration)?;
                expiration = Some(exp);
            }
            "x" => {
                let lower = tag[1].to_ascii_lowercase();
                if is_valid_sha256_hex(&lower) && !x_hashes.contains(&lower) {
                    x_hashes.push(lower);
                }
            }
            "server" => {
                let normalized = normalize_server_domain(&tag[1]);
                if normalized.is_empty() {
                    return Err(AuthError::ServerMismatch);
                }
                if !server_tags.contains(&normalized) {
                    server_tags.push(normalized);
                }
            }
            _ => {}
        }
    }

    let exp = expiration.ok_or(AuthError::MissingExpiration)?;
    if exp <= now_secs {
        return Err(AuthError::Expired);
    }
    if !has_action {
        return Err(AuthError::WrongAction);
    }
    if !server_tags.is_empty() {
        let matches_server = server_tags.iter().any(|tag_domain| {
            admitted_servers
                .iter()
                .map(|adm| normalize_server_domain(adm))
                .any(|adm| !adm.is_empty() && &adm == tag_domain)
        });
        if !matches_server {
            return Err(AuthError::ServerMismatch);
        }
    }
    if x_hashes.is_empty() {
        return Err(AuthError::MissingSha256Tag);
    }
    if let Some(expected) = expected_sha256
        && !x_hashes.iter().any(|h| h == expected)
    {
        return Err(AuthError::Sha256Mismatch);
    }
    if !allowed_authors.contains(event.pubkey()) {
        return Err(AuthError::AuthorNotAllowed);
    }

    Ok(VerifiedAuth {
        pubkey: *event.pubkey(),
        pubkey_hex: hex::encode(event.pubkey()),
        x_hashes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_nostr_core::event_id;
    use k256::schnorr::SigningKey;
    use k256::schnorr::signature::hazmat::PrehashSigner;

    #[test]
    fn accepts_standard_and_url_safe_base64_and_enforces_allowlist() {
        let signing_key = SigningKey::from_bytes(&[7u8; 32].into()).expect("valid test key");
        let pubkey: [u8; 32] = signing_key.verifying_key().to_bytes().into();
        let sha = "a".repeat(64);
        let now = 1_700_000_100u64;
        let tags = vec![
            vec!["t".to_string(), "upload".to_string()],
            vec!["expiration".to_string(), (now + 600).to_string()],
            vec!["x".to_string(), sha.clone()],
        ];
        let id = event_id(&pubkey, now - 10, 24_242, &tags, "Upload");
        let sig = signing_key.sign_prehash(&id).expect("signs");
        let event = Event::from_parts(
            pubkey,
            now - 10,
            24_242,
            tags,
            "Upload".to_string(),
            sig.to_bytes(),
        )
        .expect("verified");
        let json = event.to_json();

        let allowed = BTreeSet::from([pubkey]);
        let std_hdr = format!(
            "Nostr {}",
            base64::engine::general_purpose::STANDARD.encode(&json)
        );
        let url_hdr = format!(
            "Nostr {}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&json)
        );

        assert!(
            verify_blossom_auth(
                Some(&std_hdr),
                AuthAction::Upload,
                now,
                &allowed,
                Some(&sha),
                &["127.0.0.1:8080"],
            )
            .is_ok()
        );
        assert!(
            verify_blossom_auth(
                Some(&url_hdr),
                AuthAction::Upload,
                now,
                &allowed,
                Some(&sha),
                &["127.0.0.1:8080"],
            )
            .is_ok()
        );

        let empty_allowlist = BTreeSet::new();
        assert_eq!(
            verify_blossom_auth(
                Some(&std_hdr),
                AuthAction::Upload,
                now,
                &empty_allowlist,
                Some(&sha),
                &["127.0.0.1:8080"],
            )
            .unwrap_err(),
            AuthError::AuthorNotAllowed
        );
        assert_eq!(
            verify_blossom_auth(
                Some(&std_hdr),
                AuthAction::Upload,
                now + 700,
                &allowed,
                Some(&sha),
                &["127.0.0.1:8080"],
            )
            .unwrap_err(),
            AuthError::Expired
        );
    }

    #[test]
    fn enforces_server_tags_single_action_and_single_valid_expiration() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32].into()).expect("valid test key");
        let pubkey: [u8; 32] = signing_key.verifying_key().to_bytes().into();
        let allowed = BTreeSet::from([pubkey]);
        let sha = "b".repeat(64);
        let now = 1_700_000_100u64;

        let make_hdr = |tags: Vec<Vec<String>>| -> String {
            let id = event_id(&pubkey, now - 5, 24_242, &tags, "Auth");
            let sig = signing_key.sign_prehash(&id).expect("signs");
            let ev =
                Event::from_parts(pubkey, now - 5, 24_242, tags, "Auth".into(), sig.to_bytes())
                    .expect("valid event");
            format!(
                "Nostr {}",
                base64::engine::general_purpose::STANDARD.encode(ev.to_json())
            )
        };

        // Mismatched server tag is rejected.
        let foreign = make_hdr(vec![
            vec!["t".into(), "upload".into()],
            vec!["expiration".into(), (now + 300).to_string()],
            vec!["x".into(), sha.clone()],
            vec!["server".into(), "blossom.other.example".into()],
        ]);
        assert_eq!(
            verify_blossom_auth(
                Some(&foreign),
                AuthAction::Upload,
                now,
                &allowed,
                Some(&sha),
                &["127.0.0.1:18080", "media.example.com"],
            )
            .unwrap_err(),
            AuthError::ServerMismatch
        );

        // Matching server tag (case-insensitive / URL-form) is accepted.
        let matching = make_hdr(vec![
            vec!["t".into(), "upload".into()],
            vec!["expiration".into(), (now + 300).to_string()],
            vec!["x".into(), sha.clone()],
            vec!["server".into(), "HTTPS://Media.Example.Com/".into()],
        ]);
        assert!(
            verify_blossom_auth(
                Some(&matching),
                AuthAction::Upload,
                now,
                &allowed,
                Some(&sha),
                &["127.0.0.1:18080", "media.example.com"],
            )
            .is_ok()
        );

        // Multi-verb t tags (upload + delete) are rejected.
        let multi_verb = make_hdr(vec![
            vec!["t".into(), "upload".into()],
            vec!["t".into(), "delete".into()],
            vec!["expiration".into(), (now + 300).to_string()],
            vec!["x".into(), sha.clone()],
        ]);
        assert_eq!(
            verify_blossom_auth(
                Some(&multi_verb),
                AuthAction::Upload,
                now,
                &allowed,
                Some(&sha),
                &["127.0.0.1:18080"],
            )
            .unwrap_err(),
            AuthError::WrongAction
        );

        // Duplicate expiration tags (expired followed by far-future) are rejected.
        let dup_exp = make_hdr(vec![
            vec!["t".into(), "upload".into()],
            vec!["expiration".into(), "1".into()],
            vec!["expiration".into(), (now + 9999).to_string()],
            vec!["x".into(), sha.clone()],
        ]);
        assert_eq!(
            verify_blossom_auth(
                Some(&dup_exp),
                AuthAction::Upload,
                now,
                &allowed,
                Some(&sha),
                &["127.0.0.1:18080"],
            )
            .unwrap_err(),
            AuthError::InvalidExpiration
        );
    }
}
