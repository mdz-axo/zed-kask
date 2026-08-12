#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Shared test fixtures, property-test generators, and oracle/trace
//! infrastructure for the evolving test harness.
//!
//! Existing items:
//! - [`arb_json_value`]: recursive JSON value strategy for proptest
//! - [`test_agent_webid`]: the accounting WebID for call-ceiling seeding
//!
//! Harness evolution items (trace filesystem + oracle taxonomy):
//! - [`Oracle`] trait + [`OracleVerdict`]: three oracle strategies (HarnessLLM)
//! - [`oracle_hardcoded`] / [`oracle_reference`] / [`oracle_invariant`] / [`oracle_inconclusive`]: constructors
//! - [`write_trace`] + [`TraceEntry`]: structured trace persistence (explicit trace dir, collision-safe)
//! - [`arb_trace_entry`]: proptest generator for trace property tests

use hkask_types::WebID;
use proptest::prelude::*;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::PathBuf;

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
/// invariant holds, or `Err(message)` if it is violated. The `String` payload
/// is a human-readable verdict message (fed to [`OracleVerdict::Fail`]), not a
/// recoverable error — it is never matched on variants, so `String` is the
/// correct type rather than a structured enum. Exempted from the
/// `Result<_, String>` gate by the test-harness exclusion in
/// `scripts/check-string-errors.sh`.
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
/// `Option<JsonValue>`. A `Some` output is compared against the test output
/// (Pass/Fail); a `None` means the reference could not evaluate this input,
/// yielding [`OracleVerdict::Inconclusive`] — the oracle cannot determine
/// correctness. This is the only constructor that produces `Inconclusive`,
/// closing the HarnessLLM three-verdict model. The decline carries no payload
/// (the verdict is Inconclusive regardless of why), so `Option` is the honest
/// signature — a `Result<_, _>` error would be discarded.
#[must_use]
pub fn oracle_inconclusive<F>(reference: F) -> Box<dyn Oracle>
where
    F: Fn(&JsonValue) -> Option<JsonValue> + Send + Sync + 'static,
{
    struct InconclusiveOracle<F>(F);
    impl<F> Oracle for InconclusiveOracle<F>
    where
        F: Fn(&JsonValue) -> Option<JsonValue> + Send + Sync,
    {
        fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict {
            match (self.0)(input) {
                Some(expected) => {
                    if output == &expected {
                        OracleVerdict::Pass
                    } else {
                        OracleVerdict::Fail(format!(
                            "reference produced {:#}, got {:#}",
                            expected, output
                        ))
                    }
                }
                None => OracleVerdict::Inconclusive,
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

// ── Agent identity fixture ───────────────────────────────────────────────

/// The accounting identity used by call-meter tests. Register a call ceiling for
/// this WebID to exercise the runaway-loop breaker.
#[must_use]
pub fn test_agent_webid() -> WebID {
    WebID::from_persona(b"test-agent")
}

// ── Proptest generators ─────────────────────────────────────────────────

/// Generates arbitrary `WebID` personas from short lowercase strings.
pub fn arb_webid() -> BoxedStrategy<WebID> {
    prop::string::string_regex("[a-z]{1,12}")
        .expect("valid regex")
        .prop_map(|s| WebID::from_persona(s.as_bytes()))
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
                name,
                result: result.to_string(),
                duration_ms,
                shrunk_counterexample: shrunk,
                oracle_type: oracle_type.to_string(),
                metadata,
            },
        )
        .boxed()
}

// ── Noop port stubs (enabler for ManifestExecutor tests) ──────────────────
//
// These stubs implement InferencePort and ToolPort with no-op/error returns so
// that ManifestExecutor can be constructed in tests without a real GPUI
// runtime or MCP server. They are the critical enabler for taint-propagation
// and runtime-policy end-to-end tests (RR-0053 companion, RR-0049 class).
//
// NoopInferencePort returns InferenceError::Generation on every call.
// NoopToolPort returns ToolPortError::InvocationFailed on invoke, empty
// discover_tools, and None for get_tool_info — except when configured with
// a taint map via NoopToolPort::with_taints, which lets get_tool_info return
// ToolInfo with a specific ToolTaint for FIDES flow tests.

use std::future::Future;
use std::pin::Pin;

/// No-op InferencePort for testing. Returns `InferenceError::Generation` on
/// every `generate` call. Use when a test needs to construct a
/// `ManifestExecutor` but doesn't exercise the inference path.
#[derive(Debug, Clone)]
pub struct NoopInferencePort;

impl hkask_types::InferencePort for NoopInferencePort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::template::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Generation(
                "NoopInferencePort".to_string(),
            ))
        })
    }
}

/// No-op ToolPort for testing. Returns errors by default, but can be
/// configured with a taint map so `get_tool_info` returns `ToolInfo` with
/// a specific `ToolTaint` — enabling FIDES Source→Sink flow tests.
#[derive(Debug, Clone, Default)]
pub struct NoopToolPort {
    taints: std::collections::HashMap<String, hkask_capability::tool_taint::ToolTaint>,
}

impl NoopToolPort {
    /// Create a new no-op tool port with no taint mappings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool with a specific FIDES taint label so `get_tool_info`
    /// returns it. This enables taint-propagation tests: a Source-tainted
    /// tool's output flows into the context, and a Sink-tainted tool's
    /// invocation with untrusted input should be blocked by the runtime
    /// policy.
    #[must_use]
    pub fn with_taint(
        mut self,
        tool_name: &str,
        taint: hkask_capability::tool_taint::ToolTaint,
    ) -> Self {
        self.taints.insert(tool_name.to_string(), taint);
        self
    }
}

impl hkask_capability::ToolPort for NoopToolPort {
    fn invoke<'a>(
        &'a self,
        _server: &'a str,
        _tool: &'a str,
        _args: serde_json::Value,
        _agent: hkask_types::WebID,
    ) -> hkask_capability::ToolFuture<'a, Result<serde_json::Value, hkask_capability::ToolPortError>>
    {
        Box::pin(async {
            Err(hkask_capability::ToolPortError::InvocationFailed(
                "NoopToolPort".to_string(),
            ))
        })
    }

    fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
        Box::pin(async { self.taints.keys().cloned().collect() })
    }

    fn get_tool_info<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
        Box::pin(async move {
            self.taints
                .get(tool_name)
                .map(|taint| hkask_capability::ToolInfo {
                    name: tool_name.to_string(),
                    description: "noop tool".to_string(),
                    input_schema: serde_json::json!({}),
                    server_id: "noop".to_string(),
                    taint: *taint,
                })
        })
    }
}

// ── Security-oriented proptest strategies ─────────────────────────────────

/// Generate a `HashMap<String, ToolTaint>` with mixed Source/Sink/Pure labels.
/// Used by taint-propagation tests to build context maps for `ManifestExecutor`
/// (RR-0053 companion, FIDES L4).
#[must_use]
pub fn arb_taint_context()
-> BoxedStrategy<std::collections::HashMap<String, hkask_capability::tool_taint::ToolTaint>> {
    let taint = prop_oneof![
        Just(hkask_capability::tool_taint::ToolTaint::Source),
        Just(hkask_capability::tool_taint::ToolTaint::Sink),
        Just(hkask_capability::tool_taint::ToolTaint::Pure),
        Just(hkask_capability::tool_taint::ToolTaint::Endorser),
    ];
    prop::collection::hash_map("[a-z_][a-z0-9_]{0,20}", taint, 1..10).boxed()
}

/// Generate a Jinja `{{ }}` template expression referencing a random
/// identifier. Used by taint-propagation tests to verify `extract_referenced_keys`
/// recognizes keys from the taint-labels map (not just `step_`-prefixed keys).
/// The generated string is a single `{{ ident }}` or `{{ ident.field }}`.
#[must_use]
pub fn arb_jinja_template() -> BoxedStrategy<String> {
    prop_oneof![
        "[a-z_][a-z0-9_]{0,20}".prop_map(|ident| format!("{{{{ {ident} }}}}")),
        ("[a-z_][a-z0-9_]{0,20}", "[a-z_][a-z0-9_]{0,15}")
            .prop_map(|(ident, field)| format!("{{{{ {ident}.{field} }}}}")),
    ]
    .boxed()
}

/// Generate a string containing a secret prefix + secret value + surrounding
/// text. The caller provides the secret prefixes (e.g. `SECRET_PREFIXES` from
/// `hkask-inference`) so the harness doesn't depend on `hkask-inference`.
/// Used by `sanitize_error_body` proptests to verify no secret survives
/// redaction (RR-0049/0050/0051).
#[must_use]
pub fn arb_secret_body(secret_prefixes: &'static [&'static str]) -> BoxedStrategy<String> {
    let prefix_idx = (0..secret_prefixes.len()).prop_map(|i| secret_prefixes[i]);
    (
        prefix_idx,
        "[A-Za-z0-9+/=_-]{1,40}",
        "[a-z ]{0,20}",
        "[a-z ]{0,20}",
    )
        .prop_map(|(prefix, secret, pre, post)| format!("{pre}{prefix}{secret}{post}"))
        .boxed()
}

/// Generate a string with URL injection characters (`&`, `#`, `=`, `../`,
/// percent-encoded sequences, control chars, multi-byte UTF-8). Used by
/// `url_encode_value` / `build_url` proptests to verify no injection survives
/// encoding (RR-0052).
#[must_use]
pub fn arb_url_with_injection() -> BoxedStrategy<String> {
    prop_oneof![
        "[&#=]{1,5}",
        "%[0-9a-fA-F]{2}",
        "\\.\\./",
        "[\\x00-\\x1f]{1,3}",
        "[éñ漢]{1,5}",
        "[A-Za-z0-9_.~ -]{0,30}",
    ]
    .prop_map(|parts| parts)
    .boxed()
}

/// Generate a provider error body mixing JSON, HTML, secrets, control chars,
/// and multi-byte UTF-8. Used by cross-crate `sanitize_error_body` call-site
/// tests to fuzz the 13 provider error paths (RR-0049/0050/0051).
#[must_use]
pub fn arb_http_error_body() -> BoxedStrategy<String> {
    prop_oneof![
        // JSON error bodies
        arb_json_value().prop_map(|v| v.to_string()),
        // HTML error pages
        "<html>[a-z ]{0,50}</html>".prop_map(|s| s),
        // Bodies with control chars
        "[\\x00-\\x1f]{1,10}".prop_map(|s| s),
        // Multi-byte UTF-8
        "[éñ漢]{1,10}".prop_map(|s| s),
        // Plain text with mixed content
        "[a-zA-Z0-9 .,;:!?\"'\\-_/]{0,100}".prop_map(|s| s),
    ]
    .boxed()
}
