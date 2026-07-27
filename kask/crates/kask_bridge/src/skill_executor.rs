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
use hkask_templates::{ManifestExecutor, load_manifest_from_yaml, process_manifest_yaml};
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
    /// Tokio runtime handle — entered around manifest execution so that
    /// `tokio::time::timeout` and other tokio APIs inside ManifestExecutor
    /// have a reactor. The SkillTool runs on GPUI's foreground executor (not
    /// tokio), so without this guard, any skill with a manifest would panic
    /// with "there is no reactor running".
    tokio_handle: tokio::runtime::Handle,
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
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            inference,
            tools,
            a2a_secret,
            registry_manifests_dir,
            registry_templates_dir,
            tokio_handle,
        }
    }

    fn manifest_path(&self, skill_name: &str) -> PathBuf {
        self.registry_manifests_dir
            .join(format!("{skill_name}.yaml"))
    }

    /// Resolve a skill's manifest YAML, preferring the embedded (build-time)
    /// copy and falling back to the filesystem path. The embedded copy is
    /// authoritative for installed binaries — it works regardless of CWD or
    /// install location. The filesystem fallback exists for dev workflows
    /// where a manifest has been edited but not yet rebuilt.
    fn manifest_yaml(&self, skill_name: &str) -> Option<std::borrow::Cow<'static, str>> {
        if let Some(yaml) = process_manifest_yaml(skill_name) {
            return Some(std::borrow::Cow::Borrowed(yaml));
        }
        let path = self.manifest_path(skill_name);
        if path.is_file() {
            match std::fs::read_to_string(&path) {
                Ok(content) => return Some(std::borrow::Cow::Owned(content)),
                Err(e) => {
                    tracing::warn!(
                        "Failed to read manifest '{}' at {}: {e}",
                        skill_name,
                        path.display()
                    );
                }
            }
        }
        None
    }
}

#[async_trait]
impl agent::SkillManifestExecutor for BridgeManifestExecutor {
    fn has_manifest(&self, skill_name: &str) -> bool {
        self.manifest_yaml(skill_name).is_some()
    }

    async fn execute_skill(
        &self,
        skill_name: &str,
        mut context: HashMap<String, Value>,
    ) -> Result<String, String> {
        // Inject config-driven model defaults into the template context so
        // templates can reference {{ embedding_model }}, {{ classifier_model }},
        // etc. instead of hardcoding model names. This is the single point
        // where config flows into templates — templates should NEVER
        // hardcode model names.
        //
        // Values come from (in priority order):
        // 1. KaskSettings (settings.json "kask" section) — if non-empty
        // 2. HKASK_* env vars (.env file) — via model_constants functions
        // 3. Compile-time defaults in model_constants.rs
        if !context.contains_key("embedding_model") {
            context.insert(
                "embedding_model".into(),
                Value::String(hkask_inference::model_constants::embedding_model()),
            );
        }
        if !context.contains_key("classifier_model") {
            context.insert(
                "classifier_model".into(),
                Value::String(hkask_inference::model_constants::classifier_model()),
            );
        }
        if !context.contains_key("ocr_model") {
            context.insert(
                "ocr_model".into(),
                Value::String(hkask_inference::model_constants::ocr_model()),
            );
        }
        if !context.contains_key("default_model") {
            context.insert(
                "default_model".into(),
                Value::String(std::env::var("HKASK_DEFAULT_MODEL").unwrap_or_else(|_| {
                    hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL.to_string()
                })),
            );
        }
        if !context.contains_key("qa_model") {
            context.insert(
                "qa_model".into(),
                Value::String(std::env::var("HKASK_QA_MODEL").unwrap_or_else(|_| {
                    hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL.to_string()
                })),
            );
        }
        // Media models from env vars (KaskSettings.media.* mirrors these)
        if !context.contains_key("tts_model") {
            context.insert(
                "tts_model".into(),
                Value::String(std::env::var("HKASK_MEDIA_TTS_MODEL").unwrap_or_default()),
            );
        }
        if !context.contains_key("stt_model") {
            context.insert(
                "stt_model".into(),
                Value::String(std::env::var("HKASK_MEDIA_STT_MODEL").unwrap_or_default()),
            );
        }
        if !context.contains_key("vision_model") {
            context.insert(
                "vision_model".into(),
                Value::String(std::env::var("HKASK_MEDIA_VISION_MODEL").unwrap_or_default()),
            );
        }
        if !context.contains_key("image_gen_model") {
            context.insert(
                "image_gen_model".into(),
                Value::String(std::env::var("HKASK_MEDIA_IMAGE_GEN_MODEL").unwrap_or_default()),
            );
        }
        // Fusion models from env vars
        if !context.contains_key("judge_model") {
            context.insert(
                "judge_model".into(),
                Value::String(
                    std::env::var("HKASK_FUSION_JUDGE_MODEL")
                        .unwrap_or_else(|_| "OpenRouter/z-ai/glm-5.2".to_string()),
                ),
            );
        }
        if !context.contains_key("panel_models") {
            context.insert("panel_models".into(), Value::String(std::env::var("HKASK_FUSION_PANEL_MODELS").unwrap_or_else(|_| "OpenRouter/z-ai/glm-5.2,OpenRouter/qwen/qwen3-235b-a22b,OpenRouter/minimax/minimax3".to_string())));
        }

        let manifest_yaml = self.manifest_yaml(skill_name).ok_or_else(|| {
            format!(
                "No manifest found for skill '{skill_name}' (checked embedded registry and {})",
                self.manifest_path(skill_name).display()
            )
        })?;

        let manifest = load_manifest_from_yaml(&manifest_yaml)
            .map_err(|e| format!("Failed to load manifest '{skill_name}': {e}"))?;

        // Construct a ManifestExecutor with the bridge's InferencePort and ToolPort.
        let executor = ManifestExecutor::new(
            self.inference.clone(),
            self.tools.clone(),
            hkask_types::template::LLMParameters::default(),
            self.a2a_secret.clone(),
        )
        .with_template_base_path(self.registry_templates_dir.clone());

        // Spawn manifest execution on the tokio runtime. ManifestExecutor
        // uses tokio::time::timeout internally, which requires a tokio reactor.
        // The SkillTool runs on GPUI's foreground executor, not tokio, so we
        // can't hold a tokio EnterGuard across .await (it's not Send). Spawning
        // on the tokio handle and awaiting the JoinHandle is the Send-safe way.
        let join_handle = self.tokio_handle.spawn(async move {
            executor
                .execute_manifest(&manifest, context)
                .await
                .map_err(|e| format!("Manifest execution failed: {e}"))
        });

        let result = join_handle
            .await
            .map_err(|e| format!("Manifest execution task failed: {e}"))??;

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
