//! Property tests for the `pub(crate)` context-injector recall gate exposed
//! under the `test-utils` feature gate (`kask_bridge::test_utils::should_recall`).
//!
//! `bridge_properties.rs` (the sibling file) documents a coverage gap: the
//! prompt-length recall gate (`BridgeContextInjector::should_recall`) is a
//! private associated fn, so it was unreachable from an integration test. The
//! `test_utils` module wraps it as a free function, closing that gap. These
//! tests drive the real threshold logic directly — no `MemoryPort`, no stub.
//!
//! The gate is a zero-cost filter: prompts shorter than `MIN_RECALL_PROMPT_LEN`
//! (20) **bytes** or with fewer than `MIN_RECALL_PROMPT_WORDS` (3) words skip
//! recall entirely (no embedding HTTP call, no SQL). Note: the length check is
//! `.len()` (bytes), not `.chars().count()` — a multi-byte UTF-8 prompt can
//! reach 20 bytes with fewer than 20 characters.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant`  — never-panic, determinism, and the threshold property
//!   (the decision is exactly `byte_len >= 20 && word_count >= 3`).
//! - `oracle_reference`  — compare against an independent re-implementation of
//!   the threshold using the documented constants.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): `should_recall` returns a bool for any string —
//!   never panics.
//! - P1 (Correctness): the decision matches the documented threshold for every
//!   input.

use kask_bridge::test_utils::should_recall;
use proptest::prelude::*;
use serde_json::{Value, json};

/// The documented prompt-length gate constants (mirrors the private
/// `MIN_RECALL_PROMPT_LEN` / `MIN_RECALL_PROMPT_WORDS` in `context_injector.rs`).
/// Kept as named constants here so a future change to the source constants is
/// surfaced as a failing reference comparison (the test author must update
/// both — the drift is the signal, not silent acceptance).
const REF_MIN_LEN: usize = 20;
const REF_MIN_WORDS: usize = 3;

/// Independent reference implementation of the recall gate: byte length ≥ 20
/// AND word count ≥ 3. Structured to mirror the documented contract, not the
/// source's exact code shape.
fn should_recall_reference(prompt: &str) -> bool {
    prompt.len() >= REF_MIN_LEN && prompt.split_whitespace().count() >= REF_MIN_WORDS
}

/// Arbitrary prompt strings of bounded length (any UTF-8, including whitespace,
/// control chars, and multi-byte chars) — exercises the byte-length vs
/// word-count tension.
fn arb_prompt(max_chars: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..max_chars).prop_map(|cs| cs.into_iter().collect())
}

proptest! {
    /// `should_recall` never panics on arbitrary UTF-8.
    #[test]
    fn should_recall_never_panics(prompt in arb_prompt(80)) {
        let result = std::panic::catch_unwind(|| should_recall(&prompt));
        prop_assert!(result.is_ok(), "panicked on prompt={prompt:?}");
    }

    /// `should_recall` is deterministic: same prompt → same decision.
    #[test]
    fn should_recall_is_deterministic(prompt in arb_prompt(80)) {
        let a = should_recall(&prompt);
        let b = should_recall(&prompt);
        prop_assert_eq!(a, b);
    }

    /// Reference oracle: the real gate matches the independent reference for
    /// every input.
    #[test]
    fn should_recall_matches_reference(prompt in arb_prompt(80)) {
        let output = should_recall(&prompt);
        let input = json!(prompt);
        let oracle = oracle_reference(|inp: &Value| {
            let s = inp.as_str().unwrap_or("");
            json!(should_recall_reference(s))
        });
        let out_val = json!(output);
        prop_assert_eq!(
            oracle.verify(&input, &out_val),
            OracleVerdict::Pass,
            "should_recall({:?}) = {}, expected {}",
            prompt, output, should_recall_reference(&prompt)
        );
    }

    /// Invariant: a prompt shorter than `REF_MIN_LEN` bytes never recalls,
    /// regardless of word count.
    #[test]
    fn should_recall_false_for_short_prompts(prompt in arb_prompt(80)) {
        let input = json!(prompt);
        let oracle = oracle_invariant(|inp: &Value, out: &Value| {
            let s = inp.as_str().ok_or("input not a string")?;
            let decide = out.as_bool().ok_or("output not a bool")?;
            if s.len() < REF_MIN_LEN && decide {
                Err(format!(
                    "prompt of {} bytes (< {}) recalled true",
                    s.len(),
                    REF_MIN_LEN
                ))
            } else {
                Ok(())
            }
        });
        let out_val = json!(should_recall(&prompt));
        prop_assert_eq!(oracle.verify(&input, &out_val), OracleVerdict::Pass);
    }

    /// Invariant: a prompt with fewer than `REF_MIN_WORDS` words never recalls,
    /// regardless of byte length.
    #[test]
    fn should_recall_false_for_few_word_prompts(prompt in arb_prompt(80)) {
        let input = json!(prompt);
        let oracle = oracle_invariant(|inp: &Value, out: &Value| {
            let s = inp.as_str().ok_or("input not a string")?;
            let decide = out.as_bool().ok_or("output not a bool")?;
            let words = s.split_whitespace().count();
            if words < REF_MIN_WORDS && decide {
                Err(format!(
                    "prompt with {} words (< {}) recalled true",
                    words,
                    REF_MIN_WORDS
                ))
            } else {
                Ok(())
            }
        });
        let out_val = json!(should_recall(&prompt));
        prop_assert_eq!(oracle.verify(&input, &out_val), OracleVerdict::Pass);
    }

    /// Invariant: a prompt that meets BOTH thresholds (≥ 20 bytes AND ≥ 3
    /// words) always recalls. This pins the conjunction — a single threshold
    /// alone is insufficient.
    #[test]
    fn should_recall_true_when_both_thresholds_met(prompt in arb_prompt(80)) {
        let input = json!(prompt);
        let oracle = oracle_invariant(|inp: &Value, out: &Value| {
            let s = inp.as_str().ok_or("input not a string")?;
            let decide = out.as_bool().ok_or("output not a bool")?;
            let words = s.split_whitespace().count();
            if s.len() >= REF_MIN_LEN && words >= REF_MIN_WORDS && !decide {
                Err(format!(
                    "prompt of {} bytes (≥ {}) with {} words (≥ {}) recalled false",
                    s.len(),
                    REF_MIN_LEN,
                    words,
                    REF_MIN_WORDS
                ))
            } else {
                Ok(())
            }
        });
        let out_val = json!(should_recall(&prompt));
        prop_assert_eq!(oracle.verify(&input, &out_val), OracleVerdict::Pass);
    }
}
