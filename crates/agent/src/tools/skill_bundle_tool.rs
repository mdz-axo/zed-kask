use agent_client_protocol::schema::v1 as acp;
use agent_skills::Skill;
use gpui::{App, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    AgentTool, BundleExecutionResult, ToolCallEventStream, ToolInput,
    tools::skill_tool::SkillManifestExecutor,
};

/// Compose and execute a bundle of multiple peer-level skills in a single
/// governed cascade. Use this when a task requires three or more skills that
/// are peers (not in a delegation relationship) — the bundler optimizes
/// ordering, resolves conflicts, and produces a single composed manifest
/// before execution. For single-skill activation, use the `skill` tool
/// instead.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SkillBundleToolInput {
    /// The peer-level skill names to compose into a bundle. Must be ≥3 skills
    /// for the bundler to engage (fewer should use the `skill` tool directly).
    pub skills: Vec<String>,
    /// The user's task for the bundle to act on. Injected into the
    /// skill-bundler cascade as `user_intent` and into the composed bundle's
    /// cascade as `task`.
    #[serde(default)]
    pub task: String,
    /// Extra context entries merged into the skill-bundler cascade context.
    #[serde(default)]
    pub context: std::collections::HashMap<String, serde_json::Value>,
}

/// The output of a skill bundle execution.
///
/// Carries the structured data the post-run UI needs for the
/// save/refine/discard affordance: the composed manifest (for `Save`),
/// the composition score (for `Refine`), and the output text (for display).
#[derive(Debug, Serialize, Deserialize)]
pub enum SkillBundleToolOutput {
    /// The bundle was composed and executed successfully.
    Executed {
        /// The final output text from the composed bundle's cascade.
        rendered: String,
        /// The composed `BundleManifest` as JSON. The post-run UI's `Save`
        /// action persists this to the bundle registry.
        bundle_manifest: serde_json::Value,
        /// The deterministic composition score from `lisp.eval` (lower = better).
        /// `None` if the score was unavailable (the named skills lacked
        /// convergence blocks or the bundler cascade didn't produce one).
        composition_score: Option<f64>,
        /// The skill names actually placed in the composed bundle (may differ
        /// from the input if the bundler dropped a skill via dead-letter
        /// resolution).
        composed_skill_names: Vec<String>,
        /// The goal-extract step's output (step_1_result from the bundler
        /// cascade). Carried so the `Refine` action can pass it to
        /// `bundler-evolve` as `goal_context`.
        goal_context: serde_json::Value,
    },
    Error {
        error: String,
    },
}

impl From<SkillBundleToolOutput> for LanguageModelToolResultContent {
    fn from(output: SkillBundleToolOutput) -> Self {
        match output {
            SkillBundleToolOutput::Executed {
                rendered,
                bundle_manifest,
                composition_score,
                composed_skill_names,
                goal_context: _,
            } => {
                // Wrap the output in a skill_content envelope (same format as
                // the single-skill tool) so the model receives a consistent
                // interface. The structured fields (manifest, score, skill names)
                // are appended as a JSON block the post-run UI can parse.
                let mut text = String::new();
                text.push_str("<skill_content name=\"skill-bundle\">\n");
                text.push_str("<source>bundle</source>\n");
                let _ = std::fmt::Write::write_fmt(
                    &mut text,
                    format_args!(
                        "<composed_skills>{}</composed_skills>\n",
                        composed_skill_names.join(", ")
                    ),
                );
                if let Some(score) = composition_score {
                    let _ = std::fmt::Write::write_fmt(
                        &mut text,
                        format_args!("<composition_score>{:.4}</composition_score>\n", score),
                    );
                }
                text.push_str(&rendered);
                text.push_str("\n<bundle_manifest>\n");
                text.push_str(&serde_json::to_string_pretty(&bundle_manifest).unwrap_or_default());
                text.push_str("\n</bundle_manifest>\n");
                text.push_str("</skill_content>\n");
                LanguageModelToolResultContent::Text(text.into())
            }
            SkillBundleToolOutput::Error { error } => {
                LanguageModelToolResultContent::Text(error.into())
            }
        }
    }
}

pub struct SkillBundleTool {
    skills: Arc<dyn Fn(&App) -> Arc<Vec<Skill>> + Send + Sync>,
    /// Resolver for the hKask ManifestExecutor, read at invocation time
    /// (same pattern as SkillTool — closes the session-creation race).
    manifest_executor_resolver:
        Arc<dyn Fn() -> Option<Arc<dyn SkillManifestExecutor>> + Send + Sync>,
}

impl SkillBundleTool {
    /// Construct a `SkillBundleTool` whose manifest executor is resolved at
    /// invocation time. This mirrors `SkillTool::with_manifest_executor_resolver`
    /// — the resolver reads the process-global `manifest_executor()` so
    /// sessions created before the deferred post-login task wires the
    /// executor pick it up on later invocations.
    pub fn with_manifest_executor_resolver<F, R>(skills: F, resolver: R) -> Self
    where
        F: Fn(&App) -> Arc<Vec<Skill>> + Send + Sync + 'static,
        R: Fn() -> Option<Arc<dyn SkillManifestExecutor>> + Send + Sync + 'static,
    {
        Self {
            skills: Arc::new(skills),
            manifest_executor_resolver: Arc::new(resolver),
        }
    }
}

impl AgentTool for SkillBundleTool {
    type Input = SkillBundleToolInput;
    type Output = SkillBundleToolOutput;

    const NAME: &'static str = "skill_bundle";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        if let Ok(input) = input {
            format!("Bundle of {} Skills", input.skills.len()).into()
        } else {
            "Skill Bundle".into()
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|e| SkillBundleToolOutput::Error {
                error: e.to_string(),
            })?;

            // Validate: at least 3 skills required for the bundler to engage.
            // The heuristic gate from the spec (criterion 1): ≥3 peer-level
            // skills triggers bundler-compose; fewer should use the `skill`
            // tool directly.
            if input.skills.len() < 3 {
                return Err(SkillBundleToolOutput::Error {
                    error: format!(
                        "skill_bundle requires at least 3 skills to compose, but {} were \
                         provided. For fewer skills, use the `skill` tool to invoke each \
                         skill individually.",
                        input.skills.len()
                    ),
                });
            }

            // Snapshot the current set of skills to verify all named skills
            // exist before composing. This fails fast with a clear error
            // instead of wasting tokens on a bundler cascade that will fail
            // mid-composition when a skill is missing.
            let snapshot = cx.update(|cx| (self.skills)(cx));
            let installed_names: std::collections::HashSet<&str> =
                snapshot.iter().map(|s| s.name.as_str()).collect();
            let missing: Vec<&str> = input
                .skills
                .iter()
                .filter(|name| !installed_names.contains(name.as_str()))
                .map(|s| s.as_str())
                .collect();
            if !missing.is_empty() {
                return Err(SkillBundleToolOutput::Error {
                    error: format!(
                        "Skills not found: {}. Available skills: {}",
                        missing.join(", "),
                        snapshot
                            .iter()
                            .filter(|s| !s.disable_model_invocation)
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }

            // Resolve the manifest executor at invocation time (same pattern
            // as SkillTool — closes the session-creation race).
            let Some(executor) = (self.manifest_executor_resolver)() else {
                return Err(SkillBundleToolOutput::Error {
                    error: "Skill manifest executor not configured. The skill_bundle tool \
                            requires the hKask ManifestExecutor to be wired (this happens in \
                            the deferred post-login task). Try again in a moment."
                        .to_string(),
                });
            };

            // Check that the skill-bundler manifest itself exists — without
            // it, composition can't run.
            if !executor.has_manifest("skill-bundler") {
                return Err(SkillBundleToolOutput::Error {
                    error: "The skill-bundler manifest is not registered. The skill_bundle \
                            tool requires the skill-bundler skill to be installed in the hKask \
                            registry (kask/registry/manifests/skill-bundler.yaml)."
                        .to_string(),
                });
            }

            // Compose and execute the bundle. The executor handles:
            // 1. Running the skill-bundler cascade (compose → synthesize →
            //    validate → lisp.eval score → evolve → loop)
            // 2. Extracting the composed BundleManifest
            // 3. Executing the composed manifest's cascade
            // 4. Returning the structured result (manifest, score, output)
            let result: BundleExecutionResult = executor
                .compose_and_execute_bundle(&input.skills, &input.task, input.context)
                .await
                .map_err(|e| SkillBundleToolOutput::Error {
                    error: format!("Skill bundle composition/execution failed: {e}"),
                })?;

            Ok(SkillBundleToolOutput::Executed {
                rendered: result.output,
                bundle_manifest: result.bundle_manifest,
                composition_score: result.composition_score,
                composed_skill_names: result.composed_skill_names,
                goal_context: result.goal_context,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_skills::{SkillSource, parse_skill_frontmatter};
    use gpui::TestAppContext;
    use serde_json::json;
    use settings::{Settings, SettingsStore};
    use std::path::Path;
    use std::sync::Arc;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
            let mut settings = agent_settings::AgentSettings::get_global(cx).clone();
            settings.tool_permissions.tools.insert(
                SkillBundleTool::NAME.into(),
                agent_settings::ToolRules {
                    default: Some(settings::ToolPermissionMode::Allow),
                    always_allow: vec![],
                    always_deny: vec![],
                    always_confirm: vec![],
                    invalid_patterns: vec![],
                },
            );
            agent_settings::AgentSettings::override_global(settings, cx);
        });
    }

    fn create_test_skill(name: &str) -> Skill {
        let skill_file_path = format!("/skills/{name}/SKILL.md");
        let content = format!("---\nname: {name}\ndescription: A test skill\n---\n\n# Body");
        parse_skill_frontmatter(Path::new(&skill_file_path), &content, SkillSource::Global).unwrap()
    }

    #[gpui::test]
    async fn test_skill_bundle_rejects_fewer_than_three_skills(cx: &mut TestAppContext) {
        init_test(cx);

        let skills = Arc::new(vec![create_test_skill("a"), create_test_skill("b")]);
        let tool = Arc::new(SkillBundleTool::with_manifest_executor_resolver(
            move |_cx| skills.clone(),
            || None,
        ));

        let (mut sender, input) = ToolInput::<SkillBundleToolInput>::test();
        sender.send_full(json!({
            "skills": ["a", "b"],
            "task": "do something"
        }));
        let (event_stream, _rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| tool.run(input, event_stream, cx));
        let result = task.await;

        let err = match result {
            Err(SkillBundleToolOutput::Error { error }) => error,
            other => panic!("expected Error for <3 skills, got: {other:?}"),
        };
        assert!(
            err.contains("at least 3 skills"),
            "error should mention the 3-skill gate: {err}"
        );
    }

    #[gpui::test]
    async fn test_skill_bundle_rejects_missing_skills(cx: &mut TestAppContext) {
        init_test(cx);

        let skills = Arc::new(vec![
            create_test_skill("a"),
            create_test_skill("b"),
            create_test_skill("c"),
        ]);
        let tool = Arc::new(SkillBundleTool::with_manifest_executor_resolver(
            move |_cx| skills.clone(),
            || None,
        ));

        let (mut sender, input) = ToolInput::<SkillBundleToolInput>::test();
        sender.send_full(json!({
            "skills": ["a", "b", "nonexistent"],
            "task": "do something"
        }));
        let (event_stream, _rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| tool.run(input, event_stream, cx));
        let result = task.await;

        let err = match result {
            Err(SkillBundleToolOutput::Error { error }) => error,
            other => panic!("expected Error for missing skill, got: {other:?}"),
        };
        assert!(
            err.contains("not found"),
            "error should mention missing skills: {err}"
        );
    }

    #[gpui::test]
    async fn test_skill_bundle_errors_when_executor_not_configured(cx: &mut TestAppContext) {
        init_test(cx);

        let skills = Arc::new(vec![
            create_test_skill("a"),
            create_test_skill("b"),
            create_test_skill("c"),
        ]);
        let tool = Arc::new(SkillBundleTool::with_manifest_executor_resolver(
            move |_cx| skills.clone(),
            || None, // no executor wired
        ));

        let (mut sender, input) = ToolInput::<SkillBundleToolInput>::test();
        sender.send_full(json!({
            "skills": ["a", "b", "c"],
            "task": "do something"
        }));
        let (event_stream, _rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| tool.run(input, event_stream, cx));
        let result = task.await;

        let err = match result {
            Err(SkillBundleToolOutput::Error { error }) => error,
            other => panic!("expected Error for no executor, got: {other:?}"),
        };
        assert!(
            err.contains("not configured"),
            "error should mention executor not configured: {err}"
        );
    }
}
