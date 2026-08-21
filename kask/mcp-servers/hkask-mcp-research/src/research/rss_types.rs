use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Request types

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SubscribeRequest {
    pub url: String,
    pub label: Option<String>,
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnsubscribeRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListSubscriptionsRequest {
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FetchRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetEntriesRequest {
    pub stream_id: String,
    pub unread_only: Option<bool>,
    pub starred_only: Option<bool>,
    pub count: Option<u32>,
    pub continuation_token: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MarkReadRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnreadCountRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ImportOpmlRequest {
    pub opml_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoverRequest {
    pub url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct EditTagRequest {
    pub entry_ids: Vec<i64>,
    pub add_read: Option<bool>,
    pub add_starred: Option<bool>,
    pub remove_read: Option<bool>,
    pub remove_starred: Option<bool>,
    pub add_label: Option<String>,
    pub remove_label: Option<String>,
}

// ── Synthetic feed request types ──

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SynthesizeRequest {
    /// Source URL to extract items from.
    pub source_url: String,
    /// Extractor kind: "css", "json_path", or "diff_hash".
    pub extractor_kind: String,
    /// JSON-encoded `ExtractorSpec`: {"items_selector": "...", "fields": {...}, ...}
    pub extractor_spec: String,
    /// Optional feed title. Defaults to the source URL.
    pub title: Option<String>,
    /// Optional feed description.
    pub description: Option<String>,
    /// Optional label for the auto-created subscription.
    pub label: Option<String>,
    /// Optional folder for the auto-created subscription.
    pub folder: Option<String>,
    /// Optional suggested poll interval in seconds.
    pub cadence_hint_secs: Option<i64>,
    /// If true, subscribe to the synthetic feed after creating it.
    /// Default: true.
    pub subscribe: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FetchSyntheticRequest {
    /// Stream ID of the synthetic feed (e.g. "feed/synthetic://123").
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteSyntheticRequest {
    /// Stream ID or feed URL of the synthetic feed to delete.
    pub stream_id: String,
}

// Internal types

pub(crate) struct FetchResult {
    pub feed: feed_rs::model::Feed,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub status: u16,
}

#[derive(Serialize, Deserialize)]
pub struct Continuation {
    pub offset: usize,
    pub stream_id: String,
}
