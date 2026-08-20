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
/// 3–64 chars (verified live 2026-08-13 — a 66-char slug was rejected with
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
/// (verified live 2026-08-13 — `zed_kask_verify_<uuid>` with hyphens was
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
/// (verified live 2026-08-13 on `sensor_advisor`: `gas_charged: 5` with
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
