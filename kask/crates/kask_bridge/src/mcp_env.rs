//! MCP server env-var translators — the config half of `build_mcp_server_env`.
//!
//! Each `emit_*_env` free function translates one `Kask*Settings` subsection
//! into the env vars the corresponding MCP server reads at startup. Only
//! non-empty/non-default values are included — MCP servers have their own
//! fallback defaults for unset env vars.
//!
//! These are called by [`KaskSettings::mcp_env`](crate::settings::KaskSettings::mcp_env),
//! which is the config half of the env map. The full env for a server child
//! process — config + keychain credentials + the inference socket — is
//! assembled by [`build_mcp_server_env`](crate::build_mcp_server_env) in
//! `mcp_servers`, the single canonical path.

use crate::settings::{
    KaskCorpusSettings, KaskGeneralSettings, KaskMediaSettings, KaskMemorySettings,
    KaskModelsSettings, KaskPredictionMarketsSettings, KaskResearchSettings,
};

// Defaults are read from each subsection's `Default` impl so there's a
// single source of truth — changing `Default` automatically updates the
// comparison here. Do not inline magic numbers; they drift from
// `Default` (the same drift class that silently disabled all 10 kask
// MCP servers when `KaskMcpSettings::default()` disagreed with the
// serde default).

/// Emit general settings that MCP servers consume — the process-wide
/// concurrency ceiling from `KaskGeneralSettings.max_concurrency`.
/// MCP servers use this as the default for their own concurrency limits
/// (e.g. the corpus embedding concurrency) instead of hardcoding magic
/// numbers. Only non-default values are emitted; servers fall back to
/// their own defaults when the env var is absent.
pub(crate) fn emit_general_env(
    general: &KaskGeneralSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    let default = KaskGeneralSettings::default();
    if general.max_concurrency != default.max_concurrency {
        env.insert(
            "HKASK_MAX_CONCURRENCY".to_string(),
            general.max_concurrency.to_string(),
        );
    }
}

pub(crate) fn emit_curator_webid_env(env: &mut std::collections::HashMap<String, String>) {
    // Map the curator's WebID (stashed in `HKASK_CURATOR_WEBID` by the
    // deferred task) to `HKASK_WEBID` so the curator MCP server picks it
    // up as its identity. The `config_env` allowlist filters this to the
    // curator server only — other servers don't receive `HKASK_WEBID`
    // from this mapping and fall through to their own identity resolution.
    if let Ok(curator_webid) = std::env::var("HKASK_CURATOR_WEBID") {
        env.insert("HKASK_WEBID".to_string(), curator_webid);
    }
}

/// Emit the ALWAYS-mode distillation cadence/idle settings for the curator
/// server. Emitted only when they differ from the defaults (the condenser
/// env pattern) — the curator server's own defaults apply otherwise. The
/// per-server allowlist in `mcp_servers.rs` filters these from every other
/// server.
pub(crate) fn emit_curator_distillation_env(
    memory: &KaskMemorySettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    let default = KaskMemorySettings::default();
    if memory.distillation_cadence_secs != default.distillation_cadence_secs {
        env.insert(
            "HKASK_MEMORY_DISTILLATION_CADENCE_SECS".to_string(),
            memory.distillation_cadence_secs.to_string(),
        );
    }
    if memory.distillation_idle_secs != default.distillation_idle_secs {
        env.insert(
            "HKASK_MEMORY_DISTILLATION_IDLE_SECS".to_string(),
            memory.distillation_idle_secs.to_string(),
        );
    }
    if memory.forgetting_days != default.forgetting_days {
        env.insert(
            "HKASK_MEMORY_FORGETTING_DAYS".to_string(),
            memory.forgetting_days.to_string(),
        );
    }
    if memory.memory_life_days != default.memory_life_days {
        env.insert(
            "HKASK_MEMORY_LIFE_DAYS".to_string(),
            memory.memory_life_days.to_string(),
        );
    }
}

pub(crate) fn emit_research_env(
    research: &KaskResearchSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !research.research_db.is_empty() {
        env.insert(
            "HKASK_RESEARCH_DB".to_string(),
            research.research_db.clone(),
        );
    }
}

pub(crate) fn emit_corpus_embedding_env(
    corpus: &KaskCorpusSettings,
    effective_embedding_model: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    let corpus_default = KaskCorpusSettings::default();
    if corpus.embedding_dim != corpus_default.embedding_dim {
        env.insert(
            "HKASK_EMBEDDING_DIM".to_string(),
            corpus.embedding_dim.to_string(),
        );
    }
    // ── Embedding model ──
    // Precedence: models.embedding_model → corpus.embedding_model →
    // default. Resolved once by `effective_embedding_model` so the
    // emission cannot drift from the documented precedence. See its
    // doc comment and the `mcp_env_models_embedding_model_overrides_corpus` test.
    //
    // Emitted UNCONDITIONALLY: the former default-equality suppression
    // assumed "MCP servers have their own fallback defaults for unset env
    // vars" — they do not (every consumer of HKASK_EMBEDDING_MODEL fails
    // visibly on unset env), and once the corpus default became a real
    // model (operator ruling 2026-09-04: defaults in code), an explicit
    // setting equal to the default was silently suppressed — the exact
    // live gap observed 2026-09-04. The value is always meaningful; emit
    // it always.
    env.insert(
        "HKASK_EMBEDDING_MODEL".to_string(),
        effective_embedding_model.to_string(),
    );
}

/// Resolve the host-deployed template registry root by the canonical rule
/// shared by every template consumer: the operator's
/// `kask.corpus.template_root` override when set, else
/// `{data_dir}/skills/registry/` — the path `agent_skills::seed_templates`
/// writes at startup. The settings default (`"kask/registry"`,
/// CWD-relative) is treated as unset so production never resolves against
/// the process CWD.
///
/// One rule, two delivery seams: MCP server child processes receive the
/// root via the `HKASK_TEMPLATE_ROOT` env var
/// (`emit_corpus_template_root_env`); in-process consumers (curator memory
/// chunk tagging) read it via `KaskSettings::resolved_template_root` — the
/// env var is never set in the editor process itself.
pub(crate) fn resolve_template_root(corpus: &KaskCorpusSettings, data_dir: &str) -> String {
    let corpus_default = KaskCorpusSettings::default();
    if corpus.template_root != corpus_default.template_root {
        corpus.template_root.clone()
    } else {
        std::path::Path::new(data_dir)
            .join("skills")
            .join("registry")
            .to_string_lossy()
            .to_string()
    }
}

pub(crate) fn emit_corpus_template_root_env(
    corpus: &KaskCorpusSettings,
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    // Always emit HKASK_TEMPLATE_ROOT so MCP servers (corpus,
    // swarm) find templates in production where the CWD-relative default
    // does not exist.
    env.insert(
        "HKASK_TEMPLATE_ROOT".to_string(),
        resolve_template_root(corpus, data_dir),
    );
}

pub(crate) fn emit_scenarios_env(
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    // D28 — Standardized Artifact Storage. The scenarios data dir is derived
    // from the global `data_dir` as `mcp/scenarios/`. No per-server override —
    // one global data root, each server owns its subfolder.
    let scenarios_data_dir = std::path::Path::new(data_dir)
        .join(hkask_types::agent_paths::mcp_server_subdir("scenarios", ""))
        .to_string_lossy()
        .to_string();
    env.insert("HKASK_SCENARIOS_DATA".to_string(), scenarios_data_dir);
}

pub(crate) fn emit_prediction_markets_env(
    data_dir: &str,
    prediction_markets: &KaskPredictionMarketsSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // D28 — Standardized Artifact Storage. The prediction-markets data dir
    // is derived from the global `data_dir` as `mcp/prediction-markets/`.
    // No per-server override — one global data root, each server owns its
    // subfolder.
    let prediction_markets_data_dir = std::path::Path::new(data_dir)
        .join(hkask_types::agent_paths::mcp_server_subdir(
            "prediction-markets",
            "",
        ))
        .to_string_lossy()
        .to_string();
    env.insert(
        "HKASK_PREDICTION_MARKETS_DATA".to_string(),
        prediction_markets_data_dir,
    );
    if prediction_markets.cache_ttl_secs > 0 {
        env.insert(
            "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS".to_string(),
            prediction_markets.cache_ttl_secs.to_string(),
        );
    }
}

pub(crate) fn emit_media_env(
    media: &KaskMediaSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !media.stt_model.is_empty() {
        env.insert("HKASK_MEDIA_STT_MODEL".to_string(), media.stt_model.clone());
    }
    if !media.vision_model.is_empty() {
        env.insert(
            "HKASK_MEDIA_VISION_MODEL".to_string(),
            media.vision_model.clone(),
        );
    }
}

/// Settings-struct-backed `config_env` keys emitted by `KaskSettings::mcp_env`.
///
/// These are governed by `KaskSettings` fields (including deliberate `Default`
/// suppression so unset settings fall back to server defaults). Every OTHER
/// `BuiltinMcpServer::config_env` key is an operator/process env override with
/// no `KaskSettings` field, forwarded from the parent environment by
/// [`emit_operator_override_env`] so it survives `cmd.env_clear()`.
const SETTINGS_BACKED_CONFIG_ENV_VARS: &[&str] = &[
    "HKASK_DATA_DIR",
    "HKASK_ARTIFACTS_DIR",
    "HKASK_MAX_CONCURRENCY",
    "HKASK_WEBID",
    "HKASK_MEMORY_DISTILLATION_CADENCE_SECS",
    "HKASK_MEMORY_DISTILLATION_IDLE_SECS",
    "HKASK_MEMORY_FORGETTING_DAYS",
    "HKASK_MEMORY_LIFE_DAYS",
    "HKASK_MCP_SERVER_IDS",
    "HKASK_RESEARCH_DB",
    "HKASK_EMBEDDING_DIM",
    "HKASK_EMBEDDING_MODEL",
    "HKASK_TEMPLATE_ROOT",
    "HKASK_SCENARIOS_DATA",
    "HKASK_PREDICTION_MARKETS_DATA",
    "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
    "HKASK_MEDIA_STT_MODEL",
    "HKASK_MEDIA_VISION_MODEL",
    "HKASK_DEFAULT_MODEL",
    "HKASK_CLASSIFIER_MODEL",
    hkask_inference::model_constants::QA_GENERATION_MODEL_ENV,
    "HKASK_OCR_MODEL",
    "HKASK_RERANK_MODEL",
];

/// Pass through operator/process overrides for server `config_env` knobs that
/// have no `KaskSettings` field. Derived directly from
/// [`crate::BUILT_IN_MCP_SERVERS`] so every allowlisted `config_env` key has a
/// real delivery source across `cmd.env_clear()` without a parallel static list.
pub(crate) fn emit_operator_override_env(env: &mut std::collections::HashMap<String, String>) {
    for server in crate::BUILT_IN_MCP_SERVERS {
        for &name in server.config_env {
            if SETTINGS_BACKED_CONFIG_ENV_VARS.contains(&name) || env.contains_key(name) {
                continue;
            }
            if let Ok(value) = std::env::var(name)
                && !value.trim().is_empty()
            {
                env.insert(name.to_string(), value);
            }
        }
    }
}

pub(crate) fn emit_models_env(
    models: &KaskModelsSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // ── Kask-wide model overrides ──
    // These take precedence over the per-server model settings above.
    if !models.default_model.is_empty() {
        env.insert(
            "HKASK_DEFAULT_MODEL".to_string(),
            models.default_model.clone(),
        );
    }
    // Embedding model already emitted above via `effective_embedding_model`,
    // which encodes the `models`-over-`corpus` precedence. Do not emit it
    // again here — a second insert would be a silent duplicate.
    let classifier_model = models.classifier_model.trim();
    if !classifier_model.is_empty() {
        env.insert(
            "HKASK_CLASSIFIER_MODEL".to_string(),
            classifier_model.to_string(),
        );
    }
    if !models.qa_generation_model.is_empty() {
        env.insert(
            hkask_inference::model_constants::QA_GENERATION_MODEL_ENV.to_string(),
            models.qa_generation_model.clone(),
        );
    }
    if !models.ocr_model.is_empty() {
        env.insert("HKASK_OCR_MODEL".to_string(), models.ocr_model.clone());
    }
    if !models.rerank_model.is_empty() {
        env.insert(
            "HKASK_RERANK_MODEL".to_string(),
            models.rerank_model.clone(),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::KaskSettings;

    // `mcp_env()` must not emit env vars for settings that match `Default`.
    // Previously `mcp_env()` compared against inlined magic numbers (1024, 4,
    // 0.05, 0.15, 0.10, "registry", 5) that duplicated `Default` values. If
    // `Default` changed, the comparison would drift and emit env vars for the
    // default case. Now `mcp_env()` reads from `Default::default()`, so changing
    // `Default` automatically updates the comparison. This test pins that: a
    // `KaskSettings::default()` (all defaults) does not emit per-server config
    // vars. The always-emitted vars (`HKASK_DATA_DIR`, `HKASK_ARTIFACTS_DIR`,
    // `HKASK_TEMPLATE_ROOT`,
    // `HKASK_SCENARIOS_DATA`, `HKASK_PREDICTION_MARKETS_DATA`) are kask-wide
    // critical paths resolved from `data_dir` / the artifacts dir per the
    // Standardized Artifact Storage layout — they are NOT suppressed for
    // default settings. Model defaults (`HKASK_EMBEDDING_MODEL`,
    // `HKASK_MEDIA_STT_MODEL`, `HKASK_MEDIA_VISION_MODEL`) are also always
    // emitted: their consuming servers have no fallback for unset env, so
    // suppressing a default-equal value starves the pipeline. See the
    // `mcp_env_always_emits_*` tests for those.
    /// The decay constant reaches the curator server only through this
    /// emission (allowlisted there, read by `memory_life_days_from_env`).
    /// Emitted exactly when it differs from the default — the same pattern
    /// as the distillation/forgetting knobs.
    #[test]
    fn mcp_env_emits_memory_life_days_only_when_configured() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        assert!(
            !env.contains_key("HKASK_MEMORY_LIFE_DAYS"),
            "the default decay constant must not be emitted — the curator \
             server's own default is the same value"
        );

        let mut settings = KaskSettings::default();
        settings.memory.memory_life_days = 30.0;
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_MEMORY_LIFE_DAYS").map(String::as_str),
            Some("30"),
            "a configured decay constant must reach the curator server"
        );
    }

    #[test]
    fn mcp_env_emits_nothing_for_default_settings() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        // The only env vars that could appear for default settings are the
        // corpus/condenser numeric defaults — all should be suppressed because
        // they match `Default`.
        assert!(
            !env.contains_key("HKASK_EMBEDDING_DIM"),
            "default embedding_dim must not be emitted"
        );
        // HKASK_TEMPLATE_ROOT is now always emitted (resolved from data_dir),
        // so it's no longer suppressed for default settings — see
        // `mcp_env_always_emits_template_root`.
        //
        // HKASK_EMBEDDING_MODEL is also always emitted: servers have no
        // fallback for unset env (they fail visibly), so suppressing a
        // default-equal value starved them (live gap 2026-09-04). The
        // default itself is a real model — emit it.
        assert!(
            env.contains_key("HKASK_EMBEDDING_MODEL"),
            "the embedding model must always be emitted — servers have no fallback for unset env"
        );
        assert_eq!(
            env.get("HKASK_EMBEDDING_MODEL").map(String::as_str),
            Some("OpenRouter/qwen/qwen3-embedding-8b"),
            "default settings emit the code-default embedding model"
        );
        // Media model defaults are emitted for the same reason as the
        // embedding model: the media server has no fallback for unset env,
        // and suppressing a default-equal value starved the STT and vision
        // pipelines (live gap 2026-09-04 — transcription and gallery
        // analysis both failed NotConfigured on every call).
        assert!(
            env.contains_key("HKASK_MEDIA_STT_MODEL"),
            "default stt_model must be emitted — the media server has no fallback"
        );
        assert_eq!(
            env.get("HKASK_MEDIA_STT_MODEL").map(String::as_str),
            Some(hkask_inference::model_constants::DEFAULT_MEDIA_STT_MODEL),
            "default settings emit the code-default STT model"
        );
        assert!(
            env.contains_key("HKASK_MEDIA_VISION_MODEL"),
            "default vision_model must be emitted — the media server has no fallback"
        );
        assert_eq!(
            env.get("HKASK_MEDIA_VISION_MODEL").map(String::as_str),
            Some(hkask_inference::model_constants::DEFAULT_MEDIA_VISION_MODEL),
            "default settings emit the code-default vision model"
        );
        // The per-server data-dir env vars are now always emitted (resolved
        // from `data_dir` per the Standardized Artifact Storage layout), so
        // they are NOT suppressed for default settings. See
        // `mcp_env_always_emits_per_server_data_dirs`.
        assert!(
            env.contains_key("HKASK_SCENARIOS_DATA"),
            "HKASK_SCENARIOS_DATA must always be emitted — the scenarios server resolves its persistence dir from it"
        );
        assert!(
            env.contains_key("HKASK_PREDICTION_MARKETS_DATA"),
            "HKASK_PREDICTION_MARKETS_DATA must always be emitted — the prediction-markets server resolves its calibration journal from it"
        );
    }

    // The per-server data-dir env vars (`HKASK_SCENARIOS_DATA`,
    // `HKASK_PREDICTION_MARKETS_DATA`) must ALWAYS be
    // emitted by `mcp_env()`. They resolve from `data_dir` per
    // the Standardized Artifact Storage layout. Without this, an operator
    // override is silently dropped for those servers — the per-server env
    // var is the canonical delivery path and the server's fallback only
    // works when the root env var is also allowlisted (which it now is for
    // scenarios/prediction-markets).
    #[test]
    fn mcp_env_always_emits_per_server_data_dirs() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        let data_dir = env
            .get("HKASK_DATA_DIR")
            .expect("HKASK_DATA_DIR must be emitted");
        // Scenarios data dir resolves under the scenarios server's subtree.
        let expected_scenarios = std::path::Path::new(data_dir)
            .join("mcp")
            .join("scenarios")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            env.get("HKASK_SCENARIOS_DATA").map(String::as_str),
            Some(expected_scenarios.as_str()),
            "default scenarios_data must resolve to `{{data_dir}}/mcp/scenarios`"
        );
        // Prediction-markets data dir resolves under its server's subtree.
        let expected_pm = std::path::Path::new(data_dir)
            .join("mcp")
            .join("prediction-markets")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            env.get("HKASK_PREDICTION_MARKETS_DATA").map(String::as_str),
            Some(expected_pm.as_str()),
            "default prediction_markets_data must resolve to `{{data_dir}}/mcp/prediction-markets`"
        );
    }

    // `HKASK_DATA_DIR` is a kask-wide critical path — it must ALWAYS be
    // injected by `mcp_env()` so every MCP server can resolve databases
    // consistently, even when the operator never set the env var or the
    // settings field. The resolved default comes from
    // `hkask_types::agent_paths::resolve_data_dir()`.
    #[test]
    fn mcp_env_always_emits_data_dir() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        assert!(
            env.contains_key("HKASK_DATA_DIR"),
            "HKASK_DATA_DIR must always be emitted — without it, MCP servers \
             cannot resolve database paths consistently"
        );
        let dir = env
            .get("HKASK_DATA_DIR")
            .expect("HKASK_DATA_DIR checked above");
        assert!(
            !dir.is_empty(),
            "HKASK_DATA_DIR must resolve to a non-empty path even for default settings"
        );
    }

    // When the operator sets `data_dir` in settings, `mcp_env()` must use
    // that value instead of the env var or resolved default.
    #[test]
    fn mcp_env_data_dir_setting_overrides_env() {
        let mut settings = KaskSettings::default();
        settings.data_dir = "/custom/kask/data".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_DATA_DIR").map(String::as_str),
            Some("/custom/kask/data")
        );
    }

    // `HKASK_TEMPLATE_ROOT` must ALWAYS be emitted so MCP servers (corpus)
    // find templates in production where the CWD-relative default
    // ("kask/registry") does not exist. For default settings, the value is
    // resolved from `data_dir` as `{data_dir}/skills/registry/` — the path
    // where `seed_templates` writes at startup.
    #[test]
    fn mcp_env_always_emits_template_root() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        let template_root = env.get("HKASK_TEMPLATE_ROOT");
        assert!(
            template_root.is_some(),
            "HKASK_TEMPLATE_ROOT must always be emitted — without it, MCP servers \
             fall back to a CWD-relative path that does not exist in production"
        );
        let data_dir = env.get("HKASK_DATA_DIR").expect("data_dir must be emitted");
        let expected = std::path::Path::new(data_dir)
            .join("skills")
            .join("registry")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            template_root.map(String::as_str),
            Some(expected.as_str()),
            "default template_root must resolve to `{{data_dir}}/skills/registry`"
        );
    }

    // When the operator overrides `corpus.template_root`, `mcp_env()` must use
    // that value instead of the data-dir-resolved default.
    #[test]
    fn mcp_env_template_root_override_emits_user_value() {
        let mut settings = KaskSettings::default();
        settings.corpus.template_root = "/custom/templates".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_TEMPLATE_ROOT").map(String::as_str),
            Some("/custom/templates")
        );
    }

    // When the operator sets `data_dir` in settings, the template_root must
    // resolve from the custom data_dir, not the platform default.
    #[test]
    fn mcp_env_template_root_follows_data_dir_override() {
        let mut settings = KaskSettings::default();
        settings.data_dir = "/custom/kask/data".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_TEMPLATE_ROOT").map(String::as_str),
            Some("/custom/kask/data/skills/registry")
        );
    }

    // The in-process accessor must resolve identically to the env emission —
    // one rule, two delivery seams. A divergence means curator memory
    // tagging renders from a different registry than the MCP servers.
    #[test]
    fn resolved_template_root_method_matches_env_emission_rule() {
        let mut settings = KaskSettings::default();
        settings.data_dir = "/custom/kask/data".to_string();
        assert_eq!(
            settings.resolved_template_root(),
            std::path::PathBuf::from("/custom/kask/data/skills/registry")
        );
        settings.corpus.template_root = "/custom/templates".to_string();
        assert_eq!(
            settings.resolved_template_root(),
            std::path::PathBuf::from("/custom/templates")
        );
    }
    // This pins the other direction: the `Default`-based comparison still
    // detects non-default values.
    #[test]
    fn mcp_env_emits_for_non_default_settings() {
        let mut settings = KaskSettings::default();
        settings.corpus.embedding_dim = 2048;
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_EMBEDDING_DIM").map(String::as_str),
            Some("2048")
        );
    }

    // `models.embedding_model` defaults empty (the embedding default lives
    // in the corpus layer so it cannot shadow corpus overrides), so a
    // non-default `corpus.embedding_model` must be emitted.
    #[test]
    fn mcp_env_emits_embedding_model_when_overridden() {
        let mut settings = KaskSettings::default();
        settings.corpus.embedding_model = "OpenAI/text-embedding-3-large".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_EMBEDDING_MODEL").map(String::as_str),
            Some("OpenAI/text-embedding-3-large")
        );
    }

    // Precedence pin: when BOTH `corpus.embedding_model` and
    // `models.embedding_model` are set, the kask-wide `models` override must
    // win. Previously this precedence existed only as statement order in
    // `mcp_env()` (the `models` block overwrote the `corpus` block). It is now
    // explicit in `effective_embedding_model`; this test locks the contract so a
    // reorder cannot silently flip which setting wins.
    #[test]
    fn mcp_env_models_embedding_model_overrides_corpus() {
        let mut settings = KaskSettings::default();
        settings.corpus.embedding_model = "OpenAI/text-embedding-3-large".to_string();
        settings.models.embedding_model = "voyage/voyage-3".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_EMBEDDING_MODEL").map(String::as_str),
            Some("voyage/voyage-3")
        );
    }

    // Swarm settings: `Default` is the single source of truth — default
    // settings emit no optional swarm env vars (the always-on D28 data-dir
    // vars are the exception). The removed ABW cloud config must never be
    // re-emitted.
}
