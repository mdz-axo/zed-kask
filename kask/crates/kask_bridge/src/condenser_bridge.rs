//! Thread condenser — ingestion compression over the existing local
//! algorithms (D8).
//!
//! `BridgeThreadCondenser` delegates to `CondenserEngine` for tool-result
//! compression at ingestion (`run_tool`), honoring `auto_compress_tool_results`.
//! Compaction planning no longer routes through this bridge: the
//! deterministic pre-shrink (run-length collapse + head+tail windowing) is
//! owned by `kask_compaction` and runs on the summarizer's request copy for
//! both manual and automatic compaction. The composition root installs the
//! hook via `agent::set_thread_condenser`.

use agent::ThreadCondenser;
use hkask_condenser::engine::CondenserEngine;
use hkask_condenser::types::Profile;
use std::sync::Mutex;

/// Bridge thread condenser — wraps `CondenserEngine` for use in zed's agent threads.
pub struct BridgeThreadCondenser {
    engine: Mutex<CondenserEngine>,
    auto_compress: bool,
}

impl BridgeThreadCondenser {
    /// Construct a new thread condenser.
    ///
    /// Creates a `CondenserEngine` with the specified profile and configures
    /// auto-compression based on the settings.
    pub fn new(profile: &str, auto_compress: bool) -> Self {
        let mut engine = CondenserEngine::new();
        if let Ok(profile) = profile.parse::<Profile>() {
            engine.set_profile(profile);
        }
        Self {
            engine: Mutex::new(engine),
            auto_compress,
        }
    }
}

impl ThreadCondenser for BridgeThreadCondenser {
    fn compress_tool_result(&self, tool_name: &str, output: &str) -> String {
        if !self.auto_compress || output.is_empty() {
            return output.to_string();
        }

        let mut engine = match self.engine.lock() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.condenser",
                    error = %e,
                    "CondenserEngine lock poisoned — returning uncompressed output"
                );
                return output.to_string();
            }
        };

        let result = engine.compress(tool_name, output, None);

        tracing::debug!(
            target: "hkask.condenser",
            tool = %tool_name,
            original_bytes = result.original_bytes,
            compressed_bytes = result.compressed_bytes,
            reduction_pct = result.reduction_pct,
            algorithm = %result.algorithm,
            "Tool result compressed"
        );

        result.content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_tool_result_returns_compressed_text() {
        let condenser = BridgeThreadCondenser::new("normal", true);
        let output = "line one\nline two\nline three\nline four\nline five\nline six\nline seven\nline eight\nline nine\nline ten\n";
        let compressed = condenser.compress_tool_result("test_tool", output);
        // With "normal" profile (20% retention, 80 max lines), 10 lines should
        // be compressed to ~2 lines.
        assert!(
            compressed.len() <= output.len(),
            "compressed should be <= original: {} vs {}",
            compressed.len(),
            output.len()
        );
    }

    #[test]
    fn compress_tool_result_passthrough_when_disabled() {
        let condenser = BridgeThreadCondenser::new("normal", false);
        let output = "some output text";
        let result = condenser.compress_tool_result("test_tool", output);
        assert_eq!(
            result, output,
            "should return original when auto_compress is false"
        );
    }

    #[test]
    fn compress_tool_result_passthrough_for_empty() {
        let condenser = BridgeThreadCondenser::new("normal", true);
        let result = condenser.compress_tool_result("test_tool", "");
        assert_eq!(result, "", "empty input should return empty");
    }

    #[test]
    fn compress_tool_result_with_heavy_profile() {
        let condenser = BridgeThreadCondenser::new("heavy", true);
        let output: String = (0..50)
            .map(|i| format!("line {i}: some content here\n"))
            .collect();
        let compressed = condenser.compress_tool_result("test_tool", &output);
        // Heavy profile: 10% retention, 30 max lines — should be significantly shorter.
        assert!(
            compressed.len() < output.len(),
            "heavy profile should compress: {} vs {}",
            compressed.len(),
            output.len()
        );
    }
}
