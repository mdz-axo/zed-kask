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
    "nostr_decode",
    "nostr_fetch_events",
    "nostr_notifications",
    "nostr_profile",
    "nostr_thread",
];

/// The most content characters a tool returns per event.
pub const MAX_CONTENT_CHARS: usize = 4_000;

/// The most tags a tool returns per event.
pub const MAX_TAGS_PER_EVENT: usize = 64;

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
