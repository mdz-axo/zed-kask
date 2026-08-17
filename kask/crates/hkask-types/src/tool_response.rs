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

use crate::error::McpErrorKind;

/// A server-side tool error recovered from the `{"error": ..., "kind": ...}`
/// envelope produced by `McpToolError::to_json_string` (`hkask-mcp-server`).
///
/// This is distinct from a transport-level `InvokeError`: the tool *ran* and
/// returned an error envelope as its output string, so `invoke_tool` returns
/// `Ok(output)`. Consumers that only check `parse_tool_response(...).and_then(
/// from_value::<T>)` see a `None` (no `workspaces`/`agents`/... field) and
/// surface a misleading "Failed to parse ..." instead of the real cause
/// (e.g. `permission_denied` = "no API key configured").
///
/// `kind` is always `Some` when constructed via [`parse_tool_error`]: the
/// helper returns `None` for an unknown kind string rather than producing an
/// unclassified envelope, so a data payload that happens to carry `error`/
/// `kind` fields is not misclassified as a server error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolErrorEnvelope {
    /// The human-readable error message from the server.
    pub message: String,
    /// The typed kind. Always `Some` when constructed via `parse_tool_error`;
    /// kept `Option` so callers constructing envelopes by hand can represent an
    /// unclassified error if needed.
    pub kind: Option<McpErrorKind>,
}

impl ToolErrorEnvelope {
    /// Whether the underlying kind (if known) is retryable. An unknown kind is
    /// treated as non-retryable so a future server variant does not loop.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.kind.is_some_and(McpErrorKind::is_retryable)
    }
}

/// Parse a tool-response string and unwrap the `content` envelope.
///
/// `{"content": {…}}` → `{…}`. Defensive: if a future invoker returns the
/// payload directly (no `content` wrapper), the whole value is returned
/// rather than `None`. Returns `None` only on unparsable input.
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

/// Detect a server-side tool error envelope in a raw tool output string.
///
/// `McpToolError::to_json_string` serializes an error as
/// `{"error": <message>, "kind": <kind display>}` (pinned by golden tests in
/// `hkask-mcp-server/src/server/mod.rs`). This helper parses that shape and
/// returns the typed envelope, so a consumer in the `Ok(output)` branch of
/// `invoke_tool` can route the error through the same classification it uses
/// for the `Err(_)` branch instead of falling into "Failed to parse …".
///
/// Returns `None` when the output is not an error envelope — either it is
/// unparsable, or it is a successful payload (with or without the `content`
/// wrapper). This is the single seam for error-envelope detection: do not
/// re-implement `value.get("error")` locally.
///
/// False-positive safety: a successful payload could in principle carry an
/// `error` field as a *data* value (e.g. an agent card describing an error).
/// To avoid misclassifying such a payload as a server error, this helper
/// requires the `kind` field to be present AND to match a known
/// `McpErrorKind` display string — the server's error wire format is the
/// only producer of that exact `{error, kind}` shape with a known kind. An
/// unknown kind string returns `None` so the payload falls through to the
/// normal parse path rather than being treated as an unclassified error.
#[must_use]
pub fn parse_tool_error(output: &str) -> Option<ToolErrorEnvelope> {
    let value: Value = serde_json::from_str(output).ok()?;
    parse_tool_error_value(&value)
}

/// Like [`parse_tool_error`] but operates on an already-parsed `Value`.
///
/// Useful when the caller already parsed the output (e.g. via
/// `parse_tool_response`) and wants to check whether the *raw* output was an
/// error envelope without re-parsing. Checks the raw value before `content`
/// unwrapping, since the error envelope has no `content` wrapper.
#[must_use]
pub fn parse_tool_error_value(value: &Value) -> Option<ToolErrorEnvelope> {
    let obj = value.as_object()?;
    let message = obj.get("error")?.as_str()?;
    let kind_str = obj.get("kind")?.as_str()?;
    let kind = McpErrorKind::from_kind_str(kind_str)?;
    Some(ToolErrorEnvelope {
        message: message.to_string(),
        kind: Some(kind),
    })
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

    #[test]
    fn parse_tool_error_recovers_permission_denied() {
        // The exact wire format pinned by `error_wire_format_golden_strings`
        // in hkask-mcp-server. "no API key configured" is the real message
        // the swarm server emits when `require_auth()` fails.
        let out = r#"{"error":"no API key configured","kind":"permission_denied"}"#;
        let err = parse_tool_error(out).expect("error envelope");
        assert_eq!(err.message, "no API key configured");
        assert_eq!(err.kind, Some(McpErrorKind::PermissionDenied));
        assert!(!err.is_retryable());
    }

    #[test]
    fn parse_tool_error_recovers_unavailable() {
        let out = r#"{"error":"down","kind":"unavailable"}"#;
        let err = parse_tool_error(out).expect("error envelope");
        assert_eq!(err.message, "down");
        assert_eq!(err.kind, Some(McpErrorKind::Unavailable));
        assert!(err.is_retryable());
    }

    #[test]
    fn parse_tool_error_recovers_unknown_kind_as_unclassified() {
        // An unknown kind string is NOT detected as an error envelope — the
        // helper requires a known McpErrorKind so a data payload that happens
        // to carry `error`/`kind` fields is not misclassified as a server
        // error. A future server variant with a new kind would fall through to
        // the normal parse path (the existing "Failed to parse …" behavior),
        // which is a messaging regression, not a correctness regression, and
        // the kind set is small and stable.
        let out = r#"{"error":"future","kind":"some_new_kind"}"#;
        assert!(parse_tool_error(out).is_none());
    }

    #[test]
    fn parse_tool_error_none_for_successful_payload() {
        // A successful payload (with or without the content wrapper) is not
        // an error envelope, even if it happens to carry an `error` data
        // field — the `kind` field is required and must match a known kind.
        assert!(parse_tool_error(r#"{"content":{"agents":[]}}"#).is_none());
        assert!(parse_tool_error(r#"{"agents":[]}"#).is_none());
        // An `error` data field without a `kind` field is not an envelope.
        assert!(parse_tool_error(r#"{"error":"some data"}"#).is_none());
        // An `error` + `kind` pair where the kind is NOT a known McpErrorKind
        // is still detected as an error envelope (the shape matches), but the
        // kind is recovered as `None` so the caller treats it as unclassified
        // rather than misclassifying — see `parse_tool_error_recovers_unknown_kind_as_unclassified`.
    }

    #[test]
    fn parse_tool_error_none_on_garbage() {
        assert!(parse_tool_error("not json").is_none());
        assert!(parse_tool_error("").is_none());
    }

    #[test]
    fn parse_tool_error_value_handles_envelope_value() {
        // The error envelope has no `content` wrapper, so the value form reads
        // the top-level object directly.
        let value = serde_json::json!({"error": "boom", "kind": "internal"});
        let err = parse_tool_error_value(&value).expect("error envelope");
        assert_eq!(err.kind, Some(McpErrorKind::Internal));
    }
}
