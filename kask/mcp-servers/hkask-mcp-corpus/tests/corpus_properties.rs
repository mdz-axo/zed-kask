//! Property tests for the corpus server's pure `ModelInfo` conversions.
//!
//! The deleted stub-based tests (`MockProvider`, `AlwaysFails`, `EmptyListPort`)
//! exercised async dispatch paths that needed `InferencePort`/`ProviderIntelligence`
//! stubs. The genuinely pure, stub-free logic in those three source files is:
//!
//! - `inference_svc::ModelInfo::from(hkask_types::ModelEntry)` — parses the
//!   provider from the prefixed model name (the only branching parser in the
//!   inference-service module).
//! - `inference_svc::ModelInfo::from(hkask_inference::RouterModelEntry)` — a
//!   field-preserving conversion that drops `model` / `supports_vision`.
//!
//! These are tested here with proptest + the harness Oracle taxonomy.
//!
//! # Gaps (pure functions that are NOT testable from `tests/`)
//!
//! - `adaptive_monitor::WatchedProvider::interval_for_fraction(f64) -> Duration`
//!   is the threshold math the deleted `MockProvider` tests indirectly covered,
//!   but both `WatchedProvider` and the fn are private — unreachable from an
//!   integration test, and exposing them would require a source edit.
//! - `model_cache::ttl_from_env` (private) and `ModelCache::is_stale`/
//!   `invalidate` (public but read/mutate process-global static state with a
//!   live `Instant` clock, so they are not deterministic pure functions).
//! - `InferenceService::resolve_port` / `list_models` / `search_models` need a
//!   real `InferencePort` (constructing one would be a forbidden stub).

use hkask_inference::{ProviderId, RouterModelEntry};
use hkask_mcp_corpus::inference_svc::ModelInfo;
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use hkask_types::ModelEntry;
use proptest::prelude::*;
use serde_json::{Value as JsonValue, json};

/// Alias → serde-renamed `ProviderId` table. This is a **data-driven** reference
/// (table lookup), intentionally structurally different from the source's `match`
/// arms, so a bug in one is unlikely to appear identically in the other — the
/// independence that makes a reference oracle worth anything.
const PROVIDER_ALIASES: &[(&[&str], &str)] = &[
    (&["deepinfra", "di"], "DI"),
    (&["fal", "fa", "fal.ai"], "FA"),
    (&["together", "tg"], "TG"),
    (&["runpod", "rp"], "RP"),
    (&["openrouter", "or"], "OR"),
    (&["kilocode", "kc"], "KC"),
    (&["ollama", "om"], "OM"),
    (&["cline", "cl"], "CL"),
    (&["z.ai", "zai", "za"], "ZA"),
];

/// The serde id for every `ProviderId` variant (for the "valid provider"
/// invariant). `OpenRouter` is the fallback, so it appears once here.
const ALL_PROVIDER_IDS: &[&str] = &["DI", "FA", "TG", "RP", "OR", "KC", "OM", "CL", "ZA"];

/// All ten `ProviderId` variants — for `prop::sample::select` when generating
/// `RouterModelEntry`.
const ALL_PROVIDERS: [ProviderId; 9] = [
    ProviderId::DeepInfra,
    ProviderId::Fal,
    ProviderId::Together,
    ProviderId::Runpod,
    ProviderId::OpenRouter,
    ProviderId::KiloCode,
    ProviderId::Ollama,
    ProviderId::Cline,
    ProviderId::Zai,
];

/// Reference implementation of `ModelInfo::from(ModelEntry)`, written as a
/// table lookup over `PROVIDER_ALIASES` rather than the source's `match`.
fn reference_model_info_from_entry(input: &JsonValue) -> JsonValue {
    let name = input["prefixed_name"].as_str().unwrap_or("").to_string();
    let prefix = name.split('/').next().unwrap_or("openrouter");
    let lower = prefix.to_lowercase();
    let provider = PROVIDER_ALIASES
        .iter()
        .find_map(|(aliases, serde_id)| aliases.iter().any(|a| *a == lower).then(|| *serde_id))
        .unwrap_or("OR");
    json!({
        "name": name,
        "provider": provider,
        "family": null,
        "parameter_size": null,
        "quantization_level": null,
        "size_bytes": null,
    })
}

/// Strategy over known provider-prefix spellings (incl. case variants and the
/// empty/fallback edge) plus a fully arbitrary string, so every `match` arm and
/// the fallback are hit while still exercising pathological names.
fn arb_prefixed_name() -> BoxedStrategy<String> {
    let prefix = prop_oneof![
        Just("deepinfra".to_string()),
        Just("DI".to_string()),
        Just("Di".to_string()),
        Just("fal".to_string()),
        Just("fa".to_string()),
        Just("fal.ai".to_string()),
        Just("together".to_string()),
        Just("tg".to_string()),
        Just("runpod".to_string()),
        Just("rp".to_string()),
        Just("openrouter".to_string()),
        Just("or".to_string()),
        Just("kilocode".to_string()),
        Just("kc".to_string()),
        Just("ollama".to_string()),
        Just("om".to_string()),
        Just("cline".to_string()),
        Just("cl".to_string()),
        Just("z.ai".to_string()),
        Just("zai".to_string()),
        Just("za".to_string()),
        Just("unknown-provider".to_string()),
        Just(String::new()),
        any::<String>(),
    ];
    let suffix = prop::string::string_regex("[A-Za-z0-9._/-]{0,40}").expect("valid regex");
    (prefix, suffix)
        .prop_map(|(p, s)| format!("{p}/{s}"))
        .boxed()
}

/// `Option<String>` drawn from a JSON value: a JSON string becomes `Some`,
/// anything else becomes `None`. Exercises the `Option<String>` roundtrip with
/// arbitrary unicode string content.
fn arb_opt_string_from_json() -> BoxedStrategy<Option<String>> {
    arb_json_value()
        .prop_map(|v| match v {
            JsonValue::String(s) => Some(s),
            _ => None,
        })
        .boxed()
}

proptest! {
    /// `ModelInfo::from(ModelEntry)` derives the provider from the prefixed name
    /// exactly as the independent table-driven reference does, for every known
    /// alias spelling, case variant, the empty/fallback case, and arbitrary
    /// names containing '/', unicode, etc.
    ///
    /// Oracle: [`oracle_reference`] — the reference re-implements the prefix
    /// parser as a table lookup (structurally distinct from the source `match`).
    #[test]
    fn model_info_from_model_entry_provider_derivation_matches_reference(
        prefixed_name in arb_prefixed_name(),
    ) {
        let oracle = oracle_reference(reference_model_info_from_entry);

        let entry = ModelEntry {
            prefixed_name: prefixed_name.clone(),
            model: prefixed_name.clone(),
            supports_vision: false,
        };
        let input = json!({
            "prefixed_name": prefixed_name,
            "model": &entry.model,
            "supports_vision": false,
        });

        let info = ModelInfo::from(entry);
        let output = json!({
            "name": info.name,
            "provider": serde_json::to_value(&info.provider).expect("ProviderId serializes"),
            "family": info.family,
            "parameter_size": info.parameter_size,
            "quantization_level": info.quantization_level,
            "size_bytes": info.size_bytes,
        });

        prop_assert_eq!(
            oracle.verify(&input, &output),
            OracleVerdict::Pass,
            "provider derivation diverged from reference for input: {}",
            input
        );
    }

    /// `ModelInfo::from(ModelEntry)` is total (never panics), preserves the
    /// full prefixed name verbatim, nulls every optional field, and always
    /// yields a valid `ProviderId` — for arbitrary JSON-string-derived names
    /// (incl. empty, '/', control chars, unicode).
    ///
    /// Oracle: [`oracle_invariant`] — checks name-preservation + null-fields +
    /// valid-provider properties without re-implementing the parser.
    #[test]
    fn model_info_from_model_entry_preserves_name_and_nulls_on_arbitrary_json(
        prefixed_name in arb_json_value()
            .prop_filter("must be a JSON string", |v| v.is_string())
            .prop_map(|v| v.as_str().unwrap().to_string()),
    ) {
        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            let in_name = input["prefixed_name"].as_str().unwrap_or("");
            let out_name = output["name"].as_str().unwrap_or("");
            if in_name != out_name {
                return Err(format!("name not preserved: {in_name:?} != {out_name:?}"));
            }
            for key in ["family", "parameter_size", "quantization_level", "size_bytes"] {
                if !output[key].is_null() {
                    return Err(format!("{key} must be null, got {}", output[key]));
                }
            }
            let provider = output["provider"].as_str().unwrap_or("");
            if !ALL_PROVIDER_IDS.contains(&provider) {
                return Err(format!("invalid provider serde id: {provider}"));
            }
            Ok(())
        });

        let entry = ModelEntry {
            prefixed_name: prefixed_name.clone(),
            model: prefixed_name.clone(),
            supports_vision: false,
        };
        let input = json!({ "prefixed_name": prefixed_name });

        let info = ModelInfo::from(entry);
        let output = json!({
            "name": info.name,
            "provider": serde_json::to_value(&info.provider).expect("ProviderId serializes"),
            "family": info.family,
            "parameter_size": info.parameter_size,
            "quantization_level": info.quantization_level,
            "size_bytes": info.size_bytes,
        });

        prop_assert_eq!(
            oracle.verify(&input, &output),
            OracleVerdict::Pass,
            "invariant violated for input: {}",
            input
        );
    }

    /// `ModelInfo::from(RouterModelEntry)` copies the six carry-over fields
    /// (`prefixed_name`→`name`, `provider`, `family`, `parameter_size`,
    /// `quantization_level`, `size_bytes`) verbatim and drops `model` and
    /// `supports_vision`.
    ///
    /// Oracle: [`oracle_invariant`] — every carry-over field is preserved.
    #[test]
    fn model_info_from_router_entry_preserves_all_carryover_fields(
        prefixed_name in any::<String>(),
        provider in prop::sample::select(&ALL_PROVIDERS),
        model in any::<String>(),
        family in arb_opt_string_from_json(),
        parameter_size in arb_opt_string_from_json(),
        quantization_level in arb_opt_string_from_json(),
        size_bytes in prop::option::of(any::<u64>()),
        supports_vision in prop::option::of(any::<bool>()),
    ) {
        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            let in_name = input["prefixed_name"].as_str().unwrap_or("");
            let out_name = output["name"].as_str().unwrap_or("");
            if in_name != out_name {
                return Err(format!("name not preserved: {in_name:?} != {out_name:?}"));
            }
            let in_provider = &input["provider"];
            let out_provider = &output["provider"];
            if in_provider != out_provider {
                return Err(format!("provider not preserved: {in_provider} != {out_provider}"));
            }
            for key in ["family", "parameter_size", "quantization_level", "size_bytes"] {
                if input[key] != output[key] {
                    return Err(format!("{key} not preserved: {} != {}", input[key], output[key]));
                }
            }
            Ok(())
        });

        let entry = RouterModelEntry {
            prefixed_name,
            provider,
            model,
            family,
            parameter_size,
            quantization_level,
            size_bytes,
            supports_vision,
        };
        let input = json!({
            "prefixed_name": &entry.prefixed_name,
            "provider": serde_json::to_value(&entry.provider).expect("ProviderId serializes"),
            "model": &entry.model,
            "family": &entry.family,
            "parameter_size": &entry.parameter_size,
            "quantization_level": &entry.quantization_level,
            "size_bytes": entry.size_bytes,
            "supports_vision": entry.supports_vision,
        });

        let info = ModelInfo::from(entry);
        let output = json!({
            "name": info.name,
            "provider": serde_json::to_value(&info.provider).expect("ProviderId serializes"),
            "family": info.family,
            "parameter_size": info.parameter_size,
            "quantization_level": info.quantization_level,
            "size_bytes": info.size_bytes,
        });

        prop_assert_eq!(
            oracle.verify(&input, &output),
            OracleVerdict::Pass,
            "field-preservation invariant violated for input: {}",
            input
        );
    }
}
