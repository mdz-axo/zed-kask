//! Closed local tasks grounded in four repository defect mechanisms.
//!
//! These are development/calibration cases, not model observations or heldout
//! confirmation. The finite Rust grammar and integer-duration grammar deliberately
//! refuse unsupported reference inputs. JSON references share serde_json; duration
//! references share integer parsing/arithmetic. No policy reads hidden labels.

use super::{Case, Checker, Contract, MAX_TEXT_BYTES, bounded, parse_json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

type OraclePair = Result<(bool, bool), String>;
type Predicate = (&'static str, Option<bool>, &'static str);

// Independent of hkask-types' delimiter-depth extraction: deserialize one value
// from the first container byte and use the stream's consumed-byte boundary.
fn extraction_reference(input: &str) -> Result<Value, String> {
    bounded(input, MAX_TEXT_BYTES, "extraction input")?;
    let start = input.find(['[', '{']).ok_or("no source container")?;
    let mut stream = serde_json::Deserializer::from_str(&input[start..]).into_iter::<Value>();
    let value = stream
        .next()
        .ok_or("missing source value")?
        .map_err(|error| format!("unsupported/malformed source container: {error}"))?;
    if !value.is_array() && !value.is_object() {
        return Err("source must begin with a container".into());
    }
    Ok(value)
}

fn extracted(input: &str) -> Result<Value, String> {
    parse_json(&hkask_types::json_extract::extract_json_from_response(
        input,
    ))
}

fn compact(text: &str) -> String {
    let mut result = String::new();
    for token in text.split_whitespace() {
        if result.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            && token.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        {
            result.push(' ');
        }
        result.push_str(token);
    }
    result
}

// A depth-aware parser for the declared finite return-type grammar, not Rust.
// The reference below uses exact canonical alternatives instead of this parser.
fn error_slots(text: &str) -> Result<(String, String), String> {
    bounded(text, MAX_TEXT_BYTES, "return type")?;
    let text = compact(text);
    let inner = text
        .strip_prefix("Result<")
        .and_then(|s| s.strip_suffix('>'))
        .ok_or("expected Result<Ok, Error> return-type fragment")?;
    let mut depth = 0_u32;
    let mut separator = None;
    for (index, byte) in inner.bytes().enumerate() {
        match byte {
            b'(' => depth = depth.checked_add(1).ok_or("type nesting overflow")?,
            b')' => depth = depth.checked_sub(1).ok_or("unbalanced type")?,
            b',' if depth == 0 => {
                if separator.replace(index).is_some() {
                    return Err("multiple outer separators".into());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("unbalanced type".into());
    }
    let split = separator.ok_or("missing error slot")?;
    let ok = &inner[..split];
    let error = &inner[split + 1..];
    if !matches!(ok, "()" | "(String,String)") || !matches!(error, "String" | "TaskError") {
        return Err("unsupported return-type grammar".into());
    }
    Ok((ok.to_string(), error.to_string()))
}

fn rust_reference(input: &str) -> Result<&'static str, String> {
    match compact(input).as_str() {
        "Result<(),String>" => Ok("Result<(),TaskError>"),
        "Result<(String,String),String>" => Ok("Result<(String,String),TaskError>"),
        _ => Err("reference supports only unit/tuple Ok with String error".into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaTask {
    schema: Value,
    document: Value,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CoverageReport {
    status: CoverageStatus,
    unsupported: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CoverageStatus {
    Valid,
    Invalid,
    UnsupportedSchema,
}

impl<'de> Deserialize<'de> for CoverageStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "valid" => Ok(Self::Valid),
            "invalid" => Ok(Self::Invalid),
            "unsupported_schema" => Ok(Self::UnsupportedSchema),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["valid", "invalid", "unsupported_schema"],
            )),
        }
    }
}

fn schema_task(input: &str) -> Result<SchemaTask, String> {
    bounded(input, MAX_TEXT_BYTES, "schema task")?;
    let task: SchemaTask = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let schema = task.schema.as_object().ok_or("schema must be an object")?;
    if schema.get("type").and_then(Value::as_str) != Some("integer") {
        return Err("reference supports only explicit integer schemas".into());
    }
    if schema.get("const").is_some_and(|v| v.as_i64().is_none()) {
        return Err("reference const must be i64".into());
    }
    // Unknown keywords are inventoried, not evaluated. Never claim their
    // payload constraint passed, including unknown/nested keyword values.
    Ok(task)
}

fn coverage(task: &SchemaTask) -> CoverageReport {
    let mut unsupported = Vec::new();
    if let Some(object) = task.schema.as_object() {
        for key in object.keys() {
            if key != "type" && key != "const" {
                unsupported.push(key.clone());
            }
        }
    }
    unsupported.sort();
    let status = if !unsupported.is_empty() {
        CoverageStatus::UnsupportedSchema
    } else if task.document.as_i64().is_some()
        && task.schema.get("const").is_none_or(|v| v == &task.document)
    {
        CoverageStatus::Valid
    } else {
        CoverageStatus::Invalid
    };
    CoverageReport {
        status,
        unsupported,
    }
}

fn coverage_reference(task: &SchemaTask) -> CoverageReport {
    let mut keys: Vec<String> = task
        .schema
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .filter(|key| !["const", "type"].contains(&key.as_str()))
        .cloned()
        .collect();
    keys.sort();
    let integer = matches!(&task.document, Value::Number(n) if n.is_i64());
    let matches_const = match task.schema.get("const") {
        None => true,
        Some(v) => v == &task.document,
    };
    let status = match (keys.is_empty(), integer && matches_const) {
        (false, _) => CoverageStatus::UnsupportedSchema,
        (true, true) => CoverageStatus::Valid,
        (true, false) => CoverageStatus::Invalid,
    };
    CoverageReport {
        status,
        unsupported: keys,
    }
}

fn duration_parts(input: &str) -> Result<(u64, &str), String> {
    bounded(input, MAX_TEXT_BYTES, "duration token")?;
    let boundary = input
        .find(|c: char| !c.is_ascii_digit())
        .ok_or("missing duration suffix")?;
    let (number, unit) = input.split_at(boundary);
    let number = number
        .parse::<u64>()
        .map_err(|e| format!("unsigned integral duration required: {e}"))?;
    Ok((number, unit))
}

fn duration_units(input: &str) -> Result<u64, String> {
    let (number, unit) = duration_parts(input)?;
    let scale = match unit {
        "ms" => 1_000,
        "s" => 1_000_000,
        "m" => 60_000_000,
        "h" => 3_600_000_000,
        _ => return Err("unsupported duration unit".into()),
    };
    number
        .checked_mul(scale)
        .ok_or("duration microseconds overflow".into())
}

// Suffix-first decomposition does not use duration_parts or its boundary scan.
fn duration_reference(input: &str) -> Result<u64, String> {
    bounded(input, MAX_TEXT_BYTES, "duration token")?;
    for (suffix, scale) in [
        ("ms", 1_000_u128),
        ("s", 1_000_000),
        ("m", 60_000_000),
        ("h", 3_600_000_000),
    ] {
        if let Some(number) = input.strip_suffix(suffix) {
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return Err("unsupported reference duration number".into());
            }
            let number = number.parse::<u64>().map_err(|e| e.to_string())?;
            return u64::try_from(u128::from(number) * scale).map_err(|e| e.to_string());
        }
    }
    Err("unsupported reference duration suffix".into())
}

pub(super) fn oracle(case: &Case) -> Option<OraclePair> {
    let output = || parse_json(&case.artifact).ok();
    Some(match case.contract {
        Contract::ExtractedJson => (|| {
            let reference = extraction_reference(&case.input)?;
            let actual = extracted(&case.input)?;
            // A broken extractor is an instrument failure, not an invalid case.
            if actual != reference {
                return Err("source extraction/reference disagreement".into());
            }
            let artifact = output();
            Ok((
                artifact.as_ref() == Some(&actual),
                artifact.as_ref() == Some(&reference),
            ))
        })(),
        Contract::RustErrorSlot => (|| {
            let expected = rust_reference(&case.input)?;
            let (ok, error) = error_slots(&case.input)?;
            if error != "String" {
                return Err("source error must be String".into());
            }
            let actual =
                error_slots(&case.artifact).is_ok_and(|(a, e)| a == ok && e == "TaskError");
            Ok((actual, compact(&case.artifact) == expected))
        })(),
        Contract::SchemaCoverage => (|| {
            let task = schema_task(&case.input)?;
            let actual = coverage(&task);
            let reference = coverage_reference(&task);
            if actual != reference {
                return Err("schema reference disagreement".into());
            }
            let artifact = serde_json::from_str::<CoverageReport>(&case.artifact).ok();
            Ok((
                artifact.as_ref() == Some(&actual),
                artifact.as_ref() == Some(&reference),
            ))
        })(),
        Contract::DurationMicros => (|| {
            let actual = duration_units(&case.input)?;
            let reference = duration_reference(&case.input)?;
            if actual != reference {
                return Err("duration reference disagreement".into());
            }
            let artifact = output().and_then(|v| v.as_u64());
            Ok((artifact == Some(actual), artifact == Some(reference)))
        })(),
        _ => return None,
    })
}

pub(super) fn check(case: &Case, checker: Checker) -> Option<Predicate> {
    let output = || parse_json(&case.artifact).ok();
    Some(match checker {
        Checker::ExtractionBoundary => (
            "extraction_boundary",
            extracted(&case.input)
                .ok()
                .map(|v| output().as_ref() == Some(&v)),
            "Compare extracted first-container content; uses hkask-types extractor.",
        ),
        Checker::ExtractionArity => (
            "extraction_arity",
            extraction_reference(&case.input)
                .ok()
                .and_then(|v| v.as_array().map(|a| a.len()))
                .map(|len| output().and_then(|v| v.as_array().map(Vec::len)) == Some(len)),
            "Only array element count checked; contents unchecked.",
        ),
        Checker::JsonObjectOnly => (
            "json_object_only",
            Some(output().is_some_and(|v| v.is_object())),
            "Artifact parses as a JSON object.",
        ),
        Checker::RustErrorSlotTyped => (
            "rust_error_slot_typed",
            error_slots(&case.input)
                .ok()
                .map(|_| error_slots(&case.artifact).is_ok_and(|(_, e)| e == "TaskError")),
            "Restricted grammar error slot is TaskError; Ok preservation unchecked.",
        ),
        Checker::RustLineString => (
            "rust_line_string",
            Some(
                !case
                    .artifact
                    .lines()
                    .any(|line| line.contains("Result<") && line.contains("String")),
            ),
            "No artifact line contains both Result< and String.",
        ),
        Checker::RustTaskErrorToken => (
            "rust_task_error_token",
            Some(
                case.artifact
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .any(|token| token == "TaskError"),
            ),
            "TaskError token present; position and Ok preservation unchecked.",
        ),
        Checker::SchemaCoverageReport => (
            "schema_coverage_report",
            schema_task(&case.input).ok().map(|task| {
                serde_json::from_str::<CoverageReport>(&case.artifact)
                    .is_ok_and(|v| v == coverage(&task))
            }),
            "Closed primitive-schema report check; unsupported is not valid.",
        ),
        Checker::SchemaStatusValidOnly => (
            "schema_status_valid_only",
            Some(
                output()
                    .and_then(|v| v.get("status").and_then(Value::as_str).map(str::to_string))
                    .as_deref()
                    == Some("valid"),
            ),
            "Artifact has string status valid; other fields unchecked.",
        ),
        Checker::SchemaKeywordInventory => (
            "schema_keyword_inventory",
            schema_task(&case.input).ok().map(|task| {
                serde_json::from_str::<CoverageReport>(&case.artifact)
                    .is_ok_and(|v| v.unsupported == coverage_reference(&task).unsupported)
            }),
            "Only unsupported keyword inventory checked; reported status unchecked.",
        ),
        Checker::DurationUnits => (
            "duration_units",
            duration_units(&case.input)
                .ok()
                .map(|n| output().and_then(|v| v.as_u64()) == Some(n)),
            "Digit-boundary unit scaling to microseconds.",
        ),
        Checker::DurationRemainderScale => (
            "duration_remainder_scale",
            (|| {
                // Executable reproduction of the delimiter-consumption defect, not
                // an invocation of the repaired production parser.
                let boundary = case.input.find(|c: char| !c.is_ascii_digit())?;
                let delimiter = case.input[boundary..].chars().next()?;
                let (number, remaining) = case.input.split_once(delimiter)?;
                let scale = match remaining {
                    "s" | "" => 1_000_000_u64,
                    _ => return None,
                };
                let expected = number.parse::<u64>().ok()?.checked_mul(scale)?;
                Some(output().and_then(|v| v.as_u64()) == Some(expected))
            })(),
            "Compare numeric prefix with scale selected by the suffix remainder after splitting once at its first character.",
        ),
        Checker::DurationSuffixReference => (
            "duration_suffix_reference",
            duration_reference(&case.input)
                .ok()
                .map(|n| output().and_then(|v| v.as_u64()) == Some(n)),
            "Suffix-first checked microsecond conversion; shared integer kernel remains.",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_study::evidence::{CheckOutcome, CheckerState, Method};
    use serde_json::json;
    use std::{collections::BTreeSet, fs::OpenOptions, io::Write, path::Path};

    fn cases() -> Vec<Case> {
        let families = [
            (
                "container-boundary",
                Contract::ExtractedJson,
                "Extract the first complete JSON container from the input response, preserving decoded content and every array element; output only that JSON container.",
                "Here are the tags:\n```json\n[{\"a\":1},{\"b\":2}]\n```",
                "[{\"a\":1},{\"b\":2}]",
                "{\"a\":1}",
                Checker::ExtractionBoundary,
                Checker::JsonObjectOnly,
                Checker::JsonParses,
                Checker::ExtractionArity,
            ),
            (
                "generic-error-slot",
                Contract::RustErrorSlot,
                "Replace the outer Result error type String with TaskError, preserving the Ok type. Return only a Rust return-type fragment. Supported Ok types: () and (String,String); whitespace is immaterial.",
                "Result<(String, String), String>",
                "Result<(String, String), TaskError>",
                "Result<\n (String, String),\n String\n>",
                Checker::RustErrorSlotTyped,
                Checker::RustLineString,
                Checker::RustLineString,
                Checker::RustTaskErrorToken,
            ),
            (
                "schema-coverage",
                Contract::SchemaCoverage,
                "Report validation coverage as exactly {status,unsupported}. This primitive validator supports explicit integer type and i64 const only. Any other schema keyword requires status unsupported_schema and its sorted key inventory, even if the payload looks conforming. With full coverage use valid/invalid. Never call an unchecked schema valid.",
                "{\"schema\":{\"type\":\"integer\",\"minimum\":0},\"document\":-1}",
                "{\"status\":\"unsupported_schema\",\"unsupported\":[\"minimum\"]}",
                "{\"status\":\"valid\",\"unsupported\":[]}",
                Checker::SchemaCoverageReport,
                Checker::SchemaStatusValidOnly,
                Checker::SchemaStatusValidOnly,
                Checker::SchemaKeywordInventory,
            ),
            (
                "duration-suffix",
                Contract::DurationMicros,
                "Convert the unsigned integral single-token duration to an exact JSON u64 count of microseconds. Units ms,s,m,h; checked overflow; no fractions/signs/whitespace or other units. 1ms=1000us, 1s=1000000us, 1m=60s, 1h=60m.",
                "500ms",
                "500000",
                "500000000",
                Checker::DurationUnits,
                Checker::DurationRemainderScale,
                Checker::DurationRemainderScale,
                Checker::DurationSuffixReference,
            ),
        ];
        let mut cases = Vec::new();
        for (
            family,
            contract,
            requirement,
            input,
            good,
            bad,
            accurate,
            false_reject,
            false_pass,
            secondary,
        ) in families
        {
            for (truth, artifact) in [(true, good), (false, bad)] {
                for state in [
                    CheckerState::Accurate,
                    CheckerState::Misleading,
                    CheckerState::Inconclusive,
                ] {
                    let primary = match state {
                        CheckerState::Accurate => accurate,
                        CheckerState::Misleading if truth => false_reject,
                        CheckerState::Misleading => false_pass,
                        CheckerState::Inconclusive => Checker::NoResult,
                    };
                    cases.push(Case {
                        id: format!("e{:02}", cases.len() + 1),
                        family: family.into(),
                        requirement: requirement.into(),
                        input: input.into(),
                        artifact: artifact.into(),
                        contract,
                        primary,
                        secondary,
                        expected_checker_state: state,
                    });
                }
            }
        }
        cases
    }

    #[test]
    fn full_factorial_runs_actual_oracles_and_policies() {
        let cases = cases();
        assert_eq!(cases.len(), 24);
        crate::packet_study::validate_cases(&cases).expect("real runner accepts full case set");
        let mut cells = BTreeSet::new();
        for (index, case) in cases.iter().enumerate() {
            let truth = index % 6 < 3;
            assert_eq!(
                case.oracle().expect("determinate reference"),
                truth,
                "{}",
                case.id
            );
            case.validate()
                .expect("actual checker state must match cell");
            assert!(cells.insert((
                case.family.clone(),
                truth,
                format!("{:?}", case.expected_checker_state)
            )));
            let primary = case.check(case.primary, case.receipt_id(1));
            let secondary = case.check(case.secondary, case.receipt_id(3));
            for receipt in [&primary, &secondary] {
                for cue in [
                    "overrestrictive",
                    "legacy",
                    "misleading",
                    "accurate",
                    "false",
                    "defect",
                ] {
                    assert!(
                        !receipt.detail.to_ascii_lowercase().contains(cue),
                        "health cue {cue} in {}",
                        case.id
                    );
                }
            }
            assert_eq!(
                secondary.outcome == CheckOutcome::Pass,
                truth,
                "curated secondary challenge {}",
                case.id
            );
            assert_eq!(
                primary.artifact_sha256,
                crate::packet_study::digest(case.artifact.as_bytes())
            );
            for method in [
                None,
                Some(Method::SelfCritique),
                Some(Method::Repeat),
                Some(Method::Secondary),
            ] {
                let packet = case.visible_packet(method).expect("visible packet");
                let object = packet.as_object().expect("packet object");
                assert_eq!(object.len(), 5);
                for hidden in [
                    "family",
                    "contract",
                    "expected_checker_state",
                    "truth",
                    "secondary",
                ] {
                    assert!(!object.contains_key(hidden));
                }
                assert_eq!(
                    object["receipts"].as_array().expect("receipts").len(),
                    if matches!(method, Some(Method::Repeat | Method::Secondary)) {
                        2
                    } else {
                        1
                    }
                );
            }
            let mut relabeled = case.clone();
            relabeled.expected_checker_state = CheckerState::Inconclusive;
            assert_eq!(
                relabeled.check(case.primary, case.receipt_id(1)),
                primary,
                "policy must not consult hidden state"
            );
            relabeled.contract = Contract::IntegerSum;
            assert_eq!(
                relabeled.check(case.secondary, case.receipt_id(3)),
                secondary,
                "policy must not consult contract"
            );
        }
        assert_eq!(cells.len(), 24);
        assert_eq!(
            super::super::instrument_cases().len(),
            9,
            "engineering controls preserved separately"
        );
    }

    #[test]
    fn legitimate_and_attack_boundaries_fail_closed() {
        let mut case = cases().remove(0);
        for (input, good, bad) in [
            (
                "```json\n[{\"s\":\"]}\\\"\"},{\"b\":2}]\n```",
                "[{\"s\":\"]}\\\"\"},{\"b\":2}]",
                "{\"b\":2}",
            ),
            (
                "prefix {\"a\":[1,2]} tail {\"evil\":true}",
                "{\"a\":[1,2]}",
                "{\"evil\":true}",
            ),
            ("prefix [] tail", "[]", "{}"),
        ] {
            case.input = input.into();
            case.artifact = good.into();
            assert_eq!(case.oracle(), Ok(true));
            case.artifact = bad.into();
            assert_eq!(case.oracle(), Ok(false));
        }
        for input in ["no container", "[}", "[{\"a\":1}", "[1,]"] {
            case.input = input.into();
            assert!(case.oracle().is_err(), "{input}");
        }
        case = cases().remove(6);
        for (input, good, bad) in [
            (
                "Result<(),String>",
                "Result<\n (),\n TaskError\n>",
                "Result<(String,String),TaskError>",
            ),
            (
                "Result<(String,String),String>",
                "Result<(String,String),TaskError>",
                "Result<(),TaskError>",
            ),
        ] {
            case.input = input.into();
            case.artifact = good.into();
            assert_eq!(case.oracle(), Ok(true));
            case.artifact = bad.into();
            assert_eq!(case.oracle(), Ok(false));
        }
        for artifact in [
            "Result<(),String>",
            "// TaskError",
            "Result<(),TaskError> trailing",
            "Result<(),TaskError>>",
            "Result<()),TaskError>",
            "Result<(Str ing,String),TaskError>",
            "Result<(String,String),Task Error>",
        ] {
            case.artifact = artifact.into();
            assert_eq!(case.oracle(), Ok(false));
        }
        case.input = "Result<Vec<u8>,String>".into();
        assert!(case.oracle().is_err());
        case = cases().remove(12);
        for (input, status, keys) in [
            (
                "{\"schema\":{\"type\":\"integer\"},\"document\":2}",
                "valid",
                json!([]),
            ),
            (
                "{\"schema\":{\"type\":\"integer\",\"const\":3},\"document\":2}",
                "invalid",
                json!([]),
            ),
            (
                "{\"schema\":{\"type\":\"integer\"},\"document\":\"2\"}",
                "invalid",
                json!([]),
            ),
            (
                "{\"schema\":{\"type\":\"integer\",\"properties\":{\"x\":{\"unknown\":true}},\"minimum\":0},\"document\":-1}",
                "unsupported_schema",
                json!(["minimum", "properties"]),
            ),
        ] {
            case.input = input.into();
            case.artifact = json!({"status":status,"unsupported":keys}).to_string();
            assert_eq!(case.oracle(), Ok(true));
            case.artifact = "{\"status\":\"ready\",\"unsupported\":[]}".into();
            assert_eq!(case.oracle(), Ok(false));
        }
        for artifact in [
            "{\"status\":{\"valid\":null},\"unsupported\":[]}",
            "{\"status\":\"valid\",\"unsupported\":[],\"extra\":true}",
            "{\"status\":\"valid\",\"status\":\"invalid\",\"unsupported\":[]}",
        ] {
            case.artifact = artifact.into();
            assert_eq!(case.oracle(), Ok(false));
        }
        for input in [
            "{\"schema\":true,\"document\":1}",
            "{\"schema\":{\"type\":\"unknown\"},\"document\":1}",
            "{\"schema\":{\"type\":\"integer\",\"const\":\"x\"},\"document\":1}",
        ] {
            case.input = input.into();
            assert!(case.oracle().is_err());
        }
        case = cases().remove(18);
        for (input, expected) in [
            ("0ms", 0_u64),
            ("500ms", 500_000),
            ("2s", 2_000_000),
            ("3m", 180_000_000),
            ("1h", 3_600_000_000),
        ] {
            case.input = input.into();
            case.artifact = expected.to_string();
            assert_eq!(case.oracle(), Ok(true));
            case.artifact = "-1".into();
            assert_eq!(case.oracle(), Ok(false));
        }
        for input in [
            "-1ms",
            "+1s",
            "1.5s",
            "NaNms",
            "1us",
            "500",
            "500ms junk",
            " 500ms",
            "18446744073709551615h",
        ] {
            case.input = input.into();
            assert!(case.oracle().is_err(), "{input}");
        }
    }

    #[test]
    fn strict_case_codec_roundtrip_and_mismatch_rejection() {
        let original = cases();
        let bytes = serde_json::to_vec(&original).expect("serialize cases");
        let parsed: Vec<Case> = serde_json::from_slice(&bytes).expect("CLI-compatible case codec");
        assert_eq!(parsed.len(), 24);
        for mut case in parsed {
            case.validate().expect("roundtrip valid");
            case.expected_checker_state = match case.expected_checker_state {
                CheckerState::Accurate => CheckerState::Misleading,
                _ => CheckerState::Accurate,
            };
            assert!(case.validate().is_err(), "mislabeled state rejected");
        }
        let mut duplicate = original.clone();
        duplicate[1].id = duplicate[0].id.clone();
        assert!(crate::packet_study::validate_cases(&duplicate).is_err());
        let mut value = serde_json::to_value(&original[0]).expect("case");
        value["science_metadata"] = json!({"truth":true});
        assert!(
            serde_json::from_value::<Case>(value).is_err(),
            "metadata belongs in digest-bound sidecar"
        );
    }

    fn create_json(
        root: &Path,
        name: &str,
        value: &impl Serialize,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = serde_json::to_vec_pretty(value)?;
        let digest = crate::packet_study::digest(&bytes);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(name))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(digest)
    }

    #[test]
    fn package_files_refuse_overwrite() {
        let root = tempfile::tempdir().expect("private temporary directory");
        let first = json!({"receipt":"original"});
        create_json(root.path(), "receipt.json", &first).expect("first immutable write");
        assert!(
            create_json(
                root.path(),
                "receipt.json",
                &json!({"receipt":"replacement"})
            )
            .is_err()
        );
        let retained: Value = serde_json::from_slice(
            &std::fs::read(root.path().join("receipt.json")).expect("read retained receipt"),
        )
        .expect("retained JSON");
        assert_eq!(retained, first);
    }

    /// Opt-in deterministic exporter. Never contacts a model; create_new refuses
    /// overwrites. The caller owns a fresh output directory and source hash seal.
    #[test]
    #[ignore = "set STUDY_A_CALIBRATION_OUTPUT to a fresh existing directory"]
    fn export_calibrated_package() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::var("STUDY_A_CALIBRATION_OUTPUT")?;
        let root = Path::new(&root);
        if !root.is_dir() {
            return Err("output must be a fresh existing directory".into());
        }
        let cases = cases();
        let mut receipts = Vec::new();
        let mut metadata = Vec::new();
        let mut packets = Vec::new();
        crate::packet_study::validate_cases(&cases)?;
        for (index, case) in cases.iter().enumerate() {
            case.validate()?;
            let expected_truth = index % 6 < 3;
            let (actual, reference) = oracle(case).ok_or("task dispatch absent")??;
            if actual != expected_truth || reference != expected_truth {
                return Err("curated reference mismatch".into());
            }
            let primary = case.check(case.primary, case.receipt_id(1));
            let secondary = case.check(case.secondary, case.receipt_id(3));
            if (secondary.outcome == CheckOutcome::Pass) != expected_truth {
                return Err("secondary calibration mismatch".into());
            }
            receipts.push(json!({"id":case.id,"execution_provenance":"local_deterministic_case_calibration","artifact_sha256":crate::packet_study::digest(case.artifact.as_bytes()),"input_sha256":crate::packet_study::digest(case.input.as_bytes()),"curated_truth":expected_truth,"contract":actual,"reference":reference,"primary":primary,"secondary":secondary,"observed_checker_state":case.expected_checker_state}));
            let packet = case.visible_packet(Some(Method::Secondary))?;
            packets.push(json!({"id":case.id,"initial":case.visible_packet(None)?,"self_critique":case.visible_packet(Some(Method::SelfCritique))?,"repeat":case.visible_packet(Some(Method::Repeat))?,"secondary":packet}));
            metadata.push(json!({"id":case.id,"family":case.family,"truth":expected_truth,"checker_state":case.expected_checker_state,"case_sha256":crate::packet_study::digest(&serde_json::to_vec(case)?),"secondary_packet_sha256":crate::packet_study::digest(&serde_json::to_vec(&packet)?),"independent_unit":case.family,"role":"development_calibration_never_heldout_confirmation"}));
        }
        let cases_hash = create_json(root, "cases.json", &cases)?;
        let receipts_hash = create_json(root, "calibration-receipts.json", &receipts)?;
        let packets_hash = create_json(root, "evidence-packets.json", &packets)?;
        create_json(
            root,
            "science-sidecar.json",
            &json!({"version":"study-a-science-sidecar-v1.2.2","cases_sha256":cases_hash,"receipts_sha256":receipts_hash,"packets_sha256":packets_hash,"episodes":metadata,"family_count":4,"episode_count":24,"model_observations":0,"future_generation_counts":{"initial":24,"review_and_final":144,"cap":2,"total":170},"independence_limit":"Four distinct defect roots; six dependent episodes per root, common author and shared serde/std kernels; no statistical independence or scientific acceptance asserted.","status":"local_cases_calibrated_not_scientific_dispatch_authorization"}),
        )?;
        println!(
            "calibrated: 24 episodes / 4 mechanism roots / 24 oracle-reference-curated agreements / 48 executed checker receipts / 0 model observations"
        );
        Ok(())
    }
}
