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

/// The playback-rate steps the transport cycles through (0.5×–2× — the
/// Reduct/Descript reference set, plan S4).
pub(crate) const SPEED_STEPS: [f32; 5] = [0.5, 1.0, 1.25, 1.5, 2.0];

/// The next playback-rate step after `current`, wrapping 2× back to 0.5×.
/// An unrecognized rate restarts at the cycle's first step rather than
/// guessing a successor.
pub(crate) fn next_speed(current: f32) -> f32 {
    SPEED_STEPS
        .iter()
        .find(|step| **step > current)
        .copied()
        .unwrap_or(SPEED_STEPS[0])
}

/// Media time advanced by a wall-clock interval at the given playback rate:
/// at speed `s`, media time advances `s`× per wall second (2× → media runs at
/// twice wall time).
pub(crate) fn media_elapsed(wall_elapsed: Duration, speed: f32) -> Duration {
    wall_elapsed.mul_f32(speed)
}

/// The wall-clock time until a media interval comes due at the given playback
/// rate — frame deadlines scale inversely with the rate (1.5× → 2/3 the
/// interval). Also the wall-equivalent position rodio's `try_seek` expects
/// under a `Speed` wrapper (media / rate).
pub(crate) fn wall_deadline(media_interval: Duration, speed: f32) -> Duration {
    media_interval.div_f32(speed)
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    TogglePlay,
    Seek(f32),
    Stop,
    CycleSpeed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportState {
    pub is_playing: bool,
    pub position: Duration,
    pub duration: Duration,
    pub is_loading: bool,
    /// Active playback rate (a `SPEED_STEPS` value) — the speed button's
    /// label source.
    pub speed: f32,
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
                speed: 1.0,
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

/// The speed button's label: whole rates drop the fraction ("1×", "2×"),
/// fractional steps keep it ("0.5×", "1.25×", "1.5×").
fn speed_label(speed: f32) -> SharedString {
    if speed.fract() == 0.0 {
        SharedString::from(format!("{}×", speed as i32))
    } else {
        SharedString::from(format!("{speed}×"))
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
        let speed_text = speed_label(self.state.speed);
        let entity = cx.entity().downgrade();
        let entity_stop = entity.clone();
        let entity_speed = entity.clone();

        div()
            // Test-support hook: exposes the transport's laid-out bounds to
            // layout ground-truth tests (noop in release builds) — the row
            // whose visibility the short-pane pins assert.
            .debug_selector(|| "media-transport".into())
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
            .child(
                div()
                    .id("speed")
                    .cursor_pointer()
                    .px_2()
                    .text_sm()
                    .child(speed_text)
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(entity) = entity_speed.upgrade() {
                            entity.update(cx, |_, cx| cx.emit(TransportEvent::CycleSpeed));
                        }
                    }),
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
            speed: 1.0,
        };
        assert_eq!(play_label(&state), "Loading…");
    }

    /// dcterms:identifier: `wall_deadline` / `media_elapsed`
    /// expect: Frame deadlines scale inversely with playback rate — at 1.5× a
    /// media interval is due in 2/3 of the wall time — and media time
    /// advances by the same factor per wall second.
    /// [P1] Motivating: speed playback must keep the pacing math exact; the
    /// transport's speed control is only honest if the clock actually scales.
    /// pre: a positive rate from `SPEED_STEPS`.
    /// post: `wall_deadline(1500ms, 1.5) == 1000ms` and
    /// `media_elapsed(1s, 1.5) == 1500ms`; 1× is the exact identity.
    #[test]
    fn frame_deadlines_scale_with_rate() {
        assert_eq!(
            wall_deadline(Duration::from_millis(1500), 1.5),
            Duration::from_secs(1),
            "1.5×: a 1.5s media interval is due in 2/3 the wall time"
        );
        assert_eq!(
            media_elapsed(Duration::from_secs(1), 1.5),
            Duration::from_millis(1500)
        );
        assert_eq!(
            media_elapsed(Duration::from_secs(2), 2.0),
            Duration::from_secs(4)
        );
        assert_eq!(
            wall_deadline(Duration::from_secs(4), 2.0),
            Duration::from_secs(2)
        );
        assert_eq!(
            wall_deadline(Duration::from_secs(1), 0.5),
            Duration::from_secs(2)
        );
        // 1× is the bit-exact identity (mul_f32/div_f32 by 1.0).
        assert_eq!(
            media_elapsed(Duration::from_secs(3), 1.0),
            Duration::from_secs(3)
        );
        assert_eq!(
            wall_deadline(Duration::from_secs(3), 1.0),
            Duration::from_secs(3)
        );
    }

    /// dcterms:identifier: `next_speed` / `SPEED_STEPS`
    /// expect: The speed control cycles 0.5× → 1× → 1.25× → 1.5× → 2× and
    /// wraps 2× back to 0.5×.
    /// [P1] Motivating: the cycle is the speed affordance's whole UI contract.
    /// pre: any rate the transport can report.
    /// post: each step's successor is the next entry, wrapping at the end.
    #[test]
    fn speed_cycle_wraps_through_the_steps() {
        assert_eq!(SPEED_STEPS, [0.5, 1.0, 1.25, 1.5, 2.0]);
        assert_eq!(next_speed(0.5), 1.0);
        assert_eq!(next_speed(1.0), 1.25);
        assert_eq!(next_speed(1.25), 1.5);
        assert_eq!(next_speed(1.5), 2.0);
        assert_eq!(next_speed(2.0), 0.5);
        // An unrecognized rate falls back to the cycle start, not a panic.
        assert_eq!(next_speed(3.0), 0.5);
    }

    /// dcterms:identifier: `speed_label`
    /// expect: Each cycle step renders a distinct compact label.
    /// [P1] Motivating: the operator must see which rate is active.
    /// pre: a rate from `SPEED_STEPS`.
    /// post: whole rates drop the fraction ("1×", "2×"), others keep it
    /// ("0.5×", "1.25×", "1.5×").
    #[test]
    fn speed_label_renders_each_step() {
        assert_eq!(speed_label(0.5).as_ref(), "0.5×");
        assert_eq!(speed_label(1.0).as_ref(), "1×");
        assert_eq!(speed_label(1.25).as_ref(), "1.25×");
        assert_eq!(speed_label(1.5).as_ref(), "1.5×");
        assert_eq!(speed_label(2.0).as_ref(), "2×");
    }
}
