//! Shared fixtures for the swarm server's cfg(test) modules: the recording
//! inference port, the no-tools dispatch port, the thread-server builders,
//! the fixture-card writers, and the tool-result content helper. One
//! canonical copy (R5 batch-2) — the test modules import from here instead
//! of re-declaring.

use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::SwarmServer;
use crate::config::SwarmConfig;
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
/// ignore it, `prompt` to use it. The `with_messages` and `with_model`
/// arms are opt-in for the doubles that also implement those paths.
macro_rules! inference_generate {
    (
        $name:ident,
        generate($s:ident, $prompt:ident, $params:ident, $tools:ident): $body:block
        $(, with_messages($wm_s:ident, $messages:ident, $wm_params:ident, $wm_model:ident, $wm_tools:ident): $with_messages:block)?
        $(, with_model($gm_s:ident, $gm_prompt:ident, $gm_params:ident, $gm_model:ident, $gm_tools:ident): $with_model:block)?
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

            $(fn generate_with_model(
                &$gm_s,
                $gm_prompt: &str,
                $gm_params: &hkask_types::LLMParameters,
                $gm_model: Option<&str>,
                $gm_tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
                >,
            > $with_model)?
        }
    };
}

pub(crate) use inference_generate;

/// Generate a `ToolDispatchPort` test stub: a unit struct `$name` whose
/// `tool_definition` serves any allowed `server/tool` name (Auth-rejecting
/// the rest) with `$description` and `$parameters`, and whose `invoke_tool`
/// runs `$invoke` — a block over the four invoke parameters returning
/// `Result<Value, InferenceError>`. The `Pin<Box<dyn Future>>` signature
/// boilerplate both dispatch stubs repeated is single-sourced here;
/// parameter names come from the invocation (macro hygiene): pass `_args`
/// to ignore it, `args` to use it.
macro_rules! allowed_dispatch {
    (
        $name:ident,
        $description:literal,
        $parameters:expr,
        invoke: |$server:ident, $tool:ident, $args:ident, $allowlist:ident| $invoke:block
    ) => {
        struct $name;

        impl hkask_types::ToolDispatchPort for $name {
            fn tool_definition<'a>(
                &'a self,
                server: &'a str,
                tool: &'a str,
                allowed: &'a [String],
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                hkask_types::ChatToolDefinition,
                                hkask_types::InferenceError,
                            >,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async move {
                    $crate::test_support::allowed_tool_definition(
                        server,
                        tool,
                        allowed,
                        $description,
                        $parameters,
                    )
                })
            }

            fn invoke_tool<'a>(
                &'a self,
                $server: &'a str,
                $tool: &'a str,
                $args: serde_json::Value,
                $allowlist: &'a [String],
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<serde_json::Value, hkask_types::InferenceError>,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async move $invoke)
            }
        }
    };
}

pub(crate) use allowed_dispatch;

#[derive(Default)]
pub(crate) struct RecordingInference(pub(crate) Mutex<Vec<Vec<hkask_types::ChatMessage>>>);

inference_generate! {
    RecordingInference,
    generate(self, _prompt, _params, _tools): {
        Box::pin(async {
            Err(hkask_types::InferenceError::Model(
                "expected messages".into(),
            ))
        })
    },
    with_messages(self, messages, _params, _model, _tools): {
        let mut calls = self.0.lock().expect("recorder");
        calls.push(messages.to_vec());
        let text = format!("reply {}", calls.len());
        // The yield lets concurrent delegations interleave before their
        // replies commit — the store-serialization tests rely on it.
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(fixture_result(text))
        })
    },
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

inference_generate! {
    OverrideRecordingInference,
    generate(self, _prompt, _params, _tools): {
        self.calls.lock().expect("calls").push(("generate", None));
        Box::pin(async {
            Err(hkask_types::InferenceError::Model(
                "bare generate() is the d58fd87b drop shape — the call site must route \
                 through generate_with_model with the classifier override"
                    .into(),
            ))
        })
    },
    with_model(self, _prompt, _params, model_override, _tools): {
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
    },
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

/// The allowlist-checking `tool_definition` arm the dispatch stubs share:
/// serve any allowed `server/tool` name with the given description and
/// parameter schema, Auth-rejecting names outside the allowlist.
pub(crate) fn allowed_tool_definition(
    server: &str,
    tool: &str,
    allowed: &[String],
    description: &str,
    parameters: Value,
) -> Result<hkask_types::ChatToolDefinition, hkask_types::InferenceError> {
    let name = format!("{server}/{tool}");
    if !allowed.contains(&name) {
        return Err(hkask_types::InferenceError::Auth("not allowed".into()));
    }
    Ok(hkask_types::ChatToolDefinition {
        tool_type: "function".into(),
        function: hkask_types::ChatToolFunction {
            name,
            description: description.into(),
            parameters,
        },
    })
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
        Arc::new(SwarmConfig {
            local_agents_dir: agents.clone(),
            local_swarms_dir: swarms.clone(),
            ..SwarmConfig::default()
        }),
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

/// The standard scoped-dispatch fixture: a thread server on a fresh
/// tempdir whose runtime records every inference call. Keep the returned
/// dir alive for as long as the server — dropping it deletes the stores
/// the server points at.
pub(crate) fn recording_thread_server(
    passphrase: &str,
) -> (tempfile::TempDir, SwarmServer, Arc<RecordingInference>) {
    let dir = tempfile::tempdir().expect("dir");
    let recorder = Arc::new(RecordingInference::default());
    let server = make_thread_server(dir.path(), recorder.clone(), passphrase);
    (dir, server, recorder)
}

/// Write a fixture agent card under `id` — `agent_type: "test"`, the
/// "Answer tasks" system prompt, `fixture/model` — so the registry has a
/// delegatable agent. `extra` merges top-level fields into the card for
/// per-test additions (e.g. a `workflow_template`).
pub(crate) fn write_fixture_card(
    server: &SwarmServer,
    id: &str,
    extra: Option<Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut card = serde_json::json!({
        "agent_id": id, "agent_type": "test", "description": "fixture",
        "capabilities": {"system_prompt": "Answer tasks", "model": "fixture/model"}
    });
    if let Some(extra) = extra {
        for (key, value) in extra.as_object().expect("extra card fields are an object") {
            card[key] = value.clone();
        }
    }
    server
        .local_registry
        .write_card(&serde_json::from_value(card)?)?;
    Ok(())
}

/// Write fixture cards for every `ids` entry (see [`write_fixture_card`]).
pub(crate) fn write_fixture_cards(
    server: &SwarmServer,
    ids: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    for id in ids {
        write_fixture_card(server, id, None)?;
    }
    Ok(())
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
