//! CondenserEngine — Pure domain logic for context condensation
//!
//! No async, no MCP dependencies, no HTTP. This module owns compression
//! dispatch and profile management. The `AlgorithmRegistry` is constructed
//! once at startup and is immutable; `CondenserEngine` holds only the
//! active profile as mutable state.
//!
//! ## Telemetry
//!
//! The `tracing::debug!` calls with `target: "hkask.condenser"` are diagnostic
//! logging for human inspection, NOT cybernetic feedback signals — which is
//! why they sit under `hkask.*` rather than the reserved `reg.*` prefix
//! (PRINCIPLES §9.1). The actual feedback channel is the daemon's
//! `store_experience` call in the MCP server layer. See the condenser README.

use crate::algorithms::{AlgorithmRegistry, classify_tool};
use crate::types::*;
use std::time::Instant;

/// Compression dispatch + active profile.
///
/// The engine selects an algorithm per compression via the static
/// `default_for()` mapping in `AlgorithmRegistry`. The previous learning
/// subsystem (history ring buffer, `recommend_algorithm`, `compression_stats`,
/// `suggest_profile`, `check_global_health`) was removed: it was dormant in
/// the default-off configuration and existed only to justify the MCP tools
/// that surfaced it. The runtime bridge path (`BridgeThreadCondenser`)
/// never used it.
pub struct CondenserEngine {
    pub(crate) registry: AlgorithmRegistry,
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
            registry: AlgorithmRegistry::new(),
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
        let algo = self.registry.select(cat);
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

    #[test]
    fn profile_getter_returns_current() {
        let mut engine = CondenserEngine::new();
        assert_eq!(engine.profile(), Profile::Normal);
        engine.set_profile(Profile::Heavy);
        assert_eq!(engine.profile(), Profile::Heavy);
    }

    #[test]
    fn compress_returns_content_for_shell_output() {
        let mut engine = CondenserEngine::new();
        let input = "line1\nline2\nline3\nline4\nline5\n".repeat(20);
        let result = engine.compress("bash_execute", &input, None);
        assert!(!result.content.is_empty());
        assert_eq!(result.category, "shell_command");
        assert_eq!(result.algorithm, "rtk_style");
        assert!(result.compressed_bytes <= result.original_bytes);
    }

    #[test]
    fn compress_passthrough_when_budget_exceeds_input() {
        // Light profile (95% retention, no max) on a small input should
        // return the input unchanged (budget >= lines).
        let mut engine = CondenserEngine::new();
        engine.set_profile(Profile::Light);
        let input = "only a few lines\n";
        let result = engine.compress("bash_execute", input, None);
        assert_eq!(result.content, input);
    }
}
