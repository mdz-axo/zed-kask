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
/// Logs failures and returns an empty string when loading or rendering fails.
/// Callers requiring a template must reject that result before inference.
///
/// `HKASK_TEMPLATE_ROOT` names the host-seeded registry. Repository tests must
/// set it explicitly; neither the working directory nor the build checkout is
/// a deployment fallback. Restart the server after updating deployed templates.
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
        let Some(template_root) =
            std::env::var_os("HKASK_TEMPLATE_ROOT").filter(|root| !root.is_empty())
        else {
            tracing::warn!(target: "hkask.mcp.docproc.template", "HKASK_TEMPLATE_ROOT is not configured; required templates must be deployed by the host");
            return String::new();
        };
        let template_path = std::path::PathBuf::from(template_root)
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

// ── One-shot rendering (Strict, no cache) ───────────────────────────────────
// One Environment construction and ServiceError wrapping for one-shot renders
// (`compose.rs`).

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

/// Construct a `ServiceError` from a minijinja error.
pub(crate) fn template_error(operation: &str, name: &str, e: minijinja::Error) -> ServiceError {
    let message = format!("Template {operation} failed for '{name}': {e}");
    ServiceError::Domain {
        domain: DomainKind::Infrastructure,
        kind: ErrorKind::ServiceUnavailable,
        source: Some(Box::new(e)),
        message,
    }
}

/// Render a minijinja template from source with `Strict` undefined behavior.
/// Creates a fresh `Environment` per call — no caching, no loader.
/// Use for one-shot rendering where the template source is already in memory
/// (loaded from disk or provided inline in config).
pub(crate) fn render_one_shot(
    name: &str,
    source: impl Into<String>,
    ctx: &minijinja::Value,
) -> Result<String, ServiceError> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template_owned(name, source.into())
        .map_err(|e| template_error("parse", name, e))?;
    let tmpl = env
        .get_template(name)
        .map_err(|e| template_error("load", name, e))?;
    tmpl.render(ctx)
        .map_err(|e| template_error("render", name, e))
}

#[cfg(test)]
mod tests {
    //! The docproc convention: the corpus server renders these templates
    //! RAW (see `render_docproc_template` — no header stripping), so they
    //! are deliberately headerless. A docproc template that later grows an
    //! `[inference]` contract header (the dominant convention elsewhere in
    //! the registry) would leak verbatim into corpus inference prompts
    //! with no error anywhere. This guard pins the convention at the
    //! registry source the host seeds from.

    #[test]
    fn docproc_templates_stay_headerless() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../registry/templates/docproc");
        let entries = std::fs::read_dir(&base)
            .unwrap_or_else(|e| panic!("docproc registry dir unreadable: {e}"));

        let mut checked = 0usize;
        let mut violations = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "j2") {
                continue;
            }
            checked += 1;
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));

            // First content line: skip blanks and Jinja comments the same
            // way the registry loader does (a `{#` comment may span lines
            // until its closing `#}`).
            let mut rest = content.trim_start();
            loop {
                let first = rest.lines().next().unwrap_or("").trim_start();
                if first.starts_with("{#") {
                    match rest.find("#}") {
                        Some(close) => rest = rest[close + 2..].trim_start(),
                        None => break, // unterminated comment: nothing follows
                    }
                } else if first.is_empty() {
                    match rest.find('\n') {
                        Some(nl) => rest = rest[nl + 1..].trim_start(),
                        None => break,
                    }
                } else {
                    break;
                }
            }
            let first = rest.lines().next().unwrap_or("").trim();
            if first == "[inference]" || first == "---" {
                violations.push(format!(
                    "{}: first content line `{first}` — docproc templates are rendered raw and must stay headerless",
                    path.display()
                ));
            }
        }
        assert!(
            checked > 0,
            "docproc registry scan found no templates — scan path broken"
        );
        assert!(
            violations.is_empty(),
            "{} docproc template(s) carry a header the raw renderer would leak:\n{}",
            violations.len(),
            violations.join("\n")
        );
    }
}
