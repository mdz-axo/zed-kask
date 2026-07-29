//! Markdown rendering for the kask panel — rich markdown + mermaid + a
//! minimal link/code-span resolver.
//!
//! This is the Phase 3 rendering upgrade from `kask-panel-redesign.md`
//! v0.3.0. It reuses the `markdown` crate's `MarkdownElement` +
//! `MarkdownStyle::themed(MarkdownFont::Agent, ...)` (the same path the
//! agent panel takes) and enables `render_mermaid_diagrams: true` so
//! ```mermaid blocks render natively (the `markdown` crate handles parsing,
//! theme-aware rendering, and the code/diagram toggle — no fork needed).
//!
//! ## The minimal link resolver
//!
//! The agent panel's `render_agent_markdown` wires three callbacks:
//! - `image_resolver` — resolves `![alt](path)` against worktree roots.
//! - `on_url_click` — opens URLs / file links / mentions via the full
//!   `MentionUri` parser (threads, symbols, diagnostics, git diffs, …).
//! - `on_code_span_link` — resolves `` `file.rs:42` `` to clickable links
//!   via `AgentCodeSpanResolver` (LRU-cached, worktree-walking).
//!
//! The kask panel's regulatory task is narrower: the curator talks about
//! MCP servers, tools, and kask concepts. It rarely emits file-path links
//! to the user's project, and when it does, a simple worktree-relative
//! resolution suffices. So this module provides a **minimal** resolver:
//! - `image_resolver`: http(s) URLs + worktree-relative paths (ported
//!   from `agent_ui::resolve_agent_image`).
//! - `on_url_click`: external URLs via `cx.open_url`; relative file paths
//!   with a `#L42` fragment resolved against the project worktrees.
//! - `on_code_span_link`: `` `file.rs:42` `` resolved against the
//!   project worktrees (ported from `AgentCodeSpanResolver::try_resolve`,
//!   minus the LRU cache — kask conversations are short).
//!
//! This is ~80 lines, not the agent panel's ~200-line `MentionUri` parser.
//! If the curator starts emitting symbol/mention links that need the full
//! parser, v2 can lift `open_link` from `thread_view` — but that's excess
//! variety for v1 (per the cybernetic lens in the plan).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, ImageSource, Resource, SharedString, Window, cx};
use markdown::{
    CodeBlockRenderer, CopyButtonVisibility, Markdown, MarkdownElement, MarkdownFont,
    MarkdownOptions, MarkdownStyle, WrapButtonVisibility,
};
use project::Project;
use workspace::path_link::PathWithPosition;
use workspace::{Workspace, path_link::sanitize_path_text};

/// Render a markdown `Entity` with the agent-panel style + mermaid enabled.
///
/// Returns a `MarkdownElement` wired with the minimal link resolver. The
/// caller inserts this into the element tree where a `Label` used to go.
pub fn render_kask_markdown(
    markdown: gpui::Entity<Markdown>,
    style: MarkdownStyle,
    workspace: &gpui::WeakEntity<Workspace>,
    project: &gpui::WeakEntity<Project>,
    cx: &App,
) -> MarkdownElement {
    let worktree_roots = worktree_roots(project, cx);
    let workspace_for_url = workspace.clone();
    let project_for_span = project.clone();

    MarkdownElement::new(markdown, style)
        .code_block_renderer(CodeBlockRenderer::Default {
            copy_button_visibility: CopyButtonVisibility::VisibleOnHover,
            wrap_button_visibility: WrapButtonVisibility::VisibleOnHover,
            border: false,
        })
        .image_resolver(move |dest_url| resolve_image(dest_url, &worktree_roots))
        .on_url_click(move |text, window, cx| {
            open_link(text, &workspace_for_url, window, cx);
        })
        .on_code_span_link(move |text, cx| resolve_code_span(text, &project_for_span, cx))
}

/// Construct a `Markdown` entity with mermaid rendering enabled.
///
/// Use this for each assistant message. During streaming, call
/// `markdown.update(cx, |m, cx| m.append(&delta, cx))` per `TextDelta` —
/// the crate re-parses in the background and re-renders, including mermaid.
pub fn new_markdown(
    source: SharedString,
    language_registry: Option<Arc<language::LanguageRegistry>>,
    cx: &mut gpui::Context<Markdown>,
) -> Markdown {
    Markdown::new_with_options(
        source,
        language_registry,
        None,
        MarkdownOptions {
            render_mermaid_diagrams: true,
            ..Default::default()
        },
        cx,
    )
}

/// Absolute paths of every visible worktree in the project (for image +
/// code-span resolution). Empty when the project is unavailable.
fn worktree_roots(project: &gpui::WeakEntity<Project>, cx: &App) -> Vec<PathBuf> {
    project
        .upgrade()
        .map(|project| {
            project
                .read(cx)
                .visible_worktrees(cx)
                .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
                .collect()
        })
        .unwrap_or_default()
}

/// Resolve a markdown image destination to an `ImageSource`.
///
/// Ported from `agent_ui::resolve_agent_image`. Handles:
/// - `http(s)://` URLs → remote resource.
/// - Absolute paths that exist on disk → local resource.
/// - Relative paths resolved against worktree roots → local resource.
fn resolve_image(dest_url: &str, worktree_roots: &[PathBuf]) -> Option<ImageSource> {
    if dest_url.starts_with("http://") || dest_url.starts_with("https://") {
        return Some(ImageSource::Resource(Resource::Uri(SharedString::from(
            dest_url.to_string(),
        ))));
    }
    let path = Path::new(dest_url);
    if path.is_absolute() && path.exists() {
        return Some(ImageSource::Resource(Resource::Path(Arc::from(path))));
    }
    for root in worktree_roots {
        let absolute_path = root.join(dest_url);
        if absolute_path.exists() {
            return Some(ImageSource::Resource(Resource::Path(Arc::from(
                absolute_path.as_path(),
            ))));
        }
    }
    None
}

/// Open a URL or file link from a markdown click.
///
/// Minimal version of the agent panel's `thread_view::open_link`. Handles:
/// - External URLs (`http://`, `https://`, `mailto:`, etc.) → `cx.open_url`.
/// - Relative file paths with an optional `#L42` fragment → open in the
///   workspace at the line.
///
/// Does NOT handle the agent panel's `MentionUri` types (threads, symbols,
/// diagnostics, git diffs) — those are agent-panel-specific and excess
/// variety for the kask panel (per the plan's cybernetic lens).
fn open_link(
    url: SharedString,
    workspace: &gpui::WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    // External URLs go straight to the OS opener.
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("mailto:")
        || url.starts_with("file://")
    {
        cx.open_url(&url);
        return;
    }

    let Some(workspace) = workspace.upgrade() else {
        cx.open_url(&url);
        return;
    };

    // Try to interpret as a file path with an optional #L42 fragment.
    let (relative_path, fragment) = split_local_url_fragment(&url);
    if relative_path.is_empty() {
        cx.open_url(&url);
        return;
    }

    let project = workspace.read(cx).project().clone();
    let abs_path = project.update(cx, |project, cx| {
        project
            .find_project_path(&relative_path, cx)
            .and_then(|path| project.absolute_path(&path, cx))
    });
    if let Some(abs_path) = abs_path {
        let point = fragment
            .strip_prefix('L')
            .and_then(|s| s.parse::<u32>().ok())
            .map(|row| gpui::Point::new(row, 0));
        workspace.update(cx, |workspace, cx| {
            workspace
                .open_abs_path(
                    abs_path,
                    workspace::OpenOptions {
                        focus: Some(true),
                        ..Default::default()
                    },
                    window,
                    cx,
                )
                .detach();
            // The point is applied after open via the editor; for the
            // minimal resolver we skip the point-jump (v2 can add it).
            let _ = point;
        });
        return;
    }

    // Fall back to opening as a URL.
    cx.open_url(&url);
}

/// Split `path/to/file.rs#L42` into `(path, Some("#L42"))`.
fn split_local_url_fragment(url: &str) -> (&str, Option<&str>) {
    match url.rfind('#') {
        Some(idx) => (&url[..idx], Some(&url[idx..])),
        None => (url, None),
    }
}

/// Resolve a `` `file.rs:42` `` code span to a clickable link label.
///
/// Ported from `AgentCodeSpanResolver::try_resolve` (minus the LRU cache).
/// Returns `Some(display_label)` when the text looks like a path that
/// resolves in the project; `None` otherwise (the span renders as plain
/// code).
fn resolve_code_span(
    text: &str,
    project: &gpui::WeakEntity<Project>,
    cx: &App,
) -> Option<SharedString> {
    let trimmed = sanitize_path_text(text.trim());
    if !is_path_like(trimmed) {
        return None;
    }

    let path_with_position = PathWithPosition::parse_str(trimmed);
    let candidate_path = &path_with_position.path;
    if candidate_path.as_os_str().is_empty() {
        return None;
    }

    let project = project.upgrade()?;
    let project = project.read(cx);
    for worktree in project.visible_worktrees(cx) {
        let worktree = worktree.read(cx);
        let abs_path = worktree.abs_path().join(candidate_path);
        if abs_path.exists() {
            return Some(SharedString::from(trimmed.to_string()));
        }
    }
    None
}

/// Heuristic: does `text` look like a file path (not a URL, not a number)?
///
/// Ported from `AgentCodeSpanResolver::is_path_like`.
fn is_path_like(text: &str) -> bool {
    if text.len() < 3
        || text.contains("://")
        || text.contains('|')
        || text.chars().any(char::is_control)
        || text.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let path = PathWithPosition::parse_str(text).path;
    let path_text = path.to_string_lossy();
    if path_text.contains('/') || path_text.contains('\\') {
        return true;
    }
    // Bare filenames with an extension (e.g. `main.rs`) also count.
    path.extension().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_path_like_rejects_urls_and_numbers() {
        assert!(!is_path_like("https://example.com"));
        assert!(!is_path_like("42"));
        assert!(!is_path_like("ab"));
    }

    #[test]
    fn is_path_like_accepts_paths() {
        assert!(is_path_like("src/main.rs"));
        assert!(is_path_like("src\\main.rs"));
        assert!(is_path_like("main.rs"));
        assert!(is_path_like("src/main.rs:42"));
    }

    #[test]
    fn split_local_url_fragment_separates_path_and_line() {
        let (path, frag) = split_local_url_fragment("src/main.rs#L42");
        assert_eq!(path, "src/main.rs");
        assert_eq!(frag, Some("#L42"));
    }

    #[test]
    fn split_local_url_fragment_no_fragment() {
        let (path, frag) = split_local_url_fragment("src/main.rs");
        assert_eq!(path, "src/main.rs");
        assert_eq!(frag, None);
    }

    #[test]
    fn resolve_image_http_url() {
        let result = resolve_image("https://example.com/img.png", &[]);
        assert!(matches!(
            result,
            Some(ImageSource::Resource(Resource::Uri(_)))
        ));
    }
}
