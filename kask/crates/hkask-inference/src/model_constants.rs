//! Model name resolution — env-configurable with code defaults.
//!
//! Every model used in the system has a corresponding env var for override.
//! The accessors here read the ENV LAYER only: `HKASK_*` → `None` when unset.
//! They are not the whole chain — the settings layers carry the code defaults
//! (operator ruling 2026-09-04, superseding the former no-hidden-models
//! spec): `KaskModelsSettings::default()` / `HkaskSettings::default()` hold
//! the default model names, settings.json / the settings UI override them,
//! and `mcp_env()` injects the resolved values into MCP server children as
//! these env vars. A `None` from these accessors therefore means "env not
//! injected" — in practice only reachable for direct CLI callers that
//! bypass the settings chain.
//!
//! Naming convention:
//! QA generation is intentionally unconfigured by default (2026-09-11).
//! Unlike general inference, its resolver never consults the chat default.
//!
//! - `HKASK_QA_GENERATION_MODEL` — dedicated non-thinking QA generator
//! - `HKASK_CLASSIFIER_MODEL` — primary classifier model
//! - `HKASK_EMBEDDING_MODEL` — default embedding model
//! - `HKASK_OCR_MODEL` — OCR model for scanned PDF fallback
//! - `HKASK_RERANK_MODEL` — rerank model for research deep-search rerank
//! - `HKASK_DEFAULT_MODEL` — the kask default model (settings-layer default,
//!   injected by `mcp_env()`; the direct path fails typed when unset)
//! - `HKASK_PRIVATE_EMBEDDING_MODEL` — the private leg's local embedding
//!   model (D100 item 3 + the query-side provenance routing)
//! - `HKASK_PRIVATE_RERANK_MODEL` — the private leg's local rerank model
//!   (D100 §8.1 routed shape, operator ruling 2026-11-06)
//! - `HKASK_DECISION_MODEL` — the decision model for bounded structured
//!   judgments (the Decisions API, not a chat endpoint)

/// Environment binding for `kask.models.qa_generation_model`.
pub const QA_GENERATION_MODEL_ENV: &str = "HKASK_QA_GENERATION_MODEL";

/// Resolve an explicit tool model before the dedicated QA setting. There is
/// deliberately no generator ID default, chat fallback, or training-base input.
/// Registry/provider validation remains authoritative for model availability.
pub fn resolve_qa_generation_model(
    requested: Option<&str>,
) -> Result<String, hkask_types::InferenceError> {
    let configured = if requested.is_none() {
        match std::env::var(QA_GENERATION_MODEL_ENV) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => {
                return Err(hkask_types::InferenceError::Model(format!(
                    "invalid {QA_GENERATION_MODEL_ENV}: {error}"
                )));
            }
        }
    } else {
        None
    };
    select_qa_generation_model(requested, configured.as_deref())
}

fn select_qa_generation_model(
    requested: Option<&str>,
    configured: Option<&str>,
) -> Result<String, hkask_types::InferenceError> {
    let model = requested.or(configured).ok_or_else(|| {
        hkask_types::InferenceError::NotConfigured(format!(
            "QA generation requires kask.models.qa_generation_model ({QA_GENERATION_MODEL_ENV}) \
             or an explicit tool model; the chat and training base models are never used"
        ))
    })?;
    let qualified = model.split_once('/').is_some_and(|(provider, local)| {
        !provider.is_empty()
            && provider
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !local.is_empty()
            && !local.starts_with('/')
    });
    if !qualified || model.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(hkask_types::InferenceError::Model(format!(
            "invalid QA generation model {model:?}: set kask.models.qa_generation_model \
             ({QA_GENERATION_MODEL_ENV}) or tool model to Provider/model-id; no fallback"
        )));
    }
    Ok(model.to_owned())
}

/// Read the classifier model from the env layer: `HKASK_CLASSIFIER_MODEL`
/// → `None` when unset. The settings chain (which carries the code
/// default) injects this env var for MCP server children.
pub fn classifier_model() -> Option<String> {
    std::env::var("HKASK_CLASSIFIER_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Resolve the classifier model or refuse: `HKASK_CLASSIFIER_MODEL` unset is
/// a typed error naming the setting — never a silent fallback to the host
/// default chat model, which can be thinking-mandatory (OpenRouter 400
/// "Reasoning is mandatory for this endpoint and cannot be disabled" — the
/// d58fd87b class; operator ruling 2026-10-09: remove the fallback). The
/// settings chain injects this env var for MCP server children, so `None`
/// means the env was not injected (direct CLI callers).
pub fn resolve_classifier_model() -> Result<String, hkask_types::InferenceError> {
    classifier_model().ok_or_else(|| {
        hkask_types::InferenceError::NotConfigured(
            "kask.models.classifier_model (HKASK_CLASSIFIER_MODEL) is not set — the \
             classifier-class routes refuse rather than serve the default chat model \
             (which can be thinking-mandatory); set kask.models.classifier_model"
                .to_string(),
        )
    })
}

/// Environment binding for `kask.models.decision_model`.
pub const DECISION_MODEL_ENV: &str = "HKASK_DECISION_MODEL";

/// Read the decision model from the env layer: `HKASK_DECISION_MODEL` →
/// `None` when unset. The settings chain (which carries the code default)
/// injects this env var for MCP server children.
pub fn decision_model() -> Option<String> {
    std::env::var(DECISION_MODEL_ENV)
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Resolve the decision model or refuse: `HKASK_DECISION_MODEL` unset is a
/// typed error naming the setting — never a silent fallback to the chat
/// default. Decision models are a different model class (typed questions
/// in, typed answers with calibrated probabilities out, served over the
/// Decisions API — never a chat endpoint), so the chat default is not a
/// substitute for one; the QA/classifier fail-visible precedent applies.
pub fn resolve_decision_model() -> Result<String, hkask_types::InferenceError> {
    decision_model().ok_or_else(|| {
        hkask_types::InferenceError::NotConfigured(
            "kask.models.decision_model (HKASK_DECISION_MODEL) is not set — decision \
             routes refuse rather than serve the chat model (decision models are a \
             different model class, served over the Decisions API); set \
             kask.models.decision_model"
                .to_string(),
        )
    })
}

/// Read the embedding model from the env layer: `HKASK_EMBEDDING_MODEL`
/// → `None` when unset. The settings chain (which carries the code
/// default) injects this env var for MCP server children.
pub fn embedding_model() -> Option<String> {
    std::env::var("HKASK_EMBEDDING_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// zed-kask: D100's named follow-up (operator-approved 2026-11-06) — the env
/// name carrying the private leg's LOCAL embedding model to kask MCP children
/// (`kask.private_inference.embedding_model` → `mcp_env()` → this var). The
/// curator child routes private-call query embeds through this model ONLY
/// (the registry resolves it to the local ollama descriptor — loopback, no
/// credentials); public calls keep `HKASK_EMBEDDING_MODEL`. Empty/unset is the
/// surfaced degrade — the private semantic leg skips the embed, and the
/// query never widens to the configured cloud embedder.
pub const PRIVATE_EMBEDDING_MODEL_ENV: &str = "HKASK_PRIVATE_EMBEDDING_MODEL";

/// The private leg's LOCAL embedding model as injected into kask MCP children
/// ([`PRIVATE_EMBEDDING_MODEL_ENV`]). Empty/unset reads `None` — the private
/// call degrade, never a fallback to the configured cloud model.
pub fn private_embedding_model() -> Option<String> {
    std::env::var(PRIVATE_EMBEDDING_MODEL_ENV)
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06) — the env
/// name carrying the private leg's LOCAL rerank model to kask MCP children
/// (`kask.private_inference.rerank_model` → `mcp_env()` → this var). The
/// research child routes private-provenance deep-search rerank through this
/// model ONLY (the zed side's local arm serves `ollama/`-qualified rerank
/// models through the local ollama judge protocol — loopback, no
/// credentials); public calls keep `HKASK_RERANK_MODEL`. Empty/unset is the
/// surfaced skip — the private rerank stage degrades to heuristic order and
/// the private query and candidates never widen to the configured cloud
/// reranker.
pub const PRIVATE_RERANK_MODEL_ENV: &str = "HKASK_PRIVATE_RERANK_MODEL";

/// The private leg's LOCAL rerank model as injected into kask MCP children
/// ([`PRIVATE_RERANK_MODEL_ENV`]). Empty/unset reads `None` — the surfaced
/// heuristic skip, never a fallback to the configured cloud reranker.
pub fn private_rerank_model() -> Option<String> {
    std::env::var(PRIVATE_RERANK_MODEL_ENV)
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Read the OCR model from the env layer: `HKASK_OCR_MODEL` → `None`
/// when unset. The settings chain (which carries the code default)
/// injects this env var for MCP server children.
pub fn ocr_model() -> Option<String> {
    std::env::var("HKASK_OCR_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Read the rerank model from the env layer: `HKASK_RERANK_MODEL` →
/// `None` when unset. The settings chain carries `DEFAULT_RERANK_MODEL`
/// as the code default; a `None` here means the env was not injected
/// (direct CLI callers) — the research server surfaces a typed error
/// naming the setting rather than silently skipping the rerank stage.
pub fn rerank_model() -> Option<String> {
    std::env::var("HKASK_RERANK_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

#[cfg(test)]
mod qa_generation_tests {
    use super::*;

    #[test]
    fn explicit_model_wins_without_rewriting_the_operator_alias() {
        let model = "OpenRouter/~openai/gpt-sol-latest";
        assert_eq!(
            select_qa_generation_model(Some(model), Some("invalid")).expect("override"),
            model
        );
        assert_eq!(
            select_qa_generation_model(None, Some(model)).expect("setting"),
            model
        );
        let batch_model = format!("{model}:batch");
        assert_eq!(
            select_qa_generation_model(Some(&batch_model), None).expect("batch override"),
            batch_model
        );
    }

    #[test]
    fn unconfigured_and_invalid_are_errors_not_fallbacks() {
        assert!(matches!(
            select_qa_generation_model(None, None),
            Err(hkask_types::InferenceError::NotConfigured(_))
        ));
        for invalid in [
            "",
            " ",
            "bare-model",
            "/model",
            "OpenRouter/",
            "OpenRouter/ model",
            "~openai/gpt-sol-latest",
        ] {
            assert!(
                matches!(
                    select_qa_generation_model(
                        Some(invalid),
                        Some("OpenRouter/~openai/gpt-sol-latest")
                    ),
                    Err(hkask_types::InferenceError::Model(_))
                ),
                "{invalid:?}"
            );
            assert!(
                matches!(
                    select_qa_generation_model(None, Some(invalid)),
                    Err(hkask_types::InferenceError::Model(_))
                ),
                "{invalid:?}"
            );
        }
    }
}

#[cfg(test)]
mod decision_model_tests {
    /// The unconfigured path is the load-bearing contract: a typed
    /// refusal naming the setting — never a silent chat-model fallback
    /// (the test binary carries no `HKASK_DECISION_MODEL`; the settings
    /// chain injects it for production children).
    #[test]
    fn unconfigured_is_a_named_refusal_not_a_chat_fallback() {
        let error = super::resolve_decision_model().expect_err("unset env refuses");
        assert!(matches!(
            error,
            hkask_types::InferenceError::NotConfigured(_)
        ));
        let message = error.to_string();
        assert!(message.contains("kask.models.decision_model"));
        assert!(message.contains("HKASK_DECISION_MODEL"));
        assert!(message.contains("refuse"));
    }

    /// The pinned default names the provider prefix (decision-eligible
    /// detection) and the adopted Jev version — the dated-slug discipline
    /// (the alias `~typesafe/jev-latest` tracks newer versions; the
    /// constant pins one).
    #[test]
    fn default_decision_model_pins_the_openrouter_jev_version() {
        let model = super::DEFAULT_DECISION_MODEL;
        assert!(model.starts_with("OpenRouter/"));
        assert_eq!(model, "OpenRouter/typesafe/jev-1.14");
        assert_ne!(model, "OpenRouter/~typesafe/jev-latest");
    }
}

#[cfg(test)]
mod ocr_model_tests {
    /// The default is the dedicated RunPod OCR endpoint (D29), named with
    /// the provider prefix the registry key carries — never the pre-RunPod
    /// local `glm-ocr` literal the two settings layers each used to copy.
    #[test]
    fn default_ocr_model_is_the_runpod_endpoint() {
        let model = super::DEFAULT_OCR_MODEL;
        assert_eq!(model, "RunPod/kask-ocr");
        assert!(model.to_ascii_lowercase().contains("ocr"));
        assert!(!model.starts_with("ollama/"));
    }
}

// ── Media pipeline defaults ───────────────────────────────────────────────
//
// The media server has no in-server model fallback (its accessors return
// `None` when the env var is unset and callers fail visibly). These
// constants are the SETTINGS layer's defaults: `KaskMediaSettings::default()`
// carries them, settings.json overrides them, and `mcp_env()` injects the
// resolved value as `HKASK_MEDIA_*_MODEL` into the media server child
// (operator ruling 2026-09-04 — settings layers carry the code defaults).

/// Default STT model for the transcribe pipeline (env `HKASK_MEDIA_STT_MODEL`
/// via `KaskMediaSettings::stt_model`). Whisper-large-v3-turbo via OpenRouter —
/// word-level timestamps are reliable here (verified 2026-09-04: a
/// 62-minute talk transcribes in a single call, ~$0.04, 8.5K word
/// timings) and the educt layer system anchors to word indices, so a
/// segments-only STT response cannot back a reel. The reviewed
/// Nemotron-3.5-Transcribe flip (DeepInfra, $0.002/min, 40 languages,
/// one-hour context) is prepared and pinned — the DeepInfra arm serves
/// the documented OpenAI-compatible `/v1/openai/audio/transcriptions`
/// endpoint (verbose_json, word+segment granularities; the native
/// `/v1/inference` endpoint returned word timestamps once and then
/// segment-only across 7 same-day requests 2026-10-29, then a
/// monolithic-segment response 2026-11-02). Whisper stays the default
/// primary; Nemotron and Qwen serve as the cross-check fleet below.
pub const DEFAULT_MEDIA_STT_MODEL: &str = "OpenRouter/openai/whisper-large-v3-turbo";

/// Default cross-check STT fleet (env `HKASK_MEDIA_CROSS_CHECK_STT_MODELS`
/// via `KaskMediaSettings::cross_check_stt_models`) — the independent ASR
/// voices the bounded transcription cross-check addresses per call
/// (transcript-reel Phase 1b; operator direction 2026-11-02: three active
/// ASR models). Nemotron 3.5 Transcribe via DeepInfra — the documented
/// OpenAI-compatible endpoint (segments-level measured 2026-11-02: words
/// empty; $0.002/min). Qwen3-ASR via DeepInfra — Qwen3-ASR-1.7B, the
/// largest servable variant, word+segment timestamps at $0.0000075/s.
/// OpenRouter can no longer carry the Qwen ASR family (measured
/// 2026-11-02: the dedicated line is withdrawn from its catalog and its
/// transcriptions endpoint rejects chat-catalog omni models), and dated
/// snapshot slugs retire — resolve the current carrier from the
/// provider's live catalog before firing a leg.
pub const DEFAULT_MEDIA_CROSS_CHECK_STT_MODELS: &[&str] = &[
    "DeepInfra/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B",
    "DeepInfra/Qwen/Qwen3-ASR-1.7B",
];

/// Default vision model for gallery analysis and image description (env
/// `HKASK_MEDIA_VISION_MODEL` via `KaskMediaSettings::vision_model`). The
/// tagging pipelines send non-reasoning requests; reasoning-mandatory models
/// reject those ("Reasoning is mandatory for this endpoint and cannot be
/// disabled"). This model is non-reasoning, vision-capable, and cheap — the
/// tagging workload's shape.
pub const DEFAULT_MEDIA_VISION_MODEL: &str = "OpenRouter/openai/gpt-4o-mini";

/// Default sound-annotation model for the audio layer pass (env
/// `HKASK_MEDIA_SOUND_ANNOTATION_MODEL` via
/// `KaskMediaSettings::sound_annotation_model`). Omni Flash via OpenRouter —
/// the M2 input_audio probe (2026-10-29, goal a6c6fdf3 card M2) produced a
/// labeled audio_event layer in one call on a real recording. Audio passes
/// resolve the model per-call or through this setting only — audio-only
/// models never route via the shared pass-model env (the text/vision passes
/// share it), and the vision default is not audio-capable. Voxtral-small is
/// the recorded fallback (media server reference).
pub const DEFAULT_MEDIA_SOUND_ANNOTATION_MODEL: &str = "OpenRouter/qwen/qwen3.8-omni-flash";

/// Default grounded-tagging model for the vision pass's grounded mode (env
/// `HKASK_MEDIA_TAGGING_MODEL` via `KaskMediaSettings::tagging_model`).
/// Qwen3 VL 8B Instruct via OpenRouter — non-reasoning Instruct edition
/// (passes send non-reasoning requests). The S2 probe (2026-10-30, goal
/// a6c6fdf3, card 15ee05ae) swapped the operator-steered ollama default to
/// this OpenRouter-hosted same-family model on measured evidence: local
/// ollama/qwen3-vl:8b (8.8B, Q4_K_M) took >300s cold and 206s warm-best
/// per 4-frame chunk (two of three probe calls exceeded the 300s bridge
/// timeout; the one completed response held an out-of-frame box the
/// validation gate rejected), while this model completed the same range
/// in 12.0s with four validated, time-anchored detections — a ~17x
/// per-chunk difference that makes local untenable for per-recording
/// tagging. The ollama entry stays recorded as deliberate non-consumption
/// for this role (`kask/registry/providers/ollama.yaml`).
pub const DEFAULT_MEDIA_TAGGING_MODEL: &str = "openrouter/Qwen3 VL 8B Instruct";

/// Default rerank model for the research server's deep-search rerank stage
/// (env `HKASK_RERANK_MODEL` via `KaskModelsSettings::rerank_model`). The
/// rerank stage sends non-reasoning relevance-scoring requests. The
/// 2026-09-11 operator ruling named Qwen3-Reranker-8B (verified on
/// DeepInfra's catalog: $0.05/1M tokens, 32K context, instruction-aware);
/// the deepinfra-prefixed literal that ruling first shipped was UNROUTABLE
/// by construction — `detect_rerank_provider` serves only `OpenRouter/`-
/// and `ollama/`-prefixed models, so every default-configured deep search
/// silently degraded to heuristic rerank (the operator-reported bug,
/// repaired 2026-11-06). The default keeps the ruled model on the servable
/// cloud path; private-provenance rerank routes through the local
/// ollama reranker (`DEFAULT_PRIVATE_RERANK_MODEL`).
pub const DEFAULT_RERANK_MODEL: &str = "OpenRouter/qwen/qwen3-reranker-8b";

/// Default decision model (env `HKASK_DECISION_MODEL` via
/// `KaskModelsSettings::decision_model`). TypeSafe's Jev 1.14 — the first
/// "System One" decision model, operator adoption 2026-11-06: state plus
/// typed questions in, typed answers with calibrated per-level
/// probabilities out, never text. Served over OpenRouter's Decisions API
/// (`POST /api/alpha/decisions` — NOT the chat endpoint), so this model id
/// is decision-eligible only through `hkask-inference/src/decisions.rs`.
/// Verified from OpenRouter's model page and explainer 2026-11-06
/// (research run `b1f1663057722f5d`): $0.042/M input tokens, output free,
/// 262K context; the `~typesafe/jev-latest` alias tracks the newest
/// version — the default pins the version (dated-slug discipline). An
/// explicitly empty setting refuses the decision routes (fail-visible,
/// never a silent chat-model fallback).
pub const DEFAULT_DECISION_MODEL: &str = "OpenRouter/typesafe/jev-1.14";

/// Default embedding model (env `HKASK_EMBEDDING_MODEL` via
/// `KaskModelsSettings::embedding_model` / `KaskCorpusSettings::embedding_model`
/// — the two fields must agree; `effective_embedding_model` warns on
/// disagreement). Qwen3-Embedding-8B via OpenRouter (operator direction
/// 2026-09-28): an MRL model — native 4096, every embed request carries
/// `dimensions = embedding_dim` (default 1024) so width-bound vec0 stores
/// keep receiving fitting vectors. $0.01/M tokens (OpenRouter catalog,
/// 2026-09-28). Unlike the former ollama default this needs
/// OPENROUTER_API_KEY, surfaced visibly when missing. This is the single
/// literal both settings layers reference — a model update changes this
/// one constant (plus re-embedding the stores; the search-side model gate
/// filters and surfaces mismatched rows during the migration window).
pub const DEFAULT_EMBEDDING_MODEL: &str = "OpenRouter/qwen/qwen3-embedding-8b";

/// Default OCR model (env `HKASK_OCR_MODEL` via `KaskModelsSettings::ocr_model`
/// on the zed side and `HkaskSettings::ocr_model` in hkask-mcp-corpus — the
/// two settings layers reference this one literal). The RunPod serverless
/// endpoint named `kask-ocr` (D29): the endpoint name is the model id, so
/// `RunPod/kask-ocr` resolves through the registry's `{provider}/{name}`
/// key; `resolve_ocr_model` matches it case-insensitively. Before this
/// constant existed each layer carried its own copy of the pre-RunPod
/// `ollama/glm-ocr:latest` literal (operator correction 2026-11-13): a
/// local model the dedicated-OCR guardrail in `resolve_ocr_model` was
/// written to steer away from.
pub const DEFAULT_OCR_MODEL: &str = "RunPod/kask-ocr";

/// zed-kask: D100 item 3 (operator 2026-11-05) — the private leg's LOCAL
/// embedding model, served by the local ollama instance (the
/// `INFERENCE_PROVIDERS` "ollama" descriptor: local by construction, no
/// credentials, no cloud crossing). Private-store chunks and
/// private-context recall queries embed through the port bound to this
/// model ONLY; the configured cloud embedder above never sees private
/// content (the degrade-not-leak lift). The 0.6b emits the store's
/// default 1024 width natively (the cloud 8B is MRL-truncated to it), so
/// both stores share one vector width; the provider prefix must match
/// the descriptor's id (pinned in kask_bridge — the port's provider
/// check rejects a mismatched prefix per call).
pub const DEFAULT_PRIVATE_EMBEDDING_MODEL: &str = "ollama/qwen3-embedding:0.6b";

/// zed-kask: D100 classifier lift (operator ruling 2026-11-06) — the private
/// leg's LOCAL classifier model, served by the local ollama instance (the
/// same loopback descriptor as the private embedding model). Private-turn
/// chunk tagging runs through this model ONLY; the configured cloud
/// classifier (`kask.models.classifier_model`) never sees private content
/// (degrade-not-leak, lifted). The operator's named pick, deployed locally
/// (3.4 GB); a non-empty value must be provider-qualified for a LOCAL
/// provider — the wiring-time validation refuses a cloud-prefixed string
/// and keeps the structural-only degrade. An explicitly empty setting is
/// the opt-out (structural tags only, surfaced).
pub const DEFAULT_PRIVATE_CLASSIFIER_MODEL: &str = "ollama/qwen3.5:4b";

/// zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06) — the
/// private leg's LOCAL rerank model, served by the local ollama instance
/// through its chat API. Ollama has NO rerank endpoint (`/v1/rerank` and
/// `/api/rerank` both 404, probe-verified 2026-11-06), so the local arm
/// speaks the Qwen3-Reranker judge protocol (Zhang et al.,
/// arXiv:2506.05176): one deterministic chat call per document, the first
/// sampled token IS the verdict — `yes` → 1.0, `no` → 0.0 (probe-verified
/// on the operator's ollama 0.40.12). The 4b is the default pick — the
/// 0.6b's community GGUF template mis-serves the judge protocol
/// (probe-verified rambling output and a wrong verdict on an irrelevant
/// document, 2026-11-06). An explicitly empty setting is the operator's
/// opt-out (the surfaced heuristic skip).
pub const DEFAULT_PRIVATE_RERANK_MODEL: &str = "ollama/qwen3-reranker-4b:latest";
