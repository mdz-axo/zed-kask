//! Thread condenser — ingestion compression and manual-compaction precompression
//! over the existing local algorithms (D8).
//!
//! The `BridgeThreadCondenser` implements the `agent::ThreadCondenser` trait
//! by delegating to a `hkask_condenser::CondenserEngine`. It:
//!
//! 1. Checks if auto-compression is enabled (`KaskCondenserSettings.auto_compress_tool_results`).
//! 2. Calls `CondenserEngine::compress(tool_name, output, None)` to compress
//!    the tool output using the configured profile and algorithm selection.
//! 3. Returns the compressed text (or the original if compression is disabled
//!    or the output is already within budget).
//!
//! The condenser is wired in the composition root via `agent::set_thread_condenser`.
//! It is called from the tool-result handling path in `run_turn_internal`.

use agent::ThreadCondenser;
use anyhow::{Result, anyhow};
use hkask_condenser::engine::CondenserEngine;
use hkask_condenser::types::Profile;
use language_model::{
    LanguageModelRequestMessage, LanguageModelToolResultContent, MessageContent, Role,
};
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

    fn precompress_history(
        &self,
        messages: &mut [LanguageModelRequestMessage],
        protected_tools: &[&str],
    ) -> Result<()> {
        let protected_start = messages
            .iter()
            .rposition(|message| {
                message.role == Role::User
                    && message
                        .content
                        .iter()
                        .any(|part| !matches!(part, MessageContent::ToolResult(_)))
            })
            .unwrap_or(0);
        let mut engine = self
            .engine
            .lock()
            .map_err(|error| anyhow!("Kask precompression unavailable: {error}"))?;
        for message in messages.iter_mut().take(protected_start) {
            for part in &mut message.content {
                let MessageContent::ToolResult(result) = part else {
                    continue;
                };
                if result.is_error || protected_tools.contains(&result.tool_name.as_ref()) {
                    continue;
                }
                for part in &mut result.content {
                    let LanguageModelToolResultContent::Text(text) = part else {
                        continue;
                    };
                    // JSON must remain parseable, not become a set of disconnected lines.
                    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                        continue;
                    }
                    let compressed = engine.compress(&result.tool_name, text, None);
                    if compressed.content.trim().is_empty() {
                        continue;
                    }
                    let excerpt = format!(
                        "[Kask {} excerpt; full tool output remains in the original thread]\n{}",
                        compressed.algorithm, compressed.content
                    );
                    if excerpt.len() < text.len() {
                        *text = excerpt.into();
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual precompression reduces expendable output, not instructions or structure.
    #[test]
    fn history_precompression_preserves_protected_content_and_routes_algorithms() -> Result<()> {
        use language_model::LanguageModelToolResult;
        let user = |text: &str| LanguageModelRequestMessage {
            role: Role::User,
            content: vec![text.into()],
            cache: false,
            reasoning_details: None,
        };
        let tool = |name: &str, text: &str, is_error| LanguageModelRequestMessage {
            role: Role::User,
            content: vec![MessageContent::ToolResult(LanguageModelToolResult {
                tool_use_id: name.into(),
                tool_name: name.into(),
                is_error,
                content: vec![text.to_string().into()],
                output: Some(serde_json::json!({"original": text})),
            })],
            cache: true,
            reasoning_details: None,
        };
        let output = "repeated progress: build is processing a compilation unit\n".repeat(300);
        let mut assistant = user("Decision: keep the public API unchanged.");
        assistant.role = Role::Assistant;
        assistant.reasoning_details = Some(std::sync::Arc::new(
            serde_json::json!([{"signature": "keep"}]),
        ));
        let mut history = vec![
            user("Never remove the authentication check."),
            assistant,
            tool("terminal", &output, false),
            tool("conversation_history", &output, false),
            tool("web_fetch", &output, false),
            tool("read_file", &output, false),
            tool(
                "terminal",
                &serde_json::to_string_pretty(&vec!["entry"; 300])?,
                false,
            ),
            tool("terminal", &output, true),
            tool("terminal", "short output", false),
            user("Correction: preserve the error type too."),
            tool("terminal", &output, false),
        ];
        let mut expected = history.clone();
        let condenser = BridgeThreadCondenser::new("normal", false);
        assert_eq!(condenser.compress_tool_result("terminal", &output), output);
        condenser.precompress_history(&mut history, &["read_file"])?;
        for (index, algorithm) in [(2, "rtk_style"), (3, "word_rank"), (4, "flashrank")] {
            let changed = history
                .get(index)
                .expect("fixture message")
                .string_contents();
            assert!(changed.len() < output.len());
            assert!(changed.contains(algorithm));
            assert!(changed.contains("full tool output remains in the original thread"));
            let Some(MessageContent::ToolResult(result)) =
                expected.get_mut(index).and_then(|m| m.content.first_mut())
            else {
                panic!("expected tool result")
            };
            result.content = vec![changed.into()];
        }
        // Includes role/order, call IDs, cache flags, reasoning, debug output,
        // valid JSON, failed results, all prose, and the latest exchange.
        assert_eq!(history, expected);
        Ok(())
    }

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
