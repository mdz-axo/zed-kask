//! The `GraphWidget` GPUI view — renders the layered event-tree DAG.
//!
//! Drawing uses a `canvas` (edges + node circles, in graph-space pixels) with
//! absolutely-positioned transparent hit-area divs + label divs overlaid at the
//! same graph-space coordinates. Because nodes/labels use the layout's pixel
//! positions directly (no runtime transform), the canvas and the div layers
//! align without any bounds-caching. The container clips the graph to its
//! viewport (`overflow_hidden`); pan/zoom is a follow-up.

use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, FocusHandle, Focusable, IntoElement,
    MouseMoveEvent, ParentElement, Pixels, Point, Render, Rgba, StatefulInteractiveElement, Styled,
    Window, canvas, div, point, prelude::*, px, rgb,
};
use theme::ActiveTheme;

use crate::block::GraphBlockBody;
use crate::layout::LayeredLayout;

const NODE_RADIUS: f32 = 12.0;

/// The graph widget view. Renders inline in agent markdown (via the D18 seam
/// composed by `hkask-viz-core`) or as a standalone panel item.
pub struct GraphWidget {
    layout: LayeredLayout,
    subject: Option<String>,
    joint_probability: Option<f64>,
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
            layout,
            subject: body.subject,
            joint_probability: body.joint_probability,
            hovered: None,
            selected: None,
            focus_handle: cx.focus_handle(),
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

        let layout = self.layout.clone();
        let hovered = self.hovered;
        let selected = self.selected;

        // Edges + node circles, drawn in graph-space pixels (origin = canvas
        // top-left, which equals the graph-sized container's origin).
        let draw_layout = layout.clone();
        let draw_hovered = hovered;
        let draw_selected = selected;
        let graph_canvas = canvas(
            move |_: Bounds<Pixels>, _: &mut Window, _: &mut App| {},
            move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
                draw_graph(
                    &draw_layout,
                    bounds,
                    draw_hovered,
                    draw_selected,
                    window,
                    cx,
                );
            },
        )
        .w(layout.width)
        .h(layout.height);

        let text_color = cx.theme().colors().text;
        let muted_color = cx.theme().colors().text_muted;
        let accent_color = cx.theme().colors().text_accent;
        let border_color = cx.theme().colors().border;
        let surface_color = cx.theme().colors().elevated_surface_background;

        // Per-node hit areas + labels. Built in a loop so each `cx.listener`
        // borrow is released before the next; collected into a Vec so the
        // parent's `children(...)` doesn't hold `cx`.
        let mut overlays: Vec<AnyElement> = Vec::new();
        for (idx, node) in layout.nodes.iter().enumerate() {
            let pos = node.position;
            let label = format!(
                "{}  {}%",
                node.name,
                (node.marginal_probability.unwrap_or(0.0) * 100.0).round() as u32
            );

            let hit = div()
                .id(("graph-node", idx))
                .absolute()
                .left(pos.x - px(NODE_RADIUS))
                .top(pos.y - px(NODE_RADIUS))
                .w(px(NODE_RADIUS * 2.0))
                .h(px(NODE_RADIUS * 2.0))
                .cursor_pointer()
                .on_mouse_move(
                    cx.listener(move |this, _event: &MouseMoveEvent, _window, cx| {
                        if this.hovered != Some(idx) {
                            this.hovered = Some(idx);
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }),
                )
                .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                    this.selected = Some(idx);
                    cx.notify();
                }));
            overlays.push(hit.into_any_element());

            let label_el = div()
                .absolute()
                .left(pos.x + px(NODE_RADIUS + 6.0))
                .top(pos.y - px(8.0))
                .text_xs()
                .text_color(text_color)
                .child(label);
            overlays.push(label_el.into_any_element());
        }

        // Tooltip for the hovered node.
        if let Some(idx) = hovered
            && let Some(node) = layout.nodes.get(idx) {
                let tip = div()
                    .absolute()
                    .left(node.position.x + px(NODE_RADIUS + 10.0))
                    .top(node.position.y - px(64.0))
                    .p_2()
                    .bg(surface_color)
                    .border_1()
                    .border_color(border_color)
                    .max_w(px(300.0))
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_color)
                            .child(node.name.clone()),
                    )
                    .when_some(node.question.as_ref(), |this, question| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted_color)
                                .child(question.clone()),
                        )
                    })
                    .child(div().text_xs().text_color(accent_color).child(format!(
                        "P = {}%",
                        (node.marginal_probability.unwrap_or(0.0) * 100.0).round() as u32
                    )));
                overlays.push(tip.into_any_element());
            }

        let header = div()
            .text_xs()
            .text_color(muted_color)
            .child(self.subject.clone().unwrap_or_default())
            .when_some(self.joint_probability, |this, joint| {
                this.child(format!("   joint = {}%", (joint * 100.0).round() as u32))
            });

        // The graph-sized container (clipped to the viewport). The canvas fills
        // it; hit areas + labels + tooltip overlay on top at the same coords.
        let graph = div()
            .relative()
            .w(layout.width)
            .h(layout.height)
            .child(graph_canvas)
            .children(overlays);

        div()
            .id("graph-widget")
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            // Clear hover when the pointer leaves all nodes (node handlers
            // stop propagation, so this only fires over the background).
            .on_mouse_move(cx.listener(|this, _event: &MouseMoveEvent, _window, cx| {
                if this.hovered.is_some() {
                    this.hovered = None;
                    cx.notify();
                }
            }))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(graph),
            )
            .into_any_element()
    }
}

/// Build the inline element for the D18 seam: a full-size div wrapping the
/// `GraphWidget` view.
pub fn render_event_tree(body: GraphBlockBody, _window: &mut Window, cx: &mut App) -> AnyElement {
    let entity = cx.new(|cx| GraphWidget::new(body, cx));
    div().size_full().child(entity).into_any_element()
}

/// Draw edges + node circles (and a highlight ring for hovered/selected) in
/// graph-space pixels, offset by the canvas bounds origin.
fn draw_graph(
    layout: &LayeredLayout,
    bounds: Bounds<Pixels>,
    hovered: Option<usize>,
    selected: Option<usize>,
    window: &mut Window,
    cx: &App,
) {
    let origin = bounds.origin;
    let edge_color = cx.theme().colors().border;

    for (parent, child) in &layout.edges {
        let from = layout.nodes[*parent].position;
        let to = layout.nodes[*child].position;
        let mut builder = gpui::PathBuilder::stroke(px(1.5));
        builder.move_to(point(origin.x + from.x, origin.y + from.y));
        builder.line_to(point(origin.x + to.x, origin.y + to.y));
        if let Ok(path) = builder.build() {
            window.paint_path(path, edge_color);
        }
    }

    for (idx, node) in layout.nodes.iter().enumerate() {
        let center = point(origin.x + node.position.x, origin.y + node.position.y);
        let color = node_color(&node.certainty_tier);
        draw_circle(center, px(NODE_RADIUS), color, window);
        if hovered == Some(idx) || selected == Some(idx) {
            draw_ring(
                center,
                px(NODE_RADIUS + 4.0),
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
