//! Offline pins of the dedicated generator at both public QA tool boundaries.
use super::{GenerateQaBatchRequest, GenerateQaRequest};
use crate::CorpusServer;
use hkask_inference::model_constants::QA_GENERATION_MODEL_ENV;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult, McpErrorKind,
};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

type Reply<'a> = Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>>;
const MODEL: &str = "OpenRouter/~openai/gpt-sol-latest";

#[derive(Default)]
struct RecordingPort(Mutex<Vec<String>>);

impl RecordingPort {
    fn record(&self, parameters: &LLMParameters, model: Option<&str>) -> Reply<'_> {
        assert!(!parameters.thinking_allowed, "QA must disable thinking");
        assert_eq!(
            model,
            Some(MODEL),
            "the incompatible chat model must never be selected"
        );
        self.0
            .lock()
            .expect("record model")
            .push(model.expect("explicit model").into());
        Box::pin(async {
            Ok(InferenceResult {
            text: json!({"qa_pairs":[{"question":"What is stated?","answer":"The source states a fact.","bloom_level":"factual","evidence_quotes":[]}]}).to_string(),
            model: MODEL.into(), usage: Default::default(), finish_reason: "stop".into(),
            tool_calls: vec![], reasoning: None, cost_usd: None,
        })
        })
    }
}

impl InferencePort for RecordingPort {
    fn generate(&self, _: &str, _: &LLMParameters, _: Option<&[ChatToolDefinition]>) -> Reply<'_> {
        Box::pin(async {
            Err(InferenceError::Model(
                "active chat requires reasoning; QA must not call it".into(),
            ))
        })
    }

    fn generate_with_model(
        &self,
        _: &str,
        parameters: &LLMParameters,
        model: Option<&str>,
        _: Option<&[ChatToolDefinition]>,
    ) -> Reply<'_> {
        self.record(parameters, model)
    }

    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model: Option<&str>,
        _: Option<&[ChatToolDefinition]>,
    ) -> Reply<'_> {
        assert_eq!(messages.len(), 2);
        self.record(parameters, model)
    }
}

/// Requirement: both public tools resolve override > dedicated setting, never
/// the incompatible default/chat/base model, and refuse invalid input visibly.
#[tokio::test]
async fn qa_generator_routing_is_independent_of_chat() -> anyhow::Result<()> {
    const CASE: &str = "HKASK_QA_ROUTING_TEST_CASE";
    if let Ok(case) = std::env::var(CASE) {
        let port = Arc::new(RecordingPort::default());
        let router: Arc<dyn InferencePort> = port.clone();
        let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(router.clone()));
        let server =
            CorpusServer::new(hkask_types::WebID::new(), None, router, Arc::default(), ocr);
        let model = match case.as_str() {
            "override" => Some(MODEL.into()),
            "invalid-override" => Some("".into()),
            _ => None,
        };
        let single = server
            .corpus_generate_qa(Parameters(GenerateQaRequest {
                text: Some("The source states a fact.".into()),
                texts: None,
                chunk_id: "source-1".into(),
                bloom_levels: Some(vec!["factual".into()]),
                model: model.clone(),
            }))
            .await;
        let prompt = json!({"prompt_id":"prompt-1", "chunk_ref":"source-1", "source":"source.txt",
            "concepts":[], "salience":0.5, "qa_type":"factual", "system":"Generate grounded QA.", "user":"The source states a fact."});
        std::fs::write("prompts.jsonl", format!("{prompt}\n"))?;
        let batch = server
            .corpus_generate_qa_batch(Parameters(GenerateQaBatchRequest {
                prompts_jsonl: "prompts.jsonl".into(),
                output: "output.jsonl".into(),
                concurrency: 1,
                model,
            }))
            .await;
        if matches!(case.as_str(), "configured" | "override") {
            let single = hkask_types::tool_response::parse_tool_response(&single?)
                .expect("single tool envelope");
            assert_eq!(single["provenance"]["generator_model"], MODEL);
            let batch = hkask_types::tool_response::parse_tool_response(&batch?)
                .expect("batch tool envelope");
            assert_eq!(batch["prompts_succeeded"], 1);
            assert_eq!(*port.0.lock().expect("calls"), vec![MODEL, MODEL]);
            let row: serde_json::Value =
                serde_json::from_str(std::fs::read_to_string("output.jsonl")?.trim())?;
            assert_eq!(row["provenance"]["generator_model"], MODEL);
        } else {
            let kind = if case == "missing" {
                McpErrorKind::PermissionDenied
            } else {
                McpErrorKind::InvalidArgument
            };
            for result in [single, batch] {
                let error = result.expect_err("must fail before inference");
                assert_eq!(error.kind, kind);
                assert!(error.message.contains("qa_generation_model"), "{error:?}");
            }
            assert!(port.0.lock().expect("calls").is_empty());
            assert!(
                !std::path::Path::new("output.jsonl").exists(),
                "invalid configuration must not truncate output"
            );
        }
        return Ok(());
    }
    for case in [
        "missing",
        "blank",
        "invalid",
        "configured",
        "override",
        "invalid-override",
    ] {
        let directory = tempfile::tempdir()?;
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command
            .args([
                "--exact",
                "tools::semantic::qa_model_tests::qa_generator_routing_is_independent_of_chat",
            ])
            .env_clear()
            .env(CASE, case)
            .env("HOME", directory.path())
            .env("HKASK_DATA_DIR", directory.path())
            .env("HKASK_ARTIFACTS_DIR", directory.path())
            .env("HKASK_DEFAULT_MODEL", "OpenRouter/z-ai/glm-5.3-flash")
            .env("HKASK_QA_MODEL", "OpenRouter/z-ai/glm-5.3-flash")
            .current_dir(directory.path());
        match case {
            "blank" => {
                command.env(QA_GENERATION_MODEL_ENV, "");
            }
            "invalid" | "override" => {
                command.env(QA_GENERATION_MODEL_ENV, "not-qualified");
            }
            "configured" | "invalid-override" => {
                command.env(QA_GENERATION_MODEL_ENV, MODEL);
            }
            _ => {}
        }
        let output = command.output().await?;
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
