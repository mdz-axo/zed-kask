//! MCP tool-response envelope unwrapping — the single seam for the
//! `{"content": <value>}` envelope produced by `execute_tool_semantic`
//! (`hkask-mcp-server`) and serialized by `McpToolOutput::to_json_string`.
//!
//! Extracted so every consumer — the zed panel's `invoke_tool` path, corpus
//! tool responses, and MCP server test helpers — unwraps the envelope the
//! same way. The `.rules` trap: an extractor that reads a tool response must
//! unwrap `content` first, or every field read returns `None`/`Ok(None)`
//! with no error. Do not re-implement `value.get("content")` locally.

use serde_json::Value;

/// Parse a tool-response string and unwrap the `content` envelope.
///
/// `{"content": {…}}` → `{…}`. Defensive: if a future invoker returns the
/// payload directly (no `content` wrapper), the whole value is returned
/// rather than `None`. Returns `None` only on unparseable input.
pub fn parse_tool_response(output: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(output).ok()?;
    Some(unwrap_tool_envelope(value))
}

/// Unwrap the `content` envelope from an already-parsed tool response.
///
/// `{"content": {…}}` → `{…}`; any other value is returned unchanged.
pub fn unwrap_tool_envelope(value: Value) -> Value {
    value.get("content").cloned().unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tool_response_unwraps_content_envelope() {
        let out = r#"{"content":{"agents":[{"agent_id":"a"}]}}"#;
        let parsed = parse_tool_response(out).expect("valid envelope");
        assert_eq!(parsed["agents"][0]["agent_id"], "a");
        assert!(
            parsed.get("content").is_none(),
            "envelope must be unwrapped"
        );
    }

    #[test]
    fn parse_tool_response_returns_inner_when_no_envelope() {
        // Defensive: if a future invoker returns the payload directly (no
        // `content` wrapper), the helper returns the whole value rather than
        // failing — the field reads then work either way.
        let out = r#"{"agents":[{"agent_id":"b"}]}"#;
        let parsed = parse_tool_response(out).expect("bare payload");
        assert_eq!(parsed["agents"][0]["agent_id"], "b");
    }

    #[test]
    fn parse_tool_response_none_on_garbage() {
        assert_eq!(parse_tool_response("not json"), None);
        assert_eq!(parse_tool_response(""), None);
    }

    #[test]
    fn unwrap_tool_envelope_leaves_non_envelopes_unchanged() {
        let bare = serde_json::json!({ "direct": true });
        assert_eq!(unwrap_tool_envelope(bare.clone()), bare);
        let enveloped = serde_json::json!({ "content": { "inner": 1 } });
        assert_eq!(
            unwrap_tool_envelope(enveloped),
            serde_json::json!({ "inner": 1 })
        );
    }
}
