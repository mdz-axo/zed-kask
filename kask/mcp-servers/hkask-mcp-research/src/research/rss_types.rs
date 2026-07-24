use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Request types

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubscribeRequest {
    pub url: String,
    pub label: Option<String>,
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UnsubscribeRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSubscriptionsRequest {
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FetchRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetEntriesRequest {
    pub stream_id: String,
    pub unread_only: Option<bool>,
    pub starred_only: Option<bool>,
    pub count: Option<u32>,
    pub continuation_token: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarkReadRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UnreadCountRequest {
    pub stream_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportOpmlRequest {
    pub opml_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoverRequest {
    pub url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditTagRequest {
    pub entry_ids: Vec<i64>,
    pub add_read: Option<bool>,
    pub add_starred: Option<bool>,
    pub remove_read: Option<bool>,
    pub remove_starred: Option<bool>,
    pub add_label: Option<String>,
    pub remove_label: Option<String>,
}

// Internal types

pub struct FetchResult {
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
