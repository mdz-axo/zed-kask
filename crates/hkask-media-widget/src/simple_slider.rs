//! Lightweight GPUI-native slider — replaces the 618-dependency
//! `gpui-component` crate for transport controls.
//!
//! Uses GPUI's drag system (`on_drag` / `on_drag_move`) which provides precise
//! element bounds during drag. Emits `Change` while dragging and `Release` on
//! mouse-up, including mouse-up outside the narrow track.

use gpui::{
    App, AppContext, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
    div, px, relative,
};
use theme::ActiveTheme;

// ── Drag types ─────────────────────────────────────────────────────────────

/// Drag value — carried by the GPUI drag system to link on_drag → on_drag_move → on_drop.
#[derive(Clone)]
struct SliderDrag;

/// Invisible drag ghost — GPUI requires a Render entity for the drag visual.
/// Renders nothing (empty div).
struct SliderDragGhost;

impl Render for SliderDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

// ── Events ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SimpleSliderEvent {
    Change(f32),
    Release(f32),
}

// ── Slider ─────────────────────────────────────────────────────────────────

pub struct SimpleSlider {
    focus_handle: FocusHandle,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
}

impl SimpleSlider {
    pub fn new(cx: &mut Context<Self>, min: f32, max: f32, step: f32) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: min,
            min,
            max,
            step,
        }
    }

    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = value.clamp(self.min, self.max);
        cx.notify();
    }

    fn fraction_from_value(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            return 0.0;
        }
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    fn value_from_fraction(&self, fraction: f32) -> f32 {
        let fraction = fraction.clamp(0.0, 1.0);
        let raw = self.min + fraction * (self.max - self.min);
        let stepped = (raw / self.step).round() * self.step;
        stepped.clamp(self.min, self.max)
    }
}

impl EventEmitter<SimpleSliderEvent> for SimpleSlider {}

impl Focusable for SimpleSlider {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SimpleSlider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fraction = self.fraction_from_value();
        let theme = cx.theme().clone();
        let track_color = theme.colors().scrollbar_track_background;
        let fill_color = theme.colors().text_accent;
        let thumb_color = theme.colors().scrollbar_thumb_background;
        let entity = cx.entity().downgrade();
        let entity_release = entity.clone();
        let entity_release_out = entity.clone();

        div()
            .id("simple-slider-track")
            .flex_1()
            .h(px(6.0))
            .rounded(px(3.0))
            .bg(track_color)
            .cursor_pointer()
            .relative()
            .on_drag(SliderDrag, |_, _, _, cx| cx.new(|_| SliderDragGhost))
            .on_drag_move::<SliderDrag>(move |event, _window, cx| {
                let Some(entity) = entity.upgrade() else {
                    return;
                };
                entity.update(cx, |slider, cx| {
                    let track_left: f32 = event.bounds.left().into();
                    let track_width: f32 = event.bounds.size.width.into();
                    if track_width < 1.0 {
                        return;
                    }
                    let click_x: f32 = event.event.position.x.into();
                    let fraction = ((click_x - track_left) / track_width).clamp(0.0, 1.0);
                    let value = slider.value_from_fraction(fraction);
                    slider.value = value;
                    cx.emit(SimpleSliderEvent::Change(value));
                    cx.notify();
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _window, cx| {
                let Some(entity) = entity_release.upgrade() else {
                    return;
                };
                entity.update(cx, |slider, cx| {
                    cx.emit(SimpleSliderEvent::Release(slider.value));
                    cx.notify();
                });
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _window, cx| {
                let Some(entity) = entity_release_out.upgrade() else {
                    return;
                };
                entity.update(cx, |slider, cx| {
                    cx.emit(SimpleSliderEvent::Release(slider.value));
                    cx.notify();
                });
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(relative(fraction))
                    .min_w(px(4.0))
                    .rounded(px(3.0))
                    .bg(fill_color),
            )
            .child(
                div()
                    .absolute()
                    .top(px(-3.0))
                    .left(relative(fraction))
                    .ml(px(-6.0))
                    .size(px(12.0))
                    .rounded(px(6.0))
                    .bg(thumb_color)
                    .border_1()
                    .border_color(fill_color),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, point, size};
    use std::sync::{Arc, Mutex};

    struct SliderHost {
        slider: gpui::Entity<SimpleSlider>,
    }

    impl Render for SliderHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .w_full()
                .h(px(40.0))
                .items_center()
                .child(self.slider.clone())
        }
    }

    /// expect: Releasing a seek drag outside the narrow track still commits the seek.
    /// [P1] Motivating: users can scrub without keeping the pointer inside a six-pixel target.
    /// pre: a left-button drag starts on the track, changes value, and ends below the track.
    /// post: the slider emits Change followed by exactly one Release.
    #[gpui::test]
    fn drag_release_outside_track_emits_release(cx: &mut TestAppContext) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let (_, cx) = cx.add_window_view(|_window, cx| {
            let slider = cx.new(|cx| SimpleSlider::new(cx, 0.0, 1.0, 0.001));
            cx.subscribe(&slider, move |_, _, event, _| {
                observed
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(event.clone());
            })
            .detach();
            SliderHost { slider }
        });
        cx.simulate_resize(size(px(300.0), px(80.0)));
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("simple-slider-track")
            .expect("slider track has laid-out bounds");
        let start = point(bounds.left() + px(5.0), bounds.center().y);
        let inside = point(bounds.right() - px(5.0), bounds.center().y);
        let outside = point(inside.x, bounds.bottom() + px(20.0));
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(inside, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();

        let events = events.lock().unwrap_or_else(|error| error.into_inner());
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SimpleSliderEvent::Change(_))),
            "drag emits a value change"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, SimpleSliderEvent::Release(_)))
                .count(),
            1,
            "mouse-up outside commits exactly one release"
        );
    }
}
