//! Transport controls built on `gpui-component` (12.4k★).
//!
//! Uses `gpui_component::Slider` which has:
//! - `SliderEvent::Release` for seek-on-mouse-up (added per issue #2025
//!   explicitly for media players)
//! - `SliderScale::Logarithmic` for volume (docs cite this exact use case)
//!
//! The slider values are updated in `render()` (where `&mut Window` is
//! available) rather than in `set_state()` (where it isn't).

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, MouseButton, ParentElement, SharedString, Styled, Window, div, px,
};
use gpui_component::slider::{Slider, SliderEvent, SliderScale, SliderState, SliderValue};
use std::time::Duration;
use theme::ActiveTheme;

#[derive(Debug, Clone)]
pub enum TransportEvent {
    TogglePlay,
    Seek(f32),
    VolumeChange(f32),
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct TransportState {
    pub is_playing: bool,
    pub position: Duration,
    pub duration: Duration,
    pub volume: f32,
    pub is_loading: bool,
}

pub struct TransportBar {
    focus_handle: FocusHandle,
    state: TransportState,
    seek_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    is_dragging_seek: bool,
}

impl TransportBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let seek_slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.001));
        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.001)
                .max(1.0)
                .step(0.01)
                .scale(SliderScale::Logarithmic)
        });

        cx.subscribe(&seek_slider, Self::on_seek_slider_event)
            .detach();
        cx.subscribe(&volume_slider, Self::on_volume_slider_event)
            .detach();

        Self {
            focus_handle: cx.focus_handle(),
            state: TransportState {
                is_playing: false,
                position: Duration::ZERO,
                duration: Duration::ZERO,
                volume: 1.0,
                is_loading: false,
            },
            seek_slider,
            volume_slider,
            is_dragging_seek: false,
        }
    }

    fn on_seek_slider_event(
        &mut self,
        _slider: Entity<SliderState>,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SliderEvent::Change(_) => {
                self.is_dragging_seek = true;
            }
            SliderEvent::Release(value) => {
                self.is_dragging_seek = false;
                let fraction = match value {
                    SliderValue::Single(v) => *v,
                    SliderValue::Range(start, _) => *start,
                };
                cx.emit(TransportEvent::Seek(fraction));
            }
        }
    }

    fn on_volume_slider_event(
        &mut self,
        _slider: Entity<SliderState>,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let volume = match event {
            SliderEvent::Change(v) | SliderEvent::Release(v) => match v {
                SliderValue::Single(v) => *v,
                SliderValue::Range(_, end) => *end,
            },
        };
        cx.emit(TransportEvent::VolumeChange(volume));
    }

    pub fn set_state(&mut self, state: TransportState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    fn seek_fraction(&self) -> f32 {
        if self.state.duration.is_zero() {
            0.0
        } else {
            (self.state.position.as_secs_f32() / self.state.duration.as_secs_f32()).clamp(0.0, 1.0)
        }
    }

    fn format_time(duration: Duration) -> SharedString {
        let total_secs = duration.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        if hours > 0 {
            SharedString::from(format!("{hours}:{minutes:02}:{seconds:02}"))
        } else {
            SharedString::from(format!("{minutes}:{seconds:02}"))
        }
    }
}

impl EventEmitter<TransportEvent> for TransportBar {}

impl Focusable for TransportBar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::Render for TransportBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Update slider values before rendering (set_value requires &mut Window).
        if !self.is_dragging_seek {
            self.seek_slider.update(cx, |slider, cx| {
                slider.set_value(SliderValue::Single(self.seek_fraction()), window, cx);
            });
        }
        self.volume_slider.update(cx, |slider, cx| {
            slider.set_value(SliderValue::Single(self.state.volume), window, cx);
        });

        let play_label = if self.state.is_playing {
            "Pause"
        } else {
            "Play"
        };
        let time_text = Self::format_time(self.state.position);
        let duration_text = Self::format_time(self.state.duration);
        let entity = cx.entity().downgrade();
        let entity_stop = entity.clone();

        div()
            .flex()
            .flex_row()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .child(
                div()
                    .id("play-pause")
                    .cursor_pointer()
                    .px_2()
                    .child(SharedString::from(play_label))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(entity) = entity.upgrade() {
                            entity.update(cx, |_, cx| cx.emit(TransportEvent::TogglePlay));
                        }
                    }),
            )
            .child(
                div()
                    .id("stop")
                    .cursor_pointer()
                    .px_1()
                    .child(SharedString::from("Stop"))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(entity) = entity_stop.upgrade() {
                            entity.update(cx, |_, cx| cx.emit(TransportEvent::Stop));
                        }
                    }),
            )
            .child(div().text_sm().child(time_text))
            .child(
                div()
                    .flex_1()
                    .child(Slider::new(&self.seek_slider).horizontal()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().colors().text_muted)
                    .child(duration_text),
            )
            .child(
                div()
                    .w(px(80.0))
                    .child(Slider::new(&self.volume_slider).horizontal()),
            )
    }
}
