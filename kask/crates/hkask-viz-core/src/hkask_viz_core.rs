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
//!
//! ## Entity cache
//!
//! The D18 callback is stateless — it fires on every parent re-render (every
//! streaming token, scroll, resize). Without caching, `cx.new()` is called
//! each time, creating a fresh `MediaWidget`/`GraphWidget` entity that loses
//! all state (audio playback position, graph pan/zoom/evidence) and restarts
//! file I/O.
//!
//! To prevent this, `block_renderer()` maintains a thread-local LRU cache of
//! widget entities keyed by a hash of the block body. On a cache hit, the
//! cached `Entity<T>` is cloned (cheap — `Arc` handle) and returned as an
//! element. The cache holds **strong** references, so the entity survives
//! across renders even when the element tree is rebuilt and drops its
//! clone. On a cache miss, a new entity is created via
//! `create_media_widget`/`create_graph_widget` and inserted.
//!
//! Cache eviction: max 32 entries. When full, the oldest entry (by insertion
//! order) is evicted. This bounds memory to 32 simultaneous widget entities —
//! sufficient for a typical visible conversation. Off-screen widgets (scrolled
//! out of view) lose their cache entry and recreate on next render, which is
//! acceptable (the user isn't interacting with them).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use gpui::{AnyElement, App, Entity, IntoElement, ParentElement, Styled, Window, div};

use hkask_graph_widget::GraphWidget;
use hkask_media_widget::MediaWidget;

/// The composed block renderer: tries each registered renderer in order and
/// returns the first `Some(element)`. Structurally identical to
/// `markdown::MediaBlockRendererFn` (same erased `dyn Fn` type), so it can be
/// handed directly to `.media_block_renderer(...)`.
pub type BlockRenderer = Box<dyn Fn(&str, &mut Window, &mut App) -> Option<AnyElement>>;

/// A cached viz widget entity. The cache holds strong references so entities
/// survive across renders.
enum CachedWidget {
    Media(Entity<MediaWidget>),
    Graph(Entity<GraphWidget>),
}

impl CachedWidget {
    /// Render the cached entity as an `AnyElement`. Clones the `Entity` handle
    /// (cheap — `Arc`-based) so the cache retains ownership while the element
    /// tree gets its own clone.
    fn render(&self) -> AnyElement {
        match self {
            CachedWidget::Media(entity) => {
                div().size_full().child(entity.clone()).into_any_element()
            }
            CachedWidget::Graph(entity) => {
                div().size_full().child(entity.clone()).into_any_element()
            }
        }
    }
}

const MAX_CACHE_SIZE: usize = 32;

thread_local! {
    /// LRU cache of widget entities, keyed by a hash of the block body.
    /// Thread-local because GPUI entities are not `Send` (single-threaded).
    static VIZ_CACHE: RefCell<VizCache> = RefCell::new(VizCache::new());
}

struct VizCache {
    widgets: HashMap<u64, CachedWidget>,
    /// Insertion order for LRU eviction (oldest at front).
    order: VecDeque<u64>,
}

impl VizCache {
    fn new() -> Self {
        Self {
            widgets: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, key: u64) -> Option<&CachedWidget> {
        self.widgets.get(&key)
    }

    fn insert(&mut self, key: u64, widget: CachedWidget) {
        if !self.widgets.contains_key(&key) && self.widgets.len() >= MAX_CACHE_SIZE {
            if let Some(oldest) = self.order.pop_front() {
                self.widgets.remove(&oldest);
            }
        }
        if !self.widgets.contains_key(&key) {
            self.order.push_back(key);
        }
        self.widgets.insert(key, widget);
    }
}

/// Hash a block body to a cache key. Uses `DefaultHasher` (stable within a
/// process — sufficient since the cache is thread-local and per-process).
fn cache_key(body: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut hasher);
    hasher.finish()
}

/// Build the composed D18 block renderer with entity caching.
///
/// Tries the media renderer first (it self-selects on a JSON body with a
/// `kind` field), then the graph renderer (self-selects on a JSON body with a
/// `viz` field). Returns `None` for bodies claimed by neither, so the default
/// code-block renderer handles them. Ordering is intentional: media bodies
/// (`{"kind": ...}`) are claimed by the media renderer before the graph
/// renderer ever sees them.
///
/// On a cache hit, the cached widget entity is reused — preserving audio/video
/// playback position, graph pan/zoom/evidence state, and avoiding redundant
/// file I/O across re-renders.
pub fn block_renderer() -> BlockRenderer {
    Box::new(|body, window, cx| {
        let key = cache_key(body);

        // Cache hit — reuse the cached entity.
        let cached_element =
            VIZ_CACHE.with(|cache| cache.borrow().get(key).map(|widget| widget.render()));
        if let Some(element) = cached_element {
            return Some(element);
        }

        // Cache miss — try media first, then graph. Create and cache the entity.
        if let Some(entity) = hkask_media_widget::create_media_widget(body, window, cx) {
            VIZ_CACHE.with(|cache| {
                cache.borrow_mut().insert(key, CachedWidget::Media(entity));
            });
            // Re-read from cache to render (avoids moving the entity out).
            return VIZ_CACHE.with(|cache| cache.borrow().get(key).map(|widget| widget.render()));
        }

        if let Some(entity) = hkask_graph_widget::create_graph_widget(body, cx) {
            VIZ_CACHE.with(|cache| {
                cache.borrow_mut().insert(key, CachedWidget::Graph(entity));
            });
            return VIZ_CACHE.with(|cache| cache.borrow().get(key).map(|widget| widget.render()));
        }

        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_stable() {
        assert_eq!(cache_key("hello"), cache_key("hello"));
        assert_ne!(cache_key("hello"), cache_key("world"));
    }
}
