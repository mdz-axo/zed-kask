//! MCP tool-response envelope unwrapping — the single seam for the
//! `{"content": <value>}` envelope produced by `execute_tool_semantic`
//! (`hkask-mcp-server`) and serialized by `ToolSpanGuard::ok_json`.
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
/// Useful when the caller already has the structured value (e.g. an MCP
/// `structured_content` field carrying `{"error", "kind"}`) and wants the
/// typed envelope without re-serializing to a string.
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

/// Extract the typed error kind from a `[kind] message` display string —
/// the `McpToolError` Display convention. The single producer on the
/// governed dispatch path is `McpRuntime::dispatch`, which formats failed
/// tool details this way (from the server's `structured_content`) so
/// `invoke` can recover the typed kind for the ledger's per-kind breakdown
/// after `ToolPortError` flattens the detail to a string. The marker is
/// searched anywhere in the text because error Display impls prefix their
/// own context (e.g. `"Invocation failed: [unavailable] …"`). Returns the
/// kind's Display string (e.g. `"unavailable"`) when a `[kind] ` marker
/// carries a known kind; otherwise returns the full text unchanged (the
/// ledger treats `error_kind` as a free-form classification hint).
#[must_use]
pub fn error_kind_from_display(text: &str) -> String {
    let mut search_from = 0;
    while let Some(open) = text[search_from..].find('[') {
        let after_open = &text[search_from + open + 1..];
        if let Some((kind, _)) = after_open.split_once("] ")
            && McpErrorKind::from_kind_str(kind).is_some()
        {
            return kind.to_string();
        }
        search_from += open + 1;
    }
    text.to_string()
}

/// Whether an error-kind string signals a missing configuration or
/// dependency (a binary not installed, a credential not provisioned)
/// rather than tool unreliability. `permission_denied` is unambiguous —
/// the `.rules` MCP pattern classifies missing credentials as
/// authorization failures, and no retry changes authorization.
/// `unavailable` is included for the *reliability* consumers (the
/// RegulationLedger's success-rate math): a server whose dependency is
/// missing is an operator-actionable environment signal, not a degrading
/// domain. Retry-behavior consumers should use `McpErrorKind::is_retryable`
/// instead — `unavailable` can be transient (server restarting).
#[must_use]
pub fn is_config_gap_kind(kind: &str) -> bool {
    matches!(
        McpErrorKind::from_kind_str(kind),
        Some(McpErrorKind::Unavailable) | Some(McpErrorKind::PermissionDenied)
    )
}

/// Extract fenced media-block display hints from a tool output text (the
/// `{"content": ...}` envelope serialized by `ToolSpanGuard::ok_json`).
/// `display_hint` is a single fenced ```media block; `display_hints` is an
/// array (gallery_search, generate_variants). Returns an empty vec for
/// non-JSON or hint-free outputs — ordinary tool results carry nothing.
/// Consumers: the agent's structural rendering (T-V2 — the tool card
/// renders the blocks via the D18 media renderer) and the media panel's
/// viewing pane (surfaces assets from the conversation's tool results).
#[must_use]
pub fn display_hints_from_output_text(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let payload = unwrap_tool_envelope(value);
    let mut hints = Vec::new();
    if let Some(hint) = payload.get("display_hint").and_then(|h| h.as_str()) {
        hints.push(hint.to_string());
    }
    if let Some(array) = payload.get("display_hints").and_then(|h| h.as_array()) {
        hints.extend(array.iter().filter_map(|h| h.as_str()).map(str::to_string));
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_from_display_extracts_known_kind() {
        assert_eq!(
            error_kind_from_display("[unavailable] yt-dlp not found on system PATH"),
            "unavailable"
        );
        assert_eq!(
            error_kind_from_display("[permission_denied] OPENROUTER_API_KEY not configured"),
            "permission_denied"
        );
    }

    #[test]
    fn error_kind_from_display_unknown_kind_returns_full_text() {
        // An unknown kind string is not a typed prefix — the full text is
        // the classification hint, not a misclassified kind.
        let text = "[weird] something";
        assert_eq!(error_kind_from_display(text), text);
    }

    #[test]
    fn error_kind_from_display_no_prefix_returns_full_text() {
        assert_eq!(
            error_kind_from_display("plain failure text"),
            "plain failure text"
        );
        // A bracket that isn't a kind prefix must not be stripped.
        assert_eq!(
            error_kind_from_display("[not a kind] text"),
            "[not a kind] text"
        );
    }

    #[test]
    fn error_kind_from_display_finds_marker_behind_display_prefix() {
        // Error Display impls prefix their own context — the kind marker is
        // searched anywhere in the text, not only at the start.
        assert_eq!(
            error_kind_from_display("Invocation failed: [unavailable] yt-dlp missing"),
            "unavailable"
        );
    }

    #[test]
    fn display_hints_from_output_text_single_and_array() {
        // Single display_hint inside the content envelope.
        let output = serde_json::json!({
            "content": {
                "prompt": "a cat",
                "display_hint": "```media\n{\"kind\":\"image\",\"src\":\"/tmp/a.png\"}\n```"
            }
        })
        .to_string();
        let hints = display_hints_from_output_text(&output);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].starts_with("```media"));

        // display_hints array (gallery_search / generate_variants shape).
        let output = serde_json::json!({
            "content": {
                "results": [],
                "display_hints": [
                    "```media\n{\"kind\":\"image\",\"src\":\"/tmp/1.png\"}\n```",
                    "```media\n{\"kind\":\"image\",\"src\":\"/tmp/2.png\"}\n```"
                ]
            }
        })
        .to_string();
        let hints = display_hints_from_output_text(&output);
        assert_eq!(hints.len(), 2);
        assert!(hints.iter().all(|h| h.starts_with("```media")));
    }

    #[test]
    fn display_hints_from_output_text_no_envelope_or_hints() {
        // Non-JSON text and hint-free payloads return nothing — the helper
        // must not push content for ordinary tool results.
        assert!(display_hints_from_output_text("plain text").is_empty());
        assert!(display_hints_from_output_text("{\"content\":{\"ok\":true}}").is_empty());
        assert!(display_hints_from_output_text("not json at all").is_empty());
    }

    #[test]
    fn is_config_gap_kind_classifies_environment_signals() {
        assert!(is_config_gap_kind("unavailable"));
        assert!(is_config_gap_kind("permission_denied"));
        assert!(!is_config_gap_kind("internal"));
        assert!(!is_config_gap_kind("timeout"));
        assert!(!is_config_gap_kind("unknown-kind"));
    }
}
