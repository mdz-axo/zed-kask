//! ChatTurn — typed projection of a chat episode's content.
//!
//! The `chatted` / `chat_turn` schema (`{"user_input": ..., "agent_response": ...}`)
//! is stored as an opaque `serde_json::Value` inside episodic h_mems. Two
//! consumers — the chat service (`hkask-services-chat`) and the memory MCP
//! server (`hkask-mcp-memory`) — need to project that JSON into typed fields.
//! Without this type, each consumer re-implements the same
//! `value.as_object()?.get("user_input")?.as_str()?` extraction.
//!
//! `ChatTurn` is the single typed representation of that schema. It carries
//! only the content fields; metadata (confidence, observed_at, id, etc.) is
//! accessed from the enclosing `RecalledEpisode` or `HMem` directly.
//!
//! Rendering — how a `ChatTurn` is formatted into a prompt string, a JSON
//! object, or a role/content message — is each surface's responsibility
//! (ADR-060). This type does not format; it only projects.

/// A typed chat turn: the user's input and the agent's response.
///
/// Projected from the `chatted` / `chat_turn` episodic h_mem schema via
/// [`ChatTurn::from_value`]. The canonical field names (`"user_input"`,
/// `"agent_response"`) live in one place: the `from_value` constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTurn {
    pub user_input: String,
    pub agent_response: String,
}

impl ChatTurn {
    /// Project a `ChatTurn` from the `value` field of an episodic h_mem.
    ///
    /// Returns `None` if the value is not an object or is missing either
    /// the `user_input` or `agent_response` string field.
    ///
    /// expect: "I can recall typed chat turns from episodic memory"
    /// pre:  value is a JSON object with `user_input` and `agent_response` string fields
    /// post: returns `Some(ChatTurn)` with both fields populated, or `None` on shape mismatch
    #[must_use]
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        let v = value.as_object()?;
        let user_input = v.get("user_input")?.as_str()?.to_string();
        let agent_response = v.get("agent_response")?.as_str()?.to_string();
        Some(Self {
            user_input,
            agent_response,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_value_extracts_both_fields() {
        let value = serde_json::json!({
            "user_input": "Hello",
            "agent_response": "Hi there!",
        });
        let ct = ChatTurn::from_value(&value).expect("valid chat turn");
        assert_eq!(ct.user_input, "Hello");
        assert_eq!(ct.agent_response, "Hi there!");
    }

    #[test]
    fn from_value_returns_none_for_non_object() {
        let value = serde_json::json!("not an object");
        assert!(ChatTurn::from_value(&value).is_none());
    }

    #[test]
    fn from_value_returns_none_for_missing_field() {
        let value = serde_json::json!({"user_input": "only input"});
        assert!(ChatTurn::from_value(&value).is_none());
    }

    #[test]
    fn from_value_returns_none_for_non_string_field() {
        let value = serde_json::json!({"user_input": 42, "agent_response": "ok"});
        assert!(ChatTurn::from_value(&value).is_none());
    }
}
