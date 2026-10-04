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
    inference: Arc<RecordingInference>,
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
