//! Persistence for kask panel conversations via the workspace KVP store.
//!
//! Serializes per-tab conversations to `KeyValueStore` (the same store the
//! agent panel's `ThreadStore` uses), keyed by `kask-panel-{server_index}`.
//! On panel construction, conversations are loaded from the store; on every
//! message push, the active tab's conversation is saved.
//!
//! Only the serializable parts of `KaskMessage` are persisted: `role`,
//! `content`, `thinking`. The `markdown: Entity<Markdown>` and
//! `tool_calls: Vec<Entity<ToolCallCard>>` fields are runtime-only and
//! reconstructed on load (the markdown entity is re-created from `content`;
//! tool-call cards are dropped — they represent transient tool invocations
//! that don't persist across restarts).

use serde::{Deserialize, Serialize};

use crate::{KaskMessage, KaskMessageRole};

/// Serializable subset of `KaskMessage` for persistence.
#[derive(Serialize, Deserialize)]
struct SerializableKaskMessage {
    role: String,
    content: String,
    thinking: Option<String>,
}

impl From<&KaskMessage> for SerializableKaskMessage {
    fn from(msg: &KaskMessage) -> Self {
        let role = match msg.role {
            KaskMessageRole::User => "user",
            KaskMessageRole::Assistant => "assistant",
            KaskMessageRole::System => "system",
        };
        Self {
            role: role.to_string(),
            content: msg.content.clone(),
            thinking: msg.thinking.clone(),
        }
    }
}

impl From<SerializableKaskMessage> for KaskMessage {
    fn from(s: SerializableKaskMessage) -> Self {
        let role = match s.role.as_str() {
            "user" => KaskMessageRole::User,
            "assistant" => KaskMessageRole::Assistant,
            _ => KaskMessageRole::System,
        };
        KaskMessage {
            role,
            content: s.content,
            // For assistant messages with content, reconstruct the markdown
            // entity on first render (lazy — the render path checks
            // `markdown.is_none()` and creates it from `content`).
            markdown: None,
            tool_calls: vec![],
            thinking: s.thinking,
            thinking_expanded: false,
        }
    }
}

/// The KVP key prefix for kask panel conversations.
const KASK_PANEL_KEY_PREFIX: &str = "kask-panel-tab-";

/// Build the KVP key for a given server index.
fn kvp_key(server_index: usize) -> String {
    format!("{KASK_PANEL_KEY_PREFIX}{server_index}")
}

/// Serialize a tab's messages to a JSON string for KVP storage.
pub fn serialize_messages(messages: &[KaskMessage]) -> String {
    let serializable: Vec<SerializableKaskMessage> =
        messages.iter().map(SerializableKaskMessage::from).collect();
    serde_json::to_string(&serializable).unwrap_or_else(|_| "[]".to_string())
}

/// Deserialize messages from a KVP JSON string.
pub fn deserialize_messages(json: &str) -> Vec<KaskMessage> {
    serde_json::from_str::<Vec<SerializableKaskMessage>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(KaskMessage::from)
        .collect()
}

/// Save a tab's conversation to the KVP store (async, best-effort).
pub fn save_tab(server_index: usize, messages: &[KaskMessage], cx: &gpui::App) {
    let key = kvp_key(server_index);
    let value = serialize_messages(messages);
    let kvp = db::kvp::KeyValueStore::global(cx);
    cx.background_executor()
        .spawn(async move {
            let _ = kvp.write_kvp(key, value).await;
        })
        .detach();
}

/// Load a tab's conversation from the KVP store (synchronous read).
/// Returns `None` if the key doesn't exist or deserialization fails.
pub fn load_tab_sync(server_index: usize, cx: &gpui::App) -> Option<Vec<KaskMessage>> {
    let key = kvp_key(server_index);
    let kvp = db::kvp::KeyValueStore::global(cx);
    match kvp.read_kvp(&key) {
        Ok(Some(json)) => Some(deserialize_messages(&json)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize_roundtrip() {
        let messages = vec![
            KaskMessage::system("Welcome"),
            KaskMessage::user("Hello"),
            KaskMessage {
                role: KaskMessageRole::Assistant,
                content: "Hi there".to_string(),
                markdown: None,
                tool_calls: vec![],
                thinking: Some("Let me think...".to_string()),
                thinking_expanded: false,
            },
        ];
        let json = serialize_messages(&messages);
        let restored = deserialize_messages(&json);
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].role, KaskMessageRole::System);
        assert_eq!(restored[0].content, "Welcome");
        assert_eq!(restored[1].role, KaskMessageRole::User);
        assert_eq!(restored[1].content, "Hello");
        assert_eq!(restored[2].role, KaskMessageRole::Assistant);
        assert_eq!(restored[2].content, "Hi there");
        assert_eq!(restored[2].thinking.as_deref(), Some("Let me think..."));
    }

    #[test]
    fn deserialize_empty_json_returns_empty() {
        let result = deserialize_messages("");
        assert!(result.is_empty());
    }

    #[test]
    fn deserialize_invalid_json_returns_empty() {
        let result = deserialize_messages("not json");
        assert!(result.is_empty());
    }

    #[test]
    fn kvp_key_format() {
        assert_eq!(kvp_key(0), "kask-panel-tab-0");
        assert_eq!(kvp_key(4), "kask-panel-tab-4");
    }
}
