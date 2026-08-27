use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput, deserialize_maybe_stringified};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

/// Render a Jinja2 template from the kask registry with context variables.
///
/// This tool provides structured prompt scaffolding for skill processes.
/// The SKILL.md body tells you when to call it — e.g., "call `render_template`
/// with template_ref `essentialist/essentialist-flow` to get the structured
/// prompt for the 3-gate loop."
///
/// Templates live in `kask/registry/templates/<skill>/<file>.j2`. The tool
/// strips YAML frontmatter (the `---`-delimited header containing the contract
/// schema and `[inference]` parameters) and renders only the Jinja2 body.
///
/// The rendered text is a structured prompt — use it as guidance for your
/// next reasoning step, not as a final answer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenderTemplateToolInput {
    /// Template reference, e.g. "essentialist/essentialist-flow". The tool
    /// resolves this against the registry templates directory, trying the
    /// ref as-is, then with `.j2` appended, then with `.yaml` appended.
    pub template_ref: String,
    /// Context variables for Jinja2 interpolation. Keys become template
    /// variables (e.g., `{{ task }}`, `{{ artifact }}`). Values must be
    /// JSON-serializable.
    ///
    /// Uses `AnyJsonValue` for values (not `serde_json::Value`) because
    /// `schemars` renders `Value` as bare `true` in `additionalProperties`,
    /// which breaks strict-schema providers — context variables silently
    /// don't arrive.
    ///
    /// `deserialize_maybe_stringified` tolerates models that emit `context` as
    /// a stringified JSON string instead of a bare object — the same pattern
    /// `edit_file.edits` uses.
    #[serde(default, deserialize_with = "deserialize_maybe_stringified")]
    pub context: std::collections::HashMap<String, hkask_types::AnyJsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RenderTemplateToolOutput {
    Rendered { text: String },
    Error { error: String },
}

impl From<RenderTemplateToolOutput> for LanguageModelToolResultContent {
    fn from(value: RenderTemplateToolOutput) -> Self {
        match value {
            RenderTemplateToolOutput::Rendered { text } => text.into(),
            RenderTemplateToolOutput::Error { error } => error.into(),
        }
    }
}

pub struct RenderTemplateTool;

impl AgentTool for RenderTemplateTool {
    type Input = RenderTemplateToolInput;
    type Output = RenderTemplateToolOutput;

    const NAME: &'static str = "render_template";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("Rendering: {}", input.template_ref).into(),
            Err(_) => "Render Template".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_cx| {
            let input = input.recv().await.map_err(|e| {
                RenderTemplateToolOutput::Error {
                    error: format!("failed to receive input: {e}"),
                }
            })?;

            let base_path = crate::template_base_path().ok_or_else(|| {
                RenderTemplateToolOutput::Error {
                    error: "Template base path not configured. The registry templates directory is wired at startup."
                        .to_string(),
                }
            })?;

            // Resolve the template file, trying ref as-is, .j2, then .yaml.
            let content = read_template_file(&base_path, &input.template_ref).map_err(|e| {
                RenderTemplateToolOutput::Error { error: e }
            })?;

            // Strip YAML frontmatter (--- delimited header).
            let template_body = strip_frontmatter(&content);

            // Build minijinja environment and render.
            let env = minijinja::Environment::new();
            // Add a loader for inline template — we render the body directly.
            let result = env.render_str(&template_body, &input.context).map_err(|e| {
                RenderTemplateToolOutput::Error {
                    error: format!("Template rendering failed: {e}"),
                }
            })?;

            Ok(RenderTemplateToolOutput::Rendered { text: result })
        })
    }
}

/// Read a template file from the registry, trying ref as-is, .j2, then .yaml.
/// Prevents path traversal outside the base directory.
///
/// `resolve_template_path` returns `None` for two reasons: the joined path
/// escapes the base directory (traversal blocked), or the file doesn't exist
/// (`canonicalize` fails). Both are safe to fall through from — the `.j2`/`.yaml`
/// retries are checked against the same base path, so traversal stays blocked.
/// We only error after all three attempts fail.
fn read_template_file(base_path: &std::path::Path, template_ref: &str) -> Result<String, String> {
    // Try ref as-is. `None` means either traversal blocked or file absent —
    // fall through to the extension retries rather than erroring immediately.
    if let Some(resolved) = resolve_template_path(base_path, template_ref) {
        if let Ok(content) = std::fs::read_to_string(&resolved) {
            return Ok(content);
        }
    }

    // Try .j2 extension.
    if !template_ref.ends_with(".j2") {
        let j2_ref = format!("{template_ref}.j2");
        if let Some(j2_path) = resolve_template_path(base_path, &j2_ref) {
            if let Ok(content) = std::fs::read_to_string(&j2_path) {
                return Ok(content);
            }
        }
    }

    // Try .yaml extension.
    if !template_ref.ends_with(".yaml") {
        let yaml_ref = format!("{template_ref}.yaml");
        if let Some(yaml_path) = resolve_template_path(base_path, &yaml_ref) {
            if let Ok(content) = std::fs::read_to_string(&yaml_path) {
                return Ok(content);
            }
        }
    }

    Err(format!(
        "Template not found: tried '{template_ref}', '{template_ref}.j2', '{template_ref}.yaml' under '{}'",
        base_path.display()
    ))
}

/// Safely join a template ref to the base path, rejecting path traversal.
fn resolve_template_path(
    base_path: &std::path::Path,
    template_ref: &str,
) -> Option<std::path::PathBuf> {
    let joined = base_path.join(template_ref);
    let canonical_base = base_path.canonicalize().ok()?;
    let canonical_joined = joined.canonicalize().ok()?;
    if canonical_joined.starts_with(&canonical_base) {
        Some(canonical_joined)
    } else {
        None
    }
}

/// Strip the template metadata header and inference-param stanzas from a
/// template file, leaving only the renderable prompt body.
///
/// The on-disk convention (219 of 315 templates) is:
///
/// ```text
/// [inference]
/// contract: …
/// visibility: Public
/// ---
/// <body>
/// ```
///
/// i.e. an `[inference]`-keyed header (contract schema + visibility) that is
/// NOT YAML-frontmatter and therefore not delimited by leading `---` — the
/// terminator is a lone `---` line *after* the header. The old stripper only
/// fired on a leading `---`, which matched 0 of 309 templates, so the header
/// leaked verbatim into every rendered prompt.
///
/// Two stanzas are stripped:
/// 1. **Header** — everything from a leading `[inference]` line through the
///    first lone `---` line. Templates that still use legacy leading-`---`
///    frontmatter keep working (same rule, different opener).
/// 2. **Body param stanza** — a second `[inference]` block at the top of the
///    body (temperature/work_effort/verbosity/thinking_budget render params).
///    These are tool-execution metadata, not prompt text; minijinja would
///    otherwise emit them verbatim.
///
/// A template with neither convention passes through unchanged.
fn strip_frontmatter(content: &str) -> String {
    let mut working = content;

    // ── Stanza 1: the header ─────────────────────────────────────
    let first_line = working.lines().next().unwrap_or("").trim();
    if first_line == "[inference]" {
        // Find the terminating lone `---` line and take everything after it.
        if let Some(pos) = working.find('\n---\n') {
            working = &working[pos + 1..];
            working = working
                .strip_prefix("---\n")
                .unwrap_or(working);
        }
    } else if working.starts_with("---") {
        // Legacy YAML frontmatter: everything after the second `---`.
        if let Some(after) = working.splitn(3, "---").nth(2) {
            working = after;
        }
    }

    // ── Stanza 2: a body-leading [inference] param block ──────────
    // Runs from the `[inference]` line through the first blank line. Only
    // stripped at the very start of the body — an `[inference]` mention in
    // running prose is left alone.
    let trimmed = working.trim_start_matches('\n');
    if trimmed.starts_with("[inference]") {
        let mut stanza_end = 0usize;
        for (idx, line) in trimmed.lines().enumerate() {
            if idx > 0 && line.trim().is_empty() {
                stanza_end = idx;
                break;
            }
            stanza_end = idx + 1;
        }
        working = &trimmed[trimmed
            .lines()
            .take(stanza_end)
            .map(|l| l.len() + 1)
            .sum::<usize>()
            .min(trimmed.len())..];
    }

    working.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter_removes_yaml_header() {
        let input = "---\ntemplate_type: KnowAct\ncontract:\n  input: {}\n---\nHello {{ name }}!";
        let result = strip_frontmatter(input);
        assert_eq!(result, "Hello {{ name }}!");
    }

    #[test]
    fn test_strip_frontmatter_preserves_content_without_header() {
        let input = "Hello {{ name }}!";
        let result = strip_frontmatter(input);
        assert_eq!(result, "Hello {{ name }}!");
    }

    #[test]
    fn test_strip_frontmatter_handles_empty_body() {
        let input = "---\nfoo: bar\n---\n";
        let result = strip_frontmatter(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_resolve_template_path_rejects_traversal() {
        let base = std::path::PathBuf::from("kask/registry/templates");
        if !base.is_dir() {
            return;
        }
        let result = resolve_template_path(&base, "../../etc/passwd");
        assert!(result.is_none(), "path traversal must be rejected");
    }

    #[test]
    fn test_resolve_template_path_accepts_valid_ref() {
        let base = std::path::PathBuf::from("kask/registry/templates");
        if !base.is_dir() {
            return;
        }
        let result = resolve_template_path(&base, "essentialist/essentialist-flow.j2");
        assert!(result.is_some(), "valid template ref must resolve");
    }

    #[test]
    fn test_read_template_file_finds_j2() {
        let base = std::path::PathBuf::from("kask/registry/templates");
        if !base.is_dir() {
            return;
        }
        let result = read_template_file(&base, "essentialist/essentialist-flow");
        assert!(
            result.is_ok(),
            "should find .j2 file with extension-less ref"
        );
        let content = result.expect("checked is_ok above");
        assert!(content.contains("---"), "template should have frontmatter");
    }

    // Regression for the canonicalize-before-existence-check bug: an
    // extensionless ref whose exact path doesn't exist (the .j2 file does)
    // must resolve via the .j2 retry, not error out as "escapes base path".
    #[test]
    fn test_read_template_file_finds_j2_with_extensionless_ref() {
        let base = std::path::PathBuf::from("kask/registry/templates");
        if !base.is_dir() {
            return; // skip in CI without the source tree
        }
        // This is the exact ref that failed during the prompt-enhance run.
        let result = read_template_file(&base, "prompt-enhance/enhance-classify");
        assert!(
            result.is_ok(),
            "extensionless ref should resolve to .j2 file, got: {:?}",
            result.err()
        );
        let content = result.expect("checked is_ok above");
        assert!(
            content.contains("---"),
            "enhance-classify.j2 should have frontmatter"
        );
    }

    // Regression: when the model emits `context` as a stringified JSON string
    // instead of a bare object, `deserialize_maybe_stringified` parses the
    // string and the tool succeeds. Same pattern as `edit_file.edits`.
    #[test]
    fn test_context_accepts_stringified_json() {
        let input =
            serde_json::json!({"template_ref": "essentialist/essentialist-flow", "context": "{}"});
        let result: RenderTemplateToolInput =
            serde_json::from_value(input).expect("stringified context must be accepted");
        assert!(
            result.context.is_empty(),
            "stringified empty object must parse to empty map"
        );
    }

    // Positive path: a bare object must still work.
    #[test]
    fn test_context_accepts_bare_object() {
        let input = serde_json::json!({"template_ref": "essentialist/essentialist-flow", "context": {"task": "simplify"}});
        let result: RenderTemplateToolInput =
            serde_json::from_value(input).expect("bare object must parse");
        assert_eq!(result.context.len(), 1);
        assert!(result.context.contains_key("task"));
    }
}
