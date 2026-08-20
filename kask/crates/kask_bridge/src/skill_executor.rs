//! `SkillManifestExecutor` adapter — bridges zed's `SkillTool` to hKask's `ManifestExecutor`.
//!
//! This is the D1 seam. When zed's `SkillTool` is constructed with a manifest executor,
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

// zed-kask: core skill classification lives in `agent_skills::CORE_SKILL_NAMES`.
// `kask_bridge` depends on `agent_skills` to access `is_core_skill` for the
// registry seeder's core-vs-user split.
use agent::SkillExecutionError;
use agent_skills::is_core_skill;

/// Context keys the runtime injects itself (not user-supplied params), excluded
/// from the unknown-key check in `validate_inputs`. `task` is injected by the
/// `SkillTool`/slash-command path; the `*_model` keys are injected by
/// `execute_skill` below. Listed here so validation never flags them as typos.
const SKILL_CONTEXT_SYSTEM_KEYS: &[&str] = &[
    "task",
    "thread_model",
    "embedding_model",
    "classifier_model",
    "ocr_model",
    "default_model",
    "qa_model",
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
    /// Cache of parsed manifests keyed by skill name, with file modification
    /// time for invalidation. Avoids re-reading + re-parsing the same YAML on
    /// every invocation (e.g. repeated skill calls within a conversation).
    manifest_cache: std::sync::Mutex<
        std::collections::HashMap<String, (std::time::SystemTime, hkask_templates::BundleManifest)>,
    >,
    /// Optional RegulationLedger for recording skill feedback spans
    /// (`reg.skill.<id>.outcome`). When `None`, skill outcomes are not
    /// persisted to the regulation system (tests, pre-login).
    regulation_ledger: Option<Arc<tokio::sync::RwLock<hkask_regulation::RegulationLedger>>>,
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
            manifest_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            regulation_ledger: None,
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

    /// Wire a RegulationLedger for recording skill feedback spans.
    /// When set, `execute_skill` records `reg.skill.<id>.outcome` spans
    /// after each invocation (success or failure), closing the feedback
    /// loop for drift detection and gemba walk review.
    #[must_use]
    pub fn with_regulation_ledger(
        mut self,
        ledger: Arc<tokio::sync::RwLock<hkask_regulation::RegulationLedger>>,
    ) -> Self {
        self.regulation_ledger = Some(ledger);
        self
    }

    /// Validate a skill's golden-output fixtures by running the cascade
    /// against each fixture's input and comparing the output exactly.
    /// Returns a report of pass/fail per fixture. This is a maintenance-time
    /// check (used by `skill-maintenance` and the gemba walk briefing), not
    /// a runtime gate on every invocation.
    ///
    /// Skills without `golden_outputs` in their manifest return an empty
    /// report (not an error) — golden-output validation is opt-in and only
    /// meaningful for skills with deterministic-ish output contracts.
    pub async fn validate_golden_outputs_inner(
        &self,
        skill_name: &str,
    ) -> Result<Vec<GoldenOutputResult>, String> {
        let manifest = self.load_cached_manifest(skill_name)?;
        let fixtures = match &manifest.golden_outputs {
            Some(f) if !f.is_empty() => f,
            _ => return Ok(Vec::new()),
        };

        let mut results = Vec::with_capacity(fixtures.len());
        for (i, fixture) in fixtures.iter().enumerate() {
            let input_context: HashMap<String, Value> = serde_json::from_str(&fixture.input)
                .map_err(|e| format!("golden_outputs[{i}] input is not valid JSON: {e}"))?;

            // Run the cascade directly via run_manifest_cascade_with_manifest,
            // bypassing execute_skill's span emission. Golden-output validation
            // must NOT create spurious outcome/convergence spans that would
            // pollute the drift detector's signal (adversarial review finding 1).
            let result = self
                .run_manifest_cascade_with_manifest(
                    &manifest,
                    input_context,
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                )
                .await;

            let result = match result {
                Ok(outcome) => {
                    let actual = final_result_as_string(&outcome);
                    let passed = actual == fixture.expected_output;
                    GoldenOutputResult {
                        fixture_index: i,
                        passed,
                        actual: Some(actual),
                        expected: fixture.expected_output.clone(),
                        error: if passed {
                            None
                        } else {
                            Some("output does not match expected".to_string())
                        },
                    }
                }
                Err(msg) => GoldenOutputResult {
                    fixture_index: i,
                    passed: false,
                    actual: None,
                    expected: fixture.expected_output.clone(),
                    error: Some(msg),
                },
            };
            results.push(result);
        }
        Ok(results)
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

    /// Load and parse a skill manifest, with an in-memory cache keyed by
    /// skill name and invalidated by file modification time. Avoids
    /// re-reading + re-parsing the same YAML on repeated invocations of
    /// the same skill within a conversation.
    fn load_cached_manifest(
        &self,
        skill_name: &str,
    ) -> Result<hkask_templates::BundleManifest, String> {
        let path = self.manifest_path(skill_name);
        let mtime = std::fs::metadata(&path)
            .map(|m| m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH))
            .map_err(|e| {
                format!(
                    "No manifest found for skill '{skill_name}' on disk at {}: {e}",
                    path.display()
                )
            })?;

        match self.manifest_cache.lock() {
            Ok(cache) => {
                if let Some((cached_mtime, manifest)) = cache.get(skill_name)
                    && *cached_mtime == mtime
                {
                    return Ok(manifest.clone());
                }
            }
            Err(_) => tracing::warn!(
                target: "hkask.bridge.manifest_cache",
                skill = skill_name,
                "manifest cache lock poisoned — re-reading from disk",
            ),
        }

        let yaml = self.manifest_yaml(skill_name).ok_or_else(|| {
            format!(
                "No manifest found for skill '{skill_name}' on disk at {}",
                path.display()
            )
        })?;
        let manifest = load_manifest_from_yaml(&yaml)
            .map_err(|e| format!("Failed to load manifest '{skill_name}': {e}"))?;

        if let Ok(mut cache) = self.manifest_cache.lock() {
            cache.insert(skill_name.to_string(), (mtime, manifest.clone()));
        }

        Ok(manifest)
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
    /// Shared spawn tail: build executor, spawn on tokio, await.
    async fn spawn_cascade(
        &self,
        manifest: hkask_templates::BundleManifest,
        context: HashMap<String, Value>,
        prior_messages: Vec<hkask_types::ports::inference_types::ChatMessage>,
        memory_snippets: Vec<hkask_types::ports::memory_port::MemorySnippet>,
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<CascadeOutcome, String> {
        let executor = self.build_executor(progress, title, prior_messages, memory_snippets);
        let join_handle = self.tokio_handle.spawn(async move {
            executor
                .execute_manifest_into(manifest, context)
                .await
                .map_err(|e| format!("Manifest execution failed: {e}"))
        });
        join_handle
            .await
            .map_err(|e| format!("Manifest execution task failed: {e}"))?
    }

    async fn run_manifest_cascade(
        &self,
        skill_name: &str,
        mut context: HashMap<String, Value>,
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<CascadeOutcome, String> {
        let manifest = self.load_cached_manifest(skill_name)?;
        if !manifest.is_skill() {
            return Err(format!(
                "Skill '{skill_name}' has category '{}' — only `skill` manifests may execute via the skill tool",
                manifest
                    .category
                    .map_or_else(|| "skill (unset)".to_string(), |c| c.to_string())
            ));
        }
        self.inject_model_defaults(&mut context);
        self.spawn_cascade(manifest, context, Vec::new(), Vec::new(), progress, title)
            .await
    }

    async fn run_manifest_cascade_with_manifest(
        &self,
        manifest: &hkask_templates::BundleManifest,
        mut context: HashMap<String, Value>,
        prior_messages: Vec<hkask_types::ports::inference_types::ChatMessage>,
        memory_snippets: Vec<hkask_types::ports::memory_port::MemorySnippet>,
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<CascadeOutcome, String> {
        if !manifest.is_skill() {
            return Err(format!(
                "Manifest '{}' has category '{}' — only `skill` manifests may execute via the skill tool",
                manifest.id,
                manifest
                    .category
                    .map_or_else(|| "skill (unset)".to_string(), |c| c.to_string())
            ));
        }
        self.inject_model_defaults(&mut context);
        self.spawn_cascade(
            manifest.clone(),
            context,
            prior_messages,
            memory_snippets,
            progress,
            title,
        )
        .await
    }

    /// Inject config-driven model defaults into the template context.
    /// Factored out of `execute_skill` so both paths share the same injection.
    fn inject_model_defaults(&self, context: &mut HashMap<String, Value>) {
        // Function-based defaults (no env var override).
        const FN_DEFAULTS: &[(&str, fn() -> String)] = &[
            (
                "embedding_model",
                hkask_inference::model_constants::embedding_model,
            ),
            (
                "classifier_model",
                hkask_inference::model_constants::classifier_model,
            ),
            ("ocr_model", hkask_inference::model_constants::ocr_model),
        ];
        for (key, f) in FN_DEFAULTS {
            if !context.contains_key(*key) {
                context.insert((*key).to_string(), Value::String(f()));
            }
        }
        // Env-var-based defaults.
        const ENV_DEFAULTS: &[(&str, &str, &str)] = &[
            (
                "default_model",
                "HKASK_DEFAULT_MODEL",
                hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL,
            ),
            (
                "qa_model",
                "HKASK_QA_MODEL",
                hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL,
            ),
            (
                "vision_model",
                "HKASK_MEDIA_VISION_MODEL",
                hkask_inference::model_constants::DEFAULT_VISION_MODEL,
            ),
        ];
        for (key, env_var, default) in ENV_DEFAULTS {
            if !context.contains_key(*key) {
                let value = std::env::var(env_var).unwrap_or_else(|_| default.to_string());
                context.insert((*key).to_string(), Value::String(value));
            }
        }
    }

    /// Construct a `ManifestExecutor` with the bridge's inference/tools and
    /// profile resolver. Factored out of `execute_skill` so both paths share
    /// the same executor wiring.
    ///
    /// The FIDES runtime policy (`DefaultPolicy`) used to be wired here. It was
    /// removed, not unwired: its `Source`→`Sink` block read two constants (all
    /// tools were labelled `Pure`, and the untrusted-input flag was always
    /// false), so it denied nothing. Re-wiring a policy only helps once tools
    /// carry real taint labels.
    fn build_executor(
        &self,
        progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
        prior_messages: Vec<hkask_types::ports::inference_types::ChatMessage>,
        memory_snippets: Vec<hkask_types::ports::memory_port::MemorySnippet>,
    ) -> ManifestExecutor {
        let executor = ManifestExecutor::new(
            self.inference.clone(),
            self.tools.clone(),
            hkask_types::template::LLMParameters::default(),
        )
        .with_template_base_path(self.registry_templates_dir.clone());

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

        let executor = if let Some(title) = title {
            executor.with_title(title)
        } else {
            executor
        };

        let executor = executor.with_cascade_context(prior_messages, memory_snippets);

        // Wire the global concurrency limiter into the cascade's `Infra` so
        // step actions (`execute_parallel`, `execute_tool_invoke`,
        // `execute_select`) can acquire permits before cloud inference / tool
        // calls. `None` before startup wiring — callers skip gating.
        let executor = if let Some(limiter) = crate::global_concurrency_limiter() {
            executor.with_concurrency_limiter(Arc::clone(limiter))
        } else {
            executor
        };

        executor
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
/// `registry_root` is the on-disk registry root (D28:
/// `{kask_data_dir}/skills/registry/`). Writes:
/// - `registry_root/manifests/<skill>.yaml` (process manifests)
/// - `registry_root/templates/<skill>/manifest.yaml` (per-skill template manifests)
/// - `registry_root/templates/<skill>/<file>.j2` (Jinja2 templates)
/// - `registry_root/templates/<skill>/<file>.yaml` (YAML sub-manifests / reference docs)
pub async fn seed_registry_to_disk(fs: &dyn Fs, registry_root: &Path) {
    let manifests_dir = registry_root.join("manifests");
    for (name, content) in hkask_templates::process_manifest_seed() {
        let path = manifests_dir.join(format!("{name}.yaml"));
        let is_core = is_core_skill(name);
        // Core skills are always overwritten; user skills are seed-once.
        if fs.is_file(&path).await && !is_core {
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
        let is_core = is_core_skill(skill);
        if fs.is_file(&path).await && !is_core {
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
        // Extract the skill name from the key (first path segment).
        let skill_name = key.split('/').next().unwrap_or("");
        let is_core = is_core_skill(skill_name);
        if fs.is_file(&path).await && !is_core {
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
        let skill_name = key.split('/').next().unwrap_or("");
        let is_core = is_core_skill(skill_name);
        if fs.is_file(&path).await && !is_core {
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
        // Use path.is_file() instead of manifest_yaml() to avoid reading
        // the full file content just to check existence. Called once per
        // available skill name on every agent turn.
        self.manifest_path(skill_name).is_file()
    }

    async fn execute_skill(
        &self,
        skill_name: &str,
        context: HashMap<String, Value>,
        prior_messages: Vec<agent::CascadeChatMessage>,
        memory_snippets: Vec<agent::MemorySnippetRecord>,
        progress: Option<agent::CascadeProgress>,
        title: Option<agent::CascadeProgress>,
    ) -> Result<String, SkillExecutionError> {
        // Load the manifest with caching. Loaded before validation so we can
        // check the caller-supplied context against declared `inputs` (Layer A)
        // before injecting runtime defaults.
        let manifest = self.load_cached_manifest(skill_name).map_err(|e| {
            SkillExecutionError::CompileTime {
                skill_name: skill_name.to_string(),
                phase: "load",
                message: e,
            }
        })?;

        // Enforce the category labelling system at the execution boundary.
        if !manifest.is_skill() {
            return Err(SkillExecutionError::CompileTime {
                skill_name: skill_name.to_string(),
                phase: "category_check",
                message: format!(
                    "Skill '{skill_name}' has category '{}' — only `skill` manifests may execute via the skill tool",
                    manifest
                        .category
                        .map_or_else(|| "skill (unset)".to_string(), |c| c.to_string())
                ),
            });
        }

        // Profile enforcement (proposer/evaluator separation): if any step
        // declares a `profile`, verify that `terminal` is NOT enabled.
        let needs_profile_check = manifest.steps.iter().any(|s| s.profile.is_some());
        if needs_profile_check {
            match &self.profile_resolver {
                Some(resolver) => {
                    if resolver.is_tool_enabled("terminal") {
                        if let Some(step) = manifest.steps.iter().find(|s| s.profile.is_some())
                            && let Some(profile_name) = step.profile.as_ref()
                        {
                            return Err(SkillExecutionError::CompileTime {
                                skill_name: skill_name.to_string(),
                                phase: "profile_enforcement",
                                message: format!(
                                    "Step {} declares profile '{}' but the `terminal` tool is enabled. \
                                     This violates proposer/evaluator separation — a proposer with terminal \
                                     can evaluate its own tests (self-confirming loop anti-pattern). \
                                     Remediation: remove `terminal` from the '{}' profile in settings, \
                                     or bind this step to a profile without `terminal` (e.g. `ask`).",
                                    step.ordinal, profile_name, profile_name
                                ),
                            });
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

        // Layer A: enforce the manifest's declared `inputs` contract.
        if let Err(e) = validate_inputs(
            manifest.enforce_inputs,
            manifest.inputs.as_ref(),
            &context,
            SKILL_CONTEXT_SYSTEM_KEYS,
        ) {
            return Err(SkillExecutionError::CompileTime {
                skill_name: skill_name.to_string(),
                phase: "input_validation",
                message: format!("Skill '{skill_name}' input validation failed: {e}"),
            });
        }

        // Delegate to the shared cascade path — model defaults injection,
        // executor construction, and tokio spawning are handled there.
        // This eliminates the duplicated spawn block that was previously
        // inline in this method.
        // Convert the agent crate's local types to hkask_types types for
        // the executor. The agent crate cannot depend on hkask_types
        // (circular dependency), so the conversion happens here at the seam.
        let prior_messages = prior_messages
            .into_iter()
            .map(|m| hkask_types::ports::inference_types::ChatMessage {
                role: m.role,
                content: m.content,
            })
            .collect::<Vec<_>>();
        let memory_snippets = memory_snippets
            .into_iter()
            .map(|s| hkask_types::ports::memory_port::MemorySnippet {
                text: s.text,
                source: s.source,
                confidence: s.confidence,
                relevance_score: s.relevance_score,
            })
            .collect::<Vec<_>>();

        let result = self
            .run_manifest_cascade_with_manifest(
                &manifest,
                context,
                prior_messages,
                memory_snippets,
                progress,
                title,
            )
            .await;

        // Record the skill outcome span (reg.skill.<id>.outcome) for the
        // regulation feedback loop. Best-effort: if the ledger is not wired
        // (tests, pre-login) or the write fails, the result is unaffected.
        let skill_id = skill_name;
        let outcome_payload = match &result {
            Ok(outcome) => {
                let mut payload = serde_json::json!({
                    "success": true,
                    "skill_id": skill_id,
                    "exit_kind": format!("{:?}", outcome.exit_kind),
                });
                // Surface the on_failure resume text to the operator when the
                // cascade escalated via an on_failure config (follow-up #2).
                // Without this, the operator sees ExitKind::Escalated but not
                // the author's resume instruction.
                if let Some(ref resume) = outcome.resume_text {
                    payload["resume_text"] = serde_json::Value::String(resume.clone());
                }
                payload
            }
            Err(msg) => serde_json::json!({
                "success": false,
                "skill_id": skill_id,
                "error": msg,
            }),
        };
        if let Some(ref ledger) = self.regulation_ledger {
            let ledger_guard = ledger.read().await;
            ledger_guard
                .record_skill_span(skill_id, "outcome", outcome_payload)
                .await;
            // Also record a convergence span with iteration count and exit
            // kind, so the gemba walk can trend convergence quality per skill.
            if let Ok(ref outcome) = result {
                let convergence_payload = serde_json::json!({
                    "iterations": outcome.iterations,
                    "exit_kind": format!("{:?}", outcome.exit_kind),
                    "converged": matches!(outcome.exit_kind, hkask_templates::ExitKind::Converged),
                });
                ledger_guard
                    .record_skill_span(skill_id, "convergence", convergence_payload)
                    .await;
            }
        }

        let result = result.map_err(|e| SkillExecutionError::Runtime {
            skill_name: skill_name.to_string(),
            phase: "cascade",
            message: e,
        })?;

        // Post-execution golden-output validation (Step 4): when the manifest
        // declares `golden_outputs`, run the validation suite as a quality
        // signal after the main cascade. Non-fatal — the main result is
        // returned regardless. Failures are logged at `warn!` and recorded to
        // the regulation ledger as a `golden_output_validation` span so the
        // gemba walk can trend validation quality per skill.
        if manifest
            .golden_outputs
            .as_ref()
            .is_some_and(|f| !f.is_empty())
        {
            match self.validate_golden_outputs_inner(skill_name).await {
                Ok(validation_results) => {
                    let passed = validation_results.iter().filter(|r| r.passed).count();
                    let total = validation_results.len();
                    if passed != total {
                        tracing::warn!(
                            target: "hkask.skill.golden_outputs",
                            skill = skill_name,
                            passed, total,
                            "golden-output validation: {}/{} fixtures passed",
                            passed, total,
                        );
                    } else {
                        tracing::info!(
                            target: "hkask.skill.golden_outputs",
                            skill = skill_name,
                            passed, total,
                            "golden-output validation: all fixtures passed",
                        );
                    }
                    if let Some(ref ledger) = self.regulation_ledger {
                        let ledger_guard = ledger.read().await;
                        ledger_guard
                            .record_skill_span(
                                skill_name,
                                "golden_output_validation",
                                serde_json::json!({
                                    "passed": passed,
                                    "total": total,
                                    "results": validation_results.iter().map(|r| {
                                        serde_json::json!({
                                            "fixture_index": r.fixture_index,
                                            "passed": r.passed,
                                            "error": r.error,
                                        })
                                    }).collect::<Vec<_>>(),
                                }),
                            )
                            .await;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.skill.golden_outputs",
                        skill = skill_name,
                        error = %e,
                        "golden-output validation failed to run (non-fatal)",
                    );
                }
            }
        }

        // (K5) `extract_final_step_result` selects `last_result_step`'s
        // value (deterministic — the machine tracks it, O(1)).

        Ok(final_result_as_string(&result))
    }
}

/// Extract the cascade's final result as a string, reusing the canonical
/// typed selector `hkask_templates::extract_final_step_result` (K5:
/// `last_result_step`, not the retired ordinal-keyed HashMap scan). Falls back
/// to the full context JSON (materialized) when no step stored a result — a
/// bridge policy layer on top of the shared selector, which returns
/// `Value::Null`.
fn value_to_string(value: &Value, context: &hkask_templates::step_context::StepContext) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => serde_json::to_string(&context.materialize()).unwrap_or_default(),
        other => other.to_string(),
    }
}

fn final_result_as_string(outcome: &CascadeOutcome) -> String {
    let value = hkask_templates::extract_final_step_result(outcome);
    value_to_string(&value, &outcome.context)
}
