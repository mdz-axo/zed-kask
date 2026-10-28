//! Serde tolerance for model-emission variance: models sometimes send
//! numeric tool-input fields as strings (`"300000"` instead of `300000`,
//! `"0.8"` instead of `0.8`). rmcp's macro layer rejects schema-invalid
//! arguments before the tool runs, so one string-encoded number fails the
//! whole call. These deserializers coerce the form (number or numeric
//! string) and leave value validation (ranges, bounds) to the tool — a
//! non-numeric string still fails visibly.
//!
//! Consolidated here 2026-09-27 from `crates/agent/src/tools.rs` (the
//! `timeout_ms` precedent) so built-in tools and MCP servers share one
//! family; the kata-kanban `prediction` field adopted the f64 variant
//! after the live goal-create incident (three rmcp rejections of `"0.8"`
//! as a string, 2026-09-27).

use serde::{Deserialize, Deserializer, de::Error as _};

/// Deserialize an `Option<u64>` that may have been provided as a numeric string
/// (e.g. `"300000"` instead of `300000`). Some models emit integers as strings;
/// we coerce rather than reject to avoid wasting a turn on a retry.
pub fn deserialize_optional_u64_from_maybe_string<'de, D>(
    deserializer: D,
) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum U64OrString {
        Number(u64),
        String(String),
    }

    match Option::<U64OrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(U64OrString::Number(n)) => Ok(Some(n)),
        Some(U64OrString::String(s)) => s
            .parse::<u64>()
            .map(Some)
            .map_err(|error| D::Error::custom(format!("failed to parse u64 from string: {error}"))),
    }
}

/// Deserialize an `Option<u32>` that may have been provided as a numeric string
/// (e.g. `"18"` instead of `18`). Some models emit line numbers as strings; we
/// coerce rather than reject to avoid wasting a turn on a retry and to keep
/// persisted tool calls replayable after a schema drift.
pub fn deserialize_optional_u32_from_maybe_string<'de, D>(
    deserializer: D,
) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum U32OrString {
        Number(u32),
        String(String),
    }

    match Option::<U32OrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(U32OrString::Number(n)) => Ok(Some(n)),
        Some(U32OrString::String(s)) => s
            .parse::<u32>()
            .map(Some)
            .map_err(|error| D::Error::custom(format!("failed to parse u32 from string: {error}"))),
    }
}

/// Deserialize an `Option<f64>` that may have been provided as a numeric
/// string (e.g. `"0.8"` instead of `0.8`). Some models emit probabilities as
/// strings; we coerce the form and leave range validation (0.0–1.0) to the
/// tool.
pub fn deserialize_optional_f64_from_maybe_string<'de, D>(
    deserializer: D,
) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum F64OrString {
        Number(f64),
        String(String),
    }

    match Option::<F64OrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(F64OrString::Number(n)) => Ok(Some(n)),
        Some(F64OrString::String(s)) => s
            .parse::<f64>()
            .map(Some)
            .map_err(|error| D::Error::custom(format!("failed to parse f64 from string: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u64_numeric_string_is_coerced() {
        let input = serde_json::json!("300000");
        let result: Option<u64> = deserialize_optional_u64_from_maybe_string(input).unwrap();
        assert_eq!(result, Some(300000));
    }

    #[test]
    fn u64_integer_passes_through_unchanged() {
        let input = serde_json::json!(180000);
        let result: Option<u64> = deserialize_optional_u64_from_maybe_string(input).unwrap();
        assert_eq!(result, Some(180000));
    }

    #[test]
    fn u64_null_yields_none() {
        let input = serde_json::json!(null);
        let result: Option<u64> = deserialize_optional_u64_from_maybe_string(input).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn u64_non_numeric_string_is_rejected() {
        let input = serde_json::json!("not a number");
        let result = deserialize_optional_u64_from_maybe_string::<serde_json::Value>(input);
        assert!(result.is_err());
    }

    #[test]
    fn u32_numeric_string_is_coerced() {
        let input = serde_json::json!("18");
        let result: Option<u32> = deserialize_optional_u32_from_maybe_string(input).unwrap();
        assert_eq!(result, Some(18));
    }

    #[test]
    fn u32_non_numeric_string_is_rejected() {
        let input = serde_json::json!("not a number");
        let result = deserialize_optional_u32_from_maybe_string::<serde_json::Value>(input);
        assert!(result.is_err());
    }

    /// The live goal-create incident (2026-09-27): the agent emitted the
    /// intake prediction as a string three times and rmcp's macro layer
    /// rejected the whole call each time. Form tolerance fixes the wire;
    /// the 0.0–1.0 range check stays in the kanban service.
    #[test]
    fn f64_numeric_string_is_coerced() {
        let input = serde_json::json!("0.8");
        let result: Option<f64> = deserialize_optional_f64_from_maybe_string(input).unwrap();
        assert_eq!(result, Some(0.8));
    }

    #[test]
    fn f64_number_passes_through_unchanged() {
        let input = serde_json::json!(0.75);
        let result: Option<f64> = deserialize_optional_f64_from_maybe_string(input).unwrap();
        assert_eq!(result, Some(0.75));
    }

    #[test]
    fn f64_null_yields_none() {
        let input = serde_json::json!(null);
        let result: Option<f64> = deserialize_optional_f64_from_maybe_string(input).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn f64_non_numeric_string_is_rejected() {
        let input = serde_json::json!("high");
        let result = deserialize_optional_f64_from_maybe_string::<serde_json::Value>(input);
        assert!(result.is_err());
    }
}
