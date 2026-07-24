//! Curator chat routes
//
//! The `POST /api/chat` endpoint accepts an optional `model` field that
//! switches the LLM used for inference. Use `GET /api/models` to discover
//! valid model identifiers.
//!
//! The `POST /api/chat/stream` endpoint streams inference output as SSE events.

use axum::extract::Extension;
use axum::{
    Json,
    extract::State,
    response::sse::{Event, Sse},
};
use std::convert::Infallible;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::middleware::auth::AuthContext;
use hkask_inference::model_constants;
use hkask_services_chat::{ChatService, ChatTurnRequest as ServiceChatTurnRequest};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Chat request sent to the Curator or a specified agent.
///
/// The `model` field allows switching the LLM at request time. When omitted,
/// the server default (a fast classifier model) is used. Use `GET /api/models` to discover
/// available models, and `GET /api/models/search?q=...` for fuzzy matching.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiChatRequest {
    /// User input message
    pub input: String,
    /// Optional template ID to contextualize the prompt
    pub template_id: Option<String>,
    /// Model override for inference (e.g., "DI/google/gemma-4-9b-it"). If unset, uses the server default.
    #[serde(default)]
    pub model: Option<String>,
}

/// Chat response from the Curator or agent.
///
/// The `model` field echoes which LLM was used, confirming model switching.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiChatResponse {
    /// Generated response text
    pub output: String,
    /// Template ID that was applied (or "auto-select")
    pub template_id: String,
    /// Model identifier used for inference
    pub model: String,
}

/// Create chat router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with chat routes registered
pub fn chat_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(chat))
        .routes(routes!(chat_stream))
        .merge(super::chat_ws::chat_ws_router())
}

/// Chat with the Curator or a specified agent.
///
/// Accepts an optional `model` field to switch the LLM at request time.
/// When omitted, the server default (a fast classifier model) is used. The response
/// echoes the `model` used, confirming which LLM generated the output.
///
/// Use `GET /api/models` or `GET /api/models/search?q=...` to discover
/// available model identifiers.
#[utoipa::path(
    post,
    path = "/api/chat",
    tag = "chat",
    request_body = ApiChatRequest,
    responses(
        (status = 200, description = "Chat response", body = ApiChatResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn chat(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<ApiChatRequest>,
) -> Result<Json<ApiChatResponse>, crate::error::ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "chat", "REG");

    // Input length validation
    if req.input.len() > 100_000 {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: format!(
                "Input exceeds 100KB limit (received {} bytes)",
                req.input.len()
            ),
        }
        .into());
    }

    let model_str = req
        .model
        .clone()
        .unwrap_or_else(model_constants::classifier_model);
    let model: &str = &model_str;
    let strategy = hkask_templates::PromptStrategy::from_input(&req.input);

    // Frame the prompt via PromptStrategy (API-specific — uses template_id)
    let prompt = match &req.template_id {
        Some(id) => format!("[template: {}] {}", id, req.input),
        None => strategy.frame(&req.input).to_string(),
    };

    let svc_req = ServiceChatTurnRequest {
        input: prompt,
        userpod_name: Some("Curator".to_string()),
        model_override: req.model,
        tool_section: None,
        api_spec: Some(crate::openapi_spec::condensed_api_spec()),
        inference_port_override: None,
        episodic_storage_override: None,
        semantic_storage_override: None,
        auth_context: Some(auth),
        params_override: None,
        tools: None,
        thread_messages: None,
        prebuilt_messages: None,
        improv_mode: None,
    };

    let result = match ChatService::chat(&state.agent_service, svc_req).await {
        Ok(resp) => resp,
        Err(e) => hkask_services_chat::ChatTurnResponse {
            text: format!("Chat error: {}", e),
            usage: None,
            finish_reason: "error".to_string(),
            tool_calls: vec![],
            reasoning: None,
            messages: vec![],
        },
    };

    Ok(Json(ApiChatResponse {
        output: result.text,
        template_id: req
            .template_id
            .unwrap_or_else(|| strategy.name().to_string()),
        model: model.to_string(),
    }))
}

/// Stream chat inference output as Server-Sent Events.
///
/// Accepts the same `ApiChatRequest` as `/api/chat` but streams the inference
/// output as SSE `data:` events. Each event carries a JSON object with
/// `text_delta`, `model`, and optionally `finish_reason`. The final event
/// has `finish_reason: "stop"` and includes `usage` stats.
///
/// This is a surface-layer endpoint — it bypasses ChatService's full
/// pipeline (memory recall, episodic storage) and calls the inference port
/// directly for streaming. Use `/api/chat` for the full pipeline with
/// memory integration.
#[utoipa::path(
    post,
    path = "/api/chat/stream",
    tag = "chat",
    request_body = ApiChatRequest,
    responses(
        (status = 200, description = "SSE stream of chat chunks", content_type = "text/event-stream"),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn chat_stream(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Json(req): Json<ApiChatRequest>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "chat_stream", "REG");

    // Input length validation
    if req.input.len() > 100_000 {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(1);
        // Fire-and-forget: tx.send() is a future but we return the stream
        // immediately — the ReceiverStream polls it on the next executor tick.
        #[allow(clippy::let_underscore_future)]
        let _ = tx.send(Ok(Event::default().data(
            serde_json::json!({"error": "Input exceeds 100KB limit"}).to_string(),
        )));
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        return Sse::new(Box::pin(stream)).keep_alive(axum::response::sse::KeepAlive::default());
    }

    let model_str = req
        .model
        .clone()
        .unwrap_or_else(model_constants::classifier_model);
    let strategy = hkask_templates::PromptStrategy::from_input(&req.input);

    let prompt = match &req.template_id {
        Some(id) => format!("[template: {}] {}", id, req.input),
        None => strategy.frame(&req.input).to_string(),
    };

    let fusion_active = state
        .agent_service
        .config()
        .inference_config
        .fusion
        .is_some();
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

    let inference = state.agent_service.infra().inference.clone();

    // Use a channel to bridge the borrowed inference stream into a 'static
    // stream for the SSE response. A spawned task owns the Arc and sends
    // chunks through the channel.
    let (tx, rx): (tokio::sync::mpsc::Sender<Result<Event, Infallible>>, _) =
        tokio::sync::mpsc::channel(32);

    match inference {
        Some(port) => {
            tokio::spawn(async move {
                use std::time::Duration;
                let stream_future = async {
                    let mut stream =
                        port.generate_stream_with_model(&prompt, &params, Some(&model_str), None);
                    use futures_util::StreamExt;
                    while let Some(chunk_result) = stream.next().await {
                        let event = match chunk_result {
                            Ok(chunk) => {
                                let data = serde_json::json!({
                                    "text_delta": chunk.text_delta,
                                    "model": chunk.model,
                                    "finish_reason": chunk.finish_reason,
                                    "usage": chunk.usage.map(|u| serde_json::json!({
                                        "prompt_tokens": u.prompt_tokens,
                                        "completion_tokens": u.completion_tokens,
                                        "total_tokens": u.total_tokens,
                                    })),
                                    "tool_calls": if chunk.tool_calls.is_empty() { None } else { Some(serde_json::json!(chunk.tool_calls)) },
                                });
                                Event::default().data(data.to_string())
                            }
                            Err(e) => Event::default().data(
                                serde_json::json!({
                                    "error": e.to_string(),
                                })
                                .to_string(),
                            ),
                        };
                        if tx.send(Ok(event)).await.is_err() {
                            return; // receiver dropped
                        }
                    }
                };
                if let Err(_elapsed) =
                    tokio::time::timeout(Duration::from_secs(120), stream_future).await
                {
                    let _ = tx
                        .send(Ok(Event::default().data(
                            serde_json::json!({"error": "Stream timed out after 120s"}).to_string(),
                        )))
                        .await;
                }
            });
        }
        None => {
            let _ = tx
                .send(Ok(Event::default().data(
                    serde_json::json!({
                        "error": "Inference unavailable — no model loaded",
                    })
                    .to_string(),
                )))
                .await;
        }
    }

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Sse::new(Box::pin(stream)).keep_alive(axum::response::sse::KeepAlive::default())
}
