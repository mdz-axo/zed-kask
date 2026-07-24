//! Request validation and health error sanitization.

use crate::research::types::{
    BrowseRequest, ExtractRequest, MAX_INSTRUCTION_LENGTH, MAX_JSON_PROMPT_LENGTH,
    MAX_JSON_SCHEMA_BYTES, MAX_QUERY_LENGTH, MAX_URL_LENGTH, SearchRequest, WebError,
};

// --- Task 6: Compound provider timeout (shorter than client timeout) ---
pub const COMPOUND_PROVIDER_TIMEOUT_SECS: u64 = 10;

/// Sanitize a provider error to prevent credential leakage.
///
/// Replaces detailed error messages with generic categories and strips
/// any substrings that look like API keys (matching common prefix patterns).
/// Used in both `health_check_all()` and `search_compound()` to ensure
/// no credentials leak through Regulation tracing or compound result metadata.
pub fn sanitize_health_error(error: &str) -> String {
    /// Lazily compiled API key regex pattern for sanitization.
    /// Avoids re-compiling the regex on every call to `sanitize_health_error`.
    static API_KEY_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:sk-|pk-|fc-|ts-|br-|xai-|ghp_)[a-zA-Z0-9]{8,}")
            .expect("static API key regex pattern")
    });

    let sanitized = API_KEY_REGEX.replace_all(error, "[REDACTED]").to_string();

    let lower = sanitized.to_lowercase();
    if lower.contains("401") || lower.contains("403") || lower.contains("auth") {
        "authentication failed".to_string()
    } else if lower.contains("429") || lower.contains("rate") {
        "rate limited".to_string()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout".to_string()
    } else if lower.contains("unreachable") || lower.contains("connection") || lower.contains("dns")
    {
        "unreachable".to_string()
    } else if lower.contains("no provider") {
        "no provider available".to_string()
    } else {
        "unhealthy".to_string()
    }
}

/// Validate a `SearchRequest` at the port boundary.
///
/// This is the authoritative enforcement point per the Cockburn principle:
/// the port defines the contract, not the adapter entry point.
pub fn validate_search_request(req: &SearchRequest) -> Result<(), WebError> {
    if req.query.is_empty() {
        return Err(WebError::BadArgs("query must not be empty".into()));
    }
    if req.query.len() > MAX_QUERY_LENGTH {
        return Err(WebError::BadArgs(format!(
            "query exceeds maximum length of {} characters",
            MAX_QUERY_LENGTH
        )));
    }
    Ok(())
}

/// Validate an `ExtractRequest` at the port boundary.
pub fn validate_extract_request(req: &ExtractRequest) -> Result<(), WebError> {
    if req.url.len() > MAX_URL_LENGTH {
        return Err(WebError::BadArgs(format!(
            "url exceeds maximum length of {} characters",
            MAX_URL_LENGTH
        )));
    }
    if let Some(ref prompt) = req.json_prompt
        && prompt.len() > MAX_JSON_PROMPT_LENGTH
    {
        return Err(WebError::BadArgs(format!(
            "json_prompt exceeds maximum length of {} characters",
            MAX_JSON_PROMPT_LENGTH
        )));
    }
    if let Some(ref schema) = req.json_schema
        && let Ok(bytes) = serde_json::to_string(schema)
        && bytes.len() > MAX_JSON_SCHEMA_BYTES
    {
        return Err(WebError::BadArgs(format!(
            "json_schema exceeds maximum size of {} bytes",
            MAX_JSON_SCHEMA_BYTES
        )));
    }
    Ok(())
}

/// Validate a `BrowseRequest` at the port boundary.
pub fn validate_browse_request(req: &BrowseRequest) -> Result<(), WebError> {
    if req.url.len() > MAX_URL_LENGTH {
        return Err(WebError::BadArgs(format!(
            "url exceeds maximum length of {} characters",
            MAX_URL_LENGTH
        )));
    }
    if let Some(ref instr) = req.instruction
        && instr.len() > MAX_INSTRUCTION_LENGTH
    {
        return Err(WebError::BadArgs(format!(
            "instruction exceeds maximum length of {} characters",
            MAX_INSTRUCTION_LENGTH
        )));
    }
    Ok(())
}
