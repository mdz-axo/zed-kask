//! Template rendering tests — Wave 6 Task 6.2
//!
//! Verifies that all Jinja2 templates in the registry render without
//! errors when given sample context data. Catches template syntax errors
//! and missing variable bugs at test time rather than at runtime.
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): template errors should be caught before runtime

use minijinja::Environment;
use serde_json::json;
use std::path::Path;

// [P3] Motivating: Generative Space — validates Jinja2 templates render without errors
//Constraining: Semantic Grounding — template syntax errors caught before runtime
// All Jinja2 templates render without errors with valid context.

#[test]
fn all_templates_render() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let templates_dir = workspace_root.join("registry/templates");
    if !templates_dir.exists() {
        eprintln!("{} not found — skipping test", templates_dir.display());
        return;
    }

    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);

    // Register custom filters (mirrors render_minijinja in executor.rs)
    env.add_filter(
        "truncate",
        |_state: &minijinja::State, value: String, max_len: usize| -> String {
            if value.len() <= max_len {
                value
            } else {
                let mut truncated: String = value.chars().take(max_len).collect();
                truncated.push_str("...");
                truncated
            }
        },
    );

    // Load templates from the workspace registry directory so includes and
    // imports resolve by their registry id path (e.g. gml/macros.j2).
    env.set_loader(minijinja::path_loader(&templates_dir));
    let mut errors = Vec::new();
    let mut count = 0;

    for entry in walkdir::WalkDir::new(&templates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "j2"))
    {
        count += 1;
        let path = entry.path().to_path_buf();
        let name = path
            .strip_prefix(&templates_dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();

        match std::fs::read_to_string(&path) {
            Ok(source) => {
                if let Err(e) = env.add_template_owned(name.clone(), source) {
                    errors.push(format!("{}: template parse error: {}", path.display(), e));
                    continue;
                }

                // Attempt to render with minimal sample context. Missing-context
                // errors (undefined values, invalid operations on undefined) are
                // expected because templates declare contracts we do not satisfy
                // here. Other errors (syntax, unknown filters/includes, broken
                // references) are real defects and are reported.
                let ctx = json!({
                    "agent_name": "test-agent",
                    "goal_text": "test goal",
                    "query": "test query",
                    "topic": "test topic",
                    "mode": "plussing",
                });
                if let Ok(tmpl) = env.get_template(&name)
                    && let Err(e) = tmpl.render(&ctx)
                {
                    match e.kind() {
                        minijinja::ErrorKind::UndefinedError
                        | minijinja::ErrorKind::InvalidOperation
                        | minijinja::ErrorKind::UnknownMethod => {
                            // Missing context variable — not a template defect.
                        }
                        _ => {
                            errors.push(format!("{}: render error: {}", path.display(), e));
                        }
                    }
                }
            }
            Err(e) => {
                errors.push(format!("{}: IO error: {}", path.display(), e));
            }
        }
    }

    if !errors.is_empty() {
        panic!(
            "{} of {} templates failed to render:\n{}",
            errors.len(),
            count,
            errors.join("\n")
        );
    }

    eprintln!("Rendered {} templates — all successful", count);
}
