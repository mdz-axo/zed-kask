//! Template rendering — cached Jinja2 environment for docproc prompt templates.
//!
//! Templates live in `registry/templates/docproc/` as Jinja2 files.
//! The environment is lazily initialized on first use and cached for the
//! server's lifetime. Template names and sources are leaked as `'static`
//! only on first load — subsequent calls reuse the cached environment with
//! no leak (the `format!` lookup key allocates a short String per call, but
//! no `'static` leak occurs).

/// Cached template environment — compiled templates are stored and reused.
static TEMPLATE_CACHE: std::sync::OnceLock<std::sync::Mutex<minijinja::Environment<'static>>> =
    std::sync::OnceLock::new();

/// Load a docproc template from registry and render with minijinja.
///
/// Templates live in `registry/templates/docproc/` as Jinja2 files.
/// Falls back to empty string if the template file is missing or rendering
/// fails — callers provide an inline fallback prompt.
///
/// Template base path is resolved relative to the workspace root via
/// `HKASK_TEMPLATE_ROOT` env var (default: "registry").
pub(crate) fn render_docproc_template(
    template_name: &str,
    vars: &std::collections::HashMap<&str, String>,
) -> String {
    let env = TEMPLATE_CACHE.get_or_init(|| {
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        std::sync::Mutex::new(env)
    });

    let lookup_key = format!("docproc:{template_name}");

    // Reject path traversal — template names are file basenames, not paths.
    if template_name.contains('/') || template_name.contains('\\') || template_name.contains("..") {
        tracing::warn!(target: "hkask.mcp.docproc.template", name = %template_name, "Template name contains path separators");
        return String::new();
    }

    let mut env_guard = env.lock().unwrap_or_else(|e| e.into_inner());

    // Load template from disk on first use. The key and source are leaked as
    // 'static for minijinja's Environment<'static> — bounded by the number of
    // distinct template names (a small, fixed set). minijinja matches template
    // names by string value, so the temporary lookup_key finds templates
    // added under the leaked 'static key.
    if env_guard.get_template(&lookup_key).is_err() {
        let template_root =
            std::env::var("HKASK_TEMPLATE_ROOT").unwrap_or_else(|_| "registry".to_string());
        let template_path = std::path::Path::new(&template_root)
            .join("templates/docproc")
            .join(format!("{template_name}.j2"));

        let content = match std::fs::read_to_string(&template_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "hkask.mcp.docproc.template", path = %template_path.display(), error = %e, "Template not found");
                return String::new();
            }
        };

        // Leak only after successful read — avoids unbounded leak on missing files.
        let template_key: &'static str = Box::leak(lookup_key.clone().into_boxed_str());
        let source: &'static str = Box::leak(content.into_boxed_str());
        if let Err(e) = env_guard.add_template(template_key, source) {
            tracing::warn!(target: "hkask.mcp.docproc.template", error = %e, "Invalid template syntax");
            // Cache a sentinel so subsequent calls skip the load path instead of
            // re-reading and re-leaking on every call.
            if let Err(sentinel_err) =
                env_guard.add_template(template_key, "{# invalid template #}")
            {
                tracing::warn!(target: "hkask.mcp.docproc.template", error = %sentinel_err, "Failed to cache sentinel for invalid template");
            }
            return String::new();
        }
    }

    // Render — single path for both freshly-loaded and cached templates.
    let ctx = match serde_json::to_value(vars) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.docproc.template", error = %e, "Failed to serialize template vars");
            return String::new();
        }
    };
    match env_guard
        .get_template(&lookup_key)
        .and_then(|t| t.render(minijinja::Value::from_serialize(&ctx)))
    {
        Ok(rendered) => rendered.trim().to_string(),
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.docproc.template", error = %e, "Template render failed");
            String::new()
        }
    }
}
