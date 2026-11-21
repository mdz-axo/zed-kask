//! Signing a Nostr note in the editor process, for the Nostr client MCP
//! server (D106).
//!
//! The server never holds the key. It asks over the inference IPC wire
//! (`InferenceMethod::SignNostrNote`); the request is admitted only when the
//! child's grant names `host/sign_nostr_note`; and this module, on the GPUI
//! side, applies the operator's autopublish setting, asks the operator when
//! it is off, and only then reads the two keychain slots, decrypts the key,
//! signs one untagged kind 1 note, wipes, and returns the event.
//!
//! The order is the point: the approval comes before any keychain read, so a
//! declined publish never decrypts the key, and the setting is read where the
//! signature is made, so neither the server nor a tool description can skip
//! the approval.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{AppContext as _, AsyncApp, PromptLevel};
use hkask_types::inference_ipc::InferenceErrorPayload;
use hkask_types::nostr_wire::{
    BlossomBlobSummary, MAX_BLOSSOM_UPLOAD_BYTES, MAX_CONTENT_CHARS, MAX_MEDIA_ATTACHMENTS,
    NostrArticleWire, NostrDeletionWire, NostrFollowsWire, NostrHighlightWire, NostrListWire,
    NostrMediaAttachment, NostrNutzapWire, NostrProfileWire, NostrQuoteWire, NostrReactionWire,
    NostrReplyWire, NostrRepostWire, NostrZapRequestWire,
};
use settings::Settings as _;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::settings::KaskSettings;

/// What the admitted Nostr sign request asks the editor to sign.
#[derive(Debug)]
pub(crate) enum NostrSignOperation {
    /// Sign one kind 1 note (untagged, or carrying validated NIP-92 `imeta`
    /// tags and/or an optional NIP-10 quote target).
    Note {
        content: String,
        created_at: u64,
        relays: Vec<String>,
        imeta: Vec<NostrMediaAttachment>,
        quote: Option<NostrQuoteWire>,
    },
    /// Sign a short-lived BUD-11 `kind: 24242` Blossom upload authorization token.
    BlossomUpload {
        created_at: u64,
        expiration: u64,
        server_domains: Vec<String>,
        server_urls: Vec<String>,
        blobs: Vec<BlossomBlobSummary>,
    },
    /// Sign a NIP-57 `kind: 9734` zap request (D110 Phase 4 — gated by
    /// `NOSTR_ZAP_SIGN`, NEVER bypassed by `autopublish`).
    ZapRequest(NostrZapRequestWire),
    /// Sign a NIP-61 `kind: 9321` nutzap event (D110 Phase 4 — gated by
    /// `NOSTR_ZAP_SIGN`, NEVER bypassed by `autopublish`).
    Nutzap(NostrNutzapWire),
    /// Sign a NIP-25 `kind: 7` reaction (<https://nostr-nips.com/nip-25>).
    Reaction(NostrReactionWire),
    /// Sign a NIP-10 (`kind: 1`) or NIP-22 (`kind: 1111`) reply/comment.
    Reply(NostrReplyWire),
    /// Sign a NIP-09 `kind: 5` event deletion request (NEVER bypassed by `autopublish`).
    Deletion(NostrDeletionWire),
    /// Sign a NIP-18 repost (`kind: 6` or `kind: 16`).
    Repost(NostrRepostWire),
    /// Sign a NIP-02 `kind: 3` follow list (NEVER bypassed by `autopublish`).
    Follows(NostrFollowsWire),
    /// Sign a NIP-23 `kind: 30023` long-form Markdown article (NEVER bypassed by `autopublish`).
    Article(NostrArticleWire),
    /// Sign a NIP-51 `kind: 10000` (mute) or `kind: 10003` (bookmark) list (NEVER bypassed by `autopublish`).
    List(NostrListWire),
    /// Sign a NIP-01 `kind: 0` profile metadata event (NEVER bypassed by `autopublish`).
    Profile(NostrProfileWire),
    /// Sign a NIP-84 `kind: 9802` highlight event (<https://nostr-nips.com/nip-84>).
    Highlight(NostrHighlightWire),
}

/// The admitted sign request, carried from the tokio dispatch task to the
/// GPUI-side task over a channel (the `WorktreeSpawnRequest` pattern).
pub(crate) struct NostrSignRequest {
    pub(crate) operation: NostrSignOperation,
    pub(crate) grant: String,
    pub(crate) reply: oneshot::Sender<Result<serde_json::Value, InferenceErrorPayload>>,
}

pub(crate) const NOSTR_SIGN_QUEUE_CAPACITY: usize = 8;

/// The label on the note approval's affirmative button.
pub const APPROVE_LABEL: &str = "Publish";
/// The label on the note approval's negative button.
pub const DECLINE_LABEL: &str = "Don't publish";
/// The label on the Blossom upload approval's affirmative button.
pub const UPLOAD_APPROVE_LABEL: &str = "Upload";
/// The label on the Blossom upload approval's negative button.
pub const UPLOAD_DECLINE_LABEL: &str = "Don't upload";
/// Affirmative label on zap/nutzap approval modal (never bypassed by autopublish).
pub const ZAP_APPROVE_LABEL: &str = "Approve Zap";
/// Negative label on zap/nutzap approval modal.
pub const ZAP_DECLINE_LABEL: &str = "Decline";
/// Affirmative label on NIP-09 deletion modal (never bypassed by autopublish).
pub const DELETE_APPROVE_LABEL: &str = "Request Deletion";
/// Negative label on NIP-09 deletion modal.
pub const DELETE_DECLINE_LABEL: &str = "Cancel";

fn refusal(code: &str, message: impl Into<String>) -> InferenceErrorPayload {
    InferenceErrorPayload {
        code: code.into(),
        message: message.into(),
        status: None,
    }
}

/// The checks the tokio dispatch runs before anything is enqueued: the
/// grant and the shape of the note. Pure.
pub(crate) fn admit(
    grant: Option<&str>,
    content: Option<&str>,
    created_at: Option<u64>,
) -> Result<(String, u64), InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr note requires the parent-granted host/sign_nostr_note authority \
             (kask.mcp.delegated_tools for this server)",
        ));
    }
    let content = content.unwrap_or_default();
    if content.trim().is_empty() {
        return Err(refusal("InvalidArgument", "the note has no text"));
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(refusal(
            "InvalidArgument",
            format!("the note is longer than {MAX_CONTENT_CHARS} characters"),
        ));
    }
    let Some(created_at) = created_at else {
        return Err(refusal("InvalidArgument", "the note has no created_at"));
    };
    Ok((content.to_string(), created_at))
}

fn extract_url_host(raw: &str, expected_schemes: &[&str]) -> Option<(String, String)> {
    let trimmed = raw.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    let scheme_lower = scheme.to_ascii_lowercase();
    if !expected_schemes.contains(&scheme_lower.as_str()) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = if let Some(after_open) = authority.strip_prefix('[') {
        after_open.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    if host.is_empty() {
        return None;
    }
    Some((
        scheme_lower,
        host.trim_end_matches('.').to_ascii_lowercase(),
    ))
}

fn is_loopback_relay_url(raw: &str) -> bool {
    let Some((_, host)) = extract_url_host(raw, &["ws", "wss"]) else {
        return false;
    };
    host == "127.0.0.1" || host == "localhost" || host == "::1"
}

fn is_public_https_media_url(raw: &str) -> bool {
    let Some((scheme, host)) = extract_url_host(raw, &["https"]) else {
        return false;
    };
    if scheme != "https" {
        return false;
    }
    if host == "localhost" || host.ends_with(".localhost") {
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
            std::net::IpAddr::V6(v6) => !(v6.is_loopback() || v6.is_unspecified()),
        };
    }
    true
}

/// Admit a note sign request (including optional NIP-92 `imeta` attachments
/// and optional NIP-10 quote target).
pub(crate) fn admit_note(
    grant: Option<&str>,
    content: Option<&str>,
    created_at: Option<u64>,
    relays: Vec<String>,
    imeta: Vec<NostrMediaAttachment>,
    quote: Option<NostrQuoteWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    let (content, created_at) = admit(grant, content, created_at)?;
    if imeta.len() > MAX_MEDIA_ATTACHMENTS {
        return Err(refusal(
            "InvalidArgument",
            format!(
                "at most {MAX_MEDIA_ATTACHMENTS} media attachments may be signed onto one note"
            ),
        ));
    }
    if let Some(q) = &quote {
        let valid_hex =
            |s: &str| s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !valid_hex(&q.event_id_hex) || !valid_hex(&q.author_pubkey_hex) {
            return Err(refusal(
                "InvalidArgument",
                "quote event_id_hex and author_pubkey_hex must be 64 lowercase hex characters",
            ));
        }
        if let Some(hint) = q.relay_hint.as_deref().map(str::trim)
            && !hint.is_empty()
            && !hint.starts_with("wss://")
        {
            return Err(refusal(
                "InvalidArgument",
                "quote relay_hint must be empty or start with wss://",
            ));
        }
    }
    let publishing_to_public_relays = relays.iter().any(|r| !is_loopback_relay_url(r));
    if publishing_to_public_relays {
        for att in &imeta {
            if !is_public_https_media_url(&att.url)
                || att
                    .fallback_urls
                    .iter()
                    .any(|u| !is_public_https_media_url(u))
            {
                return Err(refusal(
                    "InvalidArgument",
                    "a note published to public seed relays cannot carry a local or non-https media URL in imeta",
                ));
            }
        }
    }
    Ok(NostrSignOperation::Note {
        content,
        created_at,
        relays,
        imeta,
        quote,
    })
}

/// Admit a BUD-11 Blossom upload authorization sign request before enqueueing.
pub(crate) fn admit_blossom_upload(
    grant: Option<&str>,
    created_at: Option<u64>,
    expiration: Option<u64>,
    server_domains: Vec<String>,
    server_urls: Vec<String>,
    blobs: Vec<BlossomBlobSummary>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Blossom upload token requires the parent-granted host/sign_nostr_note authority \
             (kask.mcp.delegated_tools for this server)",
        ));
    }
    let Some(created_at) = created_at else {
        return Err(refusal(
            "InvalidArgument",
            "the Blossom token has no created_at",
        ));
    };
    let Some(expiration) = expiration else {
        return Err(refusal(
            "InvalidArgument",
            "the Blossom token has no expiration",
        ));
    };
    if expiration <= created_at {
        return Err(refusal(
            "InvalidArgument",
            "the Blossom token expiration must be strictly after created_at",
        ));
    }
    if server_domains.is_empty() || server_domains.len() > hkask_nostr_signer::MAX_BLOSSOM_SERVERS {
        return Err(refusal(
            "InvalidArgument",
            "the Blossom token must name between 1 and 8 server domains",
        ));
    }
    if blobs.is_empty() || blobs.len() > MAX_MEDIA_ATTACHMENTS {
        return Err(refusal(
            "InvalidArgument",
            format!("between 1 and {MAX_MEDIA_ATTACHMENTS} media files may be uploaded per note"),
        ));
    }
    for blob in &blobs {
        if blob.path.trim().is_empty() || blob.mime_type.trim().is_empty() {
            return Err(refusal(
                "InvalidArgument",
                "each Blossom blob must name its file path and MIME type",
            ));
        }
        if blob.size == 0 || blob.size > MAX_BLOSSOM_UPLOAD_BYTES {
            return Err(refusal(
                "InvalidArgument",
                format!(
                    "Blossom blob size {} is outside 1..={MAX_BLOSSOM_UPLOAD_BYTES} bytes",
                    blob.size
                ),
            ));
        }
    }
    Ok(NostrSignOperation::BlossomUpload {
        created_at,
        expiration,
        server_domains,
        server_urls,
        blobs,
    })
}

/// Admit a NIP-57 `kind: 9734` zap request (requires `host/sign_nostr_zap`).
pub(crate) fn admit_zap_request(
    grant: Option<&str>,
    payload: Option<&NostrZapRequestWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_ZAP_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr zap request requires the parent-granted host/sign_nostr_zap authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_zap_request payload is required",
        ));
    };
    if payload.amount_msat == 0 {
        return Err(refusal(
            "InvalidArgument",
            "zap amount_msat must be greater than zero",
        ));
    }
    if payload.relays.is_empty() {
        return Err(refusal(
            "InvalidArgument",
            "zap request must name at least one relay",
        ));
    }
    Ok(NostrSignOperation::ZapRequest(payload.clone()))
}

/// Admit a NIP-61 `kind: 9321` nutzap (requires `host/sign_nostr_zap`).
pub(crate) fn admit_nutzap(
    grant: Option<&str>,
    payload: Option<&NostrNutzapWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_ZAP_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr nutzap requires the parent-granted host/sign_nostr_zap authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_nutzap payload is required",
        ));
    };
    if payload.proofs_json.is_empty() {
        return Err(refusal(
            "InvalidArgument",
            "nutzap must include at least one Cashu proof",
        ));
    }
    Ok(NostrSignOperation::Nutzap(payload.clone()))
}

/// Admit a NIP-25 `kind: 7` reaction (requires `host/sign_nostr_note`).
pub(crate) fn admit_reaction(
    grant: Option<&str>,
    payload: Option<&NostrReactionWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr reaction requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_reaction payload is required",
        ));
    };
    if payload.content.trim().is_empty() || payload.content.trim().chars().count() > 32 {
        return Err(refusal(
            "InvalidArgument",
            "reaction content must be 1..=32 characters",
        ));
    }
    Ok(NostrSignOperation::Reaction(payload.clone()))
}

/// Admit a NIP-10 (`kind: 1`) or NIP-22 (`kind: 1111`) reply (requires `host/sign_nostr_note`).
pub(crate) fn admit_reply(
    grant: Option<&str>,
    payload: Option<&NostrReplyWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr reply requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_reply payload is required",
        ));
    };
    if payload.content.trim().is_empty() {
        return Err(refusal("InvalidArgument", "the reply has no text"));
    }
    if payload.content.chars().count() > MAX_CONTENT_CHARS {
        return Err(refusal(
            "InvalidArgument",
            format!("the reply is longer than {MAX_CONTENT_CHARS} characters"),
        ));
    }
    Ok(NostrSignOperation::Reply(payload.clone()))
}

/// Admit a NIP-09 `kind: 5` deletion request (requires `host/sign_nostr_note`;
/// always prompts the operator regardless of `autopublish`).
pub(crate) fn admit_deletion(
    grant: Option<&str>,
    payload: Option<&NostrDeletionWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr deletion request requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_deletion payload is required",
        ));
    };
    if payload.target_event_ids.is_empty()
        || payload.target_event_ids.len() > hkask_nostr_signer::MAX_DELETION_TARGETS
    {
        return Err(refusal(
            "InvalidArgument",
            format!(
                "deletion request must name between 1 and {} target event ids",
                hkask_nostr_signer::MAX_DELETION_TARGETS
            ),
        ));
    }
    if payload.reason.chars().count() > MAX_CONTENT_CHARS {
        return Err(refusal(
            "InvalidArgument",
            format!("deletion reason exceeds {MAX_CONTENT_CHARS} characters"),
        ));
    }
    Ok(NostrSignOperation::Deletion(payload.clone()))
}

/// Admit a NIP-18 repost (`kind: 6` or `kind: 16`).
pub(crate) fn admit_repost(
    grant: Option<&str>,
    payload: Option<&NostrRepostWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr repost requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_repost payload is required",
        ));
    };
    Ok(NostrSignOperation::Repost(payload.clone()))
}

/// Admit a NIP-02 `kind: 3` follow list (requires `host/sign_nostr_note`;
/// always prompts the operator regardless of `autopublish`).
pub(crate) fn admit_follows(
    grant: Option<&str>,
    payload: Option<&NostrFollowsWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr follow list requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_follows payload is required",
        ));
    };
    if payload.followed_pubkeys_hex.len() > hkask_nostr_signer::MAX_FOLLOWS {
        return Err(refusal(
            "InvalidArgument",
            format!(
                "follow list exceeds maximum of {} pubkeys",
                hkask_nostr_signer::MAX_FOLLOWS
            ),
        ));
    }
    Ok(NostrSignOperation::Follows(payload.clone()))
}

/// Admit a NIP-84 `kind: 9802` highlight sign request.
pub(crate) fn admit_highlight(
    grant: Option<&str>,
    payload: Option<&NostrHighlightWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr highlight requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_highlight payload is required for SignNostrHighlight",
        ));
    };
    if payload.quoted_text.trim().is_empty() || payload.quoted_text.chars().count() > 4_000 {
        return Err(refusal(
            "InvalidArgument",
            "highlight quoted_text must be non-empty and at most 4000 characters",
        ));
    }
    Ok(NostrSignOperation::Highlight(payload.clone()))
}

/// Admit a NIP-01 `kind: 0` profile metadata sign request.
pub(crate) fn admit_profile(
    grant: Option<&str>,
    payload: Option<&NostrProfileWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr profile requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_profile payload is required for SignNostrProfile",
        ));
    };
    let has_any = [
        &payload.name,
        &payload.display_name,
        &payload.about,
        &payload.picture,
        &payload.website,
        &payload.nip05,
        &payload.lud16,
    ]
    .iter()
    .any(|f| f.as_deref().is_some_and(|s| !s.trim().is_empty()));
    if !has_any {
        return Err(refusal(
            "InvalidArgument",
            "at least one profile metadata field must be non-empty",
        ));
    }
    Ok(NostrSignOperation::Profile(payload.clone()))
}

/// Admit a NIP-23 `kind: 30023` long-form article sign request.
pub(crate) fn admit_article(
    grant: Option<&str>,
    payload: Option<&NostrArticleWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr article requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_article payload is required for SignNostrArticle",
        ));
    };
    if payload.title.trim().is_empty() || payload.slug.trim().is_empty() {
        return Err(refusal(
            "InvalidArgument",
            "article title and slug must be non-empty",
        ));
    }
    if payload.content.trim().is_empty()
        || payload.content.chars().count() > hkask_nostr_signer::MAX_ARTICLE_CHARS
    {
        return Err(refusal(
            "InvalidArgument",
            "article markdown body must be non-empty and at most 64000 characters",
        ));
    }
    Ok(NostrSignOperation::Article(payload.clone()))
}

/// Admit a NIP-51 `kind: 10000` (mute) or `kind: 10003` (bookmark) list sign request.
pub(crate) fn admit_list(
    grant: Option<&str>,
    payload: Option<&NostrListWire>,
) -> Result<NostrSignOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::NOSTR_SIGN) {
        return Err(refusal(
            "Auth",
            "Signing a Nostr list requires the parent-granted host/sign_nostr_note authority",
        ));
    }
    let Some(payload) = payload else {
        return Err(refusal(
            "InvalidArgument",
            "nostr_list payload is required for SignNostrList",
        ));
    };
    if !matches!(
        payload.kind,
        hkask_nostr_signer::MUTE_LIST_KIND
            | hkask_nostr_signer::BOOKMARK_LIST_KIND
            | hkask_nostr_signer::RELAY_LIST_KIND
            | hkask_nostr_signer::DM_RELAYS_KIND
            | hkask_nostr_signer::NUTZAP_INFO_KIND
    ) {
        return Err(refusal(
            "InvalidArgument",
            "only kinds 10000 (mute), 10002 (NIP-65 relay list), 10003 (bookmark), 10050 (NIP-17 DM relays), and 10019 (NIP-61 nutzap info) are admitted",
        ));
    }
    if payload.event_ids_hex.len() + payload.pubkeys_hex.len() > hkask_nostr_signer::MAX_LIST_ITEMS
    {
        return Err(refusal(
            "InvalidArgument",
            "list exceeds maximum of 500 entries",
        ));
    }
    Ok(NostrSignOperation::List(payload.clone()))
}

/// What the operator is shown when autopublish is off. The whole text is in
/// the detail so what is approved is what is signed.
pub(crate) fn approval_text(npub: &str, relays: &[String]) -> String {
    let identity = if npub.is_empty() {
        "the saved key".to_string()
    } else {
        npub.to_string()
    };
    let targets = if relays.is_empty() {
        "no relay".to_string()
    } else {
        relays.join(", ")
    };
    format!("The Curator wants to publish this note as {identity} to {targets}.")
}

/// Prompt message for approving a Blossom media upload before bytes leave the machine.
pub(crate) fn upload_approval_text(npub: &str, server_urls: &[String]) -> String {
    let identity = if npub.is_empty() {
        "the saved key".to_string()
    } else {
        npub.to_string()
    };
    let targets = if server_urls.is_empty() {
        "no Blossom server".to_string()
    } else {
        server_urls.join(", ")
    };
    format!("The Curator wants to upload media as {identity} to {targets}.")
}

fn note_approval_detail(content: &str, imeta: &[NostrMediaAttachment]) -> String {
    if imeta.is_empty() {
        return content.to_string();
    }
    let mut detail = content.to_string();
    detail.push_str("\n\nMedia attachments (NIP-92 imeta):");
    for att in imeta {
        detail.push_str(&format!(
            "\n- {} ({}, {} bytes, sha256: {})",
            att.url, att.mime_type, att.size, att.sha256
        ));
    }
    detail
}

fn upload_approval_detail(blobs: &[BlossomBlobSummary]) -> String {
    blobs
        .iter()
        .map(|blob| {
            format!(
                "{} ({} bytes, {}, sha256: {})",
                blob.path, blob.size, blob.mime_type, blob.sha256
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the operator's answer index on a prompt built with
/// `[APPROVE_LABEL, DECLINE_LABEL]` is an approval. A closed prompt (the
/// window went away) is not.
pub(crate) fn answer_approves<E>(answer: Result<usize, E>) -> bool {
    matches!(answer, Ok(0))
}

/// Decrypt, check and sign a plain kind 1 note straight from the two
/// slots. Retained for the direct-signing tests; the production path goes
/// through `sign_operation_after_approval`. `configured_npub` is
/// `kask.nostr.npub` as set; empty means none, and then the key's own
/// identity is the identity.
#[cfg(test)]
pub(crate) fn sign_with_slots(
    ncryptsec: &str,
    password: &str,
    configured_npub: &str,
    content: &str,
    created_at: u64,
) -> Result<serde_json::Value, InferenceErrorPayload> {
    sign_operation_with_slots(
        ncryptsec,
        password,
        configured_npub,
        &NostrSignOperation::Note {
            content: content.to_string(),
            created_at,
            relays: Vec::new(),
            imeta: Vec::new(),
            quote: None,
        },
    )
}

pub(crate) fn sign_operation_with_slots(
    ncryptsec: &str,
    password: &str,
    configured_npub: &str,
    operation: &NostrSignOperation,
) -> Result<serde_json::Value, InferenceErrorPayload> {
    let key = hkask_nostr_signer::decrypt_ncryptsec(ncryptsec, password).map_err(|_| {
        refusal(
            "Auth",
            "the saved key did not open under the saved password",
        )
    })?;
    let npub = hkask_nostr_core::encode_npub(&key.x_only_public_key());
    let configured = configured_npub.trim();
    if !configured.is_empty() && configured != npub {
        return Err(refusal(
            "Auth",
            "the saved key is not the identity set in kask.nostr.npub; nothing was signed",
        ));
    }
    let event = match operation {
        NostrSignOperation::Note {
            content,
            created_at,
            imeta,
            quote,
            ..
        } if imeta.is_empty() && quote.is_none() => hkask_nostr_signer::sign_event(
            &key,
            hkask_nostr_signer::NOTE_KIND,
            *created_at,
            content,
        )
        .map_err(|error| refusal("Generation", format!("signing failed: {error}")))?,
        NostrSignOperation::Note {
            content,
            created_at,
            imeta,
            quote,
            ..
        } => {
            let attachments: Vec<hkask_nostr_signer::ImetaAttachment> = imeta
                .iter()
                .map(|att| hkask_nostr_signer::ImetaAttachment {
                    url: att.url.clone(),
                    mime_type: att.mime_type.clone(),
                    sha256: att.sha256.clone(),
                    size: att.size,
                    dimensions: att.dimensions,
                    fallback_urls: att.fallback_urls.clone(),
                })
                .collect();
            let quote_target = quote.as_ref().map(|q| hkask_nostr_signer::QuoteTarget {
                event_id_hex: q.event_id_hex.clone(),
                author_pubkey_hex: q.author_pubkey_hex.clone(),
                relay_hint: q.relay_hint.clone(),
            });
            hkask_nostr_signer::sign_note_with_imeta_and_quote(
                &key,
                *created_at,
                content,
                &attachments,
                quote_target.as_ref(),
            )
            .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
        }
        NostrSignOperation::BlossomUpload {
            created_at,
            expiration,
            server_domains,
            blobs,
            ..
        } => {
            let hashes: Vec<String> = blobs.iter().map(|blob| blob.sha256.clone()).collect();
            hkask_nostr_signer::sign_blossom_upload_auth(
                &key,
                *created_at,
                *expiration,
                server_domains,
                &hashes,
            )
            .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
        }
        NostrSignOperation::ZapRequest(zap) => hkask_nostr_signer::sign_zap_request(
            &key,
            zap.created_at,
            &zap.recipient_pubkey_hex,
            zap.amount_msat,
            &zap.relays,
            zap.lnurl.as_deref(),
            zap.event_id_hex.as_deref(),
            &zap.content,
        )
        .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        NostrSignOperation::Nutzap(nz) => hkask_nostr_signer::sign_nutzap(
            &key,
            nz.created_at,
            &nz.recipient_pubkey_hex,
            &nz.mint_url,
            &nz.proofs_json,
            nz.event_id_hex.as_deref(),
            &nz.content,
        )
        .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        NostrSignOperation::Reaction(rx) => hkask_nostr_signer::sign_reaction(
            &key,
            rx.created_at,
            &rx.target_event_id_hex,
            &rx.target_pubkey_hex,
            &rx.content,
        )
        .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        NostrSignOperation::Reply(reply) => {
            let target = hkask_nostr_signer::ReplyTarget {
                root_event_id_hex: reply.root_event_id_hex.clone(),
                reply_to_event_id_hex: reply.reply_to_event_id_hex.clone(),
                target_pubkey_hex: reply.target_pubkey_hex.clone(),
                root_kind: reply.root_kind,
            };
            hkask_nostr_signer::sign_reply(&key, reply.created_at, &reply.content, &target)
                .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
        }
        NostrSignOperation::Deletion(del) => hkask_nostr_signer::sign_deletion(
            &key,
            del.created_at,
            &del.target_event_ids,
            del.target_kind,
            &del.reason,
        )
        .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        NostrSignOperation::Repost(repost) => hkask_nostr_signer::sign_repost(
            &key,
            repost.created_at,
            &repost.target_event_id_hex,
            &repost.target_pubkey_hex,
            repost.target_kind,
            repost.relay_hint.as_deref(),
        )
        .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        NostrSignOperation::Follows(follows) => hkask_nostr_signer::sign_follows(
            &key,
            follows.created_at,
            &follows.followed_pubkeys_hex,
        )
        .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        NostrSignOperation::Article(article) => {
            let draft = hkask_nostr_signer::ArticleDraft {
                slug: article.slug.clone(),
                title: article.title.clone(),
                summary: article.summary.clone(),
                published_at: article.published_at,
                hashtags: article.hashtags.clone(),
            };
            hkask_nostr_signer::sign_article(&key, article.created_at, &article.content, &draft)
                .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
        }
        NostrSignOperation::Profile(profile) => {
            let meta = hkask_nostr_signer::ProfileMetadata {
                name: profile.name.clone(),
                display_name: profile.display_name.clone(),
                about: profile.about.clone(),
                picture: profile.picture.clone(),
                banner: profile.banner.clone(),
                website: profile.website.clone(),
                nip05: profile.nip05.clone(),
                lud16: profile.lud16.clone(),
                lud06: profile.lud06.clone(),
            };
            hkask_nostr_signer::sign_profile(&key, profile.created_at, &meta)
                .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
        }
        NostrSignOperation::Highlight(hl) => {
            let draft = hkask_nostr_signer::HighlightDraft {
                source_event_id_hex: hl.source_event_id_hex.clone(),
                source_author_hex: hl.source_author_hex.clone(),
                source_url: hl.source_url.clone(),
                comment: hl.comment.clone(),
            };
            hkask_nostr_signer::sign_highlight(&key, hl.created_at, &hl.quoted_text, &draft)
                .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
        }
        NostrSignOperation::List(list) => match list.kind {
            hkask_nostr_signer::RELAY_LIST_KIND => {
                hkask_nostr_signer::sign_relay_list(&key, list.created_at, &list.relay_urls)
                    .map_err(|error| {
                        refusal("InvalidArgument", format!("signing failed: {error}"))
                    })?
            }
            hkask_nostr_signer::DM_RELAYS_KIND => {
                hkask_nostr_signer::sign_dm_relays(&key, list.created_at, &list.relay_urls)
                    .map_err(|error| {
                        refusal("InvalidArgument", format!("signing failed: {error}"))
                    })?
            }
            hkask_nostr_signer::NUTZAP_INFO_KIND => {
                let p2pk = list.p2pk_pubkey_hex.as_deref().unwrap_or("");
                hkask_nostr_signer::sign_nutzap_info(
                    &key,
                    list.created_at,
                    &list.relay_urls,
                    &list.mint_urls,
                    p2pk,
                )
                .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?
            }
            _ => hkask_nostr_signer::sign_list_with_preserved(
                &key,
                list.created_at,
                list.kind,
                &list.event_ids_hex,
                &list.pubkeys_hex,
                &list.preserved_tags,
                list.preserved_content.as_deref().unwrap_or(""),
            )
            .map_err(|error| refusal("InvalidArgument", format!("signing failed: {error}")))?,
        },
    };
    drop(key);
    serde_json::from_str(&event.to_json()).map_err(|error| {
        refusal(
            "Json",
            format!("the signed event did not serialize: {error}"),
        )
    })
}

/// Handle one admitted request on the GPUI side. Reads the setting, asks
/// the operator if the setting says so, then signs.
pub(crate) async fn handle(request: NostrSignRequest, cx: &mut AsyncApp) {
    if request.reply.is_closed() {
        return;
    }
    let NostrSignRequest {
        operation,
        grant,
        reply,
    } = request;

    let is_zap_op = matches!(
        operation,
        NostrSignOperation::ZapRequest(_) | NostrSignOperation::Nutzap(_)
    );
    let required_grant = if is_zap_op {
        crate::delegation_grants::NOSTR_ZAP_SIGN
    } else {
        crate::delegation_grants::NOSTR_SIGN
    };

    // Re-check the grant at the admission boundary, as the worktree path does.
    if !crate::delegation_grants::grant_holds(Some(&grant), required_grant) {
        let _ = reply.send(Err(refusal(
            "Auth",
            "the sign grant was revoked before execution",
        )));
        return;
    }

    let (autopublish, configured_npub) = cx.update(|cx| {
        let nostr = &KaskSettings::get_global(cx).nostr;
        (nostr.autopublish, nostr.npub.clone())
    });

    // D110, NIP-09 & NIP-02 rulings: `autopublish` NEVER applies to zaps,
    // nutzaps, deletions, or replaceable kind:3 follow list updates.
    let always_prompt_op = matches!(
        operation,
        NostrSignOperation::Deletion(_)
            | NostrSignOperation::Follows(_)
            | NostrSignOperation::Article(_)
            | NostrSignOperation::List(_)
            | NostrSignOperation::Profile(_)
    );
    if is_zap_op || always_prompt_op || !autopublish {
        let Some(window) = cx.update(|cx| {
            cx.active_window()
                .or_else(|| cx.windows().into_iter().next())
        }) else {
            let _ = reply.send(Err(refusal(
                "Unavailable",
                if is_zap_op {
                    "signing a Nostr zap/nutzap always requires an open editor window for operator approval"
                } else if always_prompt_op {
                    "signing a NIP-09 deletion or NIP-02 follow list update always requires an open editor window for operator approval"
                } else {
                    "autopublish is off and no editor window is open to approve the action"
                },
            )));
            return;
        };
        let (message, detail, buttons, decline_msg) = match &operation {
            NostrSignOperation::Note {
                content,
                relays,
                imeta,
                quote,
                ..
            } => {
                let mut detail = note_approval_detail(content, imeta);
                if let Some(q) = quote {
                    detail.push_str(&format!(
                        "\n\nQuoting event (NIP-10 q tag): {}\nQuoted author: {}",
                        q.event_id_hex, q.author_pubkey_hex
                    ));
                }
                (
                    approval_text(&configured_npub, relays),
                    detail,
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the note; nothing was signed",
                )
            }
            NostrSignOperation::BlossomUpload {
                server_urls, blobs, ..
            } => (
                upload_approval_text(&configured_npub, server_urls),
                upload_approval_detail(blobs),
                [UPLOAD_APPROVE_LABEL, UPLOAD_DECLINE_LABEL],
                "the operator did not approve the media upload; nothing was signed or uploaded",
            ),
            NostrSignOperation::ZapRequest(zap) => (
                format!(
                    "Approve signing NIP-57 Zap Request ({} sats)?",
                    zap.amount_msat / 1_000
                ),
                format!(
                    "Recipient: {}\nAmount: {} msat\nRelays: {}\nEvent: {}\nMessage: {}",
                    zap.recipient_pubkey_hex,
                    zap.amount_msat,
                    zap.relays.join(", "),
                    zap.event_id_hex.as_deref().unwrap_or("<profile zap>"),
                    zap.content
                ),
                [ZAP_APPROVE_LABEL, ZAP_DECLINE_LABEL],
                "the operator declined the zap request; nothing was signed",
            ),
            NostrSignOperation::Nutzap(nz) => (
                format!(
                    "Approve publishing NIP-61 Nutzap ({} proof(s))?",
                    nz.proofs_json.len()
                ),
                format!(
                    "Recipient: {}\nMint: {}\nEvent: {}\nMessage: {}",
                    nz.recipient_pubkey_hex,
                    nz.mint_url,
                    nz.event_id_hex.as_deref().unwrap_or("<profile nutzap>"),
                    nz.content
                ),
                [ZAP_APPROVE_LABEL, ZAP_DECLINE_LABEL],
                "the operator declined the nutzap; nothing was signed",
            ),
            NostrSignOperation::Reaction(rx) => (
                approval_text(&configured_npub, &rx.relays),
                format!(
                    "NIP-25 Reaction: {}\nTarget event: {}\nTarget author: {}",
                    rx.content, rx.target_event_id_hex, rx.target_pubkey_hex
                ),
                [APPROVE_LABEL, DECLINE_LABEL],
                "the operator did not approve the reaction; nothing was signed",
            ),
            NostrSignOperation::Reply(reply) => (
                approval_text(&configured_npub, &reply.relays),
                format!(
                    "Reply ({} to {}):\n\n{}",
                    if reply.root_kind == hkask_nostr_signer::NOTE_KIND {
                        "NIP-10 kind:1"
                    } else {
                        "NIP-22 kind:1111"
                    },
                    reply
                        .reply_to_event_id_hex
                        .as_deref()
                        .unwrap_or(&reply.root_event_id_hex),
                    reply.content
                ),
                [APPROVE_LABEL, DECLINE_LABEL],
                "the operator did not approve the reply; nothing was signed",
            ),
            NostrSignOperation::Deletion(del) => (
                format!(
                    "Request deletion of {} Nostr event(s) (NIP-09 kind:5)?",
                    del.target_event_ids.len()
                ),
                format!(
                    "This cannot be undone, and relays are not guaranteed to honor deletion requests.\n\
                     Target event(s): {}\nKind: {}\nRelays: {}\nReason: {}",
                    del.target_event_ids.join(", "),
                    del.target_kind
                        .map(|k| k.to_string())
                        .unwrap_or_else(|| "<unspecified>".to_string()),
                    del.relays.join(", "),
                    if del.reason.trim().is_empty() {
                        "<none>"
                    } else {
                        del.reason.trim()
                    }
                ),
                [DELETE_APPROVE_LABEL, DELETE_DECLINE_LABEL],
                "the operator declined the deletion request; nothing was signed",
            ),
            NostrSignOperation::Repost(repost) => (
                format!(
                    "Repost Nostr event (NIP-18 kind:{})?",
                    if repost.target_kind == hkask_nostr_signer::NOTE_KIND {
                        hkask_nostr_signer::REPOST_KIND
                    } else {
                        hkask_nostr_signer::GENERIC_REPOST_KIND
                    }
                ),
                format!(
                    "Target event: {}\nTarget author: {}\nTarget kind: {}\nRelays: {}",
                    repost.target_event_id_hex,
                    repost.target_pubkey_hex,
                    repost.target_kind,
                    repost.relays.join(", ")
                ),
                [APPROVE_LABEL, DECLINE_LABEL],
                "the operator did not approve the repost; nothing was signed",
            ),
            NostrSignOperation::Follows(follows) => (
                format!(
                    "Publish updated Nostr follow list ({} followed author(s), NIP-02 kind:3)?",
                    follows.followed_pubkeys_hex.len()
                ),
                format!(
                    "This replaces your kind:3 contact list across relays.\n\
                     Total followed pubkeys: {}\nRelays: {}",
                    follows.followed_pubkeys_hex.len(),
                    follows.relays.join(", ")
                ),
                [APPROVE_LABEL, DECLINE_LABEL],
                "the operator did not approve the follow list update; nothing was signed",
            ),
            NostrSignOperation::Article(article) => {
                let preview: String = article.content.chars().take(400).collect();
                (
                    format!(
                        "Publish NIP-23 long-form article \"{}\" (kind:30023)?",
                        article.title
                    ),
                    format!(
                        "Slug (d tag): {}\nLength: {} chars\nRelays: {}\n\nPreview:\n{}",
                        article.slug,
                        article.content.chars().count(),
                        article.relays.join(", "),
                        preview,
                    ),
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the article; nothing was signed",
                )
            }
            NostrSignOperation::Profile(profile) => {
                let mut lines = Vec::new();
                if let Some(v) = &profile.name {
                    lines.push(format!("Name: {v}"));
                }
                if let Some(v) = &profile.display_name {
                    lines.push(format!("Display name: {v}"));
                }
                if let Some(v) = &profile.about {
                    lines.push(format!("About: {v}"));
                }
                if let Some(v) = &profile.nip05 {
                    lines.push(format!("NIP-05: {v}"));
                }
                if let Some(v) = &profile.lud16 {
                    lines.push(format!("Lightning Address (lud16): {v}"));
                }
                if let Some(v) = &profile.website {
                    lines.push(format!("Website: {v}"));
                }
                if let Some(v) = &profile.picture {
                    lines.push(format!("Picture: {v}"));
                }
                lines.push(format!("Relays: {}", profile.relays.join(", ")));
                (
                    "Publish updated Nostr profile metadata (NIP-01 kind:0)?".to_string(),
                    format!(
                        "This replaces your kind:0 profile metadata across relays.\n{}",
                        lines.join("\n")
                    ),
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the profile update; nothing was signed",
                )
            }
            NostrSignOperation::Highlight(hl) => {
                let preview: String = hl.quoted_text.chars().take(400).collect();
                (
                    "Publish NIP-84 highlight (kind:9802)?".to_string(),
                    format!(
                        "Quoted text:\n{preview}\n\nComment: {}\nSource URL: {}\nRelays: {}",
                        hl.comment.as_deref().unwrap_or("<none>"),
                        hl.source_url.as_deref().unwrap_or("<none>"),
                        hl.relays.join(", ")
                    ),
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the highlight; nothing was signed",
                )
            }
            NostrSignOperation::List(list) => match list.kind {
                hkask_nostr_signer::RELAY_LIST_KIND => (
                    format!(
                        "Publish NIP-65 relay list ({} relay(s), kind:10002)?",
                        list.relay_urls.len()
                    ),
                    format!(
                        "NIP-65 relays:\n{}\n\nPublishing to relays: {}",
                        list.relay_urls.join("\n"),
                        list.relays.join(", ")
                    ),
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the NIP-65 relay list; nothing was signed",
                ),
                hkask_nostr_signer::DM_RELAYS_KIND => (
                    format!(
                        "Publish NIP-17 DM inbox relay list ({} relay(s), kind:10050)?",
                        list.relay_urls.len()
                    ),
                    format!(
                        "DM inbox relays:\n{}\n\nPublishing to relays: {}",
                        list.relay_urls.join("\n"),
                        list.relays.join(", ")
                    ),
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the DM relay list; nothing was signed",
                ),
                hkask_nostr_signer::NUTZAP_INFO_KIND => (
                    format!(
                        "Publish NIP-61 nutzap info ({} mint(s), {} relay(s), kind:10019)?",
                        list.mint_urls.len(),
                        list.relay_urls.len()
                    ),
                    format!(
                        "Mints: {}\nNutzap relays: {}\nP2PK pubkey: {}\nPublishing to relays: {}",
                        list.mint_urls.join(", "),
                        list.relay_urls.join(", "),
                        list.p2pk_pubkey_hex.as_deref().unwrap_or(""),
                        list.relays.join(", ")
                    ),
                    [APPROVE_LABEL, DECLINE_LABEL],
                    "the operator did not approve the nutzap info update; nothing was signed",
                ),
                _ => {
                    let label = if list.kind == hkask_nostr_signer::BOOKMARK_LIST_KIND {
                        "bookmark list (NIP-51 kind:10003)"
                    } else {
                        "mute list (NIP-51 kind:10000)"
                    };
                    (
                        format!(
                            "Publish updated Nostr {label} ({} event(s), {} pubkey(s))?",
                            list.event_ids_hex.len(),
                            list.pubkeys_hex.len()
                        ),
                        format!(
                            "This replaces your {label} across relays.\nRelays: {}",
                            list.relays.join(", ")
                        ),
                        [APPROVE_LABEL, DECLINE_LABEL],
                        "the operator did not approve the list update; nothing was signed",
                    )
                }
            },
        };
        let answer = cx.update_window(window, |_, window, cx| {
            window.prompt(PromptLevel::Warning, &message, Some(&detail), &buttons, cx)
        });
        let approved = match answer {
            Ok(receiver) => answer_approves(receiver.await),
            Err(_) => false,
        };
        if !approved {
            let _ = reply.send(Err(refusal("Cancelled", decline_msg)));
            return;
        }
    }

    let outcome = sign_operation_after_approval(operation, &configured_npub, cx).await;
    if reply.send(outcome).is_err() {
        tracing::debug!(
            target: "reg.inference",
            "Nostr sign caller disconnected after approval; the signed event was dropped"
        );
    }
}

/// Rebase a Blossom upload token's validity window onto `now_secs` at
/// post-approval signing time so human review latency on the native confirmation
/// dialog never consumes or expires the token's TTL before the upload starts.
fn rebase_blossom_window(mut operation: NostrSignOperation, now_secs: u64) -> NostrSignOperation {
    if let NostrSignOperation::BlossomUpload {
        created_at,
        expiration,
        ..
    } = &mut operation
    {
        let requested_ttl = expiration.saturating_sub(*created_at).max(300);
        let fresh_created_at = now_secs.saturating_sub(5);
        if fresh_created_at > *created_at {
            *created_at = fresh_created_at;
        }
        let min_expiration = created_at.saturating_add(requested_ttl);
        if *expiration < min_expiration {
            *expiration = min_expiration;
        }
    }
    operation
}

/// Read the two slots and sign `operation` on the background executor.
pub(crate) async fn sign_operation_after_approval(
    operation: NostrSignOperation,
    configured_npub: &str,
    cx: &mut AsyncApp,
) -> Result<serde_json::Value, InferenceErrorPayload> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let operation = rebase_blossom_window(operation, now_secs);
    let provider: Arc<dyn CredentialsProvider> =
        cx.update(|cx| zed_credentials_provider::global(cx));
    let read = |url: &'static str| {
        let provider = provider.clone();
        let cx = cx.clone();
        async move {
            match provider.read_credentials(url, &cx).await {
                Ok(Some((_, bytes))) if !bytes.is_empty() => {
                    Ok(Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()))
                }
                Ok(_) => Err(refusal(
                    "NotConfigured",
                    "no Nostr key is saved; enter it on Settings > Kask > Nostr",
                )),
                Err(error) => Err(refusal(
                    "Unavailable",
                    format!("the keychain could not be read: {error}"),
                )),
            }
        }
    };
    let ncryptsec = read(crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL).await?;
    let password = read(crate::NOSTR_PASSWORD_CREDENTIAL_URL).await?;
    let configured_npub = configured_npub.to_string();
    cx.background_executor()
        .spawn(async move {
            sign_operation_with_slots(&ncryptsec, &password, &configured_npub, &operation)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway key made at run time from the published NIP-19 example
    /// nsec (specification test data, not anyone's key), encrypted under a
    /// test password at NIP-49's cost.
    const EXAMPLE_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const EXAMPLE_NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn encrypted_example(password: &str) -> String {
        hkask_nostr_signer::encrypt_nsec(EXAMPLE_NSEC, password).expect("encrypts")
    }

    /// expect: nothing is enqueued without the sign grant, with no text, or
    /// with text over the wire bound; a worktree grant does not sign.
    #[test]
    fn admission_refuses_without_the_sign_grant_or_a_note() {
        let server = format!("sign-test-{}", uuid::Uuid::new_v4());
        assert_eq!(admit(None, Some("hi"), Some(1)).unwrap_err().code, "Auth");
        let worktree = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::WORKTREE_SPAWN.to_string()],
        )
        .expect("grant");
        assert_eq!(
            admit(Some(&worktree), Some("hi"), Some(1))
                .unwrap_err()
                .code,
            "Auth"
        );
        let signing = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::NOSTR_SIGN.to_string()],
        )
        .expect("grant");
        assert_eq!(
            admit(Some(&signing), Some("  "), Some(1)).unwrap_err().code,
            "InvalidArgument"
        );
        let long = "x".repeat(MAX_CONTENT_CHARS + 1);
        assert_eq!(
            admit(Some(&signing), Some(&long), Some(1))
                .unwrap_err()
                .code,
            "InvalidArgument"
        );
        assert_eq!(
            admit(Some(&signing), Some("hi"), None).unwrap_err().code,
            "InvalidArgument"
        );
        assert_eq!(
            admit(Some(&signing), Some("hi"), Some(1)).expect("admitted"),
            ("hi".to_string(), 1)
        );
        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    /// expect: only the first button approves; a closed prompt does not.
    #[test]
    fn only_the_publish_button_approves() {
        assert!(answer_approves::<()>(Ok(0)));
        assert!(!answer_approves::<()>(Ok(1)));
        let (tx, rx) = futures::channel::oneshot::channel::<usize>();
        drop(tx);
        assert!(!answer_approves(futures::executor::block_on(rx)));
        let text = approval_text(EXAMPLE_NPUB, &["wss://a".into(), "wss://b".into()]);
        assert!(text.contains(EXAMPLE_NPUB));
        assert!(text.contains("wss://a, wss://b"));
        assert!(approval_text("", &[]).contains("no relay"));
    }

    /// expect: with the right password and a matching or empty identity, the
    /// saved slots yield a verified kind 1 event under the key's npub; the
    /// wrong password, a different identity, or a tagged kind are refused and
    /// no refusal carries key material.
    #[test]
    fn the_slots_sign_a_kind_1_note_under_the_saved_identity_or_refuse() {
        let ncryptsec = encrypted_example("correct horse");
        for configured in ["", EXAMPLE_NPUB] {
            let event = sign_with_slots(
                &ncryptsec,
                "correct horse",
                configured,
                "hello",
                1_700_000_000,
            )
            .expect("signs");
            assert_eq!(event["kind"], 1);
            assert_eq!(event["content"], "hello");
            assert_eq!(event["created_at"], 1_700_000_000u64);
            assert_eq!(event["tags"], serde_json::json!([]));
            let parsed = hkask_nostr_core::Event::from_json(&event.to_string())
                .expect("the returned event verifies");
            assert_eq!(hkask_nostr_core::encode_npub(parsed.pubkey()), EXAMPLE_NPUB);
        }
        let wrong = sign_with_slots(&ncryptsec, "wrong", "", "hello", 1).unwrap_err();
        assert_eq!(wrong.code, "Auth");
        let other = sign_with_slots(&ncryptsec, "correct horse", "npub1someoneelse", "hello", 1)
            .unwrap_err();
        assert_eq!(other.code, "Auth");
        for message in [wrong.message, other.message] {
            assert!(!message.contains("nsec1"));
            assert!(!message.contains("ncryptsec1"));
            assert!(!message.contains("correct horse"));
        }
    }

    /// expect: `admit_blossom_upload` and `sign_operation_with_slots` enforce
    /// the grant, expiration, server-domain and SHA-256 bounds, and produce
    /// verified `kind: 24242` Blossom auth tokens and `kind: 1` `imeta` notes.
    #[test]
    fn blossom_upload_and_imeta_note_sign_and_verify() {
        let server = format!("sign-blossom-test-{}", uuid::Uuid::new_v4());
        let signing = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::NOSTR_SIGN.to_string()],
        )
        .expect("grant");
        let sha = "b1674191a88ec5cdd733e4240a81803105dc412d6c6708d53ab94fc248f4f553".to_string();
        let blob = BlossomBlobSummary {
            path: "/tmp/Knife_Fight.mp4".to_string(),
            mime_type: "video/mp4".to_string(),
            sha256: sha.clone(),
            size: 29_028_878,
        };
        assert_eq!(
            admit_blossom_upload(
                None,
                Some(100),
                Some(400),
                vec!["blossom.example.com".into()],
                vec!["https://blossom.example.com".into()],
                vec![blob.clone()],
            )
            .unwrap_err()
            .code,
            "Auth"
        );
        let upload_op = admit_blossom_upload(
            Some(&signing),
            Some(1_700_000_000),
            Some(1_700_000_300),
            vec!["blossom.example.com".into()],
            vec!["https://blossom.example.com".into()],
            vec![blob],
        )
        .expect("valid Blossom upload admitted");

        let ncryptsec = encrypted_example("correct horse");
        let auth_event =
            sign_operation_with_slots(&ncryptsec, "correct horse", EXAMPLE_NPUB, &upload_op)
                .expect("signs Blossom auth");
        assert_eq!(auth_event["kind"], 24_242);
        let parsed_auth = hkask_nostr_core::Event::from_json(&auth_event.to_string())
            .expect("Blossom auth verifies");
        assert_eq!(parsed_auth.kind(), 24_242);

        let imeta_op = admit_note(
            Some(&signing),
            Some("watch https://blossom.example.com/clip.mp4"),
            Some(1_700_000_001),
            vec!["wss://relay.example.com".into()],
            vec![NostrMediaAttachment {
                url: "https://blossom.example.com/clip.mp4".to_string(),
                mime_type: "video/mp4".to_string(),
                sha256: sha,
                size: 29_028_878,
                dimensions: Some((1920, 1080)),
                fallback_urls: Vec::new(),
            }],
            None,
        )
        .expect("imeta note admitted");
        let note_event =
            sign_operation_with_slots(&ncryptsec, "correct horse", EXAMPLE_NPUB, &imeta_op)
                .expect("signs imeta note");
        let parsed_note = hkask_nostr_core::Event::from_json(&note_event.to_string())
            .expect("imeta note verifies");
        assert_eq!(parsed_note.kind(), 1);
        assert_eq!(parsed_note.tags().len(), 1);
        assert_eq!(parsed_note.tags()[0][0], "imeta");

        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    /// expect: the post-approval path reads exactly the two named slots
    /// through the credentials provider and returns a verified event; with
    /// no key saved it refuses `NotConfigured` and reads nothing else. Runs
    /// against the fake provider, never the keyring.
    #[gpui::test]
    async fn after_approval_the_two_slots_are_read_and_the_note_is_signed(
        cx: &mut gpui::TestAppContext,
    ) {
        let fake = cx.update(zed_credentials_provider::set_fake_credentials_provider);
        let mut async_cx = cx.to_async();

        let missing = sign_operation_after_approval(
            NostrSignOperation::Note {
                content: "hello".to_string(),
                created_at: 1,
                relays: Vec::new(),
                imeta: Vec::new(),
                quote: None,
            },
            "",
            &mut async_cx,
        )
        .await
        .unwrap_err();
        assert_eq!(missing.code, "NotConfigured");
        assert_eq!(
            fake.take_reads(),
            vec![crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL.to_string()]
        );

        {
            let mut state = fake.state.lock();
            state.keys.insert(
                crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL.to_string(),
                encrypted_example("correct horse").into_bytes(),
            );
            state.keys.insert(
                crate::NOSTR_PASSWORD_CREDENTIAL_URL.to_string(),
                b"correct horse".to_vec(),
            );
        }
        let event = sign_operation_after_approval(
            NostrSignOperation::Note {
                content: "hello".to_string(),
                created_at: 1_700_000_000,
                relays: Vec::new(),
                imeta: Vec::new(),
                quote: None,
            },
            EXAMPLE_NPUB,
            &mut async_cx,
        )
        .await
        .expect("signs");
        let parsed = hkask_nostr_core::Event::from_json(&event.to_string()).expect("verifies");
        assert_eq!(parsed.kind(), 1);
        assert_eq!(parsed.content(), "hello");
        assert_eq!(hkask_nostr_core::encode_npub(parsed.pubkey()), EXAMPLE_NPUB);
        let mut reads = fake.take_reads();
        reads.sort();
        assert_eq!(
            reads,
            vec![
                crate::NOSTR_NCRYPTSEC_CREDENTIAL_URL.to_string(),
                crate::NOSTR_PASSWORD_CREDENTIAL_URL.to_string(),
            ]
        );
    }

    #[test]
    fn blossom_window_rebases_onto_approval_time_so_prompt_wait_never_expires_token() {
        let op = NostrSignOperation::BlossomUpload {
            created_at: 1_700_000_000,
            expiration: 1_700_000_300,
            server_domains: vec!["blossom.primal.net".to_string()],
            server_urls: vec!["https://blossom.primal.net".to_string()],
            blobs: vec![hkask_types::nostr_wire::BlossomBlobSummary {
                path: "/tmp/clip.mp4".to_string(),
                mime_type: "video/mp4".to_string(),
                sha256: "a".repeat(64),
                size: 1024,
            }],
        };
        let rebased = rebase_blossom_window(op, 1_700_001_000);
        match rebased {
            NostrSignOperation::BlossomUpload {
                created_at,
                expiration,
                ..
            } => {
                assert_eq!(created_at, 1_700_000_995);
                assert_eq!(expiration, 1_700_001_295);
            }
            NostrSignOperation::Note { .. }
            | NostrSignOperation::ZapRequest(_)
            | NostrSignOperation::Nutzap(_)
            | NostrSignOperation::Reaction(_)
            | NostrSignOperation::Reply(_)
            | NostrSignOperation::Deletion(_)
            | NostrSignOperation::Repost(_)
            | NostrSignOperation::Follows(_)
            | NostrSignOperation::Article(_)
            | NostrSignOperation::List(_)
            | NostrSignOperation::Profile(_)
            | NostrSignOperation::Highlight(_) => panic!("expected BlossomUpload"),
        }
    }

    #[test]
    fn admit_note_refuses_loopback_imeta_url_when_publishing_to_public_relays() {
        let server = format!("sign-gate-test-{}", uuid::Uuid::new_v4());
        let signing = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::NOSTR_SIGN.to_string()],
        )
        .expect("mints grant");
        let local_att = NostrMediaAttachment {
            url: "http://127.0.0.1:41234/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.mp4"
                .to_string(),
            mime_type: "video/mp4".to_string(),
            sha256: "a".repeat(64),
            size: 1024,
            dimensions: None,
            fallback_urls: Vec::new(),
        };

        let public_err = admit_note(
            Some(&signing),
            Some("clip"),
            Some(1_700_000_000),
            vec!["wss://relay.damus.io".to_string()],
            vec![local_att.clone()],
            None,
        )
        .expect_err("loopback imeta URL must be refused when publishing to public relays");
        assert_eq!(public_err.code, "InvalidArgument");

        // Local-only staging (only loopback relay) allows loopback imeta URL.
        assert!(
            admit_note(
                Some(&signing),
                Some("clip"),
                Some(1_700_000_000),
                vec!["ws://127.0.0.1:7447".to_string()],
                vec![local_att],
                None,
            )
            .is_ok()
        );
    }

    #[test]
    fn zap_and_nutzap_require_zap_grant_and_sign_and_verify() {
        let server = format!("test-zap-sign-{}", uuid::Uuid::new_v4());
        let note_grant = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::NOSTR_SIGN.to_string()],
        )
        .expect("note grant");
        let zap_wire = NostrZapRequestWire {
            created_at: 1_700_000_000,
            recipient_pubkey_hex: "aa".repeat(32),
            amount_msat: 21_000,
            relays: vec!["wss://relay.example.com".to_string()],
            lnurl: None,
            event_id_hex: None,
            content: "great post".to_string(),
        };
        let nutzap_wire = NostrNutzapWire {
            created_at: 1_700_000_000,
            recipient_pubkey_hex: "bb".repeat(32),
            mint_url: "https://mint.example.com".to_string(),
            proofs_json: vec![r#"{"amount":100,"id":"00ad","secret":"s","C":"02..."}"#.to_string()],
            event_id_hex: None,
            content: "nutzap".to_string(),
        };

        assert_eq!(
            admit_zap_request(Some(&note_grant), Some(&zap_wire))
                .unwrap_err()
                .code,
            "Auth"
        );
        assert_eq!(
            admit_nutzap(Some(&note_grant), Some(&nutzap_wire))
                .unwrap_err()
                .code,
            "Auth"
        );

        let zap_grant = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::NOSTR_ZAP_SIGN.to_string()],
        )
        .expect("zap grant");
        let zap_op = admit_zap_request(Some(&zap_grant), Some(&zap_wire)).expect("admit zap");
        let nutzap_op = admit_nutzap(Some(&zap_grant), Some(&nutzap_wire)).expect("admit nutzap");

        let enc = encrypted_example("correct horse");
        let signed_zap = sign_operation_with_slots(&enc, "correct horse", EXAMPLE_NPUB, &zap_op)
            .expect("sign kind 9734");
        let parsed_zap = hkask_nostr_core::Event::from_json(&signed_zap.to_string())
            .expect("kind 9734 verifies");
        assert_eq!(parsed_zap.kind(), hkask_nostr_core::ZAP_REQUEST_KIND);

        let signed_nz = sign_operation_with_slots(&enc, "correct horse", EXAMPLE_NPUB, &nutzap_op)
            .expect("sign kind 9321");
        let parsed_nz_ev =
            hkask_nostr_core::Event::from_json(&signed_nz.to_string()).expect("kind 9321 verifies");
        let parsed_nz = hkask_nostr_core::parse_nutzap(&parsed_nz_ev).expect("parse nutzap");
        assert_eq!(parsed_nz.mint_url, "https://mint.example.com");
        assert_eq!(parsed_nz.proofs_json.len(), 1);

        crate::delegation_grants::revoke_delegation_grant(&server);
    }
}
