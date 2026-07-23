//! Tool input JSON fuzz test — Wave 5 Task 5.2
//!
//! Verifies that JSON parsing (the core step of MCP tool input validation)
//! never panics on arbitrary input.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces must reject invalid input gracefully, never panic

use proptest::prelude::*;

// Arbitrary JSON input never panics the parser.

proptest! {
    #[test]
    fn json_parser_never_panics_on_arbitrary_bytes(
        bytes in prop::collection::vec(proptest::arbitrary::any::<u8>(), 0..100_000),
    ) {
        let result = std::panic::catch_unwind(|| {
            let _: Result<serde_json::Value, _> = serde_json::from_slice(&bytes);
        });
        prop_assert!(result.is_ok(),
            "JSON parser panicked on {} bytes of arbitrary input", bytes.len());
    }

    #[test]
    fn json_parser_never_panics_on_arbitrary_strings(
        input in proptest::arbitrary::any::<String>(),
    ) {
        let result = std::panic::catch_unwind(|| {
            let _: Result<serde_json::Value, _> = serde_json::from_str(&input);
        });
        prop_assert!(result.is_ok(),
            "JSON parser panicked on string input len={}", input.len());
    }
}
