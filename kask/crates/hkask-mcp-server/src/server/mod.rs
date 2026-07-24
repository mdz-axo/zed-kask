//! MCP server scaffolding — shared helpers for hKask MCP server binaries.
//
//! WebID resolution order: `HKASK_WEBID` → `HKASK_USERPOD_PERSONA` → anonymous.
//! No ambient authority — all identity and credentials flow through `ServerContext`.
//
//! ```rust,ignore
//! use hkask_mcp::server::{run_stdio_server, CredentialRequirement, ServerContext};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     run_stdio_server(
//!         "hkask-mcp-web",
//!         env!("CARGO_PKG_VERSION"),
//!         |ctx: ServerContext| {
//!             Ok(WebServer::new(ctx.webid))
//!         },
//!         vec![],
//!     ).await
//! }
//! ```

mod context;
mod credentials;
mod error;
mod http_helpers;
mod tool_span;
mod transport;
mod validation;

// ── Re-exports ─────────────────────────────────────────────────────────────

pub use context::{CapabilityTier, CredentialRequirement, ServerContext};
pub use credentials::{load_dotenv, resolve_credential};
pub use error::{McpError, McpToolError};
pub use http_helpers::{api_get, api_put, classify_http_error};
pub use tool_span::{
    ExperienceCallback, ToolContext, ToolSpanGuard, execute_tool, execute_tool_semantic,
    record_via_daemon, tool_internal_error,
};
pub use transport::{run_stdio_server, run_stdio_server_with_preloaded};
pub use validation::{
    validate_identifier, validate_path, validate_tool_url, validate_tool_url_permissive,
};

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::McpErrorKind;

    /// Helper: parse a `McpToolError::to_json_string()` output and extract fields.
    fn parse_error_json(json: &str) -> (String, String) {
        let v: serde_json::Value = serde_json::from_str(json).expect("error JSON should be valid");
        let error = v["error"]
            .as_str()
            .expect("should have 'error' field")
            .to_string();
        let kind = v["kind"]
            .as_str()
            .expect("should have 'kind' field")
            .to_string();
        (error, kind)
    }

    #[test]
    fn all_error_kinds_produce_correct_wire_format() {
        let cases = vec![
            (McpToolError::internal("internal bug"), "internal"),
            (McpToolError::unavailable("downstream down"), "unavailable"),
            (McpToolError::timeout("timed out"), "timeout"),
            (McpToolError::not_found("missing resource"), "not_found"),
            (
                McpToolError::invalid_argument("bad input"),
                "invalid_argument",
            ),
            (
                McpToolError::permission_denied("access denied"),
                "permission_denied",
            ),
            (
                McpToolError::rate_limited("too many requests"),
                "rate_limited",
            ),
            (
                McpToolError::failed_precondition("not initialized"),
                "failed_precondition",
            ),
        ];

        for (err, expected_kind) in cases {
            let json = err.to_json_string();
            let (error_msg, kind) = parse_error_json(&json);
            assert!(!error_msg.is_empty(), "error message should not be empty");
            assert_eq!(
                kind, expected_kind,
                "kind field should match McpErrorKind Display"
            );
            // Verify the JSON is valid and has exactly 2 top-level keys
            let v: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(
                v.as_object().unwrap().len() == 2,
                "error JSON should have exactly 2 fields (error + kind)"
            );
        }
    }

    #[test]
    fn error_wire_format_golden_strings() {
        // These exact JSON strings are the contract. Changing them breaks all clients.
        assert_eq!(
            McpToolError::internal("boom").to_json_string(),
            r#"{"error":"boom","kind":"internal"}"#
        );
        assert_eq!(
            McpToolError::not_found("gone").to_json_string(),
            r#"{"error":"gone","kind":"not_found"}"#
        );
        assert_eq!(
            McpToolError::invalid_argument("bad").to_json_string(),
            r#"{"error":"bad","kind":"invalid_argument"}"#
        );
        assert_eq!(
            McpToolError::permission_denied("nope").to_json_string(),
            r#"{"error":"nope","kind":"permission_denied"}"#
        );
        assert_eq!(
            McpToolError::unavailable("down").to_json_string(),
            r#"{"error":"down","kind":"unavailable"}"#
        );
        assert_eq!(
            McpToolError::timeout("late").to_json_string(),
            r#"{"error":"late","kind":"timeout"}"#
        );
        assert_eq!(
            McpToolError::rate_limited("wait").to_json_string(),
            r#"{"error":"wait","kind":"rate_limited"}"#
        );
        assert_eq!(
            McpToolError::failed_precondition("nope").to_json_string(),
            r#"{"error":"nope","kind":"failed_precondition"}"#
        );
    }

    #[test]
    fn error_kind_display_matches_wire_format() {
        for kind in &[
            McpErrorKind::Internal,
            McpErrorKind::Unavailable,
            McpErrorKind::Timeout,
            McpErrorKind::NotFound,
            McpErrorKind::InvalidArgument,
            McpErrorKind::PermissionDenied,
            McpErrorKind::RateLimited,
            McpErrorKind::FailedPrecondition,
        ] {
            let err = McpToolError::new(*kind, "test");
            let json = err.to_json_string();
            let (_, wire_kind) = parse_error_json(&json);
            assert_eq!(
                wire_kind,
                kind.to_string(),
                "wire format kind must match McpErrorKind Display"
            );
        }
    }

    #[test]
    fn classify_http_error_maps_status_codes() {
        use reqwest::StatusCode;

        // 401/403 → PermissionDenied
        let err = classify_http_error("TestSvc", StatusCode::UNAUTHORIZED, "unauthorized");
        assert_eq!(err.kind, McpErrorKind::PermissionDenied);
        let err = classify_http_error("TestSvc", StatusCode::FORBIDDEN, "forbidden");
        assert_eq!(err.kind, McpErrorKind::PermissionDenied);

        // 404 → NotFound
        let err = classify_http_error("TestSvc", StatusCode::NOT_FOUND, "missing");
        assert_eq!(err.kind, McpErrorKind::NotFound);

        // 422 → InvalidArgument
        let err = classify_http_error("TestSvc", StatusCode::UNPROCESSABLE_ENTITY, "bad schema");
        assert_eq!(err.kind, McpErrorKind::InvalidArgument);

        // 429 → RateLimited
        let err = classify_http_error("TestSvc", StatusCode::TOO_MANY_REQUESTS, "rate limited");
        assert_eq!(err.kind, McpErrorKind::RateLimited);

        // 502/503 → Unavailable
        let err = classify_http_error("TestSvc", StatusCode::BAD_GATEWAY, "bad gateway");
        assert_eq!(err.kind, McpErrorKind::Unavailable);
        let err = classify_http_error("TestSvc", StatusCode::SERVICE_UNAVAILABLE, "down");
        assert_eq!(err.kind, McpErrorKind::Unavailable);

        // Other 5xx → Unavailable
        let err = classify_http_error("TestSvc", StatusCode::INTERNAL_SERVER_ERROR, "boom");
        assert_eq!(err.kind, McpErrorKind::Unavailable);

        // Unknown → Internal
        let err = classify_http_error("TestSvc", StatusCode::OK, "unexpected");
        assert_eq!(err.kind, McpErrorKind::Internal);
    }

    // ── Capability Enforcement Tests ─────────────────────────────────────

    #[test]
    fn permission_denied_error_carries_message() {
        let err = McpToolError::permission_denied("agent lacks tool:execute capability");
        assert_eq!(err.kind, McpErrorKind::PermissionDenied);
        assert!(
            err.to_string()
                .contains("agent lacks tool:execute capability")
        );
        let json = err.to_json_string();
        assert!(json.contains("permission_denied"));
        assert!(json.contains("agent lacks tool:execute capability"));
    }

    #[test]
    fn failed_precondition_error_for_expired_token() {
        let err = McpToolError::failed_precondition("delegation token expired at 1000");
        assert_eq!(err.kind, McpErrorKind::FailedPrecondition);
        assert!(err.to_string().contains("delegation token expired"));
    }

    #[test]
    fn rate_limited_error_for_energy_budget_exceeded() {
        let err = McpToolError::rate_limited("energy budget exceeded for tool:execute");
        assert_eq!(err.kind, McpErrorKind::RateLimited);
        assert!(err.to_string().contains("energy budget exceeded"));
    }

    // ── Error Propagation Tests ───────────────────────────────────────────

    #[test]
    fn internal_error_propagates_with_context() {
        let err = McpToolError::internal("downstream inference engine returned 500");
        assert_eq!(err.kind, McpErrorKind::Internal);
        assert!(
            err.to_string()
                .contains("downstream inference engine returned 500")
        );
        // Verify JSON round-trip preserves error context
        let json = err.to_json_string();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"], "downstream inference engine returned 500");
        assert_eq!(parsed["kind"], "internal");
    }

    #[test]
    fn timeout_error_propagates_with_context() {
        let err = McpToolError::timeout("tool:execute timed out after 30s");
        assert_eq!(err.kind, McpErrorKind::Timeout);
        assert!(err.to_string().contains("tool:execute timed out after 30s"));
        let json = err.to_json_string();
        assert!(json.contains("timeout"));
        assert!(json.contains("tool:execute timed out after 30s"));
    }

    #[test]
    fn not_found_error_for_unknown_tool() {
        let err = McpToolError::not_found("unknown tool: none_such");
        assert_eq!(err.kind, McpErrorKind::NotFound);
        assert!(err.to_string().contains("unknown tool: none_such"));
    }

    // ── Tool Discovery Tests ──────────────────────────────────────────────

    #[test]
    fn validate_identifier_accepts_valid_names() {
        assert!(validate_identifier("tool_name", "web_search", 64).is_ok());
        assert!(validate_identifier("tool_name", "file_read", 64).is_ok());
        assert!(validate_identifier("tool_name", "my_tool_123", 64).is_ok());
        assert!(validate_identifier("tool_name", "a", 64).is_ok());
    }

    #[test]
    fn validate_identifier_rejects_invalid_names() {
        assert!(validate_identifier("tool_name", "", 64).is_err());
        assert!(validate_identifier("tool_name", "tool name", 64).is_err()); // space
    }

    #[test]
    fn validate_identifier_rejects_overly_long_names() {
        let long_name = "a".repeat(65);
        assert!(validate_identifier("tool_name", &long_name, 64).is_err());
    }

    #[test]
    fn validate_tool_url_accepts_valid_urls() {
        assert!(validate_tool_url("http://localhost:8080").is_ok());
        assert!(validate_tool_url("https://api.example.com/v1").is_ok());
    }

    #[test]
    fn validate_tool_url_rejects_invalid_urls() {
        assert!(validate_tool_url("not-a-url").is_err());
        assert!(validate_tool_url("").is_err());
    }

    // ── Ontology Concept Contract Tests (P8.1) ───────────────────────────

    /// Verify that common bridge crate constants are valid `&'static str`
    /// for use with `ToolSpanGuard::with_ontology`.
    #[test]
    fn ontology_concepts_are_static_str() {
        // PKO process axis
        let pko_concepts: &[&str] = &[
            "pko:Procedure",
            "pko:Step",
            "pko:StepExecution",
            "pko:ChangeOfStatus",
            "pko:StepVerification",
            "pko:IssueOccurrence",
            "pko:UserFeedbackOccurrence",
            "pko:UserQuestionOccurrence",
        ];
        // DC+BIBO state axis
        let dc_concepts: &[&str] = &[
            "dcterms:title",
            "dcterms:creator",
            "dcterms:Dataset",
            "bibo:Article",
            "bibo:Book",
            "cito:cites",
        ];
        // Domain supplements
        let domain_concepts: &[&str] = &[
            "fibo:Corporation",
            "golem:Character",
            "cogat:episodic_memory",
            "mls:Model",
            "omc:Image",
        ];

        let mut guard = ToolSpanGuard::new("test_tool", &hkask_types::WebID::new());
        for concept in pko_concepts
            .iter()
            .chain(dc_concepts.iter())
            .chain(domain_concepts.iter())
        {
            guard = guard.with_ontology(concept);
        }
        let _ = guard; // suppress unused warning
    }
}
