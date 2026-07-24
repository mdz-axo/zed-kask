//! CondenserEngine — Pure domain logic for context condensation
//!
//! No async, no MCP dependencies, no HTTP. This module owns compression
//! dispatch, profile management, cumulative statistics, and compression
//! history for algorithm learning.
//!
//! The `AlgorithmRegistry` is constructed once at startup and is immutable.
//! `CondenserEngine` holds mutable state: profile, stats, and a bounded
//! history ring buffer of `CompressionRecord` observations.
//!
//! ## Learning
//!
//! The engine records each compression as a `CompressionRecord`. After
//! `MIN_OBSERVATIONS_FOR_RECOMMENDATION` (10) compressions for a given
//! category, `recommend_algorithm()` returns the algorithm with the best
//! historical compression ratio. When sufficient data exists, `compress()`
//! auto-selects the recommended algorithm instead of the static `default_for()`
//! mapping — this is the condenser's learning mechanism.
//!
//! ## Regulation Spans
//!
//! The `tracing::info!` calls with `target: "reg.condenser"` are diagnostic
//! logging for human inspection, NOT cybernetic feedback signals. The
//! actual feedback channel is the daemon's `store_experience` call in the
//! MCP server layer. See the condenser README for details.

use crate::algorithms::{AlgorithmRegistry, classify_tool, derive_ontology_anchor};
use crate::types::*;
use std::collections::VecDeque;
use std::time::Instant;

/// Maximum compression records retained for learning.
const MAX_HISTORY: usize = 200;

/// Minimum observations per category before algorithm recommendation is trusted.
const MIN_OBSERVATIONS_FOR_RECOMMENDATION: usize = 10;

// G4: profile is the single source of truth; stats.current_profile is always derived from it.
pub struct CondenserEngine {
    pub registry: AlgorithmRegistry,
    profile: Profile,
    pub stats: CondenserStats,
    /// Bounded ring buffer of compression observations for learning.
    history: VecDeque<CompressionRecord>,
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
            stats: CondenserStats::default(),
            history: VecDeque::new(),
        }
    }

    /// Resolve a tool name to its category and the algorithm that handles it.
    ///
    /// D3: single call site for the classify→select path. `condenser_classify`
    /// delegates here instead of calling `classify_tool` + `registry.select` directly.
    /// Returns an owned `String` for the algorithm name so callers can hold the
    /// result across a mutable borrow of `self`.
    pub fn classify(&self, tool_name: &str) -> (ContextCategory, String) {
        let cat = classify_tool(tool_name);
        let algo = self.registry.select(cat);
        (cat, algo.name().to_string())
    }

    pub fn compress(
        &mut self,
        tool_name: &str,
        output: &str,
        category: Option<ContextCategory>,
    ) -> CompressedOutput {
        let cat = category.unwrap_or_else(|| classify_tool(tool_name));

        // Learning: if we have enough history for this category, use the
        // best-performing algorithm instead of the static default_for() mapping.
        let algo = if let Some(ref recommended) = self.recommend_algorithm(cat) {
            self.registry
                .select_by_name(recommended)
                .unwrap_or_else(|| self.registry.select(cat))
        } else {
            self.registry.select(cat)
        };
        let algorithm_name = algo.name().to_string();

        // Derive ontology anchor from tool name — every MCP server links
        // against the same bridge crates; no wire-protocol fields needed.
        let ontology_anchor = derive_ontology_anchor(tool_name);
        let tier_label = ontology_anchor.tier_label();

        let start = Instant::now();

        // Diagnostic Regulation span — see module docs: these are diagnostic-only,
        // not cybernetic feedback signals.
        tracing::info!(target: "reg.condenser", operation = "compress", algorithm = %algorithm_name, category = %cat.label(), tool_name = %tool_name, ontology_tier = %tier_label, "REG");

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
        let compression_ratio = if compressed_bytes == 0 {
            0.0
        } else {
            original_bytes as f64 / compressed_bytes as f64
        };

        // Update cumulative stats
        *self
            .stats
            .algorithm_usage
            .entry(algorithm_name.clone())
            .or_insert(0) += 1;
        *self
            .stats
            .category_usage
            .entry(cat.label().to_string())
            .or_insert(0) += 1;
        self.stats.total_compressions += 1;
        self.stats.total_original_bytes += original_bytes as u64;
        self.stats.total_compressed_bytes += compressed_bytes as u64;

        // Record compression observation for learning
        self.history.push_back(CompressionRecord {
            algorithm: algorithm_name.clone(),
            category: cat.label().to_string(),
            profile: self.profile.to_string(),
            compression_ratio,
            original_bytes,
            compressed_bytes,
        });
        if self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }

        // Diagnostic Regulation span
        tracing::info!(target: "reg.condenser", operation = "compression_ratio", algorithm = %algorithm_name, category = %cat.label(), reduction_pct = %format!("{:.1}", reduction_pct), original_bytes = original_bytes, compressed_bytes = compressed_bytes, latency_ms = start.elapsed().as_millis(), "REG");

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
        self.stats.current_profile = profile.to_string();
    }

    /// Returns the current compression profile.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn get_stats(&self) -> &CondenserStats {
        &self.stats
    }

    /// Returns the number of compression records stored in history.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Recommend the best-performing algorithm for a category based on
    /// observed compression ratios.
    ///
    /// Returns `None` if fewer than `MIN_OBSERVATIONS_FOR_RECOMMENDATION`
    /// records exist for the given category (insufficient data). When
    /// sufficient data exists, returns the algorithm name with the highest
    /// mean compression ratio.
    ///
    /// This is called automatically by `compress()` — when data is sufficient,
    /// the recommended algorithm is used instead of the static `default_for()`
    /// mapping. This is the condenser's learning mechanism: the more it
    /// compresses, the better it selects algorithms per category.
    pub fn recommend_algorithm(&self, category: ContextCategory) -> Option<String> {
        let cat_label = category.label();
        let records: Vec<&CompressionRecord> = self
            .history
            .iter()
            .filter(|r| r.category == cat_label)
            .collect();

        if records.len() < MIN_OBSERVATIONS_FOR_RECOMMENDATION {
            return None;
        }

        // Group by algorithm, compute mean compression ratio
        let mut algo_ratios: std::collections::HashMap<&str, (f64, usize)> =
            std::collections::HashMap::new();
        for r in &records {
            let entry = algo_ratios.entry(r.algorithm.as_str()).or_insert((0.0, 0));
            entry.0 += r.compression_ratio;
            entry.1 += 1;
        }

        // Return the algorithm with the highest mean ratio
        algo_ratios
            .into_iter()
            .map(|(name, (sum, count))| (name, sum / count as f64))
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(name, _)| name.to_string())
    }

    /// Compute compression statistics grouped by algorithm and category.
    ///
    /// Returns per-algorithm and per-category summaries (count, mean/min/max
    /// ratio, best algorithm per category) from the compression history.
    pub fn compression_stats(&self) -> CompressionHistoryStats {
        let mut by_algorithm: std::collections::HashMap<String, (Vec<f64>, usize)> =
            std::collections::HashMap::new();
        let mut by_category: std::collections::HashMap<
            String,
            (Vec<f64>, std::collections::HashMap<String, f64>),
        > = std::collections::HashMap::new();

        for r in &self.history {
            let algo_entry = by_algorithm
                .entry(r.algorithm.clone())
                .or_insert((Vec::new(), 0));
            algo_entry.0.push(r.compression_ratio);
            algo_entry.1 += 1;

            let cat_entry = by_category
                .entry(r.category.clone())
                .or_insert((Vec::new(), std::collections::HashMap::new()));
            cat_entry.0.push(r.compression_ratio);
            *cat_entry.1.entry(r.algorithm.clone()).or_insert(0.0) += r.compression_ratio;
        }

        let by_algorithm: std::collections::HashMap<String, AlgorithmStats> = by_algorithm
            .into_iter()
            .map(|(name, (ratios, count))| {
                let mean = ratios.iter().sum::<f64>() / count as f64;
                let min = ratios.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = ratios.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                (
                    name,
                    AlgorithmStats {
                        count,
                        mean_ratio: mean,
                        min_ratio: min,
                        max_ratio: max,
                    },
                )
            })
            .collect();

        let by_category: std::collections::HashMap<String, CategoryStats> = by_category
            .into_iter()
            .map(|(name, (ratios, algo_sums))| {
                let count = ratios.len();
                let mean = ratios.iter().sum::<f64>() / count as f64;
                let best_algorithm = algo_sums
                    .into_iter()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(algo, _)| algo)
                    .unwrap_or_default();
                (
                    name,
                    CategoryStats {
                        count,
                        mean_ratio: mean,
                        best_algorithm,
                    },
                )
            })
            .collect();

        CompressionHistoryStats {
            by_algorithm,
            by_category,
            total_records: self.history.len(),
        }
    }

    /// Suggest a more aggressive profile when compression health is degraded.
    ///
    /// Returns `Heavy` if the global health check flags `low_compression_ratio`
    /// and the current profile is not already `Heavy`. Otherwise returns the
    /// current profile (no change needed).
    ///
    /// This is advisory — the caller (MCP server's `condenser_ping`) includes
    /// the suggestion in its response. The operator decides whether to accept.
    pub fn suggest_profile(&self) -> Profile {
        let health = self.check_global_health();
        let degraded = health
            .iter()
            .any(|s| s.signal_type == "low_compression_ratio");

        if degraded && self.profile != Profile::Heavy {
            Profile::Heavy
        } else {
            self.profile
        }
    }

    /// Check for global health violations across all compressions.
    ///
    /// Returns health signals for systemic issues: overall compression ratio
    /// below 2:1 or throughput anomalies. Callers should emit these as
    /// `reg.condenser.degraded` ν-events.
    pub fn check_global_health(&self) -> Vec<CondenserHealthSignal> {
        let mut signals = Vec::new();
        let stats = &self.stats;

        if stats.total_original_bytes > 0 {
            let ratio =
                stats.total_original_bytes as f64 / stats.total_compressed_bytes.max(1) as f64;
            if ratio < 2.0 && stats.total_compressions >= 10 {
                signals.push(CondenserHealthSignal {
                    algorithm: "global".into(),
                    signal_type: "low_compression_ratio".into(),
                    detail: format!(
                        "Overall compression ratio {:.2}:1 below 2:1 SLA ({} compressions)",
                        ratio, stats.total_compressions
                    ),
                    zero_score_count: None,
                    budget_requested: None,
                    budget_filled: None,
                });
            }
        }

        // Diagnostic Regulation span — see module docs.
        tracing::info!(target: "reg.condenser", operation = "health", total_compressions = stats.total_compressions, health_signal_count = signals.len(), "REG");

        signals
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_starts_empty() {
        let engine = CondenserEngine::new();
        assert_eq!(engine.history_len(), 0);
    }

    #[test]
    fn history_increments_after_compress() {
        let mut engine = CondenserEngine::new();
        let input = "line1\nline2\nline3\nline4\nline5\n".repeat(20);
        engine.compress("bash_execute", &input, None);
        assert_eq!(engine.history_len(), 1);
        engine.compress("bash_execute", &input, None);
        assert_eq!(engine.history_len(), 2);
    }

    #[test]
    fn history_bounded_at_max() {
        let mut engine = CondenserEngine::new();
        let input = "line1\nline2\nline3\n".repeat(10);
        for _ in 0..(MAX_HISTORY + 10) {
            engine.compress("bash_execute", &input, None);
        }
        assert_eq!(engine.history_len(), MAX_HISTORY);
    }

    #[test]
    fn recommend_algorithm_none_with_insufficient_data() {
        let mut engine = CondenserEngine::new();
        let input = "line1\nline2\nline3\n".repeat(10);
        for _ in 0..5 {
            engine.compress("bash_execute", &input, None);
        }
        assert_eq!(
            engine.recommend_algorithm(ContextCategory::ShellCommand),
            None,
            "should return None with fewer than MIN_OBSERVATIONS"
        );
    }

    #[test]
    fn recommend_algorithm_returns_best_after_sufficient_data() {
        let mut engine = CondenserEngine::new();
        let input = "line1\nline2\nline3\nline4\nline5\n".repeat(20);
        for _ in 0..15 {
            engine.compress("bash_execute", &input, None);
        }
        let recommended = engine.recommend_algorithm(ContextCategory::ShellCommand);
        assert!(
            recommended.is_some(),
            "should return Some after 15 compressions"
        );
        assert_eq!(recommended.unwrap(), "rtk_style");
    }

    #[test]
    fn compression_stats_empty_for_fresh_engine() {
        let engine = CondenserEngine::new();
        let stats = engine.compression_stats();
        assert_eq!(stats.total_records, 0);
        assert!(stats.by_algorithm.is_empty());
        assert!(stats.by_category.is_empty());
    }

    #[test]
    fn compression_stats_after_compress() {
        let mut engine = CondenserEngine::new();
        let input = "line1\nline2\nline3\nline4\nline5\n".repeat(20);
        for _ in 0..3 {
            engine.compress("bash_execute", &input, None);
        }
        let stats = engine.compression_stats();
        assert_eq!(stats.total_records, 3);
        assert!(stats.by_algorithm.contains_key("rtk_style"));
        assert!(stats.by_category.contains_key("shell_command"));
        let algo_stats = &stats.by_algorithm["rtk_style"];
        assert_eq!(algo_stats.count, 3);
        assert!(algo_stats.mean_ratio > 0.0);
    }

    #[test]
    fn suggest_profile_returns_current_when_healthy() {
        let engine = CondenserEngine::new();
        assert_eq!(engine.suggest_profile(), Profile::Normal);
    }

    #[test]
    fn suggest_profile_returns_heavy_when_degraded() {
        let mut engine = CondenserEngine::new();
        for _ in 0..15 {
            engine.compress("bash_execute", "ab", None);
        }
        assert_eq!(engine.suggest_profile(), Profile::Heavy);
    }

    #[test]
    fn profile_getter_returns_current() {
        let mut engine = CondenserEngine::new();
        assert_eq!(engine.profile(), Profile::Normal);
        engine.set_profile(Profile::Heavy);
        assert_eq!(engine.profile(), Profile::Heavy);
    }
}
