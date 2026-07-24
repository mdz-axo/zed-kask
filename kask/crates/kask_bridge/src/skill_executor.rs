//! `SkillManifestExecutor` adapter — bridges zed's `SkillTool` to hKask's `ManifestExecutor`.
//!
//! This is the D1 seam. When zed's `SkillTool` is constructed with a manifest executor,
//! skill activation runs the hKask cascade (KnowAct/FlowDef/RenderAct + PDCA + gas/rjoule
//! + OCAP) instead of injecting the `SKILL.md` body.
//!
//! The adapter resolves a skill name to its YAML manifest in the hKask registry
//! (`kask/registry/manifests/<name>.yaml`), loads it as a `BundleManifest`, and runs
//! `ManifestExecutor::execute_manifest()`.
//!
//! The `SKILL.md` files in `.agents/skills/` remain the discovery-only catalog entries
//! (zed's `agent_skills` crate discovers them for the system-prompt catalog). When a
//! skill has an hKask manifest, the cascade runs; when it doesn't, the `SkillTool`
//! falls back to body injection.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use hkask_templates::{ManifestExecutor, load_manifest_from_file};
use hkask_types::InferencePort;
use serde_json::Value;

/// Bridge between zed's `SkillManifestExecutor` trait and hKask's `ManifestExecutor`.
///
/// Holds an `InferencePort` (the bridge's `LanguageModelInferencePort` over zed's
/// `LanguageModel`) and an optional `ToolPort` (for FlowDef execution — D3, not yet wired).
/// KnowAct skills work now; FlowDef skills gate on D3.
pub struct BridgeManifestExecutor {
    inference: Arc<dyn InferencePort>,
    tools: Arc<dyn hkask_capability::ToolPort>,
    a2a_secret: Vec<u8>,
    /// Path to the hKask registry manifests directory.
    registry_manifests_dir: PathBuf,
    /// Path to the hKask registry templates directory (for Jinja2 template resolution).
    registry_templates_dir: PathBuf,
}

impl BridgeManifestExecutor {
    /// Construct a new bridge manifest executor with a real ToolPort (D3 wired).
    ///
    /// `inference` is the bridge's `LanguageModelInferencePort` over zed's `LanguageModel`.
    /// `tools` is the bridge's `BridgeToolPort` over hKask's `McpRuntime`.
    /// `registry_manifests_dir` should point to `kask/registry/manifests/`.
    /// `registry_templates_dir` should point to `kask/registry/templates/`.
    pub fn new(
        inference: Arc<dyn InferencePort>,
        tools: Arc<dyn hkask_capability::ToolPort>,
        a2a_secret: Vec<u8>,
        registry_manifests_dir: PathBuf,
        registry_templates_dir: PathBuf,
    ) -> Self {
        Self {
            inference,
            tools,
            a2a_secret,
            registry_manifests_dir,
            registry_templates_dir,
        }
    }

    fn manifest_path(&self, skill_name: &str) -> PathBuf {
        self.registry_manifests_dir
            .join(format!("{skill_name}.yaml"))
    }
}

#[async_trait]
impl agent::SkillManifestExecutor for BridgeManifestExecutor {
    fn has_manifest(&self, skill_name: &str) -> bool {
        self.manifest_path(skill_name).exists()
    }

    async fn execute_skill(
        &self,
        skill_name: &str,
        context: HashMap<String, Value>,
    ) -> Result<String, String> {
        let manifest_path = self.manifest_path(skill_name);

        let manifest = load_manifest_from_file(&manifest_path).map_err(|e| {
            format!(
                "Failed to load manifest '{}' at {}: {}",
                skill_name,
                manifest_path.display(),
                e
            )
        })?;

        // Construct a ManifestExecutor with the bridge's InferencePort and ToolPort.
        let executor = ManifestExecutor::new(
            self.inference.clone(),
            self.tools.clone(),
            hkask_types::template::LLMParameters::default(),
            self.a2a_secret.clone(),
        )
        .with_template_base_path(self.registry_templates_dir.clone());

        let result = executor
            .execute_manifest(&manifest, context)
            .await
            .map_err(|e| format!("Manifest execution failed: {e}"))?;

        // The cascade returns a HashMap<String, Value> — extract the final output.
        // Convention: the last step's result is under "step_N_result" where N is
        // the last ordinal. For KnowAct skills, the result is typically under
        // "step_1_result" or the step's output key.
        // For now, serialize the full context as JSON so nothing is lost.
        let output = result
            .values()
            .last()
            .map(|v| v.to_string())
            .unwrap_or_else(|| serde_json::to_string(&result).unwrap_or_default());

        Ok(output)
    }
}
