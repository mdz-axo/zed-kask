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

/// Result of a single golden-output fixture validation. Returned by
/// `BridgeManifestExecutor::validate_golden_outputs`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GoldenOutputResult {
    /// Index of the fixture in the manifest's `golden_outputs` list.
    pub fixture_index: usize,
    /// Whether the skill's output matched the expected output exactly.
    pub passed: bool,
    /// The actual output from the skill cascade (`None` if execution failed).
    pub actual: Option<String>,
    /// The expected output from the fixture.
    pub expected: String,
    /// Error message if the skill failed to execute or the output didn't match.
    pub error: Option<String>,
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
    /// Optional VerificationStore for grounding skill cascade outputs.
    /// When set, `execute_skill` calls `enforce_for_agent` with
    /// `source: "skill_cascade"`, the skill name as `agent_id`,
    /// `"skill"` as `agent_type`, and the cascade output + tool-call
    /// summary. Grounding checks for fabricated file paths and unsourced
    /// claims in the skill's output (Phase 5).
    verification_store: Option<Arc<hkask_verification::VerificationStore>>,
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
            verification_store: None,
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

    /// Wire a VerificationStore for grounding skill cascade outputs.
    /// When set, `execute_skill` calls `enforce_for_agent` after the
    /// cascade completes, checking for fabricated file paths and unsourced
    /// claims. The tool-call summary from the cascade is passed as the
    /// `tool_calls` argument (Phase 5: skill cascade grounding).
    #[must_use]
    pub fn with_verification_store(
        mut self,
        store: Arc<hkask_verification::VerificationStore>,
    ) -> Self {
        self.verification_store = Some(store);
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
                    let actual = extract_final_step_result(&outcome);
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

        // Fast path: cache hit with matching mtime
        if let Ok(cache) = self.manifest_cache.lock() {
            if let Some((cached_mtime, manifest)) = cache.get(skill_name) {
                if *cached_mtime == mtime {
                    return Ok(manifest.clone());
                }
            }
        }

        // Slow path: read from disk + parse
        let yaml = self.manifest_yaml(skill_name).ok_or_else(|| {
            format!(
                "No manifest found for skill '{skill_name}' on disk at {}",
                path.display()
            )
        })?;
        let manifest = load_manifest_from_yaml(&yaml)
            .map_err(|e| format!("Failed to load manifest '{skill_name}': {e}"))?;

        // Update cache
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
                "Skill '{skill_name}' has category '{:?}' — only `skill` manifests may execute via the skill tool",
                manifest.category
            ));
        }

        // Inject model defaults (same as execute_skill).
        self.inject_model_defaults(&mut context);

        // Sub-cascade path (run_manifest_cascade resolves by name) — no
        // thread context. Prior messages and memory snippets are only
        // injected at the top-level skill invocation site.
        let executor = self.build_executor(progress, title, Vec::new(), Vec::new());

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

    /// Variant of `run_manifest_cascade` that takes a pre-loaded manifest
    /// instead of resolving by name. Used by `refine_bundle` to run the
    /// minimal single-step evolve manifest without a registry lookup.
    async fn run_manifest_cascade_with_manifest(
        &self,
        manifest: &hkask_templates::BundleManifest,
        mut context: HashMap<String, Value>,
        prior_messages: Vec<hkask_types::ports::inference_types::ChatMessage>,
        memory_snippets: Vec<hkask_types::ports::memory_port::MemorySnippet>,
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

        // NOTE: Short-term (prior_messages) and long-term (memory_snippets)
        // context are injected structurally via `build_cascade_messages` in
        // `step_actions.rs` — prepended as a system message to every
        // `execute_select` inference call. They are NOT injected as template
        // fields (`{{ session_history }}` / `{{ memory_context }}`).
        //
        // Per the design requirement: memory context must be part of the
        // system prompt or injected with every template call — not an
        // optional template field that templates must opt into. The
        // structural injection in `build_cascade_messages` ensures every
        // template step sees the memory and prior turns, regardless of
        // whether the template references them by name.
        //
        // `prior_outcomes` (intra-cascade step results for Brier scoring)
        // remains a separate template field — it is NOT conflated with
        // memory context.
        let executor = self.build_executor(progress, title, prior_messages, memory_snippets);

        let join_handle = self.tokio_handle.spawn({
            let manifest = manifest.clone();
            async move {
                executor
                    .execute_manifest_into(manifest, context)
                    .await
                    .map_err(|e| format!("Manifest execution failed: {e}"))
            }
        });

        join_handle
            .await
            .map_err(|e| format!("Manifest execution task failed: {e}"))?
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
                Value::String(std::env::var("HKASK_MEDIA_TTS_MODEL").unwrap_or_else(|_| {
                    hkask_inference::model_constants::DEFAULT_TTS_MODEL.to_string()
                })),
            );
        }
        if !context.contains_key("stt_model") {
            context.insert(
                "stt_model".into(),
                Value::String(std::env::var("HKASK_MEDIA_STT_MODEL").unwrap_or_else(|_| {
                    hkask_inference::model_constants::DEFAULT_STT_MODEL.to_string()
                })),
            );
        }
        if !context.contains_key("vision_model") {
            context.insert(
                "vision_model".into(),
                Value::String(
                    std::env::var("HKASK_MEDIA_VISION_MODEL").unwrap_or_else(|_| {
                        hkask_inference::model_constants::DEFAULT_VISION_MODEL.to_string()
                    }),
                ),
            );
        }
        if !context.contains_key("image_gen_model") {
            context.insert(
                "image_gen_model".into(),
                Value::String(
                    std::env::var("HKASK_MEDIA_IMAGE_GEN_MODEL").unwrap_or_else(|_| {
                        hkask_inference::model_constants::DEFAULT_IMAGE_GEN_MODEL.to_string()
                    }),
                ),
            );
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
                    "Skill '{skill_name}' has category '{:?}' — only `skill` manifests may execute via the skill tool",
                    manifest.category
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
        // Phase 5: Ground the cascade output via the VerificationStore when
        // wired. The tool-call summary from the cascade (outcome.tool_calls)
        // is passed so the grounding check can verify that sourced fields
        // (e.g. deliverable_path) were produced by a successful tool call.
        // The cleaned output replaces the raw output — nulled fields are
        // removed before the agent sees the result.
        let raw_value = hkask_templates::extract_final_step_result(&result);
        let tool_calls = &result.tool_calls;
        let grounded_value = if let Some(ref store) = self.verification_store {
            let response_str = match &raw_value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let (grounding_result, cleaned) = store.enforce_for_agent(
                "skill_cascade",
                skill_name,
                "skill",
                &raw_value,
                tool_calls,
                &response_str,
                // Skill cascade: top-level skill execution, not an
                // agent-to-agent composition hop. No parent envelope.
                &[],
            );
            if let Some(ref gr) = grounding_result {
                if !gr.nulled_fields.is_empty() {
                    tracing::warn!(
                        target: "hkask.verification.skill_cascade",
                        skill = skill_name,
                        nulled_fields = ?gr.nulled_fields,
                        narrative_leaks = ?gr.narrative_leaks,
                        "skill cascade grounding: nulled {} unsourced field(s), found {} narrative leak(s)",
                        gr.nulled_fields.len(),
                        gr.narrative_leaks.len(),
                    );
                }
            }
            cleaned
        } else {
            raw_value
        };
        Ok(final_result_as_string(&grounded_value, &result.context))
    }

    async fn compose_and_execute_bundle(
        &self,
        skill_names: &[String],
        task: &str,
        context: HashMap<String, Value>,
        progress: Option<agent::CascadeProgress>,
        title: Option<agent::CascadeProgress>,
    ) -> Result<agent::BundleExecutionResult, String> {
        // Phase 1: Run all skills concurrently via tokio. Each skill gets
        // its own manifest cascade with the shared context + task. We use
        // allSettled semantics — a skill error is captured as an error
        // string in the output array, not a hard abort. The merge step
        // handles errored skills explicitly.
        let task_string = task.to_string();
        let mut cascade_futures = Vec::with_capacity(skill_names.len());

        for skill_name in skill_names {
            let skill_context = {
                let mut ctx = context.clone();
                ctx.insert("task".to_string(), Value::String(task_string.clone()));
                ctx.insert(
                    "user_intent".to_string(),
                    Value::String(task_string.clone()),
                );
                self.inject_model_defaults(&mut ctx);
                ctx
            };

            let skill_name_owned = skill_name.clone();
            let progress_clone = progress.clone();
            let title_clone = title.clone();

            // Collect the cascade futures without spawning here — each
            // `run_manifest_cascade` call spawns its own task on the tokio
            // runtime internally, so `join_all` over the futures drives all N
            // cascades concurrently. Spawning here would require `&self` to
            // escape into a `'static` future, which the borrow checker
            // rejects (E0521).
            let future = async move {
                let result = self
                    .run_manifest_cascade(
                        &skill_name_owned,
                        skill_context,
                        progress_clone,
                        title_clone,
                    )
                    .await;
                (skill_name_owned, result)
            };
            cascade_futures.push(future);
        }

        // Await all cascades concurrently (not sequentially).
        let cascade_results = futures::future::join_all(cascade_futures).await;

        // Collect outputs in the same order as skill_names. Errored
        // skills produce a JSON object with an `error` field so the merge
        // template can distinguish them from successful outputs.
        let mut skill_outputs = Vec::with_capacity(cascade_results.len());
        for (skill_name, result) in cascade_results {
            match result {
                Ok(outcome) => {
                    let output_text = extract_final_step_result(&outcome);
                    skill_outputs.push(serde_json::json!({
                        "skill": skill_name,
                        "output": output_text,
                        "errored": false,
                    }));
                }
                Err(error) => {
                    tracing::warn!(
                        target: "reg.skill.bundle_compose",
                        skill = %skill_name,
                        error = %error,
                        "Parallel skill cascade failed — including error in merge input",
                    );
                    skill_outputs.push(serde_json::json!({
                        "skill": skill_name,
                        "output": error,
                        "errored": true,
                    }));
                }
            }
        }

        // Phase 2: Run the skill-bundler merge manifest to synthesize all
        // outputs into a single unified report.
        let merge_context = {
            let mut ctx = HashMap::new();
            ctx.insert(
                "skill_names".to_string(),
                Value::Array(
                    skill_names
                        .iter()
                        .map(|s| Value::String(s.clone()))
                        .collect(),
                ),
            );
            ctx.insert("task".to_string(), Value::String(task_string));
            ctx.insert("skill_outputs".to_string(), Value::Array(skill_outputs));
            ctx
        };

        let merge_result = self
            .run_manifest_cascade("skill-bundler", merge_context, progress, title)
            .await?;

        let merged_report = extract_final_step_result(&merge_result);

        Ok(agent::BundleExecutionResult {
            output: merged_report,
            composed_skill_names: skill_names.to_vec(),
        })
    }

    /// Execute a pipeline manifest by file path. Unlike `execute_skill`,
    /// this loads the manifest from an explicit file path (not a skill name
    /// lookup), skips the `is_skill()` guard, and runs the cascade.
    /// The manifest must declare `category: pipeline`.
    async fn execute_pipeline(
        &self,
        manifest_path: &str,
        resume_from: Option<String>,
        dry_run: bool,
        progress: Option<agent::CascadeProgress>,
        title: Option<agent::CascadeProgress>,
    ) -> Result<String, String> {
        // The manifest path is already resolved to an absolute path and
        // contained by the PipelineTool (via find_project_path + absolute_path).
        // The bridge is process-global and cannot do per-project containment,
        // so containment is enforced at the tool layer, not here.
        let yaml = std::fs::read_to_string(manifest_path)
            .map_err(|e| format!("Failed to read pipeline manifest at '{manifest_path}': {e}"))?;

        let manifest = load_manifest_from_yaml(&yaml)
            .map_err(|e| format!("Failed to parse pipeline manifest: {e:?}"))?;

        // Verify it's a pipeline manifest, not a skill.
        if manifest.is_skill() {
            return Err(format!(
                "Manifest '{}' is category 'skill' — use the skill tool for skill manifests. \
                 execute_pipeline is for category: pipeline manifests.",
                manifest.id
            ));
        }

        if dry_run {
            return Ok(format!(
                "Dry run: manifest '{}' parsed successfully. {} steps, category: {:?}.",
                manifest.id,
                manifest.steps.len(),
                manifest.category
            ));
        }

        // If resume_from is specified, skip steps before the named step.
        // We do this by building a reduced manifest with only the steps
        // from the resume point onward.
        let manifest = if let Some(ref resume_id) = resume_from {
            let resume_ordinal = manifest
                .steps
                .iter()
                .find(|s| s.id.as_deref() == Some(resume_id.as_str()))
                .map(|s| s.ordinal)
                .ok_or_else(|| format!("resume_from: step '{resume_id}' not found in manifest"))?;
            let mut reduced = manifest.clone();
            reduced.steps.retain(|s| s.ordinal >= resume_ordinal);
            reduced
        } else {
            manifest
        };

        let mut context = HashMap::new();
        self.inject_model_defaults(&mut context);

        // Pipeline path — no thread context (pipelines are invoked via the
        // pipeline tool, not from a chat thread).
        let executor = self.build_executor(progress, title, Vec::new(), Vec::new());

        let join_handle = self.tokio_handle.spawn({
            let manifest = manifest.clone();
            async move {
                executor
                    .execute_manifest_into(manifest, context)
                    .await
                    .map_err(|e| format!("Pipeline execution failed: {e}"))
            }
        });

        let result = join_handle
            .await
            .map_err(|e| format!("Pipeline execution task failed: {e}"))??;

        Ok(extract_final_step_result(&result))
    }

    async fn record_operator_feedback(
        &self,
        skill_name: &str,
        disposition: &str,
        comments: Option<&str>,
    ) -> Result<(), String> {
        let ledger = self.regulation_ledger.as_ref().ok_or_else(|| {
            "Regulation ledger not wired — operator feedback cannot be recorded".to_string()
        })?;

        let payload = serde_json::json!({
            "disposition": disposition,
            "comments": comments.unwrap_or(""),
            "skill_name": skill_name,
        });

        let ledger_guard = ledger.read().await;
        ledger_guard
            .record_skill_span(skill_name, "operator_feedback", payload)
            .await;
        Ok(())
    }

    async fn validate_golden_outputs(&self, skill_name: &str) -> Result<String, String> {
        let results = self.validate_golden_outputs_inner(skill_name).await?;
        serde_json::to_string(&results)
            .map_err(|e| format!("Failed to serialize golden-output results: {e}"))
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
    final_result_as_string(&value, &outcome.context)
}

/// Bridge Value→String policy, shared by the raw-extraction path
/// (`extract_final_step_result`) and the post-grounding path in
/// `execute_skill` (which needs the `Value` form for `enforce_for_agent`
/// before stringifying). Keeping one policy guarantees both paths unwrap
/// strings, serialize objects, and fall back to the materialized context
/// identically.
fn final_result_as_string(
    value: &Value,
    context: &hkask_templates::step_context::StepContext,
) -> String {
    match value {
        // F1: `render` steps store Value::String(rendered_text). Calling
        // `to_string()` on Value::String produces `"s"` (quoted + escaped),
        // double-encoding the output. Unwrap to the inner string so the
        // agent sees the raw rendered text (JSON or prose) directly.
        // `select` steps store Value::Object(parsed_json), which falls
        // through to `other.to_string()` producing the JSON string.
        Value::String(s) => s.clone(),
        Value::Null => serde_json::to_string(&context.materialize()).unwrap_or_default(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_templates::budget::BudgetSnapshot;
    use hkask_templates::step_context::StepContext;
    use hkask_templates::step_graph::{ExitKind, StepId};
    use hkask_templates::step_machine::CascadeOutcome;
    use serde_json::json;

    /// Build a minimal `CascadeOutcome` for the bridge's `extract_final_step_result`
    /// tests: typed context + machine-tracked `last_result_step`. Zeroed budget.
    fn outcome_with_last(context: StepContext, last: Option<StepId>) -> CascadeOutcome {
        CascadeOutcome {
            context,
            iterations: 1,
            exit_kind: ExitKind::Converged,
            last_result_step: last,
            budget_snapshot: BudgetSnapshot {
                rjoule_used: 0.0,
                rjoule_cap: 0.0,
                rjoule_remaining: 0.0,
                rjoule_enabled: false,
            },
            resume_text: None,
            tool_calls: Vec::new(),
        }
    }

    /// (K5) the retired ordinal-keyed HashMap scan is gone; the bridge's
    /// `extract_final_step_result` now selects `last_result_step`'s value from
    /// the typed `CascadeOutcome`. Deterministic by construction (no randomized
    /// HashMap order). Pins that contract + the null→full-context fallback.
    ///
    /// F1: `Value::String` is unwrapped to the inner string (not quoted),
    /// so `render` steps that store `Value::String(rendered_text)` return
    /// the raw text, not a double-quoted/escaped version.
    #[test]
    fn extract_final_step_result_returns_last_result_step() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!("first"));
        ctx.store_result(2, 3, json!("third"));
        ctx.store_result(1, 2, json!("second"));
        let outcome = outcome_with_last(ctx, Some(2));
        let out = extract_final_step_result(&outcome);
        assert_eq!(
            out, "third",
            "must return last_result_step's inner string value (step_id 2 = ordinal 3), not the double-quoted form"
        );
    }

    /// F1: `Value::String` unwrapping must not affect `Value::Object` —
    /// `select` steps store parsed JSON objects, which must still serialize
    /// to a JSON string via `to_string()`.
    #[test]
    fn extract_final_step_result_serializes_object_not_string() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!({"answer": 42}));
        let outcome = outcome_with_last(ctx, Some(0));
        let out = extract_final_step_result(&outcome);
        assert_eq!(
            out, "{\"answer\":42}",
            "Value::Object must serialize to JSON string, not be unwrapped"
        );
    }

    #[test]
    fn extract_final_step_result_ignores_protocol_and_named_keys() {
        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!({"answer": 42}));
        ctx.insert_protocol("task".into(), json!("user request"));
        ctx.store_named(1, 2, "populated", json!("populated"));
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
        ctx.store_result(0, 1, json!({"convergence_metric": 0.05}));
        let outcome = outcome_with_last(ctx, Some(0));
        let out = extract_final_step_result(&outcome);
        assert!(out.contains("convergence_metric"));
        assert!(out.contains("0.05"));
    }

    // ── Integration: BridgeManifestExecutor grounding wiring (Phase 5) ──
    // Tests the integration between hkask-templates (extract_final_step_result)
    // and hkask-verification (enforce_for_agent) that execute_skill wires
    // together. The tests simulate a CascadeOutcome with known output and
    // tool_calls, then run the same grounding path execute_skill uses.

    /// When a skill cascade produces a `deliverable_path` field but no
    /// file-writing tool was called, grounding must null the field (it's a
    /// fabrication). The cleaned output replaces the raw output.
    #[test]
    fn grounding_wiring_nulls_unsourced_deliverable_path() {
        let store = hkask_verification::VerificationStore::in_memory();

        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(
            0,
            1,
            json!({
                "deliverable_path": "/src/new_file.rs",
                "summary": "Created the file",
            }),
        );
        let outcome = outcome_with_last(ctx, Some(0));
        // No tool calls — deliverable_path is unsourced.

        let raw_value = hkask_templates::extract_final_step_result(&outcome);
        let response_str = match &raw_value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let (result, cleaned) = store.enforce_for_agent(
            "skill_cascade",
            "test-skill",
            "skill",
            &raw_value,
            &outcome.tool_calls,
            &response_str,
            &[],
        );

        assert!(
            result.is_some(),
            "grounding result must be Some (contract exists for 'skill')"
        );
        let gr = result.unwrap();
        assert!(
            gr.nulled_fields.contains(&"deliverable_path".to_string()),
            "deliverable_path must be nulled — no file-writing tool was called"
        );
        assert_eq!(
            cleaned["deliverable_path"],
            Value::Null,
            "cleaned output must have deliverable_path nulled"
        );
        assert_eq!(
            cleaned["summary"],
            json!("Created the file"),
            "summary must be preserved (inferred field)"
        );
    }

    /// When a skill cascade produces a `deliverable_path` field AND a
    /// file-writing tool was called successfully, grounding must keep the
    /// field (it's sourced). The cleaned output preserves the value.
    #[test]
    fn grounding_wiring_keeps_sourced_deliverable_path() {
        let store = hkask_verification::VerificationStore::in_memory();

        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(
            0,
            1,
            json!({
                "deliverable_path": "/src/new_file.rs",
                "summary": "Created the file",
            }),
        );
        let mut outcome = outcome_with_last(ctx, Some(0));
        outcome.tool_calls = vec![json!({
            "tool": "zed/write_file",
            "ok": true,
            "result": {"path": "/src/new_file.rs"}
        })];

        let raw_value = hkask_templates::extract_final_step_result(&outcome);
        let response_str = match &raw_value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let (result, cleaned) = store.enforce_for_agent(
            "skill_cascade",
            "test-skill",
            "skill",
            &raw_value,
            &outcome.tool_calls,
            &response_str,
            &[],
        );

        assert!(result.is_some());
        let gr = result.unwrap();
        assert!(
            gr.nulled_fields.is_empty(),
            "no fields must be nulled — write_file tool was called successfully"
        );
        assert_eq!(
            cleaned["deliverable_path"],
            json!("/src/new_file.rs"),
            "deliverable_path must be preserved (sourced from write_file)"
        );
    }

    /// When the cascade output is a string (not a JSON object), grounding
    /// records an unenforceable record and returns the original value
    /// unchanged. This is the narrative-only path — the skill produced prose,
    /// not structured JSON.
    #[test]
    fn grounding_wiring_handles_string_output_as_unenforceable() {
        let store = hkask_verification::VerificationStore::in_memory();

        let mut ctx = StepContext::new(std::collections::HashMap::new());
        ctx.store_result(0, 1, json!("This is a prose summary, not JSON."));
        let outcome = outcome_with_last(ctx, Some(0));

        let raw_value = hkask_templates::extract_final_step_result(&outcome);
        let response_str = match &raw_value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let (result, cleaned) = store.enforce_for_agent(
            "skill_cascade",
            "test-skill",
            "skill",
            &raw_value,
            &outcome.tool_calls,
            &response_str,
            &[],
        );

        assert!(
            result.is_none(),
            "grounding result must be None for non-object output (unenforceable)"
        );
        assert_eq!(
            cleaned, raw_value,
            "cleaned output must equal raw output for unenforceable records"
        );
    }

    /// The inline refine manifest YAML in `refine_bundle` must parse via
    /// `load_manifest_from_yaml` and produce a single-step manifest pointing
    /// at `skill-bundler/bundler-evolve`. If the YAML drifts (typo in
    /// template_ref, missing field, wrong indentation), this test catches it
    /// before the refine path fails at runtime.
    #[test]
    fn refine_manifest_yaml_parses_and_targets_bundler_evolve() {
        let refine_manifest_yaml = r#"
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
    timeout_seconds: 60
    input_mapping:
      bundle_name: "{{ bundle_name }}"
      current_manifest: "{{ current_manifest }}"
      changed_skills: "{{ changed_skills }}"
      goal_context: "{{ goal_context }}"
      goal_delta: "{{ goal_delta }}"
      convergence_failure_reason: "{{ convergence_failure_reason }}"
"#;

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

    /// `seed_registry_to_disk` must overwrite core-skill manifests and
    /// templates on every call (so user edits to core artifacts are
    /// discarded on restart) while preserving user-skill artifacts that
    /// already exist. This pins the core-vs-user split for all four seed
    /// loops (process manifests, template manifests, .j2 files, .yaml
    /// files) — the contract that `agent_skills::seed_shipped_skills` pins
    /// for SKILL.md, mirrored here for the registry.
    #[gpui::test]
    async fn test_seed_registry_to_disk_overwrites_core_preserves_user(
        cx: &mut gpui::TestAppContext,
    ) {
        use fs::FakeFs;
        use std::path::Path;

        let fs = FakeFs::new(cx.executor());
        let registry_root = Path::new("/registry");
        fs.create_dir(registry_root).await.unwrap();

        // Seed once — all shipped manifests and templates land on disk.
        seed_registry_to_disk(fs.as_ref(), registry_root).await;

        // A known core skill's process manifest must be present.
        let core_manifest = registry_root.join("manifests/create-skill.yaml");
        assert!(
            fs.is_file(&core_manifest).await,
            "core skill process manifest should be seeded"
        );
        let original_core = fs.load(&core_manifest).await.unwrap();

        // Tamper with the core manifest (simulating a user edit or
        // corruption). Re-seeding must overwrite it with the shipped copy.
        fs.write(&core_manifest, b"TAMPERED").await.unwrap();
        seed_registry_to_disk(fs.as_ref(), registry_root).await;
        let after_core = fs.load(&core_manifest).await.unwrap();
        assert_eq!(
            after_core, original_core,
            "core skill manifest must be overwritten on re-seed"
        );

        // A known user (non-core) skill's process manifest must be present
        // after the first seed.
        let user_manifest = registry_root.join("manifests/lora-training.yaml");
        assert!(
            fs.is_file(&user_manifest).await,
            "user skill process manifest should be seeded"
        );

        // Overwrite the user manifest with a user edit. Re-seeding must
        // PRESERVE the user edit (user skills are seed-once).
        fs.write(&user_manifest, b"USER EDIT").await.unwrap();
        seed_registry_to_disk(fs.as_ref(), registry_root).await;
        let after_user = fs.load(&user_manifest).await.unwrap();
        assert_eq!(
            after_user, "USER EDIT",
            "user skill manifest must be preserved on re-seed"
        );
    }
}
