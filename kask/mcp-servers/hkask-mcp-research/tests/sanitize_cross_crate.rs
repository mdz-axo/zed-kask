//! Cross-crate test: verify `sanitize_error_body` from `hkask-inference` works
//! correctly when called from the research server's error paths. The in-crate
//! tests in `hkask-inference/src/openai_compat.rs` cover the function in
//! isolation; this test verifies the cross-crate call site (RR-0049/0050/0051).
//!
//! The property: for any body containing a secret prefix from `SECRET_PREFIXES`,
//! the sanitized output must not contain the secret token that followed the
//! prefix. This is the same property as the in-crate test, but exercised from
//! the downstream crate to verify the `pub` API surface is correct.

use hkask_inference::openai_compat::{SECRET_PREFIXES, sanitize_error_body};
use proptest::prelude::*;

proptest! {
    /// P1 invariant: for any body with a secret prefix, the sanitized output
    /// must not contain the secret after [REDACTED]. Cross-crate verification
    /// that the `pub fn sanitize_error_body` API works from downstream crates.
    #[test]
    fn cross_crate_sanitize_redacts_secrets(
        prefix_idx in 0usize..SECRET_PREFIXES.len(),
        secret in "[A-Za-z0-9+/=_-]{1,40}",
        prefix_text in "[a-z ]{0,20}",
        suffix_text in "[a-z ]{0,20}"
    ) {
        let prefix = SECRET_PREFIXES[prefix_idx];
        let body = format!("{prefix_text}{prefix}{secret}{suffix_text}");
        let sanitized = sanitize_error_body(&body);
        let redacted_pos = sanitized.find("[REDACTED]");
        if let Some(pos) = redacted_pos {
            let after = &sanitized[pos + "[REDACTED]".len()..];
            prop_assert!(
                !after.contains(&secret),
                "secret '{}' survived after [REDACTED] for prefix '{}': sanitized={:?}",
                secret, prefix, sanitized
            );
        } else {
            prop_assert!(
                !sanitized.contains(&secret),
                "no [REDACTED] marker and secret '{}' survived for prefix '{}': sanitized={:?}",
                secret, prefix, sanitized
            );
        }
    }

    /// P4 panic-freedom: sanitize_error_body must never panic on any input
    /// from the downstream crate. Uses arb_http_error_body from the harness.
    #[test]
    fn cross_crate_sanitize_never_panics(
        body in hkask_test_harness::arb_http_error_body()
    ) {
        let _ = sanitize_error_body(&body);
    }
}
