//! Shared media wire contract — what the media server
//! (`hkask-mcp-media`) and the `nostr_panel` agree on for the one
//! labelled-click image fetch.
//!
//! The panel links neither the media server nor any other MCP server: it
//! names the server and the tool as strings and the governed invoker routes
//! them. The names live here so both sides read one spelling, and the tool
//! name is pinned against the media server's generated `TOOL_NAMES` by
//! `wire_tool_name_is_the_servers_own` in that server — the same pin chain
//! the relay and client servers carry for their lists.

/// The media server's id — MUST match its `id` in
/// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS`.
pub const MEDIA_SERVER: &str = "media";

/// The one media tool the nostr panel dispatches: fetch one image over
/// HTTPS from a public host into the media server's content-addressed
/// cache and return its local path.
pub const MEDIA_FETCH_IMAGE_TOOL: &str = "media_fetch_image";

/// Download a platform video (YouTube, Vimeo, or direct URL) via `yt-dlp`
/// into the gallery and return its local path.
pub const MEDIA_VIDEO_FETCH_TOOL: &str = "video_fetch";

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: the server id and the tool name are distinct strings, and
    /// the tool name carries no server id separator (an MCP tool name is
    /// how a reader tells which server it belongs to).
    #[test]
    fn the_ids_are_plain_names() {
        assert_eq!(MEDIA_SERVER, "media");
        assert_eq!(MEDIA_FETCH_IMAGE_TOOL, "media_fetch_image");
        assert_eq!(MEDIA_VIDEO_FETCH_TOOL, "video_fetch");
    }
}
