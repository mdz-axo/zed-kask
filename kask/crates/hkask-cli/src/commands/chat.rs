//! Chat inference helpers — used by `tui.rs` for non-interactive mode (`kask tui -f`).
//!
//! The interactive runtime lives in the TUI, which embeds the REPL via
//! `ReplBridge`. This module provides the streaming inference path for
//! one-shot file/stdin input.

use std::sync::Arc;

use hkask_services_chat::{ChatService, ChatTurnRequest, MemoryService, PreparedChat};
use hkask_services_context::AgentService;
use hkask_services_onboarding::ResolvedSecrets;
use hkask_types::template::LLMParameters;
use hkask_types::{InferencePort, InferenceUsage};

/// Response from a chat inference call.
pub use hkask_services_chat::ChatTurnResponse;

/// Token usage breakdown for gas accounting.
pub use hkask_services_chat::TokenUsage;

/// Build AgentService from secrets or environment.
fn build_chat_context(
    name: &str,
    secrets: Option<&ResolvedSecrets>,
) -> Result<AgentService, Box<ChatTurnResponse>> {
    let from_secrets = secrets.map(|s| (name, s));
    super::helpers::build_agent_service_from_secrets(from_secrets).map_err(|e| {
        Box::new(ChatTurnResponse {
            text: format!("AgentService error: {}", e),
            usage: None,
            finish_reason: "error".to_string(),
            tool_calls: vec![],
            reasoning: None,
            messages: vec![],
        })
    })
}

/// Stream inference output, store episodic memory, and return assembled response.
async fn finish_stream(
    prepared: &PreparedChat,
    params: &LLMParameters,
    input: &str,
) -> ChatTurnResponse {
    let stream = prepared.inference_port.generate_stream_with_messages(
        &prepared.messages,
        params,
        Some(&prepared.model),
        None,
    );

    let mut full_text = String::new();
    let mut final_reasoning = String::new();
    let mut final_usage: Option<InferenceUsage> = None;
    let mut final_finish_reason = String::from("stop");
    let mut final_tool_calls: Vec<hkask_types::StructuredToolCall> = vec![];

    use futures_util::StreamExt;
    let mut stream = Box::pin(stream);
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if !chunk.text_delta.is_empty() {
                    print!("{}", chunk.text_delta);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
                full_text.push_str(&chunk.text_delta);
                final_reasoning.push_str(&chunk.reasoning_delta);
                if let Some(usage) = chunk.usage {
                    final_usage = Some(usage);
                }
                if let Some(reason) = chunk.finish_reason {
                    final_finish_reason = reason;
                }
                if !chunk.tool_calls.is_empty() {
                    final_tool_calls = chunk.tool_calls;
                }
            }
            Err(e) => {
                return ChatTurnResponse {
                    text: format!("Stream error: {}", e),
                    usage: None,
                    finish_reason: "error".to_string(),
                    tool_calls: vec![],
                    reasoning: None,
                    messages: vec![],
                };
            }
        }
    }
    println!();

    MemoryService::store_episodic(
        &prepared.episodic_port,
        input,
        &full_text,
        prepared.agent_webid,
        &prepared.capability_token,
        &prepared.userpod_name,
    );

    ChatTurnResponse {
        text: full_text,
        reasoning: (!final_reasoning.is_empty()).then_some(final_reasoning),
        usage: final_usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }),
        finish_reason: final_finish_reason,
        tool_calls: final_tool_calls,
        messages: prepared.messages.clone(),
    }
}

/// Send a chat message to an agent and print tokens as they arrive.
///
/// Used by `kask tui -f <file>` (non-interactive mode). Uses
/// `ChatService::prepare_chat()` for prompt composition and memory recall,
/// then streams inference output via `generate_stream_with_model()`.
#[allow(clippy::too_many_arguments)]
pub async fn chat_with_agent_streaming(
    input: &str,
    userpod_name: Option<&str>,
    model_override: Option<&str>,
    inference_port: Option<Arc<dyn InferencePort>>,
    secrets: Option<&ResolvedSecrets>,
    episodic_storage: Option<Arc<dyn hkask_memory::EpisodicStoragePort>>,
    semantic_storage: Option<Arc<dyn hkask_memory::SemanticStoragePort>>,
    _agent_webid: Option<hkask_types::WebID>,
    tool_section: Option<&str>,
) -> ChatTurnResponse {
    let name = userpod_name.unwrap_or("Curator");

    let ctx = match build_chat_context(name, secrets) {
        Ok(ctx) => ctx,
        Err(resp) => return *resp,
    };

    let req = ChatTurnRequest {
        input: input.to_string(),
        userpod_name: Some(name.to_string()),
        model_override: model_override.map(|s| s.to_string()),
        tool_section: tool_section.map(|s| s.to_string()),
        inference_port_override: inference_port,
        episodic_storage_override: episodic_storage,
        semantic_storage_override: semantic_storage,
        auth_context: None,
        params_override: None,
        api_spec: None,
        tools: None,
        thread_messages: None,
        prebuilt_messages: None,
        improv_mode: None,
    };

    let prepared = match ChatService::prepare_chat(&ctx, &req).await {
        Ok(p) => p,
        Err(e) => {
            return ChatTurnResponse {
                text: format!("Chat prepare error: {}", e),
                usage: None,
                finish_reason: "error".to_string(),
                tool_calls: vec![],
                reasoning: None,
                messages: vec![],
            };
        }
    };

    // Stream inference — chat bypasses fusion so the user's chosen model is
    // used directly, while skills route through the fusion group.
    let fusion_active = ctx.config().inference_config.fusion.is_some();
    let params = LLMParameters {
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        min_p: 0.0,
        typical_p: 0.0,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        max_tokens: 512,
        seed: None,
        disable_thinking: false,
        adapter: None,
        bypass_fusion: fusion_active,
        fusion_config: None,
        system_prompt: None,
    };

    finish_stream(&prepared, &params, input).await
}
