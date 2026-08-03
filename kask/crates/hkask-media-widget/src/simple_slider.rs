//! Lightweight GPUI-native slider — replaces the 618-dependency
//! `gpui-component` crate for transport controls.
//!
//! Uses GPUI's drag system (`on_drag` / `on_drag_move` / `on_drop`) which
//! provides precise element bounds during drag — no manual bounds tracking.
//! Supports linear and logarithmic scales. Emits `Change` while dragging
//! and `Release` on drop (seek-on-release semantics for media players).

use gpui::{
    App, AppContext, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px,
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
    logarithmic: bool,
}

impl SimpleSlider {
    pub fn new(cx: &mut Context<Self>, min: f32, max: f32, step: f32) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: min,
            min,
            max,
            step,
            logarithmic: false,
        }
    }

    pub fn logarithmic(mut self) -> Self {
        self.logarithmic = true;
        self
    }

    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = value.clamp(self.min, self.max);
        cx.notify();
    }

    fn fraction_from_value(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            return 0.0;
        }
        if self.logarithmic && self.min > 0.0 && self.max > 0.0 {
            let log_min = self.min.ln();
            let log_max = self.max.ln();
            ((self.value.ln() - log_min) / (log_max - log_min)).clamp(0.0, 1.0)
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    fn value_from_fraction(&self, fraction: f32) -> f32 {
        let fraction = fraction.clamp(0.0, 1.0);
        let raw = if self.logarithmic && self.min > 0.0 && self.max > 0.0 {
            let log_min = self.min.ln();
            let log_max = self.max.ln();
            (log_min + fraction * (log_max - log_min)).exp()
        } else {
            self.min + fraction * (self.max - self.min)
        };
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
        let entity_drop = entity.clone();

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
            .on_drop::<SliderDrag>(move |drag, _window, cx| {
                let Some(entity) = entity_drop.upgrade() else {
                    return;
                };
                entity.update(cx, |slider, cx| {
                    cx.emit(SimpleSliderEvent::Release(slider.value));
                    cx.notify();
                });
                let _ = drag;
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(px(fraction * 100.0))
                    .min_w(px(4.0))
                    .rounded(px(3.0))
                    .bg(fill_color),
            )
            .child(
                div()
                    .absolute()
                    .top(px(-3.0))
                    .left(px(fraction * 100.0))
                    .ml(px(-6.0))
                    .size(px(12.0))
                    .rounded(px(6.0))
                    .bg(thumb_color)
                    .border_1()
                    .border_color(fill_color),
            )
    }
}
