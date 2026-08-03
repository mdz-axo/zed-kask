//! Transport controls (play/pause/seek/volume) built from GPUI primitives.
//!
//! These are the minimum viable controls. When `gpui-component` is wired as
//! a workspace dependency (via the `[patch]` strategy), these can be replaced
//! with `gpui_component::button::Button`, `gpui_component::slider::Slider`,
//! and `gpui_component::progress::Progress` — all of which are production-
//! quality, themed, and media-player-aware (Slider has `Release` events and
//! `reverse()` built for media players).
//!
//! The API surface here mirrors what those components provide so the swap
//! is a localized change in this file.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseUpEvent, SharedString, StatefulInteractiveElement, Styled,
    Window, cx, div, px,
};

use std::time::Duration;

/// Transport control events emitted by the transport bar.
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// Play/pause toggle requested.
    TogglePlay,
    /// Seek to a position (fraction 0.0–1.0 of duration).
    Seek(f32),
    /// Volume changed (0.0–1.0).
    VolumeChange(f32),
    /// Stop requested.
    Stop,
}

/// State of the transport bar.
#[derive(Debug, Clone, Copy)]
pub struct TransportState {
    pub is_playing: bool,
    pub position: Duration,
    pub duration: Duration,
    pub volume: f32,
    pub is_loading: bool,
}

/// A transport bar widget — play/pause button, seek bar, time display, volume.
///
/// Built from raw GPUI primitives. When `gpui-component` is available, this
/// can be reimplemented using `gpui_component::Slider` (which has
/// `SliderEvent::Release` for seek-on-mouse-up and `SliderScale::Logarithmic`
/// for volume) and `gpui_component::Button` for play/pause.
pub struct TransportBar {
    focus_handle: FocusHandle,
    state: TransportState,
    is_dragging_seek: bool,
}

impl TransportBar {
    /// Create a new transport bar.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            state: TransportState {
                is_playing: false,
                position: Duration::ZERO,
                duration: Duration::ZERO,
                volume: 1.0,
                is_loading: false,
            },
            is_dragging_seek: false,
        }
    }

    /// Update the transport state (called by the owning MediaWidget each frame).
    pub fn set_state(&mut self, state: TransportState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    /// Format a duration as M:SS or H:MM:SS.
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

    /// The seek fraction (0.0–1.0), or 0 if no duration.
    fn seek_fraction(&self) -> f32 {
        if self.state.duration.is_zero() {
            0.0
        } else {
            self.state.position.as_secs_f32() / self.state.duration.as_secs_f32()
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
        let play_label = if self.state.is_playing { "⏸" } else { "▶" };
        let time_text = Self::format_time(self.state.position);
        let duration_text = Self::format_time(self.state.duration);
        let seek_fraction = self.seek_fraction();
        let volume = self.state.volume;
        let view = cx.entity().clone();

        div()
            .h_flex()
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
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.dispatch_event(TransportEvent::TogglePlay);
                        let _ = view;
                    }),
            )
            // Stop button
            .child(
                div()
                    .id("stop")
                    .cursor_pointer()
                    .px_1()
                    .child(SharedString::from("⏹"))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.dispatch_event(TransportEvent::Stop);
                    }),
            )
            // Time display
            .child(div().text_sm().child(time_text))
            // Seek bar (clickable progress bar)
            .child(
                div()
                    .id("seek-bar")
                    .flex_1()
                    .h_1()
                    .rounded_full()
                    .bg(cx.theme().colors().border)
                    .relative()
                    .cursor_pointer()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .rounded_full()
                            .bg(cx.theme().colors().text_accent)
                            .w(seek_fraction * 100.0),
                    )
                    .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                        // Seek bar click — calculate fraction from click position
                        // (The actual bounds calculation needs the element bounds,
                        // which we get from the canvas callback in production.)
                        let _ = event;
                        cx.dispatch_event(TransportEvent::Seek(0.5)); // placeholder
                    }),
            )
            // Duration display
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().colors().text_muted)
                    .child(duration_text),
            )
            // Volume control
            .child(
                div()
                    .id("volume")
                    .h_flex()
                    .gap_1()
                    .items_center()
                    .child(SharedString::from("🔊"))
                    .child(
                        div()
                            .w_8()
                            .h_1()
                            .rounded_full()
                            .bg(cx.theme().colors().border)
                            .relative()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .h_full()
                                    .rounded_full()
                                    .bg(cx.theme().colors().text_accent)
                                    .w(volume * 100.0),
                            ),
                    ),
            )
            .when(self.state.is_loading, |this| {
                this.child(SharedString::from("⟳"))
            })
    }
}
