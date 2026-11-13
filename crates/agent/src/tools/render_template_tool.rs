use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput, deserialize_teaching_field};
use agent_client_protocol::v2::schema as acp;
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
    /// Optional path to a JSON file holding the context object. Use this when
    /// the context exceeds what fits in a single tool-call argument: write the
    /// object to a file and pass its path instead of inlining the JSON text.
    /// The file must contain a single JSON object document. When both this and
    /// `context` are supplied, the inline `context` is merged over the file's
    /// object (inline values win). Bounded by `MAX_CONTEXT_FILE_BYTES`.
    ///
    /// Anchoring: the model's single-response output budget is the binding
    /// limit, so a large context cannot cross the model boundary as one
    /// argument — a truncated inline `context` fails with
    /// `expected complete JSON text: EOF while parsing ...` and is never
    /// retried (`ToolCallTruncated` is not retried by design). The file
    /// reference is the same pattern the corpus review uses for large
    /// payloads (`.agents/skills/mcp-tool-review/SKILL.md`: write in chunks,
    /// merge, validate — no truncation risk), applied at the tool boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_path: Option<String>,
}

/// Upper bound on a context file loaded through `context_path`, so a
/// mis-specified path cannot pull an unbounded file into the render.
const MAX_CONTEXT_FILE_BYTES: usize = 1024 * 1024;

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

    // A large context crosses the model boundary as a file reference rather
    // than one tool-call argument: load it, then let inline values win.
    let mut context = input.context.clone();
    if let Some(path) = &input.context_path {
        let file_context = load_context_file(path)?;
        for (key, value) in file_context {
            context.entry(key).or_insert(value);
        }
    }

    // Resolve the template file, trying ref as-is, then .j2.
    let content = read_template_file(base_path, &input.template_ref)?;

    validate_contract_inputs(&content, &context).map_err(|error| {
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
    env.render_str(&template_body, &context)
        .map_err(|e| format!("Template rendering failed: {e}"))
}

/// Load a context object from a JSON file (the `context_path` contract).
///
/// The file must hold one JSON object document; anything else is rejected
/// before rendering. The read is size-bounded so a mis-specified path cannot
/// pull an unbounded file into the render.
fn load_context_file(
    path: &str,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("context_path '{path}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("context_path '{path}' is not a regular file"));
    }
    if metadata.len() > MAX_CONTEXT_FILE_BYTES as u64 {
        return Err(format!(
            "context_path '{path}' is {} bytes, over the {} byte bound",
            metadata.len(),
            MAX_CONTEXT_FILE_BYTES
        ));
    }
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("context_path '{path}': {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("context_path '{path}': {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("context_path '{path}' must contain a JSON object document"))?;
    Ok(object.clone().into_iter().collect())
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
                {"id":1,"question_id":11,"text":"ANSWER_SENTINEL","evidence":[],"round":1,"rating":null,"failed":false,"note":null},
                {"id":2,"question_id":12,"text":"After it exists","evidence":[],"round":1,"rating":null,"failed":false,"note":null}
            ],
            "new_answers":[],"answer_ids":[],"evaluations":[],"level":2,"round":1,"mode":"interrogate",
            "calibrated_level":null,"respondent":"human","regrades_used":0
        })
    }

    // Apply one update and carry the returned ledger and regrade count forward,
    // as the skill's instructions do.
    fn grill_step(input: &mut serde_json::Value) -> Result<serde_json::Value, String> {
        let result = grill_jq("Feedback/update", input.clone())?;
        input["answers"] = result["answers"].clone();
        input["regrades_used"] = result["regrades_used"].clone();
        Ok(result)
    }

    #[test]
    fn grill_feedback_counts_distinct_answers_not_regrades() {
        let mut input = grill_input();
        input["answer_ids"] = serde_json::json!([2, 1]);
        input["evaluations"] = serde_json::json!([
            {"answer_id":2,"rating":"Solid","note":"Correct"},
            {"answer_id":1,"rating":"Gap","note":"Incorrect"}
        ]);
        let first = grill_step(&mut input).expect("first grading");
        assert_eq!(first["regrades_used"], 0, "a first grade is not a regrade");
        assert_eq!(first["direction"], "hold");
        input["answer_ids"] = serde_json::json!([1]);
        for (rating, used) in [("Gap", 1), ("Solid", 2)] {
            input["evaluations"] =
                serde_json::json!([{"answer_id":1,"rating":rating,"note":"Regrade"}]);
            let result = grill_step(&mut input).expect("regrade within the bound");
            assert_eq!(result["regrades_used"], used);
            assert_eq!(result["retired_ids"], serde_json::json!([]));
            assert_eq!(result["answers"].as_array().expect("ledger").len(), 2);
        }
        // The latest grade decides the direction; the earlier Gap stays on record.
        let corrected = grill_jq("Feedback/update", {
            let mut query = input.clone();
            query["answer_ids"] = serde_json::json!([]);
            query["evaluations"] = serde_json::json!([]);
            query
        })
        .expect("query after regrades");
        assert_eq!(corrected["direction"], "escalate");
        assert_eq!(corrected["answers"][0]["rating"], "Solid");
        assert_eq!(corrected["answers"][0]["failed"], true);
        assert_eq!(corrected["regrades_used"], 2, "a query is not a regrade");
        let refused = grill_jq("Feedback/update", input.clone()).expect_err("third regrade");
        assert!(refused.contains("regrade bound exceeded"), "{refused}");
        // Two more distinct submissions to the same question, each graded Gap:
        // the third failed answer retires the question, and neither is a regrade.
        for (id, round) in [(3, 2), (4, 3)] {
            input["round"] = serde_json::json!(round);
            input["answer_ids"] = serde_json::json!([]);
            input["evaluations"] = serde_json::json!([]);
            input["new_answers"] = serde_json::json!([
                {"question_id":11,"text":"same text, distinct submission","evidence":[]}
            ]);
            let appended = grill_step(&mut input).expect("append");
            assert_eq!(appended["pending_ids"], serde_json::json!([id]));
            assert_eq!(appended["direction"], "await_grades");
            assert_eq!(
                appended["next_round"], round,
                "an ungraded batch does not advance"
            );
            input["new_answers"] = serde_json::json!([]);
            input["answer_ids"] = serde_json::json!([id]);
            input["evaluations"] =
                serde_json::json!([{"answer_id":id,"rating":"Gap","note":"New attempt"}]);
            let result = grill_step(&mut input).expect("distinct attempt");
            assert_eq!(result["regrades_used"], 2);
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
    fn grill_update_appends_answers_and_requires_agent_evidence() {
        let mut input = grill_input();
        input["answers"] = serde_json::json!([
            {"id":7,"question_id":11,"text":"Earlier","evidence":["src/a.rs:3"],"round":1,"rating":"Solid","failed":false,"note":"ok"}
        ]);
        input["round"] = serde_json::json!(2);
        input["respondent"] = serde_json::json!("agent");
        input["new_answers"] = serde_json::json!([
            {"question_id":12,"text":"SECOND","evidence":["src/b.rs:9"]},
            {"question_id":11,"text":"THIRD","evidence":["tool:grep#2","src/c.rs:1"]}
        ]);
        let appended = grill_jq("Feedback/update", input.clone()).expect("append");
        // IDs follow the ledger's maximum, not its length, and rows keep their evidence.
        assert_eq!(
            appended["answers"],
            serde_json::json!([
                {"id":7,"question_id":11,"text":"Earlier","evidence":["src/a.rs:3"],"round":1,"rating":"Solid","failed":false,"note":"ok"},
                {"id":8,"question_id":12,"text":"SECOND","evidence":["src/b.rs:9"],"round":2,"rating":null,"failed":false,"note":null},
                {"id":9,"question_id":11,"text":"THIRD","evidence":["tool:grep#2","src/c.rs:1"],"round":2,"rating":null,"failed":false,"note":null}
            ])
        );
        assert_eq!(appended["pending_ids"], serde_json::json!([8, 9]));

        let mut unsupported = input.clone();
        unsupported["new_answers"][1]["evidence"] = serde_json::json!([]);
        assert!(
            grill_jq("Feedback/update", unsupported.clone()).is_err(),
            "an agent answer without evidence is refused"
        );
        unsupported["respondent"] = serde_json::json!("human");
        assert!(
            grill_jq("Feedback/update", unsupported).is_ok(),
            "a human answer needs no locator"
        );
        for (label, patch) in [
            (
                "blank locator",
                serde_json::json!([{"question_id":12,"text":"x","evidence":[""]}]),
            ),
            (
                "unknown question",
                serde_json::json!([{"question_id":99,"text":"x","evidence":["a:1"]}]),
            ),
            (
                "empty text",
                serde_json::json!([{"question_id":12,"text":"","evidence":["a:1"]}]),
            ),
            (
                "same question twice in one call",
                serde_json::json!([
                    {"question_id":12,"text":"x","evidence":["a:1"]},
                    {"question_id":12,"text":"y","evidence":["a:2"]}
                ]),
            ),
        ] {
            let mut bad = input.clone();
            bad["new_answers"] = patch;
            assert!(grill_jq("Feedback/update", bad).is_err(), "{label}");
        }
        // A second answer to a question already answered in this round is refused.
        let mut repeat = input.clone();
        repeat["answers"] = appended["answers"].clone();
        repeat["new_answers"] =
            serde_json::json!([{"question_id":12,"text":"again","evidence":["a:1"]}]);
        assert!(
            grill_jq("Feedback/update", repeat).is_err(),
            "second answer in the round"
        );
        // One call appends or grades, never both.
        let mut both = input.clone();
        both["answer_ids"] = serde_json::json!([7]);
        both["evaluations"] = serde_json::json!([{"answer_id":7,"rating":"Gap","note":"n"}]);
        assert!(
            grill_jq("Feedback/update", both).is_err(),
            "append and grade together"
        );
        let mut stranger = input;
        stranger["respondent"] = serde_json::json!("committee");
        assert!(
            grill_jq("Feedback/update", stranger).is_err(),
            "unknown respondent"
        );
    }

    #[test]
    fn grill_feedback_preserves_boundaries_and_rejects_bad_batches() {
        // (solid of five, level, round) -> direction, and the level, round and stop
        // the caller is told to use next.
        for (solid, level, round, direction, next_level, next_round, stop) in [
            (4, 2, 1, "escalate", 3, 2, false),
            (3, 2, 1, "hold", 2, 2, false),
            (2, 2, 1, "hold", 2, 2, false),
            (1, 2, 1, "reprobe", 2, 2, false),
            (4, 5, 1, "complete", 5, 1, true),
            (0, 2, 5, "complete", 2, 5, true),
        ] {
            let mut input = grill_input();
            input["level"] = serde_json::json!(level);
            input["round"] = serde_json::json!(round);
            input["answers"] = serde_json::json!((1..=5).map(|id| serde_json::json!({
                "id":id,"question_id":11,"text":"Fixture","evidence":[],"round":round,"rating":null,"failed":false,"note":null
            })).collect::<Vec<_>>());
            input["answer_ids"] = serde_json::json!([1, 2, 3, 4, 5]);
            input["evaluations"] =
                serde_json::json!((1..=5).map(|id| serde_json::json!({
                "answer_id":id,"rating":if id<=solid {"Solid"} else {"Gap"},"note":"Fixture"
            })).collect::<Vec<_>>());
            let result = grill_jq("Feedback/update", input).expect("threshold");
            assert_eq!(result["direction"], direction, "{solid}/5 at {level}");
            assert_eq!(result["next_level"], next_level, "{solid}/5 at {level}");
            assert_eq!(result["next_round"], next_round, "{solid}/5 round {round}");
            assert_eq!(result["next_mode"], "interrogate");
            assert_eq!(result["stop"], stop, "{solid}/5 round {round}");
        }
        // Grading the calibration batch seeds the level and switches the mode once.
        let mut calibration = grill_input();
        calibration["mode"] = serde_json::json!("calibrate");
        calibration["level"] = serde_json::json!(1);
        calibration["calibrated_level"] = serde_json::json!(3);
        calibration["answer_ids"] = serde_json::json!([1, 2]);
        calibration["evaluations"] = serde_json::json!([
            {"answer_id":1,"rating":"Solid","note":"n"},{"answer_id":2,"rating":"Solid","note":"n"}
        ]);
        let seeded = grill_jq("Feedback/update", calibration).expect("calibration");
        assert_eq!(
            (
                &seeded["direction"],
                &seeded["next_level"],
                &seeded["next_round"],
                &seeded["next_mode"]
            ),
            (
                &serde_json::json!("hold"),
                &serde_json::json!(3),
                &serde_json::json!(2),
                &serde_json::json!("interrogate")
            )
        );
        let input = grill_input();
        let pending = grill_jq("Feedback/update", input.clone()).expect("pending");
        assert_eq!(pending["pending_ids"], serde_json::json!([1, 2]));
        assert_eq!(pending["direction"], "await_grades");
        assert_eq!(
            pending["next_round"], 1,
            "an ungraded batch does not advance"
        );
        // The fifth batch is graded before it is assessed: stop waits for the grades.
        let mut last = input.clone();
        last["round"] = serde_json::json!(5);
        last["answers"][0]["round"] = serde_json::json!(5);
        let last = grill_jq("Feedback/update", last).expect("ungraded fifth batch");
        assert_eq!(last["direction"], "await_grades");
        assert_eq!(last["stop"], false);
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
        let mut context = serde_json::json!({"topic":"fixture","respondent":"human","phase":"grade","mode":"interrogate",
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
    fn grill_round_tells_an_examiner_of_an_agent_to_grade_the_evidence() {
        let session = grill_input();
        let mut answers = session["answers"].clone();
        answers[0]["evidence"] = serde_json::json!(["LOCATOR_SENTINEL:12"]);
        let mut context = serde_json::json!({"topic":"fixture","respondent":"agent","phase":"grade","mode":"interrogate",
            "current_level":2,"round_number":1,"question_bank":session["question_bank"],
            "answer_ids":[1],"feedback":{"answers":answers,"direction":"await_grades","retired_ids":[],"stop":false}});
        let agent_grade = render_template_for_test("grill-me/grill-me-round", context.clone())
            .expect("agent grade render");
        // The selected answer reaches the grader with its locator; the unselected one does not.
        assert!(
            agent_grade.contains("LOCATOR_SENTINEL:12") && !agent_grade.contains("After it exists")
        );
        assert!(agent_grade.contains("You are the\nexaminer"));
        assert!(agent_grade.contains("does not show what it claims is Gap"));
        context["phase"] = serde_json::json!("ask");
        context["answer_ids"] = serde_json::json!([]);
        context["feedback"]["direction"] = serde_json::json!("hold");
        let agent_ask = render_template_for_test("grill-me/grill-me-round", context.clone())
            .expect("agent ask render");
        assert!(agent_ask.contains("answerable with\na locator"));
        context["respondent"] = serde_json::json!("human");
        for phase in ["ask", "grade"] {
            context["phase"] = serde_json::json!(phase);
            let human = render_template_for_test("grill-me/grill-me-round", context.clone())
                .expect("human render");
            assert!(
                !human.contains("examiner") && !human.contains("locator"),
                "a human respondent gets no agent instructions in {phase}"
            );
        }
        context["respondent"] = serde_json::json!("committee");
        let unknown = render_template_for_test("grill-me/grill-me-round", context)
            .expect("unknown respondent render");
        assert!(
            unknown.contains("invalid_phase_mode_or_respondent")
                && !unknown.contains("## Draft Next Questions Only")
                && !unknown.contains("## Grade Identified Answers Only")
        );
    }

    #[test]
    fn grill_assessment_skeleton_groups_graded_answers_by_area() {
        let mut session = grill_input();
        session["question_bank"]
            .as_array_mut()
            .expect("bank")
            .push(serde_json::json!({"id":13,"question":"Third","level":3,"area":"math"}));
        session["answers"] = serde_json::json!([
            {"id":1,"question_id":11,"text":"a","evidence":[],"round":1,"rating":"Gap","failed":true,"note":"n"},
            {"id":2,"question_id":12,"text":"b","evidence":[],"round":1,"rating":null,"failed":false,"note":null},
            {"id":3,"question_id":13,"text":"c","evidence":[],"round":2,"rating":"Solid","failed":false,"note":"n"}
        ]);
        let skeleton = grill_jq(
            "Assessment skeleton",
            serde_json::json!({"question_bank":session["question_bank"],"answers":session["answers"]}),
        )
        .expect("skeleton");
        // An answered-but-ungraded area is not assessable; areas come out sorted.
        assert_eq!(
            skeleton,
            serde_json::json!([
                {"area":"evidence","question_ids":[12],"graded_answer_ids":[],"latest_ratings":[],"assessable":false},
                {"area":"math","question_ids":[11,13],"graded_answer_ids":[1,3],"latest_ratings":["Gap","Solid"],"assessable":true}
            ])
        );
        // A summary that follows the skeleton passes the delivery check; one that
        // rates the unassessable area does not.
        let mut final_input = serde_json::json!({"question_bank":session["question_bank"],"answers":session["answers"],
            "summary":[{"area":"evidence","rating":"Unassessed","answer_ids":[]},
                       {"area":"math","rating":"Partial","answer_ids":[1,3]}]});
        assert_eq!(
            grill_jq("Final evidence", final_input.clone()).expect("skeleton-shaped summary"),
            true
        );
        final_input["summary"][0] =
            serde_json::json!({"area":"evidence","rating":"Gap","answer_ids":[2]});
        assert!(grill_jq("Final evidence", final_input).is_err());
    }

    #[test]
    fn grill_assessment_preserves_answer_evidence_and_absent_calibration() {
        let session = grill_input();
        let areas = serde_json::json!([
            {"area":"AREA_SENTINEL","question_ids":[11],"graded_answer_ids":[],"latest_ratings":[],"assessable":false}
        ]);
        let mut context = serde_json::json!({
            "topic":"fixture","respondent":"human","areas":areas,"question_bank":session["question_bank"],
            "answers":session["answers"],"calibrated_level":null
        });
        let output = render_template_for_test("grill-me/grill-me-assess", context.clone())
            .expect("final render");
        for text in [
            "QUESTION_SENTINEL",
            "ANSWER_SENTINEL",
            "AREA_SENTINEL",
            "Unassessed",
            "answer_ids",
        ] {
            assert!(output.contains(text), "{text}");
        }
        assert!(!output.contains("solid|partial|gap"));
        assert!(!output.contains("not study advice"));
        context["respondent"] = serde_json::json!("agent");
        let agent = render_template_for_test("grill-me/grill-me-assess", context.clone())
            .expect("agent final render");
        assert!(agent.contains("not study advice"));
        context.as_object_mut().expect("context").remove("areas");
        assert!(
            render_template_for_test("grill-me/grill-me-assess", context)
                .expect_err("the skeleton is required")
                .contains("areas")
        );
    }

    // One cargo `--message-format=json` record, as clippy prints it.
    fn cargo_message(level: &str, code: Option<&str>, message: &str, line: u32) -> String {
        serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": level, "message": message,
                "code": code.map(|code| serde_json::json!({"code": code})),
                "spans": [{"file_name": "src/lib.rs", "line_start": line, "is_primary": true}]
            }
        })
        .to_string()
    }

    fn cargo_capture(messages: &[String], success: bool) -> String {
        let mut lines = vec!["sccache enabled: /not/json".to_string()];
        lines.extend(messages.iter().cloned());
        lines.push(serde_json::json!({"reason": "build-finished", "success": success}).to_string());
        lines.join("\n") + "\n"
    }

    fn compiler_facts(
        baseline: &str,
        proposed: &str,
        unsafe_grep: &str,
        miri: &str,
        record: &str,
    ) -> Result<serde_json::Value, String> {
        authored_jq(
            "idiomatic-rust",
            "Compiler facts",
            serde_json::json!({"baseline": baseline, "proposed": proposed,
                "unsafe_grep": unsafe_grep, "miri": miri, "record": record}),
        )
    }

    #[test]
    fn idiomatic_rust_compiler_facts_are_measured_from_the_captures() {
        let ptr_arg = cargo_message("error", Some("clippy::ptr_arg"), "writing `&Vec`", 1);
        let ptr_arg_moved = cargo_message("error", Some("clippy::ptr_arg"), "writing `&Vec`", 9);
        let unused = cargo_message("warning", Some("unused_variables"), "unused variable", 2);
        let mismatch = cargo_message("error", Some("E0308"), "mismatched types", 4);
        let syntax = cargo_message("error", None, "mismatched closing delimiter", 3);
        let note = serde_json::json!({"reason": "compiler-message",
            "message": {"level": "failure-note", "message": "For more information", "code": null, "spans": []}})
        .to_string();
        let clean = cargo_capture(&[], true);
        let lints = cargo_capture(&[ptr_arg.clone(), ptr_arg, unused.clone()], false);

        // Before the proposal is applied: the baseline's diagnostics stand, and
        // nothing is reported as introduced or resolved.
        let cold = compiler_facts(&lints, "", "", "", "").expect("baseline only");
        assert_eq!(cold["baseline_status"], "ran");
        assert_eq!(cold["proposed_status"], "not_run");
        assert_eq!(
            cold["open_codes"],
            serde_json::json!(["clippy::ptr_arg", "unused_variables"])
        );
        // The duplicate record cargo prints per target is one diagnostic.
        assert_eq!(
            cold["diagnostics"].as_array().expect("diagnostics").len(),
            2
        );
        assert_eq!(cold["new_lints"], serde_json::json!([]));
        assert_eq!(cold["resolved_lints"], serde_json::json!([]));
        assert_eq!(
            cold["gate"],
            serde_json::json!({"baseline_status":"ran","proposed_status":"not_run",
                "open_codes":["clippy::ptr_arg","unused_variables"],"miri":"not_run",
                "compile_errors":0,"new_lints":0,"unsafe_lines":0,"history":[],"iteration":1})
        );

        // A lint in both captures is not new, even when its line moved.
        let moved = cargo_capture(&[ptr_arg_moved, unused], false);
        let same = compiler_facts(&lints, &moved, "", "", "").expect("same lints");
        assert_eq!(same["gate"]["new_lints"], 0);
        assert_eq!(same["resolved_lints"], serde_json::json!([]));

        // Lints are told from compile errors by code, not level: with warnings
        // denied a lint arrives at level error.
        let introduced = compiler_facts(&clean, &lints, "", "", "").expect("introduced");
        assert_eq!(introduced["gate"]["compile_errors"], 0);
        assert_eq!(introduced["gate"]["new_lints"], 2);
        let resolved = compiler_facts(&lints, &clean, "", "", "").expect("resolved");
        assert_eq!(resolved["gate"]["new_lints"], 0);
        assert_eq!(
            resolved["resolved_lints"]
                .as_array()
                .expect("resolved")
                .len(),
            2
        );
        assert_eq!(resolved["open_codes"], serde_json::json!([]));

        let broken = cargo_capture(&[mismatch, syntax, note], false);
        let errors = compiler_facts(&clean, &broken, "", "", "").expect("compile errors");
        assert_eq!(errors["gate"]["compile_errors"], 2);
        assert_eq!(errors["gate"]["new_lints"], 0);
        assert_eq!(
            errors["open_codes"],
            serde_json::json!(["E0308", "rustc-error"])
        );

        // A build that fails and reports nothing is not a clean proposal.
        let silent =
            compiler_facts(&clean, &cargo_capture(&[], false), "", "", "").expect("silent failure");
        assert_eq!(silent["proposed_status"], "unexplained_failure");
        let absent = compiler_facts("", "", "", "", "").expect("nothing ran");
        assert_eq!(absent["baseline_status"], "not_run");

        // unsafe lines drop comments; Miri's status is read from its output.
        let grep = "src/a.rs:4:    // SAFETY: unsafe is fine\nsrc/a.rs:5:    unsafe { *p }\n9:unsafe fn f() {}\n";
        for (miri, status) in [
            ("", "not_run"),
            ("test result: ok.\nmiri-exit=0\n", "passed"),
            (
                "error: Undefined Behavior: dereference\nmiri-exit=nonzero\n",
                "ub",
            ),
            (
                "error: 'cargo-miri' is not installed\nmiri-exit=nonzero\n",
                "failed",
            ),
        ] {
            let facts = compiler_facts(&clean, &clean, grep, miri, "").expect("miri");
            assert_eq!(facts["gate"]["miri"], status, "{miri}");
            assert_eq!(facts["gate"]["unsafe_lines"], 2);
        }

        // The iteration and the confirmed history come from the record, in order.
        let record = "{\"iteration\":1,\"confirmed\":2,\"outcome\":\"continue\"}\n{\"iteration\":2,\"confirmed\":4,\"outcome\":\"continue\"}\n";
        let third = compiler_facts(&clean, &clean, "", "", record).expect("third pass");
        assert_eq!(third["gate"]["history"], serde_json::json!([2, 4]));
        assert_eq!(third["gate"]["iteration"], 3);
        for bad in [
            "{\"iteration\":2,\"confirmed\":1}\n",
            "{\"iteration\":1,\"outcome\":\"continue\"}\n",
        ] {
            assert!(
                compiler_facts(&clean, &clean, "", "", bad).is_err(),
                "record refused: {bad}"
            );
        }
    }

    #[test]
    fn idiomatic_rust_templates_tell_not_run_from_clean_from_reported() {
        let lint = cargo_message("error", Some("clippy::ptr_arg"), "LINT_SENTINEL", 1);
        let error = cargo_message("error", Some("E0308"), "ERROR_SENTINEL", 4);
        let clean = cargo_capture(&[], true);
        let lints = cargo_capture(&[lint], false);
        let broken = cargo_capture(&[error], false);
        let inquiry = |facts: serde_json::Value, focus: &str| {
            render_template_for_test(
                "idiomatic-rust/idiomatic-rust-inquiry",
                serde_json::json!({"design_problem":"PROBLEM_SENTINEL","existing_code":"fn f() {}",
                    "design_constraints":{},"compiler_facts":facts,"iteration_focus":focus}),
            )
            .expect("inquiry render")
        };
        let not_run = inquiry(compiler_facts("", "", "", "", "").expect("facts"), "");
        assert!(not_run.contains("did not run on this code (`not_run`)"));
        assert!(!not_run.contains("reported nothing") && !not_run.contains("ground\ntruth"));
        let clean_inquiry = inquiry(compiler_facts(&clean, "", "", "", "").expect("facts"), "");
        assert!(clean_inquiry.contains("ran on this code and reported nothing"));
        assert!(!clean_inquiry.contains("did not run"));
        let reported = inquiry(
            compiler_facts(&lints, "", "", "", "").expect("facts"),
            "FOCUS_SENTINEL",
        );
        assert!(reported.contains("LINT_SENTINEL") && reported.contains("[\"clippy::ptr_arg\"]"));
        assert!(reported.contains("Principle 2 (ownership is architecture)"));
        assert!(reported.contains("FOCUS_SENTINEL") && reported.contains("PROBLEM_SENTINEL"));
        assert!(!reported.contains("did not run") && !reported.contains("reported nothing"));
        // The eight principles reach the prompt; no typed diagnostic count is asked for.
        assert_eq!(reported.matches("\n8. **Unsafe as contract.**").count(), 1);
        assert!(
            !reported.contains("diagnostics_interpreted")
                && !reported.contains("compiler_confirmed")
        );

        let design = |actions: serde_json::Value| {
            render_template_for_test(
                "idiomatic-rust/idiomatic-rust-design",
                serde_json::json!({"design_problem":"PROBLEM_SENTINEL","existing_code":"",
                    "assessment":{"invariants":["i"]},"code_actions":actions,"iteration_focus":""}),
            )
            .expect("design render")
        };
        let no_actions = design(serde_json::json!([]));
        assert!(no_actions.contains("`get_code_actions` was not run"));
        let actions = design(serde_json::json!([
            "Found 1 code action(s):\n1. ACTION_SENTINEL"
        ]));
        assert!(actions.contains("ACTION_SENTINEL") && !actions.contains("was not run"));
        assert!(actions.contains("\"status\": \"fixed\"") && actions.contains("`open`"));

        let challenge = |facts: serde_json::Value, references: serde_json::Value| {
            render_template_for_test(
                "idiomatic-rust/idiomatic-rust-challenge",
                serde_json::json!({"design_problem":"PROBLEM_SENTINEL","design_proposal":{"proposed_design":"x"},
                    "assessment":{"invariants":["i"]},"design_constraints":{},
                    "compiler_facts":facts,"reference_results":references}),
            )
            .expect("challenge render")
        };
        let unapplied = challenge(
            compiler_facts(&clean, "", "", "", "").expect("facts"),
            serde_json::json!([]),
        );
        assert!(unapplied.contains("The proposal has not been compiled"));
        assert!(unapplied.contains("`find_references` was not run"));
        assert!(
            !unapplied.contains("was applied and compiled") && !unapplied.contains("## `unsafe`")
        );
        let silent = challenge(
            compiler_facts(&clean, &cargo_capture(&[], false), "", "", "").expect("facts"),
            serde_json::json!([]),
        );
        assert!(silent.contains("failed without reporting a diagnostic"));
        let measured = challenge(
            compiler_facts(
                &lints,
                &broken,
                "src/a.rs:5:    unsafe { UNSAFE_SENTINEL }\n",
                "error: Undefined Behavior\nmiri-exit=nonzero\n",
                "",
            )
            .expect("facts"),
            serde_json::json!(["Found 2 references to `Queue`:\n\n## REFERENCE_SENTINEL"]),
        );
        assert!(measured.contains("was applied and compiled"));
        // Each measured list reaches the prompt under its own heading: the text
        // between one heading and the next holds that list and no other.
        let section = |text: &str, from: &str, to: &str| -> String {
            let (_, tail) = text
                .split_once(from)
                .unwrap_or_else(|| panic!("no {from:?}"));
            let (body, _) = tail.split_once(to).unwrap_or_else(|| panic!("no {to:?}"));
            body.to_string()
        };
        let errors = section(&measured, "Compile errors", "Lints the proposal introduced");
        assert!(errors.contains("ERROR_SENTINEL") && !errors.contains("LINT_SENTINEL"));
        let introduced = section(
            &measured,
            "Lints the proposal introduced",
            "Baseline lints the proposal resolved",
        );
        assert!(
            !introduced.contains("_SENTINEL"),
            "this proposal introduced no lint"
        );
        let resolved = section(
            &measured,
            "Baseline lints the proposal resolved",
            "Everything still reported",
        );
        assert!(resolved.contains("LINT_SENTINEL") && !resolved.contains("ERROR_SENTINEL"));
        let still = section(
            &measured,
            "Everything still reported",
            "Clippy is a codified",
        );
        assert!(still.contains("ERROR_SENTINEL") && !still.contains("LINT_SENTINEL"));
        // A lint the proposal introduces is listed as introduced, not as resolved.
        let regressed = challenge(
            compiler_facts(&clean, &lints, "", "", "").expect("facts"),
            serde_json::json!(["Found 1 references to `Queue`"]),
        );
        let introduced = section(
            &regressed,
            "Lints the proposal introduced",
            "Baseline lints the proposal resolved",
        );
        assert!(introduced.contains("LINT_SENTINEL"));
        let resolved = section(
            &regressed,
            "Baseline lints the proposal resolved",
            "Everything still reported",
        );
        assert!(!resolved.contains("LINT_SENTINEL"));
        assert!(measured.contains("REFERENCE_SENTINEL") && measured.contains("UNSAFE_SENTINEL"));
        assert!(measured.contains("Miri on the proposal: `ub`"));
        assert!(!measured.contains("has not been compiled") && !measured.contains("was not run"));
        for typed in [
            "find_references_count",
            "proposed_code_compiles",
            "miri_checked",
            "unsafe_blocks",
        ] {
            assert!(
                !measured.contains(typed),
                "the model is not asked to type {typed}"
            );
        }
    }

    fn change_scope(
        before: &str,
        after: &str,
        audits: &str,
        plan: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        authored_jq(
            "coding-guidelines",
            "Change scope",
            serde_json::json!({"before": before, "after": after, "audits": audits,
                "assess": [{"constrained_plan": plan}]}),
        )
    }

    #[test]
    fn coding_guidelines_change_scope_is_measured_against_the_plan() {
        let plan = serde_json::json!([
            {"step":"a","files_to_touch":["src/a.rs","src/b.rs"],"files_not_to_touch":["src/c.rs"]},
            {"step":"b","files_to_touch":["src/b.rs"],"files_not_to_touch":["src/d.rs"]}
        ]);
        // Another session's uncommitted file is in both captures, unchanged.
        let before = "7\t2\tkask/other/session.rs\n";
        let inside = "7\t2\tkask/other/session.rs\n3\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n";
        let exact = change_scope(before, inside, "", plan.clone()).expect("change inside the plan");
        assert_eq!(
            exact,
            serde_json::json!({"changed":["src/a.rs","src/b.rs"],"unplanned":[],
                "untouched_planned":[],"forbidden_touched":[],"prior_audits":[]})
        );

        // A stray file, a forbidden file, an untracked file, and a planned file
        // left alone are each reported in their own list.
        let stray =
            "7\t2\tkask/other/session.rs\n3\t1\tsrc/a.rs\n2\t2\tsrc/c.rs\nnew\tnew\tsrc/z.rs\n";
        let audits = "{\"audit\":1,\"verdict\":\"correct\",\"overall\":0.65}\n";
        let strayed = change_scope(before, stray, audits, plan.clone()).expect("stray change");
        assert_eq!(
            strayed,
            serde_json::json!({"changed":["src/a.rs","src/c.rs","src/z.rs"],
                "unplanned":["src/c.rs","src/z.rs"],"untouched_planned":["src/b.rs"],
                "forbidden_touched":["src/c.rs"],"prior_audits":["correct"]})
        );

        // A file whose line moved between the captures counts as changed.
        let grown = "9\t2\tkask/other/session.rs\n3\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n";
        assert_eq!(
            change_scope(before, grown, "", plan.clone()).expect("other file grew")["unplanned"],
            serde_json::json!(["kask/other/session.rs"])
        );
        let nothing = change_scope(before, before, "", plan.clone()).expect("no change");
        assert_eq!(nothing["changed"], serde_json::json!([]));
        assert_eq!(
            nothing["untouched_planned"],
            serde_json::json!(["src/a.rs", "src/b.rs"])
        );

        // A committed change is audited with no baseline.
        let committed = change_scope(
            "",
            "3\t1\tsrc/a.rs\n-\t-\tassets/logo.png\n",
            "",
            plan.clone(),
        )
        .expect("committed change");
        assert_eq!(
            committed["changed"],
            serde_json::json!(["src/a.rs", "assets/logo.png"])
        );
        assert_eq!(
            committed["unplanned"],
            serde_json::json!(["assets/logo.png"])
        );

        assert!(
            change_scope("", "not numstat output\n", "", plan.clone()).is_err(),
            "a line that is not numstat output is refused"
        );
        for bad in [
            "{\"audit\":2,\"verdict\":\"correct\"}\n",
            "{\"audit\":1}\n",
            "{\"audit\":1,\"verdict\":\"correct\"}\n{\"audit\":1,\"verdict\":\"pass\"}\n",
        ] {
            assert!(
                change_scope(before, inside, bad, plan.clone()).is_err(),
                "audit record refused: {bad}"
            );
        }
    }

    #[test]
    fn coding_guidelines_templates_carry_the_plan_fields_and_the_measured_scope() {
        let assess = render_template_for_test(
            "coding-guidelines/guidelines-assess",
            serde_json::json!({"task_description":"TASK_SENTINEL","code_context":{"note":"CONTEXT_SENTINEL"}}),
        )
        .expect("assess render");
        assert!(assess.contains("TASK_SENTINEL") && assess.contains("CONTEXT_SENTINEL"));
        // The field is asked for (with the rule that it is an exact quote) and shown.
        assert!(assess.contains("- `traces_to`: The words of the task above"));
        assert!(assess.contains("copied exactly") && assess.contains("\"traces_to\":"));
        assert!(assess.contains("7. **Style changes outside task scope**"));
        assert!(!assess.contains("assumption_log"));

        let verify = |scope: serde_json::Value, diff: &str| {
            render_template_for_test(
                "coding-guidelines/guidelines-verify",
                serde_json::json!({"task_description":"TASK_SENTINEL",
                    "assumptions":[{"assumption":"ASSUMPTION_SENTINEL"}],
                    "success_criteria":[{"goal":"CRITERION_SENTINEL"}],
                    "constrained_plan":[{"step":"PLAN_SENTINEL"}],
                    "diff_preview":diff,"scope":scope}),
            )
            .expect("verify render")
        };
        let audited = verify(
            serde_json::json!({"changed":["CHANGED_SENTINEL","UNPLANNED_SENTINEL","FORBIDDEN_SENTINEL"],
                "unplanned":["UNPLANNED_SENTINEL"],"untouched_planned":["UNTOUCHED_SENTINEL"],
                "forbidden_touched":["FORBIDDEN_SENTINEL"],"prior_audits":[]}),
            "DIFF_SENTINEL",
        );
        for text in [
            "TASK_SENTINEL",
            "ASSUMPTION_SENTINEL",
            "CRITERION_SENTINEL",
            "PLAN_SENTINEL",
            "DIFF_SENTINEL",
            "CHANGED_SENTINEL",
        ] {
            assert!(audited.contains(text), "{text}");
        }
        // Each measured list sits on its own labelled line.
        for (label, sentinel) in [
            (
                "- Changed but in no step's `files_to_touch`: ",
                "[\"UNPLANNED_SENTINEL\"]",
            ),
            (
                "- Changed although a step listed it under `files_not_to_touch`: ",
                "[\"FORBIDDEN_SENTINEL\"]",
            ),
            ("- Planned but not changed: ", "[\"UNTOUCHED_SENTINEL\"]"),
        ] {
            assert!(audited.contains(&format!("{label}{sentinel}")), "{label}");
        }
        // The audit checks the same seven anti-patterns the plan forbade.
        assert!(audited.contains("6. **Unrequested logging/telemetry**"));
        assert!(audited.contains("7. **Style changes outside task scope**"));
        assert!(!audited.contains("No file was changed"));
        // The model is asked for no overall score, count or recommendations list.
        assert!(!audited.contains("\"recommendations\"") && !audited.contains("overall_score"));

        let unchanged = verify(
            serde_json::json!({"changed":[],"unplanned":[],"untouched_planned":["UNTOUCHED_SENTINEL"],
                "forbidden_touched":[],"prior_audits":[]}),
            "",
        );
        assert!(
            unchanged.contains("No file was changed")
                && unchanged.contains("No diff text was provided")
        );
        assert!(!unchanged.contains("- Changed:"));
        let mut missing = serde_json::json!({"task_description":"t","assumptions":[],"success_criteria":[],
            "constrained_plan":[],"diff_preview":""});
        assert!(
            render_template_for_test("coding-guidelines/guidelines-verify", missing.take())
                .expect_err("the measured scope is required")
                .contains("scope")
        );
    }

    /// The essentialist template is the interrogator's once-per-run reference:
    /// the three gates, the finding vocabulary the `ESS-1` gate consumes, and
    /// the reduction actions. The loop (mode, order, retries, rounds, zero-delta,
    /// score) lives in the SKILL.md form and must not be restated here as
    /// pseudo-code, nor may a `mode: autonomous` claim be printed as verified.
    #[test]
    fn essentialist_flow_is_a_gate_reference_not_a_loop() {
        let context = serde_json::json!({
            "artifact": {"name": "Wrapper", "source": "pub fn get() {}"},
            "code_context": {"callers": {"a.rs": "get()"}, "dependencies": {}, "structure": ["mod wrapper"]},
            "mode": "autonomous",
            "scope": "auto"
        });
        let rendered = render_template_for_test("essentialist/essentialist-flow", context.clone())
            .expect("essentialist flow renders with the documented context");
        for gate in [
            "Gate 1 — EXIST: Deletion Test",
            "Gate 2 — SURFACE: Interface Count",
            "Gate 3 — CONTRACT: Abstraction Trace",
        ] {
            assert!(rendered.contains(gate), "missing {gate}");
        }
        // The finding vocabulary ESS-1 validates against, verbatim.
        assert!(rendered.contains(
            "\"constraint_force\": \"prohibition|guardrail|guideline|evidence|hypothesis\""
        ));
        assert!(rendered.contains("\"action\": \"DELETE|MERGE|JUSTIFY|RETAIN\""));
        assert!(rendered.contains("malformed_findings"));
        // G3 is this template's own trace; the former delegation is gone.
        assert!(rendered.contains("Every generic parameter"));
        assert!(!rendered.contains("guidelines-verify"));
        // No loop restatement and no authorization claim from a parameter.
        for absent in [
            "FOR round",
            "WHILE retries",
            "max_retries_per_gate",
            "authorization was verified",
        ] {
            assert!(
                !rendered.contains(absent),
                "template restates the loop: {absent}"
            );
        }
        assert!(rendered.contains("Claimed mode**: autonomous"));
        assert!(rendered.contains("resolves to advisory"));

        let mut missing = context;
        missing
            .as_object_mut()
            .expect("context object")
            .remove("code_context");
        assert!(
            render_template_for_test("essentialist/essentialist-flow", missing)
                .expect_err("the code context is required")
                .contains("code_context")
        );
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
    fn context_path_loads_a_json_object_file() {
        let dir =
            std::env::temp_dir().join(format!("render-template-context-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("context.json");
        std::fs::write(
            &path,
            serde_json::to_string(&serde_json::json!({"task": "from-file", "items": [1, 2]}))
                .expect("serialize context"),
        )
        .expect("write context file");
        let input: RenderTemplateToolInput = serde_json::from_value(serde_json::json!({
            "template_ref": "essentialist/essentialist-flow",
            "context_path": path.to_string_lossy().into_owned(),
        }))
        .expect("context_path input");
        assert!(input.context.is_empty(), "inline context stays empty");
        let loaded = load_context_file(&input.context_path.unwrap()).expect("load");
        assert_eq!(loaded["task"], "from-file");
        assert_eq!(loaded["items"], serde_json::json!([1, 2]));
    }

    #[test]
    fn context_path_rejects_non_object_and_missing_files() {
        let dir = std::env::temp_dir().join(format!(
            "render-template-context-bad-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let array_path = dir.join("array.json");
        std::fs::write(&array_path, "[1,2]").expect("write array file");
        assert!(
            load_context_file(&array_path.to_string_lossy()).is_err(),
            "non-object context file must be rejected"
        );
        assert!(
            load_context_file(&dir.join("missing.json").to_string_lossy()).is_err(),
            "missing context file must be rejected"
        );
    }

    #[test]
    fn context_path_renders_a_real_template_end_to_end() {
        // The original failure: a context payload large enough that the model
        // could not emit it as one tool-call argument. Through context_path the
        // same payload renders without crossing the model boundary inline.
        let dir = std::env::temp_dir().join(format!(
            "render-template-context-e2e-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("context.json");
        let large_value = "x".repeat(64 * 1024);
        let context = serde_json::json!({
            "prompt": large_value,
            "prompt_type": "agent-task",
            "effort_tier": "medium",
            "risks": [],
            "checkability_map": [],
            "task": "large-context render",
            "unfold_mode_resolved": "task_graph",
            "finer_pico_framing": {},
            "reference_models": [],
            "prior_enhanced_prompt": "",
            "critic_feedback": []
        });
        std::fs::write(&path, serde_json::to_string(&context).expect("serialize")).expect("write");
        crate::set_template_base_path(registry_template_base());
        let input: RenderTemplateToolInput = serde_json::from_value(serde_json::json!({
            "template_ref": "prompt-enhance/enhance-rewrite",
            "context_path": path.to_string_lossy().into_owned(),
        }))
        .expect("context_path input");
        let rendered = render_registry_template(&input).expect("large context renders");
        assert!(
            rendered.contains("large-context render"),
            "rendered prompt must carry the file-supplied task"
        );
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

    /// Every key the classify and verify templates ask the model to emit has
    /// a reader: a later stage's contract input, the PE-State gate, or the
    /// SKILL.md step that consumes it. An output with no reader is a model
    /// call's worth of tokens nothing uses.
    #[test]
    fn prompt_enhance_stage_outputs_all_have_a_reader() {
        fn output_keys(rendered: &str) -> Vec<String> {
            let section = rendered.rsplit_once("## Output").expect("output section").1;
            let block = section.split_once("```json\n").expect("JSON block").1;
            let payload = block.split_once("\n```").expect("closed JSON block").0;
            payload
                .lines()
                .filter_map(|line| line.strip_prefix("  \""))
                .filter_map(|rest| rest.split_once('"'))
                .map(|(key, _)| key.to_string())
                .collect()
        }
        let classify = render_template_for_test(
            "prompt-enhance/enhance-classify",
            json!({"prompt": "p", "task": "t", "effort": "medium", "output_format": "inline"}),
        )
        .expect("classify renders");
        assert_eq!(
            output_keys(&classify),
            [
                "prompt_type",
                "effort_tier",
                "output_format_resolved",
                "output_path_resolved",
                "unfold_mode_resolved",
                "anchor_terms",
                "finer_pico_framing",
                "proxy_eval_set",
                "checkability_map",
                "risks"
            ]
        );
        let verify = render_template_for_test(
            "prompt-enhance/enhance-verify",
            json!({
                "enhanced_prompt": "e", "original_prompt": "p", "prompt_type": "coding",
                "effort_tier": "high", "proxy_eval_set": [], "acceptance_criteria": [],
                "checkability_map": [], "attempt_id": 1, "reviewed_revision": 0, "round": 3
            }),
        )
        .expect("verify renders");
        // The three keys PE-State reads from a completed attempt's outcome.
        assert_eq!(output_keys(&verify), ["ratings", "findings", "verdict"]);
        for area in [
            "Recall",
            "Mechanism",
            "Rationale",
            "Edge Cases",
            "Synthesis",
        ] {
            assert!(verify.contains(&format!("\"area\": \"{area}\"")), "{area}");
        }
    }

    /// The conflict template asks for classifications and a strategy; the
    /// ranking, the winner and the escalation cases are PS-2's. A template
    /// that asked the model for a rank or a winner would duplicate the gate
    /// and let the two disagree.
    #[test]
    fn pragmatic_semantics_conflict_template_classifies_and_does_not_rank() {
        let render = |conflicts: serde_json::Value, statements: serde_json::Value| {
            render_template_for_test(
                "pragmatic-semantics/semantics-conflict-resolve",
                json!({
                    "provenance_result": {"conflicts_detected": conflicts},
                    "classification_result": {"provenance": "runtime"},
                    "statements": statements
                }),
            )
            .expect("conflict-resolve renders")
        };
        let detected = render(
            json!(true),
            json!([{"id": "S1", "statement": "A"}, {"id": "S2", "statement": "B"}]),
        );
        assert!(detected.contains("## Conflict Detected"));
        assert!(detected.contains("## Statements (the multi-statement case)"));
        let output = detected
            .rsplit_once("## Output Requirements")
            .expect("output section")
            .1;
        let block = output.split_once("```json\n").expect("JSON block").1;
        let shape = block.split_once("\n```").expect("closed block").0;
        let top_level: Vec<&str> = shape
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .filter_map(|rest| rest.split_once('"'))
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            top_level,
            ["statements", "resolution_strategy", "reasoning"]
        );
        for computed in [
            "\"rank\"",
            "\"winner\"",
            "\"ranked_statements\"",
            "escalate\"",
        ] {
            assert!(
                !shape.contains(computed),
                "{computed} is PS-2's, not the model's"
            );
        }
        // Every tier PS-2 reads is asked for, under the name PS-2 reads.
        for tier in [
            "\"id\"",
            "\"ontological_mode\"",
            "\"epistemic_mode\"",
            "\"constraint_force\"",
            "\"provenance\"",
            "\"ontology_anchor\"",
        ] {
            assert!(shape.contains(tier), "{tier} missing from the output shape");
        }

        let none = render(json!(false), serde_json::Value::Null);
        assert!(none.contains("## No Conflicts Detected"));
        assert!(!none.contains("## Statements (the multi-statement case)"));
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

    const EQM_HELPS: [&str; 6] = [
        "statistical_reasoning",
        "fact_based",
        "concrete_reasoning",
        "forecast_rationale_align",
        "best_practices",
        "statistical_causal_blend",
    ];
    const EQM_HURTS: [&str; 6] = [
        "gut_based",
        "simplification_bias",
        "confirmation_bias",
        "extreme_confidence",
        "forecast_rationale_misalign",
        "speculative_terms",
    ];

    /// One `per_rationale_scores` record: every helps marker at `helps`,
    /// every hurts marker at 0, then the named overrides. The composite is
    /// derived, so a fixture cannot disagree with its own scores.
    fn eqm_record(
        index: usize,
        question: &str,
        forecaster: Option<&str>,
        helps: u8,
        overrides: &[(&str, u8)],
    ) -> serde_json::Value {
        let score = |id: &str, base: u8| {
            overrides
                .iter()
                .find(|(name, _)| *name == id)
                .map_or(base, |(_, value)| *value)
        };
        let mut composite = 0i64;
        let mut scores = Vec::new();
        for id in EQM_HELPS {
            let value = score(id, helps);
            composite += i64::from(value);
            scores.push(json!({"id": id, "score": value}));
        }
        for id in EQM_HURTS {
            let value = score(id, 0);
            composite -= i64::from(value);
            scores.push(json!({"id": id, "score": value}));
        }
        let mut record = json!({
            "rationale_index": index, "question": question,
            "composite_score": composite, "scores": scores
        });
        if let Some(forecaster) = forecaster {
            record["forecaster_id"] = json!(forecaster);
        }
        record
    }

    fn eqm_aggregate(records: Vec<serde_json::Value>) -> serde_json::Value {
        let rendered = render_template_for_test(
            "eqm/eqm-aggregate",
            json!({"per_rationale_scores": records}),
        )
        .expect("the aggregate renders");
        serde_json::from_str(&rendered).expect("the aggregate render is a JSON document")
    }

    fn eqm_ids(markers: &serde_json::Value) -> Vec<&str> {
        markers
            .as_array()
            .expect("marker list")
            .iter()
            .map(|marker| marker["id"].as_str().expect("marker id"))
            .collect()
    }

    #[test]
    fn eqm_aggregate_computes_composites_screens_and_bias() {
        // r0: helps 9, hurts 4, composite 5; two hurts at 2.
        // r1: all on target, composite 12. r2: helps 6, one hurts at 1.
        let out = eqm_aggregate(vec![
            eqm_record(
                0,
                "Q1",
                Some("f1"),
                2,
                &[
                    ("statistical_reasoning", 0),
                    ("best_practices", 1),
                    ("gut_based", 2),
                    ("extreme_confidence", 2),
                ],
            ),
            eqm_record(1, "Q1", Some("f2"), 2, &[]),
            eqm_record(2, "Q2", Some("f1"), 1, &[("speculative_terms", 1)]),
        ]);

        assert_eq!(out["status"], "complete");
        assert_eq!(out["incomplete_rationales"], json!([]));
        assert_eq!(
            out["forecast_level_composite"],
            json!([
                {"question": "Q1", "mean_composite": 8.5, "red_flag_count": 2, "green_flag_count": 10, "rationale_count": 2},
                {"question": "Q2", "mean_composite": 5.0, "red_flag_count": 0, "green_flag_count": 0, "rationale_count": 1}
            ])
        );
        assert_eq!(
            out["forecaster_level_composite"],
            json!([
                {"forecaster_id": "f1", "mean_composite": 5.0, "red_flag_frequency": 1.0, "green_flag_frequency": 2.0, "rationale_count": 2},
                {"forecaster_id": "f2", "mean_composite": 12.0, "red_flag_frequency": 0.0, "green_flag_frequency": 6.0, "rationale_count": 1}
            ])
        );
        // (1 - 4 - 2) / (2 * 3)
        assert_eq!(out["overconfidence_bias"], json!(-0.8333));

        // EQM-2: two hurts at 2 screens red; composite >= 4 endorses only
        // with no hurts above 0 (r0 and r2 both have composite 5).
        let red = out["red_flag_screen"].as_array().expect("red screen");
        assert_eq!(red.len(), 1);
        assert_eq!(red[0]["rationale_index"], 0);
        assert_eq!(
            red[0]["red_flags"],
            json!(["Extreme Confidence", "Gut Based"])
        );
        let green = out["green_flag_endorsement"]
            .as_array()
            .expect("green endorsement");
        assert_eq!(green.len(), 1);
        assert_eq!(green[0]["rationale_index"], 1);
        assert_eq!(green[0]["confidence"], "weak");

        // Priority order: hurts at 2, hurts at 1, helps at 0, helps at 1.
        let summary = &out["failing_marker_summary"];
        assert_eq!(
            eqm_ids(&summary[0]["failing_markers"]),
            [
                "extreme_confidence",
                "gut_based",
                "statistical_reasoning",
                "best_practices"
            ]
        );
        assert_eq!(summary[1]["failing_markers"], json!([]));
        assert_eq!(
            eqm_ids(&summary[2]["failing_markers"])[0],
            "speculative_terms"
        );
        let first = &summary[0]["failing_markers"][0];
        assert_eq!(first["target_score"], 0);
        assert_eq!(first["gap"], 2);
        assert!(
            first["description"]
                .as_str()
                .is_some_and(|text| text.starts_with("The forecaster expresses a high degree"))
        );
    }

    #[test]
    fn eqm_aggregate_keeps_case_distinct_groups_apart() {
        let out = eqm_aggregate(vec![
            eqm_record(0, "q", Some("F1"), 2, &[]),
            eqm_record(1, "Q", Some("f1"), 2, &[]),
            eqm_record(2, "Q", None, 2, &[]),
        ]);
        let counts = |field: &str, key: &str| -> Vec<(String, u64)> {
            out[field]
                .as_array()
                .expect("group list")
                .iter()
                .map(|group| {
                    (
                        group[key].as_str().expect("group key").to_string(),
                        group["rationale_count"].as_u64().expect("count"),
                    )
                })
                .collect()
        };
        assert_eq!(
            counts("forecast_level_composite", "question"),
            [("Q".to_string(), 2), ("q".to_string(), 1)]
        );
        // The rationale with no forecaster_id is in no forecaster group.
        assert_eq!(
            counts("forecaster_level_composite", "forecaster_id"),
            [("F1".to_string(), 1), ("f1".to_string(), 1)]
        );
        assert_eq!(out["overconfidence_bias"], json!(-2.0));
    }

    #[test]
    fn eqm_aggregate_refuses_incomplete_batches() {
        let good = || eqm_record(0, "Q1", Some("f1"), 2, &[]);
        let empty_aggregates = |out: &serde_json::Value| {
            for field in [
                "forecast_level_composite",
                "forecaster_level_composite",
                "red_flag_screen",
                "green_flag_endorsement",
                "failing_marker_summary",
            ] {
                assert_eq!(out[field], json!([]), "{field}");
            }
            assert!(out["overconfidence_bias"].is_null());
        };

        let mut absent = eqm_record(1, "Q1", Some("f2"), 2, &[]);
        absent["scores"]
            .as_array_mut()
            .expect("scores")
            .retain(|score| score["id"] != "fact_based");
        let mut tool_missing = eqm_record(2, "Q2", Some("f1"), 2, &[]);
        tool_missing["missing_eqms"] = json!(["gut_based"]);
        let mut duplicated = eqm_record(3, "Q2", Some("f1"), 2, &[]);
        let repeat = duplicated["scores"][0].clone();
        duplicated["scores"]
            .as_array_mut()
            .expect("scores")
            .push(repeat);
        let mut out_of_range = eqm_record(4, "Q2", Some("f1"), 2, &[]);
        out_of_range["scores"][6]["score"] = json!(3);
        let mut wrong_composite = eqm_record(5, "Q2", Some("f1"), 2, &[]);
        wrong_composite["composite_score"] = json!(9);

        let out = eqm_aggregate(vec![
            good(),
            absent,
            tool_missing,
            duplicated,
            out_of_range,
            wrong_composite,
        ]);
        assert_eq!(out["status"], "incomplete");
        empty_aggregates(&out);
        let reasons = out["incomplete_rationales"]
            .as_array()
            .expect("incomplete list");
        assert_eq!(
            reasons
                .iter()
                .map(|reason| reason["rationale_index"].as_u64().expect("index"))
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5],
            "the complete rationale is not listed"
        );
        assert_eq!(reasons[0]["absent_ids"], json!(["fact_based"]));
        assert_eq!(reasons[1]["missing_eqms"], json!(["gut_based"]));
        assert_eq!(reasons[2]["duplicate_ids"], true);
        assert_eq!(reasons[3]["out_of_range_ids"], json!(["gut_based"]));
        assert_eq!(reasons[4]["composite_mismatch"], true);

        let out = eqm_aggregate(Vec::new());
        assert_eq!(out["status"], "unassessed");
        assert_eq!(out["incomplete_rationales"], json!([]));
        empty_aggregates(&out);
    }

    #[test]
    fn eqm_improve_templates_compute_the_failing_markers() {
        let record = eqm_record(
            0,
            "Q1",
            None,
            2,
            &[
                ("statistical_reasoning", 0),
                ("best_practices", 1),
                ("gut_based", 2),
                ("extreme_confidence", 2),
            ],
        );
        let scores = record["scores"].clone();

        let predict = render_template_for_test(
            "eqm/eqm-imp-predict",
            json!({"scores": scores, "current_composite": 5}),
        )
        .expect("predict renders");
        let selected = section_json(&predict, "## The Marker This Iteration Addresses");
        assert_eq!(eqm_ids(&selected), ["extreme_confidence"]);
        assert_eq!(selected[0]["current_score"], 2);
        assert_eq!(selected[0]["target_score"], 0);

        let on_target = eqm_record(0, "Q1", None, 2, &[]);
        let predict = render_template_for_test(
            "eqm/eqm-imp-predict",
            json!({"scores": on_target["scores"], "current_composite": 12}),
        )
        .expect("predict renders with nothing off target");
        assert_eq!(
            section_json(&predict, "## The Marker This Iteration Addresses"),
            json!([])
        );

        let experiment = render_template_for_test(
            "eqm/eqm-imp-experiment",
            json!({
                "rationale": "RATIONALE-TEXT", "forecast_probability": 0.35,
                "question": "Q1", "prediction": {"hypothesis": "H"}, "scores": scores
            }),
        )
        .expect("experiment renders");
        let failing = section_json(&experiment, "## Failing Markers");
        assert_eq!(
            eqm_ids(&failing),
            [
                "extreme_confidence",
                "gut_based",
                "statistical_reasoning",
                "best_practices"
            ]
        );
        assert!(experiment.contains("RATIONALE-TEXT"));
        assert!(experiment.contains("0.35"));
    }

    #[test]
    fn eqm_validate_returns_the_constant_result_without_outcomes() {
        let rendered = render_template_for_test(
            "eqm/eqm-validate",
            json!({"forecaster_level_composite": [], "realized_outcomes": [], "outcome_statistics": {}}),
        )
        .expect("validate renders without outcomes");
        let out: serde_json::Value =
            serde_json::from_str(&rendered).expect("the no-outcomes render is a JSON document");
        assert_eq!(out["validation_verdict"], "undetermined");
        assert!(out["gaming_suspected"].is_null());
        assert_eq!(out["validation_findings"][0]["status"], "skipped");

        let rendered = render_template_for_test(
            "eqm/eqm-validate",
            json!({
                "forecaster_level_composite": [{"forecaster_id": "f1", "mean_composite": 5.0}],
                "realized_outcomes": [{"question": "Q1", "forecast_probability": 0.35, "outcome": false, "forecaster_id": "f1"}],
                "outcome_statistics": {"forecasters_resolved": 1, "brier_by_forecaster": {"f1": 0.1225}, "composite_accuracy_r": "Undetermined"}
            }),
        )
        .expect("validate renders with outcomes");
        assert!(rendered.contains("You are an EQM validation analyst"));
        assert!(rendered.contains("0.1225"));
        assert!(rendered.contains("\"composite_accuracy_r\":\"Undetermined\""));

        // A forecaster with resolved outcomes and no Brier score: the fixed
        // result names them, and no prompt is rendered.
        let rendered = render_template_for_test(
            "eqm/eqm-validate",
            json!({
                "forecaster_level_composite": [],
                "realized_outcomes": [
                    {"question": "Q1", "forecast_probability": 0.35, "outcome": false, "forecaster_id": "f1"},
                    {"question": "Q1", "forecast_probability": 0.6, "outcome": true, "forecaster_id": "f2"}
                ],
                "outcome_statistics": {"forecasters_resolved": 1, "brier_by_forecaster": {"f1": 0.1225}, "composite_accuracy_r": "Undetermined"}
            }),
        )
        .expect("validate renders with an uncovered forecaster");
        let out: serde_json::Value =
            serde_json::from_str(&rendered).expect("the uncovered render is a JSON document");
        assert_eq!(out["validation_verdict"], "undetermined");
        assert_eq!(out["validation_findings"][0]["forecasters"], json!(["f2"]));
    }

    /// `predictive-markers.j2` is generated from `eqm-catalog.yaml`. A catalog
    /// edit that is not regenerated into the fragment fails here: ids, names
    /// and directions must be equal, and each description must be the
    /// catalog's folded text.
    #[test]
    fn eqm_marker_specs_match_the_catalog() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../kask/registry/templates");
        let catalog: serde_yaml::Value = serde_yaml::from_str(
            &std::fs::read_to_string(base.join("eqm/eqm-catalog.yaml")).expect("catalog"),
        )
        .expect("catalog parses as YAML");
        let text = |value: &serde_yaml::Value, key: &str| {
            value[key]
                .as_str()
                .unwrap_or_else(|| panic!("catalog marker lacks {key}"))
                .trim()
                .to_string()
        };
        let mut expected = Vec::new();
        for family in catalog["categories"].as_sequence().expect("families") {
            for marker in family["markers"].as_sequence().expect("markers") {
                if marker["predictive"].as_bool() == Some(true) {
                    expected.push(json!({
                        "id": text(marker, "id"), "name": text(marker, "name"),
                        "direction": text(marker, "direction"),
                        "description": text(marker, "description")
                    }));
                }
            }
        }
        assert_eq!(expected.len(), 12, "the catalog marks twelve predictive");

        let mut environment = minijinja::Environment::new();
        super::bind_registry_loader(&mut environment, &base);
        let rendered = environment
            .render_str(
                "{% from \"eqm/predictive-markers.j2\" import marker_specs %}{{ marker_specs | tojson }}",
                json!({}),
            )
            .expect("the fragment exports marker_specs");
        let specs: serde_json::Value = serde_json::from_str(&rendered).expect("marker_specs JSON");
        assert_eq!(specs, serde_json::Value::Array(expected));
    }

    #[test]
    fn seam_audit_final_report_computes_its_facts() {
        let context = json!({
            "task": "fixture",
            "security_track": {"findings": [], "defense_layer_coverage": {"layers": [{"id": 1}, {"id": 2}], "verdict": "Conditional"}},
            "arch_track": {"findings": [], "dead_surface_summary": {}},
            "ui_track": {"findings": [], "pattern_gap_summary": {}},
            "annotated_findings": [
                {"id": "KS-01", "constraint_force": "blocking"},
                {"id": "KS-02", "constraint_force": "blocking"},
                {"id": "RA-01", "constraint_force": "directing"}
            ],
            "gate_b": {"per_track": [2, 1, 0], "per_severity": [0, 1, 1, 1, 0]},
            "ksa_2": {"top3": ["KS-01", "RA-01"], "stable_under_perturbation": false, "excluded": ["UI-07"]},
            "applied_remediations": [{"id": "KS-01", "test_description": "pins it"}],
            "hard_stop": {"triggered": false},
            "convergence_score": 0
        });
        let rendered = render_template_for_test("kask-seam-audit/final-report", context)
            .expect("final-report renders");
        assert_eq!(
            section_json(&rendered, "## Computed facts"),
            json!({
                "findings_per_track": {"security": 2, "architecture": 1, "ui": 0},
                "findings_per_severity": {"critical": 0, "high": 1, "medium": 1, "low": 1, "info": 0},
                "defense_layers_declared": 2,
                "defense_layer_verdict": "Conditional",
                "mcda_top3": ["KS-01", "RA-01"],
                "mcda_stable_under_perturbation": false,
                "mcda_excluded": ["UI-07"],
                "applied_remediation_ids": ["KS-01"],
                // KS-02 is blocking and was not applied; KS-01 was.
                "blocking_findings_unremediated": ["KS-02"],
                "hard_stop_triggered": false,
                "convergence_score": 0
            })
        );
        // The model's output shape no longer carries a computed value.
        let output = rendered
            .split_once("## Output (JSON only)")
            .expect("output section")
            .1;
        for retyped in [
            "convergence_score",
            "mcda_top3",
            "hard_stop_triggered",
            "iterations",
        ] {
            assert!(
                !output.contains(&format!("\"{retyped}\"")),
                "{retyped} is retyped"
            );
        }
    }

    #[test]
    fn seam_audit_remediate_shows_only_the_top_ranked_candidates() {
        let context = |defects: Option<serde_json::Value>| {
            let mut context = json!({
                "task": "fixture", "top3": ["KS-01", "RA-01"],
                "candidates": [
                    {"id": "KS-01", "remediation": "tighten the gate"},
                    {"id": "UI-07", "remediation": "not ranked"},
                    {"id": "RA-01", "remediation": "delete the helper"}
                ],
                "divergence_surface": ["crates/zed/src/main.rs", "crates/hkask-steer/"],
                "seam_scope": "kask/"
            });
            if let Some(defects) = defects {
                context["gate_defects"] = defects;
            }
            context
        };
        let rendered = render_template_for_test("kask-seam-audit/remediate", context(None))
            .expect("remediate renders");
        assert_eq!(
            section_json(&rendered, "Their descriptions:"),
            json!([
                {"id": "KS-01", "remediation": "tighten the gate"},
                {"id": "RA-01", "remediation": "delete the helper"}
            ])
        );
        assert!(!rendered.contains("not ranked"));
        assert_eq!(
            section_json(&rendered, "## Which files may be touched"),
            json!(["crates/zed/src/main.rs", "crates/hkask-steer/"])
        );
        assert!(
            !rendered.contains("within_kask"),
            "the seam test is Gate C's"
        );
        assert!(!rendered.contains("Gate C defects from the previous attempt"));

        let repair = render_template_for_test(
            "kask-seam-audit/remediate",
            context(Some(json!([["KS-01", "remediation_missing_test"]]))),
        )
        .expect("remediate renders a repair");
        assert_eq!(
            section_json(&repair, "## Gate C defects from the previous attempt"),
            json!([["KS-01", "remediation_missing_test"]])
        );
    }

    #[test]
    fn seam_audit_templates_carry_gate_defects_only_on_a_repair() {
        let adjudicate = |defects: Option<serde_json::Value>| {
            let mut context = json!({
                "task": "fixture",
                "security_findings": [{"id": "KS-01", "severity": "high", "file_line": "kask/a.rs:1"}],
                "arch_findings": [], "ui_findings": [],
                "defense_layer_coverage": {"layers": [], "verdict": "Pass"}
            });
            if let Some(defects) = defects {
                context["gate_defects"] = defects;
            }
            render_template_for_test("kask-seam-audit/adjudicate", context)
                .expect("adjudicate renders")
        };
        let first = adjudicate(None);
        assert!(!first.contains("Gate FA defects from the previous attempt"));
        assert_eq!(
            section_json(&first, "security:"),
            json!([{"id": "KS-01", "severity": "high", "file_line": "kask/a.rs:1"}])
        );
        let repair = adjudicate(Some(json!([["KS-01", "revoked_function_not_retired"]])));
        assert_eq!(
            section_json(&repair, "## Gate FA defects from the previous attempt"),
            json!([["KS-01", "revoked_function_not_retired"]])
        );

        for (template, context) in [
            (
                "kask-seam-audit/audit-security",
                json!({"task": "t", "seam_scope": "kask/", "prior_verdicts": []}),
            ),
            (
                "kask-seam-audit/audit-architecture",
                json!({"task": "t", "seam_scope": "kask/"}),
            ),
            ("kask-seam-audit/audit-ui", json!({"task": "t"})),
        ] {
            let first = render_template_for_test(template, context.clone()).expect("track renders");
            assert!(
                !first.contains("Gate B defects from the previous attempt"),
                "{template}"
            );
            let mut with_defects = context;
            with_defects["gate_defects"] = json!([["X-1", "invalid_severity"]]);
            let repair = render_template_for_test(template, with_defects).expect("track repair");
            assert_eq!(
                section_json(&repair, "## Gate B defects from the previous attempt"),
                json!([["X-1", "invalid_severity"]]),
                "{template}"
            );
        }
    }
}
