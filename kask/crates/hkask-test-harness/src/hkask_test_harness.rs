#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Shared test fixtures, property-test generators, and oracle/trace
//! infrastructure for the evolving test harness.
//!
//! Existing items:
//! - [`arb_json_value`]: recursive JSON value strategy for proptest
//! - [`NoopToolPort`]: stub `ToolPort` returning NotFound for all invocations
//! - [`test_token_for_tool`]: deterministic `DelegationToken` fixture for governance tests
//! - [`test_agent_webid`]: the `delegated_to` WebID for gas-budget seeding
//!
//! Harness evolution items (trace filesystem + oracle taxonomy):
//! - [`Oracle`] trait + [`OracleVerdict`]: three oracle strategies (HarnessLLM)
//! - [`oracle_hardcoded`] / [`oracle_reference`] / [`oracle_invariant`] / [`oracle_inconclusive`]: constructors
//! - [`write_trace`] + [`TraceEntry`]: structured trace persistence (explicit trace dir, collision-safe)

use hkask_capability::{
    DelegationAction, DelegationResource, DelegationToken, ToolFuture, ToolInfo, ToolPort,
    ToolPortError,
};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatToolDefinition, InferenceError, InferencePort, InferenceResult, NotFound, WebID,
};
use proptest::prelude::*;
use serde_json::Value as JsonValue;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

// ── Oracle taxonomy (HarnessLLM §3) ───────────────────────────────────────

/// Verdict returned by an [`Oracle`] check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleVerdict {
    /// The output is correct for the given input.
    Pass,
    /// The output is wrong; the string explains why.
    Fail(String),
    /// The oracle cannot determine correctness for this input/output pair.
    Inconclusive,
}

/// An oracle checks whether a test case's outcome is correct.
///
/// Three strategies (HarnessLLM): hardcoded expected output, reference
/// implementation, invariant checking. Prefer programmatic generators
/// (invariant/reference) over hardcoded pairs — they scale with case count.
pub trait Oracle: Send + Sync {
    fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict;
}

/// Oracle 1: hardcoded expected output.
///
/// Scales poorly — TBR decays exponentially with case count (HarnessLLM).
/// Use only when the expected output is a single fixed value.
#[must_use]
pub fn oracle_hardcoded(expected: JsonValue) -> Box<dyn Oracle> {
    struct HardcodedOracle(JsonValue);
    impl Oracle for HardcodedOracle {
        fn verify(&self, _input: &JsonValue, output: &JsonValue) -> OracleVerdict {
            if output == &self.0 {
                OracleVerdict::Pass
            } else {
                OracleVerdict::Fail(format!("expected {:#}, got {:#}", self.0, output))
            }
        }
    }
    Box::new(HardcodedOracle(expected))
}

/// Oracle 2: reference implementation.
///
/// Scales — compare the output against a trusted independent implementation.
/// The reference function receives the same input and produces the expected
/// output; the oracle compares the two.
#[must_use]
pub fn oracle_reference<F>(reference: F) -> Box<dyn Oracle>
where
    F: Fn(&JsonValue) -> JsonValue + Send + Sync + 'static,
{
    struct ReferenceOracle<F>(F);
    impl<F> Oracle for ReferenceOracle<F>
    where
        F: Fn(&JsonValue) -> JsonValue + Send + Sync,
    {
        fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict {
            let expected = (self.0)(input);
            if output == &expected {
                OracleVerdict::Pass
            } else {
                OracleVerdict::Fail(format!(
                    "reference produced {:#}, got {:#}",
                    expected, output
                ))
            }
        }
    }
    Box::new(ReferenceOracle(reference))
}

/// Oracle 3: invariant checking.
///
/// Scales best — check properties of the output, not the output itself.
/// The check function receives `(input, output)` and returns `Ok(())` if the
/// invariant holds, or `Err(message)` if it is violated.
#[must_use]
pub fn oracle_invariant<F>(check: F) -> Box<dyn Oracle>
where
    F: Fn(&JsonValue, &JsonValue) -> Result<(), String> + Send + Sync + 'static,
{
    struct InvariantOracle<F>(F);
    impl<F> Oracle for InvariantOracle<F>
    where
        F: Fn(&JsonValue, &JsonValue) -> Result<(), String> + Send + Sync,
    {
        fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict {
            match (self.0)(input, output) {
                Ok(()) => OracleVerdict::Pass,
                Err(msg) => OracleVerdict::Fail(msg),
            }
        }
    }
    Box::new(InvariantOracle(check))
}

/// Oracle 4: reference implementation that may be unable to handle an input.
///
/// Like [`oracle_reference`], but the reference function returns
/// `Result<JsonValue, String>`. An `Ok` output is compared against the test
/// output (Pass/Fail); an `Err` means the reference could not evaluate this
/// input, yielding [`OracleVerdict::Inconclusive`] — the oracle cannot
/// determine correctness. This is the only constructor that produces
/// `Inconclusive`, closing the HarnessLLM three-verdict model.
#[must_use]
pub fn oracle_inconclusive<F>(reference: F) -> Box<dyn Oracle>
where
    F: Fn(&JsonValue) -> Result<JsonValue, String> + Send + Sync + 'static,
{
    struct InconclusiveOracle<F>(F);
    impl<F> Oracle for InconclusiveOracle<F>
    where
        F: Fn(&JsonValue) -> Result<JsonValue, String> + Send + Sync,
    {
        fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict {
            match (self.0)(input) {
                Ok(expected) => {
                    if output == &expected {
                        OracleVerdict::Pass
                    } else {
                        OracleVerdict::Fail(format!(
                            "reference produced {:#}, got {:#}",
                            expected, output
                        ))
                    }
                }
                Err(_) => OracleVerdict::Inconclusive,
            }
        }
    }
    Box::new(InconclusiveOracle(reference))
}

// ── Trace filesystem (Meta-Harness) ────────────────────────────────────────

/// A structured execution trace record, written to the trace filesystem by
/// [`write_trace`]. Consumed by the `harness-optimize` skill (proposer) and
/// the stability gate to form causal hypotheses about test suite quality.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// What produced this trace: `proptest`, `bug-hunt`, `test-run`.
    pub kind: String,
    /// Test or probe name (e.g., `prop_round_trip`, `charter_hkask_mcp`).
    pub name: String,
    /// `pass`, `fail`, `flaky`, or a custom status string.
    pub result: String,
    /// Execution duration in milliseconds (0 if not measured).
    pub duration_ms: u64,
    /// Proptest shrunk counterexample (if applicable, empty otherwise).
    pub shrunk_counterexample: String,
    /// Oracle type used: `hardcoded`, `reference`, `invariant`, empty if N/A.
    pub oracle_type: String,
    /// Free-form metadata (target function, crate, failure output, etc.).
    pub metadata: JsonValue,
}

impl TraceEntry {
    /// Serializes to a JSON object (no `serde` dependency — manual construction).
    fn to_json(&self) -> JsonValue {
        let mut map = serde_json::Map::new();
        map.insert("kind".to_string(), JsonValue::String(self.kind.clone()));
        map.insert("name".to_string(), JsonValue::String(self.name.clone()));
        map.insert("result".to_string(), JsonValue::String(self.result.clone()));
        map.insert(
            "duration_ms".to_string(),
            JsonValue::Number(self.duration_ms.into()),
        );
        map.insert(
            "shrunk_counterexample".to_string(),
            JsonValue::String(self.shrunk_counterexample.clone()),
        );
        map.insert(
            "oracle_type".to_string(),
            JsonValue::String(self.oracle_type.clone()),
        );
        map.insert("metadata".to_string(), self.metadata.clone());
        JsonValue::Object(map)
    }
}

/// Writes a structured trace entry to the trace filesystem.
///
/// The entry is written to `{trace_dir}/{run_id}/{kind}-{name}.json`. If a
/// file with the same `(kind, name)` already exists in the run directory, a
/// `-N` suffix is appended (starting at 2) so concurrent or repeated traces
/// with the same name do not silently overwrite each other.
///
/// `trace_dir` is taken explicitly so callers (and tests) do not have to
/// mutate the process-global `HKASK_TRACE_DIR` env var — which is unsafe under
/// parallel test execution. Production callers can resolve the dir from the
/// env var once at startup and pass it in.
///
/// This is the persistence layer that makes test execution visible to the
/// `harness-optimize` skill (the proposer) and the stability gate to form
/// causal hypotheses about test suite quality. Raw traces,
/// not compressed pass/fail scalars, are the key ingredient for harness
/// improvement (Meta-Harness paper).
pub fn write_trace(
    trace_dir: &std::path::Path,
    run_id: &str,
    entry: &TraceEntry,
) -> std::io::Result<PathBuf> {
    let run_dir = trace_dir.join(run_id);
    fs::create_dir_all(&run_dir)?;

    let safe_name = entry.name.replace(['/', '\\', ' ', ':'], "_");
    let stem = format!("{}-{}", entry.kind, safe_name);
    let path = unique_path(&run_dir, &stem, "json");

    let json = serde_json::to_string_pretty(&entry.to_json()).map_err(std::io::Error::other)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// Resolves a non-clobbering path: `{dir}/{stem}.{ext}`, or `{dir}/{stem}-N.{ext}`
/// if it already exists (N = 2, 3, …). Keeps the first match stable so repeated
/// writes with the same stem do not overwrite earlier traces.
fn unique_path(dir: &std::path::Path, stem: &str, ext: &str) -> PathBuf {
    let base = dir.join(format!("{stem}.{ext}"));
    if !base.exists() {
        return base;
    }
    for counter in 2..u64::MAX {
        let candidate = dir.join(format!("{stem}-{counter}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Practically unreachable: u64 counter space. Fall back to the base path.
    base
}

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

// ── PanicInferencePort ───────────────────────────────────────────────────

/// `InferencePort` that fails loudly if inference is invoked. Use in tests
/// that exercise compute-only or tool-only paths and should never call the
/// LLM. Unlike a silent noop, this returns an error so the test fails instead
/// of silently passing by skipping the inference path.
#[derive(Debug, Clone, Copy, Default)]
pub struct PanicInferencePort;

impl InferencePort for PanicInferencePort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async {
            Err(InferenceError::Generation(
                "PanicInferencePort: inference was called in a test that should not invoke LLM inference".into(),
            ))
        })
    }
}

// ── Proptest generators ─────────────────────────────────────────────────

/// Generates arbitrary `DelegationResource` variants.
pub fn arb_resource() -> BoxedStrategy<DelegationResource> {
    prop::sample::select(&[
        DelegationResource::Tool,
        DelegationResource::Template,
        DelegationResource::Registry,
        DelegationResource::Key,
    ])
    .boxed()
}

/// Generates arbitrary `DelegationAction` variants.
pub fn arb_action() -> BoxedStrategy<DelegationAction> {
    prop::sample::select(&[
        DelegationAction::Read,
        DelegationAction::Write,
        DelegationAction::Execute,
    ])
    .boxed()
}

/// Generates arbitrary `WebID` personas from short lowercase strings.
pub fn arb_webid() -> BoxedStrategy<WebID> {
    prop::string::string_regex("[a-z]{1,12}")
        .expect("valid regex")
        .prop_map(|s| WebID::from_persona(s.as_bytes()))
        .boxed()
}

/// Generates arbitrary `DelegationToken` values across all resource/action
/// combinations with arbitrary resource IDs and WebID personas.
pub fn arb_delegation_token() -> BoxedStrategy<DelegationToken> {
    (
        arb_resource(),
        prop::string::string_regex("[a-z_][a-z0-9_/]{0,20}").expect("valid regex"),
        arb_action(),
        arb_webid(),
        arb_webid(),
    )
        .prop_map(|(resource, resource_id, action, from, to)| {
            DelegationToken::new(resource, resource_id, action, from, to)
        })
        .boxed()
}

/// Generates arbitrary `TraceEntry` values for trace-filesystem property tests.
pub fn arb_trace_entry() -> BoxedStrategy<TraceEntry> {
    (
        prop::sample::select(&["proptest", "bug-hunt", "test-run"]),
        prop::string::string_regex("[a-z_][a-z0-9_/]{0,30}").expect("valid regex"),
        prop::sample::select(&["pass", "fail", "flaky"]),
        any::<u64>(),
        prop::option::of(prop::string::string_regex("[a-z0-9_=]{0,40}").expect("valid regex"))
            .prop_map(Option::unwrap_or_default),
        prop::sample::select(&["hardcoded", "reference", "invariant", ""]),
        arb_json_value(),
    )
        .prop_map(
            |(kind, name, result, duration_ms, shrunk, oracle_type, metadata)| TraceEntry {
                kind: kind.to_string(),
                name: name.to_string(),
                result: result.to_string(),
                duration_ms,
                shrunk_counterexample: shrunk,
                oracle_type: oracle_type.to_string(),
                metadata,
            },
        )
        .boxed()
}
