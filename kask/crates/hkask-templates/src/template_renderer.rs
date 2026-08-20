//! Template resolution and rendering — extracted from the executor.
//!
//! This module owns the template-resolution ladder (embedded `.j2` → embedded
//! `.yaml` → filesystem `.j2` → filesystem `.yaml`) and the minijinja/inline
//! rendering dispatch. It is a pure function of `(template_ref, context,
//! base_path)` — it has no dependency on `InferencePort`, `ToolPort`, or
//! convergence, which is why it lives outside the executor.
//!
//! # Security
//!
//! All filesystem resolution goes through `safe_template_join`, which rejects
//! any path segment starting with `.` or containing a backslash. This prevents
//! `{% include "../../etc/passwd" %}` and `template_ref: "../../../secrets"`
//! from reading files outside the base path (CWE-22).

use crate::ports::{Result, TemplateError};
use crate::step_context::StepContext;
use hkask_types::NotFound;
use minijinja::UndefinedBehavior;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Default base path for template files relative to the project root.
pub const DEFAULT_TEMPLATE_BASE_PATH: &str = "registry/templates";

/// Safely join a base path with a template reference, rejecting path traversal.
///
/// Mirrors minijinja's internal `safe_join`: any segment starting with `.`
/// or containing a backslash is rejected. This prevents `{% include "../../etc/passwd" %}`
/// and `template_ref: "../../../secrets"` from reading files outside the base.
///
/// Returns `None` if the template_ref would escape the base path.
pub fn safe_template_join(base: &Path, template_ref: &str) -> Option<PathBuf> {
    let mut rv = base.to_path_buf();
    for segment in template_ref.split('/') {
        if segment.starts_with('.') || segment.contains('\\') {
            return None;
        }
        rv.push(segment);
    }
    Some(rv)
}

/// Template renderer — owns the resolution ladder and a cached minijinja
/// `Environment`.
///
/// Constructed once with a `base_path` and reused across renders. The
/// `Environment` (filter registration, undefined behavior, `{% include %}`
/// loader) is built once in `new` and reused on every render — only the
/// per-render template string is re-registered via `add_template_owned`.
/// This eliminates the ~50 `Environment` reconstructions per cascade
/// iteration that the prior per-render construction incurred.
///
/// The `Environment` is behind a `Mutex` because `add_template_owned`
/// requires `&mut`. The guard is held only for the duration of the
/// synchronous render (no await points), so contention is negligible —
/// the executor is single-threaded per cascade.
pub struct TemplateRenderer {
    base_path: PathBuf,
    env: Mutex<minijinja::Environment<'static>>,
    /// Disk content cache: template_ref → (mtime, content). Avoids re-reading
    /// the same .j2/.yaml file on every cascade iteration when the file hasn't
    /// changed. Without this, a 5-step cascade with 3 `select` steps re-reads
    /// 3 files from disk per iteration — 30 file reads for a 10-iteration
    /// convergence loop.
    disk_cache:
        std::sync::Mutex<std::collections::HashMap<String, (std::time::SystemTime, String)>>,
}

impl Clone for TemplateRenderer {
    fn clone(&self) -> Self {
        Self {
            base_path: self.base_path.clone(),
            env: Mutex::new(self.env.lock().unwrap_or_else(|e| e.into_inner()).clone()),
            disk_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl TemplateRenderer {
    /// Construct a renderer rooted at `base_path`.
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            env: Mutex::new(build_environment(&base_path)),
            base_path,
            disk_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// The base path this renderer resolves template_refs against.
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Load a template by ref from the filesystem. Disk is the **primary**
    /// runtime source — the shipped templates are seeded to disk at startup by
    /// the registry seeding path, so a fresh install has the full template tree
    /// on disk and edits take effect immediately without recompilation.
    ///
    /// **Fallback to embedded seeds:** when a template_ref is not found on disk
    /// (e.g. the registry seeding path was skipped, or the disk tree is
    /// partial), callers fall back to the compiled-in embedded seeds via
    /// `crate::template_file` / `crate::template_yaml_file`. This fallback
    /// exists in three production call sites:
    /// - `step_actions.rs::execute_flowdef` (sub-manifest resolution)
    /// - `step_actions.rs::execute_parallel` (parallel branch sub-manifest)
    /// - `hkask-mcp-kata-kanban/kata/execution.rs::render_template`
    /// The fallback is intentional (the embedded seeds are the bootstrapping
    /// source for a fresh install before the seeding path runs), but it means
    /// a disk edit to a shipped template may be shadowed by the embedded seed
    /// if the disk file is missing. Operators relying on disk edits should
    /// verify the file exists at the expected path.
    ///
    /// Resolution order on disk: ref as-is → ref `.j2` → ref `.yaml`.
    ///
    /// `step_ordinal` is used for error messages.
    pub fn load(&self, template_ref: &str, step_ordinal: u32) -> Result<String> {
        self.load_from_disk(template_ref, step_ordinal)
    }

    /// Load a template from the filesystem, trying the ref as-is, then with
    /// `.j2` appended, then with `.yaml` appended.
    pub fn load_from_disk(&self, template_ref: &str, step_ordinal: u32) -> Result<String> {
        let template_path = safe_template_join(&self.base_path, template_ref).ok_or_else(|| {
            TemplateError::PathTraversal(format!(
                "step {step_ordinal}: template_ref '{template_ref}' escapes base path '{}'",
                self.base_path.display()
            ))
        })?;

        // Check the disk cache — avoids re-reading the same file on every
        // cascade iteration when it hasn't changed.
        if let Ok(metadata) = std::fs::metadata(&template_path) {
            let mtime = metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if let Ok(cache) = self.disk_cache.lock() {
                if let Some((cached_mtime, content)) = cache.get(template_ref) {
                    if *cached_mtime == mtime {
                        return Ok(content.clone());
                    }
                }
            }
        }

        // Try the ref as-is first.
        if let Ok(c) = std::fs::read_to_string(&template_path) {
            self.cache_template(template_ref, &template_path, &c);
            return Ok(c);
        }

        // Try .j2 extension if not already present.
        if !template_ref.ends_with(".j2") {
            let j2_ref = format!("{template_ref}.j2");
            if let Some(j2_path) = safe_template_join(&self.base_path, &j2_ref)
                && let Ok(c) = std::fs::read_to_string(&j2_path)
            {
                self.cache_template(template_ref, &j2_path, &c);
                return Ok(c);
            }
        }

        // Try .yaml extension if not already present (FlowDef sub-manifests
        // and RenderAct reference docs can be .yaml files).
        if !template_ref.ends_with(".yaml") {
            let yaml_ref = format!("{template_ref}.yaml");
            if let Some(yaml_path) = safe_template_join(&self.base_path, &yaml_ref)
                && let Ok(c) = std::fs::read_to_string(&yaml_path)
            {
                self.cache_template(template_ref, &yaml_path, &c);
                return Ok(c);
            }
        }

        Err(TemplateError::NotFound(NotFound {
            entity_type: "template".to_string(),
            id: format!(
                "Step {step_ordinal}: template file not found at {} (also tried .j2 and .yaml extensions)",
                template_path.display()
            ),
        }))
    }

    /// Cache a template's content with its file modification time.
    fn cache_template(&self, template_ref: &str, path: &Path, content: &str) {
        if let Ok(metadata) = std::fs::metadata(path) {
            let mtime = metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if let Ok(mut cache) = self.disk_cache.lock() {
                cache.insert(template_ref.to_string(), (mtime, content.to_string()));
            }
        }
    }

    /// Render a template with full Jinja2 syntax via minijinja.
    ///
    /// `template_content` is the raw template string (already loaded via `load`).
    /// `context` provides the template variables. `{% include %}` references
    /// resolve relative to `base_path` using the same resolution ladder.
    ///
    /// The cached `Environment` is reused across renders — only the "step"
    /// template is re-registered per call via `add_template_owned` (which
    /// replaces any prior "step" registration). This avoids rebuilding the
    /// Environment, re-registering filters, and re-setting the loader on every
    /// render.
    pub fn render(&self, template_content: &str, context: &StepContext) -> Result<String> {
        let mut env = self.env.lock().unwrap_or_else(|e| e.into_inner());

        // Strip YAML front matter before rendering. hKask templates have a
        // front matter block (metadata, contract) terminated by
        // `\n---\n`. Without stripping, the front matter is sent to the LLM
        // as literal prompt text — confusing the model and wasting tokens.
        let after_front_matter = strip_front_matter(template_content);

        // Strip the `[inference]` config block from the body. This TOML-like
        // block declares per-step parameters (temperature
        // thinking_budget). Without stripping, it's sent to the LLM as prompt
        // text. The parsed config is extracted separately by the caller via
        // `parse_and_strip_inference_block` on the raw template content.
        let (renderable, _inference_block) = parse_and_strip_inference_block(after_front_matter);

        // Register the per-render template under the synthetic name "step".
        // `add_template_owned` replaces any prior "step" registration (no
        // accumulation across renders). The loader handles only `{% include %}`
        // references, not "step".
        env.add_template_owned("step", renderable)
            .map_err(|e| TemplateError::Render(format!("Invalid template: {}", e)))?;

        // `Value::from_serialize` accepts any `Serialize` type directly — the
        // prior code serialized to an intermediate `serde_json::Value` first,
        // a redundant double-conversion on every render.
        let minijinja_context = minijinja::Value::from_serialize(context);

        env.get_template("step")
            .and_then(|tmpl| tmpl.render(minijinja_context))
            .map_err(|e| TemplateError::Render(format!("Template render error: {}", e)))
    }

    /// Render an inline template using simple `{{key}}` substitution.
    ///
    /// This is the fast path for templates that only use `{{variable}}` placeholders
    /// — no `{% %}` logic. Used for `template_ref` and `mcp` field resolution
    /// before loading.
    pub fn render_inline(template: &str, context: &StepContext) -> String {
        let mut result = template.to_string();
        for (key, value) in context.entries() {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
        result
    }
}

/// Strip YAML front matter from a template file before rendering.
///
/// hKask `.j2` templates have a front matter block at the top containing
/// metadata (`[inference]`, `template_type`, `contract`
/// `visibility`) terminated by a `\n---\n` separator. The body after the
/// separator is the actual Jinja2 prompt template.
///
/// Without stripping, the front matter is sent to the LLM as literal prompt
/// text — the model sees `[inference]\ntemplate_type: KnowAct\ncontract:...`
/// before the actual instructions, wasting tokens and confusing the model.
///
/// Templates without a `\n---\n` separator are returned as-is (no front
/// matter to strip — e.g., inline templates, simple prompts).
pub fn strip_front_matter(template_content: &str) -> &str {
    if let Some(separator_pos) = template_content.find("\n---\n") {
        // Skip past the separator (5 chars: \n---\n)
        &template_content[separator_pos + 5..]
    } else {
        template_content
    }
}

/// Parsed `[inference]` config block from a template body.
///
/// Templates declare per-step inference parameters in a TOML-like block:
/// ```text
/// [inference]
/// temperature = 0.2
/// thinking_budget = "full"
/// work_effort = "high"
/// verbosity = "detailed"
/// ```
/// This struct captures the parsed values. Fields not present in the block
/// remain `None` — the caller merges them over `LLMParameters::default()`.
#[derive(Debug, Default, Clone)]
pub struct InferenceBlock {
    pub temperature: Option<f32>,
    pub thinking_budget: Option<String>,
    /// Work effort level: "high" | "medium" | "low" | "minimal".
    /// Maps to thinking_budget when thinking_budget is not explicitly set:
    /// "high"/"medium" → thinking ON, "low"/"minimal" → thinking OFF.
    /// thinking_budget takes precedence if both are declared.
    pub work_effort: Option<String>,
    /// Output verbosity: "terse" | "concise" | "standard" | "detailed" | "verbose".
    /// Injects a system-prompt instruction controlling output length.
    /// "standard" is the default (no instruction injected).
    pub verbosity: Option<String>,
}

/// Parse and strip the `[inference]` config block from a template body.
///
/// The block starts with `[inference]` on its own line and ends at the first
/// blank line. Key-value pairs use `key = value` syntax (TOML-like). String
/// values are quoted; numeric values are bare.
///
/// Returns `(stripped_body, parsed_config)`. The stripped body has the
/// `[inference]` block removed so it's not sent to the LLM as prompt text.
/// If no `[inference]` block is found, returns the original text and an empty
/// `InferenceBlock`.
pub fn parse_and_strip_inference_block(body: &str) -> (String, InferenceBlock) {
    // Find the `[inference]` marker on its own line.
    let marker = "[inference]";
    let marker_pos = match body.find(marker) {
        Some(pos) => pos,
        None => return (body.to_string(), InferenceBlock::default()),
    };

    // The marker must be at the start of a line (or start of string).
    if marker_pos > 0 && body.as_bytes().get(marker_pos - 1) != Some(&b'\n') {
        return (body.to_string(), InferenceBlock::default());
    }

    // Find the end of the block: the first blank line after the marker.
    let after_marker = &body[marker_pos + marker.len()..];
    let block_end = after_marker.find("\n\n").unwrap_or(after_marker.len());
    let block_content = &after_marker[..block_end];

    // Parse key = value lines.
    let mut config = InferenceBlock::default();
    for line in block_content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            // Strip surrounding quotes from string values.
            let unquoted = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            match key {
                "temperature" => {
                    config.temperature = unquoted.parse().ok();
                }
                "thinking_budget" => {
                    config.thinking_budget = Some(unquoted.to_string());
                }
                _ => {}
            }
        }
    }

    // Rebuild the body without the `[inference]` block.
    let before = &body[..marker_pos];
    let after = if marker_pos + marker.len() + block_end < body.len() {
        // Skip past the `\n\n` blank line separator that terminated the block.
        let rest_start = marker_pos + marker.len() + block_end;
        let rest = &body[rest_start..];
        // Strip leading newlines — the blank line(s) that terminated the block.
        rest.trim_start_matches('\n')
    } else {
        ""
    };

    let stripped = format!("{before}{after}");
    (stripped, config)
}

/// Build a minijinja `Environment` configured for the renderer.
///
/// The environment has:
/// - `UndefinedBehavior::Lenient` (undefined values render as empty).
/// - The `truncate` custom filter.
/// - A loader that resolves `{% include %}` references from the filesystem
///   relative to `base_path`, with `.j2`/`.yaml` extension fallbacks. The
///   loader does NOT handle the synthetic "step" name — that is registered
///   per-render via `add_template_owned`.
///
/// Built once in `TemplateRenderer::new` and reused across renders.
fn build_environment(base_path: &Path) -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);

    env.add_filter(
        "truncate",
        |state: &minijinja::State, value: String, max_len: usize| -> String {
            let _ = state;
            if value.len() <= max_len {
                value
            } else {
                let mut truncated: String = value.chars().take(max_len).collect();
                truncated.push_str("...");
                truncated
            }
        },
    );

    let base = base_path.to_path_buf();
    env.set_loader(
        move |name: &str| -> std::result::Result<Option<String>, minijinja::Error> {
            // The "step" name is registered via `add_template_owned` per-render;
            // the loader handles only `{% include %}` references here.
            let primary = match safe_template_join(&base, name) {
                Some(p) => p,
                None => return Ok(None),
            };
            if let Ok(content) = std::fs::read_to_string(&primary) {
                return Ok(Some(content));
            }
            if !name.ends_with(".j2") {
                let j2_name = format!("{name}.j2");
                if let Some(j2_path) = safe_template_join(&base, &j2_name)
                    && let Ok(content) = std::fs::read_to_string(&j2_path)
                {
                    return Ok(Some(content));
                }
            }
            if !name.ends_with(".yaml") {
                let yaml_name = format!("{name}.yaml");
                if let Some(yaml_path) = safe_template_join(&base, &yaml_name)
                    && let Ok(content) = std::fs::read_to_string(&yaml_path)
                {
                    return Ok(Some(content));
                }
            }
            Ok(None)
        },
    );

    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── Path traversal regression tests (CWE-22) ──────────────────────────

    #[test]
    fn render_rejects_include_traversal() {
        let tmp = std::env::temp_dir().join("hkask-renderer-include-traversal-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("legit.j2"), "hello").unwrap();

        let malicious_template = r#"{% include "../../../etc/passwd" %}"#;
        let ctx = StepContext::new(HashMap::new());
        let renderer = TemplateRenderer::new(tmp.clone());
        let result = renderer.render(malicious_template, &ctx);
        assert!(
            result.is_err(),
            "expected render error for traversal include, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn render_rejects_backslash_include_traversal() {
        let tmp = std::env::temp_dir().join("hkask-renderer-backslash-include-test");
        std::fs::create_dir_all(&tmp).unwrap();

        let malicious_template = r#"{% include "..\\..\\etc\\passwd" %}"#;
        let ctx = StepContext::new(HashMap::new());
        let renderer = TemplateRenderer::new(tmp.clone());
        let result = renderer.render(malicious_template, &ctx);
        assert!(
            result.is_err(),
            "expected render error for backslash traversal, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn render_allows_legit_include() {
        let tmp = std::env::temp_dir().join("hkask-renderer-legit-include-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("fragment.j2"), "world").unwrap();

        let template = r#"hello {% include "fragment.j2" %}"#;
        let ctx = StepContext::new(HashMap::new());
        let renderer = TemplateRenderer::new(tmp.clone());
        let result = renderer.render(template, &ctx);
        assert!(
            result.is_ok(),
            "legitimate include should succeed, got: {result:?}"
        );
        assert_eq!(result.unwrap(), "hello world");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── safe_template_join unit tests ─────────────────────────────────────

    #[test]
    fn safe_join_rejects_dot_segments() {
        let base = Path::new("/tmp/templates");
        assert!(safe_template_join(base, "../../etc/passwd").is_none());
        assert!(safe_template_join(base, "./local").is_none());
        assert!(safe_template_join(base, ".env").is_none());
    }

    #[test]
    fn safe_join_rejects_backslash_segments() {
        let base = Path::new("/tmp/templates");
        assert!(safe_template_join(base, "..\\..\\etc\\passwd").is_none());
        assert!(safe_template_join(base, "foo\\bar").is_none());
    }

    #[test]
    fn safe_join_allows_legit_refs() {
        let base = Path::new("/tmp/templates");
        let p = safe_template_join(base, "skill/template.j2").unwrap();
        assert_eq!(p, Path::new("/tmp/templates/skill/template.j2"));
    }

    // ── render_inline tests ───────────────────────────────────────────────

    #[test]
    fn render_inline_substitutes_string_values() {
        let mut inputs = HashMap::new();
        inputs.insert("name".to_string(), Value::String("world".to_string()));
        let ctx = StepContext::new(inputs);
        let out = TemplateRenderer::render_inline("hello {{name}}", &ctx);
        assert_eq!(out, "hello world");
    }

    #[test]
    fn render_inline_substitutes_non_string_values() {
        let mut inputs = HashMap::new();
        inputs.insert("count".to_string(), serde_json::json!(42));
        let ctx = StepContext::new(inputs);
        let out = TemplateRenderer::render_inline("count={{count}}", &ctx);
        assert_eq!(out, "count=42");
    }

    #[test]
    fn render_inline_leaves_unknown_keys_intact() {
        let ctx = StepContext::new(HashMap::new());
        let out = TemplateRenderer::render_inline("hello {{missing}}", &ctx);
        assert_eq!(out, "hello {{missing}}");
    }

    // ── load_from_disk tests ──────────────────────────────────────────────

    #[test]
    fn load_from_disk_resolves_j2_fallback() {
        let tmp = std::env::temp_dir().join("hkask-renderer-j2-fallback-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("tmpl.j2"), "body").unwrap();

        let renderer = TemplateRenderer::new(tmp.clone());
        // Request without extension — should resolve via .j2 fallback.
        let content = renderer.load_from_disk("tmpl", 1).unwrap();
        assert_eq!(content, "body");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_disk_resolves_yaml_fallback() {
        let tmp = std::env::temp_dir().join("hkask-renderer-yaml-fallback-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("manifest.yaml"), "steps: []").unwrap();

        let renderer = TemplateRenderer::new(tmp.clone());
        let content = renderer.load_from_disk("manifest", 1).unwrap();
        assert_eq!(content, "steps: []");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_disk_errors_on_traversal() {
        let renderer = TemplateRenderer::new(PathBuf::from("/tmp/templates"));
        let result = renderer.load_from_disk("../../etc/passwd", 1);
        assert!(
            matches!(result, Err(TemplateError::PathTraversal(_))),
            "expected PathTraversal error, got: {result:?}"
        );
    }

    #[test]
    fn load_from_disk_errors_on_missing_template() {
        let tmp = std::env::temp_dir().join("hkask-renderer-missing-test");
        std::fs::create_dir_all(&tmp).unwrap();

        let renderer = TemplateRenderer::new(tmp.clone());
        let result = renderer.load_from_disk("nonexistent", 1);
        assert!(
            matches!(result, Err(TemplateError::NotFound(_))),
            "expected NotFound error, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── Front matter stripping tests ──────────────────────────────────────

    #[test]
    fn strip_front_matter_removes_yaml_metadata() {
        let template = "{# comment #}\n[inference]\ntemplate_type: KnowAct\ncontract:\n  output:\n    result: string\n---\nYou are an evaluator.\n";
        let result = strip_front_matter(template);
        assert!(result.starts_with("You are an evaluator."));
        assert!(!result.contains("[inference]"));
        assert!(!result.contains("template_type"));
    }

    #[test]
    fn strip_front_matter_passes_through_without_separator() {
        let template = "You are an evaluator. Respond with JSON.";
        let result = strip_front_matter(template);
        assert_eq!(result, template);
    }

    // ── Inference block parsing tests ─────────────────────────────────────

    #[test]
    fn parse_inference_block_extracts_temperature() {
        let body = "[inference]\ntemperature = 0.2\nthinking_budget = \"full\"\n\nYou are a code reviewer.";
        let (stripped, config) = parse_and_strip_inference_block(body);
        assert_eq!(config.temperature, Some(0.2));
        assert_eq!(config.thinking_budget.as_deref(), Some("full"));
        assert!(stripped.starts_with("You are a code reviewer."));
        assert!(!stripped.contains("[inference]"));
        assert!(!stripped.contains("temperature ="));
    }

    #[test]
    fn parse_inference_block_returns_empty_when_no_block() {
        let body = "You are an evaluator. Respond with JSON.";
        let (stripped, config) = parse_and_strip_inference_block(body);
        assert_eq!(config.temperature, None);
        assert_eq!(config.thinking_budget, None);
        assert_eq!(stripped, body);
    }

    #[test]
    fn parse_inference_block_handles_partial_config() {
        let body = "[inference]\nthinking_budget = \"off\"\n\nYou are a decomposer.";
        let (stripped, config) = parse_and_strip_inference_block(body);
        assert_eq!(config.temperature, None);
        assert_eq!(config.thinking_budget.as_deref(), Some("off"));
        assert!(stripped.starts_with("You are a decomposer."));
    }

    #[test]
    fn parse_inference_block_handles_thinking_budget_none() {
        let body = "[inference]\ntemperature = 0.0\nthinking_budget = \"none\"\n\nYou are a triage agent.\n";
        let (_stripped, config) = parse_and_strip_inference_block(body);
        assert_eq!(config.thinking_budget.as_deref(), Some("none"));
    }

    #[test]
    fn parse_inference_block_handles_thinking_budget_off() {
        let body = "[inference]\nthinking_budget = \"off\"\n\nFormat probe results.\n";
        let (_stripped, config) = parse_and_strip_inference_block(body);
        assert_eq!(config.thinking_budget.as_deref(), Some("off"));
    }

    #[test]
    fn parse_inference_block_handles_thinking_budget_on() {
        let body = "[inference]\nthinking_budget = \"on\"\n\nGenerate a response.\n";
        let (_stripped, config) = parse_and_strip_inference_block(body);
        assert_eq!(config.thinking_budget.as_deref(), Some("on"));
    }
}
