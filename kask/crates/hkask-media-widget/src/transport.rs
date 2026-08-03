//! Transport controls (play/pause/seek/volume) built from GPUI primitives.
//!
//! These are the minimum viable controls. When `gpui-component` is wired as
//! a workspace dependency, these can be replaced with `gpui_component::Slider`
//! (which has `SliderEvent::Release` for seek-on-mouse-up and
//! `SliderScale::Logarithmic` for volume) and `gpui_component::Button`.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px,
};
use std::time::Duration;

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
}

impl TransportBar {
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
        }
    }

    pub fn set_state(&mut self, state: TransportState, cx: &mut Context<Self>) {
        self.state = state;
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
        let play_label = if self.state.is_playing {
            "Pause"
        } else {
            "Play"
        };
        let time_text = Self::format_time(self.state.position);
        let duration_text = Self::format_time(self.state.duration);
        let seek_fraction = self.seek_fraction();
        let volume = self.state.volume;
        let entity = cx.entity().downgrade();

        let theme = cx.theme();

        div()
            .h_flex()
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
                        let _ = entity.upgrade();
                        cx.dispatch_event(TransportEvent::TogglePlay);
                    }),
            )
            .child(
                div()
                    .id("stop")
                    .cursor_pointer()
                    .px_1()
                    .child(SharedString::from("Stop"))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.dispatch_event(TransportEvent::Stop);
                    }),
            )
            .child(div().text_sm().child(time_text))
            .child(
                div()
                    .id("seek-bar")
                    .flex_1()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(theme.colors().border)
                    .relative()
                    .cursor_pointer()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .rounded_full()
                            .bg(theme.colors().text_accent)
                            .w(px(seek_fraction * 300.0)),
                    )
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx| {
                        cx.dispatch_event(TransportEvent::Seek(0.5));
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors().text_muted)
                    .child(duration_text),
            )
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .items_center()
                    .child(SharedString::from("Vol"))
                    .child(
                        div()
                            .w(px(32.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.colors().border)
                            .relative()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .h_full()
                                    .rounded_full()
                                    .bg(theme.colors().text_accent)
                                    .w(px(volume * 32.0)),
                            ),
                    ),
            )
    }
}
