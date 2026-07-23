//! Manifest parser fuzz test — Wave 5 Task 5.1
//!
//! Verifies that YAML manifest parsing never panics on arbitrary input.
//! Tests the core serde_yaml_neo parsing step that all manifest loading goes through.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces must reject invalid input gracefully, never panic

use proptest::prelude::*;

// [P3] Motivating: Generative Space — validates YAML parsing is panic-free
//Constraining: Clear Boundaries — arbitrary input must be rejected gracefully
// Arbitrary input to YAML parser never panics.

proptest! {
    #[test]
    fn yaml_parser_never_panics_on_arbitrary_bytes(
        bytes in prop::collection::vec(proptest::arbitrary::any::<u8>(), 0..100_000),
    ) {
        let result = std::panic::catch_unwind(|| {
            let _: Result<serde_yaml_neo::Value, _> = serde_yaml_neo::from_slice(&bytes);
        });
        prop_assert!(result.is_ok(),
            "YAML parser panicked on {} bytes of arbitrary input", bytes.len());
    }

    // [P3] Motivating: Generative Space — validates YAML parsing is panic-free
    //Constraining: Clear Boundaries — arbitrary input must be rejected gracefully
    #[test]
    fn yaml_parser_never_panics_on_arbitrary_strings(
        input in proptest::arbitrary::any::<String>(),
    ) {
        let result = std::panic::catch_unwind(|| {
            let _: Result<serde_yaml_neo::Value, _> = serde_yaml_neo::from_str(&input);
        });
        prop_assert!(result.is_ok(),
            "YAML parser panicked on string input len={}", input.len());
    }
}
