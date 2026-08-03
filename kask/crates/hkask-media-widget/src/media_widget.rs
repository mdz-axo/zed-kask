//! The `MediaWidget` GPUI view — dispatches on `MediaKind` and renders the
//! appropriate media (image via `img()`, SVG via `img()`, audio via `rodio`
//! with transport controls, video via FFmpeg → `RenderImage` → `img()`).

use gpui::{
    AnyElement, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    ImageSource, InteractiveElement, IntoElement, ObjectFit, ParentElement, RenderImage,
    SharedString, Styled, StyledImage, Task, Window, div, img, px,
};
use smallvec::SmallVec;
use theme::ActiveTheme;

use crate::audio_player::AudioPlayer;
use crate::media_ref::{MediaKind, MediaRef};
use crate::transport::{TransportBar, TransportEvent, TransportState};
use crate::video_decoder::VideoPlayer;

use std::sync::Arc;
use std::time::{Duration, Instant};

/// The media widget view. Renders inline in markdown (via the D18 seam)
/// or as a standalone panel item.
pub struct MediaWidget {
    reference: MediaRef,
    focus_handle: FocusHandle,
    audio_player: Option<Arc<AudioPlayer>>,
    video_player: Option<VideoPlayer>,
    transport: Option<Entity<TransportBar>>,
    current_frame: Option<Arc<RenderImage>>,
    playback_task: Option<Task<()>>,
    error: Option<SharedString>,
}

impl MediaWidget {
    pub fn new(reference: MediaRef, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let mut widget = Self {
            reference,
            focus_handle,
            audio_player: None,
            video_player: None,
            transport: None,
            current_frame: None,
            playback_task: None,
            error: None,
        };
        widget.initialize(cx);
        widget
    }

    fn initialize(&mut self, cx: &mut Context<Self>) {
        let kind = match &self.reference {
            MediaRef::Asset { kind, .. } => *kind,
            MediaRef::Error(message) => {
                self.error = Some(message.clone());
                return;
            }
        };

        match kind {
            MediaKind::Image | MediaKind::Svg => {}
            MediaKind::Audio => {
                let player = Arc::new(AudioPlayer::new());
                self.audio_player = Some(player);
                self.transport = Some(cx.new(TransportBar::new));
            }
            MediaKind::Video => {
                self.video_player = Some(VideoPlayer::new());
                self.transport = Some(cx.new(TransportBar::new));
            }
        }
        cx.notify();
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        let src = self.reference.src().to_string();

        match self.reference.kind() {
            Some(MediaKind::Audio) => {
                if let Some(player) = &self.audio_player {
                    if let Some(encoded) = src.strip_prefix("data:audio/") {
                        if let Some((_, data)) = encoded.split_once(',') {
                            match base64::Engine::decode(
                                &base64::engine::general_purpose::STANDARD,
                                data,
                            ) {
                                Ok(bytes) => {
                                    if let Err(error) = player.play_bytes(bytes) {
                                        self.error = Some(SharedString::from(error.to_string()));
                                    }
                                }
                                Err(error) => {
                                    self.error = Some(SharedString::from(format!(
                                        "base64 decode failed: {error}"
                                    )));
                                }
                            }
                        }
                    } else {
                        match std::fs::read(&src) {
                            Ok(bytes) => {
                                if let Err(error) = player.play_bytes(bytes) {
                                    self.error = Some(SharedString::from(error.to_string()));
                                }
                            }
                            Err(error) => {
                                self.error = Some(SharedString::from(format!(
                                    "failed to read audio file: {error}"
                                )));
                            }
                        }
                    }
                }
                self.start_playback_loop(cx);
            }
            Some(MediaKind::Video) => {
                if let Some(player) = &mut self.video_player {
                    let path = std::path::PathBuf::from(&src);
                    if let Err(error) = player.open(&path) {
                        self.error = Some(SharedString::from(error.to_string()));
                    }
                }
                self.start_playback_loop(cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn start_playback_loop(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        self.playback_task = Some(cx.spawn(async move |_this, cx| {
            let mut last_tick = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;

                let Ok(()) = entity.update(cx, |widget, cx| {
                    widget.tick_playback(last_tick.elapsed(), cx);
                    last_tick = Instant::now();
                }) else {
                    break;
                };
            }
        }));
    }

    fn tick_playback(&mut self, delta: Duration, cx: &mut Context<Self>) {
        let mut transport_state = TransportState {
            is_playing: false,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            volume: 1.0,
            is_loading: false,
        };

        if let Some(player) = &self.audio_player {
            transport_state.is_playing = player.is_playing();
            transport_state.position = player.position();
            transport_state.duration = player.duration();
            transport_state.volume = player.volume();
        }

        if let Some(player) = &mut self.video_player {
            if player.is_playing() {
                match player.advance_and_decode(delta) {
                    Ok(Some(frame)) => {
                        let buffer =
                            image::ImageBuffer::from_raw(frame.width, frame.height, frame.rgba)
                                .unwrap_or_else(|| {
                                    image::ImageBuffer::new(frame.width, frame.height)
                                });
                        let image_frame = image::Frame::new(buffer);
                        let render_image =
                            Arc::new(RenderImage::new(SmallVec::from_elem(image_frame, 1)));
                        self.current_frame = Some(render_image);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        log::warn!("hkask-media-widget: video decode error: {error}");
                    }
                }
            }
            transport_state.is_playing = player.is_playing();
            transport_state.position = player.position();
            transport_state.duration = player.duration();
            transport_state.volume = player.volume();
        }

        if let Some(transport) = &self.transport {
            transport.update(cx, |transport, cx| {
                transport.set_state(transport_state, cx);
            });
        }

        cx.notify();
    }

    fn handle_transport_event(&mut self, event: &TransportEvent, cx: &mut Context<Self>) {
        match event {
            TransportEvent::TogglePlay => {
                if let Some(player) = &self.audio_player {
                    player.toggle();
                }
                if let Some(player) = &mut self.video_player {
                    if player.is_playing() {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
            }
            TransportEvent::Seek(fraction) => {
                if let Some(player) = &self.audio_player {
                    let duration = player.duration();
                    player.seek(Duration::from_secs_f32(duration.as_secs_f32() * fraction));
                }
                if let Some(player) = &mut self.video_player {
                    let duration = player.duration();
                    player.seek(Duration::from_secs_f32(duration.as_secs_f32() * fraction));
                }
            }
            TransportEvent::VolumeChange(volume) => {
                if let Some(player) = &self.audio_player {
                    player.set_volume(*volume);
                }
                if let Some(player) = &mut self.video_player {
                    player.set_volume(*volume);
                }
            }
            TransportEvent::Stop => {
                if let Some(player) = &self.audio_player {
                    player.stop();
                }
                if let Some(player) = &mut self.video_player {
                    player.stop();
                }
            }
        }
        cx.notify();
    }
}

impl Focusable for MediaWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<TransportEvent> for MediaWidget {}

impl gpui::Render for MediaWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(transport) = &self.transport {
            cx.subscribe(transport, |this, _transport, event: &TransportEvent, cx| {
                this.handle_transport_event(event, cx);
            })
            .detach();
        }

        let theme = cx.theme();

        let main_content = match &self.reference {
            MediaRef::Error(message) => div()
                .p_4()
                .text_sm()
                .text_color(theme.colors().text_muted)
                .child(SharedString::from(format!("Media error: {message}")))
                .into_any_element(),

            MediaRef::Asset { src, kind } => match kind {
                MediaKind::Image | MediaKind::Svg => {
                    let source: ImageSource = src.as_str().into();
                    div()
                        .size_full()
                        .min_h(px(100.0))
                        .child(img(source).size_full().object_fit(ObjectFit::Contain))
                        .into_any_element()
                }
                MediaKind::Audio => {
                    let transport = self.transport.clone();
                    let mut container = div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .border_1()
                        .border_color(theme.colors().border)
                        .rounded_md()
                        .child(
                            div()
                                .text_sm()
                                .child(SharedString::from(format!("Audio: {src}"))),
                        );
                    if let Some(transport) = transport {
                        container = container.child(transport);
                    }
                    container.into_any_element()
                }
                MediaKind::Video => {
                    let transport = self.transport.clone();
                    let frame = self.current_frame.clone();
                    let mut container = div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .border_1()
                        .border_color(theme.colors().border)
                        .rounded_md()
                        .overflow_hidden();

                    let mut video_area = div()
                        .flex_1()
                        .min_h(px(120.0))
                        .bg(theme.colors().editor_background);

                    if let Some(frame) = frame {
                        video_area = video_area.child(
                            img(ImageSource::Render(frame))
                                .size_full()
                                .object_fit(ObjectFit::Contain),
                        );
                    } else {
                        video_area = video_area
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(SharedString::from("Video"));
                    }

                    container = container.child(video_area);
                    if let Some(transport) = transport {
                        container = container.child(transport);
                    }
                    container.into_any_element()
                }
            },
        };

        div()
            .id("media-widget")
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h(px(80.0))
            .child(main_content)
    }
}

/// Render a `MediaRef` as a GPUI `AnyElement` — the entry point called from the
/// D18 seam (`media_block_renderer`).
pub fn render_media_ref(reference: MediaRef, cx: &mut App) -> AnyElement {
    let entity = cx.new(|cx| {
        let mut widget = MediaWidget::new(reference, cx);
        widget.load(cx);
        widget
    });
    div().size_full().child(entity).into_any_element()
}
