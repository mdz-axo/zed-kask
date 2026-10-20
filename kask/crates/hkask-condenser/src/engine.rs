//! CondenserEngine — Pure domain logic for context condensation
//!
//! No async, no MCP dependencies, no HTTP. This module owns exhaustive
//! category dispatch and holds only the active compression profile.
//!
//! ## Telemetry
//!
//! The `tracing::debug!` calls with `target: "hkask.condenser"` are diagnostic
//! logging for human inspection, NOT cybernetic feedback signals — which is
//! why they sit under `hkask.*` rather than the reserved `reg.*` prefix
//! (PRINCIPLES §9.1). There is no feedback channel: the condenser is a pure
//! in-process compressor wired via `BridgeThreadCondenser` (D8).

use crate::algorithms::{
    CondenserAlgorithm, FlashrankAlgorithm, RtkStyleAlgorithm, WordRankAlgorithm, classify_tool,
};
use crate::types::*;
use std::time::Instant;

/// Exhaustive category dispatch and the active compression profile.
pub struct CondenserEngine {
    profile: Profile,
}

impl Default for CondenserEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CondenserEngine {
    pub fn new() -> Self {
        Self {
            profile: Profile::Normal,
        }
    }

    pub fn compress(
        &mut self,
        tool_name: &str,
        output: &str,
        category: Option<ContextCategory>,
    ) -> CompressedOutput {
        let cat = category.unwrap_or_else(|| classify_tool(tool_name));
        let algo: &dyn CondenserAlgorithm = match cat {
            ContextCategory::ShellCommand
            | ContextCategory::TestOutput
            | ContextCategory::BuildOutput => &RtkStyleAlgorithm,
            ContextCategory::ConversationHistory | ContextCategory::LogOutput => &WordRankAlgorithm,
            ContextCategory::FileContents
            | ContextCategory::StructuredData
            | ContextCategory::Unknown => &FlashrankAlgorithm,
        };
        let algorithm_name = algo.name().to_string();

        // Derive ontology anchor from tool name — every MCP server links
        // against the same bridge crates; no wire-protocol fields needed.
        let ontology_anchor = select_ontology_anchor(tool_name);
        let tier_label = ontology_anchor.tier_label();

        let start = Instant::now();

        // Diagnostic telemetry — see module docs: these are diagnostic-only,
        // not cybernetic feedback signals. Emitted at `debug` to avoid log spam
        // (one pair of spans per tool-result compression).
        tracing::debug!(target: "hkask.condenser", operation = "compress", algorithm = %algorithm_name, category = %cat.label(), tool_name = %tool_name, ontology_tier = %tier_label, "REG");

        let (compressed_content, health_signals) =
            algo.compress(output, self.profile, cat, Some(&ontology_anchor));

        let original_lines = output.lines().count();
        let compressed_lines = compressed_content.lines().count();
        let original_bytes = output.len();
        let compressed_bytes = compressed_content.len();
        let reduction_pct = if original_bytes == 0 {
            0.0
        } else {
            (1.0 - (compressed_bytes as f64 / original_bytes as f64)) * 100.0
        };

        // Diagnostic telemetry
        tracing::debug!(target: "hkask.condenser", operation = "compression_ratio", algorithm = %algorithm_name, category = %cat.label(), reduction_pct = %format!("{:.1}", reduction_pct), original_bytes = original_bytes, compressed_bytes = compressed_bytes, latency_ms = start.elapsed().as_millis(), "REG");

        CompressedOutput {
            content: compressed_content,
            algorithm: algorithm_name,
            category: cat.label().to_string(),
            profile: self.profile.to_string(),
            original_lines,
            compressed_lines,
            original_bytes,
            compressed_bytes,
            reduction_pct,
            health_signals,
        }
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    /// Returns the current compression profile.
    pub fn profile(&self) -> Profile {
        self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::{
        CondenserAlgorithm, FlashrankAlgorithm, RtkStyleAlgorithm, WordRankAlgorithm,
    };

    #[test]
    fn profile_getter_returns_current() {
        let mut engine = CondenserEngine::new();
        assert_eq!(engine.profile(), Profile::Normal);
        engine.set_profile(Profile::Heavy);
        assert_eq!(engine.profile(), Profile::Heavy);
    }

    #[test]
    fn compress_preserves_dispatch_profiles_and_output_accounting() {
        // Independent dispatch oracle: do not derive this table from the registry.
        let routes: [(ContextCategory, &str, &str, &dyn CondenserAlgorithm); 8] = [
            (
                ContextCategory::ShellCommand,
                "bash_execute",
                "rtk_style",
                &RtkStyleAlgorithm,
            ),
            (
                ContextCategory::TestOutput,
                "pytest",
                "rtk_style",
                &RtkStyleAlgorithm,
            ),
            (
                ContextCategory::BuildOutput,
                "compile",
                "rtk_style",
                &RtkStyleAlgorithm,
            ),
            (
                ContextCategory::ConversationHistory,
                "chat",
                "word_rank",
                &WordRankAlgorithm,
            ),
            (
                ContextCategory::LogOutput,
                "log",
                "word_rank",
                &WordRankAlgorithm,
            ),
            (
                ContextCategory::FileContents,
                "read_file",
                "flashrank",
                &FlashrankAlgorithm,
            ),
            (
                ContextCategory::StructuredData,
                "query",
                "flashrank",
                &FlashrankAlgorithm,
            ),
            (
                ContextCategory::Unknown,
                "xyz",
                "flashrank",
                &FlashrankAlgorithm,
            ),
        ];
        let budgeted = "# Summary\nerror: failed assertion\nwarning: retry\nvalue: 42\nordinary progress\nfinished successfully\n".repeat(4);
        let low_signal = "x\n".repeat(40);
        let mut engine = CondenserEngine::new();
        for profile in [
            Profile::Heavy,
            Profile::Normal,
            Profile::Soft,
            Profile::Light,
        ] {
            engine.set_profile(profile);
            for (category, inferred_tool, name, algorithm) in routes {
                for (tool, override_category) in [
                    (inferred_tool, None),
                    // A conflicting tool category must not defeat an explicit override.
                    (
                        if category == ContextCategory::ShellCommand {
                            "read_file"
                        } else {
                            "bash_execute"
                        },
                        Some(category),
                    ),
                ] {
                    let anchor = select_ontology_anchor(tool);
                    for input in [
                        "",
                        "```\nshort payload\n```\n",
                        budgeted.as_str(),
                        low_signal.as_str(),
                    ] {
                        let (content, health) =
                            algorithm.compress(input, profile, category, Some(&anchor));
                        let result = engine.compress(tool, input, override_category);
                        let case = format!(
                            "{category:?}/{profile}/{tool}/{} lines",
                            input.lines().count()
                        );
                        assert_eq!(result.algorithm, name, "{case}");
                        assert_eq!(result.category, category.label(), "{case}");
                        assert_eq!(result.profile, profile.to_string(), "{case}");
                        assert_eq!(engine.profile(), profile, "{case}");
                        assert_eq!(result.content, content, "{case}");
                        assert_eq!(result.health_signals.len(), health.len(), "{case}");
                        for (actual, expected) in result.health_signals.iter().zip(&health) {
                            assert_eq!(actual.algorithm, expected.algorithm, "{case}");
                            assert_eq!(actual.signal_type, expected.signal_type, "{case}");
                            assert_eq!(actual.detail, expected.detail, "{case}");
                            assert_eq!(
                                actual.zero_score_count, expected.zero_score_count,
                                "{case}"
                            );
                            assert_eq!(
                                actual.budget_requested, expected.budget_requested,
                                "{case}"
                            );
                            assert_eq!(actual.budget_filled, expected.budget_filled, "{case}");
                        }
                        assert_eq!(result.original_lines, input.lines().count(), "{case}");
                        assert_eq!(result.original_bytes, input.len(), "{case}");
                        assert_eq!(result.compressed_lines, content.lines().count(), "{case}");
                        assert_eq!(result.compressed_bytes, content.len(), "{case}");
                        let reduction = if input.is_empty() {
                            0.0
                        } else {
                            (1.0 - content.len() as f64 / input.len() as f64) * 100.0
                        };
                        assert_eq!(result.reduction_pct, reduction, "{case}");
                    }
                }
            }
        }
    }
}
