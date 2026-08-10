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

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, FocusHandle, Focusable, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point, Render, Rgba, ScrollWheelEvent,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, prelude::*, px, rgb,
};
use theme::ActiveTheme;
use ui::{Color, Label, LabelCommon, LabelSize};

use crate::block::{EvidenceKind, GraphBlockBody};
use crate::layout::LayeredLayout;
use crate::propagate;

const NODE_RADIUS: f32 = 12.0;
/// Hit radius (in graph-space px) is a bit larger than the drawn node so small
/// nodes are still grabbable when zoomed out.
const HIT_RADIUS: f32 = NODE_RADIUS * 1.6;

/// A saved what-if: a snapshot of the evidence overrides the user explored.
/// The base tree is empty evidence; branches capture non-empty snapshots the
/// user can revert to, load, delete, or compare against base.
struct WhatIfBranch {
    name: String,
    evidence: HashMap<usize, EvidenceKind>,
}

/// The graph widget view. Renders inline in agent markdown (via the D18 seam
/// composed by `hkask-viz-core`) or as a standalone panel item.
pub struct GraphWidget {
    /// The parsed block body — source of truth for edges, conditionals, and
    /// base (root) probabilities. Held so evidence overrides can re-propagate.
    body: GraphBlockBody,
    layout: LayeredLayout,
    /// Evidence overrides: node index → evidence kind (hard clamp or soft
    /// likelihood-ratio update).
    evidence: HashMap<usize, EvidenceKind>,
    pan: Point<Pixels>,
    zoom: f32,
    /// Last mouse position while left-dragging, for pan deltas.
    last_mouse: Option<Point<Pixels>>,
    /// The canvas's last laid-out bounds, set during paint and read in
    /// `render()`/handlers to map between screen and graph coordinates.
    last_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    hovered: Option<usize>,
    selected: Option<usize>,
    /// Saved what-if branches: snapshots of `evidence` the user can reload or
    /// compare against the base (empty-evidence) tree.
    branches: Vec<WhatIfBranch>,
    /// Index into `branches` for the inline compare diff panel; `None` = no
    /// panel. Cleared by `revert_to_base` and `load_branch`.
    compare_branch: Option<usize>,
    /// Whether the DAG is a polytree, enabling backward inference (Pearl π-λ).
    /// Computed once in `new` from the body; used by `repropagate` to choose
    /// between `recompute_posteriors` (polytree) and `recompute_marginals`
    /// (multiply-connected fallback).
    backward_inference_available: bool,
    focus_handle: FocusHandle,
    /// Surfaced when no conversation injector is active so the composed "I
    /// disagree" body is still copyable — visible, not a silent no-op (repo
    /// `.rules`). Cleared after a successful inject.
    disagree_draft: Option<String>,
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
        // Compute polytree status before `body` is moved into the struct.
        let backward_inference_available = crate::propagate::is_polytree(&body);
        // T7: raw signal for the reask/what-if measurement gate. Counted via
        // tracing target `reg.widget.graph_render`. See
        // tasks/widget-interactivity/plan.md (Track 3, decision 10).
        let node_count = body.nodes.len();
        let subject = body.subject.clone().unwrap_or_default();
        tracing::info!(
            target: "reg.widget.graph_render",
            subject = %subject,
            node_count = node_count,
            "REG"
        );
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
            branches: Vec::new(),
            compare_branch: None,
            backward_inference_available,
            focus_handle: cx.focus_handle(),
            disagree_draft: None,
        }
    }

    /// Set a node's probability as observed hard evidence and re-propagate.
    fn set_evidence(&mut self, idx: usize, value: f64, cx: &mut Context<Self>) {
        self.set_evidence_kind(idx, EvidenceKind::Hard(value), cx);
    }

    /// Set a node's evidence as a soft likelihood-ratio update and re-propagate.
    /// `likelihood_ratio` is P(evidence | node=true) / P(evidence | node=false).
    fn set_soft_evidence(&mut self, idx: usize, likelihood_ratio: f64, cx: &mut Context<Self>) {
        self.set_evidence_kind(idx, EvidenceKind::Soft(likelihood_ratio), cx);
    }

    fn set_evidence_kind(&mut self, idx: usize, kind: EvidenceKind, cx: &mut Context<Self>) {
        self.evidence.insert(idx, kind);
        tracing::info!(
            target: "reg.widget.evidence_set",
            node_idx = idx,
            evidence_count = self.evidence.len(),
            "REG"
        );
        self.repropagate(cx);
    }

    /// Clear evidence on a node and re-propagate back to the base tree.
    fn reset_evidence(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.evidence.remove(&idx);
        self.repropagate(cx);
    }

    /// Recompute every node's marginal from the body + evidence and refresh the
    /// display values (marginal probability + certainty tier) on the layout.
    ///
    /// Uses backward inference (`recompute_posteriors`, Pearl π-λ) when the DAG
    /// is a polytree and evidence is set — so evidence on a leaf propagates to
    /// its parents. Falls back to forward-only `recompute_marginals` for
    /// multiply-connected DAGs, with a `tracing::warn!` so an operator can
    /// distinguish "no evidence" from "backward inference unavailable."
    fn repropagate(&mut self, cx: &mut Context<Self>) {
        let marginals = if self.backward_inference_available && !self.evidence.is_empty() {
            propagate::recompute_posteriors(&self.body, &self.layout.topo_order, &self.evidence)
        } else if !self.evidence.is_empty() && !self.backward_inference_available {
            tracing::warn!(
                target: "hkask-graph-widget",
                "DAG is multiply-connected; backward inference requires a polytree, falling back to forward-only marginalization"
            );
            propagate::recompute_marginals(&self.body, &self.layout.topo_order, &self.evidence)
        } else {
            propagate::recompute_marginals(&self.body, &self.layout.topo_order, &self.evidence)
        };
        for (i, marginal) in marginals.iter().enumerate() {
            if let Some(node) = self.layout.nodes.get_mut(i) {
                node.marginal_probability = Some(*marginal);
                node.certainty_tier = Some(hkask_forecast::certainty_tier(*marginal).to_string());
            }
        }
        cx.notify();
    }

    /// Save the current evidence overrides as a named what-if branch. No-op
    /// when evidence is empty (the base tree is not saved as a branch).
    fn save_branch(&mut self, cx: &mut Context<Self>) {
        if !self.evidence.is_empty() {
            self.branches.push(WhatIfBranch {
                name: format!("what-if {}", self.branches.len() + 1),
                evidence: self.evidence.clone(),
            });
            cx.notify();
        }
    }

    /// Discard all evidence overrides and return to the agent's base tree.
    /// Also clears the compare panel — no comparing against a branch while at
    /// base.
    fn revert_to_base(&mut self, cx: &mut Context<Self>) {
        self.evidence.clear();
        self.compare_branch = None;
        self.repropagate(cx);
    }

    /// Load a saved branch's evidence overrides as the live view. No-op on an
    /// out-of-bounds index. Clears the compare panel — the loaded branch is
    /// the live view now, not a compare target.
    fn load_branch(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(branch) = self.branches.get(idx) {
            self.evidence = branch.evidence.clone();
            self.compare_branch = None;
            self.repropagate(cx);
        }
    }

    /// Delete a saved branch. No-op on an out-of-bounds index. Adjusts
    /// `compare_branch` so it stays valid or clears it if it pointed at the
    /// deleted branch.
    fn delete_branch(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.branches.len() {
            return;
        }
        self.branches.remove(idx);
        match self.compare_branch {
            Some(current) if current == idx => self.compare_branch = None,
            Some(current) if current > idx => self.compare_branch = Some(current - 1),
            _ => {}
        }
        cx.notify();
    }

    /// Toggle the compare diff panel for a saved branch. No-op on an
    /// out-of-bounds index.
    fn toggle_compare(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.branches.len() {
            return;
        }
        self.compare_branch = if self.compare_branch == Some(idx) {
            None
        } else {
            Some(idx)
        };
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

    /// Joint probability for the header. When no evidence is set, this is the
    /// server-provided `body.joint_probability`. When evidence is set, the cached
    /// value is stale, so recompute it as the product of the current node
    /// marginals (under the same independence assumption the propagation engine
    /// makes). This keeps the header informative during what-if exploration
    /// rather than hiding the joint entirely.
    fn joint_probability_for_header(&self) -> Option<f64> {
        if self.evidence.is_empty() {
            return self.body.joint_probability;
        }
        // Recompute from the current marginals stored on the layout by
        // `repropagate`. Product under independence: Π_i P(node_i).
        let product = self
            .layout
            .nodes
            .iter()
            .filter_map(|n| n.marginal_probability)
            .fold(1.0_f64, |acc, p| acc * p.clamp(0.0, 1.0));
        Some(product)
    }

    /// Compose the "I disagree" body. References the tree's subject and, when a
    /// node is selected, names it so the agent can correlate the revision
    /// request to the exact node whose probability or conditional the user
    /// disputes. The graph widget carries no provenance field (decision 10 —
    /// it is deliberately local-only), so a static tool label is baked into the
    /// framing instead of `provenance.tool`. Falls back to "this event tree"
    /// when the subject is absent (grill-me edge case c) and omits the node
    /// clause when no node is selected (edge case d — `.get` bounds check, no
    /// panicking indexing).
    fn compose_disagree_body(&self) -> String {
        let subject = self
            .body
            .subject
            .clone()
            .unwrap_or_else(|| "this event tree".to_string());
        let node_clause = self
            .selected
            .and_then(|index| self.body.nodes.get(index))
            .map(|node| {
                format!(
                    " — specifically node '{}'",
                    node.name.clone().unwrap_or_else(|| node.id.clone())
                )
            })
            .unwrap_or_default();
        format!(
            "Re: the event tree for {subject}{node_clause}.\n\
             I believe a node's probability or the conditional structure is incorrect. \
             Please re-check the base rates and conditionals.\n\n\
             My concern: "
        )
    }

    /// The "I disagree" affordance handler (C). Composes the revision request
    /// and injects it back into the active conversation via the shared
    /// `compose_back_via_injector` (S10/R2). When no conversation is active,
    /// surfaces the composed body as a copyable draft instead of a silent
    /// no-op (repo `.rules`). Never auto-sends — the production injector only
    /// pre-fills the composer; the user reviews and submits.
    fn on_disagree_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.compose_disagree_body();
        let widget = cx.entity().downgrade();
        let draft = hkask_conversation_injector::compose_back_via_injector(
            body,
            window,
            cx,
            widget,
            |this, draft| {
                this.disagree_draft = draft;
            },
        );
        self.disagree_draft = draft;
        cx.notify();
    }
}

impl Focusable for GraphWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// T7: a what-if the user explored (evidence was set) is being lost because the
// widget is dropped without a saved branch (branches do not exist yet — T8b).
// Counted via tracing target `reg.widget.whatif_discarded`; paired with
// `reg.widget.evidence_set` to form the discard rate that gates Track 3.
// See tasks/widget-interactivity/plan.md (decision 10, T7).
impl Drop for GraphWidget {
    fn drop(&mut self) {
        if !self.evidence.is_empty() {
            tracing::info!(
                target: "reg.widget.whatif_discarded",
                subject = %self.body.subject.clone().unwrap_or_default(),
                node_count = self.body.nodes.len(),
                evidence_count = self.evidence.len(),
                "REG",
            );
        }
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
                            .child(
                                div()
                                    .id(("ev-soft", idx))
                                    .text_xs()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        // Soft evidence: likelihood ratio 3:1 (moderate
                                        // "I have evidence this is more likely" signal).
                                        // The Bayesian update moves the marginal toward 1
                                        // without clamping, then propagates.
                                        this.set_soft_evidence(idx, 3.0, cx);
                                        cx.stop_propagation();
                                    }))
                                    .child("Observe (LR 3:1)"),
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
            .flex()
            .gap_2()
            .items_center()
            .text_xs()
            .text_color(cx.theme().colors().text_muted)
            .child(self.subject_for_header())
            .when_some(self.joint_probability_for_header(), |t, joint| {
                t.child(format!("   joint = {}%", (joint * 100.0).round() as u32))
            })
            .when(
                !self.backward_inference_available && !self.evidence.is_empty(),
                |t| {
                    t.child(
                        "   (backward inference unavailable for this graph shape — evidence propagates forward only)",
                    )
                },
            )
            // C: "I disagree" affordance — composes a revision request back
            // into the active conversation (D21). Board-level (the whole
            // tree); optionally references the selected node in the body.
            .child(
                div()
                    .id("graph-disagree")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.on_disagree_click(window, cx);
                    }))
                    .child(
                        Label::new("I disagree")
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    ),
            );

        // What-if branch controls: save/revert + per-branch load/compare/delete.
        let mut branch_chips: Vec<AnyElement> = Vec::new();
        for (i, branch) in self.branches.iter().enumerate() {
            let is_compare = self.compare_branch == Some(i);
            branch_chips.push(
                div()
                    .id(("whatif-load", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.load_branch(i, cx);
                        cx.stop_propagation();
                    }))
                    .child(format!("{}: Load", branch.name))
                    .into_any_element(),
            );
            branch_chips.push(
                div()
                    .id(("whatif-compare", i))
                    .cursor_pointer()
                    .when(is_compare, |t| {
                        t.text_color(cx.theme().colors().text_accent)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_compare(i, cx);
                        cx.stop_propagation();
                    }))
                    .child("Compare")
                    .into_any_element(),
            );
            branch_chips.push(
                div()
                    .id(("whatif-delete", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_branch(i, cx);
                        cx.stop_propagation();
                    }))
                    .child("×")
                    .into_any_element(),
            );
        }
        let controls = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .text_xs()
            .when(!self.evidence.is_empty(), |t| {
                t.child(
                    div()
                        .id("whatif-save")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_branch(cx);
                            cx.stop_propagation();
                        }))
                        .child("Save what-if"),
                )
                .child(
                    div()
                        .id("whatif-revert")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.revert_to_base(cx);
                            cx.stop_propagation();
                        }))
                        .child("Revert to base"),
                )
            })
            .children(branch_chips);

        // Compare diff panel: recompute base vs branch marginals fresh each
        // render (no stale snapshot) and list per-node deltas. Reads
        // `self.branches[idx].evidence` at render time so it tracks edits.
        let compare_panel = self
            .compare_branch
            .and_then(|idx| self.branches.get(idx).map(|branch| (idx, branch)))
            .map(|(_idx, branch)| {
                let base_marginals = propagate::recompute_marginals(
                    &self.body,
                    &self.layout.topo_order,
                    &HashMap::new(),
                );
                let branch_marginals = propagate::recompute_marginals(
                    &self.body,
                    &self.layout.topo_order,
                    &branch.evidence,
                );
                let pct = |m: Option<&f64>| (m.copied().unwrap_or(0.0) * 100.0).round() as i32;
                let mut rows: Vec<AnyElement> = Vec::new();
                for (i, node) in self.body.nodes.iter().enumerate() {
                    let base_pct = pct(base_marginals.get(i));
                    let branch_pct = pct(branch_marginals.get(i));
                    let delta = branch_pct - base_pct;
                    let name = node.name.clone().unwrap_or_else(|| node.id.clone());
                    rows.push(
                        div()
                            .child(format!(
                                "{}: {}% → {}% (Δ{:+}%)",
                                name, base_pct, branch_pct, delta
                            ))
                            .into_any_element(),
                    );
                }
                div()
                    .p_2()
                    .bg(cx.theme().colors().elevated_surface_background)
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .text_xs()
                    .child(
                        div()
                            .text_color(cx.theme().colors().text)
                            .child(format!("Compare: {} vs base", branch.name)),
                    )
                    .children(rows)
            });

        div()
            .id("graph-widget")
            .size_full()
            .overflow_hidden()
            .relative()
            .flex()
            .flex_col()
            .child(header)
            // Fallback draft (no active conversation): surface the composed
            // body so the user can copy it into chat — visible, not a silent
            // no-op (repo `.rules`).
            .when_some(self.disagree_draft.clone(), |this, draft| {
                this.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .child(
                            Label::new(draft)
                                .size(LabelSize::XSmall)
                                .color(Color::Warning),
                        ),
                )
            })
            .child(controls)
            .when_some(compare_panel, |t, panel| t.child(panel))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{DependencyBody, EvidenceKind, GraphBlockBody, NodeBody};

    /// Two-node body: `a` (root, P=0.5) → `b` (conditionals [0.1, 0.6]).
    /// Base P(b) = 0.1*0.5 + 0.6*0.5 = 0.35.
    fn make_body() -> GraphBlockBody {
        let a = NodeBody {
            id: "a".into(),
            name: Some("a".into()),
            question: None,
            marginal_probability: Some(0.5),
            depends_on: Vec::new(),
            parents: Vec::new(),
        };
        let b = NodeBody {
            id: "b".into(),
            name: Some("b".into()),
            question: None,
            marginal_probability: Some(0.0),
            depends_on: vec![DependencyBody {
                parent_event_ids: vec!["a".into()],
                conditionals: vec![0.1, 0.6],
            }],
            parents: Vec::new(),
        };
        GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes: vec![a, b],
            ontology: None,
        }
    }

    #[gpui::test]
    async fn save_branch_stores_current_evidence(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        assert_eq!(widget.read_with(cx, |w, _| w.branches.len()), 1);
        let evidence = widget.read_with(cx, |w, _| w.branches[0].evidence.clone());
        let mut expected = HashMap::new();
        expected.insert(0, EvidenceKind::Hard(0.9));
        assert_eq!(evidence, expected);
    }

    #[gpui::test]
    async fn save_branch_noop_when_evidence_empty(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        assert_eq!(widget.read_with(cx, |w, _| w.branches.len()), 0);
    }

    #[gpui::test]
    async fn revert_to_base_clears_evidence(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        assert!(!widget.read_with(cx, |w, _| w.evidence.is_empty()));
        widget.update(cx, |w, cx| w.revert_to_base(cx));
        assert!(widget.read_with(cx, |w, _| w.evidence.is_empty()));
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), None);
    }

    #[gpui::test]
    async fn joint_probability_recomputes_after_evidence(cx: &mut gpui::TestAppContext) {
        // make_body has joint_probability = None, so build a body with a stale
        // server joint to verify the recompute path overrides it.
        let mut body = make_body();
        body.joint_probability = Some(0.999); // stale server value
        let widget = cx.new(|cx| GraphWidget::new(body, cx));
        // Before evidence: header shows the server-provided joint.
        assert_eq!(
            widget.read_with(cx, |w, _| w.joint_probability_for_header()),
            Some(0.999)
        );
        // After evidence on node 0 (a, P=0.5 → 0.9): marginals become
        //   a = 0.9, b = 0.1*0.1 + 0.6*0.9 = 0.55
        // joint (product under independence) = 0.9 * 0.55 = 0.495.
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        let joint = widget
            .read_with(cx, |w, _| w.joint_probability_for_header())
            .expect("recomputed joint is present after evidence");
        assert!((joint - 0.495).abs() < 1e-9, "got {joint}");
        // revert restores the server joint.
        widget.update(cx, |w, cx| w.revert_to_base(cx));
        assert_eq!(
            widget.read_with(cx, |w, _| w.joint_probability_for_header()),
            Some(0.999)
        );
    }

    #[gpui::test]
    async fn soft_evidence_updates_marginal_without_clamping(cx: &mut gpui::TestAppContext) {
        // make_body: a (root, P=0.5) → b (conditionals [0.1, 0.6]).
        // Base P(b) = 0.1*0.5 + 0.6*0.5 = 0.35.
        // Soft evidence on a with LR=3.0: P'(a) = 0.5*3 / (0.5*3 + 0.5) = 0.75.
        // Then P(b) re-propagates: 0.1*0.25 + 0.6*0.75 = 0.025 + 0.45 = 0.475.
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_soft_evidence(0, 3.0, cx));
        let marginals = widget.read_with(cx, |w, _| {
            w.layout
                .nodes
                .iter()
                .map(|n| n.marginal_probability.unwrap_or(0.0))
                .collect::<Vec<_>>()
        });
        // P(a) should be 0.75 (Bayesian update, not clamped).
        assert!(
            (marginals[0] - 0.75).abs() < 1e-6,
            "P(a) = {}, expected 0.75",
            marginals[0]
        );
        // P(b) should re-propagate to 0.475.
        assert!(
            (marginals[1] - 0.475).abs() < 1e-6,
            "P(b) = {}, expected 0.475",
            marginals[1]
        );
    }

    #[gpui::test]
    async fn load_branch_restores_evidence(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        let saved = widget.read_with(cx, |w, _| w.branches[0].evidence.clone());
        widget.update(cx, |w, cx| w.revert_to_base(cx));
        assert!(widget.read_with(cx, |w, _| w.evidence.is_empty()));
        widget.update(cx, |w, cx| w.load_branch(0, cx));
        assert_eq!(widget.read_with(cx, |w, _| w.evidence.clone()), saved);
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), None);
    }

    #[gpui::test]
    async fn delete_branch_adjusts_compare_index(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.1, cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        assert_eq!(widget.read_with(cx, |w, _| w.branches.len()), 2);
        widget.update(cx, |w, cx| w.toggle_compare(1, cx));
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), Some(1));
        widget.update(cx, |w, cx| w.delete_branch(0, cx));
        // The compared branch shifted from index 1 to index 0 after removal.
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), Some(0));
        assert_eq!(widget.read_with(cx, |w, _| w.branches.len()), 1);
    }

    #[gpui::test]
    async fn toggle_compare_toggles(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        widget.update(cx, |w, cx| w.toggle_compare(0, cx));
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), Some(0));
        widget.update(cx, |w, cx| w.toggle_compare(0, cx));
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), None);
    }

    #[gpui::test]
    async fn delete_branch_clears_compare_when_pointing_at_deleted(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| w.set_evidence(0, 0.9, cx));
        widget.update(cx, |w, cx| w.save_branch(cx));
        widget.update(cx, |w, cx| w.toggle_compare(0, cx));
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), Some(0));
        widget.update(cx, |w, cx| w.delete_branch(0, cx));
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), None);
    }

    #[gpui::test]
    async fn out_of_bounds_indices_are_noops(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| GraphWidget::new(make_body(), cx));
        widget.update(cx, |w, cx| {
            w.load_branch(7, cx);
            w.delete_branch(7, cx);
            w.toggle_compare(7, cx);
        });
        assert_eq!(widget.read_with(cx, |w, _| w.branches.len()), 0);
        assert_eq!(widget.read_with(cx, |w, _| w.compare_branch), None);
        assert!(widget.read_with(cx, |w, _| w.evidence.is_empty()));
    }

    // ── "I disagree" compose-back affordance (C, D21) ─────────────────────
    //
    // The injector is a per-app GPUI global, so each test's `TestAppContext`
    // has its own — no cross-test serialization or RAII reset is needed.

    /// Records the body of every `inject` call. `Send + Sync` for the
    /// `Arc<dyn ConversationInjector>` global.
    #[derive(Default)]
    struct MockConversationInjector {
        bodies: std::sync::Mutex<Vec<String>>,
    }

    impl hkask_conversation_injector::ConversationInjector for MockConversationInjector {
        fn inject(
            &self,
            body: String,
            _window: &mut gpui::Window,
            _cx: &mut gpui::App,
        ) -> gpui::Task<Result<(), String>> {
            self.bodies
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(body);
            gpui::Task::ready(Ok(()))
        }
    }

    /// Trivial root view for `add_window_view` so the test can obtain a `Window`
    /// for `on_disagree_click` without rendering `GraphWidget` (which would
    /// need a theme global this leaf crate's tests don't initialise). Renders a
    /// bare `div()`.
    struct DummyView;
    impl Render for DummyView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// `make_body` with a subject set, so the disagree body can reference it.
    fn body_with_subject() -> GraphBlockBody {
        let mut body = make_body();
        body.subject = Some("Q3 launch".into());
        body
    }

    #[gpui::test]
    async fn disagree_routes_through_injector(cx: &mut gpui::TestAppContext) {
        let mock = std::sync::Arc::new(MockConversationInjector::default());
        cx.update(|cx| {
            hkask_conversation_injector::set_active_injector(cx, Some(mock.clone()));
        });

        let body = body_with_subject();
        // Use a throwaway window root so we get a `Window` for `on_disagree_click`
        // without rendering `GraphWidget` (no theme global in these tests).
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| GraphWidget::new(body, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        let bodies = mock
            .bodies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(bodies.len(), 1, "exactly one inject");
        assert!(bodies[0].contains("Re:"), "body references the revision");
        assert!(
            bodies[0].contains("Q3 launch"),
            "body references the subject"
        );

        // A successful inject clears the fallback draft.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        assert!(draft.is_none(), "draft cleared after a successful inject");
    }

    #[gpui::test]
    async fn disagree_surfaces_draft_when_no_injector(cx: &mut gpui::TestAppContext) {
        // Per-app global starts empty — no injector is wired by default.

        let body = body_with_subject();
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| GraphWidget::new(body, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        // No injector: the composed body is surfaced as a copyable draft
        // (visible, not a silent no-op — repo `.rules`), and no panic.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        let draft = draft.expect("draft surfaced when no injector is active");
        assert!(draft.contains("Re:"), "draft carries the revision prefix");
    }

    #[gpui::test]
    async fn disagree_body_falls_back_when_subject_absent(cx: &mut gpui::TestAppContext) {
        // grill-me edge case (c): absent subject → generic "this event tree"
        // framing. `compose_disagree_body` is pure, so no window is needed.

        let widget = cx.update(|cx| cx.new(|cx| GraphWidget::new(make_body(), cx)));
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("this event tree"),
            "absent subject falls back to the generic framing"
        );
    }

    #[gpui::test]
    async fn disagree_body_references_selected_node(cx: &mut gpui::TestAppContext) {
        // grill-me edge case (d): with a selected node → body references the
        // node name; with no selection → no node clause. Uses `.get` bounds
        // check, so an out-of-range `selected` is a no-op (no panic).
        let widget = cx.update(|cx| cx.new(|cx| GraphWidget::new(body_with_subject(), cx)));
        // No selection: no node clause.
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            !body.contains("specifically node"),
            "no node clause when nothing is selected"
        );

        // Select node 0 ("a"): body references it.
        widget.update(cx, |widget, _cx| {
            widget.selected = Some(0);
        });
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("specifically node 'a'"),
            "body references the selected node name"
        );

        // Out-of-range selection: no node clause, no panic.
        widget.update(cx, |widget, _cx| {
            widget.selected = Some(99);
        });
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            !body.contains("specifically node"),
            "out-of-range selection adds no clause"
        );
    }
}
