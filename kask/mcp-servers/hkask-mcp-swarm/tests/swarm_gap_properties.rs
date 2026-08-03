//! Property tests for the `pub(crate)` swarm helpers exposed under the
//! `test-utils` feature gate (`hkask_mcp_swarm::test_utils`).
//!
//! These cover the gap left by `tests/swarm_properties.rs`, which only reaches
//! the small `pub` surface (`LocalAgentCard`, `SwarmError`). The sanitizers,
//! ABW utilities, config parsing, and consent helpers are `pub(crate)` and are
//! re-exported through `test_utils` when the `test-utils` feature is enabled.
//! This file exercises that seam with proptest + the shared oracle taxonomy.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant`  — check a property of (input, output): never-panic,
//!   determinism, blocked-pattern absence, charset/length conformance.
//! - `oracle_reference`  — compare the output against a trusted independent
//!   implementation (`url_encode_segment`, `effective_hire_cost`).
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): every helper returns a value or `Err` for arbitrary
//!   input — never panics.
//! - P1 (Correctness): deterministic helpers produce identical output for
//!   identical input.
//! - Defense-in-depth: the sanitizers strip the documented injection prefixes
//!   from any input; the invariant pins that the stripped patterns never appear
//!   in the output.

use hkask_mcp_swarm::test_utils::{
    SwarmMode, detect_embedded_error, effective_hire_cost, extract_quoted, filter_declared_skills,
    filter_mcp_tools, fnv1a, make_swarm_slug, mint_token, sanitize_abw_response, sanitize_abw_text,
    sanitize_agent_id, strip_leading_mentions, url_encode_segment, validate_agent_name,
};
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The five injection prefixes `sanitize_abw_text` rewrites. The invariant
/// checks that none of these survives sanitization.
const BLOCKED_PATTERNS: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous instructions",
    "disregard prior instructions",
    "you are now",
    "new instructions:",
];

/// Arbitrary UTF-8 strings of bounded length (exercises unicode, control chars,
/// path separators, injection-shaped payloads).
fn arb_short_string(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..max).prop_map(|cs| cs.into_iter().collect())
}

/// A vector of short identifier-ish strings.
fn arb_string_vec(max_fields: usize, max_len: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(arb_short_string(max_len), 0..max_fields)
}

/// An arbitrary `SystemTime` constructed from a u64 millis offset since the
/// epoch. Keeping the time generated (not `SystemTime::now()`) makes
/// `make_swarm_slug` deterministic across two calls.
fn arb_system_time() -> impl Strategy<Value = SystemTime> {
    (0u64..u64::MAX / 2).prop_map(|millis| UNIX_EPOCH + Duration::from_millis(millis))
}

/// ASCII-only base strings for `make_swarm_slug`. The ABW slug charset is
/// lowercase letters, digits, and underscores (verified live 2026-08-02), so
/// production inputs are ASCII. Note: `make_swarm_slug` truncates with
/// `&base[..max_base]` (a *byte* slice), which panics on multi-byte UTF-8 input
/// that lands mid-character — a pre-existing source bug outside this task's
/// scope (source files are off-limits). Restricting to ASCII exercises the real
/// production surface; the multi-byte case is documented as a gap below.
fn arb_ascii_base(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 0..max).prop_map(|bs| {
        // Keep only printable ASCII so the base is a valid UTF-8 ASCII string.
        bs.into_iter()
            .filter(|b| *b >= 0x20 && *b < 0x7f)
            .map(|b| b as char)
            .collect()
    })
}

/// Independent reference implementation of the path-segment percent-encoder
/// (RFC 3986 §2.3 unreserved set preserved). Structured differently from the
/// real `url_encode_segment` (builds a `Vec<u8>` of bytes first, then joins) so a
/// copy-paste drift in the real impl is not mirrored here.
fn url_encode_segment_reference(segment: &str) -> String {
    let unreserved = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
    let mut parts: Vec<String> = Vec::with_capacity(segment.len());
    for b in segment.bytes() {
        if unreserved(b) {
            parts.push((b as char).to_string());
        } else {
            parts.push(format!("%{b:02X}"));
        }
    }
    parts.join("")
}

/// Independent reference for the hire-cost floor. A dependency-less agent (no
/// `has_dependencies: true`) is floored at `OWNED_ADD_FLAT_FEE`; otherwise the
/// quoted `total_hire_cost` is authoritative. Missing fields default the same
/// way the real impl does.
fn effective_hire_cost_reference(deps: &Value) -> u64 {
    const FEE: u64 = 2;
    let total = deps
        .get("total_hire_cost")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let has_deps = deps
        .get("has_dependencies")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if has_deps {
        total
    } else {
        std::cmp::max(total, FEE)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 1. Sanitizers — never panic, deterministic, blocked patterns removed
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `strip_leading_mentions` never panics on arbitrary UTF-8.
    #[test]
    fn strip_leading_mentions_never_panics(task in arb_short_string(64)) {
        let result = std::panic::catch_unwind(|| strip_leading_mentions(&task));
        prop_assert!(result.is_ok(), "panicked on task={task:?}");
    }

    /// `strip_leading_mentions` is deterministic: same input → same output.
    #[test]
    fn strip_leading_mentions_is_deterministic(task in arb_short_string(64)) {
        let a = strip_leading_mentions(&task);
        let b = strip_leading_mentions(&task);
        prop_assert_eq!(a, b);
    }

    /// Invariant: the result never starts with `@` (all leading mentions were
    /// stripped). An empty result is allowed (task was only mentions).
    #[test]
    fn strip_leading_mentions_result_never_starts_with_at(task in arb_short_string(64)) {
        let out = strip_leading_mentions(&task);
        let input = json!(task);
        let oracle = oracle_invariant(|_, output: &Value| {
            let s = output.as_str().ok_or("output not a string")?;
            if s.starts_with('@') {
                Err(format!("result starts with '@': {s:?}"))
            } else {
                Ok(())
            }
        });
        let out_val = json!(out);
        prop_assert_eq!(oracle.verify(&input, &out_val), OracleVerdict::Pass);
    }

    /// `sanitize_abw_text` never panics on arbitrary UTF-8.
    #[test]
    fn sanitize_abw_text_never_panics(text in arb_short_string(128)) {
        let result = std::panic::catch_unwind(|| sanitize_abw_text(&text));
        prop_assert!(result.is_ok(), "panicked on text={text:?}");
    }

    /// `sanitize_abw_text` is deterministic.
    #[test]
    fn sanitize_abw_text_is_deterministic(text in arb_short_string(128)) {
        let a = sanitize_abw_text(&text);
        let b = sanitize_abw_text(&text);
        prop_assert_eq!(a, b);
    }

    /// Invariant: none of the blocked injection patterns survives in the
    /// sanitized output, for any input.
    #[test]
    fn sanitize_abw_text_never_contains_blocked_patterns(text in arb_short_string(128)) {
        let out = sanitize_abw_text(&text);
        let input = json!(text);
        let oracle = oracle_invariant(|_, output: &Value| {
            let s = output.as_str().ok_or("output not a string")?;
            for pat in BLOCKED_PATTERNS {
                if s.contains(pat) {
                    return Err(format!("output contains blocked pattern {pat:?}: {s:?}"));
                }
            }
            Ok(())
        });
        let out_val = json!(out);
        prop_assert_eq!(oracle.verify(&input, &out_val), OracleVerdict::Pass);
    }

    /// `sanitize_abw_response` never panics on arbitrary JSON (Some and None).
    #[test]
    fn sanitize_abw_response_never_panics(value in arb_json_value()) {
        let result = std::panic::catch_unwind(|| sanitize_abw_response(Some(&value)));
        prop_assert!(result.is_ok(), "panicked on value={value}");
    }

    /// `sanitize_abw_response` is deterministic.
    #[test]
    fn sanitize_abw_response_is_deterministic(value in arb_json_value()) {
        let a = sanitize_abw_response(Some(&value));
        let b = sanitize_abw_response(Some(&value));
        prop_assert_eq!(a, b);
    }

    /// `sanitize_agent_id` never panics and, when `Some`, the result contains
    /// no path separators and is non-empty / not dot-only.
    #[test]
    fn sanitize_agent_id_safe_and_deterministic(id in arb_short_string(48)) {
        let a = std::panic::catch_unwind(|| sanitize_agent_id(&id));
        prop_assert!(a.is_ok(), "panicked on id={id:?}");
        let out = sanitize_agent_id(&id);
        let out2 = sanitize_agent_id(&id);
        prop_assert_eq!(&out, &out2, "non-deterministic for id={:?}", id);
        if let Some(clean) = out {
            prop_assert!(!clean.is_empty(), "empty result for id={id:?}");
            prop_assert!(
                !clean.chars().all(|c| c == '.'),
                "dot-only result for id={id:?}"
            );
            for c in clean.chars() {
                prop_assert!(
                    c.is_alphanumeric() || c == '-' || c == '_' || c == '.',
                    "invalid char {c:?} in result for id={id:?}"
                );
            }
        }
    }

    /// `filter_mcp_tools` never panics on arbitrary string vectors + allowlists.
    #[test]
    fn filter_mcp_tools_never_panics_and_deterministic(
        tools in arb_string_vec(12, 32),
        allowed in prop::option::of(arb_string_vec(6, 24)),
    ) {
        let result = std::panic::catch_unwind(|| filter_mcp_tools(tools.clone(), allowed.as_deref()));
        prop_assert!(result.is_ok(), "panicked on tools={tools:?}");
        let a = filter_mcp_tools(tools.clone(), allowed.as_deref());
        let b = filter_mcp_tools(tools, allowed.as_deref());
        prop_assert_eq!(a, b);
    }

    /// Invariant: every kept entry is `server/tool` shaped with the documented
    /// charset (non-empty, alphanumeric + `-_.`).
    #[test]
    fn filter_mcp_tools_kept_entries_are_well_formed(
        tools in arb_string_vec(12, 32),
        allowed in prop::option::of(arb_string_vec(6, 24)),
    ) {
        let kept = filter_mcp_tools(tools, allowed.as_deref());
        for entry in &kept {
            let (server, tool) = match entry.split_once('/') {
                Some(x) => x,
                None => {
                    prop_assert!(false, "not server/tool shaped: {entry:?}");
                    return Ok(());
                }
            };
            prop_assert!(!server.is_empty(), "empty server in {entry:?}");
            prop_assert!(
                server.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "bad server charset in {entry:?}"
            );
            prop_assert!(!tool.is_empty(), "empty tool in {entry:?}");
            prop_assert!(
                tool.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
                "bad tool charset in {entry:?}"
            );
        }
        // Also: when an allowlist is set, every kept server is in it.
        if let Some(allow) = allowed.as_deref() {
            for entry in &kept {
                let server = entry.split_once('/').unwrap().0;
                prop_assert!(
                    allow.iter().any(|s| s == server),
                    "kept server {server:?} not in allowlist"
                );
            }
        }
    }

    /// `filter_declared_skills` never panics and is deterministic; kept entries
    /// conform to the id charset + length bound.
    #[test]
    fn filter_declared_skills_safe_and_conformant(skills in arb_string_vec(12, 32)) {
        let result = std::panic::catch_unwind(|| filter_declared_skills(skills.clone()));
        prop_assert!(result.is_ok(), "panicked on skills={skills:?}");
        let a = filter_declared_skills(skills.clone());
        let b = filter_declared_skills(skills);
        prop_assert_eq!(&a, &b);
        for id in &a {
            prop_assert!(!id.is_empty() && id.len() <= 128, "bad length for {id:?}");
            prop_assert!(
                id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
                "bad charset for {id:?}"
            );
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. ABW utilities — never panic, deterministic, reference-checked
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `detect_embedded_error` never panics on arbitrary JSON and is
    /// deterministic.
    #[test]
    fn detect_embedded_error_safe_and_deterministic(value in arb_json_value()) {
        let result = std::panic::catch_unwind(|| detect_embedded_error(&value));
        prop_assert!(result.is_ok(), "panicked on value={value}");
        let a = detect_embedded_error(&value);
        let b = detect_embedded_error(&value);
        prop_assert_eq!(a.is_some(), b.is_some(), "non-deterministic Some/None");
    }

    /// `extract_quoted` never panics and is deterministic.
    #[test]
    fn extract_quoted_safe_and_deterministic(text in arb_short_string(64)) {
        let result = std::panic::catch_unwind(|| extract_quoted(&text));
        prop_assert!(result.is_ok(), "panicked on text={text:?}");
        let a = extract_quoted(&text);
        let b = extract_quoted(&text);
        prop_assert_eq!(a, b);
    }

    /// `url_encode_segment` never panics on arbitrary UTF-8.
    #[test]
    fn url_encode_segment_never_panics(segment in arb_short_string(48)) {
        let result = std::panic::catch_unwind(|| url_encode_segment(&segment));
        prop_assert!(result.is_ok(), "panicked on segment={segment:?}");
    }

    /// `url_encode_segment` is deterministic.
    #[test]
    fn url_encode_segment_is_deterministic(segment in arb_short_string(48)) {
        let a = url_encode_segment(&segment);
        let b = url_encode_segment(&segment);
        prop_assert_eq!(a, b);
    }

    /// Reference oracle: the real encoder matches an independent RFC 3986
    /// implementation for any input.
    #[test]
    fn url_encode_segment_matches_reference(segment in arb_short_string(48)) {
        let output = url_encode_segment(&segment);
        let input = json!(segment);
        let oracle = oracle_reference(|inp: &Value| {
            let s = inp.as_str().unwrap_or("");
            json!(url_encode_segment_reference(s))
        });
        let out_val = json!(output);
        prop_assert_eq!(
            oracle.verify(&input, &out_val),
            OracleVerdict::Pass,
            "url_encode_segment({:?}) = {}, expected {}",
            segment, output, url_encode_segment_reference(&segment)
        );
    }

    /// `make_swarm_slug` never panics on arbitrary ASCII base + (pre- or
    /// post-epoch) times and is deterministic for a fixed time. (ASCII-only —
    /// see `arb_ascii_base` for the multi-byte UTF-8 source-bug note.)
    #[test]
    fn make_swarm_slug_safe_deterministic_and_bounded(
        base in arb_ascii_base(100),
        now in arb_system_time(),
    ) {
        let result = std::panic::catch_unwind(|| make_swarm_slug(&base, now));
        prop_assert!(result.is_ok(), "panicked on base={base:?} now={now:?}");
        let a = make_swarm_slug(&base, now);
        let b = make_swarm_slug(&base, now);
        prop_assert_eq!(&a, &b, "non-deterministic for base={:?}", base);
        // ABW slug cap (verified live 2026-08-02): total length ≤ 64.
        prop_assert!(a.len() <= 64, "slug exceeds 64 chars: {a} ({} chars)", a.len());
    }

    /// `validate_agent_name` never panics and is deterministic. When it returns
    /// `Ok`, the name conforms to the ABW slug rule (3–64 chars, lowercase
    /// alphanumeric + underscore).
    #[test]
    fn validate_agent_name_safe_deterministic_conformant(name in arb_short_string(80)) {
        let result = std::panic::catch_unwind(|| validate_agent_name(&name));
        prop_assert!(result.is_ok(), "panicked on name={name:?}");
        let a = validate_agent_name(&name);
        let b = validate_agent_name(&name);
        prop_assert_eq!(&a, &b, "non-deterministic for name={:?}", name);
        if let Ok(()) = a {
            let len = name.chars().count();
            prop_assert!((3..=64).contains(&len), "Ok for out-of-range length {len}");
            prop_assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "Ok for invalid-charset name {name:?}"
            );
        }
    }

    /// `effective_hire_cost` never panics on arbitrary JSON and is deterministic.
    #[test]
    fn effective_hire_cost_safe_and_deterministic(deps in arb_json_value()) {
        let result = std::panic::catch_unwind(|| effective_hire_cost(&deps));
        prop_assert!(result.is_ok(), "panicked on deps={deps}");
        let a = effective_hire_cost(&deps);
        let b = effective_hire_cost(&deps);
        prop_assert_eq!(a, b);
    }

    /// Reference oracle: the real floor matches the independent reference for
    /// any JSON input.
    #[test]
    fn effective_hire_cost_matches_reference(deps in arb_json_value()) {
        let output = effective_hire_cost(&deps);
        let input = deps.clone();
        let oracle = oracle_reference(|inp: &Value| json!(effective_hire_cost_reference(inp)));
        let out_val = json!(output);
        prop_assert_eq!(
            oracle.verify(&input, &out_val),
            OracleVerdict::Pass,
            "effective_hire_cost({}) = {}, expected {}",
            deps, output, effective_hire_cost_reference(&deps)
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. Consent helpers — fnv1a deterministic; mint_token format; SwarmMode total
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `fnv1a` never panics on arbitrary (action, target) and is deterministic:
    /// two calls with the same inputs produce the same hash.
    #[test]
    fn fnv1a_safe_and_deterministic(action in arb_short_string(48), target in arb_short_string(48)) {
        let result = std::panic::catch_unwind(|| fnv1a(&action, &target));
        prop_assert!(result.is_ok(), "panicked on action={action:?} target={target:?}");
        let a = fnv1a(&action, &target);
        let b = fnv1a(&action, &target);
        prop_assert_eq!(&a, &b, "fnv1a non-deterministic for action={:?} target={:?}", action, target);
    }

    /// `mint_token` never panics on arbitrary (action, target).
    #[test]
    fn mint_token_never_panics(action in arb_short_string(48), target in arb_short_string(48)) {
        let result = std::panic::catch_unwind(|| mint_token(&action, &target));
        prop_assert!(result.is_ok(), "panicked on action={action:?} target={target:?}");
    }

    /// Invariant: `mint_token` always returns the documented opaque format —
    /// `hkask-consent-` prefix followed by 16 lowercase hex digits. (The token
    /// embeds `SystemTime::now()` nanos XOR `fnv1a`, so it is not bit-stable
    /// across calls; the format invariant is the stable property.)
    #[test]
    fn mint_token_has_documented_format(action in arb_short_string(48), target in arb_short_string(48)) {
        let token = mint_token(&action, &target);
        let input = json!({"action": action, "target": target});
        let oracle = oracle_invariant(|_, output: &Value| {
            let s = output.as_str().ok_or("token not a string")?;
            let rest = s
                .strip_prefix("hkask-consent-")
                .ok_or_else(|| format!("missing hkask-consent- prefix: {s:?}"))?;
            if rest.len() != 16 {
                return Err(format!("suffix not 16 hex chars: {rest:?} (len {})", rest.len()));
            }
            if !rest.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("suffix not hex: {rest:?}"));
            }
            Ok(())
        });
        let out_val = json!(token);
        prop_assert_eq!(oracle.verify(&input, &out_val), OracleVerdict::Pass);
    }
}

proptest! {
    /// `SwarmMode::from_str` is total: for any string it returns `Ok` (a valid
    /// mode) or `Err` — never panics.
    #[test]
    fn swarm_mode_from_str_total(s in arb_short_string(16)) {
        let result = std::panic::catch_unwind(|| s.parse::<SwarmMode>());
        prop_assert!(result.is_ok(), "from_str panicked on s={s:?}");
        let parsed = result.unwrap();
        prop_assert!(parsed.is_ok() || parsed.is_err(), "neither Ok nor Err");
    }

    /// Determinism: the same string parses to the same variant (or the same
    /// error) every time.
    #[test]
    fn swarm_mode_from_str_deterministic(s in arb_short_string(16)) {
        let a = s.parse::<SwarmMode>();
        let b = s.parse::<SwarmMode>();
        prop_assert_eq!(a.ok(), b.ok(), "non-deterministic parse for s={:?}", s);
    }
}
