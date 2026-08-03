//! Transport controls built on `gpui-component` (12.4k★).
//!
//! Uses `gpui_component::Slider` which has:
//! - `SliderEvent::Release` for seek-on-mouse-up (added per issue #2025
//!   explicitly for media players)
//! - `SliderScale::Logarithmic` for volume (docs cite this exact use case)
//! - `reverse()` for "time remaining" display (PR #2541, media-player motivated)
//!
//! Theme is initialized by `ensure_theme_initialized` in `hkask_media_widget`
//! before this module's components are rendered.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
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
    /// Seek slider state (0..duration in seconds).
    seek_slider: Entity<SliderState>,
    /// Volume slider state (0..1, logarithmic).
    volume_slider: Entity<SliderState>,
    /// Whether the seek slider is being dragged (suppress position updates).
    is_dragging_seek: bool,
}

impl TransportBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let seek_slider = cx.new(|_| SliderState::new().min(0.0).max(100.0).step(0.1));
        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.001)
                .max(1.0)
                .step(0.01)
                .scale(SliderScale::Logarithmic)
                .set_value(1.0.into())
        });

        // Subscribe to seek slider events.
        cx.subscribe(&seek_slider, Self::on_seek_slider_event)
            .detach();
        // Subscribe to volume slider events.
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
            SliderEvent::Change(value) => {
                self.is_dragging_seek = true;
                // Live update — just track that we're dragging
            }
            SliderEvent::Release(value) => {
                self.is_dragging_seek = false;
                let position_secs = match value {
                    SliderValue::Single(v) => *v,
                    SliderValue::Range(start, _) => *start,
                };
                cx.emit(TransportEvent::Seek(position_secs));
            }
        }
    }

    fn on_volume_slider_event(
        &mut self,
        _slider: Entity<SliderState>,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        if let SliderEvent::Release(value) | SliderEvent::Change(value) = event {
            let volume = match value {
                SliderValue::Single(v) => *v,
                SliderValue::Range(_, end) => *end,
            };
            cx.emit(TransportEvent::VolumeChange(volume));
        }
    }

    pub fn set_state(&mut self, state: TransportState, cx: &mut Context<Self>) {
        let duration_secs = state.duration.as_secs_f32();
        let position_secs = state.position.as_secs_f32();

        // Update seek slider range and position (unless dragging).
        self.state = state;
        if !self.is_dragging_seek {
            self.seek_slider.update(cx, |slider, cx| {
                slider.set_max(duration_secs.max(0.1));
                slider.set_value(position_secs.into(), cx);
            });
        }

        // Update volume slider.
        self.volume_slider.update(cx, |slider, cx| {
            slider.set_value(state.volume.into(), cx);
        });

        cx.notify();
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            // Play/pause button
            .child(
                div()
                    .id("play-pause")
                    .cursor_pointer()
                    .px_2()
                    .child(SharedString::from(play_label))
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        if let Some(entity) = entity.upgrade() {
                            entity.update(cx, |_, cx| cx.emit(TransportEvent::TogglePlay));
                        }
                    }),
            )
            // Stop button
            .child(
                div()
                    .id("stop")
                    .cursor_pointer()
                    .px_1()
                    .child(SharedString::from("Stop"))
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        if let Some(entity) = entity_stop.upgrade() {
                            entity.update(cx, |_, cx| cx.emit(TransportEvent::Stop));
                        }
                    }),
            )
            // Time display
            .child(div().text_sm().child(time_text))
            // Seek slider — gpui-component Slider with Release event
            .child(
                div()
                    .flex_1()
                    .child(Slider::new(&self.seek_slider).horizontal()),
            )
            // Duration display
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().colors().text_muted)
                    .child(duration_text),
            )
            // Volume slider — logarithmic scale
            .child(
                div()
                    .w(px(80.0))
                    .child(Slider::new(&self.volume_slider).horizontal()),
            )
    }
}
