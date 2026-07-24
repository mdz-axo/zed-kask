//! ChatService — unified inference, memory integration, and prompt composition.
//!
//! This is the deepest service module: it encapsulates the full chat turn
//! pipeline — agent lookup, system prompt assembly, semantic recall,
//! inference, episodic storage, and tool-call handling — so that both
//! CLI and API surfaces delegate to a single implementation rather than
//! duplicating ~400 lines of business logic.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::Stream;
use futures_util::StreamExt;

use hkask_capability::DelegationAction;
use hkask_memory::{EpisodicStoragePort, SemanticStoragePort};
use hkask_types::InferencePort;
use hkask_types::regulation::RegulationSpan;

use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::template::LLMParameters;
use hkask_types::{DataCategory, WebID};

use super::improv::improv_system_prompt;
use super::types::{
    ChatStreamEvent, ChatTurnRequest, ChatTurnResponse, PreparedChat, TokenUsage, TurnRequest,
    TurnResult,
};
use crate::memory::MemoryService;
use hkask_pods::curator_agent::CuratorAgent;
use hkask_services_context::AgentService;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_services_inference::{InferenceContext, InferenceService};
use hkask_types::curator::CuratorHandle;
use hkask_types::ports::inference_types::InferenceError;

/// Chat service — encapsulates the full chat turn pipeline.
pub struct ChatService;

impl ChatService {
    /// Run a curator metacognition cycle and return a human-readable summary.
    ///
    /// Originally in `CuratorService`; merged here as the sole behavioral
    /// method that performed real orchestration (the other three were pure
    /// pass-throughs to `governance::` free functions).
    ///
    /// expect: "The system runs curator metacognition cycles for system health assessment"
    #[must_use = "result must be used"]
    pub async fn run_curator_metacognition(ctx: &AgentService) -> Result<String, ServiceError> {
        let queue = Arc::clone(&ctx.governance().escalations);
        let reg_lock = &ctx.ledger().runtime;
        let ledger = Arc::new(reg_lock.read().await.clone());

        let agents_ctx = Arc::new(hkask_pods::CuratorContext::new(
            CuratorHandle::system(),
            ledger,
            None,
            queue,
        ));
        let agent = CuratorAgent::new(agents_ctx);
        let snapshot =
            agent
                .metacognition()
                .run_cycle()
                .await
                .map_err(|e| ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Curator,
                    source: None,
                    message: e.to_string(),
                })?;
        let summary = agent.metacognition().generate_summary(&snapshot);

        Self::post_to_matrix_if_configured(ctx, &summary).await;

        Ok(summary)
    }

    async fn post_to_matrix_if_configured(ctx: &AgentService, summary: &str) {
        let room_id = match std::env::var("HKASK_CURATOR_ROOM_ID") {
            Ok(id) if !id.is_empty() => id,
            _ => return,
        };

        let transport = match ctx.infra().matrix.as_ref() {
            Some(t) => t,
            None => return,
        };

        use hkask_communication::matrix::RoomId;
        let room = RoomId(room_id);
        if let Err(e) = transport
            .lock()
            .await
            .send_message(&room, summary, None)
            .await
        {
            tracing::warn!(
                target: "reg.curation.matrix",
                room_id = %room.0,
                error = %e,
                "Failed to post metacognition summary to Matrix"
            );
        } else {
            tracing::info!(
                target: "reg.curation.matrix",
                room_id = %room.0,
                "Metacognition summary posted to Matrix standing session"
            );
        }
    }

    // ── Chat pipeline ─────────────────────────────────────────────────
    /// Prepare a chat turn without executing inference.
    ///
    /// Does agent lookup, prompt composition, semantic recall,
    /// and resolves the inference port. Returns a `PreparedChat`
    /// that the caller can use to stream inference output.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx must be fully built; req.input must be non-empty; agent must be registered
    /// post: returns PreparedChat with prompt, model, agent_webid, capability_token, inference_port, episodic_port, and userpod_name; Err(AgentNotFound) if agent not registered
    ///
    /// expect: "The system orchestrates LLM chat turns with memory recall, episodic storage, and Regulation observability"
    #[must_use = "result must be used"]
    pub async fn prepare_chat(
        ctx: &AgentService,
        req: &ChatTurnRequest,
    ) -> Result<PreparedChat, ServiceError> {
        let name = req.userpod_name.as_deref().unwrap_or("Curator");

        // Agent registry removed (consolidation): compose a direct system prompt.
        let mut system_prompt = format!("You are {} in the hKask system.\n\n", name);

        // Append tool-call format instructions
        if let Some(ref section) = req.tool_section
            && !section.is_empty()
        {
            system_prompt.push_str(section);
        }

        // Append condensed API reference for answering API questions
        if let Some(ref spec) = req.api_spec
            && !spec.is_empty()
        {
            system_prompt.push_str("\n\n## API Reference\n");
            system_prompt.push_str(spec);
        }

        // Prepend improv mode instructions to the system prompt.
        if let Some(ref mode) = req.improv_mode {
            let improv_instruction = improv_system_prompt(mode);
            system_prompt = format!("{}\n\n{}", improv_instruction, system_prompt);
        }

        // Agent kind taxonomy removed (consolidation); capability routing no longer keyed on kind.
        // Model flows from request override → config default.
        // Agent kind no longer hardcodes model selection — use session/userpod settings.
        let model = req
            .model_override
            .as_deref()
            .unwrap_or(&ctx.config().default_model)
            .to_string();

        // Resolve inference port — prefer override, then shared port from AgentService
        let inference: Arc<dyn InferencePort> =
            match (&req.inference_port_override, ctx.infra().inference.clone()) {
                (Some(port), _) => Arc::clone(port),
                (None, Some(port)) => port,
                (None, None) => {
                    let inf_ctx = InferenceContext::from_parts(
                        None,
                        &model,
                        ctx.config().inference_config.clone(),
                    );
                    InferenceService::resolve_port(&inf_ctx, &model)?
                }
            };

        // Derive WebID for the agent
        let agent_webid = WebID::from_persona_with_namespace(name.as_bytes(), "userpod");

        // Create capability token for memory operations.
        let capability_token = ctx.governance().checker.grant_registry(
            DelegationAction::Execute,
            req.auth_context.as_ref().map_or(*ctx.webid(), |a| a.webid),
            agent_webid,
        );

        // Recall relevant knowledge from semantic memory (third-person facts)
        let semantic_port: Arc<dyn SemanticStoragePort> = req
            .semantic_storage_override
            .clone()
            .unwrap_or_else(|| ctx.infra().semantic.clone());
        let episodic_port: Arc<dyn EpisodicStoragePort> = req
            .episodic_storage_override
            .clone()
            .unwrap_or_else(|| ctx.infra().episodic.clone());

        // \[NORMATIVE\] Sovereignty gate (H3/P2): only recall memory when
        // the owner has granted consent for the category. No consent ⇒ no recall.
        let semantic_context = if MemoryService::has_memory_consent(
            ctx,
            &agent_webid,
            &DataCategory::SemanticMemory,
        ) {
            MemoryService::recall_semantic(&semantic_port, &req.input, &capability_token)
        } else {
            None
        };
        let episodic_context = if MemoryService::has_memory_consent(
            ctx,
            &agent_webid,
            &DataCategory::EpisodicMemory,
        ) {
            MemoryService::recall_episodic(
                &episodic_port,
                &req.input,
                &agent_webid,
                &capability_token,
            )
        } else {
            None
        };

        // Merge semantic (third-person) and episodic (first-person) memory.
        // Both are recall_* results that mirror each other in structure —
        // concatenated into a single "Relevant Memory" section sorted by salience.
        let memory_context = match (semantic_context, episodic_context) {
            (Some(s), Some(e)) => Some(format!("{}\n\n{}", s, e)),
            (Some(s), None) => Some(s),
            (None, Some(e)) => Some(e),
            (None, None) => None,
        };

        // Build typed message array for multi-turn inference. The system message
        // carries the system prompt + memory context; thread history (when
        // present) is inserted between system and the current user turn with
        // proper role tags.
        let system_with_memory = match memory_context {
            Some(ref ctx_text) => {
                format!("{}\n\n## Relevant Memory\n{}", system_prompt, ctx_text)
            }
            None => system_prompt.clone(),
        };
        let messages = match req.thread_messages.as_ref() {
            Some(thread_msgs) if !thread_msgs.is_empty() => {
                let mut msgs = Vec::with_capacity(thread_msgs.len() + 2);
                msgs.push(hkask_types::ChatMessage::system(&system_with_memory));
                msgs.extend(thread_msgs.iter().cloned());
                msgs.push(hkask_types::ChatMessage::user(&req.input));
                msgs
            }
            _ => vec![
                hkask_types::ChatMessage::system(&system_with_memory),
                hkask_types::ChatMessage::user(&req.input),
            ],
        };

        Ok(PreparedChat {
            messages,
            model,
            agent_webid,
            capability_token,
            inference_port: inference,
            episodic_port,
            userpod_name: name.to_string(),
        })
    }

    /// Execute a single chat turn: agent lookup → prompt composition →
    /// semantic recall → inference → episodic storage.
    ///
    /// Uses `AgentService` for shared infrastructure (inference port,
    /// memory ports, A2A runtime, agent registry). When the context's
    /// inference_port is `None`, creates a fresh port via InferenceService.
    ///
    /// For streaming, use `prepare_chat()` + `generate_stream_with_model()`
    /// directly on the inference port.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx must be fully built; req.input must be non-empty
    /// post: returns ChatTurnResponse with text, usage, finish_reason, and tool_calls; Regulation spans emitted; episodic trace stored; Err on agent lookup or inference failure
    ///
    /// expect: "The system orchestrates LLM chat turns with memory recall, episodic storage, and Regulation observability"
    #[must_use = "result must be used"]
    pub async fn chat(
        ctx: &AgentService,
        req: ChatTurnRequest,
    ) -> Result<ChatTurnResponse, ServiceError> {
        // When pre-built messages are provided (turn-loop iterations 2+),
        // skip prepare_chat entirely — the growing message array already
        // contains the system prompt, thread history, and tool results.
        // Resolve only the model, inference port, and episodic port.
        let prepared = if req.prebuilt_messages.is_some() {
            let messages = req.prebuilt_messages.unwrap();
            let name = req.userpod_name.as_deref().unwrap_or("Curator");
            let model = req
                .model_override
                .as_deref()
                .unwrap_or(&ctx.config().default_model)
                .to_string();
            let inference: Arc<dyn InferencePort> =
                match (&req.inference_port_override, ctx.infra().inference.clone()) {
                    (Some(port), _) => Arc::clone(port),
                    (None, Some(port)) => port,
                    (None, None) => {
                        let inf_ctx = InferenceContext::from_parts(
                            None,
                            &model,
                            ctx.config().inference_config.clone(),
                        );
                        InferenceService::resolve_port(&inf_ctx, &model)?
                    }
                };
            let agent_webid = WebID::from_persona_with_namespace(name.as_bytes(), "userpod");
            let capability_token = ctx.governance().checker.grant_registry(
                DelegationAction::Execute,
                req.auth_context.as_ref().map_or(*ctx.webid(), |a| a.webid),
                agent_webid,
            );
            let episodic_port: Arc<dyn EpisodicStoragePort> = req
                .episodic_storage_override
                .clone()
                .unwrap_or_else(|| ctx.infra().episodic.clone());
            PreparedChat {
                messages,
                model,
                agent_webid,
                capability_token,
                inference_port: inference,
                episodic_port,
                userpod_name: name.to_string(),
            }
        } else {
            Self::prepare_chat(ctx, &req).await?
        };
        // Access params_override after prepare_chat returns (prepare_chat only borrows req)
        let params_override = req.params_override;

        // Resolve LLM parameters: caller override > agent-kind defaults.
        // When fusion is active, the primary chat model bypasses fusion so
        // the user's chosen model is used directly. Skills route through fusion.
        let chat_bypass = ctx.config().inference_config.fusion.is_some();
        let mut params = params_override.unwrap_or(LLMParameters {
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
            bypass_fusion: chat_bypass,
            fusion_config: None,
            system_prompt: None,
        });
        // Always enforce the service-layer bypass decision — a caller's
        // params_override must not silently defeat the cost-safety guard.
        params.bypass_fusion = chat_bypass;

        let request_span = Span::new(
            SpanNamespace::new("reg.chat").expect("canonical namespace: reg.chat"),
            "request",
        );
        let request_event = RegulationRecord::new(
            prepared.agent_webid,
            request_span,
            CyclePhase::Act,
            serde_json::json!({
                "agent": &prepared.userpod_name,
                "model": &prepared.model,
                "prompt_len": prepared.messages.len(),
            }),
            0,
        );
        let _ = ctx.ledger().events.persist(&request_event);

        let result = tokio::time::timeout(
            Duration::from_secs(120),
            prepared.inference_port.generate_with_messages(
                &prepared.messages,
                &params,
                Some(&prepared.model),
                req.tools.as_deref(),
            ),
        )
        .await
        .map_err(|_elapsed| ServiceError::ModelService {
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: "Inference call timed out after 120s".to_string(),
            retryable: true,
        })?
        .map_err(|e| {
            // Classify by error type: connection/circuit/generation errors are
            // transient (retryable); model/json errors are client-side (not retryable).
            let (kind, retryable) = match &e {
                InferenceError::Connection(_) | InferenceError::CircuitOpen(_) => {
                    (ErrorKind::ServiceUnavailable, true)
                }
                InferenceError::Generation(_) => (ErrorKind::ServiceUnavailable, true),
                InferenceError::Model(_)
                | InferenceError::Json(_)
                | InferenceError::VisionUnsupported(_) => (ErrorKind::BadRequest, false),
            };
            ServiceError::ModelService {
                kind,
                source: None,
                message: e.to_string(),
                retryable,
            }
        })?;

        let response_span = Span::new(
            SpanNamespace::new("reg.chat").expect("canonical namespace: reg.chat"),
            "response",
        );
        let response_event = RegulationRecord::new(
            prepared.agent_webid,
            response_span,
            CyclePhase::Act,
            serde_json::json!({
                "agent": &prepared.userpod_name,
                "model": &prepared.model,
                "tokens": result.usage.total_tokens,
                "finish_reason": &result.finish_reason,
            }),
            0,
        )
        .with_parent(request_event.id);
        let _ = ctx.ledger().events.persist(&response_event);

        // Store the exchange
        let memory_span = Span::new(
            SpanNamespace::try_from(RegulationSpan::MemoryEncode).expect("canonical span"),
            "episodic_stored",
        );
        let memory_event = RegulationRecord::new(
            prepared.agent_webid,
            memory_span,
            CyclePhase::Act,
            serde_json::json!({
                "agent": &prepared.userpod_name,
                "operation": "store_episodic",
                "input_len": req.input.len(),
                "response_len": result.text.len(),
            }),
            0,
        );
        let _ = ctx.ledger().events.persist(&memory_event);

        // \[NORMATIVE\] Sovereignty gate (H3/P2): only persist the exchange to
        // episodic (sovereign) memory when the owner has granted consent.
        if MemoryService::has_memory_consent(
            ctx,
            &prepared.agent_webid,
            &DataCategory::EpisodicMemory,
        ) {
            MemoryService::store_episodic(
                &prepared.episodic_port,
                &req.input,
                &result.text,
                prepared.agent_webid,
                &prepared.capability_token,
                &prepared.userpod_name,
            );
        } else {
            tracing::debug!(
                target: "hkask.chat.memory",
                agent = %prepared.userpod_name,
                "Episodic store skipped — no episodic-memory consent (P2)"
            );
        }

        Ok(ChatTurnResponse {
            text: result.text,
            reasoning: result.reasoning,
            usage: Some(TokenUsage {
                prompt_tokens: result.usage.prompt_tokens,
                completion_tokens: result.usage.completion_tokens,
                total_tokens: result.usage.total_tokens,
            }),
            finish_reason: result.finish_reason,
            tool_calls: result.tool_calls,
            messages: prepared.messages.clone(),
        })
    }

    /// Execute a streaming chat turn with full pipeline.
    ///
    /// Calls `prepare_chat()` for agent lookup + memory recall + prompt composition,
    /// then streams tokens from `generate_stream_with_model()`, then stores the
    /// exchange in episodic memory (with sovereignty gate) and emits Regulation spans.
    ///
    /// Returns a Stream of `ChatStreamEvent` — the caller decides how to deliver
    /// (WebSocket frames, SSE events, etc.).
    ///
    /// **Phase 1 limitation:** Tool calls are passed to inference but mid-stream
    /// tool execution is not yet handled. Tool calls that arrive in the final chunk
    /// are available via `ChatStreamEvent::Done.tool_calls` (future).
    ///
    /// \[P5\] Motivating: Essentialism — composes existing `prepare_chat()` + streaming.
    /// pre:  ctx must be fully built; req.input must be non-empty
    /// post: returns `Stream<ChatStreamEvent>`; episodic stored (if P2 consent)
    /// post: Err on agent lookup or inference port resolution failure
    ///
    /// expect: "The system orchestrates LLM chat turns with memory recall, episodic storage, and Regulation observability"
    #[must_use = "stream must be consumed"]
    pub fn chat_stream(
        ctx: &AgentService,
        req: ChatTurnRequest,
    ) -> Pin<Box<dyn Stream<Item = ChatStreamEvent> + Send + '_>> {
        // Validate input early
        if req.input.is_empty() {
            return Box::pin(futures_util::stream::once(async {
                ChatStreamEvent::Error {
                    message: "Input must be non-empty".to_string(),
                }
            }));
        }

        Box::pin(async_stream::stream! {
            // --- Phase 1: Prepare (agent lookup, memory recall, prompt composition) ---
            let prepared = match ChatService::prepare_chat(ctx, &req).await {
                Ok(p) => p,
                Err(e) => {
                    yield ChatStreamEvent::Error {
                        message: format!("Chat preparation failed: {e}"),
                    };
                    return;
                }
            };

            // Resolve LLM parameters (same logic as chat())
            let params_override = req.params_override;
            let chat_bypass = ctx.config().inference_config.fusion.is_some();
            let mut params = params_override.unwrap_or(LLMParameters {
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
                bypass_fusion: chat_bypass,
                fusion_config: None,
            system_prompt: None,
            });
            params.bypass_fusion = chat_bypass;

            // --- Phase 2: Stream tokens ---
            let tools_ref = req.tools.as_deref();
            let mut stream = prepared.inference_port.generate_stream_with_messages(
                &prepared.messages,
                &params,
                Some(&prepared.model),
                tools_ref,
            );

            let mut full_text = String::new();
            let mut usage = None;
            let mut finish_reason = String::from("stop");

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        let text = chunk.text_delta.clone();
                        full_text.push_str(&text);
                        if let Some(ref u) = chunk.usage {
                            usage = Some(TokenUsage {
                                prompt_tokens: u.prompt_tokens,
                                completion_tokens: u.completion_tokens,
                                total_tokens: u.total_tokens,
                            });
                        }
                        if let Some(ref fr) = chunk.finish_reason {
                            finish_reason = fr.clone();
                        }
                        yield ChatStreamEvent::Token {
                            text_delta: text,
                            model: chunk.model,
                        };
                    }
                    Err(e) => {
                        yield ChatStreamEvent::Error {
                            message: format!("Inference streaming error: {e}"),
                        };
                        return;
                    }
                }
            }

            // --- Phase 3: Episodic storage (sovereignty-gated, P2) ---
            let memory_stored = if MemoryService::has_memory_consent(
                ctx,
                &prepared.agent_webid,
                &DataCategory::EpisodicMemory,
            ) {
                MemoryService::store_episodic(
                    &prepared.episodic_port,
                    &req.input,
                    &full_text,
                    prepared.agent_webid,
                    &prepared.capability_token,
                    &prepared.userpod_name,
                );
                true
            } else {
                tracing::debug!(
                    target: "hkask.chat.memory",
                    agent = %prepared.userpod_name,
                    "Episodic store skipped — no episodic-memory consent (P2)"
                );
                false
            };

            yield ChatStreamEvent::Done {
                finish_reason,
                usage,
                memory_stored,
            };
        })
    }

    /// Run a process manifest cascade for the agent, returning manifest-derived context.
    ///
    /// The manifest is a declarative pipeline (from `process_manifest` in the agent
    /// definition) that enriches the user input with context before inference.
    /// Returns `None` if the agent has no manifest or execution fails.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  executor must be initialized; manifest must be valid; input and userpod_name must be non-empty
    /// post: returns Some(String) of concatenated step outputs if cascade completes; None if no manifest or execution fails
    ///
    /// expect: "The system executes skill manifest cascades with template rendering and inference composition"
    pub async fn execute_manifest_cascade(
        executor: &hkask_templates::ManifestExecutor,
        manifest: &hkask_templates::BundleManifest,
        input: &str,
        userpod_name: &str,
    ) -> Option<String> {
        let mut initial_ctx = std::collections::HashMap::new();
        initial_ctx.insert(
            "user_input".to_string(),
            serde_json::Value::String(input.to_string()),
        );
        initial_ctx.insert(
            "agent".to_string(),
            serde_json::Value::String(userpod_name.to_string()),
        );

        let ctx = match executor.execute_manifest(manifest, initial_ctx).await {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::warn!(
                    target: "reg.spec.executor",
                    error = %e,
                    "Manifest cascade failed — continuing without manifest enrichment"
                );
                return None;
            }
        };

        let context_parts: Vec<String> = ctx
            .iter()
            .filter_map(|(key, value)| {
                if key.starts_with("step_") {
                    Some(format!("{}: {}", key, value))
                } else {
                    None
                }
            })
            .collect();

        if context_parts.is_empty() {
            None
        } else {
            tracing::info!(
                target: "reg.spec.executor",
                steps_completed = context_parts.len(),
                "Manifest cascade completed"
            );
            Some(context_parts.join("\n"))
        }
    }

    /// Wrap input with manifest context when a cascade completed successfully.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  input and manifest_context must be non-empty
    /// post: returns formatted string with [Manifest Context] block prepended to input
    ///
    /// expect: "The system executes skill manifest cascades with template rendering and inference composition"
    pub fn wrap_manifest_input(input: &str, manifest_context: &str) -> String {
        format!(
            "[Manifest Context]\n{}\n[/Manifest Context]\n\n{}",
            manifest_context, input
        )
    }

    /// Execute a full single-agent turn — manifest cascade, history suffix,
    /// inference via `ChatService::chat()`, and persona filter.
    ///
    /// Returns the final response text, token usage, and iteration count.
    /// The caller is responsible for gas governance (reserving/settling energy),
    /// streaming display, tool-call execution, and Regulation update display.
    ///
    /// Tool-call handling: when the model returns structured tool calls,
    /// the response includes them in `structured_tool_calls`. The caller
    /// executes tools, formats results, and passes them as `tool_results`
    /// on the next iteration via a new `TurnRequest` (only `input`,
    /// `tool_results`, and iteration counter fields matter for continuations).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx must be fully built; req.input must be non-empty; req.userpod_name must be registered
    /// post: returns TurnResult with response text, token usage, tool calls, and iteration count; manifest cascade and history suffix applied; persona filter applied; Err on inference failure
    ///
    /// expect: "The system orchestrates LLM chat turns with memory recall, episodic storage, and Regulation observability"
    pub async fn execute_turn(
        ctx: &AgentService,
        req: &TurnRequest,
        _manifest_executor: Option<&hkask_templates::ManifestExecutor>,
        _process_manifest: Option<&hkask_templates::BundleManifest>,
    ) -> Result<TurnResult, ServiceError> {
        // When pre-built messages are provided (turn-loop iterations 2+),
        // skip manifest cascade, thread history concatenation, auto-condensation,
        // and tool results injection — all of that is already in the prebuilt
        // message array. Just pass through to chat() directly.
        if req.prebuilt_messages.is_some() {
            let chat_req = ChatTurnRequest {
                input: req.input.clone(),
                userpod_name: Some(req.userpod_name.clone()),
                model_override: Some(req.model.clone()),
                tool_section: None,
                api_spec: None,
                inference_port_override: Some(req.inference_port.clone()),
                episodic_storage_override: Some(req.episodic_storage.clone()),
                semantic_storage_override: Some(req.semantic_storage.clone()),
                auth_context: None,
                params_override: Some(req.llm_params.clone()),
                tools: req.tools.clone(),
                thread_messages: None,
                prebuilt_messages: req.prebuilt_messages.clone(),
                improv_mode: None,
            };
            let chat_response = Self::chat(ctx, chat_req).await?;
            return Ok(TurnResult {
                text: chat_response.text,
                reasoning: chat_response.reasoning,
                usage: chat_response.usage.unwrap_or(TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                }),
                structured_tool_calls: chat_response.tool_calls,
                messages: chat_response.messages,
            });
        }

        // Manifest cascade is now run by the caller (run_turn_with_state)
        // before the turn loop — not per-iteration inside execute_turn.
        let base_input = req.input.clone();

        // Auto-condense if context pressure exceeds threshold (87.5%).
        let history_token = req.capability_checker.grant_registry(
            DelegationAction::Read,
            req.system_webid,
            req.agent_webid,
        );
        const CONDENSE_THRESHOLD: f64 = 0.875;
        let effective_input = if req.auto_condense
            && let Some(window) = req.context_window
        {
            let threshold = (window as f64 * CONDENSE_THRESHOLD) as usize;
            if hkask_condenser::inference::approx_token_count(&base_input) > threshold
                && let Some(condensed) =
                    Self::condense_history(ctx, req, &history_token, &base_input).await
            {
                condensed
            } else {
                base_input.clone()
            }
        } else {
            base_input
        };

        let chat_req = ChatTurnRequest {
            input: effective_input,
            userpod_name: Some(req.userpod_name.clone()),
            model_override: Some(req.model.clone()),
            tool_section: if req.tool_section.is_empty() {
                None
            } else {
                Some(req.tool_section.clone())
            },
            api_spec: req.api_spec.clone(),
            inference_port_override: Some(req.inference_port.clone()),
            episodic_storage_override: Some(req.episodic_storage.clone()),
            semantic_storage_override: Some(req.semantic_storage.clone()),
            auth_context: None,
            params_override: Some(req.llm_params.clone()),
            tools: req.tools.clone(),
            thread_messages: req.thread_messages.clone(),
            prebuilt_messages: None,
            improv_mode: req.improv_mode.clone(),
        };
        let chat_response = Self::chat(ctx, chat_req).await?;

        Ok(TurnResult {
            text: chat_response.text,
            reasoning: chat_response.reasoning,
            usage: chat_response.usage.unwrap_or(TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
            structured_tool_calls: chat_response.tool_calls,
            messages: chat_response.messages,
        })
    }
}
