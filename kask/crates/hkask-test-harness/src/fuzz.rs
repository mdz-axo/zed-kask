//! Fuzz test seed utilities — pre-built inputs for CLI parsers, deserializers,
//! and other input surfaces that need coverage-guided fuzzing.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces must reject malformed input gracefully
//! - P5 (Essentialism): one function per seed category, no framework

/// Common malformed inputs for testing CLI argument parsers.
///
/// post: returns Vec of strings including empty, special chars, unicode, overflow
pub fn cli_fuzz_seeds() -> Vec<String> {
    vec![
        String::new(),
        "-".to_string(),
        "--".to_string(),
        "-h".to_string(),
        "--help".to_string(),
        "\0".to_string(),
        "\n".to_string(),
        "\t".to_string(),
        " ".to_string(),
        "  ".to_string(),
        "\\".to_string(),
        "'".to_string(),
        "\"".to_string(),
        "$HOME".to_string(),
        "`id`".to_string(),
        "$(id)".to_string(),
        "a".repeat(10_000),
        "a".repeat(100_000),
        "🤖".to_string(),
        "こんにちは".to_string(),
        "null".to_string(),
        "undefined".to_string(),
        "NaN".to_string(),
        "Infinity".to_string(),
        "-0".to_string(),
        "0xDEADBEEF".to_string(),
        "18446744073709551616".to_string(), // u64::MAX + 1
    ]
}

/// Common malformed JSON inputs for testing deserializers.
///
/// post: returns Vec of strings including partial, nested, malformed JSON
pub fn json_fuzz_seeds() -> Vec<String> {
    vec![
        String::new(),
        "{".to_string(),
        "}".to_string(),
        "[".to_string(),
        "]".to_string(),
        "{]".to_string(),
        "[}".to_string(),
        "{\"a\":}".to_string(),
        "{\"a\":1,}".to_string(),
        "{\"a\":1,\"a\":2}".to_string(),
        "[1,]".to_string(),
        "[1 2]".to_string(),
        "{\"a\":1\n}".to_string(),
        "\"unclosed".to_string(),
        "null".to_string(),
        "true".to_string(),
        "false".to_string(),
        "\"".to_string(),
        "\"\\uD800\"".to_string(),  // lone surrogate
        "[1,2,3][4,5]".to_string(), // trailing data
        format!("{{\"a\":{}}}", "1".repeat(10000)),
        format!("[{}]", "1,".repeat(10000)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_fuzz_seeds_non_empty() {
        assert!(!cli_fuzz_seeds().is_empty());
    }

    #[test]
    fn json_fuzz_seeds_non_empty() {
        assert!(!json_fuzz_seeds().is_empty());
    }

    #[test]
    fn cli_fuzz_seeds_all_strings() {
        for seed in cli_fuzz_seeds() {
            let _ = seed.len();
        }
    }

    #[test]
    fn json_fuzz_seeds_contains_edge_cases() {
        let seeds = json_fuzz_seeds();
        assert!(seeds.iter().any(|s| s.is_empty()), "should have empty");
        assert!(seeds.iter().any(|s| s == "{"), "should have bare brace");
        assert!(
            seeds.iter().any(|s| s == "null"),
            "should have null literal"
        );
    }
}
