//! Property layer for the kask-core small pins
//! (`kask/docs/reference/testing-protocol.md`). Batch 6 of the propagation
//! plan (`kask/docs/reference/testing-protocol.md`): the unit pins for these
//! seams exist (`sanitize_name_blocks_path_traversal`,
//! `equal_versions_retain_the_first_candidate`); the properties generalize
//! them over generated input domains. A shrunk counterexample is a finding
//! to report, never a signal to weaken a property.

use crate::agent_paths::{agent_dir, sanitize_name};
use proptest::prelude::*;

proptest! {
    /// Hypothesis: sanitization is traversal-proof over any generated name —
    /// the result never contains a path separator, never collapses to "." or
    /// "..", never carries leading/trailing dashes, and always lands exactly
    /// one level under the agents tree.
    #[test]
    fn sanitize_name_is_traversal_proof_over_generated_names(
        name in "[a-zA-Z0-9./\\\\:*?\"<>|() -]{0,40}",
    ) {
        let sanitized = sanitize_name(&name);
        prop_assert!(
            !sanitized.contains('/') && !sanitized.contains('\\'),
            "sanitized {:?} carries a path separator",
            sanitized
        );
        prop_assert_ne!(sanitized.as_str(), ".");
        prop_assert_ne!(sanitized.as_str(), "..");
        if !sanitized.is_empty() {
            prop_assert!(!sanitized.starts_with('-'));
            prop_assert!(!sanitized.ends_with('-'));
        }
        let dir = agent_dir(&name);
        prop_assert!(
            sanitized.is_empty() || dir.ends_with(&sanitized),
            "agent_dir must join the sanitized name, got {:?}",
            dir
        );
    }
}
