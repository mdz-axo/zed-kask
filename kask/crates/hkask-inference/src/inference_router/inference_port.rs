//! InferencePort trait implementation — canonical route-to-backend dispatch.

use super::InferenceRouter;
use crate::chat_protocol::validate_prompt;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
    InferenceStreamChunk,
};
use std::pin::Pin;

impl InferenceRouter {
    pub(super) fn generate_ungoverned(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        if !parameters.bypass_fusion && model_override.is_none() {
            let fusion = parameters
                .fusion_config
                .clone()
                .or_else(|| self.config.fusion.clone());
            if let Some(fusion) = fusion {
                let prompt = prompt.to_string();
                let parameters = parameters.clone();
                let tools = tools.map(<[ChatToolDefinition]>::to_vec);
                return Box::pin(async move {
                    validate_prompt(&prompt)?;
                    self.orchestrate_fusion(&prompt, &parameters, tools.as_deref(), &fusion)
                        .await
                        .map_err(|error| self.heal_error(error, "generate_with_model"))
                });
            }
        }

        if let Some(adapter) = &parameters.adapter {
            let (provider, model) = match self.resolve_chat(adapter) {
                Ok(route) => route,
                Err(error) => {
                    return Box::pin(
                        async move { Err(self.heal_error(error, "generate_with_model")) },
                    );
                }
            };
            let model = model.to_string();
            let prompt = prompt.to_string();
            let parameters = parameters.clone();
            let tools = tools.map(<[ChatToolDefinition]>::to_vec);
            return Box::pin(async move {
                validate_prompt(&prompt)?;
                self.dispatch_generate(provider, &model, &prompt, &parameters, tools.as_deref())
                    .await
                    .map_err(|error| self.heal_error(error, "generate_with_model"))
            });
        }

        let model_name = self.effective_model(model_override, parameters);
        let (provider, model) = match self.resolve_chat(&model_name) {
            Ok(route) => route,
            Err(error) => {
                return Box::pin(async move { Err(self.heal_error(error, "generate_with_model")) });
            }
        };
        let model = model.to_string();
        let prompt = prompt.to_string();
        let parameters = parameters.clone();
        let tools = tools.map(<[ChatToolDefinition]>::to_vec);

        Box::pin(async move {
            validate_prompt(&prompt)?;
            self.dispatch_generate(provider, &model, &prompt, &parameters, tools.as_deref())
                .await
                .map_err(|error| self.heal_error(error, "generate_with_model"))
        })
    }

    /// Multi-turn variant of `generate_ungoverned` — accepts an explicit message
    /// array instead of a single prompt string. Fusion and adapter paths flatten
    /// messages to a string (they are prompt-based internally); the default path
    /// dispatches the message array directly to the backend.
    pub(super) fn generate_ungoverned_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        // Fusion is prompt-based internally — flatten messages when fusion is active.
        if !parameters.bypass_fusion && model_override.is_none() {
            let fusion = parameters
                .fusion_config
                .clone()
                .or_else(|| self.config.fusion.clone());
            if let Some(fusion) = fusion {
                let prompt = messages
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.content))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let parameters = parameters.clone();
                let tools = tools.map(<[ChatToolDefinition]>::to_vec);
                return Box::pin(async move {
                    validate_prompt(&prompt)?;
                    self.orchestrate_fusion(&prompt, &parameters, tools.as_deref(), &fusion)
                        .await
                        .map_err(|error| self.heal_error(error, "generate_with_messages"))
                });
            }
        }

        // LoRA adapter path — flatten messages (adapter dispatch is prompt-based).
        if let Some(adapter) = &parameters.adapter {
            let prompt = messages
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            let (provider, model) = match self.resolve_chat(adapter) {
                Ok(route) => route,
                Err(error) => {
                    return Box::pin(async move {
                        Err(self.heal_error(error, "generate_with_messages"))
                    });
                }
            };
            let model = model.to_string();
            let prompt = prompt.to_string();
            let parameters = parameters.clone();
            let tools = tools.map(<[ChatToolDefinition]>::to_vec);
            return Box::pin(async move {
                validate_prompt(&prompt)?;
                self.dispatch_generate(provider, &model, &prompt, &parameters, tools.as_deref())
                    .await
                    .map_err(|error| self.heal_error(error, "generate_with_messages"))
            });
        }

        // Default path — dispatch the message array directly to the backend.
        let model_name = self.effective_model(model_override, parameters);
        let (provider, model) = match self.resolve_chat(&model_name) {
            Ok(route) => route,
            Err(error) => {
                return Box::pin(
                    async move { Err(self.heal_error(error, "generate_with_messages")) },
                );
            }
        };
        let model = model.to_string();
        let messages = messages.to_vec();
        let parameters = parameters.clone();
        let tools = tools.map(<[ChatToolDefinition]>::to_vec);

        Box::pin(async move {
            self.dispatch_generate_messages(
                provider,
                &model,
                &messages,
                &parameters,
                tools.as_deref(),
            )
            .await
            .map_err(|error| self.heal_error(error, "generate_with_messages"))
        })
    }
}

impl InferencePort for InferenceRouter {
    // pre:  prompt is non-empty; parameters are valid
    // post: Ok(InferenceResult) when resolved provider backend is configured;
    //       Err(Connection) when resolved provider backend is None
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        self.generate_with_model(prompt, parameters, None, tools)
    }

    // pre:  prompt is non-empty; parameters are valid; model_override may be None
    // post: Ok(InferenceResult) when resolved provider backend is configured;
    //       Err(Connection) when resolved provider backend is None
    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let Some(governance) = self.governance.clone() else {
            return self.generate_ungoverned(prompt, parameters, model_override, tools);
        };
        let prompt = prompt.to_string();
        let parameters = parameters.clone();
        let model_override = model_override.map(str::to_string);
        let tools = tools.map(<[ChatToolDefinition]>::to_vec);
        Box::pin(async move {
            self.generate_governed(governance, prompt, parameters, model_override, tools)
                .await
        })
    }

    // pre:  messages is non-empty; parameters are valid; model_override may be None
    // post: Ok(InferenceResult) when resolved provider backend is configured;
    //       Err(Connection) when resolved provider backend is None
    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let Some(governance) = self.governance.clone() else {
            return self.generate_ungoverned_messages(messages, parameters, model_override, tools);
        };
        let messages = messages.to_vec();
        let parameters = parameters.clone();
        let model_override = model_override.map(str::to_string);
        let tools = tools.map(<[ChatToolDefinition]>::to_vec);
        Box::pin(async move {
            self.generate_governed_messages(governance, messages, parameters, model_override, tools)
                .await
        })
    }

    // pre:  prompt is non-empty; parameters are valid
    // post: Stream of Ok(InferenceStreamChunk) when resolved provider backend is configured;
    //       Stream of Err(Connection) when resolved provider backend is None
    fn generate_stream(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        self.generate_stream_with_model(prompt, parameters, None, tools)
    }

    // pre:  prompt is non-empty; parameters are valid; model_override may be None
    // post: Stream of Ok(InferenceStreamChunk) when resolved provider backend is configured;
    //       Stream of Err(Connection) when resolved provider backend is None
    fn generate_stream_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        // Fusion (multi-model deliberation) is non-streamable at the token
        // level: the orchestrator dispatches a panel in parallel and a judge
        // synthesizes a single InferenceResult — there is no token stream to
        // emit. When fusion is active and not bypassed, run the full fusion and
        // emit the result as a single stream chunk. This preserves the caller's
        // stream interface (non-breaking: ACP/API/CLI streaming callers keep
        // working) while delivering the fused answer the caller enabled fusion
        // for. The latency is inherent to fusion (multi-round panel+judge), not
        // to this path. Priority: per-call fusion_config > global config (same
        // as `generate`).
        if !parameters.bypass_fusion {
            let fusion = parameters
                .fusion_config
                .clone()
                .or_else(|| self.config.fusion.clone());
            if let Some(fusion) = fusion {
                let prompt = prompt.to_string();
                let parameters = parameters.clone();
                let tools = tools.map(|t| t.to_vec());
                return Box::pin(futures_util::stream::once(async move {
                    validate_prompt(&prompt)?;
                    let result = self
                        .orchestrate_fusion(&prompt, &parameters, tools.as_deref(), &fusion)
                        .await
                        .map_err(|e| self.heal_error(e, "generate_stream_with_model"))?;
                    Ok(InferenceStreamChunk {
                        text_delta: result.text,
                        reasoning_delta: result.reasoning.unwrap_or_default(),
                        model: result.model,
                        finish_reason: Some(result.finish_reason),
                        usage: Some(result.usage),
                        tool_calls: result.tool_calls,
                    })
                }));
            }
        }

        // LoRA adapter overrides the model entirely (bypasses fusion).
        if let Some(ref adapter) = parameters.adapter {
            let adapter_str = adapter.to_string();
            let (provider, model) = match self.resolve_chat(&adapter_str) {
                Ok(r) => r,
                Err(e) => {
                    return Box::pin(futures_util::stream::once(async move { Err(e) }));
                }
            };
            let model = model.to_string();
            let prompt = prompt.to_string();
            let parameters = parameters.clone();
            let tools = tools.map(|t| t.to_vec());
            return self.dispatch_generate_stream(
                provider,
                &model,
                &prompt,
                &parameters,
                tools.as_deref(),
            );
        }

        let model_name = self.effective_model(model_override, parameters);
        let (provider, model) = match self.resolve_chat(&model_name) {
            Ok(r) => r,
            Err(e) => {
                return Box::pin(futures_util::stream::once(async move { Err(e) }));
            }
        };
        let model = model.to_string();
        let prompt = prompt.to_string();
        let parameters = parameters.clone();
        let tools = tools.map(|t| t.to_vec());

        self.dispatch_generate_stream(provider, &model, &prompt, &parameters, tools.as_deref())
    }

    fn generate_vision(
        &self,
        prompt: &str,
        images: &[String],
        parameters: &LLMParameters,
        model_override: Option<&str>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let prompt = prompt.to_string();
        let images = images.to_vec();
        let parameters = parameters.clone();
        let model_override = model_override.map(|s| s.to_string());
        Box::pin(async move {
            self.generate_vision(&prompt, &images, &parameters, model_override.as_deref())
                .await
        })
    }
}
