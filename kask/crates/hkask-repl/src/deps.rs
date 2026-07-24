//! Dependency injection traits for the turn loop.
//!
//! Defines the capability traits that `run_turn_loop` needs, plus
//! production adapters that bridge these traits to the existing
//! infrastructure. The key design decision: `TurnExecutor` takes a
//! `TurnInput` (primitives only) and builds the `TurnRequest` internally.
//! This keeps port types (`Arc<dyn InferencePort>`, etc.) out of the
//! test layer — tests construct `TurnInput` from strings and numbers.

use std::pin::Pin;
use std::sync::Arc;

use hkask_capability::ToolPort;
use hkask_services_chat::{ChatService, TurnRequest, TurnResult};
use hkask_services_context::AgentService;
use hkask_services_core::ServiceError;
use hkask_types::WebID;

use super::energy::EnergyGuard;
use super::threads::ThreadRegistry;

// ── TurnInput: primitive inputs to a turn ────────────────────────────

/// Primitive inputs for one inference iteration.
///
/// Contains only strings, numbers, and Options — no port types.
/// The executor builds a full `TurnRequest` from this plus its
/// internal state. This keeps the test layer free of `Arc<dyn Port>`.
pub struct TurnInput<'a> {
    pub input: &'a str,
    pub iteration: usize,
    pub agent_override: Option<&'a str>,
    /// Typed message array from thread history for iteration 1.
    /// When None and `messages` is also None, the executor builds from input only.
    pub thread_messages: Option<Vec<hkask_types::ChatMessage>>,
    /// Pre-built growing message array for iterations 2+.
    /// When present, the executor skips preparation and calls inference directly.
    /// The turn loop appends `assistant(response)` and `user(tool_results)` to
    /// this array between iterations, maintaining proper role tags.
    pub messages: Option<Vec<hkask_types::ChatMessage>>,
}

// ── TurnConfig: loop configuration ───────────────────────────────────

/// Loop configuration values extracted from `ReplSettings`.
pub struct TurnConfig {
    pub max_loops: usize,
    pub gas_heuristic: u64,
    pub saliency_window: usize,
    pub default_agent: String,
    /// Model name (used for display abbreviation in prompt/response).
    pub model_name: String,
    /// A2A secret for minting tool delegation tokens.
    pub a2a_secret: hkask_types::secret::ZeroizingSecret,
    /// Principal (user) WebID — the authorizing identity.
    pub principal_webid: hkask_types::WebID,
    /// Agent WebID — the delegated identity.
    pub agent_webid: hkask_types::WebID,
}

// ── Capability traits ────────────────────────────────────────────────

/// Execute inference turns. Takes primitive `TurnInput` and builds
/// the full `TurnRequest` internally — callers never see port types.
///
/// Production: wraps `ChatService::execute_turn`.
/// Mock: returns predetermined `TurnResult`s, ignores `TurnInput`.
#[async_trait::async_trait]
pub trait TurnExecutor: Send + Sync {
    async fn execute_turn(&self, input: &TurnInput<'_>) -> Result<TurnResult, ServiceError>;

    /// Streaming variant — yields text deltas as they arrive, then a final Done chunk.
    /// Default: non-streaming (single Done chunk from execute_turn).
    fn execute_turn_streaming<'a>(
        &'a self,
        input: &'a TurnInput<'a>,
    ) -> Pin<Box<dyn futures_util::Stream<Item = Result<TurnStreamChunk, ServiceError>> + Send + 'a>>
    {
        // Non-streaming default: run the full turn, then surface the model's
        // reasoning trace (if any) as a single `Thinking` chunk before the
        // final `Done`. This makes a reasoning model's chain-of-thought
        // visible even when the executor doesn't stream tokens live. A
        // streaming executor overrides this to emit `Delta`/`Thinking`
        // chunks as they arrive from the wire.
        enum State<R> {
            Pending,
            Thinking(R),
            Done,
        }
        Box::pin(futures_util::stream::unfold(
            State::Pending,
            move |state| async move {
                match state {
                    State::Pending => match self.execute_turn(input).await {
                        Ok(result) => match result.reasoning.clone() {
                            Some(reasoning) if !reasoning.is_empty() => Some((
                                Ok(TurnStreamChunk::Thinking(reasoning)),
                                State::Thinking(result),
                            )),
                            _ => Some((Ok(TurnStreamChunk::Done(result)), State::Done)),
                        },
                        Err(e) => Some((Err(e), State::Done)),
                    },
                    State::Thinking(result) => {
                        Some((Ok(TurnStreamChunk::Done(result)), State::Done))
                    }
                    State::Done => None,
                }
            },
        ))
    }
}

/// A chunk from a streaming inference turn.
#[derive(Debug, Clone)]
pub enum TurnStreamChunk {
    /// Incremental text delta from the LLM.
    Delta(String),
    /// Incremental reasoning/thinking delta from a reasoning model
    /// (Qwen3/GLM-5.2/DeepSeek-R1). Surfaced separately from `Delta` so the
    /// REPL renders a distinct "thinking" section instead of mixing the
    /// chain-of-thought into the answer (Cline issue #8636 regression class).
    Thinking(String),
    /// Final result — contains the complete text, usage, tool calls, and messages.
    Done(TurnResult),
}

/// Reserve and settle gas for inference.
pub trait GasGovernor: Send + Sync {
    fn try_reserve(&self, heuristic: u64) -> Option<Box<dyn GasReservation>>;
    fn gas_status(&self) -> (u64, u64);
}

pub trait GasReservation: Send {
    fn heuristic(&self) -> u64;
    fn settle(&mut self, actual: u64);
    fn release(&mut self);
}

/// Thread memory: short-term conversation stream.
pub trait ThreadMemory: Send {
    fn is_seeded(&self) -> bool;
    fn thread_history_messages(&self, window: usize) -> Option<Vec<hkask_types::ChatMessage>>;
    fn append_turn(&mut self, agent: &str, input: &str, response: &str);
    fn mark_seeded(&mut self);
}

// ── TurnDeps: bundled dependencies ───────────────────────────────────

/// All dependencies `run_turn_loop` needs.
/// 3 traits (behavioral) + 1 tool port + 1 closure (Regulation tick).
pub struct TurnDeps<'a> {
    pub executor: &'a dyn TurnExecutor,
    pub gas: &'a dyn GasGovernor,
    pub tools: &'a dyn ToolPort,
    pub threads: &'a mut dyn ThreadMemory,
    pub on_reg_update: &'a dyn Fn(),
}

// ── Production adapters ──────────────────────────────────────────────

/// Wraps `ChatService::execute_turn`. Holds all data needed to build
/// a `TurnRequest` from a `TurnInput` — port types stay private.
pub struct ReplTurnExecutor {
    ctx: Arc<AgentService>,
    manifest_executor: Option<hkask_templates::ManifestExecutor>,
    manifest: Option<hkask_templates::BundleManifest>,
    settings: super::ReplSettings,
    current_agent: String,
    current_model: String,
    agent_webid: WebID,
    tool_definitions: Vec<hkask_types::ChatToolDefinition>,
    improv_mode: Option<hkask_services_chat::ImprovMode>,
}

impl ReplTurnExecutor {
    pub fn from_state(state: &super::ReplState) -> Self {
        Self {
            ctx: state.service_context.clone(),
            manifest_executor: state.manifest_state.as_ref().map(|c| c.executor.clone()),
            manifest: state.manifest_state.as_ref().map(|c| c.manifest.clone()),
            settings: state.repl_settings.clone(),
            current_agent: state.current_agent.clone(),
            current_model: state.current_model.clone(),
            agent_webid: state.agent_webid,
            improv_mode: state.improv_mode.clone(),
            tool_definitions: state.tool_definitions.clone(),
        }
    }
}

#[async_trait::async_trait]
impl TurnExecutor for ReplTurnExecutor {
    async fn execute_turn(&self, input: &TurnInput<'_>) -> Result<TurnResult, ServiceError> {
        let settings = &self.settings;
        let mem = self
            .ctx
            .per_agent_memory(&self.current_agent)
            .expect("per-agent memory");
        let req = TurnRequest {
            input: input.input.to_string(),
            userpod_name: input
                .agent_override
                .unwrap_or(&self.current_agent)
                .to_string(),
            model: self.current_model.clone(),
            inference_port: self.ctx.inference_port().expect("inference port"),
            episodic_storage: mem.episodic_storage,
            semantic_storage: mem.semantic_storage,
            agent_webid: self.agent_webid,
            api_spec: None,
            llm_params: super::handlers::to_llm_params(settings),
            capability_checker: self.ctx.governance().checker.clone(),
            system_webid: *self.ctx.webid(),
            iteration: input.iteration,
            auto_condense: settings.auto_condense,
            context_window: settings.model_meta.as_ref().map(|m| m.context_length),
            thread_messages: input.thread_messages.clone(),
            improv_mode: self.improv_mode.clone(),
            tool_section: String::new(),
            tools: if self.tool_definitions.is_empty() {
                None
            } else {
                Some(self.tool_definitions.clone())
            },
            prebuilt_messages: input.messages.clone(),
        };
        ChatService::execute_turn(
            &self.ctx,
            &req,
            self.manifest_executor.as_ref(),
            self.manifest.as_ref(),
        )
        .await
    }
}

/// Wraps `EnergyGuard` + gas status.
pub struct ReplGasGovernor {
    cybernetics_loop: Arc<tokio::sync::RwLock<hkask_regulation::CyberneticsLoop>>,
    inference_loop: Arc<hkask_pods::InferenceLoop>,
    webid: WebID,
    rt: tokio::runtime::Handle,
    ctx: Arc<AgentService>,
}

impl ReplGasGovernor {
    pub fn from_state(state: &super::ReplState, rt: &tokio::runtime::Handle) -> Self {
        Self {
            cybernetics_loop: state.service_context.ledger().cybernetics.clone(),
            inference_loop: state
                .service_context
                .inference_loop()
                .expect("inference loop")
                .clone(),
            webid: state.agent_webid,
            rt: rt.clone(),
            ctx: state.service_context.clone(),
        }
    }
}

impl GasGovernor for ReplGasGovernor {
    fn try_reserve(&self, heuristic: u64) -> Option<Box<dyn GasReservation>> {
        EnergyGuard::try_reserve(
            &self.cybernetics_loop,
            &self.inference_loop,
            &self.webid,
            &self.rt,
            heuristic,
        )
        .map(|guard| Box::new(ReplGasReservation { guard: Some(guard) }) as Box<dyn GasReservation>)
    }
    fn gas_status(&self) -> (u64, u64) {
        (
            self.ctx.gas_remaining().unwrap_or(0),
            self.ctx.gas_cap().unwrap_or(0),
        )
    }
}

struct ReplGasReservation {
    guard: Option<EnergyGuard>,
}

impl GasReservation for ReplGasReservation {
    fn heuristic(&self) -> u64 {
        self.guard.as_ref().map(|g| g.heuristic()).unwrap_or(0)
    }
    fn settle(&mut self, actual: u64) {
        if let Some(g) = self.guard.take() {
            g.settle(actual);
        }
    }
    fn release(&mut self) {
        if let Some(g) = self.guard.take() {
            g.release();
        }
    }
}

/// Wraps `ThreadRegistry`.
pub struct ReplThreadMemory<'a> {
    registry: &'a mut ThreadRegistry,
}

impl<'a> ReplThreadMemory<'a> {
    pub fn new(registry: &'a mut ThreadRegistry) -> Self {
        Self { registry }
    }
}

impl<'a> ThreadMemory for ReplThreadMemory<'a> {
    fn is_seeded(&self) -> bool {
        self.registry.seeded
    }
    fn thread_history_messages(&self, window: usize) -> Option<Vec<hkask_types::ChatMessage>> {
        self.registry.thread_history_messages(Some(window))
    }
    fn append_turn(&mut self, agent: &str, input: &str, response: &str) {
        self.registry.append_turn(agent, input, response);
    }
    fn mark_seeded(&mut self) {
        self.registry.mark_seeded();
    }
}
