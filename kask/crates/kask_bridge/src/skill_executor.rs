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
use hkask_templates::{CascadeOutcome, ManifestExecutor, load_manifest_from_yaml, validate_inputs};
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
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<CascadeOutcome, String> {
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

        let executor = self.build_executor(progress, title);

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

    /// Variant of `run_manifest_cascade` that takes a pre-loaded manifest
    /// instead of resolving by name. Used by `refine_bundle` to run the
    /// minimal single-step evolve manifest without a registry lookup.
    async fn run_manifest_cascade_with_manifest(
        &self,
        manifest: &hkask_templates::BundleManifest,
        mut context: HashMap<String, Value>,
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<CascadeOutcome, String> {
        // Enforce the same `is_skill()` guard as `run_manifest_cascade` — the
        // inline refine manifest is hardcoded (not user-supplied) so this is
        // defense in depth, but the guard must be uniform to prevent an infra
        // manifest from executing via the skill tool if the inline YAML is
        // ever edited to add `category: pipeline`.
        if !manifest.is_skill() {
            return Err(format!(
                "Refine manifest has category '{:?}' — only `skill` manifests may execute via the skill tool",
                manifest.category
            ));
        }

        self.inject_model_defaults(&mut context);

        let executor = self.build_executor(progress, title);

        let join_handle = self.tokio_handle.spawn({
            let manifest = manifest.clone();
            async move {
                executor
                    .execute_manifest(&manifest, context)
                    .await
                    .map_err(|e| format!("Manifest execution failed: {e}"))
            }
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
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<String, String> {
        self.inject_model_defaults(&mut context);

        let executor = self.build_executor(progress, title);

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
            .map_err(|e| format!("Composed manifest execution task failed: {e}"))?
            .map_err(|e| format!("Composed manifest execution failed: {e}"))?;

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
    ///
    /// Wires `DefaultPolicy` as the runtime policy so the FIDES Source→Sink
    /// block (Layer 4) fires on every production cascade. Without this, the
    /// `reg.runtime.policy` span and Block/RequireHuman enforcement
    /// are dead code — `runtime_policy` stays `None` and untrusted input flows
    /// to Sink tools unchecked (OWASP LLM06, RR-0053).
    fn build_executor(
        &self,
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> ManifestExecutor {
        let executor = ManifestExecutor::new(
            self.inference.clone(),
            self.tools.clone(),
            hkask_types::template::LLMParameters::default(),
        )
        .with_template_base_path(self.registry_templates_dir.clone())
        .with_runtime_policy(std::sync::Arc::new(
            hkask_regulation::DefaultPolicy::default(),
        ));

        let executor = if let Some(ref resolver) = self.profile_resolver {
            let resolver = resolver.clone();
            executor.with_terminal_check(std::sync::Arc::new(move || {
                resolver.is_tool_enabled("terminal")
            }))
        } else {
            executor
        };

        let executor = if let Some(progress) = progress {
            executor.with_progress(progress)
        } else {
            executor
        };

        if let Some(title) = title {
            executor.with_title(title)
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
        progress: Option<agent::CascadeProgress>,
        title: Option<agent::CascadeProgress>,
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
        let executor = self.build_executor(progress, title);

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

        // (K5) `result` is the typed `CascadeOutcome`; `extract_final_step_result`
        // selects `last_result_step`'s value (deterministic — the machine tracks
        // it, no randomized HashMap scan).
        let output = extract_final_step_result(&result);

        Ok(output)
    }

    async fn compose_and_execute_bundle(
        &self,
        skill_names: &[String],
        task: &str,
        context: HashMap<String, Value>,
        progress: Option<agent::CascadeProgress>,
        title: Option<agent::CascadeProgress>,
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
            ctx.insert("user_intent".to_string(), Value::String(task.to_string()));
            ctx
        };

        // Run the skill-bundler cascade and get the full context back (not
        // just the final text) so we can extract the composed manifest and
        // the composition score structurally.
        let bundler_result = self
            .run_manifest_cascade(
                "skill-bundler",
                bundler_context,
                progress.clone(),
                title.clone(),
            )
            .await?;

        // Extract the composed manifest from step_3_result.candidates[0].composite_manifest.
        // The synthesize step produces a `candidates` array; the first candidate's
        // `composite_manifest` is the governed BundleManifest.
        let bundle_manifest_json = bundler_result
            .context
            .lookup("step_3_result")
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
            .context
            .lookup("step_5_result")
            .and_then(|v| v.as_f64());

        // Extract the goal-extract step's output (step_1_result) so the
        // `Refine` action can pass it to `bundler-evolve` as `goal_context`.
        // Without it, the evolve step runs blind — it can't reference the
        // original goal. `Null` if the bundler cascade didn't produce it.
        let goal_context = bundler_result
            .context
            .lookup("step_1_result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Extract the skill names actually placed in the composed manifest
        // (may differ from the input if the bundler dropped a skill via
        // dead-letter resolution).
        let composed_skill_names = bundler_result
            .context
            .lookup("step_2_result")
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
            .execute_manifest_direct(&manifest, execution_context, progress, title)
            .await?;

        Ok(agent::BundleExecutionResult {
            bundle_manifest: bundle_manifest_json,
            output,
            composition_score,
            composed_skill_names,
            goal_context,
        })
    }

    async fn save_bundle(&self, bundle_manifest: serde_json::Value) -> Result<String, String> {
        // The composed manifest JSON from the bundler cascade is a flat
        // structure (id, name, steps, ... at the top level). The on-disk
        // `ManifestFile` format wraps the header fields under a `manifest:`
        // key with `steps`, `skills`, etc. as siblings. Reshape into that
        // form before serializing to YAML so `load_manifest_from_yaml` can
        // round-trip the saved file.
        let manifest_file_json = reshape_composite_to_manifest_file(&bundle_manifest);
        let yaml_string = serde_yaml_neo::to_string(&manifest_file_json)
            .map_err(|e| format!("Failed to serialize bundle manifest to YAML: {e}"))?;

        let raw_id = bundle_manifest
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| {
                "bundle manifest has no `id` field — cannot save without an id".to_string()
            })?;

        // Namespace the bundle ID with a `bundle-` prefix to prevent the
        // model-generated ID from colliding with an existing skill manifest
        // (e.g. if the model produces `id: "grill-me"`, this becomes
        // `bundle-grill-me` and won't overwrite `grill-me.yaml`).
        let namespaced_id = if raw_id.starts_with("bundle-") {
            raw_id
        } else {
            format!("bundle-{raw_id}")
        };

        let path = self.manifest_path(&namespaced_id);

        // Guard against overwriting an existing manifest at the namespaced
        // path. A collision here means a prior save used the same ID — the
        // operator should choose a different name or accept the overwrite
        // explicitly (future enhancement: prompt for confirmation).
        if path.is_file() {
            return Err(format!(
                "A bundle manifest already exists at {} (id: {}). \
                 Choose a different bundle ID or remove the existing file first.",
                path.display(),
                namespaced_id
            ));
        }

        std::fs::write(&path, yaml_string.as_bytes())
            .map_err(|e| format!("Failed to write bundle manifest to {}: {e}", path.display()))?;

        tracing::info!(
            target: "reg.skill.bundle_save",
            bundle_id = %namespaced_id,
            path = %path.display(),
            "Saved composed bundle manifest to registry"
        );
        Ok(namespaced_id)
    }

    async fn refine_bundle(
        &self,
        bundle_manifest: serde_json::Value,
        goal_context: serde_json::Value,
        goal_delta: f64,
        convergence_failure_reason: String,
    ) -> Result<agent::BundleExecutionResult, String> {
        // Run the `skill-bundler/bundler-evolve` template via a minimal
        // single-step manifest, then execute the evolved manifest's cascade.
        // The evolve template's contract declares `evolved_manifest` as an
        // output field; the executor's structured-output extraction stores
        // the parsed JSON under `step_1_result`.
        let bundle_name = bundle_manifest
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("composite-skill")
            .to_string();

        let evolve_context = {
            let mut ctx = HashMap::new();
            ctx.insert("bundle_name".to_string(), Value::String(bundle_name));
            ctx.insert("current_manifest".to_string(), bundle_manifest);
            ctx.insert("changed_skills".to_string(), Value::Array(vec![]));
            ctx.insert("goal_context".to_string(), goal_context);
            ctx.insert("goal_delta".to_string(), serde_json::json!(goal_delta));
            ctx.insert(
                "convergence_failure_reason".to_string(),
                Value::String(convergence_failure_reason),
            );
            self.inject_model_defaults(&mut ctx);
            ctx
        };

        // Construct a minimal single-step manifest wrapping the evolve
        // template. The input_mapping mirrors ordinal 6 of skill-bundler.yaml
        // so the evolve template receives the same bindings the full cascade
        // would produce.
        let refine_manifest_yaml = "\
manifest:
  id: refine-bundle
  name: Refine Bundle
  description: Single-step goal-delta-driven bundle refinement
steps:
  - ordinal: 1
    action: know
    description: Refine bundle via goal-delta evolution
    renderer: minijinja
    template_ref: skill-bundler/bundler-evolve
    gas_cap: 6000
    timeout_seconds: 60
    input_mapping:
      bundle_name: \"{{ bundle_name }}\"
      current_manifest: \"{{ current_manifest }}\"
      changed_skills: \"{{ changed_skills }}\"
      goal_context: \"{{ goal_context }}\"
      goal_delta: \"{{ goal_delta }}\"
      convergence_failure_reason: \"{{ convergence_failure_reason }}\"
";

        let refine_manifest = load_manifest_from_yaml(refine_manifest_yaml)
            .map_err(|e| format!("Failed to load refine manifest: {e}"))?;

        let refine_result = self
            .run_manifest_cascade_with_manifest(&refine_manifest, evolve_context, None, None)
            .await?;

        // Extract the evolved manifest from step_1_result.evolved_manifest.
        let evolved_manifest_json = refine_result
            .context
            .lookup("step_1_result")
            .and_then(|v| v.get("evolved_manifest"))
            .cloned()
            .ok_or_else(|| {
                "bundler-evolve did not produce `evolved_manifest` in step_1_result — \
                 the evolve step may have failed or returned an unexpected shape"
                    .to_string()
            })?;

        // Extract the evolved skill names.
        let composed_skill_names = evolved_manifest_json
            .get("skills")
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Load and execute the evolved manifest.
        let manifest_json_string = serde_json::to_string(&evolved_manifest_json)
            .map_err(|e| format!("Failed to serialize evolved manifest: {e}"))?;
        let manifest = load_manifest_from_yaml(&manifest_json_string)
            .map_err(|e| format!("Failed to load evolved manifest: {e}"))?;

        let execution_context = HashMap::new();
        let output = self
            .execute_manifest_direct(&manifest, execution_context, None, None)
            .await?;

        Ok(agent::BundleExecutionResult {
            bundle_manifest: evolved_manifest_json,
            output,
            composition_score: None,
            composed_skill_names,
            goal_context: serde_json::Value::Null,
        })
    }
}

/// Extract the cascade's final result as a string, reusing the canonical
/// typed selector `hkask_templates::extract_final_step_result` (K5:
/// `last_result_step`, not the retired ordinal-keyed HashMap scan). Falls back
/// to the full context JSON (materialized) when no step stored a result — a
/// bridge policy layer on top of the shared selector, which returns
/// `Value::Null`.
fn extract_final_step_result(outcome: &CascadeOutcome) -> String {
    let value = hkask_templates::extract_final_step_result(outcome);
    if value.is_null() {
        serde_json::to_string(&outcome.context.materialize()).unwrap_or_default()
    } else {
        value.to_string()
    }
}

/// Reshape a flat composite manifest JSON (as produced by the skill-bundler's
/// `bundler-synthesize` step under `composite_manifest`) into the
/// `ManifestFile` structure that `load_manifest_from_yaml` expects on disk.
/// The on-disk format wraps the header fields (`id`, `name`, `description`,
/// `version`, `editor`, `visibility`, `functional_role`, `category`,
/// `enforce_inputs`) under a `manifest:` key, with `steps`, `skills`,
/// `conflicts`, `complementarities`, `convergence`, `gas`, `rjoule`,
/// `error_handling`, `ledger`, `audit`, `inputs`, `principles` as siblings.
///
/// This is the inverse of the flattening `load_manifest_from_yaml` performs
/// when it constructs a `BundleManifest` from a `ManifestFile`. Keeping the
/// reshape here (in the bridge) avoids exposing the on-disk format to the
/// `agent` crate and keeps disk as the single source of truth (D1).
fn reshape_composite_to_manifest_file(composite: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;

    let header_keys = [
        "id",
        "name",
        "description",
        "version",
        "editor",
        "visibility",
        "functional_role",
        "category",
        "enforce_inputs",
    ];

    let sibling_keys = [
        "steps",
        "skills",
        "conflicts",
        "complementarities",
        "convergence",
        "gas",
        "rjoule",
        "error_handling",
        "ledger",
        "audit",
        "inputs",
        "principles",
    ];

    let manifest_header: serde_json::Map<String, serde_json::Value> = header_keys
        .iter()
        .filter_map(|k| composite.get(*k).map(|v| (k.to_string(), v.clone())))
        .collect();

    let mut out = serde_json::Map::new();
    out.insert("manifest".to_string(), json!(manifest_header));
    for k in sibling_keys {
        if let Some(v) = composite.get(k) {
            out.insert(k.to_string(), v.clone());
        }
    }
    json!(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::tool_taint::ToolTaint;
    use hkask_templates::budget::BudgetSnapshot;
    use hkask_templates::step_context::StepContext;
    use hkask_templates::step_graph::{ExitKind, StepId};
    use hkask_templates::step_machine::CascadeOutcome;
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;

    /// Build a minimal `CascadeOutcome` for the bridge's `extract_final_step_result`
    /// tests: typed context + machine-tracked `last_result_step`. Zeroed budget.
    fn outcome_with_last(context: StepContext, last: Option<StepId>) -> CascadeOutcome {
        CascadeOutcome {
            context,
            iterations: 1,
            exit_kind: ExitKind::Converged,
            last_result_step: last,
            budget_snapshot: BudgetSnapshot {
                gas_used: 0,
                gas_cap: 0,
                gas_remaining: 0,
                gas_cost_per_iteration: 0,
                rjoule_used: 0.0,
                rjoule_cap: 0.0,
                rjoule_remaining: 0.0,
                rjoule_enabled: false,
            },
        }
    }

    /// (K5) the retired ordinal-keyed HashMap scan is gone; the bridge's
    /// `extract_final_step_result` now selects `last_result_step`'s value from
    /// the typed `CascadeOutcome`. Deterministic by construction (no randomized
    /// HashMap order). Pins that contract + the null→full-context fallback.
    #[test]
    fn extract_final_step_result_returns_last_result_step() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!("first"), ToolTaint::Pure);
        ctx.store_result(2, 3, json!("third"), ToolTaint::Pure);
        ctx.store_result(1, 2, json!("second"), ToolTaint::Pure);
        let outcome = outcome_with_last(ctx, Some(2));
        let out = extract_final_step_result(&outcome);
        assert_eq!(
            out, "\"third\"",
            "must return last_result_step's value (step_id 2 = ordinal 3)"
        );
    }

    #[test]
    fn extract_final_step_result_ignores_protocol_and_named_keys() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!({"answer": 42}), ToolTaint::Pure);
        ctx.insert_protocol("task".into(), json!("user request"));
        ctx.store_named(1, 2, "populated", json!("populated"), ToolTaint::Pure);
        let outcome = outcome_with_last(ctx, Some(0));
        let out = extract_final_step_result(&outcome);
        assert_eq!(
            out, "{\"answer\":42}",
            "must return last_result_step's value, not protocol or named keys"
        );
    }

    #[test]
    fn extract_final_step_result_falls_back_to_full_context_when_no_step_results() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.insert_protocol("task".into(), json!("user request"));
        ctx.insert_protocol("_convergence".into(), json!({"status": "running"}));
        let outcome = outcome_with_last(ctx, None);
        let out = extract_final_step_result(&outcome);
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("fallback must be valid JSON");
        assert_eq!(parsed["task"], json!("user request"));
        assert_eq!(parsed["_convergence"]["status"], json!("running"));
    }

    #[test]
    fn extract_final_step_result_handles_single_step() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!({"convergence_metric": 0.05}), ToolTaint::Pure);
        let outcome = outcome_with_last(ctx, Some(0));
        let out = extract_final_step_result(&outcome);
        assert!(out.contains("convergence_metric"));
        assert!(out.contains("0.05"));
    }

    /// The reshape must move header fields under `manifest:` and keep the
    /// rest as siblings, so `load_manifest_from_yaml` can round-trip a saved
    /// bundle. Pins the on-disk format for the `Save` action.
    #[test]
    fn reshape_composite_to_manifest_file_moves_header_under_manifest_key() {
        let composite = json!({
            "id": "my-bundle",
            "name": "My Bundle",
            "description": "A test bundle",
            "version": "1.0.0",
            "editor": "operator",
            "visibility": "Public",
            "steps": [{"ordinal": 1, "action": "know", "description": "step 1"}],
            "skills": [{"id": "skill-a", "polarity": "proposer", "manifest_ref": "skill-a", "content_hash": "abc"}],
            "convergence": {"max_iterations": 3, "threshold": 0.1, "field": "score"},
            "gas": {"cap": 10000}
        });

        let reshaped = reshape_composite_to_manifest_file(&composite);

        // Header fields are under `manifest:`.
        let manifest_header = reshaped.get("manifest").expect("manifest key present");
        assert_eq!(
            manifest_header.get("id").and_then(|v| v.as_str()),
            Some("my-bundle")
        );
        assert_eq!(
            manifest_header.get("name").and_then(|v| v.as_str()),
            Some("My Bundle")
        );
        assert_eq!(
            manifest_header.get("description").and_then(|v| v.as_str()),
            Some("A test bundle")
        );

        // Sibling fields are at the top level.
        assert!(reshaped.get("steps").is_some());
        assert!(reshaped.get("skills").is_some());
        assert!(reshaped.get("convergence").is_some());
        assert!(reshaped.get("gas").is_some());
    }

    /// The reshape must not leak header fields to the top level (would
    /// confuse `load_manifest_from_yaml`'s `deny_unknown_fields` on
    /// `ManifestFile`).
    #[test]
    fn reshape_composite_to_manifest_file_does_not_leak_header_to_top_level() {
        let composite = json!({
            "id": "leak-test",
            "name": "Leak Test",
            "steps": []
        });

        let reshaped = reshape_composite_to_manifest_file(&composite);

        // `id` and `name` must NOT be at the top level.
        assert!(reshaped.get("id").is_none(), "id leaked to top level");
        assert!(reshaped.get("name").is_none(), "name leaked to top level");
        // They must be under `manifest:`.
        let header = reshaped.get("manifest").expect("manifest key present");
        assert_eq!(header.get("id").and_then(|v| v.as_str()), Some("leak-test"));
    }

    /// The reshape must handle a composite missing optional fields without
    /// inserting nulls (absent fields stay absent, so the YAML is clean).
    #[test]
    fn reshape_composite_to_manifest_file_handles_missing_optional_fields() {
        let composite = json!({
            "id": "minimal",
            "name": "Minimal",
            "steps": []
        });

        let reshaped = reshape_composite_to_manifest_file(&composite);

        let header = reshaped.get("manifest").expect("manifest key present");
        assert_eq!(header.get("id").and_then(|v| v.as_str()), Some("minimal"));
        // Optional header fields that were absent are not present.
        assert!(header.get("description").is_none());
        assert!(header.get("version").is_none());
        // Optional sibling fields that are absent are not present.
        assert!(reshaped.get("skills").is_none());
        assert!(reshaped.get("convergence").is_none());
    }

    /// Round-trip: reshape a composite manifest to `ManifestFile` format,
    /// serialize to YAML, and verify `load_manifest_from_yaml` can parse it
    /// back. This catches any field mismatch between the hardcoded key
    /// lists in `reshape_composite_to_manifest_file` and the actual
    /// `ManifestFile` struct fields — if `ManifestFile` gains a new field,
    /// this test will still pass (the field is optional with `#[serde(default)]`),
    /// but if a field is renamed or removed, the round-trip will fail.
    #[test]
    fn reshape_composite_round_trips_through_load_manifest_from_yaml() {
        let composite = json!({
            "id": "round-trip-test",
            "name": "Round Trip Test",
            "description": "A bundle for round-trip testing",
            "version": "1.0.0",
            "editor": "test",
            "visibility": "Public",
            "steps": [{
                "ordinal": 1,
                "action": "know",
                "description": "test step",
                "renderer": "minijinja",
                "template_ref": "some/template.j2",
                "gas_cap": 1000,
                "timeout_seconds": 30
            }],
            "skills": [{
                "id": "skill-a",
                "polarity": "Generative",
                "manifest_ref": "skill-a",
                "content_hash": "abc123"
            }],
            "convergence": {"max_iterations": 3, "threshold": 0.1, "field": "score"},
            "gas": {"cap": 10000}
        });

        let reshaped = reshape_composite_to_manifest_file(&composite);
        let yaml_string =
            serde_yaml_neo::to_string(&reshaped).expect("reshape output must serialize to YAML");

        // The critical assertion: `load_manifest_from_yaml` must accept the
        // YAML without error. If the key lists in `reshape_composite_to_manifest_file`
        // don't match `ManifestFile`'s fields, this will fail.
        let manifest = load_manifest_from_yaml(&yaml_string)
            .expect("reshaped YAML must round-trip through load_manifest_from_yaml");

        // Verify the round-trip preserved key fields.
        assert_eq!(manifest.id, "round-trip-test");
        assert_eq!(manifest.name, "Round Trip Test");
        assert_eq!(manifest.steps.len(), 1);
        assert_eq!(manifest.steps[0].ordinal, 1);
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.skills[0].id, "skill-a");
    }

    /// The inline refine manifest YAML in `refine_bundle` must parse via
    /// `load_manifest_from_yaml` and produce a single-step manifest pointing
    /// at `skill-bundler/bundler-evolve`. If the YAML drifts (typo in
    /// template_ref, missing field, wrong indentation), this test catches it
    /// before the refine path fails at runtime.
    #[test]
    fn refine_manifest_yaml_parses_and_targets_bundler_evolve() {
        let refine_manifest_yaml = "\
manifest:
  id: refine-bundle
  name: Refine Bundle
  description: Single-step goal-delta-driven bundle refinement
steps:
  - ordinal: 1
    action: know
    description: Refine bundle via goal-delta evolution
    renderer: minijinja
    template_ref: skill-bundler/bundler-evolve
    gas_cap: 6000
    timeout_seconds: 60
    input_mapping:
      bundle_name: \"{{ bundle_name }}\"
      current_manifest: \"{{ current_manifest }}\"
      changed_skills: \"{{ changed_skills }}\"
      goal_context: \"{{ goal_context }}\"
      goal_delta: \"{{ goal_delta }}\"
      convergence_failure_reason: \"{{ convergence_failure_reason }}\"
";

        let manifest =
            load_manifest_from_yaml(refine_manifest_yaml).expect("refine manifest YAML must parse");
        assert_eq!(manifest.id, "refine-bundle");
        assert_eq!(manifest.steps.len(), 1);
        assert_eq!(manifest.steps[0].ordinal, 1);
        assert_eq!(
            manifest.steps[0].template_ref.as_deref(),
            Some("skill-bundler/bundler-evolve")
        );
        assert_eq!(manifest.steps[0].renderer.as_deref(), Some("minijinja"));
        // The input_mapping must bind all 6 evolve template inputs.
        let mapping = manifest.steps[0]
            .input_mapping
            .as_ref()
            .expect("input_mapping present");
        for key in [
            "bundle_name",
            "current_manifest",
            "changed_skills",
            "goal_context",
            "goal_delta",
            "convergence_failure_reason",
        ] {
            assert!(
                mapping.get(key).is_some(),
                "input_mapping missing key: {key}"
            );
        }
    }

    /// Stub InferencePort for testing — returns an error on every call.
    /// Needed because BridgeManifestExecutor::new requires an InferencePort,
    /// and no NoopInferencePort exists in the test harness yet.
    #[cfg(test)]
    struct StubInferencePort;

    #[cfg(test)]
    impl InferencePort for StubInferencePort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::template::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { Err(hkask_types::InferenceError::Generation("stub".to_string())) })
        }
    }

    /// Stub ToolPort for testing — returns errors on every call.
    #[cfg(test)]
    struct StubToolPort;

    #[cfg(test)]
    impl hkask_capability::ToolPort for StubToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _token: &'a hkask_capability::DelegationToken,
        ) -> hkask_capability::ToolFuture<
            'a,
            Result<serde_json::Value, hkask_capability::ToolPortError>,
        > {
            Box::pin(async {
                Err(hkask_capability::ToolPortError::InvocationFailed(
                    "stub".to_string(),
                ))
            })
        }
        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { Vec::new() })
        }
        fn get_tool_info<'a>(
            &'a self,
            _tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            Box::pin(async { None })
        }
    }

    /// RR-0053 wiring test: build_executor MUST wire with_runtime_policy so
    /// the FIDES Source→Sink block (Layer 4) fires on production cascades.
    /// Without .with_runtime_policy(...), runtime_policy stays None and
    /// untrusted input flows to Sink tools unchecked (OWASP LLM06).
    #[test]
    fn build_executor_wires_runtime_policy() {
        // BridgeManifestExecutor::new requires a tokio Handle — create a
        // runtime for the test (build_executor doesn't actually run async
        // code, it just constructs the executor).
        let runtime = tokio::runtime::Runtime::new().expect("test tokio runtime");
        let _guard = runtime.enter();
        let executor = BridgeManifestExecutor::new(
            Arc::new(StubInferencePort),
            Arc::new(StubToolPort),
            PathBuf::from("/tmp/nonexistent-manifests"),
            PathBuf::from("/tmp/nonexistent-templates"),
            runtime.handle().clone(),
        );
        let manifest_executor = executor.build_executor(None, None);
        assert!(
            manifest_executor.runtime_policy_is_wired(),
            "build_executor must wire with_runtime_policy — without it the FIDES Source→Sink block is dead (OWASP LLM06, RR-0053)"
        );
    }
}
