//! Lexicon term format validation.
//!
//! Reduced via essentialist 3-gate challenge (Exist → Surface → Contract).
//! The 420-term allowlist (`KNOWN_TERMS`) and its binary-search lookup
//! (`is_known`) were deleted — they were a closed loop with no external
//! consumer. No skill router queries lexicon terms, no runtime decision
//! depends on them, and `search_by_lexicon` (the only method that would
//! have used them) had zero callers.
//!
//! What survives is `is_well_formed` — the format check that catches real
//! errors (casing drift like `Multi-Step`, separator drift like `multi-step`
//! vs `multi_step`, whitespace, leading digits). This is the only function
//! that carries genuine behavior beyond a direct call.

/// Check that a lexicon term matches the naming convention.
///
/// Convention: lowercase letters, digits, and underscores; must start with
/// a letter. Rejects mixed case, hyphens, spaces, and leading digits/underscores.
///
/// expect: "The system validates template contracts against the lexicon"
/// pre:  term may be any string
/// post: returns true if term matches ^[a-z][a-z0-9_]*$
pub fn is_well_formed(term: &str) -> bool {
    let mut chars = term.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    }
}

/// Validate an entry's `lexicon_terms` for format compliance.
/// Returns warnings for any ill-formed terms.
///
/// expect: "The system validates template contracts against the lexicon"
/// pre:  entry is a valid RegistryEntry
/// post: returns Vec of warning strings for ill-formed terms
pub(crate) fn validate_entry(entry: &hkask_types::registry::RegistryEntry) -> Vec<String> {
    let mut warnings = Vec::new();
    for term in &entry.lexicon_terms {
        if !is_well_formed(term) {
            warnings.push(format!(
                "entry '{}' declares ill-formed lexicon term '{}' (must match ^[a-z][a-z0-9_]*$)",
                entry.id, term
            ));
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_well_formed_accepts_lowercase_and_underscores() {
        assert!(is_well_formed("compose"));
        assert!(is_well_formed("rule_out"));
        assert!(is_well_formed("value_chain"));
        assert!(is_well_formed("dead_code"));
        assert!(is_well_formed("stage_3"));
        assert!(is_well_formed("a"));
    }

    #[test]
    fn is_well_formed_rejects_mixed_case() {
        assert!(!is_well_formed("Multi-Step"));
        assert!(!is_well_formed("multiStep"));
        assert!(!is_well_formed("COMPOSE"));
    }

    #[test]
    fn is_well_formed_rejects_hyphens() {
        assert!(!is_well_formed("multi-step"));
        assert!(!is_well_formed("rule-out"));
    }

    #[test]
    fn is_well_formed_rejects_leading_digit_or_underscore() {
        assert!(!is_well_formed("3stage"));
        assert!(!is_well_formed("_private"));
    }

    #[test]
    fn is_well_formed_rejects_empty() {
        assert!(!is_well_formed(""));
    }
}
