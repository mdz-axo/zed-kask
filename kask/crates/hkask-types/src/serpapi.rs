//! SerpAPI response shaping shared by the YouTube gatherers — the companies
//! server's `fetch_corpus_transcripts` and the corpus server's YouTube
//! search. Both are leaf MCP servers (neither may depend on the other), so
//! the shared shaping lives here with the other cross-server domain
//! vocabulary (`url_utils`).

use serde_json::Value;

/// Shape a SerpAPI `engine=youtube` search response's `video_results` array
/// into `(title, link, channel)` records. Entries missing a title or link
/// are dropped; a missing channel name falls back to `"unknown"` (the
/// response carries either a string channel or a `{"name": ...}` object);
/// an absent or non-array `video_results` yields an empty vec.
pub fn youtube_video_results(search: &Value) -> Vec<(String, String, String)> {
    search["video_results"]
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|video| {
                    let title = video["title"].as_str()?.to_string();
                    let link = video["link"].as_str()?.to_string();
                    let channel = video["channel"]
                        .as_str()
                        .or_else(|| video["channel"]["name"].as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Some((title, link, channel))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
