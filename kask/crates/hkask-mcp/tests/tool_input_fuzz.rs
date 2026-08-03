//! MCP tool response parser fuzz test — retargeted at kask's `tool_response`.
//!
//! Verifies that `parse_tool_response` and `unwrap_tool_envelope` (kask's own
//! JSON parsing functions for MCP tool responses) never panic on arbitrary
//! structured JSON input, and that the envelope-unwrapping property holds:
//! if the input has a "content" key, the result is the inner value; otherwise
//! the input passes through unchanged.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces must reject invalid input gracefully, never panic
//! - P1 (Correctness): envelope unwrapping is deterministic and lossless

use hkask_types::tool_response::{parse_tool_response, unwrap_tool_envelope};
use proptest::prelude::*;
use serde_json::Value as JsonValue;

/// Recursive JSON value strategy — produces structured trees, not raw bytes.
fn arb_json_value() -> BoxedStrategy<JsonValue> {
    let leaf = prop_oneof![
        Just(JsonValue::Null),
        any::<bool>().prop_map(JsonValue::Bool),
        any::<i64>().prop_map(|n| serde_json::json!(n)),
        any::<u64>().prop_map(|n| serde_json::json!(n)),
        any::<f64>()
            .prop_filter("must be finite", |f| f.is_finite())
            .prop_map(|n| serde_json::json!(n)),
        any::<String>().prop_map(JsonValue::String),
    ];
    leaf.prop_recursive(
        4,  // max depth
        64, // desired size
        8,  // expected branch size
        |element| {
            prop_oneof![
                prop::collection::vec(element.clone(), 0..8).prop_map(JsonValue::Array),
                prop::collection::vec((any::<String>(), element), 0..8).prop_map(|pairs| {
                    let mut map = serde_json::Map::new();
                    for (k, v) in pairs {
                        map.insert(k, v);
                    }
                    JsonValue::Object(map)
                }),
            ]
            .boxed()
        },
    )
    .boxed()
}

proptest! {
    // unwrap_tool_envelope never panics and correctly unwraps the "content" key
    // when present, returning the input unchanged otherwise.
    #[test]
    fn unwrap_envelope_never_panics_and_preserves_property(
        value in arb_json_value(),
    ) {
        let result = std::panic::catch_unwind(|| {
            unwrap_tool_envelope(value.clone())
        });
        prop_assert!(result.is_ok(),
            "unwrap_tool_envelope panicked on: {}", value);

        let unwrapped = result.unwrap();
        match &value {
            JsonValue::Object(map) if map.contains_key("content") => {
                prop_assert_eq!(
                    unwrapped,
                    map.get("content").unwrap().clone(),
                    "envelope with content key must unwrap to inner value"
                );
            }
            _ => {
                prop_assert_eq!(
                    unwrapped, value,
                    "non-envelope value must pass through unchanged"
                );
            }
        }
    }

    // parse_tool_response never panics and is consistent with
    // unwrap_tool_envelope on valid JSON strings.
    #[test]
    fn parse_tool_response_never_panics_and_is_consistent(
        value in arb_json_value(),
    ) {
        let json_string = serde_json::to_string(&value)
            .expect("JSON serialization is infallible for Value");
        // Re-parse the serialized string so both sides of the comparison go
        // through the same serialization → parsing path, avoiding float
        // precision mismatches between the original Value and the parsed one.
        let reparsed: JsonValue = serde_json::from_str(&json_string)
            .expect("re-parsing a serialized Value must succeed");
        let result = std::panic::catch_unwind(|| {
            parse_tool_response(&json_string)
        });
        prop_assert!(result.is_ok(),
            "parse_tool_response panicked on: {}", json_string);

        let parsed = result.unwrap();
        let expected = unwrap_tool_envelope(reparsed);
        prop_assert_eq!(
            parsed, Some(expected),
            "parse_tool_response must return Some(unwrap_tool_envelope(value)) for valid JSON"
        );
    }

    // parse_tool_response returns None on invalid JSON strings without panicking.
    #[test]
    fn parse_tool_response_returns_none_on_invalid_json(
        input in proptest::arbitrary::any::<String>()
            .prop_filter("must not be valid JSON", |s| {
                serde_json::from_str::<JsonValue>(s).is_err()
            }),
    ) {
        let result = std::panic::catch_unwind(|| {
            parse_tool_response(&input)
        });
        prop_assert!(result.is_ok(),
            "parse_tool_response panicked on invalid input: {}", input);
    }
}
