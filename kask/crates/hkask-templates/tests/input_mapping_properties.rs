//! Property tests for the dot-path context resolver exposed under the
//! `test-utils` feature gate (`hkask_templates::test_utils::resolve_dot_path`).
//!
//! `input_mapping.rs` is `pub(crate)` — the `test_utils` module re-exports
//! `resolve_dot_path`, closing the gap that left this pure lookup surface with
//! no direct test (it was only exercised indirectly through `condition` and
//! `convergence`). These tests drive the real function directly across a
//! generated input space.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant` — never-panic (catch_unwind), determinism.
//! - `oracle_reference` — resolution matches a trusted reference built from
//!   the same contract (single-segment == `context.get`; nested resolution
//!   walks the object chain).
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): `resolve_dot_path` is total — for any path string
//!   and any context it returns `Option<Value>`, never panics.
//! - P1 (Correctness): a single-segment path equals `context.get`; a nested
//!   path resolves iff every segment exists and every intermediate is an
//!   object.
//! - Determinism: pure function — no I/O, no RNG.

use hkask_templates::test_utils::resolve_dot_path;
use hkask_test_harness::arb_json_value;
use proptest::prelude::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────────
// Input strategies
// ──────────────────────────────────────────────────────────────────────────

/// A single path segment: short identifier-ish strings.
fn arb_segment() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z_][a-z0-9_]{0,11}").expect("valid regex")
}

/// A dot-path: 1 to 4 segments joined by `.`. Covers the single-segment
/// (direct key) and nested (dot-path) branches.
fn arb_dot_path() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_segment(), 1..4).prop_map(|segs| segs.join("."))
}

/// A context value restricted to JSON objects (the only kind that supports
/// nested dot-path resolution) and scalars (which terminate resolution).
fn arb_resolvable_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        prop::string::string_regex("[a-z0-9_]{0,8}")
            .unwrap()
            .prop_map(Value::String),
    ];
    // Recursive: leaf or object with a few resolvable values. Depth bounded
    // by proptest's default size to keep generated values tractable.
    let node = leaf.clone().prop_map(|v| v);
    prop_oneof![
        leaf,
        prop::collection::hash_map(arb_segment(), node, 0..4)
            .prop_map(|m| Value::Object(m.into_iter().collect())),
    ]
}

/// A context: a small map of identifier keys to resolvable values.
fn arb_context() -> impl Strategy<Value = HashMap<String, Value>> {
    prop::collection::hash_map(arb_segment(), arb_resolvable_value(), 0..6)
}

// ──────────────────────────────────────────────────────────────────────────
// 1. resolve_dot_path — totality and determinism
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `resolve_dot_path` never panics on arbitrary path strings and arbitrary
    /// context — it is total (returns `Option<Value>`).
    #[test]
    fn resolve_dot_path_never_panics(
        path in arb_dot_path(),
        context in arb_context(),
    ) {
        let result = std::panic::catch_unwind(|| {
            resolve_dot_path(&path, &context)
        });
        prop_assert!(result.is_ok(), "panicked on path={:?} context={:?}", path, context);
    }

    /// `resolve_dot_path` is deterministic: two calls with the same path and
    /// context produce equal results.
    #[test]
    fn resolve_dot_path_is_deterministic(
        path in arb_dot_path(),
        context in arb_context(),
    ) {
        let a = resolve_dot_path(&path, &context);
        let b = resolve_dot_path(&path, &context);
        prop_assert_eq!(a, b, "non-deterministic for path={:?}", path);
    }

    /// `resolve_dot_path` on arbitrary JSON (not just objects) never panics —
    /// the context values themselves can be arrays/numbers/strings, and a
    /// nested path into a non-object returns `None` (not a panic).
    #[test]
    fn resolve_dot_path_never_panics_on_arbitrary_json_context(
        path in arb_dot_path(),
        context in arb_json_value(),
    ) {
        // Wrap the arbitrary JSON in a single-key HashMap so the first segment
        // resolves (to the arbitrary value); subsequent segments walk into it.
        let mut ctx = HashMap::new();
        ctx.insert("root".to_string(), context);
        let full_path = format!("root.{path}");
        let result = std::panic::catch_unwind(|| {
            resolve_dot_path(&full_path, &ctx)
        });
        prop_assert!(result.is_ok(), "panicked on path={:?}", full_path);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. resolve_dot_path — single-segment equivalence
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// A single-segment path resolves to the same value as `context.get(key)`.
    /// This is the reference oracle: `resolve_dot_path(key) == context.get(key)`.
    #[test]
    fn resolve_dot_path_single_segment_equals_context_get(
        key in arb_segment(),
        value in arb_resolvable_value(),
    ) {
        let mut context = HashMap::new();
        context.insert(key.clone(), value);
        let resolved = resolve_dot_path(&key, &context);
        let reference = context.get(&key).cloned();
        prop_assert_eq!(resolved, reference, "single-segment mismatch for {}", key);
    }

    /// A single-segment path for an absent key returns `None` (matches
    /// `context.get`).
    #[test]
    fn resolve_dot_path_absent_single_segment_is_none(
        key in arb_segment(),
    ) {
        let context: HashMap<String, Value> = HashMap::new();
        let resolved = resolve_dot_path(&key, &context);
        prop_assert_eq!(resolved, None, "absent key resolved to a value for {}", key);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. resolve_dot_path — nested resolution contract
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// A two-segment path `outer.inner` resolves iff `outer` is an object
    /// containing `inner`. The reference walks the same chain.
    #[test]
    fn resolve_dot_path_two_segment_matches_reference(
        outer in arb_segment(),
        inner in arb_segment(),
        leaf in arb_resolvable_value(),
        present in any::<bool>(),
    ) {
        let mut inner_map = Map::new();
        if present {
            inner_map.insert(inner.clone(), leaf.clone());
        }
        let mut context = HashMap::new();
        context.insert(outer.clone(), Value::Object(inner_map));

        let path = format!("{outer}.{inner}");
        let resolved = resolve_dot_path(&path, &context);
        let reference = if present {
            Some(leaf)
        } else {
            None
        };
        prop_assert_eq!(resolved, reference, "two-segment mismatch for {}", path);
    }

    /// A nested path into a non-object intermediate returns `None` (not a
    /// panic). Pins the `Value::Object(map) => ... _ => return None` arm.
    #[test]
    fn resolve_dot_path_into_non_object_returns_none(
        outer in arb_segment(),
        inner in arb_segment(),
        scalar in prop_oneof![
            any::<i64>().prop_map(|n| json!(n)),
            any::<bool>().prop_map(Value::Bool),
            prop::string::string_regex("[a-z]{0,6}").unwrap().prop_map(Value::String),
            Just(Value::Null),
        ],
    ) {
        let mut context = HashMap::new();
        context.insert(outer.clone(), scalar);
        let path = format!("{outer}.{inner}");
        let resolved = resolve_dot_path(&path, &context);
        prop_assert_eq!(resolved, None, "nested path into scalar resolved for {}", path);
    }

    /// A deeply-nested path resolves iff every intermediate is an object
    /// containing the next segment. The reference walks the same chain.
    #[test]
    fn resolve_dot_path_deep_nested_matches_reference(
        segs in prop::collection::vec(arb_segment(), 2..5),
        leaf in arb_resolvable_value(),
    ) {
        // Build the inner chain: segs[1] -> { segs[2] -> { ... -> leaf } },
        // then insert under segs[0] as the context key.
        let mut current = leaf.clone();
        for seg in segs[1..].iter().rev() {
            let mut map = Map::new();
            map.insert(seg.clone(), current);
            current = Value::Object(map);
        }
        let root_key = segs[0].clone();
        let mut context = HashMap::new();
        context.insert(root_key, current);

        let path = segs.join(".");
        let resolved = resolve_dot_path(&path, &context);
        prop_assert_eq!(resolved, Some(leaf), "deep-nested mismatch for {}", path);
    }

    /// A deeply-nested path with a missing intermediate segment returns `None`.
    #[test]
    fn resolve_dot_path_deep_nested_missing_intermediate_is_none(
        present_segs in prop::collection::vec(arb_segment(), 2..4),
        missing_seg in arb_segment(),
        leaf in arb_resolvable_value(),
    ) {
        // Build the inner chain from present_segs[1..], then insert under
        // present_segs[0] as the context key.
        let mut current = leaf;
        for seg in present_segs[1..].iter().rev() {
            let mut map = Map::new();
            map.insert(seg.clone(), current);
            current = Value::Object(map);
        }
        let root_key = present_segs[0].clone();
        let mut context = HashMap::new();
        context.insert(root_key, current);

        // Path: present_segs[0] . missing_seg . present_segs[1..]
        let mut path_segs = vec![present_segs[0].clone(), missing_seg];
        path_segs.extend(present_segs[1..].iter().cloned());
        let path = path_segs.join(".");
        let resolved = resolve_dot_path(&path, &context);
        prop_assert_eq!(resolved, None, "missing intermediate resolved for {}", path);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 4. resolve_dot_path — empty/edge paths
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// An empty path string: `"".split('.')` yields `[""]`, so this looks up
    /// the key `""` in the context. Pins the actual behavior (it equals
    /// `context.get("")`), not an assumed `None`.
    #[test]
    fn resolve_dot_path_empty_string_behavior(
        context in arb_context(),
    ) {
        let resolved = resolve_dot_path("", &context);
        prop_assert_eq!(resolved, context.get("").cloned(), "empty-path behavior diverged");
    }

    /// A path with trailing dots (`a.b.`) splits to `["a", "b", ""]`; the empty
    /// final segment looks up `""` in the intermediate object, which is
    /// absent, so resolution returns `None`. Pins the no-panic-on-trailing-dot
    /// property.
    #[test]
    fn resolve_dot_path_trailing_dot_returns_none(
        outer in arb_segment(),
        inner in arb_segment(),
        leaf in arb_resolvable_value(),
    ) {
        let mut inner_map = Map::new();
        inner_map.insert(inner.clone(), leaf);
        let mut context = HashMap::new();
        context.insert(outer.clone(), Value::Object(inner_map));

        let path = format!("{outer}.{inner}.");
        let result = std::panic::catch_unwind(|| resolve_dot_path(&path, &context));
        prop_assert!(result.is_ok(), "panicked on trailing-dot path {:?}", path);
        let resolved = result.unwrap();
        prop_assert_eq!(resolved, None, "trailing-dot path resolved for {}", path);
    }
}
