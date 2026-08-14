//! Pipeline tool — executes hKask pipeline manifests (category: pipeline)
//! via the `SkillManifestExecutor::execute_pipeline` method.
//!
//! This is the agent-facing entry point for pipeline manifests. Unlike
//! `SkillTool` (which resolves skills by name from the registry), this tool
//! takes an explicit manifest file path and calls `execute_pipeline` on the
//! bridge's `SkillManifestExecutor`. The bridge loads the manifest, verifies
//! it's `category: pipeline` (not a skill), and runs the `ManifestExecutor`
//! cascade with the `ToolPort` — giving the pipeline access to all MCP tools
//! across all servers (corpus, training, etc.).
//!
//! The tool supports `resume_from` (skip steps before the named step id) and
//! `dry_run` (parse + validate without executing).

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{App, SharedString, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::skill_tool::{CascadeProgress, SkillManifestExecutor};
use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Input for the pipeline tool.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PipelineToolInput {
    /// Project-root-relative path to the pipeline manifest YAML file.
    /// Example: "kask/corpus/pipeline-capabilities-researcher.yaml"
    pub manifest_path: String,
    /// Optional step `id` to resume from. Skips all steps before this one.
    /// Use this to resume after a gate failure or interruption.
    #[serde(default)]
    pub resume_from: Option<String>,
    /// If true, parse and validate the manifest without executing any steps.
    /// Use this to check the manifest is well-formed before running.
    #[serde(default)]
    pub dry_run: bool,
}

/// Output for the pipeline tool.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PipelineToolOutput {
    /// The pipeline completed (or dry-run validated) successfully.
    Ok { result: String },
    /// The pipeline failed — gate failure, tool error, or parse error.
    Error { error: String },
}

impl From<PipelineToolOutput> for language_model::LanguageModelToolResultContent {
    fn from(output: PipelineToolOutput) -> Self {
        match output {
            PipelineToolOutput::Ok { result } => {
                language_model::LanguageModelToolResultContent::Text(result.into())
            }
            PipelineToolOutput::Error { error } => {
                language_model::LanguageModelToolResultContent::Text(error.into())
            }
        }
    }
}

/// The pipeline tool. Resolves the `SkillManifestExecutor` at invocation time
/// (same pattern as `SkillTool`) to avoid the session-creation race.
pub struct PipelineTool {
    manifest_executor_resolver:
        Arc<dyn Fn() -> Option<Arc<dyn SkillManifestExecutor>> + Send + Sync>,
}

impl PipelineTool {
    /// Construct a `PipelineTool` whose manifest executor is resolved at
    /// invocation time by calling `resolver`. This mirrors `SkillTool`'s
    /// production constructor.
    pub fn with_manifest_executor_resolver<R>(resolver: R) -> Self
    where
        R: Fn() -> Option<Arc<dyn SkillManifestExecutor>> + Send + Sync + 'static,
    {
        Self {
            manifest_executor_resolver: Arc::new(resolver),
        }
    }
}

impl AgentTool for PipelineTool {
    type Input = PipelineToolInput;
    type Output = PipelineToolOutput;

    const NAME: &'static str = "run_pipeline";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        if let Ok(input) = input {
            if input.dry_run {
                "Pipeline (dry run)".into()
            } else if let Some(ref resume) = input.resume_from {
                format!("Pipeline (resume from {})", resume).into()
            } else {
                "Pipeline".into()
            }
        } else {
            "Pipeline".into()
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_cx| {
            let input = input.recv().await.map_err(|e| PipelineToolOutput::Error {
                error: e.to_string(),
            })?;

            let executor =
                (self.manifest_executor_resolver)().ok_or_else(|| PipelineToolOutput::Error {
                    error: "Pipeline manifest executor not configured. \
                            The hKask bridge must be wired before pipeline manifests \
                            can be executed."
                        .to_string(),
                })?;

            let progress: Option<CascadeProgress> = Some(event_stream.thinking_sender());
            let title: Option<CascadeProgress> = Some(event_stream.title_sender());

            match executor
                .execute_pipeline(
                    &input.manifest_path,
                    input.resume_from,
                    input.dry_run,
                    progress,
                    title,
                )
                .await
            {
                Ok(result_text) => Ok(PipelineToolOutput::Ok {
                    result: result_text,
                }),
                Err(e) => Err(PipelineToolOutput::Error {
                    error: format!("Pipeline execution failed: {e}"),
                }),
            }
        })
    }
}
