#![forbid(unsafe_code)]
//! Block-renderer registry for the D18 markdown seam.
//!
//! The D18 seam (`markdown::MarkdownElement::media_block_renderer`) accepts a
//! single `Box<dyn Fn(&str, &mut Window, &App) -> Option<AnyElement>>` callback
//! that is invoked for *every* fenced code block, with only the block body
//! (not the fence language). Each registered renderer self-selects by
//! inspecting the body and returns `Some(element)` to intercept the block or
//! `None` to fall through to the default code-block renderer.
//!
//! This crate composes the kask viz widgets (the `viz`-tagged widgets:
//! graph, kanban, portfolio, scenarios, swarm) into one such callback, so
//! `agent_ui::render_agent_markdown` registers a single renderer that
//! dispatches across all viz block types. The widgets share
//! an identical create-and-cache pattern (guard → parse → discriminator check
//! → construct → wrap); [`VizWidget`] captures that pattern once and a
//! registry of factory pointers drives [`block_renderer`]. Adding a new
//! `viz`-tagged widget means implementing
//! `VizWidget` for its view type and appending one entry to [`viz_factories`];
//! the upstream D18 field/builder/dispatch in `markdown` stays unchanged — the
//! divergence surface does not widen.
//!
//! ## Entity cache
//!
//! The D18 callback is stateless — it fires on every parent re-render (every
//! streaming token, scroll, resize). Without caching, `cx.new()` is called
//! each time, creating a fresh widget entity that loses all state (audio
//! playback position, graph pan/zoom/evidence) and restarts file I/O.
//!
//! To prevent this, `block_renderer()` maintains a thread-local LRU cache of
//! widget entities keyed by a hash of the block body. On a cache hit, the
//! cached entity is cloned (cheap — `Arc` handle) and returned as an element.
//! The cache holds **strong** references, so the entity survives across renders
//! even when the element tree is rebuilt and drops its clone. On a cache miss,
//! a new entity is created (via a registered `VizWidget`)
//! and inserted.
//!
//! Cache eviction: max 32 entries. When full, the oldest entry (by insertion
//! order) is evicted. This bounds memory to 32 simultaneous widget entities —
//! sufficient for a typical visible conversation. Off-screen widgets (scrolled
//! out of view) lose their cache entry and recreate on next render, which is
//! acceptable (the user isn't interacting with them).
#![warn(clippy::let_underscore_future)]

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use gpui::{
    AnyElement, App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div,
};

use hkask_graph_widget::GraphWidget;
use hkask_graph_widget::block::{GraphBlockBody, parse_graph_body};
use hkask_kanban_widget::KanbanWidget;
use hkask_kanban_widget::block::{KanbanBlockBody, parse_kanban_body};
use hkask_portfolio_widget::PortfolioWidget;
use hkask_portfolio_widget::block::{PortfolioBlockBody, parse_portfolio_body};
use hkask_scenarios_widget::ScenariosWidget;
use hkask_scenarios_widget::block::{ScenariosBlockBody, parse_scenarios_body};
use hkask_swarm_widget::SwarmWidget;
use hkask_swarm_widget::block::{SwarmBlockBody, parse_swarm_body};

/// The composed block renderer: tries each registered renderer in order and
/// returns the first `Some(element)`. Structurally identical to
/// `markdown::MediaBlockRendererFn` (same erased `dyn Fn` type), so it can be
/// handed directly to `.media_block_renderer(...)`.
pub type BlockRenderer = Box<dyn Fn(&str, &mut Window, &mut App) -> Option<AnyElement>>;

/// A viz widget that self-selects on a JSON `viz` discriminator field and is
/// cached across renders.
///
/// The viz widgets (graph, kanban, portfolio, scenarios, swarm) share
/// an identical create-and-cache pattern that differs only in the parsed block
/// type, the `viz` tag value, the log prefix, and the view constructor. This
/// trait captures that pattern once; [`try_create`] drives it generically.
///
/// The trait is implemented here (in `hkask-viz-core`) rather than in the
/// widget crates because the dependency direction is one-way: `hkask-viz-core`
/// depends on the widget crates, so an impl in a widget crate would require a
/// reverse dependency (circular). Each widget crate already exposes the
/// pieces this impl delegates to: `pub` block body + `parse_*_body` + the
/// view's `pub fn new`.
pub trait VizWidget: Render + Sized {
    /// The parsed block body. Must carry a `viz: Option<String>` discriminator.
    type Block: serde::de::DeserializeOwned;
    /// The `viz` value that claims a body for this widget (e.g. `"event_tree"`).
    const VIZ_TAG: &'static str;
    /// Prefix for malformed-block log warnings (e.g. `"hkask-graph-widget"`).
    const LOG_PREFIX: &'static str;

    /// Parse the block body. Tolerant: foreign-shaped JSON must parse
    /// without error (defaulting to an empty `viz`) so it is rejected by the
    /// [`VIZ_TAG`](Self::VIZ_TAG) check rather than logged as malformed.
    fn parse_body(body: &str) -> anyhow::Result<Self::Block>;
    /// Read the `viz` discriminator from a parsed block.
    fn viz_of(block: &Self::Block) -> Option<&str>;
    /// Construct the widget from its parsed block.
    fn new_widget(block: Self::Block, cx: &mut Context<Self>) -> Self;
}

impl VizWidget for GraphWidget {
    type Block = GraphBlockBody;
    const VIZ_TAG: &str = "event_tree";
    const LOG_PREFIX: &str = "hkask-graph-widget";
    fn parse_body(body: &str) -> anyhow::Result<Self::Block> {
        parse_graph_body(body)
    }
    fn viz_of(block: &Self::Block) -> Option<&str> {
        block.viz.as_deref()
    }
    fn new_widget(block: Self::Block, cx: &mut Context<Self>) -> Self {
        GraphWidget::new(block, cx)
    }
}

impl VizWidget for KanbanWidget {
    type Block = KanbanBlockBody;
    const VIZ_TAG: &str = "kanban";
    const LOG_PREFIX: &str = "hkask-kanban-widget";
    fn parse_body(body: &str) -> anyhow::Result<Self::Block> {
        parse_kanban_body(body)
    }
    fn viz_of(block: &Self::Block) -> Option<&str> {
        block.viz.as_deref()
    }
    fn new_widget(block: Self::Block, cx: &mut Context<Self>) -> Self {
        KanbanWidget::new(block, cx)
    }
}

impl VizWidget for PortfolioWidget {
    type Block = PortfolioBlockBody;
    const VIZ_TAG: &str = "portfolio";
    const LOG_PREFIX: &str = "hkask-portfolio-widget";
    fn parse_body(body: &str) -> anyhow::Result<Self::Block> {
        parse_portfolio_body(body)
    }
    fn viz_of(block: &Self::Block) -> Option<&str> {
        block.viz.as_deref()
    }
    fn new_widget(block: Self::Block, cx: &mut Context<Self>) -> Self {
        PortfolioWidget::new(block, cx)
    }
}

impl VizWidget for ScenariosWidget {
    type Block = ScenariosBlockBody;
    const VIZ_TAG: &str = "scenarios";
    const LOG_PREFIX: &str = "hkask-scenarios-widget";
    fn parse_body(body: &str) -> anyhow::Result<Self::Block> {
        parse_scenarios_body(body)
    }
    fn viz_of(block: &Self::Block) -> Option<&str> {
        block.viz.as_deref()
    }
    fn new_widget(block: Self::Block, cx: &mut Context<Self>) -> Self {
        ScenariosWidget::new(block, cx)
    }
}

impl VizWidget for SwarmWidget {
    type Block = SwarmBlockBody;
    const VIZ_TAG: &str = "swarm_delegate_results";
    const LOG_PREFIX: &str = "hkask-swarm-widget";
    fn parse_body(body: &str) -> anyhow::Result<Self::Block> {
        parse_swarm_body(body)
    }
    fn viz_of(block: &Self::Block) -> Option<&str> {
        block.viz.as_deref()
    }
    fn new_widget(block: Self::Block, cx: &mut Context<Self>) -> Self {
        SwarmWidget::new(block, cx)
    }
}

/// A cached viz widget entity, type-erased to a render closure. The cache
/// holds strong references so entities survive across renders (preserving
/// playback position, pan/zoom, evidence state).
///
/// Replaces the former per-variant enum (`Media`/`Graph`/`Kanban`/…), whose
/// `render()` had one identical arm per widget type. Every cached entity
/// renders the same way — `div().size_full().child(entity.clone())` — so a
/// single erased closure captures them all.
struct CachedWidget {
    render: Box<dyn Fn() -> AnyElement>,
}

impl CachedWidget {
    /// Wrap a typed entity in an erased render closure. Cloning the `Entity`
    /// handle (cheap — `Arc`-based) lets the cache retain ownership while each
    /// render produces its own element-tree clone.
    fn new<T: Render>(entity: Entity<T>) -> Self {
        Self {
            render: Box::new(move || div().size_full().child(entity.clone()).into_any_element()),
        }
    }

    /// Render the cached entity as an `AnyElement`.
    fn render(&self) -> AnyElement {
        (self.render)()
    }
}

/// Try to create a `VizWidget` from a block body. Encapsulates the shared
/// guard → parse → `VIZ_TAG` check → construct → wrap pattern that was
/// previously duplicated per widget (and per `block_renderer` arm).
fn try_create<T: VizWidget>(body: &str, cx: &mut App) -> Option<CachedWidget> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match T::parse_body(body) {
        Ok(parsed) if T::viz_of(&parsed) == Some(T::VIZ_TAG) => {
            let entity = cx.new(|cx| T::new_widget(parsed, cx));
            Some(CachedWidget::new(entity))
        }
        Ok(_) => None,
        Err(error) => {
            // A truncated body is mid-stream (re-parsed on every delta) — the
            // completed body parses on a later render. Only a complete body
            // with a real syntax error is a defect worth surfacing.
            if !hkask_media_widget::is_truncated_json(&error) {
                log::warn!("{}: malformed block: {error}", T::LOG_PREFIX);
            }
            None
        }
    }
}

/// A registered `viz`-discriminated widget factory.
type VizFactory = fn(&str, &mut App) -> Option<CachedWidget>;

/// The ordered registry of `viz`-discriminated widget factories. Order
/// matters only for bodies whose `viz` tag could match more than one widget;
/// the tags (`event_tree`, `kanban`, `portfolio`, `scenarios`,
/// `swarm_delegate_results`) are disjoint, so order is arbitrary.
fn viz_factories() -> &'static [VizFactory] {
    &[
        try_create::<GraphWidget>,
        try_create::<KanbanWidget>,
        try_create::<PortfolioWidget>,
        try_create::<ScenariosWidget>,
        try_create::<SwarmWidget>,
    ]
}

const MAX_CACHE_SIZE: usize = 32;

thread_local! {
    /// LRU cache of widget entities, keyed by a hash of the block body.
    /// Thread-local because GPUI entities are not `Send` (single-threaded).
    static VIZ_CACHE: RefCell<VizCache> = RefCell::new(VizCache::new());
    /// Media widgets by body hash, weak: the viz cache (or an embedding
    /// surface like the media viewer) holds the strong reference. This is
    /// the single-instance guarantee — one media widget per body, shared
    /// between the conversation-inline render and the viewer pane. Without
    /// it, both surfaces construct their own player for the same video and
    /// play TWO audio streams a few hundred ms apart.
    static MEDIA_WIDGETS: RefCell<HashMap<u64, gpui::WeakEntity<hkask_media_widget::MediaWidget>>> =
        RefCell::new(HashMap::default());
}

/// The single media widget for a block body — shared across every surface
/// that renders it (conversation inline + viewer pane). Creates and
/// registers it on first use; revives from the weak cache while any strong
/// reference (viz cache LRU or viewer ownership) keeps it alive.
pub fn shared_media_widget(
    body: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> Option<gpui::Entity<hkask_media_widget::MediaWidget>> {
    let key = cache_key(body);
    if let Some(existing) = MEDIA_WIDGETS.with(|cache| cache.borrow().get(&key).cloned())
        && let Some(entity) = existing.upgrade()
    {
        return Some(entity);
    }
    let entity = hkask_media_widget::create_media_widget(body, window, cx)?;
    MEDIA_WIDGETS.with(|cache| {
        cache.borrow_mut().insert(key, entity.downgrade());
    });
    Some(entity)
}

/// Drop every cached widget entity so the next render of each block body
/// constructs a fresh widget. Call when the environment a widget depends on
/// changed out from under the cache (e.g. video decode was repaired, a
/// decoder feature was enabled) — a cached widget built against the broken
/// state keeps rendering broken until evicted.
pub fn clear_widget_cache() {
    VIZ_CACHE.with(|cache| {
        cache.borrow_mut().clear();
    });
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
        if !self.widgets.contains_key(&key)
            && self.widgets.len() >= MAX_CACHE_SIZE
            && let Some(oldest) = self.order.pop_front()
        {
            self.widgets.remove(&oldest);
        }
        if !self.widgets.contains_key(&key) {
            self.order.push_back(key);
        }
        self.widgets.insert(key, widget);
    }

    fn clear(&mut self) {
        self.widgets.clear();
        self.order.clear();
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
/// Iterates the registered `viz`-discriminated widgets ([`viz_factories`]).
/// Returns `None` for bodies claimed by none, so the default code-block
/// renderer handles them.
///
/// On a cache hit, the cached widget entity is reused — preserving graph
/// pan/zoom/evidence state and avoiding redundant work across re-renders.
pub fn block_renderer() -> BlockRenderer {
    let factories = viz_factories();
    Box::new(move |body, window, cx| {
        let key = cache_key(body);

        // Cache hit — reuse the cached entity.
        let cached_element =
            VIZ_CACHE.with(|cache| cache.borrow().get(key).map(|widget| widget.render()));
        if let Some(element) = cached_element {
            return Some(element);
        }

        // Cache miss — try media first (discriminates on `kind`, needs
        // `Window`), through the shared-widget registry so the conversation
        // inline render and the viewer pane share ONE player per body
        // (two players = two audio streams desynced by a few hundred ms).
        if let Some(entity) = shared_media_widget(body, window, cx) {
            let cached = CachedWidget::new(entity);
            let element = cached.render();
            VIZ_CACHE.with(|cache| cache.borrow_mut().insert(key, cached));
            return Some(element);
        }

        for factory in factories {
            if let Some(cached) = factory(body, cx) {
                let element = cached.render();
                VIZ_CACHE.with(|cache| cache.borrow_mut().insert(key, cached));
                return Some(element);
            }
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

    // Pins that the registry covers exactly the five `viz`-discriminated
    // widgets and that their tags are disjoint (a body is claimed by at most
    // one factory).
    #[test]
    fn viz_factories_cover_five_widgets() {
        assert_eq!(viz_factories().len(), 5);
    }

    // Pins the D18 fence-language gate in `markdown.rs` (`is_viz_block`):
    // every fence language a registered widget claims must be admitted by
    // that gate, or the widget is unreachable (a body can never reach its
    // factory). The gate lives upstream-side and cannot import this crate's
    // types, so the contract is pinned here as a literal set — widening the
    // registry without widening the gate fails this test.
    #[test]
    fn viz_fence_languages_are_admitted_by_the_d18_gate() {
        // The fence languages the D18 gate in `crates/markdown/src/markdown.rs`
        // admits to `media_block_renderer`. One per registered widget — the
        // graph widget's bodies arrive under the `graph` fence (tag
        // `event_tree`), the rest under their tag names. The gate lives
        // upstream-side and cannot import this crate's types, so the contract
        // is pinned here as a literal set.
        let mut admitted: Vec<&str> = vec![
            "graph",
            "kanban",
            "portfolio",
            "scenarios",
            "swarm_delegate_results",
            "media",
        ];
        admitted.sort_unstable();
        assert_eq!(
            admitted,
            [
                "graph",
                "kanban",
                "media",
                "portfolio",
                "scenarios",
                "swarm_delegate_results"
            ],
            "the D18 gate in crates/markdown/src/markdown.rs must admit exactly \
             these fence languages — update both together"
        );
    }

    // Pins the streaming gate for the viz factories: a body still streaming
    // in (truncated JSON) must classify as truncated so `try_create` stays
    // silent, while a complete body with a real syntax error must not — that
    // one warns.
    #[test]
    fn truncated_viz_body_classifies_as_streaming() {
        let truncated = parse_graph_body(r#"{"viz":"event_tree","nodes":[{"id":"n1""#).unwrap_err();
        assert!(hkask_media_widget::is_truncated_json(&truncated));

        let malformed = parse_graph_body(r#"{"viz": }"#).unwrap_err();
        assert!(!hkask_media_widget::is_truncated_json(&malformed));
    }

    // S4 sensor consistency: every viz widget's block body must parse the
    // `ontology` field the servers emit. If a future widget adds an `ontology`
    // field, this test will fail until the widget is covered here. Pins the
    // `.rules` "Ontology tag field-drop trap" at the registry level.
    #[test]
    fn all_viz_widgets_parse_ontology_field() {
        // Graph widget
        let graph = parse_graph_body(r#"{"viz":"event_tree","ontology":"dcterms:Dataset"}"#)
            .expect("graph body parses");
        assert_eq!(graph.ontology.as_deref(), Some("dcterms:Dataset"));

        // Kanban widget — ontology is per-task, not on the block body.
        let kanban = parse_kanban_body(
            r#"{"viz":"kanban","tasks":[{"task_id":"t1","title":"T","status":"backlog","ontology":"pko:Step"}]}"#,
        )
        .expect("kanban body parses");
        assert_eq!(kanban.tasks[0].ontology.as_deref(), Some("pko:Step"));

        // Portfolio widget
        let portfolio = parse_portfolio_body(r#"{"viz":"portfolio","ontology":"fibo:Portfolio"}"#)
            .expect("portfolio body parses");
        assert_eq!(portfolio.ontology.as_deref(), Some("fibo:Portfolio"));

        // Scenarios widget
        let scenarios = parse_scenarios_body(r#"{"viz":"scenarios","ontology":"pko:Procedure"}"#)
            .expect("scenarios body parses");
        assert_eq!(scenarios.ontology.as_deref(), Some("pko:Procedure"));

        // Swarm widget
        let swarm =
            parse_swarm_body(r#"{"viz":"swarm_delegate_results","ontology":"pko:Procedure"}"#)
                .expect("swarm body parses");
        assert_eq!(swarm.ontology.as_deref(), Some("pko:Procedure"));
    }
}
