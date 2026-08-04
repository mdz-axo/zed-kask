# hKask Graph Widget — Class Diagram

`hkask-graph-widget` renders ```` ```graph ```` fenced blocks as an interactive
event-tree DAG (the `viz: "event_tree"` shape that mirrors the
`scenario_quantify` tool response). It parses the body, computes a layered
topological layout, and lets the user set evidence on a node and re-propagate
marginals interactively (without coupling the UI crate to the scenarios MCP
server).

```mermaid
classDiagram
    class GraphBlockBody {
        +viz: Option~String~
        +subject: Option~String~
        +joint_probability: Option~f64~
        +nodes: Vec~NodeBody~
    }
    class NodeBody {
        +id: String
        +name: Option~String~
        +question: Option~String~
        +marginal_probability: Option~f64~
        +depends_on: Vec~DependencyBody~
        +parents: Vec~String~
        +parent_ids() Vec~String~
    }
    class DependencyBody {
        +parent_event_ids: Vec~String~
        +conditionals: Vec~f64~
    }
    class parse_graph_body {
        +parse_graph_body(body) Result~GraphBlockBody~
    }
    class LayeredLayout {
        +nodes: Vec~LayoutNode~
        +edges: Vec~(usize, usize)~
        +topo_order: Vec~usize~
        +width: Pixels
        +height: Pixels
        +empty() LayeredLayout
    }
    class LayoutNode {
        +id: String
        +name: String
        +question: Option~String~
        +marginal_probability: Option~f64~
        +certainty_tier: Option~String~
        +parents: Vec~String~
        +position: Point~Pixels~
    }
    class compute_layout {
        +compute_layout(body) Result~LayeredLayout~
    }
    class recompute_marginals {
        +recompute_marginals(body, topo_order, evidence) Vec~f64~
    }
    class GraphWidget {
        +body: GraphBlockBody
        +layout: LayeredLayout
        +evidence: HashMap~usize, f64~
        +pan
        +zoom
        +hovered
        +selected
        +focus_handle: FocusHandle
        +new(body, cx) GraphWidget
        +repropagate()
    }
    class create_graph_widget {
        +create_graph_widget(body, cx) Option~Entity~GraphWidget~~
    }

    GraphBlockBody "1" o-- "many" NodeBody : nodes
    NodeBody "1" o-- "many" DependencyBody : depends_on
    LayeredLayout "1" o-- "many" LayoutNode : nodes
    compute_layout ..> GraphBlockBody
    compute_layout ..> LayeredLayout
    recompute_marginals ..> GraphBlockBody
    recompute_marginals ..> hkask_forecast [marginalize / certainty_tier]
    GraphWidget --> GraphBlockBody
    GraphWidget --> LayeredLayout
    GraphWidget ..|> gpui_Focusable [Focusable]
    GraphWidget ..|> gpui_Render [Render]
    create_graph_widget ..> GraphWidget : viz == "event_tree"
```

**Block shape:** a JSON body with `viz: "event_tree"`, an optional `subject`
and `joint_probability`, and a `nodes` array. Edges are child-side: each node
lists its parents in `depends_on[].parent_event_ids` (the `scenario_quantify`
shape) or a flat `parents` array (tolerant fallback). `certainty_tier` is
derived from `marginal_probability` via `hkask_forecast::certainty_tier` (one
source of truth) rather than trusted from the body.

**Layout:** Kahn topological sort assigns layers (roots at layer 0; a node's
layer is one past its deepest parent); cycles and unknown parents are rejected.

**Re-propagation:** `recompute_marginals` marginalizes over the full joint
truth-assignment space of `depends_on[0]` parents (delegated to
`hkask_forecast::marginalize` to avoid drift from
`hkask-mcp-scenarios::compute_marginal_probabilities`); a node in the `evidence`
map is treated as observed.

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-GRAPH
verified_date: 2026-08-03
verified_against: crates/hkask-graph-widget/src/block.rs; crates/hkask-graph-widget/src/layout.rs; crates/hkask-graph-widget/src/propagate.rs; crates/hkask-graph-widget/src/view.rs
status: VERIFIED 2026-08-03
-->