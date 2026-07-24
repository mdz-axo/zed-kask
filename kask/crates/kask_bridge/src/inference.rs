//! `InferencePort` adapter over zed's `LanguageModel`.
//!
//! hKask's `InferencePort` is non-streaming (`generate() -> InferenceResult`).
//! Zed's `LanguageModel` streams (`stream_completion() -> BoxStream<CompletionEvent>`).
//! This adapter collects the stream into a single `InferenceResult`, mapping the
//! event types. Streaming is lost in this adapter — that's acceptable for the
//! ManifestExecutor cascade (which needs complete results for PDCA convergence),
//! and for MCP servers that already use the non-streaming `InferenceRouter`.
//!
//! For zed-kask's direct chat (which needs streaming UX), the guard layer is
//! applied separately (D4) — see the plan §10 R3 for the streaming guard strategy.

use std::sync::Arc;

use futures_util::{FutureExt, StreamExt};
use gpui::AsyncApp;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
    InferenceUsage, StructuredToolCall,
};
use language_model::LanguageModel;
use language_model_core::language_model_core::{
    LanguageModelCompletionEvent, LanguageModelToolUseInput, StopReason,
};
use language_model_core::request::{
    LanguageModelRequest, LanguageModelRequestMessage, LanguageModelRequestTool,
    LanguageModelToolChoice, MessageContent, Role,
};

/// `InferencePort` implementation over zed's `LanguageModel`.
///
/// Collects the streaming completion into a single `InferenceResult`.
/// The model is selected at construction time — one adapter instance per model.
///
/// `AsyncApp` is `Send` but not `Sync` (it holds a GPUI `ForegroundExecutor`
/// with `Rc`-based single-threaded state). We wrap it in `tokio::sync::Mutex`,
/// which is `Sync` when `T: Send`, so the adapter satisfies `InferencePort: Send + Sync`.
pub struct LanguageModelInferencePort {
    model: Arc<dyn LanguageModel>,
    cx: tokio::sync::Mutex<AsyncApp>,
}

impl LanguageModelInferencePort {
    pub fn new(model: Arc<dyn LanguageModel>, cx: AsyncApp) -> Self {
        Self {
            model,
            cx: tokio::sync::Mutex::new(cx),
        }
    }

    fn build_request(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> LanguageModelRequest {
        let req_messages: Vec<LanguageModelRequestMessage> = messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    "user" | _ => Role::User,
                };
                LanguageModelRequestMessage {
                    role,
                    content: vec![MessageContent::Text(m.content.clone())],
                    cache: false,
                    reasoning_details: None,
                }
            })
            .collect();

        let req_tools: Vec<LanguageModelRequestTool> = tools
            .unwrap_or(&[])
            .iter()
            .map(|t| {
                LanguageModelRequestTool::function(
                    t.function.name.clone(),
                    t.function.description.clone(),
                    t.function.parameters.clone(),
                    false,
                )
            })
            .collect();

        LanguageModelRequest {
            messages: req_messages,
            tools: req_tools,
            temperature: Some(parameters.temperature),
            tool_choice: if tools.is_some() {
                Some(LanguageModelToolChoice::Auto)
            } else {
                None
            },
            ..Default::default()
        }
    }

    async fn run_completion(
        &self,
        request: LanguageModelRequest,
    ) -> Result<InferenceResult, InferenceError> {
        let cx = self.cx.lock().await.clone();
        let stream_result = self
            .model
            .stream_completion(request, &cx)
            .await
            .map_err(|e| InferenceError::Connection(e.to_string()))?;

        let mut stream = stream_result;
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = "stop".to_string();
        let mut usage = InferenceUsage::default();

        while let Some(event) = stream.next().await {
            match event {
                Ok(LanguageModelCompletionEvent::Text(delta)) => {
                    text.push_str(&delta);
                }
                Ok(LanguageModelCompletionEvent::Thinking { thinking, .. }) => {
                    reasoning.push_str(&thinking);
                }
                Ok(LanguageModelCompletionEvent::ToolUse(tool_use))
                    if tool_use.is_input_complete =>
                {
                    let args = match &tool_use.input {
                        LanguageModelToolUseInput::Json(json) => json.clone(),
                        LanguageModelToolUseInput::Text(text) => {
                            serde_json::from_str(text).unwrap_or(serde_json::Value::Null)
                        }
                    };
                    tool_calls.push(StructuredToolCall {
                        server: tool_use.name.to_string(),
                        tool: tool_use.name.to_string(),
                        args,
                        call_id: Some(tool_use.id.0.to_string()),
                    });
                }
                Ok(LanguageModelCompletionEvent::Stop(reason)) => {
                    finish_reason = match reason {
                        StopReason::EndTurn => "stop",
                        StopReason::MaxTokens => "length",
                        StopReason::ToolUse => "tool_calls",
                        StopReason::Refusal => "refusal",
                    }
                    .to_string();
                }
                Ok(LanguageModelCompletionEvent::UsageUpdate(token_usage)) => {
                    usage = InferenceUsage {
                        prompt_tokens: token_usage.input_tokens as u32,
                        completion_tokens: token_usage.output_tokens as u32,
                        total_tokens: (token_usage.input_tokens + token_usage.output_tokens) as u32,
                    };
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(InferenceError::Generation(e.to_string()));
                }
            }
        }

        Ok(InferenceResult {
            text,
            model: self.model.name().0.to_string(),
            usage,
            finish_reason,
            token_probabilities: None,
            tool_calls,
            reasoning: if reasoning.is_empty() {
                None
            } else {
                Some(reasoning)
            },
        })
    }
}

impl InferencePort for LanguageModelInferencePort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let messages = vec![ChatMessage::user(prompt.to_string())];
        self.generate_with_messages(&messages, parameters, None, tools)
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        _model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let messages = vec![ChatMessage::user(prompt.to_string())];
        self.generate_with_messages(&messages, parameters, None, tools)
    }

    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        _model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let request = self.build_request(messages, parameters, tools);
        self.run_completion(request).boxed()
    }
}
