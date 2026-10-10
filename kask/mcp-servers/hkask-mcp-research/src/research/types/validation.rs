//! Request validation and health error sanitization.

use super::WebError;

// --- Task 6: Compound provider timeout (shorter than client timeout) ---
/// 20s: raised from 10s (2026-09-30) because SerpAPI — the slowest compound
/// participant — consistently exceeded the 10s bound in live use (observed:
/// "Provider timed out after 10s" on most fusion calls of the zk-reference
/// retrieval sweep, so the provider never participated at all). The bound
/// stays below the 30s client timeout so a hung provider still fails the
/// fusion rather than hanging it.
pub(crate) const COMPOUND_PROVIDER_TIMEOUT_SECS: u64 = 20;

/// Normalize an LLM-emitted closed-vocabulary value before validation:
/// trim surrounding whitespace, strip ONE layer of symmetric quotes
/// (double or single), trim again. Tool-call emitters wrap enum values
/// in quotes or padding — `strategy: "\"quick\""` arrives as the string
/// `"quick"` and every closed-vocabulary parse site rejected it
/// (observed live 2026-10-09: agents burned retries on `web_search`
/// strategy/intent/provider before discovering the quoting). One layer
/// only: a value that is genuinely `""` after stripping still fails
/// validation with the teaching error, and interior quotes are preserved.
/// Byte slicing is safe because the strip guard matches ASCII quote bytes,
/// which are always char boundaries.
pub(crate) fn normalize_closed_vocab(raw: &str) -> &str {
    let s = raw.trim();
    if s.len() >= 2 {
        let first = s.as_bytes()[0];
        let last = s.as_bytes()[s.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].trim();
        }
    }
    s
}

#[cfg(test)]
mod normalize_closed_vocab_tests {
    use super::normalize_closed_vocab;

    #[test]
    fn strips_one_layer_of_symmetric_quotes_and_padding() {
        assert_eq!(normalize_closed_vocab("\"quick\""), "quick");
        assert_eq!(normalize_closed_vocab("'quick'"), "quick");
        assert_eq!(normalize_closed_vocab("  \"quick\"  "), "quick");
        assert_eq!(normalize_closed_vocab("quick"), "quick");
        assert_eq!(normalize_closed_vocab(" quick "), "quick");
    }

    #[test]
    fn preserves_interior_quotes_and_asymmetric_wrapping() {
        // Interior quotes are data, not emission noise.
        assert_eq!(
            normalize_closed_vocab("a \"quoted\" title"),
            "a \"quoted\" title"
        );
        // Asymmetric wrapping is not emission noise either — reject downstream.
        assert_eq!(normalize_closed_vocab("\"quick"), "\"quick");
        assert_eq!(normalize_closed_vocab("quick'"), "quick'");
    }

    #[test]
    fn degenerate_forms_stay_visible_to_validation() {
        assert_eq!(normalize_closed_vocab("\"\""), "");
        assert_eq!(normalize_closed_vocab("''"), "");
        assert_eq!(normalize_closed_vocab("\""), "\"");
        assert_eq!(normalize_closed_vocab(""), "");
    }
}

/// Parse a closed-vocabulary field value: normalize LLM emission noise
/// (`normalize_closed_vocab`), match case-insensitively against a table
/// of accepted spellings mapped to values. The single parse site for
/// every enum-ish tool parameter — one owner of the tolerance +
/// teaching-error contract (C2: the scattered match arms — strategy,
/// freshness, intent, format, duplication — collapsed onto this helper).
/// On miss, the error names the field and the canonical spellings (the
/// first entry of each group); aliases are accepted but not listed.
/// Case-insensitive so canonical spellings may be any case (the schema's
/// `DataOnly` matches an emitted `dataonly`).
pub(crate) fn parse_closed_vocab<T: Copy>(
    field: &'static str,
    raw: &str,
    table: &[(&'static [&'static str], T)],
) -> Result<T, WebError> {
    let normalized = normalize_closed_vocab(raw);
    for (spellings, value) in table {
        if spellings.iter().any(|s| s.eq_ignore_ascii_case(normalized)) {
            return Ok(*value);
        }
    }
    let accepted: Vec<&str> = table.iter().map(|(spellings, _)| spellings[0]).collect();
    Err(WebError::BadArgs(format!(
        "{field} must be one of {}, got '{raw}'",
        accepted.join(", ")
    )))
}

/// Parse a closed-vocabulary field whose accepted spellings are owned as a
/// flat `&'static [&'static str]` const by a domain module (runs.rs's
/// `FINISH_STATUSES` / `VERIFICATION_STATES`): the same tolerance +
/// teaching contract as `parse_closed_vocab`, but the vocabulary has a
/// single owner outside the boundary and no aliases — each spelling is
/// canonical. Returns the canonical spelling borrowed from `accepted`, so
/// the domain guard downstream receives the clean form.
pub(crate) fn parse_closed_vocab_const(
    field: &'static str,
    raw: &str,
    accepted: &[&'static str],
) -> Result<&'static str, WebError> {
    let normalized = normalize_closed_vocab(raw);
    if let Some(canonical) = accepted.iter().find(|a| a.eq_ignore_ascii_case(normalized)) {
        return Ok(canonical);
    }
    Err(WebError::BadArgs(format!(
        "{field} must be one of {}, got '{raw}'",
        accepted.join(", ")
    )))
}

#[cfg(test)]
mod parse_closed_vocab_tests {
    use super::{parse_closed_vocab, parse_closed_vocab_const};

    #[test]
    fn matches_canonical_alias_and_noisy_forms() {
        let table: &[(&[&str], i32)] = &[(&["quick"], 1), (&["web", "semantic"], 2)];
        assert_eq!(parse_closed_vocab("strategy", "quick", table).unwrap(), 1);
        assert_eq!(
            parse_closed_vocab("strategy", "SEMANTIC", table).unwrap(),
            2
        );
        assert_eq!(
            parse_closed_vocab("strategy", " \"web\" ", table).unwrap(),
            2
        );
        assert_eq!(parse_closed_vocab("strategy", "'quick'", table).unwrap(), 1);
    }

    #[test]
    fn matches_case_insensitively_against_mixed_case_canonicals() {
        // Canonical spellings may be PascalCase (the schema's DataOnly):
        // an emitted lowercase/quoted form must still parse, and the miss
        // error must list the canonical spelling verbatim.
        let table: &[(&[&str], i32)] = &[(&["DataOnly"], 1), (&["InlineTable"], 2)];
        assert_eq!(
            parse_closed_vocab("presentation", "dataonly", table).unwrap(),
            1
        );
        assert_eq!(
            parse_closed_vocab("presentation", " \"inlinetable\" ", table).unwrap(),
            2
        );
        let err = parse_closed_vocab("presentation", "bogus", table).expect_err("must miss");
        assert!(err.to_string().contains("DataOnly, InlineTable"));
    }

    #[test]
    fn const_variant_parses_against_a_domain_owned_vocabulary() {
        const STATUSES: &[&str] = &["completed", "partial", "blocked", "failed"];
        assert_eq!(
            parse_closed_vocab_const("status", " \"Blocked\" ", STATUSES).unwrap(),
            "blocked"
        );
        // The canonical spelling is returned, not the raw emission.
        assert_eq!(
            parse_closed_vocab_const("status", "BLOCKED", STATUSES).unwrap(),
            "blocked"
        );
        let err = parse_closed_vocab_const("status", "bogus", STATUSES).expect_err("must miss");
        assert!(
            err.to_string()
                .contains("status must be one of completed, partial, blocked, failed"),
            "{}",
            err
        );
    }

    #[test]
    fn miss_errors_name_the_field_and_canonical_spellings() {
        let table: &[(&[&str], i32)] = &[(&["quick"], 1), (&["web", "semantic"], 2)];
        let err = parse_closed_vocab("strategy", "bogus", table).expect_err("must miss");
        assert!(matches!(err, super::WebError::BadArgs(_)));
        let msg = err.to_string();
        assert!(msg.contains("strategy must be one of quick, web"), "{msg}");
        assert!(msg.contains("got 'bogus'"), "{msg}");
        // Aliases are accepted but not listed — the canonical spellings teach.
        assert!(!msg.contains("semantic"), "{msg}");
    }
}

/// Sanitize a provider error to prevent credential leakage.
///
/// Replaces detailed error messages with generic categories and strips
/// any substrings that look like API keys (matching common prefix patterns).
/// Used in both `health_check_all()` and `search_compound()` to ensure
/// no credentials leak through Regulation tracing or compound result metadata.
pub(crate) fn sanitize_health_error(error: &str) -> String {
    /// Lazily compiled API key regex pattern for sanitization.
    /// Avoids re-compiling the regex on every call to `sanitize_health_error`.
    static API_KEY_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:sk-|pk-|fc-|ts-|br-|xai-|ghp_)[a-zA-Z0-9]{8,}").unwrap_or_else(|e| {
            tracing::error!(
                error = %e,
                "API key regex failed to compile; credential redaction degraded"
            );
            // Over-redact rather than under-redact: a broken pattern must
            // not leak credentials silently.
            regex::Regex::new(r".*").expect("fallback regex .* always compiles")
        })
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
