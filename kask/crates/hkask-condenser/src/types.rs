//! hKask Condenser — Domain types
//!
//! Pure domain types with no MCP dependencies. Error types use `String`
//! for `FromStr` impls; MCP servers wrap these at the boundary.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ═══════════════════════════════════════════════════════════════════════════
// Ontology Anchoring (P5.2 / P5.4 / P8.1)
// ═══════════════════════════════════════════════════════════════════════════

/// Domain ontology tier for content produced by an MCP tool.
///
/// Every piece of content in hKask exists within the 3-tier ontology
/// architecture. The condenser uses this to apply domain-aware saliency
/// weighting — different ontologies carry different confidence baselines
/// and information density expectations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum OntologyAnchor {
    /// Universal 5W1H core — no domain supplement (P5.2 default ground).
    /// Content anchored only to Who/What/When/Where/Why/How.
    #[serde(rename = "core")]
    #[default]
    Core,
    /// Process axis (PKO) or state axis (DC+BIBO) — dual-axis framework (P5.4).
    /// `concept` is the canonical concept URI, e.g. "pko:StepExecution" or "bibo:Article".
    DualAxis { axis: OntologyAxis, concept: String },
    /// Domain supplement — FIBO, GOLEM, CogAT, ML-Schema, or OMC (P8.1).
    /// Layered on top of the dual-axis core for domain-specific precision.
    DomainSupplement {
        namespace: OntologyNamespace,
        concept: String,
    },
}

impl OntologyAnchor {
    /// Return the confidence modifier for this ontology tier (per pragmatic-semantics §Domain Ontology Anchoring).
    /// FIBO: +0.10 (OMG standard, high adoption)
    /// CogAT: -0.10 (metaphorical mapping)
    /// Others: ±0.00 (standard baseline)
    pub fn confidence_modifier(&self) -> f64 {
        match self {
            OntologyAnchor::Core => 0.0,
            OntologyAnchor::DualAxis { .. } => 0.0,
            OntologyAnchor::DomainSupplement { namespace, .. } => match namespace {
                OntologyNamespace::Fibo => 0.10,
                OntologyNamespace::Cogat => -0.10,
                _ => 0.0,
            },
        }
    }

    /// Return the information density expectation for this ontology tier.
    /// Higher values = more information per token; condenser should be more conservative.
    /// FIBO financial data: dense numerical content, high precision needed → higher retention
    /// CogAT metaphorical: semantic meaning over exact wording → standard retention
    /// PKO process: status transitions matter → standard retention
    pub fn density_factor(&self) -> f64 {
        match self {
            OntologyAnchor::Core => 1.0,
            OntologyAnchor::DualAxis { axis, .. } => match axis {
                OntologyAxis::Pko => 1.0,    // process steps: standard density
                OntologyAxis::DcBibo => 1.0, // entity metadata: standard density
            },
            OntologyAnchor::DomainSupplement { namespace, .. } => match namespace {
                OntologyNamespace::Fibo => 1.3, // financial data: higher information density
                OntologyNamespace::Cogat => 0.9, // metaphorical: preserve semantic meaning
                OntologyNamespace::Golem => 1.0, // narrative: standard density
                OntologyNamespace::MlSchema => 1.1, // ML experiments: structured data
                OntologyNamespace::Omc => 1.0,  // media metadata: standard density
            },
        }
    }

    /// Which axis of the dual-axis framework this anchor belongs to (P5.4).
    pub fn axis(&self) -> Option<OntologyAxis> {
        match self {
            OntologyAnchor::Core => None,
            OntologyAnchor::DualAxis { axis, .. } => Some(*axis),
            OntologyAnchor::DomainSupplement { .. } => None, // domain supplements are beyond dual-axis
        }
    }

    /// Human-readable label for the ontology tier.
    pub fn tier_label(&self) -> &str {
        match self {
            OntologyAnchor::Core => "5w1h_core",
            OntologyAnchor::DualAxis { .. } => "dual_axis",
            OntologyAnchor::DomainSupplement { .. } => "domain_supplement",
        }
    }
}

/// Which axis of the dual-axis ontological framework (P5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OntologyAxis {
    /// Process (flow) axis — PKO: how did this come to be?
    Pko,
    /// State (entity) axis — Dublin Core + BIBO: what is this?
    DcBibo,
}

/// Domain supplement namespace — which domain-specific ontology bridge (P8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OntologyNamespace {
    /// Financial Industry Business Ontology (companies server)
    Fibo,
    /// GOLEM narrative ontology (replica server)
    Golem,
    /// Cognitive Atlas (memory server)
    Cogat,
    /// ML-Schema (training server)
    MlSchema,
    /// MovieLabs Ontology for Media Creation (media server)
    Omc,
}

impl OntologyNamespace {
    /// Map this domain supplement namespace to its canonical Dublin Core concept.
    pub fn dc_concept(&self) -> hkask_bridge_dublincore::DcConcept {
        match self {
            OntologyNamespace::Fibo => hkask_bridge_dublincore::DATASET,
            OntologyNamespace::Golem => hkask_bridge_dublincore::TEXT,
            OntologyNamespace::Cogat => hkask_bridge_dublincore::DATASET,
            OntologyNamespace::MlSchema => hkask_bridge_dublincore::DATASET,
            OntologyNamespace::Omc => hkask_bridge_dublincore::COLLECTION,
        }
    }

    /// Map this domain supplement namespace to its canonical PKO concept.
    pub fn pko_concept(&self) -> hkask_bridge_dublincore::PkoConcept {
        match self {
            OntologyNamespace::Fibo => hkask_bridge_dublincore::PROCEDURE,
            OntologyNamespace::Golem => hkask_bridge_dublincore::PROCEDURE,
            OntologyNamespace::Cogat => hkask_bridge_dublincore::FUNCTION,
            OntologyNamespace::MlSchema => hkask_bridge_dublincore::PROCEDURE,
            OntologyNamespace::Omc => hkask_bridge_dublincore::PROCEDURE_EXECUTION,
        }
    }
}

impl std::str::FromStr for OntologyNamespace {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fibo" => Ok(OntologyNamespace::Fibo),
            "golem" => Ok(OntologyNamespace::Golem),
            "cogat" => Ok(OntologyNamespace::Cogat),
            "mlschema" | "ml_schema" | "ml-schema" => Ok(OntologyNamespace::MlSchema),
            "omc" => Ok(OntologyNamespace::Omc),
            _ => Err(format!("Unknown ontology namespace: {s}")),
        }
    }
}

impl std::fmt::Display for OntologyNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OntologyNamespace::Fibo => write!(f, "fibo"),
            OntologyNamespace::Golem => write!(f, "golem"),
            OntologyNamespace::Cogat => write!(f, "cogat"),
            OntologyNamespace::MlSchema => write!(f, "mlschema"),
            OntologyNamespace::Omc => write!(f, "omc"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Request Types
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompressRequest {
    pub tool_name: String,
    pub output: String,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetProfileRequest {
    pub profile: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClassifyRequest {
    pub tool_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PersistRequest {
    /// Tool name that produced the content.
    pub tool_name: String,
    /// Content to persist (compressed output or thread summary).
    pub compressed_output: String,
    /// Optional confidence for the stored h_mem (0.0–1.0, default 1.0).
    pub confidence: Option<f64>,
}

/// Request to score text saliency against a target (persona or memory).
/// Used by the communication gate to inform CAT speak/silent decisions.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaliencyScoreRequest {
    /// The text to score (e.g., incoming message body).
    pub text: String,
    /// Scoring target: "persona" or "memory".
    pub against: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Heavy,
    Normal,
    Soft,
    Light,
}

impl Profile {
    /// Retention percentage: how much of the original content to keep.
    /// Lower = more aggressive compression (closer to minimal representation).
    pub fn retention_pct(&self) -> f64 {
        match self {
            Profile::Heavy => 0.10,
            Profile::Normal => 0.20,
            Profile::Soft => 0.60,
            Profile::Light => 0.95,
        }
    }

    /// Action threshold: how aggressively the compressor seeks minimal representation.
    ///
    /// This is the lazy universe tuning knob (P3 — Generative Space).
    /// Lower threshold = more aggressive action minimization (the system is "lazier").
    /// Higher threshold = more permissive (the user chooses a higher-action path).
    ///
    /// # Mapping to least action principle
    ///
    /// | Profile | Threshold | Lazy Universe Interpretation |
    /// |---------|-----------|------------------------------|
    /// | Heavy   | 0.10      | Aggressive minimization — system strongly seeks stationary action |
    /// | Normal  | 0.25      | Balanced — default operating point |
    /// | Soft    | 0.50      | Permissive — allows higher-action representations |
    /// | Light   | 0.90      | Minimal enforcement — user sovereignty overrides lazy tendency |
    pub fn action_threshold(&self) -> f64 {
        match self {
            Profile::Heavy => 0.10,
            Profile::Normal => 0.25,
            Profile::Soft => 0.50,
            Profile::Light => 0.90,
        }
    }

    pub fn max_lines(&self) -> Option<usize> {
        match self {
            Profile::Heavy => Some(30),
            Profile::Normal => Some(80),
            Profile::Soft => Some(200),
            Profile::Light => None,
        }
    }
}

impl std::str::FromStr for Profile {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "heavy" => Ok(Profile::Heavy),
            "normal" => Ok(Profile::Normal),
            "soft" => Ok(Profile::Soft),
            "light" => Ok(Profile::Light),
            _ => Err(format!(
                "Unknown profile '{s}'. Use: heavy, normal, soft, light"
            )),
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Heavy => write!(f, "heavy"),
            Profile::Normal => write!(f, "normal"),
            Profile::Soft => write!(f, "soft"),
            Profile::Light => write!(f, "light"),
        }
    }
}

/// Context category for compressor algorithm dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextCategory {
    ShellCommand,
    TestOutput,
    BuildOutput,
    FileContents,
    ConversationHistory,
    StructuredData,
    LogOutput,
    Unknown,
}

impl ContextCategory {
    pub fn label(&self) -> &str {
        match self {
            ContextCategory::ShellCommand => "shell_command",
            ContextCategory::TestOutput => "test_output",
            ContextCategory::BuildOutput => "build_output",
            ContextCategory::FileContents => "file_contents",
            ContextCategory::ConversationHistory => "conversation_history",
            ContextCategory::StructuredData => "structured_data",
            ContextCategory::LogOutput => "log_output",
            ContextCategory::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for ContextCategory {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "shell_command" => Ok(ContextCategory::ShellCommand),
            "test_output" => Ok(ContextCategory::TestOutput),
            "build_output" => Ok(ContextCategory::BuildOutput),
            "file_contents" => Ok(ContextCategory::FileContents),
            "conversation_history" => Ok(ContextCategory::ConversationHistory),
            "structured_data" => Ok(ContextCategory::StructuredData),
            "log_output" => Ok(ContextCategory::LogOutput),
            _ => Ok(ContextCategory::Unknown),
        }
    }
}

/// Output of a compression operation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompressedOutput {
    pub content: String,
    pub algorithm: String,
    pub category: String,
    pub profile: String,
    pub original_lines: usize,
    pub compressed_lines: usize,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub reduction_pct: f64,
    /// Health signals — populated when algorithmic behavior is unexpected.
    /// Absent means the compression ran within expected bounds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub health_signals: Vec<CondenserHealthSignal>,
}

/// Signal emitted when a condenser algorithm exhibits unexpected behavior.
/// These are Regulation `reg.condenser.*` ν-event candidates — they indicate that
/// the algorithmic performance deviated from expected bounds, not that the
/// compression failed (content is still returned).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CondenserHealthSignal {
    /// Algorithm that produced the signal.
    pub algorithm: String,
    /// Signal type: "negative_compression", "low_signal", "budget_shortfall".
    pub signal_type: String,
    /// Human-readable diagnostic.
    pub detail: String,
    /// Lines that scored zero (only for "low_signal" signals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zero_score_count: Option<usize>,
    /// Budget requested vs. actually filled (only for "budget_shortfall").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_requested: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_filled: Option<usize>,
}

/// Cumulative compression statistics.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CondenserStats {
    pub total_compressions: u64,
    pub total_original_bytes: u64,
    pub total_compressed_bytes: u64,
    pub algorithm_usage: std::collections::HashMap<String, u64>,
    pub category_usage: std::collections::HashMap<String, u64>,
    pub current_profile: String,
}

impl Default for CondenserStats {
    fn default() -> Self {
        Self {
            total_compressions: 0,
            total_original_bytes: 0,
            total_compressed_bytes: 0,
            algorithm_usage: std::collections::HashMap::new(),
            category_usage: std::collections::HashMap::new(),
            current_profile: "normal".to_string(),
        }
    }
}

/// A single compression observation for learning.
///
/// Stored in `CondenserEngine::history` (bounded ring buffer). Used by
/// `recommend_algorithm()` to select the best-performing algorithm per
/// category based on observed compression ratios.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompressionRecord {
    /// Algorithm name (e.g., "word_rank", "rtk_style", "flashrank").
    pub algorithm: String,
    /// Context category label (e.g., "log_output", "shell_command").
    pub category: String,
    /// Profile name at time of compression (e.g., "heavy", "normal").
    pub profile: String,
    /// Compression ratio: original_bytes / compressed_bytes (higher = better).
    pub compression_ratio: f64,
    /// Original input size in bytes.
    pub original_bytes: usize,
    /// Compressed output size in bytes.
    pub compressed_bytes: usize,
}

/// Per-algorithm compression statistics computed from history.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AlgorithmStats {
    pub count: usize,
    pub mean_ratio: f64,
    pub min_ratio: f64,
    pub max_ratio: f64,
}

/// Per-category compression statistics computed from history.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategoryStats {
    pub count: usize,
    pub mean_ratio: f64,
    pub best_algorithm: String,
}

/// Summary of compression history grouped by algorithm and category.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompressionHistoryStats {
    pub by_algorithm: std::collections::HashMap<String, AlgorithmStats>,
    pub by_category: std::collections::HashMap<String, CategoryStats>,
    pub total_records: usize,
}

/// Request for thread summarization via the centralized inference router.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadSummaryRequest {
    /// Conversation messages to summarize, as an array of {role, content} objects.
    pub messages: Vec<serde_json::Value>,
    /// The current user query for relevance-weighted summarization.
    pub current_query: String,
    /// Maximum tokens for the summary output (default 500).
    pub max_tokens: Option<u32>,
    /// Override the server's default inference model.
    /// When provided, this model is used instead of the server-configured default.
    /// Supports provider-prefixed names (OM/, FW/, DI/) for backend routing.
    pub model: Option<String>,
}

/// Output of a thread summarization.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThreadSummaryOutput {
    pub summary: String,
    pub original_message_count: usize,
    /// Approximate token count of the original conversation (before summarization).
    /// Uses whitespace-split heuristic — rough estimate for context window planning.
    pub original_tokens_approx: usize,
    pub summary_tokens_approx: usize,
    pub inference_model: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_parsing_known_values() {
        assert_eq!("heavy".parse::<Profile>().unwrap(), Profile::Heavy);
        assert_eq!("normal".parse::<Profile>().unwrap(), Profile::Normal);
        assert_eq!("soft".parse::<Profile>().unwrap(), Profile::Soft);
        assert_eq!("light".parse::<Profile>().unwrap(), Profile::Light);
    }

    #[test]
    fn profile_parsing_case_insensitive() {
        assert_eq!("HEAVY".parse::<Profile>().unwrap(), Profile::Heavy);
        assert_eq!("Normal".parse::<Profile>().unwrap(), Profile::Normal);
        assert_eq!("SoFt".parse::<Profile>().unwrap(), Profile::Soft);
    }

    #[test]
    fn profile_parsing_unknown_is_error() {
        assert!("extreme".parse::<Profile>().is_err());
        assert!("super_heavy".parse::<Profile>().is_err());
        assert!("".parse::<Profile>().is_err());
    }

    #[test]
    fn profile_retention_pct_bounds() {
        assert!((Profile::Heavy.retention_pct() - 0.10).abs() < 0.001);
        assert!((Profile::Normal.retention_pct() - 0.20).abs() < 0.001);
        assert!((Profile::Soft.retention_pct() - 0.60).abs() < 0.001);
        assert!((Profile::Light.retention_pct() - 0.95).abs() < 0.001);
        for profile in &[
            Profile::Heavy,
            Profile::Normal,
            Profile::Soft,
            Profile::Light,
        ] {
            let pct = profile.retention_pct();
            assert!(
                pct > 0.0 && pct < 1.0,
                "{profile}: retention {pct} out of bounds"
            );
        }
    }

    #[test]
    fn profile_max_lines() {
        assert_eq!(Profile::Heavy.max_lines(), Some(30));
        assert_eq!(Profile::Normal.max_lines(), Some(80));
        assert_eq!(Profile::Soft.max_lines(), Some(200));
        assert_eq!(Profile::Light.max_lines(), None);
    }

    #[test]
    fn profile_display_roundtrip() {
        for original in &[
            Profile::Heavy,
            Profile::Normal,
            Profile::Soft,
            Profile::Light,
        ] {
            let s = original.to_string();
            let parsed: Profile = s.parse().unwrap();
            assert_eq!(parsed, *original);
        }
    }

    //
    // TASK 4.4: Each profile carries an action_threshold that controls how
    // aggressively the compressor seeks minimal representation. Heavy = most
    // aggressive (lowest threshold), Light = most permissive (highest threshold).
    #[test]
    fn action_threshold_ordering() {
        let heavy = Profile::Heavy.action_threshold();
        let normal = Profile::Normal.action_threshold();
        let soft = Profile::Soft.action_threshold();
        let light = Profile::Light.action_threshold();

        // Heavy should be most aggressive (lowest threshold)
        assert!(
            heavy < normal,
            "Heavy ({heavy}) should be < Normal ({normal})"
        );
        assert!(normal < soft, "Normal ({normal}) should be < Soft ({soft})");
        assert!(soft < light, "Soft ({soft}) should be < Light ({light})");

        // All thresholds must be in (0.0, 1.0)
        for (name, threshold) in &[
            ("Heavy", heavy),
            ("Normal", normal),
            ("Soft", soft),
            ("Light", light),
        ] {
            assert!(
                *threshold > 0.0 && *threshold < 1.0,
                "{name} action_threshold {threshold} out of bounds"
            );
        }
    }

    //
    // The user controls how "lazy" their system is by selecting a profile.
    // Light profile = user sovereignty overrides lazy tendency (P1 + P3).
    #[test]
    fn light_profile_is_most_permissive() {
        let light = Profile::Light.action_threshold();
        let heavy = Profile::Heavy.action_threshold();
        assert!(
            light > heavy,
            "Light should be most permissive (highest threshold)"
        );
        // Light threshold should be close to 1.0 — minimal enforcement
        assert!(light >= 0.85, "Light threshold {light} should be >= 0.85");
    }

    #[test]
    fn context_category_parsing() {
        assert_eq!(
            "shell_command".parse::<ContextCategory>().unwrap(),
            ContextCategory::ShellCommand
        );
        assert_eq!(
            "test_output".parse::<ContextCategory>().unwrap(),
            ContextCategory::TestOutput
        );
        assert_eq!(
            "build_output".parse::<ContextCategory>().unwrap(),
            ContextCategory::BuildOutput
        );
        assert_eq!(
            "file_contents".parse::<ContextCategory>().unwrap(),
            ContextCategory::FileContents
        );
        assert_eq!(
            "conversation_history".parse::<ContextCategory>().unwrap(),
            ContextCategory::ConversationHistory
        );
        assert_eq!(
            "structured_data".parse::<ContextCategory>().unwrap(),
            ContextCategory::StructuredData
        );
        assert_eq!(
            "log_output".parse::<ContextCategory>().unwrap(),
            ContextCategory::LogOutput
        );
    }

    #[test]
    fn context_category_unknown_fallback() {
        assert_eq!(
            "garbage".parse::<ContextCategory>().unwrap(),
            ContextCategory::Unknown
        );
        assert_eq!(
            "".parse::<ContextCategory>().unwrap(),
            ContextCategory::Unknown
        );
    }

    #[test]
    fn context_category_label_roundtrip() {
        let all = [
            ContextCategory::ShellCommand,
            ContextCategory::TestOutput,
            ContextCategory::BuildOutput,
            ContextCategory::FileContents,
            ContextCategory::ConversationHistory,
            ContextCategory::StructuredData,
            ContextCategory::LogOutput,
            ContextCategory::Unknown,
        ];
        for cat in &all {
            let label = cat.label();
            let parsed: ContextCategory = label.parse().unwrap();
            assert_eq!(parsed, *cat, "round-trip failed for {cat:?}");
        }
    }

    // ── Ontology Anchor Tests (P5.2/P5.4/P8.1) ───────────────────────────

    #[test]
    fn ontology_anchor_confidence_modifiers() {
        // Core and DualAxis have no modifier
        assert!((OntologyAnchor::Core.confidence_modifier() - 0.0).abs() < 0.001);
        assert!(
            (OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: "pko:StepExecution".into()
            }
            .confidence_modifier()
                - 0.0)
                .abs()
                < 0.001
        );

        // FIBO: +0.10 (OMG standard, high adoption)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: "fibo:Corporation".into()
            }
            .confidence_modifier()
                - 0.10)
                .abs()
                < 0.001
        );

        // CogAT: -0.10 (metaphorical mapping)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Cogat,
                concept: "cogat:episodic_memory".into()
            }
            .confidence_modifier()
                - (-0.10))
                .abs()
                < 0.001
        );

        // GOLEM, ML-Schema, OMC: ±0.00 (standard)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Golem,
                concept: "golem:Character".into()
            }
            .confidence_modifier()
                - 0.0)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn ontology_anchor_density_factors() {
        // FIBO financial data: densest (1.3x retention)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: "fibo:Corporation".into()
            }
            .density_factor()
                - 1.3)
                .abs()
                < 0.001
        );

        // CogAT metaphorical: lowest density (0.9x — preserve semantic meaning)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Cogat,
                concept: "cogat:salience".into()
            }
            .density_factor()
                - 0.9)
                .abs()
                < 0.001
        );

        // PKO/DC: standard (1.0x)
        assert!(
            (OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: "pko:StepExecution".into()
            }
            .density_factor()
                - 1.0)
                .abs()
                < 0.001
        );

        // Core: standard (1.0x)
        assert!((OntologyAnchor::Core.density_factor() - 1.0).abs() < 0.001);
    }

    #[test]
    fn ontology_anchor_tier_labels() {
        assert_eq!(OntologyAnchor::Core.tier_label(), "5w1h_core");
        assert_eq!(
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: "pko:Step".into()
            }
            .tier_label(),
            "dual_axis"
        );
        assert_eq!(
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: "fibo:Corporation".into()
            }
            .tier_label(),
            "domain_supplement"
        );
    }

    #[test]
    fn ontology_anchor_axis_detection() {
        assert_eq!(OntologyAnchor::Core.axis(), None);
        assert_eq!(
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: "pko:Step".into()
            }
            .axis(),
            Some(OntologyAxis::Pko)
        );
        assert_eq!(
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::DcBibo,
                concept: "bibo:Article".into()
            }
            .axis(),
            Some(OntologyAxis::DcBibo)
        );
    }

    #[test]
    fn ontology_namespace_parsing() {
        assert_eq!(
            "fibo".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::Fibo
        );
        assert_eq!(
            "golem".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::Golem
        );
        assert_eq!(
            "cogat".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::Cogat
        );
        assert_eq!(
            "mlschema".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::MlSchema
        );
        assert_eq!(
            "ml_schema".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::MlSchema
        );
        assert_eq!(
            "omc".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::Omc
        );
        assert!("unknown".parse::<OntologyNamespace>().is_err());
    }

    #[test]
    fn ontology_namespace_display_roundtrip() {
        let namespaces = [
            OntologyNamespace::Fibo,
            OntologyNamespace::Golem,
            OntologyNamespace::Cogat,
            OntologyNamespace::MlSchema,
            OntologyNamespace::Omc,
        ];
        for ns in &namespaces {
            let s = ns.to_string();
            let parsed: OntologyNamespace = s.parse().unwrap();
            assert_eq!(parsed, *ns, "round-trip failed for {ns:?}");
        }
    }

    #[test]
    fn compress_request_defaults_category_to_none() {
        let req: CompressRequest =
            serde_json::from_str(r#"{"tool_name": "test", "output": "hello"}"#).unwrap();
        assert_eq!(req.category, None);
    }

    #[test]
    fn compress_request_parses_explicit_category() {
        let req: CompressRequest = serde_json::from_str(
            r#"{
                "tool_name": "company_profile",
                "output": "AAPL market cap 3.2T",
                "category": "structured_data"
            }"#,
        )
        .unwrap();
        assert_eq!(req.category.as_deref(), Some("structured_data"));
        assert_eq!(req.tool_name, "company_profile");
    }
}
