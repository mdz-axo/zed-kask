//! BIP-340 signing, restricted to kind 1 notes (untagged or carrying validated
//! NIP-92 `imeta` media tags) and scoped BUD-11 `kind: 24242` Blossom upload
//! authorization tokens.

use chacha20poly1305::aead::Generate;
use hkask_nostr_core::{Event, event_id};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::secret::SecretKey;

/// Short text note kind (NIP-01: <https://nostr-nips.com/nip-01>).
pub const NOTE_KIND: u16 = 1;

/// Follow list kind (NIP-02: <https://nostr-nips.com/nip-02>).
pub const FOLLOWS_KIND: u16 = 3;

/// Maximum followed pubkeys in one NIP-02 `kind: 3` contact list.
pub const MAX_FOLLOWS: usize = 2_000;

/// Event deletion request kind (NIP-09: <https://nostr-nips.com/nip-09>).
pub const DELETION_KIND: u16 = 5;

/// Repost kind for `kind: 1` notes (NIP-18: <https://nostr-nips.com/nip-18>).
pub const REPOST_KIND: u16 = 6;

/// Generic repost kind for non-`kind: 1` events (NIP-18: <https://nostr-nips.com/nip-18>).
pub const GENERIC_REPOST_KIND: u16 = 16;

/// Maximum target event IDs in one NIP-09 deletion request.
pub const MAX_DELETION_TARGETS: usize = 20;

/// Reaction kind (NIP-25: <https://nostr-nips.com/nip-25>).
pub const REACTION_KIND: u16 = 7;

/// Comment kind (NIP-22: <https://nostr-nips.com/nip-22>).
pub const COMMENT_KIND: u16 = 1111;

/// Blossom authorization token kind (BUD-11).
pub const BLOSSOM_AUTH_KIND: u16 = 24_242;

/// NIP-57 Lightning zap request kind.
pub const ZAP_REQUEST_KIND: u16 = hkask_nostr_core::ZAP_REQUEST_KIND;

/// NIP-61 Cashu nutzap kind.
pub const NUTZAP_KIND: u16 = hkask_nostr_core::NUTZAP_KIND;

/// Maximum `imeta` attachments signed onto one kind 1 note.
pub const MAX_IMETA_ATTACHMENTS: usize = 4;

/// Maximum Blossom servers scoped into one `kind: 24242` upload token.
pub const MAX_BLOSSOM_SERVERS: usize = 8;

/// Validated input for one NIP-92 `imeta` media tag on a kind 1 note.
///
/// Only `imeta` tags can be built from this struct — never `e`, `p`, `a`, `q`
/// or any reply/mention tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImetaAttachment {
    pub url: String,
    pub mime_type: String,
    pub sha256: String,
    pub size: u64,
    pub dimensions: Option<(u32, u32)>,
    pub fallback_urls: Vec<String>,
}

/// Why nothing was signed. No variant carries key material.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignError {
    /// The kind is not [`NOTE_KIND`]. Refused before the key is used.
    #[error("kind {kind} is not signed here; only kind 1 notes are")]
    KindNotAllowed { kind: u16 },
    /// An `imeta` attachment violated NIP-92's shape invariants.
    #[error("invalid media attachment: {reason}")]
    InvalidAttachment { reason: &'static str },
    /// A Blossom upload auth request violated BUD-11's scoping invariants.
    #[error("invalid Blossom upload authorization: {reason}")]
    InvalidBlossomAuth { reason: &'static str },
    /// A NIP-57 zap request or NIP-61 nutzap violated its protocol invariants.
    #[error("invalid zap payload: {reason}")]
    InvalidZap { reason: &'static str },
    /// A NIP-25 reaction violated its protocol invariants.
    #[error("invalid reaction payload: {reason}")]
    InvalidReaction { reason: &'static str },
    /// A NIP-10 or NIP-22 reply violated its protocol invariants.
    #[error("invalid reply payload: {reason}")]
    InvalidReply { reason: &'static str },
    /// A NIP-09 deletion request violated its protocol invariants.
    #[error("invalid deletion payload: {reason}")]
    InvalidDeletion { reason: &'static str },
    /// A NIP-18 repost violated its protocol invariants.
    #[error("invalid repost payload: {reason}")]
    InvalidRepost { reason: &'static str },
    /// A NIP-02 follow list violated its protocol invariants.
    #[error("invalid follow list payload: {reason}")]
    InvalidFollows { reason: &'static str },
    /// The operating system did not supply random bytes for the signature's
    /// auxiliary randomness.
    #[error("the operating system did not supply random bytes")]
    Randomness,
    /// The curve library produced no signature, or the signature it produced
    /// did not verify. Not reachable for a valid key; present so no path
    /// panics and nothing unverified is returned.
    #[error("signing failed")]
    Signature,
}

fn is_lower_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_clean_http_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() > "https://".len()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn is_clean_server_domain(domain: &str) -> bool {
    !domain.is_empty()
        && !domain.contains("://")
        && !domain.contains('/')
        && !domain
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c.is_ascii_uppercase())
}

fn build_imeta_tag(attachment: &ImetaAttachment) -> Result<Vec<String>, SignError> {
    if !is_clean_http_url(&attachment.url) {
        return Err(SignError::InvalidAttachment {
            reason: "url must be an http(s) URL with no whitespace or control characters",
        });
    }
    if attachment.mime_type.is_empty()
        || !attachment.mime_type.contains('/')
        || attachment
            .mime_type
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(SignError::InvalidAttachment {
            reason: "mime_type must be type/subtype with no whitespace",
        });
    }
    if !is_lower_sha256_hex(&attachment.sha256) {
        return Err(SignError::InvalidAttachment {
            reason: "sha256 must be 64 lowercase hex characters",
        });
    }
    if attachment.size == 0 {
        return Err(SignError::InvalidAttachment {
            reason: "size must be greater than zero",
        });
    }
    if let Some((w, h)) = attachment.dimensions
        && (w == 0 || h == 0)
    {
        return Err(SignError::InvalidAttachment {
            reason: "dimensions must have non-zero width and height",
        });
    }
    for fallback in &attachment.fallback_urls {
        if !is_clean_http_url(fallback) {
            return Err(SignError::InvalidAttachment {
                reason: "fallback url must be an http(s) URL with no whitespace",
            });
        }
    }

    let mut tag = vec![
        "imeta".to_string(),
        format!("url {}", attachment.url),
        format!("m {}", attachment.mime_type),
        format!("x {}", attachment.sha256),
        format!("size {}", attachment.size),
    ];
    if let Some((w, h)) = attachment.dimensions {
        tag.push(format!("dim {w}x{h}"));
    }
    for fallback in &attachment.fallback_urls {
        tag.push(format!("fallback {fallback}"));
    }
    Ok(tag)
}

fn sign_verified_parts(
    key: &SecretKey,
    kind: u16,
    created_at: u64,
    tags: Vec<Vec<String>>,
    content: &str,
) -> Result<Event, SignError> {
    let pubkey = key.x_only_public_key();
    let id = event_id(&pubkey, created_at, kind, &tags, content);
    let aux_rand = Zeroizing::new(<[u8; 32]>::try_generate().map_err(|_| SignError::Randomness)?);
    let signature = sign_with_aux(key, &id, &aux_rand)?;
    // `Event::from_parts` recomputes the id and verifies the signature, so an
    // event that does not verify never leaves this function.
    Event::from_parts(
        pubkey,
        created_at,
        kind,
        tags,
        content.to_string(),
        signature,
    )
    .map_err(|_| SignError::Signature)
}

/// Sign one untagged event with `key` and return it verified.
///
/// Only `kind` 1 is accepted. The event carries no tags, so a reply, a
/// mention or any other tagged note cannot be produced here.
pub fn sign_event(
    key: &SecretKey,
    kind: u16,
    created_at: u64,
    content: &str,
) -> Result<Event, SignError> {
    if kind != NOTE_KIND {
        return Err(SignError::KindNotAllowed { kind });
    }
    sign_verified_parts(key, NOTE_KIND, created_at, Vec::new(), content)
}

/// Target metadata for a NIP-10 quote note (`["q", event_id, relay_hint, pubkey]` + `["p", pubkey]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteTarget {
    pub event_id_hex: String,
    pub author_pubkey_hex: String,
    pub relay_hint: Option<String>,
}

/// Sign one kind 1 note with zero or more validated NIP-92 `imeta` media tags
/// and an optional NIP-10 quote target (`q` + `p` tags).
pub fn sign_note_with_imeta_and_quote(
    key: &SecretKey,
    created_at: u64,
    content: &str,
    attachments: &[ImetaAttachment],
    quote: Option<&QuoteTarget>,
) -> Result<Event, SignError> {
    if attachments.len() > MAX_IMETA_ATTACHMENTS {
        return Err(SignError::InvalidAttachment {
            reason: "at most 4 media attachments may be signed onto one note",
        });
    }
    let mut tags = Vec::with_capacity(attachments.len() + if quote.is_some() { 2 } else { 0 });
    for attachment in attachments {
        tags.push(build_imeta_tag(attachment)?);
    }
    if let Some(quote) = quote {
        if !is_lower_sha256_hex(&quote.event_id_hex) {
            return Err(SignError::InvalidReply {
                reason: "quote event_id_hex must be 64 lowercase hex characters",
            });
        }
        if !is_lower_sha256_hex(&quote.author_pubkey_hex) {
            return Err(SignError::InvalidReply {
                reason: "quote author_pubkey_hex must be 64 lowercase hex characters",
            });
        }
        let relay_hint = quote.relay_hint.as_deref().unwrap_or("").trim();
        if !relay_hint.is_empty() && !relay_hint.starts_with("wss://") {
            return Err(SignError::InvalidReply {
                reason: "quote relay_hint must be empty or start with wss://",
            });
        }
        tags.push(vec![
            "q".to_string(),
            quote.event_id_hex.clone(),
            relay_hint.to_string(),
            quote.author_pubkey_hex.clone(),
        ]);
        tags.push(vec!["p".to_string(), quote.author_pubkey_hex.clone()]);
    }
    sign_verified_parts(key, NOTE_KIND, created_at, tags, content)
}

/// Sign one kind 1 note with zero or more validated NIP-92 `imeta` media tags.
///
/// Only `imeta` tags can be emitted here; replies, mentions, and arbitrary tag
/// names remain impossible.
pub fn sign_note_with_imeta(
    key: &SecretKey,
    created_at: u64,
    content: &str,
    attachments: &[ImetaAttachment],
) -> Result<Event, SignError> {
    sign_note_with_imeta_and_quote(key, created_at, content, attachments, None)
}

/// Sign a NIP-25 `kind: 7` reaction (<https://nostr-nips.com/nip-25>) targeting
/// a specific event and its author.
pub fn sign_reaction(
    key: &SecretKey,
    created_at: u64,
    target_event_id_hex: &str,
    target_pubkey_hex: &str,
    content: &str,
) -> Result<Event, SignError> {
    if !is_lower_sha256_hex(target_event_id_hex) {
        return Err(SignError::InvalidReaction {
            reason: "target_event_id_hex must be 64 lowercase hex characters",
        });
    }
    if !is_lower_sha256_hex(target_pubkey_hex) {
        return Err(SignError::InvalidReaction {
            reason: "target_pubkey_hex must be 64 lowercase hex characters",
        });
    }
    let reaction = content.trim();
    if reaction.is_empty() || reaction.chars().count() > 32 {
        return Err(SignError::InvalidReaction {
            reason: "reaction content must be 1..=32 characters (e.g. '+' or an emoji)",
        });
    }
    let tags = vec![
        vec!["e".to_string(), target_event_id_hex.to_string()],
        vec!["p".to_string(), target_pubkey_hex.to_string()],
    ];
    sign_verified_parts(key, REACTION_KIND, created_at, tags, reaction)
}

/// Target metadata for a NIP-10 (`kind: 1`, <https://nostr-nips.com/nip-10>) or
/// NIP-22 (`kind: 1111`, <https://nostr-nips.com/nip-22>) reply/comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub root_event_id_hex: String,
    pub reply_to_event_id_hex: Option<String>,
    pub target_pubkey_hex: String,
    pub root_kind: u16,
}

/// Sign a reply/comment:
/// - When `target.root_kind == 1`, emits a NIP-10 marked `kind: 1` reply
///   (<https://nostr-nips.com/nip-10>).
/// - For non-`kind: 1` roots (e.g. `kind: 30023` articles), emits a NIP-22
///   `kind: 1111` comment (<https://nostr-nips.com/nip-22>).
pub fn sign_reply(
    key: &SecretKey,
    created_at: u64,
    content: &str,
    target: &ReplyTarget,
) -> Result<Event, SignError> {
    if content.trim().is_empty() {
        return Err(SignError::InvalidReply {
            reason: "reply content cannot be empty",
        });
    }
    if !is_lower_sha256_hex(&target.root_event_id_hex) {
        return Err(SignError::InvalidReply {
            reason: "root_event_id_hex must be 64 lowercase hex characters",
        });
    }
    if !is_lower_sha256_hex(&target.target_pubkey_hex) {
        return Err(SignError::InvalidReply {
            reason: "target_pubkey_hex must be 64 lowercase hex characters",
        });
    }
    if let Some(reply_to) = &target.reply_to_event_id_hex {
        if !is_lower_sha256_hex(reply_to) {
            return Err(SignError::InvalidReply {
                reason: "reply_to_event_id_hex must be 64 lowercase hex characters",
            });
        }
    }

    if target.root_kind == NOTE_KIND {
        let mut tags = vec![vec![
            "e".to_string(),
            target.root_event_id_hex.clone(),
            String::new(),
            "root".to_string(),
        ]];
        if let Some(reply_to) = target
            .reply_to_event_id_hex
            .as_deref()
            .filter(|id| *id != target.root_event_id_hex)
        {
            tags.push(vec![
                "e".to_string(),
                reply_to.to_string(),
                String::new(),
                "reply".to_string(),
            ]);
        }
        tags.push(vec!["p".to_string(), target.target_pubkey_hex.clone()]);
        sign_verified_parts(key, NOTE_KIND, created_at, tags, content)
    } else {
        let parent_id = target
            .reply_to_event_id_hex
            .as_deref()
            .unwrap_or(&target.root_event_id_hex);
        let parent_kind = if target
            .reply_to_event_id_hex
            .as_deref()
            .is_some_and(|id| id != target.root_event_id_hex)
        {
            COMMENT_KIND
        } else {
            target.root_kind
        };
        let tags = vec![
            vec![
                "E".to_string(),
                target.root_event_id_hex.clone(),
                String::new(),
                target.target_pubkey_hex.clone(),
            ],
            vec!["K".to_string(), target.root_kind.to_string()],
            vec!["P".to_string(), target.target_pubkey_hex.clone()],
            vec![
                "e".to_string(),
                parent_id.to_string(),
                String::new(),
                target.target_pubkey_hex.clone(),
            ],
            vec!["k".to_string(), parent_kind.to_string()],
            vec!["p".to_string(), target.target_pubkey_hex.clone()],
        ];
        sign_verified_parts(key, COMMENT_KIND, created_at, tags, content)
    }
}

/// Sign a NIP-09 `kind: 5` event deletion request (<https://nostr-nips.com/nip-09>)
/// targeting one or more event IDs authored by the same key.
///
/// Always requires explicit operator confirmation in the editor (never bypassed
/// by `autopublish`), because NIP-09 deletion requests cannot be undone.
pub fn sign_deletion(
    key: &SecretKey,
    created_at: u64,
    target_event_ids: &[String],
    target_kind: Option<u16>,
    reason: &str,
) -> Result<Event, SignError> {
    if target_event_ids.is_empty() || target_event_ids.len() > MAX_DELETION_TARGETS {
        return Err(SignError::InvalidDeletion {
            reason: "target_event_ids must name between 1 and 20 events",
        });
    }
    let mut tags = Vec::with_capacity(target_event_ids.len() + usize::from(target_kind.is_some()));
    for id in target_event_ids {
        if !is_lower_sha256_hex(id) {
            return Err(SignError::InvalidDeletion {
                reason: "each target event id must be 64 lowercase hex characters",
            });
        }
        tags.push(vec!["e".to_string(), id.clone()]);
    }
    if let Some(kind) = target_kind {
        tags.push(vec!["k".to_string(), kind.to_string()]);
    }
    sign_verified_parts(key, DELETION_KIND, created_at, tags, reason.trim())
}

/// Sign a NIP-18 repost (<https://nostr-nips.com/nip-18>): `kind: 6` when
/// `target_kind == 1`, or `kind: 16` generic repost with a `["k", kind]` tag
/// for any other kind (e.g. `kind: 30023` articles).
pub fn sign_repost(
    key: &SecretKey,
    created_at: u64,
    target_event_id_hex: &str,
    target_pubkey_hex: &str,
    target_kind: u16,
    relay_hint: Option<&str>,
) -> Result<Event, SignError> {
    if !is_lower_sha256_hex(target_event_id_hex) {
        return Err(SignError::InvalidRepost {
            reason: "target_event_id_hex must be 64 lowercase hex characters",
        });
    }
    if !is_lower_sha256_hex(target_pubkey_hex) {
        return Err(SignError::InvalidRepost {
            reason: "target_pubkey_hex must be 64 lowercase hex characters",
        });
    }
    let hint = relay_hint.map(str::trim).unwrap_or("");
    let mut e_tag = vec!["e".to_string(), target_event_id_hex.to_string()];
    if !hint.is_empty() {
        e_tag.push(hint.to_string());
    }
    let mut tags = vec![e_tag, vec!["p".to_string(), target_pubkey_hex.to_string()]];
    let kind = if target_kind == NOTE_KIND {
        REPOST_KIND
    } else {
        tags.push(vec!["k".to_string(), target_kind.to_string()]);
        GENERIC_REPOST_KIND
    };
    sign_verified_parts(key, kind, created_at, tags, "")
}

/// Sign a NIP-02 `kind: 3` follow list (<https://nostr-nips.com/nip-02>).
/// Each pubkey must be 64 lowercase hex characters; duplicates are dropped in
/// order and capped at [`MAX_FOLLOWS`].
pub fn sign_follows(
    key: &SecretKey,
    created_at: u64,
    followed_pubkeys_hex: &[String],
) -> Result<Event, SignError> {
    if followed_pubkeys_hex.len() > MAX_FOLLOWS {
        return Err(SignError::InvalidFollows {
            reason: "follow list exceeds maximum of 2000 pubkeys",
        });
    }
    let mut seen = std::collections::HashSet::with_capacity(followed_pubkeys_hex.len());
    let mut tags = Vec::with_capacity(followed_pubkeys_hex.len());
    for pk in followed_pubkeys_hex {
        if !is_lower_sha256_hex(pk) {
            return Err(SignError::InvalidFollows {
                reason: "each followed pubkey must be 64 lowercase hex characters",
            });
        }
        if seen.insert(pk.as_str()) {
            tags.push(vec!["p".to_string(), pk.clone()]);
        }
    }
    sign_verified_parts(key, FOLLOWS_KIND, created_at, tags, "")
}

/// Sign a short-lived BUD-11 `kind: 24242` Blossom upload authorization token
/// scoped to `server_domains` and `sha256_hashes`.
pub fn sign_blossom_upload_auth(
    key: &SecretKey,
    created_at: u64,
    expiration: u64,
    server_domains: &[String],
    sha256_hashes: &[String],
) -> Result<Event, SignError> {
    if expiration <= created_at {
        return Err(SignError::InvalidBlossomAuth {
            reason: "expiration must be strictly after created_at",
        });
    }
    if server_domains.is_empty() || server_domains.len() > MAX_BLOSSOM_SERVERS {
        return Err(SignError::InvalidBlossomAuth {
            reason: "server_domains must name between 1 and 8 Blossom servers",
        });
    }
    for domain in server_domains {
        if !is_clean_server_domain(domain) {
            return Err(SignError::InvalidBlossomAuth {
                reason: "server domain must be lowercase host[:port] without scheme or path",
            });
        }
    }
    if sha256_hashes.is_empty() || sha256_hashes.len() > MAX_IMETA_ATTACHMENTS {
        return Err(SignError::InvalidBlossomAuth {
            reason: "sha256_hashes must name between 1 and 4 blobs",
        });
    }
    for hash in sha256_hashes {
        if !is_lower_sha256_hex(hash) {
            return Err(SignError::InvalidBlossomAuth {
                reason: "each blob hash must be 64 lowercase hex characters",
            });
        }
    }

    let mut tags = Vec::with_capacity(2 + server_domains.len() + sha256_hashes.len());
    tags.push(vec!["t".to_string(), "upload".to_string()]);
    for domain in server_domains {
        tags.push(vec!["server".to_string(), domain.clone()]);
    }
    tags.push(vec!["expiration".to_string(), expiration.to_string()]);
    for hash in sha256_hashes {
        tags.push(vec!["x".to_string(), hash.clone()]);
    }

    sign_verified_parts(
        key,
        BLOSSOM_AUTH_KIND,
        created_at,
        tags,
        "Upload media blob",
    )
}

/// Sign a NIP-57 `kind: 9734` zap request event.
///
/// Only admitted via `host/sign_nostr_zap` in the editor — never `autopublish`.
#[allow(clippy::too_many_arguments)]
pub fn sign_zap_request(
    key: &SecretKey,
    created_at: u64,
    recipient_pubkey_hex: &str,
    amount_msat: u64,
    relays: &[String],
    lnurl: Option<&str>,
    event_id_hex: Option<&str>,
    content: &str,
) -> Result<Event, SignError> {
    if !is_lower_sha256_hex(recipient_pubkey_hex) {
        return Err(SignError::InvalidZap {
            reason: "recipient_pubkey_hex must be 64 lowercase hex characters",
        });
    }
    if amount_msat == 0 {
        return Err(SignError::InvalidZap {
            reason: "zap amount_msat must be greater than zero",
        });
    }
    if relays.is_empty() {
        return Err(SignError::InvalidZap {
            reason: "zap request must list at least one relay",
        });
    }
    if let Some(eid) = event_id_hex {
        if !is_lower_sha256_hex(eid) {
            return Err(SignError::InvalidZap {
                reason: "event_id_hex must be 64 lowercase hex characters",
            });
        }
    }

    let mut relay_tag = Vec::with_capacity(1 + relays.len());
    relay_tag.push("relays".to_string());
    relay_tag.extend(relays.iter().cloned());

    let mut tags = vec![
        relay_tag,
        vec!["amount".to_string(), amount_msat.to_string()],
        vec!["p".to_string(), recipient_pubkey_hex.to_string()],
    ];
    if let Some(eid) = event_id_hex {
        tags.push(vec!["e".to_string(), eid.to_string()]);
    }
    if let Some(lnurl_str) = lnurl.filter(|s| !s.trim().is_empty()) {
        tags.push(vec!["lnurl".to_string(), lnurl_str.trim().to_string()]);
    }

    sign_verified_parts(key, ZAP_REQUEST_KIND, created_at, tags, content)
}

/// Sign a NIP-61 `kind: 9321` nutzap event carrying P2PK-locked Cashu proofs.
///
/// Only admitted via `host/sign_nostr_zap` in the editor — never `autopublish`.
pub fn sign_nutzap(
    key: &SecretKey,
    created_at: u64,
    recipient_pubkey_hex: &str,
    mint_url: &str,
    proofs_json: &[String],
    event_id_hex: Option<&str>,
    content: &str,
) -> Result<Event, SignError> {
    if !is_lower_sha256_hex(recipient_pubkey_hex) {
        return Err(SignError::InvalidZap {
            reason: "recipient_pubkey_hex must be 64 lowercase hex characters",
        });
    }
    if !is_clean_http_url(mint_url) {
        return Err(SignError::InvalidZap {
            reason: "mint_url must be a clean http(s) URL",
        });
    }
    if proofs_json.is_empty() {
        return Err(SignError::InvalidZap {
            reason: "nutzap must include at least one Cashu proof JSON entry",
        });
    }
    if let Some(eid) = event_id_hex {
        if !is_lower_sha256_hex(eid) {
            return Err(SignError::InvalidZap {
                reason: "event_id_hex must be 64 lowercase hex characters",
            });
        }
    }

    let mut tags = Vec::with_capacity(3 + proofs_json.len());
    for proof in proofs_json {
        tags.push(vec!["proof".to_string(), proof.clone()]);
    }
    tags.push(vec!["u".to_string(), mint_url.to_string()]);
    tags.push(vec!["p".to_string(), recipient_pubkey_hex.to_string()]);
    if let Some(eid) = event_id_hex {
        tags.push(vec!["e".to_string(), eid.to_string()]);
    }

    sign_verified_parts(key, NUTZAP_KIND, created_at, tags, content)
}

/// BIP-340 signature of `message` under `key` with the given auxiliary
/// randomness. [`sign_event`] supplies fresh randomness; tests supply the
/// published values.
fn sign_with_aux(
    key: &SecretKey,
    message: &[u8],
    aux_rand: &[u8; 32],
) -> Result<[u8; 64], SignError> {
    key.sign_raw(message, aux_rand).ok_or(SignError::Signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BIP-340's test vectors: the same unmodified file the core crate's
    /// verification test reads.
    const BIP340_VECTORS: &str =
        include_str!("../../hkask-nostr-core/tests/fixtures/bip340-test-vectors.csv");

    /// NIP-19's published `nsec` example and the public key NIP-19 lists for
    /// it. A throwaway key from the specification.
    const NIP19_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const NIP19_PUBKEY_HEX: &str =
        "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";

    fn bytes<const N: usize>(text: &str, what: &str, index: &str) -> [u8; N] {
        hex::decode(text)
            .unwrap_or_else(|error| panic!("vector {index}: {what}: {error}"))
            .try_into()
            .unwrap_or_else(|_| panic!("vector {index}: {what} is not {N} bytes"))
    }

    /// expect: this crate signs exactly as BIP-340 specifies, and derives the
    /// public key BIP-340 states for each secret key.
    /// pre: every published vector that carries a secret key (0 to 3 and 15 to
    /// 18; message sizes 32, 0, 1, 17 and 100 bytes).
    /// post: the derived public key and the signature equal the published
    /// "public key" and "signature" columns.
    #[test]
    fn bip340_published_signing_vectors_produce_published_signatures() {
        let mut signed = 0;
        for line in BIP340_VECTORS.lines().skip(1) {
            let fields: Vec<&str> = line.splitn(8, ',').collect();
            assert_eq!(fields.len(), 8, "malformed vector line: {line}");
            let index = fields[0];
            if fields[1].is_empty() {
                continue;
            }
            let key = SecretKey::from_bytes(&bytes::<32>(fields[1], "secret key", index))
                .unwrap_or_else(|error| panic!("vector {index}: secret key: {error}"));
            assert_eq!(
                hex::encode_upper(key.x_only_public_key()),
                fields[2],
                "vector {index}: public key"
            );
            let aux_rand = bytes::<32>(fields[3], "aux_rand", index);
            let message = hex::decode(fields[4])
                .unwrap_or_else(|error| panic!("vector {index}: message: {error}"));
            let signature = sign_with_aux(&key, &message, &aux_rand)
                .unwrap_or_else(|error| panic!("vector {index}: {error}"));
            assert_eq!(
                hex::encode_upper(signature),
                fields[5],
                "vector {index}: signature"
            );
            signed += 1;
        }
        assert_eq!(signed, 8, "the fixture holds 8 vectors with a secret key");
    }

    /// expect: a kind 1 note is signed into an event any client can verify.
    /// pre: NIP-19's example key, time 1700000000, content `hello nostr`.
    /// post: the event's id is the SHA-256 (coreutils `sha256sum`) of
    /// `[0,"7e7e…df4e",1700000000,1,[],"hello nostr"]`; author, kind, time and
    /// content are as given; it has no tags; and its JSON form verifies when
    /// read back through the core crate.
    #[test]
    fn a_kind_1_note_is_signed_into_a_verifiable_event() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let event = sign_event(&key, 1, 1_700_000_000, "hello nostr")
            .unwrap_or_else(|error| panic!("a kind 1 note must sign: {error}"));
        assert_eq!(
            hex::encode(event.id()),
            "03b64435a898c62cf0cd7e19eb48af2b6ae2787457a1e1f475f8f0368907fade"
        );
        assert_eq!(hex::encode(event.pubkey()), NIP19_PUBKEY_HEX);
        assert_eq!(event.kind(), 1);
        assert_eq!(event.created_at(), 1_700_000_000);
        assert_eq!(event.content(), "hello nostr");
        assert!(event.tags().is_empty());
        let read_back = Event::from_json(&event.to_json())
            .unwrap_or_else(|error| panic!("the signed event must verify: {error}"));
        assert_eq!(read_back, event);
    }

    #[test]
    fn sign_reaction_emits_verified_nip25_kind_7() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let target_id = "a".repeat(64);
        let target_pk = "b".repeat(64);
        let event = sign_reaction(&key, 1_700_000_000, &target_id, &target_pk, "+")
            .expect("valid NIP-25 reaction signs");
        assert_eq!(event.kind(), REACTION_KIND);
        assert_eq!(event.content(), "+");
        assert_eq!(
            event.tags(),
            &[
                vec!["e".to_string(), target_id],
                vec!["p".to_string(), target_pk]
            ]
        );
        assert_eq!(Event::from_json(&event.to_json()).expect("verifies"), event);
    }

    #[test]
    fn sign_reply_emits_nip10_for_kind_1_and_nip22_for_articles() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let root = "1".repeat(64);
        let parent = "2".repeat(64);
        let author = "3".repeat(64);

        let nip10 = sign_reply(
            &key,
            1_700_000_001,
            "great point",
            &ReplyTarget {
                root_event_id_hex: root.clone(),
                reply_to_event_id_hex: Some(parent.clone()),
                target_pubkey_hex: author.clone(),
                root_kind: 1,
            },
        )
        .expect("NIP-10 reply signs");
        assert_eq!(nip10.kind(), NOTE_KIND);
        assert_eq!(
            nip10.tags(),
            &[
                vec!["e".into(), root.clone(), String::new(), "root".into()],
                vec!["e".into(), parent, String::new(), "reply".into()],
                vec!["p".into(), author.clone()],
            ]
        );

        let nip22 = sign_reply(
            &key,
            1_700_000_002,
            "nice article",
            &ReplyTarget {
                root_event_id_hex: root.clone(),
                reply_to_event_id_hex: None,
                target_pubkey_hex: author.clone(),
                root_kind: 30023,
            },
        )
        .expect("NIP-22 comment signs");
        assert_eq!(nip22.kind(), COMMENT_KIND);
        assert_eq!(nip22.tags()[0], vec!["E", &root, "", &author]);
        assert_eq!(nip22.tags()[1], vec!["K", "30023"]);
    }

    #[test]
    fn sign_repost_emits_kind_6_for_notes_and_kind_16_for_other_kinds() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let target_id = "a".repeat(64);
        let target_pk = "b".repeat(64);

        let r6 = sign_repost(&key, 1_700_000_000, &target_id, &target_pk, 1, None)
            .expect("kind:6 repost signs");
        assert_eq!(r6.kind(), REPOST_KIND);
        assert_eq!(r6.content(), "");
        assert_eq!(
            r6.tags(),
            &[
                vec!["e".to_string(), target_id.clone()],
                vec!["p".to_string(), target_pk.clone()],
            ]
        );
        assert_eq!(Event::from_json(&r6.to_json()).expect("verifies"), r6);

        let r16 = sign_repost(
            &key,
            1_700_000_001,
            &target_id,
            &target_pk,
            30023,
            Some("wss://nos.lol"),
        )
        .expect("kind:16 generic repost signs");
        assert_eq!(r16.kind(), GENERIC_REPOST_KIND);
        assert_eq!(
            r16.tags(),
            &[
                vec!["e".to_string(), target_id, "wss://nos.lol".to_string()],
                vec!["p".to_string(), target_pk],
                vec!["k".to_string(), "30023".to_string()],
            ]
        );
        assert_eq!(Event::from_json(&r16.to_json()).expect("verifies"), r16);
    }

    #[test]
    fn sign_follows_deduplicates_and_rejects_invalid_hex() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let event = sign_follows(&key, 1_700_000_002, &[a.clone(), b.clone(), a.clone()])
            .expect("kind:3 follows signs");
        assert_eq!(event.kind(), FOLLOWS_KIND);
        assert_eq!(event.tags(), &[vec!["p", &a], vec!["p", &b]]);
        assert_eq!(Event::from_json(&event.to_json()).expect("verifies"), event);

        assert!(sign_follows(&key, 1_700_000_003, &["not-hex".to_string()]).is_err());
    }

    /// expect: nothing but a kind 1 note is signed.
    /// pre: metadata (0), contacts (3), an encrypted DM (4), a deletion (5), a
    /// repost (6), a reaction (7), a seal (13), a gift wrap (1059), a comment
    /// (1111), a relay list (10002), an article (30023), and the edges 2 and
    /// 65535.
    /// post: `KindNotAllowed` naming the kind, for each. The refusal is the
    /// whole result: there is no event and no signature in it.
    #[test]
    fn any_kind_other_than_1_is_refused_without_a_signature() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        for kind in [0u16, 2, 3, 4, 5, 6, 7, 13, 1059, 1111, 10002, 30023, 65535] {
            assert_eq!(
                sign_event(&key, kind, 1_700_000_000, "hello nostr"),
                Err(SignError::KindNotAllowed { kind })
            );
        }
    }

    /// expect: each signature uses fresh auxiliary randomness, as BIP-340
    /// recommends.
    /// pre: the same key, time and content signed twice.
    /// post: the two events have the same id and different signatures; both
    /// verified when they were built.
    #[test]
    fn signing_the_same_note_twice_gives_two_signatures() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let first = sign_event(&key, 1, 1_700_000_000, "hello nostr").expect("signs");
        let second = sign_event(&key, 1, 1_700_000_000, "hello nostr").expect("signs again");
        assert_eq!(first.id(), second.id());
        assert_ne!(first.sig(), second.sig());
    }

    #[test]
    fn note_with_imeta_builds_nip92_tag_and_rejects_malformed_fields() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let sha = "b1674191a88ec5cdd733e4240a81803105dc412d6c6708d53ab94fc248f4f553".to_string();
        let valid = ImetaAttachment {
            url: format!("https://cdn.example.com/{sha}.mp4"),
            mime_type: "video/mp4".to_string(),
            sha256: sha.clone(),
            size: 29_028_878,
            dimensions: Some((1920, 1080)),
            fallback_urls: vec![format!("https://mirror.example.com/{sha}.mp4")],
        };
        let event = sign_note_with_imeta(&key, 1_700_000_000, "clip", std::slice::from_ref(&valid))
            .expect("valid imeta signs");
        assert_eq!(event.kind(), 1);
        assert_eq!(
            event.tags(),
            &[vec![
                "imeta".to_string(),
                format!("url https://cdn.example.com/{sha}.mp4"),
                "m video/mp4".to_string(),
                format!("x {sha}"),
                "size 29028878".to_string(),
                "dim 1920x1080".to_string(),
                format!("fallback https://mirror.example.com/{sha}.mp4"),
            ]]
        );
        Event::from_json(&event.to_json()).expect("round-trip verifies");

        let mut bad_hash = valid;
        bad_hash.sha256 = "NOT_HEX".to_string();
        assert!(matches!(
            sign_note_with_imeta(&key, 1_700_000_000, "clip", &[bad_hash]),
            Err(SignError::InvalidAttachment { .. })
        ));
    }

    #[test]
    fn note_with_quote_builds_nip10_q_and_p_tags_and_rejects_bad_ids() {
        let key = SecretKey::from_bytes(&[7u8; 32]).expect("valid test key");
        let quote = QuoteTarget {
            event_id_hex: "11".repeat(32),
            author_pubkey_hex: "22".repeat(32),
            relay_hint: Some("wss://nos.lol".to_string()),
        };
        let event = sign_note_with_imeta_and_quote(
            &key,
            1_700_000_000,
            "Great point! nostr:note1...",
            &[],
            Some(&quote),
        )
        .expect("valid quote signs");
        assert_eq!(event.kind(), NOTE_KIND);
        assert_eq!(
            event.tags(),
            &[
                vec![
                    "q".to_string(),
                    "11".repeat(32),
                    "wss://nos.lol".to_string(),
                    "22".repeat(32),
                ],
                vec!["p".to_string(), "22".repeat(32)],
            ]
        );
        Event::from_json(&event.to_json()).expect("round-trip verifies");

        let bad_quote = QuoteTarget {
            event_id_hex: "not-hex".to_string(),
            author_pubkey_hex: "22".repeat(32),
            relay_hint: None,
        };
        assert!(
            sign_note_with_imeta_and_quote(&key, 1_700_000_000, "hi", &[], Some(&bad_quote))
                .is_err()
        );
    }

    #[test]
    fn blossom_upload_auth_signs_scoped_kind_24242_event_and_rejects_unscoped_inputs() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let sha = "b1674191a88ec5cdd733e4240a81803105dc412d6c6708d53ab94fc248f4f553".to_string();
        let event = sign_blossom_upload_auth(
            &key,
            1_700_000_000,
            1_700_000_300,
            &["blossom.example.com".to_string()],
            std::slice::from_ref(&sha),
        )
        .expect("scoped BUD-11 auth signs");
        assert_eq!(event.kind(), BLOSSOM_AUTH_KIND);
        assert_eq!(event.content(), "Upload media blob");
        assert_eq!(
            event.tags(),
            &[
                vec!["t".to_string(), "upload".to_string()],
                vec!["server".to_string(), "blossom.example.com".to_string()],
                vec!["expiration".to_string(), "1700000300".to_string()],
                vec!["x".to_string(), sha.clone()],
            ]
        );
        Event::from_json(&event.to_json()).expect("round-trip verifies");

        assert!(matches!(
            sign_blossom_upload_auth(
                &key,
                100,
                100,
                &["blossom.example.com".into()],
                std::slice::from_ref(&sha)
            ),
            Err(SignError::InvalidBlossomAuth { .. })
        ));
        assert!(matches!(
            sign_blossom_upload_auth(&key, 100, 200, &[], std::slice::from_ref(&sha)),
            Err(SignError::InvalidBlossomAuth { .. })
        ));
        assert!(matches!(
            sign_blossom_upload_auth(
                &key,
                100,
                200,
                &["https://blossom.example.com".into()],
                &[sha]
            ),
            Err(SignError::InvalidBlossomAuth { .. })
        ));
    }

    #[test]
    fn sign_deletion_emits_verified_nip09_kind_5_and_rejects_empty_targets() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let target = "ab".repeat(32);
        let event = sign_deletion(
            &key,
            1_700_000_200,
            std::slice::from_ref(&target),
            Some(NOTE_KIND),
            "published by accident",
        )
        .expect("NIP-09 deletion signs");
        assert_eq!(event.kind(), DELETION_KIND);
        assert_eq!(event.content(), "published by accident");
        assert_eq!(
            event.tags(),
            &[
                vec!["e".to_string(), target],
                vec!["k".to_string(), "1".to_string()],
            ]
        );
        Event::from_json(&event.to_json()).expect("round-trip verifies");

        assert!(matches!(
            sign_deletion(&key, 1_700_000_200, &[], Some(NOTE_KIND), ""),
            Err(SignError::InvalidDeletion { .. })
        ));
        assert!(matches!(
            sign_deletion(&key, 1_700_000_200, &["not-hex".to_string()], None, ""),
            Err(SignError::InvalidDeletion { .. })
        ));
    }
}
