use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    #[serde(default)]
    pub context: std::collections::HashMap<String, Value>,
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
            let mut env = minijinja::Environment::new();
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
fn read_template_file(base_path: &std::path::Path, template_ref: &str) -> Result<String, String> {
    let resolved = resolve_template_path(base_path, template_ref).ok_or_else(|| {
        format!(
            "template_ref '{template_ref}' escapes base path '{}'",
            base_path.display()
        )
    })?;

    // Try the resolved path as-is.
    if let Ok(content) = std::fs::read_to_string(&resolved) {
        return Ok(content);
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
        "Template not found at {} (also tried .j2 and .yaml extensions)",
        resolved.display()
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

/// Strip YAML frontmatter from a template file. The frontmatter is delimited
/// by `---` at the start of the file. Everything between the first and second
/// `---` is the frontmatter; everything after the second `---` is the body.
fn strip_frontmatter(content: &str) -> String {
    if content.starts_with("---") {
        content
            .splitn(3, "---")
            .nth(2)
            .unwrap_or(content)
            .trim()
            .to_string()
    } else {
        content.to_string()
    }
}
