//! Inference router — multi-provider `InferencePort` implementation.
//!
//! Routes requests to DeepInfra, fal.ai, Together AI, OpenRouter, or KiloCode based on the
//! 2-letter provider prefix in the model name. Unprefixed model names
//! use the configured default provider.

use crate::cline_backend::ClineBackend;
use crate::config::{FusionConfig, InferenceConfig, ProviderId};
use crate::deepinfra_backend::DeepInfraBackend;
use crate::embedding_router::EmbeddingRouter;
use crate::fal_backend::FalBackend;
use crate::kilocode_backend::KiloCodeBackend;
use crate::ollama_backend::OllamaBackend;
use crate::openrouter_backend::OpenRouterBackend;
use crate::runpod_backend::RunpodBackend;
use crate::together_backend::TogetherBackend;
use hkask_regulation::{CyberneticsLoop, GasCost};
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult};
use hkask_types::{RegulationSink, WebID};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub(crate) mod backend;
mod dispatch;
mod inference_port;
mod media;
mod models;
pub(crate) use backend::{ChatBackend, VisionBackend};

type HealCallback = Box<dyn Fn(&str, &str) + Send + Sync>;

#[derive(Clone)]
struct InferenceGovernance {
    cybernetics: Arc<RwLock<CyberneticsLoop>>,
    event_sink: Arc<dyn RegulationSink>,
    agent: WebID,
}

/// Multi-provider inference router implementing `InferencePort`.
///
/// Parses the `XX/` prefix from model names and dispatches to the appropriate
/// backend via two small match-fns (`chat_backend`/`vision_backend`) that return
/// `&dyn ChatBackend`/`&dyn VisionBackend` borrowed from the typed fields below.
/// Adding a provider = implement the trait(s) + add a field + construct it in
/// `new` + add a match arm in `chat_backend`/`vision_backend`. The typed fields are
/// the single source of truth (no separate map, no `Arc`, no dual storage) —
/// Fal/DeepInfra's specialist media methods read them directly too.
pub struct InferenceRouter {
    config: InferenceConfig,
    deepinfra: Option<DeepInfraBackend>,
    fal: Option<FalBackend>,
    together: Option<TogetherBackend>,
    openrouter: Option<OpenRouterBackend>,
    kilocode: Option<KiloCodeBackend>,
    runpod: Option<RunpodBackend>,
    ollama: Option<OllamaBackend>,
    cline: Option<ClineBackend>,
    embedding: EmbeddingRouter,
    heal_error_cb: Option<HealCallback>,
    governance: Option<InferenceGovernance>,
}

impl InferenceRouter {
    /// Build the router from an `InferenceConfig`.
    ///
    /// Create a new inference router from config.
    ///
    /// Constructs backends lazily — a backend is only created if its
    /// configuration is valid (e.g., API key is present for cloud providers).
    ///
    /// # Availability gate (coding-guidelines + pragmatic-laziness)
    ///
    /// Gates on **configuration**, not reachability: cloud backends require a
    /// non-empty API key; Ollama requires a non-empty base URL (defaults to
    /// `localhost:11434`, so it is `Some` even when the daemon is down). A `None`
    /// backend is skipped by `list_models` (contributes no models) and rejected by
    /// `resolve_chat` (clear "provider not available" error) — so unconfigured
    /// providers never appear in the model list and can't be used for inference.
    ///
    /// Reachability is detected **lazily at use**, not at construction: an Ollama
    /// daemon that is down yields an empty `list_models` and a connection error on
    /// dispatch. This avoids a blocking network probe in the sync constructor and
    /// keeps startup fast — the cheapest knowable signal (config presence) gates
    /// construction; the runtime signal (reachability) gates use.
    ///
    /// expect: "The system creates multi-provider membranes assembled from configured boundaries"
    /// \[P4\] Motivating: Clear Boundaries — multi-provider membrane assembled from configured boundaries
    /// pre:  config is a valid InferenceConfig
    /// post: returns InferenceRouter with backends for configured providers
    pub fn new(config: InferenceConfig) -> Self {
        let shared_client = config
            .build_client()
            .map(Arc::new)
            .map_err(|e| warn!(target: "reg.inference", "HTTP client build failed: {}", e))
            .ok();

        let deepinfra = shared_client
            .as_ref()
            .and_then(|c| DeepInfraBackend::new(&config, Arc::clone(c)).ok());
        let fal = shared_client
            .as_ref()
            .and_then(|c| FalBackend::new(&config, Arc::clone(c)).ok());
        let together = shared_client
            .as_ref()
            .and_then(|c| TogetherBackend::new(&config, Arc::clone(c)).ok());
        let openrouter = shared_client
            .as_ref()
            .and_then(|c| OpenRouterBackend::new(&config, Arc::clone(c)).ok());
        let kilocode = shared_client
            .as_ref()
            .and_then(|c| KiloCodeBackend::new(&config, Arc::clone(c)).ok());
        let runpod = shared_client
            .as_ref()
            .and_then(|c| RunpodBackend::new(&config, Arc::clone(c)).ok());
        let ollama = shared_client
            .as_ref()
            .and_then(|c| OllamaBackend::new(&config, Arc::clone(c)).ok());
        let cline = shared_client
            .as_ref()
            .and_then(|c| ClineBackend::new(&config, Arc::clone(c)).ok());

        if deepinfra.is_none() {
            warn!(target: "reg.inference", "DeepInfra backend unavailable (no API key)");
        }
        if fal.is_none() {
            warn!(target: "reg.inference", "fal.ai backend unavailable (no API key)");
        }
        if together.is_none() {
            warn!(target: "reg.inference", "Together AI backend unavailable (no API key)");
        }
        if openrouter.is_none() {
            warn!(target: "reg.inference", "OpenRouter backend unavailable (no API key)");
        }
        if kilocode.is_none() {
            warn!(target: "reg.inference", "KiloCode backend unavailable (no API key)");
        }
        if runpod.is_none() {
            warn!(target: "reg.inference", "RunPod backend unavailable (no API key or template)");
        }
        if ollama.is_none() {
            warn!(target: "reg.inference", "Ollama backend unavailable (no base URL)");
        }
        if cline.is_none() {
            warn!(target: "reg.inference", "Cline backend unavailable (no API key)");
        }

        let embedding = shared_client
            .as_ref()
            .map(|c| EmbeddingRouter::with_client(&config, Arc::clone(c)))
            .unwrap_or_else(|| EmbeddingRouter::new(config.clone()));

        Self {
            config: config.clone(),
            deepinfra,
            fal,
            together,
            openrouter,
            kilocode,
            runpod,
            ollama,
            cline,
            embedding,
            heal_error_cb: None,
            governance: None,
        }
    }

    /// Enable gas accounting and Regulation events for inference calls.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_governance(
        mut self,
        cybernetics: Arc<RwLock<CyberneticsLoop>>,
        event_sink: Arc<dyn RegulationSink>,
        agent: WebID,
    ) -> Self {
        self.governance = Some(InferenceGovernance {
            cybernetics,
            event_sink,
            agent,
        });
        self
    }

    async fn generate_governed(
        &self,
        governance: InferenceGovernance,
        prompt: String,
        parameters: LLMParameters,
        model_override: Option<String>,
        tools: Option<Vec<ChatToolDefinition>>,
    ) -> Result<InferenceResult, InferenceError> {
        let estimated = GasCost(u64::from(parameters.max_tokens.max(1)));
        let model = model_override.as_deref().unwrap_or("default");

        let cybernetics = governance.cybernetics.read().await;
        if !cybernetics.can_proceed(&governance.agent, estimated).await {
            return Err(InferenceError::Generation(
                "Energy budget exceeded for inference call".into(),
            ));
        }
        cybernetics
            .reserve_gas(&governance.agent, estimated)
            .await
            .map_err(|error| {
                InferenceError::Generation(format!("Gas reservation failed: {error}"))
            })?;
        drop(cybernetics);

        let invoked = RegulationRecord::new(
            governance.agent,
            Span::new(
                SpanNamespace::try_from(RegulationSpan::Inference).expect("canonical span"),
                "invoked",
            ),
            CyclePhase::Sense,
            serde_json::json!({
                "model": model,
                "estimated_cost": estimated.0,
                "max_tokens": parameters.max_tokens,
                "settled": false,
            }),
            0,
        );
        if let Err(error) = governance.event_sink.persist(&invoked) {
            warn!(target: "reg.inference", %error, "failed to persist inference invocation");
        }

        let result = self
            .generate_ungoverned(
                &prompt,
                &parameters,
                model_override.as_deref(),
                tools.as_deref(),
            )
            .await;
        let actual = if result.is_ok() {
            estimated.0
        } else {
            estimated.0 / 2
        };

        if let Err(error) = governance
            .cybernetics
            .read()
            .await
            .settle_gas(&governance.agent, estimated, GasCost(actual))
            .await
        {
            warn!(target: "reg.inference", %error, "failed to settle inference gas");
        }

        let (status, error, tokens_used) = match &result {
            Ok(response) => ("success", None, Some(response.usage.total_tokens)),
            Err(error) => ("failure", Some(error.to_string()), None),
        };
        let completed = RegulationRecord::new(
            governance.agent,
            Span::new(
                SpanNamespace::try_from(RegulationSpan::Inference).expect("canonical span"),
                "completed",
            ),
            CyclePhase::Act,
            serde_json::json!({
                "model": model,
                "estimated_cost": estimated.0,
                "actual_cost": actual,
                "status": status,
                "error": error,
                "tokens_used": tokens_used,
                "settled": true,
            }),
            0,
        )
        .with_parent(invoked.id);
        if let Err(error) = governance.event_sink.persist(&completed) {
            warn!(target: "reg.inference", %error, "failed to persist inference outcome");
        }

        info!(
            target: "reg.inference",
            agent = ?governance.agent,
            model,
            reserved = estimated.0,
            actual,
            status,
            "inference gas settled"
        );
        result
    }

    /// Multi-turn variant of `generate_governed` — accepts an explicit message
    /// array. Performs the same gas estimation, reservation, and settlement as
    /// `generate_governed`, but delegates to `generate_ungoverned_messages`.
    async fn generate_governed_messages(
        &self,
        governance: InferenceGovernance,
        messages: Vec<ChatMessage>,
        parameters: LLMParameters,
        model_override: Option<String>,
        tools: Option<Vec<ChatToolDefinition>>,
    ) -> Result<InferenceResult, InferenceError> {
        let estimated = GasCost(u64::from(parameters.max_tokens.max(1)));
        let model = model_override.as_deref().unwrap_or("default");

        let cybernetics = governance.cybernetics.read().await;
        if !cybernetics.can_proceed(&governance.agent, estimated).await {
            return Err(InferenceError::Generation(
                "Energy budget exceeded for inference call".into(),
            ));
        }
        cybernetics
            .reserve_gas(&governance.agent, estimated)
            .await
            .map_err(|error| {
                InferenceError::Generation(format!("Gas reservation failed: {error}"))
            })?;
        drop(cybernetics);

        let invoked = RegulationRecord::new(
            governance.agent,
            Span::new(
                SpanNamespace::try_from(RegulationSpan::Inference).expect("canonical span"),
                "invoked",
            ),
            CyclePhase::Sense,
            serde_json::json!({
                "model": model,
                "estimated_cost": estimated.0,
                "max_tokens": parameters.max_tokens,
                "messages": messages.len(),
                "settled": false,
            }),
            0,
        );
        if let Err(error) = governance.event_sink.persist(&invoked) {
            warn!(target: "reg.inference", %error, "failed to persist inference invocation");
        }

        let result = self
            .generate_ungoverned_messages(
                &messages,
                &parameters,
                model_override.as_deref(),
                tools.as_deref(),
            )
            .await;
        let actual = if result.is_ok() {
            estimated.0
        } else {
            estimated.0 / 2
        };

        if let Err(error) = governance
            .cybernetics
            .read()
            .await
            .settle_gas(&governance.agent, estimated, GasCost(actual))
            .await
        {
            warn!(target: "reg.inference", %error, "failed to settle inference gas");
        }

        let (status, error, tokens_used) = match &result {
            Ok(response) => ("success", None, Some(response.usage.total_tokens)),
            Err(error) => ("failure", Some(error.to_string()), None),
        };
        let completed = RegulationRecord::new(
            governance.agent,
            Span::new(
                SpanNamespace::try_from(RegulationSpan::Inference).expect("canonical span"),
                "completed",
            ),
            CyclePhase::Act,
            serde_json::json!({
                "model": model,
                "estimated_cost": estimated.0,
                "actual_cost": actual,
                "status": status,
                "error": error,
                "tokens_used": tokens_used,
                "messages": messages.len(),
                "settled": true,
            }),
            0,
        )
        .with_parent(invoked.id);
        if let Err(error) = governance.event_sink.persist(&completed) {
            warn!(target: "reg.inference", %error, "failed to persist inference outcome");
        }

        info!(
            target: "reg.inference",
            agent = ?governance.agent,
            model,
            reserved = estimated.0,
            actual,
            status,
            "inference gas settled (messages)"
        );
        result
    }

    /// Attach a self-healing callback for automatic error recovery.
    /// The callback receives (error_string, operation_name) and should
    /// delegate to a SelfHealer instance.
    pub fn with_heal_cb(mut self, cb: HealCallback) -> Self {
        self.heal_error_cb = Some(cb);
        self
    }

    fn heal_error(&self, error: InferenceError, operation: &str) -> InferenceError {
        if let Some(ref cb) = self.heal_error_cb {
            cb(&error.to_string(), operation);
        }
        error
    }

    /// Return the chat backend for a provider as a trait object, or `None` if
    /// the provider is not configured (or is vision-only, e.g. RunPod).
    ///
    /// This is the single dispatch point for chat — `dispatch_generate`,
    /// `dispatch_generate_stream`, `resolve_chat`, and `list_models` all go
    /// through it, so adding a chat provider = add a field + arm here.
    fn chat_backend(&self, provider: ProviderId) -> Option<&dyn ChatBackend> {
        match provider {
            ProviderId::DeepInfra => self.deepinfra.as_ref().map(|b| b as &dyn ChatBackend),
            ProviderId::Fal => self.fal.as_ref().map(|b| b as &dyn ChatBackend),
            ProviderId::Together => self.together.as_ref().map(|b| b as &dyn ChatBackend),
            ProviderId::OpenRouter => self.openrouter.as_ref().map(|b| b as &dyn ChatBackend),
            ProviderId::KiloCode => self.kilocode.as_ref().map(|b| b as &dyn ChatBackend),
            ProviderId::Ollama => self.ollama.as_ref().map(|b| b as &dyn ChatBackend),
            ProviderId::Cline => self.cline.as_ref().map(|b| b as &dyn ChatBackend),
            // RunPod is vision/OCR-only — it is not a ChatBackend.
            ProviderId::Runpod => None,
        }
    }

    /// Return the vision backend for a provider as a trait object, or `None` if
    /// the provider is not configured. All chat-capable providers plus RunPod
    /// implement `VisionBackend`.
    fn vision_backend(&self, provider: ProviderId) -> Option<&dyn VisionBackend> {
        match provider {
            ProviderId::DeepInfra => self.deepinfra.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::Fal => self.fal.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::Together => self.together.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::OpenRouter => self.openrouter.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::KiloCode => self.kilocode.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::Ollama => self.ollama.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::Cline => self.cline.as_ref().map(|b| b as &dyn VisionBackend),
            ProviderId::Runpod => self.runpod.as_ref().map(|b| b as &dyn VisionBackend),
        }
    }

    /// Compute the effective model name, applying the fusion override when active.
    ///
    /// Priority: per-call `params.fusion_config` > global `config.fusion` >
    /// explicit model > default model. When `params.bypass_fusion` is true,
    /// all fusion overrides are skipped.
    fn effective_model(&self, explicit: Option<&str>, params: &LLMParameters) -> String {
        if !params.bypass_fusion {
            if let Some(fusion) = &params.fusion_config {
                return fusion.model_id();
            }
            if let Some(ref fusion) = self.config.fusion {
                return fusion.model_id();
            }
        }
        explicit.unwrap_or(&self.config.default_model).to_string()
    }

    /// Parse a model name into `(provider, stripped_model)` with unknown-prefix rejection.
    ///
    /// Unprefixed names (no `XX/` shape) route to the configured default provider.
    /// A name with two uppercase letters followed by `/` that is not a recognized
    /// provider code is rejected explicitly (fail fast on unknown prefix).
    ///
    /// expect: "The system parses provider routing from model names"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — fail fast on unknown prefix
    /// pre:  model is non-empty
    /// post: returns Ok((ProviderId, stripped_model)) for recognized or unprefixed models
    /// post: returns Err(Connection) for unrecognized `XX/` prefix
    fn parse_provider<'a>(&self, model: &'a str) -> Result<(ProviderId, &'a str), InferenceError> {
        match ProviderId::parse_from_model(model) {
            Some(parsed) => Ok(parsed),
            None => {
                if ProviderId::looks_like_prefix(model) {
                    return Err(InferenceError::Connection(format!(
                        "Unknown provider prefix '{}/' in model '{}'",
                        &model[..2],
                        model
                    )));
                }
                Ok((self.config.default_provider, model))
            }
        }
    }

    /// Resolve which **chat** backend to use for a given model name.
    ///
    /// Returns `(provider, backend_model_name)` or an error if no chat backend
    /// is available for the requested provider, or if the model carries an
    /// unrecognized `XX/` prefix.
    ///
    /// This is chat-only: RunPod (vision/OCR-only) is not a chat backend and
    /// will resolve to "not available" here — vision callers use the vision
    /// dispatch path (`media::generate_vision` → `vision_backend`), not this.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — fail fast on unknown prefix
    /// pre:  model is non-empty
    /// post: returns Ok((ProviderId, stripped_model)) for recognized or unprefixed models
    /// post: returns Err(Connection) for unrecognized `XX/` prefix or unavailable chat backend
    pub(crate) fn resolve_chat<'a>(
        &self,
        model: &'a str,
    ) -> Result<(ProviderId, &'a str), InferenceError> {
        let (provider, stripped_model) = self.parse_provider(model)?;
        if self.chat_backend(provider).is_none() {
            return Err(InferenceError::Connection(format!(
                "Provider {} is not available (check configuration)",
                provider.as_str()
            )));
        }
        Ok((provider, stripped_model))
    }

    // ── Embedding ──────────────────────────────────────────────────────────

    /// Generate a text embedding vector via the embedding router.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated embedding dispatch
    /// pre:  text is a non-empty string
    /// post: delegates to EmbeddingRouter::embed_sentence with resolved model
    /// post: if embedding fails → Err(EmbeddingGenerationError)
    #[must_use = "result must be used"]
    pub async fn embed_text(
        &self,
        text: &str,
        model_override: Option<&str>,
    ) -> Result<Vec<f32>, hkask_types::EmbeddingGenerationError> {
        let model = model_override.unwrap_or(&self.config.default_model);
        self.embedding.embed_sentence(model, text).await
    }

    /// Provider-agnostic fusion orchestration.
    ///
    /// Delegates to the fusion orchestrator which dispatches to panel
    /// models in parallel, then routes to the configured fusion mode.
    ///
    /// expect: "Fusion orchestrates multi-model deliberation provider-agnostically"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — hKask-side fusion orchestration
    /// pre:  fusion.panel is non-empty, fusion.judge is valid
    /// post: returns judge output per the configured mode
    async fn orchestrate_fusion(
        &self,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
        fusion: &FusionConfig,
    ) -> Result<InferenceResult, InferenceError> {
        crate::fusion_orchestrator::orchestrate(self, prompt, params, tools, fusion).await
    }

    /// Verify that the configured fusion judge model is *configured* (its
    /// provider backend is available).
    ///
    /// Resolves the judge model name to a provider and checks backend
    /// availability (config presence — not a network reachability probe).
    /// Returns Ok(true) if the backend is available, Ok(false) if not, or
    /// Err on resolution failure.
    ///
    /// Note: this is a configuration check, not a reachability check — it
    /// confirms the judge model's provider is configured but does not ping it.
    /// Reachability is detected lazily at use time.
    ///
    /// expect: "Fusion model is verified before use to prevent unexpected costs"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — proactive cost-safety check
    /// pre:  config.fusion may be None or Some
    /// post: if Some → resolves judge model to verify provider availability
    /// post: if None → returns Ok(true) immediately (nothing to verify)
    #[must_use = "result must be used"]
    pub async fn verify_fusion_model(&self) -> Result<bool, InferenceError> {
        let fusion = match &self.config.fusion {
            Some(f) => f,
            None => return Ok(true),
        };

        match self.resolve_chat(&fusion.judge) {
            Ok((provider, _)) => {
                tracing::info!(
                    target: "reg.inference",
                    fusion_judge = %fusion.judge,
                    provider = %provider.as_str(),
                    panel_count = fusion.panel.len(),
                    "Fusion judge model provider is configured"
                );
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    fusion_judge = %fusion.judge,
                    error = %e,
                    "Fusion judge model provider NOT configured"
                );
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AlgoMethod, FusionConfig, FusionMode, InferenceConfig, NonEmptyVec};

    fn config_with_fusion(judge: Option<&str>, panel: Option<&[&str]>) -> InferenceConfig {
        InferenceConfig {
            fusion: judge.map(|j| FusionConfig {
                judge: j.to_string(),
                panel: NonEmptyVec::from_vec(
                    panel
                        .unwrap_or(&["default-panel"])
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                )
                .expect("config_with_fusion panel must not be empty"),
                mode: FusionMode::Synthesis,
                skills: Vec::new(),
                max_rounds: 5,
                algo_method: AlgoMethod::default(),
            }),
            ..Default::default()
        }
    }

    // ── C1: effective_model routing ────────────────────────────────────

    /// REQ: P9-inf-fusion-effective-model-routing
    /// expect: "Fusion model overrides default when configured and not bypassed" [P9]
    #[test]
    fn effective_model_routes_to_fusion() {
        let config = config_with_fusion(Some("kask"), Some(&["Kimi2.7", "Qwen3.7 Max"]));
        let router = InferenceRouter::new(config);
        let params = LLMParameters {
            bypass_fusion: false,
            ..Default::default()
        };
        assert_eq!(router.effective_model(None, &params), "kask");
    }

    /// REQ: P9-inf-fusion-bypass
    /// expect: "Bypass flag prevents fusion override" [P9]
    #[test]
    fn effective_model_bypasses_fusion() {
        let config = config_with_fusion(Some("kask"), Some(&["Kimi2.7"]));
        let default = config.default_model.clone();
        let router = InferenceRouter::new(config);
        let params = LLMParameters {
            bypass_fusion: true,
            ..Default::default()
        };
        assert_eq!(router.effective_model(None, &params), default);
    }

    /// REQ: P9-inf-fusion-effective-model-explicit
    /// expect: "Explicit model used when fusion is None" [P9]
    #[test]
    fn effective_model_uses_explicit_when_no_fusion() {
        let config = config_with_fusion(None, None);
        let router = InferenceRouter::new(config);
        let params = LLMParameters::default();
        assert_eq!(
            router.effective_model(Some("DI/custom-model"), &params),
            "DI/custom-model"
        );
    }

    /// REQ: P9-inf-fusion-default-fallback
    /// expect: "Default model used when nothing overrides" [P9]
    #[test]
    fn effective_model_falls_back_to_default() {
        let config = config_with_fusion(None, None);
        let default = config.default_model.clone();
        let router = InferenceRouter::new(config);
        let params = LLMParameters::default();
        assert_eq!(router.effective_model(None, &params), default);
    }

    // ── C2: Per-call fusion_config override ───────────────────────────

    /// REQ: P9-inf-fusion-per-call-override
    /// expect: "Per-call fusion_config overrides global config judge model" [P9]
    #[test]
    fn per_call_fusion_config_overrides_global() {
        let config = config_with_fusion(Some("global-judge"), Some(&["Kimi2.7"]));
        let router = InferenceRouter::new(config);
        let params = LLMParameters {
            bypass_fusion: false,
            fusion_config: Some(FusionConfig {
                judge: "manifest-judge".to_string(),
                panel: NonEmptyVec::one("Qwen3.7 Max".to_string()),
                mode: FusionMode::Critique,
                skills: Vec::new(),
                max_rounds: 3,
                algo_method: AlgoMethod::default(),
            }),
            system_prompt: None,
            ..Default::default()
        };
        assert_eq!(router.effective_model(None, &params), "manifest-judge");
    }

    /// REQ: P9-inf-fusion-per-call-no-global
    /// expect: "Per-call fusion_config used when no global config exists" [P9]
    #[test]
    fn per_call_fusion_config_without_global() {
        let config = config_with_fusion(None, None);
        let router = InferenceRouter::new(config);
        let params = LLMParameters {
            bypass_fusion: false,
            fusion_config: Some(FusionConfig {
                judge: "manifest-only-judge".to_string(),
                panel: NonEmptyVec::one("GLM5.2".to_string()),
                mode: FusionMode::Synthesis,
                skills: Vec::new(),
                max_rounds: 5,
                algo_method: AlgoMethod::default(),
            }),
            system_prompt: None,
            ..Default::default()
        };
        assert_eq!(router.effective_model(None, &params), "manifest-only-judge");
    }

    /// REQ: P9-inf-fusion-per-call-bypass
    /// expect: "Bypass flag overrides per-call fusion_config" [P9]
    #[test]
    fn per_call_fusion_config_bypassed() {
        let config = config_with_fusion(Some("global-judge"), Some(&["Kimi2.7"]));
        let default = config.default_model.clone();
        let router = InferenceRouter::new(config);
        let params = LLMParameters {
            bypass_fusion: true,
            fusion_config: Some(FusionConfig {
                judge: "manifest-judge".to_string(),
                panel: NonEmptyVec::one("Qwen3.7 Max".to_string()),
                mode: FusionMode::Synthesis,
                skills: Vec::new(),
                max_rounds: 5,
                algo_method: AlgoMethod::default(),
            }),
            system_prompt: None,
            ..Default::default()
        };
        assert_eq!(router.effective_model(None, &params), default);
    }

    // ── Ollama provider routing ────────────────────────────────────────

    /// REQ: P9-inf-ollama-prefix-routing
    /// expect: "OM/ prefix routes to the Ollama provider with the tag stripped" [P9]
    #[test]
    fn parse_from_model_routes_ollama_prefix() {
        assert_eq!(
            ProviderId::parse_from_model("OM/qwen3:8b"),
            Some((ProviderId::Ollama, "qwen3:8b"))
        );
    }

    /// REQ: P9-inf-ollama-prefix-routing
    /// expect: "Ollama provider formats models with the OM/ prefix" [P9]
    #[test]
    fn ollama_prefix_format() {
        assert_eq!(ProviderId::Ollama.prefix_model("qwen3:8b"), "OM/qwen3:8b");
        assert_eq!(ProviderId::Ollama.as_str(), "OM");
    }

    /// REQ: P9-inf-ollama-resolve
    /// expect: "Router resolves an OM/-prefixed model to the Ollama chat backend" [P9]
    #[test]
    fn resolve_chat_routes_ollama_model() {
        let config = InferenceConfig::default();
        // Default config sets ollama_base_url, so the backend constructs.
        if config.ollama_base_url.is_empty() {
            // Environment stripped the default; skip rather than assert a false negative.
            return;
        }
        let router = InferenceRouter::new(config);
        let (provider, model) = router
            .resolve_chat("OM/qwen3:8b")
            .expect("OM/-prefixed model should resolve");
        assert_eq!(provider, ProviderId::Ollama);
        assert_eq!(model, "qwen3:8b");
    }

    /// REQ: P9-inf-ollama-default-provider
    /// expect: "Unprefixed model routes to Ollama when it is the default provider" [P9]
    #[test]
    fn unprefixed_model_uses_ollama_default() {
        let config = InferenceConfig {
            default_provider: ProviderId::Ollama,
            ..InferenceConfig::default()
        };
        if config.ollama_base_url.is_empty() {
            return;
        }
        let router = InferenceRouter::new(config);
        let (provider, _model) = router
            .resolve_chat("qwen3:8b")
            .expect("unprefixed model with Ollama default should resolve");
        assert_eq!(provider, ProviderId::Ollama);
    }

    // ── Cline provider routing ─────────────────────────────────────────

    /// REQ: P9-inf-cline-prefix-routing
    /// expect: "CL/ prefix routes to the Cline provider with the org/model stripped" [P9]
    #[test]
    fn parse_from_model_routes_cline_prefix() {
        assert_eq!(
            ProviderId::parse_from_model("CL/anthropic/claude-sonnet-4-6"),
            Some((ProviderId::Cline, "anthropic/claude-sonnet-4-6"))
        );
    }

    /// REQ: P9-inf-cline-prefix-routing
    /// expect: "Cline provider formats models with the CL/ prefix" [P9]
    #[test]
    fn cline_prefix_format() {
        assert_eq!(
            ProviderId::Cline.prefix_model("openai/gpt-4o"),
            "CL/openai/gpt-4o"
        );
        assert_eq!(ProviderId::Cline.as_str(), "CL");
    }

    /// REQ: P9-inf-cline-resolve
    /// expect: "Router resolves a CL/-prefixed model; unavailable without a key" [P9]
    #[test]
    fn resolve_chat_cline_unavailable_without_key() {
        // Default config has no CLINE_API_KEY → cline backend is None → resolve_chat errors.
        let config = InferenceConfig::default();
        let router = InferenceRouter::new(config);
        let err = router.resolve_chat("CL/anthropic/claude-sonnet-4-6");
        assert!(
            err.is_err(),
            "CL/ model should not resolve without CLINE_API_KEY"
        );
    }

    /// REQ: P9-inf-unknown-prefix-reject
    /// expect: "An unrecognized XX/ prefix is rejected, not silently routed to the default provider" [P9]
    #[test]
    fn resolve_chat_unknown_prefix_errors() {
        let config = InferenceConfig {
            default_provider: ProviderId::Ollama,
            ..InferenceConfig::default()
        };
        if config.ollama_base_url.is_empty() {
            return;
        }
        let router = InferenceRouter::new(config);
        assert!(
            router.resolve_chat("qwen3:8b").is_ok(),
            "unprefixed model with Ollama default should resolve"
        );
        let err = router.resolve_chat("BT/foo").unwrap_err();
        assert!(
            err.to_string().contains("Unknown provider prefix"),
            "BT/ should be rejected as unknown prefix"
        );
        let err = router.resolve_chat("XX/model").unwrap_err();
        assert!(
            err.to_string().contains("Unknown provider prefix"),
            "XX/ should be rejected as unknown prefix"
        );
    }

    /// REQ: P9-inf-runpod-chat-not-available
    /// expect: "RunPod is vision-only — chat resolution reports it not available" [P9]
    #[test]
    fn resolve_chat_runpod_not_a_chat_backend() {
        // RunPod is vision/OCR-only: even when configured, resolve_chat must
        // reject it (it is not in the chat_backend match). Configure RunPod via
        // env so the backend constructs, then assert chat resolution fails but
        // vision dispatch would succeed.
        // SAFETY: single-threaded test; set/restore env.
        unsafe {
            std::env::set_var("RUNPOD_API_KEY", "test-key");
            std::env::set_var("RUNPOD_TEMPLATE_ID", "test-template");
        }
        let config = InferenceConfig::default();
        let router = InferenceRouter::new(config);
        let err = router.resolve_chat("RP/kask-ocr").unwrap_err();
        assert!(
            err.to_string().contains("not available"),
            "RP/ chat resolution should report RunPod not available, got: {err}"
        );
        // Vision dispatch path: vision_backend(Runpod) should be Some when configured.
        // (RunpodBackend::new reads RUNPOD_API_KEY/RUNPOD_TEMPLATE_ID; if env present it constructs.)
        unsafe {
            std::env::remove_var("RUNPOD_API_KEY");
            std::env::remove_var("RUNPOD_TEMPLATE_ID");
        }
    }
}
