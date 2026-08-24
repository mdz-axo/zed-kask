---
title: "hKask Viz-Core — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-24
version: "1.1.0"
status: "Active"
domain: "Composition"
mds_categories: [composition]
---

# hKask Viz-Core — Class Diagram

`hkask-viz-core` is the D18 composition root for the viz widgets. It composes every
widget's `create_*` factory into one `BlockRenderer` callback (the erased
`dyn Fn(&str, &mut Window, &mut App) -> Option<AnyElement>` that
`markdown::MarkdownElement::media_block_renderer` expects) and caches widget
entities by a hash of the block body so state survives the per-token re-renders
of the streaming chat.

```mermaid
classDiagram
    class BlockRenderer {
        <<interface>>
    }
    class block_renderer {
        +block_renderer() BlockRenderer
    }
    class VizCache {
        +widgets: HashMap~u64, CachedWidget~
        +order: VecDeque~u64~
        +get(key) Option~CachedWidget~
        +insert(key, widget)
    }
    class CachedWidget {
        <<erased closure>>
        +render() AnyElement
    }
    class cache_key {
        +cache_key(body) u64
    }
    class create_graph_widget
    class create_kanban_widget
    class create_portfolio_widget
    class create_scenarios_widget
    class create_swarm_widget

    block_renderer ..> VizCache : thread-local LRU max 32
    block_renderer ..> cache_key
    block_renderer ..> create_graph_widget
    block_renderer ..> create_kanban_widget
    block_renderer ..> create_portfolio_widget
    block_renderer ..> create_scenarios_widget
    block_renderer ..> create_swarm_widget
    VizCache o-- CachedWidget : holds strong refs
    CachedWidget ..> GraphWidget
    CachedWidget ..> KanbanWidget
    CachedWidget ..> PortfolioWidget
    CachedWidget ..> ScenariosWidget
    CachedWidget ..> SwarmWidget
```

**Selection order** (intentional): graph (`viz: "event_tree"`), kanban
(`viz: "kanban"`), portfolio (`viz: "portfolio"`), scenarios
(`viz: "scenarios"`), swarm (`viz: "swarm_delegate_results"`). A body claimed by none
returns `None` and falls through to the default code-block renderer.

**Wiring seam:** `crates/agent_ui/src/conversation_view.rs` —
`render_agent_markdown` calls `.media_block_renderer(hkask_viz_core::block_renderer())`.
The upstream D18 field/builder/dispatch in `markdown` stay unchanged (see
`DIVERGENCE.md` D18).

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-CORE
verified_date: 2026-08-24
verified_against: crates/hkask-viz-core/src/hkask_viz_core.rs (imports L55-63, VizWidget trait L85-100, block_renderer, create_* factories, VizCache, cache_key); crates/hkask-graph-widget/src/view.rs; crates/hkask-kanban-widget/src/view.rs; crates/hkask-portfolio-widget/src/view.rs; crates/hkask-scenarios-widget/src/view.rs; crates/hkask-swarm-widget/src/hkask_swarm_widget.rs; crates/agent_ui/src/conversation_view.rs
status: VERIFIED
-->