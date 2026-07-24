//! Input validation — shared sanitization for MCP tool parameters.

use super::error::McpToolError;

/// Validate a string identifier.
/// Validate an identifier (tool name, server name, etc.).
///
/// expect: "The system validates tool input against safety and length constraints"
/// pre:  name and value are non-empty, max_len > 0
/// post: returns Ok(()) if valid (non-empty, ≤max_len, alphanumeric+hyphen+underscore+dot+colon)
/// post: returns Err if invalid
#[must_use = "result must be used"]
pub fn validate_identifier(name: &str, value: &str, max_len: usize) -> Result<(), McpToolError> {
    if value.is_empty() {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must not be empty"
        )));
    }
    if value.len() > max_len {
        return Err(McpToolError::invalid_argument(format!(
            "{name} exceeds maximum length of {max_len} (got {})",
            value.len()
        )));
    }
    if !value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-' || c == ':')
    {
        return Err(McpToolError::invalid_argument(format!(
            "{name} contains invalid characters (allowed: alphanumeric, _, ., -, :)"
        )));
    }
    Ok(())
}

/// Validate a filesystem path without restricting legitimate filename punctuation.
///
/// expect: "The system validates tool input against safety and length constraints"
/// pre:  name and value are non-empty, max_len > 0
/// post: returns Ok(()) if valid
/// post: returns Err if invalid
#[must_use = "result must be used"]
pub fn validate_path(name: &str, value: &str, max_len: usize) -> Result<(), McpToolError> {
    if value.is_empty() {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must not be empty"
        )));
    }
    if value.len() > max_len {
        return Err(McpToolError::invalid_argument(format!(
            "{name} exceeds maximum length of {max_len} (got {})",
            value.len()
        )));
    }
    if value.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(McpToolError::invalid_argument(format!(
            "{name} contains a NUL or control character"
        )));
    }
    if std::path::Path::new(value)
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must not contain parent-directory traversal"
        )));
    }
    Ok(())
}

/// Validate a URL parameter against SSRF protection rules.
///
/// Delegates to `hkask_mcp::validate_url()` with the default (strict) config.
/// Use this for any tool that accepts a user-provided URL.
/// Validate a tool URL (http/https only, no path traversal).
///
/// expect: "The system validates tool input against safety and length constraints"
/// pre:  url is non-empty
/// post: returns Ok(()) if valid http/https URL
/// post: returns Err if invalid scheme or format
#[must_use = "result must be used"]
pub fn validate_tool_url(url: &str) -> Result<(), McpToolError> {
    crate::security::validate_url(url, &crate::security::UrlValidationConfig::default())
        .map_err(|e| McpToolError::invalid_argument(format!("URL validation failed: {e}")))
}

/// Validate a tool URL with permissive SSRF config (allows private IPs and loopback).
///
/// Use this for user-curated URL lists like RSS subscriptions, where the
/// user has explicitly chosen to fetch from a local address (e.g., a
/// self-hosted RSS aggregator). Do NOT use this for arbitrary user-supplied
/// URLs from untrusted sources.
///
/// expect: "The system validates tool input against safety and length constraints"
/// pre:  url is non-empty
/// post: returns Ok(()) if valid http/https URL (private IPs and loopback allowed)
/// post: returns Err if invalid scheme or format
#[must_use = "result must be used"]
pub fn validate_tool_url_permissive(url: &str) -> Result<(), McpToolError> {
    crate::security::validate_url(url, &crate::security::UrlValidationConfig::permissive())
        .map_err(|e| McpToolError::invalid_argument(format!("URL validation failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_accepts_normal_document_punctuation() {
        assert!(
            validate_path(
                "path",
                "/library/Damodaran Book on Investment Valuation, 2nd Edition (Final).pdf",
                4096,
            )
            .is_ok()
        );
    }

    #[test]
    fn path_rejects_parent_traversal_and_control_characters() {
        assert!(validate_path("path", "../secret", 4096).is_err());
        assert!(validate_path("path", "safe/evil\0name", 4096).is_err());
    }
}
