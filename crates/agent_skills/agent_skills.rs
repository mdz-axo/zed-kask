use anyhow::{Context as _, Result};
use const_format::formatcp;
use fs::Fs;
use futures::StreamExt;
use gpui::{App, Global, SharedString};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use util::paths::component_matches_ignore_ascii_case;

/// First segment of the project-local skills directory path: `.agents`.
/// Used only for project-local skills (`.agents/skills/` inside a worktree).
/// Global skills use `{kask_data_dir}/skills/` (D28 — Standardized Artifact Storage).
pub const AGENTS_DIR_NAME: &str = ".agents";

/// Second segment of the skills directory path: `skills`.
pub const SKILLS_DIR_NAME: &str = "skills";

/// The canonical list of core skill names. Core skills are always-on,
/// re-seeded on every startup (overwriting user edits), locked against
/// editing, and undisableable. They cannot be shadowed by project-local
/// skills of the same name.
///
/// This list is the single source of truth for which skills are core. It is
/// consumed by:
/// - `seed_shipped_skills` — to decide whether to overwrite or seed-once.
/// - `kask_bridge::seed_registry_to_disk` — same, for manifest.yaml + .j2
///   templates.
/// - `apply_skill_overrides` — to enforce unshadowability.
/// - `skill_tool` — to skip the authorization prompt.
/// - `settings_ui::skills_setup` — to render core skills with a distinct
///   visual treatment and hide edit/delete controls.
///
/// A skill is core if it meets ALL of:
/// 1. System-critical: the skill system, agent loop, curator, or an MCP
///    server/panel depends on it being present and unmodified.
/// 2. Called autonomously by code, or hard-delegated to by another core
///    skill, or named in the Curator's static context as an anchored
///    methodology.
/// 3. All delegation targets (hard or optional) of a core skill must also
///    be core — an editable delegate is a backdoor around consent/trust.
pub const CORE_SKILL_NAMES: &[&str] = &[
    // Skill-authoring + maintenance
    "create-skill",
    "skill-bundler",
    "skill-discovery",
    "skill-logic-audit",
    "skill-maintenance",
    "skill-router",
    // Curator methodologies
    "metacognition",
    "pragmatic-cybernetics",
    "pragmatic-semantics",
    "superforecasting",
    // Curator human-in-the-loop review
    "algedonic-review",
    "gemba-walk",
    // Universal quality gates
    "code-review",
    "essentialist",
    "refactor-architecture",
    // Essentialist delegates
    "deep-module",
    "coding-guidelines",
    // skill-router integration
    "task-breakdown",
    // MCP server / panel invocations
    "swarm-compose-guide",
    "swarm-intelligence",
    "swarm-steering",
    "kanban-task-management",
    // code-review optional delegates (Q16: editable delegate = backdoor)
    "kali-audit",
    "bug-hunt",
];

/// Returns `true` if the given skill name is a core skill.
pub fn is_core_skill(name: &str) -> bool {
    CORE_SKILL_NAMES.contains(&name)
}

/// Returns `true` if the given skill name is reserved — i.e. it is the name
/// of a core skill and cannot be used by a user-authored skill. A user
/// skill (frontmatter `core: false` or missing) whose name is reserved is
/// refused at load time and at save time, so a hand-edited or
/// marketplace-installed file can never usurp a core skill's identity.
/// Legitimate core skills (frontmatter `core: true`) are exempt — they
/// are the only skills allowed to bear a reserved name.
pub fn is_reserved_skill_name(name: &str) -> bool {
    is_core_skill(name)
}

/// User-facing display form of the global skills directory path.
///
/// zed-kask: D28 — global skills live under the kask data root
/// (`~/.local/share/hkask/skills/`), not the app data root.
#[cfg(target_os = "windows")]
pub const GLOBAL_SKILLS_DIR_DISPLAY: &str = r"%LOCALAPPDATA%\hkask\skills";
#[cfg(not(target_os = "windows"))]
pub const GLOBAL_SKILLS_DIR_DISPLAY: &str = "~/.local/share/hkask/skills";

/// Opaque identifier for the project scope a skill was loaded from.
///
/// `agent_skills` is a leaf crate and intentionally does not depend on
/// `worktree`. Callers (e.g. the `agent` crate) construct these from
/// `worktree::WorktreeId::to_usize()` and recover the original ID via
/// `worktree::WorktreeId::from_usize()` when needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkillScopeId(pub usize);

/// Cap on concurrent filesystem operations during skill discovery and loading.
/// Without this bound, a `.agents/skills` directory containing thousands of
/// entries would fan out an equally large number of concurrent OS-level I/O
/// operations, potentially exhausting file descriptors or stalling the app.
const SKILL_IO_CONCURRENCY: usize = 16;

/// Maximum size for a single SKILL.md file (100KB)
pub const MAX_SKILL_FILE_SIZE: usize = 100 * 1024;

/// Maximum total size for skill descriptions in system prompt (50KB)
pub const MAX_SKILL_DESCRIPTIONS_SIZE: usize = 50 * 1024;

/// The name of the skill definition file
pub const SKILL_FILE_NAME: &str = "SKILL.md";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SkillLoadWarning {
    DescriptionTooLong { actual_len: usize, max_len: usize },
}

impl SkillLoadWarning {
    pub fn message(&self) -> String {
        match self {
            Self::DescriptionTooLong {
                actual_len,
                max_len,
            } => format!(
                "Skill description is {actual_len} bytes, exceeding the {max_len}-byte limit. The skill was loaded, but long descriptions may consume more model-context tokens."
            ),
        }
    }
}

/// Represents a loaded skill with all its metadata and content.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    /// Absolute path to the skill directory
    pub directory_path: PathBuf,
    /// Absolute path to the SKILL.md file
    pub skill_file_path: PathBuf,
    /// Non-fatal issues found while loading this skill.
    pub load_warnings: Vec<SkillLoadWarning>,
    /// When `true`, this skill is hidden from the model's catalog and the
    /// `skill` tool refuses to load it. The user can still invoke it as a
    /// slash command.
    pub disable_model_invocation: bool,
    /// Skill names this skill depends on. Checked at invocation time so the
    /// user gets a clear error before tokens are spent on a cascade that
    /// will fail mid-execution.
    pub dependencies: Vec<String>,
    /// When `true`, this skill is a core skill: always-on, re-seeded on every
    /// startup, locked against editing, undisableable, and unshadowable.
    /// See `SkillMetadata::core` for the full contract.
    pub core: bool,
}

/// Indicates where a skill was loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// From the global skills directory (zed-kask D28: `{kask_data_dir}/skills/`).
    Global,
    /// From {project}/.agents/skills/
    ProjectLocal {
        worktree_id: SkillScopeId,
        worktree_root_name: Arc<str>,
    },
    /// Installed from the kask marketplace. Lives under
    /// `{global_skills_dir}/_marketplace/{source_user}/{skill_name}/` so it
    /// never overwrites a locally-authored skill of the same name. The
    /// `original_skill_id` is the canonical id (`{source_user}/{skill_name}`)
    /// the marketplace catalog indexes it under.
    // zed-kask: marketplace-installed skills are a new source variant distinct
    // from `Global`. Precedence is intentionally *lower* than `Global` so a
    // user's locally-authored skill of the same name shadows the marketplace
    // copy (see `test_skill_source_public_precedence_between_built_in_and_global`).
    Public {
        source_user: Arc<str>,
        original_skill_id: Arc<str>,
    },
}

impl SkillSource {
    /// Precedence for resolving same-named skills. Higher values shadow
    /// lower ones: `ProjectLocal` > `Global` > `Public`. Two sources
    /// returning equal precedence (e.g. two project-local skills from
    /// different worktrees) leave the winner up to the caller, which by
    /// convention keeps the first one in iteration order.
    ///
    /// Adding a new `SkillSource` variant should be a one-line change
    /// here — every consumer routes through this method so the hierarchy
    /// stays in sync.
    // zed-kask: `Public` (marketplace-installed) sits below `Global` so a
    // local skill of the same name wins.
    pub fn precedence(&self) -> u8 {
        match self {
            Self::Public { .. } => 1,
            Self::Global => 2,
            Self::ProjectLocal { .. } => 3,
        }
    }

    /// Scope prefix used in the `/<prefix>:<name>` slash-command
    /// syntax that the autocomplete popup inserts. Global skills use
    /// an empty prefix (so the inserted text is `/:<name>`), and
    /// project-local skills use their worktree root name (so the
    /// inserted text is `/<worktree>:<name>`).
    ///
    /// Using an empty prefix for globals rather than a literal
    /// `global` means a worktree literally named `global` is no
    /// longer ambiguous with the global source: the global skill is
    /// invoked as `/:<name>`, and the worktree's skill is invoked as
    /// `/global:<name>`. The two grammars never collide on the
    /// inserted text.
    /// Human-readable label for this source, used in the UI to
    /// distinguish skills from different origins.
    //
    // zed-kask: `Public` returns the namespaced `"{source_user}/{skill_name}"`
    // form so the marketplace listing identity is preserved on installed
    // skills. Pinned by `test_skill_source_public_display_label_is_namespaced`.
    pub fn display_label(&self) -> &str {
        match self {
            Self::Global => "global",
            Self::ProjectLocal {
                worktree_root_name, ..
            } => worktree_root_name.as_ref(),
            Self::Public {
                original_skill_id, ..
            } => original_skill_id.as_ref(),
        }
    }

    pub fn scope_prefix(&self) -> &str {
        match self {
            Self::Global | Self::Public { .. } => "",
            Self::ProjectLocal {
                worktree_root_name, ..
            } => worktree_root_name.as_ref(),
        }
    }

    /// Whether this source matches the given scope qualifier from a
    /// `/<scope>:<name>` slash command. The empty scope is reserved
    /// for global skills; non-empty scopes match a project-local
    /// skill whose worktree root name equals the scope.
    ///
    /// Hand-typed `/global:<name>` is NOT treated as an alias for
    /// `/:<name>`. It looks for a project-local skill from a worktree
    /// named `global` and fails if none exists. The popup always
    /// inserts the unambiguous form (`/:<name>` for globals), so this
    /// strictness only affects users typing by memory.
    // zed-kask: `Public` (marketplace-installed) skills behave like `Global`
    // for slash-command scoping — they use the empty prefix and match the
    // empty scope. Pinned by `test_skill_source_public_matches_empty_scope`.
    pub fn matches_scope(&self, scope: &str) -> bool {
        match self {
            Self::Global | Self::Public { .. } => scope.is_empty(),
            Self::ProjectLocal {
                worktree_root_name, ..
            } => !scope.is_empty() && worktree_root_name.as_ref() == scope,
        }
    }
}

/// App-wide index of loaded skills, published by NativeAgent and read
/// by any UI that needs to display the skill list (e.g. Settings UI).
#[derive(Default, Clone)]
pub struct SkillIndex {
    pub global_skills: Vec<Skill>,
    pub project_skills: Vec<ProjectSkillGroup>,
}

#[derive(Clone)]
pub struct ProjectSkillGroup {
    pub worktree_id: SkillScopeId,
    pub worktree_root_name: SharedString,
    pub skills: Vec<Skill>,
}

impl Global for SkillIndex {}

/// Rescan skill agent skill directories when skills are created or modified via UI
pub struct SkillsUpdatedHook(pub Rc<dyn Fn(&mut App)>);

impl Global for SkillsUpdatedHook {}

/// Just the frontmatter, used for parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default, rename = "disable-model-invocation")]
    pub disable_model_invocation: bool,
    /// Skill names this skill depends on. At invocation time, the skill tool
    /// checks that all dependencies are installed and returns a clear error
    /// before running the cascade (saves tokens by failing fast).
    /// Dependencies are skill names (not IDs) — e.g. `deep-module`, not
    /// `alice/deep-module`. Any installed skill of that name satisfies the
    /// dependency, regardless of source.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// When `true`, this skill is a core skill: always-on, re-seeded on every
    /// startup (overwriting user edits), locked against editing, and
    /// undisableable. Core skills cannot be shadowed by project-local skills
    /// of the same name. The `core` flag is the zed-kask mechanism for
    /// distinguishing system-critical kask-skills from user kask-skills.
    #[serde(default)]
    pub core: bool,
}

/// Minimal skill info for system prompt.
///
/// `Serialize` is required for handlebars rendering of the system prompt
/// template (see `ProjectContext` in `prompt_store`). `PartialEq, Eq` lets
/// the agent compare freshly-built `ProjectContext`s and skip pushing an
/// unchanged value through the project_context entity (which would
/// otherwise look like a system-prompt change to the model and invalidate
/// the API's prompt cache).
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    /// Absolute path to the SKILL.md file, so the model can resolve
    /// references relative to the skill's directory when reading bundled
    /// resources.
    pub location: String,
}

impl From<&Skill> for SkillSummary {
    fn from(skill: &Skill) -> Self {
        Self {
            name: skill.name.clone(),
            description: skill.description.clone(),
            location: skill.skill_file_path.to_string_lossy().into_owned(),
        }
    }
}

/// Error that occurred while loading a skill
#[derive(Debug, Clone)]
pub struct SkillLoadError {
    pub path: PathBuf,
    pub message: String,
}

impl std::fmt::Display for SkillLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for SkillLoadError {}

/// Parse the frontmatter of a SKILL.md file into a `Skill` struct.
///
/// The file must have YAML frontmatter between `---` delimiters containing
/// `name` and `description` fields. The body (everything after the closing
/// `---`) is intentionally NOT returned — it's read on demand via
/// `read_skill_body` when the skill is actually being materialized for the
/// model, so we don't pay N × body-size in memory for N skills.
///
/// `content` only needs to contain bytes up through the closing `---`; any
/// trailing body bytes are ignored.
pub fn parse_skill_frontmatter(
    skill_file_path: &Path,
    content: &str,
    source: SkillSource,
) -> Result<Skill> {
    let (metadata, _body, load_warnings) = parse_skill_file_content_for_loading(content)?;

    let directory_path = skill_file_path
        .parent()
        .context("SKILL.md file has no parent directory")?
        .to_path_buf();

    Ok(Skill {
        name: metadata.name,
        description: metadata.description,
        source,
        directory_path,
        skill_file_path: skill_file_path.to_path_buf(),
        load_warnings,
        disable_model_invocation: metadata.disable_model_invocation,
        dependencies: metadata.dependencies,
        core: metadata.core,
    })
}

/// Extract the YAML frontmatter and body from a SKILL.md file without
/// validating the metadata fields.
pub fn extract_skill_frontmatter(content: &str) -> Result<(SkillMetadata, &str)> {
    if content.len() > MAX_SKILL_FILE_SIZE {
        anyhow::bail!(
            "SKILL.md file exceeds maximum size of {}KB",
            MAX_SKILL_FILE_SIZE / 1024
        );
    }

    extract_frontmatter(content)
}

/// Parse and validate the YAML frontmatter and body from a SKILL.md file.
pub fn parse_skill_file_content(content: &str) -> Result<(SkillMetadata, &str)> {
    let (metadata, body) = extract_skill_frontmatter(content)?;

    validate_name(&metadata.name).map_err(anyhow::Error::msg)?;
    validate_description(&metadata.description).map_err(anyhow::Error::msg)?;

    Ok((metadata, body))
}

fn parse_skill_file_content_for_loading(
    content: &str,
) -> Result<(SkillMetadata, &str, Vec<SkillLoadWarning>)> {
    let (metadata, body) = extract_skill_frontmatter(content)?;

    validate_name(&metadata.name).map_err(anyhow::Error::msg)?;
    let load_warnings =
        validate_description_for_loading(&metadata.description).map_err(anyhow::Error::msg)?;

    // zed-kask: enforce that `core: true` frontmatter is honored only for
    // skills whose name is in `CORE_SKILL_NAMES`. A marketplace-installed or
    // hand-edited skill declaring `core: true` with a non-reserved name would
    // otherwise be treated as core (sovereign, not overwritten on seed, hidden
    // edit/delete controls) — a backdoor around the core-skill
    // contract. `is_reserved_skill_name` already refuses a non-core skill from
    // bearing a core *name*; this closes the symmetric case where a non-core
    // name claims core *status*. Pinned by
    // `test_parse_refuses_core_true_on_non_reserved_name`.
    if metadata.core && !is_core_skill(&metadata.name) {
        anyhow::bail!(
            "skill '{}' declares `core: true` but is not a registered core skill \
             (not in CORE_SKILL_NAMES). The `core` frontmatter field is reserved \
             for shipped system-critical skills; remove it or rename the skill.",
            metadata.name
        );
    }

    Ok((metadata, body, load_warnings))
}

fn validate_description_for_loading(
    description: &str,
) -> Result<Vec<SkillLoadWarning>, &'static str> {
    if description.trim().is_empty() {
        return Err("Skill description cannot be empty");
    }

    // zed-kask: Description length warnings are disabled. SKILL.md files
    // are reference-only — their descriptions appear in the discovery
    // catalog but are never injected into prompts. Skills execute via
    // YAML manifests in the kask registry, so description length has no
    // token-cost impact.
    Ok(Vec::new())
}

fn extract_frontmatter(content: &str) -> Result<(SkillMetadata, &str)> {
    let content = content.trim_start();

    if !content.starts_with("---") {
        anyhow::bail!("SKILL.md must start with YAML frontmatter (---)");
    }

    // Find every candidate closing `---` line: a line consisting EXACTLY of
    // `---` (followed by `\n`, `\r\n`, or EOF) at column 0, excluding the
    // opening line itself. The opener occupies bytes 0..(first line ending),
    // and our scan starts after each `\n`, so the opener is naturally skipped.
    //
    // For each candidate we record the byte position right after its line
    // ending; that's both where the YAML stream slice ends and where the body
    // begins.
    let bytes = content.as_bytes();
    let mut candidates: Vec<usize> = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'\n' {
            continue;
        }
        let line_start = i + 1;
        if line_start + 3 > bytes.len() {
            continue;
        }
        if &bytes[line_start..line_start + 3] != b"---" {
            continue;
        }
        let after_dashes = line_start + 3;
        let end = if after_dashes == bytes.len() {
            after_dashes
        } else if bytes[after_dashes] == b'\n' {
            after_dashes + 1
        } else if after_dashes + 1 < bytes.len()
            && bytes[after_dashes] == b'\r'
            && bytes[after_dashes + 1] == b'\n'
        {
            after_dashes + 2
        } else {
            // Line is something like `---trailing` or `----`; not a candidate.
            continue;
        };
        candidates.push(end);
    }

    if candidates.is_empty() {
        anyhow::bail!("SKILL.md missing closing frontmatter delimiter (---)");
    }

    // Try each candidate in order: slice content up through the candidate's
    // terminator and ask `serde_yaml_ng` to parse it as a YAML stream. If the
    // first document deserializes into `SkillMetadata`, that candidate is the
    // real closer. Otherwise an earlier candidate may have cut the YAML in the
    // middle of a scalar / quoted string; try the next one.
    let mut last_error: Option<anyhow::Error> = None;
    for end in candidates {
        let prefix = &content[..end];
        let mut docs = serde_yaml_ng::Deserializer::from_str(prefix);
        let Some(first_doc) = docs.next() else {
            continue;
        };
        match SkillMetadata::deserialize(first_doc) {
            Ok(metadata) => return Ok((metadata, &content[end..])),
            Err(e) => last_error = Some(anyhow::Error::new(e)),
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("could not parse YAML frontmatter"))
        .context("Invalid YAML frontmatter"))
}

/// Maximum length for a valid skill name. Mirrors the upper bound enforced
/// by [`validate_name`].
pub const MAX_SKILL_NAME_LEN: usize = 64;

/// Maximum recommended length (in bytes) for a skill description. The
/// create-skill UI enforces this as a hard limit, while the loader emits a
/// warning and still loads longer descriptions.
///
/// Byte-based rather than char-based because that's what `.len()` returns
/// and what every caller currently measures; the UI also surfaces this
/// limit as a byte count so the editor's counter matches the validator.
pub const MAX_SKILL_DESCRIPTION_LEN: usize = 1024;

/// Convert an arbitrary human-readable string into a valid skill name, or
/// return `None` if no valid name can be produced (e.g. the input contains
/// no ASCII alphanumeric characters at all).
///
/// The transformation:
///
/// 1. Replaces each `&` with the word `and` (with separators on either
///    side), so titles like "rock & roll" or "AT&T" round-trip something
///    meaningful (`rock-and-roll`, `at-and-t`) rather than dropping the
///    `&` and silently mashing the neighbours together.
/// 2. ASCII-lowercases every ASCII letter.
/// 3. Replaces each space with `-`. Existing `-` characters are kept.
/// 4. **Drops** every other non-alphanumeric character entirely (NOT
///    replaced with a dash). So `foo!bar` slugifies to `foobar`, not
///    `foo-bar` — only word boundaries the user actually wrote (spaces)
///    become dashes.
/// 5. Collapses runs of `-` into a single `-`.
/// 6. Trims leading and trailing `-`.
/// 7. Truncates to [`MAX_SKILL_NAME_LEN`] bytes (then re-trims trailing `-`
///    in case the truncation landed on one).
///
/// The result, if `Some`, always satisfies [`validate_name`].
pub fn slugify_skill_name(input: &str) -> Option<String> {
    // Substitute `&` with `-and-` BEFORE the per-character pass; the
    // existing dash-collapsing and edge-trimming logic then handles the
    // boundary cases (`foo & bar`, `&foo`, `foo&`, `&&`, etc.) for free.
    let input = input.replace('&', "-and-");
    let mut slug = String::with_capacity(input.len());
    let mut last_was_dash = true; // suppress a leading `-`
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == ' ' || ch == '-' {
            Some('-')
        } else {
            // Drop the character entirely — and importantly, do NOT touch
            // `last_was_dash`. That way `foo!bar` stays one run of
            // alphanumerics (`foobar`) rather than getting a fake
            // separator inserted (`foo-bar`).
            None
        };
        let Some(c) = mapped else { continue };
        if c == '-' {
            if last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        slug.push(c);
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.len() > MAX_SKILL_NAME_LEN {
        slug.truncate(MAX_SKILL_NAME_LEN);
        while slug.ends_with('-') {
            slug.pop();
        }
    }
    if slug.is_empty() { None } else { Some(slug) }
}

/// Validate a skill name against the rules enforced by both the loader
/// and the create-skill UI.
///
/// Rules:
/// * non-empty
/// * at most [`MAX_SKILL_NAME_LEN`] bytes
/// * ASCII lowercase letters, digits, and hyphens only
/// * must not start or end with a hyphen — [`slugify_skill_name`]
///   already guarantees this for its output, so requiring it in the
///   validator keeps hand-written `SKILL.md` files consistent with
///   slugifier output
///
/// Error messages are returned as `&'static str` (interpolated at
/// compile time via `formatcp!`) so that UI surfaces can store them in
/// `Option<&'static str>` fields without allocating, and loader callers
/// can convert them to `anyhow::Error` via `anyhow::Error::msg`.
pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Skill name cannot be empty");
    }
    if name.len() > MAX_SKILL_NAME_LEN {
        return Err(formatcp!(
            "Skill name must be at most {MAX_SKILL_NAME_LEN} characters"
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err("Skill name must not start or end with a hyphen");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("Skill name must contain only lowercase letters, numbers, and hyphens");
    }
    Ok(())
}

/// Validate a skill description against the strict rules enforced by the
/// create-skill UI and imported/shared skill parsing.
pub fn validate_description(description: &str) -> Result<(), &'static str> {
    if description.trim().is_empty() {
        return Err("Skill description cannot be empty");
    }
    if description.len() > MAX_SKILL_DESCRIPTION_LEN {
        return Err(formatcp!(
            "Skill description must be at most {MAX_SKILL_DESCRIPTION_LEN} bytes"
        ));
    }
    Ok(())
}

pub async fn load_skills_from_directory(
    fs: &Arc<dyn Fs>,
    directory: &Path,
    source: SkillSource,
) -> Vec<Result<Skill, SkillLoadError>> {
    if !fs.is_dir(directory).await {
        return Vec::new();
    }

    let skill_files = find_skill_files(fs, directory).await;

    let mut results: Vec<Result<Skill, SkillLoadError>> = futures::stream::iter(skill_files)
        .map(|path| {
            let fs = fs.clone();
            let source = source.clone();
            async move { load_skill_frontmatter(fs, path, source).await }
        })
        .buffer_unordered(SKILL_IO_CONCURRENCY)
        .collect()
        .await;

    // Sort by path so name-conflict resolution in `apply_skill_overrides`
    // is deterministic — `fs.read_dir` order is filesystem-dependent.
    results.sort_by(|a, b| {
        let path_a: &Path = match a {
            Ok(skill) => &skill.skill_file_path,
            Err(error) => &error.path,
        };
        let path_b: &Path = match b {
            Ok(skill) => &skill.skill_file_path,
            Err(error) => &error.path,
        };
        path_a.cmp(path_b)
    });

    results
}

/// zed-kask: Load marketplace-installed skills from
/// `{global_skills_dir}/_marketplace/{source_user}/{skill_name}/`.
///
/// The marketplace directory has a two-level namespace: `_marketplace/`
/// contains `{source_user}/` directories, each of which contains
/// `{skill_name}/` directories, each of which contains `SKILL.md`.
/// This function scans both levels and loads each skill with
/// `SkillSource::Public { source_user, original_skill_id }`.
pub async fn load_marketplace_skills(
    fs: &Arc<dyn Fs>,
    marketplace_dir: &Path,
) -> Vec<Result<Skill, SkillLoadError>> {
    if !fs.is_dir(marketplace_dir).await {
        return Vec::new();
    }

    let mut all_results = Vec::new();

    // List source_user directories.
    let Ok(mut user_entries) = fs.read_dir(marketplace_dir).await else {
        log::warn!(
            "Failed to read marketplace directory: {} — returning empty skill list",
            marketplace_dir.display()
        );
        return Vec::new();
    };
    while let Some(Ok(user_path)) = user_entries.next().await {
        if !fs.is_dir(&user_path).await {
            continue;
        }
        let source_user = user_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // List skill_name directories under each source_user.
        let Ok(mut skill_entries) = fs.read_dir(&user_path).await else {
            log::warn!(
                "Failed to read marketplace user directory: {} — skipping",
                user_path.display()
            );
            continue;
        };
        while let Some(Ok(skill_path)) = skill_entries.next().await {
            if !fs.is_dir(&skill_path).await {
                continue;
            }
            let skill_name = skill_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let skill_file = skill_path.join("SKILL.md");
            if !fs.is_file(&skill_file).await {
                continue;
            }

            let original_skill_id = format!("{}/{}", source_user, skill_name);
            let source = SkillSource::Public {
                source_user: Arc::from(source_user.as_str()),
                original_skill_id: Arc::from(original_skill_id.as_str()),
            };
            let result = load_skill_frontmatter(fs.clone(), skill_file, source).await;
            all_results.push(result);
        }
    }

    all_results
}
///
/// Discovery is intentionally one level deep: a skill is the immediate
/// child directory of the skills root, and `SKILL.md` is the file that
/// names it. See `crates/agent_skills/README.md` for why we don't recurse.
async fn find_skill_files(fs: &Arc<dyn Fs>, directory: &Path) -> Vec<PathBuf> {
    let Ok(mut entries) = fs.read_dir(directory).await else {
        log::warn!(
            "Failed to read skill directory: {} — returning empty",
            directory.display()
        );
        return Vec::new();
    };

    let mut entry_paths = Vec::new();
    while let Some(entry) = entries.next().await {
        if let Ok(entry_path) = entry {
            entry_paths.push(entry_path);
        }
    }

    futures::stream::iter(entry_paths)
        .map(|entry_path| {
            let fs = fs.clone();
            async move {
                let Ok(Some(metadata)) = fs.metadata(&entry_path).await else {
                    log::warn!(
                        "Failed to stat skill entry: {} — skipping",
                        entry_path.display()
                    );
                    return None;
                };
                if !metadata.is_dir {
                    return None;
                }
                let skill_file = entry_path.join(SKILL_FILE_NAME);
                fs.is_file(&skill_file).await.then_some(skill_file)
            }
        })
        .buffer_unordered(SKILL_IO_CONCURRENCY)
        .filter_map(|x| async move { x })
        .collect()
        .await
}

/// Read `skill_file_path` from disk and parse its frontmatter. The
/// SKILL.md body is parsed away by `parse_skill_frontmatter` and not
/// surfaced here; it's re-read on demand via `read_skill_body` when a
/// skill is actually being loaded for the model.
///
/// We load the whole file in one go rather than streaming up to the
/// closing `---`. `MAX_SKILL_FILE_SIZE` is 100KB and the metadata check
/// below caps the worst case at that, so the peak transient cost is
/// trivially small (≤ `MAX_SKILL_FILE_SIZE` × `SKILL_IO_CONCURRENCY`).
pub async fn load_skill_frontmatter(
    fs: Arc<dyn Fs>,
    skill_file_path: PathBuf,
    source: SkillSource,
) -> Result<Skill, SkillLoadError> {
    // Short-circuit on oversized files before reading any of their
    // contents, so a stray multi-GB file named `SKILL.md` can't OOM the
    // app. If metadata is unavailable, refuse to read.
    let metadata = fs
        .metadata(&skill_file_path)
        .await
        .map_err(|e| SkillLoadError {
            path: skill_file_path.clone(),
            message: format!("Failed to read SKILL.md metadata: {}", e),
        })?;
    if let Some(metadata) = metadata
        && metadata.len > MAX_SKILL_FILE_SIZE as u64
    {
        return Err(SkillLoadError {
            path: skill_file_path.clone(),
            message: format!(
                "SKILL.md file exceeds maximum size of {}KB",
                MAX_SKILL_FILE_SIZE / 1024
            ),
        });
    }

    let content = fs
        .load(&skill_file_path)
        .await
        .map_err(|e| SkillLoadError {
            path: skill_file_path.clone(),
            message: format!("Failed to read file: {}", e),
        })?;

    parse_skill_frontmatter(&skill_file_path, &content, source).map_err(|e| SkillLoadError {
        path: skill_file_path.clone(),
        message: e.to_string(),
    })
}

/// Read the body of a SKILL.md from disk — everything after the closing
/// `---`. Called only when a skill is being materialized for the model
/// (via `SkillTool` or a slash invocation). The body is intentionally
/// NOT kept in memory between materializations.
pub async fn read_skill_body(
    fs: &dyn Fs,
    skill_file_path: &Path,
) -> Result<String, SkillLoadError> {
    let content = fs.load(skill_file_path).await.map_err(|e| SkillLoadError {
        path: skill_file_path.to_path_buf(),
        message: format!("Failed to read file: {}", e),
    })?;

    read_skill_body_from_content(skill_file_path, &content)
}

pub fn read_skill_body_from_content(
    skill_file_path: &Path,
    content: &str,
) -> Result<String, SkillLoadError> {
    let (_metadata, body, _load_warnings) =
        parse_skill_file_content_for_loading(content).map_err(|e| SkillLoadError {
            path: skill_file_path.to_path_buf(),
            message: e.to_string(),
        })?;

    Ok(body.trim().to_string())
}

// Auto-generated by build.rs — the kask SKILL.md files authored in this
// repo's `.agents/skills/` directory, compiled in as a **one-time seed
// payload**. At startup `seed_shipped_skills` writes each one to the user's
// on-disk global skills directory (`paths::data_dir()/agents/skills/`).
// The disk copy is the single runtime source of truth — skills are loaded
// from disk via `load_skills_from_directory`, never from this compiled
// payload. User edits take effect immediately without recompilation.
//
// The SKILL.md is the *interface* (frontmatter parsed for discovery); the
// *implementation* (YAML manifests + Jinja2 templates) is seeded to disk by
// the `hkask-templates` seeding path and resolved from disk at runtime by
// `BridgeManifestExecutor` and `TemplateRenderer`.
include!(concat!(env!("OUT_DIR"), "/embedded_global_skills.rs"));

/// Returns the `(name, raw_content)` pairs for the kask skills shipped with
/// this binary, exactly as authored in `.agents/skills/`. Seed-only: used by
/// [`seed_shipped_skills`] to materialise the skills on disk. Not used as a
/// runtime catalog source — the catalog is built from disk.
fn shipped_skill_seed() -> &'static [(&'static str, &'static str)] {
    SHIPPED_SKILL_SEED_ENTRIES
}

/// Materialise the shipped kask skills onto the user's disk if missing.
///
/// For each shipped skill in [`shipped_skill_seed`], writes
/// `<skills_dir>/<name>/SKILL.md` when the file does not already exist. Existing
/// files are **never overwritten** — user edits are sovereign. A user who
/// deletes a shipped skill will see it re-seeded on the next startup; to
/// suppress a skill instead, edit its `disable-model-invocation` frontmatter
/// or remove it from the catalog via the skills UI.
///
/// This is the sole mechanism by which a self-contained binary populates the
/// on-disk skill catalog on a fresh install. After seeding, all discovery and
/// editing happens against the disk copies — no compiled-in data is read at
/// runtime, so changes take effect without recompilation or a new release.
pub async fn seed_shipped_skills(fs: &dyn Fs, skills_dir: &Path) {
    for (name, content) in shipped_skill_seed() {
        let skill_dir = skills_dir.join(name);
        let skill_file = skill_dir.join(SKILL_FILE_NAME);
        let is_core = is_core_skill(name);

        // Core skills are always overwritten on every startup so user edits
        // cannot break system-critical functionality. User skills are seeded
        // only if missing — user edits are sovereign.
        if fs.is_file(&skill_file).await && !is_core {
            continue;
        }
        if let Err(error) = fs.create_dir(&skill_dir).await {
            log::warn!(
                "Failed to create shipped skill directory '{}' for seeding: {error}",
                skill_dir.display()
            );
            continue;
        }
        if let Err(error) = fs.write(&skill_file, content.as_bytes()).await {
            log::warn!("Failed to seed shipped skill '{}': {error}", name);
        }
    }
}

/// Returns the global skills directory.
///
/// zed-kask: D28 — Standardized Artifact Storage. Global skills live
/// under the kask data root (`{kask_data_dir}/skills/`), resolved via the
/// `global_skills_dir_override` hook (set by `kask_bridge` at startup), NOT
/// `paths::data_dir()/agents/skills/` (the zed-kask app data root). This
/// keeps all kask artifacts under one rooted tree per the standardized
/// storage layout (`kask/docs/architecture/standardized-artifact-storage.md`).
///
/// zed-kask skills are manifest-driven (manifest.yaml + Jinja2 templates
/// executed via BridgeManifestExecutor), not SKILL.md body-injection. They
/// are not portable to upstream Zed or any tool without the hKask cascade
/// engine.
///
/// When the override is set (production, wired early in `main.rs`), returns
/// the kask data-root skills path. When unset (tests, or pre-wiring), falls
/// back to `paths::data_dir()/agents/skills/` so tests and the editor remain
/// functional.
///
/// In test builds, `paths::home_dir()` is hardcoded to a fixed path
/// (e.g. `/Users/zed`), so all tests using this function operate on the
/// same simulated home directory. Each test should use its own `FakeFs`
/// instance to keep skill setups from leaking across tests.
pub fn global_skills_dir() -> PathBuf {
    // zed-kask: D28 — check the override first.
    if let Some(path) = global_skills_dir_override() {
        return path;
    }
    paths::data_dir().join("agents").join(SKILLS_DIR_NAME)
}

// zed-kask: D28 — Standardized Artifact Storage.
// Global hook for the kask data-root skills directory path. Mirrors the
// `THREADS_DB_PATH_OVERRIDE` pattern (D28 in `agent.rs`). `agent_skills`
// does NOT depend on `hkask-types` (preserving the §13.1 invariant); the
// path is passed through this hook from `main.rs`.
//
// Uses a `Mutex` (not `OnceLock`) so the path can be replaced after startup
// (e.g. if the operator changes the kask data directory). Per `.rules`,
// `Mutex` hooks are re-settable and do not need the `Err`-branch warn.
static GLOBAL_SKILLS_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Set the global skills directory override (D28 composition root).
///
/// Called by zed-kask's app startup to wire the canonical kask data-root
/// skills path. When `None`, `global_skills_dir()` falls back to the
/// upstream `paths::data_dir()/agents/skills/` path.
pub fn set_global_skills_dir_override(path: Option<PathBuf>) {
    *GLOBAL_SKILLS_DIR_OVERRIDE
        .lock()
        .expect("GLOBAL_SKILLS_DIR_OVERRIDE poisoned") = path;
}

/// Get the global skills directory override, if set.
pub(crate) fn global_skills_dir_override() -> Option<PathBuf> {
    GLOBAL_SKILLS_DIR_OVERRIDE
        .lock()
        .expect("GLOBAL_SKILLS_DIR_OVERRIDE poisoned")
        .clone()
}

/// Project-local skills live at this path relative to a worktree root,
/// e.g. `<worktree>/.agents/skills/<skill>/SKILL.md`.
pub fn project_skills_relative_path() -> &'static str {
    ".agents/skills"
}

/// Returns `true` if `path` looks like it points into an agent skills
/// directory — i.e. it contains `AGENTS_DIR_NAME` immediately followed by
/// `SKILLS_DIR_NAME` as two consecutive path components, anywhere in the
/// path. Comparison is case-insensitive so it agrees with classifiers
/// that canonicalize against `~/.agents/skills` on case-insensitive
/// filesystems (macOS/Windows by default).
///
/// The path arriving here can be any of:
///
///   1. Bare relative-to-worktree-root: `.agents/skills/...`
///   2. Worktree-name prefixed:         `<worktree>/.agents/skills/...`
///   3. Absolute:                       `/path/to/worktree/.agents/skills/...`
///
/// Any-depth matching has a known cost: a `.agents/skills` directory
/// nested inside vendored sources (e.g. `vendor/x/.agents/skills/...`)
/// would also be flagged. We accept that as the safer-failing direction —
/// an extra confirmation prompt for a vendored file is annoying, while
/// silently letting the agent overwrite a `.agents/skills` tree the user
/// didn't expect to be touched is unsafe.
pub fn is_agents_skills_path(path: &Path) -> bool {
    let mut components = path.components().map(|c| c.as_os_str());
    let Some(mut prev) = components.next() else {
        return false;
    };
    for curr in components {
        if component_matches_ignore_ascii_case(prev, AGENTS_DIR_NAME)
            && component_matches_ignore_ascii_case(curr, SKILLS_DIR_NAME)
        {
            return true;
        }
        prev = curr;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::FakeFs;
    use gpui::TestAppContext;

    #[test]
    fn test_skill_source_precedence_is_total_and_ordered() {
        // Pin the hierarchy: project-local > global > public (marketplace).
        // Every override and conflict-resolution site routes through this,
        // so the rest of the codebase relies on it being correct.
        let public = SkillSource::Public {
            source_user: "alice".into(),
            original_skill_id: "alice/bug-hunt".into(),
        }
        .precedence();
        let global = SkillSource::Global.precedence();
        let project = SkillSource::ProjectLocal {
            worktree_id: SkillScopeId(1),
            worktree_root_name: "my-project".into(),
        }
        .precedence();

        assert!(public < global, "global must shadow public (marketplace)");
        assert!(global < project, "project-local must shadow global");

        // Two project-local skills from different worktrees tie. The
        // "first wins" convention is enforced by the callers, but the
        // precedence itself must be equal so neither silently shadows
        // the other.
        let other_project = SkillSource::ProjectLocal {
            worktree_id: SkillScopeId(2),
            worktree_root_name: "other-project".into(),
        }
        .precedence();
        assert_eq!(project, other_project);
    }

    // zed-kask: `SkillSource::Public` (marketplace-installed) precedence sits
    // below `Global` so a locally-authored skill of the same name shadows the
    // marketplace copy. This is the core deviation from upstream (which has no
    // marketplace source variant). Pinned here so a future refactor that
    // reorders the precedence match arms fails loudly.
    #[test]
    fn test_skill_source_public_precedence_below_global() {
        let public = SkillSource::Public {
            source_user: "alice".into(),
            original_skill_id: "alice/bug-hunt".into(),
        }
        .precedence();
        let global = SkillSource::Global.precedence();

        assert!(
            public < global,
            "Public must sit strictly below Global: public={}, global={}",
            public,
            global
        );
    }

    // zed-kask: `SkillSource::Public::display_label` returns the namespaced
    // `{source_user}/{skill_name}` form so the marketplace listing identity
    // is preserved on installed skills. Upstream has no `Public` variant.
    #[test]
    fn test_skill_source_public_display_label_is_namespaced() {
        let source = SkillSource::Public {
            source_user: "alice".into(),
            original_skill_id: "alice/bug-hunt".into(),
        };
        assert_eq!(source.display_label(), "alice/bug-hunt");
    }

    // zed-kask: `SkillSource::Public` behaves like `Global` for slash-command
    // scoping — empty prefix, matches the empty scope. This means a
    // marketplace-installed skill is invoked as `/:<name>`, same as a global.
    // Upstream has no `Public` variant.
    #[test]
    fn test_skill_source_public_matches_empty_scope() {
        let source = SkillSource::Public {
            source_user: "alice".into(),
            original_skill_id: "alice/bug-hunt".into(),
        };
        assert_eq!(source.scope_prefix(), "");
        assert!(source.matches_scope(""));
        assert!(!source.matches_scope("alice"));
        assert!(!source.matches_scope("global"));
    }

    // zed-kask: A skill declaring `core: true` with a non-reserved name must
    // be refused at load time. This closes the backdoor where a marketplace-
    // installed or hand-edited skill claims core status to gain sovereignty
    // (never overwritten on seed) and hidden edit/delete controls.
    // The symmetric case (a non-core skill bearing a core *name*) is already
    // refused by `is_reserved_skill_name`; this pins the status side.
    #[test]
    fn test_parse_refuses_core_true_on_non_reserved_name() {
        let content = r"---
name: my-impostor-skill
description: A skill falsely claiming core status.
core: true
---

Body.
";
        let result = parse_skill_frontmatter(
            Path::new("/skills/my-impostor-skill/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(
            result.is_err(),
            "a non-core skill declaring `core: true` must be refused at load time"
        );
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("not a registered core skill"),
            "error should name the violation; got: {message}"
        );
    }

    // zed-kask: A skill declaring `core: true` with a reserved (core) name
    // parses successfully — legitimate core skills carry `core: true`. This
    // pins that the enforcement does not reject the shipped core skills.
    #[test]
    fn test_parse_accepts_core_true_on_reserved_name() {
        let content = r"---
name: create-skill
description: The canonical core skill authoring tool.
core: true
---

Body.
";
        let skill = parse_skill_frontmatter(
            Path::new("/skills/create-skill/SKILL.md"),
            content,
            SkillSource::Global,
        )
        .expect("a core skill with `core: true` and a reserved name must parse");
        assert!(skill.core, "core flag must be preserved");
    }

    #[test]
    fn test_parse_valid_skill() {
        let content = r#"---
name: my-skill
description: A test skill for testing purposes
---

# My Skill

## Instructions
Do the thing.
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/my-skill/SKILL.md"),
            content,
            SkillSource::Global,
        );
        let skill = result.expect("Should parse successfully");

        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "A test skill for testing purposes");
        assert_eq!(skill.directory_path, Path::new("/skills/my-skill"));
        // Default: skill is invocable by both model and user.
        assert!(!skill.disable_model_invocation);
    }

    #[test]
    fn test_parse_skill_file_content_returns_body() {
        let content = r#"---
name: my-skill
description: A test skill for testing purposes
---

# My Skill

Do the thing.
"#;

        let (metadata, body) = parse_skill_file_content(content)
            .expect("valid skill content should parse successfully");

        assert_eq!(metadata.name, "my-skill");
        assert_eq!(metadata.description, "A test skill for testing purposes");
        assert_eq!(body.trim(), "# My Skill\n\nDo the thing.");
    }

    #[test]
    fn test_parse_disable_model_invocation_true() {
        let content = r#"---
name: deploy
description: Deploy the application to production.
disable-model-invocation: true
---

Steps to deploy.
"#;

        let skill = parse_skill_frontmatter(
            Path::new("/skills/deploy/SKILL.md"),
            content,
            SkillSource::Global,
        )
        .expect("should parse");
        assert!(skill.disable_model_invocation);
    }

    #[test]
    fn test_parse_disable_model_invocation_explicit_false() {
        let content = r#"---
name: helper
description: A helper skill.
disable-model-invocation: false
---

Help.
"#;

        let skill = parse_skill_frontmatter(
            Path::new("/skills/helper/SKILL.md"),
            content,
            SkillSource::Global,
        )
        .expect("should parse");
        assert!(!skill.disable_model_invocation);
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# My Skill\n\nNo frontmatter here.";

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must start with YAML frontmatter")
        );
    }

    #[test]
    fn test_parse_missing_closing_delimiter() {
        let content = r#"---
name: test
description: Test
# No closing delimiter
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing closing frontmatter delimiter")
        );
    }

    #[test]
    fn test_parse_empty_frontmatter_closing_on_next_line() {
        // An empty frontmatter (closer immediately after the opener) is a real
        // authoring case. Parsing should ultimately fail because the empty YAML
        // doc lacks `name` and `description`, but the error must be the proper
        // YAML/missing-field error rather than "missing closing frontmatter
        // delimiter" — the closer is right there.
        let content = "---\n---\nbody\n";

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_chain = format!("{:?}", err);
        assert!(
            !err_chain.contains("missing closing frontmatter delimiter"),
            "Error should NOT be the missing-closer error since the closer is present: {}",
            err_chain
        );
        assert!(
            err_chain.contains("missing field")
                || err_chain.contains("name")
                || err_chain.contains("description")
                || err_chain.contains("Invalid YAML"),
            "Error should mention missing name/description field or invalid YAML: {}",
            err_chain
        );
    }

    #[test]
    fn test_parse_missing_name() {
        let content = r#"---
description: A test skill
---

Content here.
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_chain = format!("{:?}", err);
        assert!(
            err_chain.contains("missing field")
                || err_chain.contains("name")
                || err_chain.contains("Invalid YAML"),
            "Error should mention missing name field or invalid YAML: {}",
            err_chain
        );
    }

    #[test]
    fn test_parse_missing_description() {
        let content = r#"---
name: test-skill
---

Content here.
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_chain = format!("{:?}", err);
        assert!(
            err_chain.contains("missing field")
                || err_chain.contains("description")
                || err_chain.contains("Invalid YAML"),
            "Error should mention missing description field or invalid YAML: {}",
            err_chain
        );
    }

    #[test]
    fn test_parse_name_too_long() {
        let long_name = "a".repeat(65);
        let content = format!(
            r#"---
name: {long_name}
description: Test
---

Content.
"#
        );

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            &content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        let expected = format!("at most {MAX_SKILL_NAME_LEN} characters");
        assert!(result.unwrap_err().to_string().contains(&expected));
    }

    #[test]
    fn test_parse_name_invalid_chars() {
        let content = r#"---
name: My_Skill
description: Test
---

Content.
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("lowercase letters, numbers, and hyphens")
        );
    }

    #[test]
    fn test_slugify_basic() {
        assert_eq!(
            slugify_skill_name("My Cool Skill").as_deref(),
            Some("my-cool-skill")
        );
    }

    #[test]
    fn test_slugify_strips_invalid_chars() {
        // Punctuation is dropped; spaces between words still produce dashes.
        // `Hello,` → `hello`, then `␣` → `-`, then `World!` → `world`, etc.
        assert_eq!(
            slugify_skill_name("Hello, World! (v2)").as_deref(),
            Some("hello-world-v2")
        );
    }

    #[test]
    fn test_slugify_drops_punctuation_in_middle_no_spaces() {
        // Punctuation between alphanumerics is dropped entirely — it does
        // NOT become a dash. Only user-written spaces become dashes.
        assert_eq!(slugify_skill_name("foo!bar").as_deref(), Some("foobar"));
        assert_eq!(slugify_skill_name("foo?bar").as_deref(), Some("foobar"));
        assert_eq!(slugify_skill_name("foo%bar").as_deref(), Some("foobar"));
        assert_eq!(slugify_skill_name("100%sure").as_deref(), Some("100sure"));
        assert_eq!(
            slugify_skill_name("what's that").as_deref(),
            Some("whats-that")
        );
        // `&` is special-cased to become `and` — see
        // `test_slugify_ampersand_becomes_and` for the full coverage.
        assert_eq!(
            slugify_skill_name("don't&won't").as_deref(),
            Some("dont-and-wont")
        );
    }

    #[test]
    fn test_slugify_ampersand_becomes_and() {
        // No spaces around `&`.
        assert_eq!(
            slugify_skill_name("foo&bar").as_deref(),
            Some("foo-and-bar")
        );
        assert_eq!(
            slugify_skill_name("rock&roll").as_deref(),
            Some("rock-and-roll")
        );
        // Spaces around `&`: collapses to a single dash on each side.
        assert_eq!(
            slugify_skill_name("foo & bar").as_deref(),
            Some("foo-and-bar")
        );
        // Asymmetric spacing.
        assert_eq!(
            slugify_skill_name("foo& bar").as_deref(),
            Some("foo-and-bar")
        );
        assert_eq!(
            slugify_skill_name("foo &bar").as_deref(),
            Some("foo-and-bar")
        );
        // Leading/trailing `&`: the substituted spaces become leading/
        // trailing dashes which then get trimmed.
        assert_eq!(slugify_skill_name("&foo").as_deref(), Some("and-foo"));
        assert_eq!(slugify_skill_name("foo&").as_deref(), Some("foo-and"));
        // `&` alone slugifies to the word `and`, not to `None`.
        assert_eq!(slugify_skill_name("&").as_deref(), Some("and"));
        assert_eq!(slugify_skill_name(" & ").as_deref(), Some("and"));
        // Multiple `&`s with various spacing all collapse properly.
        assert_eq!(slugify_skill_name("&&").as_deref(), Some("and-and"));
        assert_eq!(
            slugify_skill_name("foo & & bar").as_deref(),
            Some("foo-and-and-bar")
        );
        // Mixed with other punctuation (other punctuation is still dropped).
        assert_eq!(slugify_skill_name("AT&T").as_deref(), Some("at-and-t"));
        assert_eq!(slugify_skill_name("Q&A!").as_deref(), Some("q-and-a"));
    }

    #[test]
    fn test_slugify_punctuation_surrounded_by_spaces() {
        // `foo ! bar` → `foo-bar`: the two spaces would each produce a
        // dash, but consecutive dashes are collapsed.
        assert_eq!(slugify_skill_name("foo ! bar").as_deref(), Some("foo-bar"));
        assert_eq!(slugify_skill_name("foo ? bar").as_deref(), Some("foo-bar"));
        assert_eq!(
            slugify_skill_name("100 % sure").as_deref(),
            Some("100-sure")
        );
        assert_eq!(
            slugify_skill_name("foo @ bar @ baz").as_deref(),
            Some("foo-bar-baz")
        );
    }

    #[test]
    fn test_slugify_punctuation_adjacent_to_space() {
        // `foo! bar` and `foo !bar` both produce `foo-bar` — the
        // punctuation contributes nothing, the single space contributes
        // the dash.
        assert_eq!(slugify_skill_name("foo! bar").as_deref(), Some("foo-bar"));
        assert_eq!(slugify_skill_name("foo !bar").as_deref(), Some("foo-bar"));
        assert_eq!(slugify_skill_name("foo? bar").as_deref(), Some("foo-bar"));
    }

    #[test]
    fn test_slugify_leading_and_trailing_punctuation() {
        // Punctuation at the edges is dropped; there's no leading/trailing
        // dash to trim because the punctuation never became a dash in the
        // first place.
        assert_eq!(slugify_skill_name("!foo").as_deref(), Some("foo"));
        assert_eq!(slugify_skill_name("foo!").as_deref(), Some("foo"));
        assert_eq!(slugify_skill_name("!!!foo!!!").as_deref(), Some("foo"));
        assert_eq!(slugify_skill_name("?foo?").as_deref(), Some("foo"));
        assert_eq!(slugify_skill_name("...foo...").as_deref(), Some("foo"));
    }

    #[test]
    fn test_slugify_only_punctuation_returns_none() {
        assert_eq!(slugify_skill_name("!!!"), None);
        assert_eq!(slugify_skill_name("?@$"), None);
        assert_eq!(slugify_skill_name("()[]{}"), None);
        assert_eq!(slugify_skill_name(".,;:"), None);
    }

    #[test]
    fn test_slugify_mixed_punctuation_spaces_and_dashes() {
        // A messy realistic input: combination of punctuation, spaces,
        // existing dashes, and casing.
        assert_eq!(
            slugify_skill_name("  -- Hello, World!! -- ").as_deref(),
            Some("hello-world")
        );
        assert_eq!(
            slugify_skill_name("C++ vs. Rust?").as_deref(),
            Some("c-vs-rust")
        );
        assert_eq!(
            slugify_skill_name("v1.2.3-beta").as_deref(),
            Some("v123-beta")
        );
    }

    #[test]
    fn test_slugify_underscores_are_dropped() {
        // Underscores aren't a valid skill-name character and aren't
        // separators — only spaces become dashes — so underscores get
        // dropped entirely.
        assert_eq!(slugify_skill_name("foo_bar").as_deref(), Some("foobar"));
        assert_eq!(slugify_skill_name("FOO_BAR").as_deref(), Some("foobar"));
        assert_eq!(
            slugify_skill_name("snake_case style").as_deref(),
            Some("snakecase-style")
        );
    }

    #[test]
    fn test_slugify_collapses_consecutive_dashes() {
        assert_eq!(
            slugify_skill_name("foo   ---  bar").as_deref(),
            Some("foo-bar")
        );
    }

    #[test]
    fn test_slugify_trims_leading_and_trailing_dashes() {
        assert_eq!(slugify_skill_name("---foo---").as_deref(), Some("foo"));
        assert_eq!(slugify_skill_name("  foo  ").as_deref(), Some("foo"));
    }

    #[test]
    fn test_slugify_lowercases() {
        assert_eq!(slugify_skill_name("FOO BAR").as_deref(), Some("foo-bar"));
        assert_eq!(
            slugify_skill_name("MyCoolSkill").as_deref(),
            Some("mycoolskill")
        );
    }

    #[test]
    fn test_slugify_strips_non_ascii_letters() {
        // Non-ASCII chars are replaced with `-`, then collapsed.
        assert_eq!(slugify_skill_name("abc\u{00e9}").as_deref(), Some("abc"));
        assert_eq!(slugify_skill_name("\u{4e2d}\u{6587}"), None);
    }

    #[test]
    fn test_slugify_returns_none_for_empty_or_unmappable() {
        assert_eq!(slugify_skill_name(""), None);
        assert_eq!(slugify_skill_name("   "), None);
        assert_eq!(slugify_skill_name("!!!"), None);
        assert_eq!(slugify_skill_name("---"), None);
    }

    #[test]
    fn test_slugify_truncates_long_inputs() {
        let input = "a".repeat(200);
        let slug = slugify_skill_name(&input).expect("should slugify");
        assert_eq!(slug.len(), MAX_SKILL_NAME_LEN);
        assert!(slug.chars().all(|c| c == 'a'));
    }

    #[test]
    fn test_slugify_truncation_does_not_leave_trailing_dash() {
        // The 64th byte lands on a `-`, which we must strip post-truncation.
        let mut input = "a".repeat(63);
        input.push_str(" extra");
        let slug = slugify_skill_name(&input).expect("should slugify");
        assert!(!slug.ends_with('-'));
        assert!(slug.len() <= MAX_SKILL_NAME_LEN);
    }

    #[test]
    fn test_slugify_output_passes_validate_name() {
        for input in [
            "My Cool Skill",
            "Hello, World!",
            "---foo---",
            "123 abc",
            "a".repeat(200).as_str(),
        ] {
            let slug = slugify_skill_name(input).expect("should slugify");
            validate_name(&slug).unwrap_or_else(|err| {
                panic!("slug {slug:?} from {input:?} failed validation: {err}")
            });
        }
    }

    #[test]
    fn test_parse_description_too_long_loads_with_warning() {
        // zed-kask: Description length warnings are disabled. SKILL.md files
        // are reference-only — their descriptions appear in the discovery
        // catalog but are never injected into prompts. Skills execute via
        // YAML manifests in the kask registry, so description length has no
        // token-cost impact. A long description now loads cleanly with no
        // warning; this test pins that contract so the disabled warning
        // can't silently regress.
        let long_desc = "a".repeat(MAX_SKILL_DESCRIPTION_LEN + 1);
        let content = format!(
            r#"---
name: test
description: {long_desc}
---

Content.
"#
        );

        let skill = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            &content,
            SkillSource::Global,
        )
        .expect("long descriptions should load without a warning");

        assert_eq!(skill.description, long_desc);
        assert!(
            skill.load_warnings.is_empty(),
            "zed-kask disables description-length warnings; got {:?}",
            skill.load_warnings,
        );
    }

    #[test]
    fn test_parse_skill_file_content_rejects_description_too_long() {
        let long_desc = "a".repeat(MAX_SKILL_DESCRIPTION_LEN + 1);
        let content = format!(
            r#"---
name: test
description: {long_desc}
---

Content.
"#
        );

        let result = parse_skill_file_content(&content);
        assert!(result.is_err());
        let expected = format!("at most {MAX_SKILL_DESCRIPTION_LEN} bytes");
        assert!(result.unwrap_err().to_string().contains(&expected));
    }

    #[test]
    fn test_parse_empty_description() {
        let content = r#"---
name: test
description: ""
---

Content.
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_parse_file_too_large() {
        let large_content = format!(
            r#"---
name: test
description: Test skill
---

{}"#,
            "x".repeat(MAX_SKILL_FILE_SIZE + 1)
        );

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            &large_content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_parse_empty_body_after_frontmatter() {
        let content = r#"---
name: minimal-skill
description: A skill with no body content
---
"#;

        let result = parse_skill_frontmatter(
            Path::new("/skills/minimal/SKILL.md"),
            content,
            SkillSource::Global,
        );

        let skill = result.expect("Empty body should be allowed");
        assert_eq!(skill.name, "minimal-skill");
        assert_eq!(skill.description, "A skill with no body content");
    }

    #[test]
    fn test_parse_whitespace_only_body() {
        let content = "---\nname: whitespace-skill\ndescription: Test\n---\n\n   \n\n   \n";

        let result = parse_skill_frontmatter(
            Path::new("/skills/ws/SKILL.md"),
            content,
            SkillSource::Global,
        );

        let skill = result.expect("Whitespace-only body should be allowed");
        assert_eq!(skill.name, "whitespace-skill");
    }

    #[test]
    fn test_parse_skill_with_crlf_line_endings() {
        let content = "---\r\nname: crlf-skill\r\ndescription: A skill with CRLF line endings\r\n---\r\n\r\n# CRLF Skill\r\n\r\nDo the thing.\r\n";

        let result = parse_skill_frontmatter(
            Path::new("/skills/crlf-skill/SKILL.md"),
            content,
            SkillSource::Global,
        );
        let skill = result.expect("CRLF document should parse successfully");

        assert_eq!(skill.name, "crlf-skill");
        assert_eq!(skill.description, "A skill with CRLF line endings");
    }

    #[test]
    fn test_parse_skill_with_mixed_line_endings() {
        let content = "---\r\nname: mixed-skill\r\ndescription: Frontmatter uses CRLF, body uses LF\r\n---\r\n\n# Mixed Skill\n\nBody uses LF only.\n";

        let result = parse_skill_frontmatter(
            Path::new("/skills/mixed-skill/SKILL.md"),
            content,
            SkillSource::Global,
        );
        let skill = result.expect("Mixed line endings should parse successfully");

        assert_eq!(skill.name, "mixed-skill");
        assert_eq!(skill.description, "Frontmatter uses CRLF, body uses LF");
    }

    #[test]
    fn test_parse_rejects_closing_delimiter_with_trailing_chars() {
        // The only `---` after the opener has trailing junk on the same line,
        // so it isn't a valid closing delimiter and parsing must error.
        let content = "---\nname: foo\ndescription: bar\n---trailing-junk\nbody content\n";

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing closing frontmatter delimiter")
        );
    }

    #[test]
    fn test_parse_accepts_only_truly_terminated_closing_delimiter() {
        // The first `---trailing` appears inside a quoted YAML string and is
        // NOT alone on its line, so it must not be treated as the closer.
        // The real closer comes later as `\n---\n`.
        let content = "---\nname: skill-name\ndescription: A real description\nsummary: \"---trailing\"\n---\nbody content\n";

        let skill = parse_skill_frontmatter(
            Path::new("/skills/skill-name/SKILL.md"),
            content,
            SkillSource::Global,
        )
        .expect("Should pick the truly-terminated closing delimiter");

        assert_eq!(skill.name, "skill-name");
        assert_eq!(skill.description, "A real description");
    }

    #[test]
    fn test_parse_accepts_four_dashes_as_invalid_closer() {
        // A line of four dashes is NOT a valid closing delimiter; with no
        // valid closer following, parsing must error.
        let content = "---\nname: foo\ndescription: bar\n----\nbody content\n";

        let result = parse_skill_frontmatter(
            Path::new("/skills/test/SKILL.md"),
            content,
            SkillSource::Global,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing closing frontmatter delimiter")
        );
    }

    #[gpui::test]
    async fn test_load_skills_from_empty_directory(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/skills", serde_json::json!({})).await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;
        assert!(results.is_empty());
    }

    #[gpui::test]
    async fn test_load_single_skill(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "my-skill": {
                    "SKILL.md": "---\nname: my-skill\ndescription: Test skill\n---\n\n# Instructions\nDo stuff."
                }
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 1);
        let skill = results[0].as_ref().expect("Should load successfully");
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "Test skill");
    }

    #[gpui::test]
    async fn test_load_symlinked_skill_directory(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/external/my-skill",
            serde_json::json!({
                "SKILL.md": "---\nname: my-skill\ndescription: Symlinked skill\n---\n\n# Instructions"
            }),
        )
        .await;
        fs.create_dir(Path::new("/skills")).await.unwrap();
        fs.create_symlink(
            Path::new("/skills/my-skill"),
            PathBuf::from("/external/my-skill"),
        )
        .await
        .unwrap();

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 1);
        let skill = results[0].as_ref().expect("Should load successfully");
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "Symlinked skill");
        assert_eq!(
            skill.skill_file_path,
            Path::new("/skills/my-skill/SKILL.md")
        );
    }

    #[gpui::test]
    async fn test_load_nested_skills(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "skill-one": {
                    "SKILL.md": "---\nname: skill-one\ndescription: First skill\n---\n\nContent one"
                },
                "skill-two": {
                    "SKILL.md": "---\nname: skill-two\ndescription: Second skill\n---\n\nContent two"
                }
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"skill-one"));
        assert!(names.contains(&"skill-two"));
    }

    #[gpui::test]
    async fn test_load_skills_returns_results_sorted_by_path(cx: &mut TestAppContext) {
        // `apply_skill_overrides` resolves same-source name collisions
        // by keeping the first entry in iteration order. Without a
        // stable sort here, the result depends on `fs.read_dir`, which
        // is OS/filesystem-dependent. Assert the contract: results
        // come back sorted by skill file path regardless of insertion
        // order.
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "charlie": {
                    "SKILL.md": "---\nname: charlie\ndescription: C\n---\n\nC"
                },
                "alpha": {
                    "SKILL.md": "---\nname: alpha\ndescription: A\n---\n\nA"
                },
                "bravo": {
                    "SKILL.md": "---\nname: bravo\ndescription: B\n---\n\nB"
                },
                "delta": {
                    "SKILL.md": "No frontmatter, will fail"
                },
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 4);

        let paths: Vec<PathBuf> = results
            .iter()
            .map(|r| match r {
                Ok(skill) => skill.skill_file_path.clone(),
                Err(error) => error.path.clone(),
            })
            .collect();

        let mut expected = paths.clone();
        expected.sort();
        assert_eq!(paths, expected);
    }

    #[gpui::test]
    async fn test_load_ignores_non_skill_files(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "my-skill": {
                    "SKILL.md": "---\nname: my-skill\ndescription: Test\n---\n\nContent"
                },
                "not-a-skill.txt": "This is not a skill",
                "some-dir": {
                    "other-file.md": "Not a SKILL.md"
                }
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 1);
        let skill = results[0].as_ref().expect("Should load successfully");
        assert_eq!(skill.name, "my-skill");
    }

    #[gpui::test]
    async fn test_load_returns_errors_for_invalid_skills(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "valid-skill": {
                    "SKILL.md": "---\nname: valid-skill\ndescription: Valid\n---\n\nContent"
                },
                "invalid-skill": {
                    "SKILL.md": "No frontmatter here"
                }
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 2);

        let (successes, errors): (Vec<_>, Vec<_>) = results.iter().partition(|r| r.is_ok());

        assert_eq!(successes.len(), 1);
        assert_eq!(errors.len(), 1);

        let error = errors[0].as_ref().unwrap_err();
        assert!(error.path.to_string_lossy().contains("invalid-skill"));
    }

    #[gpui::test]
    async fn test_load_from_nonexistent_directory(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/nonexistent"),
            SkillSource::Global,
        )
        .await;

        assert!(results.is_empty());
    }

    #[test]
    fn test_skill_summary_from_skill() {
        let skill = Skill {
            name: "test-skill".to_string(),
            description: "A test description".to_string(),
            source: SkillSource::Global,
            directory_path: PathBuf::from("/skills/test-skill"),
            skill_file_path: PathBuf::from("/skills/test-skill/SKILL.md"),
            load_warnings: Vec::new(),
            disable_model_invocation: false,
            dependencies: Vec::new(),
            core: false,
        };

        let summary = SkillSummary::from(&skill);
        assert_eq!(summary.name, "test-skill");
        assert_eq!(summary.description, "A test description");
        assert_eq!(summary.location, "/skills/test-skill/SKILL.md");
    }

    #[gpui::test]
    async fn test_nested_skill_md_inside_skill_resources_is_not_loaded(cx: &mut TestAppContext) {
        // We only look at immediate children of the skills root, so a
        // `SKILL.md` nested inside a skill's resources directory cannot
        // accidentally be picked up as a separate skill.
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "outer": {
                    "SKILL.md": "---\nname: outer\ndescription: Outer skill\n---\n\nBody",
                    "references": {
                        "SKILL.md": "---\nname: bogus-inner\ndescription: Should not load\n---\n\nBody"
                    },
                },
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        let names: Vec<&str> = results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec!["outer"]);
    }

    #[gpui::test]
    async fn test_load_oversized_skill_file_short_circuits(cx: &mut TestAppContext) {
        // A `SKILL.md` whose size exceeds `MAX_SKILL_FILE_SIZE` must be
        // rejected via metadata before we read its contents into memory.
        // Otherwise a stray multi-GB file dropped into a skill directory
        // would OOM the application before `parse_skill`'s size check fires.
        let fs = FakeFs::new(cx.executor());
        let oversized_body = "x".repeat(MAX_SKILL_FILE_SIZE + 1);
        let oversized_content = format!(
            "---\nname: huge\ndescription: Too big\n---\n\n{}",
            oversized_body
        );
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "huge": {
                    "SKILL.md": oversized_content,
                }
            }),
        )
        .await;

        let results = load_skills_from_directory(
            &(fs as Arc<dyn Fs>),
            Path::new("/skills"),
            SkillSource::Global,
        )
        .await;

        assert_eq!(results.len(), 1);
        let err = results[0].as_ref().expect_err("Oversized file must error");
        assert!(
            err.message.contains("exceeds maximum size"),
            "unexpected error message: {}",
            err.message
        );
    }

    #[gpui::test]
    async fn test_load_skill_frontmatter_parses_metadata_without_body(cx: &mut TestAppContext) {
        // `load_skill_frontmatter` should read just enough of the file to
        // parse the frontmatter and return a `Skill` with name/description/
        // disable_model_invocation populated. The body is intentionally not
        // surfaced; callers go through `read_skill_body` for that.
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "my-skill": {
                    "SKILL.md": "---\nname: my-skill\ndescription: A skill for tests\ndisable-model-invocation: true\n---\n\n# Body\n\nLots of body text here.\n"
                }
            }),
        )
        .await;

        let skill = load_skill_frontmatter(
            fs as Arc<dyn Fs>,
            PathBuf::from("/skills/my-skill/SKILL.md"),
            SkillSource::Global,
        )
        .await
        .expect("frontmatter should parse");

        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "A skill for tests");
        assert!(skill.disable_model_invocation);
        assert_eq!(
            skill.skill_file_path,
            PathBuf::from("/skills/my-skill/SKILL.md")
        );
        assert_eq!(skill.directory_path, PathBuf::from("/skills/my-skill"));
    }

    #[gpui::test]
    async fn test_read_skill_body_returns_trimmed_body(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "my-skill": {
                    "SKILL.md": "---\nname: my-skill\ndescription: Test skill\n---\n\n# Instructions\n\nDo the thing.\n\n"
                }
            }),
        )
        .await;

        let body = read_skill_body(fs.as_ref(), Path::new("/skills/my-skill/SKILL.md"))
            .await
            .expect("body should load");

        // Trimmed: no leading blank line after the closing `---`, and no
        // trailing whitespace.
        assert_eq!(body, "# Instructions\n\nDo the thing.");
    }

    #[gpui::test]
    async fn test_read_skill_body_accepts_description_too_long(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        let long_desc = "a".repeat(MAX_SKILL_DESCRIPTION_LEN + 1);
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "long-description": {
                    "SKILL.md": format!("---\nname: long-description\ndescription: {long_desc}\n---\n\nBody")
                }
            }),
        )
        .await;

        let body = read_skill_body(fs.as_ref(), Path::new("/skills/long-description/SKILL.md"))
            .await
            .expect("body should load despite description-length warning");

        assert_eq!(body, "Body");
    }

    #[gpui::test]
    async fn test_read_skill_body_for_skill_without_body(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "empty": {
                    "SKILL.md": "---\nname: empty\ndescription: No body\n---\n"
                }
            }),
        )
        .await;

        let body = read_skill_body(fs.as_ref(), Path::new("/skills/empty/SKILL.md"))
            .await
            .expect("body should load");

        assert!(body.is_empty(), "expected empty body, got: {body:?}");
    }

    #[test]
    fn is_agents_skills_path_simple_positive() {
        assert!(is_agents_skills_path(Path::new(
            "foo/.agents/skills/my-skill/SKILL.md"
        )));
    }

    #[test]
    fn is_agents_skills_path_simple_negative() {
        assert!(!is_agents_skills_path(Path::new("foo/bar/baz")));
    }

    #[test]
    fn is_agents_skills_path_double_agents() {
        // `foo/.agents/.agents/skills` contains a `.agents/skills` pair at
        // depths 2-3. Any-depth matching catches it; this is intentional, so
        // a `.agents/skills` directory the user wasn't expecting to be
        // touched still prompts for confirmation.
        assert!(is_agents_skills_path(Path::new(
            "foo/.agents/.agents/skills"
        )));
    }

    #[test]
    fn is_agents_skills_path_agents_without_skills() {
        assert!(!is_agents_skills_path(Path::new("foo/.agents/other")));
    }

    #[test]
    fn is_agents_skills_path_at_start() {
        assert!(is_agents_skills_path(Path::new(".agents/skills")));
    }

    #[test]
    fn is_agents_skills_path_trailing_agents() {
        assert!(!is_agents_skills_path(Path::new("foo/.agents")));
    }

    #[test]
    fn is_agents_skills_path_deep_match() {
        // Any-depth matching: nested `.agents/skills` directories — e.g.
        // inside vendored sources — are flagged too. We prefer the extra
        // prompt over silently letting the agent edit something named
        // `.agents/skills`.
        assert!(is_agents_skills_path(Path::new("a/b/.agents/skills/x.txt")));
        assert!(is_agents_skills_path(Path::new(
            "some/random/place/.agents/skills/foo"
        )));
    }

    #[test]
    fn is_agents_skills_path_absolute() {
        // Absolute paths into a project-local `.agents/skills/` are caught
        // by the same consecutive-component match.
        assert!(is_agents_skills_path(Path::new(
            "/Users/foo/project/.agents/skills/my-skill/SKILL.md"
        )));
        assert!(!is_agents_skills_path(Path::new("/etc/hosts")));
    }

    #[test]
    fn is_agents_skills_path_case_insensitive() {
        // Filesystems on macOS/Windows are case-insensitive by default; the
        // classifier must agree.
        assert!(is_agents_skills_path(Path::new(".AGENTS/skills/foo")));
        assert!(is_agents_skills_path(Path::new(".agents/SKILLS/foo")));
        assert!(is_agents_skills_path(Path::new(
            "project/.AGENTS/SKILLS/foo"
        )));
    }

    #[test]
    fn validate_name_accepts_valid_names() {
        assert!(validate_name("draft-pr").is_ok());
        assert!(validate_name("a").is_ok());
        assert!(validate_name("skill1").is_ok());
        assert!(validate_name(&"a".repeat(MAX_SKILL_NAME_LEN)).is_ok());
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn validate_name_rejects_uppercase() {
        assert!(validate_name("Draft-PR").is_err());
    }

    #[test]
    fn validate_name_rejects_leading_and_trailing_hyphens() {
        assert!(validate_name("-draft").is_err());
        assert!(validate_name("draft-").is_err());
    }

    #[test]
    fn validate_name_rejects_invalid_chars() {
        assert!(validate_name("draft_pr").is_err());
        assert!(validate_name("draft pr").is_err());
        assert!(validate_name("draft.pr").is_err());
    }

    #[test]
    fn validate_name_rejects_too_long() {
        assert!(validate_name(&"a".repeat(MAX_SKILL_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn validate_description_accepts_valid() {
        assert!(validate_description("A useful skill").is_ok());
    }

    #[test]
    fn validate_description_rejects_empty_and_whitespace_only() {
        assert!(validate_description("").is_err());
        assert!(validate_description("   ").is_err());
        assert!(validate_description("\t\n ").is_err());
    }

    #[test]
    fn validate_description_rejects_too_long() {
        assert!(validate_description(&"a".repeat(MAX_SKILL_DESCRIPTION_LEN + 1)).is_err());
    }

    #[test]
    fn validate_description_length_is_measured_in_bytes() {
        // "é" is 2 bytes in UTF-8. A string of MAX/2 + 1 "é" characters has
        // only ~MAX/2 + 1 chars but exceeds MAX bytes, so it must be
        // rejected by a byte-based validator (and accepted by a char-based
        // one). This regression-tests the byte semantics that strict
        // validation and load-time warnings both rely on.
        let chars = MAX_SKILL_DESCRIPTION_LEN / 2 + 1;
        let description = "é".repeat(chars);
        assert!(description.chars().count() <= MAX_SKILL_DESCRIPTION_LEN);
        assert!(description.len() > MAX_SKILL_DESCRIPTION_LEN);
        assert!(validate_description(&description).is_err());
    }

    #[test]
    fn slugify_output_always_passes_validate_name() {
        for input in [
            "foo",
            "Foo Bar",
            "rock & roll",
            "---weird---",
            "a".repeat(200).as_str(),
        ] {
            if let Some(slug) = slugify_skill_name(input) {
                assert!(
                    validate_name(&slug).is_ok(),
                    "slug {slug:?} from {input:?} failed validate_name"
                );
            }
        }
    }

    // zed-kask: Shipped skills (authored in .agents/skills/) must parse
    // cleanly via the strict `validate_description` path (hard-rejects
    // >1024-byte descriptions). This is a deliberate deviation from the
    // disk-loaded path (`parse_skill_file_content_for_loading`), which
    // accepts long descriptions with no warning — see
    // `test_parse_description_too_long_loads_with_warning`. The rationale:
    // shipped skills are authored by us and should meet the strict bar;
    // user-installed skills on disk get the lenient path. This test pins
    // that contract so a shipped skill with an over-long description can't
    // silently regress — it will fail this test AND fail the build-time
    // check in `build.rs`.
    #[test]
    fn shipped_skill_seed_all_parse_without_errors() {
        let seed = shipped_skill_seed();
        // Sanity: we ship more than a handful of skills.
        assert!(
            seed.len() > 10,
            "expected the shipped skill seed to include the kask catalog, got {}",
            seed.len()
        );
        // Every shipped skill must parse cleanly. If a skill is missing
        // from this list, check the test output for a parse warning.
        let known_skills = [
            "bug-hunt",
            "lora-training",
            "kali-audit",
            "metacognition",
            "pragmatic-semantics",
            "pragmatic-cybernetics",
            "self-improvement",
            "tdd",
        ];
        let parsed_names: std::collections::HashSet<&str> =
            seed.iter().map(|(name, _)| *name).collect();
        for name in known_skills {
            assert!(
                parsed_names.contains(name),
                "shipped skill '{name}' is missing from the seed payload — \
                 most likely cause: description exceeds {MAX_SKILL_DESCRIPTION_LEN} bytes \
                 (caught at build time) or the SKILL.md was removed from .agents/skills/."
            );
        }
        // Every shipped SKILL.md must parse cleanly (frontmatter valid,
        // description within the strict limit).
        for (name, content) in seed {
            let (metadata, _body) = extract_frontmatter(content)
                .unwrap_or_else(|e| panic!("shipped skill '{name}' failed to parse: {e}"));
            validate_name(&metadata.name)
                .unwrap_or_else(|e| panic!("shipped skill '{name}' has invalid name: {e}"));
            validate_description(&metadata.description)
                .unwrap_or_else(|e| panic!("shipped skill '{name}' has invalid description: {e}"));
        }
    }

    // Seeding materialises the shipped SKILL.md files to disk so the catalog
    // is built from disk (editable, no compiled-in runtime source). Pins:
    // - a missing skill is written;
    // - an existing USER (non-core) skill is never overwritten (user edits are
    //   sovereign). Core skills are always overwritten — that contract is
    //   pinned by `test_seed_shipped_skills_overwrites_core_skills`.
    #[gpui::test]
    async fn test_seed_shipped_skills_writes_missing_and_preserves_existing(
        cx: &mut TestAppContext,
    ) {
        let fs = FakeFs::new(cx.executor());
        fs.create_dir(Path::new("/skills")).await.unwrap();

        seed_shipped_skills(fs.as_ref(), Path::new("/skills")).await;

        // A known shipped USER (non-core) skill must be present on disk after
        // seeding. `lora-training` is non-core, so user edits to it are
        // sovereign and must survive re-seeding.
        let lora_training = Path::new("/skills/lora-training/SKILL.md");
        assert!(
            fs.is_file(lora_training).await,
            "shipped skill 'lora-training' should be seeded to disk"
        );

        // Overwrite the seeded file with a user edit, then re-seed. The
        // user edit must survive — seeding never clobbers existing files.
        fs.write(
            lora_training,
            b"---\nname: lora-training\ndescription: My custom edit\n---\n",
        )
        .await
        .unwrap();
        seed_shipped_skills(fs.as_ref(), Path::new("/skills")).await;
        let content = fs.load(lora_training).await.unwrap();
        assert!(
            content.contains("My custom edit"),
            "user edit to a shipped user skill must survive re-seeding; got: {content}"
        );
    }

    // The `CORE_SKILL_NAMES` constant is the single source of truth for which
    // skills are core. Every shipped SKILL.md with `core: true` in its
    // frontmatter must appear in `CORE_SKILL_NAMES`, and every name in
    // `CORE_SKILL_NAMES` must have a shipped SKILL.md marked `core: true`.
    // A drift in either direction is a contract violation: a frontmatter
    // `core: true` not in the constant would be seeded-once (editable) instead
    // of always-overwritten, and a constant entry without frontmatter would
    // be overwritten on every startup against the user's will.
    #[test]
    fn test_core_skill_names_constant_matches_shipped_core_frontmatter() {
        let seed = shipped_skill_seed();

        // Skills whose shipped SKILL.md frontmatter has `core: true`.
        let frontmatter_core: std::collections::HashSet<&str> = seed
            .iter()
            .filter_map(|(name, content)| {
                let (metadata, _body) = extract_frontmatter(content)
                    .unwrap_or_else(|e| panic!("shipped skill '{name}' failed to parse: {e}"));
                if metadata.core { Some(*name) } else { None }
            })
            .collect();

        let constant_core: std::collections::HashSet<&str> =
            CORE_SKILL_NAMES.iter().copied().collect();

        // Every frontmatter-core skill must be in the constant.
        let missing_from_constant: Vec<&str> = frontmatter_core
            .difference(&constant_core)
            .copied()
            .collect();
        assert!(
            missing_from_constant.is_empty(),
            "skills with `core: true` frontmatter but absent from CORE_SKILL_NAMES: \
             {missing_from_constant:?} — add them to CORE_SKILL_NAMES or remove the \
             frontmatter flag"
        );

        // Every constant entry must have a shipped SKILL.md marked core.
        let missing_from_frontmatter: Vec<&str> = constant_core
            .difference(&frontmatter_core)
            .copied()
            .collect();
        assert!(
            missing_from_frontmatter.is_empty(),
            "CORE_SKILL_NAMES entries with no shipped SKILL.md marked `core: true`: \
             {missing_from_frontmatter:?} — add `core: true` to the SKILL.md frontmatter \
             or remove the name from CORE_SKILL_NAMES"
        );

        // Sanity: the core set is non-empty and a strict subset of shipped.
        assert!(
            !constant_core.is_empty(),
            "CORE_SKILL_NAMES must not be empty"
        );
        let shipped_names: std::collections::HashSet<&str> =
            seed.iter().map(|(name, _)| *name).collect();
        assert!(
            constant_core.is_subset(&shipped_names),
            "CORE_SKILL_NAMES references skills with no shipped SKILL.md"
        );
    }

    #[test]
    fn test_parse_skill_frontmatter_accepts_core_skill_with_reserved_name() {
        // A legitimate core skill (frontmatter `core: true`) bearing a
        // reserved name must pass — it is the only kind of skill allowed
        // to use a reserved name.
        let content =
            "---\nname: create-skill\ncore: true\ndescription: Legitimate core skill\n---\nbody\n";
        let skill = parse_skill_frontmatter(
            Path::new("/skills/create-skill/SKILL.md"),
            content,
            SkillSource::Global,
        )
        .expect("core skill with reserved name must be accepted");
        assert_eq!(skill.name, "create-skill");
        assert!(skill.core);
    }

    #[test]
    fn test_is_reserved_skill_name_matches_core_skill_names() {
        // `is_reserved_skill_name` is an alias for `is_core_skill` — the
        // reserved-name set is exactly the core-skill-name set.
        assert!(is_reserved_skill_name("create-skill"));
        assert!(is_reserved_skill_name("bug-hunt"));
        assert!(!is_reserved_skill_name("my-user-skill"));
        assert!(!is_reserved_skill_name(""));
    }

    // Core skills are always overwritten on every startup so user edits
    // cannot break system-critical functionality. This pins the overwrite
    // half of the seeder contract (the preserve-existing half is pinned by
    // `test_seed_shipped_skills_writes_missing_and_preserves_existing`).
    #[gpui::test]
    async fn test_seed_shipped_skills_overwrites_core_skills(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.create_dir(Path::new("/skills")).await.unwrap();

        // Seed once — the core skill lands on disk.
        seed_shipped_skills(fs.as_ref(), Path::new("/skills")).await;
        let create_skill = Path::new("/skills/create-skill/SKILL.md");
        assert!(
            fs.is_file(create_skill).await,
            "core skill 'create-skill' should be seeded to disk"
        );
        let original = fs.load(create_skill).await.unwrap();

        // A user edit (or corruption) is written over the core skill.
        fs.write(
            create_skill,
            b"---\nname: create-skill\ndescription: TAMPERED\n---\n",
        )
        .await
        .unwrap();

        // Re-seed — the core skill must be overwritten with the shipped copy.
        seed_shipped_skills(fs.as_ref(), Path::new("/skills")).await;
        let after = fs.load(create_skill).await.unwrap();
        assert_eq!(
            after, original,
            "core skill must be overwritten on re-seed; user edits are not sovereign \
             for core skills. Got tampered content."
        );
        assert!(
            !after.contains("TAMPERED"),
            "tampered content survived re-seed — core overwrite is broken: {after}"
        );
    }
}
