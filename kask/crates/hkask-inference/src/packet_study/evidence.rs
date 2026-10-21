//! Offline smoke/instrument controls, not a pilot dataset or a model judge.
//!
//! Contract implementations and reference checks share serde_json's trusted parsing
//! kernel. Their agreement catches implementation mistakes, not shared parser bugs
//! or scientific validity. Weak checker policies never consult contract truth.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_ID_BYTES: usize = 64;
const MAX_TEXT_BYTES: usize = 65_536;
const MAX_REQUIREMENT_BYTES: usize = 8_192;
const MAX_REASON_BYTES: usize = 4_096;
const MAX_JUDGMENT_BYTES: usize = 16_384;
const MAX_REFS: usize = 8;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub(super) id: String,
    pub(super) family: String,
    pub(super) requirement: String,
    pub(super) input: String,
    pub(super) artifact: String,
    pub(super) contract: Contract,
    pub(super) primary: Checker,
    pub(super) secondary: Checker,
    pub(super) expected_checker_state: CheckerState,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Contract {
    JsonPreserved,
    IntegerSum,
    SupportedStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Checker {
    JsonParses,
    JsonArrayLength,
    IntegerNonnegative,
    IntegerParity,
    StatusFieldPresent,
    StatusNotUnsupported,
    NoResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CheckerState {
    Accurate,
    Misleading,
    Inconclusive,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CheckOutcome {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Method {
    SelfCritique,
    Repeat,
    Secondary,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub(super) id: String,
    pub(super) artifact_sha256: String,
    pub(super) checker: String,
    pub(super) outcome: CheckOutcome,
    pub(super) detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Judgment {
    pub(super) verdict: Verdict,
    pub(super) evidence_refs: Vec<String>,
    pub(super) reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Verdict {
    Complete,
    Incomplete,
    InsufficientEvidence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusInput {
    supported: bool,
    required: Vec<String>,
}

fn bounded(text: &str, limit: usize, label: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > limit {
        return Err(format!(
            "{label} must be nonempty and at most {limit} bytes"
        ));
    }
    Ok(())
}

fn identifier(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > MAX_ID_BYTES
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("id must be 1..=64 ASCII letters, digits, underscores or hyphens".into());
    }
    Ok(())
}

fn parse_json(text: &str) -> Result<Value, String> {
    bounded(text, MAX_TEXT_BYTES, "JSON")?;
    serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))
}

fn integers(input: &Value) -> Result<Vec<i64>, String> {
    input
        .as_array()
        .ok_or_else(|| "integer_sum input must be an array".to_string())?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "input elements must be i64 integers".into())
        })
        .collect()
}

fn status_input(input: &str) -> Result<StatusInput, String> {
    // Deserialize the source, not a Value clone: typed serde rejects duplicate
    // fields before Value's object map can collapse them.
    let spec: StatusInput = serde_json::from_str(input)
        .map_err(|error| format!("invalid supported_status input: {error}"))?;
    let mut seen = HashSet::new();
    for key in &spec.required {
        bounded(key, MAX_REQUIREMENT_BYTES, "required key")?;
        if !seen.insert(key) {
            return Err("duplicate required key".into());
        }
    }
    Ok(spec)
}

// Independent traversal from Value's equality implementation; both use the same
// parsed representation (including its number semantics), not independent parsers.
fn reference_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| reference_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key)
                        .is_some_and(|other| reference_equal(value, other))
                })
        }
        _ => left == right,
    }
}

impl Case {
    fn validate_fields(&self) -> Result<(), String> {
        identifier(&self.id)?;
        bounded(&self.family, 128, "family")?;
        bounded(&self.requirement, MAX_REQUIREMENT_BYTES, "requirement")?;
        bounded(&self.input, MAX_TEXT_BYTES, "input")?;
        self.validate_artifact_size()?;
        if self.primary == self.secondary {
            return Err("primary and secondary must be different checker policies".into());
        }
        Ok(())
    }

    fn validate_artifact_size(&self) -> Result<(), String> {
        if self.artifact.len() > MAX_TEXT_BYTES {
            return Err(format!("artifact must be at most {MAX_TEXT_BYTES} bytes"));
        }
        Ok(())
    }

    /// Invalid/overflowing reference inputs are indeterminate (Err), whereas an
    /// invalid artifact is a determinate contract failure (Ok(false)). JSON
    /// preservation accepts only object/array inputs, compares decoded values,
    /// ignores object key order, and retains serde_json's number semantics.
    pub(super) fn oracle(&self) -> Result<bool, String> {
        let input = parse_json(&self.input)?;
        // An artifact need not be valid JSON to be a known contract violation.
        self.validate_artifact_size()?;
        let artifact = parse_json(&self.artifact).ok();
        let (actual, reference) = match self.contract {
            Contract::JsonPreserved => {
                if !input.is_array() && !input.is_object() {
                    return Err("json_preserved input must be a JSON container".into());
                }
                (
                    artifact.as_ref().is_some_and(|output| *output == input),
                    artifact
                        .as_ref()
                        .is_some_and(|output| reference_equal(&input, output)),
                )
            }
            Contract::IntegerSum => {
                let numbers = integers(&input)?;
                let sum = numbers.iter().try_fold(0_i64, |sum, number| {
                    sum.checked_add(*number)
                        .ok_or_else(|| "i64 sum overflow".to_string())
                })?;
                let reference_sum = numbers.iter().try_fold(0_i128, |sum, number| {
                    sum.checked_add(i128::from(*number))
                        .ok_or_else(|| "reference sum overflow".to_string())
                })?;
                let output = artifact.as_ref().and_then(Value::as_i64);
                (
                    output == Some(sum),
                    output.map(i128::from) == Some(reference_sum),
                )
            }
            Contract::SupportedStatus => {
                let spec = status_input(&self.input)?;
                // The contract constrains readiness, not a closed status vocabulary:
                // a non-ready string does not assert readiness and is admissible.
                let output = artifact.as_ref().and_then(Value::as_object);
                let status = output
                    .and_then(|object| object.get("status"))
                    .and_then(Value::as_str);
                let actual = match (output, status) {
                    (Some(object), Some("ready")) => {
                        spec.supported && spec.required.iter().all(|key| object.contains_key(key))
                    }
                    (Some(_), Some(_)) => true,
                    _ => false,
                };
                let reference = match artifact.as_ref() {
                    Some(Value::Object(object)) => match object.get("status") {
                        Some(Value::String(status)) if status == "ready" => {
                            let missing = spec
                                .required
                                .iter()
                                .filter(|key| !object.contains_key(*key))
                                .count();
                            spec.supported && missing == 0
                        }
                        Some(Value::String(_)) => true,
                        _ => false,
                    },
                    _ => false,
                };
                (actual, reference)
            }
        };
        if actual != reference {
            return Err("contract and reference disagree".into());
        }
        Ok(actual)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        self.validate_fields()?;
        let truth = self.oracle()?;
        let receipt = self.check(self.primary, self.receipt_id(1));
        let state = match receipt.outcome {
            CheckOutcome::Inconclusive => CheckerState::Inconclusive,
            outcome if (outcome == CheckOutcome::Pass) == truth => CheckerState::Accurate,
            _ => CheckerState::Misleading,
        };
        if state != self.expected_checker_state {
            return Err(format!(
                "expected_checker_state mismatch: observed {state:?}"
            ));
        }
        Ok(())
    }

    fn receipt_id(&self, ordinal: u8) -> String {
        format!("{}:r{ordinal}", self.id)
    }

    pub(super) fn allowed_receipt_ids(&self, method: Option<Method>) -> Vec<String> {
        let mut ids = vec![self.receipt_id(1)];
        match method {
            Some(Method::Repeat) => ids.push(self.receipt_id(2)),
            Some(Method::Secondary) => ids.push(self.receipt_id(3)),
            None | Some(Method::SelfCritique) => {}
        }
        ids
    }

    /// Only an explicit allowlist is projected. Do not serialize Case into a
    /// request: its metadata includes the oracle and checker-health labels.
    pub(super) fn visible_packet(&self, method: Option<Method>) -> Result<Value, String> {
        self.validate()?;
        let mut receipts = vec![self.check(self.primary, self.receipt_id(1))];
        match method {
            Some(Method::Repeat) => receipts.push(self.check(self.primary, self.receipt_id(2))),
            Some(Method::Secondary) => {
                receipts.push(self.check(self.secondary, self.receipt_id(3)))
            }
            None | Some(Method::SelfCritique) => {}
        }
        Ok(json!({
            "id": self.id,
            "requirement": self.requirement,
            "input": self.input,
            "artifact": self.artifact,
            "receipts": receipts,
        }))
    }

    /// Executes only the chosen partial predicate. Neither the oracle nor
    /// expected_checker_state may influence the resulting evidence.
    fn check(&self, checker: Checker, id: String) -> Receipt {
        let output = parse_json(&self.artifact);
        let (name, result, detail) = match checker {
            Checker::JsonParses => (
                "json_parses",
                Some(output.is_ok()),
                "Artifact parses as JSON.",
            ),
            Checker::JsonArrayLength => {
                let result = match parse_json(&self.input) {
                    Ok(Value::Array(input)) => Some(matches!(
                        &output, Ok(Value::Array(artifact)) if artifact.len() == input.len()
                    )),
                    _ => None,
                };
                (
                    "json_array_length",
                    result,
                    "Input and artifact are arrays of equal length.",
                )
            }
            Checker::IntegerNonnegative => (
                "integer_nonnegative",
                Some(
                    output
                        .as_ref()
                        .ok()
                        .and_then(Value::as_i64)
                        .is_some_and(|number| number >= 0),
                ),
                "Artifact is a nonnegative i64 integer.",
            ),
            Checker::IntegerParity => {
                let result = match parse_json(&self.input).and_then(|value| integers(&value)) {
                    Ok(numbers) => {
                        // Modular addition does not compute the full contract's sum.
                        let parity = numbers
                            .iter()
                            .fold(0, |parity, number| parity ^ number.rem_euclid(2));
                        Some(
                            output
                                .as_ref()
                                .ok()
                                .and_then(Value::as_i64)
                                .is_some_and(|number| number.rem_euclid(2) == parity),
                        )
                    }
                    Err(_) => None,
                };
                (
                    "integer_parity",
                    result,
                    "Artifact integer parity matches input sum parity.",
                )
            }
            Checker::StatusFieldPresent => (
                "status_field_present",
                Some(
                    output
                        .as_ref()
                        .ok()
                        .and_then(Value::as_object)
                        .is_some_and(|object| object.contains_key("status")),
                ),
                "Artifact object contains a status field (type and value unchecked).",
            ),
            Checker::StatusNotUnsupported => (
                "status_not_unsupported",
                Some(
                    output
                        .as_ref()
                        .ok()
                        .and_then(|value| value.get("status"))
                        .and_then(Value::as_str)
                        .is_some_and(|status| status != "unsupported"),
                ),
                "Artifact has a string status other than unsupported.",
            ),
            Checker::NoResult => ("no_result", None, "Checker policy supplies no result."),
        };
        let outcome = match result {
            Some(true) => CheckOutcome::Pass,
            Some(false) => CheckOutcome::Fail,
            None => CheckOutcome::Inconclusive,
        };
        Receipt {
            id,
            artifact_sha256: super::digest(self.artifact.as_bytes()),
            checker: name.to_string(),
            outcome,
            detail: if result.is_none() && checker != Checker::NoResult {
                format!(
                    "Predicate unavailable: input does not satisfy checker prerequisites. {detail}"
                )
            } else {
                detail.to_string()
            },
        }
    }
}

/// Parse exactly one JSON judgment. Syntax/shape/reference failures are errors,
/// never an insufficient_evidence verdict; no markdown or substring salvage.
pub(super) fn parse_judgment(text: &str, allowed: &[String]) -> Result<Judgment, String> {
    bounded(text, MAX_JUDGMENT_BYTES, "judgment")?;
    let judgment: Judgment =
        serde_json::from_str(text).map_err(|error| format!("invalid judgment JSON: {error}"))?;
    bounded(&judgment.reason, MAX_REASON_BYTES, "reason")?;
    if judgment.evidence_refs.len() > MAX_REFS {
        return Err(format!(
            "at most {MAX_REFS} evidence references are allowed"
        ));
    }
    let mut seen = HashSet::new();
    for reference in &judgment.evidence_refs {
        bounded(reference, MAX_ID_BYTES + 3, "evidence reference")?;
        if !allowed.contains(reference) {
            return Err(format!("foreign evidence reference: {reference}"));
        }
        if !seen.insert(reference) {
            return Err(format!("duplicate evidence reference: {reference}"));
        }
    }
    Ok(judgment)
}

/// Small deterministic smoke/instrument controls ONLY. Opaque IDs do not encode
/// hidden truth. These are not a pilot sample and support no scientific estimate.
pub(super) fn instrument_cases() -> Vec<Case> {
    use Checker::{
        IntegerNonnegative, IntegerParity, JsonArrayLength, JsonParses, NoResult,
        StatusFieldPresent, StatusNotUnsupported,
    };
    use CheckerState::{Accurate, Inconclusive, Misleading};
    use Contract::{IntegerSum, JsonPreserved, SupportedStatus};

    let controls = [
        (
            "c01",
            "container",
            "Preserve this JSON container exactly in decoded content.",
            "[1,2]",
            "[1,2]",
            JsonPreserved,
            JsonParses,
            JsonArrayLength,
            Accurate,
        ),
        (
            "c02",
            "container",
            "Preserve this JSON container exactly in decoded content.",
            "[1,2]",
            "[9,2]",
            JsonPreserved,
            JsonArrayLength,
            JsonParses,
            Misleading,
        ),
        (
            "c03",
            "container",
            "Preserve this JSON container exactly in decoded content.",
            "[1,2]",
            "not JSON",
            JsonPreserved,
            JsonParses,
            JsonArrayLength,
            Accurate,
        ),
        (
            "c04",
            "sum",
            "Return a JSON i64 integer equal to the exact checked sum of the input integers.",
            "[2,3]",
            "5",
            IntegerSum,
            IntegerParity,
            IntegerNonnegative,
            Accurate,
        ),
        (
            "c05",
            "sum",
            "Return a JSON i64 integer equal to the exact checked sum of the input integers.",
            "[2,3]",
            "7",
            IntegerSum,
            IntegerParity,
            IntegerNonnegative,
            Misleading,
        ),
        (
            "c06",
            "sum",
            "Return a JSON i64 integer equal to the exact checked sum of the input integers.",
            "[-3,1]",
            "-2",
            IntegerSum,
            IntegerNonnegative,
            IntegerParity,
            Misleading,
        ),
        (
            "c07",
            "status",
            "Return a JSON object with a string status; ready is allowed only when supported is true and every required key is present.",
            "{\"supported\":true,\"required\":[\"result\"]}",
            "{\"status\":\"ready\",\"result\":1}",
            SupportedStatus,
            StatusFieldPresent,
            StatusNotUnsupported,
            Accurate,
        ),
        (
            "c08",
            "status",
            "Return a JSON object with a string status; ready is allowed only when supported is true and every required key is present.",
            "{\"supported\":false,\"required\":[\"result\"]}",
            "{\"status\":\"ready\",\"result\":1}",
            SupportedStatus,
            StatusNotUnsupported,
            StatusFieldPresent,
            Misleading,
        ),
        (
            "c09",
            "status",
            "Return a JSON object with a string status; ready is allowed only when supported is true and every required key is present.",
            "{\"supported\":true,\"required\":[\"result\"]}",
            "{\"status\":\"ready\"}",
            SupportedStatus,
            NoResult,
            StatusFieldPresent,
            Inconclusive,
        ),
    ];
    controls
        .into_iter()
        .map(
            |(
                id,
                family,
                requirement,
                input,
                artifact,
                contract,
                primary,
                secondary,
                expected_checker_state,
            )| Case {
                id: id.into(),
                family: family.into(),
                requirement: requirement.into(),
                input: input.into(),
                artifact: artifact.into(),
                contract,
                primary,
                secondary,
                expected_checker_state,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(contract: Contract, input: &str, artifact: &str) -> Case {
        Case {
            id: "t01".into(),
            family: "control".into(),
            requirement: "Apply the supplied contract.".into(),
            input: input.into(),
            artifact: artifact.into(),
            contract,
            primary: Checker::JsonParses,
            secondary: Checker::NoResult,
            expected_checker_state: CheckerState::Accurate,
        }
    }

    #[test]
    fn full_contract_oracles_match_known_answers() {
        use Contract::*;
        let examples = [
            (
                JsonPreserved,
                "{\"a\":[1,true],\"b\":null}",
                "{\"b\":null,\"a\":[1,true]}",
                true,
            ),
            (JsonPreserved, "[]", "[]", true),
            (JsonPreserved, "[1,2]", "[2,1]", false),
            (JsonPreserved, "{\"a\":1}", "{\"a\":1,\"b\":2}", false),
            (JsonPreserved, "[1]", "[2]", false),
            (JsonPreserved, "[]", "broken", false),
            (IntegerSum, "[]", "0", true),
            (IntegerSum, "[-3,1]", "-2", true),
            (IntegerSum, "[2,3]", "5", true),
            (IntegerSum, "[2,3]", "7", false),
            (IntegerSum, "[2,3]", "5.0", false),
            (IntegerSum, "[2,3]", "\"5\"", false),
            (
                IntegerSum,
                "[9223372036854775807]",
                "9223372036854775807",
                true,
            ),
            (
                IntegerSum,
                "[-9223372036854775808]",
                "-9223372036854775808",
                true,
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[\"x\"]}",
                "{\"status\":\"ready\",\"x\":null}",
                true,
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[\"x\"]}",
                "{\"status\":\"ready\"}",
                false,
            ),
            (
                SupportedStatus,
                "{\"supported\":false,\"required\":[]}",
                "{\"status\":\"ready\"}",
                false,
            ),
            (
                SupportedStatus,
                "{\"supported\":false,\"required\":[\"x\"]}",
                "{\"status\":\"unsupported\"}",
                true,
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[]}",
                "{\"status\":\"pending\"}",
                true,
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[]}",
                "{\"status\":null}",
                false,
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[]}",
                "{}",
                false,
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[]}",
                "[]",
                false,
            ),
        ];
        for (contract, input, artifact, expected) in examples {
            assert_eq!(
                case(contract, input, artifact).oracle(),
                Ok(expected),
                "{contract:?}: {input} -> {artifact}"
            );
        }
    }

    #[test]
    fn invalid_reference_inputs_are_indeterminate_not_negative_labels() {
        use Contract::*;
        for (contract, input) in [
            (JsonPreserved, "bad"),
            (JsonPreserved, "null"),
            (IntegerSum, "{}"),
            (IntegerSum, "[1.0]"),
            (IntegerSum, "[true]"),
            (IntegerSum, "[9223372036854775808]"),
            (IntegerSum, "[9223372036854775807,1]"),
            (IntegerSum, "[-9223372036854775808,-1]"),
            (SupportedStatus, "{\"supported\":true}"),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[],\"extra\":1}",
            ),
            (
                SupportedStatus,
                "{\"supported\":true,\"required\":[\"x\",\"x\"]}",
            ),
            (SupportedStatus, "{\"supported\":true,\"required\":[\"\"]}"),
        ] {
            assert!(
                case(contract, input, "null").oracle().is_err(),
                "{contract:?}: {input}"
            );
        }
    }

    #[test]
    fn controls_validate_and_cover_good_bad_and_all_checker_states() {
        let cases = instrument_cases();
        let mut ids = HashSet::new();
        let mut good = false;
        let mut bad = false;
        for case in &cases {
            case.validate().expect("instrument control must validate");
            assert!(ids.insert(&case.id));
            assert_ne!(case.primary, case.secondary);
            match case.oracle().expect("determinate instrument control") {
                true => good = true,
                false => bad = true,
            }
        }
        assert!(good && bad);
        for state in [
            CheckerState::Accurate,
            CheckerState::Misleading,
            CheckerState::Inconclusive,
        ] {
            assert!(
                cases
                    .iter()
                    .any(|case| case.expected_checker_state == state)
            );
        }
        assert!(cases.iter().any(|case| case.oracle() == Ok(false)
            && case.check(case.primary, "probe".into()).outcome == CheckOutcome::Pass));
    }

    #[test]
    fn weak_predicates_execute_without_hidden_truth() {
        let mut weak = case(Contract::IntegerSum, "[2,3]", "7");
        weak.primary = Checker::IntegerParity;
        weak.secondary = Checker::IntegerNonnegative;
        weak.expected_checker_state = CheckerState::Misleading;
        weak.validate()
            .expect("real parity false-positive is misleading");
        let before = weak.check(weak.primary, "probe".into());
        assert_eq!(before.outcome, CheckOutcome::Pass);
        weak.contract = Contract::JsonPreserved;
        weak.expected_checker_state = CheckerState::Accurate;
        assert_eq!(before, weak.check(weak.primary, "probe".into()));
        weak.contract = Contract::IntegerSum;
        assert!(weak.validate().is_err());
        weak.artifact = "8".into();
        weak.validate()
            .expect("real parity failure agrees with bad artifact");
        weak.primary = Checker::NoResult;
        weak.expected_checker_state = CheckerState::Inconclusive;
        weak.validate().expect("actual no-result is inconclusive");
    }

    #[test]
    fn empty_artifacts_are_known_failures_not_indeterminate_references() {
        let mut control = case(Contract::JsonPreserved, "[]", "");
        control
            .validate()
            .expect("empty artifact fails both oracle and parse checker");
        assert_eq!(control.oracle(), Ok(false));
        control.artifact = " ".repeat(MAX_TEXT_BYTES + 1);
        assert!(control.oracle().is_err());
        assert!(control.validate().is_err());
    }

    #[test]
    fn duplicate_status_input_fields_are_indeterminate() {
        let control = case(
            Contract::SupportedStatus,
            r#"{"supported":false,"supported":true,"required":[]}"#,
            r#"{"status":"ready"}"#,
        );
        assert!(control.oracle().is_err());
        assert!(control.validate().is_err());
    }

    #[test]
    fn secondary_packet_executes_its_own_differing_predicate() {
        let mut control = case(Contract::JsonPreserved, "[1]", "[1,2]");
        control.secondary = Checker::JsonArrayLength;
        control.expected_checker_state = CheckerState::Misleading;
        let packet = control
            .visible_packet(Some(Method::Secondary))
            .expect("packet");
        let receipts: Vec<Receipt> =
            serde_json::from_value(packet["receipts"].clone()).expect("receipts");
        assert_eq!(receipts[0].outcome, CheckOutcome::Pass);
        assert_eq!(receipts[1].outcome, CheckOutcome::Fail);
        assert_eq!(
            receipts[1],
            control.check(Checker::JsonArrayLength, control.receipt_id(3))
        );
    }

    #[test]
    fn all_checker_predicates_have_positive_negative_or_inapplicable_controls() {
        use Checker::*;
        let examples = [
            (JsonParses, "[]", "null", CheckOutcome::Pass),
            (JsonParses, "[]", "no", CheckOutcome::Fail),
            (JsonArrayLength, "[1]", "[9]", CheckOutcome::Pass),
            (JsonArrayLength, "[1]", "[]", CheckOutcome::Fail),
            (JsonArrayLength, "{}", "[]", CheckOutcome::Inconclusive),
            (IntegerNonnegative, "[]", "0", CheckOutcome::Pass),
            (IntegerNonnegative, "[]", "-1", CheckOutcome::Fail),
            (IntegerParity, "[-3,1]", "-4", CheckOutcome::Pass),
            (IntegerParity, "[2,3]", "8", CheckOutcome::Fail),
            (IntegerParity, "[true]", "0", CheckOutcome::Inconclusive),
            (
                StatusFieldPresent,
                "[]",
                "{\"status\":null}",
                CheckOutcome::Pass,
            ),
            (StatusFieldPresent, "[]", "{}", CheckOutcome::Fail),
            (
                StatusNotUnsupported,
                "[]",
                "{\"status\":\"ready\"}",
                CheckOutcome::Pass,
            ),
            (
                StatusNotUnsupported,
                "[]",
                "{\"status\":\"unsupported\"}",
                CheckOutcome::Fail,
            ),
            (
                StatusNotUnsupported,
                "[]",
                "{\"status\":null}",
                CheckOutcome::Fail,
            ),
            (NoResult, "[]", "[]", CheckOutcome::Inconclusive),
        ];
        for (checker, input, artifact, outcome) in examples {
            assert_eq!(
                case(Contract::JsonPreserved, input, artifact)
                    .check(checker, "probe".into())
                    .outcome,
                outcome,
                "{checker:?}"
            );
        }
    }

    #[test]
    fn packets_bind_exact_artifact_bytes_and_match_allowed_receipts() {
        let mut control = instrument_cases().remove(1);
        for method in [
            None,
            Some(Method::SelfCritique),
            Some(Method::Repeat),
            Some(Method::Secondary),
        ] {
            let packet = control.visible_packet(method).expect("valid packet");
            let receipts: Vec<Receipt> =
                serde_json::from_value(packet["receipts"].clone()).expect("receipt shape");
            assert_eq!(
                receipts
                    .iter()
                    .map(|receipt| receipt.id.clone())
                    .collect::<Vec<_>>(),
                control.allowed_receipt_ids(method)
            );
            for receipt in &receipts {
                assert_eq!(
                    receipt.artifact_sha256,
                    super::super::digest(control.artifact.as_bytes())
                );
            }
            if method == Some(Method::Repeat) {
                assert_ne!(receipts[0].id, receipts[1].id);
                assert_eq!(receipts[0].checker, receipts[1].checker);
                assert_eq!(receipts[0].outcome, receipts[1].outcome);
            }
            if method == Some(Method::Secondary) {
                assert_ne!(receipts[0].checker, receipts[1].checker);
            }
        }
        let original = control.visible_packet(None).expect("packet");
        control.artifact.push(' ');
        let changed = control
            .visible_packet(None)
            .expect("whitespace still preserves decoded content");
        assert_ne!(
            original["receipts"][0]["artifact_sha256"],
            changed["receipts"][0]["artifact_sha256"]
        );
        assert_eq!(
            control.visible_packet(None),
            control.visible_packet(Some(Method::SelfCritique))
        );
    }

    #[test]
    fn packets_allowlist_visible_fields_without_private_labels() {
        for mut control in instrument_cases() {
            control.family = "PRIVATE_FAMILY_SENTINEL".into();
            let packet = control
                .visible_packet(Some(Method::Secondary))
                .expect("packet");
            let keys: HashSet<_> = packet
                .as_object()
                .expect("object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                HashSet::from(["id", "requirement", "input", "artifact", "receipts"])
            );
            let text = packet.to_string();
            for private in [
                "PRIVATE_FAMILY_SENTINEL",
                "expected_checker_state",
                "misleading",
                "accurate",
                "contract",
                "oracle",
                "primary",
                "secondary",
                "truth",
            ] {
                assert!(!text.contains(private), "leaked {private}: {text}");
            }
        }
    }

    #[test]
    fn case_validation_rejects_bad_metadata_limits_and_duplicate_policies() {
        let base = instrument_cases().remove(0);
        for id in ["", "bad:id", "a b", "é"] {
            let mut control = base.clone();
            control.id = id.into();
            assert!(control.validate().is_err());
        }
        let mut control = base.clone();
        control.id = "a".repeat(MAX_ID_BYTES + 1);
        assert!(control.validate().is_err());
        control = base.clone();
        control.secondary = control.primary;
        assert!(control.validate().is_err());
        control = base.clone();
        control.requirement = " ".into();
        assert!(control.validate().is_err());
        control = base.clone();
        control.input = " ".repeat(MAX_TEXT_BYTES + 1);
        assert!(control.validate().is_err());
        let mut value = serde_json::to_value(&base).expect("case serialization");
        value["unexpected"] = json!(true);
        assert!(serde_json::from_value::<Case>(value).is_err());
    }

    #[test]
    fn strict_judgment_accepts_only_explicit_well_formed_verdicts() {
        let allowed = vec!["t01:r1".into()];
        for verdict in ["complete", "incomplete", "insufficient_evidence"] {
            let text = json!({"verdict": verdict, "evidence_refs": ["t01:r1"], "reason": "Observed supplied receipt."}).to_string();
            let judgment = parse_judgment(&text, &allowed).expect("strict judgment");
            assert_eq!(judgment.evidence_refs, allowed);
        }
        let abstention = r#"{"verdict":"insufficient_evidence","evidence_refs":[],"reason":"No useful evidence."}"#;
        assert_eq!(
            parse_judgment(abstention, &[])
                .expect("explicit abstention")
                .verdict,
            Verdict::InsufficientEvidence
        );
    }

    #[test]
    fn strict_judgment_rejects_missing_unknown_enum_foreign_duplicate_and_salvage() {
        let allowed = vec!["t01:r1".into()];
        for text in [
            "{}",
            "null",
            "[]",
            "not json",
            r#"{"verdict":"complete","reason":"x"}"#,
            r#"{"verdict":"complete","evidence_refs":[]}"#,
            r#"{"verdict":"complete","evidence_refs":[],"reason":"x","truth":true}"#,
            r#"{"verdict":"abstain","evidence_refs":[],"reason":"x"}"#,
            r#"{"verdict":"Complete","evidence_refs":[],"reason":"x"}"#,
            r#"{"verdict":"complete","evidence_refs":["foreign"],"reason":"x"}"#,
            r#"{"verdict":"complete","evidence_refs":["t01:r1","t01:r1"],"reason":"x"}"#,
            r#"{"verdict":"complete","evidence_refs":[],"reason":" "}"#,
            r#"{"verdict":"complete","evidence_refs":[],"reason":null}"#,
            r#"{"verdict":"complete","evidence_refs":[],"reason":"x","reason":"y"}"#,
            r#"```json
{"verdict":"complete","evidence_refs":[],"reason":"x"}
```"#,
            r#"{"verdict":"complete","evidence_refs":[],"reason":"x"} trailing"#,
        ] {
            assert!(parse_judgment(text, &allowed).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn strict_types_reject_unknown_receipt_fields_and_round_trip_methods() {
        let control = instrument_cases().remove(0);
        let receipt = control.check(control.primary, control.receipt_id(1));
        let mut value = serde_json::to_value(receipt).expect("receipt serialization");
        value["truth"] = json!(true);
        assert!(serde_json::from_value::<Receipt>(value).is_err());
        for (method, name) in [
            (Method::SelfCritique, "self_critique"),
            (Method::Repeat, "repeat"),
            (Method::Secondary, "secondary"),
        ] {
            assert_eq!(
                serde_json::to_value(method).expect("method serialization"),
                json!(name)
            );
            assert_eq!(
                serde_json::from_value::<Method>(json!(name)).expect("method parsing"),
                method
            );
        }
        assert!(Method::SelfCritique < Method::Repeat && Method::Repeat < Method::Secondary);
        assert!(serde_json::from_value::<Method>(json!("unknown")).is_err());
        assert!(serde_json::from_value::<Checker>(json!("unknown")).is_err());
        assert!(serde_json::from_value::<CheckerState>(json!("unknown")).is_err());
        assert!(serde_json::from_value::<CheckOutcome>(json!("unknown")).is_err());
        assert!(serde_json::from_value::<Contract>(json!("unknown")).is_err());
    }

    #[test]
    fn judgment_limits_bound_bytes_reason_and_reference_count() {
        assert!(parse_judgment(&"x".repeat(MAX_JUDGMENT_BYTES + 1), &[]).is_err());
        let oversized_reason = json!({"verdict":"complete", "evidence_refs":[], "reason":"é".repeat(MAX_REASON_BYTES / 2 + 1)}).to_string();
        assert!(parse_judgment(&oversized_reason, &[]).is_err());
        let allowed: Vec<String> = (0..=MAX_REFS).map(|i| format!("t01:r{i}")).collect();
        let too_many =
            json!({"verdict":"complete", "evidence_refs": allowed, "reason":"x"}).to_string();
        assert!(parse_judgment(&too_many, &allowed).is_err());
    }
}
