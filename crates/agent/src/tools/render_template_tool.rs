use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput, deserialize_teaching_field};
use agent_client_protocol::schema::v2 as acp;
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
/// strips the `[inference]`-keyed header (the metadata block terminated by a
/// lone `---` line, carrying the contract schema) and renders only the Jinja2
/// body.
///
/// The rendered text is a structured prompt — use it as guidance for your
/// next reasoning step, not as a final answer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenderTemplateToolInput {
    /// Template reference, e.g. "essentialist/essentialist-flow". The tool
    /// resolves this against the registry templates directory, trying the
    /// ref as-is, then with `.j2` appended.
    #[serde(deserialize_with = "deserialize_template_ref_field")]
    pub template_ref: String,
    /// Context variables for Jinja2 interpolation. Keys become template
    /// variables (e.g., `{{ task }}`, `{{ artifact }}`). Values must be
    /// JSON-serializable.
    ///
    /// Uses `AnyJsonValue` for values (not `serde_json::Value`) with
    /// `permissive_json_map_schema` so the outer schema is `type: object`
    /// while `additionalProperties` is `{}` (scalars stay unwrapped and no
    /// bare boolean schema is emitted).
    ///
    /// `deserialize_context_field` tolerates models that emit `context` as a
    /// stringified JSON string instead of a bare object, and teaches on every
    /// rejection: the error names the field, the received shape, and the
    /// accepted shape (lisp-repair L1 — an error that does not teach produces
    /// identical retries).
    #[serde(default, deserialize_with = "deserialize_context_field")]
    #[schemars(schema_with = "super::permissive_json_map_schema")]
    pub context: std::collections::HashMap<String, hkask_types::AnyJsonValue>,
}

fn deserialize_template_ref_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_teaching_field(
        deserializer,
        "template_ref",
        "a string template reference like \"essentialist/essentialist-flow\"",
        false,
    )
}

fn deserialize_context_field<'de, D>(
    deserializer: D,
) -> Result<std::collections::HashMap<String, hkask_types::AnyJsonValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_teaching_field(
        deserializer,
        "context",
        "a JSON object of template variables like {\"task\": \"...\"} (a stringified JSON object is also accepted)",
        true,
    )
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
            let input = input
                .recv()
                .await
                .map_err(|e| RenderTemplateToolOutput::Error {
                    error: format!("failed to receive input: {e}"),
                })?;
            render_registry_template(&input)
                .map(|text| RenderTemplateToolOutput::Rendered { text })
                .map_err(|error| RenderTemplateToolOutput::Error { error })
        })
    }
}

/// The tool's render, shared with delegated `host/render_template` dispatch.
pub fn render_registry_template(input: &RenderTemplateToolInput) -> Result<String, String> {
    let base_path = crate::template_base_path().ok_or_else(|| {
        "Template base path not configured. The registry templates directory is wired at startup."
            .to_string()
    })?;

    // Resolve the template file, trying ref as-is, then .j2.
    let content = read_template_file(base_path, &input.template_ref)?;

    validate_contract_inputs(&content, &input.context).map_err(|error| {
        format!(
            "Template contract validation failed for '{}': {error}",
            input.template_ref
        )
    })?;

    // Strip YAML frontmatter (--- delimited header).
    let template_body = strip_frontmatter(&content);

    // Build minijinja environment and render. The loader resolves
    // `{% include %}` names against the registry root — a bare
    // render_str environment has no templates, so includes fail.
    let mut env = minijinja::Environment::new();
    bind_registry_loader(&mut env, base_path);
    env.render_str(&template_body, &input.context)
        .map_err(|e| format!("Template rendering failed: {e}"))
}

/// Read a template file from the registry, trying ref as-is, then `.j2`.
/// Prevents path traversal outside the base directory.
///
/// `resolve_template_path` returns `None` for two reasons: the joined path
/// escapes the base directory (traversal blocked), or the file doesn't exist
/// (`canonicalize` fails). Both are safe to fall through from — the .j2
/// retry is checked against the same base path, so traversal stays blocked.
/// We only error after both attempts fail.
fn read_template_file(base_path: &std::path::Path, template_ref: &str) -> Result<String, String> {
    // Try ref as-is. `None` means either traversal blocked or file absent —
    // fall through to the .j2 retry rather than erroring immediately.
    if let Some(resolved) = resolve_template_path(base_path, template_ref) {
        if let Ok(content) = std::fs::read_to_string(&resolved) {
            return Ok(content);
        }
    }

    // Try .j2 extension — the documented extensionless-ref convention
    // (skills cite `skill/name`, the file is `name.j2`).
    if !template_ref.ends_with(".j2") {
        let j2_ref = format!("{template_ref}.j2");
        if let Some(j2_path) = resolve_template_path(base_path, &j2_ref) {
            if let Ok(content) = std::fs::read_to_string(&j2_path) {
                return Ok(content);
            }
        }
    }

    Err(format!(
        "Template not found: tried '{template_ref}' and '{template_ref}.j2' under '{}'",
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

    if !missing.is_empty() {
        return Err(format!(
            "missing required input field(s): {}",
            missing.join(", ")
        ));
    }

    let mut blank = inputs
        .iter()
        .filter_map(|(name, specification)| {
            let name = name.as_str()?;
            let non_blank = specification
                .get("non_blank")
                .and_then(serde_yaml::Value::as_bool)
                == Some(true);
            (non_blank
                && context
                    .get(name)
                    .is_some_and(|value| value.as_str().is_none_or(|text| text.trim().is_empty())))
            .then(|| name.to_string())
        })
        .collect::<Vec<_>>();
    blank.sort();
    if blank.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "non-blank input field(s) required: {}",
            blank.join(", ")
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

/// Split a template into its optional `[inference]` metadata header and its
/// renderable body. Leading `{# … #}` Jinja comments are skipped before
/// checking for `[inference]`. Using a single splitter for both
/// `template_metadata_header` and `strip_frontmatter` guarantees contract
/// validation and frontmatter stripping cannot disagree on whether a header is
/// present (e.g. when `[inference]` has trailing whitespace or CRLF endings).
fn split_template_frontmatter(content: &str) -> (Option<&str>, &str) {
    let mut working = content;
    loop {
        let Some(first_line) = working.lines().next() else {
            return (None, content);
        };
        if !first_line.trim().starts_with("{#") {
            break;
        }
        let Some(close_pos) = working.find("#}") else {
            return (None, working);
        };
        working = working[close_pos + 2..].trim_start_matches(['\r', '\n']);
    }

    let Some(first_line) = working.lines().next() else {
        return (None, working);
    };
    if first_line.trim() == "[inference]" {
        let after_marker = working
            .get(first_line.len()..)
            .unwrap_or("")
            .trim_start_matches(['\r', '\n']);
        let mut offset = 0usize;
        for line in after_marker.split_inclusive('\n') {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed == "---" {
                let header = &after_marker[..offset];
                let body = &after_marker[offset + line.len()..];
                return (Some(header), body);
            }
            offset += line.len();
        }
    }
    (None, working)
}

fn template_metadata_header(content: &str) -> Option<&str> {
    split_template_frontmatter(content).0
}

/// Strip the template metadata header from a template file, leaving only
/// the renderable prompt body.
///
/// The on-disk convention is:
///
/// ```text
/// {# goal: … #}
/// [inference]
/// contract:
///   input: …
///   output: …
/// ---
/// <body>
/// ```
///
/// i.e. an `[inference]`-keyed header holding the contract schema that is
/// NOT YAML-frontmatter and therefore not delimited by leading `---` — the
/// terminator is a lone `---` line *after* the header. Everything from a
/// leading `[inference]` line through the first lone `---` line is
/// stripped; a file with no header passes through unchanged.
///
/// The header is the only metadata carrier. The former body-leading
/// `[inference]` parameter stanza (temperature/work_effort/verbosity/
/// thinking_budget) and the header `visibility:` key had no consumer and
/// were removed corpus-wide on 2026-09-28; `all_registry_templates_conform`
/// rejects their reintroduction.
fn strip_frontmatter(content: &str) -> String {
    split_template_frontmatter(content).1.trim().to_string()
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

    fn render_template_for_test(
        template_ref: &str,
        context: serde_json::Value,
    ) -> Result<String, String> {
        let base = registry_template_base();
        let content = read_template_file(&base, template_ref)?;
        let context: std::collections::HashMap<String, hkask_types::AnyJsonValue> =
            serde_json::from_value(context).map_err(|error| error.to_string())?;
        validate_contract_inputs(&content, &context)?;
        let body = strip_frontmatter(&content);
        let mut environment = minijinja::Environment::new();
        bind_registry_loader(&mut environment, &base);
        environment
            .render_str(&body, &context)
            .map_err(|error| error.to_string())
    }

    #[test]
    fn corpus_audit_repairs_are_pinned_at_template_handoffs() {
        let complete_intake = render_template_for_test(
            "product-manager/intake-brief",
            serde_json::json!({
                "requirement": "Prevent broken gates",
                "spec_provenance": "operator provided 2026-10-17",
                "acceptance_criteria": ["Malformed gates fail", "Valid gates pass"],
                "constraints": ["Do not touch concurrent streams"],
                "standing_rules": ["Fix the tool"]
            }),
        )
        .expect("complete intake renders");
        assert!(complete_intake.contains("\"missing_fields\": []"));

        let sparse_intake = render_template_for_test(
            "product-manager/intake-brief",
            serde_json::json!({
                "requirement": "",
                "spec_provenance": "",
                "acceptance_criteria": [],
                "constraints": [],
                "standing_rules": []
            }),
        )
        .expect("sparse intake renders");
        for field in [
            "requirement",
            "spec_provenance",
            "acceptance_criteria",
            "constraints",
            "standing_rules",
        ] {
            assert!(
                sparse_intake.contains(&format!("\"field\":\"{field}\"")),
                "missing question for {field}: {sparse_intake}"
            );
        }

        let excessive_criteria = render_template_for_test(
            "product-manager/intake-brief",
            serde_json::json!({
                "requirement": "Prevent broken gates",
                "spec_provenance": "operator provided 2026-10-17",
                "acceptance_criteria": ["a", "b", "c", "d", "e"],
                "constraints": ["bounded"],
                "standing_rules": ["fix the tool"]
            }),
        )
        .expect("excessive criteria intake renders");
        assert!(excessive_criteria.contains("\"field\":\"acceptance_criteria\""));

        let design = render_template_for_test(
            "program-manager/design-review",
            serde_json::json!({
                "requirement": "Prevent recurrence",
                "spec_provenance": "operator provided 2026-10-17",
                "design_pattern": "parser-backed gate",
                "invariants": ["one parser", "positive controls"],
                "lazy_version": "grep only",
                "premortem_failure_modes": ["misses multiline forms"],
                "is_replacement_task": true,
                "deleted_targets": ["ad-hoc parser"],
                "unmapped_requirements": []
            }),
        )
        .expect("design review renders");
        assert!(design.contains("\"invariants\": [\"one parser\",\"positive controls\"]"));
        assert!(!design.contains("\"invariants\": [["));
        assert!(design.contains("\"premortem_failure_modes\": [\"misses multiline forms\"]"));
        assert!(design.contains("\"deleted_targets\": [\"ad-hoc parser\"]"));
        assert!(design.contains("\"unmapped_requirements\": []"));

        let verified_closeout = render_template_for_test(
            "program-manager/closeout-report",
            serde_json::json!({
                "goal_text": "Prevent broken gates",
                "functional_outcome": "Broken gates now fail CI.",
                "change_summary": "Added a parser-backed gate over skill Lisp forms.",
                "decisions": [["Parser-backed, not grep", "curator:h_mem:d1"]],
                "open_items": [],
                "validation": [{
                    "command": "bash gate.sh",
                    "observed_output": "OK: 324 forms",
                    "verdict": "pass"
                }],
                "learning": "Use the production parser."
            }),
        )
        .expect("verified closeout renders");
        assert!(verified_closeout.contains("`bash gate.sh`"));
        assert!(verified_closeout.contains("OK: 324 forms"));
        assert!(verified_closeout.contains("| pass |"));
        // The DoD-verified change summary must reach the report, not be
        // re-narrated by the model: the section had no producer field before
        // the corpus-audit repair.
        assert!(
            verified_closeout.contains("Added a parser-backed gate over skill Lisp forms."),
            "change_summary did not reach the report: {verified_closeout}"
        );
        assert!(
            verified_closeout.contains("| Parser-backed, not grep | curator:h_mem:d1 |"),
            "decisions ledger did not render: {verified_closeout}"
        );

        let unverified_closeout = render_template_for_test(
            "program-manager/closeout-report",
            serde_json::json!({
                "goal_text": "Prevent broken gates",
                "functional_outcome": "Not verified.",
                "change_summary": "No change landed.",
                "decisions": [],
                "open_items": [],
                "validation": [],
                "learning": "Missing evidence stays visible."
            }),
        )
        .expect("unverified closeout renders");
        assert!(unverified_closeout.contains("verification is **not run**, not passed"));
        assert!(
            unverified_closeout.contains("None were recorded this cycle."),
            "empty decisions must read as none-recorded: {unverified_closeout}"
        );

        let sankey = render_template_for_test(
            "sankey-flow/present-sankey",
            serde_json::json!({
                "subject": "fixture",
                "domain": "resource-allocation",
                "weight_unit": "USD",
                "conservation_mode": "mandatory",
                "conservation_check": "observed_balanced",
                "nodes": [{"id":"Revenue"},{"id":"Profit"},{"id":"Costs"},{"id":"Payroll"}],
                "edges": [
                    {"source":"Revenue","target":"Profit","weight":5000,"weight_source":"statement"},
                    {"source":"Revenue","target":"Costs","weight":3000,"weight_source":"statement"},
                    {"source":"Costs","target":"Payroll","weight":2000,"weight_source":"ledger"}
                ]
            }),
        )
        .expect("Sankey render succeeds");
        for row in [
            "Revenue,Profit,5000",
            "Revenue,Costs,3000",
            "Costs,Payroll,2000",
        ] {
            assert_eq!(sankey.matches(row).count(), 1, "row drift: {row}\n{sankey}");
        }

        let approved_only = render_template_for_test(
            "therapy/report",
            serde_json::json!({
                "target": "fixture",
                "findings": [
                    {"id":"F001","issue_type":"contradiction"},
                    {"id":"F002","issue_type":"reification_candidate"}
                ],
                "proposals": [
                    {"approved":true,"finding_id":"F001","strategy":"forget"},
                    {"approved":true,"finding_id":"F002","strategy":"create_skill"}
                ],
                "executions": [],
                "values_pass": {}
            }),
        )
        .expect("approved-only therapy report renders");
        assert!(approved_only.contains("Contradictions resolved: 0"));
        assert!(approved_only.contains("Skills created: 0"));

        let executed = render_template_for_test(
            "therapy/report",
            serde_json::json!({
                "target": "fixture",
                "findings": [
                    {"id":"F001","issue_type":"contradiction"},
                    {"id":"F002","issue_type":"reification_candidate"}
                ],
                "proposals": [
                    {"approved":true,"finding_id":"F001","strategy":"forget"},
                    {"approved":true,"finding_id":"F002","strategy":"create_skill"}
                ],
                "executions": [
                    {"finding_id":"F001","strategy":"forget","success":true,"outcome":"deleted"},
                    {"finding_id":"F001","strategy":"forget","success":true,"outcome":"duplicate receipt"},
                    {"finding_id":"F002","strategy":"create_skill","success":true,"outcome":"created"}
                ],
                "values_pass": {}
            }),
        )
        .expect("executed therapy report renders");
        assert!(executed.contains("Contradictions resolved: 1"));
        assert!(!executed.contains("Contradictions resolved: 2"));
        assert!(executed.contains("Skills created: 1"));
    }

    #[test]
    fn optional_cross_skill_inputs_are_actually_optional() {
        for (template_ref, context) in [
            (
                "falsifiability/falsifiability-admit",
                serde_json::json!({"target": "A treatment reduces p95 latency"}),
            ),
            (
                "task-breakdown/task-breakdown-decompose",
                serde_json::json!({
                    "context_summary": "Implement a parser guard",
                    "dependency_graph": [],
                    "risks": [],
                    "refinement_directives": [],
                    "parallelization_hint": "sequential"
                }),
            ),
            (
                "skill-discovery/skill-discovery-route",
                serde_json::json!({
                    "task_description": "debug a performance regression",
                    "task_context": "Rust workspace",
                    "skill_catalog": []
                }),
            ),
        ] {
            render_template_for_test(template_ref, context).unwrap_or_else(|error| {
                panic!("{template_ref} should accept omitted optional inputs: {error}")
            });
        }
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

        let present_but_blank = std::collections::HashMap::from([
            (
                "grill_ratings".to_string(),
                hkask_types::AnyJsonValue::from(serde_json::json!([])),
            ),
            (
                "grill_verdict".to_string(),
                hkask_types::AnyJsonValue::from(serde_json::json!("")),
            ),
        ]);
        validate_contract_inputs(input, &present_but_blank)
            .expect("required strings without non_blank retain presence-only validation");
    }

    #[test]
    fn expectation_inquiry_requires_active_user_goal() {
        let path = registry_template_base().join("gradient-hunter/expectation-inquiry.j2");
        let template = std::fs::read_to_string(path).expect("shipped inquiry template");
        let mut context = std::collections::HashMap::from([
            (
                "expectation".to_string(),
                hkask_types::AnyJsonValue::from(serde_json::json!({"statement": "expected"})),
            ),
            (
                "observation".to_string(),
                hkask_types::AnyJsonValue::from(serde_json::json!({"statement": "observed"})),
            ),
            (
                "probe_results".to_string(),
                hkask_types::AnyJsonValue::from(serde_json::json!([])),
            ),
        ]);
        let error = validate_contract_inputs(&template, &context)
            .expect_err("an inquiry without the user's goal cannot pass admission");
        assert!(error.contains("active_goal"), "got: {error}");

        for invalid_goal in [
            serde_json::json!(""),
            serde_json::json!(" \t\n "),
            serde_json::json!(null),
            serde_json::json!(42),
        ] {
            context.insert(
                "active_goal".to_string(),
                hkask_types::AnyJsonValue::from(invalid_goal),
            );
            let error = validate_contract_inputs(&template, &context)
                .expect_err("an inquiry with a blank or non-string goal cannot render");
            assert!(error.contains("active_goal"), "got: {error}");
            assert!(error.contains("non-blank"), "got: {error}");
        }

        context.insert(
            "active_goal".to_string(),
            hkask_types::AnyJsonValue::from(serde_json::json!("Investigate meaningful surprises")),
        );
        validate_contract_inputs(&template, &context).expect("goal-carrying inquiry admits");
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
    fn test_strip_inference_header_through_first_lone_terminator() {
        // The dominant convention: [inference]-keyed header terminated by a
        // lone `---` — NOT leading frontmatter. The old stripper matched 0
        // templates because it required the file to START with `---`.
        let input = "[inference]\ncontract:\n  input: {}\n---\nYou are a triage agent.";
        let result = strip_frontmatter(input);
        assert_eq!(result, "You are a triage agent.");
        assert!(!result.contains("contract"));
    }

    #[test]
    fn test_inference_mention_in_running_prose_is_preserved() {
        // Only the header is stripped; an `[inference]` mention in prose stays.
        let input = "[inference]\ncontract: {}\n---\nUse the [inference] block to set temperature.";
        let result = strip_frontmatter(input);
        assert!(result.contains("Use the [inference] block"));
    }

    #[test]
    fn test_real_triage_template_strips_clean() {
        // The shape of kask/registry/templates/kanban-task-management/triage.j2.
        let input = "{# goal: Triage. #}\n[inference]\ncontract:\n  input:\n    project_description: string\n  output:\n    phase: string\n---\n{# Ontology: PKO #}\n\nYou are a kanban task management triage agent.";
        let result = strip_frontmatter(input);
        // The {# Ontology #} Jinja comment survives stripping (minijinja
        // removes it at render time); the header must be gone.
        assert!(
            result.contains("You are a kanban task management triage agent."),
            "got: {result:?}"
        );
        assert!(!result.contains("[inference]"));
        assert!(!result.contains("contract"));
        // Render through minijinja to confirm the comment is gone too.
        let env = minijinja::Environment::new();
        let rendered = env.render_str(&result, &()).unwrap();
        assert!(rendered.contains("You are a kanban task management triage agent."));
        assert!(!rendered.contains("{#"));
    }

    /// The former body `[inference]` parameter stanza keys. No code reads
    /// them (removed corpus-wide 2026-09-28); a line matching one is a
    /// reintroduced performative stanza that would leak verbatim into the
    /// rendered prompt.
    fn is_inference_param_line(line: &str) -> bool {
        let trimmed = line.trim_start();
        ["temperature", "work_effort", "verbosity", "thinking_budget"]
            .iter()
            .any(|key| {
                trimmed
                    .strip_prefix(key)
                    .is_some_and(|rest| rest.starts_with(" =") || rest.starts_with('='))
            })
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
        let mut leaked_param_templates = Vec::new();
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

            if strip_frontmatter(&content)
                .lines()
                .any(|line| is_inference_param_line(line))
            {
                leaked_param_templates.push(path.display().to_string());
            }

            let Some(header) = template_metadata_header(&content) else {
                continue;
            };
            match serde_yaml::from_str::<serde_yaml::Value>(header) {
                Ok(metadata) if !metadata.is_mapping() => parse_errors.push(format!(
                    "{}: metadata header must be a YAML mapping",
                    path.display()
                )),
                // The header carries `contract` only. `visibility` had no
                // reader anywhere (removed corpus-wide 2026-09-28); any other
                // key is decoration nothing consumes.
                Ok(metadata) => {
                    let extra: Vec<String> = metadata
                        .as_mapping()
                        .into_iter()
                        .flat_map(|mapping| mapping.keys())
                        .filter_map(|key| key.as_str())
                        .filter(|key| *key != "contract")
                        .map(str::to_owned)
                        .collect();
                    if !extra.is_empty() {
                        parse_errors.push(format!(
                            "{}: header carries keys nothing reads: {}",
                            path.display(),
                            extra.join(", ")
                        ));
                    }
                }
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
        assert!(
            leaked_param_templates.is_empty(),
            "templates carrying a body [inference] parameter stanza \
             (performative: no code reads these keys; removed 2026-09-28):\n{}",
            leaked_param_templates.join("\n")
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
    // instead of a bare object, `deserialize_context_field` parses the
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

    #[test]
    fn context_schema_advertises_permissive_values() {
        let schema =
            serde_json::to_value(schemars::schema_for!(RenderTemplateToolInput)).expect("schema");
        assert_eq!(
            schema["properties"]["context"]["type"],
            serde_json::json!("object")
        );
        assert_eq!(
            schema["properties"]["context"]["additionalProperties"],
            serde_json::json!({})
        );
        assert!(schema["properties"]["context"].get("properties").is_none());
        assert!(hkask_types::find_boolean_schema_positions(&schema).is_empty());
    }

    // Wire-path pin (2026-10-19): `render_template.context` and `lisp_eval.env`
    // are declared with the SAME `permissive_json_map_schema` helper and must
    // advertise the identical permissive shape on the wire — through the full
    // `normalize_tool_schema` pipeline, not only at raw `schema_for!` time. A
    // live session observed the two advertised differently (`context` arrived
    // at the model closed: `properties:{}, additionalProperties:false`, while
    // `env` arrived permissive), with no in-tree transform producing that
    // split (binary was current; `normalize_tool_schema` only injects
    // `properties` at the root). The raw-schema pins above cannot catch a
    // transform that acts between generation and the wire, so this pin asserts
    // the normalized wire form of BOTH params and their equality: if any layer
    // tightens `context` (or `env`) into a closed object, this fails.
    #[test]
    fn context_and_env_advertise_the_same_permissive_wire_schema() {
        fn normalized_tool_schema<T: crate::AgentTool>() -> serde_json::Value {
            let mut schema = T::input_schema().to_value();
            language_model::tool_schema::normalize_tool_schema(&mut schema);
            schema
        }
        let render = normalized_tool_schema::<super::super::RenderTemplateTool>();
        let lisp = normalized_tool_schema::<super::super::LispEvalTool>();
        for (tool_name, schema, field) in [
            ("render_template", &render, "context"),
            ("lisp_eval", &lisp, "env"),
        ] {
            let field_schema = schema
                .get("properties")
                .and_then(|p| p.get(field))
                .unwrap_or_else(|| panic!("{tool_name}.{field} missing from wire schema"));
            assert_eq!(
                field_schema.get("type"),
                Some(&serde_json::json!("object")),
                "{tool_name}.{field} must stay object-typed on the wire"
            );
            assert_eq!(
                field_schema.get("additionalProperties"),
                Some(&serde_json::json!({})),
                "{tool_name}.{field} must stay permissive on the wire"
            );
            assert!(
                field_schema.get("properties").is_none(),
                "{tool_name}.{field} must not gain a closed `properties` map on the wire"
            );
        }
        // The two fields share one declaration helper; their wire schemas must
        // be structurally identical (modulo each field's own doc-comment
        // `description`), not merely individually well-formed.
        let structural = |schema: Option<&serde_json::Value>| {
            let mut s = schema
                .expect("field schema")
                .as_object()
                .expect("object schema")
                .clone();
            s.remove("description");
            serde_json::Value::Object(s)
        };
        assert_eq!(
            structural(render.get("properties").and_then(|p| p.get("context"))),
            structural(lisp.get("properties").and_then(|p| p.get("env")))
        );
    }

    #[test]
    fn header_with_trailing_whitespace_or_crlf_does_not_bypass_contract_validation() {
        for header_line in ["[inference]   \n", "[inference]\r\n"] {
            let template =
                format!("{header_line}contract:\n  input:\n    required_key: string\n---\nbody");
            let stripped = strip_frontmatter(&template);
            assert_eq!(stripped, "body", "strip_frontmatter stripped the header");
            let err = validate_contract_inputs(&template, &std::collections::HashMap::new())
                .expect_err("contract validation must not be bypassed when header is stripped");
            assert!(err.contains("required_key"), "got: {err}");
        }
    }

    #[test]
    fn non_string_template_ref_teaches_field_and_shape() {
        let err = serde_json::from_value::<RenderTemplateToolInput>(
            serde_json::json!({"template_ref": 42, "context": {}}),
        )
        .expect_err("non-string template_ref must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("template_ref: received number 42"),
            "got: {msg}"
        );
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
            // prose that says "verify the contract: it enforces…").
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
