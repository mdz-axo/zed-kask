//! Pins for the `swarm_ai_assist` suggest path's model routing (card
//! d58fd87b, live 2026-10-08). The suggest path called bare
//! `InferencePort::generate()` — no `model_override` — so every suggest
//! request landed on the host session's default chat model, which can be
//! thinking-mandatory (OpenRouter 400 "Reasoning is mandatory for this
//! endpoint and cannot be disabled"), leaving the authoring aid
//! permanently unavailable while wire probes against the live bridge
//! proved the bridge itself honors overrides. The fix routes suggest
//! through `generate_with_model` with the classifier override
//! (`kask.models.classifier_model`, env `HKASK_CLASSIFIER_MODEL`) — the
//! same fix class as the validate advisory's reroute (f8ba3c40d8). These
//! pins hold that route in place: the trait arm used, and the override
//! value read from the env at call time.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;

use crate::request_types::AiAssistRequest;
use crate::test_support::{content, fixture_result, make_thread_server};

/// A port double that records which trait method served each call and the
/// `model_override` it carried. The bare-`generate` arm fails with the drop
/// shape's name so a regression reads as the routing bug it is, not as a
/// stub quirk.
#[derive(Default)]
struct OverrideRecordingInference {
    calls: Mutex<Vec<(&'static str, Option<String>)>>,
}

impl hkask_types::InferencePort for OverrideRecordingInference {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
                + Send
                + '_,
        >,
    > {
        self.calls.lock().expect("calls").push(("generate", None));
        Box::pin(async {
            Err(hkask_types::InferenceError::Model(
                "bare generate() is the d58fd87b drop shape — the suggest path must \
                 route through generate_with_model with the classifier override"
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
    ) -> Pin<
        Box<
            dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
                + Send
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

/// Drive one `swarm_ai_assist` suggest call through the given double and
/// return the parsed tool payload.
async fn drive_suggest(
    inference: Arc<OverrideRecordingInference>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let server = make_thread_server(dir.path(), inference, "test-passphrase");
    let output = server
        .swarm_ai_assist(Parameters(AiAssistRequest {
            action: "suggest".into(),
            surface: "agent".into(),
            mode: "local".into(),
            name: "probe-agent".into(),
            agent_type: "research".into(),
            description: "A probe.".into(),
            system_prompt: String::new(),
            mission: String::new(),
            agents: String::new(),
            tags: String::new(),
            sample_queries: String::new(),
            accepts: String::new(),
            produces: String::new(),
            has_valence: false,
        }))
        .await?;
    content(&output)
}

#[tokio::test]
async fn suggest_routes_through_generate_with_model_with_the_classifier_override()
-> Result<(), Box<dyn std::error::Error>> {
    let inference = Arc::new(OverrideRecordingInference::default());
    let payload = drive_suggest(inference.clone()).await?;
    assert_eq!(
        payload["suggestions"]["name"], "probe-name",
        "the suggest payload must come from the override-bearing arm's fixture"
    );
    let calls = inference.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1, "one inference call per suggest");
    assert_eq!(
        calls[0].0, "generate_with_model",
        "suggest must route through generate_with_model — bare generate() lands on the \
         host default chat model, which can be thinking-mandatory (OpenRouter 400, the \
         d58fd87b drop shape)"
    );
    assert_eq!(
        calls[0].1.as_deref(),
        hkask_inference::model_constants::classifier_model().as_deref(),
        "the override must be the classifier model resolved at call time — never a \
         hardcoded value and never silently absent"
    );
    Ok(())
}

/// The value pin: `classifier_model()` reads `HKASK_CLASSIFIER_MODEL` at
/// call time, so the recorded override must equal the env the server
/// process runs with. In-process env mutation is unsafe (edition 2024) and
/// racy under parallel test threads, so this spawns the test binary as a
/// subprocess with the env set — the corpus precedent
/// (`retrieval_tests::tagging_persists_method_signals_without_trusting_the_model`).
#[tokio::test]
async fn suggest_passes_the_env_classifier_value_through_to_the_port()
-> Result<(), Box<dyn std::error::Error>> {
    const FIXTURE_ENV: &str = "KASK_SUGGEST_OVERRIDE_FIXTURE";
    if std::env::var_os(FIXTURE_ENV).is_some() {
        let inference = Arc::new(OverrideRecordingInference::default());
        let payload = drive_suggest(inference.clone()).await?;
        assert_eq!(
            payload["suggestions"]["name"], "probe-name",
            "fixture: the suggest payload must come from the override-bearing arm"
        );
        let calls = inference.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1, "fixture: one inference call per suggest");
        assert_eq!(
            calls[0].0, "generate_with_model",
            "fixture: suggest must route through generate_with_model"
        );
        assert_eq!(
            calls[0].1.as_deref(),
            Some("probe/classifier-model"),
            "the port must see the env-injected classifier override, not the default \
             chat model"
        );
        return Ok(());
    }
    let out = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "ai_assist_override_tests::suggest_passes_the_env_classifier_value_through_to_the_port",
            "--nocapture",
        ])
        .env(FIXTURE_ENV, "1")
        .env("HKASK_CLASSIFIER_MODEL", "probe/classifier-model")
        .output()
        .await?;
    assert!(
        out.status.success(),
        "fixture run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}
