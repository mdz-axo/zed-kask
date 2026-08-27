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
    KaskCompaniesSettings, KaskCondenserSettings, KaskCorpusSettings, KaskCuratorEmailSettings,
    KaskGeneralSettings, KaskModelsSettings, KaskPredictionMarketsSettings, KaskResearchSettings,
    KaskSwarmSettings, KaskTrainingSettings,
};

// Defaults are read from each subsection's `Default` impl so there's a
// single source of truth — changing `Default` automatically updates the
// comparison here. Do not inline magic numbers; they drift from
// `Default` (the same drift class that silently disabled all 10 kask
// MCP servers when `KaskMcpSettings::default()` disagreed with the
// serde default).

pub(crate) fn emit_data_dir_env(
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    env.insert("HKASK_DATA_DIR".to_string(), data_dir.to_string());
}

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

pub(crate) fn emit_mcp_server_ids_env(env: &mut std::collections::HashMap<String, String>) {
    // Pass the governed server id set to the swarm server so it can
    // filter cloned cards' declared `mcp_tools` to these servers (the
    // provenance boundary for third-party ABW cards). Only the swarm
    // server's `config_env` allowlist includes this var, so no other
    // child receives it.
    env.insert(
        "HKASK_MCP_SERVER_IDS".to_string(),
        crate::builtin_mcp_server_ids().join(","),
    );
}

pub(crate) fn emit_condenser_env(
    condenser: &KaskCondenserSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    let condenser_default = KaskCondenserSettings::default();
    if !condenser.persona_keywords.is_empty() {
        env.insert(
            "HKASK_CONDENSER_PERSONA_KEYWORDS".to_string(),
            condenser.persona_keywords.join(","),
        );
    }
    if condenser.saliency_window != condenser_default.saliency_window {
        env.insert(
            "HKASK_CONDENSE_SALIENCY_WINDOW".to_string(),
            condenser.saliency_window.to_string(),
        );
    }
}

pub(crate) fn emit_research_env(
    research: &KaskResearchSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !research.rss_db.is_empty() {
        env.insert("HKASK_RSS_DB".to_string(), research.rss_db.clone());
    }
}

pub(crate) fn emit_companies_env(
    companies: &KaskCompaniesSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if companies.chronic_staleness_days > 0 {
        env.insert(
            "HKASK_CHRONIC_STALENESS_DAYS".to_string(),
            companies.chronic_staleness_days.to_string(),
        );
    }
    if !companies.fermi_defaults.is_empty() {
        env.insert(
            "HKASK_FERMI_DEFAULTS".to_string(),
            companies.fermi_defaults.clone(),
        );
    }
}

/// Portfolio MCP server env emission.
///
/// No per-server path field — the transactions dir is derived from the
/// global `data_dir` as `mcp/portfolio/transactions/` per the Standardized
/// Artifact Storage layout. The server reads it via `HKASK_TRANSACTIONS_DIR`.
pub(crate) fn emit_portfolio_env(
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    let transactions_dir = std::path::Path::new(data_dir)
        .join(hkask_types::agent_paths::mcp_server_subdir(
            "portfolio",
            "transactions",
        ))
        .to_string_lossy()
        .to_string();
    env.insert("HKASK_TRANSACTIONS_DIR".to_string(), transactions_dir);
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
    if effective_embedding_model != corpus_default.embedding_model {
        env.insert(
            "HKASK_EMBEDDING_MODEL".to_string(),
            effective_embedding_model.to_string(),
        );
    }
}

pub(crate) fn emit_corpus_ocr_env(
    corpus: &KaskCorpusSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    let corpus_default = KaskCorpusSettings::default();
    if corpus.ocr_concurrency != corpus_default.ocr_concurrency {
        env.insert(
            "HKASK_OCR_CONCURRENCY".to_string(),
            corpus.ocr_concurrency.to_string(),
        );
    }
    if (corpus.ocr_simple_max - corpus_default.ocr_simple_max).abs() > f64::EPSILON {
        env.insert(
            "HKASK_OCR_SIMPLE_MAX".to_string(),
            corpus.ocr_simple_max.to_string(),
        );
    }
    if (corpus.ocr_moderate_max - corpus_default.ocr_moderate_max).abs() > f64::EPSILON {
        env.insert(
            "HKASK_OCR_MODERATE_MAX".to_string(),
            corpus.ocr_moderate_max.to_string(),
        );
    }
    if (corpus.ocr_sample_rate - corpus_default.ocr_sample_rate).abs() > f64::EPSILON {
        env.insert(
            "HKASK_OCR_SAMPLE_RATE".to_string(),
            corpus.ocr_sample_rate.to_string(),
        );
    }
    if corpus.ocr_tuneable != corpus_default.ocr_tuneable {
        env.insert("HKASK_OCR_TUNEABLE".to_string(), "false".to_string());
    }
}

pub(crate) fn emit_corpus_template_root_env(
    corpus: &KaskCorpusSettings,
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    let corpus_default = KaskCorpusSettings::default();
    // Always emit HKASK_TEMPLATE_ROOT so MCP servers (corpus, training)
    // find templates in production where the CWD-relative default does not
    // exist. When the operator hasn't overridden the default, resolve to
    // `{data_dir}/skills/registry/` — the path where `seed_templates` writes.
    // When overridden, use the operator's value.
    let template_root = if corpus.template_root != corpus_default.template_root {
        corpus.template_root.clone()
    } else {
        std::path::Path::new(data_dir)
            .join("skills")
            .join("registry")
            .to_string_lossy()
            .to_string()
    };
    env.insert("HKASK_TEMPLATE_ROOT".to_string(), template_root);
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
    if !prediction_markets.base_events.is_empty() {
        env.insert(
            "HKASK_PREDICTION_MARKETS_BASE_EVENTS".to_string(),
            prediction_markets.base_events.clone(),
        );
    }
}

pub(crate) fn emit_swarm_env(
    data_dir: &str,
    swarm: &KaskSwarmSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // ── Swarm (ABW + Local) ──
    // The API key is a credential (injected by `build_mcp_server_env`
    // from the keychain), not config — only non-secret fields are here.
    let swarm_default = KaskSwarmSettings::default();
    if swarm.mode != swarm_default.mode {
        env.insert("HKASK_SWARM_MODE".to_string(), swarm.mode.to_string());
    }
    if !swarm.api_url.is_empty() {
        env.insert("HKASK_ABW_API_URL".to_string(), swarm.api_url.clone());
    }
    if swarm.max_credits_per_dispatch != swarm_default.max_credits_per_dispatch {
        env.insert(
            "HKASK_ABW_MAX_CREDITS".to_string(),
            swarm.max_credits_per_dispatch.to_string(),
        );
    }
    if swarm.curator_consent_default != swarm_default.curator_consent_default {
        env.insert(
            "HKASK_ABW_CURATOR_CONSENT_DEFAULT".to_string(),
            swarm.curator_consent_default.to_string(),
        );
    }
    // D28 — Standardized Artifact Storage. The swarm server's local
    // registries and memory DB are derived from the global `data_dir` under
    // `mcp/swarm/`. No per-server override — one global data root, each
    // server owns its subfolder.
    let swarm_root = std::path::Path::new(data_dir)
        .join(hkask_types::agent_paths::mcp_server_subdir("swarm", ""));
    let local_agents_dir = swarm_root
        .join("agents")
        .join("curated")
        .to_string_lossy()
        .to_string();
    env.insert("HKASK_LOCAL_AGENTS_DIR".to_string(), local_agents_dir);
    let local_swarms_dir = swarm_root.join("swarms").to_string_lossy().to_string();
    env.insert("HKASK_LOCAL_SWARMS_DIR".to_string(), local_swarms_dir);
    if !swarm.skills_dir.is_empty() {
        env.insert("HKASK_SKILLS_DIR".to_string(), swarm.skills_dir.clone());
    }
    if !swarm.default_agent_model.is_empty() {
        env.insert(
            "HKASK_ABW_DEFAULT_AGENT_MODEL".to_string(),
            swarm.default_agent_model.clone(),
        );
    }
    if swarm.a2a_http_enabled != swarm_default.a2a_http_enabled {
        env.insert(
            "HKASK_A2A_HTTP_ENABLE".to_string(),
            swarm.a2a_http_enabled.to_string(),
        );
    }
    if swarm.memory_passphrase != swarm_default.memory_passphrase {
        env.insert(
            "HKASK_SWARM_MEMORY_PASSPHRASE".to_string(),
            swarm.memory_passphrase.clone(),
        );
    }
    let memory_db_path = swarm_root.join("memory.db").to_string_lossy().to_string();
    env.insert("HKASK_SWARM_MEMORY_DB".to_string(), memory_db_path);
    if swarm.embedding_dim != swarm_default.embedding_dim {
        env.insert(
            "HKASK_SWARM_EMBEDDING_DIM".to_string(),
            swarm.embedding_dim.to_string(),
        );
    }
}

pub(crate) fn emit_training_env(
    training: &KaskTrainingSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !training.host.is_empty() {
        env.insert("HKASK_TRAINING_HOST".to_string(), training.host.clone());
    }
    if !training.cache_dir.is_empty() {
        env.insert(
            "HKASK_TRAINING_CACHE_DIR".to_string(),
            training.cache_dir.clone(),
        );
    }
}

/// Pass through operator shell overrides for server knobs that have no
/// `KaskSettings` field. These are read by the servers via `std::env::var`
/// with in-code defaults, but under governed launch the child environment is
/// cleared (`cmd.env_clear()`), so a shell-set value never reaches the
/// process unless `mcp_env()` carries it and the server's `config_env`
/// allowlist admits it. Without this passthrough, the allowlist entries for
/// these vars advertise a delivery path that nothing sources (RR-0061's
/// "allowlist entry naming a credential that nothing ever sources", in
/// config form). Only non-empty parent values are forwarded — an empty shell
/// var is not a meaningful override.
const OPERATOR_OVERRIDE_ENV_VARS: &[&str] = &[
    // swarm — event-store path + retention knobs (hkask_mcp_swarm.rs,
    // local_tools.rs). Also read by kata-kanban's spawn path so both
    // processes share one ledger.
    "HKASK_SWARM_LEDGER_PATH",
    "HKASK_SWARM_EVENTS_PATH",
    "HKASK_SWARM_BODY_RETENTION_HOURS",
    "HKASK_SWARM_ROLLOUT_RETENTION_DAYS",
    // corpus — embedding batch concurrency (tools/semantic.rs)
    "HKASK_EMBED_CONCURRENCY",
];

pub(crate) fn emit_operator_override_env(env: &mut std::collections::HashMap<String, String>) {
    for name in OPERATOR_OVERRIDE_ENV_VARS {
        if let Ok(value) = std::env::var(name)
            && !value.trim().is_empty()
        {
            env.insert(name.to_string(), value);
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
    if !models.classifier_model.is_empty() {
        env.insert(
            "HKASK_CLASSIFIER_MODEL".to_string(),
            models.classifier_model.clone(),
        );
    }
    if !models.ocr_model.is_empty() {
        env.insert("HKASK_OCR_MODEL".to_string(), models.ocr_model.clone());
    }
}

pub(crate) fn emit_curator_email_env(
    email: &KaskCuratorEmailSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // ── Curator email (non-secret) ──
    // The SMTP password is injected separately by `build_mcp_server_env`
    // from the keychain entry `kask://credentials/hkask_smtp_password`.
    if !email.mxroute_server.is_empty() {
        env.insert(
            "HKASK_MXROUTE_SERVER".to_string(),
            email.mxroute_server.clone(),
        );
    }
    if !email.smtp_username.is_empty() {
        env.insert(
            "HKASK_SMTP_USERNAME".to_string(),
            email.smtp_username.clone(),
        );
        // `HKASK_CURATOR_EMAIL` defaults to `HKASK_SMTP_USERNAME` in the
        // email crate; only inject when explicitly set.
        if !email.curator_email.is_empty() {
            env.insert(
                "HKASK_CURATOR_EMAIL".to_string(),
                email.curator_email.clone(),
            );
        }
        // `HKASK_ALERT_EMAIL` defaults to `HKASK_SMTP_USERNAME` in the
        // email crate; only inject when explicitly set.
        if !email.alert_email.is_empty() {
            env.insert("HKASK_ALERT_EMAIL".to_string(), email.alert_email.clone());
        }
    }
    if !email.authorized_emails.is_empty() {
        env.insert(
            "HKASK_AUTHORIZED_EMAILS".to_string(),
            email.authorized_emails.join(","),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::{KaskSettings, SwarmModeConfig};

    // `mcp_env()` must not emit env vars for settings that match `Default`.
    // Previously `mcp_env()` compared against inlined magic numbers (1024, 4,
    // 0.05, 0.15, 0.10, "registry", 5) that duplicated `Default` values. If
    // `Default` changed, the comparison would drift and emit env vars for the
    // default case. Now `mcp_env()` reads from `Default::default()`, so changing
    // `Default` automatically updates the comparison. This test pins that: a
    // `KaskSettings::default()` (all defaults) does not emit per-server config
    // vars. The always-emitted vars (`HKASK_DATA_DIR`, `HKASK_TEMPLATE_ROOT`,
    // `HKASK_TRANSACTIONS_DIR`, `HKASK_SCENARIOS_DATA`,
    // `HKASK_PREDICTION_MARKETS_DATA`) are kask-wide critical paths resolved
    // from `data_dir` per the Standardized Artifact Storage layout — they are
    // NOT suppressed for default settings. See the `mcp_env_always_emits_*`
    // tests for those.
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
        assert!(
            !env.contains_key("HKASK_OCR_CONCURRENCY"),
            "default ocr_concurrency must not be emitted"
        );
        assert!(
            !env.contains_key("HKASK_OCR_SIMPLE_MAX"),
            "default ocr_simple_max must not be emitted"
        );
        // HKASK_TEMPLATE_ROOT is now always emitted (resolved from data_dir),
        // so it's no longer suppressed for default settings — see
        // `mcp_env_always_emits_template_root`.
        assert!(
            !env.contains_key("HKASK_EMBEDDING_MODEL"),
            "default embedding_model must not be emitted — the `is_empty()` check was a drift bug; the default is non-empty"
        );
        assert!(
            !env.contains_key("HKASK_CONDENSE_SALIENCY_WINDOW"),
            "default saliency_window must not be emitted"
        );
        // The per-server data-dir env vars are now always emitted (resolved
        // from `data_dir` per the Standardized Artifact Storage layout), so
        // they are NOT suppressed for default settings. See
        // `mcp_env_always_emits_per_server_data_dirs`.
        assert!(
            env.contains_key("HKASK_TRANSACTIONS_DIR"),
            "HKASK_TRANSACTIONS_DIR must always be emitted — the portfolio server auto-loads from it"
        );
        assert!(
            env.contains_key("HKASK_SCENARIOS_DATA"),
            "HKASK_SCENARIOS_DATA must always be emitted — the scenarios server resolves its persistence dir from it"
        );
        assert!(
            env.contains_key("HKASK_PREDICTION_MARKETS_DATA"),
            "HKASK_PREDICTION_MARKETS_DATA must always be emitted — the prediction-markets server resolves its calibration journal from it"
        );
    }

    // The per-server data-dir env vars (`HKASK_TRANSACTIONS_DIR`,
    // `HKASK_SCENARIOS_DATA`, `HKASK_PREDICTION_MARKETS_DATA`) must ALWAYS be
    // emitted by `mcp_env()`, resolved from `data_dir` per the Standardized
    // Artifact Storage layout. Without this, an operator `data_dir` override
    // is silently dropped for those servers — the server falls back to
    // `resolve_under_data_dir` which reads `HKASK_DATA_DIR` from its own env,
    // but the per-server env var is the canonical delivery path and the
    // server's fallback only works when `HKASK_DATA_DIR` is also allowlisted
    // (which it now is for portfolio/scenarios/prediction-markets/companies).
    #[test]
    fn mcp_env_always_emits_per_server_data_dirs() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        let data_dir = env
            .get("HKASK_DATA_DIR")
            .expect("HKASK_DATA_DIR must be emitted");
        // Transactions dir resolves under the portfolio server's subtree.
        let expected_transactions = std::path::Path::new(data_dir)
            .join("mcp")
            .join("portfolio")
            .join("transactions")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            env.get("HKASK_TRANSACTIONS_DIR").map(String::as_str),
            Some(expected_transactions.as_str()),
            "default transactions_dir must resolve to `{{data_dir}}/mcp/portfolio/transactions`"
        );
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
        // Swarm local agents dir resolves under the swarm server's subtree.
        let expected_swarm_agents = std::path::Path::new(data_dir)
            .join("mcp")
            .join("swarm")
            .join("agents")
            .join("curated")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            env.get("HKASK_LOCAL_AGENTS_DIR").map(String::as_str),
            Some(expected_swarm_agents.as_str()),
            "default local_agents_dir must resolve to `{{data_dir}}/mcp/swarm/agents/curated`"
        );
        // Swarm local swarms dir resolves under the swarm server's subtree.
        let expected_swarm_swarms = std::path::Path::new(data_dir)
            .join("mcp")
            .join("swarm")
            .join("swarms")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            env.get("HKASK_LOCAL_SWARMS_DIR").map(String::as_str),
            Some(expected_swarm_swarms.as_str()),
            "default local_swarms_dir must resolve to `{{data_dir}}/mcp/swarm/swarms`"
        );
        // Swarm memory DB resolves under the swarm server's subtree.
        let expected_swarm_mem = std::path::Path::new(data_dir)
            .join("mcp")
            .join("swarm")
            .join("memory.db")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            env.get("HKASK_SWARM_MEMORY_DB").map(String::as_str),
            Some(expected_swarm_mem.as_str()),
            "default swarm memory_db_path must resolve to `{{data_dir}}/mcp/swarm/memory.db`"
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
        let dir = env.get("HKASK_DATA_DIR").unwrap();
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

    // `HKASK_TEMPLATE_ROOT` must ALWAYS be emitted so MCP servers (corpus,
    // training) find templates in production where the CWD-relative default
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

    // The `embedding_model` field has a non-empty `Default`
    // (`DEFAULT_EMBEDDING_MODEL`), so the comparison must be
    // against `Default`, not `is_empty()`. A user override must be emitted;
    // the default must not.
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
    // settings emit no swarm env vars; a non-default credit ceiling emits
    // `HKASK_ABW_MAX_CREDITS`; the API key is never in `mcp_env()` (it is a
    // keychain credential, injected by `build_mcp_server_env`).

    #[test]
    fn swarm_settings_default_emits_no_env() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        assert!(!env.contains_key("HKASK_ABW_API_URL"));
        assert!(!env.contains_key("HKASK_ABW_MAX_CREDITS"));
        assert!(!env.contains_key("HKASK_ABW_CURATOR_CONSENT_DEFAULT"));
        assert!(!env.contains_key("HKASK_SWARM_MODE"));
        assert!(!env.contains_key("HKASK_SKILLS_DIR"));
        assert!(!env.contains_key("HKASK_ABW_DEFAULT_AGENT_MODEL"));
        assert!(!env.contains_key("HKASK_A2A_HTTP_ENABLE"));
        assert!(!env.contains_key("HKASK_SWARM_MEMORY_PASSPHRASE"));
        assert!(!env.contains_key("HKASK_SWARM_EMBEDDING_DIM"));
        assert!(
            !env.contains_key("HKASK_ABW_API_KEY"),
            "the ABW API key is a credential, not config — it must never appear in mcp_env()"
        );
        // D28 — HKASK_LOCAL_AGENTS_DIR, HKASK_LOCAL_SWARMS_DIR, and
        // HKASK_SWARM_MEMORY_DB are now always emitted (resolved from
        // `data_dir` per the Standardized Artifact Storage layout), so they
        // are NOT suppressed for default settings. See
        // `mcp_env_always_emits_per_server_data_dirs`.
        assert!(
            env.contains_key("HKASK_LOCAL_AGENTS_DIR"),
            "HKASK_LOCAL_AGENTS_DIR must always be emitted — the swarm server resolves its local agent registry from it"
        );
        assert!(
            env.contains_key("HKASK_LOCAL_SWARMS_DIR"),
            "HKASK_LOCAL_SWARMS_DIR must always be emitted — the swarm server resolves its local swarms registry from it"
        );
        assert!(
            env.contains_key("HKASK_SWARM_MEMORY_DB"),
            "HKASK_SWARM_MEMORY_DB must always be emitted — the swarm server resolves its semantic-memory DB from it"
        );
        assert_eq!(settings.swarm.max_credits_per_dispatch, 50);
        assert!(!settings.swarm.curator_consent_default);
        assert_eq!(settings.swarm.mode, SwarmModeConfig::Abw);
        assert!(!settings.swarm.a2a_http_enabled);
        assert_eq!(settings.swarm.embedding_dim, 1024);
    }

    #[test]
    fn swarm_settings_non_default_emits_env() {
        let mut settings = KaskSettings::default();
        settings.data_dir = "/custom/kask/data".to_string();
        settings.swarm.mode = SwarmModeConfig::Local;
        settings.swarm.max_credits_per_dispatch = 100;
        settings.swarm.api_url = "https://staging.agent-bestiary.world".to_string();
        settings.swarm.curator_consent_default = true;
        settings.swarm.skills_dir = "/custom/skills/dir".to_string();
        settings.swarm.default_agent_model = "claude-sonnet-4-6".to_string();
        settings.swarm.a2a_http_enabled = true;
        settings.swarm.memory_passphrase = "real-secret".to_string();
        settings.swarm.embedding_dim = 2048;
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_SWARM_MODE").map(String::as_str),
            Some("local")
        );
        assert_eq!(
            env.get("HKASK_ABW_MAX_CREDITS").map(String::as_str),
            Some("100")
        );
        assert_eq!(
            env.get("HKASK_ABW_API_URL").map(String::as_str),
            Some("https://staging.agent-bestiary.world")
        );
        assert_eq!(
            env.get("HKASK_ABW_CURATOR_CONSENT_DEFAULT")
                .map(String::as_str),
            Some("true")
        );
        // Local agents/swarms/memory paths are derived from the global
        // `data_dir` — no per-server override. The custom data_dir propagates.
        assert_eq!(
            env.get("HKASK_LOCAL_AGENTS_DIR").map(String::as_str),
            Some("/custom/kask/data/mcp/swarm/agents/curated")
        );
        assert_eq!(
            env.get("HKASK_LOCAL_SWARMS_DIR").map(String::as_str),
            Some("/custom/kask/data/mcp/swarm/swarms")
        );
        assert_eq!(
            env.get("HKASK_SKILLS_DIR").map(String::as_str),
            Some("/custom/skills/dir")
        );
        assert_eq!(
            env.get("HKASK_ABW_DEFAULT_AGENT_MODEL").map(String::as_str),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(
            env.get("HKASK_A2A_HTTP_ENABLE").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            env.get("HKASK_SWARM_MEMORY_PASSPHRASE").map(String::as_str),
            Some("real-secret")
        );
        assert_eq!(
            env.get("HKASK_SWARM_MEMORY_DB").map(String::as_str),
            Some("/custom/kask/data/mcp/swarm/memory.db")
        );
        assert_eq!(
            env.get("HKASK_SWARM_EMBEDDING_DIM").map(String::as_str),
            Some("2048")
        );
    }
}
