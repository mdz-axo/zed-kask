//! ABW API conventions — slug/name validation, URL encoding, hire-cost
//! flooring, and embedded-error detection.
//!
//! Extracted from the swarm server root. These helpers encode ABW's
//! domain rules (slug charset/length, the owned-agent add flat fee) and
//! inspect 200-response bodies for upstream errors ABW buries in the body
//! (never leaks reqwest types — `detect_embedded_error` maps to `SwarmError`).

use crate::error::LocalSwarmError;
use crate::error::SwarmError;

/// Inspect a 200-response body for ABW's embedded upstream-error pattern.
/// Returns a typed `SwarmError` when the payload is an error in disguise.
pub fn detect_embedded_error(value: &serde_json::Value) -> Option<SwarmError> {
    // Xaman Ek puts upstream failures in the `response` string field.
    let text = value
        .get("response")
        .and_then(|r| r.as_str())
        .or_else(|| value.get("error").and_then(|e| e.as_str()))?;
    if !(text.contains("I encountered an error") || text.contains("Execution failed")) {
        return None;
    }
    if text.contains("credit balance is too low") || text.contains("credit balance") {
        return Some(SwarmError::UpstreamModelError {
            provider: "anthropic".to_string(),
            message: text.to_string(),
        });
    }
    if text.contains("not funded") {
        return Some(SwarmError::AgentNotFunded {
            agent: extract_quoted(text).unwrap_or_default(),
            message: text.to_string(),
        });
    }
    Some(SwarmError::UpstreamModelError {
        provider: "unknown".to_string(),
        message: text.to_string(),
    })
}

/// Extract the first 'single-quoted' token (ABW uses it for agent names in
/// error strings like "Agent 'david_dunning' is not funded").
pub fn extract_quoted(text: &str) -> Option<String> {
    let start = text.find('\'')? + 1;
    let end = text[start..].find('\'')? + start;
    Some(text[start..end].to_string())
}

/// Percent-encode a path segment for safe interpolation into a URL path.
/// ABW workspace ids and agent names are operator-controlled, but a slug
/// containing `?`, `&`, `#`, `/`, or space would corrupt the URL path if
/// interpolated raw. This is a minimal encoder for the path-unsafe subset
/// (RFC 3986 unreserved + path-allowed characters are preserved).
pub fn url_encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            // Unreserved (RFC 3986 §2.3) + path-allowed (/ is NOT included —
            // we are encoding a single segment).
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// Build an ABW workspace slug from a name base and a timestamp. ABW slugs
/// allow only lowercase letters, digits, and underscores, and are capped at
/// 3–64 chars (verified live 2026-08-02 — a 66-char slug was rejected with
/// HTTP 400). The timestamp suffix disambiguates swarms created with the
/// same name: the FULL epoch-millis value is used — the prior version
/// truncated to the first 4 digits of the epoch-millis string, which is
/// constant for ~3.17 years (the 4th digit of a 13-digit value rolls over
/// every 10^11 ms), so two swarms with the same name created months apart
/// received the SAME slug. The base is truncated (keeping the trailing
/// underscore-trim) so base + '_' + suffix fits within 64 chars. Extracted
/// from `swarm_create_swarm` for testability (KA-03: the prior inline version
/// panicked on a pre-epoch clock via `&string[..4]` on an empty string).
pub fn make_swarm_slug(slug_base: &str, now: std::time::SystemTime) -> String {
    let suffix = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let base = slug_base.trim_matches('_');
    // ABW slugs are capped at 64 chars (verified live) — reserve room for the
    // `_` separator + the full millis suffix and truncate the base.
    let max_base = 64usize.saturating_sub(suffix.len() + 1);
    let base = if base.len() > max_base {
        // Truncate on a char boundary — base may contain multi-byte UTF-8
        // (local_swarms::create uses char::is_alphanumeric which admits
        // CJK/accented chars). A byte slice mid-codepoint would panic.
        let mut end = max_base;
        while !base.is_char_boundary(end) {
            end -= 1;
        }
        &base[..end]
    } else {
        base
    };
    format!("{base}_{suffix}")
}

/// Validate an ABW agent name (the creation surface). ABW agent names are
/// slugs: 3–64 chars, lowercase letters, digits, and underscores only
/// (verified live 2026-08-02 — `zed_kask_verify_<uuid>` with hyphens was
/// rejected with HTTP 400 "slug must contain only lowercase letters, digits,
/// and underscores"). Rejecting here turns ABW's confusing 400 into a clear
/// argument error.
pub fn validate_agent_name(name: &str) -> Result<(), LocalSwarmError> {
    let len = name.chars().count();
    if !(3..=64).contains(&len) {
        return Err(LocalSwarmError::InvalidInput(format!(
            "invalid agent_name: must be 3–64 chars, got {len}"
        )));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(LocalSwarmError::InvalidInput(
            "invalid agent_name: must contain only lowercase letters, digits, and underscores"
                .to_string(),
        ));
    }
    Ok(())
}

/// The flat fee ABW charges to add an owned agent to a workspace. Verified
/// live 2026-08-02: `POST /workspaces/{id}/add` returned `gas_charged: 2`
/// for a no-dependency owned agent, while `/agents/{name}/dependencies`
/// reports `total_hire_cost: 0` — the dependency quote UNDER-states the
/// actual add charge. The consent gate's re-verification must floor the
/// quote at this fee so a 1-credit authorization cannot spend 2.
///
/// Third-party hires are a different tier: `/hire` charges a flat 5 cr base
/// (verified live 2026-08-02 on `sensor_advisor`: `gas_charged: 5` with
/// `dependencies_hired: []`), and the third-party `/dependencies` quote
/// already INCLUDES the base (quote `total=10, required=0, optional=5` =
/// base 5 + optional 5). So the floor only needs to cover the owned-agent
/// case; the third-party quote is trustworthy as-is.
pub const OWNED_ADD_FLAT_FEE: u64 = 2;

/// The effective hire cost for a re-verified `/agents/{name}/dependencies`
/// payload. A dependency-less agent quotes `total_hire_cost: 0` but the add
/// charges `OWNED_ADD_FLAT_FEE` — the gate must never under-quote a spend.
/// Only call this after the caller has already rejected a MISSING
/// `total_hire_cost` (missing = unknown, never zero — the `.rules` trap).
pub fn effective_hire_cost(deps: &serde_json::Value) -> u64 {
    let total = deps
        .get("total_hire_cost")
        .and_then(|c| c.as_u64())
        .unwrap_or(0);
    let has_deps = deps
        .get("has_dependencies")
        .and_then(|h| h.as_bool())
        .unwrap_or(false);
    if has_deps {
        total
    } else {
        std::cmp::max(total, OWNED_ADD_FLAT_FEE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_error_detects_anthropic_credit_exhaustion() {
        let v = serde_json::json!({
            "response": "I encountered an error: Execution failed: API error: Your credit balance is too low to access the Anthropic API."
        });
        match detect_embedded_error(&v) {
            Some(SwarmError::UpstreamModelError { provider, .. }) => {
                assert_eq!(provider, "anthropic")
            }
            other => panic!("expected UpstreamModelError, got {other:?}"),
        }
    }

    #[test]
    fn embedded_error_detects_not_funded() {
        let v = serde_json::json!({
            "response": "Execution failed: Agent 'david_dunning' is not funded. Its owner has not set an ANTHROPIC_API_KEY."
        });
        match detect_embedded_error(&v) {
            Some(SwarmError::AgentNotFunded { agent, .. }) => {
                assert_eq!(agent, "david_dunning")
            }
            other => panic!("expected AgentNotFunded, got {other:?}"),
        }
    }

    #[test]
    fn embedded_error_ignores_clean_payload() {
        let v = serde_json::json!({"response": "The bestiary is a living ecology of AI agents."});
        assert!(detect_embedded_error(&v).is_none());
    }

    #[test]
    fn extract_quoted_pulls_agent_name() {
        assert_eq!(
            extract_quoted("Agent 'market_analyst' is not funded"),
            Some("market_analyst".to_string())
        );
        assert_eq!(extract_quoted("no quotes here"), None);
    }

    // URL encoding: path segments with special characters must be encoded
    // so they don't corrupt the URL path.
    #[test]
    fn url_encode_segment_encodes_special_chars() {
        assert_eq!(url_encode_segment("market_analyst"), "market_analyst");
        assert_eq!(
            url_encode_segment("agent with spaces"),
            "agent%20with%20spaces"
        );
        assert_eq!(url_encode_segment("a/b"), "a%2Fb");
        assert_eq!(url_encode_segment("a?b"), "a%3Fb");
        assert_eq!(url_encode_segment("a&b"), "a%26b");
        assert_eq!(url_encode_segment("a#b"), "a%23b");
    }

    // The slug must not panic on a pre-epoch clock. The prior inline version
    // used `&string[..4]` on an empty string (from `unwrap_or_default()` on
    // a pre-epoch `duration_since`), which panicked. The extracted helper
    // uses safe slicing.
    #[test]
    fn make_swarm_slug_handles_pre_epoch_clock() {
        // A time before UNIX_EPOCH — duration_since returns Err.
        let pre_epoch = std::time::SystemTime::UNIX_EPOCH
            .checked_sub(std::time::Duration::from_secs(60))
            .expect("construct pre-epoch time");
        let slug = make_swarm_slug("my_swarm", pre_epoch);
        // Must not panic, must produce a valid slug.
        assert!(slug.starts_with("my_swarm_"));
        assert!(!slug.is_empty());
    }

    #[test]
    fn make_swarm_slug_produces_suffix() {
        let now = std::time::SystemTime::now();
        let slug = make_swarm_slug("test", now);
        assert!(slug.starts_with("test_"));
        // The suffix is the full epoch-millis value — two swarms created with
        // the same name at different times must NOT collide (the prior 4-digit
        // truncation was constant for ~3.17 years).
        let suffix = slug.strip_prefix("test_").unwrap_or("");
        assert!(
            suffix.len() >= 10,
            "full millis suffix expected, got '{suffix}'"
        );
        assert!(
            suffix.chars().all(|c| c.is_ascii_digit()),
            "suffix must be digits only, got '{suffix}'"
        );
    }

    #[test]
    fn make_swarm_slug_disambiguates_same_name_over_time() {
        // Two swarms with the same name created 1 second apart must produce
        // different slugs. The prior first-4-digits-of-millis truncation made
        // the suffix constant for ~3.17 years — this pins the fix.
        let t0 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let t1 = t0 + std::time::Duration::from_secs(1);
        let slug0 = make_swarm_slug("my_swarm", t0);
        let slug1 = make_swarm_slug("my_swarm", t1);
        assert_ne!(
            slug0, slug1,
            "same-name swarms created 1s apart must not collide"
        );
    }

    #[test]
    fn make_swarm_slug_caps_total_length_at_64() {
        // ABW rejects slugs longer than 64 chars (verified live 2026-08-02).
        // A long name base must be truncated, keeping the disambiguating
        // millis suffix, so the total never exceeds 64.
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let long_base = "a".repeat(100);
        let slug = make_swarm_slug(&long_base, now);
        assert!(
            slug.len() <= 64,
            "slug must fit ABW's 64-char cap, got {} chars: {slug}",
            slug.len()
        );
        assert!(
            slug.ends_with("_1700000000000"),
            "millis suffix kept: {slug}"
        );
        // A short base is untouched.
        assert_eq!(make_swarm_slug("alpha", now), "alpha_1700000000000");
    }

    #[test]
    fn validate_agent_name_enforces_abw_slug_rule() {
        assert!(validate_agent_name("sensor_advisor").is_ok());
        assert!(validate_agent_name("abc123").is_ok());
        // Hyphens are rejected (the verified ABW rule — uuid suffixes fail).
        assert!(validate_agent_name("zed_kask_verify-abc").is_err());
        // Uppercase rejected.
        assert!(validate_agent_name("Sensor").is_err());
        // Length bounds.
        assert!(validate_agent_name("ab").is_err());
        assert!(validate_agent_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn make_swarm_slug_trims_underscores_from_base() {
        let now = std::time::SystemTime::now();
        let slug = make_swarm_slug("__leading_and_trailing__", now);
        assert!(
            !slug.contains("__leading"),
            "leading underscores must be trimmed"
        );
    }

    #[test]
    fn effective_hire_cost_floors_dependency_less_agents() {
        // Owned, no-dependency agents quote total_hire_cost: 0 but /add
        // charges the flat fee (verified live) — the gate must floor at it.
        let no_deps = serde_json::json!({
            "total_hire_cost": 0,
            "has_dependencies": false,
            "required": [],
            "optional": [],
        });
        assert_eq!(effective_hire_cost(&no_deps), OWNED_ADD_FLAT_FEE);
        // With dependencies, the quoted total is authoritative.
        let with_deps = serde_json::json!({
            "total_hire_cost": 5,
            "has_dependencies": true,
            "required": ["a"],
            "optional": [],
        });
        assert_eq!(effective_hire_cost(&with_deps), 5);
    }
}
