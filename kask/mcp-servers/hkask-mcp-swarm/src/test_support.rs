//! Shared fixtures for the swarm server's cfg(test) modules: the recording
//! inference port, the no-tools dispatch port, the thread-server builder,
//! and the tool-result content helper. One canonical copy (R5 batch-2) —
//! the test modules import from here instead of re-declaring.

use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::SwarmServer;
use crate::abw_client::SwarmClient;
use crate::config::SwarmConfig;
use crate::consent::ConsentStore;
use crate::local_knowledge::LazyLocalMemory;
use crate::local_registry::LocalAgentRegistry;
use crate::local_runtime::{LazyEventStore, LazyLocalSwarmRuntime, LocalSwarmRuntime};
use crate::local_swarms::LocalSwarmRegistry;

/// A canned successful inference result — the literal every fixture-based
/// test returns.
pub(crate) fn fixture_result(text: String) -> hkask_types::InferenceResult {
    hkask_types::InferenceResult {
        text,
        model: "fixture".into(),
        usage: Default::default(),
        finish_reason: "stop".into(),
        tool_calls: vec![],
        reasoning: None,
        cost_usd: None,
    }
}

/// The canned stub answer the executor doubles return — "stub" text from
/// "stub-model" with single-token usage and no tool calls. Distinct from
/// `fixture_result`: the executor tests assert these exact values.
pub(crate) fn stub_result() -> hkask_types::InferenceResult {
    hkask_types::InferenceResult {
        text: "stub".into(),
        model: "stub-model".into(),
        usage: hkask_types::InferenceUsage {
            prompt_tokens: 1,
            completion_tokens: 1,
            total_tokens: 2,
            reported: true,
        },
        finish_reason: "stop".into(),
        tool_calls: vec![],
        reasoning: None,
        cost_usd: None,
    }
}

/// Generate the `InferencePort` impl for a swarm test double.
/// The `Pin<Box<dyn Future>>` signature boilerplate every double repeated
/// is single-sourced here; the bodies are the double's own. Parameter
/// names come from the invocation (macro hygiene): pass `_prompt` to
/// ignore it, `prompt` to use it. The `with_messages` arm is opt-in for
/// the doubles that also implement the messages path.
macro_rules! inference_generate {
    (
        $name:ident,
        generate($s:ident, $prompt:ident, $params:ident, $tools:ident): $body:block
        $(, with_messages($wm_s:ident, $messages:ident, $wm_params:ident, $wm_model:ident, $wm_tools:ident): $with_messages:block)?
        $(,)?
    ) => {
        impl hkask_types::InferencePort for $name {
            fn generate(
                &$s,
                $prompt: &str,
                $params: &hkask_types::LLMParameters,
                $tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                        > + Send
                        + '_,
                >,
            > $body

            $(fn generate_with_messages(
                &$wm_s,
                $messages: &[hkask_types::ChatMessage],
                $wm_params: &hkask_types::LLMParameters,
                $wm_model: Option<&str>,
                $wm_tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                        > + Send
                        + '_,
                >,
            > $with_messages)?
        }
    };
}

pub(crate) use inference_generate;

#[derive(Default)]
pub(crate) struct RecordingInference(pub(crate) Mutex<Vec<Vec<hkask_types::ChatMessage>>>);

impl hkask_types::InferencePort for RecordingInference {
    fn generate(
        &self,
        _: &str,
        _: &hkask_types::LLMParameters,
        _: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Model(
                "expected messages".into(),
            ))
        })
    }

    fn generate_with_messages(
        &self,
        messages: &[hkask_types::ChatMessage],
        _: &hkask_types::LLMParameters,
        _: Option<&str>,
        _: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        let mut calls = self.0.lock().expect("recorder");
        calls.push(messages.to_vec());
        let text = format!("reply {}", calls.len());
        // The yield lets concurrent delegations interleave before their
        // replies commit — the store-serialization tests rely on it.
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(fixture_result(text))
        })
    }
}

/// A port double that records which trait method served each call and the
/// `model_override` it carried — the fixture for the model-routing pins
/// (the d58fd87b class: a bare-`generate` call site lands on the host
/// default chat model, which can be thinking-mandatory). The
/// bare-`generate` arm fails with the drop shape's name so a regression
/// reads as the routing bug it is, not as a stub quirk.
#[derive(Default)]
pub(crate) struct OverrideRecordingInference {
    pub(crate) calls: Mutex<Vec<(&'static str, Option<String>)>>,
}

impl hkask_types::InferencePort for OverrideRecordingInference {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        self.calls.lock().expect("calls").push(("generate", None));
        Box::pin(async {
            Err(hkask_types::InferenceError::Model(
                "bare generate() is the d58fd87b drop shape — the call site must route \
                 through generate_with_model with the classifier override"
                    .into(),
            ))
        })
    }

    fn generate_with_model(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::LLMParameters,
        model_override: Option<&str>,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        self.calls
            .lock()
            .expect("calls")
            .push(("generate_with_model", model_override.map(str::to_string)));
        Box::pin(async {
            Ok(fixture_result(
                r#"{"name":"probe-name","agent_type":"research","description":"A probe.",
                    "system_prompt":"You probe.","mission":"","agents":""}"#
                    .to_string(),
            ))
        })
    }
}

pub(crate) struct NoTools;
impl hkask_types::ToolDispatchPort for NoTools {
    fn tool_definition<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: &'a [String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::ChatToolDefinition, hkask_types::InferenceError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(hkask_types::InferenceError::Model("no tools".into())) })
    }
    fn invoke_tool<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Value,
        _: &'a [String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Value, hkask_types::InferenceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(hkask_types::InferenceError::Model("no tools".into())) })
    }
}

pub(crate) fn make_thread_server(
    dir: &std::path::Path,
    inference: Arc<dyn hkask_types::InferencePort>,
    passphrase: &str,
) -> SwarmServer {
    let agents = dir.join("agents").to_string_lossy().into_owned();
    let swarms = dir.join("swarms").to_string_lossy().into_owned();
    SwarmServer::new(
        hkask_types::WebID::new(),
        Arc::new(SwarmClient::new(
            reqwest::Client::new(),
            SwarmConfig::default(),
        )),
        Arc::new(ConsentStore::default()),
        Arc::new(LocalAgentRegistry::new(&agents)),
        Arc::new(LazyLocalSwarmRuntime::with_runtime(
            LocalSwarmRuntime::new_for_test(inference, Arc::new(NoTools), String::new()),
        )),
        Arc::new(LocalSwarmRegistry::new(swarms)),
        Arc::new(LazyLocalMemory::lazy(
            dir.join("semantic.db").to_string_lossy().into_owned(),
            "test-passphrase".into(),
            1024,
        )),
        Arc::new(crate::agent_stats::AgentStatsStore::load(&agents)),
        Arc::new(LazyEventStore::lazy(
            dir.join("events.db").to_string_lossy().into_owned(),
        )),
        Arc::new(crate::thread_store::SwarmThreadStore::new(
            dir.join("threads.db").to_string_lossy().into_owned(),
            passphrase.into(),
        )),
    )
}

pub(crate) fn content(output: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str::<Value>(output)?["content"].clone())
}

/// The probe classifier-model value the env-dependent legs set — distinct
/// from every real model id so a leaked value reads as a fixture bug.
pub(crate) const PROBE_CLASSIFIER_MODEL: &str = "probe/classifier-model";

/// Spawn the test binary as a subprocess for an env-dependent test leg (the
/// corpus retrieval_tests precedent): in-process env mutation is unsafe
/// (edition 2024) and racy under parallel test threads. `set_classifier`
/// sets `HKASK_CLASSIFIER_MODEL` to the probe value; `false` removes it (the
/// refusal legs).
pub(crate) async fn spawn_env_leg(
    leg_env: &str,
    test: &str,
    set_classifier: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args(["--exact", test, "--nocapture"])
        .env(leg_env, "1");
    if set_classifier {
        command.env("HKASK_CLASSIFIER_MODEL", PROBE_CLASSIFIER_MODEL);
    } else {
        command.env_remove("HKASK_CLASSIFIER_MODEL");
    }
    let out = command.output().await?;
    assert!(
        out.status.success(),
        "{leg_env} run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}
