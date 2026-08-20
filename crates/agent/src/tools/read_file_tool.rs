use action_log::ActionLog;
use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context as _, Result, anyhow};
use futures::FutureExt as _;
use gpui::{App, Entity, SharedString, Task};
use indoc::formatdoc;
use language::Point;
use language_model::{LanguageModelImage, LanguageModelImageExt, LanguageModelToolResultContent};
use project::{AgentLocation, ImageItem, Project, WorktreeSettings, image_store};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::path::Path;
use std::sync::Arc;
use util::markdown::MarkdownCodeBlock;

fn tool_content_err(e: impl std::fmt::Display) -> LanguageModelToolResultContent {
    LanguageModelToolResultContent::from(e.to_string())
}

/// Detect a read of a skill's `SKILL.md` — the discovery-only catalog entry
/// whose body is never injected (see `skill_tool.rs`: body injection is disabled
/// in zed-kask). Reading it bypasses the manifest cascade, the gas/OCAP
/// membrane, and the convergence loop.
///
/// This is the enforcement point for the invariant the system prompt states.
/// It was previously advisory (`log::warn!` only), which made the prompt prose
/// the *sole* defense — and per the `.rules` "advertised invariants need
/// enforcement points" trap, an invariant with no gate is a declaration. The
/// model's trained prior actively pushes against it: every other major agent
/// system loads skill bodies as prose, so "read the skill file" is the
/// statistically expected action.
///
/// Only the catalog file itself is gated. Resource files *inside* a skill
/// directory (templates, references, scripts) stay readable — a cascade result
/// may legitimately direct the model to them.
fn is_skill_catalog_file(path: &Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
        return false;
    }
    // A skills directory is one whose parent is `.agents` (project) or the
    // global skills root (D28: `{kask_data_dir}/skills/` or legacy
    // `paths::data_dir()/agents/skills/`). Walk ancestors so nested layouts
    // (`.../skills/_marketplace/<name>/SKILL.md`) match too.
    //
    // zed-kask: D28 — the global skills dir is resolved via
    // `agent_skills::global_skills_dir()` which honors the override hook.
    // We check `starts_with` against it so any kask data root (custom
    // `HKASK_DATA_DIR`, XDG default, etc.) is accepted.
    let global_skills_dir = agent_skills::global_skills_dir();
    path.ancestors().skip(1).any(|anc| {
        anc.file_name().and_then(|n| n.to_str()) == Some("skills")
            && (anc
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n == ".agents" || n == "agents")
                .unwrap_or(false)
                || anc.starts_with(&global_skills_dir))
    })
}

/// Refuse a `SKILL.md` read, returning the redirect the model should act on.
///
/// Returns `Err` (the tool-error channel) when `resolved_path` is a skill
/// catalog file, so the model receives the redirect as the tool result rather
/// than the file body.
///
/// The `log::warn!` counts *blocked* attempts — the measurement that tells us
/// whether the system prompt's prohibition prose is still carrying load now that
/// a gate exists. It is deliberately **not** in the `reg.skill.*` namespace:
/// `reg.skill.<id>.<phase>` is reserved for per-skill feedback spans
/// (`RegulationLedger::record_skill_span`, CI-enforced by
/// `kask/scripts/check-skill-span-namespace.sh`), and this is a tool-boundary
/// event about a *refused file read*, not a skill's own outcome. It is also a
/// `log` record rather than a `tracing` event because the `agent` crate has no
/// `tracing` dependency and no `log`→`tracing` bridge is installed, so a
/// `target:`-style span here would not reach the regulation ledger.
/// Telemetry key for a refused `SKILL.md` read. Named as a constant so the
/// namespace test asserts on the value actually emitted rather than on this
/// file's source text.
const BLOCKED_READ_TELEMETRY_KEY: &str = "skill.catalog_read_blocked";

fn refuse_skill_catalog_read(
    _resolved_path: &Path,
    _requested_path: &str,
) -> Result<(), LanguageModelToolResultContent> {
    // zed-kask D1 revert: SKILL.md body injection is restored. `read_file`
    // may read SKILL.md files — the `skill` tool also reads them and injects
    // the body into the conversation. No gate needed.
    Ok(())
}

/// Resolves the optional `start_line` / `end_line` inputs from the tool schema
/// to a concrete 1-indexed, inclusive `(start, end)` line range:
///
/// - `start` defaults to 1 and is clamped to `>= 1` (the model occasionally passes
///   `0` despite instructions to be 1-indexed).
/// - `end` defaults to `u32::MAX` and is clamped to `>= start`, so callers always
///   read at least one line even when the model passes `end < start`.
///
/// Callers translate this 1-indexed inclusive range to whichever coordinate
/// system their slicing API wants (e.g. 0-indexed exclusive row ranges for
/// `Buffer::text_for_range`).
fn resolve_line_range(start_line: Option<u32>, end_line: Option<u32>) -> (u32, u32) {
    let start = start_line.unwrap_or(1).max(1);
    let end = end_line.unwrap_or(u32::MAX).max(start);
    (start, end)
}

/// Prefixes each line of `text` with its line number in `cat -n` format:
/// the line number is right-aligned in a 6-character field, followed by a
/// single tab, followed by the line's original content (including its
/// trailing newline if present). Numbering starts at `start_line`.
///
/// This format matches what the model expects in the edit tool, where the
/// line number prefix is `line number + tab` and everything after the tab is
/// the actual file content to match.
fn format_with_line_numbers(text: &str, start_line: u32) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut output = String::with_capacity(text.len() + text.len() / 4);
    write_lines_numbered(&mut output, std::iter::once(text), start_line);
    output
}

/// Streams `cat -n`-style line-numbered output directly into `output` from an
/// iterator of string slices. Chunks do not need to align to line boundaries:
/// a single chunk may contain multiple newlines, span multiple lines, or end
/// mid-line. This lets callers consume `Buffer::text_for_range`'s `Chunks`
/// iterator without materializing the unnumbered text first.
fn write_lines_numbered<'a>(
    output: &mut String,
    chunks: impl IntoIterator<Item = &'a str>,
    start_line: u32,
) {
    use std::fmt::Write as _;

    let mut line_number = start_line;
    let mut at_line_start = true;
    for chunk in chunks {
        let mut rest = chunk;
        while !rest.is_empty() {
            if at_line_start {
                // Writes to a `String` are infallible, so the `Result` can be ignored.
                let _ = write!(output, "{line_number:>6}\t");
                at_line_start = false;
            }
            match rest.find('\n') {
                Some(nl) => {
                    let (head, tail) = rest.split_at(nl + 1);
                    output.push_str(head);
                    line_number = line_number.saturating_add(1);
                    at_line_start = true;
                    rest = tail;
                }
                None => {
                    output.push_str(rest);
                    break;
                }
            }
        }
    }
}

/// Read a file under the global skills directory directly via the filesystem,
/// bypassing project/worktree resolution. Used for skill resources that live
/// outside any worktree.
///
/// Skill resources are expected to be plain text (Markdown, scripts, configs).
/// Image rendering, the action log, and the buffer-backed outline path are
/// intentionally not exercised here — those are project concerns.
async fn read_global_skill_file(
    canonical_path: &Path,
    fs: &dyn fs::Fs,
    start_line: Option<u32>,
    end_line: Option<u32>,
    requested_path: &str,
    event_stream: &ToolCallEventStream,
) -> Result<LanguageModelToolResultContent, LanguageModelToolResultContent> {
    let content = fs.load(canonical_path).await.map_err(tool_content_err)?;

    event_stream.update_fields(acp::ToolCallUpdateFields::new().locations(vec![
        acp::ToolCallLocation::new(canonical_path)
            .line(start_line.map(|line| line.saturating_sub(1))),
    ]));

    let (raw_text, first_line_number) = if start_line.is_some() || end_line.is_some() {
        // `split_inclusive` keeps each line's terminator attached, so CRLF stays
        // CRLF and the trailing newline of the last returned line is preserved —
        // matching `Buffer::text_for_range` in the buffer-backed path.
        let (start, end) = resolve_line_range(start_line, end_line);
        let lines: Vec<&str> = content.split_inclusive('\n').collect();
        let start_idx = (start as usize).saturating_sub(1).min(lines.len());
        let end_idx = (end as usize).min(lines.len()).max(start_idx);
        (lines[start_idx..end_idx].concat(), start)
    } else {
        (content, 1)
    };

    let result_text = format_with_line_numbers(&raw_text, first_line_number);

    let markdown = MarkdownCodeBlock {
        tag: requested_path,
        text: &result_text,
    }
    .to_string();
    event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
        acp::ToolCallContent::Content(acp::Content::new(markdown)),
    ]));

    Ok(result_text.into())
}

use super::deserialize_optional_u32_from_maybe_string;
use super::tool_permissions::{
    ResolvedProjectPath, authorize_symlink_access, canonicalize_worktree_roots,
    resolve_global_skill_path, resolve_project_path,
};
use crate::{AgentTool, ToolCallEventStream, ToolInput, outline};

/// Reads the content of the given file in the project.
///
/// - Never attempt to read a path that hasn't been previously mentioned.
/// - For large files, this tool returns a file outline with symbol names and line numbers instead of the full content.
///   This outline IS a successful response - use the line numbers to read specific sections with start_line/end_line.
///   Do NOT retry reading the same file without line numbers if you receive an outline.
/// - This tool supports reading image files. Supported formats: PNG, JPEG, WebP, GIF, BMP, TIFF.
///   Image files are returned as visual content that you can analyze directly.
/// - A skill's `SKILL.md` cannot be read: it is a discovery-only catalog entry, and this tool
///   refuses it. Invoke the `skill` tool instead. Other files inside a skill directory are readable.
///
/// The only supported path outside the project is the global skills directory or a descendant, for global agent skills.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileToolInput {
    /// The relative path of the file to read.
    ///
    /// This path should never be absolute, and the first component of the path should always be a root directory in a project, unless it's a global agent skill under the global skills directory.
    ///
    /// <example>
    /// If the project has the following root directories:
    ///
    /// - /a/b/directory1
    /// - /c/d/directory2
    ///
    /// If you want to access `file.txt` in `directory1`, you should use the path `directory1/file.txt`.
    /// If you want to access `file.txt` in `directory2`, you should use the path `directory2/file.txt`.
    /// </example>
    ///
    /// <example>
    /// To read a global agent skill file, use the global skills directory path shown in the system prompt.
    /// </example>
    pub path: String,
    /// Optional line number to start reading on (1-based index)
    #[serde(
        default,
        deserialize_with = "deserialize_optional_u32_from_maybe_string"
    )]
    pub start_line: Option<u32>,
    /// Optional line number to end reading on (1-based index, inclusive)
    #[serde(
        default,
        deserialize_with = "deserialize_optional_u32_from_maybe_string"
    )]
    pub end_line: Option<u32>,
}

pub struct ReadFileTool {
    project: Entity<Project>,
    action_log: Entity<ActionLog>,
    update_agent_location: bool,
}

impl ReadFileTool {
    pub fn new(
        project: Entity<Project>,
        action_log: Entity<ActionLog>,
        update_agent_location: bool,
    ) -> Self {
        Self {
            project,
            action_log,
            update_agent_location,
        }
    }
}

impl AgentTool for ReadFileTool {
    type Input = ReadFileToolInput;
    type Output = LanguageModelToolResultContent;

    const NAME: &'static str = "read_file";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Read
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        cx: &mut App,
    ) -> SharedString {
        if let Ok(input) = input
            && let Some(project_path) = self.project.read(cx).find_project_path(&input.path, cx)
            && let Some(path) = self
                .project
                .read(cx)
                .short_full_path_for_project_path(&project_path, cx)
        {
            match (input.start_line, input.end_line) {
                (Some(start), Some(end)) => {
                    format!("Read file `{path}` (lines {}-{})", start, end,)
                }
                (Some(start), None) => {
                    format!("Read file `{path}` (from line {})", start)
                }
                _ => format!("Read file `{path}`"),
            }
            .into()
        } else {
            "Read file".into()
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<LanguageModelToolResultContent, LanguageModelToolResultContent>> {
        let project = self.project.clone();
        let action_log = self.action_log.clone();
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(tool_content_err)?;
            let fs = project.read_with(cx, |project, _cx| project.fs().clone());

            // Fast path: if the model passes a path that resolves under the
            // global skills directory, read it directly via the
            // filesystem. Global skills live outside any worktree, so the
            // standard project-path machinery would refuse them.
            if let Some(skill_path) =
                resolve_global_skill_path(Path::new(&input.path), fs.as_ref()).await
            {
                refuse_skill_catalog_read(&skill_path, &input.path)?;
                return read_global_skill_file(
                    &skill_path,
                    fs.as_ref(),
                    input.start_line,
                    input.end_line,
                    &input.path,
                    &event_stream,
                )
                .await;
            }

            let canonical_roots = canonicalize_worktree_roots(&project, &fs, cx).await;

            let (project_path, symlink_canonical_target) =
                project.read_with(cx, |project, cx| {
                    let resolved =
                        resolve_project_path(project, &input.path, &canonical_roots, cx)?;
                    anyhow::Ok(match resolved {
                        ResolvedProjectPath::Safe(path) => (path, None),
                        ResolvedProjectPath::SymlinkEscape {
                            project_path,
                            canonical_target,
                        } => (project_path, Some(canonical_target)),
                    })
                }).map_err(tool_content_err)?;

            let abs_path = project
                .read_with(cx, |project, cx| {
                    project.absolute_path(&project_path, cx)
                })
                .ok_or_else(|| {
                    anyhow!("Failed to convert {} to absolute path", input.path)
                }).map_err(tool_content_err)?;

            refuse_skill_catalog_read(&abs_path, &input.path)?;

            // Check settings exclusions synchronously
            project.read_with(cx, |_project, cx| {
                let global_settings = WorktreeSettings::get_global(cx);
                if global_settings.is_path_excluded(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the global `file_scan_exclusions` setting: {}",
                        input.path
                    );
                }

                if global_settings.is_path_private(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the global `private_files` setting: {}",
                        input.path
                    );
                }

                let worktree_settings = WorktreeSettings::get(Some((&project_path).into()), cx);
                if worktree_settings.is_path_excluded(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the worktree `file_scan_exclusions` setting: {}",
                        input.path
                    );
                }

                if worktree_settings.is_path_private(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the worktree `private_files` setting: {}",
                        input.path
                    );
                }

                anyhow::Ok(())
            }).map_err(tool_content_err)?;

            if fs.is_dir(&abs_path).await {
                return Err(tool_content_err(format!(
                    "{} is a directory, not a file. Use the list_directory tool to explore directory contents.",
                    input.path
                )));
            }

            if let Some(canonical_target) = &symlink_canonical_target {
                let authorize = cx.update(|cx| {
                    authorize_symlink_access(
                        Self::NAME,
                        &input.path,
                        canonical_target,
                        &event_stream,
                        cx,
                    )
                });
                authorize.await.map_err(tool_content_err)?;
            }

            let file_path = input.path.clone();

            cx.update(|_cx| {
                event_stream.update_fields(acp::ToolCallUpdateFields::new().locations(vec![
                    acp::ToolCallLocation::new(&abs_path)
                        .line(input.start_line.map(|line| line.saturating_sub(1))),
                ]));
            });

            let is_image = project.read_with(cx, |_project, cx| {
                image_store::is_image_file(&project, &project_path, cx)
            });

            if is_image {
                let image_entity: Entity<ImageItem> = cx
                    .update(|cx| {
                        self.project.update(cx, |project, cx| {
                            project.open_image(project_path.clone(), cx)
                        })
                    })
                    .await.map_err(tool_content_err)?;

                let image =
                    image_entity.read_with(cx, |image_item, _| Arc::clone(&image_item.image));

                let language_model_image = cx
                    .update(|cx| LanguageModelImage::from_image(image, cx))
                    .await
                    .context("processing image")
                    .map_err(tool_content_err)?;

                event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                    acp::ToolCallContent::Content(acp::Content::new(acp::ContentBlock::Image(
                        acp::ImageContent::new(language_model_image.source.clone(), "image/png"),
                    ))),
                ]));

                return Ok(language_model_image.into());
            }

            let open_buffer_task = project.update(cx, |project, cx| {
                project.open_buffer(project_path.clone(), cx)
            });

            let buffer = futures::select! {
                result = open_buffer_task.fuse() => result.map_err(tool_content_err)?,
                _ = event_stream.cancelled_by_user().fuse() => {
                    return Err(tool_content_err("File read cancelled by user"));
                }
            };
            if buffer.read_with(cx, |buffer, _| {
                buffer
                    .file()
                    .as_ref()
                    .is_none_or(|file| !file.disk_state().exists())
            }) {
                return Err(tool_content_err(format!("{file_path} not found")));
            }

            let mut anchor = None;
            let mut is_outline_response = false;

            // Check if specific line ranges are provided
            let result = if input.start_line.is_some() || input.end_line.is_some() {
                let result_text = buffer.read_with(cx, |buffer, _cx| {
                    let (start, end) = resolve_line_range(input.start_line, input.end_line);
                    let start_row = start - 1;
                    if start_row <= buffer.max_point().row {
                        let column = buffer.line_indent_for_row(start_row).raw_len();
                        anchor = Some(buffer.anchor_before(Point::new(start_row, column)));
                    }

                    // `end` is 1-indexed inclusive; `Point` rows are 0-indexed.
                    // Using `end` directly as the (exclusive) end row is the
                    // standard inclusive→exclusive translation, and since
                    // `resolve_line_range` guarantees `end >= start`, we always
                    // read at least one line.
                    let start_anchor = buffer.anchor_before(Point::new(start_row, 0));
                    let end_anchor = buffer.anchor_before(Point::new(end, 0));
                    // Stream the numbered output directly from the buffer's
                    // chunk iterator so the unnumbered range is never
                    // materialized as its own `String`.
                    let mut output = String::new();
                    write_lines_numbered(
                        &mut output,
                        buffer.text_for_range(start_anchor..end_anchor),
                        start,
                    );
                    output
                });

                action_log.update(cx, |log, cx| {
                    log.buffer_read(buffer.clone(), cx);
                });

                Ok(result_text.into())
            } else {
                // No line ranges specified, so check file size to see if it's too big.
                let buffer_content = outline::get_buffer_content_or_outline(
                    buffer.clone(),
                    Some(&abs_path.to_string_lossy()),
                    cx,
                )
                .await.map_err(tool_content_err)?;

                action_log.update(cx, |log, cx| {
                    log.buffer_read(buffer.clone(), cx);
                });


                is_outline_response = buffer_content.is_synthetic;

                if buffer_content.is_synthetic {
                    Ok(formatdoc! {"
                        SUCCESS: File outline retrieved. This file is too large to read all at once, so the outline below shows the file's structure with line numbers.

                        IMPORTANT: Do NOT retry this call without line numbers - you will get the same outline.
                        Instead, use the line numbers below to read specific sections by calling this tool again with start_line and end_line parameters.

                        {}

                        NEXT STEPS: To read a specific symbol's implementation, call read_file with the same path plus start_line and end_line from the outline above.
                        For example, to read a function shown as [L100-150], use start_line: 100 and end_line: 150.", buffer_content.text
                    }
                    .into())
                } else {
                    Ok(format_with_line_numbers(&buffer_content.text, 1).into())
                }
            };

            project.update(cx, |project, cx| {
                if self.update_agent_location {
                    project.set_agent_location(
                        Some(AgentLocation {
                            buffer: buffer.downgrade(),
                            position: anchor.unwrap_or_else(|| {
                                text::Anchor::min_for_buffer(buffer.read(cx).remote_id())
                            }),
                        }),
                        cx,
                    );
                }
                if let Ok(LanguageModelToolResultContent::Text(text)) = &result {
                    let text: &str = text;
                    // For outline responses, omit the path tag so the markdown renderer
                    // does not invoke tree-sitter syntax highlighting against pseudo-code
                    // outline text. The outline is not valid source for the file's language,
                    // so highlighting would be both expensive and incorrect.
                    let tag: &str = if is_outline_response { "" } else { &input.path };
                    let markdown = MarkdownCodeBlock { tag, text }.to_string();
                    event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                        acp::ToolCallContent::Content(acp::Content::new(markdown)),
                    ]));
                }
            });

            result
        })
    }

    fn replay(
        &self,
        input: Self::Input,
        output: Self::Output,
        event_stream: ToolCallEventStream,
        _cx: &mut App,
    ) -> Result<()> {
        if let LanguageModelToolResultContent::Text(text) = output {
            let markdown = MarkdownCodeBlock {
                tag: &input.path,
                text: &text,
            }
            .to_string();
            event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                acp::ToolCallContent::Content(acp::Content::new(markdown)),
            ]));
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fs::Fs as _;
    use gpui::{AppContext, TestAppContext, UpdateGlobal as _};
    use project::{FakeFs, Project};
    use serde_json::json;
    use settings::{SettingsStore, SplicingVec};
    use std::path::PathBuf;
    use std::sync::Arc;
    use util::path;

    #[test]
    fn test_is_skill_catalog_file_detects_skill_catalog_reads() {
        // Project skill: `.agents/skills/<name>/SKILL.md`.
        assert!(is_skill_catalog_file(Path::new(
            "/home/u/proj/.agents/skills/hypothesis-framer/SKILL.md"
        )));
        // Global skill: `<data>/agents/skills/<name>/SKILL.md`.
        assert!(is_skill_catalog_file(Path::new(
            "/home/u/.local/share/zed-kask/agents/skills/grill-me/SKILL.md"
        )));
        // Marketplace skill nested under `_marketplace` is still detected.
        assert!(is_skill_catalog_file(Path::new(
            "/home/u/.local/share/zed-kask/agents/skills/_marketplace/published/SKILL.md"
        )));
        // A resource file inside a skill dir is NOT a catalog read.
        assert!(!is_skill_catalog_file(Path::new(
            "/home/u/proj/.agents/skills/hypothesis-framer/templates/foo.j2"
        )));
        // A user file named SKILL.md not under a skills dir is NOT flagged.
        assert!(!is_skill_catalog_file(Path::new(
            "/home/u/proj/docs/SKILL.md"
        )));
        // A `skills/` dir whose parent is neither `.agents` nor `agents` is NOT
        // flagged (avoids false positives on unrelated `skills/` trees).
        assert!(!is_skill_catalog_file(Path::new(
            "/home/u/repos/skills/foo/SKILL.md"
        )));
        // zed-kask: D28 — a `skills/` dir under the kask data root (via the
        // `global_skills_dir` override) IS flagged. Use a tempdir so the
        // path matches what `global_skills_dir()` returns.
        {
            let tmp = tempfile::TempDir::new().expect("tempdir");
            let kask_skills = tmp.path().join("skills");
            std::fs::create_dir_all(&kask_skills).expect("create skills dir");
            agent_skills::set_global_skills_dir_override(Some(kask_skills.clone()));
            assert!(is_skill_catalog_file(
                &kask_skills.join("my-skill/SKILL.md")
            ));
            assert!(is_skill_catalog_file(
                &kask_skills.join("_marketplace/alice/bug-hunt/SKILL.md")
            ));
            agent_skills::set_global_skills_dir_override(None);
        }
    }

    #[test]
    fn test_refuse_skill_catalog_read_allows_skill_md() {
        // D1 revert: SKILL.md reads are now allowed. The `skill` tool reads
        // the body from disk and injects it. `read_file` can also read it.
        refuse_skill_catalog_read(
            Path::new("/home/u/proj/.agents/skills/hypothesis-framer/SKILL.md"),
            ".agents/skills/hypothesis-framer/SKILL.md",
        )
        .expect("SKILL.md reads must be allowed (body injection restored)");
    }

    /// The blocked-read telemetry key must stay out of the `reg.skill.*`
    /// namespace, which is reserved for per-skill feedback spans recorded via
    /// `RegulationLedger::record_skill_span` and CI-enforced to be exactly
    /// `reg.skill.<manifest.id>` by `kask/scripts/check-skill-span-namespace.sh`.
    /// A tool-boundary event squatting on that prefix would look like a skill's
    /// own outcome span to anything grepping the logs.
    #[test]
    fn test_blocked_read_telemetry_key_avoids_reserved_skill_span_namespace() {
        // Reconstruct the reserved prefix at runtime so this assertion's own
        // source text cannot satisfy the `contains` check it performs.
        let reserved_prefix = format!("reg{}skill{}", ".", ".");
        let emitted_key = BLOCKED_READ_TELEMETRY_KEY;
        assert!(
            !emitted_key.starts_with(&reserved_prefix),
            "blocked-read telemetry key `{emitted_key}` must not use the reserved \
             `reg.skill.*` feedback-span namespace (CI-enforced for per-skill \
             feedback spans)"
        );
        assert!(
            emitted_key.contains("catalog_read_blocked"),
            "the key must name the event it reports: {emitted_key}"
        );
    }

    #[test]
    fn test_refuse_skill_catalog_read_allows_skill_resources_and_other_files() {
        // Resource files inside a skill directory stay readable — a cascade
        // result may direct the model to a template or reference. Gating the
        // whole directory would break that path.
        refuse_skill_catalog_read(
            Path::new("/home/u/proj/.agents/skills/hypothesis-framer/templates/finer.j2"),
            "templates/finer.j2",
        )
        .expect("skill resource files must remain readable");

        // A user's own file that happens to be named SKILL.md is not a catalog
        // entry and must not be refused.
        refuse_skill_catalog_read(Path::new("/home/u/proj/docs/SKILL.md"), "docs/SKILL.md")
            .expect("a non-catalog SKILL.md must remain readable");
    }

    #[gpui::test]
    async fn test_read_directory_path(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "some_dir": {}
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let (event_stream, _) = ToolCallEventStream::test();

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/some_dir".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(ToolInput::resolved(input), event_stream, cx)
            })
            .await;
        assert_eq!(
            error_text(result.unwrap_err()),
            "root/some_dir is a directory, not a file. Use the list_directory tool to explore directory contents."
        );
    }

    #[gpui::test]
    async fn test_read_nonexistent_file(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let (event_stream, _) = ToolCallEventStream::test();

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/nonexistent_file.txt".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(ToolInput::resolved(input), event_stream, cx)
            })
            .await;
        assert_eq!(
            error_text(result.unwrap_err()),
            "root/nonexistent_file.txt not found"
        );
    }

    #[gpui::test]
    async fn test_read_small_file(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "small_file.txt": "This is a small file content"
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/small_file.txt".into(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert_eq!(
            result.unwrap(),
            "     1\tThis is a small file content".into()
        );
    }

    #[gpui::test]
    async fn test_read_large_file(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "large_file.rs": (0..1000).map(|i| format!("struct Test{} {{\n    a: u32,\n    b: usize,\n}}", i)).collect::<Vec<_>>().join("\n")
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let language_registry = project.read_with(cx, |project, _| project.languages().clone());
        language_registry.add(language::rust_lang());
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/large_file.rs".into(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await
            .unwrap();
        let content = result.to_str().unwrap();

        assert_eq!(
            content.lines().skip(7).take(6).collect::<Vec<_>>(),
            vec![
                "struct Test0 [L1-4]",
                " a [L2]",
                " b [L3]",
                "struct Test1 [L5-8]",
                " a [L6]",
                " b [L7]",
            ]
        );

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/large_file.rs".into(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await
            .unwrap();
        let content = result.to_str().unwrap();
        let expected_content = (0..1000)
            .flat_map(|i| {
                vec![
                    format!("struct Test{} [L{}-{}]", i, i * 4 + 1, i * 4 + 4),
                    format!(" a [L{}]", i * 4 + 2),
                    format!(" b [L{}]", i * 4 + 3),
                ]
            })
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            content
                .lines()
                .skip(7)
                .take(expected_content.len())
                .collect::<Vec<_>>(),
            expected_content
        );
    }

    // The outline returned for a large file is not valid source for the file's
    // language, so the UI-side markdown wrapping must omit the path tag.
    // Otherwise the markdown renderer routes the fenced block through
    // `CodeBlockKind::FencedSrc`, resolves the file's language, and runs
    // tree-sitter against pseudo-code outline text on every paint.
    #[gpui::test]
    async fn test_outline_response_uses_untagged_code_block(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "large_file.rs": (0..1000).map(|i| format!("struct Test{} {{\n    a: u32,\n    b: usize,\n}}", i)).collect::<Vec<_>>().join("\n")
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let language_registry = project.read_with(cx, |project, _| project.languages().clone());
        language_registry.add(language::rust_lang());
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let (event_stream, mut rx) = ToolCallEventStream::test();

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/large_file.rs".into(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone()
                    .run(ToolInput::resolved(input), event_stream, cx)
            })
            .await
            .unwrap();

        // Sanity-check: the file is large enough to trigger the outline branch.
        assert!(
            result
                .to_str()
                .unwrap()
                .starts_with("SUCCESS: File outline retrieved."),
            "expected outline response, got: {:?}",
            result.to_str().unwrap()
        );

        // The first update carries the location; the second carries the
        // markdown content destined for the tool-call UI.
        let _location_update = rx.expect_update_fields().await;
        let content_update = rx.expect_update_fields().await;
        let content_blocks = content_update.content.expect("expected content update");
        let acp::ToolCallContent::Content(content) = content_blocks
            .first()
            .expect("expected at least one content block")
        else {
            panic!("expected ContentBlock, got {:?}", content_blocks.first());
        };
        let acp::ContentBlock::Text(text) = &content.content else {
            panic!("expected text content block, got {:?}", content.content);
        };

        assert!(
            text.text.starts_with("```\n"),
            "outline response must use an untagged fenced code block; got first line: {:?}",
            text.text.lines().next()
        );
        assert!(
            !text.text.starts_with("```root/"),
            "outline response must not include the file path as a code block tag"
        );
    }

    // The full-file (non-outline) response should still tag the code block
    // with the file path so the markdown renderer can resolve the file's
    // language for syntax highlighting.
    #[gpui::test]
    async fn test_full_file_response_keeps_path_tag(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "small_file.rs": "fn main() {}"
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let (event_stream, mut rx) = ToolCallEventStream::test();

        cx.update(|cx| {
            let input = ReadFileToolInput {
                path: "root/small_file.rs".into(),
                start_line: None,
                end_line: None,
            };
            tool.clone()
                .run(ToolInput::resolved(input), event_stream, cx)
        })
        .await
        .unwrap();

        let _location_update = rx.expect_update_fields().await;
        let content_update = rx.expect_update_fields().await;
        let content_blocks = content_update.content.expect("expected content update");
        let acp::ToolCallContent::Content(content) = content_blocks
            .first()
            .expect("expected at least one content block")
        else {
            panic!("expected ContentBlock, got {:?}", content_blocks.first());
        };
        let acp::ContentBlock::Text(text) = &content.content else {
            panic!("expected text content block, got {:?}", content.content);
        };

        assert!(
            text.text.starts_with("```root/small_file.rs\n"),
            "full-file response must tag the code block with the file path; got first line: {:?}",
            text.text.lines().next()
        );
    }

    // When a worktree is named "foo" and contains a subdirectory also named "foo",
    // read_file({"path": "foo/test.txt"}) should return the file at the worktree
    // root (as the tool schema promises), not the one inside the foo/ subdirectory.
    #[gpui::test]
    async fn test_read_file_worktree_root_not_shadowed_by_subdir(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/foo"),
            json!({
                "test.txt": "root content",
                "foo": {
                    "test.txt": "subdir content"
                }
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/foo").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        // The tool schema says the first component must be the worktree root name,
        // so "foo/test.txt" means test.txt at the root of the "foo" worktree.
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "foo/test.txt".into(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert_eq!(result.unwrap(), "     1\troot content".into());
    }

    #[gpui::test]
    async fn test_read_file_with_line_range(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "multiline.txt": "Line 1\nLine 2\nLine 3\nLine 4\nLine 5"
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/multiline.txt".to_string(),
                    start_line: Some(2),
                    end_line: Some(4),
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert_eq!(
            result.unwrap(),
            "     2\tLine 2\n     3\tLine 3\n     4\tLine 4\n".into()
        );
    }

    #[gpui::test]
    async fn test_read_file_line_range_edge_cases(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "multiline.txt": "Line 1\nLine 2\nLine 3\nLine 4\nLine 5"
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        // start_line of 0 should be treated as 1
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/multiline.txt".to_string(),
                    start_line: Some(0),
                    end_line: Some(2),
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert_eq!(result.unwrap(), "     1\tLine 1\n     2\tLine 2\n".into());

        // end_line of 0 should result in at least 1 line
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/multiline.txt".to_string(),
                    start_line: Some(1),
                    end_line: Some(0),
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert_eq!(result.unwrap(), "     1\tLine 1\n".into());

        // when start_line > end_line, should still return at least 1 line
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "root/multiline.txt".to_string(),
                    start_line: Some(3),
                    end_line: Some(2),
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert_eq!(result.unwrap(), "     3\tLine 3\n".into());
    }

    fn error_text(content: LanguageModelToolResultContent) -> String {
        match content {
            LanguageModelToolResultContent::Text(text) => text.to_string(),
            other => panic!("Expected text error, got: {other:?}"),
        }
    }

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    fn single_pixel_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    #[gpui::test]
    async fn test_read_file_security(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());

        fs.insert_tree(
            path!("/"),
            json!({
                "project_root": {
                    "allowed_file.txt": "This file is in the project",
                    ".mysecrets": "SECRET_KEY=abc123",
                    ".secretdir": {
                        "config": "special configuration"
                    },
                    ".mymetadata": "custom metadata",
                    "subdir": {
                        "normal_file.txt": "Normal file content",
                        "special.privatekey": "private key content",
                        "data.mysensitive": "sensitive data"
                    }
                },
                "outside_project": {
                    "sensitive_file.txt": "This file is outside the project"
                }
            }),
        )
        .await;

        cx.update(|cx| {
            use gpui::UpdateGlobal;
            use settings::SettingsStore;
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.project.worktree.file_scan_exclusions = Some(SplicingVec::from(vec![
                        "**/.secretdir".to_string(),
                        "**/.mymetadata".to_string(),
                    ]));
                    settings.project.worktree.private_files = Some(
                        vec![
                            "**/.mysecrets".to_string(),
                            "**/*.privatekey".to_string(),
                            "**/*.mysensitive".to_string(),
                        ]
                        .into(),
                    );
                });
            });
        });

        let project = Project::test(fs.clone(), [path!("/project_root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        // Reading a file outside the project worktree should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "/outside_project/sensitive_file.txt".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read an absolute path outside a worktree"
        );

        // Reading a file within the project should succeed
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/allowed_file.txt".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_ok(),
            "read_file_tool should be able to read files inside worktrees"
        );

        // Reading files that match file_scan_exclusions should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/.secretdir/config".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read files in .secretdir (file_scan_exclusions)"
        );

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/.mymetadata".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read .mymetadata files (file_scan_exclusions)"
        );

        // Reading private files should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/.mysecrets".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read .mysecrets (private_files)"
        );

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/subdir/special.privatekey".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read .privatekey files (private_files)"
        );

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/subdir/data.mysensitive".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read .mysensitive files (private_files)"
        );

        // Reading a normal file should still work, even with private_files configured
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/subdir/normal_file.txt".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(result.is_ok(), "Should be able to read normal files");
        assert_eq!(result.unwrap(), "     1\tNormal file content".into());

        // Path traversal attempts with .. should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "project_root/../outside_project/sensitive_file.txt".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;
        assert!(
            result.is_err(),
            "read_file_tool should error when attempting to read a relative path that resolves to outside a worktree"
        );
    }

    #[gpui::test]
    async fn test_read_image_symlink_requires_authorization(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;
        fs.insert_tree(path!("/outside"), json!({})).await;
        fs.insert_file(path!("/outside/secret.png"), single_pixel_png())
            .await;
        fs.insert_symlink(
            path!("/root/secret.png"),
            PathBuf::from("/outside/secret.png"),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let (event_stream, mut event_rx) = ToolCallEventStream::test();
        let read_task = cx.update(|cx| {
            tool.run(
                ToolInput::resolved(ReadFileToolInput {
                    path: "root/secret.png".to_string(),
                    start_line: None,
                    end_line: None,
                }),
                event_stream,
                cx,
            )
        });

        let authorization = event_rx.expect_authorization().await;
        assert!(
            authorization
                .tool_call
                .fields
                .title
                .as_deref()
                .is_some_and(|title| title.contains("points outside the project")),
            "Expected symlink escape authorization before reading the image"
        );
        authorization
            .response
            .send(acp_thread::SelectedPermissionOutcome::new(
                acp::PermissionOptionId::new("allow"),
                acp::PermissionOptionKind::AllowOnce,
            ))
            .unwrap();

        let result = read_task.await;
        assert!(result.is_ok());
    }

    #[gpui::test]
    async fn test_read_file_with_multiple_worktree_settings(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());

        // Create first worktree with its own private_files setting
        fs.insert_tree(
            path!("/worktree1"),
            json!({
                "src": {
                    "main.rs": "fn main() { println!(\"Hello from worktree1\"); }",
                    "secret.rs": "const API_KEY: &str = \"secret_key_1\";",
                    "config.toml": "[database]\nurl = \"postgres://localhost/db1\""
                },
                "tests": {
                    "test.rs": "mod tests { fn test_it() {} }",
                    "fixture.sql": "CREATE TABLE users (id INT, name VARCHAR(255));"
                },
                ".zed": {
                    "settings.json": r#"{
                        "file_scan_exclusions": ["**/fixture.*"],
                        "private_files": ["**/secret.rs", "**/config.toml"]
                    }"#
                }
            }),
        )
        .await;

        // Create second worktree with different private_files setting
        fs.insert_tree(
            path!("/worktree2"),
            json!({
                "lib": {
                    "public.js": "export function greet() { return 'Hello from worktree2'; }",
                    "private.js": "const SECRET_TOKEN = \"private_token_2\";",
                    "data.json": "{\"api_key\": \"json_secret_key\"}"
                },
                "docs": {
                    "README.md": "# Public Documentation",
                    "internal.md": "# Internal Secrets and Configuration"
                },
                ".zed": {
                    "settings.json": r#"{
                        "file_scan_exclusions": ["**/internal.*"],
                        "private_files": ["**/private.js", "**/data.json"]
                    }"#
                }
            }),
        )
        .await;

        // Set global settings
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.project.worktree.file_scan_exclusions = Some(SplicingVec::from(vec![
                        "**/.git".to_string(),
                        "**/node_modules".to_string(),
                    ]));
                    settings.project.worktree.private_files =
                        Some(vec!["**/.env".to_string()].into());
                });
            });
        });

        let project = Project::test(
            fs.clone(),
            [path!("/worktree1").as_ref(), path!("/worktree2").as_ref()],
            cx,
        )
        .await;

        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project.clone(), action_log.clone(), true));

        // Test reading allowed files in worktree1
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree1/src/main.rs".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await
            .unwrap();

        assert_eq!(
            result,
            "     1\tfn main() { println!(\"Hello from worktree1\"); }".into()
        );

        // Test reading private file in worktree1 should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree1/src/secret.rs".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        assert!(result.is_err());
        assert!(
            error_text(result.unwrap_err()).contains("worktree `private_files` setting"),
            "Error should mention worktree private_files setting"
        );

        // Test reading excluded file in worktree1 should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree1/tests/fixture.sql".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        assert!(result.is_err());
        assert!(
            error_text(result.unwrap_err()).contains("worktree `file_scan_exclusions` setting"),
            "Error should mention worktree file_scan_exclusions setting"
        );

        // Test reading allowed files in worktree2
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree2/lib/public.js".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await
            .unwrap();

        assert_eq!(
            result,
            "     1\texport function greet() { return 'Hello from worktree2'; }".into()
        );

        // Test reading private file in worktree2 should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree2/lib/private.js".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        assert!(result.is_err());
        assert!(
            error_text(result.unwrap_err()).contains("worktree `private_files` setting"),
            "Error should mention worktree private_files setting"
        );

        // Test reading excluded file in worktree2 should fail
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree2/docs/internal.md".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        assert!(result.is_err());
        assert!(
            error_text(result.unwrap_err()).contains("worktree `file_scan_exclusions` setting"),
            "Error should mention worktree file_scan_exclusions setting"
        );

        // Test that files allowed in one worktree but not in another are handled correctly
        // (e.g., config.toml is private in worktree1 but doesn't exist in worktree2)
        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: "worktree1/src/config.toml".to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.clone().run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        assert!(result.is_err());
        assert!(
            error_text(result.unwrap_err()).contains("worktree `private_files` setting"),
            "Config.toml should be blocked by worktree1's private_files setting"
        );
    }

    #[gpui::test]
    async fn test_read_file_symlink_escape_requests_authorization(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "project": {
                    "src": { "main.rs": "fn main() {}" }
                },
                "external": {
                    "secret.txt": "SECRET_KEY=abc123"
                }
            }),
        )
        .await;

        fs.create_symlink(
            path!("/root/project/secret_link.txt").as_ref(),
            PathBuf::from("../external/secret.txt"),
        )
        .await
        .unwrap();

        let project = Project::test(fs.clone(), [path!("/root/project").as_ref()], cx).await;
        cx.executor().run_until_parked();

        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project.clone(), action_log, true));

        let (event_stream, mut event_rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| {
            tool.clone().run(
                ToolInput::resolved(ReadFileToolInput {
                    path: "project/secret_link.txt".to_string(),
                    start_line: None,
                    end_line: None,
                }),
                event_stream,
                cx,
            )
        });

        let auth = event_rx.expect_authorization().await;
        let title = auth.tool_call.fields.title.as_deref().unwrap_or("");
        assert!(
            title.contains("points outside the project"),
            "title: {title}"
        );

        auth.response
            .send(acp_thread::SelectedPermissionOutcome::new(
                acp::PermissionOptionId::new("allow"),
                acp::PermissionOptionKind::AllowOnce,
            ))
            .unwrap();

        let result = task.await;
        assert!(result.is_ok(), "should succeed after approval: {result:?}");
    }

    #[gpui::test]
    async fn test_read_file_symlink_escape_denied(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "project": {
                    "src": { "main.rs": "fn main() {}" }
                },
                "external": {
                    "secret.txt": "SECRET_KEY=abc123"
                }
            }),
        )
        .await;

        fs.create_symlink(
            path!("/root/project/secret_link.txt").as_ref(),
            PathBuf::from("../external/secret.txt"),
        )
        .await
        .unwrap();

        let project = Project::test(fs.clone(), [path!("/root/project").as_ref()], cx).await;
        cx.executor().run_until_parked();

        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project.clone(), action_log, true));

        let (event_stream, mut event_rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| {
            tool.clone().run(
                ToolInput::resolved(ReadFileToolInput {
                    path: "project/secret_link.txt".to_string(),
                    start_line: None,
                    end_line: None,
                }),
                event_stream,
                cx,
            )
        });

        let auth = event_rx.expect_authorization().await;
        drop(auth);

        let result = task.await;
        assert!(
            result.is_err(),
            "Tool should fail when authorization is denied"
        );
    }

    #[gpui::test]
    async fn test_read_file_symlink_escape_private_path_no_authorization(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "project": {
                    "src": { "main.rs": "fn main() {}" }
                },
                "external": {
                    "secret.txt": "SECRET_KEY=abc123"
                }
            }),
        )
        .await;

        fs.create_symlink(
            path!("/root/project/secret_link.txt").as_ref(),
            PathBuf::from("../external/secret.txt"),
        )
        .await
        .unwrap();

        cx.update(|cx| {
            settings::SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.project.worktree.private_files =
                        Some(vec!["**/secret_link.txt".to_string()].into());
                });
            });
        });

        let project = Project::test(fs.clone(), [path!("/root/project").as_ref()], cx).await;
        cx.executor().run_until_parked();

        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project.clone(), action_log, true));

        let (event_stream, mut event_rx) = ToolCallEventStream::test();
        let result = cx
            .update(|cx| {
                tool.clone().run(
                    ToolInput::resolved(ReadFileToolInput {
                        path: "project/secret_link.txt".to_string(),
                        start_line: None,
                        end_line: None,
                    }),
                    event_stream,
                    cx,
                )
            })
            .await;

        assert!(
            result.is_err(),
            "Expected read_file to fail on private path"
        );
        let error = error_text(result.unwrap_err());
        assert!(
            error.contains("private_files"),
            "Expected private-files validation error, got: {error}"
        );

        let event = event_rx.try_recv();
        assert!(
            !matches!(
                event,
                Ok(Ok(crate::thread::ThreadEvent::ToolCallAuthorization(_)))
            ),
            "No authorization should be requested when validation fails before read",
        );
    }

    #[gpui::test]
    async fn test_read_global_skill_file(cx: &mut TestAppContext) {
        init_test(cx);

        // Set up a project that does NOT contain the skills tree, plus a
        // global skill file outside the worktree.
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            json!({
                "src": { "main.rs": "fn main() {}" }
            }),
        )
        .await;

        let skill_md_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("references")
            .join("spec.md");
        fs.create_dir(skill_md_path.parent().unwrap())
            .await
            .unwrap();
        fs.insert_file(&skill_md_path, b"# Spec\n\nReference body.".to_vec())
            .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: skill_md_path.to_string_lossy().into_owned(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let content = result.unwrap();
        let LanguageModelToolResultContent::Text(text) = content else {
            panic!("expected text content");
        };
        assert_eq!(
            text.as_ref(),
            "     1\t# Spec\n     2\t\n     3\tReference body."
        );
    }

    #[gpui::test]
    async fn test_read_file_refuses_global_skill_catalog_entry(cx: &mut TestAppContext) {
        init_test(cx);

        // End-to-end: the tool itself must refuse, not merely the helper. The
        // global-skills fast path resolves before project-path machinery, so it
        // needs its own coverage — a gate wired into only one of the two call
        // sites would leave the bypass open.
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;

        let catalog_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("SKILL.md");
        fs.create_dir(catalog_path.parent().unwrap()).await.unwrap();
        fs.insert_file(
            &catalog_path,
            b"---\nname: my-skill\n---\nSENTINEL_SKILL_BODY_TEXT".to_vec(),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: catalog_path.to_string_lossy().into_owned(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let error = result.expect_err("reading a SKILL.md catalog entry must be refused");
        let message = format!("{error:?}");
        assert!(
            message.contains("Refused") && message.contains("my-skill"),
            "refusal must name the skill so the model can invoke it: {message}"
        );
        // The gate is pointless if the refusal echoes the content it withholds.
        assert!(
            !message.contains("SENTINEL_SKILL_BODY_TEXT"),
            "the refusal must not leak the SKILL.md body it is gating: {message}"
        );
    }

    #[gpui::test]
    async fn test_read_global_skill_file_with_line_range(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;

        let skill_md_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("references")
            .join("long.md");
        fs.create_dir(skill_md_path.parent().unwrap())
            .await
            .unwrap();
        fs.insert_file(
            &skill_md_path,
            b"line one\nline two\nline three\nline four\n".to_vec(),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: skill_md_path.to_string_lossy().into_owned(),
                    start_line: Some(2),
                    end_line: Some(3),
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let LanguageModelToolResultContent::Text(text) = result.unwrap() else {
            panic!("expected text content");
        };
        // Mirrors the buffer-backed path: lines 2-3 inclusive, WITH trailing
        // newline of the last returned line.
        assert_eq!(text.as_ref(), "     2\tline two\n     3\tline three\n");
    }

    #[gpui::test]
    async fn test_read_global_skill_file_line_range_zero_start(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;

        let skill_md_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("references")
            .join("long.md");
        fs.create_dir(skill_md_path.parent().unwrap())
            .await
            .unwrap();
        fs.insert_file(
            &skill_md_path,
            b"Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_vec(),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: skill_md_path.to_string_lossy().into_owned(),
                    start_line: Some(0),
                    end_line: Some(2),
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let LanguageModelToolResultContent::Text(text) = result.unwrap() else {
            panic!("expected text content");
        };
        assert_eq!(text.as_ref(), "     1\tLine 1\n     2\tLine 2\n");
    }

    #[gpui::test]
    async fn test_read_global_skill_file_line_range_zero_end(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;

        let skill_md_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("references")
            .join("long.md");
        fs.create_dir(skill_md_path.parent().unwrap())
            .await
            .unwrap();
        fs.insert_file(
            &skill_md_path,
            b"Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_vec(),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: skill_md_path.to_string_lossy().into_owned(),
                    start_line: Some(1),
                    end_line: Some(0),
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let LanguageModelToolResultContent::Text(text) = result.unwrap() else {
            panic!("expected text content");
        };
        assert_eq!(text.as_ref(), "     1\tLine 1\n");
    }

    #[gpui::test]
    async fn test_read_global_skill_file_line_range_inverted(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;

        let skill_md_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("references")
            .join("long.md");
        fs.create_dir(skill_md_path.parent().unwrap())
            .await
            .unwrap();
        fs.insert_file(
            &skill_md_path,
            b"Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_vec(),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: skill_md_path.to_string_lossy().into_owned(),
                    start_line: Some(3),
                    end_line: Some(2),
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let LanguageModelToolResultContent::Text(text) = result.unwrap() else {
            panic!("expected text content");
        };
        assert_eq!(text.as_ref(), "     3\tLine 3\n");
    }

    #[gpui::test]
    async fn test_read_global_skill_file_line_range_crlf(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;

        let skill_md_path = agent_skills::global_skills_dir()
            .join("my-skill")
            .join("references")
            .join("long.md");
        fs.create_dir(skill_md_path.parent().unwrap())
            .await
            .unwrap();
        fs.insert_file(
            &skill_md_path,
            b"line one\r\nline two\r\nline three\r\n".to_vec(),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: skill_md_path.to_string_lossy().into_owned(),
                    start_line: Some(1),
                    end_line: Some(2),
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        let LanguageModelToolResultContent::Text(text) = result.unwrap() else {
            panic!("expected text content");
        };
        assert_eq!(text.as_ref(), "     1\tline one\r\n     2\tline two\r\n");
    }

    #[gpui::test]
    async fn test_read_outside_skills_dir_still_rejected(cx: &mut TestAppContext) {
        init_test(cx);

        // A path that's neither in the worktree nor under the global skills
        // dir should still fail — the fast path is gated, not a backdoor for
        // arbitrary external reads.
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root"), json!({})).await;
        fs.create_dir(path!("/etc").as_ref()).await.unwrap();
        fs.insert_file(path!("/etc/secret"), b"top secret".to_vec())
            .await;

        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
        let action_log = cx.new(|_| ActionLog::new(project.clone()));
        let tool = Arc::new(ReadFileTool::new(project, action_log, true));

        let result = cx
            .update(|cx| {
                let input = ReadFileToolInput {
                    path: path!("/etc/secret").to_string(),
                    start_line: None,
                    end_line: None,
                };
                tool.run(
                    ToolInput::resolved(input),
                    ToolCallEventStream::test().0,
                    cx,
                )
            })
            .await;

        assert!(
            result.is_err(),
            "path outside skills dir should be rejected"
        );
    }
}
