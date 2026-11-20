//! Declarative model-native harness shaping for local open-weight models
//! (Study 2 / C1, experiment `exp_3a92b16072684529b484c5ccd7f78b0e`,
//! reference model: Open Interpreter `codex-rs/core` `/harness` profiles).
//!
//! Decouples model-trained prompt/tool-schema dialects (`qwen-code`,
//! `deepseek-tui`, `minimal`, `compact-local`) from `zed-kask`'s native Rust
//! tool runtime without altering frontier cloud models (`full`).

/// Context-window ceiling below which a local model receives the ultra-compact
/// D107 tool & prompt budget (6 core tools, no cloud affordance injection).
pub const COMPACT_PROFILE_MAX_CONTEXT: u64 = 16_384;

/// Declarative harness profile resolved for a `(provider, model, context)` tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessProfile {
    /// Standard full Zed-Kask prompt (`system_prompt.hbs`) for frontier cloud models.
    Full,
    /// D107 ultra-compact prompt (`compact_system_prompt.hbs`) for small-context
    /// (`< COMPACT_PROFILE_MAX_CONTEXT`) local models.
    CompactLocal,
    /// Model-native concise coding harness for local Qwen-family models
    /// (`qwen3`, `qwen3.5`, `qwen2.5`, `ornith`, `bonsai`) on Ollama / llama.cpp.
    QwenCode,
    /// Model-native concise coding harness for local DeepSeek models on Ollama / llama.cpp.
    DeepSeekTui,
    /// Concise model-native coding harness for other local open-weight models.
    Minimal,
}

/// Resolved harness specification consumed by `Thread::render_system_prompt`
/// and `Thread::enabled_tools`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessProfileSpec {
    pub profile: HarnessProfile,
    /// Stable wire/log identifier (e.g. `"full"`, `"compact-local"`, `"qwen-code"`).
    pub id: &'static str,
    /// Whether the model stays on the D107 6-tool compact surface.
    pub uses_compact_tool_surface: bool,
    /// Optional concise model-family-native tool-calling & recovery rules
    /// rendered into the local harness template.
    pub dialect_rules: Option<&'static str>,
}

const QWEN_CODE_DIALECT_RULES: &str = "\
- Emit a structured tool call directly whenever the user asks to inspect, search, edit, create, or run a command; never print pseudo tool calls in markdown code blocks.
- Strictly match JSON argument types: integers as numbers (`start_line`, `end_line`, `timeout_ms`), `edit_file.edits` as an array of `{\"old_text\":\"...\",\"new_text\":\"...\"}` objects.
- For `grep`, put only the regex pattern in `regex` and file path globs in `include_pattern`.
- After an `edit_file` mismatch error, call `read_file` on that file before editing again.";

const DEEPSEEK_TUI_DIALECT_RULES: &str = "\
- Call the exact matching tool immediately with valid JSON arguments when the task requires repo inspection, editing, or command execution; keep reasoning brief.
- Never put file paths inside `grep.regex`; put the regex pattern in `regex` and the path glob in `include_pattern`.
- Never retry a failed `edit_file` blindly: call `read_file` on the file first to verify exact lines.
- Pass integer `timeout_ms` for `terminal` and avoid shell substitutions.";

const MINIMAL_LOCAL_DIALECT_RULES: &str = "\
- Emit structured tool calls directly with valid JSON arguments matching each tool schema; never wrap tool calls in markdown code blocks.
- After an `edit_file` mismatch error, call `read_file` on the target file before retrying.";

impl HarnessProfileSpec {
    pub const fn full() -> Self {
        Self {
            profile: HarnessProfile::Full,
            id: "full",
            uses_compact_tool_surface: false,
            dialect_rules: None,
        }
    }

    pub const fn compact_local() -> Self {
        Self {
            profile: HarnessProfile::CompactLocal,
            id: "compact-local",
            uses_compact_tool_surface: true,
            dialect_rules: None,
        }
    }

    pub const fn qwen_code() -> Self {
        Self {
            profile: HarnessProfile::QwenCode,
            id: "qwen-code",
            uses_compact_tool_surface: false,
            dialect_rules: Some(QWEN_CODE_DIALECT_RULES),
        }
    }

    pub const fn deepseek_tui() -> Self {
        Self {
            profile: HarnessProfile::DeepSeekTui,
            id: "deepseek-tui",
            uses_compact_tool_surface: false,
            dialect_rules: Some(DEEPSEEK_TUI_DIALECT_RULES),
        }
    }

    pub const fn minimal() -> Self {
        Self {
            profile: HarnessProfile::Minimal,
            id: "minimal",
            uses_compact_tool_surface: false,
            dialect_rules: Some(MINIMAL_LOCAL_DIALECT_RULES),
        }
    }

    /// True when this profile uses the concise local harness template rather
    /// than the full ~18 KB frontier system prompt.
    pub const fn uses_local_harness_template(&self) -> bool {
        !matches!(self.profile, HarnessProfile::Full)
    }
}

/// Resolve the declarative harness profile for a model.
///
/// Frontier/cloud providers always receive [`HarnessProfile::Full`].
/// Local models (`is_local_model == true`, i.e. local Ollama / llama.cpp
/// excluding Ollama `:cloud` proxies) below [`COMPACT_PROFILE_MAX_CONTEXT`]
/// receive [`HarnessProfile::CompactLocal`]; larger-context local models
/// receive their model-family-native harness (`qwen-code`, `deepseek-tui`, or
/// `minimal`), preventing the ~18 KB frontier prompt from suppressing
/// structured tool calls (measured in `exp_3a92b16072684529b484c5ccd7f78b0e`:
/// `0.00` -> `0.8333` held-out task pass rate across `qwen3.5:9b` and `qwen3.5:4b`).
pub fn resolve_harness_profile(
    model_id: &str,
    max_token_count: u64,
    is_local_model: bool,
) -> HarnessProfileSpec {
    if !is_local_model {
        return HarnessProfileSpec::full();
    }
    if max_token_count < COMPACT_PROFILE_MAX_CONTEXT {
        return HarnessProfileSpec::compact_local();
    }

    let lower = model_id.to_ascii_lowercase();
    if lower.contains("qwen") || lower.contains("ornith") || lower.contains("bonsai") {
        HarnessProfileSpec::qwen_code()
    } else if lower.contains("deepseek") {
        HarnessProfileSpec::deepseek_tui()
    } else {
        HarnessProfileSpec::full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{CompactSystemPromptTemplate, Template, Templates};

    #[test]
    fn public_and_cloud_models_stay_on_full_profile() {
        let spec = resolve_harness_profile("qwen/qwen3.5-coder", 262_144, false);
        assert_eq!(spec.profile, HarnessProfile::Full);
        assert_eq!(spec.id, "full");
        assert!(!spec.uses_local_harness_template());
        assert!(!spec.uses_compact_tool_surface);
    }

    #[test]
    fn small_context_local_models_keep_d107_compact_local_profile() {
        let spec = resolve_harness_profile("qwen3:1.7b", 4_096, true);
        assert_eq!(spec.profile, HarnessProfile::CompactLocal);
        assert_eq!(spec.id, "compact-local");
        assert!(spec.uses_local_harness_template());
        assert!(spec.uses_compact_tool_surface);
        assert!(spec.dialect_rules.is_none());
    }

    #[test]
    fn large_context_local_models_route_to_model_native_harness_profiles() {
        let qwen = resolve_harness_profile("qwen3.5:9b", 262_144, true);
        assert_eq!(qwen.profile, HarnessProfile::QwenCode);
        assert_eq!(qwen.id, "qwen-code");
        assert!(qwen.uses_local_harness_template());
        assert!(!qwen.uses_compact_tool_surface);
        assert!(qwen.dialect_rules.is_some());

        let deepseek = resolve_harness_profile("deepseek-r1:8b", 131_072, true);
        assert_eq!(deepseek.profile, HarnessProfile::DeepSeekTui);
        assert_eq!(deepseek.id, "deepseek-tui");
        assert!(deepseek.uses_local_harness_template());
        assert!(!deepseek.uses_compact_tool_surface);

        let generic = resolve_harness_profile("llama3.3:70b", 131_072, true);
        assert_eq!(generic.profile, HarnessProfile::Full);
        assert_eq!(generic.id, "full");
        assert!(!generic.uses_local_harness_template());
    }

    #[test]
    fn qwen_code_harness_prompt_stays_compact_and_carries_dialect_rules() {
        let project = prompt_store::ProjectContext::default();
        let templates = Templates::new();
        let spec = resolve_harness_profile("qwen3.5:9b", 262_144, true);
        let rendered = CompactSystemPromptTemplate {
            project: &project,
            available_tools: vec!["read_file".into(), "edit_file".into()],
            provider_id: "ollama".to_string(),
            model_id: "qwen3.5:9b".to_string(),
            private_inference: true,
            date: "2026-11-18".to_string(),
            harness_profile_id: Some(spec.id),
            harness_dialect_rules: spec.dialect_rules,
        }
        .render(&templates)
        .expect("render qwen-code harness prompt");

        assert!(rendered.contains("Harness profile: qwen-code."));
        assert!(rendered.contains("never print pseudo tool calls in markdown code blocks"));
        assert!(rendered.contains("provider=ollama, model=qwen3.5:9b, private=true"));
        assert!(
            rendered.len() < 2_200,
            "qwen-code harness prompt grew to {} bytes (budget 2,200)",
            rendered.len()
        );
    }
}
