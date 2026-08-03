//! Manifest deserializer fuzz test — retargeted at kask's `RegistryEntry`.
//!
//! Verifies that `RegistryEntry` deserialization (the kask type that all
//! manifest loading goes through) never panics on arbitrary structured YAML
//! input, and that successfully-deserialized entries survive a
//! serialize→deserialize round-trip with no field loss.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces must reject invalid input gracefully, never panic
//! - P1 (Correctness): serialization is lossless — round-trip preserves all fields

use hkask_types::RegistryEntry;
use hkask_types::template_type::TemplateType;
use proptest::prelude::*;
use serde_json::Value as JsonValue;

/// Recursive JSON value strategy — produces structured trees, not raw bytes.
/// JSON is a subset of YAML, so `serde_yaml_neo` can parse these strings,
/// exercising kask's derived `Deserialize` for `RegistryEntry` on valid YAML
/// structures that may or may not match the `RegistryEntry` schema.
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

/// Strategy for valid `TemplateType` variants.
fn arb_template_type() -> BoxedStrategy<TemplateType> {
    prop::sample::select(&[
        TemplateType::WordAct,
        TemplateType::KnowAct,
        TemplateType::FlowDef,
        TemplateType::RenderAct,
    ])
    .boxed()
}

/// Strategy for valid `RegistryEntry` values.
fn arb_registry_entry() -> BoxedStrategy<RegistryEntry> {
    (
        any::<String>(),
        arb_template_type(),
        any::<String>(),
        any::<String>(),
        any::<String>(),
        any::<u32>(),
        any::<u32>(),
    )
        .prop_map(
            |(
                id,
                template_type,
                name,
                description,
                source_path,
                cascade_level,
                matroshka_limit,
            )| {
                RegistryEntry {
                    id,
                    template_type,
                    name,
                    description,
                    source_path,
                    cascade_level,
                    matroshka_limit,
                }
            },
        )
        .boxed()
}

proptest! {
    // Arbitrary structured YAML never panics RegistryEntry's deserializer.
    #[test]
    fn registry_entry_deserializer_never_panics_on_structured_input(
        value in arb_json_value(),
    ) {
        let yaml_string = serde_json::to_string(&value).expect("JSON serialization is infallible for Value");
        let result = std::panic::catch_unwind(|| {
            let _: Result<RegistryEntry, _> = serde_yaml_neo::from_str(&yaml_string);
        });
        prop_assert!(result.is_ok(),
            "RegistryEntry deserializer panicked on structured input: {}", yaml_string);
    }

    // Successfully-deserialized entries survive a serialize→deserialize round-trip
    // with no field loss.
    #[test]
    fn registry_entry_round_trips_through_yaml(
        entry in arb_registry_entry(),
    ) {
        let serialized = serde_yaml_neo::to_string(&entry)
            .expect("serialization of a valid RegistryEntry must succeed");
        let deserialized: RegistryEntry = serde_yaml_neo::from_str(&serialized)
            .expect("deserialization of serialized RegistryEntry must succeed");
        prop_assert_eq!(entry.id, deserialized.id, "round-trip lost id");
        prop_assert_eq!(entry.template_type, deserialized.template_type, "round-trip lost template_type");
        prop_assert_eq!(entry.name, deserialized.name, "round-trip lost name");
        prop_assert_eq!(entry.description, deserialized.description, "round-trip lost description");
        prop_assert_eq!(entry.source_path, deserialized.source_path, "round-trip lost source_path");
        prop_assert_eq!(entry.cascade_level, deserialized.cascade_level, "round-trip lost cascade_level");
        prop_assert_eq!(entry.matroshka_limit, deserialized.matroshka_limit, "round-trip lost matroshka_limit");
    }
}
