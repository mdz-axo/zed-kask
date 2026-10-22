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
    /// JSON text encoding an object of template variables, for example
    /// `"{\"task\":\"simplify\",\"items\":[1,2]}"`. Keys become Jinja2
    /// variables; nested arrays, objects and scalar values retain their types.
    /// Malformed JSON or a non-object document is rejected before rendering.
    #[serde(
        default,
        deserialize_with = "deserialize_context_field",
        serialize_with = "hkask_types::tool_schema::serialize_json_text"
    )]
    #[schemars(schema_with = "hkask_types::tool_schema::json_text_schema")]
    pub context: std::collections::HashMap<String, serde_json::Value>,
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
) -> Result<std::collections::HashMap<String, serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    hkask_types::tool_schema::deserialize_json_text_object(deserializer, "context")
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

    fn deserialize_replay_input(input: serde_json::Value) -> Result<Self::Input> {
        super::deserialize_saved_json_map(input, "context")
    }

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
    context: &std::collections::HashMap<String, serde_json::Value>,
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

    pub(super) fn render_template_for_test(
        template_ref: &str,
        context: serde_json::Value,
    ) -> Result<String, String> {
        let base = registry_template_base();
        let content = read_template_file(&base, template_ref)?;
        let context: std::collections::HashMap<String, serde_json::Value> =
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
    fn mcp_review_artifact_serializes_supplied_evidence_without_reconstruction() {
        let context = serde_json::json!({
            "server": "fixture-server", "run_date": "2026-10-20",
            "tools": [{"name": "wire_alias", "status": "pending-rulings", "anchor": "unresolved"}],
            "findings": [{"id": "F1", "tool": "wire_alias", "claim": "Quoted \"evidence\"\nnext line", "status": "pending-rulings"}],
            "reconciliation": {
                "input_valid": true, "source_count": 3, "live_count": 3, "reviewed_count": 1,
                "source_live_match": false, "review_complete": false,
                "source_only_names": ["old"], "live_only_names": ["new"],
                "not_reviewed_names": ["old", "other"], "unknown_reviewed_names": [],
                "result": "source_live_drift"
            },
            "gated_pending": ["The source ruling remains open"],
            "notes": ["Not a complete server review", "Unicode: λ; braces: {x}"],
            "efficiency_metric_space": {"harness": "fixture", "axes": [{"axis": "tokens", "type": "blocked"}]},
            "anchor_registry": {"ruled": [], "pending": ["unresolved"]},
            "citation_check": {"result": "not_run", "checked": null},
            "ask_first": {"batched_ask_presented": false, "defaults_applied": ["flagged technical default"]}
        });
        let output = render_template_for_test("mcp-tool-review/findings-artifact", context.clone())
            .expect("completed-context deterministic formatter renders");
        let artifact: serde_json::Value = serde_json::from_str(&output)
            .expect("the render itself is the JSON artifact, not a generation prompt");
        let template = read_template_file(
            &registry_template_base(),
            "mcp-tool-review/findings-artifact",
        )
        .expect("artifact template exists");
        let metadata: serde_yaml::Value = serde_yaml::from_str(
            template_metadata_header(&template).expect("artifact declares its contract"),
        )
        .expect("artifact metadata is valid YAML");
        let declared_fields = metadata["contract"]["output"]
            .as_mapping()
            .expect("artifact output contract is a mapping")
            .keys()
            .map(|key| key.as_str().expect("output names are strings"))
            .collect::<std::collections::BTreeSet<_>>();
        let rendered_fields = artifact
            .as_object()
            .expect("artifact is a flat JSON object")
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            declared_fields, rendered_fields,
            "output contract matches the artifact"
        );
        assert_eq!(artifact["capability"], "mcp-tool-review");
        for key in [
            "server",
            "run_date",
            "tools",
            "findings",
            "reconciliation",
            "notes",
            "efficiency_metric_space",
            "anchor_registry",
            "citation_check",
            "ask_first",
        ] {
            assert_eq!(artifact[key], context[key], "lossless {key} handoff");
        }
        assert_eq!(artifact["gated_pending_rulings"], context["gated_pending"]);
        assert_eq!(artifact["reconciliation"]["review_complete"], false);
        assert_eq!(artifact["citation_check"]["result"], "not_run");
        for key in [
            "reconciliation",
            "efficiency_metric_space",
            "anchor_registry",
            "citation_check",
            "ask_first",
        ] {
            let mut missing = context.clone();
            missing.as_object_mut().expect("context object").remove(key);
            assert!(
                render_template_for_test("mcp-tool-review/findings-artifact", missing)
                    .expect_err("missing evidence must not be reconstructed")
                    .contains(key)
            );
        }
    }

    // Run the authored filters, not a Rust translation of the decision rules.
    fn grill_jq(section: &str, input: serde_json::Value) -> Result<serde_json::Value, String> {
        authored_jq("grill-me", section, input)
    }

    fn authored_jq(
        skill: &str,
        section: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use smol::{io::AsyncWriteExt, process::Command};
        use std::process::Stdio;
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.agents/skills")
            .join(skill)
            .join("SKILL.md");
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let filter = source
            .split_once(&format!("### {section}\n"))
            .and_then(|(_, tail)| tail.split_once("```jq\n"))
            .and_then(|(_, tail)| tail.split_once("\n```"))
            .map(|(filter, _)| filter)
            .ok_or_else(|| format!("missing authored {section} filter"))?;
        let mut child = Command::new("jq")
            .args(["-ce", filter])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("jq required: {e}"))?;
        smol::block_on(async {
            let bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
            let mut stdin = child.stdin.take().expect("piped input");
            stdin.write_all(&bytes).await.map_err(|e| e.to_string())?;
            drop(stdin);
            let output = child.output().await.map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
        })
    }

    #[test]
    fn superforecasting_record_and_admission_preserve_source_ownership() {
        use serde_json::json;
        let events = json!([
            {"id":"root", "question":"Root", "deadline":"2027-01-01", "probability":0.6, "depends_on":[]},
            {"id":"outcome", "question":"QUESTION_SENTINEL", "deadline":"2027-01-01", "probability":0.5,
             "depends_on":[{"parent_event_ids":["root"],"conditionals":[0.2,0.8]}]}
        ]);
        let mut input = json!({
            "stage0":{"forecasting_question":"QUESTION_SENTINEL", "resolution_criteria":"CRITERIA_SENTINEL", "resolution_deadline":"2027-01-01"},
            "stage6":{"final_probability":0.56,"confidence_level":"medium"},
            "outcome_node_id":"outcome",
            "quantify":{"arguments":{"events":events},"response":{"nodes":[
                {"id":"root","question":"Root","deadline":"2027-01-01","marginal_probability":0.6},
                {"id":"outcome","question":"QUESTION_SENTINEL","deadline":"2027-01-01","marginal_probability":0.56}
            ]}},
            "declaration":{"forecast_id":"caller-owned","events":events,"resolver":"declared-test-source",
                "resolution_at":{"root":"2027-01-01T12:00:00Z","outcome":"2027-01-01T12:00:00Z"}},
            "narrative":{"reasoning_summary":"EVIDENCE_SENTINEL","key_assumptions":["ASSUMPTION_SENTINEL"]},
            "receipt":null
        });
        let binding = authored_jq("superforecasting", "Record assembly", input.clone())
            .expect("dependent marginal, not raw prior, binds");
        assert_eq!(
            binding["declaration"], input["declaration"],
            "pass exact arguments, not a reconstructed tree"
        );
        let record = &binding["record_result"];
        assert_eq!(record["admission_receipt"], json!(null));
        assert_eq!(record["forecast_record"]["probability"], 0.56);
        assert_eq!(
            record["forecast_record"]["resolution_criteria"],
            "CRITERIA_SENTINEL"
        );
        assert_eq!(record["tracking_id"], "caller-owned");
        assert!(record["forecast_record"].get("timestamp").is_none());
        for (path, value) in [
            ("final_probability", json!(0.42)),
            ("final_probability", json!(null)),
        ] {
            let mut bad = input.clone();
            bad["stage6"][path] = value;
            assert!(authored_jq("superforecasting", "Record assembly", bad).is_err());
        }
        let mut stale = input.clone();
        stale["declaration"]["events"][1]["probability"] = json!(0.42);
        assert!(authored_jq("superforecasting", "Record assembly", stale).is_err());
        for key in ["question", "deadline", "id"] {
            let mut bad = input.clone();
            bad["quantify"]["response"]["nodes"][1][key] = json!("changed");
            assert!(
                authored_jq("superforecasting", "Record assembly", bad).is_err(),
                "changed {key}"
            );
        }
        let mut duplicate = input.clone();
        let node = duplicate["quantify"]["response"]["nodes"][1].clone();
        duplicate["quantify"]["response"]["nodes"]
            .as_array_mut()
            .expect("nodes")
            .push(node);
        assert!(authored_jq("superforecasting", "Record assembly", duplicate).is_err());
        for key in ["probability", "timestamp", "tracking_id"] {
            let mut injected = input.clone();
            injected["narrative"][key] = json!("injected");
            assert!(
                authored_jq("superforecasting", "Record assembly", injected).is_err(),
                "reject narrative {key}"
            );
        }
        for malformed in [
            json!(null),
            json!({}),
            json!({"reasoning_summary":"ok","key_assumptions":"not an array"}),
        ] {
            let mut bad = input.clone();
            bad["narrative"] = malformed;
            assert!(authored_jq("superforecasting", "Record assembly", bad).is_err());
        }
        let mut omitted = input.clone();
        omitted.as_object_mut().expect("handoff").remove("receipt");
        assert!(authored_jq("superforecasting", "Record assembly", omitted).is_err());
        let quality_rows = json!({"crit":{"calibration_realism":[1,1,1,1],"confidence_justification":[1,1,1,1],"evidence_trail":[1,1,1,1],"record_completeness":[1,1,1,1]},"gate_findings":[]});
        assert_eq!(
            authored_jq(
                "superforecasting",
                "Quality input shape",
                quality_rows.clone()
            )
            .expect("raw critic object"),
            quality_rows["crit"]
        );
        for malformed in [
            json!([]),
            json!([["calibration_realism", [1, 1, 1, 1]]]),
            json!({"calibration_realism":[1,1,1,1]}),
        ] {
            let mut bad = quality_rows.clone();
            bad["crit"] = malformed;
            assert!(authored_jq("superforecasting", "Quality input shape", bad).is_err());
        }
        input["receipt"] = json!({"forecast_id":"caller-owned","status":"declared","event_count":2,
            "resolver":"declared-test-source","resolution_at":input["declaration"]["resolution_at"]});
        let admitted = authored_jq("superforecasting", "Record assembly", input.clone())
            .expect("actual receipt shape");
        let admitted = &admitted["record_result"];
        assert_eq!(admitted["admission_receipt"], input["receipt"]);
        for key in [
            "forecast_id",
            "status",
            "resolver",
            "resolution_at",
            "event_count",
        ] {
            let mut bad = input.clone();
            bad["receipt"][key] = json!(null);
            assert!(
                authored_jq("superforecasting", "Record assembly", bad).is_err(),
                "receipt {key}"
            );
        }
        let prompt = render_template_for_test("superforecasting/stage_7_record", json!({
            "forecasting_question":"QUESTION_SENTINEL","final_probability":0.56,"confidence_level":"medium",
            "pipeline_summary":{"stage0":input["stage0"],"stage6":input["stage6"],"evidence":["EVIDENCE_SENTINEL"]},
            "resolution_criteria":"CRITERIA_SENTINEL","expiration_date":"2027-01-01"
        })).expect("real narrative renderer");
        assert!(
            prompt.contains("EVIDENCE_SENTINEL"),
            "raw evidence survives rather than only counts"
        );
        assert!(!prompt.contains("<ISO 8601 timestamp>"));
        let quality = render_template_for_test(
            "superforecasting/forecast-quality-gate",
            json!({
                "forecasting_question":"QUESTION_SENTINEL","calibration_result":input["stage6"],
                "record_result":admitted,"synthesis_output":{"evidence":["EVIDENCE_SENTINEL"]}
            }),
        )
        .expect("assembled record feeds real quality renderer");
        assert!(quality.contains("CRITERIA_SENTINEL") && quality.contains("caller-owned"));
        assert!(quality.contains("\"crit\"") && !quality.contains("\"calibration_realism_score\""));
    }

    fn grill_input() -> serde_json::Value {
        serde_json::json!({
            "question_bank": [
                {"id":11,"question":"QUESTION_SENTINEL","level":2,"area":"math"},
                {"id":12,"question":"When may an answer be graded?","level":2,"area":"evidence"}
            ],
            "answers": [
                {"id":1,"question_id":11,"text":"ANSWER_SENTINEL","round":1,"rating":null,"failed":false,"note":null},
                {"id":2,"question_id":12,"text":"After it exists","round":1,"rating":null,"failed":false,"note":null}
            ],
            "answer_ids":[],"evaluations":[],"level":2,"round":1,"mode":"interrogate","calibrated_level":null
        })
    }

    #[test]
    fn grill_feedback_counts_distinct_answers_not_regrades() {
        let mut input = grill_input();
        input["answer_ids"] = serde_json::json!([2, 1]);
        input["evaluations"] = serde_json::json!([
            {"answer_id":2,"rating":"Solid","note":"Correct"},
            {"answer_id":1,"rating":"Gap","note":"Incorrect"}
        ]);
        for rating in ["Gap", "Gap", "Solid"] {
            let result = grill_jq("Feedback/update", input.clone()).expect("accepted grading");
            assert_eq!(
                result["attempt_counts"],
                serde_json::json!([[11, 1], [12, 0]])
            );
            assert_eq!(result["retired_ids"], serde_json::json!([]));
            input["answers"] = result["answers"].clone();
            input["answer_ids"] = serde_json::json!([1]);
            input["evaluations"] =
                serde_json::json!([{"answer_id":1,"rating":rating,"note":"Regrade"}]);
        }
        let corrected =
            grill_jq("Feedback/update", input.clone()).expect("latest grade changes feedback");
        assert_eq!(corrected["direction"], "escalate");
        assert_eq!(corrected["answers"][0]["rating"], "Solid");
        assert_eq!(corrected["answers"][0]["failed"], true);
        input["answers"] = corrected["answers"].clone();
        for (id, round) in [(3, 2), (4, 3)] {
            input["answers"]
                .as_array_mut()
                .expect("ledger")
                .push(serde_json::json!({
                    "id":id,"question_id":11,"text":"same text, distinct submission","round":round,
                    "rating":null,"failed":false,"note":null
                }));
            input["round"] = serde_json::json!(round);
            input["answer_ids"] = serde_json::json!([id]);
            input["evaluations"] =
                serde_json::json!([{"answer_id":id,"rating":"Gap","note":"New attempt"}]);
            let result = grill_jq("Feedback/update", input.clone()).expect("distinct attempt");
            input["answers"] = result["answers"].clone();
            assert_eq!(result["attempt_counts"][0][1], id - 1);
            assert_eq!(
                result["retired_ids"],
                if id == 4 {
                    serde_json::json!([11])
                } else {
                    serde_json::json!([])
                }
            );
        }
    }

    #[test]
    fn grill_feedback_preserves_boundaries_and_rejects_bad_batches() {
        for (solid, level, round, direction) in [
            (4, 2, 1, "escalate"),
            (3, 2, 1, "hold"),
            (2, 2, 1, "hold"),
            (1, 2, 1, "reprobe"),
            (4, 5, 1, "complete"),
            (0, 2, 5, "complete"),
        ] {
            let mut input = grill_input();
            input["level"] = serde_json::json!(level);
            input["round"] = serde_json::json!(round);
            input["answers"] = serde_json::json!((1..=5).map(|id| serde_json::json!({
                "id":id,"question_id":11,"text":"Fixture","round":round,"rating":null,"failed":false,"note":null
            })).collect::<Vec<_>>());
            input["answer_ids"] = serde_json::json!([1, 2, 3, 4, 5]);
            input["evaluations"] =
                serde_json::json!((1..=5).map(|id| serde_json::json!({
                "answer_id":id,"rating":if id<=solid {"Solid"} else {"Gap"},"note":"Fixture"
            })).collect::<Vec<_>>());
            assert_eq!(
                grill_jq("Feedback/update", input).expect("threshold")["direction"],
                direction
            );
        }
        let input = grill_input();
        assert_eq!(
            grill_jq("Feedback/update", input.clone()).expect("pending")["pending_ids"],
            serde_json::json!([1, 2])
        );
        for (key, value) in [
            ("level", serde_json::json!(0)),
            ("round", serde_json::json!(6)),
            ("round", serde_json::json!(1.5)),
            ("answer_ids", serde_json::json!([1])),
        ] {
            let mut bad = input.clone();
            bad[key] = value;
            assert!(grill_jq("Feedback/update", bad).is_err(), "bad {key}");
        }
        let mut unknown = input.clone();
        unknown["answer_ids"] = serde_json::json!([99]);
        unknown["evaluations"] =
            serde_json::json!([{"answer_id":99,"rating":"Gap","note":"Unknown"}]);
        assert!(
            grill_jq("Feedback/update", unknown).is_err(),
            "matching batch IDs must still reference an actual answer"
        );
        let mut empty = input;
        empty["answers"] = serde_json::json!([]);
        empty["round"] = serde_json::json!(5);
        let end = grill_jq("Feedback/update", empty).expect("empty fifth batch");
        assert_eq!(end["direction"], "hold");
        assert_eq!(end["stop"], true);
    }

    #[test]
    fn grill_delivery_preserves_retries_explanations_and_final_evidence() {
        let session = grill_input();
        let mut delivery = serde_json::json!({"question_bank":session["question_bank"],"retired_ids":[11],"level":2,
            "drafts":[{"retry_id":12,"question":null,"area":null},{"retry_id":null,"question":"New question","area":"math"}],
            "explanations":[{"question_id":11,"explanation":"Correct answer"}]});
        let questions = grill_jq("Question delivery", delivery.clone()).expect("legal retry");
        assert_eq!(questions[0], session["question_bank"][1]);
        assert_eq!(questions[1]["id"], 14);
        delivery["drafts"][0]["retry_id"] = serde_json::json!(11);
        assert!(
            grill_jq("Question delivery", delivery.clone()).is_err(),
            "retired retry"
        );
        delivery["drafts"][0]["retry_id"] = serde_json::json!(12);
        delivery["explanations"] = serde_json::json!([]);
        assert!(
            grill_jq("Question delivery", delivery).is_err(),
            "missing retired explanation"
        );
        let mut final_input = serde_json::json!({"question_bank":session["question_bank"],"answers":session["answers"],
            "summary":[{"area":"math","rating":"Unassessed","answer_ids":[]},
                       {"area":"evidence","rating":"Unassessed","answer_ids":[]}]});
        assert_eq!(
            grill_jq("Final evidence", final_input.clone()).expect("unanswered areas"),
            true
        );
        final_input["summary"][0] =
            serde_json::json!({"area":"math","rating":"Gap","answer_ids":[1]});
        assert!(
            grill_jq("Final evidence", final_input.clone()).is_err(),
            "ungraded cannot support Gap"
        );
        final_input["answers"][0]["rating"] = serde_json::json!("Gap");
        assert_eq!(
            grill_jq("Final evidence", final_input.clone()).expect("actual evidence"),
            true
        );
        final_input["summary"][0]["answer_ids"] = serde_json::json!([99]);
        assert!(
            grill_jq("Final evidence", final_input).is_err(),
            "invented evidence"
        );
    }

    #[test]
    fn grill_round_separates_identified_grading_from_next_questions() {
        let session = grill_input();
        let mut context = serde_json::json!({"topic":"fixture","phase":"grade","mode":"interrogate",
            "current_level":2,"round_number":1,"question_bank":session["question_bank"],
            "answer_ids":[1,2],"feedback":{"answers":session["answers"],"direction":"await_grades","retired_ids":[],"stop":false}});
        let grade = render_template_for_test("grill-me/grill-me-round", context.clone())
            .expect("grade render");
        assert!(grade.contains("QUESTION_SENTINEL") && grade.contains("ANSWER_SENTINEL"));
        assert!(
            grade.contains("## Grade Identified Answers Only")
                && !grade.contains("## Draft Next Questions Only")
        );
        context["phase"] = serde_json::json!("ask");
        context["answer_ids"] = serde_json::json!([]);
        for (direction, stop, allowed) in [
            ("escalate", false, true),
            ("await_grades", false, false),
            ("complete", true, false),
        ] {
            context["feedback"] = serde_json::json!({"answers":[],"direction":direction,"retired_ids":[11],"stop":stop});
            let output = render_template_for_test("grill-me/grill-me-round", context.clone())
                .expect("ask render");
            assert_eq!(output.contains("## Draft Next Questions Only"), allowed);
            if allowed {
                assert!(
                    output.contains("retired_explanations") && !output.contains("\"assessment\":")
                );
            }
        }
    }

    #[test]
    fn grill_assessment_preserves_answer_evidence_and_absent_calibration() {
        let session = grill_input();
        let output = render_template_for_test("grill-me/grill-me-assess", serde_json::json!({
            "topic":"fixture","question_bank":session["question_bank"],"answers":session["answers"],"calibrated_level":null
        })).expect("final render");
        for text in [
            "QUESTION_SENTINEL",
            "ANSWER_SENTINEL",
            "Unassessed",
            "answer_ids",
        ] {
            assert!(output.contains(text));
        }
        assert!(!output.contains("solid|partial|gap"));
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
            serde_json::json!({
                "verdict": "rewrite_needed",
                "ratings": []
            }),
        )]);

        let error = validate_contract_inputs(input, &context).expect_err("missing flat inputs");

        assert!(error.contains("grill_ratings"), "got: {error}");
        assert!(error.contains("grill_verdict"), "got: {error}");

        let present_but_blank = std::collections::HashMap::from([
            ("grill_ratings".to_string(), serde_json::json!([])),
            ("grill_verdict".to_string(), serde_json::json!("")),
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
                serde_json::json!({"statement": "expected"}),
            ),
            (
                "observation".to_string(),
                serde_json::json!({"statement": "observed"}),
            ),
            ("probe_results".to_string(), serde_json::json!([])),
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
            context.insert("active_goal".to_string(), invalid_goal);
            let error = validate_contract_inputs(&template, &context)
                .expect_err("an inquiry with a blank or non-string goal cannot render");
            assert!(error.contains("active_goal"), "got: {error}");
            assert!(error.contains("non-blank"), "got: {error}");
        }

        context.insert(
            "active_goal".to_string(),
            serde_json::json!("Investigate meaningful surprises"),
        );
        validate_contract_inputs(&template, &context).expect("goal-carrying inquiry admits");
    }

    #[test]
    fn contract_validation_allows_optional_and_defaulted_inputs() {
        let input = "[inference]\ncontract:\n  input:\n    required_value: string\n    nullable_value: string|null\n    defaulted_value:\n      type: string\n      default: fallback\n    explicitly_optional:\n      type: string\n      required: false\n---\n{{ required_value }}";
        let context = std::collections::HashMap::from([(
            "required_value".to_string(),
            serde_json::json!("present"),
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

    #[test]
    fn test_context_decodes_json_text() {
        let input =
            serde_json::json!({"template_ref": "essentialist/essentialist-flow", "context": "{}"});
        let result: RenderTemplateToolInput =
            serde_json::from_value(input).expect("stringified context must be accepted");
        assert!(
            result.context.is_empty(),
            "stringified empty object must parse to empty map"
        );
    }

    #[test]
    fn context_json_text_preserves_nested_values_and_required_input_validation() {
        let variables = serde_json::json!({
            "task": "simplify", "items": [1, 2], "literal": "{\"not\":\"an object\"}"
        });
        let input: RenderTemplateToolInput = serde_json::from_value(serde_json::json!({
            "template_ref": "essentialist/essentialist-flow",
            "context": variables.to_string()
        }))
        .expect("JSON-text context");
        assert_eq!(input.context["items"], serde_json::json!([1, 2]));
        assert!(input.context["literal"].is_string());
        let wire = serde_json::to_value(&input).expect("native input serialization");
        assert!(wire["context"].is_string());
        let required = "[inference]\ncontract:\n  input:\n    missing: string\n---\nbody";
        assert!(validate_contract_inputs(required, &input.context).is_err());
        for invalid in [
            serde_json::json!({}),
            serde_json::json!("{\"cut\":"),
            serde_json::json!("[]"),
        ] {
            let error = serde_json::from_value::<RenderTemplateToolInput>(serde_json::json!({
                "template_ref": "probe", "context": invalid
            }))
            .expect_err("invalid context must be rejected");
            assert!(error.to_string().contains("context"));
        }
    }

    #[test]
    fn context_schema_advertises_json_text() {
        let schema =
            serde_json::to_value(schemars::schema_for!(RenderTemplateToolInput)).expect("schema");
        assert_eq!(schema["properties"]["context"]["type"], "string");
        assert!(
            schema["properties"]["context"]
                .get("additionalProperties")
                .is_none()
        );
        assert!(hkask_types::find_boolean_schema_positions(&schema).is_empty());
    }

    // Exercise the shared normalized wire schema, not only raw schema generation.
    #[test]
    fn context_and_env_advertise_the_same_json_text_wire_schema() {
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
                Some(&serde_json::json!("string")),
                "{tool_name}.{field} must carry JSON text on the wire"
            );
            assert!(field_schema.get("properties").is_none());
            assert!(field_schema.get("additionalProperties").is_none());
            if let Some(default) = field_schema.get("default") {
                assert!(
                    default.is_string(),
                    "{tool_name}.{field} default must match its wire type"
                );
                let decoded: serde_json::Value =
                    serde_json::from_str(default.as_str().expect("checked JSON-text default"))
                        .expect("default is valid JSON text");
                assert!(decoded.is_object());
            }
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

#[cfg(test)]
mod finalizer_handoff_tests {
    use super::tests::render_template_for_test;
    use serde_json::{Value, json};

    fn critic_context(findings: Value) -> Value {
        json!({
            "enhanced_prompt": "Preserve the supplied evidence.",
            "prompt_type": "agent-task",
            "effort_tier": "medium",
            "output_format_resolved": "inline",
            "output_path_resolved": "handoff-fixture.md",
            "audit_findings": [],
            "grill_ratings": [],
            "grill_findings": findings,
            "grill_verdict": "rewrite_needed",
            "mutations_applied": [],
            "mutations_deferred": [],
            "unfold_mode_resolved": "rewrite_only",
            "reference_models": [],
            "unfolded_plan": []
        })
    }

    fn section_json(rendered: &str, heading: &str) -> Value {
        let section = rendered.split_once(heading).expect("handoff section").1;
        let block = section.split_once("```json\n").expect("JSON block").1;
        let payload = block.split_once("\n```").expect("closed JSON block").0;
        serde_json::from_str(payload).expect("handoff data remains valid JSON")
    }

    #[test]
    fn prompt_enhance_preserves_critic_findings_at_delivery() {
        let findings = json!([{
            "area": "Mechanism",
            "gap": "Keep this exact critic finding, including \"quotes\".\nSecond line.",
            "severity": "Guardrail",
            "suggested_fix_direction": "Do not reconstruct the finding from its rating.",
            "evidence": {"source": "independent fixture"}
        }]);
        for verdict in ["rewrite_needed", "fail", "pass"] {
            let mut context = critic_context(findings.clone());
            context["grill_verdict"] = json!(verdict);
            let rendered =
                render_template_for_test("prompt-enhance/enhance-output-render", context)
                    .expect("complete finalizer context renders");
            let result: Value = serde_json::from_str(&rendered).expect("structured delivery");
            assert_eq!(result["change_log"]["grill_findings"], findings);
            assert_eq!(result["change_log"]["grill_verdict"], verdict);
            assert_eq!(
                section_json(
                    result["delivered_output"].as_str().expect("human delivery"),
                    "### Specific critic findings",
                ),
                findings,
            );
        }

        for mode in ["inline", "file", "both"] {
            let mut context = critic_context(json!([]));
            context["effort_tier"] = json!("low");
            context["grill_verdict"] = json!("skipped");
            context["output_format_resolved"] = json!(mode);
            let rendered =
                render_template_for_test("prompt-enhance/enhance-output-render", context)
                    .expect("explicit empty findings render in every delivery mode");
            let result: Value = serde_json::from_str(&rendered).expect("structured empty delivery");
            assert_eq!(result["change_log"]["grill_findings"], json!([]));
            assert_eq!(result["change_log"]["grill_verdict"], "skipped");
            assert_eq!(result["change_log"]["input_errors"], json!([]));
            assert_eq!(result["output_path_written"], "");
        }

        let mut missing = critic_context(json!([]));
        missing
            .as_object_mut()
            .expect("context object")
            .remove("grill_findings");
        let error = render_template_for_test("prompt-enhance/enhance-output-render", missing)
            .expect_err("omitted critic handoff must not look like an empty review");
        assert!(error.contains("grill_findings"), "{error}");

        for malformed in [json!("[]"), json!({}), json!(7), Value::Null] {
            let rendered = render_template_for_test(
                "prompt-enhance/enhance-output-render",
                critic_context(malformed),
            )
            .expect("invalid handoff shape is surfaced in the delivery");
            let result: Value =
                serde_json::from_str(&rendered).expect("error delivery is valid JSON");
            assert_eq!(result["change_log"]["grill_findings"], json!([]));
            assert!(
                result["change_log"]["input_errors"]
                    .as_array()
                    .expect("errors")
                    .iter()
                    .any(|error| error
                        .as_str()
                        .is_some_and(|text| text.contains("grill_findings")))
            );
            assert!(
                result["delivered_output"]
                    .as_str()
                    .expect("delivery")
                    .contains("INPUT ERROR")
            );
        }
    }

    fn planner_context(questions: Value) -> Value {
        json!({
            "context_summary": "Question handoff fixture, not an approved implementation plan.",
            "target_condition": "Retain the operator's supplied questions at finalization.",
            "dependency_graph": [],
            "tasks": [],
            "slices": [],
            "risks": [],
            "open_questions": questions,
            "gate_result": {"fixture_only": true},
            "refinement_directives": [],
            "phases": [],
            "checkpoints": []
        })
    }

    #[test]
    fn task_breakdown_preserves_open_questions_at_finalization() {
        let questions = json!([
            "Which source license permits redistribution?",
            "Retain this \"quoted\" question.\nAnd its second line.",
            {"question": "Does the operator accept the source limitation?", "source": "operator"}
        ]);
        for supplied in [questions, json!([])] {
            let rendered = render_template_for_test(
                "task-breakdown/task-breakdown-write-plan",
                planner_context(supplied.clone()),
            )
            .expect("an explicit question array renders");
            assert_eq!(
                section_json(&rendered, "## Open Questions from Plan"),
                supplied
            );
        }

        let mut missing = planner_context(json!([]));
        missing
            .as_object_mut()
            .expect("context object")
            .remove("open_questions");
        let error = render_template_for_test("task-breakdown/task-breakdown-write-plan", missing)
            .expect_err("omitted questions must not look like an empty plan stage");
        assert!(error.contains("open_questions"), "{error}");

        for malformed in [json!("[]"), json!({}), json!(7), Value::Null] {
            let rendered = render_template_for_test(
                "task-breakdown/task-breakdown-write-plan",
                planner_context(malformed),
            )
            .expect("invalid question shape is surfaced");
            assert!(rendered.contains("INPUT ERROR") && rendered.contains("open_questions"));
            assert!(!rendered.contains("You are a plan-finalization agent"));
            assert!(!rendered.contains("Full markdown content of plan.md"));
        }
    }

    #[test]
    fn task_count_facts_drive_the_critic_directive_mandate() {
        let context = |count: usize, penalty: f64| {
            json!({
                "tasks": vec![json!({"id": "fixture"}); count],
                "slices": [], "dependency_graph": [],
                "context_summary": "Count-mandate rendering fixture, not a graded plan.",
                "target_condition": "Preserve one source of truth for task-count adjustment.",
                "phases": [], "checkpoints": [],
                "task_count": count, "task_count_penalty": penalty
            })
        };
        for (count, penalty, mandated) in [(2, 0.1, true), (3, 0.0, false)] {
            let rendered = render_template_for_test(
                "task-breakdown/task-breakdown-quality-gate",
                context(count, penalty),
            )
            .expect("precomputed facts reach the real critic template");
            assert_eq!(
                section_json(&rendered, "### Task-count awareness"),
                json!({"task_count": count, "task_count_penalty": penalty}),
            );
            assert_eq!(
                rendered.contains("Required count-induced refinement"),
                mandated
            );
            assert!(rendered.contains("\"base_gate_scores\""));
            assert!(!rendered.contains("\"gate_scores\": {"));
        }
        for missing in ["task_count", "task_count_penalty"] {
            let mut incomplete = context(2, 0.1);
            incomplete
                .as_object_mut()
                .expect("context object")
                .remove(missing);
            let error =
                render_template_for_test("task-breakdown/task-breakdown-quality-gate", incomplete)
                    .expect_err("missing deterministic facts must not be fabricated by the critic");
            assert!(error.contains(missing), "{error}");
        }
    }
}
