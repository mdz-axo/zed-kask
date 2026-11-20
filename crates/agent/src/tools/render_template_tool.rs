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
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|e| RenderTemplateToolOutput::Error {
                    error: format!("failed to receive input: {e}"),
                })?;
            let project = event_stream.thread().and_then(|weak| {
                cx.update(|cx| {
                    weak.upgrade()
                        .map(|thread| thread.read(cx).project().clone())
                })
            });
            render_registry_template_in_project(&input, project.as_ref(), cx)
                .await
                .map(|text| RenderTemplateToolOutput::Rendered { text })
                .map_err(|error| RenderTemplateToolOutput::Error { error })
        })
    }
}

enum ProjectContextPathResolution {
    WorktreeFile {
        abs_path: std::path::PathBuf,
        canonical_roots: Vec<std::path::PathBuf>,
    },
    External,
}

async fn resolve_project_context_path(
    project: &gpui::Entity<project::Project>,
    raw_path: &str,
    cx: &mut gpui::AsyncApp,
) -> Result<ProjectContextPathResolution, String> {
    use settings::Settings as _;

    let fs = project.read_with(cx, |project, _cx| project.fs().clone());
    let canonical_roots =
        super::tool_permissions::canonicalize_worktree_roots(project, &fs, cx).await;

    project.read_with(cx, |project, cx| {
        let expanded = super::tool_permissions::expand_home_prefix(std::path::Path::new(raw_path));
        let lookup_str;
        let lookup_path: &str = if project.find_project_path(raw_path, cx).is_some() {
            raw_path
        } else if let Some(ref exp) = expanded
            && project.find_project_path(exp, cx).is_some()
        {
            lookup_str = exp.to_string_lossy().into_owned();
            lookup_str.as_str()
        } else {
            if expanded.is_none() {
                return Err(format!(
                    "context_path '{raw_path}': path is not in any open project worktree"
                ));
            }
            return Ok(ProjectContextPathResolution::External);
        };

        let resolved = super::tool_permissions::resolve_project_path(
            project,
            lookup_path,
            &canonical_roots,
            cx,
        )
        .map_err(|error| format!("context_path '{raw_path}': {error}"))?;

        let project_path = match resolved {
            super::tool_permissions::ResolvedProjectPath::Safe(project_path) => project_path,
            super::tool_permissions::ResolvedProjectPath::SymlinkEscape { .. } => {
                return Err(format!(
                    "context_path '{raw_path}': symlinks escaping the project worktree are not permitted"
                ));
            }
        };

        let global_settings = project::WorktreeSettings::get_global(cx);
        if global_settings.is_path_private(&project_path.path) {
            return Err(format!(
                "context_path '{raw_path}': matches global `private_files` setting"
            ));
        }
        if global_settings.is_path_excluded(&project_path.path) {
            return Err(format!(
                "context_path '{raw_path}': matches global `file_scan_exclusions` setting"
            ));
        }

        let worktree_settings =
            project::WorktreeSettings::get(Some((&project_path).into()), cx);
        if worktree_settings.is_path_private(&project_path.path) {
            return Err(format!(
                "context_path '{raw_path}': matches worktree `private_files` setting"
            ));
        }
        if worktree_settings.is_path_excluded(&project_path.path) {
            return Err(format!(
                "context_path '{raw_path}': matches worktree `file_scan_exclusions` setting"
            ));
        }

        let abs_path = project.absolute_path(&project_path, cx).ok_or_else(|| {
            format!("context_path '{raw_path}': failed to resolve worktree path")
        })?;

        Ok(ProjectContextPathResolution::WorktreeFile {
            abs_path,
            canonical_roots,
        })
    })
}

async fn render_registry_template_in_project(
    input: &RenderTemplateToolInput,
    project: Option<&gpui::Entity<project::Project>>,
    cx: &mut gpui::AsyncApp,
) -> Result<String, String> {
    let base_path = crate::template_base_path().ok_or_else(|| {
        "Template base path not configured. The registry templates directory is wired at startup."
            .to_string()
    })?;

    let mut context = input.context.clone();
    if let Some(raw_path) = &input.context_path {
        let file_context = match project {
            Some(project) => match resolve_project_context_path(project, raw_path, cx).await? {
                ProjectContextPathResolution::WorktreeFile {
                    abs_path,
                    canonical_roots,
                } => load_context_file_with_roots(&abs_path.to_string_lossy(), &canonical_roots)?,
                ProjectContextPathResolution::External => load_context_file(raw_path)?,
            },
            None => load_context_file(raw_path)?,
        };
        for (key, value) in file_context {
            context.entry(key).or_insert(value);
        }
    }

    render_template_with_resolved_context(base_path, &input.template_ref, &context)
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

    render_template_with_resolved_context(base_path, &input.template_ref, &context)
}

fn render_template_with_resolved_context(
    base_path: &std::path::Path,
    template_ref: &str,
    context: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<String, String> {
    // Resolve the template file, trying ref as-is, then .j2.
    let content = read_template_file(base_path, template_ref)?;

    validate_contract_inputs(&content, context).map_err(|error| {
        format!("Template contract validation failed for '{template_ref}': {error}",)
    })?;

    // Strip YAML frontmatter (--- delimited header).
    let template_body = strip_frontmatter(&content);

    // Build minijinja environment and render. The loader resolves
    // `{% include %}` names against the registry root — a bare
    // render_str environment has no templates, so includes fail.
    let mut env = minijinja::Environment::new();
    bind_registry_loader(&mut env, base_path);
    env.render_str(&template_body, context)
        .map_err(|e| format!("Template rendering failed: {e}"))
}

#[cfg(unix)]
fn current_euid() -> u32 {
    // SAFETY: `geteuid` is thread-safe, takes no arguments, and cannot fail.
    unsafe { libc::geteuid() }
}

fn is_virtual_or_device_path(path: &std::path::Path) -> bool {
    ["/proc", "/sys", "/dev"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn is_sensitive_component(name: &std::ffi::OsStr) -> bool {
    let local_settings = paths::local_settings_folder_name();
    util::paths::component_matches_ignore_ascii_case(name, local_settings)
        || [".ssh", ".aws", ".gnupg", ".git"]
            .iter()
            .any(|blocked| util::paths::component_matches_ignore_ascii_case(name, blocked))
}

fn is_sensitive_filename(name: &std::ffi::OsStr) -> bool {
    let lowered = name.to_string_lossy().to_ascii_lowercase();
    lowered == ".env"
        || lowered.starts_with(".env.")
        || lowered.ends_with(".pem")
        || lowered.ends_with(".key")
        || lowered.ends_with(".p12")
        || lowered.ends_with(".pfx")
        || lowered == "id_rsa"
        || lowered.starts_with("id_rsa.")
        || lowered == "id_ed25519"
        || lowered.starts_with("id_ed25519.")
}

fn is_disallowed_temp_component(name: &std::ffi::OsStr) -> bool {
    let text = name.to_string_lossy();
    text.starts_with("zed-agent-output-")
        || text.starts_with("zed-crash-handler-")
        || text.starts_with("kask-inference-")
}

fn reject_sensitive_or_virtual_path(
    candidate: &std::path::Path,
    display_path: &str,
) -> Result<(), String> {
    if is_virtual_or_device_path(candidate) {
        return Err(format!(
            "context_path '{display_path}': virtual and device filesystems (/proc, /sys, /dev) are not permitted"
        ));
    }

    for component in candidate.components() {
        if let std::path::Component::Normal(segment) = component
            && is_sensitive_component(segment)
        {
            return Err(format!(
                "context_path '{display_path}': sensitive directory '{}' is not permitted",
                segment.to_string_lossy()
            ));
        }
    }

    if let Some(file_name) = candidate.file_name()
        && is_sensitive_filename(file_name)
    {
        return Err(format!(
            "context_path '{display_path}': sensitive file '{}' is not permitted",
            file_name.to_string_lossy()
        ));
    }

    let normalized_config = util::paths::normalize_path(paths::config_dir());
    if candidate.starts_with(&normalized_config)
        || paths::config_dir()
            .canonicalize()
            .is_ok_and(|canonical_cfg| candidate.starts_with(&canonical_cfg))
    {
        return Err(format!(
            "context_path '{display_path}': configuration directory files are not permitted"
        ));
    }

    let home = paths::home_dir();
    for secret_dir in [".ssh", ".aws", ".gnupg"] {
        let secret_path = util::paths::normalize_path(&home.join(secret_dir));
        if candidate.starts_with(&secret_path) {
            return Err(format!(
                "context_path '{display_path}': '{secret_dir}' files are not permitted"
            ));
        }
    }

    Ok(())
}

struct AdmittedContextRoot {
    canonical_root: std::path::PathBuf,
    relative_path: std::path::PathBuf,
    is_scratch_temp: bool,
}

fn match_admitted_root(
    normalized: &std::path::Path,
    raw_root: &std::path::Path,
    is_scratch_temp: bool,
) -> Option<AdmittedContextRoot> {
    let canonical_root = raw_root.canonicalize().ok()?;
    let lexical_root = util::paths::normalize_path(raw_root);
    let relative = normalized
        .strip_prefix(&lexical_root)
        .or_else(|_| normalized.strip_prefix(&canonical_root))
        .ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(AdmittedContextRoot {
        canonical_root,
        relative_path: relative.to_path_buf(),
        is_scratch_temp,
    })
}

fn resolve_admitted_context_root(
    normalized: &std::path::Path,
    allowed_worktree_roots: &[std::path::PathBuf],
) -> Option<AdmittedContextRoot> {
    for wt_root in allowed_worktree_roots {
        if let Some(matched) = match_admitted_root(normalized, wt_root, false) {
            return Some(matched);
        }
    }
    if let Some(matched) =
        match_admitted_root(normalized, &agent_skills::global_skills_dir(), false)
    {
        return Some(matched);
    }
    if let Some(matched) = match_admitted_root(
        normalized,
        &super::tool_permissions::kask_artifacts_dir(),
        false,
    ) {
        return Some(matched);
    }
    match_admitted_root(normalized, &std::env::temp_dir(), true)
}

fn read_bounded_context_json<R: std::io::Read>(
    reader: R,
    path: &str,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    reader
        .take((MAX_CONTEXT_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("context_path '{path}': {error}"))?;
    if bytes.len() > MAX_CONTEXT_FILE_BYTES {
        return Err(format!(
            "context_path '{path}' exceeds the {MAX_CONTEXT_FILE_BYTES} byte bound"
        ));
    }
    let text =
        String::from_utf8(bytes).map_err(|error| format!("context_path '{path}': {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("context_path '{path}': {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("context_path '{path}' must contain a JSON object document"))?;
    Ok(object.clone().into_iter().collect())
}

/// Load a context object from a JSON file (the `context_path` contract).
///
/// Enforces strict root containment (non-private project worktrees, global
/// skills dir, Kask visible artifacts dir, or user-owned `.json` scratch files
/// under `std::env::temp_dir()`), sensitive-path rejection, symlink-free
/// ancestor + `O_NOFOLLOW` leaf verification, and single-descriptor
/// `Read::take(MAX_CONTEXT_FILE_BYTES + 1)` bounded reading.
fn load_context_file(
    path: &str,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    load_context_file_with_roots(path, &[])
}

fn load_context_file_with_roots(
    path: &str,
    allowed_worktree_roots: &[std::path::PathBuf],
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    if path.trim().is_empty() || path.contains('\0') {
        return Err(format!("context_path '{path}' is invalid"));
    }

    let raw_path = std::path::Path::new(path);
    reject_sensitive_or_virtual_path(raw_path, path)?;

    let Some(expanded) = super::tool_permissions::expand_home_prefix(raw_path) else {
        return Err(format!(
            "context_path '{path}' must be an absolute, '~'-prefixed, or project-worktree path"
        ));
    };
    reject_sensitive_or_virtual_path(&expanded, path)?;

    let normalized = util::paths::normalize_path(&expanded);
    reject_sensitive_or_virtual_path(&normalized, path)?;

    let Some(admitted) = resolve_admitted_context_root(&normalized, allowed_worktree_roots) else {
        return Err(format!(
            "context_path '{path}' is outside permitted directories (project worktrees, global skills, Kask artifacts, or temp scratch .json files)"
        ));
    };

    let mut normal_segments: Vec<std::ffi::OsString> = Vec::new();
    for component in admitted.relative_path.components() {
        match component {
            std::path::Component::Normal(seg) => normal_segments.push(seg.to_os_string()),
            std::path::Component::CurDir => {}
            _ => {
                return Err(format!(
                    "context_path '{path}': path traversal components are not permitted"
                ));
            }
        }
    }
    let Some((leaf_name, ancestor_segments)) = normal_segments.split_last() else {
        return Err(format!("context_path '{path}' is not a regular file"));
    };

    if is_sensitive_filename(leaf_name) {
        return Err(format!(
            "context_path '{path}': sensitive file '{}' is not permitted",
            leaf_name.to_string_lossy()
        ));
    }

    if admitted.is_scratch_temp {
        if is_disallowed_temp_component(leaf_name)
            || ancestor_segments
                .iter()
                .any(|seg| is_disallowed_temp_component(seg))
        {
            return Err(format!(
                "context_path '{path}': internal runtime/output temp files are not permitted"
            ));
        }
        let has_json_ext = std::path::Path::new(leaf_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if !has_json_ext {
            return Err(format!(
                "context_path '{path}': scratch temp files must have a .json extension"
            ));
        }
    }

    // Walk every ancestor inside the admitted root via `symlink_metadata` so no
    // intermediate directory symlink can escape containment.
    let mut cursor = admitted.canonical_root.clone();
    for segment in ancestor_segments {
        if is_sensitive_component(segment) {
            return Err(format!(
                "context_path '{path}': sensitive directory '{}' is not permitted",
                segment.to_string_lossy()
            ));
        }
        cursor.push(segment);
        let dir_meta = std::fs::symlink_metadata(&cursor)
            .map_err(|error| format!("context_path '{path}': {error}"))?;
        if dir_meta.file_type().is_symlink() {
            return Err(format!(
                "context_path '{path}': symlink directory component '{}' is not permitted",
                segment.to_string_lossy()
            ));
        }
        if !dir_meta.is_dir() {
            return Err(format!(
                "context_path '{path}': parent '{}' is not a directory",
                segment.to_string_lossy()
            ));
        }
        #[cfg(unix)]
        if admitted.is_scratch_temp {
            use std::os::unix::fs::MetadataExt as _;
            if dir_meta.uid() != current_euid() {
                return Err(format!(
                    "context_path '{path}': temp directory '{}' is not owned by the current user",
                    segment.to_string_lossy()
                ));
            }
        }
    }

    let canonical_parent = cursor
        .canonicalize()
        .map_err(|error| format!("context_path '{path}': {error}"))?;
    if !canonical_parent.starts_with(&admitted.canonical_root) {
        return Err(format!(
            "context_path '{path}': parent directory escaped permitted root"
        ));
    }
    reject_sensitive_or_virtual_path(&canonical_parent, path)?;

    let target_file = canonical_parent.join(leaf_name);
    let leaf_lstat = std::fs::symlink_metadata(&target_file)
        .map_err(|error| format!("context_path '{path}': {error}"))?;
    if leaf_lstat.file_type().is_symlink() {
        return Err(format!("context_path '{path}': symlinks are not permitted"));
    }

    let mut open_options = std::fs::OpenOptions::new();
    open_options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open_options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = open_options
        .open(&target_file)
        .map_err(|error| format!("context_path '{path}': {error}"))?;
    let file_meta = file
        .metadata()
        .map_err(|error| format!("context_path '{path}': {error}"))?;
    if !file_meta.is_file() {
        return Err(format!("context_path '{path}' is not a regular file"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if (file_meta.dev(), file_meta.ino()) != (leaf_lstat.dev(), leaf_lstat.ino()) {
            return Err(format!(
                "context_path '{path}': file identity changed during open"
            ));
        }
        if admitted.is_scratch_temp && file_meta.uid() != current_euid() {
            return Err(format!(
                "context_path '{path}': scratch file is not owned by the current user"
            ));
        }
    }

    read_bounded_context_json(file, path)
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
mod corpus_sweep_tests;
#[cfg(test)]
mod finalizer_handoff_tests;
#[cfg(test)]
mod tests;
