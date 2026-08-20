use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use fs::Fs;
use gpui::{App, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ui::SharedString;

/// Render a Jinja2 template file with provided context variables.
///
/// This tool reads a `.j2` template file from disk, renders it with Jinja2
/// (minijinja) using the provided context, and returns the rendered text.
/// Use it when a skill's SKILL.md instructs you to load a template for
/// structured prompt generation.
///
/// The template path resolves relative to the project root. Templates inside
/// a skill directory (e.g. `.agents/skills/my-skill/analyze.j2`) can be
/// referenced directly.
///
/// Jinja2 syntax is fully supported: `{{ variable }}`, `{% if %}`,
/// `{% for %}`, filters, macros, etc.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TemplateRenderToolInput {
    /// Path to the `.j2` template file, relative to the project root
    /// (e.g. `.agents/skills/my-skill/analyze.j2`).
    template_path: String,
    /// JSON object whose keys become Jinja2 template variables.
    /// e.g. `{"task": "analyze X", "step_1_result": {...}}` makes
    /// `{{ task }}` and `{{ step_1_result }}` available in the template.
    #[serde(default)]
    context: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TemplateRenderToolOutput {
    Success { rendered: String },
    Error { error: String },
}

impl From<TemplateRenderToolOutput> for LanguageModelToolResultContent {
    fn from(value: TemplateRenderToolOutput) -> Self {
        match value {
            TemplateRenderToolOutput::Success { rendered } => rendered.into(),
            TemplateRenderToolOutput::Error { error } => error.into(),
        }
    }
}

pub struct TemplateRenderTool {
    fs: Arc<dyn Fs>,
}

impl TemplateRenderTool {
    pub fn new(fs: Arc<dyn Fs>) -> Self {
        Self { fs }
    }
}

impl AgentTool for TemplateRenderTool {
    type Input = TemplateRenderToolInput;
    type Output = TemplateRenderToolOutput;

    const NAME: &'static str = "template_render";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => {
                let name = input
                    .template_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&input.template_path);
                format!("Rendering {name}").into()
            }
            Err(_) => "Template Render".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        _cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let fs = self.fs.clone();
        _cx.spawn(async move |_cx| {
            let input = input
                .recv()
                .await
                .map_err(|e| TemplateRenderToolOutput::Error {
                    error: format!("failed to receive input: {e}"),
                })?;

            // Read the template file from disk.
            let template_content = fs.load(input.template_path.as_ref()).await.map_err(|e| {
                TemplateRenderToolOutput::Error {
                    error: format!("failed to read template {}: {e}", input.template_path),
                }
            })?;

            let template_str = String::from_utf8(template_content).map_err(|e| {
                TemplateRenderToolOutput::Error {
                    error: format!("template is not valid UTF-8: {e}"),
                }
            })?;

            // Render with minijinja.
            let mut env = minijinja::Environment::new();
            env.add_template("template", &template_str).map_err(|e| {
                TemplateRenderToolOutput::Error {
                    error: format!("template parse error: {e}"),
                }
            })?;

            let tmpl =
                env.get_template("template")
                    .map_err(|e| TemplateRenderToolOutput::Error {
                        error: format!("template lookup error: {e}"),
                    })?;

            let context = if input.context.is_null() {
                minijinja::Value::from_serialize(serde_json::Map::new())
            } else {
                minijinja::Value::from_serialize(&input.context)
            };

            let rendered = tmpl
                .render(context)
                .map_err(|e| TemplateRenderToolOutput::Error {
                    error: format!("template render error: {e}"),
                })?;

            Ok(TemplateRenderToolOutput::Success { rendered })
        })
    }
}
