//! The `MediaWidget` GPUI view — dispatches on `MediaKind` and renders the
//! appropriate media (image via `img()`, SVG via `img()`, audio via `rodio`
//! with transport controls, video via FFmpeg → `RenderImage` → `img()`).

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, ImageSource, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, RenderImage, SharedString, Styled, StyledImage,
    Subscription, Task, Window, div, img, px,
};
use smallvec::SmallVec;
use theme::ActiveTheme;

use crate::audio_player::AudioPlayer;
use crate::media_ref::{MediaKind, MediaRef, MediaStorage, PathMediaStorage, ResolvedMedia};
use crate::transport::{TransportBar, TransportEvent, TransportState};
use crate::video_decoder::VideoPlayer;

use std::sync::Arc;
use std::time::{Duration, Instant};

/// The media widget view. Renders inline in markdown (via the D18 seam)
/// or as a standalone panel item.
pub struct MediaWidget {
    reference: MediaRef,
    storage: Arc<dyn MediaStorage>,
    focus_handle: FocusHandle,
    audio_player: Option<Arc<AudioPlayer>>,
    video_player: Option<VideoPlayer>,
    transport: Option<Entity<TransportBar>>,
    current_frame: Option<Arc<RenderImage>>,
    playback_task: Option<Task<()>>,
    /// Transport state snapshot from the last tick. Used to suppress re-renders
    /// when nothing changed (paused/stopped/finished) so a visible media widget
    /// does not re-render at 30 fps forever. See `tick_playback`.
    last_transport: Option<TransportState>,
    // True while an audio file is being read off the foreground thread
    // (load_audio_file_async). Flows into the transport bar is_loading.
    audio_loading: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

// Stat + read an audio file with the 256 MiB size guard. Pure (no `self`), so it
// is safe to move into a background task; the bytes are handed back to the
// foreground thread where `play_bytes` (rodio device + decode) runs. See SF-1.
fn read_audio_file(path: &std::path::Path) -> Result<Vec<u8>, SharedString> {
    const MAX_AUDIO_FILE_SIZE: u64 = 256 * 1024 * 1024;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Err(SharedString::from(format!(
                "failed to stat audio file: {error}"
            )));
        }
    };
    if metadata.len() > MAX_AUDIO_FILE_SIZE {
        return Err(SharedString::from(format!(
            "audio file too large ({} bytes, max {}); refusing to read",
            metadata.len(),
            MAX_AUDIO_FILE_SIZE
        )));
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) => Err(SharedString::from(format!(
            "failed to read audio file: {error}"
        ))),
    }
}

impl MediaWidget {
    pub fn new(reference: MediaRef, cx: &mut Context<Self>) -> Self {
        Self::with_storage(reference, Arc::new(PathMediaStorage), cx)
    }

    pub fn with_storage(
        reference: MediaRef,
        storage: Arc<dyn MediaStorage>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let mut widget = Self {
            reference,
            storage,
            focus_handle,
            audio_player: None,
            video_player: None,
            transport: None,
            current_frame: None,
            playback_task: None,
            last_transport: None,
            audio_loading: false,
            error: None,
            _subscriptions: Vec::new(),
        };
        widget.initialize(cx);
        widget
    }

    fn initialize(&mut self, cx: &mut Context<Self>) {
        // Sync gpui-component theme colors whenever the Zed GlobalTheme changes,
        // so the transport bar (Slider, Button) stays in sync even when no
        // media block is visible.
        self._subscriptions
            .push(cx.observe_global::<theme::GlobalTheme>(|_this, _cx| {
                // Theme is read directly via cx.theme() in render — no external
                // theme sync needed now that gpui-component is removed.
            }));

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
                let transport = cx.new(TransportBar::new);
                self._subscriptions.push(cx.subscribe(
                    &transport,
                    |this, _transport, event: &TransportEvent, cx| {
                        this.handle_transport_event(event, cx);
                    },
                ));
                self.transport = Some(transport);
            }
            MediaKind::Video => {
                self.video_player = Some(VideoPlayer::new());
                let transport = cx.new(TransportBar::new);
                self._subscriptions.push(cx.subscribe(
                    &transport,
                    |this, _transport, event: &TransportEvent, cx| {
                        this.handle_transport_event(event, cx);
                    },
                ));
                self.transport = Some(transport);
            }
        }
        cx.notify();
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        match self.storage.resolve(&self.reference) {
            Ok(resolved) => self.load_resolved(resolved, cx),
            Err(error) => {
                log::warn!(
                    "hkask-media-widget: media resolution failed: {error}, falling back to direct src"
                );
                self.load_direct(cx);
            }
        }
        cx.notify();
    }

    fn load_resolved(&mut self, resolved: ResolvedMedia, cx: &mut Context<Self>) {
        match resolved.kind {
            MediaKind::Audio => {
                if let Some(bytes) = resolved.bytes {
                    if let Some(player) = &self.audio_player {
                        if let Err(error) = player.play_bytes(bytes) {
                            self.error = Some(SharedString::from(error.to_string()));
                        }
                    }
                } else if let Some(path) = resolved.path {
                    self.load_audio_file_async(path, cx);
                } else if let Some(url) = &resolved.url {
                    if let Some(player) = &self.audio_player {
                        let player = player.clone();
                        self.load_audio_data_uri(&player, url.as_str());
                    }
                }
                self.start_playback_loop(cx);
            }
            MediaKind::Video => {
                if let Some(player) = &mut self.video_player {
                    if let Some(path) = &resolved.path {
                        if let Err(error) = player.open(path) {
                            self.error = Some(SharedString::from(error.to_string()));
                        }
                    } else if let Some(url) = &resolved.url
                        && let Some(path) = url.as_str().strip_prefix("file://")
                        && let Err(error) = player.open(std::path::Path::new(path))
                    {
                        self.error = Some(SharedString::from(error.to_string()));
                    }
                }
                self.start_playback_loop(cx);
            }
            _ => {}
        }
    }

    // Read + stat an audio file off the foreground thread. The blocking I/O
    // (stat + read up to 256 MiB) runs on a background worker; `play_bytes`
    // (rodio device + decode) stays on the foreground thread where the
    // AudioPlayer was constructed. See SF-1 in tasks/widget-interactivity/plan.md.
    fn load_audio_file_async(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.audio_loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { read_audio_file(&path) })
                .await;
            this.update(cx, |widget, cx| {
                widget.audio_loading = false;
                match result {
                    Ok(bytes) => {
                        if let Some(player) = &widget.audio_player {
                            if let Err(error) = player.play_bytes(bytes) {
                                widget.error = Some(SharedString::from(error.to_string()));
                            }
                        }
                    }
                    Err(message) => {
                        widget.error = Some(message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_audio_data_uri(&mut self, player: &Arc<AudioPlayer>, url: &str) {
        if let Some(encoded) = url.strip_prefix("data:audio/")
            && let Some((_, data)) = encoded.split_once(',')
        {
            match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data) {
                Ok(bytes) => {
                    if let Err(error) = player.play_bytes(bytes) {
                        self.error = Some(SharedString::from(error.to_string()));
                    }
                }
                Err(error) => {
                    self.error = Some(SharedString::from(format!("base64 decode failed: {error}")));
                }
            }
        }
    }

    fn load_direct(&mut self, cx: &mut Context<Self>) {
        let src = self.reference.src().to_string();

        match self.reference.kind() {
            Some(MediaKind::Audio) => {
                if let Some(encoded) = src.strip_prefix("data:audio/") {
                    // Data URI — in-memory base64 + decode on the foreground thread.
                    if let Some((_, data)) = encoded.split_once(',') {
                        if let Some(player) = &self.audio_player {
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
                    }
                } else {
                    // Filesystem path — offload the read off the foreground thread.
                    self.load_audio_file_async(std::path::PathBuf::from(&src), cx);
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
    }

    fn start_playback_loop(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        self.playback_task = Some(cx.spawn(async move |_this, cx| {
            let mut last_tick = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;

                let keep_going = match entity.update(cx, |widget, cx| {
                    let keep_going = widget.tick_playback(last_tick.elapsed(), cx);
                    last_tick = Instant::now();
                    keep_going
                }) {
                    Ok(keep_going) => keep_going,
                    Err(_) => break,
                };
                if !keep_going {
                    break;
                }
            }
        }));
    }

    /// Advance playback one tick. Returns `false` when there is no loaded
    /// player to keep alive (both `audio_player` and `video_player` are
    /// `None`); the caller stops the loop in that case.
    ///
    /// To avoid re-rendering every visible media widget at 30 fps forever —
    /// even when paused, stopped, or finished — the transport state is
    /// compared against the last tick: `set_state` and `cx.notify()` fire
    /// only when the state changed or a new video frame was decoded. While
    /// idle the loop stays alive (so pause/resume/seek keep working) but
    /// performs no re-render.
    fn tick_playback(&mut self, delta: Duration, cx: &mut Context<Self>) -> bool {
        let mut transport_state = TransportState {
            is_playing: false,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            volume: 1.0,
            is_loading: self.audio_loading,
        };
        let mut frame_decoded = false;

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
                        frame_decoded = true;
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

        // No loaded player → nothing to play or poll; stop the loop.
        if self.audio_player.is_none() && self.video_player.is_none() {
            return false;
        }

        // Re-render only when something visible changed: a new video frame,
        // or a transport state transition (play/pause/seek/finish/volume).
        let changed = frame_decoded || self.last_transport.as_ref() != Some(&transport_state);
        self.last_transport = Some(transport_state);
        if changed {
            if let Some(transport) = &self.transport {
                transport.update(cx, |transport, cx| {
                    transport.set_state(transport_state, cx);
                });
            }
            cx.notify();
        }

        true
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

impl gpui::Render for MediaWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
