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
    KaskModelsSettings, KaskNostrRelaySettings, KaskNostrSettings, KaskPredictionMarketsSettings,
    KaskResearchSettings,
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
    // Always emit HKASK_TEMPLATE_ROOT so MCP servers (corpus)
    // find templates in production where the CWD-relative default
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

/// The local Nostr relay's policy, from `kask.nostr.relay`.
///
/// All three keys are emitted for every settings state, empty or zero
/// included. That makes settings the only source: a value left in the
/// editor's own environment cannot fill in for one the operator cleared,
/// because `emit_operator_override_env` skips settings-backed keys. The
/// relay server reads an empty value as "not set".
pub(crate) fn emit_nostr_relay_env(
    relay: &KaskNostrRelaySettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    env.insert("HKASK_NOSTR_RELAY_PORT".to_string(), relay.port.to_string());
    env.insert(
        "HKASK_NOSTR_RELAY_AUTHORS".to_string(),
        relay.allowed_authors.join(","),
    );
    env.insert(
        "HKASK_NOSTR_RELAY_SYNC_RELAYS".to_string(),
        relay.sync_relays.join(","),
    );
}

/// The Nostr client server's public identity and seed relays, from
/// `kask.nostr.npub` and `kask.nostr.seed_relays`.
///
/// Both keys are emitted for every settings state, empty included, for the
/// same reason as the relay's: settings are the only source, so a value left
/// in the editor's own environment cannot stand in for one the operator
/// cleared. Public data only; no key is ever emitted here.
pub(crate) fn emit_nostr_client_env(
    nostr: &KaskNostrSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    env.insert("HKASK_NOSTR_CLIENT_NPUB".to_string(), nostr.npub.clone());
    env.insert(
        "HKASK_NOSTR_CLIENT_SEED_RELAYS".to_string(),
        nostr.seed_relays.join(","),
    );
}

pub(crate) fn emit_media_env(
    media: &KaskMediaSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !media.stt_model.is_empty() {
        env.insert("HKASK_MEDIA_STT_MODEL".to_string(), media.stt_model.clone());
    }
    if !media.cross_check_stt_models.is_empty() {
        env.insert(
            "HKASK_MEDIA_CROSS_CHECK_STT_MODELS".to_string(),
            media.cross_check_stt_models.join(","),
        );
    }
    if !media.vision_model.is_empty() {
        env.insert(
            "HKASK_MEDIA_VISION_MODEL".to_string(),
            media.vision_model.clone(),
        );
    }
    if !media.sound_annotation_model.is_empty() {
        env.insert(
            "HKASK_MEDIA_SOUND_ANNOTATION_MODEL".to_string(),
            media.sound_annotation_model.clone(),
        );
    }
    if !media.tagging_model.is_empty() {
        env.insert(
            "HKASK_MEDIA_TAGGING_MODEL".to_string(),
            media.tagging_model.clone(),
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
    "HKASK_NOSTR_RELAY_PORT",
    "HKASK_NOSTR_RELAY_AUTHORS",
    "HKASK_NOSTR_RELAY_SYNC_RELAYS",
    "HKASK_NOSTR_CLIENT_NPUB",
    "HKASK_NOSTR_CLIENT_SEED_RELAYS",
    "HKASK_MEDIA_STT_MODEL",
    "HKASK_MEDIA_VISION_MODEL",
    "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
    "HKASK_MEDIA_TAGGING_MODEL",
    "HKASK_DEFAULT_MODEL",
    "HKASK_CLASSIFIER_MODEL",
    hkask_inference::model_constants::QA_GENERATION_MODEL_ENV,
    hkask_inference::model_constants::PRIVATE_EMBEDDING_MODEL_ENV,
    hkask_inference::model_constants::PRIVATE_RERANK_MODEL_ENV,
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
    if !models.decision_model.is_empty() {
        env.insert(
            "HKASK_DECISION_MODEL".to_string(),
            models.decision_model.clone(),
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
        let default_fleet =
            hkask_inference::model_constants::DEFAULT_MEDIA_CROSS_CHECK_STT_MODELS.join(",");
        assert_eq!(
            env.get("HKASK_MEDIA_CROSS_CHECK_STT_MODELS")
                .map(String::as_str),
            Some(default_fleet.as_str()),
            "default settings emit the cross-check STT fleet"
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
        // The media role models (settings > kask > models, operator ruling
        // 2026-10-30) are emitted for the same reason: the audio pass and the
        // grounded tagging pass resolve them with no other fallback.
        assert!(
            env.contains_key("HKASK_MEDIA_SOUND_ANNOTATION_MODEL"),
            "default sound_annotation_model must be emitted — the audio pass has no fallback"
        );
        assert_eq!(
            env.get("HKASK_MEDIA_SOUND_ANNOTATION_MODEL")
                .map(String::as_str),
            Some(hkask_inference::model_constants::DEFAULT_MEDIA_SOUND_ANNOTATION_MODEL),
            "default settings emit the code-default sound-annotation model"
        );
        assert!(
            env.contains_key("HKASK_MEDIA_TAGGING_MODEL"),
            "default tagging_model must be emitted — the grounded pass has no fallback"
        );
        assert_eq!(
            env.get("HKASK_MEDIA_TAGGING_MODEL").map(String::as_str),
            Some(hkask_inference::model_constants::DEFAULT_MEDIA_TAGGING_MODEL),
            "default settings emit the code-default tagging model"
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

    /// zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06) — the
    /// private leg's LOCAL rerank model reaches the research child through
    /// `HKASK_PRIVATE_RERANK_MODEL`, carrying the operator's explicit
    /// opt-out (an EMPTY value is emitted as empty — the child's reader
    /// maps empty/unset to the surfaced heuristic skip, never the
    /// configured cloud reranker).
    #[test]
    fn mcp_env_emits_the_private_rerank_model() {
        let mut settings = KaskSettings::default();
        settings.private_inference.rerank_model = "ollama/qwen3-reranker-4b:latest".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_PRIVATE_RERANK_MODEL").map(String::as_str),
            Some("ollama/qwen3-reranker-4b:latest")
        );

        // The explicit opt-out: an empty setting emits empty — the child
        // reads the surfaced skip, never a hidden default.
        settings.private_inference.rerank_model = String::new();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_PRIVATE_RERANK_MODEL").map(String::as_str),
            Some("")
        );
    }

    /// zed-kask: D100's named follow-up (operator-approved 2026-11-06) — the
    /// private leg's LOCAL embedding model reaches the kask MCP children
    /// through `HKASK_PRIVATE_EMBEDDING_MODEL`, carrying the operator's
    /// explicit opt-out (an EMPTY value is emitted as empty — the child's
    /// reader maps empty/unset to the surfaced degrade, never a cloud
    /// fallback). The per-server admission is the curator server's
    /// `config_env` allowlist entry.
    #[test]
    fn mcp_env_emits_the_private_embedding_model() {
        let mut settings = KaskSettings::default();
        settings.private_inference.embedding_model = "ollama/qwen3-embedding:0.6b".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_PRIVATE_EMBEDDING_MODEL").map(String::as_str),
            Some("ollama/qwen3-embedding:0.6b")
        );

        // The explicit opt-out: an empty setting emits empty — the child
        // reads the degrade, never a hidden default.
        settings.private_inference.embedding_model = String::new();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_PRIVATE_EMBEDDING_MODEL").map(String::as_str),
            Some("")
        );
    }

    const NOSTR_RELAY_KEYS: [&str; 3] = [
        "HKASK_NOSTR_RELAY_PORT",
        "HKASK_NOSTR_RELAY_AUTHORS",
        "HKASK_NOSTR_RELAY_SYNC_RELAYS",
    ];

    /// `kask.nostr.relay` is the relay's policy, and settings are its only
    /// source. Default settings therefore still emit all three keys (port 0,
    /// two empty lists): a key left out here would be filled from the
    /// editor's own environment by `emit_operator_override_env`, and the
    /// operator clearing the allowlist in settings would not clear it.
    #[test]
    fn mcp_env_emits_the_nostr_relay_policy_even_when_unset() {
        let env = KaskSettings::default().mcp_env();
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_PORT").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_AUTHORS").map(String::as_str),
            Some("")
        );
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_SYNC_RELAYS").map(String::as_str),
            Some("")
        );
        for key in NOSTR_RELAY_KEYS {
            assert!(
                super::SETTINGS_BACKED_CONFIG_ENV_VARS.contains(&key),
                "{key} must be settings-backed, so the editor's environment cannot supply it"
            );
        }
    }

    /// What the operator writes in `kask.nostr.relay` is what the relay
    /// server receives, joined with commas (the server splits on commas and
    /// whitespace), and only the relay server receives it.
    #[test]
    fn mcp_env_delivers_the_nostr_relay_policy_only_to_the_relay_server() {
        let mut settings = KaskSettings::default();
        settings.nostr.relay.port = 7447;
        settings.nostr.relay.allowed_authors = vec!["npub1aaa".to_string(), "abcd".to_string()];
        settings.nostr.relay.sync_relays = vec![
            "wss://nos.lol".to_string(),
            "wss://relay.example".to_string(),
        ];
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_PORT").map(String::as_str),
            Some("7447")
        );
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_AUTHORS").map(String::as_str),
            Some("npub1aaa,abcd")
        );
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_SYNC_RELAYS").map(String::as_str),
            Some("wss://nos.lol,wss://relay.example")
        );

        for server in crate::BUILT_IN_MCP_SERVERS {
            let filtered = crate::mcp_servers::filter_config_env_for_server(server.id, &env);
            for key in NOSTR_RELAY_KEYS {
                assert_eq!(
                    filtered.contains_key(key),
                    server.id == "nostr-relay",
                    "{key} for server {}",
                    server.id
                );
            }
        }
    }

    /// Settings content becomes policy with entries trimmed and empties and
    /// repeats dropped. Nothing is validated here: the relay server decides
    /// what is usable and reports what it refused.
    #[test]
    fn nostr_relay_settings_content_is_cleaned_not_validated() {
        use settings_content::{KaskNostrRelaySettingsContent, KaskNostrSettingsContent};
        let content = KaskNostrSettingsContent {
            relay: Some(KaskNostrRelaySettingsContent {
                port: Some(7447),
                allowed_authors: Some(vec![
                    " npub1aaa ".to_string(),
                    String::new(),
                    "npub1aaa".to_string(),
                    "not-a-key".to_string(),
                ]),
                sync_relays: None,
            }),
            ..Default::default()
        };
        let nostr: crate::settings::KaskNostrSettings = content.into();
        assert_eq!(nostr.relay.port, 7447);
        assert_eq!(nostr.relay.allowed_authors, ["npub1aaa", "not-a-key"]);
        assert!(nostr.relay.sync_relays.is_empty());

        let absent: crate::settings::KaskNostrSettings = KaskNostrSettingsContent::default().into();
        assert_eq!(absent, crate::settings::KaskNostrSettings::default());
    }

    /// The key names an operator writes under `"kask"` in settings.json are
    /// the contract. This pins them end to end: JSON to the three variables.
    #[test]
    fn nostr_relay_settings_json_reaches_the_relay_env() {
        let content: settings_content::KaskSettingsContent = serde_json::from_str(
            r#"{ "nostr": { "relay": {
                "port": 7447,
                "allowed_authors": ["npub1aaa"],
                "sync_relays": ["wss://nos.lol"]
            } } }"#,
        )
        .expect("kask.nostr.relay parses");
        let settings: KaskSettings = content.into();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_PORT").map(String::as_str),
            Some("7447")
        );
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_AUTHORS").map(String::as_str),
            Some("npub1aaa")
        );
        assert_eq!(
            env.get("HKASK_NOSTR_RELAY_SYNC_RELAYS").map(String::as_str),
            Some("wss://nos.lol")
        );
    }

    const NOSTR_CLIENT_KEYS: [&str; 2] =
        ["HKASK_NOSTR_CLIENT_NPUB", "HKASK_NOSTR_CLIENT_SEED_RELAYS"];

    /// What an operator might type. Deliberately not a key: settings pass the
    /// text through as written, and the client server decides whether it is a
    /// usable `npub`.
    const PLACEHOLDER_NPUB: &str = "npub-as-typed-in-settings";

    fn nostr_identity_settings() -> KaskSettings {
        let content: settings_content::KaskSettingsContent =
            serde_json::from_value(serde_json::json!({
                "nostr": {
                    "npub": format!("  {PLACEHOLDER_NPUB} "),
                    "seed_relays": [" wss://relay.example.com", "", "wss://other.example.com",
                                    "wss://relay.example.com"]
                }
            }))
            .expect("kask.nostr parses");
        content.into()
    }

    /// The key names an operator writes under `"kask": { "nostr": … }` are
    /// the contract: JSON to the two variables the client server reads. The
    /// npub is trimmed; the relays are trimmed, de-duplicated and joined
    /// with commas. Nothing is validated here: the server reports a value it
    /// cannot use.
    #[test]
    fn nostr_settings_json_reaches_the_client_env() {
        let env = nostr_identity_settings().mcp_env();
        assert_eq!(
            env.get("HKASK_NOSTR_CLIENT_NPUB").map(String::as_str),
            Some(PLACEHOLDER_NPUB)
        );
        assert_eq!(
            env.get("HKASK_NOSTR_CLIENT_SEED_RELAYS")
                .map(String::as_str),
            Some("wss://relay.example.com,wss://other.example.com")
        );
    }

    /// Only the Nostr client server's config allowlist admits the two
    /// names; every other server, the relay included, is filtered clear.
    #[test]
    fn nostr_settings_identity_reaches_only_the_client_server() {
        let env = nostr_identity_settings().mcp_env();
        for key in NOSTR_CLIENT_KEYS {
            assert!(env.contains_key(key), "the control: {key} is emitted");
        }
        for server in crate::BUILT_IN_MCP_SERVERS {
            let filtered = crate::mcp_servers::filter_config_env_for_server(server.id, &env);
            for key in NOSTR_CLIENT_KEYS {
                assert_eq!(
                    filtered.contains_key(key),
                    server.id == "nostr-client",
                    "{key} for server {}",
                    server.id
                );
            }
        }
    }

    /// expect: autopublish (operator ruling 2026-11-13: "unselected by
    /// default") resolves to off when the section does not mention it, and
    /// to on only when written `true`.
    /// pre: a `kask.nostr` section without the key, and one with it on.
    /// post: `false`, then `true`.
    #[test]
    fn nostr_autopublish_resolves_off_unless_written_on() {
        assert!(!KaskSettings::default().nostr.autopublish);
        assert!(!nostr_identity_settings().nostr.autopublish);
        let content: settings_content::KaskSettingsContent =
            serde_json::from_value(serde_json::json!({ "nostr": { "autopublish": true } }))
                .expect("kask.nostr parses");
        let settings: KaskSettings = content.into();
        assert!(settings.nostr.autopublish);
    }

    /// expect: autopublish is enforced where the signature is made, in the
    /// editor, so it is delivered to no MCP server. A copy in the client
    /// server's environment would be a second place to read the switch, and
    /// a check there would not be an approval.
    /// pre: autopublish on.
    /// post: no emitted variable carries it, in any server's filtered env.
    #[test]
    fn nostr_autopublish_reaches_no_mcp_server() {
        let content: settings_content::KaskSettingsContent =
            serde_json::from_value(serde_json::json!({ "nostr": { "autopublish": true } }))
                .expect("kask.nostr parses");
        let settings: KaskSettings = content.into();
        let env = settings.mcp_env();
        for (key, value) in &env {
            assert!(
                !key.to_ascii_lowercase().contains("autopublish"),
                "{key} names the switch"
            );
            if key.starts_with("HKASK_NOSTR_") {
                assert!(!value.contains("autopublish"), "{key} carries the switch");
            }
        }
    }

    /// Settings are the only source of the identity. Default settings still
    /// emit both keys, empty: a key left out would be filled from the
    /// editor's own environment by `emit_operator_override_env`, and the
    /// operator clearing the npub in settings would not clear it.
    #[test]
    fn nostr_settings_identity_is_emitted_even_when_unset() {
        let env = KaskSettings::default().mcp_env();
        for key in NOSTR_CLIENT_KEYS {
            assert_eq!(env.get(key).map(String::as_str), Some(""), "{key}");
            assert!(
                super::SETTINGS_BACKED_CONFIG_ENV_VARS.contains(&key),
                "{key} must be settings-backed, so the editor's environment cannot supply it"
            );
        }
    }
}
