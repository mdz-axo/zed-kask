//! Response-body redaction for inference and MCP provider errors.
//!
//! [`sanitize_error_body`] redacts secret-shaped substrings from a raw
//! provider response body before it is embedded in an error string, then
//! truncates to [`ERROR_BODY_MAX_CHARS`] (char-boundary safe). It is shared
//! across inference backends, the MCP `classify_http_error` helper
//! (`hkask-mcp-server`), and the research web-search/browse providers
//! (`hkask-mcp-research`) — RR-0035 class, hardened by RR-0049/0050/0051.
//!
//! The direct-HTTP OpenAI-compatible chat completion path that previously
//! lived here was removed when chat inference routing moved to the IPC
//! bridge (`InferenceIpcClient` → zed's `LanguageModelRegistry`). Only the
//! redaction utility remains — it has no `reqwest` dependency.

/// Maximum length of a provider response body embedded in an error string.
pub const ERROR_BODY_MAX_CHARS: usize = 200;

/// Secret-shaped prefixes that a provider error page or proxy debug dump may
/// echo back (CWE-209). Redaction is a simple prefix scan, not a parser:
/// defense-in-depth before the body reaches IPC/log sinks.
///
/// All prefixes MUST be lowercase — they are matched against the lowercased
/// body in `redact_secret_tokens`.
pub const SECRET_PREFIXES: &[&str] = &[
    "authorization:",
    "bearer ",
    "sk-",
    "api_key",
    // Common credential prefixes beyond OpenAI's `sk-` (RR-0049/0050/0051):
    // GitHub PATs, AWS keys, Slack tokens, GitLab tokens, JWTs.
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "akaa", // AWS access key id prefix (case-insensitive match)
    "xoxb-",
    "xoxp-",
    "glpat-",
    "eyj", // JWT header base64 prefix (eyJ...)
];

/// Sanitizes a raw provider response body before embedding it in an error
/// string: redacts secret-shaped substrings (prefix through the end of the
/// whitespace-delimited token) and truncates to [`ERROR_BODY_MAX_CHARS`]
/// (char-boundary safe) with a total-length suffix.
///
/// Shared across inference backends, the MCP `classify_http_error` helper,
/// and research providers (RR-0035 class — RR-0049/0050/0051).
#[must_use]
pub fn sanitize_error_body(body: &str) -> String {
    let redacted = redact_secret_tokens(body);
    let total_bytes = body.len();
    if redacted.chars().count() <= ERROR_BODY_MAX_CHARS {
        redacted
    } else {
        let boundary = redacted
            .char_indices()
            .nth(ERROR_BODY_MAX_CHARS)
            .map(|(index, _)| index)
            .unwrap_or(redacted.len());
        format!("{}… ({} bytes total)", &redacted[..boundary], total_bytes)
    }
}

pub fn redact_secret_tokens(body: &str) -> String {
    // Case-insensitive byte-index scan; prefixes are ASCII so byte matching is exact.
    let lower = body.to_ascii_lowercase();
    let mut output = String::with_capacity(body.len());
    let mut cursor = 0;
    while cursor < body.len() {
        let match_at = SECRET_PREFIXES
            .iter()
            .filter_map(|prefix| lower[cursor..].find(prefix).map(|offset| cursor + offset))
            .min();
        match match_at {
            Some(start) => {
                output.push_str(&body[cursor..start]);
                output.push_str("[REDACTED]");
                let prefix_len = SECRET_PREFIXES
                    .iter()
                    .filter(|prefix| lower[start..].starts_with(**prefix))
                    .map(|prefix| prefix.len())
                    .max()
                    .unwrap_or(0);
                // Skip whitespace after the prefix — for "Authorization: Basic abc123"
                // the token follows a space, and without this the scan stops at
                // that space and the secret survives.
                let token_start = start
                    + prefix_len
                    + (body[start + prefix_len..].len()
                        - body[start + prefix_len..].trim_start().len());
                // Header-style prefixes ("Authorization:", "api_key") may be
                // followed by a scheme word ("Basic", "Bearer") before the
                // secret, so redact to end-of-line. Opaque token prefixes
                // ("Bearer ", "sk-") redact to end-of-token.
                let matched_prefix = SECRET_PREFIXES
                    .iter()
                    .filter(|prefix| lower[start..].starts_with(**prefix))
                    .max_by_key(|prefix| prefix.len());
                let to_end_of_line =
                    matches!(matched_prefix, Some(p) if p.ends_with(':') || *p == "api_key");
                cursor = if to_end_of_line {
                    body[token_start..]
                        .find('\n')
                        .map(|offset| token_start + offset)
                        .unwrap_or(body.len())
                } else {
                    body[token_start..]
                        .find(char::is_whitespace)
                        .map(|offset| token_start + offset)
                        .unwrap_or(body.len())
                };
            }
            None => {
                output.push_str(&body[cursor..]);
                break;
            }
        }
    }
    output
}
