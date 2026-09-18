//! Strict model-facing passage-tagging protocol.
//!
//! This module owns deployed-template rendering and response correlation only.
//! Callers retain model invocation, ontology enrichment, persistence, recall,
//! and degradation policy.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use hkask_types::corpus::ExpertiseLevel;
use serde::Deserialize;
use thiserror::Error;

const TEMPLATE_NAME: &str = "tag-passages-batch";
const TEMPLATE_DIR: &str = "templates/docproc";
const MIN_CANDIDATE_TERMS: usize = 3;
const MAX_CANDIDATE_TERMS: usize = 5;
const MAX_CORRELATION_ID_LEN: usize = 32;

/// One process-lifetime snapshot of the fixed deployed template. Explicit
/// alternate roots (used by isolated tests) render uncached, so the cache and
/// leaked source remain strictly bounded to one entry.
static TEMPLATE_CACHE: std::sync::OnceLock<
    std::sync::Mutex<HashMap<PathBuf, minijinja::Environment<'static>>>,
> = std::sync::OnceLock::new();

/// One passage with a caller-generated, batch-local correlation identifier.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Passage {
    pub correlation_id: String,
    pub text: String,
}

impl Passage {
    pub fn new(correlation_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            correlation_id: correlation_id.into(),
            text: text.into(),
        }
    }
}

/// Whether expertise is deterministic caller metadata or a model judgment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertiseMode {
    ServerDerived,
    ModelAssigned,
}

/// Validated request used by the deployed tagging template and response parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassageTaggingRequest {
    passages: Vec<Passage>,
    model_dimensions: Vec<String>,
    server_dimensions: Vec<String>,
    expertise_mode: ExpertiseMode,
}

impl PassageTaggingRequest {
    /// Build a request whose short identifiers and policy can be validated
    /// before any model call.
    pub fn new(
        passages: Vec<Passage>,
        model_dimensions: Vec<impl Into<String>>,
        server_dimensions: Vec<impl Into<String>>,
        expertise_mode: ExpertiseMode,
    ) -> Result<Self, PassageTaggingError> {
        if passages.is_empty() {
            return Err(PassageTaggingError::InvalidRequest(
                "at least one passage is required".to_string(),
            ));
        }
        let mut ids = HashSet::with_capacity(passages.len());
        for passage in &passages {
            validate_correlation_id(&passage.correlation_id)?;
            if passage.text.trim().is_empty() {
                return Err(PassageTaggingError::InvalidRequest(format!(
                    "passage '{}' has blank text",
                    passage.correlation_id
                )));
            }
            if !ids.insert(passage.correlation_id.clone()) {
                return Err(PassageTaggingError::InvalidRequest(format!(
                    "duplicate correlation ID '{}'",
                    passage.correlation_id
                )));
            }
        }
        let model_dimensions = normalized_unique(model_dimensions, "model dimensions")?;
        if model_dimensions.is_empty() {
            return Err(PassageTaggingError::InvalidRequest(
                "at least one model dimension is required".to_string(),
            ));
        }
        let server_dimensions = normalized_unique(server_dimensions, "server dimensions")?;
        if model_dimensions
            .iter()
            .any(|dimension| server_dimensions.contains(dimension))
        {
            return Err(PassageTaggingError::InvalidRequest(
                "model and server dimensions must be disjoint".to_string(),
            ));
        }
        Ok(Self {
            passages,
            model_dimensions,
            server_dimensions,
            expertise_mode,
        })
    }

    pub fn passages(&self) -> &[Passage] {
        &self.passages
    }

    pub fn expertise_mode(&self) -> ExpertiseMode {
        self.expertise_mode
    }
}

/// One validated model result, returned in request order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassageTag {
    pub correlation_id: String,
    pub dimensions: Vec<String>,
    pub candidate_terms: Vec<String>,
    pub expertise: Option<ExpertiseLevel>,
}

/// Typed failures at the shared model-protocol boundary.
#[derive(Debug, Error)]
pub enum PassageTaggingError {
    #[error("invalid passage-tagging request: {0}")]
    InvalidRequest(String),
    #[error(
        "HKASK_TEMPLATE_ROOT is not configured; required passage-tagging template must be deployed by the host"
    )]
    TemplateRootNotConfigured,
    #[error("template root does not exist or is not a directory: {0}")]
    MissingTemplateRoot(PathBuf),
    #[error("template path escapes the deployed template root: {0}")]
    TemplateTraversal(PathBuf),
    #[error("failed to read required template {path}: {source}")]
    TemplateRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("required template has invalid syntax: {0}")]
    TemplateSyntax(#[source] minijinja::Error),
    #[error("required template failed to render: {0}")]
    TemplateRender(#[source] minijinja::Error),
    #[error("required template rendered blank output")]
    BlankTemplateOutput,
    #[error("invalid passage-tagging response: {0}")]
    InvalidResponse(String),
}

/// Resolve the host-deployed template root and render the fixed tagging template.
pub fn render_deployed_tagging_prompt(
    request: &PassageTaggingRequest,
) -> Result<String, PassageTaggingError> {
    let root = std::env::var_os("HKASK_TEMPLATE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(PassageTaggingError::TemplateRootNotConfigured)?;
    render_named_template(&root, TEMPLATE_NAME, request, true)
}

/// Render the fixed deployed tagging template at an explicit host-resolved
/// root, with the same process-lifetime caching as the env-resolved variant.
///
/// The host process (the editor) threads the root from settings because the
/// env-var seam (`HKASK_TEMPLATE_ROOT`) only reaches MCP server child
/// processes — it is never set in the host process itself.
pub fn render_deployed_tagging_prompt_at(
    template_root: &Path,
    request: &PassageTaggingRequest,
) -> Result<String, PassageTaggingError> {
    render_named_template(template_root, TEMPLATE_NAME, request, true)
}

/// Render the fixed deployed passage-tagging template with strict variables.
pub fn render_tagging_prompt(
    template_root: &Path,
    request: &PassageTaggingRequest,
) -> Result<String, PassageTaggingError> {
    render_named_template(template_root, TEMPLATE_NAME, request, false)
}

fn render_named_template(
    template_root: &Path,
    template_name: &str,
    request: &PassageTaggingRequest,
    cache_deployed: bool,
) -> Result<String, PassageTaggingError> {
    if template_name.contains('/') || template_name.contains('\\') || template_name.contains("..") {
        return Err(PassageTaggingError::TemplateTraversal(
            template_root.join(template_name),
        ));
    }
    let canonical_root = template_root
        .canonicalize()
        .map_err(|_| PassageTaggingError::MissingTemplateRoot(template_root.to_path_buf()))?;
    if !canonical_root.is_dir() {
        return Err(PassageTaggingError::MissingTemplateRoot(canonical_root));
    }
    let path = canonical_root
        .join(TEMPLATE_DIR)
        .join(format!("{template_name}.j2"));
    let canonical_path =
        path.canonicalize()
            .map_err(|source| PassageTaggingError::TemplateRead {
                path: path.clone(),
                source,
            })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(PassageTaggingError::TemplateTraversal(canonical_path));
    }
    if !cache_deployed {
        return render_uncached(&canonical_path, request);
    }
    let cache = TEMPLATE_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !cache.contains_key(&canonical_path) && !cache.is_empty() {
        drop(cache);
        return render_uncached(&canonical_path, request);
    }
    if !cache.contains_key(&canonical_path) {
        let source = std::fs::read_to_string(&canonical_path).map_err(|source| {
            PassageTaggingError::TemplateRead {
                path: canonical_path.clone(),
                source,
            }
        })?;
        let source: &'static str = Box::leak(source.into_boxed_str());
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_template(TEMPLATE_NAME, source)
            .map_err(PassageTaggingError::TemplateSyntax)?;
        cache.insert(canonical_path.clone(), env);
    }
    let env = cache.get(&canonical_path).ok_or_else(|| {
        PassageTaggingError::InvalidRequest("deployed template cache insertion failed".to_string())
    })?;
    let template = env
        .get_template(TEMPLATE_NAME)
        .map_err(PassageTaggingError::TemplateSyntax)?;
    let context = serde_json::json!({
        "passages": request.passages,
        "model_dimensions": request.model_dimensions,
        "server_dimensions": request.server_dimensions,
        "expertise_mode": request.expertise_mode,
    });
    let rendered = template
        .render(minijinja::Value::from_serialize(&context))
        .map_err(PassageTaggingError::TemplateRender)?;
    let rendered = rendered.trim().to_string();
    if rendered.is_empty() {
        return Err(PassageTaggingError::BlankTemplateOutput);
    }
    Ok(rendered)
}

fn render_uncached(
    canonical_path: &Path,
    request: &PassageTaggingRequest,
) -> Result<String, PassageTaggingError> {
    let source = std::fs::read_to_string(canonical_path).map_err(|source| {
        PassageTaggingError::TemplateRead {
            path: canonical_path.to_path_buf(),
            source,
        }
    })?;
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template_owned(TEMPLATE_NAME, source)
        .map_err(PassageTaggingError::TemplateSyntax)?;
    let template = env
        .get_template(TEMPLATE_NAME)
        .map_err(PassageTaggingError::TemplateSyntax)?;
    let context = serde_json::json!({
        "passages": request.passages,
        "model_dimensions": request.model_dimensions,
        "server_dimensions": request.server_dimensions,
        "expertise_mode": request.expertise_mode,
    });
    let rendered = template
        .render(minijinja::Value::from_serialize(&context))
        .map_err(PassageTaggingError::TemplateRender)?;
    let rendered = rendered.trim().to_string();
    if rendered.is_empty() {
        return Err(PassageTaggingError::BlankTemplateOutput);
    }
    Ok(rendered)
}

#[derive(Debug, Deserialize)]
struct WireTag(String, Vec<String>, Vec<String>, Option<String>);

/// Validate and correlate one model response. The outer array and four-field
/// tuple are mandatory; singleton objects and positional compatibility are not
/// accepted.
pub fn parse_tagging_response(
    request: &PassageTaggingRequest,
    response: &str,
) -> Result<Vec<PassageTag>, PassageTaggingError> {
    let extracted = hkask_types::json_extract::extract_json_from_response(response);
    let wire: Vec<WireTag> = serde_json::from_str(&extracted).map_err(|error| {
        PassageTaggingError::InvalidResponse(format!(
            "expected an array of four-field tuples: {error}"
        ))
    })?;
    if wire.len() != request.passages.len() {
        return Err(PassageTaggingError::InvalidResponse(format!(
            "expected {} results, got {}",
            request.passages.len(),
            wire.len()
        )));
    }

    let expected: HashSet<&str> = request
        .passages
        .iter()
        .map(|passage| passage.correlation_id.as_str())
        .collect();
    let mut correlated = HashMap::with_capacity(wire.len());
    for WireTag(correlation_id, dimensions, candidate_terms, expertise) in wire {
        if !expected.contains(correlation_id.as_str()) {
            return Err(PassageTaggingError::InvalidResponse(format!(
                "unknown correlation ID '{correlation_id}'"
            )));
        }
        if correlated.contains_key(&correlation_id) {
            return Err(PassageTaggingError::InvalidResponse(format!(
                "duplicate correlation ID '{correlation_id}'"
            )));
        }
        let dimensions = validate_dimensions(dimensions, &request.model_dimensions)?;
        let candidate_terms = validate_candidate_terms(candidate_terms)?;
        let expertise = validate_expertise(expertise, request.expertise_mode)?;
        correlated.insert(
            correlation_id.clone(),
            PassageTag {
                correlation_id,
                dimensions,
                candidate_terms,
                expertise,
            },
        );
    }

    request
        .passages
        .iter()
        .map(|passage| {
            correlated.remove(&passage.correlation_id).ok_or_else(|| {
                PassageTaggingError::InvalidResponse(format!(
                    "omitted correlation ID '{}'",
                    passage.correlation_id
                ))
            })
        })
        .collect()
}

fn validate_correlation_id(id: &str) -> Result<(), PassageTaggingError> {
    if id.is_empty()
        || id.len() > MAX_CORRELATION_ID_LEN
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(PassageTaggingError::InvalidRequest(format!(
            "correlation ID '{id}' must be 1-{MAX_CORRELATION_ID_LEN} ASCII letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

fn normalized_unique(
    values: Vec<impl Into<String>>,
    field: &str,
) -> Result<Vec<String>, PassageTaggingError> {
    let values: Vec<String> = values
        .into_iter()
        .map(Into::into)
        .map(|value| value.trim().to_string())
        .collect();
    if values.iter().any(String::is_empty) {
        return Err(PassageTaggingError::InvalidRequest(format!(
            "{field} contain a blank value"
        )));
    }
    let unique: HashSet<&str> = values.iter().map(String::as_str).collect();
    if unique.len() != values.len() {
        return Err(PassageTaggingError::InvalidRequest(format!(
            "{field} contain duplicates"
        )));
    }
    Ok(values)
}

fn validate_dimensions(
    dimensions: Vec<String>,
    allowlist: &[String],
) -> Result<Vec<String>, PassageTaggingError> {
    let dimensions: Vec<String> = dimensions
        .into_iter()
        .map(|value| value.trim().to_string())
        .collect();
    let unique: HashSet<&str> = dimensions.iter().map(String::as_str).collect();
    if unique.len() != dimensions.len()
        || dimensions
            .iter()
            .any(|dimension| !allowlist.contains(dimension))
    {
        return Err(PassageTaggingError::InvalidResponse(
            "dimensions must be unique members of the requested allowlist".to_string(),
        ));
    }
    Ok(dimensions)
}

fn validate_candidate_terms(
    candidate_terms: Vec<String>,
) -> Result<Vec<String>, PassageTaggingError> {
    let candidate_terms: Vec<String> = candidate_terms
        .into_iter()
        .map(|term| term.trim().to_string())
        .collect();
    let unique: HashSet<&str> = candidate_terms.iter().map(String::as_str).collect();
    if !(MIN_CANDIDATE_TERMS..=MAX_CANDIDATE_TERMS).contains(&candidate_terms.len())
        || candidate_terms.iter().any(String::is_empty)
        || unique.len() != candidate_terms.len()
    {
        return Err(PassageTaggingError::InvalidResponse(format!(
            "candidate terms must contain {MIN_CANDIDATE_TERMS}-{MAX_CANDIDATE_TERMS} unique nonblank strings"
        )));
    }
    Ok(candidate_terms)
}

fn validate_expertise(
    expertise: Option<String>,
    mode: ExpertiseMode,
) -> Result<Option<ExpertiseLevel>, PassageTaggingError> {
    match (mode, expertise.as_deref()) {
        (ExpertiseMode::ServerDerived, None) => Ok(None),
        (ExpertiseMode::ServerDerived, Some(_)) => Err(PassageTaggingError::InvalidResponse(
            "expertise must be null in server-derived mode".to_string(),
        )),
        (ExpertiseMode::ModelAssigned, Some("practitioner")) => {
            Ok(Some(ExpertiseLevel::Practitioner))
        }
        (ExpertiseMode::ModelAssigned, Some("analyst")) => Ok(Some(ExpertiseLevel::Analyst)),
        (ExpertiseMode::ModelAssigned, Some("researcher")) => Ok(Some(ExpertiseLevel::Researcher)),
        (ExpertiseMode::ModelAssigned, _) => Err(PassageTaggingError::InvalidResponse(
            "expertise must be practitioner, analyst, or researcher in model-assigned mode"
                .to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: ExpertiseMode) -> PassageTaggingRequest {
        PassageTaggingRequest::new(
            vec![
                Passage::new("p0", "first passage"),
                Passage::new("p1", "second passage"),
            ],
            vec!["what", "why"],
            vec!["how"],
            mode,
        )
        .expect("valid request")
    }

    /// expect: "Passage tags remain attached to their requested passages even when the model reorders results."
    /// [P3] Motivating: Generative Space — reusable semantic metadata stays correlated with its source passage.
    /// [P1] Constraining: exact identifiers prevent one passage from receiving another passage's tags.
    /// pre: the response contains every requested identifier exactly once.
    /// post: results are returned in request order with tags selected by identifier, not position.
    #[test]
    fn reordered_results_correlate_by_exact_id() {
        let request = request(ExpertiseMode::ServerDerived);
        let response = r#"[["p1",["what"],["second","passage","result"],null],["p0",["why"],["first","passage","result"],null]]"#;
        let tags = parse_tagging_response(&request, response).expect("response correlates");
        assert_eq!(
            tags.iter()
                .map(|tag| tag.correlation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["p0", "p1"]
        );
        assert_eq!(tags[0].candidate_terms[0], "first");
        assert_eq!(tags[1].candidate_terms[0], "second");
    }

    #[test]
    fn exact_id_set_and_tuple_shape_are_mandatory() {
        let request = request(ExpertiseMode::ServerDerived);
        for response in [
            r#"[["p0",[],["a","b","c"],null],["invented",[],["a","b","c"],null]]"#,
            r#"[["p0",[],["a","b","c"],null],["p0",[],["a","b","c"],null]]"#,
            r#"[["p0",[],["a","b","c"],null]]"#,
            r#"{"correlation_id":"p0"}"#,
            r#"[[[],["a","b","c"],null],[[],["a","b","c"],null]]"#,
        ] {
            assert!(
                parse_tagging_response(&request, response).is_err(),
                "{response}"
            );
        }
    }

    #[test]
    fn dimensions_candidates_and_expertise_obey_request_policy() {
        let server = request(ExpertiseMode::ServerDerived);
        assert!(
            parse_tagging_response(
                &server,
                r#"[["p0",["who"],["a","b","c"],null],["p1",[],["d","e","f"],null]]"#
            )
            .is_err()
        );
        assert!(
            parse_tagging_response(
                &server,
                r#"[["p0",[],["a","b"],null],["p1",[],["d","e","f"],null]]"#
            )
            .is_err()
        );
        assert!(
            parse_tagging_response(
                &server,
                r#"[["p0",[],["a","b","c"],"analyst"],["p1",[],["d","e","f"],null]]"#
            )
            .is_err()
        );

        let model = request(ExpertiseMode::ModelAssigned);
        let tags = parse_tagging_response(
            &model,
            r#"[["p0",[],["a","b","c"],"researcher"],["p1",[],["d","e","f"],"analyst"]]"#,
        )
        .expect("valid model expertise");
        assert_eq!(tags[0].expertise, Some(ExpertiseLevel::Researcher));
        assert!(
            parse_tagging_response(
                &model,
                r#"[["p0",[],["a","b","c"],null],["p1",[],["d","e","f"],"analyst"]]"#
            )
            .is_err()
        );
    }

    fn write_template(root: &Path, source: &str) {
        let directory = root.join(TEMPLATE_DIR);
        std::fs::create_dir_all(&directory).expect("create template directory");
        std::fs::write(directory.join(format!("{TEMPLATE_NAME}.j2")), source)
            .expect("write template");
    }

    #[test]
    fn required_template_failures_are_typed() {
        let missing = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            render_tagging_prompt(
                &missing.path().join("absent"),
                &request(ExpertiseMode::ServerDerived)
            ),
            Err(PassageTaggingError::MissingTemplateRoot(_))
        ));
        assert!(matches!(
            render_tagging_prompt(missing.path(), &request(ExpertiseMode::ServerDerived)),
            Err(PassageTaggingError::TemplateRead { .. })
        ));

        let syntax = tempfile::tempdir().expect("tempdir");
        write_template(syntax.path(), "{% if %}");
        assert!(matches!(
            render_tagging_prompt(syntax.path(), &request(ExpertiseMode::ServerDerived)),
            Err(PassageTaggingError::TemplateSyntax(_))
        ));

        let undefined = tempfile::tempdir().expect("tempdir");
        write_template(undefined.path(), "{{ missing_variable }}");
        assert!(matches!(
            render_tagging_prompt(undefined.path(), &request(ExpertiseMode::ServerDerived)),
            Err(PassageTaggingError::TemplateRender(_))
        ));

        let blank = tempfile::tempdir().expect("tempdir");
        write_template(blank.path(), "{# blank #}");
        assert!(matches!(
            render_tagging_prompt(blank.path(), &request(ExpertiseMode::ServerDerived)),
            Err(PassageTaggingError::BlankTemplateOutput)
        ));
    }

    #[test]
    fn template_name_traversal_is_rejected() {
        let root = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            render_named_template(
                root.path(),
                "../secret",
                &request(ExpertiseMode::ServerDerived),
                false,
            ),
            Err(PassageTaggingError::TemplateTraversal(_))
        ));
    }

    /// The host-threaded variant must render the same deployed template the
    /// env-resolved path uses — the editor process never has the env var
    /// set, so in-process tagging depends entirely on this entry point.
    #[test]
    fn explicit_root_variant_renders_the_deployed_template() {
        let root = tempfile::tempdir().expect("tempdir");
        write_template(root.path(), "SENTINEL-ROOT {{ passages[0].text }}");
        let request = request(ExpertiseMode::ModelAssigned);
        for attempt in 0..2 {
            let rendered =
                render_deployed_tagging_prompt_at(root.path(), &request).expect("render");
            assert!(
                rendered.starts_with("SENTINEL-ROOT"),
                "attempt {attempt}: prompt must come from the explicit root's template"
            );
            assert!(
                rendered.contains("first passage"),
                "attempt {attempt}: passage text reaches the template"
            );
        }
    }
}
