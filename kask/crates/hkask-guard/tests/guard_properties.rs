//! Property tests for the pure functions of the `hkask-guard` content-safety
//! pipeline.
//!
//! These tests replaced the deleted `mod tests` block in `guarded_inference.rs`
//! that used `EchoPort` / `ReasoningEchoPort` stubs. They target the *public*
//! pure API of the crate — `ContentGuard`'s input/output scanners, the canary
//! token, and `GuardOutput` — using proptest generators and the
//! `hkask-test-harness` Oracle taxonomy (invariant + reference oracles).
//!
//! No stub, fake, mock, or noop inference ports are constructed. The guard is
//! exercised through its real `scan_input` / `scan_output` / `check_canary`
//! methods over arbitrary JSON-derived text.

#![forbid(unsafe_code)]

use hkask_guard::{ContentGuard, GuardConfig, GuardOutput};
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use proptest::prelude::*;

/// Build the mandatory guard once; its canary is fixed for the test process.
fn guard() -> ContentGuard {
    ContentGuard::mandatory(&GuardConfig::default())
}

/// Serialize an arbitrary JSON value into a string to use as scanner input.
/// JSON is a structurally varied text source — numbers, booleans, nested
/// objects/arrays, arbitrary unicode strings — that exercises the scanners
/// without us hand-picking cases.
fn arb_text() -> BoxedStrategy<String> {
    arb_json_value().prop_map(|v| v.to_string()).boxed()
}

// ── scan_output ───────────────────────────────────────────────────────────

proptest! {
    /// `scan_output` is well-formed for every input text: `passed` holds exactly
    /// when there are no violations, and the output is `Sanitized` exactly when
    /// it did not pass (secrets/canary redacted). It must never panic.
    #[test]
    fn prop_scan_output_well_formed(text in arb_text()) {
        let guard = guard();
        let result = guard.scan_output(&text);

        let output = serde_json::json!({
            "passed": result.passed,
            "violations": result.violations.len(),
            "modified": result.output.is_modified(),
        });
        let input = serde_json::json!({ "text_len": text.len() });

        let oracle = oracle_invariant(|_input, output| {
            let passed = output["passed"].as_bool().ok_or("passed not bool")?;
            let violations = output["violations"].as_u64().ok_or("violations not u64")?;
            let modified = output["modified"].as_bool().ok_or("modified not bool")?;
            if passed != (violations == 0) {
                return Err(format!(
                    "passed={passed} inconsistent with violations={violations}"
                ));
            }
            // scan_output redacts on every violation, so modified == !passed.
            if modified != !passed {
                return Err(format!(
                    "modified={modified} must equal !passed={passed}"
                ));
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }

    /// `scan_input` is well-formed for every input text: `passed` holds exactly
    /// when there are no violations. Unlike `scan_output`, input scanning blocks
    /// rather than redacts, so the output is *always* `Clean` (never modified).
    #[test]
    fn prop_scan_input_well_formed(text in arb_text()) {
        let guard = guard();
        let result = guard.scan_input(&text);

        let output = serde_json::json!({
            "passed": result.passed,
            "violations": result.violations.len(),
            "modified": result.output.is_modified(),
        });
        let input = serde_json::json!({ "text_len": text.len() });

        let oracle = oracle_invariant(|_input, output| {
            let passed = output["passed"].as_bool().ok_or("passed not bool")?;
            let violations = output["violations"].as_u64().ok_or("violations not u64")?;
            let modified = output["modified"].as_bool().ok_or("modified not bool")?;
            if passed != (violations == 0) {
                return Err(format!(
                    "passed={passed} inconsistent with violations={violations}"
                ));
            }
            // scan_input refuses; it never produces sanitized content.
            if modified {
                return Err("scan_input must never modify content".into());
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── canary detection ──────────────────────────────────────────────────────

proptest! {
    /// `check_canary(text)` is exactly a substring test against the guard's
    /// canary token: true iff the canary appears in the text. Embedding the
    /// canary into arbitrary text must trigger detection; clean text must not
    /// produce a false positive.
    #[test]
    fn prop_check_canary_substring_sound(base in arb_text(), embed in any::<bool>()) {
        let guard = guard();
        let canary = guard.canary().as_str().to_string();

        let text = if embed {
            format!("{base}::{canary}::{base}")
        } else {
            base
        };
        let expected = text.contains(&canary);
        let detected = guard.check_canary(&text);

        let input = serde_json::json!({ "embed": embed, "expected": expected });
        let output = serde_json::json!({ "detected": detected });

        let oracle = oracle_invariant(|input, output| {
            let expected = input["expected"].as_bool().ok_or("expected not bool")?;
            let detected = output["detected"].as_bool().ok_or("detected not bool")?;
            if detected != expected {
                return Err(format!("canary detection {detected} != expected {expected}"));
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }

    /// When the canary is present in output text, `scan_output` must reject
    /// (`passed == false`) and the returned sanitized content must no longer
    /// contain the canary token (it is replaced with `[REDACTED-CANARY]`).
    #[test]
    fn prop_scan_output_redacts_canary(base in arb_text()) {
        let guard = guard();
        let canary = guard.canary().as_str().to_string();
        let text = format!("{base}::{canary}::{base}");

        let result = guard.scan_output(&text);
        let sanitized = match &result.output {
            GuardOutput::Sanitized(s) => s.clone(),
            GuardOutput::Clean => String::new(),
        };

        let input = serde_json::json!({ "canary_len": canary.len() });
        let output = serde_json::json!({
            "passed": result.passed,
            "sanitized_contains_canary": sanitized.contains(&canary),
        });

        let oracle = oracle_invariant(|_input, output| {
            let passed = output["passed"].as_bool().ok_or("passed not bool")?;
            let leaked = output["sanitized_contains_canary"]
                .as_bool()
                .ok_or("leaked not bool")?;
            if passed {
                return Err("canary present but scan_output passed".into());
            }
            if leaked {
                return Err("canary survives redaction in sanitized output".into());
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── GuardOutput accessors ─────────────────────────────────────────────────

proptest! {
    /// `GuardOutput::content` returns the original for `Clean` and the stored
    /// sanitized string for `Sanitized`; `is_modified` is true exactly for
    /// `Sanitized`. Verified against a reference re-derivation.
    #[test]
    fn prop_guard_output_accessors(
        original in arb_text(),
        sanitized in arb_text(),
        is_sanitized in any::<bool>(),
    ) {
        let output = if is_sanitized {
            GuardOutput::Sanitized(sanitized.clone())
        } else {
            GuardOutput::Clean
        };
        let content = output.content(&original).to_string();
        let modified = output.is_modified();

        let input = serde_json::json!({
            "original": original,
            "sanitized": sanitized,
            "is_sanitized": is_sanitized,
        });
        let produced = serde_json::json!({ "content": content, "modified": modified });

        let oracle = oracle_reference(|input| {
            let original = input["original"].as_str().unwrap_or("");
            let sanitized = input["sanitized"].as_str().unwrap_or("");
            let is_sanitized = input["is_sanitized"].as_bool().unwrap_or(false);
            let content = if is_sanitized { sanitized } else { original };
            serde_json::json!({ "content": content, "modified": is_sanitized })
        });

        prop_assert_eq!(oracle.verify(&input, &produced), OracleVerdict::Pass);
    }
}

// ── CanaryToken generation ───────────────────────────────────────────────

proptest! {
    /// Two independently generated canary tokens must differ (the token is 32
    /// random bytes hex-encoded; collision is astronomically unlikely). Both
    /// must be 64-character hex strings. This pins the OWASP LLM07 canary
    /// contract: a fresh guard gets a fresh, unpredictable token.
    #[test]
    fn prop_canary_tokens_unique_and_well_formed(_seed in any::<u64>()) {
        let guard = guard();
        let canary = guard.canary().as_str().to_string();
        let other = hkask_guard::CanaryToken::generate();
        let other_str = other.as_str().to_string();

        let input = serde_json::json!({ "seed": _seed });
        let output = serde_json::json!({
            "canary_len": canary.len(),
            "canary_is_hex": canary.chars().all(|c| c.is_ascii_hexdigit()),
            "other_len": other_str.len(),
            "other_is_hex": other_str.chars().all(|c| c.is_ascii_hexdigit()),
            "differ": canary != other_str,
        });

        let oracle = oracle_invariant(|_input, output| {
            let differ = output["differ"].as_bool().ok_or("differ not bool")?;
            if !differ {
                return Err("two generated canary tokens collided".into());
            }
            for key in ["canary_len", "other_len"] {
                let len = output[key].as_u64().ok_or("len not u64")?;
                if len != 64 {
                    return Err(format!("{key} = {len}, expected 64"));
                }
            }
            for key in ["canary_is_hex", "other_is_hex"] {
                let is_hex = output[key].as_bool().ok_or("is_hex not bool")?;
                if !is_hex {
                    return Err(format!("{key} false — token not hex-encoded"));
                }
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}
