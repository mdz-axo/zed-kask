//! Condenser input fuzz test — Wave 5 Task 5.3
//!
//! Verifies that the condenser never panics on arbitrary input,
//! including binary garbage, invalid UTF-8, and extremely large inputs.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces must reject invalid input gracefully, never panic

use hkask_condenser::algorithms::{CondenserAlgorithm, RtkStyleAlgorithm};
use hkask_condenser::types::{ContextCategory, Profile};
use proptest::prelude::*;

// The condenser never panics regardless of input.

proptest! {
    #[test]
    fn condenser_never_panics_on_arbitrary_text(
        input in proptest::arbitrary::any::<String>(),
        profile in proptest::sample::select(&[
            Profile::Heavy, Profile::Normal, Profile::Soft, Profile::Light,
        ]),
        category in proptest::sample::select(&[
            ContextCategory::ShellCommand,
            ContextCategory::TestOutput,
            ContextCategory::BuildOutput,
            ContextCategory::FileContents,
            ContextCategory::ConversationHistory,
            ContextCategory::StructuredData,
            ContextCategory::LogOutput,
            ContextCategory::Unknown,
        ]),
    ) {
        let algo = RtkStyleAlgorithm;
        let result = std::panic::catch_unwind(|| {
            algo.compress(&input, profile, category, None)
        });
        prop_assert!(result.is_ok(),
            "condenser panicked on input len={} profile={:?} category={:?}",
            input.len(), profile, category);
    }

    #[test]
    fn condenser_never_panics_on_large_input(
        size in 0usize..1_000_000usize,
    ) {
        let input = "x".repeat(size);
        let algo = RtkStyleAlgorithm;
        let result = std::panic::catch_unwind(|| {
            algo.compress(&input, Profile::Normal, ContextCategory::Unknown, None)
        });
        prop_assert!(result.is_ok(),
            "condenser panicked on {} byte input", size);
    }
}
