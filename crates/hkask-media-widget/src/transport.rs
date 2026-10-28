//! Transport controls using a lightweight GPUI-native slider.
//!
//! Replaces the 618-dependency `gpui-component` crate with a simple
//! inline slider (`SimpleSlider`). Provides seek-on-release semantics while
//! leaving volume and mute exclusively to the operating system.

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, MouseButton, ParentElement, SharedString, Styled, Window, div,
};
use std::time::Duration;
use theme::ActiveTheme;

use crate::simple_slider::{SimpleSlider, SimpleSliderEvent};

#[derive(Debug, Clone)]
pub enum TransportEvent {
    TogglePlay,
    Seek(f32),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportState {
    pub is_playing: bool,
    pub position: Duration,
    pub duration: Duration,
    pub is_loading: bool,
}

pub struct TransportBar {
    focus_handle: FocusHandle,
    state: TransportState,
    seek_slider: Entity<SimpleSlider>,
    is_dragging_seek: bool,
}

impl TransportBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let seek_slider = cx.new(|cx| SimpleSlider::new(cx, 0.0, 1.0, 0.001));

        cx.subscribe(&seek_slider, Self::on_seek_slider_event)
            .detach();

        Self {
            focus_handle: cx.focus_handle(),
            state: TransportState {
                is_playing: false,
                position: Duration::ZERO,
                duration: Duration::ZERO,
                is_loading: false,
            },
            seek_slider,
            is_dragging_seek: false,
        }
    }

    fn on_seek_slider_event(
        &mut self,
        _slider: Entity<SimpleSlider>,
        event: &SimpleSliderEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SimpleSliderEvent::Change => {
                self.is_dragging_seek = true;
            }
            SimpleSliderEvent::Release(value) => {
                self.is_dragging_seek = false;
                cx.emit(TransportEvent::Seek(*value));
            }
        }
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

    fn format_seconds(total_secs: u64) -> SharedString {
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        if hours > 0 {
            SharedString::from(format!("{hours}:{minutes:02}:{seconds:02}"))
        } else {
            SharedString::from(format!("{minutes}:{seconds:02}"))
        }
    }

    fn format_position(position: Duration, duration: Duration) -> SharedString {
        let seconds = if !duration.is_zero() && position >= duration {
            duration
                .as_secs()
                .saturating_add(u64::from(duration.subsec_nanos() > 0))
        } else {
            position.as_secs()
        };
        Self::format_seconds(seconds)
    }

    fn format_duration(duration: Duration) -> SharedString {
        let seconds = duration
            .as_secs()
            .saturating_add(u64::from(duration.subsec_nanos() > 0));
        Self::format_seconds(seconds)
    }
}

impl EventEmitter<TransportEvent> for TransportBar {}

impl Focusable for TransportBar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn play_label(state: &TransportState) -> &'static str {
    if state.is_loading {
        "Loading…"
    } else if state.is_playing {
        "Pause"
    } else {
        "Play"
    }
}

impl gpui::Render for TransportBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_dragging_seek {
            self.seek_slider
                .update(cx, |slider, cx| slider.set_value(self.seek_fraction(), cx));
        }

        let play_label = play_label(&self.state);
        let time_text = Self::format_position(self.state.position, self.state.duration);
        let duration_text = Self::format_duration(self.state.duration);
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
                            entity.update(cx, |transport, cx| {
                                if !transport.state.is_loading {
                                    cx.emit(TransportEvent::TogglePlay);
                                }
                            });
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
            .child(div().flex_1().child(self.seek_slider.clone()))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().colors().text_muted)
                    .child(duration_text),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: Loading media is visibly distinct from a ready Play control.
    /// [P1] Motivating: users can distinguish slow loading from an idle player.
    /// pre: transport state reports loading.
    /// post: the control label reports Loading rather than Play or Pause.
    #[test]
    fn subsecond_duration_is_visible_and_terminal_position_converges() {
        let duration = Duration::from_millis(600);
        assert_eq!(TransportBar::format_duration(duration).as_ref(), "0:01");
        assert_eq!(
            TransportBar::format_position(Duration::from_millis(300), duration).as_ref(),
            "0:00"
        );
        assert_eq!(
            TransportBar::format_position(duration, duration).as_ref(),
            "0:01"
        );
    }

    #[test]
    fn loading_state_has_a_visible_transport_label() {
        let state = TransportState {
            is_playing: false,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            is_loading: true,
        };
        assert_eq!(play_label(&state), "Loading…");
    }
}
