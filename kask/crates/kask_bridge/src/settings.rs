//! Kask settings — the `"kask"` section in zed-kask's settings.json (D9a).
//!
//! This struct holds kask-unique, **non-secret** config. API keys and other
//! secrets live in the OS keychain via `CredentialsProvider` (D9b), not here.
//!
//! Registered with zed's settings system so it appears in the settings schema
//! and can be edited in the settings UI.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, Settings};
pub use settings_content::CuratorInteractionMode;
use settings_content::{
    KaskCondenserSettingsContent, KaskCorpusSettingsContent, KaskCuratorSettingsContent,
    KaskGeneralSettingsContent, KaskMcpSettingsContent, KaskMediaSettingsContent,
    KaskMemorySettingsContent, KaskModelsSettingsContent, KaskNostrRelaySettingsContent,
    KaskNostrSettingsContent, KaskPredictionMarketsSettingsContent,
    KaskPrivateInferenceSettingsContent, KaskResearchSettingsContent, KaskScenariosSettingsContent,
    KaskSettingsContent,
};

use collections::HashMap;

/// Kask-specific settings (the `"kask"` section in settings.json).
///
/// Non-secret configuration for hKask features: MCP server load set,
/// data-service toggles, curator/regulation/memory/condenser settings.
/// API keys are stored in the keychain via `CredentialsProvider` (D9b).
///
/// `Default` is the single source of truth for every subsection's defaults.
/// `From<KaskSettingsContent>` delegates to each subsection's `Default` via
/// `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]` attributes
/// here — `KaskSettings` is never deserialized directly (the settings system
/// deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, RegisterSetting)]
pub struct KaskSettings {
    /// Kask data directory — the root for all kask databases, agent state,
    /// and file-based stores (the hidden infrastructure tree). When empty,
    /// `mcp_env()` resolves a default via
    /// `hkask_types::agent_paths::resolve_data_dir()` (HKASK_DATA_DIR
    /// env var → XDG_DATA_HOME/zed-kask → ~/.local/share/zed-kask) and injects
    /// it as `HKASK_DATA_DIR` for every MCP server. This ensures servers
    /// always receive a consistent data directory without requiring the
    /// operator to set environment variables manually.
    pub data_dir: String,

    /// Kask artifacts directory — the visible root for ALL artifact files
    /// and outputs of the MCP servers at `{server}-mcp/{artifact-type}/`.
    /// When empty, `mcp_env()` resolves a default via
    /// `hkask_types::agent_paths::resolve_artifacts_dir()` (HKASK_ARTIFACTS_DIR
    /// env var → ~/Documents/zk-data) and injects it for every server that
    /// resolves artifact routes. Databases are infrastructure and stay in
    /// `data_dir`.
    pub artifacts_dir: String,

    /// Kask-wide general configuration: global inference concurrency + batching.
    pub general: KaskGeneralSettings,

    /// MCP server configuration — which of the 8 built-in servers to load.
    pub mcp: KaskMcpSettings,

    /// Curator configuration.
    pub curator: KaskCuratorSettings,

    /// Memory consolidation and recall configuration.
    pub memory: KaskMemorySettings,

    /// Condenser configuration for context management in inference threads.
    pub condenser: KaskCondenserSettings,

    /// Research MCP server configuration.
    pub research: KaskResearchSettings,

    /// Corpus MCP server configuration.
    pub corpus: KaskCorpusSettings,

    /// Scenarios MCP server configuration.
    pub scenarios: KaskScenariosSettings,
    /// Prediction-markets data-service configuration.
    pub prediction_markets: KaskPredictionMarketsSettings,

    /// Media MCP server configuration: research transcription and vision models.
    pub media: KaskMediaSettings,

    /// Kask-wide model configuration: default, embedding, and classifier models.
    pub models: KaskModelsSettings,

    /// Private-thread policy: the MCP servers disabled on privacy-marked
    /// threads and the local models that serve private memory.
    pub private_inference: KaskPrivateInferenceSettings,

    /// Nostr configuration. Public data only: no key is stored in settings.
    pub nostr: KaskNostrSettings,
}

/// Kask-wide general configuration: global inference concurrency + batching.
/// The limiter is process-global (one `Arc` shared across all consumers —
/// corpus OCR, MCP tool calls). Inference admission and resilience are
/// enforced by `LanguageModelInferencePort`.
///
/// `Default` is the single source of truth for defaults — `From<Content>`
/// reads from it via `unwrap_or(default.field)`. Do not add
/// `#[serde(default = ...)]` attributes here; `KaskSettings` is never
/// deserialized directly (the settings system deserializes `SettingsContent`
/// and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskGeneralSettings {
    /// Maximum concurrent cloud inference provider calls across the whole
    /// process. Default 96. Providers throttle at different levels;
    /// OpenRouter scales to this ceiling.
    pub max_concurrency: u32,

    /// Admission-to-completion timeout (queue wait, model resolution, stream
    /// establishment and event drain). A hung provider stalls the request indefinitely without
    /// this — the cybernetics variety check flagged this as a critical gap
    /// (disturbance class D2: provider timeout, no response). 0 disables the
    /// timeout (legacy behavior). Default 300 (5 minutes).
    pub inference_timeout_secs: u64,

    /// Consecutive transient failures required to open the inference circuit.
    pub inference_circuit_failure_threshold: u32,

    /// Seconds before an open circuit admits one half-open recovery probe.
    pub inference_circuit_open_secs: u64,
}

impl Default for KaskGeneralSettings {
    fn default() -> Self {
        Self {
            max_concurrency: 96,
            inference_timeout_secs: 300,
            inference_circuit_failure_threshold: 3,
            inference_circuit_open_secs: 30,
        }
    }
}

/// MCP server load configuration.
///
/// `Default` is the single source of truth for defaults — `From<Content>` reads
/// from it via `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]`
/// attributes here; `KaskSettings` is never deserialized directly (the settings
/// system deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskMcpSettings {
    /// Exact server/tool names each child server may invoke through IPC.
    /// `host/create_worktree_thread` separately grants worktree creation;
    /// the spawned child's MCP tools are narrowed against this same grant.
    pub delegated_tools: HashMap<String, Vec<String>>,
    /// Whether to load the default MCP server set (`BUILT_IN_MCP_SERVERS`).
    /// Set to `false` to disable all kask MCP servers.
    pub load_default: bool,

    /// Per-server overrides (e.g. `"curator": false` to unload the curator MCP).
    pub overrides: HashMap<String, bool>,
}

impl Default for KaskMcpSettings {
    fn default() -> Self {
        Self {
            load_default: true,
            overrides: HashMap::default(),
            delegated_tools: HashMap::default(),
        }
    }
}

/// Curator configuration.
///
/// `Default` is the single source of truth for defaults — `From<Content>` reads
/// from it via `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]`
/// attributes here; `KaskSettings` is never deserialized directly (the settings
/// system deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCuratorSettings {
    /// Whether the Curator agent is always-on (runs regulation loops in background).
    pub always_on: bool,

    /// Algedonic signal threshold (0.0–1.0).
    pub algedonic_threshold: f64,

    /// How the user works with the Curator: Level 1 control, Level 2
    /// collaboration, Level 3 learning collaboration. Set only by the user;
    /// read by `NativeAgent::new_session` for each new Curator thread.
    pub interaction_mode: CuratorInteractionMode,
}

impl Default for KaskCuratorSettings {
    fn default() -> Self {
        Self {
            always_on: true,
            algedonic_threshold: 0.8,
            interaction_mode: CuratorInteractionMode::default(),
        }
    }
}

/// Memory consolidation and recall configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskMemorySettings {
    /// Consolidation cadence in seconds (0 = disabled).
    pub consolidation_cadence_secs: u64,

    /// Confidence floor for memory retention (0.0–1.0).
    pub confidence_floor: f64,

    /// Maximum number of memory snippets to retrieve for context injection.
    pub recall_limit: u32,

    /// Minimum confidence for a memory to be injected into context (0.0–1.0).
    pub recall_min_confidence: f64,

    /// Whether to automatically inject recalled memories into prompts.
    pub auto_inject: bool,

    /// Opt in to injecting selected federated sources into curator chat.
    pub federated_auto_inject: bool,

    /// Source IDs selected from the curator federated sources manifest.
    pub federated_source_ids: Vec<String>,

    /// Memory life S in days (Wozniak-Gorzelanczyk 1995 forgetting curve:
    /// R(t) = exp(-t/S)). After S days without recall, confidence decays to
    /// exp(-1) ≈ 36.8%; the half-life is S·ln(2). Recalling a memory resets
    /// its decay clock. Default: 180 (≈6 months). Overridden by the
    /// `HKASK_MEMORY_LIFE_DAYS` env var.
    pub memory_life_days: f64,

    /// ALWAYS-mode distillation cadence in seconds (0 = disabled). The
    /// curator server's background pass distills finished threads into
    /// additive candidate lesson h_mems at the 0.5 confidence floor. Its
    /// per-thread watermark is bounded control state: each successful batch
    /// atomically replaces the prior marker.
    pub distillation_cadence_secs: u64,

    /// A thread counts as finished when its newest turn is at least this
    /// many seconds old — the distillation pass skips younger threads.
    pub distillation_idle_secs: u64,

    /// Distillation-gated forgetting age in days (0 = disabled). The
    /// curator server's background pass forgets (deletes) the
    /// shared-copy turns of threads whose distillation watermark is
    /// older than this — the goldfish principle's automatic leg.
    /// Time-based and distillation-gated, never count-based: budgets
    /// are deprecated (operator ruling 2026-09-04). Default 7, matching
    /// the curator server's `DEFAULT_FORGETTING_DAYS`.
    pub forgetting_days: u64,
}

impl Default for KaskMemorySettings {
    fn default() -> Self {
        Self {
            consolidation_cadence_secs: 300,
            confidence_floor: 0.3,
            recall_limit: 5,
            recall_min_confidence: 0.3,
            auto_inject: true,
            federated_auto_inject: false,
            federated_source_ids: Vec::new(),
            memory_life_days: hkask_memory::MemoryStore::default_memory_life_days(),
            distillation_cadence_secs: 600,
            distillation_idle_secs: 300,
            forgetting_days: 7,
        }
    }
}

/// Condenser configuration for context management in inference threads.
///
/// Controls how tool results are compressed before entering the message
/// history, and what compression profile to use.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCondenserSettings {
    /// Compression profile: "heavy", "normal", "soft", or "light".
    /// - Heavy: 10% retention, 30 max lines — aggressive compression
    /// - Normal: 20% retention, 80 max lines — balanced
    /// - Soft: 60% retention, 200 max lines — light touch
    /// - Light: 95% retention, no max — near-passthrough
    pub profile: String,

    /// Whether to automatically compress tool results before they enter
    /// the message history. When false, tool results are stored verbatim.
    pub auto_compress_tool_results: bool,
}

impl Default for KaskCondenserSettings {
    fn default() -> Self {
        Self {
            profile: "normal".to_string(),
            auto_compress_tool_results: false,
        }
    }
}

/// Research MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskResearchSettings {
    /// Research database path (feed substrate + research-run ledger). When
    /// empty, the server resolves a default path under the hKask data
    /// directory.
    pub research_db: String,
}

/// Corpus MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCorpusSettings {
    /// Embedding dimensionality (must match the embedding model's output).
    pub embedding_dim: u32,

    /// Embedding model. Empty = not configured — embedding-dependent
    /// calls fail visibly naming the setting (the operator's
    /// no-hidden-models spec: no constant fallback).
    pub embedding_model: String,

    /// Template root directory for Jinja2 templates.
    pub template_root: String,
}

impl Default for KaskCorpusSettings {
    fn default() -> Self {
        Self {
            embedding_dim: 1024,
            embedding_model: default_embedding_model(),
            template_root: "kask/registry".to_string(),
        }
    }
}

fn default_embedding_model() -> String {
    // The single literal lives in hkask_inference::model_constants
    // (DEFAULT_EMBEDDING_MODEL) — both settings layers reference it, so a
    // model update changes one constant. MRL contract: every embed request
    // carries `dimensions = embedding_dim` (default 1024); the 8B default
    // needs OPENROUTER_API_KEY, surfaced visibly when missing.
    // `kask.corpus.embedding_model` / `kask.models.embedding_model` override it.
    hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL.to_string()
}

/// Prediction-markets data-service configuration.
///
/// No path fields — the prediction-markets data dir is derived from the
/// global `KaskSettings::data_dir` as `mcp/prediction-markets/` by
/// `mcp_env()`. The server reads it via `HKASK_PREDICTION_MARKETS_DATA`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskPredictionMarketsSettings {
    /// Cache TTL in seconds for market-data responses (0 = server default).
    pub cache_ttl_secs: u64,
}

/// Nostr configuration (`kask.nostr`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
pub struct KaskNostrSettings {
    /// The local relay server's policy.
    pub relay: KaskNostrRelaySettings,
}

/// Policy for the local Nostr relay MCP server (`kask.nostr.relay`).
///
/// Delivered to the server as `HKASK_NOSTR_RELAY_PORT`,
/// `HKASK_NOSTR_RELAY_AUTHORS` and `HKASK_NOSTR_RELAY_SYNC_RELAYS` by
/// `mcp_env()`. The values are passed as the operator wrote them, trimmed
/// and de-duplicated: the relay server is the one place that decides
/// whether an author or a relay address is usable, and it reports what it
/// refused in `nostr_relay_status`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
pub struct KaskNostrRelaySettings {
    /// Loopback port (0 = a free port picked at each start).
    pub port: u16,
    /// Authors whose events the relay archives. Empty admits nothing.
    pub allowed_authors: Vec<String>,
    /// Public relays the relay's sync reads from.
    pub sync_relays: Vec<String>,
}

/// Trim each entry and drop empties and repeats, keeping first-seen order.
fn clean_list(entries: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in entries {
        let entry = raw.trim();
        if !entry.is_empty() && !out.iter().any(|seen| seen == entry) {
            out.push(entry.to_string());
        }
    }
    out
}

/// Scenarios MCP server configuration.
///
/// No path fields — the scenarios data dir is derived from the global
/// `KaskSettings::data_dir` as `mcp/scenarios/` by `mcp_env()`. The server
/// reads it via `HKASK_SCENARIOS_DATA`.
///
/// Reserved: the struct is intentionally empty today (the scenarios server
/// has no non-secret operator-tunable knobs beyond `HKASK_DATA_DIR`). It
/// exists in the settings schema so the settings UI page and the
/// `KaskSettings` struct stay symmetric with the other per-server sections;
/// adding a field here when the server gains a knob requires no schema
/// migration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskScenariosSettings {}

/// Media MCP server configuration.
///
/// Research transcription, vision, and media role-model overrides. All
/// four carry settings-layer defaults (`hkask_inference::model_constants::
/// DEFAULT_MEDIA_*`) — the media server has no in-server fallback, so
/// without an injected value those pipelines fail visibly (NotConfigured).
/// An explicitly empty string in settings.json clears a default: the env
/// var is not injected and the pipeline fails closed. All values are
/// provider-prefixed strings.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskMediaSettings {
    /// STT model override (env `HKASK_MEDIA_STT_MODEL`).
    pub stt_model: String,
    /// Additional STT models for the bounded transcription cross-check
    /// (transcript-reel Phase 1b) — addressed per call via the transcribe
    /// tools' `model` override and listed by `model_list` (env
    /// `HKASK_MEDIA_CROSS_CHECK_STT_MODELS`, comma-joined).
    pub cross_check_stt_models: Vec<String>,
    /// Vision model override (env `HKASK_MEDIA_VISION_MODEL`).
    pub vision_model: String,
    /// Sound-annotation model for the audio passes (env
    /// `HKASK_MEDIA_SOUND_ANNOTATION_MODEL`) — audio-only; resolved per-call
    /// or from this setting, never via the shared pass-model env.
    pub sound_annotation_model: String,
    /// Grounded-tagging model for the vision pass's grounded mode (env
    /// `HKASK_MEDIA_TAGGING_MODEL`).
    pub tagging_model: String,
}

impl Default for KaskMediaSettings {
    fn default() -> Self {
        Self {
            stt_model: hkask_inference::model_constants::DEFAULT_MEDIA_STT_MODEL.to_string(),
            cross_check_stt_models:
                hkask_inference::model_constants::DEFAULT_MEDIA_CROSS_CHECK_STT_MODELS
                    .iter()
                    .map(|model| model.to_string())
                    .collect(),
            vision_model: hkask_inference::model_constants::DEFAULT_MEDIA_VISION_MODEL.to_string(),
            sound_annotation_model:
                hkask_inference::model_constants::DEFAULT_MEDIA_SOUND_ANNOTATION_MODEL.to_string(),
            tagging_model: hkask_inference::model_constants::DEFAULT_MEDIA_TAGGING_MODEL
                .to_string(),
        }
    }
}

/// Kask-wide model configuration.
///
/// Provider-prefixed model names that override the kask built-in defaults.
/// When a field is empty, kask falls back to its default model selection
/// (typically the zed `agent.default_model`).
///
// **Default-model policy (operator spec, 2026-09-04):** `default_model` has
// NO code-constant fallback — empty means "the zed default" (the user's
// active default, visible in Settings → AI) on the bridge path, or a typed
// `NotConfigured` error on the direct path. Never a hidden model. The
// per-subsystem fields (`embedding_model`, `classifier_model`, ...) still
// fall back to their `DEFAULT_*_MODEL` constants via their `effective_*`
// methods — pending the no-hidden-models audit; changing e.g. the embedding
// default invalidates existing vector indexes, so each subsystem needs a
// ratified decision rather than a blanket swap.
//
// **Adding a field here touches six files — the full chain, in order:**
// 1. `crates/settings_content/src/settings_content.rs` — the `*Content` twin
//    (zed's settings.json schema; the `From<Content>` impl below reads it)
// 2. This file — the field + the `From<Content>` arm
// 3. `kask/crates/kask_bridge/src/mcp_env.rs` — `emit_models_env` (or the
//    per-server `emit_*_env`) writes the `HKASK_*` env var
// 4. `kask/crates/kask_bridge/src/mcp_servers.rs` — the target server's
//    `config_env` allowlist (pinned by `*_allowlist_matches_actual_reads`)
// 5. The consuming MCP server — reads the env var
// 6. `crates/settings_ui/src/pages/kask_page/models.rs` — the settings UI page
// The corpus process reads model settings through
// `kask/mcp-servers/hkask-mcp-corpus/src/settings.rs`,
// using host-injected environment variables before its JSONC file fallback.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskModelsSettings {
    /// Default inference model (provider-prefixed, e.g. `"OpenRouter/z-ai/glm-5.3"`).
    /// When set, overrides the kask default for Curator, skill execution, and
    /// kask panel inference.
    pub default_model: String,

    /// Embedding model for corpus indexing and memory semantic recall
    /// (provider-prefixed). When empty, falls back to the corpus MCP server's
    /// `embedding_model` setting, then to the kask default.
    pub embedding_model: String,

    /// Classifier model for guard/regulation classification tasks and
    /// the curator's memory-distillation lesson extraction
    /// (provider-prefixed). Both are non-thinking workloads — output
    /// tokens, not reasoning tokens — so the model must accept
    /// `reasoning_effort: "none"`. An absent settings field is filled from
    /// `Default`; an explicitly empty value leaves classifier routes
    /// unconfigured so they degrade or refuse visibly rather than selecting a
    /// hidden model.
    pub classifier_model: String,

    /// Dedicated non-thinking QA generator (provider-prefixed). Empty is
    /// unconfigured; explicit tool models override this, never the chat model.
    pub qa_generation_model: String,

    /// OCR vision model for scanned document OCR (provider-prefixed).
    /// When empty, the corpus server falls back to the kask default
    /// (env `HKASK_OCR_MODEL` → `HkaskSettings::ocr_model` →
    /// resolved in hkask-mcp-corpus, not here).
    pub ocr_model: String,

    /// Rerank model for the research server's deep-search rerank stage
    /// (provider-prefixed). When empty, the research server falls back
    /// to the kask default (env `HKASK_RERANK_MODEL` — resolved per call
    /// in `hkask_inference::model_constants::rerank_model`).
    pub rerank_model: String,

    /// Decision model for bounded structured judgments (provider-prefixed,
    /// served over the provider's Decisions API — never a chat endpoint).
    /// Empty is unconfigured: decision routes refuse (env
    /// `HKASK_DECISION_MODEL` — resolved in
    /// `hkask_inference::model_constants::resolve_decision_model`), never
    /// the chat model.
    pub decision_model: String,
}

// Code defaults (operator ruling 2026-09-04, superseding the
// no-hidden-models spec): configured models have code defaults; the settings
// UI / settings.json overrides them. QA generation stays unconfigured
// (no verified dedicated default). The decision model's code default is the
// operator's 2026-11-06 adoption: TypeSafe's Jev 1.14 over OpenRouter's
// Decisions API (a System One decision model — typed questions in, typed
// answers with calibrated probabilities out; never a chat endpoint).
// The values are the operator's configured models, verbatim.
//
// `embedding_model` also stays empty HERE because its
// precedence chain is two-layer (`models.embedding_model` →
// `corpus.embedding_model` → default). A non-empty default at the models
// layer would shadow every `corpus.embedding_model` override — the
// From<Content> impl fills absent fields with these defaults, so
// "absent" and "explicitly set" are indistinguishable one layer down.
// The embedding default lives in `KaskCorpusSettings::default()`
// (`default_embedding_model`), the chain's final hop.
impl Default for KaskModelsSettings {
    fn default() -> Self {
        Self {
            default_model: "OpenRouter/z-ai/glm-5.3".to_string(),
            embedding_model: String::new(),
            // glm-5.2, not the operator's glm-5.3-flash: the classifier must
            // be a non-thinking model (or one where thinking is disable-able
            // via `reasoning_effort: "none"`) — classification and tagging
            // need output tokens, not reasoning tokens. glm-5.3-flash is a
            // thinking model that cannot disable it (operator ruling
            // 2026-09-04).
            classifier_model: "OpenRouter/z-ai/glm-5.2".to_string(),
            // No verified generator default (operator ruling 2026-09-11).
            qa_generation_model: String::new(),
            ocr_model: "ollama/glm-ocr:latest".to_string(),
            // DeepInfra Qwen3 reranker (operator ruling 2026-09-11): the
            // research server's rerank stage defaults to the verified
            // catalog model instead of failing unconfigured.
            rerank_model: hkask_inference::model_constants::DEFAULT_RERANK_MODEL.to_string(),
            decision_model: hkask_inference::model_constants::DEFAULT_DECISION_MODEL.to_string(),
        }
    }
}

impl KaskModelsSettings {
    // NOTE (superseded 2026-09-04): the former no-hidden-models spec —
    // empty defaults, callers fail visibly — is replaced by code defaults
    // in the `Default` impl above (operator ruling: defaults in code so the
    // code works; the settings UI overrides). The per-subsystem
    // `effective_*` methods below resolve settings → default; an empty
    // effective value remains possible for QA generation and verification.
    // `rerank_model` now has a configured default.

    /// Resolve the classifier model for inference that runs inside the host
    /// process. The saved Kask setting is authoritative: an inherited
    /// `HKASK_CLASSIFIER_MODEL` belongs to child-process configuration and
    /// must never override a later Settings → Kask → Models choice.
    ///
    /// MCP children receive this same setting through `emit_models_env`;
    /// direct CLI processes may still intentionally set the env variable and
    /// resolve it through `hkask_inference::model_constants`.
    #[must_use]
    pub fn classifier_model_for_host(&self) -> Option<String> {
        let model = self.classifier_model.trim();
        (!model.is_empty()).then(|| model.to_string())
    }
}

/// Private-thread policy (the `kask.private_inference` section).
///
/// Private inference is served by the local Ollama or llama.cpp install
/// through Zed's built-in providers. This section carries what a
/// privacy-marked thread still needs from kask: the tool perimeter and
/// the local models that embed, tag and rerank private memory. Which
/// models a private thread may run is fixed
/// (`settings_content::PRIVATE_THREAD_LOCAL_PROVIDERS`) and is not a
/// setting.
///
/// `Default` is the single source of truth. Do not add
/// `#[serde(default = ...)]` attributes here (see `KaskSettings`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskPrivateInferenceSettings {
    /// zed-kask: D100 — MCP servers whose tool surface is disabled on
    /// privacy-marked threads (cloud-inference-dependent tools). Defaults
    /// to the media server.
    pub private_thread_disabled_servers: Vec<String>,
    /// zed-kask: D100 item 3 (operator 2026-11-05) — the private leg's
    /// LOCAL embedding model (provider-qualified for the local ollama
    /// provider). Private-store chunks and private-context recall queries
    /// embed through the local port bound to this model; the configured
    /// cloud embedder never sees private content. Empty = the explicit
    /// opt-out (keyword-only degrade persists, surfaced).
    pub embedding_model: String,
    /// zed-kask: D100 classifier lift (operator ruling 2026-11-06) — the
    /// private leg's LOCAL classifier model: private-turn chunk tagging
    /// runs through this model only; the configured cloud classifier never
    /// sees private content. Empty = the explicit opt-out (structural tags
    /// only, surfaced). The default is the operator's named pick
    /// (`ollama/qwen3.5:4b`, deployed locally).
    pub classifier_model: String,
    /// zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06) — the
    /// private leg's LOCAL rerank model: private-provenance deep-search
    /// rerank runs through this model only (the local ollama judge
    /// protocol); the configured cloud reranker never sees private
    /// content. Empty = the explicit opt-out (the surfaced heuristic skip).
    pub rerank_model: String,
}

impl Default for KaskPrivateInferenceSettings {
    fn default() -> Self {
        Self {
            // D100: the single-home default (settings_content's const —
            // never a second literal here).
            private_thread_disabled_servers:
                settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS
                    .iter()
                    .map(|id| id.to_string())
                    .collect(),
            // D100 item 3: the single literal lives in
            // `hkask_inference::model_constants` (DEFAULT_PRIVATE_EMBEDDING_MODEL)
            // — the same single-home rule as the cloud embedding default.
            embedding_model: hkask_inference::model_constants::DEFAULT_PRIVATE_EMBEDDING_MODEL
                .to_string(),
            // D100 classifier lift: the same single-home rule — the
            // operator's named local pick.
            classifier_model: hkask_inference::model_constants::DEFAULT_PRIVATE_CLASSIFIER_MODEL
                .to_string(),
            // D100 §8.1 routed shape: the same single-home rule — the
            // local ollama reranker (the 4b judge-protocol pick).
            rerank_model: hkask_inference::model_constants::DEFAULT_PRIVATE_RERANK_MODEL
                .to_string(),
        }
    }
}

impl From<KaskPrivateInferenceSettingsContent> for KaskPrivateInferenceSettings {
    fn from(c: KaskPrivateInferenceSettingsContent) -> Self {
        let default = Self::default();
        Self {
            private_thread_disabled_servers: c
                .private_thread_disabled_servers
                .unwrap_or(default.private_thread_disabled_servers),
            // `Some("")` is preserved — the explicit opt-out is distinct from
            // absent (the default local model).
            embedding_model: c.embedding_model.unwrap_or(default.embedding_model),
            // `Some("")` preserved here too — the structural-only opt-out.
            classifier_model: c.classifier_model.unwrap_or(default.classifier_model),
            // `Some("")` preserved — the heuristic-skip opt-out.
            rerank_model: c.rerank_model.unwrap_or(default.rerank_model),
        }
    }
}

/// Resolve a storage-root setting with the single priority chain shared by
/// both roots (hidden data dir, visible artifacts dir): settings field →
/// env var → resolved platform default. One chain, not two — the duplicated
/// inline versions drifted apart historically.
fn resolve_root_dir(
    setting: &str,
    env_var: &str,
    platform_default: fn() -> std::path::PathBuf,
) -> String {
    if !setting.is_empty() {
        return setting.to_string();
    }
    std::env::var(env_var).unwrap_or_else(|_| platform_default().to_string_lossy().to_string())
}

impl Settings for KaskSettings {
    fn from_settings(s: &settings_content::SettingsContent) -> Self {
        s.kask.clone().map(|c| c.into()).unwrap_or_default()
    }
}

impl KaskSettings {
    /// Effective embedding model, resolving the documented precedence:
    /// `models.embedding_model` (if non-empty) → `corpus.embedding_model`
    /// (if non-empty — its `Default` carries the code default, operator
    /// ruling 2026-09-04) → empty. Empty is reachable only when the user
    /// explicitly empties both fields — embedding calls then fail visibly
    /// naming the setting. When BOTH fields are set and disagree, the
    /// resolution warns before applying the precedence — a model update
    /// that sets only one field silently shadows the other, which is the
    /// classic way an embedding-model change breaks every consumer while
    /// each surface still looks configured.
    ///
    /// This is the single source of truth for the `HKASK_EMBEDDING_MODEL`
    /// env emission. Previously two separate `env.insert` blocks in
    /// `mcp_env()` targeted the same env var, with precedence enforced only
    /// by statement order — a silent reorder would flip which setting won.
    /// The precedence is now explicit and pinned by
    /// `mcp_env_models_embedding_model_overrides_corpus`.
    #[must_use]
    pub fn effective_embedding_model(&self) -> String {
        if !self.models.embedding_model.is_empty()
            && !self.corpus.embedding_model.is_empty()
            && self.models.embedding_model != self.corpus.embedding_model
        {
            tracing::warn!(
                target: "kask.settings",
                models_embedding_model = %self.models.embedding_model,
                corpus_embedding_model = %self.corpus.embedding_model,
                "kask.models.embedding_model and kask.corpus.embedding_model disagree — \
                 models wins; set both to the same value"
            );
        }
        if !self.models.embedding_model.is_empty() {
            self.models.embedding_model.clone()
        } else if !self.corpus.embedding_model.is_empty() {
            self.corpus.embedding_model.clone()
        } else {
            String::new()
        }
    }

    /// Build the environment variable map for MCP server child processes.
    ///
    /// Translates all kask settings into the env vars that MCP servers read
    /// at startup. Only non-empty/non-default values are included — MCP
    /// servers have their own fallback defaults for unset env vars.
    ///
    /// This is the **config** half of the env map. The full env for a server
    /// child process — config + keychain credentials + the inference socket —
    /// is assembled by [`build_mcp_server_env`](crate::build_mcp_server_env)
    /// in `mcp_servers`, the single canonical path. It composes this crate's
    /// `mcp_env()` with the per-server credential/config allowlists. There
    /// is no other env-construction path; do not re-introduce one.
    pub fn mcp_env(&self) -> std::collections::HashMap<String, String> {
        let mut env = std::collections::HashMap::new();

        // Always inject both storage roots so every MCP server resolves
        // paths consistently, via the single priority chain in
        // `resolve_root_dir`: settings field → env var → resolved platform
        // default. Without this, servers fall back to their own resolution
        // and an operator override is silently dropped.
        let data_dir = resolve_root_dir(
            &self.data_dir,
            "HKASK_DATA_DIR",
            hkask_types::agent_paths::resolve_data_dir,
        );
        env.insert("HKASK_DATA_DIR".to_string(), data_dir.clone());
        let artifacts_dir = resolve_root_dir(
            &self.artifacts_dir,
            "HKASK_ARTIFACTS_DIR",
            hkask_types::agent_paths::resolve_artifacts_dir,
        );
        env.insert("HKASK_ARTIFACTS_DIR".to_string(), artifacts_dir);
        crate::mcp_env::emit_general_env(&self.general, &mut env);
        crate::mcp_env::emit_curator_webid_env(&mut env);
        crate::mcp_env::emit_research_env(&self.research, &mut env);
        let effective_embedding = self.effective_embedding_model();
        crate::mcp_env::emit_corpus_embedding_env(&self.corpus, &effective_embedding, &mut env);
        crate::mcp_env::emit_corpus_template_root_env(&self.corpus, &data_dir, &mut env);
        // zed-kask: D100's named follow-up (operator-approved 2026-11-06) —
        // the private leg's LOCAL embedding model reaches the kask MCP
        // children (the curator server's private-call query embeds route
        // through it). Emitted unconditionally: an empty value carries the
        // operator's explicit opt-out, and the child's reader maps
        // empty/unset to the surfaced degrade — never a cloud fallback.
        env.insert(
            hkask_inference::model_constants::PRIVATE_EMBEDDING_MODEL_ENV.to_string(),
            self.private_inference.embedding_model.clone(),
        );
        // zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06) —
        // the private leg's LOCAL rerank model reaches the research child
        // the same way: emitted unconditionally (an empty value carries the
        // explicit opt-out), admitted by the research server's allowlist.
        env.insert(
            hkask_inference::model_constants::PRIVATE_RERANK_MODEL_ENV.to_string(),
            self.private_inference.rerank_model.clone(),
        );
        crate::mcp_env::emit_scenarios_env(&data_dir, &mut env);
        crate::mcp_env::emit_prediction_markets_env(&data_dir, &self.prediction_markets, &mut env);
        crate::mcp_env::emit_media_env(&self.media, &mut env);
        crate::mcp_env::emit_nostr_relay_env(&self.nostr.relay, &mut env);
        crate::mcp_env::emit_models_env(&self.models, &mut env);
        crate::mcp_env::emit_curator_distillation_env(&self.memory, &mut env);
        crate::mcp_env::emit_operator_override_env(&mut env);
        env
    }

    /// The single curator manifest used by both the settings picker and the
    /// in-process injector, including the configured data directory override.
    pub fn federated_sources_manifest_path(&self) -> std::path::PathBuf {
        let data_dir = resolve_root_dir(
            &self.data_dir,
            "HKASK_DATA_DIR",
            hkask_types::agent_paths::resolve_data_dir,
        );
        std::path::Path::new(&data_dir)
            .join(hkask_types::agent_paths::agent_dir("curator"))
            .join("federated-sources.json")
    }

    /// Read only the registered source IDs and names from the curator's manifest.
    /// An unavailable or invalid manifest is an error, not an empty registry.
    pub fn federated_sources(
        &self,
    ) -> Result<Vec<(String, String)>, hkask_memory::FederatedRecallError> {
        let manifest =
            hkask_memory::FederatedSourcesManifest::load(self.federated_sources_manifest_path())?;
        Ok(manifest
            .sources
            .into_iter()
            .map(|source| (source.id, source.display_name))
            .collect())
    }

    /// The host-deployed template registry root for in-process consumers
    /// (curator memory chunk tagging renders `tag-passages-batch` from it).
    ///
    /// Same rule as the `HKASK_TEMPLATE_ROOT` env injection for MCP server
    /// children ([`Self::mcp_env`]) — one resolution, two delivery seams.
    /// The env var is only set in child processes, never in the editor
    /// process itself, so in-process consumers must read the root here
    /// instead of from env.
    pub fn resolved_template_root(&self) -> std::path::PathBuf {
        let data_dir = resolve_root_dir(
            &self.data_dir,
            "HKASK_DATA_DIR",
            hkask_types::agent_paths::resolve_data_dir,
        );
        std::path::PathBuf::from(crate::mcp_env::resolve_template_root(
            &self.corpus,
            &data_dir,
        ))
    }
}

// ── Content → Settings conversions ─────────────────────────────────────────
//
// Each subsection has a `From<Content>` impl that reads defaults from `Default`.
// The top-level `From<KaskSettingsContent>` is then a one-liner per field.
// This makes `Default` the single source of truth — no inlined literals in the
// `From` impl, no dead `#[serde(default)]` attributes on `KaskSettings`.

impl From<KaskMcpSettingsContent> for KaskMcpSettings {
    fn from(c: KaskMcpSettingsContent) -> Self {
        let default = Self::default();
        let mut overrides = c.overrides;
        overrides.remove("training");
        let mut delegated_tools = c.delegated_tools.unwrap_or(default.delegated_tools);
        delegated_tools.remove("training");
        Self {
            load_default: c.load_default.unwrap_or(default.load_default),
            overrides,
            delegated_tools,
        }
    }
}

impl From<KaskGeneralSettingsContent> for KaskGeneralSettings {
    fn from(c: KaskGeneralSettingsContent) -> Self {
        let default = Self::default();
        // Treat 0 as "use default" — a user setting `max_concurrency: 0` would
        // construct a limiter that admits no permits, deadlocking every
        // inference call. `inference_timeout_secs: 0` is the legitimate
        // "disable timeout" value (legacy behavior), so it passes through
        // without the filter.
        Self {
            max_concurrency: c
                .max_concurrency
                .filter(|&v| v > 0)
                .unwrap_or(default.max_concurrency),
            inference_timeout_secs: c
                .inference_timeout_secs
                .unwrap_or(default.inference_timeout_secs),
            inference_circuit_failure_threshold: c
                .inference_circuit_failure_threshold
                .filter(|&value| value > 0)
                .unwrap_or(default.inference_circuit_failure_threshold),
            inference_circuit_open_secs: c
                .inference_circuit_open_secs
                .filter(|&value| value > 0)
                .unwrap_or(default.inference_circuit_open_secs),
        }
    }
}

impl From<KaskCuratorSettingsContent> for KaskCuratorSettings {
    fn from(c: KaskCuratorSettingsContent) -> Self {
        let default = Self::default();
        Self {
            always_on: c.always_on.unwrap_or(default.always_on),
            algedonic_threshold: c.algedonic_threshold.unwrap_or(default.algedonic_threshold),
            interaction_mode: c.interaction_mode.unwrap_or(default.interaction_mode),
        }
    }
}

impl From<KaskMemorySettingsContent> for KaskMemorySettings {
    fn from(c: KaskMemorySettingsContent) -> Self {
        let default = Self::default();
        Self {
            consolidation_cadence_secs: c
                .consolidation_cadence_secs
                .unwrap_or(default.consolidation_cadence_secs),
            confidence_floor: c.confidence_floor.unwrap_or(default.confidence_floor),
            recall_limit: c.recall_limit.unwrap_or(default.recall_limit),
            recall_min_confidence: c
                .recall_min_confidence
                .unwrap_or(default.recall_min_confidence),
            auto_inject: c.auto_inject.unwrap_or(default.auto_inject),
            federated_auto_inject: c
                .federated_auto_inject
                .unwrap_or(default.federated_auto_inject),
            federated_source_ids: c
                .federated_source_ids
                .unwrap_or(default.federated_source_ids),
            memory_life_days: c.memory_life_days.unwrap_or(default.memory_life_days),
            distillation_cadence_secs: c
                .distillation_cadence_secs
                .unwrap_or(default.distillation_cadence_secs),
            distillation_idle_secs: c
                .distillation_idle_secs
                .unwrap_or(default.distillation_idle_secs),
            forgetting_days: c.forgetting_days.unwrap_or(default.forgetting_days),
        }
    }
}

impl From<KaskCondenserSettingsContent> for KaskCondenserSettings {
    fn from(c: KaskCondenserSettingsContent) -> Self {
        let default = Self::default();
        Self {
            profile: c.profile.unwrap_or(default.profile),
            auto_compress_tool_results: c
                .auto_compress_tool_results
                .unwrap_or(default.auto_compress_tool_results),
        }
    }
}

impl From<KaskResearchSettingsContent> for KaskResearchSettings {
    fn from(c: KaskResearchSettingsContent) -> Self {
        let default = Self::default();
        Self {
            research_db: c.research_db.unwrap_or(default.research_db),
        }
    }
}

impl From<KaskCorpusSettingsContent> for KaskCorpusSettings {
    fn from(c: KaskCorpusSettingsContent) -> Self {
        let default = Self::default();
        // Treat 0 as "use default" — a user setting `embedding_dim: 0` would
        // otherwise construct a zero-dimensional EmbeddingStore that silently
        // rejects every vector (DimensionMismatch { expected: 0, ... }),
        // disabling embedding-based recall with no startup signal.
        Self {
            embedding_dim: c
                .embedding_dim
                .filter(|&d| d > 0)
                .unwrap_or(default.embedding_dim),
            embedding_model: c.embedding_model.unwrap_or(default.embedding_model),
            template_root: c.template_root.unwrap_or(default.template_root),
        }
    }
}

impl From<KaskPredictionMarketsSettingsContent> for KaskPredictionMarketsSettings {
    fn from(c: KaskPredictionMarketsSettingsContent) -> Self {
        let default = Self::default();
        Self {
            cache_ttl_secs: c.cache_ttl_secs.unwrap_or(default.cache_ttl_secs),
        }
    }
}

impl From<KaskNostrRelaySettingsContent> for KaskNostrRelaySettings {
    fn from(c: KaskNostrRelaySettingsContent) -> Self {
        Self {
            port: c.port.unwrap_or_default(),
            allowed_authors: clean_list(c.allowed_authors.unwrap_or_default()),
            sync_relays: clean_list(c.sync_relays.unwrap_or_default()),
        }
    }
}

impl From<KaskNostrSettingsContent> for KaskNostrSettings {
    fn from(c: KaskNostrSettingsContent) -> Self {
        Self {
            relay: c.relay.map(Into::into).unwrap_or_default(),
        }
    }
}

impl From<KaskScenariosSettingsContent> for KaskScenariosSettings {
    fn from(_c: KaskScenariosSettingsContent) -> Self {
        Self::default()
    }
}

impl From<KaskMediaSettingsContent> for KaskMediaSettings {
    fn from(c: KaskMediaSettingsContent) -> Self {
        let default = Self::default();
        Self {
            stt_model: c.stt_model.unwrap_or(default.stt_model),
            cross_check_stt_models: c
                .cross_check_stt_models
                .unwrap_or(default.cross_check_stt_models),
            vision_model: c.vision_model.unwrap_or(default.vision_model),
            sound_annotation_model: c
                .sound_annotation_model
                .unwrap_or(default.sound_annotation_model),
            tagging_model: c.tagging_model.unwrap_or(default.tagging_model),
        }
    }
}

impl From<KaskModelsSettingsContent> for KaskModelsSettings {
    fn from(c: KaskModelsSettingsContent) -> Self {
        let default = Self::default();
        Self {
            default_model: c.default_model.unwrap_or(default.default_model),
            embedding_model: c.embedding_model.unwrap_or(default.embedding_model),
            classifier_model: c.classifier_model.unwrap_or(default.classifier_model),
            qa_generation_model: c.qa_generation_model.unwrap_or(default.qa_generation_model),
            ocr_model: c.ocr_model.unwrap_or(default.ocr_model),
            rerank_model: c.rerank_model.unwrap_or(default.rerank_model),
            decision_model: c.decision_model.unwrap_or(default.decision_model),
        }
    }
}

impl From<KaskSettingsContent> for KaskSettings {
    fn from(c: KaskSettingsContent) -> Self {
        Self {
            data_dir: c.data_dir.unwrap_or_default(),
            artifacts_dir: c.artifacts_dir.unwrap_or_default(),
            general: c.general.map(Into::into).unwrap_or_default(),
            mcp: c.mcp.map(Into::into).unwrap_or_default(),
            media: c.media.map(Into::into).unwrap_or_default(),
            curator: c.curator.map(Into::into).unwrap_or_default(),
            memory: c.memory.map(Into::into).unwrap_or_default(),
            condenser: c.condenser.map(Into::into).unwrap_or_default(),
            research: c.research.map(Into::into).unwrap_or_default(),
            corpus: c.corpus.map(Into::into).unwrap_or_default(),
            scenarios: c.scenarios.map(Into::into).unwrap_or_default(),
            prediction_markets: c.prediction_markets.map(Into::into).unwrap_or_default(),
            models: c.models.map(Into::into).unwrap_or_default(),
            private_inference: c.private_inference.map(Into::into).unwrap_or_default(),
            nostr: c.nostr.map(Into::into).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_classifier_uses_saved_setting_and_normalizes_whitespace() {
        let models = KaskModelsSettings {
            classifier_model: "  openrouter/openai/gpt-6-sol  ".to_string(),
            ..Default::default()
        };

        assert_eq!(
            models.classifier_model_for_host().as_deref(),
            Some("openrouter/openai/gpt-6-sol")
        );
    }

    #[test]
    fn saved_classifier_reaches_host_and_mcp_children_without_fallback() {
        let settings = KaskSettings::from(KaskSettingsContent {
            models: Some(KaskModelsSettingsContent {
                classifier_model: Some("  openrouter/openai/gpt-6-sol  ".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(
            settings.models.classifier_model_for_host().as_deref(),
            Some("openrouter/openai/gpt-6-sol")
        );
        assert_eq!(
            settings
                .mcp_env()
                .get("HKASK_CLASSIFIER_MODEL")
                .map(String::as_str),
            Some("openrouter/openai/gpt-6-sol")
        );
    }

    #[test]
    fn host_classifier_treats_blank_setting_as_unconfigured() {
        let models = KaskModelsSettings {
            classifier_model: "   \n\t".to_string(),
            ..Default::default()
        };

        assert_eq!(models.classifier_model_for_host(), None);
    }

    // The media server has no in-server model fallback — the settings
    // default is the only thing that feeds HKASK_MEDIA_* into the server
    // child. Before 2026-09-04 both fields defaulted to empty and the
    // transcribe + gallery-analysis pipelines shipped dead (NotConfigured
    // on every call). This test pins the wiring: every media model field —
    // STT, vision, and the two role models (sound annotation, grounded
    // tagging; operator ruling 2026-10-30) — carries its model_constants
    // default, and the struct exposes exactly those four fields (defaults
    // live in `Default` impls only, never serde(default) or From<Content>
    // literals).
    #[test]
    fn media_settings_default_carries_stt_vision_and_role_models() {
        let default = KaskMediaSettings::default();
        assert_eq!(
            default.stt_model,
            hkask_inference::model_constants::DEFAULT_MEDIA_STT_MODEL,
            "default stt_model must carry the settings-layer constant"
        );
        assert_eq!(
            default.vision_model,
            hkask_inference::model_constants::DEFAULT_MEDIA_VISION_MODEL,
            "default vision_model must carry the settings-layer constant"
        );
        assert_eq!(
            default.sound_annotation_model,
            hkask_inference::model_constants::DEFAULT_MEDIA_SOUND_ANNOTATION_MODEL,
            "default sound_annotation_model must carry the settings-layer constant"
        );
        assert_eq!(
            default.tagging_model,
            hkask_inference::model_constants::DEFAULT_MEDIA_TAGGING_MODEL,
            "default tagging_model must carry the settings-layer constant"
        );
        assert_eq!(
            default.cross_check_stt_models,
            hkask_inference::model_constants::DEFAULT_MEDIA_CROSS_CHECK_STT_MODELS
                .iter()
                .map(|model| model.to_string())
                .collect::<Vec<_>>(),
            "default cross_check_stt_models must carry the settings-layer fleet"
        );
        let serialized = serde_json::to_value(default).expect("serialize research media settings");
        let fields = serialized.as_object().expect("media settings object");
        assert_eq!(fields.len(), 5);
        assert!(fields.contains_key("stt_model"));
        assert!(fields.contains_key("cross_check_stt_models"));
        assert!(fields.contains_key("vision_model"));
        assert!(fields.contains_key("sound_annotation_model"));
        assert!(fields.contains_key("tagging_model"));
    }

    // Regression test for the silent `embedding_dim == 0` bug. A user
    // setting `embedding_dim: 0` in their settings file would construct a
    // zero-dimensional EmbeddingStore that rejects every vector with
    // `DimensionMismatch { expected: 0, actual: N }`, silently disabling
    // embedding-based recall. The `unwrap_or(1024)` default only fires for
    // `None`, not for `Some(0)`. This test pins the fix: 0 is treated as
    // "use default".
    #[test]
    fn corpus_settings_treats_zero_embedding_dim_as_default() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: Some(0),
                embedding_model: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(
            settings.corpus.embedding_dim, 1024,
            "embedding_dim: 0 should fall back to the default (1024), \
             not construct a zero-dimensional store"
        );
    }

    #[test]
    fn corpus_settings_preserves_explicit_nonzero_embedding_dim() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: Some(2560),
                embedding_model: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(settings.corpus.embedding_dim, 2560);
    }

    #[test]
    fn corpus_settings_defaults_embedding_dim_when_absent() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: None,
                embedding_model: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(settings.corpus.embedding_dim, 1024);
    }

    // Regression test for the Default-path crash (2026-07-28).
    // KaskCorpusSettings derived Default, but u32::default() == 0, not 1024.
    // The #[serde(default)] attribute only fires during deserialization, NOT
    // for Default::default(). When the user had no `kask.corpus` section,
    // from_settings fell back to KaskSettings::default() →
    // KaskCorpusSettings::default() → embedding_dim: 0, which panicked in
    // EmbeddingStore::from_driver (assert dim > 0). The manual Default impl
    // above returns 1024, matching the serde default.
    #[test]
    fn corpus_settings_default_embedding_dim_is_not_zero() {
        let default_corpus = KaskCorpusSettings::default();
        assert_eq!(
            default_corpus.embedding_dim, 1024,
            "KaskCorpusSettings::default() must return embedding_dim: 1024, not 0 — \
             a zero-dimensional store panics in EmbeddingStore::from_driver"
        );
    }

    // KaskSettings::default() (used when there is no kask section at all) must
    // also produce a non-zero embedding_dim, since it delegates to
    // KaskCorpusSettings::default() for the corpus field.
    #[test]
    fn kask_settings_default_corpus_embedding_dim_is_not_zero() {
        let settings = KaskSettings::default();
        assert_eq!(settings.corpus.embedding_dim, 1024);
    }

    // Regression test for the silent MCP-server-registration bug (2026-07-28).
    // KaskMcpSettings derived Default, but bool::default() == false, not true.
    // The #[serde(default = "default_true")] attribute only fires during
    // deserialization, NOT for Default::default(). When the user had a `kask`
    // section but no `kask.mcp` subsection, From<KaskSettingsContent> fell back
    // to KaskMcpSettings::default() → load_default: false, and sync_kask_mcp_servers
    // treated all built-in servers as disabled, registering nothing. The manual Default
    // impl above returns true, matching the serde default.
    #[test]
    fn mcp_settings_default_load_default_is_true() {
        let default_mcp = KaskMcpSettings::default();
        assert!(
            default_mcp.load_default,
            "KaskMcpSettings::default() must return load_default: true, not false — \
             a false default silently disables all kask MCP server registration"
        );
    }

    // The bug manifests when a user has a `kask` section but no `kask.mcp`
    // subsection: From<KaskSettingsContent> hits the `.unwrap_or_default()` path.
    // This test pins that path to load_default: true.
    #[test]
    fn kask_settings_from_content_without_mcp_section_loads_defaults() {
        let content = KaskSettingsContent {
            mcp: None,
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(
            settings.mcp.load_default,
            "a kask section with no mcp subsection must default to load_default: true"
        );
    }

    // ── Drift-class regression tests ─────────────────────────────────────────
    //
    // The triple-default drift class: a struct with `#[serde(default =
    // "default_true")]` on a bool field AND `#[derive(Default)]` would get
    // `Default::default() → false`, disagreeing with the serde spec (`true`).
    // When the user omitted that subsection, `From` hit `.unwrap_or_default()` and
    // silently used the wrong default. These tests pin every drifting subsection's
    // `Default` to match the intended (serde) default, so the drift can't recur.

    #[test]
    fn curator_settings_default_always_on_is_true() {
        assert!(
            KaskCuratorSettings::default().always_on,
            "KaskCuratorSettings::default() must return always_on: true"
        );
    }

    #[test]
    fn memory_settings_default_auto_inject_is_true() {
        assert!(
            KaskMemorySettings::default().auto_inject,
            "KaskMemorySettings::default() must return auto_inject: true"
        );
    }

    #[test]
    fn condenser_settings_default_auto_compress_is_false() {
        assert!(
            !KaskCondenserSettings::default().auto_compress_tool_results,
            "KaskCondenserSettings::default() must return auto_compress_tool_results: false"
        );
    }

    // The absent-subsection path: when a user has a `kask` section but omits a
    // subsection, `From` hits `.unwrap_or_default()`. This test verifies ALL
    // subsections produce their intended defaults through that path.
    #[test]
    fn kask_settings_from_empty_content_uses_all_defaults() {
        let settings = KaskSettings::from(KaskSettingsContent::default());
        assert!(settings.mcp.load_default);
        assert!(settings.curator.always_on);
        assert_eq!(settings.curator.algedonic_threshold, 0.8);
        assert_eq!(
            settings.curator.interaction_mode,
            CuratorInteractionMode::Collaboration
        );
        assert!(settings.memory.auto_inject);
        assert_eq!(settings.memory.consolidation_cadence_secs, 300);
        assert!(!settings.memory.federated_auto_inject);
        assert!(settings.memory.federated_source_ids.is_empty());
        assert_eq!(settings.memory.distillation_cadence_secs, 600);
        assert_eq!(settings.memory.distillation_idle_secs, 300);
        assert!(!settings.condenser.auto_compress_tool_results);
        assert_eq!(settings.condenser.profile, "normal");
        assert_eq!(settings.corpus.embedding_dim, 1024);
        // Both storage roots default to empty (runtime resolves the platform
        // default) — the artifacts root is a first-class setting, not a
        // hard-coded path.
        assert_eq!(settings.data_dir, "");
        assert_eq!(settings.artifacts_dir, "");
    }

    #[test]
    fn private_inference_from_absent_content_fields_uses_defaults() {
        let effective: KaskPrivateInferenceSettings =
            KaskPrivateInferenceSettingsContent::default().into();
        assert_eq!(
            effective.private_thread_disabled_servers,
            settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
            "absent perimeter content uses the single-home default set"
        );
        assert_eq!(
            effective.classifier_model,
            hkask_inference::model_constants::DEFAULT_PRIVATE_CLASSIFIER_MODEL,
            "absent classifier content uses the single-home default (the operator's named local pick)"
        );
        assert_eq!(
            effective.rerank_model,
            hkask_inference::model_constants::DEFAULT_PRIVATE_RERANK_MODEL,
            "absent rerank content uses the single-home default (the local ollama reranker)"
        );
        // A present section with every field set overrides each one.
        let content = KaskPrivateInferenceSettingsContent {
            private_thread_disabled_servers: Some(vec!["media".into()]),
            embedding_model: Some("ollama/custom-embed".into()),
            classifier_model: Some("ollama/custom-classifier".into()),
            rerank_model: Some("ollama/custom-rerank".into()),
        };
        let effective: KaskPrivateInferenceSettings = content.into();
        assert_eq!(
            effective.private_thread_disabled_servers,
            vec!["media".to_string()]
        );
        assert_eq!(effective.embedding_model, "ollama/custom-embed");
        assert_eq!(effective.classifier_model, "ollama/custom-classifier");
        assert_eq!(effective.rerank_model, "ollama/custom-rerank");
    }

    // D100 item 3 (operator 2026-11-05) — the private leg's local embedding
    // model: absent content uses the single-home default (the model_constants
    // constant, never a second literal), a present value overrides it, and an
    // EXPLICITLY EMPTY string is the operator's opt-out (distinct from absent
    // — the keyword-only degrade persists instead of the default local model).
    #[test]
    fn private_embedding_model_defaults_overrides_and_opts_out() {
        let default = KaskPrivateInferenceSettings::default();
        assert_eq!(
            default.embedding_model,
            hkask_inference::model_constants::DEFAULT_PRIVATE_EMBEDDING_MODEL
        );
        let absent: KaskPrivateInferenceSettings =
            KaskPrivateInferenceSettingsContent::default().into();
        assert_eq!(
            absent.embedding_model,
            hkask_inference::model_constants::DEFAULT_PRIVATE_EMBEDDING_MODEL,
            "an absent field uses the default local model"
        );
        // The classifier sibling (D100 classifier lift, 2026-11-06): the
        // same absent-default / explicit-empty contract.
        let classifier_default: KaskPrivateInferenceSettings =
            KaskPrivateInferenceSettingsContent::default().into();
        assert_eq!(
            classifier_default.classifier_model,
            hkask_inference::model_constants::DEFAULT_PRIVATE_CLASSIFIER_MODEL,
            "an absent field uses the operator's named local pick"
        );
        let classifier_opted_out: KaskPrivateInferenceSettings =
            KaskPrivateInferenceSettingsContent {
                classifier_model: Some(String::new()),
                ..Default::default()
            }
            .into();
        assert_eq!(
            classifier_opted_out.classifier_model, "",
            "an explicitly empty string is the structural-only opt-out"
        );
        let opted_out: KaskPrivateInferenceSettings = KaskPrivateInferenceSettingsContent {
            embedding_model: Some(String::new()),
            ..Default::default()
        }
        .into();
        assert_eq!(
            opted_out.embedding_model, "",
            "an explicitly empty string is the operator's opt-out, not the default"
        );
    }

    // The artifacts root must flow through mcp_env() with the same priority
    // chain as the data dir (settings field wins). Without this, an operator
    // `artifacts_dir` override is silently dropped and every `{server}-mcp/`
    // artifact route resolves under the Documents default instead.
    #[test]
    fn mcp_env_honors_artifacts_dir_setting() {
        let settings = KaskSettings {
            artifacts_dir: "/custom/artifacts-root".to_string(),
            ..Default::default()
        };
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_ARTIFACTS_DIR").map(String::as_str),
            Some("/custom/artifacts-root"),
            "artifacts_dir setting must be emitted as HKASK_ARTIFACTS_DIR"
        );
    }

    #[test]
    fn retired_training_settings_are_tolerated_but_cannot_reenter_runtime_state() {
        let content: KaskSettingsContent = serde_json::from_value(serde_json::json!({
            "training": {"host": "nebius", "cache_dir": "/tmp/adapters"},
            "mcp": {
                "load_default": true,
                "overrides": {"training": true, "corpus": false, "future-server": true},
                "delegated_tools": {
                    "training": ["training_submit"],
                    "corpus": ["corpus_query"],
                    "future-server": ["future_tool"]
                }
            }
        }))
        .expect("legacy kask.training JSON must remain upgrade-tolerant");
        let settings = KaskSettings::from(content);
        assert!(!settings.mcp.overrides.contains_key("training"));
        assert!(!settings.mcp.delegated_tools.contains_key("training"));
        assert_eq!(settings.mcp.overrides.get("corpus"), Some(&false));
        assert_eq!(settings.mcp.overrides.get("future-server"), Some(&true));
        assert_eq!(
            settings.mcp.delegated_tools.get("corpus"),
            Some(&vec!["corpus_query".to_string()])
        );
        assert_eq!(
            settings.mcp.delegated_tools.get("future-server"),
            Some(&vec!["future_tool".to_string()])
        );
        let serialized = serde_json::to_value(settings).expect("runtime settings serialize");
        assert!(serialized.get("training").is_none());
    }

    // The present-but-null-field path: when a subsection is present but a field
    // is `None`, `From` hits `.unwrap_or(default.field)`. This test verifies the
    // field-level defaults also come from `Default`, not inlined literals.
    #[test]
    fn curator_interaction_mode_is_read_from_the_user_setting() {
        let content: KaskCuratorSettingsContent =
            serde_json::from_str(r#"{"interaction_mode": "learning_collaboration"}"#)
                .expect("interaction_mode must deserialize from its snake_case name");
        let curator = KaskCuratorSettings::from(content);
        assert_eq!(
            curator.interaction_mode,
            CuratorInteractionMode::LearningCollaboration
        );
        let control: KaskCuratorSettingsContent =
            serde_json::from_str(r#"{"interaction_mode": "control"}"#)
                .expect("control must deserialize");
        assert_eq!(
            KaskCuratorSettings::from(control).interaction_mode,
            CuratorInteractionMode::Control
        );
    }

    #[test]
    fn kask_settings_from_present_subsection_with_null_fields_uses_defaults() {
        let content = KaskSettingsContent {
            mcp: Some(KaskMcpSettingsContent {
                load_default: None,
                overrides: HashMap::default(),
                delegated_tools: None,
            }),
            curator: Some(KaskCuratorSettingsContent {
                always_on: None,
                algedonic_threshold: None,
                interaction_mode: None,
            }),
            memory: Some(KaskMemorySettingsContent {
                consolidation_cadence_secs: None,
                confidence_floor: None,
                recall_limit: None,
                recall_min_confidence: None,
                auto_inject: None,
                federated_auto_inject: None,
                federated_source_ids: None,
                memory_life_days: None,
                distillation_cadence_secs: None,
                distillation_idle_secs: None,
                forgetting_days: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(settings.mcp.load_default);
        assert!(settings.curator.always_on);
        assert_eq!(settings.curator.algedonic_threshold, 0.8);
        assert!(settings.memory.auto_inject);
        assert_eq!(settings.memory.consolidation_cadence_secs, 300);
        assert!(!settings.memory.federated_auto_inject);
        assert!(settings.memory.federated_source_ids.is_empty());
    }

    #[test]
    fn federated_settings_content_preserves_multiple_selected_ids() {
        let content = KaskSettingsContent {
            memory: Some(KaskMemorySettingsContent {
                federated_auto_inject: Some(true),
                federated_source_ids: Some(vec!["alpha".into(), "beta".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(settings.memory.federated_auto_inject);
        assert_eq!(settings.memory.federated_source_ids, ["alpha", "beta"]);
    }

    #[test]
    fn federated_sources_require_valid_manifest_in_configured_data_dir() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let settings = KaskSettings {
            data_dir: dir.path().to_string_lossy().into_owned(),
            ..Default::default()
        };
        assert!(settings.federated_sources().is_err());
        let manifest_path = dir
            .path()
            .join(hkask_types::agent_paths::agent_dir("curator"))
            .join("federated-sources.json");
        std::fs::create_dir_all(
            manifest_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("no parent"))?,
        )?;
        std::fs::write(&manifest_path, "not json")?;
        assert!(settings.federated_sources().is_err());
        std::fs::write(
            &manifest_path,
            r#"{"schema_version":1,"sources":[{"id":"alpha","display_name":"Alpha corpus","database_path":"a.db","run_identity_path":"a.json","representations_manifest_path":"r.json","index_name":"main"},{"id":"beta","display_name":"Beta corpus","database_path":"b.db","run_identity_path":"b.json","representations_manifest_path":"s.json","index_name":"main"}]}"#,
        )?;
        assert_eq!(
            settings.federated_sources()?,
            vec![
                ("alpha".to_string(), "Alpha corpus".to_string()),
                ("beta".to_string(), "Beta corpus".to_string()),
            ]
        );
        Ok(())
    }

    // When `models.embedding_model` is empty, the effective value falls back
    // to the corpus setting — whose `Default` carries the code default
    // (operator ruling 2026-09-04). Lives in `settings.rs` (not
    // `mcp_env.rs`) because it pins `effective_embedding_model`, a method on
    // `KaskSettings` defined here.
    #[test]
    fn effective_embedding_model_falls_back_to_corpus_when_models_empty() {
        let mut settings = KaskSettings::default();
        // Nothing configured → the corpus-layer code default.
        assert_eq!(
            settings.effective_embedding_model(),
            "OpenRouter/qwen/qwen3-embedding-8b"
        );
        settings.corpus.embedding_model = "OpenAI/text-embedding-3-large".to_string();
        assert_eq!(
            settings.effective_embedding_model(),
            "OpenAI/text-embedding-3-large"
        );
        // The kask-wide override takes precedence over the corpus setting.
        settings.models.embedding_model = "voyage/voyage-3".to_string();
        assert_eq!(settings.effective_embedding_model(), "voyage/voyage-3");
    }
}
