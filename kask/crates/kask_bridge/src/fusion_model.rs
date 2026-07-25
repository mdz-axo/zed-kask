//! `FusionLanguageModel` — a zed `LanguageModel` that delegates to hKask's
//! fusion orchestrator.
//!
//! This is the bridge that makes fusion reachable from zed's agent thread
//! (Slice 2). The agent thread calls `model.stream_completion(request, cx)`
//! on a single `Arc<dyn LanguageModel>`. When that model is a
//! `FusionLanguageModel`, the request is routed through hKask's
//! `fusion_orchestrator::orchestrate`, which dispatches to a panel of models
//! in parallel and fuses their outputs via a judge (LLM or algorithmic).
//!
//! ## Architecture
//!
//! ```text
//! agent thread
//!     │
//!     ▼
//! FusionLanguageModel.stream_completion(request, cx)
//!     │
//!     │  flatten messages → prompt string
//!     │  build LLMParameters with fusion_config
//!     │
//!     ▼
//! fusion_orchestrator::orchestrate(router, prompt, params, tools, fusion)
//!     │
//!     │  dispatch_panel: parallel calls to each panel model
//!     │  call_judge: fuse panel responses
//!     │
//!     ▼
//! MultiModelInferencePort (routes generate_with_model by name)
//!     │
//!     │  resolve model name → LanguageModelInferencePort
//!     │  (each port holds a channel to a GPUI-side task)
//!     │
//!     ▼
//! zed's LanguageModel providers (Anthropic, OpenAI, Ollama, etc.)
//! ```
//!
//! ## Streaming
//!
//! Fusion is non-streaming (the orchestrator collects complete panel
//! responses before fusing). The fused result is emitted as a single
//! `Text` chunk followed by a `Stop(EndTurn)`. This is acceptable for the
//! manifest cascade (which needs complete results for PDCA convergence)
//! and for the kask panel. For the regular chat path, the user sees a
//! brief delay while the panel runs in parallel, then the fused output
//! appears at once.
//!
//! ## Send/Sync
//!
//! `LanguageModel: Send + Sync`, but `AsyncApp` is not `Send` (GPUI's
//! `Rc`-based state). Following the same pattern as `LanguageModelInferencePort`,
//! the `AsyncApp` is moved into a GPUI-side channel task at construction
//! time. The `FusionLanguageModel` holds only `LanguageModelInferencePort`
//! instances (which hold `Send + Sync` channel senders), not `AsyncApp`.

use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::BoxStream;
use futures_util::{FutureExt, StreamExt, stream};
use gpui::{App, AsyncApp};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
};
use language_model::LanguageModel;
use language_model_core::{
    LanguageModelCompletionEvent, LanguageModelId, LanguageModelName, LanguageModelProviderId,
    LanguageModelProviderName, LanguageModelRequest, LanguageModelRequestToolInput,
    LanguageModelToolChoice, MessageContent, Role, StopReason, TokenUsage,
};

use crate::inference::LanguageModelInferencePort;

/// The provider ID under which fusion models are registered.
pub const FUSION_PROVIDER_ID: &str = "kask-fusion";

/// The model ID for the default fusion model.
pub const FUSION_MODEL_ID: &str = "fusion";

/// A zed `LanguageModel` that delegates to hKask's fusion orchestrator.
///
/// Constructed by the composition root when `kask.fusion.enabled == true`.
/// The agent thread treats it as a regular `LanguageModel` — it has no
/// knowledge of fusion.
pub struct FusionLanguageModel {
    id: LanguageModelId,
    name: LanguageModelName,
    config: hkask_types::fusion::FusionConfig,
    /// Pre-constructed inference ports, keyed by provider-prefixed model name.
    /// Each port holds a `Send + Sync` channel sender to a GPUI-side task.
    ports: HashMap<String, Arc<LanguageModelInferencePort>>,
}

impl FusionLanguageModel {
    /// Construct a fusion model from a `FusionConfig`, a map of resolved
    /// models, and the GPUI async context.
    ///
    /// The `models` map keys must match the names in `config.panel` and
    /// `config.judge`. The composition root is responsible for resolving
    /// provider-prefixed names to `Arc<dyn LanguageModel>` instances from
    /// the `LanguageModelRegistry`.
    ///
    /// For each resolved model, a `LanguageModelInferencePort` is constructed
    /// (spawning a GPUI-side channel task). The tasks are detached — they
    /// live until the ports are dropped.
    ///
    /// Returns `None` if the judge model is missing (and judge != "algo"),
    /// or if no panel models are present.
    #[must_use]
    pub fn new(
        config: hkask_types::fusion::FusionConfig,
        models: HashMap<String, Arc<dyn LanguageModel>>,
        cx: AsyncApp,
    ) -> Option<Self> {
        let is_algo = config.judge.to_lowercase() == "algo";
        if !is_algo && !models.contains_key(&config.judge) {
            tracing::warn!(
                target: "reg.fusion",
                judge = %config.judge,
                "Fusion judge model not in resolved set — fusion disabled"
            );
            return None;
        }

        // Construct a LanguageModelInferencePort for each resolved model.
        let mut ports: HashMap<String, Arc<LanguageModelInferencePort>> = HashMap::new();
        for (name, model) in &models {
            let (port, task) = LanguageModelInferencePort::new(model.clone(), cx.clone());
            task.detach();
            ports.insert(name.clone(), Arc::new(port));
        }

        // Check that at least one panel model is present.
        let panel_present = config.panel.iter().any(|name| ports.contains_key(name));
        if !panel_present {
            tracing::warn!(
                target: "reg.fusion",
                "No panel models resolved — fusion disabled"
            );
            return None;
        }

        let mode_str = config.mode.as_str();
        Some(Self {
            id: LanguageModelId::from(FUSION_MODEL_ID.to_string()),
            name: LanguageModelName::from(format!(
                "Fusion ({mode_str}, {} panelists)",
                config.panel.len()
            )),
            config,
            ports,
        })
    }

    /// Build the multi-model `InferencePort` adapter that the fusion
    /// orchestrator will call.
    fn build_router(&self) -> MultiModelInferencePort {
        MultiModelInferencePort {
            ports: self.ports.clone(),
        }
    }

    /// Flatten a `LanguageModelRequest` into the prompt + tools that the
    /// fusion orchestrator expects.
    ///
    /// Fusion is prompt-based internally (see `fusion_orchestrator::dispatch_panel`
    /// and `call_judge`). The orchestrator sends a single prompt string to
    /// each panel model, not a message array. We flatten the messages into
    /// a single prompt with role prefixes.
    fn flatten_request(request: &LanguageModelRequest) -> (String, Vec<ChatToolDefinition>) {
        let prompt = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::Assistant => "assistant",
                    Role::User => "user",
                };
                let content = m
                    .content
                    .iter()
                    .map(|c| match c {
                        MessageContent::Text(text) => text.as_str(),
                        _ => "",
                    })
                    .collect::<Vec<_>>()
                    .join("");
                format!("{role}: {content}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let tools: Vec<ChatToolDefinition> = request
            .tools
            .iter()
            .map(|t| ChatToolDefinition {
                tool_type: "function".to_string(),
                function: hkask_types::ChatToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: match &t.input {
                        LanguageModelRequestToolInput::Function { input_schema, .. } => {
                            input_schema.clone()
                        }
                        LanguageModelRequestToolInput::Custom { .. } => serde_json::Value::Null,
                    },
                },
            })
            .collect();

        (prompt, tools)
    }
}

impl LanguageModel for FusionLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        self.name.clone()
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        LanguageModelProviderId(FUSION_PROVIDER_ID.into())
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        LanguageModelProviderName("Kask Fusion".into())
    }

    fn telemetry_id(&self) -> String {
        format!("kask-fusion/{}", self.config.mode.as_str())
    }

    fn supports_images(&self) -> bool {
        // Fusion flattens messages to text — images are dropped.
        false
    }

    fn supports_tools(&self) -> bool {
        // The fusion orchestrator passes tools through to panel models.
        // Whether tools actually work depends on the panel models.
        true
    }

    fn supports_tool_choice(&self, _choice: LanguageModelToolChoice) -> bool {
        false
    }

    fn max_token_count(&self) -> u64 {
        // Use the minimum of all panel models' max token counts, since the
        // prompt goes to all of them. Fall back to a large default.
        self.ports
            .values()
            .map(|_| 128_000u64) // LanguageModelInferencePort doesn't expose max_token_count
            .min()
            .unwrap_or(128_000)
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        _cx: &AsyncApp,
    ) -> futures::future::BoxFuture<
        'static,
        Result<
            BoxStream<
                'static,
                Result<
                    LanguageModelCompletionEvent,
                    language_model_core::LanguageModelCompletionError,
                >,
            >,
            language_model_core::LanguageModelCompletionError,
        >,
    > {
        let config = self.config.clone();
        let router = self.build_router();
        let (prompt, tools) = Self::flatten_request(&request);
        let temperature = request.temperature;

        async move {
            let params = LLMParameters {
                temperature: temperature.unwrap_or(0.7),
                ..Default::default()
            };

            let result = hkask_inference::fusion_orchestrator::orchestrate(
                &router,
                &prompt,
                &params,
                if tools.is_empty() { None } else { Some(&tools) },
                &config,
            )
            .await
            .map_err(|e| {
                language_model_core::LanguageModelCompletionError::Other(anyhow::anyhow!(
                    "Fusion orchestration failed: {e}"
                ))
            })?;

            // Emit the fused result as a single Text chunk + Stop.
            let stream = stream::iter([
                Ok(LanguageModelCompletionEvent::StartMessage {
                    message_id: "fusion".to_string(),
                }),
                Ok(LanguageModelCompletionEvent::Text(result.text)),
                Ok(LanguageModelCompletionEvent::UsageUpdate(TokenUsage {
                    input_tokens: result.usage.prompt_tokens as u64,
                    output_tokens: result.usage.completion_tokens as u64,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                })),
                Ok(LanguageModelCompletionEvent::Stop(StopReason::EndTurn)),
            ])
            .boxed();

            Ok(stream)
        }
        .boxed()
    }
}

// ── MultiModelInferencePort ──────────────────────────────────────────────────

/// An `InferencePort` that routes `generate_with_model` to different
/// `LanguageModelInferencePort` instances based on the model name.
///
/// This is the adapter that lets hKask's fusion orchestrator (which uses
/// provider-prefixed model names) talk to zed's `LanguageModel` instances.
struct MultiModelInferencePort {
    ports: HashMap<String, Arc<LanguageModelInferencePort>>,
}

impl InferencePort for MultiModelInferencePort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        // No model override — use the first available port.
        let port = self.ports.values().next();
        match port {
            Some(p) => p.generate(prompt, parameters, tools),
            None => Box::pin(async {
                Err(InferenceError::Generation(
                    "No models available in fusion router".into(),
                ))
            }),
        }
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let model_name = match model_override {
            Some(name) => name,
            None => {
                return self.generate(prompt, parameters, tools);
            }
        };

        match self.ports.get(model_name) {
            // LanguageModelInferencePort ignores model_override (each port is
            // bound to a single model at construction), so we pass None.
            Some(port) => port.generate_with_model(prompt, parameters, None, tools),
            None => {
                let model_name = model_name.to_string();
                Box::pin(async move {
                    Err(InferenceError::Generation(format!(
                        "Model '{model_name}' not found in fusion router"
                    )))
                })
            }
        }
    }

    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let model_name = match model_override {
            Some(name) => name,
            None => {
                // Flatten and use the first port.
                let port = match self.ports.values().next() {
                    Some(p) => p,
                    None => {
                        return Box::pin(async {
                            Err(InferenceError::Generation(
                                "No models available in fusion router".into(),
                            ))
                        });
                    }
                };
                return port.generate_with_messages(messages, parameters, None, tools);
            }
        };

        match self.ports.get(model_name) {
            // LanguageModelInferencePort ignores model_override.
            Some(port) => port.generate_with_messages(messages, parameters, None, tools),
            None => {
                let model_name = model_name.to_string();
                Box::pin(async move {
                    Err(InferenceError::Generation(format!(
                        "Model '{model_name}' not found in fusion router"
                    )))
                })
            }
        }
    }
}
