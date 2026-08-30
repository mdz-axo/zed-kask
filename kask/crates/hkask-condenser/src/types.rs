//! hKask Condenser — Domain types
//!
//! Domain types for the condenser. Error types use `String` for `FromStr`
//! impls; MCP servers wrap these at the boundary.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════
// Ontology Anchoring (P5.2 / P5.4 / P8.1)
//
// The ontology types (`OntologyAnchor`, `OntologyAxis`, `OntologyNamespace`)
// and the domain-selection logic live in the shared `hkask-bridge-ontology`
// crate. The condenser re-exports them internally so its call sites
// (`crate::types::OntologyAnchor` etc.) keep working without touching every
// reference; the single source of truth is the bridge crate.
// ═══════════════════════════════════════════════════════════════════════════

pub(crate) use hkask_bridge_ontology::axis::{
    OntologyAnchor, OntologyAxis, OntologyNamespace, select_ontology_anchor,
};

// ═══════════════════════════════════════════════════════════════════════════
// Request Types
// ═══════════════════════════════════════════════════════════════════════════

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
/// They indicate that the algorithmic performance deviated from expected
/// bounds, not that the compression failed (content is still returned).
/// Condenser telemetry is diagnostic and rides `hkask.condenser`; promoting a
/// signal to a ν-event means registering a `reg.*` namespace and wiring a
/// consumer, neither of which exists today.
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
                concept: hkask_bridge_ontology::pko::STEP_EXECUTION.into()
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
                concept: hkask_bridge_ontology::fibo::CORPORATION.into()
            }
            .confidence_modifier()
                - 0.10)
                .abs()
                < 0.001
        );

        // SUMO: +0.05 (upper ontology, broad coverage)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Sumo,
                concept: hkask_bridge_ontology::sumo::ENTITY.into()
            }
            .confidence_modifier()
                - 0.05)
                .abs()
                < 0.001
        );

        // GOLEM, ML-Schema: ±0.00 (standard)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Golem,
                concept: hkask_bridge_ontology::golem::CHARACTER.into()
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
                concept: hkask_bridge_ontology::fibo::CORPORATION.into()
            }
            .density_factor()
                - 1.3)
                .abs()
                < 0.001
        );

        // SUMO: standard density (1.0x)
        assert!(
            (OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Sumo,
                concept: hkask_bridge_ontology::sumo::PROCESS.into()
            }
            .density_factor()
                - 1.0)
                .abs()
                < 0.001
        );

        // PKO/DC: standard (1.0x)
        assert!(
            (OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: hkask_bridge_ontology::pko::STEP_EXECUTION.into()
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
                concept: hkask_bridge_ontology::pko::STEP.into()
            }
            .tier_label(),
            "dual_axis"
        );
        assert_eq!(
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: hkask_bridge_ontology::fibo::CORPORATION.into()
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
                concept: hkask_bridge_ontology::pko::STEP.into()
            }
            .axis(),
            Some(OntologyAxis::Pko)
        );
        assert_eq!(
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::DcBibo,
                concept: hkask_bridge_ontology::dc_bibo::ARTICLE.into()
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
            "sumo".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::Sumo
        );
        assert_eq!(
            "mlschema".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::MlSchema
        );
        assert_eq!(
            "ml_schema".parse::<OntologyNamespace>().unwrap(),
            OntologyNamespace::MlSchema
        );
        assert!("unknown".parse::<OntologyNamespace>().is_err());
    }

    #[test]
    fn ontology_namespace_display_roundtrip() {
        let namespaces = [
            OntologyNamespace::Fibo,
            OntologyNamespace::Golem,
            OntologyNamespace::MlSchema,
            OntologyNamespace::Sumo,
        ];
        for ns in &namespaces {
            let s = ns.to_string();
            let parsed: OntologyNamespace = s.parse().unwrap();
            assert_eq!(parsed, *ns, "round-trip failed for {ns:?}");
        }
    }
}
