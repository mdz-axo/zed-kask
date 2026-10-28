use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v2 as acp;
use anyhow::Result;
use futures::{
    FutureExt,
    channel::oneshot,
    future::{Either, select},
};
use gpui::{App, AppContext, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ui::SharedString;

/// Evaluate a deterministic Lisp form against a JSON environment.
///
/// This tool provides the deterministic computation layer for skill processes.
/// Use it to check structural invariants, compute convergence signals, and
/// perform arithmetic on structured data that the LLM cannot reliably
/// self-evaluate (counting its own outputs, verifying field presence, scoring).
///
/// The interpreter is sandboxed: no I/O, no `eval`, no `load`, no network.
/// Evaluation is bounded by `max_steps` and `max_depth` to prevent infinite loops.
///
/// JSON objects become association lists — use `(assoc "key" alist)` to access
/// fields. Pretty JSON results are capped at 1048576 UTF-8 bytes, excluding envelopes. Its pretty-printed UTF-8 document
/// is limited to 1048576 bytes, excluding tool/MCP envelopes.
///
/// Example: count open threats from a prior step's output:
/// ```text
/// form: "(+ (assoc \"confirmed_bugs\" (assoc \"summary\" step_5_result)) (assoc \"potential_bugs\" (assoc \"summary\" step_5_result)))"
/// env: "{\"step_5_result\":{\"summary\":{\"confirmed_bugs\":3,\"potential_bugs\":2}}}"
/// ```
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LispEvalToolInput {
    /// The Lisp form to evaluate. Special forms: `quote`, `if`, `let`,
    /// `lambda`, `define`, `begin`, `and`, `or`, `not`, `cond`. Builtins:
    /// arithmetic (`+`, `-`, `*`, `/`, `=`, `!=`, `<`, `<=`, `>`, `>=`),
    /// `car`, `cdr`, `cons`, `list`, `length`, `nth`, `reverse`, `is_null`,
    /// `numberp`, `listp`, `stringp`, `assoc`, `assoc!`, `append`, `member`, `abs`,
    /// `sqrt`, `max`, `min`, `eq`, `string=`, `string-contains`,
    /// `starts-with`, `ends-with`, `concat`, `boundary-ok`.
    /// Integer arithmetic is checked (overflow errors, never wraps silently)
    /// and all-integer comparisons are exact (never coerced through f64).
    /// `/` always returns a Float (`(/ 6 3)` → `2.0`); over 3+ args `!=`
    /// compares adjacent pairs. Bare infix is accepted for the arithmetic
    /// and comparison operators (`a + b` → `(+ a b)`; same-operator chains
    /// fold; mixed operators do not associate; a parenthesized `(a + b)`
    /// is NOT equivalent — it errors).
    /// Source is limited to 65536 raw UTF-8 bytes before tokenization.
    #[serde(deserialize_with = "deserialize_form_field")]
    form: String,
    /// JSON text encoding an object whose keys become Lisp bindings, for
    /// example `"{\"a\":1,\"b\":true}"`. Values retain their JSON types:
    /// objects become association lists, arrays become lists, numbers stay
    /// numbers and strings stay strings. Do not wrap scalars in objects.
    /// At most 1048576 raw UTF-8 bytes; size is checked before inner JSON decoding.
    /// Malformed JSON or a non-object document is rejected before evaluation.
    #[serde(
        default,
        deserialize_with = "deserialize_env_field",
        serialize_with = "hkask_types::tool_schema::serialize_json_text"
    )]
    #[schemars(schema_with = "hkask_types::tool_schema::json_text_schema")]
    env: std::collections::HashMap<String, serde_json::Value>,
    /// Maximum evaluation steps (default 100000, ceiling 2000000).
    /// Requests above the provisional ceiling are rejected, not clamped.
    /// The `maxSteps` alias tolerates camelCase emissions so an explicit budget
    /// is applied rather than silently dropped (a silently-dropped budget was
    /// an unteachable failure: the eval failed with the DEFAULT limit while
    /// the model believed it had raised it).
    #[serde(
        default = "default_max_steps",
        alias = "maxSteps",
        deserialize_with = "deserialize_max_steps_field"
    )]
    max_steps: u64,
    /// Maximum evaluation depth (default 1024, ceiling 8192).
    /// Requests above the provisional ceiling are rejected, not clamped.
    /// Recursive helper forms over lists consume roughly 2–4 depth frames
    /// per element, so the former default of 64 overflowed at ~16 elements —
    /// real-scale validation lists (100+ claims) failed on the first attempt
    /// and wasted turns on retries (observed live: a 134-element list needed
    /// 300). 1024 covers realistic registries out of the box; genuinely
    /// infinite recursion still trips the budget immediately.
    #[serde(
        default = "default_max_depth",
        alias = "maxDepth",
        deserialize_with = "deserialize_max_depth_field"
    )]
    max_depth: u64,
}

// Per-field teaching deserializers: each names its field, the received shape,
// and the accepted shape on rejection (lisp-repair L1). An error that does
// not teach produces identical retries — the 2026-09-28 audit lockout
// anatomy: 5 identical malformed-env emissions hard-refused by the per-input
// retry tracker.

fn deserialize_form_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "form",
        "a string containing the Lisp form, e.g. \"(+ 1 2)\"",
        false,
    )
}

fn deserialize_env_field<'de, D>(
    deserializer: D,
) -> Result<std::collections::HashMap<String, serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[cfg(test)]
    ENV_DECODE_COUNT.with(|count| count.set(count.get() + 1));
    let teaching_error = |error| {
        serde::de::Error::custom(format!(
            "env: expected JSON text encoding an object: {error}"
        ))
    };
    let text =
        String::deserialize(deserializer).map_err(|error| teaching_error(error.to_string()))?;
    if text.len() > hkask_lisp::MAX_ENV_JSON_BYTES {
        return Err(serde::de::Error::custom(format!(
            "env requested {} raw JSON UTF-8 bytes; allowed {}",
            text.len(),
            hkask_lisp::MAX_ENV_JSON_BYTES
        )));
    }
    validate_env_json_numbers(&text)
        .map_err(|error| teaching_error(format!("expected complete JSON text: {error}")))?;
    hkask_types::tool_schema::deserialize_json_text_object(
        serde::de::value::StringDeserializer::<D::Error>::new(text),
        "env",
    )
}

/// Validate spelling before Value can round a large integer or discard an
/// earlier duplicate-key value. RawValue's scanner does not enforce serde's
/// default recursion limit, so check structure first without constructing Value.
fn validate_env_json_numbers(text: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<EnvJsonStructure>(text)?;
    let root: &serde_json::value::RawValue = serde_json::from_str(text)?;
    let mut pending = vec![root];
    while let Some(raw) = pending.pop() {
        let token = raw.get();
        match token.as_bytes().first() {
            Some(b'{') => {
                let children: EnvJsonMembers<'_> = serde_json::from_str(token)?;
                pending.extend(children.0);
            }
            Some(b'[') => {
                let children: Vec<&serde_json::value::RawValue> = serde_json::from_str(token)?;
                pending.extend(children);
            }
            Some(b'-' | b'0'..=b'9')
                if !token.contains(['.', 'e', 'E']) && token.parse::<i64>().is_err() =>
            {
                return Err(serde::de::Error::custom(format!(
                    "integer {token} is outside signed i64 range ({}..={})",
                    i64::MIN,
                    i64::MAX
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Keep all object values, including values overwritten by a duplicate key.
struct EnvJsonMembers<'a>(Vec<&'a serde_json::value::RawValue>);

impl<'de> Deserialize<'de> for EnvJsonMembers<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MembersVisitor;
        impl<'de> serde::de::Visitor<'de> for MembersVisitor {
            type Value = EnvJsonMembers<'de>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut values = Vec::new();
                while map.next_key::<serde::de::IgnoredAny>()?.is_some() {
                    values.push(map.next_value()?);
                }
                Ok(EnvJsonMembers(values))
            }
        }
        deserializer.deserialize_map(MembersVisitor)
    }
}

/// Unlike IgnoredAny/RawValue, recursive deserialize_any calls retain serde's
/// default nesting checks. Numeric callbacks here discard only their temporary
/// parsed representation; the original text remains authoritative below.
struct EnvJsonStructure;

impl<'de> Deserialize<'de> for EnvJsonStructure {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(EnvJsonStructure)
    }
}

impl<'de> serde::de::Visitor<'de> for EnvJsonStructure {
    type Value = Self;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_seq<S: serde::de::SeqAccess<'de>>(self, mut seq: S) -> Result<Self, S::Error> {
        while seq.next_element::<Self>()?.is_some() {}
        Ok(self)
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Self, M::Error> {
        while map.next_key::<serde::de::IgnoredAny>()?.is_some() {
            map.next_value::<Self>()?;
        }
        Ok(self)
    }
}

fn deserialize_max_steps_field<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "max_steps",
        "a non-negative integer (default 100000)",
        false,
    )
}

fn deserialize_max_depth_field<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "max_depth",
        "a non-negative integer (default 1024)",
        false,
    )
}

fn default_max_steps() -> u64 {
    100000
}

fn default_max_depth() -> u64 {
    // 1024, not 64: recursive helpers consume 2–4 depth frames per list
    // element, so 64 overflowed at ~16 elements and real validation lists
    // (100+) failed their first attempt (observed: 134 elements needed 300).
    // Infinite recursion still trips this immediately — the budget is a
    // guard, not a workload ceiling.
    1024
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LispEvalToolOutput {
    Success { result: Value },
    Error { error: String },
}

impl From<LispEvalToolOutput> for LanguageModelToolResultContent {
    fn from(value: LispEvalToolOutput) -> Self {
        match value {
            LispEvalToolOutput::Success { result } => match hkask_lisp::format_json_output(&result)
            {
                Ok(text) => text.into(),
                Err(error) => error.to_string().into(),
            },
            LispEvalToolOutput::Error { error } => error.into(),
        }
    }
}

pub struct LispEvalTool;

fn title_for_form(form: Option<&str>) -> SharedString {
    match form {
        Some(form) => format!("Evaluating: {}", form.chars().take(80).collect::<String>()).into(),
        None => "Lisp Evaluation".into(),
    }
}

#[cfg(test)]
thread_local! {
    static ENV_DECODE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static RUN_PROBE: std::cell::RefCell<Option<RunProbe>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct RunProbe {
    pub(crate) executor: gpui::BackgroundExecutor,
    pub(crate) entered: futures::channel::oneshot::Sender<bool>,
    pub(crate) release: futures::channel::oneshot::Receiver<()>,
    pub(crate) decoded: futures::channel::oneshot::Sender<(bool, usize)>,
    pub(crate) exited: Option<futures::channel::oneshot::Sender<bool>>,
}

impl AgentTool for LispEvalTool {
    type Input = LispEvalToolInput;
    type Output = LispEvalToolOutput;

    const NAME: &'static str = "lisp_eval";

    fn deserialize_replay_input(input: serde_json::Value) -> Result<Self::Input> {
        super::deserialize_saved_json_map(input, "env")
    }

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn raw_initial_title(&self, input: &Value) -> Option<SharedString> {
        Some(title_for_form(input.get("form").and_then(Value::as_str)))
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        title_for_form(input.as_ref().ok().map(|input| input.form.as_str()))
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        #[cfg(test)]
        let probe = RUN_PROBE.with(|probe| probe.take());
        let cancellation = Arc::new(AtomicBool::new(false));
        let cancel_on_drop = util::defer({
            let cancellation = cancellation.clone();
            move || cancellation.store(true, Ordering::Release)
        });
        let done = event_stream.register_lisp_completion();
        let mut worker = cx.background_spawn(lisp_worker(
            input,
            cancellation.clone(),
            event_stream.cancelled_by_user(),
            done,
            #[cfg(test)]
            probe,
        ));
        cx.spawn(async move |_| {
            let _cancel_on_drop = cancel_on_drop;
            let result = match select(&mut worker, Box::pin(event_stream.cancelled_by_user())).await
            {
                Either::Left((result, _)) => result,
                Either::Right(((), worker)) => {
                    cancellation.store(true, Ordering::Release);
                    worker.await
                }
            };
            if event_stream.cancelled_by_user().now_or_never().is_some() {
                Err(LispEvalToolOutput::Error {
                    error: hkask_lisp::LispError::Cancelled.to_string(),
                })
            } else {
                result
            }
        })
    }
}

/// Build the worker with its completion guard already alive before the first poll.
fn lisp_worker(
    input: ToolInput<LispEvalToolInput>,
    cancellation: Arc<AtomicBool>,
    input_cancelled: impl Future<Output = ()> + Send + 'static,
    done: oneshot::Sender<()>,
    #[cfg(test)] mut probe: Option<RunProbe>,
) -> impl Future<Output = Result<LispEvalToolOutput, LispEvalToolOutput>> + Send {
    #[cfg(test)]
    let exited = probe.as_mut().and_then(|probe| probe.exited.take());
    #[cfg(test)]
    let exit_cancellation = cancellation.clone();
    let worker_done = util::defer(move || {
        if done.send(()).is_err() {
            log::debug!("Lisp worker cleanup completed after its turn owner was dropped");
        }
        #[cfg(test)]
        if let Some(exited) = exited {
            exited
                .send(exit_cancellation.load(Ordering::Acquire))
                .expect("worker exit observer");
        }
    });
    async move {
        // Captured before spawn, so a never-polled future acknowledges too.
        let _worker_done = worker_done;
        #[cfg(test)]
        let decoded_probe = if let Some(probe) = probe {
            probe
                .entered
                .send(probe.executor.is_main_thread())
                .expect("entry observer");
            probe.release.await.expect("release real evaluation");
            ENV_DECODE_COUNT.with(|count| count.set(0));
            Some((probe.executor, probe.decoded))
        } else {
            None
        };
        let input = match select(Box::pin(input.recv()), Box::pin(input_cancelled)).await {
            Either::Left((input, _)) => input.map_err(|e| LispEvalToolOutput::Error {
                error: format!("failed to receive input: {e}"),
            })?,
            Either::Right(((), _)) => {
                return Err(LispEvalToolOutput::Error {
                    error: hkask_lisp::LispError::Cancelled.to_string(),
                });
            }
        };
        #[cfg(test)]
        if let Some((executor, decoded)) = decoded_probe {
            decoded
                .send((
                    executor.is_main_thread(),
                    ENV_DECODE_COUNT.with(|count| count.get()),
                ))
                .expect("decode observer");
        }
        evaluate_lisp_controlled(input, Some(cancellation))
            .map(|result| LispEvalToolOutput::Success { result })
            .map_err(|error| LispEvalToolOutput::Error { error })
    }
}

/// The tool's evaluation, shared with delegated `host/lisp_eval` dispatch.
pub fn evaluate_lisp(input: LispEvalToolInput) -> Result<Value, String> {
    evaluate_lisp_controlled(input, None)
}

/// Shared evaluator for adapters that own an invocation cancellation flag.
/// Decoding uses `LispEvalToolInput`; the pure entry above remains uncontrolled.
pub fn evaluate_lisp_controlled(
    input: LispEvalToolInput,
    cancellation: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> Result<Value, String> {
    // Sorted for byte-identical error messages across processes (HashMap
    // iteration order is process-randomized — the L2 canonicalization rule
    // applied to the error path too).
    let mut env_bindings: Vec<String> = input.env.keys().cloned().collect();
    env_bindings.sort();
    let nested_env = input
        .env
        .get("env")
        .is_some_and(|value| value.as_object().is_some());
    // Bindings whose value is a JSON object — the wrapped-scalar signature.
    // An object becomes an association list, so a scalar wrapped as
    // `{"n": 1}` reaches a scalar position as a list and fails with
    // "got list" — an error that (pre-L3) named neither the binding nor
    // the fix. Observed live 2026-10-02: three identical retries across two
    // lisp_eval gate forms, including one under explicit deliberation to
    // emit bare scalars — the wrapping is deterministic for this emitter,
    // so the error must teach on the FIRST failure (lisp-repair L1).
    let mut object_bindings: Vec<String> = input
        .env
        .iter()
        .filter(|(_, value)| value.as_object().is_some())
        .map(|(key, _)| key.clone())
        .collect();
    object_bindings.sort();
    let env_value = {
        // Canonical order: env keys are sorted before the object is built, so
        // the boundary is deterministic across processes (HashMap iteration
        // order is process-randomized; preserve_order then freezes whatever
        // order arrives first). No form can observe top-level key order today
        // (each key becomes an individual binding — proven lisp-repair L0), so
        // this is defense-in-depth for any future env-introspection surface;
        // the error path sorts independently (bindings list).
        let mut pairs: Vec<(String, serde_json::Value)> = input.env.into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        serde_json::Value::Object(pairs.into_iter().collect())
    };
    match cancellation {
        Some(cancellation) => hkask_lisp::eval_sandboxed_with_cancellation(
            &input.form,
            &env_value,
            input.max_steps,
            input.max_depth,
            cancellation,
        ),
        None => hkask_lisp::eval_sandboxed_with_budget(
            &input.form,
            &env_value,
            input.max_steps,
            input.max_depth,
        ),
    }
    .map_err(|error| match error {
        // The unbound-symbol error teaches: it names the available bindings,
        // so a model that nested env one level too deep (the unteachable class
        // — the emission deserialized cleanly, the error never named the
        // nesting) sees the stray "env" binding and the flatten fix.
        hkask_lisp::LispError::UnboundSymbol(name) => {
            let mut message = format!("unbound symbol: {name} — env bindings: {env_bindings:?}");
            if nested_env {
                message.push_str(
                    " (an env binding named \"env\" was received — if env was nested one \
                     level too deep, flatten it: {\"a\": 1}, not {\"env\": {\"a\": 1}})",
                );
            }
            message
        }
        // The type error teaches when the wrapped-scalar signature is
        // present: an object-valued binding reached a scalar position
        // (surfacing as "got list"). Names the suspects and the fix so the
        // first error changes the next emission; a type error without
        // object bindings (or with a non-list actual — the objects are not
        // the culprit) keeps the plain message.
        hkask_lisp::LispError::TypeError { expected, actual }
            if actual == "list" && !object_bindings.is_empty() =>
        {
            format!(
                "type error: expected {expected}, got {actual} — env bindings \
                 carrying objects (objects become association lists): \
                 {object_bindings:?}. If you wrapped a scalar in an object, \
                 pass the scalar directly: {{\"a\": 1}}, not {{\"a\": {{\"n\": 1}}}}"
            )
        }
        hkask_lisp::LispError::TypeError { expected, actual }
            if expected == "number" && actual == "nil" =>
        {
            "type error: expected number, got nil (if this value came from \
             (assoc \"key\" obj), the key is missing or null — use (assoc! \
             \"key\" obj) to list available keys, or guard with (is_null ...))"
                .to_string()
        }
        hkask_lisp::LispError::TypeError { expected, actual }
            if expected == "list"
                && actual != "list"
                && (input.form.contains("(assoc ") || input.form.contains("(assoc! "))
                && (input.form.contains("(cdr ") || input.form.contains("(car ")) =>
        {
            format!(
                "type error: expected {expected}, got {actual} (note: in lisp_eval, \
                 (assoc key obj) returns the value directly, not a (key . value) \
                 cons pair — omit (cdr ...) / (car ...) around assoc)"
            )
        }
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::LispEvalToolInput;
    use serde_json::json;

    /// expect: "Dropping the actual production worker before any poll acknowledges cleanup without entering or decoding" [H6B]
    #[test]
    fn h6b_unpolled_production_worker_acknowledges_without_decoding() {
        use futures::channel::oneshot;
        use std::sync::{Arc, atomic::AtomicBool};

        let (done_tx, mut done_rx) = oneshot::channel();
        let (entered_tx, mut entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (decoded_tx, mut decoded_rx) = oneshot::channel();
        let (exited_tx, mut exited_rx) = oneshot::channel();
        let probe = super::RunProbe {
            executor: gpui::BackgroundExecutor::new(Arc::new(gpui::TestDispatcher::new(0))),
            entered: entered_tx,
            release: release_rx,
            decoded: decoded_tx,
            exited: Some(exited_tx),
        };
        super::ENV_DECODE_COUNT.with(|count| count.set(0));
        let worker = super::lisp_worker(
            crate::ToolInput::ready(json!({"form":"(+ a b)", "env":"{unfinished"})),
            Arc::new(AtomicBool::new(false)),
            std::future::pending::<()>(),
            done_tx,
            Some(probe),
        );
        assert_eq!(
            done_rx.try_recv(),
            Ok(None),
            "worker owns a live acknowledgement"
        );
        assert_eq!(entered_rx.try_recv(), Ok(None), "worker has not entered");
        assert_eq!(super::ENV_DECODE_COUNT.with(|count| count.get()), 0);
        // No spawn, poll, scheduler tick, or executor-driven drop ordering.
        drop(worker);
        assert_eq!(
            done_rx.try_recv(),
            Ok(Some(())),
            "affirmative cleanup, not a closed channel"
        );
        assert_eq!(
            super::ENV_DECODE_COUNT.with(|count| count.get()),
            0,
            "malformed env was never decoded"
        );
        assert!(
            entered_rx.try_recv().is_err(),
            "entry sender dropped without a message"
        );
        assert!(
            decoded_rx.try_recv().is_err(),
            "decode sender dropped without a message"
        );
        assert_eq!(
            exited_rx.try_recv(),
            Ok(Some(false)),
            "constructor-created guard ran without foreground cancellation"
        );
        assert!(
            release_tx.send(()).is_err(),
            "no worker awaits the H5 barrier"
        );
    }

    /// expect: "Native control reaches H6A without changing delegated/default evaluation or model inputs" [H6B]
    #[test]
    fn h6b_private_control_preserves_public_default() {
        let form = "(let ((f (lambda () 3))) (f))";
        let input = || {
            serde_json::from_value::<LispEvalToolInput>(json!({"form":form}))
                .expect("closure input")
        };
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        assert_eq!(
            super::evaluate_lisp_controlled(input(), Some(cancelled)),
            Err("Lisp evaluation cancelled".into())
        );
        assert_eq!(
            super::evaluate_lisp(input()).expect("delegated default unchanged"),
            json!(3)
        );
        let schema =
            serde_json::to_value(schemars::schema_for!(LispEvalToolInput)).expect("input schema");
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(properties.len(), 4);
        for key in ["form", "env", "max_steps", "max_depth"] {
            assert!(properties.contains_key(key));
        }
    }

    /// expect: "Erased partial and full titles inspect form only, never decode env" [H5]
    #[gpui::test]
    fn h5_raw_title_skips_env_decode(cx: &mut gpui::TestAppContext) {
        use crate::{AgentTool, ToolInputPayload};
        let tool = super::LispEvalTool.erase();
        let long_form = "é🙂".repeat(50);
        for form in ["(+ a b)", long_form.as_str()] {
            for env in [json!("{unfinished"), json!(false), json!("{\"a\":2}")] {
                let raw = json!({"form": form, "env": env});
                for payload in [
                    ToolInputPayload::Partial(raw.clone()),
                    ToolInputPayload::Full(raw.clone()),
                ] {
                    let (ToolInputPayload::Partial(raw) | ToolInputPayload::Full(raw)) = payload
                    else {
                        unreachable!()
                    };
                    super::ENV_DECODE_COUNT.with(|count| count.set(0));
                    let title = cx.update(|cx| tool.initial_title(raw, cx));
                    assert_eq!(
                        super::ENV_DECODE_COUNT.with(|count| count.get()),
                        0,
                        "title must not enter the environment decoder"
                    );
                    assert_eq!(
                        title.as_ref(),
                        format!("Evaluating: {}", form.chars().take(80).collect::<String>())
                    );
                }
            }
        }
        for raw in [
            json!({}),
            json!({"form": 7}),
            json!({"form": null}),
            json!(null),
        ] {
            super::ENV_DECODE_COUNT.with(|count| count.set(0));
            assert_eq!(
                cx.update(|cx| tool.initial_title(raw, cx)).as_ref(),
                "Lisp Evaluation"
            );
            assert_eq!(super::ENV_DECODE_COUNT.with(|count| count.get()), 0);
        }
    }

    /// expect: "Real erased run decodes and evaluates on the background executor while foreground progresses" [H5]
    #[gpui::test]
    async fn h5_run_dispatches_decode_and_evaluation(cx: &mut gpui::TestAppContext) {
        use crate::{AgentTool, ToolCallEventStream, ToolInputSender};
        use futures::{FutureExt, channel::oneshot};
        use std::{cell::Cell, rc::Rc};

        let (entered_tx, mut entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (decoded_tx, decoded_rx) = oneshot::channel();
        let tool = super::LispEvalTool.erase();
        let (mut sender, input) = ToolInputSender::channel();
        sender.send_partial(json!({"form": "(+ a b)", "env": "{unfinished"}));
        sender.send_full(json!({"form": "(+ a b)", "env": "{\"a\":2,\"b\":3}"}));
        let (events, _rx) = ToolCallEventStream::test();
        let mut task = cx.update(|cx| {
            super::RUN_PROBE.with(|probe| {
                assert!(
                    probe
                        .borrow_mut()
                        .replace(super::RunProbe {
                            executor: cx.background_executor().clone(),
                            entered: entered_tx,
                            release: release_rx,
                            decoded: decoded_tx,
                            exited: None,
                        })
                        .is_none()
                );
            });
            tool.run(input, events, cx)
        });
        cx.executor().run_until_parked();
        let entered_on_main = entered_rx
            .try_recv()
            .expect("entry channel")
            .expect("worker entered");
        assert!(
            (&mut task).now_or_never().is_none(),
            "real run held before decoding"
        );
        let foreground_ran = Rc::new(Cell::new(false));
        cx.spawn({
            let foreground_ran = foreground_ran.clone();
            async move |_| foreground_ran.set(true)
        })
        .detach();
        cx.executor().run_until_parked();
        assert!(
            foreground_ran.get(),
            "foreground callback while worker held"
        );
        release_tx.send(()).expect("release worker");
        let output = task
            .await
            .unwrap_or_else(|error| panic!("real evaluator failed: {}", error.raw_output));
        let (decoded_on_main, decode_count) = decoded_rx.await.expect("decode provenance");
        assert!(!entered_on_main, "worker entry must not be foreground");
        assert!(!decoded_on_main, "typed decode must not be foreground");
        assert_eq!(decode_count, 1, "only Full input is decoded");
        assert_eq!(output.raw_output, json!({"result": 5}));
        let [language_model::LanguageModelToolResultContent::Text(text)] =
            output.llm_output.as_slice()
        else {
            panic!("generic wrapper must produce one text result")
        };
        assert_eq!(text.as_ref(), "5");
    }

    #[test]
    fn env_wire_schema_and_decoder_preserve_scalar_bindings() {
        let schema =
            serde_json::to_value(schemars::schema_for!(LispEvalToolInput)).expect("schema");
        assert_eq!(schema["properties"]["env"]["type"], "string");
        assert!(hkask_types::find_boolean_schema_positions(&schema).is_empty());
        let input: LispEvalToolInput = serde_json::from_value(json!({
            "form": "(+ count 1)",
            "env": json!({"count": 7, "literal": "{\"not\":\"an object\"}"}).to_string()
        }))
        .expect("JSON-text environment");
        assert_eq!(input.env["count"], json!(7));
        assert!(input.env["literal"].is_string());
        let wire = serde_json::to_value(&input).expect("serialize native input");
        assert!(wire["env"].is_string());
        assert_eq!(
            super::evaluate_lisp(input).expect("evaluate decoded bindings"),
            json!(8)
        );
    }

    #[test]
    fn saved_env_migration_preserves_bindings_without_relaxing_live_inputs() {
        use crate::AgentTool;

        let bindings = json!({
            "count": 7,
            "ready": true,
            "items": [1, null, {"nested": 2}],
            "literal": "{\"not\":\"an object\"}"
        });
        let saved = json!({"form": "(+ count 1)", "env": bindings});
        assert!(serde_json::from_value::<LispEvalToolInput>(saved.clone()).is_err());
        for env in [bindings.clone(), json!(bindings.to_string())] {
            let mut input = saved.clone();
            input["env"] = env;
            let decoded = super::LispEvalTool::deserialize_replay_input(input)
                .expect("decode saved environment");
            assert_eq!(
                serde_json::to_value(&decoded.env).expect("bindings"),
                bindings
            );
            assert_eq!(
                super::evaluate_lisp(decoded).expect("scalar arithmetic"),
                json!(8)
            );
        }
        let omitted = super::LispEvalTool::deserialize_replay_input(json!({"form": "(+ 1 2)"}))
            .expect("omitted environment keeps its default");
        assert_eq!(
            super::evaluate_lisp(omitted).expect("constant arithmetic"),
            json!(3)
        );
    }

    // The following interpreter tests cover evaluation independently of the
    // model-facing JSON-text transport; the wire round-trip is tested above.

    #[test]
    fn default_max_depth_covers_real_scale_lists() {
        // The regression pin for the 64→1024 raise: deserializing a call that
        // omits max_depth must yield the raised default — the observed live
        // case (a 134-element recursive helper needing ~300) failed its first
        // attempt at 64.
        let input: LispEvalToolInput =
            serde_json::from_str(r#"{"form": "(+ 1 2)"}"#).expect("deserializes");
        assert_eq!(input.max_depth, 1024);
    }

    #[test]
    fn test_interp_assoc_access_on_json_object() {
        let result = hkask_lisp::eval_sandboxed_with_budget(
            r#"(assoc "count" step_result)"#,
            &json!({ "step_result": { "count": 42, "status": "complete" } }),
            100000,
            64,
        );
        assert!(result.is_ok(), "assoc should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(42));
    }

    #[test]
    fn test_interp_length_on_list() {
        let result = hkask_lisp::eval_sandboxed_with_budget(
            "(length items)",
            &json!({ "items": [1, 2, 3, 4, 5] }),
            100000,
            64,
        );
        assert!(result.is_ok(), "length should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(5));
    }

    #[test]
    fn test_interp_compound_convergence_pattern() {
        // The convergence pattern from the enhanced prompt: extract chunk
        // count, guard against zero, return the count.
        let form = r#"(let ((chunks (assoc "chunks" embed_result))) (if (> chunks 0) chunks 0))"#;
        let result = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({ "embed_result": { "chunks": 127, "model": "text-embedding-3-small" } }),
            100000,
            64,
        );
        assert!(
            result.is_ok(),
            "compound form should succeed: {:?}",
            result.err()
        );
        assert_eq!(result.expect("checked is_ok above"), json!(127));
    }

    #[test]
    fn test_interp_zero_count_guard() {
        // Same form, but chunks is 0 — the guard should return 0, not error.
        let form = r#"(let ((chunks (assoc "chunks" embed_result))) (if (> chunks 0) chunks 0))"#;
        let result = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({ "embed_result": { "chunks": 0 } }),
            100000,
            64,
        );
        assert!(
            result.is_ok(),
            "zero-count guard should succeed: {:?}",
            result.err()
        );
        assert_eq!(result.expect("checked is_ok above"), json!(0));
    }

    #[test]
    fn test_interp_arithmetic_literal() {
        // The form that was confirmed working via the tool earlier.
        let result = hkask_lisp::eval_sandboxed_with_budget("(+ 1 2 3)", &json!({}), 100000, 64);
        assert!(
            result.is_ok(),
            "arithmetic should succeed: {:?}",
            result.err()
        );
        assert_eq!(result.expect("checked is_ok above"), json!(6));
    }

    // JSON text is the wire representation, including for an empty environment.
    #[test]
    fn test_env_accepts_stringified_json() {
        let input = json!({"form": "(+ 1 2)", "env": "{}"});
        let result: LispEvalToolInput =
            serde_json::from_value(input).expect("stringified env must be accepted");
        assert!(
            result.env.is_empty(),
            "stringified empty object must parse to empty map"
        );
    }

    // Regression: a valid stringified JSON object must also parse.
    #[test]
    fn test_env_accepts_stringified_json_object() {
        let input = json!({"form": "(+ 1 2)", "env": "{\"step_5_result\": {\"count\": 3}}"});
        let result: LispEvalToolInput =
            serde_json::from_value(input).expect("stringified env object must be accepted");
        assert_eq!(result.env.len(), 1);
        assert!(result.env.contains_key("step_5_result"));
    }

    #[test]
    fn test_env_rejects_unencoded_object() {
        let input = json!({"form": "(+ 1 2)", "env": {"step_5_result": {"count": 3}}});
        let error = serde_json::from_value::<LispEvalToolInput>(input)
            .expect_err("unencoded map is not the wire contract");
        assert!(error.to_string().contains("env:"));
        assert!(error.to_string().contains("JSON text"));
    }
}
