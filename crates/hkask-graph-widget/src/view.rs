//! The `GraphWidget` GPUI view — renders the layered event-tree DAG with
//! pan/zoom and interactive set-evidence propagation.
//!
//! Rendering: a `canvas` (sized to the viewport) draws edges + node circles in
//! graph-space pixels, transformed by a fit+zoom+pan map. Labels, the tooltip,
//! and evidence buttons are absolutely-positioned divs overlaid at the same
//! transformed coordinates. The canvas and the overlay share the same
//! transform, computed from the canvas bounds (cached on first paint via the
//! `git_graph` pattern) plus the widget's `pan`/`zoom` state, so they align
//! without per-node hit-areas.
//!
//! Interaction is centralized: `on_mouse_move` pans (left-drag) and hit-tests
//! for hover; `on_click` selects; `on_scroll_wheel` zooms around the cursor.
//! Selecting a node reveals evidence buttons (Set P≈90% / ≈10% / Reset);
//! setting evidence overrides that node's marginal and re-propagates the whole
//! tree via `propagate::recompute_marginals`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point, Render,
    Rgba, ScrollWheelEvent, StatefulInteractiveElement, Styled, Window, canvas, div, point,
    prelude::*, px, rgb,
};
use theme::ActiveTheme;

use crate::block::GraphBlockBody;
use crate::layout::LayeredLayout;
use crate::propagate;

const NODE_RADIUS: f32 = 12.0;
/// Hit radius (in graph-space px) is a bit larger than the drawn node so small
/// nodes are still grabbable when zoomed out.
const HIT_RADIUS: f32 = NODE_RADIUS * 1.6;

/// The graph widget view. Renders inline in agent markdown (via the D18 seam
/// composed by `hkask-viz-core`) or as a standalone panel item.
pub struct GraphWidget {
    /// The parsed block body — source of truth for edges, conditionals, and
    /// base (root) probabilities. Held so evidence overrides can re-propagate.
    body: GraphBlockBody,
    layout: LayeredLayout,
    /// Evidence overrides: node index → observed probability.
    evidence: HashMap<usize, f64>,
    pan: Point<Pixels>,
    zoom: f32,
    /// Last mouse position while left-dragging, for pan deltas.
    last_mouse: Option<Point<Pixels>>,
    /// The canvas's last laid-out bounds, set during paint and read in
    /// `render()`/handlers to map between screen and graph coordinates.
    last_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    hovered: Option<usize>,
    selected: Option<usize>,
    focus_handle: FocusHandle,
}

impl GraphWidget {
    /// Create a new graph widget for the parsed block body.
    pub fn new(body: GraphBlockBody, cx: &mut Context<Self>) -> Self {
        let layout = match crate::layout::compute_layout(&body) {
            Ok(layout) => layout,
            Err(error) => {
                log::warn!("hkask-graph-widget: layout failed: {error}");
                LayeredLayout::empty()
            }
        };
        Self {
            body,
            layout,
            evidence: HashMap::new(),
            pan: point(px(0.0), px(0.0)),
            zoom: 1.0,
            last_mouse: None,
            last_bounds: Rc::new(Cell::new(None)),
            hovered: None,
            selected: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Set a node's probability as observed evidence and re-propagate.
    fn set_evidence(&mut self, idx: usize, value: f64, cx: &mut Context<Self>) {
        self.evidence.insert(idx, value);
        self.repropagate(cx);
    }

    /// Clear evidence on a node and re-propagate back to the base tree.
    fn reset_evidence(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.evidence.remove(&idx);
        self.repropagate(cx);
    }

    /// Recompute every node's marginal from the body + evidence and refresh the
    /// display values (marginal probability + certainty tier) on the layout.
    fn repropagate(&mut self, cx: &mut Context<Self>) {
        let marginals =
            propagate::recompute_marginals(&self.body, &self.layout.topo_order, &self.evidence);
        for (i, marginal) in marginals.iter().enumerate() {
            if let Some(node) = self.layout.nodes.get_mut(i) {
                node.marginal_probability = Some(*marginal);
                node.certainty_tier = Some(hkask_forecast::certainty_tier(*marginal).to_string());
            }
        }
        cx.notify();
    }

    /// Convert a screen (window) point to graph-space coordinates.
    fn screen_to_graph(
        &self,
        screen: Point<Pixels>,
        bounds: Bounds<Pixels>,
    ) -> Option<Point<Pixels>> {
        let (scale, ox, oy) = transform(
            bounds,
            self.layout.width,
            self.layout.height,
            self.pan,
            self.zoom,
        );
        if scale <= 0.0 {
            return None;
        }
        Some(point(
            px((screen.x.as_f32() - ox) / scale),
            px((screen.y.as_f32() - oy) / scale),
        ))
    }

    /// Find the node (if any) under a graph-space point.
    fn node_at(&self, graph: Point<Pixels>) -> Option<usize> {
        let gx = graph.x.as_f32();
        let gy = graph.y.as_f32();
        let r = HIT_RADIUS;
        self.layout
            .nodes
            .iter()
            .enumerate()
            .find(|(_, node)| {
                let dx = node.position.x.as_f32() - gx;
                let dy = node.position.y.as_f32() - gy;
                dx * dx + dy * dy < r * r
            })
            .map(|(i, _)| i)
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Record the press position so the next mouse-move can pan from here.
        self.last_mouse = Some(event.position);
    }

    fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.last_bounds.get() else {
            return;
        };
        // Pan while the left button is held.
        if event.pressed_button == Some(MouseButton::Left) {
            if let Some(last) = self.last_mouse {
                let delta = event.position - last;
                self.pan += delta;
                self.last_mouse = Some(event.position);
                cx.notify();
            }
            return;
        }
        self.last_mouse = None;
        // Otherwise, hit-test for hover.
        let Some(graph) = self.screen_to_graph(event.position, bounds) else {
            return;
        };
        let hit = self.node_at(graph);
        if hit != self.hovered {
            self.hovered = hit;
            cx.notify();
        }
    }

    fn handle_click(&mut self, event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.last_bounds.get() else {
            return;
        };
        let Some(graph) = self.screen_to_graph(event.position(), bounds) else {
            return;
        };
        self.selected = self.node_at(graph);
        self.last_mouse = None;
        cx.notify();
    }

    fn handle_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.last_bounds.get() else {
            return;
        };
        let gw = self.layout.width.as_f32();
        let gh = self.layout.height.as_f32();
        // Zoom factor from vertical scroll delta.
        let delta_y = event.delta.pixel_delta(window.line_height()).y.as_f32();
        let factor = 1.0 - delta_y * 0.01;
        let new_zoom = (self.zoom * factor).clamp(0.1, 4.0);
        if (new_zoom - self.zoom).abs() < 1e-4 {
            return;
        }
        // Anchor the zoom at the cursor: keep the graph point under the cursor
        // fixed by adjusting pan.
        let (scale_old, ox_old, oy_old) = transform(
            bounds,
            self.layout.width,
            self.layout.height,
            self.pan,
            self.zoom,
        );
        if scale_old <= 0.0 {
            return;
        }
        let graph_x = (event.position.x.as_f32() - ox_old) / scale_old;
        let graph_y = (event.position.y.as_f32() - oy_old) / scale_old;
        let fit = (bounds.size.width.as_f32() / gw).min(bounds.size.height.as_f32() / gh);
        let new_scale = fit * new_zoom;
        let center_offset_x = (bounds.size.width.as_f32() - gw * new_scale) / 2.0;
        let center_offset_y = (bounds.size.height.as_f32() - gh * new_scale) / 2.0;
        let new_pan_x = event.position.x.as_f32()
            - bounds.origin.x.as_f32()
            - center_offset_x
            - graph_x * new_scale;
        let new_pan_y = event.position.y.as_f32()
            - bounds.origin.y.as_f32()
            - center_offset_y
            - graph_y * new_scale;
        self.zoom = new_zoom;
        self.pan = point(px(new_pan_x), px(new_pan_y));
        cx.notify();
    }

    /// Subject for the header (the body's subject, if any).
    fn subject_for_header(&self) -> String {
        self.body.subject.clone().unwrap_or_default()
    }

    /// Joint probability for the header — only meaningful before any evidence
    /// override (evidence changes the tree; the cached joint is stale then), so
    /// hide it once evidence is set.
    fn joint_probability_for_header(&self) -> Option<f64> {
        if self.evidence.is_empty() {
            self.body.joint_probability
        } else {
            None
        }
    }
}

impl Focusable for GraphWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GraphWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.layout.nodes.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().colors().text_muted)
                .child("Empty graph — no nodes to lay out.")
                .into_any_element();
        }

        let bounds = self.last_bounds.get();

        // Canvas: draws edges + node circles using its OWN bounds (fresh each
        // paint) + the captured pan/zoom. Caches bounds for the overlay/hit-tests
        // and requests one re-render on the first paint so labels appear.
        let draw_layout = self.layout.clone();
        let draw_hovered = self.hovered;
        let draw_selected = self.selected;
        let draw_pan = self.pan;
        let draw_zoom = self.zoom;
        let last_bounds = self.last_bounds.clone();
        let graph_canvas = canvas(
            move |_, _, _| {},
            move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
                let was_none = last_bounds.replace(Some(bounds)).is_none();
                let (scale, ox, oy) = transform(
                    bounds,
                    draw_layout.width,
                    draw_layout.height,
                    draw_pan,
                    draw_zoom,
                );
                draw_graph(
                    &draw_layout,
                    ox,
                    oy,
                    scale,
                    draw_hovered,
                    draw_selected,
                    window,
                    cx,
                );
                if was_none {
                    // Bounds are now known — re-render so the label overlay, which
                    // reads cached bounds, appears on the next frame.
                    window.request_animation_frame();
                }
            },
        )
        .size_full()
        .absolute();

        // Overlay: labels + tooltip + evidence buttons, positioned from the
        // cached bounds + transform (container-relative coords).
        let mut overlays: Vec<AnyElement> = Vec::new();
        if let Some(bounds) = bounds {
            let (scale, ox, oy) = transform(
                bounds,
                self.layout.width,
                self.layout.height,
                self.pan,
                self.zoom,
            );
            let bx = bounds.origin.x.as_f32();
            let by = bounds.origin.y.as_f32();
            for node in &self.layout.nodes {
                let sx = ox + node.position.x.as_f32() * scale - bx;
                let sy = oy + node.position.y.as_f32() * scale - by;
                let label = format!(
                    "{}  {}%",
                    node.name,
                    (node.marginal_probability.unwrap_or(0.0) * 100.0).round() as u32
                );
                overlays.push(
                    div()
                        .absolute()
                        .left(px(sx + NODE_RADIUS + 6.0))
                        .top(px(sy - 8.0))
                        .text_xs()
                        .text_color(cx.theme().colors().text)
                        .child(label)
                        .into_any_element(),
                );
            }

            // Tooltip + evidence controls for the hovered/selected node.
            let focus = self.hovered.or(self.selected);
            if let Some((idx, node)) = focus.and_then(|i| self.layout.nodes.get(i).map(|n| (i, n)))
            {
                let sx = ox + node.position.x.as_f32() * scale - bx;
                let sy = oy + node.position.y.as_f32() * scale - by;
                let is_evidence = self.evidence.contains_key(&idx);
                let text_color = cx.theme().colors().text;
                let muted = cx.theme().colors().text_muted;
                let accent = cx.theme().colors().text_accent;
                let surface = cx.theme().colors().elevated_surface_background;
                let border = cx.theme().colors().border;

                let tip = div()
                    .absolute()
                    .left(px(sx + NODE_RADIUS + 10.0))
                    .top(px(sy - 70.0))
                    .p_2()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .max_w(px(320.0))
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_color)
                            .child(node.name.clone()),
                    )
                    .when_some(node.question.as_ref(), |t, q| {
                        t.child(div().text_xs().text_color(muted).child(q.clone()))
                    })
                    .child(div().text_xs().text_color(accent).child(format!(
                        "P = {}%",
                        (node.marginal_probability.unwrap_or(0.0) * 100.0).round() as u32
                    )))
                    .when(is_evidence, |t| {
                        t.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("evidence (overridden)"),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .mt_1()
                            .child(
                                div()
                                    .id(("ev-90", idx))
                                    .text_xs()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_evidence(idx, 0.9, cx);
                                        cx.stop_propagation();
                                    }))
                                    .child("Set P≈90%"),
                            )
                            .child(
                                div()
                                    .id(("ev-10", idx))
                                    .text_xs()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_evidence(idx, 0.1, cx);
                                        cx.stop_propagation();
                                    }))
                                    .child("Set P≈10%"),
                            )
                            .when(is_evidence, |t| {
                                t.child(
                                    div()
                                        .id(("ev-reset", idx))
                                        .text_xs()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.reset_evidence(idx, cx);
                                            cx.stop_propagation();
                                        }))
                                        .child("Reset"),
                                )
                            }),
                    );
                overlays.push(tip.into_any_element());
            }
        }

        let header = div()
            .text_xs()
            .text_color(cx.theme().colors().text_muted)
            .child(self.subject_for_header())
            .when_some(self.joint_probability_for_header(), |t, joint| {
                t.child(format!("   joint = {}%", (joint * 100.0).round() as u32))
            });

        div()
            .id("graph-widget")
            .size_full()
            .overflow_hidden()
            .relative()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .min_h_0()
                    .child(graph_canvas)
                    .children(overlays),
            )
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_click(cx.listener(Self::handle_click))
            .on_scroll_wheel(cx.listener(Self::handle_scroll))
            .into_any_element()
    }
}

/// Process-foreground cache of live `GraphWidget` entities keyed by
/// `(window_id, body_hash)`, so pan/zoom/evidence survive `ConversationView`
/// re-renders (which would otherwise `cx.new` a fresh entity each time and wipe
/// interaction state). The markdown renderer invokes `render_event_tree` on every
/// re-layout; without this cache the widget is recreated per token during
/// streaming.
///
/// Keying by window as well as body means each window showing the same graph
/// block gets its OWN entity — independent pan/zoom/evidence, and its own
/// `last_bounds` (no cross-window bounds race). Foreground-thread-only (markdown
/// layout runs on the GPUI foreground thread), so a `thread_local` is correct and
/// avoids any `Send`/`Sync` question about `Entity`. Bounded by `GRAPH_CACHE_CAP`
/// with FIFO eviction.
const GRAPH_CACHE_CAP: usize = 64;

thread_local! {
    static GRAPH_CACHE: RefCell<GraphEntityCache> = RefCell::new(GraphEntityCache::default());
}

#[derive(Default)]
struct GraphEntityCache {
    by_hash: HashMap<(u64, u64), Entity<GraphWidget>>,
    order: VecDeque<(u64, u64)>,
}

impl GraphEntityCache {
    fn get_or_insert(
        &mut self,
        cx: &mut App,
        key: (u64, u64),
        build: impl FnOnce(&mut Context<GraphWidget>) -> GraphWidget,
    ) -> Entity<GraphWidget> {
        if let Some(entity) = self.by_hash.get(&key) {
            return entity.clone();
        }
        while self.by_hash.len() >= GRAPH_CACHE_CAP {
            match self.order.pop_front() {
                Some(old) => {
                    self.by_hash.remove(&old);
                }
                None => break,
            }
        }
        let entity = cx.new(build);
        self.by_hash.insert(key, entity.clone());
        self.order.push_back(key);
        entity
    }
}

fn body_hash(body: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    hasher.finish()
}

/// Build the inline element for the D18 seam: a full-size div wrapping the
/// `GraphWidget` view. The entity is cached by `(window_id, body_hash)` so
/// interaction state (pan/zoom/evidence) persists across `ConversationView`
/// re-renders instead of being wiped by a fresh `cx.new` each re-layout, and so
/// each window showing the same block gets an independent widget.
pub fn render_event_tree(
    body: GraphBlockBody,
    body_text: &str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let window_id = window.window_handle().window_id().as_u64();
    let key = (window_id, body_hash(body_text));
    let entity = GRAPH_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .get_or_insert(cx, key, |cx| GraphWidget::new(body, cx))
    });
    div().size_full().child(entity).into_any_element()
}

/// The fit+zoom+pan map: graph-space → screen-space.
/// Returns `(scale, origin_x, origin_y)` in raw f32 (pixels).
fn transform(
    bounds: Bounds<Pixels>,
    graph_w: Pixels,
    graph_h: Pixels,
    pan: Point<Pixels>,
    zoom: f32,
) -> (f32, f32, f32) {
    let bw = bounds.size.width.as_f32();
    let bh = bounds.size.height.as_f32();
    let gw = graph_w.as_f32();
    let gh = graph_h.as_f32();
    if gw <= 0.0 || gh <= 0.0 {
        return (1.0, bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
    }
    let fit = (bw / gw).min(bh / gh);
    let scale = fit * zoom;
    let ox = bounds.origin.x.as_f32() + (bw - gw * scale) / 2.0 + pan.x.as_f32();
    let oy = bounds.origin.y.as_f32() + (bh - gh * scale) / 2.0 + pan.y.as_f32();
    (scale, ox, oy)
}

/// Draw edges + node circles (and a highlight ring for hovered/selected) in
/// screen-space, given the transform origin and scale.
fn draw_graph(
    layout: &LayeredLayout,
    ox: f32,
    oy: f32,
    scale: f32,
    hovered: Option<usize>,
    selected: Option<usize>,
    window: &mut Window,
    cx: &App,
) {
    let edge_color = cx.theme().colors().border;

    for (parent, child) in &layout.edges {
        let from = layout.nodes[*parent].position;
        let to = layout.nodes[*child].position;
        let mut builder = gpui::PathBuilder::stroke(px(1.5));
        builder.move_to(point(
            px(ox + from.x.as_f32() * scale),
            px(oy + from.y.as_f32() * scale),
        ));
        builder.line_to(point(
            px(ox + to.x.as_f32() * scale),
            px(oy + to.y.as_f32() * scale),
        ));
        if let Ok(path) = builder.build() {
            window.paint_path(path, edge_color);
        }
    }

    for (idx, node) in layout.nodes.iter().enumerate() {
        let center = point(
            px(ox + node.position.x.as_f32() * scale),
            px(oy + node.position.y.as_f32() * scale),
        );
        let color = node_color(&node.certainty_tier);
        let radius = px(NODE_RADIUS * scale.max(0.4));
        draw_circle(center, radius, color, window);
        if hovered == Some(idx) || selected == Some(idx) {
            draw_ring(
                center,
                px(NODE_RADIUS * scale.max(0.4) + 4.0),
                cx.theme().colors().text_accent,
                window,
            );
        }
    }
}

fn node_color(certainty: &Option<String>) -> Rgba {
    match certainty.as_deref() {
        Some("proximate") => rgb(0x4CAF50), // green — ≥67%
        Some("probable") => rgb(0xFFA726),  // amber — 33–66%
        Some("possible") => rgb(0xEF5350),  // red — <33%
        _ => rgb(0x607D8B),                 // neutral — unknown/none
    }
}

fn draw_circle(center: Point<Pixels>, radius: Pixels, color: Rgba, window: &mut Window) {
    let mut builder = gpui::PathBuilder::fill();
    builder.move_to(point(center.x + radius, center.y));
    builder.arc_to(
        point(radius, radius),
        px(0.0),
        false,
        true,
        point(center.x - radius, center.y),
    );
    builder.arc_to(
        point(radius, radius),
        px(0.0),
        false,
        true,
        point(center.x + radius, center.y),
    );
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn draw_ring(center: Point<Pixels>, radius: Pixels, color: gpui::Hsla, window: &mut Window) {
    let mut builder = gpui::PathBuilder::stroke(px(2.0));
    builder.move_to(point(center.x + radius, center.y));
    builder.arc_to(
        point(radius, radius),
        px(0.0),
        false,
        true,
        point(center.x - radius, center.y),
    );
    builder.arc_to(
        point(radius, radius),
        px(0.0),
        false,
        true,
        point(center.x + radius, center.y),
    );
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}
