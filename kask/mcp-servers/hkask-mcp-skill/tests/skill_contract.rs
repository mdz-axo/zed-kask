//! Contract tests for hkask-mcp-skill — skill loading and server construction.
//!
//! Every test carries the full traceability chain:
//! `UserFunctionalExpectation (expect:) → GoalPrinciple [P{N}] → ConstrainingPrinciple [P{N}] → REQ: → Test`
//!
//! Tested seam: SkillServer construction, skill index loading (no inference dependency).

use hkask_mcp_server::server::CapabilityTier;
use hkask_mcp_skill::SkillExecuteRequest;
use hkask_mcp_skill::SkillServer;
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tempfile::tempdir;

/// A no-op InferencePort for testing SkillServer construction.
struct NoopInferencePort;

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
            Ok(hkask_types::InferenceResult {
                text: String::new(),
                model: "noop".into(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                finish_reason: "stop".into(),
                token_probabilities: None,
                tool_calls: vec![],
                reasoning: None,
            })
        })
    }
}

// ── Server construction tests ──────────────────────────────────────────────

#[test]
fn skill_server_constructs_with_noop_port() {
    let server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    assert_eq!(server.userpod, "test-userpod");
    assert!(server.skills.is_empty(), "new server should have no skills");
}

#[test]
fn skill_server_loads_skills_from_registry() {
    let mut server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    server.load_skills();
    // load_skills should succeed (even if empty) — no panic
}

// ── Skill index type tests ─────────────────────────────────────────────────────────

#[test]
fn skill_server_stores_registry_entries_directly() {
    // After C1, the server stores RegistryEntry values (no SkillToolDef wrapper).
    let mut server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    assert!(
        server.skills.is_empty(),
        "new server should have no entries"
    );
    // Insert a synthetic entry to confirm the field type is HashMap<String, RegistryEntry>.
    server.skills.insert(
        "test.step".to_string(),
        hkask_types::RegistryEntry {
            id: "test/step".into(),
            template_type: hkask_types::template_type::TemplateType::WordAct,
            name: "step".into(),
            lexicon_terms: vec![],
            description: "a test template".into(),
            source_path: "registry/templates/test/step.j2".into(),
            required_capabilities: vec![],
            cascade_level: 0,
            matroshka_limit: 7,
        },
    );
    assert_eq!(server.skills.len(), 1);
    assert_eq!(server.skills["test.step"].description, "a test template");
}

// ── Schema generation test ─────────────────────────────────────────────────

#[test]
fn skill_execute_request_has_schema() {
    let schema = schemars::schema_for!(hkask_mcp_skill::SkillExecuteRequest);
    let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
    assert!(schema_json.is_object());
}

// ── Tool-behavior contract tests (Parameters<T> seam) ───────────────────────
//
// These exercise the actual MCP tool methods through the public `Parameters<T>`
// seam — the same surface an agent uses. Closes the test-variety gap that hid
// the create-new-file, range-inversion, and multibyte-truncation defects in
// hkask-mcp-filesystem.

/// Parse the success envelope `{"content": <value>}`; falls back to the raw
/// value for non-envelope outputs.
fn parse_content(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("content").cloned().unwrap_or(v)
}

/// Extract the `error` message from an error envelope, if present.
fn error_kind(out: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("kind").and_then(|e| e.as_str()).map(String::from)
}

// REQ: skill_ping returns liveness and profile info (P5 Testing Discipline).
// expect: skill_ping returns status=ok and skills_loaded=0 for a fresh server.
#[tokio::test]
async fn skill_ping_returns_status_ok_via_parameters_seam() {
    let server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    let out = server.skill_ping().await;
    let content = parse_content(&out);
    assert_eq!(content["status"], "ok");
    assert_eq!(content["skills_loaded"], 0);
    assert_eq!(content["mode"], "standalone");
}

// REQ: skill_list returns the registered skills (P5).
// expect: an empty server returns an empty skills array.
#[tokio::test]
async fn skill_list_returns_empty_for_fresh_server_via_parameters_seam() {
    let server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    let out = server.skill_list().await;
    let content = parse_content(&out);
    assert!(content["skills"].is_array());
    assert_eq!(content["skills"].as_array().unwrap().len(), 0);
}

// REQ: skill_execute rejects an unknown skill_id with NotFound (P5, P3).
// expect: a non-existent skill_id returns an error with kind=not_found.
#[tokio::test]
async fn skill_execute_rejects_unknown_skill_id_via_parameters_seam() {
    let server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    // SkillExecuteRequest fields are private — construct via JSON deserialization,
    // the same path the MCP framework uses to build parameters from wire data.
    let req: SkillExecuteRequest = serde_json::from_value(serde_json::json!({
        "skill_id": "nonexistent.skill",
        "context": {}
    }))
    .expect("deserialize SkillExecuteRequest");
    let out = server.skill_execute(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for unknown skill");
    assert_eq!(kind, "not_found", "got: {out}");
}

// REQ: skill_execute rejects a non-object context with invalid_argument (P5).
// expect: an array context is rejected with kind=invalid_argument.
#[tokio::test]
async fn skill_execute_rejects_non_object_context_via_parameters_seam() {
    // Insert a synthetic skill so we get past the not_found check and reach
    // the context validation branch.
    let mut server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    server.skills.insert(
        "test.step".to_string(),
        hkask_types::RegistryEntry {
            id: "test/step".into(),
            template_type: hkask_types::template_type::TemplateType::WordAct,
            name: "step".into(),
            lexicon_terms: vec![],
            description: "a test template".into(),
            source_path: "registry/templates/test/step.j2".into(),
            required_capabilities: vec![],
            cascade_level: 0,
            matroshka_limit: 7,
        },
    );
    let req: SkillExecuteRequest = serde_json::from_value(serde_json::json!({
        "skill_id": "test.step",
        "context": ["not", "an", "object"]
    }))
    .expect("deserialize SkillExecuteRequest");
    let out = server.skill_execute(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for non-object context");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}

// REQ: skill_execute reports internal error when the template file is missing (P5).
// expect: a registered skill whose source_path does not exist returns kind=internal.
#[tokio::test]
async fn skill_execute_internal_error_when_template_missing_via_parameters_seam() {
    let mut server = SkillServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(NoopInferencePort),
        HashMap::new(),
        CapabilityTier::detect(&HashMap::new()),
    );
    // Register a skill pointing at a path that does not exist on disk.
    let dir = tempdir().expect("tempdir");
    let missing_path: PathBuf = dir.path().join("does_not_exist.j2");
    server.skills.insert(
        "missing.template".to_string(),
        hkask_types::RegistryEntry {
            id: "missing/template".into(),
            template_type: hkask_types::template_type::TemplateType::WordAct,
            name: "template".into(),
            lexicon_terms: vec![],
            description: "a missing template".into(),
            source_path: missing_path.to_string_lossy().to_string(),
            required_capabilities: vec![],
            cascade_level: 0,
            matroshka_limit: 7,
        },
    );
    let req: SkillExecuteRequest = serde_json::from_value(serde_json::json!({
        "skill_id": "missing.template",
        "context": {}
    }))
    .expect("deserialize SkillExecuteRequest");
    let out = server.skill_execute(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for missing template");
    assert_eq!(kind, "internal", "got: {out}");
}
