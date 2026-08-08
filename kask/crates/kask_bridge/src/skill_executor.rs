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
use fs::Fs;
use hkask_templates::{ManifestExecutor, load_manifest_from_yaml, validate_inputs};
use hkask_types::InferencePort;
use serde_json::Value;
use std::path::Path;

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

/// Resolves whether a tool is enabled in the current agent profile.
/// Used by `BridgeManifestExecutor` to enforce proposer/evaluator separation:
/// a step declaring `profile: ask` must not have `terminal` available.
/// The caller (main.rs) provides an implementation that reads from
/// `AgentProfileSettings::is_tool_enabled`. If not wired, the bridge warns
/// but does not enforce (the `.rules` "startup-failure signal" pattern).
pub trait ProfileResolver: Send + Sync {
    fn is_tool_enabled(&self, tool_name: &str) -> bool;
}

/// A `ProfileResolver` backed by a snapshot of a profile's `terminal` tool state,
/// read once at wiring time (in `main.rs`'s deferred post-login task).
///
/// This is the only feasible `Send + Sync` resolver over GPUI-held settings:
/// `AgentProfileSettings` lives behind `&App` (not `Send`), so the bridge — which
/// is process-global and runs the cascade on a tokio worker — cannot read profile
/// state live from within the sync `is_tool_enabled` callback.
///
/// Limitation: the snapshot is stale if the user changes profiles after wiring.
/// Today no `category: skill` manifest declares `profile:`, so the gate has no
/// production trigger and the staleness is moot. Per-session profile enforcement
/// (re-snapshot on profile change, or thread the invoking agent's
/// `terminal`-enabled state through the `SkillTool`) is a future enhancement.
pub struct SnapshotProfileResolver {
    terminal_enabled: bool,
}

impl SnapshotProfileResolver {
    pub fn new(terminal_enabled: bool) -> Self {
        Self { terminal_enabled }
    }
}

impl ProfileResolver for SnapshotProfileResolver {
    fn is_tool_enabled(&self, tool_name: &str) -> bool {
        tool_name == "terminal" && self.terminal_enabled
    }
}

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
    /// Profile resolver for proposer/evaluator separation enforcement.
    profile_resolver: Option<Arc<dyn ProfileResolver>>,
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
            profile_resolver: None,
        }
    }

    fn manifest_path(&self, skill_name: &str) -> PathBuf {
        self.registry_manifests_dir
            .join(format!("{skill_name}.yaml"))
    }

    /// Wire a profile resolver for proposer/evaluator separation enforcement.
    /// When a manifest step declares `profile: ask`, the bridge checks
    /// `resolver.is_tool_enabled("terminal")` and refuses if true.
    #[must_use]
    pub fn with_profile_resolver(mut self, resolver: Arc<dyn ProfileResolver>) -> Self {
        self.profile_resolver = Some(resolver);
        self
    }

    /// Resolve a skill's manifest YAML from the filesystem. Disk is the
    /// single runtime source — there is no compiled-in fallback. The shipped
    /// manifests are seeded to disk at startup by the registry seeding path,
    /// so a fresh install has the full manifest set on disk and YAML edits
    /// take effect immediately without recompilation.
    fn manifest_yaml(&self, skill_name: &str) -> Option<std::borrow::Cow<'static, str>> {
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

    /// Run a named skill's manifest cascade and return the full context
    /// HashMap (not just the final text). Used by `compose_and_execute_bundle`
    /// to extract structured fields (composed manifest, composition score)
    /// from intermediate step results.
    ///
    /// This is the shared manifest-loading + executor-construction + tokio-spawn
    /// path, factored out of `execute_skill` so both the single-skill and
    /// bundle-composition paths use the same wiring (model defaults injection,
    /// profile enforcement, tokio handle).
    async fn run_manifest_cascade(
        &self,
        skill_name: &str,
        mut context: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>, String> {
        let manifest_yaml = self.manifest_yaml(skill_name).ok_or_else(|| {
            format!(
                "No manifest found for skill '{skill_name}' on disk at {}",
                self.manifest_path(skill_name).display()
            )
        })?;

        let manifest = load_manifest_from_yaml(&manifest_yaml)
            .map_err(|e| format!("Failed to load manifest '{skill_name}': {e}"))?;

        if !manifest.is_skill() {
            return Err(format!(
                "Skill '{skill_name}' has category '{:?}' — only `skill` manifests may execute via the skill tool",
                manifest.category
            ));
        }

        // Inject model defaults (same as execute_skill).
        self.inject_model_defaults(&mut context);

        let executor = self.build_executor();

        let join_handle = self.tokio_handle.spawn(async move {
            executor
                .execute_manifest(&manifest, context)
                .await
                .map_err(|e| format!("Manifest execution failed: {e}"))
        });

        join_handle
            .await
            .map_err(|e| format!("Manifest execution task failed: {e}"))?
    }

    /// Execute an already-loaded `BundleManifest` directly (no name lookup).
    /// Used by `compose_and_execute_bundle` to run the composed manifest
    /// produced by the skill-bundler cascade.
    async fn execute_manifest_direct(
        &self,
        manifest: &hkask_templates::BundleManifest,
        mut context: HashMap<String, Value>,
    ) -> Result<String, String> {
        self.inject_model_defaults(&mut context);

        let executor = self.build_executor();

        let join_handle = self.tokio_handle.spawn({
            let manifest = manifest.clone();
            async move {
                executor
                    .execute_manifest(&manifest, context)
                    .await
                    .map_err(|e| format!("Composed manifest execution failed: {e}"))
            }
        });

        let result = join_handle
            .await
            .map_err(|e| format!("Composed manifest execution task failed: {e}"))??;

        Ok(extract_final_step_result(&result))
    }

    /// Inject config-driven model defaults into the template context.
    /// Factored out of `execute_skill` so both paths share the same injection.
    fn inject_model_defaults(&self, context: &mut HashMap<String, Value>) {
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
    }

    /// Construct a `ManifestExecutor` with the bridge's inference/tools and
    /// profile resolver. Factored out of `execute_skill` so both paths share
    /// the same executor wiring.
    fn build_executor(&self) -> ManifestExecutor {
        let executor = ManifestExecutor::new(
            self.inference.clone(),
            self.tools.clone(),
            hkask_types::template::LLMParameters::default(),
        )
        .with_template_base_path(self.registry_templates_dir.clone());

        if let Some(ref resolver) = self.profile_resolver {
            let resolver = resolver.clone();
            executor.with_terminal_check(std::sync::Arc::new(move || {
                resolver.is_tool_enabled("terminal")
            }))
        } else {
            executor
        }
    }
}

/// Materialise the shipped hKask registry (process manifests + Jinja2/YAML
/// templates) onto the user's disk if missing. The disk copy is the single
/// runtime source of truth — `BridgeManifestExecutor::manifest_yaml` and
/// `TemplateRenderer::load` read exclusively from disk, so YAML/J2 edits take
/// effect immediately without recompilation. The compiled seed payload exists
/// solely so a self-contained binary can populate the registry on a fresh
/// install with no source tree.
///
/// Existing files are **never overwritten** — user edits are sovereign. A
/// user who deletes a shipped manifest/template will see it re-seeded on the
/// next startup.
///
/// `registry_root` is the on-disk registry root (e.g.
/// `data_dir()/agents/registry/`). Writes:
/// - `registry_root/manifests/<skill>.yaml` (process manifests)
/// - `registry_root/templates/<skill>/manifest.yaml` (per-skill template manifests)
/// - `registry_root/templates/<skill>/<file>.j2` (Jinja2 templates)
/// - `registry_root/templates/<skill>/<file>.yaml` (YAML sub-manifests / reference docs)
pub async fn seed_registry_to_disk(fs: &dyn Fs, registry_root: &Path) {
    let manifests_dir = registry_root.join("manifests");
    for (name, content) in hkask_templates::process_manifest_seed() {
        let path = manifests_dir.join(format!("{name}.yaml"));
        if fs.is_file(&path).await {
            continue;
        }
        if let Err(e) = fs.create_dir(&manifests_dir).await {
            tracing::warn!(
                "Failed to create manifests dir '{}': {e}",
                manifests_dir.display()
            );
            break;
        }
        if let Err(e) = fs.write(&path, content.as_bytes()).await {
            tracing::warn!("Failed to seed process manifest '{name}': {e}");
        }
    }

    let templates_dir = registry_root.join("templates");
    // Per-skill template manifests.
    for (skill, content) in hkask_templates::template_manifest_seed() {
        let skill_dir = templates_dir.join(skill);
        let path = skill_dir.join("manifest.yaml");
        if fs.is_file(&path).await {
            continue;
        }
        let _ = fs.create_dir(&skill_dir).await;
        if let Err(e) = fs.write(&path, content.as_bytes()).await {
            tracing::warn!("Failed to seed template manifest for '{skill}': {e}");
        }
    }
    // Jinja2 templates (key is `<skill>/<file>.j2`).
    for (key, content) in hkask_templates::template_file_seed() {
        let path = templates_dir.join(key);
        if fs.is_file(&path).await {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = fs.create_dir(parent).await;
        }
        if let Err(e) = fs.write(&path, content.as_bytes()).await {
            tracing::warn!("Failed to seed template '{key}': {e}");
        }
    }
    // YAML template files (key is `<skill>/<file>.yaml`, excluding manifest.yaml).
    for (key, content) in hkask_templates::template_yaml_file_seed() {
        let path = templates_dir.join(key);
        if fs.is_file(&path).await {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = fs.create_dir(parent).await;
        }
        if let Err(e) = fs.write(&path, content.as_bytes()).await {
            tracing::warn!("Failed to seed YAML template '{key}': {e}");
        }
    }

    // Company-source manifests (corpus-specific resource, not a skill manifest).
    // Seeded to `registry_root/company-sources/<symbol>.yaml` so the corpus
    // MCP server's `corpus_discover_company` tool can resolve the default
    // `manifest_path` against the data directory in production.
    let company_sources_dir = registry_root.join("company-sources");
    for (symbol, content) in hkask_templates::company_source_seed() {
        let path = company_sources_dir.join(format!("{symbol}.yaml"));
        if fs.is_file(&path).await {
            continue;
        }
        let _ = fs.create_dir(&company_sources_dir).await;
        if let Err(e) = fs.write(&path, content.as_bytes()).await {
            tracing::warn!("Failed to seed company-source manifest '{symbol}': {e}");
        }
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
                "No manifest found for skill '{skill_name}' on disk at {}",
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

        // Profile enforcement (proposer/evaluator separation): if any step
        // declares a `profile`, verify that `terminal` is NOT enabled.
        let needs_profile_check = manifest.steps.iter().any(|s| s.profile.is_some());
        if needs_profile_check {
            match &self.profile_resolver {
                Some(resolver) => {
                    if resolver.is_tool_enabled("terminal") {
                        // `needs_profile_check` above guarantees a step with a
                        // `profile` exists, so these `if let`s always match —
                        // but the non-panicking form avoids `expect` (`.rules`).
                        if let Some(step) = manifest.steps.iter().find(|s| s.profile.is_some())
                            && let Some(profile_name) = step.profile.as_ref()
                        {
                            return Err(format!(
                                "Step {} declares profile '{}' but the `terminal` tool is enabled. \
                                 This violates proposer/evaluator separation — a proposer with terminal \
                                 can evaluate its own tests (self-confirming loop anti-pattern). \
                                 Remediation: remove `terminal` from the '{}' profile in settings, \
                                 or bind this step to a profile without `terminal` (e.g. `ask`).",
                                step.ordinal, profile_name, profile_name
                            ));
                        }
                    }
                }
                None => {
                    tracing::warn!(
                        target: "reg.skill.profile_enforcement",
                        skill = skill_name,
                        "Profile enforcement not wired — a step declares a profile but no ProfileResolver is set. \
                         Proposer/evaluator separation is NOT enforced. \
                         Remediation: wire a ProfileResolver via BridgeManifestExecutor::with_profile_resolver in main.rs.",
                    );
                }
            }
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

        // Inject config-driven model defaults and construct the executor.
        // Factored into `inject_model_defaults` and `build_executor` so the
        // single-skill and bundle-composition paths share the same wiring.
        // Values come from (in priority order):
        // 1. KaskSettings (settings.json "kask" section) — if non-empty
        // 2. HKASK_* env vars (.env file) — via model_constants functions
        // 3. Compile-time defaults in model_constants.rs
        self.inject_model_defaults(&mut context);
        let executor = self.build_executor();

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

    async fn compose_and_execute_bundle(
        &self,
        skill_names: &[String],
        task: &str,
        context: HashMap<String, Value>,
    ) -> Result<agent::BundleExecutionResult, String> {
        // Phase 1: Run the skill-bundler manifest to compose a BundleManifest.
        // The bundler cascade is: goal-extract → compose → synthesize → validate
        // → lisp.eval score → evolve → loop. The composed manifest is at
        // step_3_result.candidates[0].composite_manifest.
        let bundler_context = {
            let mut ctx = context;
            ctx.insert(
                "skill_names".to_string(),
                Value::Array(
                    skill_names
                        .iter()
                        .map(|s| Value::String(s.clone()))
                        .collect(),
                ),
            );
            ctx.insert(
                "user_intent".to_string(),
                Value::String(task.to_string()),
            );
            ctx
        };

        // Run the skill-bundler cascade and get the full context back (not
        // just the final text) so we can extract the composed manifest and
        // the composition score structurally.
        let bundler_result = self
            .run_manifest_cascade("skill-bundler", bundler_context)
            .await?;

        // Extract the composed manifest from step_3_result.candidates[0].composite_manifest.
        // The synthesize step produces a `candidates` array; the first candidate's
        // `composite_manifest` is the governed BundleManifest.
        let bundle_manifest_json = bundler_result
            .get("step_3_result")
            .and_then(|v| v.get("candidates"))
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("composite_manifest"))
            .cloned()
            .ok_or_else(|| {
                "skill-bundler cascade did not produce a composite_manifest at \
                 step_3_result.candidates[0].composite_manifest — the synthesize \
                 step may have failed or produced no candidates"
                    .to_string()
            })?;

        // Extract the deterministic composition score from step_5_result
        // (the lisp.eval step). This is the falsifier anchor — if lisp.eval
        // were removed, this would be absent and the UI's score display
        // would degrade to "unavailable".
        let composition_score = bundler_result
            .get("step_5_result")
            .and_then(|v| v.as_f64());

        // Extract the skill names actually placed in the composed manifest
        // (may differ from the input if the bundler dropped a skill via
        // dead-letter resolution).
        let composed_skill_names = bundler_result
            .get("step_2_result")
            .and_then(|v| v.get("bundle_manifest"))
            .and_then(|bm| bm.get("skills"))
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| skill_names.to_vec());

        // Phase 2: Load the composed manifest and execute its cascade.
        let manifest_json_string = serde_json::to_string(&bundle_manifest_json)
            .map_err(|e| format!("Failed to serialize composed manifest: {e}"))?;
        let manifest = load_manifest_from_yaml(&manifest_json_string)
            .map_err(|e| format!("Failed to load composed manifest: {e}"))?;

        // Validate the composed manifest before execution. A manifest that
        // fails validation should still proceed (best-available) but the
        // operator gets a warning signal — the .rules "advertised invariants
        // need enforcement points" trap.
        let validation = manifest.validate();
        if !validation.errors.is_empty() {
            tracing::warn!(
                target: "reg.skill.bundle_compose",
                errors = ?validation.errors,
                skill_names = ?composed_skill_names,
                "bundler-validate failed on the composed manifest — proceeding with \
                 best-available manifest. The composition may have structural issues.",
            );
        }

        let execution_context = HashMap::new();
        let output = self
            .execute_manifest_direct(&manifest, execution_context)
            .await?;

        Ok(agent::BundleExecutionResult {
            bundle_manifest: bundle_manifest_json,
            output,
            composition_score,
            composed_skill_names,
        })
    }
}

/// Deterministically extract the final step's result from the cascade context,
/// reusing the canonical ordinal-keyed selector `hkask_templates::extract_final_step_result`
/// (the .rules "ManifestExecutor final-result extraction must be ordinal-keyed"
/// trap — do not re-implement the ordinal parse). Falls back to the full context
/// JSON when no `step_N_result` keys exist (e.g. manifests whose final step is
/// `populate`, emitting only `step_N_populated`); this fallback is a bridge
/// policy layer on top of the shared selector, which returns `Value::Null`.
fn extract_final_step_result(result: &std::collections::HashMap<String, Value>) -> String {
    let value = hkask_templates::extract_final_step_result(result);
    if value.is_null() {
        serde_json::to_string(result).unwrap_or_default()
    } else {
        value.to_string()
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
