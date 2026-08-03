//! Template resolution and rendering — extracted from the executor.
//!
//! This module owns the template-resolution ladder (embedded `.j2` → embedded
//! `.yaml` → filesystem `.j2` → filesystem `.yaml`) and the minijinja/inline
//! rendering dispatch. It is a pure function of `(template_ref, context,
//! base_path)` — it has no dependency on `InferencePort`, `ToolPort`, gas, or
//! convergence, which is why it lives outside the executor.
//!
//! # Security
//!
//! All filesystem resolution goes through `safe_template_join`, which rejects
//! any path segment starting with `.` or containing a backslash. This prevents
//! `{% include "../../etc/passwd" %}` and `template_ref: "../../../secrets"`
//! from reading files outside the base path (CWE-22).

use crate::ports::{Result, TemplateError};
use crate::{template_file, template_yaml_file};
use hkask_types::NotFound;
use minijinja::UndefinedBehavior;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Template renderer — owns the resolution ladder and minijinja environment.
///
/// Constructed once with a `base_path` and reused across renders. The renderer
/// is stateless beyond the base path; it holds no locks, no ports, no mutable
/// state. Cloning is cheap (one `PathBuf`).
#[derive(Clone)]
pub struct TemplateRenderer {
    base_path: PathBuf,
}

impl TemplateRenderer {
    /// Construct a renderer rooted at `base_path`.
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// The base path this renderer resolves template_refs against.
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Load a template by ref, preferring the filesystem copy and falling
    /// back to the embedded (build-time) copy.
    ///
    /// Resolution order: filesystem as-is → filesystem `.j2` → filesystem `.yaml`
    /// → embedded `.j2` → embedded `.yaml`. The filesystem is primary so YAML/J2
    /// edits take effect immediately without recompilation. Embedded copies are
    /// a fallback for production deployments where the registry directory may
    /// not exist on disk.
    ///
    /// `step_ordinal` is used for error messages and heal callbacks.
    pub fn load(&self, template_ref: &str, step_ordinal: u32) -> Result<String> {
        // Filesystem first — allows J2/YAML edits without recompilation.
        match self.load_from_disk(template_ref, step_ordinal) {
            Ok(content) => return Ok(content),
            Err(TemplateError::NotFound(_)) => {
                // Not on disk — fall through to embedded fallback.
            }
            Err(e) => {
                // Non-NotFound error (e.g., PathTraversal) — log and fall
                // through to embedded. The embedded copy is safe by construction.
                tracing::warn!(
                    "Filesystem template load failed for '{template_ref}' (step {step_ordinal}): {e}; falling back to embedded"
                );
            }
        }
        // Embedded fallback — for production where registry dir is absent.
        if let Some(content) = template_file(template_ref) {
            return Ok(content.to_string());
        }
        if let Some(content) = template_yaml_file(template_ref) {
            return Ok(content.to_string());
        }
        Err(TemplateError::NotFound(NotFound {
            entity_type: "template".to_string(),
            id: format!(
                "step {step_ordinal}: template '{template_ref}' not found on filesystem or in embedded registry"
            ),
        }))
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

        // Try the ref as-is first.
        if let Ok(c) = std::fs::read_to_string(&template_path) {
            return Ok(c);
        }

        // Try .j2 extension if not already present.
        if !template_ref.ends_with(".j2") {
            let j2_ref = format!("{template_ref}.j2");
            if let Some(j2_path) = safe_template_join(&self.base_path, &j2_ref)
                && let Ok(c) = std::fs::read_to_string(&j2_path)
            {
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

    /// Render a template with full Jinja2 syntax via minijinja.
    ///
    /// `template_content` is the raw template string (already loaded via `load`).
    /// `context` provides the template variables. `{% include %}` references
    /// resolve relative to `base_path` using the same resolution ladder.
    pub fn render(
        &self,
        template_content: &str,
        context: &HashMap<String, Value>,
    ) -> Result<String> {
        render_minijinja(template_content, context, &self.base_path)
    }

    /// Render an inline template using simple `{{key}}` substitution.
    ///
    /// This is the fast path for templates that only use `{{variable}}` placeholders
    /// — no `{% %}` logic. Used for `template_ref` and `mcp` field resolution
    /// before loading.
    pub fn render_inline(template: &str, context: &HashMap<String, Value>) -> String {
        let mut result = template.to_string();
        for (key, value) in context {
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

/// Render a template using minijinja (full Jinja2 syntax).
///
/// Supports `{% for %}`, `{{ var }}`, `| filter`, `{% if %}`, `{% include %}`
/// etc. The main template is registered under the synthetic name `"step"`;
/// `{% include "path/frag.j2" %}` references resolve relative to
/// `template_base_path` using the same embedded→filesystem ladder.
pub fn render_minijinja(
    template: &str,
    context: &HashMap<String, Value>,
    template_base_path: &Path,
) -> Result<String> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);

    // Register custom filters
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

    // Loader: the synthetic "step" name resolves to the in-memory main
    // template; any other name (from `{% include %}`) resolves from the
    // filesystem first (so J2 edits take effect without recompilation),
    // then from the embedded registry as a fallback (for production
    // deployments where the registry directory may not exist on disk),
    // mirroring the `template_ref` resolution rules (including the `.j2`
    // extension fallback).
    let main_template = template.to_string();
    let base = template_base_path.to_path_buf();
    env.set_loader(
        move |name: &str| -> std::result::Result<Option<String>, minijinja::Error> {
            if name == "step" {
                return Ok(Some(main_template.clone()));
            }
            // Filesystem first — allows J2/YAML edits without recompilation.
            // safe_join rejects any segment starting with '.' or containing '\\',
            // preventing `{% include "../../etc/passwd" %}` path traversal.
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
            // Embedded fallback — for production where registry dir is absent.
            if let Some(content) = template_file(name) {
                return Ok(Some(content.to_string()));
            }
            if let Some(content) = template_yaml_file(name) {
                return Ok(Some(content.to_string()));
            }
            Ok(None)
        },
    );

    // Convert HashMap<String, Value> to minijinja context via serde
    let context_value = serde_json::to_value(context)
        .map_err(|e| TemplateError::Render(format!("Failed to serialize context: {}", e)))?;
    let minijinja_context = minijinja::Value::from_serialize(&context_value);

    // Validate the main template parses, surfacing syntax errors with a
    // clear message (the loader resolves "step" lazily on first access).
    env.add_template("step", template)
        .map_err(|e| TemplateError::Render(format!("Invalid template: {}", e)))?;

    env.get_template("step")
        .and_then(|tmpl| tmpl.render(minijinja_context))
        .map_err(|e| TemplateError::Render(format!("Template render error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Path traversal regression tests (CWE-22) ──────────────────────────

    #[test]
    fn render_minijinja_rejects_include_traversal() {
        let tmp = std::env::temp_dir().join("hkask-renderer-include-traversal-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("legit.j2"), "hello").unwrap();

        let malicious_template = r#"{% include "../../../etc/passwd" %}"#;
        let ctx = HashMap::new();
        let result = render_minijinja(malicious_template, &ctx, &tmp);
        assert!(
            result.is_err(),
            "expected render error for traversal include, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn render_minijinja_rejects_backslash_include_traversal() {
        let tmp = std::env::temp_dir().join("hkask-renderer-backslash-include-test");
        std::fs::create_dir_all(&tmp).unwrap();

        let malicious_template = r#"{% include "..\\..\\etc\\passwd" %}"#;
        let ctx = HashMap::new();
        let result = render_minijinja(malicious_template, &ctx, &tmp);
        assert!(
            result.is_err(),
            "expected render error for backslash traversal, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn render_minijinja_allows_legit_include() {
        let tmp = std::env::temp_dir().join("hkask-renderer-legit-include-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("fragment.j2"), "world").unwrap();

        let template = r#"hello {% include "fragment.j2" %}"#;
        let ctx = HashMap::new();
        let result = render_minijinja(template, &ctx, &tmp);
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
        let mut ctx = HashMap::new();
        ctx.insert("name".to_string(), Value::String("world".to_string()));
        let out = TemplateRenderer::render_inline("hello {{name}}", &ctx);
        assert_eq!(out, "hello world");
    }

    #[test]
    fn render_inline_substitutes_non_string_values() {
        let mut ctx = HashMap::new();
        ctx.insert("count".to_string(), serde_json::json!(42));
        let out = TemplateRenderer::render_inline("count={{count}}", &ctx);
        assert_eq!(out, "count=42");
    }

    #[test]
    fn render_inline_leaves_unknown_keys_intact() {
        let ctx = HashMap::new();
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
}
