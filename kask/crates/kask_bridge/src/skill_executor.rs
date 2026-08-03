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
use hkask_templates::{
    ManifestExecutor, load_manifest_from_yaml, process_manifest_yaml, validate_inputs,
};
use hkask_types::InferencePort;
use serde_json::Value;

/// Context keys the runtime injects itself (not user-supplied params), excluded
/// from the unknown-key check in `validate_inputs`. `task` is injected by the
/// `SkillTool`/slash-command path; the `*_model` keys are injected by
/// `execute_skill` below. Listed here so validation never flags them as typos.
const SKILL_CONTEXT_SYSTEM_KEYS: &[&str] = &[
    "task",
    "embedding_model",
    "classifier_model",
    "ocr_model",
    "default_model",
    "qa_model",
    "tts_model",
    "stt_model",
    "vision_model",
    "image_gen_model",
];

/// Bridge between zed's `SkillManifestExecutor` trait and hKask's `ManifestExecutor`.
///
/// Holds an `InferencePort` (the bridge's `LanguageModelInferencePort` over zed's
/// `LanguageModel`) and an optional `ToolPort` (for FlowDef execution — D3, not yet wired).
/// KnowAct skills work now; FlowDef skills gate on D3.
pub struct BridgeManifestExecutor {
    inference: Arc<dyn InferencePort>,
    tools: Arc<dyn hkask_capability::ToolPort>,
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
    /// `tools` is the bridge's `ToolPort` over hKask's `McpRuntime`.
    /// `registry_manifests_dir` should point to `kask/registry/manifests/`.
    /// `registry_templates_dir` should point to `kask/registry/templates/`.
    pub fn new(
        inference: Arc<dyn InferencePort>,
        tools: Arc<dyn hkask_capability::ToolPort>,
        registry_manifests_dir: PathBuf,
        registry_templates_dir: PathBuf,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            inference,
            tools,
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
        // Load the manifest FIRST so we can validate the caller-supplied context
        // against its declared `inputs` (Layer A) before injecting runtime
        // defaults or running the cascade. Validating before the model-default
        // injection keeps the user-supplied keys distinguishable from the
        // runtime-injected system keys (listed in SKILL_CONTEXT_SYSTEM_KEYS).
        let manifest_yaml = self.manifest_yaml(skill_name).ok_or_else(|| {
            format!(
                "No manifest found for skill '{skill_name}' (checked embedded registry and {})",
                self.manifest_path(skill_name).display()
            )
        })?;
        let manifest = load_manifest_from_yaml(&manifest_yaml)
            .map_err(|e| format!("Failed to load manifest '{skill_name}': {e}"))?;

        // Enforce the category labelling system at the execution boundary.
        // `resolve_manifest` enforces `is_skill()` on the `flowdef` sub-cascade
        // binding path, but this primary path (`execute_skill` →
        // `load_manifest_from_yaml` → `execute_manifest`) bypasses it. Without
        // this check, an infra manifest (pipeline, runtime-config, qa-script,
        // daemon-process) in the embedded registry could execute via the skill
        // tool if its name were passed to `execute_skill`. This makes the
        // `compute_ref` "gated to category: skill manifests only" comment in
        // create-skill.yaml redundant — all manifests reaching the executor
        // through this path are guaranteed to be skills.
        if !manifest.is_skill() {
            return Err(format!(
                "Skill '{skill_name}' has category '{:?}' — only `skill` manifests may execute via the skill tool",
                manifest.category
            ));
        }

        // Layer A: enforce the manifest's declared `inputs` contract at the
        // boundary. Opt-in via `enforce_inputs: true` in the manifest; skills
        // that don't opt in are unaffected (back-compat). Turns silent wrong-
        // params (missing required, wrong-typed) into a structured error that
        // propagates to the UI as a `SkillToolOutput::Error`.
        if let Err(e) = validate_inputs(
            manifest.enforce_inputs,
            manifest.inputs.as_ref(),
            &context,
            SKILL_CONTEXT_SYSTEM_KEYS,
        ) {
            return Err(format!("Skill '{skill_name}' input validation failed: {e}"));
        }

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

        // Construct a ManifestExecutor with the bridge's InferencePort and ToolPort.
        let executor = ManifestExecutor::new(
            self.inference.clone(),
            self.tools.clone(),
            hkask_types::template::LLMParameters::default(),
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

        // The cascade returns a HashMap<String, Value> whose iteration order is
        // randomized (HashMap uses RandomState). Extract the final step's result
        // deterministically by selecting the highest-ordinal `step_N_result` key.
        // This is the convention enforced by ManifestExecutor (executor.rs stores
        // every step's output under `step_{ordinal}_result`).
        let output = extract_final_step_result(&result);

        Ok(output)
    }
}

/// Deterministically extract the final step's result from the cascade context.
///
/// `ManifestExecutor::execute_manifest` stores each step's output under a
/// `step_{ordinal}_result` key. HashMap iteration order is randomized, so
/// `values().last()` would pick an arbitrary step. This function parses the
/// ordinal from each `step_N_result` key and returns the value of the highest
/// ordinal, serialized as JSON. Falls back to the full context if no
/// `step_N_result` keys are present (e.g. manifests that only emit
/// `step_N_populated` or other keys).
fn extract_final_step_result(result: &std::collections::HashMap<String, Value>) -> String {
    let mut step_results: Vec<(u32, &Value)> = result
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("step_")
                .and_then(|rest| rest.strip_suffix("_result"))
                .and_then(|n| n.parse::<u32>().ok())
                .map(|ordinal| (ordinal, value))
        })
        .collect();
    step_results.sort_by_key(|(ordinal, _)| *ordinal);
    match step_results.last() {
        Some((_, value)) => value.to_string(),
        None => serde_json::to_string(result).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Regression for the non-deterministic `values().last()` extraction bug.
    /// HashMap iteration order is randomized per-process; the extractor must
    /// deterministically pick the highest-ordinal `step_N_result`, not an
    /// arbitrary value.
    #[test]
    fn extract_final_step_result_picks_highest_ordinal() {
        let mut map: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        map.insert("step_1_result".to_string(), json!("first"));
        map.insert("step_3_result".to_string(), json!("third"));
        map.insert("step_2_result".to_string(), json!("second"));
        map.insert("_convergence".to_string(), json!({"status": "converged"}));

        let out = extract_final_step_result(&map);
        assert_eq!(
            out, "\"third\"",
            "must return step_3_result (highest ordinal)"
        );
    }

    #[test]
    fn extract_final_step_result_ignores_non_result_keys() {
        let mut map: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        map.insert("step_1_populated".to_string(), json!("populated"));
        map.insert("step_1_result".to_string(), json!({"answer": 42}));
        map.insert("task".to_string(), json!("user request"));

        let out = extract_final_step_result(&map);
        assert_eq!(
            out, "{\"answer\":42}",
            "must pick step_N_result, not _populated or other keys"
        );
    }

    #[test]
    fn extract_final_step_result_falls_back_to_full_context_when_no_step_results() {
        let mut map: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        map.insert("task".to_string(), json!("user request"));
        map.insert("_convergence".to_string(), json!({"status": "running"}));

        let out = extract_final_step_result(&map);
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("fallback must be valid JSON");
        assert_eq!(parsed["task"], json!("user request"));
        assert_eq!(parsed["_convergence"]["status"], json!("running"));
    }

    #[test]
    fn extract_final_step_result_handles_single_step() {
        let mut map: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        map.insert(
            "step_1_result".to_string(),
            json!({"convergence_metric": 0.05}),
        );

        let out = extract_final_step_result(&map);
        assert!(out.contains("convergence_metric"));
        assert!(out.contains("0.05"));
    }
}
