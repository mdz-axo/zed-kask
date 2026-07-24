//! `InferencePort` adapter over zed's `LanguageModel`.
//!
//! hKask's `InferencePort` is non-streaming (`generate() -> InferenceResult`).
//! Zed's `LanguageModel` streams (`stream_completion() -> BoxStream<CompletionEvent>`).
//! This adapter collects the stream into a single `InferenceResult`, mapping the
//! event types. Streaming is lost in this adapter — that's acceptable for the
//! ManifestExecutor cascade (which needs complete results for PDCA convergence),
//! and for MCP servers that already use the non-streaming `InferenceRouter`.
//!
//! `AsyncApp` is not `Send` (GPUI's `ForegroundExecutor` holds `Rc`-based state),
//! so the bridge uses a channel: trait methods send a request to a GPUI-side task
//! that holds the `AsyncApp` and executes the streaming completion, collecting the
//! result and sending it back. The adapter struct itself only holds a channel
//! sender (`Send + Sync`).

use std::sync::Arc;

use futures_util::{FutureExt, StreamExt};
use gpui::AsyncApp;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
    InferenceUsage, StructuredToolCall,
};
use language_model::LanguageModel;
use language_model_core::{
    LanguageModelCompletionEvent, LanguageModelRequest, LanguageModelRequestMessage,
    LanguageModelRequestTool, LanguageModelToolChoice, LanguageModelToolUseInput, MessageContent,
    Role, StopReason,
};
use tokio::sync::oneshot;

/// Request sent from the tokio side (trait method) to the GPUI side (executor).
struct InferenceRequest {
    request: LanguageModelRequest,
    reply: oneshot::Sender<Result<InferenceResult, InferenceError>>,
}

/// `InferencePort` implementation over zed's `LanguageModel`.
///
/// Collects the streaming completion into a single `InferenceResult`.
/// The model is selected at construction time — one adapter instance per model.
///
/// The adapter holds only a channel sender (`Send + Sync`); the actual inference
/// call happens on the GPUI side via a spawned task that owns the `AsyncApp`.
pub struct LanguageModelInferencePort {
    tx: tokio::sync::mpsc::UnboundedSender<InferenceRequest>,
}

impl LanguageModelInferencePort {
    /// Construct the adapter and spawn the GPUI-side receiver task.
    ///
    /// The receiver task runs on the GPUI foreground executor and processes
    /// inference requests. Drop the returned `Task` to stop it.
    pub fn new(model: Arc<dyn LanguageModel>, cx: AsyncApp) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let model_for_task = model.clone();

        let task = cx.spawn(async move |cx| {
            while let Some(req) = rx.recv().await {
                let model = model_for_task.clone();
                let cx = cx.clone();
                // Run on the foreground executor — stream_completion needs &AsyncApp
                // which is not Send, so it can't go to background_spawn.
                let result = async move {
                    let stream_result = model
                        .stream_completion(req.request, &cx)
                        .await
                        .map_err(|e| InferenceError::Connection(e.to_string()));

                    match stream_result {
                        Err(e) => Err(e),
                        Ok(mut stream) => {
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
                                    Ok(LanguageModelCompletionEvent::Thinking {
                                        text: thinking,
                                        ..
                                    }) => {
                                        reasoning.push_str(&thinking);
                                    }
                                    Ok(LanguageModelCompletionEvent::ToolUse(tool_use))
                                        if tool_use.is_input_complete =>
                                    {
                                        let args = match &tool_use.input {
                                            LanguageModelToolUseInput::Json(json) => json.clone(),
                                            LanguageModelToolUseInput::Text(text) => {
                                                serde_json::from_str(text)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }
                                        };
                                        tool_calls.push(StructuredToolCall {
                                            server: tool_use.name.to_string(),
                                            tool: tool_use.name.to_string(),
                                            args,
                                            call_id: Some(tool_use.id.to_string()),
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
                                            total_tokens: (token_usage.input_tokens
                                                + token_usage.output_tokens)
                                                as u32,
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
                                model: model.name().0.to_string(),
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
                }
                .await;

                let _ = req.reply.send(result);
            }
        });

        (Self { tx }, task)
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
        let (tx_reply, rx_reply) = oneshot::channel();
        async move {
            self.tx
                .send(InferenceRequest {
                    request,
                    reply: tx_reply,
                })
                .map_err(|e| InferenceError::Connection(e.to_string()))?;
            rx_reply
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()))?
        }
        .boxed()
    }
}
