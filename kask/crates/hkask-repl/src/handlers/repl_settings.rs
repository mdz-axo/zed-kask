//! REPL settings handler — /repl command for user-configurable inference parameters.
//!
//! Magna Carta P3 (Generative Space): all parameters are user-exposed,
//! no privileged engineer access.

use super::super::ReplState;
use hkask_types::template::LLMParameters;

/// Default condensation pressure threshold (87.5%). When context window
/// fill exceeds this fraction, auto-condensation triggers. Single source
/// of truth — referenced by `default_condense_threshold()` and tests.
pub(crate) const DEFAULT_CONDENSE_THRESHOLD: f32 = 0.875;

/// Show all REPL settings.
pub fn handle_repl_show(state: &ReplState) {
    println!("{}", render_settings(state));
}

/// Render the current REPL settings as a display string (no printing).
/// Shared by the REPL `/repl` handler and the TUI `SettingsBridge`.
pub(crate) fn render_settings(state: &ReplState) -> String {
    let s = &state.repl_settings;
    let mut out = String::new();
    out.push_str("  \x1b[1mREPL Settings\x1b[0m\n\n");
    out.push_str(&format!(
        "  \x1b[36mtool_loop_limit\x1b[0m:  {}\n",
        s.tool_loop_limit
    ));
    out.push_str(&format!(
        "  \x1b[36mcontext_turns\x1b[0m:   {} (→ aliases saliency_window)\n",
        s.condense_saliency_window
    ));
    out.push_str(&format!(
        "  \x1b[36m  pre_compress\x1b[0m:       {}\n",
        if s.pre_compress { "on" } else { "off" }
    ));
    out.push_str(&format!(
        "  \x1b[36mtemperature\x1b[0m:     {}\n",
        s.temperature
    ));
    out.push_str(&format!("  \x1b[36mtop_p\x1b[0m:           {}\n", s.top_p));
    out.push_str(&format!("  \x1b[36mtop_k\x1b[0m:           {}\n", s.top_k));
    out.push_str(&format!("  \x1b[36mmin_p\x1b[0m:          {}\n", s.min_p));
    out.push_str(&format!(
        "  \x1b[36mtypical_p\x1b[0m:       {}\n",
        s.typical_p
    ));
    out.push_str(&format!(
        "  \x1b[36mmax_tokens\x1b[0m:      {}\n",
        s.max_tokens
    ));
    out.push_str(&format!(
        "  \x1b[36mseed\x1b[0m:            {}\n",
        s.seed.map_or("random".to_string(), |v| v.to_string())
    ));
    out.push_str(&format!(
        "  \x1b[36mgas_heuristic\x1b[0m:    {}\n",
        s.gas_heuristic
    ));
    out.push_str(&format!(
        "  \x1b[36mgas_cap\x1b[0m:         {}\n",
        s.gas_cap
    ));
    out.push_str(&format!(
        "  \x1b[36mauto_condense\x1b[0m:     {}\n",
        if s.auto_condense { "on" } else { "off" }
    ));
    if s.auto_condense {
        out.push_str(&format!(
            "  \x1b[36m  pressure_threshold\x1b[0m: {:.1}%\n",
            s.condense_pressure_threshold * 100.0
        ));
        out.push_str(&format!(
            "  \x1b[36m  saliency_window\x1b[0m:     {}\n",
            s.condense_saliency_window
        ));
        out.push_str(&format!(
            "  \x1b[36m  pre_compress\x1b[0m:       {}\n",
            if s.pre_compress { "on" } else { "off" }
        ));
    }
    out.push_str(&format!(
        "  \x1b[36mshort_term_memory_life\x1b[0m: {} days\n",
        s.short_term_memory_life
    ));
    if let Some(ref meta) = s.model_meta {
        out.push_str("  \x1b[36m─ model info ─\x1b[0m\n");
        out.push_str(&format!(
            "  \x1b[36m  context_length\x1b[0m: {}\n",
            meta.context_length
        ));
        out.push_str(&format!(
            "  \x1b[36m  thinking\x1b[0m:       {}\n",
            if meta.supports_thinking { "yes" } else { "no" }
        ));
        if !meta.capabilities.is_empty() {
            out.push_str(&format!(
                "  \x1b[36m  capabilities\x1b[0m:   {}\n",
                meta.capabilities.join(", ")
            ));
        }
    } else {
        out.push_str(
            "  \x1b[36m─ model info ─\x1b[0m  (not available — provider catalog does not expose context_length; using DEFAULT_CONTEXT_WINDOW)\n",
        );
    }
    out.push_str("  \x1b[36m─ model defaults ─\x1b[0m\n");
    out.push_str(&format!(
        "  \x1b[36mdisable_thinking\x1b[0m:  {}\n",
        if s.disable_thinking { "yes" } else { "no" }
    ));
    out.push_str(&format!(
        "  \x1b[36membedding_model\x1b[0m:  {}\n",
        s.embedding_model
    ));
    out.push_str(&format!(
        "  \x1b[36mclassifier_model\x1b[0m:   {}\n",
        s.classifier_model
    ));
    out.push_str(&format!(
        "  \x1b[36mocr_model\x1b[0m:                  {}\n",
        s.ocr_model
    ));
    out.push_str(&format!(
        "  \x1b[36mocr_simple_max\x1b[0m:   {}\n",
        s.ocr_simple_max
    ));
    out.push_str(&format!(
        "  \x1b[36mocr_moderate_max\x1b[0m: {}\n",
        s.ocr_moderate_max
    ));
    out.push_str(&format!(
        "  \x1b[36mocr_sample_rate\x1b[0m:  {}\n",
        s.ocr_sample_rate
    ));
    out
}

/// Parse a /repl subcommand and apply the setting.
pub fn handle_repl_set(arg1: &str, arg2: &str, state: &mut ReplState) {
    match arg1 {
        "reset" => {
            state.repl_settings = ReplSettings::default();
            println!("  \x1b[32mAll REPL settings reset to defaults\x1b[0m");
            handle_repl_show(state);
        }
        "" | "status" | "show" => {
            handle_repl_show(state);
        }
        _ => match apply_setting(state, arg1, arg2) {
            Ok(msg) => println!("  {}", msg),
            Err(e) => println!("  \x1b[31mError:\x1b[0m {}", e),
        },
    }
}

/// Apply `key=value` to `state.repl_settings` and persist to disk.
/// Returns a rendered confirmation on success or an error on validation
/// failure. Shared by the REPL `/repl` handler and the TUI `SettingsBridge`.
pub(crate) fn apply_setting(
    state: &mut ReplState,
    key: &str,
    value: &str,
) -> anyhow::Result<String> {
    state.repl_settings.apply(key, value)?;
    if ReplSettings::is_valid_setting(key) {
        let path = settings_path();
        if let Ok(json) = serde_json::to_string_pretty(&state.repl_settings) {
            let _ = std::fs::write(&path, json);
        }
    }
    Ok(format!("{} set to {}", key, value))
}

/// Path to the persisted settings file. Delegates to the shared
/// hkask_services_core::settings_path for single-source-of-truth across surfaces.
pub fn settings_path() -> std::path::PathBuf {
    hkask_services_core::settings_path()
}

/// Default REPL settings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplSettings {
    /// Maximum tool-call loop iterations per turn.
    pub tool_loop_limit: usize,
    /// Past conversation turns to append as context (0 = no history).
    pub context_turns: usize,
    /// LLM sampling temperature.
    pub temperature: f32,
    /// Nucleus sampling threshold.
    pub top_p: f32,
    /// Top-k filter.
    pub top_k: u32,
    /// Min-p threshold.
    pub min_p: f32,
    /// Typical-p threshold (locally typical sampling).
    pub typical_p: f32,
    /// Maximum completion tokens per call.
    pub max_tokens: u32,
    /// Deterministic seed (None = random).
    pub seed: Option<u32>,
    /// Per-turn gas reservation heuristic.
    pub gas_heuristic: u64,
    /// Total session energy budget cap.
    pub gas_cap: u64,
    /// Auto-condense when context reaches 87.5% of model's window.
    /// When false, the user must condense manually.
    #[serde(alias = "auto_compact")]
    pub auto_condense: bool,
    /// Context pressure threshold (0.0–1.0) — when the context window fill
    /// exceeds this fraction, auto-condensation triggers. Default 0.875.
    /// Lower values trigger condensation sooner; higher values risk overflow.
    #[serde(default = "default_condense_threshold")]
    pub condense_pressure_threshold: f32,
    /// Number of recent messages to preserve during condensation (saliency
    /// anchor). The most recent N exchanges are kept verbatim; older messages
    /// are summarized. Default 5. Higher values preserve more context but
    /// leave less room for condensation.
    #[serde(default = "default_saliency_window")]
    pub condense_saliency_window: usize,
    /// Whether to CPU-pre-compress conversation history before LLM summarization
    /// (two-phase condensation). When true, the old half is first compressed
    /// with CondenserEngine (Profile::Heavy), then fed to the LLM summarizer.
    /// Reduces token count and inference cost. Default: true.
    #[serde(default = "default_pre_compress")]
    pub pre_compress: bool,
    /// Short-term memory lifespan in days. Chat threads inactive for longer
    /// than this are auto-archived. Default 60 days. Set to 0 to disable
    /// auto-archival (threads never expire).
    #[serde(default = "default_stm_life")]
    pub short_term_memory_life: u32,
    /// Disable thinking/reasoning tokens for all inference calls.
    /// When false (default), thinking is enabled — models that support reasoning
    /// will emit chain-of-thought tokens. Set to true for faster, cheaper responses
    /// on simple queries. Alias: `thinking` (/set thinking on → disable_thinking=false).
    #[serde(default)]
    pub disable_thinking: bool,
    /// Read-only model metadata — populated by /model switch.
    /// None until the first model detail fetch succeeds.
    pub model_meta: Option<ModelMeta>,

    // ── Model defaults (shared across all servers) ──────────────
    /// Default embedding model for vectorization.
    /// Override: `HKASK_EMBEDDING_MODEL` env var.
    #[serde(default = "default_emb_model")]
    pub embedding_model: String,

    /// Default classifier model for section type / h_mem extraction.
    /// Override: `HKASK_CLASSIFIER_MODEL` env var.
    #[serde(default = "default_cls_model")]
    pub classifier_model: String,

    /// Default OCR model for scanned PDF fallback.
    /// Override: `HKASK_OCR_MODEL` env var.
    #[serde(default = "default_ocr")]
    pub ocr_model: String,

    // ── OCR pipeline thresholds ────────────────────────────
    /// Edge-density ratio below which a page is considered Simple.
    /// Range: 0.0–1.0.
    #[serde(default = "default_ocr_simple_max")]
    pub ocr_simple_max: f32,
    /// Edge-density ratio below which a page is considered Moderate.
    /// Values ≥ this are Complex. Range: 0.0–1.0.
    #[serde(default = "default_ocr_moderate_max")]
    pub ocr_moderate_max: f32,
    /// Dual-routing sampling rate for Moderate-tier pages [0.0, 1.0].
    #[serde(default = "default_ocr_sample_rate")]
    pub ocr_sample_rate: f32,
}

/// Model metadata — populated from provider when the model changes.
/// Read-only — populated automatically when the model changes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelMeta {
    pub context_length: u32,
    pub supports_thinking: bool,
    pub capabilities: Vec<String>,
}

/// Fallback context-window size (tokens) used when `model_meta` is `None`.
///
/// Provenance: this is a conservative default, NOT a measured value. The
/// inference router's model catalog (`RouterModelEntry`) does not expose
/// `context_length`, so until a provider metadata fetch is wired (REPL spec
/// Phase 15), the system uses this named default rather than a bare magic
/// number. Real `model_meta.context_length` always takes precedence when
/// present. 128K matches the context window of the default fallback model
/// family (`DEFAULT_FALLBACK_MODEL` in `hkask-inference::model_constants`).
pub const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;

fn default_emb_model() -> String {
    hkask_inference::model_constants::embedding_model()
}
fn default_cls_model() -> String {
    hkask_inference::model_constants::classifier_model()
}
fn default_ocr() -> String {
    hkask_inference::model_constants::ocr_model()
}
fn default_ocr_simple_max() -> f32 {
    0.05
}
fn default_ocr_moderate_max() -> f32 {
    0.15
}
fn default_ocr_sample_rate() -> f32 {
    0.10
}
fn default_condense_threshold() -> f32 {
    DEFAULT_CONDENSE_THRESHOLD
}
fn default_saliency_window() -> usize {
    5
}
fn default_pre_compress() -> bool {
    true
}
fn default_stm_life() -> u32 {
    60
}

impl Default for ReplSettings {
    fn default() -> Self {
        Self {
            tool_loop_limit: 21,
            context_turns: 3,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            max_tokens: 2048,
            seed: None,
            gas_heuristic: 500,
            gas_cap: 10_000,
            auto_condense: true,
            condense_pressure_threshold: default_condense_threshold(),
            condense_saliency_window: default_saliency_window(),
            pre_compress: default_pre_compress(),
            short_term_memory_life: default_stm_life(),
            disable_thinking: false,
            model_meta: None,
            embedding_model: default_emb_model(),
            classifier_model: default_cls_model(),
            ocr_model: default_ocr(),
            ocr_simple_max: default_ocr_simple_max(),
            ocr_moderate_max: default_ocr_moderate_max(),
            ocr_sample_rate: default_ocr_sample_rate(),
        }
    }
}

impl ReplSettings {
    /// Apply a key=value pair. Returns `Ok(())` on success or `Err(msg)` on validation failure.
    /// Centralizes all validation logic — both CLI and REPL surfaces use this method.
    pub fn apply(&mut self, name: &str, value: &str) -> anyhow::Result<()> {
        match name {
            "tool_loop_limit" | "loops" => match value.parse::<usize>() {
                Ok(n) if n > 0 => self.tool_loop_limit = n,
                Ok(_) => return Err(anyhow::anyhow!("tool_loop_limit must be > 0")),
                _ => return Err(anyhow::anyhow!("expected positive integer")),
            },
            "context_turns" | "context" => {
                // Alias for condense_saliency_window — short-term memory recency.
                // Kept for backward compatibility with existing settings files.
                match value.parse::<usize>() {
                    Ok(n) if n > 0 => self.condense_saliency_window = n,
                    Ok(_) => {
                        return Err(anyhow::anyhow!(
                            "context_turns must be > 0 (now aliases saliency_window)"
                        ));
                    }
                    _ => return Err(anyhow::anyhow!("expected positive integer")),
                }
            }
            "temperature" | "temp" => match value.parse::<f32>() {
                Ok(v) if (0.0..=2.0).contains(&v) => self.temperature = v,
                Ok(_) => return Err(anyhow::anyhow!("temperature must be 0.0–2.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "top_p" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => self.top_p = v,
                Ok(_) => return Err(anyhow::anyhow!("top_p must be 0.0–1.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "top_k" => match value.parse::<u32>() {
                Ok(v) if v >= 1 => self.top_k = v,
                Ok(_) => return Err(anyhow::anyhow!("top_k must be >= 1")),
                _ => return Err(anyhow::anyhow!("expected positive integer")),
            },
            "min_p" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => self.min_p = v,
                Ok(_) => return Err(anyhow::anyhow!("min_p must be 0.0–1.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "typical_p" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => self.typical_p = v,
                Ok(_) => return Err(anyhow::anyhow!("typical_p must be 0.0–1.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "max_tokens" => match value.parse::<u32>() {
                Ok(v) if v > 0 => self.max_tokens = v,
                Ok(_) => return Err(anyhow::anyhow!("max_tokens must be > 0")),
                _ => return Err(anyhow::anyhow!("expected positive integer")),
            },
            "seed" => match value {
                "off" | "random" => self.seed = None,
                _ => match value.parse::<u32>() {
                    Ok(v) => self.seed = Some(v),
                    _ => return Err(anyhow::anyhow!("expected u32 or 'off'")),
                },
            },
            "gas_heuristic" => match value.parse::<u64>() {
                Ok(v) if v > 0 => self.gas_heuristic = v,
                Ok(_) => return Err(anyhow::anyhow!("gas_heuristic must be > 0")),
                _ => return Err(anyhow::anyhow!("expected positive integer")),
            },
            "gas_cap" => match value.parse::<u64>() {
                Ok(v) if v > 0 => self.gas_cap = v,
                Ok(_) => return Err(anyhow::anyhow!("gas_cap must be > 0")),
                _ => return Err(anyhow::anyhow!("expected positive integer")),
            },
            "auto_condense" => match value {
                "on" | "true" => self.auto_condense = true,
                "off" | "false" => self.auto_condense = false,
                _ => return Err(anyhow::anyhow!("expected 'on' or 'off'")),
            },
            "condense_pressure_threshold" | "pressure" => match value.parse::<f32>() {
                Ok(v) if (0.5..=0.99).contains(&v) => self.condense_pressure_threshold = v,
                Ok(_) => return Err(anyhow::anyhow!("pressure_threshold must be 0.5–0.99")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "condense_saliency_window" | "saliency" => match value.parse::<usize>() {
                Ok(v) if (1..=50).contains(&v) => self.condense_saliency_window = v,
                Ok(_) => return Err(anyhow::anyhow!("saliency_window must be 1–50")),
                _ => return Err(anyhow::anyhow!("expected positive integer")),
            },
            "pre_compress" | "precompress" => match value.to_lowercase().as_str() {
                "on" | "true" | "enabled" | "1" => self.pre_compress = true,
                "off" | "false" | "disabled" | "0" => self.pre_compress = false,
                _ => {
                    return Err(anyhow::anyhow!("expected on/off, true/false, or 1/0"));
                }
            },
            "short_term_memory_life" | "stm_life" => match value.parse::<u32>() {
                Ok(v) => self.short_term_memory_life = v, // 0 = never archive
                _ => return Err(anyhow::anyhow!("expected non-negative integer")),
            },
            "disable_thinking" | "thinking" => match value.to_lowercase().as_str() {
                "on" | "true" | "enabled" | "1" => self.disable_thinking = false, // thinking ON = disable_thinking false
                "off" | "false" | "disabled" | "0" => self.disable_thinking = true,
                _ => {
                    return Err(anyhow::anyhow!(
                        "disable_thinking must be: on, off, true, false"
                    ));
                }
            },
            "embedding_model" | "emb_model" => self.embedding_model = value.to_string(),
            "classifier_model" | "cls_model" => self.classifier_model = value.to_string(),
            "ocr_model" => self.ocr_model = value.to_string(),
            "ocr_simple_max" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => self.ocr_simple_max = v,
                Ok(_) => return Err(anyhow::anyhow!("ocr_simple_max must be 0.0–1.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "ocr_moderate_max" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => self.ocr_moderate_max = v,
                Ok(_) => return Err(anyhow::anyhow!("ocr_moderate_max must be 0.0–1.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            "ocr_sample_rate" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => self.ocr_sample_rate = v,
                Ok(_) => return Err(anyhow::anyhow!("ocr_sample_rate must be 0.0–1.0")),
                _ => return Err(anyhow::anyhow!("expected float")),
            },
            _ => return Err(anyhow::anyhow!("unknown setting: {}", name)),
        }
        Ok(())
    }

    /// Check whether a name refers to a recognized mutable setting.
    pub fn is_valid_setting(name: &str) -> bool {
        matches!(
            name,
            "loops"
                | "context"
                | "temp"
                | "top_p"
                | "top_k"
                | "min_p"
                | "typical_p"
                | "max_tokens"
                | "seed"
                | "gas_heuristic"
                | "gas_cap"
                | "auto_condense"
                | "disable_thinking"
                | "thinking"
                | "ocr_model"
                | "ocr_simple_max"
                | "ocr_moderate_max"
                | "ocr_sample_rate"
                | "tool_loop_limit"
                | "context_turns"
                | "temperature"
                | "embedding_model"
                | "emb_model"
                | "classifier_model"
                | "cls_model"
        )
    }
}

/// Build LLMParameters from ReplSettings for inference calls.
pub fn to_llm_params(settings: &ReplSettings) -> LLMParameters {
    LLMParameters {
        temperature: settings.temperature,
        top_p: settings.top_p,
        top_k: settings.top_k,
        min_p: settings.min_p,
        typical_p: settings.typical_p,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        max_tokens: settings.max_tokens,
        seed: settings.seed.map(|s| s as u64),
        disable_thinking: settings.disable_thinking,
        adapter: None,
        bypass_fusion: false,
        fusion_config: None,
        system_prompt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ReplSettings::default() ──────────────────────────────────────

    #[test]
    fn repl_settings_defaults_match_spec() {
        let s = ReplSettings::default();
        assert_eq!(s.tool_loop_limit, 21, "tool_loop_limit default");
        assert_eq!(s.context_turns, 3, "context_turns default");
        assert!(
            (s.temperature - 0.7).abs() < f32::EPSILON,
            "temperature default"
        );
        assert!((s.top_p - 0.9).abs() < f32::EPSILON, "top_p default");
        assert_eq!(s.top_k, 40, "top_k default");
        assert!((s.min_p - 0.0).abs() < f32::EPSILON, "min_p default");
        assert!(
            (s.typical_p - 0.0).abs() < f32::EPSILON,
            "typical_p default"
        );
        assert_eq!(s.max_tokens, 2048, "max_tokens default");
        assert_eq!(s.seed, None, "seed default (random)");
        assert_eq!(s.gas_heuristic, 500, "gas_heuristic default");
        assert_eq!(s.gas_cap, 10_000, "gas_cap default");
        assert!(s.auto_condense, "auto_condense default");
        assert!(s.model_meta.is_none(), "model_meta default (not fetched)");
    }

    // ── to_llm_params() ──────────────────────────────────────────────

    #[test]
    fn to_llm_params_maps_all_fields_correctly() {
        let s = ReplSettings {
            tool_loop_limit: 10,
            context_turns: 5,
            temperature: 0.8,
            top_p: 0.95,
            top_k: 50,
            min_p: 0.05,
            typical_p: 0.9,
            max_tokens: 1024,
            seed: Some(42),
            gas_heuristic: 100,
            gas_cap: 5_000,
            auto_condense: false,
            condense_pressure_threshold: DEFAULT_CONDENSE_THRESHOLD,
            condense_saliency_window: 5,
            pre_compress: false,
            short_term_memory_life: 60,
            disable_thinking: true,
            model_meta: None,
            embedding_model: "test-emb".into(),
            classifier_model: "test-cls".into(),
            ocr_model: "test-ocr".into(),
            ocr_simple_max: 0.05,
            ocr_moderate_max: 0.15,
            ocr_sample_rate: 0.10,
        };
        let p = to_llm_params(&s);
        assert!((p.temperature - 0.8).abs() < f32::EPSILON);
        assert!((p.top_p - 0.95).abs() < f32::EPSILON);
        assert_eq!(p.top_k, 50);
        assert!((p.min_p - 0.05).abs() < f32::EPSILON);
        assert!((p.typical_p - 0.9).abs() < f32::EPSILON);
        assert_eq!(p.max_tokens, 1024);
        assert_eq!(p.seed, Some(42));
        assert!(p.disable_thinking, "disable_thinking from settings");
        // Hardcoded in to_llm_params
        assert!((p.frequency_penalty - 0.0).abs() < f32::EPSILON);
        assert!((p.presence_penalty - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn to_llm_params_handles_none_seed() {
        let s = ReplSettings::default();
        let p = to_llm_params(&s);
        assert_eq!(p.seed, None, "None seed → None in LLMParameters");
    }

    // ── ReplSettings round-trip via settings.json ────────────────────

    #[test]
    fn repl_settings_json_round_trip_preserves_all_fields() {
        let original = ReplSettings {
            tool_loop_limit: 15,
            context_turns: 4,
            temperature: 0.5,
            top_p: 0.8,
            top_k: 30,
            min_p: 0.02,
            typical_p: 0.01,
            max_tokens: 256,
            seed: Some(12345),
            gas_heuristic: 250,
            gas_cap: 7_500,
            auto_condense: false,
            condense_pressure_threshold: 0.75,
            condense_saliency_window: 7,
            pre_compress: true,
            short_term_memory_life: 30,
            disable_thinking: false,
            model_meta: Some(ModelMeta {
                context_length: 8192,
                supports_thinking: true,
                capabilities: vec!["chat".into(), "vision".into()],
            }),
            embedding_model: "roundtrip-emb".into(),
            classifier_model: "roundtrip-cls".into(),
            ocr_model: "roundtrip-ocr".into(),
            ocr_simple_max: 0.03,
            ocr_moderate_max: 0.12,
            ocr_sample_rate: 0.20,
        };

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");

        // Write
        let json = serde_json::to_string_pretty(&original).expect("serialize");
        std::fs::write(&path, &json).expect("write");

        // Read
        let read_back: ReplSettings =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read"))
                .expect("deserialize");

        assert_eq!(read_back.tool_loop_limit, original.tool_loop_limit);
        assert_eq!(read_back.context_turns, original.context_turns);
        assert!((read_back.temperature - original.temperature).abs() < f32::EPSILON);
        assert!((read_back.top_p - original.top_p).abs() < f32::EPSILON);
        assert_eq!(read_back.top_k, original.top_k);
        assert!((read_back.min_p - original.min_p).abs() < f32::EPSILON);
        assert!((read_back.typical_p - original.typical_p).abs() < f32::EPSILON);
        assert_eq!(read_back.max_tokens, original.max_tokens);
        assert_eq!(read_back.seed, original.seed);
        assert_eq!(read_back.gas_heuristic, original.gas_heuristic);
        assert_eq!(read_back.gas_cap, original.gas_cap);
        assert_eq!(read_back.auto_condense, original.auto_condense);
        let meta = read_back.model_meta.expect("model_meta");
        assert_eq!(meta.context_length, 8192);
        assert!(meta.supports_thinking);
        assert_eq!(meta.capabilities, vec!["chat", "vision"]);
    }

    // ── handle_repl_set() invalid args ───────────────────────────────
    // These tests verify through the CLI's apply_setting function
    // (commands/settings.rs) which has identical validation logic.
    // handle_repl_set itself requires a fully-wired ReplState and is
    // tested via integration.
}
