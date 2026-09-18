use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput, deserialize_maybe_stringified};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

/// Render a Jinja2 template from the kask registry with context variables.
///
/// This tool provides structured prompt scaffolding for skill processes.
/// The SKILL.md body tells you when to call it — e.g., "call `render_template`
/// with template_ref `essentialist/essentialist-flow` to get the structured
/// prompt for the 3-gate loop."
///
/// Templates live in `kask/registry/templates/<skill>/<file>.j2`. The tool
/// strips YAML frontmatter (the `---`-delimited header containing the contract
/// schema and `[inference]` parameters) and renders only the Jinja2 body.
///
/// The rendered text is a structured prompt — use it as guidance for your
/// next reasoning step, not as a final answer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenderTemplateToolInput {
    /// Template reference, e.g. "essentialist/essentialist-flow". The tool
    /// resolves this against the registry templates directory, trying the
    /// ref as-is, then with `.j2` appended, then with `.yaml` appended.
    pub template_ref: String,
    /// Context variables for Jinja2 interpolation. Keys become template
    /// variables (e.g., `{{ task }}`, `{{ artifact }}`). Values must be
    /// JSON-serializable.
    ///
    /// Uses `AnyJsonValue` for values (not `serde_json::Value`) because
    /// `schemars` renders `Value` as bare `true` in `additionalProperties`,
    /// which breaks strict-schema providers — context variables silently
    /// don't arrive.
    ///
    /// `deserialize_maybe_stringified` tolerates models that emit `context` as
    /// a stringified JSON string instead of a bare object — the same pattern
    /// `edit_file.edits` uses.
    #[serde(default, deserialize_with = "deserialize_maybe_stringified")]
    pub context: std::collections::HashMap<String, hkask_types::AnyJsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RenderTemplateToolOutput {
    Rendered { text: String },
    Error { error: String },
}

impl From<RenderTemplateToolOutput> for LanguageModelToolResultContent {
    fn from(value: RenderTemplateToolOutput) -> Self {
        match value {
            RenderTemplateToolOutput::Rendered { text } => text.into(),
            RenderTemplateToolOutput::Error { error } => error.into(),
        }
    }
}

pub struct RenderTemplateTool;

impl AgentTool for RenderTemplateTool {
    type Input = RenderTemplateToolInput;
    type Output = RenderTemplateToolOutput;

    const NAME: &'static str = "render_template";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("Rendering: {}", input.template_ref).into(),
            Err(_) => "Render Template".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_cx| {
            let input = input.recv().await.map_err(|e| {
                RenderTemplateToolOutput::Error {
                    error: format!("failed to receive input: {e}"),
                }
            })?;

            let base_path = crate::template_base_path().ok_or_else(|| {
                RenderTemplateToolOutput::Error {
                    error: "Template base path not configured. The registry templates directory is wired at startup."
                        .to_string(),
                }
            })?;

            // Resolve the template file, trying ref as-is, .j2, then .yaml.
            let content = read_template_file(&base_path, &input.template_ref).map_err(|e| {
                RenderTemplateToolOutput::Error { error: e }
            })?;

            validate_contract_inputs(&content, &input.context).map_err(|error| {
                RenderTemplateToolOutput::Error {
                    error: format!(
                        "Template contract validation failed for '{}': {error}",
                        input.template_ref
                    ),
                }
            })?;

            // Strip YAML frontmatter (--- delimited header).
            let template_body = strip_frontmatter(&content);

            // Build minijinja environment and render. The loader resolves
            // `{% include %}` names against the registry root — a bare
            // render_str environment has no templates, so includes fail.
            let mut env = minijinja::Environment::new();
            bind_registry_loader(&mut env, &base_path);
            let result = env.render_str(&template_body, &input.context).map_err(|e| {
                RenderTemplateToolOutput::Error {
                    error: format!("Template rendering failed: {e}"),
                }
            })?;

            Ok(RenderTemplateToolOutput::Rendered { text: result })
        })
    }
}

/// Read a template file from the registry, trying ref as-is, .j2, then .yaml.
/// Prevents path traversal outside the base directory.
///
/// `resolve_template_path` returns `None` for two reasons: the joined path
/// escapes the base directory (traversal blocked), or the file doesn't exist
/// (`canonicalize` fails). Both are safe to fall through from — the `.j2`/`.yaml`
/// retries are checked against the same base path, so traversal stays blocked.
/// We only error after all three attempts fail.
fn read_template_file(base_path: &std::path::Path, template_ref: &str) -> Result<String, String> {
    // Try ref as-is. `None` means either traversal blocked or file absent —
    // fall through to the extension retries rather than erroring immediately.
    if let Some(resolved) = resolve_template_path(base_path, template_ref) {
        if let Ok(content) = std::fs::read_to_string(&resolved) {
            return Ok(content);
        }
    }

    // Try .j2 extension.
    if !template_ref.ends_with(".j2") {
        let j2_ref = format!("{template_ref}.j2");
        if let Some(j2_path) = resolve_template_path(base_path, &j2_ref) {
            if let Ok(content) = std::fs::read_to_string(&j2_path) {
                return Ok(content);
            }
        }
    }

    // Try .yaml extension.
    if !template_ref.ends_with(".yaml") {
        let yaml_ref = format!("{template_ref}.yaml");
        if let Some(yaml_path) = resolve_template_path(base_path, &yaml_ref) {
            if let Ok(content) = std::fs::read_to_string(&yaml_path) {
                return Ok(content);
            }
        }
    }

    Err(format!(
        "Template not found: tried '{template_ref}', '{template_ref}.j2', '{template_ref}.yaml' under '{}'",
        base_path.display()
    ))
}

/// Safely join a template ref to the base path, rejecting path traversal.
fn resolve_template_path(
    base_path: &std::path::Path,
    template_ref: &str,
) -> Option<std::path::PathBuf> {
    let joined = base_path.join(template_ref);
    let canonical_base = base_path.canonicalize().ok()?;
    let canonical_joined = joined.canonicalize().ok()?;
    if canonical_joined.starts_with(&canonical_base) {
        Some(canonical_joined)
    } else {
        None
    }
}

fn validate_contract_inputs(
    content: &str,
    context: &std::collections::HashMap<String, hkask_types::AnyJsonValue>,
) -> Result<(), String> {
    let Some(header) = template_metadata_header(content) else {
        return Ok(());
    };
    let metadata: serde_yaml::Value = serde_yaml::from_str(header)
        .map_err(|error| format!("invalid template metadata: {error}"))?;
    let Some(inputs) = metadata
        .get("contract")
        .and_then(|contract| contract.get("input"))
        .and_then(serde_yaml::Value::as_mapping)
    else {
        return Ok(());
    };

    let mut missing = inputs
        .iter()
        .filter_map(|(name, specification)| {
            let name = name.as_str()?;
            (contract_input_is_required(specification) && !context.contains_key(name))
                .then(|| name.to_string())
        })
        .collect::<Vec<_>>();
    missing.sort();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "missing required input field(s): {}",
            missing.join(", ")
        ))
    }
}

fn contract_input_is_required(specification: &serde_yaml::Value) -> bool {
    if specification
        .as_str()
        .is_some_and(|value| value.split('|').any(|part| part.trim() == "null"))
    {
        return false;
    }

    let Some(mapping) = specification.as_mapping() else {
        return true;
    };
    let key = |name: &str| serde_yaml::Value::String(name.to_string());
    if mapping
        .get(key("required"))
        .and_then(serde_yaml::Value::as_bool)
        == Some(false)
        || mapping.contains_key(key("default"))
    {
        return false;
    }

    !mapping
        .get(key("type"))
        .and_then(serde_yaml::Value::as_str)
        .is_some_and(|value| value.split('|').any(|part| part.trim() == "null"))
}

fn template_metadata_header(content: &str) -> Option<&str> {
    let mut working = content.trim_start();
    loop {
        if !working.lines().next()?.trim().starts_with("{#") {
            break;
        }
        let close_pos = working.find("#}")?;
        working = working.get(close_pos + 2..)?.trim_start_matches('\n');
    }

    if let Some(metadata) = working.strip_prefix("[inference]\n") {
        let end = metadata.find("\n---\n")?;
        metadata.get(..end)
    } else if let Some(metadata) = working.strip_prefix("---\n") {
        let end = metadata.find("\n---\n")?;
        metadata.get(..end)
    } else {
        None
    }
}

/// Strip the template metadata header and inference-param stanzas from a
/// template file, leaving only the renderable prompt body.
///
/// The on-disk convention (219 of 315 templates) is:
///
/// ```text
/// [inference]
/// contract: …
/// visibility: Public
/// ---
/// <body>
/// ```
///
/// i.e. an `[inference]`-keyed header (contract schema + visibility) that is
/// NOT YAML-frontmatter and therefore not delimited by leading `---` — the
/// terminator is a lone `---` line *after* the header. The old stripper only
/// fired on a leading `---`, which matched 0 of 309 templates, so the header
/// leaked verbatim into every rendered prompt.
///
/// Two stanzas are stripped:
/// 1. **Header** — everything from a leading `[inference]` line through the
///    first lone `---` line. Templates that still use legacy leading-`---`
///    frontmatter keep working (same rule, different opener).
/// 2. **Body param stanza** — a second `[inference]` block at the top of the
///    body (temperature/work_effort/verbosity/thinking_budget render params).
///    These are tool-execution metadata, not prompt text; minijinja would
///    otherwise emit them verbatim.
///
/// A template with neither convention passes through unchanged.
fn strip_frontmatter(content: &str) -> String {
    let mut working: &str = content;

    // ── Stanza 1: the header ─────────────────────────────────────
    // Skip leading Jinja `{# … #}` comment lines first — ~30 templates carry
    // a goal/ontology comment above the header.
    loop {
        let first = working.lines().next().unwrap_or("").trim();
        if first.starts_with("{#") {
            // Jinja comment — may span multiple lines. Skip through the line
            // containing the closing `#}`.
            match working.find("#}") {
                Some(close_pos) => {
                    working = &working[close_pos + 2..];
                    working = working.trim_start_matches('\n');
                }
                None => break, // Unterminated comment — leave the rest alone.
            }
        } else {
            break;
        }
    }
    let first_line = working.lines().next().unwrap_or("").trim();
    if first_line == "[inference]" {
        // Find the terminating lone `---` line and take everything after it.
        if let Some(pos) = working.find("\n---\n") {
            working = &working[pos + 1..];
            working = working.strip_prefix("---\n").unwrap_or(working);
        }
    } else if working.starts_with("---") {
        // Legacy YAML frontmatter: everything after the second `---`.
        if let Some(after) = working.splitn(3, "---").nth(2) {
            working = after;
        }
    }

    // ── Stanza 2: a body-leading [inference] param block ──────────
    // Runs from the `[inference]` line through the first blank line. Leading
    // blank lines and Jinja `{# comment #}` lines are skipped first — many
    // templates carry an ontology comment between the header terminator and
    // the param stanza. Only a stanza at the very start of the body is
    // stripped; an `[inference]` mention in running prose is left alone.
    let trimmed = working.trim_start_matches('\n');
    let mut scan = trimmed;
    loop {
        let first = scan.lines().next().unwrap_or("").trim();
        if first.is_empty() && !scan.is_empty() {
            scan = scan.strip_prefix('\n').unwrap_or(scan);
        } else if first.starts_with("{#") && first.contains("#}") {
            let line_len = scan.find('\n').map(|i| i + 1).unwrap_or(scan.len());
            scan = &scan[line_len..];
        } else {
            break;
        }
    }
    if scan.starts_with("[inference]") {
        let mut stanza_end = 0usize;
        for (idx, line) in scan.lines().enumerate() {
            if idx > 0 && line.trim().is_empty() {
                stanza_end = idx;
                break;
            }
            stanza_end = idx + 1;
        }
        // Cut the stanza out of `trimmed` (preserving any skipped comment
        // lines) by slicing from the stanza start to its end.
        let stanza_start = scan.as_ptr() as usize - trimmed.as_ptr() as usize;
        let stanza_len: usize = scan
            .lines()
            .take(stanza_end)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        let cut_start = stanza_start.min(trimmed.len());
        let cut_end = (cut_start + stanza_len).min(trimmed.len());
        let mut kept = String::with_capacity(trimmed.len());
        kept.push_str(&trimmed[..cut_start]);
        kept.push_str(&trimmed[cut_end..]);
        return kept.trim().to_string();
    }

    working.trim().to_string()
}

/// Bind a loader that resolves `{% include %}` names against the registry
/// templates base, so shared fragments are reachable from the templates
/// that include them. Included templates get
/// the same frontmatter stripping as the top-level render, so a fragment's
/// `[inference]` header never leaks into the rendered prompt. Names that
/// escape the base directory resolve to not-found — the same traversal
/// boundary as `resolve_template_path` (canonicalize + starts_with).
fn bind_registry_loader(env: &mut minijinja::Environment<'_>, base: &std::path::Path) {
    let Ok(base) = base.canonicalize() else {
        // Unreadable base — includes simply don't resolve, as before.
        return;
    };
    env.set_loader(move |name| {
        if name.contains("..") || name.starts_with('/') {
            return Ok(None);
        }
        let Ok(canonical) = base.join(name).canonicalize() else {
            return Ok(None);
        };
        if !canonical.starts_with(&base) {
            return Ok(None);
        }
        match std::fs::read_to_string(&canonical) {
            Ok(content) => Ok(Some(strip_frontmatter(&content))),
            Err(_) => Ok(None),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_template_base() -> std::path::PathBuf {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../kask/registry/templates");
        assert!(
            base.is_dir(),
            "registry template directory does not exist: {}",
            base.display()
        );
        base
    }

    #[test]
    fn test_strip_frontmatter_removes_yaml_header() {
        let input = "---\ntemplate_type: KnowAct\ncontract:\n  input: {}\n---\nHello {{ name }}!";
        let result = strip_frontmatter(input);
        assert_eq!(result, "Hello {{ name }}!");
    }

    #[test]
    fn test_strip_frontmatter_preserves_content_without_header() {
        let input = "Hello {{ name }}!";
        let result = strip_frontmatter(input);
        assert_eq!(result, "Hello {{ name }}!");
    }

    #[test]
    fn contract_validation_rejects_missing_required_render_inputs() {
        let input = "[inference]\ncontract:\n  input:\n    grill_ratings:\n      type: array\n    grill_verdict:\n      type: string\n---\n{{ grill_verdict }}";
        let context = std::collections::HashMap::from([(
            "verification".to_string(),
            hkask_types::AnyJsonValue::from(serde_json::json!({
                "verdict": "rewrite_needed",
                "ratings": []
            })),
        )]);

        let error = validate_contract_inputs(input, &context).expect_err("missing flat inputs");

        assert!(error.contains("grill_ratings"), "got: {error}");
        assert!(error.contains("grill_verdict"), "got: {error}");
    }

    #[test]
    fn contract_validation_allows_optional_and_defaulted_inputs() {
        let input = "[inference]\ncontract:\n  input:\n    required_value: string\n    nullable_value: string|null\n    defaulted_value:\n      type: string\n      default: fallback\n    explicitly_optional:\n      type: string\n      required: false\n---\n{{ required_value }}";
        let context = std::collections::HashMap::from([(
            "required_value".to_string(),
            hkask_types::AnyJsonValue::from(serde_json::json!("present")),
        )]);

        validate_contract_inputs(input, &context).expect("optional inputs may be absent");
    }

    #[test]
    fn test_strip_frontmatter_handles_empty_body() {
        let input = "---\nfoo: bar\n---\n";
        let result = strip_frontmatter(input);
        assert_eq!(result, "");
    }

    // ── The on-disk convention (219/315 templates) ─────────────────

    #[test]
    fn test_strip_inference_header_through_first_lone_terminator() {
        // The dominant convention: [inference]-keyed header terminated by a
        // lone `---` — NOT leading frontmatter. The old stripper matched 0
        // templates because it required the file to START with `---`.
        let input =
            "[inference]\ncontract:\n  input: {}\nvisibility: Public\n---\nYou are a triage agent.";
        let result = strip_frontmatter(input);
        assert_eq!(result, "You are a triage agent.");
        assert!(!result.contains("contract"));
        assert!(!result.contains("visibility"));
    }

    #[test]
    fn test_strip_body_inference_param_stanza() {
        // The body's own [inference] block (temperature etc.) is tool-execution
        // metadata and must not leak into the rendered prompt.
        let input = "[inference]\ncontract: {}\nvisibility: Public\n---\n[inference]\ntemperature = 0.0\nwork_effort = \"low\"\n\nYou are a triage agent.";
        let result = strip_frontmatter(input);
        assert_eq!(result, "You are a triage agent.");
        assert!(!result.contains("temperature"));
        assert!(!result.contains("work_effort"));
        assert!(!result.contains("[inference]"));
    }

    #[test]
    fn test_body_inference_stanza_without_trailing_blank_line() {
        // A stanza that runs to EOF (no blank line) is still stripped whole.
        let input = "[inference]\ncontract: {}\n---\n[inference]\ntemperature = 0.1";
        let result = strip_frontmatter(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_inference_mention_in_running_prose_is_preserved() {
        // Only a body-LEADING stanza is stripped; a mention in prose stays.
        let input = "[inference]\ncontract: {}\n---\nUse the [inference] block to set temperature.";
        let result = strip_frontmatter(input);
        assert!(result.contains("Use the [inference] block"));
    }

    #[test]
    fn test_real_triage_template_strips_clean() {
        // The exact shape of kask/registry/templates/kanban-task-management/triage.j2.
        let input = "[inference]\ncontract:\n  input:\n    project_description: string\n  output:\n    phase: string\nvisibility: Public\n---\n{# Ontology: PKO #}\n\n[inference]\ntemperature = 0.0\nwork_effort = \"low\"\nverbosity = \"concise\"\n\nYou are a kanban task management triage agent.";
        let result = strip_frontmatter(input);
        // The {# Ontology #} Jinja comment survives stripping (minijinja
        // removes it at render time); everything else must be gone.
        assert!(
            result.contains("You are a kanban task management triage agent."),
            "got: {result:?}"
        );
        assert!(!result.contains("[inference]"));
        assert!(!result.contains("contract"));
        assert!(!result.contains("visibility"));
        assert!(!result.contains("temperature"));
        assert!(!result.contains("work_effort"));
        // Render through minijinja to confirm the comment is gone too.
        let env = minijinja::Environment::new();
        let rendered = env.render_str(&result, &()).unwrap();
        assert!(rendered.contains("You are a kanban task management triage agent."));
        assert!(!rendered.contains("{#"));
    }

    #[test]
    fn all_registry_templates_conform() {
        fn collect_templates(
            directory: &std::path::Path,
            templates: &mut Vec<std::path::PathBuf>,
        ) -> Result<(), String> {
            let entries = std::fs::read_dir(directory)
                .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
            for entry in entries {
                let entry = entry.map_err(|error| {
                    format!(
                        "failed to read an entry in {}: {error}",
                        directory.display()
                    )
                })?;
                let path = entry.path();
                if path.is_dir() {
                    collect_templates(&path, templates)?;
                } else if path.extension().is_some_and(|extension| extension == "j2") {
                    templates.push(path);
                }
            }
            Ok(())
        }

        let base = registry_template_base();
        let mut templates = Vec::new();
        if let Err(error) = collect_templates(&base, &mut templates) {
            panic!("{error}");
        }
        assert!(!templates.is_empty(), "registry template census is empty");

        let mut parse_errors = Vec::new();
        let mut python_templates = Vec::new();
        for path in templates {
            let content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) => panic!("failed to read {}: {error}", path.display()),
            };
            if content.lines().any(|line| {
                line.starts_with("import ")
                    || (line.starts_with("from ") && line.contains(" import "))
            }) {
                python_templates.push(path.display().to_string());
            }

            let Some(header) = template_metadata_header(&content) else {
                continue;
            };
            match serde_yaml::from_str::<serde_yaml::Value>(header) {
                Ok(metadata) if !metadata.is_mapping() => parse_errors.push(format!(
                    "{}: metadata header must be a YAML mapping",
                    path.display()
                )),
                Ok(_) => {}
                Err(error) => parse_errors.push(format!("{}: {error}", path.display())),
            }
        }
        assert!(
            parse_errors.is_empty(),
            "failed to parse registry contract headers:\n{}",
            parse_errors.join("\n")
        );
        assert!(
            python_templates.is_empty(),
            "registry contains Python code templates:\n{}",
            python_templates.join("\n")
        );
    }

    #[test]
    fn test_include_traversal_is_blocked() {
        let base = registry_template_base();
        let mut env = minijinja::Environment::new();
        bind_registry_loader(&mut env, &base);
        // A traversal name resolves to not-found → render error, never a
        // file outside the registry.
        let result = env.render_str("{% include \"../../Cargo.toml\" %}", serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_template_path_rejects_traversal() {
        let base = registry_template_base();
        let result = resolve_template_path(&base, "../../etc/passwd");
        assert!(result.is_none(), "path traversal must be rejected");
    }

    #[test]
    fn test_resolve_template_path_accepts_valid_ref() {
        let base = registry_template_base();
        let result = resolve_template_path(&base, "essentialist/essentialist-flow.j2");
        assert!(result.is_some(), "valid template ref must resolve");
    }

    #[test]
    fn test_read_template_file_finds_j2() {
        let base = registry_template_base();
        let result = read_template_file(&base, "essentialist/essentialist-flow");
        assert!(
            result.is_ok(),
            "should find .j2 file with extension-less ref"
        );
        let content = result.expect("checked is_ok above");
        assert!(content.contains("---"), "template should have frontmatter");
    }

    // Regression for the canonicalize-before-existence-check bug: an
    // extensionless ref whose exact path doesn't exist (the .j2 file does)
    // must resolve via the .j2 retry, not error out as "escapes base path".
    #[test]
    fn test_read_template_file_finds_j2_with_extensionless_ref() {
        let base = registry_template_base();
        // This is the exact ref that failed during the prompt-enhance run.
        let result = read_template_file(&base, "prompt-enhance/enhance-classify");
        assert!(
            result.is_ok(),
            "extensionless ref should resolve to .j2 file, got: {:?}",
            result.err()
        );
        let content = result.expect("checked is_ok above");
        assert!(
            content.contains("---"),
            "enhance-classify.j2 should have frontmatter"
        );
    }

    // Regression: when the model emits `context` as a stringified JSON string
    // instead of a bare object, `deserialize_maybe_stringified` parses the
    // string and the tool succeeds. Same pattern as `edit_file.edits`.
    #[test]
    fn test_context_accepts_stringified_json() {
        let input =
            serde_json::json!({"template_ref": "essentialist/essentialist-flow", "context": "{}"});
        let result: RenderTemplateToolInput =
            serde_json::from_value(input).expect("stringified context must be accepted");
        assert!(
            result.context.is_empty(),
            "stringified empty object must parse to empty map"
        );
    }

    // Positive path: a bare object must still work.
    #[test]
    fn test_context_accepts_bare_object() {
        let input = serde_json::json!({"template_ref": "essentialist/essentialist-flow", "context": {"task": "simplify"}});
        let result: RenderTemplateToolInput =
            serde_json::from_value(input).expect("bare object must parse");
        assert_eq!(result.context.len(), 1);
        assert!(result.context.contains_key("task"));
    }
}

#[cfg(test)]
mod corpus_sweep_tests {
    use super::*;

    /// Corpus-wide pin: every shipped .j2 template, after stripping, must not
    /// leak its `[inference]` header, contract block, or visibility line into
    /// the renderable body — and must still contain its prose. This is the
    /// C1 finding's enforcement point: the old stripper fired on 0 of 309
    /// templates because it required a leading `---`.
    #[test]
    fn corpus_templates_strip_without_leaking_metadata() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../kask/registry/templates");
        let base = base.canonicalize().expect("templates dir exists in repo");

        let mut checked = 0usize;
        let mut leaked = Vec::new();
        let mut empty = Vec::new();
        for entry in walkdir(&base) {
            let content = std::fs::read_to_string(&entry).expect("read template");
            let stripped = strip_frontmatter(&content);
            checked += 1;

            // The header's load-bearing markers must never survive as
            // leading metadata. Checked only in the first few lines — the
            // word `contract:` legitimately appears mid-prose later (e.g.
            // tdd-tracer's "verify the contract: it enforces…").
            for (idx, line) in stripped.lines().take(5).enumerate() {
                let trimmed_line = line.trim();
                if trimmed_line.starts_with("visibility:")
                    || trimmed_line.starts_with("contract:")
                    // Bracketed header markers: the algedonic templates'
                    // TOML-style [contract] block leaked wholesale when the
                    // header lacked its `---` terminator (fixed 2026-09-09).
                    || trimmed_line.starts_with("[contract]")
                    || trimmed_line.starts_with("[/contract]")
                    // Manifest residue: a KnowAct-style manifest block after
                    // leading comments survives stripping (the
                    // logo-formal-prompt leak, fixed 2026-09-09) and always
                    // carries template_type: first.
                    || trimmed_line.starts_with("template_type:")
                {
                    leaked.push(format!(
                        "{}: line {} leaked header metadata `{}`",
                        entry.display(),
                        idx,
                        trimmed_line
                    ));
                }
            }
            // A leading [inference] stanza must be gone (a mid-prose mention
            // is allowed).
            if stripped.trim_start().starts_with("[inference]") {
                leaked.push(format!(
                    "{}: leading [inference] stanza survived",
                    entry.display()
                ));
            }
            // A template must retain SOME body — a stripper that eats
            // everything is worse than one that eats nothing.
            if stripped.is_empty() {
                empty.push(entry.display().to_string());
            }
        }

        assert!(
            checked > 200,
            "corpus scan found only {checked} templates — scan path broken"
        );
        assert!(
            leaked.is_empty(),
            "{} templates leak metadata after stripping:\n{}",
            leaked.len(),
            leaked.join("\n")
        );
        assert!(
            empty.is_empty(),
            "{} templates strip to empty (over-stripping):\n{}",
            empty.len(),
            empty.join("\n")
        );
    }

    /// Corpus-wide pin: every shipped .j2 template must RENDER without a
    /// context-independent defect under the same environment the tool uses
    /// (lenient undefined behavior, registry-rooted loader). Stripping-clean
    /// is not enough — triage.j2 stripped clean but failed at render time
    /// with an unknown filter (`truncate`), and extract-hmems.j2's bare
    /// `[inference]` marker made the stanza stripper eat its `{% if %}`
    /// (orphaning the `{% else %}`) — both live-observed 2026-09-09; only a
    /// full render catches the class.
    ///
    /// Classification: only syntax errors, unknown filters/tests/functions/
    /// methods, bad escapes, and broken includes are defects — they fail
    /// regardless of context. Operation-on-undefined (InvalidOperation,
    /// UndefinedError, …) with an EMPTY context is the template's context
    /// contract failing loud — correct behavior; the caller retries with
    /// the required variables. Those are allowed here.
    #[test]
    fn corpus_templates_render_without_error() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../kask/registry/templates");
        let base = base.canonicalize().expect("templates dir exists in repo");

        let mut checked = 0usize;
        let mut failed = Vec::new();
        for entry in walkdir(&base) {
            let content = std::fs::read_to_string(&entry).expect("read template");
            let stripped = strip_frontmatter(&content);
            checked += 1;
            let mut env = minijinja::Environment::new();
            bind_registry_loader(&mut env, &base);
            if let Err(err) = env.render_str(&stripped, serde_json::json!({})) {
                match err.kind() {
                    minijinja::ErrorKind::SyntaxError
                    | minijinja::ErrorKind::UnknownFilter
                    | minijinja::ErrorKind::UnknownTest
                    | minijinja::ErrorKind::UnknownFunction
                    | minijinja::ErrorKind::UnknownMethod
                    | minijinja::ErrorKind::BadEscape
                    | minijinja::ErrorKind::BadInclude
                    | minijinja::ErrorKind::TemplateNotFound => {
                        failed.push(format!("{}: {err}", entry.display()));
                    }
                    _ => {}
                }
            }
        }

        assert!(
            checked > 200,
            "corpus scan found only {checked} templates — scan path broken"
        );
        assert!(
            failed.is_empty(),
            "{} of {checked} templates fail to render:\n  {}",
            failed.len(),
            failed.join("\n  ")
        );
    }

    /// Generic, not a name list: every `{% include %}` directive in the
    /// corpus must resolve through the registry loader and render without
    /// leaking the included template's header. Include names are extracted
    /// from the shipped templates at test time — file names belong to the
    /// skills that own them, not to this crate.
    #[test]
    fn corpus_includes_resolve_and_strip() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../kask/registry/templates");
        let base = base.canonicalize().expect("templates dir exists in repo");

        let mut checked = 0usize;
        let mut failed = Vec::new();
        for entry in walkdir(&base) {
            let content = std::fs::read_to_string(&entry).expect("read template");
            for (line_idx, line) in content.lines().enumerate() {
                let Some(include_name) = extract_include_name(line) else {
                    continue;
                };
                checked += 1;
                let mut env = minijinja::Environment::new();
                bind_registry_loader(&mut env, &base);
                match env.render_str(
                    &format!("{{% include \"{include_name}\" %}}"),
                    serde_json::json!({}),
                ) {
                    Ok(rendered) => {
                        // Leading header markers must not survive — the same
                        // leak convention as the strip sweep (first lines
                        // only; mid-prose mentions are legitimate).
                        for (idx, out_line) in rendered.lines().take(5).enumerate() {
                            let trimmed = out_line.trim();
                            if trimmed.starts_with("[inference]")
                                || trimmed.starts_with("[contract]")
                                || trimmed.starts_with("template_type:")
                            {
                                failed.push(format!(
                                    "{} line {}: include `{include_name}` leaks `{trimmed}` at output line {}",
                                    entry.display(),
                                    line_idx + 1,
                                    idx
                                ));
                            }
                        }
                    }
                    Err(err) => {
                        failed.push(format!(
                            "{} line {}: include `{include_name}` failed: {err}",
                            entry.display(),
                            line_idx + 1
                        ));
                    }
                }
            }
        }
        assert!(
            checked > 0,
            "no {{% include %}} directives found in the corpus — the scan is \
             broken or includes were removed (delete this test with the last \
             include; do not leave it vacuous)"
        );
        assert!(
            failed.is_empty(),
            "{} include failures across the corpus:\n  {}",
            failed.len(),
            failed.join("\n  ")
        );
    }

    /// Extract the include target from a `{% include "name" %}` line, if any.
    fn extract_include_name(line: &str) -> Option<String> {
        let start = line.find("{%")?;
        let rest = &line[start..];
        let end = rest.find("%}")?;
        let tag = &rest[..end];
        if !tag
            .trim_start_matches("{%")
            .trim_start()
            .starts_with("include")
        {
            return None;
        }
        let q1 = tag.find('"')?;
        let after = &tag[q1 + 1..];
        let q2 = after.find('"')?;
        Some(after[..q2].to_string())
    }

    /// Recursive .j2 walk (mirrors agent_skills/build.rs's collector).
    fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                out.extend(walkdir(&path));
            } else if path.extension().is_some_and(|e| e == "j2") {
                out.push(path);
            }
        }
        out
    }
}
