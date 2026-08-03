//! Shared test fixtures and property-test generators for hKask.
//!
//! Three public items, each with lightweight dependencies:
//! - [`arb_json_value`]: recursive JSON value strategy for proptest
//! - [`NoopToolPort`]: stub `ToolPort` returning NotFound for all invocations
//! - [`test_token_for_tool`]: deterministic `DelegationToken` fixture for governance tests

use hkask_capability::{
    DelegationAction, DelegationResource, DelegationToken, ToolFuture, ToolInfo, ToolPort,
    ToolPortError,
};
use hkask_types::{NotFound, WebID};
use proptest::prelude::*;
use serde_json::Value as JsonValue;

// ── Property-test generator ──────────────────────────────────────────────

/// Recursive JSON value strategy — produces structured trees, not raw bytes.
///
/// JSON is a subset of YAML, so `serde_yaml_neo` can parse these strings.
/// Leaves: Null, Bool, i64, u64, finite f64, String.
/// Branches: Array (0..8 elements), Object (0..8 string-keyed entries).
/// Max depth: 4. This exercises deserializers with structurally valid input
/// that may or may not match the target type's schema.
pub fn arb_json_value() -> BoxedStrategy<JsonValue> {
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

// ── ToolPort stub ────────────────────────────────────────────────────────

/// Stub `ToolPort` that returns `NotFound` for every invocation and
/// `None`/empty for discovery. Use in tests that need a `ToolPort` fixture
/// but don't exercise the invoke path.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopToolPort;

impl ToolPort for NoopToolPort {
    fn invoke<'a>(
        &'a self,
        _server: &'a str,
        tool: &'a str,
        _args: JsonValue,
        _token: &'a DelegationToken,
    ) -> ToolFuture<'a, Result<JsonValue, ToolPortError>> {
        Box::pin(async move {
            Err(ToolPortError::NotFound(NotFound {
                entity_type: "tool".to_string(),
                id: tool.to_string(),
            }))
        })
    }

    fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
        Box::pin(async { Vec::new() })
    }

    fn get_tool_info<'a>(&'a self, _tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
        Box::pin(async { None })
    }
}

// ── Token fixture ────────────────────────────────────────────────────────

/// Deterministic `DelegationToken` fixture for governance tests.
///
/// Mints a `Tool:Execute` token for the named tool, using fixed test personas.
/// The `delegated_to` field (the gas-budget owner) is `WebID::from_persona(b"test-agent")`.
/// Seed gas budgets for this WebID to test the allow path.
#[must_use]
pub fn test_token_for_tool(tool_name: &str) -> DelegationToken {
    DelegationToken::new(
        DelegationResource::Tool,
        tool_name.to_string(),
        DelegationAction::Execute,
        WebID::from_persona(b"test-from"),
        WebID::from_persona(b"test-agent"),
    )
}

/// The `delegated_to` WebID used by [`test_token_for_tool`].
/// Seed gas budgets for this agent in governance tests.
#[must_use]
pub fn test_agent_webid() -> WebID {
    WebID::from_persona(b"test-agent")
}
