//! Shared Nostr wire contract — what the relay server
//! (`hkask-mcp-nostr-relay`), the client server (`hkask-mcp-nostr-client`)
//! and the `nostr_panel` agree on.
//!
//! It lives here so the panel reads the servers' output without linking
//! either server: the relay brings an LMDB store and an HTTP listener, the
//! client a relay client, and neither belongs in the editor process.
//!
//! Three things:
//!
//! - the server ids and tool names, each pinned against its server's live
//!   router by a test in that server;
//! - [`WireEvent`], the one shape an event has in any tool's output;
//! - [`LocalRelayAnnouncement`], how the relay tells the client where it
//!   listens. Its port is picked by the operating system at each start
//!   unless the operator fixes one, so settings alone cannot say.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The relay server's id — MUST match its `id` in
/// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS`.
pub const NOSTR_RELAY_SERVER: &str = "nostr-relay";

/// The client server's id — MUST match its `id` in
/// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS`.
pub const NOSTR_CLIENT_SERVER: &str = "nostr-client";

/// The Blossom sidecar server's id — MUST match its `id` in
/// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS`.
pub const NOSTR_BLOSSOM_SERVER: &str = "nostr-blossom";

/// The relay server's tools. Pinned against the server's generated
/// `TOOL_NAMES` by `wire_tool_names_match_the_generated_list` there.
pub const RELAY_TOOL_NAMES: &[&str] = &[
    "nostr_relay_log",
    "nostr_relay_query",
    "nostr_relay_status",
    "nostr_relay_sync",
];

/// The client server's tools. Pinned against the server's generated
/// `TOOL_NAMES` by `wire_tool_names_match_the_generated_list` there.
///
/// No client tool is named under the relay server's `nostr_relay_` prefix:
/// a name is how a reader tells which server a tool belongs to.
pub const CLIENT_TOOL_NAMES: &[&str] = &[
    "nostr_account",
    "nostr_author_relays",
    "nostr_blossom_archive",
    "nostr_blossom_mirror",
    "nostr_decode",
    "nostr_fetch_events",
    "nostr_notifications",
    "nostr_prepare_zap",
    "nostr_profile",
    "nostr_publish_article",
    "nostr_publish_deletion",
    "nostr_publish_follows",
    "nostr_publish_highlight",
    "nostr_publish_list",
    "nostr_publish_note",
    "nostr_publish_nutzap",
    "nostr_publish_profile",
    "nostr_publish_reaction",
    "nostr_publish_reply",
    "nostr_publish_repost",
    "nostr_thread",
];

/// The Blossom sidecar server's tools. Pinned against the server's generated
/// `TOOL_NAMES` by `wire_tool_names_match_the_generated_list` there.
pub const BLOSSOM_TOOL_NAMES: &[&str] = &[
    "nostr_blossom_list",
    "nostr_blossom_log",
    "nostr_blossom_status",
];

/// The most content characters a tool returns per event.
pub const MAX_CONTENT_CHARS: usize = 4_000;

/// The most tags a tool returns per event.
pub const MAX_TAGS_PER_EVENT: usize = 64;

/// Maximum media attachments on one published note.
pub const MAX_MEDIA_ATTACHMENTS: usize = 4;

/// Maximum size in bytes of one media file uploaded to Blossom (100 MiB).
pub const MAX_BLOSSOM_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// Summary of one local media file about to be uploaded to Blossom, carried to
/// the editor for operator approval and signed into the BUD-11 `kind: 24242`
/// `x` tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlossomBlobSummary {
    /// Local path or filename shown to the operator in the approval prompt.
    pub path: String,
    /// MIME type (`video/mp4`, `image/jpeg`, …).
    pub mime_type: String,
    /// Lowercase 64-character hex SHA-256 of the file bytes.
    pub sha256: String,
    /// File size in bytes.
    pub size: u64,
}

/// One uploaded media attachment carried as a NIP-92 `imeta` tag on a kind 1
/// note. Validated in `hkask-nostr-signer` so only `imeta` tags can be built
/// from it — never `e`, `p`, `a`, `q` or any reply/mention tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrMediaAttachment {
    /// Publicly accessible URL returned by the primary Blossom server.
    pub url: String,
    /// MIME type (`video/mp4`, `image/jpeg`, …).
    pub mime_type: String,
    /// Lowercase 64-character hex SHA-256 of the uploaded bytes.
    pub sha256: String,
    /// File size in bytes.
    pub size: u64,
    /// Optional `(width, height)` in pixels (`dim WxH`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<(u32, u32)>,
    /// Optional mirror URLs returned by additional Blossom servers (`fallback <url>`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_urls: Vec<String>,
}

/// Optional NIP-10 quote target carried on `SignNostrNote` (`q` + `p` tags).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrQuoteWire {
    pub event_id_hex: String,
    pub author_pubkey_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hint: Option<String>,
}

/// Wire payload for requesting an editor-signed NIP-57 `kind: 9734` zap request
/// (D110 Phase 4). Gated by `host/sign_nostr_zap` and never `autopublish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrZapRequestWire {
    pub created_at: u64,
    pub recipient_pubkey_hex: String,
    pub amount_msat: u64,
    pub relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lnurl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id_hex: Option<String>,
    #[serde(default)]
    pub content: String,
}

/// Wire payload for requesting an editor-signed NIP-61 `kind: 9321` nutzap
/// (D110 Phase 4). Gated by `host/sign_nostr_zap` and never `autopublish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrNutzapWire {
    pub created_at: u64,
    pub recipient_pubkey_hex: String,
    pub mint_url: String,
    pub proofs_json: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id_hex: Option<String>,
    #[serde(default)]
    pub content: String,
}

/// Wire payload for requesting an editor-signed NIP-25 `kind: 7` reaction
/// (<https://nostr-nips.com/nip-25>). Gated by `host/sign_nostr_note`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrReactionWire {
    pub created_at: u64,
    pub target_event_id_hex: String,
    pub target_pubkey_hex: String,
    pub content: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-10 (`kind: 1`,
/// <https://nostr-nips.com/nip-10>) or NIP-22 (`kind: 1111`,
/// <https://nostr-nips.com/nip-22>) reply/comment. Gated by `host/sign_nostr_note`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrReplyWire {
    pub created_at: u64,
    pub root_event_id_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_event_id_hex: Option<String>,
    pub target_pubkey_hex: String,
    pub root_kind: u16,
    pub content: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-09 `kind: 5` event deletion
/// request (<https://nostr-nips.com/nip-09>). Gated by `host/sign_nostr_note`
/// and NEVER bypassed by `autopublish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrDeletionWire {
    pub created_at: u64,
    pub target_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<u16>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-18 repost (`kind: 6` for
/// `kind: 1` notes, `kind: 16` generic repost otherwise;
/// <https://nostr-nips.com/nip-18>). Gated by `host/sign_nostr_note`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrRepostWire {
    pub created_at: u64,
    pub target_event_id_hex: String,
    pub target_pubkey_hex: String,
    pub target_kind: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hint: Option<String>,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-02 `kind: 3` follow list
/// (<https://nostr-nips.com/nip-02>). Gated by `host/sign_nostr_note` and
/// NEVER bypassed by `autopublish` because `kind: 3` replaces the full list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrFollowsWire {
    pub created_at: u64,
    pub followed_pubkeys_hex: Vec<String>,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-23 `kind: 30023` long-form
/// Markdown article (<https://nostr-nips.com/nip-23>). Gated by
/// `host/sign_nostr_note` and NEVER bypassed by `autopublish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrArticleWire {
    pub created_at: u64,
    pub slug: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub published_at: u64,
    #[serde(default)]
    pub hashtags: Vec<String>,
    pub content: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed replaceable list:
/// `kind: 10000` (NIP-51 Mute list), `kind: 10003` (NIP-51 Bookmark list),
/// `kind: 10050` (NIP-17 DM inbox relay list), or `kind: 10019` (NIP-61 Nutzap
/// info). Gated by `host/sign_nostr_note` and NEVER bypassed by `autopublish`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrListWire {
    pub created_at: u64,
    pub kind: u16,
    #[serde(default)]
    pub event_ids_hex: Vec<String>,
    #[serde(default)]
    pub pubkeys_hex: Vec<String>,
    #[serde(default)]
    pub relay_urls: Vec<String>,
    #[serde(default)]
    pub mint_urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p2pk_pubkey_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserved_tags: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_content: Option<String>,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-01 `kind: 0` profile
/// metadata event (<https://nostr-nips.com/nip-01>). Gated by
/// `host/sign_nostr_note` and NEVER bypassed by `autopublish` because `kind: 0`
/// replaces the operator's entire public profile metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NostrProfileWire {
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nip05: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lud16: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lud06: Option<String>,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// Wire payload for requesting an editor-signed NIP-84 `kind: 9802` highlight
/// event (<https://nostr-nips.com/nip-84>).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NostrHighlightWire {
    pub created_at: u64,
    pub quoted_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_author_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default)]
    pub relays: Vec<String>,
}

/// One Nostr event as any tool reports it.
///
/// `content` and `tags` are text written by the event's author, not by this
/// system: data, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEvent {
    /// The event id, lower-case hex.
    pub id: String,
    /// The author's public key, lower-case hex.
    pub author: String,
    /// The author's claimed creation time, in seconds since the Unix epoch.
    pub created_at: u64,
    /// The event kind.
    pub kind: u16,
    /// The content, cut to [`MAX_CONTENT_CHARS`].
    pub content: String,
    /// Whether `content` was cut.
    pub content_truncated: bool,
    /// The tags, cut to [`MAX_TAGS_PER_EVENT`].
    pub tags: Vec<Vec<String>>,
    /// Whether `tags` was cut.
    pub tags_truncated: bool,
    /// The relays that returned this event, by address. Empty when the
    /// event was read from the local archive's store directly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub served_by: Vec<String>,
}

impl WireEvent {
    /// Build the bounded form of an event from its parts.
    pub fn bounded<'tag>(
        id: String,
        author: String,
        created_at: u64,
        kind: u16,
        content: &str,
        tags: impl IntoIterator<Item = &'tag [String]>,
    ) -> Self {
        let (content, content_truncated) = truncate_chars(content, MAX_CONTENT_CHARS);
        let mut kept: Vec<Vec<String>> = Vec::new();
        let mut tags_truncated = false;
        for tag in tags {
            if kept.len() == MAX_TAGS_PER_EVENT {
                tags_truncated = true;
                break;
            }
            kept.push(tag.to_vec());
        }
        Self {
            id,
            author,
            created_at,
            kind,
            content,
            content_truncated,
            tags: kept,
            tags_truncated,
            served_by: Vec::new(),
        }
    }
}

/// Cut `text` to at most `max` characters, on a character boundary, and say
/// whether anything was cut.
pub fn truncate_chars(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((index, _)) => (text[..index].to_string(), true),
        None => (text.to_string(), false),
    }
}

/// File name of the relay's announcement, beside its archive directory.
pub const LOCAL_RELAY_ANNOUNCEMENT_FILE: &str = "listener.json";

/// Where the relay announces its listener, relative to the data directory:
/// `mcp/nostr-relay/listener.json`. Resolve it with
/// [`crate::agent_paths::resolve_under_data_dir`].
pub fn local_relay_announcement_path() -> PathBuf {
    crate::agent_paths::mcp_server_subdir(NOSTR_RELAY_SERVER, LOCAL_RELAY_ANNOUNCEMENT_FILE)
}

/// What the relay writes when its listener is up. The client does not trust
/// the address as written: it accepts only a loopback websocket address
/// with a port, and nothing else from this file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRelayAnnouncement {
    /// The listener's address, `ws://127.0.0.1:<port>`.
    pub address: String,
}

/// Where the Blossom sidecar announces its listener, relative to the data
/// directory: `mcp/nostr-blossom/listener.json`. Resolve it with
/// [`crate::agent_paths::resolve_under_data_dir`].
pub fn local_blossom_announcement_path() -> PathBuf {
    crate::agent_paths::mcp_server_subdir(NOSTR_BLOSSOM_SERVER, LOCAL_RELAY_ANNOUNCEMENT_FILE)
}

/// What the Blossom sidecar writes when its listener is up. The client
/// accepts only a loopback HTTP address (`http://127.0.0.1:<port>`) with a
/// non-zero port, and nothing else from this file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBlossomAnnouncement {
    /// The listener's address, `http://127.0.0.1:<port>`.
    pub address: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    /// expect: an event within the bounds is carried whole and marked uncut.
    #[test]
    fn a_small_event_is_carried_whole() {
        let tags = [tag(&["e", "abc"]), tag(&["p", "def"])];
        let event = WireEvent::bounded(
            "id".to_string(),
            "author".to_string(),
            7,
            1,
            "héllo",
            tags.iter().map(Vec::as_slice),
        );
        assert_eq!(event.content, "héllo");
        assert!(!event.content_truncated);
        assert_eq!(event.tags, tags);
        assert!(!event.tags_truncated);
        assert!(event.served_by.is_empty());
    }

    /// expect: content is cut on a character boundary at the bound, and tags
    /// at theirs, and each cut is marked.
    #[test]
    fn long_content_and_long_tag_lists_are_cut_and_marked() {
        let content = "é".repeat(MAX_CONTENT_CHARS + 1);
        let tags: Vec<Vec<String>> = (0..=MAX_TAGS_PER_EVENT)
            .map(|index| tag(&["t", &index.to_string()]))
            .collect();
        let event = WireEvent::bounded(
            String::new(),
            String::new(),
            0,
            1,
            &content,
            tags.iter().map(Vec::as_slice),
        );
        assert_eq!(event.content.chars().count(), MAX_CONTENT_CHARS);
        assert!(event.content_truncated);
        assert_eq!(event.tags.len(), MAX_TAGS_PER_EVENT);
        assert!(event.tags_truncated);

        let exact = "é".repeat(MAX_CONTENT_CHARS);
        let (kept, cut) = truncate_chars(&exact, MAX_CONTENT_CHARS);
        assert_eq!(kept, exact);
        assert!(!cut, "content exactly at the bound is not marked cut");
    }

    /// expect: the relay's output, which names no serving relay, and the
    /// client's, which does, both read back as the same type.
    #[test]
    fn served_by_is_left_out_when_empty_and_read_back_when_present() {
        let mut event =
            WireEvent::bounded(String::new(), String::new(), 0, 1, "", std::iter::empty());
        let without = serde_json::to_value(&event).expect("serializes");
        assert!(without.get("served_by").is_none());
        assert_eq!(
            serde_json::from_value::<WireEvent>(without).expect("reads back"),
            event
        );

        event.served_by.push("wss://relay.example.com".to_string());
        let with = serde_json::to_value(&event).expect("serializes");
        assert_eq!(with["served_by"][0], "wss://relay.example.com");
        assert_eq!(
            serde_json::from_value::<WireEvent>(with).expect("reads back"),
            event
        );
    }

    /// expect: the announcement sits beside the relay's archive directory.
    #[test]
    fn the_announcement_path_is_under_the_relay_servers_directory() {
        assert_eq!(
            local_relay_announcement_path(),
            PathBuf::from("mcp")
                .join("nostr-relay")
                .join("listener.json")
        );
    }
}
