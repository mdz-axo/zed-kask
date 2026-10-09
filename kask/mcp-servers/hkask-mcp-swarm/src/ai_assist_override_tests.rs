//! Pins for the `swarm_ai_assist` model routing (card d58fd87b class, and
//! the 2026-10-09 refuse-on-None ruling): the suggest and advisory inference
//! routes resolve the classifier model or refuse with a typed error naming
//! `kask.models.classifier_model` (env `HKASK_CLASSIFIER_MODEL`) — never a
//! silent `None` fallback to the host default chat model, which can be
//! thinking-mandatory (OpenRouter 400 "Reasoning is mandatory for this
//! endpoint and cannot be disabled" — the original drop shape). Every pin
//! runs as a subprocess leg (the corpus retrieval_tests precedent):
//! in-process env mutation is unsafe (edition 2024) and racy under
//! parallel test threads.

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;

use crate::request_types::AiAssistRequest;
use crate::test_support::{
    OverrideRecordingInference, PROBE_CLASSIFIER_MODEL, content, make_thread_server, spawn_env_leg,
};

/// Drive one `swarm_ai_assist` call through the given double and return the
/// parsed tool payload.
async fn drive(
    action: &str,
    inference: Arc<OverrideRecordingInference>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let server = make_thread_server(dir.path(), inference, "test-passphrase");
    let output = server
        .swarm_ai_assist(Parameters(AiAssistRequest {
            action: action.into(),
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

/// Suggest routes through `generate_with_model` carrying the env-resolved
/// classifier override — the retained d58fd87b route behavior, now under the
/// refuse-on-None ruling.
#[tokio::test]
async fn suggest_routes_through_generate_with_model_with_the_classifier_override()
-> Result<(), Box<dyn std::error::Error>> {
    const LEG: &str = "KASS_SUGGEST_ROUTE_LEG";
    if std::env::var_os(LEG).is_some() {
        let inference = Arc::new(OverrideRecordingInference::default());
        let payload = drive("suggest", inference.clone()).await?;
        assert_eq!(
            payload["suggestions"]["name"], "probe-name",
            "the suggest payload must come from the override-bearing arm's fixture"
        );
        let calls = inference.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1, "one inference call per suggest");
        assert_eq!(
            calls[0].0, "generate_with_model",
            "suggest must route through generate_with_model — bare generate() lands on the \
             host default chat model, which can be thinking-mandatory (the d58fd87b drop shape)"
        );
        assert_eq!(
            calls[0].1.as_deref(),
            Some(PROBE_CLASSIFIER_MODEL),
            "the override must be the env-resolved classifier value — never silently absent"
        );
        return Ok(());
    }
    spawn_env_leg(
        LEG,
        "ai_assist_override_tests::suggest_routes_through_generate_with_model_with_the_classifier_override",
        true,
    )
    .await
}

/// Suggest refuses when the classifier model is unset: a typed error naming
/// the setting, zero port calls — the default chat model is never served
/// (RED on the pre-ruling tree, where the None arm silently succeeded).
#[tokio::test]
async fn suggest_refuses_without_the_classifier_model_naming_the_setting()
-> Result<(), Box<dyn std::error::Error>> {
    const LEG: &str = "KASS_SUGGEST_REFUSE_LEG";
    if std::env::var_os(LEG).is_some() {
        let inference = Arc::new(OverrideRecordingInference::default());
        let error = drive("suggest", inference.clone())
            .await
            .expect_err("suggest must refuse when the classifier model is unset");
        let message = error.to_string();
        assert!(
            message.contains("HKASK_CLASSIFIER_MODEL")
                || message.contains("kask.models.classifier_model"),
            "the refusal must name the setting, got: {message}"
        );
        assert!(
            inference.calls.lock().expect("calls").is_empty(),
            "no port call on refusal — the default chat model is never served"
        );
        return Ok(());
    }
    spawn_env_leg(
        LEG,
        "ai_assist_override_tests::suggest_refuses_without_the_classifier_model_naming_the_setting",
        false,
    )
    .await
}

/// The validate advisory routes through the override-bearing arm (env set) —
/// the advisory call carries the classifier override, never None.
#[tokio::test]
async fn validate_advisory_routes_through_the_classifier_override()
-> Result<(), Box<dyn std::error::Error>> {
    const LEG: &str = "KASS_VALIDATE_ROUTE_LEG";
    if std::env::var_os(LEG).is_some() {
        let inference = Arc::new(OverrideRecordingInference::default());
        let _payload = drive("validate", inference.clone()).await?;
        let calls = inference.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1, "one advisory inference call per validate");
        assert_eq!(
            calls[0].0, "generate_with_model",
            "the advisory must route through generate_with_model"
        );
        assert_eq!(
            calls[0].1.as_deref(),
            Some(PROBE_CLASSIFIER_MODEL),
            "the advisory override must be the env-resolved classifier value"
        );
        return Ok(());
    }
    spawn_env_leg(
        LEG,
        "ai_assist_override_tests::validate_advisory_routes_through_the_classifier_override",
        true,
    )
    .await
}

/// The advisory refusal degrades visably: the deterministic verdict stands,
/// the note names the setting, zero port calls (the degradation contract).
#[tokio::test]
async fn validate_advisory_refusal_degrades_visibly_naming_the_setting()
-> Result<(), Box<dyn std::error::Error>> {
    const LEG: &str = "KASS_VALIDATE_REFUSE_LEG";
    if std::env::var_os(LEG).is_some() {
        let inference = Arc::new(OverrideRecordingInference::default());
        let payload = drive("validate", inference.clone()).await?;
        let notes = payload["notes"].as_str().unwrap_or_default();
        assert!(
            notes.contains("HKASK_CLASSIFIER_MODEL")
                || notes.contains("kask.models.classifier_model"),
            "the advisory refusal must surface as a note naming the setting, got notes: {notes}"
        );
        assert!(
            inference.calls.lock().expect("calls").is_empty(),
            "no port call on refusal — the deterministic verdict stands without inference"
        );
        return Ok(());
    }
    spawn_env_leg(
        LEG,
        "ai_assist_override_tests::validate_advisory_refusal_degrades_visibly_naming_the_setting",
        false,
    )
    .await
}
