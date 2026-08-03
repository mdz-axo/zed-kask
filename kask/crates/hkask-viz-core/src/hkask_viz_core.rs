//! Block-renderer registry for the D18 markdown seam.
//!
//! The D18 seam (`markdown::MarkdownElement::media_block_renderer`) accepts a
//! single `Box<dyn Fn(&str, &mut Window, &App) -> Option<AnyElement>>` callback
//! that is invoked for *every* fenced code block, with only the block body
//! (not the fence language). Each registered renderer self-selects by
//! inspecting the body and returns `Some(element)` to intercept the block or
//! `None` to fall through to the default code-block renderer.
//!
//! This crate composes the kask viz widgets (media, graph) into one such
//! callback, so `agent_ui::render_agent_markdown` registers a single renderer
//! that dispatches across all viz block types. Adding a new viz block type
//! means calling its factory below (and depending on its crate); the upstream
//! D18 field/builder/dispatch in `markdown` stay unchanged — the divergence
//! surface does not widen.

use gpui::{AnyElement, App, Window};

/// The composed block renderer: tries each registered renderer in order and
/// returns the first `Some(element)`. Structurally identical to
/// `markdown::MediaBlockRendererFn` (same erased `dyn Fn` type), so it can be
/// handed directly to `.media_block_renderer(...)`.
pub type BlockRenderer = Box<dyn Fn(&str, &mut Window, &App) -> Option<AnyElement>>;

/// Build the composed D18 block renderer.
///
/// Tries the media renderer first (it self-selects on a JSON body with a
/// `kind` field), then the graph renderer (self-selects on a JSON body with a
/// `viz` field). Returns `None` for bodies claimed by neither, so the default
/// code-block renderer handles them. Ordering is intentional: media bodies
/// (`{"kind": ...}`) are claimed by the media renderer before the graph
/// renderer ever sees them.
pub fn block_renderer() -> BlockRenderer {
    let media = hkask_media_widget::media_block_renderer();
    let graph = hkask_graph_widget::graph_block_renderer();
    Box::new(move |body, window, cx| {
        if let Some(element) = media(body, window, cx) {
            return Some(element);
        }
        graph(body, window, cx)
    })
}
