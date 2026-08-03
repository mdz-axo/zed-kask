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

use hkask_test_harness::arb_json_value;
use hkask_types::tool_response::{parse_tool_response, unwrap_tool_envelope};
use proptest::prelude::*;
use serde_json::{Value as JsonValue, json};

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

    // Wrapping any value in a {"content": v} envelope and unwrapping returns v.
    #[test]
    fn unwrap_envelope_lifts_content_key(
        value in arb_json_value(),
    ) {
        let enveloped = json!({"content": value.clone()});
        let unwrapped = unwrap_tool_envelope(enveloped);
        prop_assert_eq!(
            unwrapped, value,
            "unwrap_tool_envelope must extract the content key's value"
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
