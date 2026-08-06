//! URL utility functions shared across MCP servers.

/// Extract the 11-character YouTube video ID from a URL.
///
/// Handles both `youtube.com/watch?v=...` and `youtu.be/...` formats.
/// Returns `None` if the URL doesn't contain a valid 11-character video ID
/// (alphanumeric + `_` + `-`).
///
/// This function is shared between the companies server (corpus mode YouTube
/// transcript fetch) and the corpus server (company discovery YouTube search)
/// so both servers parse video IDs identically.
#[must_use]
pub fn extract_youtube_id(url: &str) -> Option<String> {
    if let Some(position) = url.find("v=") {
        let id: String = url[position + 2..].chars().take(11).collect();
        if is_valid_youtube_id(&id) {
            return Some(id);
        }
    }
    if let Some(position) = url.find("youtu.be/") {
        let id: String = url[position + 9..].chars().take(11).collect();
        if is_valid_youtube_id(&id) {
            return Some(id);
        }
    }
    None
}

/// Check whether a string is a valid 11-character YouTube video ID.
fn is_valid_youtube_id(id: &str) -> bool {
    id.len() == 11
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_watch_url() {
        assert_eq!(
            extract_youtube_id("https://www.youtube.com/watch?v=ceV3RsG946s").as_deref(),
            Some("ceV3RsG946s")
        );
    }

    #[test]
    fn extract_from_short_url() {
        assert_eq!(
            extract_youtube_id("https://youtu.be/ceV3RsG946s").as_deref(),
            Some("ceV3RsG946s")
        );
    }

    #[test]
    fn rejects_invalid_urls() {
        assert!(extract_youtube_id("https://example.com").is_none());
        assert!(extract_youtube_id("not a url").is_none());
        assert!(extract_youtube_id("").is_none());
    }

    #[test]
    fn rejects_short_ids() {
        // A 10-character ID is not valid (YouTube IDs are 11 chars).
        assert!(extract_youtube_id("https://youtu.be/abc1234567").is_none());
    }
}
