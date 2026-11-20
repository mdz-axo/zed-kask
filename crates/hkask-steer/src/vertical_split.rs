//! Shared vertical split state and layout for Steer panels with a viewer above
//! a director (`media_panel`, `nostr_panel`).

use agent_ui::ConversationView;
use gpui::{
    Bounds, ClickEvent, Context, DefiniteLength, DragMoveEvent, Entity, Pixels, SharedString, px,
};
use ui::prelude::*;

use crate::ThreadPicker;

/// Drag payload used by the divider handle and panel-root drag surface.
struct VerticalSplitDrag;

/// Height of the divider's invisible grab area. The visible rule remains 1px.
const SPLIT_HANDLE_HIT_HEIGHT: f32 = 8.0;

const DEFAULT_BOTTOM_FRACTION: f32 = 0.5;
const MIN_BOTTOM_FRACTION: f32 = 0.2;
const MAX_BOTTOM_FRACTION: f32 = 0.8;

/// In-memory split state. The bottom pane keeps between 20% and 80% of the
/// available height so neither the viewer nor the Steer editor can be starved.
#[derive(Debug, Clone)]
pub struct VerticalSplitState {
    bottom_fraction: f32,
}

impl Default for VerticalSplitState {
    fn default() -> Self {
        Self {
            bottom_fraction: DEFAULT_BOTTOM_FRACTION,
        }
    }
}

impl VerticalSplitState {
    pub fn bottom_fraction(&self) -> f32 {
        self.bottom_fraction
    }

    fn update_from_drag(&mut self, event: &DragMoveEvent<VerticalSplitDrag>) {
        if let Some(fraction) = bottom_fraction_from_pointer(event.event.position.y, event.bounds) {
            self.bottom_fraction = fraction;
        }
    }

    fn reset(&mut self) {
        self.bottom_fraction = DEFAULT_BOTTOM_FRACTION;
    }
}

/// Render a viewer pane above a Steer director pane separated by a draggable
/// 1px divider (with an 8px hit area; double-click resets the split).
///
/// Preserves `{id_prefix}-split-divider` and `{id_prefix}-split-handle` element
/// IDs and `min_h_0()` / `min_w_0()` narrow-dock shrink invariants on the
/// viewer and director panes.
pub fn render_vertical_split<P: 'static>(
    id_prefix: &'static str,
    bottom_fraction: f32,
    viewer: impl IntoElement,
    thread_picker: Entity<ThreadPicker>,
    conversation: Option<Entity<ConversationView>>,
    split_mut: fn(&mut P) -> &mut VerticalSplitState,
    cx: &mut Context<P>,
) -> impl IntoElement {
    let divider = div()
        .id(SharedString::from(format!("{id_prefix}-split-divider")))
        .relative()
        .w_full()
        .flex_shrink_0()
        .h(px(1.))
        .bg(cx.theme().colors().border_variant)
        .child(
            div()
                .id(SharedString::from(format!("{id_prefix}-split-handle")))
                .absolute()
                .top(px(-SPLIT_HANDLE_HIT_HEIGHT / 2.0))
                .h(px(SPLIT_HANDLE_HIT_HEIGHT))
                .w_full()
                .cursor_row_resize()
                .block_mouse_except_scroll()
                .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                    if event.click_count() >= 2 {
                        split_mut(this).reset();
                        cx.notify();
                    }
                    cx.stop_propagation();
                }))
                .on_drag(VerticalSplitDrag, |_, _, _, cx| cx.new(|_| gpui::Empty)),
        );

    let director = div()
        .w_full()
        .h(DefiniteLength::Fraction(bottom_fraction))
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            h_flex()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .child(thread_picker),
        )
        .when_some(conversation, |el, conversation| el.child(conversation));

    v_flex()
        .size_full()
        .min_h_0()
        .overflow_hidden()
        .on_drag_move::<VerticalSplitDrag>(cx.listener(move |this, event, _window, cx| {
            split_mut(this).update_from_drag(event);
            cx.notify();
        }))
        .child(
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_hidden()
                .child(viewer),
        )
        .child(divider)
        .child(director)
}

fn bottom_fraction_from_pointer(pointer_y: Pixels, panel: Bounds<Pixels>) -> Option<f32> {
    let panel_height = panel.bottom() - panel.top();
    if panel_height <= px(0.) {
        return None;
    }
    let bottom_height = panel.bottom() - pointer_y;
    Some((bottom_height / panel_height).clamp(MIN_BOTTOM_FRACTION, MAX_BOTTOM_FRACTION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, point, size};

    #[test]
    fn pointer_fraction_clamps_without_starving_either_pane() {
        let bounds = Bounds::new(point(px(0.), px(100.)), size(px(200.), px(100.)));
        assert_eq!(bottom_fraction_from_pointer(px(100.), bounds), Some(0.8));
        assert_eq!(bottom_fraction_from_pointer(px(150.), bounds), Some(0.5));
        assert_eq!(bottom_fraction_from_pointer(px(200.), bounds), Some(0.2));
    }

    #[test]
    fn zero_height_has_no_fraction() {
        let bounds = Bounds::new(point(px(0.), px(100.)), size(px(200.), px(0.)));
        assert_eq!(bottom_fraction_from_pointer(px(100.), bounds), None);
    }
}
