//! The `MediaWidget` GPUI view — dispatches on `MediaKind` and renders the
//! appropriate media (image via `img()`, SVG via `img()`, audio via `rodio`
//! with transport controls, video via FFmpeg → `RenderImage` → `img()`).
//!
//! When the parsed block carries an OMC concept tag + provenance (the
//! `media_block::enrich_with_omc_and_provenance` path), the widget also renders
//! the **Explain** affordance: dispatches the OMC-aware explain tool
//! (`describe_image` or `gallery_analyze`) via `shared_tool_invoker()`. The OMC
//! concept drives the tool selection — ontology-bounded affordances.

use futures::AsyncReadExt as _;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, ImageSource, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, RenderImage, SharedString, Styled, StyledImage,
    Subscription, Task, Window, div, img,
};
use gpui_util::ResultExt as _;
use hkask_bridge_ontology::omc::explain_tool_for;
use hkask_tool_invoker::shared_tool_invoker;
use hkask_types::BlockProvenance;
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request};
use smallvec::SmallVec;
use theme::ActiveTheme;
use ui::prelude::*;

use crate::audio_player::AudioPlayer;
use crate::media_ref::{
    MediaBlockBody, MediaKind, MediaRef, MediaStorage, PathMediaStorage, ResolvedMedia,
};
use crate::transport::{TransportBar, TransportEvent, TransportState, next_speed};
use crate::video_decoder::{
    DISPLAY_POLL_INTERVAL, DecodedFrame, VideoPlaybackEvent, WidgetVideoPlayer,
};
#[cfg(feature = "bench-support")]
use crate::video_decoder::{PlaybackState, VideoDeliveryStats};

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Server that hosts the media tools. Fallback dispatch target when a block
/// carries no dispatchable provenance.
const DEFAULT_SERVER: &str = "hkask-mcp-media";
/// Surfaced when the process-global `ToolInvoker` is not wired. Visible state,
/// not a silent no-op (repo `.rules` startup-failure-signal trap).
const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";
/// A playing/loading shared widget that has not been requested by a renderer
/// within this interval is offscreen and must suspend its active work.
const VISIBILITY_GRACE: Duration = Duration::from_millis(250);

/// Convert a decoder-owned BGRA frame into GPUI's render-image container.
/// GPUI uploads this byte buffer as BGRA; the image crate supplies storage,
/// not channel-order conversion.
fn render_video_frame(frame: DecodedFrame) -> Arc<RenderImage> {
    let buffer = image::ImageBuffer::from_raw(frame.width, frame.height, frame.bgra)
        .unwrap_or_else(|| image::ImageBuffer::new(frame.width, frame.height));
    let image_frame = image::Frame::new(buffer);
    Arc::new(RenderImage::new(SmallVec::from_elem(image_frame, 1)))
}

/// The media widget view. Renders inline in markdown (via the D18 seam)
/// or as a standalone panel item.
#[cfg(feature = "bench-support")]
#[derive(Clone, Debug)]
pub struct PlaybackBenchmarkSnapshot {
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Duration,
    pub has_frame: bool,
    pub suspended: bool,
    pub polling: bool,
    pub error: Option<String>,
    pub delivery: VideoDeliveryStats,
}

pub struct MediaWidget {
    reference: MediaRef,
    storage: Arc<dyn MediaStorage>,
    http_client: Arc<dyn HttpClient>,
    focus_handle: FocusHandle,
    audio_player: Option<Arc<AudioPlayer>>,
    video_player: Option<WidgetVideoPlayer>,
    transport: Option<Entity<TransportBar>>,
    current_frame: Option<Arc<RenderImage>>,
    image_data: Option<Arc<gpui::Image>>,
    image_load_task: Option<Task<()>>,
    playback_task: Option<Task<()>>,
    playback_loop_active: bool,
    /// True when an embedding surface hid this shared player. Suspension is
    /// distinct from an operator pause: it is the lifecycle signal that no
    /// hidden audio/decode polling may continue.
    suspended: bool,
    /// Set by the shared D18 registry. Directly-owned widgets are not governed
    /// by the render-heartbeat watchdog.
    visibility_managed: bool,
    last_visible_at: Instant,
    /// Edit marks for interactive trimming: the in/out points the operator
    /// set on the transport, in playback-clock seconds. `None` until set.
    mark_in_secs: Option<f64>,
    mark_out_secs: Option<f64>,
    /// Transport state snapshot from the last tick. Used to suppress re-renders
    /// when nothing changed (paused/stopped/finished) so a visible media widget
    /// does not re-render at 30 fps forever. See `tick_playback`.
    last_transport: Option<TransportState>,
    // True while an audio file is being read off the foreground thread
    // (load_audio_file_async). Flows into the transport bar is_loading.
    audio_loading: bool,
    // True while a video stream URL is being resolved off the foreground
    // thread (load_video_stream_async). Flows into the transport bar
    // is_loading so the user sees a loading state during yt-dlp resolution.
    video_loading: bool,
    // Single-flight guard for `load_audio_file_async`: storing the latest spawn
    // here drops (cancels) any prior in-flight read, so a stale larger read
    // cannot overwrite a newer smaller one, and dropping the widget cancels the
    // outstanding read (no wasteful I/O after drop). See M3.
    audio_load_task: Option<Task<()>>,
    // Single-flight guard for `load_video_stream_async`: same pattern as
    // audio_load_task. Dropping the widget cancels the outstanding yt-dlp
    // resolution.
    video_load_task: Option<Task<()>>,
    // Request identity for remote stream resolution. Suspension and replacement
    // loads invalidate prior completions before they can open a player or start
    // hidden polling.
    video_load_generation: u64,
    /// Transport playback rate (S4): the operator's chosen `SPEED_STEPS` value,
    /// forwarded to whichever player is live. Resets to 1× on load — a fresh
    /// load constructs fresh players (a rodio `Player` per audio load, a worker
    // `VideoPlayer` per open), so the label must not outlive the player it
    /// describes.
    playback_speed: f32,
    error: Option<SharedString>,
    warning: Option<SharedString>,
    /// Ontology concept tag from the parsed block body (e.g. `omc:CreativeWork`).
    /// Drives the "Explain" affordance's tool selection
    /// (the "I" pattern). `None` on older blocks → the widget falls back to
    /// the default explain tool.
    ontology: Option<String>,
    /// Server-authoritative provenance from the parsed block body. Drives the
    /// "Explain" dispatch (re-issues the originating tool's args).
    /// `BlockProvenance::default()` on older blocks → the widget renders without
    /// the dispatch affordance.
    provenance: BlockProvenance,
    /// F — inline drill-down: the explain result text shown inline once the
    /// OMC-driven explain tool completes. `None` = idle.
    explain_result: Option<String>,
    /// Visible error when an explain dispatch cannot proceed (missing invoker
    /// or tool failure). Never silently dropped (repo `.rules`).
    explain_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

// Stat + read an audio file with the 256 MiB size guard. Pure (no `self`), so it
// is safe to move into a background task; the bytes are handed back to the
// foreground thread where `load_bytes_paused` initializes rodio.
async fn fetch_remote_media(
    client: Arc<dyn HttpClient>,
    url: String,
) -> Result<Vec<u8>, SharedString> {
    crate::streaming::validate_network_url(&url)
        .await
        .map_err(|error| SharedString::from(error.to_string()))?;
    let request = Request::get(&url)
        .follow_redirects(RedirectPolicy::NoFollow)
        .body(AsyncBody::default())
        .map_err(|error| SharedString::from(format!("invalid media request: {error}")))?;
    let mut response = client
        .send(request)
        .await
        .map_err(|error| SharedString::from(format!("public media request failed: {error}")))?;
    if response.status().is_redirection() {
        return Err(
            "public media redirects are rejected because the destination was not validated".into(),
        );
    }
    if !response.status().is_success() {
        return Err(SharedString::from(format!(
            "public media request failed with HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .take(crate::media_ref::MAX_INLINE_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| SharedString::from(format!("public media body read failed: {error}")))?;
    if bytes.len() > crate::media_ref::MAX_INLINE_MEDIA_BYTES {
        return Err(SharedString::from(format!(
            "public media exceeds {} bytes",
            crate::media_ref::MAX_INLINE_MEDIA_BYTES
        )));
    }
    Ok(bytes)
}

fn image_from_bytes(kind: MediaKind, bytes: Vec<u8>) -> Result<Arc<gpui::Image>, SharedString> {
    let format = if kind == MediaKind::Svg {
        gpui::ImageFormat::Svg
    } else {
        match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Png) => gpui::ImageFormat::Png,
            Ok(image::ImageFormat::Jpeg) => gpui::ImageFormat::Jpeg,
            Ok(image::ImageFormat::WebP) => gpui::ImageFormat::Webp,
            Ok(image::ImageFormat::Gif) => gpui::ImageFormat::Gif,
            Ok(image::ImageFormat::Bmp) => gpui::ImageFormat::Bmp,
            Ok(image::ImageFormat::Tiff) => gpui::ImageFormat::Tiff,
            Ok(image::ImageFormat::Ico) => gpui::ImageFormat::Ico,
            Ok(image::ImageFormat::Pnm) => gpui::ImageFormat::Pnm,
            Ok(other) => {
                return Err(SharedString::from(format!(
                    "unsupported public image format: {other:?}"
                )));
            }
            Err(error) => {
                return Err(SharedString::from(format!(
                    "public image format detection failed: {error}"
                )));
            }
        }
    };
    Ok(Arc::new(gpui::Image::from_bytes(format, bytes)))
}

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
        Self::with_storage(reference, Arc::new(PathMediaStorage::default()), cx)
    }

    /// Construct a widget from a parsed block body, carrying OMC + provenance
    /// for the "Explain" affordance. Used by `create_media_widget` so the
    /// widget gains the affordance when the block carries OMC + provenance,
    /// and falls back to transport-only display when it doesn't.
    pub fn new_with_block(
        reference: MediaRef,
        block: MediaBlockBody,
        cx: &mut Context<Self>,
    ) -> Self {
        hkask_tool_invoker::record_render(
            block.provenance.tool.clone(),
            block.provenance.span_id.clone(),
        );
        tracing::info!(
            target: "reg.widget.render",
            tool = block.provenance.tool.as_deref().unwrap_or(""),
            span_id = block.provenance.span_id.as_deref().unwrap_or(""),
            ontology = block.ontology.as_deref().unwrap_or(""),
            "REG",
        );
        let mut widget = Self::with_storage(reference, Arc::new(PathMediaStorage::default()), cx);
        widget.ontology = block.ontology;
        widget.provenance = block.provenance;
        widget
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
            http_client: cx.http_client(),
            focus_handle,
            audio_player: None,
            video_player: None,
            transport: None,
            current_frame: None,
            image_data: None,
            image_load_task: None,
            playback_task: None,
            playback_loop_active: false,
            suspended: false,
            visibility_managed: false,
            last_visible_at: Instant::now(),
            mark_in_secs: None,
            mark_out_secs: None,
            last_transport: None,
            audio_loading: false,
            video_loading: false,
            audio_load_task: None,
            video_load_task: None,
            video_load_generation: 0,
            playback_speed: 1.0,
            error: None,
            warning: None,
            ontology: None,
            provenance: BlockProvenance::default(),
            explain_result: None,
            explain_error: None,
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

        let kind = self.reference.kind();

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
                match WidgetVideoPlayer::new() {
                    Ok(player) => self.video_player = Some(player),
                    Err(error) => {
                        self.error = Some(SharedString::from(error.to_string()));
                        return;
                    }
                }
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
        self.error = None;
        self.warning = None;
        // A load replaces the players (fresh rodio `Player` / worker
        // `VideoPlayer`, both at rodio's default 1×) — reset the transport's
        // rate with them so the label never reports a speed the live player
        // left behind.
        self.playback_speed = 1.0;
        match self.storage.resolve(&self.reference) {
            Ok(resolved) => self.load_resolved(resolved, cx),
            Err(error) => {
                self.error = Some(SharedString::from(error.to_string()));
            }
        }
        self.sync_transport_state(cx);
    }

    fn load_resolved(&mut self, resolved: ResolvedMedia, cx: &mut Context<Self>) {
        match resolved.kind {
            MediaKind::Image | MediaKind::Svg => {
                if let Some(bytes) = resolved.bytes {
                    match image_from_bytes(resolved.kind, bytes) {
                        Ok(image) => self.image_data = Some(image),
                        Err(error) => self.error = Some(error),
                    }
                } else if let Some(url) = resolved.url {
                    self.load_remote_image(url.to_string(), resolved.kind, cx);
                }
            }
            MediaKind::Audio => {
                if let Some(bytes) = resolved.bytes {
                    if let Some(player) = &self.audio_player {
                        // No unsolicited audio: load paused.
                        if let Err(error) = player.load_bytes_paused(bytes) {
                            self.error = Some(SharedString::from(error.to_string()));
                        }
                    }
                } else if let Some(path) = resolved.path {
                    self.load_audio_file_async(path, cx);
                } else if let Some(url) = resolved.url {
                    self.load_remote_audio(url.to_string(), cx);
                }
            }
            MediaKind::Video => {
                if let Some(path) = &resolved.path {
                    if let Some(player) = &mut self.video_player {
                        self.video_loading = true;
                        player.open(path);
                        self.start_playback_loop(cx);
                    }
                } else if let Some(url) = &resolved.url {
                    if let Some(path) = url.as_str().strip_prefix("file://") {
                        if let Some(player) = &mut self.video_player {
                            self.video_loading = true;
                            player.open(std::path::Path::new(path));
                            self.start_playback_loop(cx);
                        }
                    } else if url.as_str().starts_with("http://")
                        || url.as_str().starts_with("https://")
                    {
                        // Remote URL — resolve via yt-dlp if needed (for
                        // platform URLs like YouTube), then stream via
                        // FFmpeg's http/https protocol handler. Resolution
                        // runs on a background thread to avoid blocking
                        // the UI during the yt-dlp subprocess.
                        self.load_video_stream_async(url.as_str(), cx);
                    }
                } else {
                    self.error = Some(SharedString::from("resolved video has no path or URL"));
                }
            }
        }
    }

    fn load_remote_image(&mut self, url: String, kind: MediaKind, cx: &mut Context<Self>) {
        let client = self.http_client.clone();
        self.image_load_task = Some(cx.spawn(async move |this, cx| {
            let result = fetch_remote_media(client, url)
                .await
                .and_then(|bytes| image_from_bytes(kind, bytes));
            this.update(cx, |widget, cx| {
                match result {
                    Ok(image) => widget.image_data = Some(image),
                    Err(error) => widget.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn load_remote_audio(&mut self, url: String, cx: &mut Context<Self>) {
        let client = self.http_client.clone();
        self.audio_loading = true;
        self.sync_transport_state(cx);
        self.audio_load_task = Some(cx.spawn(async move |this, cx| {
            let result = fetch_remote_media(client, url).await;
            this.update(cx, |widget, cx| {
                widget.audio_loading = false;
                match result {
                    Ok(bytes) => {
                        if let Some(player) = &widget.audio_player
                            && let Err(error) = player.load_bytes_paused(bytes)
                        {
                            widget.error = Some(SharedString::from(error.to_string()));
                        }
                    }
                    Err(error) => widget.error = Some(error),
                }
                widget.sync_transport_state(cx);
            })
            .ok();
        }));
    }

    // Read + stat an audio file off the foreground thread. The blocking I/O
    // (stat + read up to 256 MiB) runs on a background worker;
    // `load_bytes_paused` initializes rodio on the foreground thread where the
    // AudioPlayer was constructed.
    fn load_audio_file_async(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.audio_loading = true;
        self.sync_transport_state(cx);
        self.audio_load_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { read_audio_file(&path) })
                .await;
            this.update(cx, |widget, cx| {
                widget.audio_loading = false;
                match result {
                    Ok(bytes) => {
                        if let Some(player) = &widget.audio_player {
                            // No unsolicited audio: load paused.
                            if let Err(error) = player.load_bytes_paused(bytes) {
                                widget.error = Some(SharedString::from(error.to_string()));
                            }
                        }
                    }
                    Err(message) => {
                        widget.error = Some(message);
                    }
                }
                widget.sync_transport_state(cx);
            })
            .ok();
        }));
    }

    /// Resolve a remote video URL off the foreground thread, then hand the
    /// direct stream URL(s) to the dedicated playback worker. For direct video
    /// file URLs (mp4, webm, etc.), FFmpeg streams directly. For platform URLs
    /// (YouTube, Vimeo, etc.), `yt-dlp -g` resolves the direct stream URL(s)
    /// first — DASH sources yield separate video and audio URLs, and the
    /// player opens both so streamed video is not silent. The loading state
    /// remains visible until the worker returns the poster frame or an error.
    fn load_video_stream_async(&mut self, url: &str, cx: &mut Context<Self>) {
        self.video_load_generation = self.video_load_generation.saturating_add(1);
        let load_generation = self.video_load_generation;
        self.video_loading = true;
        self.sync_transport_state(cx);
        let url = url.to_string();
        self.video_load_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { crate::streaming::resolve_stream_urls(&url).await })
                .await;
            this.update(cx, |widget, cx| {
                if widget.suspended || widget.video_load_generation != load_generation {
                    return;
                }
                match result {
                    Ok(stream_urls) => {
                        widget.error = None;
                        widget.warning = stream_urls.warning.map(SharedString::from);
                        if let Some(player) = &mut widget.video_player {
                            player.open_stream(&stream_urls.video, stream_urls.audio.as_deref());
                            widget.start_playback_loop(cx);
                        }
                    }
                    Err(error) => {
                        widget.video_loading = false;
                        widget.error = Some(SharedString::from(error.to_string()));
                    }
                }
                widget.sync_transport_state(cx);
            })
            .ok();
        }));
    }

    fn current_transport_state(&self) -> TransportState {
        let mut state = TransportState {
            is_playing: false,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            is_loading: self.audio_loading || self.video_loading,
            speed: self.playback_speed,
        };
        if let Some(player) = &self.audio_player {
            state.is_playing = player.is_playing();
            state.position = player.position();
            state.duration = player.duration();
        }
        if let Some(player) = &self.video_player {
            state.is_playing = player.is_playing();
            state.position = player.position();
            state.duration = player.duration();
        }
        state
    }

    fn sync_transport_state(&mut self, cx: &mut Context<Self>) {
        let state = self.current_transport_state();
        self.last_transport = Some(state);
        if let Some(transport) = &self.transport {
            transport.update(cx, |transport, cx| transport.set_state(state, cx));
        }
        cx.notify();
    }

    fn start_playback_loop(&mut self, cx: &mut Context<Self>) {
        if self.playback_loop_active {
            return;
        }

        self.playback_loop_active = true;
        let entity = cx.entity().downgrade();
        self.playback_task = Some(cx.spawn(async move |_this, cx| {
            let mut last_tick = Instant::now();
            loop {
                cx.background_executor().timer(DISPLAY_POLL_INTERVAL).await;

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
            if let Some(entity) = entity.upgrade() {
                entity.update(cx, |widget, _cx| {
                    widget.playback_loop_active = false;
                });
            }
        }));
    }

    /// Drain one non-blocking playback update. Returns `true` only while media
    /// is loading or playing; pause, stop, finish, and failure close the timer
    /// loop. Rendering occurs only when transport state changes or a new frame
    /// arrives.
    fn tick_playback(&mut self, _delta: Duration, cx: &mut Context<Self>) -> bool {
        if self.visibility_managed && self.last_visible_at.elapsed() > VISIBILITY_GRACE {
            self.suspend(cx);
            return false;
        }
        let mut transport_state = TransportState {
            is_playing: false,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            is_loading: self.audio_loading || self.video_loading,
            speed: self.playback_speed,
        };
        let mut frame_decoded = false;

        if let Some(player) = &self.audio_player {
            transport_state.is_playing = player.is_playing();
            transport_state.position = player.position();
            transport_state.duration = player.duration();
        }

        if let Some(player) = &mut self.video_player {
            let poll = player.poll();
            if let Some(frame) = poll.frame {
                self.current_frame = Some(render_video_frame(frame));
                frame_decoded = true;
            }
            for event in poll.events {
                match event {
                    VideoPlaybackEvent::Opened
                    | VideoPlaybackEvent::Seeked
                    | VideoPlaybackEvent::Completed => {
                        self.video_loading = false;
                    }
                    VideoPlaybackEvent::Failed(error) => {
                        self.video_loading = false;
                        self.error = Some(SharedString::from(format!(
                            "video playback failed: {error}"
                        )));
                    }
                }
            }
            transport_state.is_loading = self.audio_loading || self.video_loading;
            transport_state.is_playing = player.is_playing();
            transport_state.position = player.position();
            transport_state.duration = player.duration();
        }

        // No loaded player → nothing to play or poll; stop the loop.
        if self.audio_player.is_none() && self.video_player.is_none() {
            return false;
        }

        // Re-render only when something visible changed: a new video frame,
        // or a transport state transition (play/pause/seek/finish).
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

        transport_state.is_loading || transport_state.is_playing
    }

    /// expect: Hidden media produces no audio, decode playback, or foreground polling.
    /// [P1] Motivating: Switching surfaces cannot leave a ghost player running.
    /// pre: the widget may be loading, paused, playing, completed, or failed.
    /// post: loaded audio/video is paused and the playback timer task is cancelled; media and edit marks remain loaded.
    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        self.suspended = true;
        if let Some(player) = &self.audio_player {
            player.pause();
        }
        if let Some(player) = &mut self.video_player {
            player.pause();
        }
        self.video_load_generation = self.video_load_generation.saturating_add(1);
        self.video_load_task = None;
        self.video_loading = false;
        self.playback_task = None;
        self.playback_loop_active = false;
        self.sync_transport_state(cx);
    }

    /// Mark a shared widget as requested by a visible embedding. Active work
    /// self-suspends when this heartbeat stops; paused media retains only its
    /// bounded UI state.
    pub fn mark_visible(&mut self, cx: &mut Context<Self>) {
        self.visibility_managed = true;
        self.last_visible_at = Instant::now();
        self.activate(cx);
    }

    /// Reactivate a shared widget when a visible embedding requests it.
    /// Loaded media stays paused; an interrupted initial load restarts so a
    /// hidden panel cannot strand a visible inline player at Loading/0:00.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        let needs_reload = match self.reference.kind() {
            MediaKind::Video => self.current_frame.is_none() && self.error.is_none(),
            MediaKind::Audio => {
                self.audio_player
                    .as_ref()
                    .is_some_and(|player| player.duration().is_zero())
                    && self.error.is_none()
            }
            _ => false,
        };
        if needs_reload {
            self.load(cx);
        } else {
            self.sync_transport_state(cx);
        }
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    fn handle_transport_event(&mut self, event: &TransportEvent, cx: &mut Context<Self>) {
        match event {
            TransportEvent::TogglePlay => {
                self.suspended = false;
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
                let is_playing = self
                    .audio_player
                    .as_ref()
                    .is_some_and(|player| player.is_playing())
                    || self
                        .video_player
                        .as_ref()
                        .is_some_and(WidgetVideoPlayer::is_playing);
                if is_playing {
                    self.start_playback_loop(cx);
                } else {
                    self.playback_task = None;
                    self.playback_loop_active = false;
                }
            }
            TransportEvent::Seek(fraction) => {
                self.suspended = false;
                if let Some(player) = &self.audio_player {
                    let duration = player.duration();
                    player.seek(Duration::from_secs_f32(duration.as_secs_f32() * fraction));
                }
                let video_seek = if let Some(player) = &mut self.video_player {
                    let duration = player.duration();
                    player.seek(Duration::from_secs_f32(duration.as_secs_f32() * fraction));
                    true
                } else {
                    false
                };
                if video_seek {
                    self.video_loading = true;
                    self.start_playback_loop(cx);
                }
            }

            TransportEvent::Stop => {
                self.suspended = false;
                if let Some(player) = &self.audio_player {
                    player.stop();
                }
                if let Some(player) = &mut self.video_player {
                    player.stop();
                }
                self.playback_task = None;
                self.playback_loop_active = false;
            }

            TransportEvent::CycleSpeed => {
                self.suspended = false;
                let next = next_speed(self.playback_speed);
                self.playback_speed = next;
                if let Some(player) = &self.audio_player {
                    player.set_speed(next);
                }
                if let Some(player) = &mut self.video_player {
                    player.set_speed(next);
                }
            }
        }
        self.sync_transport_state(cx);
    }

    /// F — inline drill-down handler (the "Explain" affordance). Dispatches
    /// the OMC-aware explain tool via the governed `shared_tool_invoker()`
    /// (OCAP/gas-budgeted in production via `McpRuntime`). The OMC concept
    /// drives the tool selection (the "I" pattern — ontology-bounded
    /// affordances):
    /// - `omc:Scene` / `omc:Asset` → `gallery_analyze`
    /// - others (CreativeWork, Version, MediaSource, …) → `describe_image`
    ///
    /// Surfaced states (never silent per repo `.rules`):
    /// - `INVOKER_NOT_WIRED_MSG` when `shared_tool_invoker()` returns `None`.
    /// - The tool's own error string when dispatch fails.
    fn on_explain_click(&mut self, cx: &mut Context<Self>) {
        let invoker = match shared_tool_invoker() {
            None => {
                self.explain_error = Some(INVOKER_NOT_WIRED_MSG.to_string());
                self.explain_result = None;
                cx.notify();
                return;
            }
            Some(invoker) => invoker,
        };
        // The "I" pattern: OMC concept drives the explain tool.
        let ontology_tag = self.ontology.as_deref().unwrap_or("");
        let tool = explain_tool_for(ontology_tag);
        // Build the args from the block's provenance + src. `describe_image`
        // takes `image_url`; `gallery_analyze` takes `mode`/`image_indices`.
        // We pass the provenance args through (merged with the src) so the
        // explain tool has the context of the original generation.
        let mut args = self.provenance.args.clone();
        if let serde_json::Value::Object(ref mut map) = args {
            // Ensure the src is available as `image_url` for describe_image.
            if !map.contains_key("image_url") {
                map.insert(
                    "image_url".into(),
                    serde_json::Value::String(self.reference.src().to_string()),
                );
            }
        }
        self.explain_error = None;
        self.explain_result = None;
        let task = invoker.invoke_tool(DEFAULT_SERVER, tool, args);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                match outcome {
                    Ok(text) => {
                        this.explain_result = Some(text);
                        this.explain_error = None;
                    }
                    Err(error) => {
                        this.explain_error = Some(error.message());
                        this.explain_result = None;
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    #[cfg(feature = "bench-support")]
    pub fn benchmark_start_playback(&mut self, cx: &mut Context<Self>) {
        if self
            .video_player
            .as_ref()
            .is_some_and(|player| !player.is_playing())
        {
            self.handle_transport_event(&TransportEvent::TogglePlay, cx);
        }
    }

    #[cfg(feature = "bench-support")]
    #[must_use]
    pub fn benchmark_snapshot(&self) -> Option<PlaybackBenchmarkSnapshot> {
        let player = self.video_player.as_ref()?;
        Some(PlaybackBenchmarkSnapshot {
            state: player.playback_state(),
            position: player.position(),
            duration: player.duration(),
            has_frame: self.current_frame.is_some(),
            suspended: self.suspended,
            polling: self.playback_loop_active,
            error: self.error.as_ref().map(ToString::to_string),
            delivery: player.delivery_stats(),
        })
    }
}

impl Focusable for MediaWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::Render for MediaWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.visibility_managed {
            self.last_visible_at = Instant::now();
        }
        let theme = cx.theme();

        // A load/decode failure renders as a compact, dismissible status
        // line — never a full-viewport takeover. The operator reported the
        // old full-pane error burying the transcript text and every
        // navigation affordance (a dead end); Dismiss clears the error so
        // the surrounding surface (transcript text, back button) is usable.
        let main_content = if let Some(message) = &self.error {
            div()
                .p_2()
                .flex()
                .flex_wrap()
                .gap_2()
                .text_xs()
                .child(SharedString::from(format!("Media error: {message}")))
                .child(
                    div()
                        .id("media-widget-dismiss-error")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(SharedString::from("Dismiss"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.error = None;
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            {
                let src = SharedString::from(self.reference.src());
                match self.reference.kind() {
                    MediaKind::Image | MediaKind::Svg => {
                        // Same floor discipline as the video area below: usable-
                        // height floors belong to the embedding site
                        // (block_renderer holds the content-flow floor);
                        // definite-height panes shrink this area freely.
                        let mut container = div().flex_1().min_h_0();
                        if let Some(image) = self.image_data.clone() {
                            // max_h_full: img.rs injects the intrinsic aspect
                            // ratio into the style when none is set, and that
                            // derivation overrides the height percentage — a
                            // tall image overflows a short definite pane the
                            // same way a video frame did (the video arm's fix,
                            // pinned by `transcript_row_fits_frame_and_transport`).
                            container = container.child(
                                img(ImageSource::from(image))
                                    .size_full()
                                    .max_h_full()
                                    .object_fit(ObjectFit::Contain),
                            );
                        } else {
                            container = container
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(SharedString::from("Loading public media…"));
                        }
                        container.into_any_element()
                    }
                    MediaKind::Audio => {
                        let transport = self.transport.clone();
                        let mut container = div()
                            .flex()
                            .flex_col()
                            .flex_1()
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
                            .flex_1()
                            .gap_1()
                            .border_1()
                            .border_color(theme.colors().border)
                            .rounded_md()
                            .overflow_hidden();

                        let mut video_area = div()
                            .id("media-video-area")
                            // Test-support hook: exposes this element's laid-out
                            // bounds to layout ground-truth tests (noop in
                            // release builds).
                            .debug_selector(|| "media-video-area".into())
                            // min_h_0, never a min_h floor: usable-height
                            // floors belong to the embedding site
                            // (block_renderer holds the content-flow floor),
                            // and a definite-height pane shrinks the widget
                            // freely — operator report 2026-11-04: a 120px
                            // floor here pushed the transport (play/stop +
                            // seek slider) past the pane's clipped bottom, so
                            // the workspace player lost its slider entirely
                            // while the same shared widget kept it in the
                            // chat flow. In content flow the decoded frame's
                            // aspect derivation against the definite width
                            // (img.rs) holds the video open without a floor.
                            .flex_1()
                            .min_h_0()
                            .bg(theme.colors().editor_background);

                        if let Some(frame) = frame {
                            // size_full + max_h_full + Contain against the
                            // flex_1 parent's definite height: the frame
                            // scales to fit the pane in both dimensions.
                            // (A fraction width alone does NOT trigger
                            // gpui's img auto-height derivation — that
                            // only fires for `Definite(Absolute)` widths,
                            // img.rs — so w_full here left the frame at its
                            // natural 480px height, unscaled. The max_h_full
                            // clamp is load-bearing for SHORT definite hosts:
                            // img.rs injects the frame's intrinsic aspect
                            // ratio into the style whenever none is set, and
                            // that derivation overrides the height
                            // percentage — without the clamp a 16:9 frame
                            // laid out 449px tall inside a 100px area
                            // (operator report 2026-11-06: the transcript
                            // row showed the top third of the image with the
                            // transport clipped past the row), pinned by
                            // `transcript_row_fits_frame_and_transport`. In
                            // the content flow the percentage max is inert —
                            // the aspect derivation against the definite
                            // flow width still holds the video open there.)
                            video_area = video_area.child(
                                img(ImageSource::Render(frame))
                                    .size_full()
                                    .max_h_full()
                                    .object_fit(ObjectFit::Contain)
                                    // Test-support hook: exposes the frame
                                    // element's laid-out bounds to layout
                                    // ground-truth tests (noop in release).
                                    .debug_selector(|| "media-video-frame".into()),
                            );
                        } else {
                            video_area = video_area
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
                }
            }
        };

        div()
            .id("media-widget")
            .track_focus(&self.focus_handle)
            // Test-support hook: exposes the widget's laid-out bounds to
            // layout ground-truth tests (noop in release builds).
            .debug_selector(|| "media-widget".into())
            .size_full()
            // The usable-height floor no longer lives on the widget: this
            // is a shared per-asset entity rendered by BOTH the viewer's
            // definite-height panes and the conversation's content flow,
            // so a render-site floor on the entity cannot differ per
            // embedding. The content-flow embedder holds the floor
            // (`hkask_viz_core::block_renderer`); definite-height panes
            // shrink the widget freely and the panel's content container
            // clips as the backstop (the close-transcript overlap,
            // 2026-11-04: the 240px floor painted the transport 228px past
            // a 24px content area, pinned by
            // `closing_transcript_leaves_widget_inside_pane`).
            // Flex column so the main content's flex_1 resolves against a
            // definite height — the root's size_full is definite only when
            // the embedding pane provides one (the viewer's Media tab does;
            // scrollable containers do not).
            .flex()
            .flex_col()
            .child(main_content)
            .when_some(self.warning.clone(), |element, warning| {
                element.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(cx.theme().colors().text_muted)
                        .child(SharedString::from(format!("Media warning: {warning}"))),
                )
            })
            .children(self.render_affordances(cx))
    }
}

impl MediaWidget {
    /// The surfaced load-error message, if any — the embedding sites'
    /// layout gate. A failed load renders as a compact status line, so the
    /// content-flow embedder (`block_renderer`) suppresses its
    /// usable-height floor while an error is shown: a floor under an error
    /// would bury the surrounding surface (operator report 2026-10-28).
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The current playback-clock position in seconds. For audio the
    /// player is eagerly constructed, so the audio arm reports `Some(0.0)`
    /// before a load completes or after a load error — callers gate on the
    /// load outcome, not on `None`. The one position accessor for both
    /// media kinds: the transcript pane's follow-along source and the edit
    /// toolbar's marks/position source (2026-10-27 unification — it
    /// replaced the video-only accessor, which could not mark audio).
    #[must_use]
    pub fn transport_position_secs(&self) -> Option<f64> {
        if let Some(player) = &self.audio_player {
            return Some(player.position().as_secs_f64());
        }
        self.video_player
            .as_ref()
            .map(|player| player.position().as_secs_f64())
    }

    /// Whether the loaded media is actively playing — the pane's
    /// follow-along gate (D3: the position poll runs only while playing).
    #[must_use]
    pub fn is_transport_playing(&self) -> bool {
        self.current_transport_state().is_playing
    }

    /// Seek playback to an absolute time in seconds across whichever media is
    /// loaded, mirroring the transport seek arms for both the audio and video
    /// players; the target clamps to a known duration; non-finite targets
    /// (NaN, ±inf) are ignored — NaN poisons the clamp and ±inf panics
    /// `Duration::from_secs_f64` on the foreground thread (2026-10-26
    /// review). The transcript pane's click-to-seek entry point
    /// (transcript-bench plan S2); the transport slider path is unchanged.
    /// Both arms carry discriminating position-landing tests (video: the
    /// exact sought frame; audio: a backward jump impossible under natural
    /// playback).
    pub fn seek_to_secs(&mut self, secs: f64, cx: &mut Context<Self>) {
        if !secs.is_finite() {
            return;
        }
        self.suspended = false;
        if let Some(player) = &self.audio_player {
            let target = clamp_seek_target(secs, player.duration());
            player.seek(Duration::from_secs_f64(target));
        }
        let video_seek = if let Some(player) = &mut self.video_player {
            let target = clamp_seek_target(secs, player.duration());
            player.seek(Duration::from_secs_f64(target));
            true
        } else {
            false
        };
        if video_seek {
            self.video_loading = true;
            self.start_playback_loop(cx);
        }
        self.sync_transport_state(cx);
    }

    /// The asset src this widget plays (path or URL) — the input for
    /// `video_clip` / `video_concat` dispatches from the viewer.
    #[must_use]
    pub fn src(&self) -> &str {
        self.reference.src()
    }

    /// Mark the current playback position as the trim in-point.
    pub fn mark_in(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.transport_position_secs() {
            self.mark_in_secs = Some(position);
            cx.notify();
        }
    }

    /// Mark the current playback position as the trim out-point.
    pub fn mark_out(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.transport_position_secs() {
            self.mark_out_secs = Some(position);
            cx.notify();
        }
    }

    /// Clear both trim marks.
    pub fn clear_marks(&mut self, cx: &mut Context<Self>) {
        self.mark_in_secs = None;
        self.mark_out_secs = None;
        cx.notify();
    }

    /// The trim range as `(in, out)` seconds once both marks are set and
    /// ordered. `None` while incomplete or inverted — the caller must not
    /// dispatch `video_clip` with an invalid range (the server would reject
    /// it, but the button should not offer a no-op).
    #[must_use]
    pub fn trim_range(&self) -> Option<(f64, f64)> {
        match (self.mark_in_secs, self.mark_out_secs) {
            (Some(in_secs), Some(out_secs)) if in_secs < out_secs => Some((in_secs, out_secs)),
            _ => None,
        }
    }

    /// Toolbar display state: the playback position label, the marks label,
    /// and whether the trim range is dispatchable.
    #[must_use]
    pub fn edit_state_labels(&self) -> (gpui::SharedString, gpui::SharedString, bool) {
        let position = self
            .transport_position_secs()
            .map(|secs| format!("{secs:.1}s"))
            .unwrap_or_else(|| "—".to_string());
        let marks = match (self.mark_in_secs, self.mark_out_secs) {
            (Some(in_secs), Some(out_secs)) => format!("in {in_secs:.1}s out {out_secs:.1}s"),
            (Some(in_secs), None) => format!("in {in_secs:.1}s out —"),
            (None, Some(out_secs)) => format!("in — out {out_secs:.1}s"),
            (None, None) => "no marks".to_string(),
        };
        (position.into(), marks.into(), self.trim_range().is_some())
    }

    /// Render the OMC-driven affordance bar (Explain) when the block carries
    /// dispatchable provenance. Blocks without provenance render only the
    /// media + transport (no affordances) — the additive contract.
    fn render_affordances(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // Only render affordances when the block carries dispatchable provenance
        // (tool + server present). Older blocks without provenance render
        // transport-only — the additive contract.
        if !self.provenance.is_dispatchable() {
            return Vec::new();
        }
        let mut elements = Vec::new();
        let mut bar = h_flex()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .child(
                div()
                    .id("media-explain")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.on_explain_click(cx);
                    }))
                    .child(
                        Label::new("Explain")
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    ),
            );
        // Surface the explain result inline when present.
        if let Some(result) = &self.explain_result {
            let truncated = truncate_explain_result(result);
            bar = bar.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().colors().text_muted)
                    .child(SharedString::from(truncated)),
            );
        }
        // Surface the explain error visibly when present.
        if let Some(error) = &self.explain_error {
            bar = bar.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().colors().text)
                    .child(SharedString::from(format!("Explain error: {error}"))),
            );
        }
        elements.push(bar.into_any_element());
        elements
    }
}

/// Clamp a seek target to `[0, duration]` when the duration is known, else
/// to `[0, ∞)`. Word-start callers pass in-range times by construction; the
/// clamp keeps a stray overshoot from seeking past the end of the media.
fn clamp_seek_target(secs: f64, duration: Duration) -> f64 {
    let duration_secs = duration.as_secs_f64();
    if duration_secs > 0.0 {
        secs.clamp(0.0, duration_secs)
    } else {
        secs.max(0.0)
    }
}

/// Truncate the explain result for inline display. The full result stays in
/// the agent conversation as the durable record; the widget only shows a
/// compact truncation for at-a-glance context.
fn truncate_explain_result(result: &str) -> String {
    const MAX_CHARS: usize = 280;
    if result.chars().count() <= MAX_CHARS {
        return result.to_string();
    }
    let truncated: String = result.chars().take(MAX_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::media_ref::MediaBlockBody;
    use gpui::{AppContext, TestAppContext};
    use std::sync::{Arc, Mutex};

    /// expect: Remote media redirects are rejected before an unvalidated destination is loaded.
    /// [P1] Motivating: a public URL cannot redirect the widget into a private network.
    #[test]
    fn remote_media_redirect_is_visible_and_not_followed() -> anyhow::Result<()> {
        let client = http_client::FakeHttpClient::create(|request| async move {
            assert_eq!(request.uri().host(), Some("93.184.216.34"));
            Ok(http_client::Response::builder()
                .status(http_client::StatusCode::FOUND)
                .header("location", "http://127.0.0.1/private.png")
                .body(AsyncBody::default())?)
        });
        let error = futures::executor::block_on(fetch_remote_media(
            client,
            "https://93.184.216.34/public.png".to_string(),
        ))
        .expect_err("redirect must be rejected");
        assert!(error.contains("redirects are rejected"));
        Ok(())
    }

    /// Serializes tests that mutate the process-global `ToolInvoker`, and
    /// every test that opens the default audio output device — without this
    /// lock, parallel invoker tests observe each other's invoker and
    /// intermittently fail with "invoker not wired" even when the test wired
    /// a mock, and parallel device opens race each other's
    /// playback/position timing (observed flaky under local parallel
    /// execution 2026-10-26; CI runs this suite serially with a consuming
    /// null sink). Shared with the video_decoder device tests via
    /// `pub(crate)` — one serialization domain, one lock.
    pub(crate) static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A `MockToolInvoker` whose calls and canned result are configurable.
    struct MockToolInvoker {
        calls: Mutex<Vec<(String, String, serde_json::Value)>>,
        result: Mutex<Result<String, hkask_tool_invoker::InvokeError>>,
    }

    impl hkask_tool_invoker::ToolInvoker for MockToolInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> Task<Result<String, hkask_tool_invoker::InvokeError>> {
            self.calls.lock().unwrap_or_else(|e| e.into_inner()).push((
                server.to_string(),
                tool.to_string(),
                args,
            ));
            let outcome = self
                .result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            Task::ready(outcome)
        }
    }

    /// RAII guard that restores the tool-invoker global to `None` on drop so a
    /// test failure cannot leak a mock into sibling tests.
    struct InvokerGuard;
    impl Drop for InvokerGuard {
        fn drop(&mut self) {
            hkask_tool_invoker::set_tool_invoker(None);
        }
    }

    fn video_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-lifecycle.mp4")
    }

    fn audio_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-seek.wav")
    }

    /// A 2s video WITH an audio stream — exercises the audio-master clock
    /// path (as opposed to the wall-clock fallback arm).
    fn video_with_audio_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-opus.mp4")
    }

    struct RetryStorage {
        attempts: std::sync::atomic::AtomicUsize,
    }

    impl MediaStorage for RetryStorage {
        fn resolve(&self, _reference: &MediaRef) -> anyhow::Result<ResolvedMedia> {
            if self
                .attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                anyhow::bail!("transient storage failure")
            }
            // The retry must complete synchronously: local bytes decode
            // in-process, so no background fetch task can set the error
            // after the test's load call returns. (A remote URL here made
            // the test timing-dependent — the fetch failure could land
            // inside the parking window and re-set the error.)
            Ok(ResolvedMedia {
                kind: MediaKind::Image,
                path: None,
                bytes: Some(RETRY_IMAGE_BYTES.to_vec()),
                url: None,
            })
        }
    }

    /// A minimal valid 1x1 PNG — the retry fixture's successful payload.
    const RETRY_IMAGE_BYTES: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// expect: Pause and Stop update the visible transport state in the same event turn.
    /// [P1] Motivating: controls never display playback state that the child player has already left.
    /// pre: a video is loaded and playing.
    /// post: pause publishes not-playing immediately; stop publishes not-playing at position zero immediately.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn pause_and_stop_synchronize_transport_immediately(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = video_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));

        let load_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.current_frame.is_none())
            && std::time::Instant::now() < load_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }

        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        let play_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !widget.read_with(cx, |widget, _cx| {
            widget.last_transport.is_some_and(|state| state.is_playing)
        }) && std::time::Instant::now() < play_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(widget.read_with(cx, |widget, _cx| {
            widget.last_transport.is_some_and(|state| state.is_playing)
        }));

        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        assert!(widget.read_with(cx, |widget, _cx| {
            widget.last_transport.is_some_and(|state| !state.is_playing)
        }));

        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        let replay_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !widget.read_with(cx, |widget, _cx| {
            widget.last_transport.is_some_and(|state| state.is_playing)
        }) && std::time::Instant::now() < replay_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(widget.read_with(cx, |widget, _cx| {
            widget.last_transport.is_some_and(|state| state.is_playing)
        }));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::Stop, cx)
            })
        });
        assert!(widget.read_with(cx, |widget, _cx| {
            widget
                .last_transport
                .is_some_and(|state| !state.is_playing && state.position == Duration::ZERO)
        }));
    }

    /// expect: Hiding a widget cancels an in-flight remote resolution and stale completion cannot restart polling.
    /// [P1] Motivating: hidden media remains quiescent even when an earlier request completes later.
    /// pre: remote video resolution owns an in-flight task.
    /// post: suspension drops the task, clears loading, and completion cannot start playback polling.
    #[gpui::test]
    async fn suspend_cancels_in_flight_remote_video_resolution(cx: &mut TestAppContext) {
        let reference = MediaRef::new(
            SharedString::from("https://example.invalid/video.mp4"),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        assert!(widget.read_with(cx, |widget, _cx| widget.video_load_task.is_some()));

        cx.update(|cx| widget.update(cx, |widget, cx| widget.suspend(cx)));
        assert!(widget.read_with(cx, |widget, _cx| {
            widget.video_load_task.is_none()
                && !widget.video_loading
                && !widget.playback_loop_active
        }));

        cx.run_until_parked();
        assert!(widget.read_with(cx, |widget, _cx| {
            widget.suspended && !widget.playback_loop_active
        }));
    }

    /// expect: A visible embedding can restart a local video load cancelled by suspension.
    /// [P1] Motivating: Shared panel/inline ownership must not strand controls at Loading/0:00.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn activate_restarts_suspended_local_video_load(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = video_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.load(cx);
                widget.suspend(cx);
                widget.activate(cx);
            })
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        // Wait for BOTH completion signals: the first decoded frame AND the
        // transport state's settled sync — they land on different ticks (the
        // frame arrives from the decode worker; `last_transport` syncs on
        // the playback loop). Waiting only for the frame left a race where
        // the state assert read a still-loading transport under parallel
        // device contention (observed flaky 2026-10-26: 2 of 6 local
        // parallel runs; CI runs this suite serially).
        while widget.read_with(cx, |widget, _cx| {
            widget.current_frame.is_none()
                || !widget.last_transport.is_some_and(|state| {
                    !state.is_loading && !state.is_playing && !state.duration.is_zero()
                })
        }) && std::time::Instant::now() < deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        cx.run_until_parked();

        let state = widget.read_with(cx, |widget, _cx| widget.last_transport);
        assert!(widget.read_with(cx, |widget, _cx| widget.current_frame.is_some()));
        assert!(state.is_some_and(|state| {
            !state.is_loading && !state.is_playing && !state.duration.is_zero()
        }));
    }

    /// expect: A shared player suspends active work when renderer heartbeats stop.
    /// [P1] Motivating: Bounded strong ownership must never reintroduce offscreen audio or polling.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn stale_visibility_heartbeat_suspends_playback(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = video_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.load(cx);
                widget.mark_visible(cx);
            })
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.current_frame.is_none())
            && Instant::now() < deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx);
                widget.last_visible_at =
                    Instant::now() - VISIBILITY_GRACE - Duration::from_millis(1);
                assert!(!widget.tick_playback(Duration::from_millis(33), cx));
            })
        });
        assert!(widget.read_with(cx, |widget, _cx| {
            widget.is_suspended() && !widget.playback_loop_active
        }));
    }

    /// expect: Retrying a failed media load removes the old visible failure and successful completion stays clear.
    /// [P1] Motivating: recovered media is not obscured by stale failure state.
    /// pre: the first storage attempt fails and the second succeeds.
    /// post: the first cause is visible; after retry the widget error is absent.
    #[gpui::test]
    async fn successful_retry_clears_prior_visible_error(cx: &mut TestAppContext) {
        let storage = Arc::new(RetryStorage {
            attempts: std::sync::atomic::AtomicUsize::new(0),
        });
        let reference = MediaRef::new(SharedString::from("retry.png"), MediaKind::Image);
        let widget =
            cx.update(|cx| cx.new(|cx| MediaWidget::with_storage(reference, storage.clone(), cx)));

        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        assert_eq!(
            widget.read_with(cx, |widget, _cx| widget
                .error
                .as_ref()
                .map(ToString::to_string)),
            Some("transient storage failure".to_string())
        );

        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        assert!(widget.read_with(cx, |widget, _cx| widget.error.is_none()));
        cx.run_until_parked();
        assert!(widget.read_with(cx, |widget, _cx| widget.error.is_none()));
    }

    /// expect: A missing image or SVG reports the original path-resolution cause in the widget.
    /// [P1] Motivating: a broken visual asset is diagnosable where the empty widget would appear.
    /// pre: PathMediaStorage receives a nonexistent local image or SVG path.
    /// post: the visible widget error preserves the requested path and filesystem cause.
    #[gpui::test]
    async fn missing_image_and_svg_surface_path_storage_cause(cx: &mut TestAppContext) {
        for (kind, path) in [
            (MediaKind::Image, "/definitely/missing/hkask-image.png"),
            (MediaKind::Svg, "/definitely/missing/hkask-image.svg"),
        ] {
            let reference = MediaRef::new(SharedString::from(path), kind);
            let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
            cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
            let error = widget
                .read_with(cx, |widget, _cx| {
                    widget.error.as_ref().map(ToString::to_string)
                })
                .expect("missing path must be visible");
            assert!(error.contains(&format!("media file not found: {path}")));
            assert!(error.contains("No such file or directory"));
        }
    }

    /// expect: A loaded video displays a poster frame without polling until
    /// the operator presses Play, and hiding it suspends playback and polling.
    /// [P1] Motivating: media is inspectable without autoplay or idle work.
    /// pre: the video fixture is present and decodable.
    /// post: load is paused with a frame; play owns one task; suspend pauses the player and owns none.
    /// [P2] Constraining: loading never produces unsolicited audio.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn video_loads_first_frame_paused_and_polls_only_while_playing(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = video_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));

        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        let poster_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| {
            widget.current_frame.is_none() || widget.playback_loop_active
        }) && std::time::Instant::now() < poster_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        cx.run_until_parked();
        let (has_frame, is_playing, has_task, error) = widget.read_with(cx, |widget, _cx| {
            (
                widget.current_frame.is_some(),
                widget
                    .video_player
                    .as_ref()
                    .is_some_and(WidgetVideoPlayer::is_playing),
                widget.playback_loop_active,
                widget.error.clone(),
            )
        });
        assert!(has_frame, "load installs the poster frame; error={error:?}");
        assert!(!is_playing, "load remains paused");
        assert!(!has_task, "paused media has no polling task");

        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        assert!(
            widget.read_with(cx, |widget, _cx| widget.playback_loop_active),
            "Play starts polling"
        );

        cx.update(|cx| widget.update(cx, |widget, cx| widget.suspend(cx)));
        let (is_playing, has_task) = widget.read_with(cx, |widget, _cx| {
            (
                widget
                    .video_player
                    .as_ref()
                    .is_some_and(WidgetVideoPlayer::is_playing),
                widget.playback_loop_active,
            )
        });
        assert!(!is_playing, "suspension pauses video and its audio clock");
        assert!(!has_task, "suspension cancels polling");

        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::Seek(5.0 / 6.0), cx)
            })
        });
        let seek_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| {
            widget
                .video_player
                .as_ref()
                .and_then(|player| player.delivery_stats().last_consumed_pts_ms)
                != Some(500)
        }) && std::time::Instant::now() < seek_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert_eq!(
            widget.read_with(cx, |widget, _cx| {
                widget
                    .video_player
                    .as_ref()
                    .and_then(|player| player.delivery_stats().last_consumed_pts_ms)
            }),
            Some(500),
            "paused seek restarts polling until the sought frame is visible"
        );
        assert!(
            !widget.read_with(cx, |widget, _cx| widget.playback_loop_active),
            "paused seek polling stops after delivering the frame"
        );
    }

    /// expect: A fatal video-source failure is visible once and closes polling.
    /// [P1] Motivating: broken media is diagnosable without a warning storm.
    /// pre: the source cannot be opened as video.
    /// post: one stable widget error remains and playback polling is inactive.
    #[gpui::test]
    async fn fatal_video_failure_surfaces_once_and_stops_polling(cx: &mut TestAppContext) {
        let reference = MediaRef::new(
            SharedString::from("/definitely/missing/hkask-video.mp4"),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));

        let error_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.error.is_none())
            && std::time::Instant::now() < error_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }

        let first_error = widget
            .read_with(cx, |widget, _cx| widget.error.clone())
            .expect("fatal open failure is visible");
        assert!(first_error.contains("media file not found"));
        assert!(!widget.read_with(cx, |widget, _cx| widget.playback_loop_active));

        for _ in 0..3 {
            let keep_polling = cx.update(|cx| {
                widget.update(cx, |widget, cx| {
                    widget.tick_playback(Duration::from_millis(33), cx)
                })
            });
            assert!(!keep_polling, "fatal playback remains terminal");
            assert_eq!(
                widget.read_with(cx, |widget, _cx| widget.error.clone()),
                Some(first_error.clone())
            );
        }
    }

    /// Build a `MediaBlockBody` carrying ontology + dispatchable provenance.
    fn block_with_provenance(ontology: &str, tool: &str, prompt: &str) -> MediaBlockBody {
        MediaBlockBody {
            kind: "image".to_string(),
            src: "/tmp/img.png".to_string(),
            gallery_asset_id: None,
            ontology: Some(ontology.to_string()),
            provenance: BlockProvenance {
                tool: Some(tool.to_string()),
                server: Some("hkask-mcp-media".to_string()),
                args: serde_json::json!({ "prompt": prompt }),
                span_id: None,
            },
        }
    }

    /// Build a `MediaBlockBody` without provenance (the older block shape).
    fn block_without_provenance() -> MediaBlockBody {
        MediaBlockBody {
            kind: "image".to_string(),
            src: "/tmp/img.png".to_string(),
            gallery_asset_id: None,
            ontology: None,
            provenance: BlockProvenance::default(),
        }
    }

    #[gpui::test]
    async fn explain_dispatches_describe_image_for_creative_work(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = InvokerGuard;
        let mock = Arc::new(MockToolInvoker {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Ok("a cat in space".to_string())),
        });
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        let block = block_with_provenance("omc:CreativeWork", "generate_image", "a cat");
        let reference = block.to_media_ref().expect("resolves");
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.on_explain_click(cx));
        });
        cx.run_until_parked();

        let calls = mock.calls.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(calls.len(), 1, "exactly one explain dispatch");
        assert_eq!(calls[0].0, "hkask-mcp-media");
        // The "I" pattern: omc:CreativeWork → describe_image.
        assert_eq!(calls[0].1, "describe_image");
        // The src is merged into args as image_url.
        assert_eq!(calls[0].2["image_url"], "/tmp/img.png");
        assert_eq!(calls[0].2["prompt"], "a cat");

        let (result, error) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (widget.explain_result.clone(), widget.explain_error.clone())
            })
        });
        assert_eq!(result.as_deref(), Some("a cat in space"));
        assert!(error.is_none(), "no error on success");
    }

    #[gpui::test]
    async fn explain_dispatches_gallery_analyze_for_scene(cx: &mut TestAppContext) {
        // The "I" pattern: omc:Scene → gallery_analyze (not describe_image).
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = InvokerGuard;
        let mock = Arc::new(MockToolInvoker {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Ok("scene analysis".to_string())),
        });
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        let block = block_with_provenance("omc:Scene", "gallery_analyze", "scene");
        let reference = block.to_media_ref().expect("resolves");
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.on_explain_click(cx));
        });
        cx.run_until_parked();

        let calls = mock.calls.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1, "gallery_analyze",
            "omc:Scene dispatches gallery_analyze"
        );
    }

    #[gpui::test]
    async fn explain_surfaces_error_when_no_invoker(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = InvokerGuard;
        hkask_tool_invoker::set_tool_invoker(None);

        let block = block_with_provenance("omc:CreativeWork", "generate_image", "a cat");
        let reference = block.to_media_ref().expect("resolves");
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.on_explain_click(cx));
        });
        cx.run_until_parked();

        let (error, result) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (widget.explain_error.clone(), widget.explain_result.clone())
            })
        });
        assert_eq!(error.as_deref(), Some(INVOKER_NOT_WIRED_MSG));
        assert!(result.is_none(), "no result without an invoker");
    }

    #[gpui::test]
    async fn explain_surfaces_error_on_tool_failure(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = InvokerGuard;
        let mock = Arc::new(MockToolInvoker {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Err(hkask_tool_invoker::InvokeError::Failed {
                message: "describe_image unavailable".to_string(),
                kind: None,
            })),
        });
        hkask_tool_invoker::set_tool_invoker(Some(mock));

        let block = block_with_provenance("omc:CreativeWork", "generate_image", "a cat");
        let reference = block.to_media_ref().expect("resolves");
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.on_explain_click(cx));
        });
        cx.run_until_parked();

        let (error, result) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (widget.explain_error.clone(), widget.explain_result.clone())
            })
        });
        assert_eq!(
            error.as_deref(),
            Some("describe_image unavailable"),
            "tool error surfaced visibly"
        );
        assert!(result.is_none(), "no result on failure");
    }

    #[gpui::test]
    async fn affordances_not_rendered_without_provenance(cx: &mut TestAppContext) {
        // The additive contract: older blocks without provenance render only
        // the media + transport (no affordances).
        let block = block_without_provenance();
        let reference = block.to_media_ref().expect("resolves");
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        let affordances =
            cx.update(|cx| widget.update(cx, |widget, cx| widget.render_affordances(cx)));
        assert!(
            affordances.is_empty(),
            "no affordances rendered without dispatchable provenance"
        );
    }

    #[test]
    fn truncate_explain_result_short_passthrough() {
        assert_eq!(truncate_explain_result("short"), "short");
    }

    #[test]
    fn truncate_explain_result_long_truncates() {
        let long: String = "a".repeat(500);
        let truncated = truncate_explain_result(&long);
        assert!(truncated.ends_with('…'));
        assert!(truncated.chars().count() <= 281); // 280 + ellipsis
    }

    #[test]
    fn explain_tool_for_omc_dispatches_correctly() {
        // The "I" pattern: ontology concept drives the explain tool.
        // Uses the shared `hkask_bridge_ontology::omc::explain_tool_for`.
        // Every OMC input is a fixture-verified bridge constant — the former
        // `omc:Version` / `omc:MediaSource` inputs were fabricated (OMC v2.8
        // publishes no such classes; versioning is VersionInfo, captured
        // source material is Capture).
        use hkask_bridge_ontology::omc::{
            ASSET, CAPTURE, CREATIVE_WORK, SCENE, SEQUENCE, SHOT, VERSION_INFO,
        };
        assert_eq!(explain_tool_for(SCENE), "gallery_analyze");
        assert_eq!(explain_tool_for(ASSET), "gallery_analyze");
        assert_eq!(explain_tool_for(CREATIVE_WORK), "describe_image");
        assert_eq!(explain_tool_for(VERSION_INFO), "describe_image");
        assert_eq!(explain_tool_for(CAPTURE), "describe_image");
        assert_eq!(explain_tool_for(SEQUENCE), "describe_image");
        assert_eq!(explain_tool_for(SHOT), "describe_image");
        // Non-OMC and unknown concepts fall back to the general explain tool.
        assert_eq!(
            explain_tool_for(hkask_bridge_ontology::sumo::CORPORATION),
            "describe_image"
        );
        assert_eq!(explain_tool_for(""), "describe_image");
    }

    /// dcterms:identifier: `MediaWidget::seek_to_secs`
    /// expect: An absolute seek moves audio playback to the requested time.
    /// [P1] Motivating: click-to-seek must land on the clicked word, not near it.
    /// The load gate is the surfaced error state, not the eagerly-constructed
    /// player: an empty player is present from widget construction, so a
    /// fixture outside the approved roots "loads" vacuously while the
    /// actual read fails — the 2026-10-25 false positive this test replaces.
    /// Serializes this test against every other sink-opening test — the
    /// four invoker tests, the five local-video tests, and the three
    /// video_decoder device tests, all on the shared GLOBAL_TEST_LOCK
    /// (parallel default-sink opens raced the audio playback's position
    /// reads; the local-only flake never reached CI's serial executor).
    /// The test harness runs each test on its own thread, so a waiter
    /// blocks its thread while the holder finishes — no shared executor,
    /// no deadlock.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn seek_to_secs_positions_audio_playback(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = audio_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Audio,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        let load_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.audio_loading)
            && std::time::Instant::now() < load_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(
            widget.read_with(cx, |widget, _cx| widget.error.is_none()),
            "the approved-root fixture loads: {:?}",
            widget.read_with(cx, |widget, _cx| widget.error.clone())
        );
        let duration = widget.read_with(cx, |widget, _cx| {
            widget
                .audio_player
                .as_ref()
                .map_or(Duration::ZERO, |player| player.duration())
        });
        assert!(
            duration >= Duration::from_millis(2900) && duration <= Duration::from_millis(3100),
            "the decoder reports the 3s fixture duration (got {duration:?})"
        );
        assert_eq!(
            widget.read_with(cx, |widget, _| widget.transport_position_secs()),
            Some(0.0),
            "the pane's position source covers the loaded audio at zero"
        );
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        let play_deadline = std::time::Instant::now() + Duration::from_secs(3);
        while widget.read_with(cx, |widget, _cx| {
            widget.audio_player.as_ref().map_or(true, |player| {
                player.position() < Duration::from_millis(1200)
            })
        }) && std::time::Instant::now() < play_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(
            widget
                .read_with(cx, |widget, _cx| widget.audio_player.as_ref().is_some_and(
                    |player| player.position() >= Duration::from_millis(1200)
                )),
            "natural playback advances past 1.2s before the seek"
        );
        cx.update(|cx| widget.update(cx, |widget, cx| widget.seek_to_secs(0.5, cx)));
        // A backward jump is impossible under natural playback — only a real
        // seek drops the position back below 800ms.
        let seek_deadline = std::time::Instant::now() + Duration::from_secs(1);
        while widget.read_with(cx, |widget, _cx| {
            widget.audio_player.as_ref().map_or(true, |player| {
                player.position() >= Duration::from_millis(800)
            })
        }) && std::time::Instant::now() < seek_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(
            widget
                .read_with(cx, |widget, _cx| widget.audio_player.as_ref().is_some_and(
                    |player| player.position() < Duration::from_millis(800)
                )),
            "seek_to_secs(0.5) jumps audio playback backward to ~0.5s"
        );
        // The edit toolbar's marks cover audio too (2026-10-27): the same
        // transport position source drives both media kinds, so trim is
        // dispatchable on audio — mark in at the post-seek position while
        // playing, advance past it, mark out, and the toolbar reports a
        // dispatchable range.
        cx.update(|cx| widget.update(cx, |widget, cx| widget.mark_in(cx)));
        let mark_out_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _| {
            !widget
                .mark_in_secs
                .zip(widget.transport_position_secs())
                .is_some_and(|(in_secs, position)| in_secs + 0.2 <= position)
        }) && std::time::Instant::now() < mark_out_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        cx.update(|cx| widget.update(cx, |widget, cx| widget.mark_out(cx)));
        let (_position_label, marks_label, trim_ready) =
            widget.read_with(cx, |widget, _| widget.edit_state_labels());
        assert!(
            trim_ready,
            "audio marks produce a dispatchable trim range: {marks_label}"
        );
        assert!(
            marks_label.starts_with("in "),
            "the toolbar reports both marks: {marks_label}"
        );
    }

    /// expect: At 2× speed the audio position advances at twice wall-clock —
    /// 1.6s of media time arrives in strictly less wall time than 1×
    /// playback needs (≥1.6s), and the media-time rate stays ≥1.5× wall.
    /// [P1] Motivating: the transport's speed control must actually speed the
    /// audio clock, not just the label (Reduct/Descript table-stakes parity).
    /// pre: the approved-root 3s audio fixture loads without error, the
    /// transport cycles 1× → 1.25× → 1.5× → 2×.
    /// post: after playing at 2×, the position delta reaches ≥1600ms with wall
    /// elapsed <1.2s and delta ≥ 1.5× wall. The bounds discriminate on a
    /// real-time-cadenced output device (the gate environment); a CI null sink
    /// that drains faster than real time satisfies them vacuously — the
    /// discrimination claim is scoped to the real-device gates.
    /// Serializes on GLOBAL_TEST_LOCK like every sink-opening test.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn speed_two_x_advances_audio_position_at_double_wall_clock(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = audio_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Audio,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        let load_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.audio_loading)
            && std::time::Instant::now() < load_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(
            widget.read_with(cx, |widget, _cx| widget.error.is_none()),
            "the approved-root fixture loads: {:?}",
            widget.read_with(cx, |widget, _cx| widget.error.clone())
        );
        // Cycle 1× → 1.25× → 1.5× → 2× — the transport's only speed entry point.
        for _ in 0..3 {
            cx.update(|cx| {
                widget.update(cx, |widget, cx| {
                    widget.handle_transport_event(&TransportEvent::CycleSpeed, cx)
                })
            });
        }
        assert_eq!(
            widget.read_with(cx, |widget, _cx| widget.last_transport.map(|s| s.speed)),
            Some(2.0),
            "three cycles from 1× land on 2× and the transport state reports it"
        );
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        let wall_start = std::time::Instant::now();
        let position_start = widget.read_with(cx, |widget, _cx| {
            widget
                .audio_player
                .as_ref()
                .map_or(Duration::ZERO, |p| p.position())
        });
        let play_deadline = wall_start + Duration::from_secs(3);
        while widget.read_with(cx, |widget, _cx| {
            widget.audio_player.as_ref().map_or(true, |p| {
                p.position().saturating_sub(position_start) < Duration::from_millis(1600)
            })
        }) && std::time::Instant::now() < play_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        let wall_elapsed = wall_start.elapsed();
        let delta = widget.read_with(cx, |widget, _cx| {
            widget
                .audio_player
                .as_ref()
                .map_or(Duration::ZERO, |p| p.position())
                .saturating_sub(position_start)
        });
        assert!(
            delta >= Duration::from_millis(1600),
            "2× playback advances the audio clock past 1.6s of media (got {delta:?})"
        );
        assert!(
            wall_elapsed < Duration::from_millis(1200),
            "2× playback reaches 1.6s of media in <1.2s wall (took {wall_elapsed:?}; 1× needs ≥1.6s)"
        );
        assert!(
            delta >= wall_elapsed.mul_f32(1.5),
            "media time advances at ≥1.5× wall clock (delta {delta:?} over {wall_elapsed:?})"
        );
    }

    /// expect: At 2× speed the VIDEO position advances at twice wall-clock —
    /// the CycleSpeed command reaches the worker, the audio pipeline's output
    /// player speeds, and the audio-master clock (consumed audio scaled back
    /// to media time per read) advances at rate.
    /// [P1] Motivating: speed on both media kinds is the S4 objective; the
    /// video chain (transport → command → worker → decoder pipeline) must
    /// actually speed, not just the audio-kind path.
    /// pre: the 2s video+audio opus fixture loads its first frame without
    /// error; the transport cycles to 2×.
    /// post: after playing at 2×, the position delta reaches ≥900ms of media
    /// in <0.8s wall and at ≥1.5× wall (1× needs ≥0.9s; 2× ≈ 0.45s). The
    /// bounds discriminate on a real-time-cadenced output device — a CI null
    /// sink that drains faster than real time satisfies them vacuously.
    /// Serializes on GLOBAL_TEST_LOCK like every sink-opening test.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn speed_two_x_advances_video_position_at_double_wall_clock(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = video_with_audio_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        let load_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.current_frame.is_none())
            && std::time::Instant::now() < load_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(
            widget.read_with(cx, |widget, _cx| widget.current_frame.is_some()),
            "the opus video fixture loads its first frame"
        );
        // Cycle 1× → 1.25× → 1.5× → 2× — the full widget → command → worker
        // → decoder pipeline chain.
        for _ in 0..3 {
            cx.update(|cx| {
                widget.update(cx, |widget, cx| {
                    widget.handle_transport_event(&TransportEvent::CycleSpeed, cx)
                })
            });
        }
        assert_eq!(
            widget.read_with(cx, |widget, _cx| widget.last_transport.map(|s| s.speed)),
            Some(2.0),
            "three cycles from 1× land on 2× and the transport state reports it"
        );
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.handle_transport_event(&TransportEvent::TogglePlay, cx)
            })
        });
        let wall_start = std::time::Instant::now();
        let position_start = widget
            .read_with(cx, |widget, _cx| widget.transport_position_secs())
            .unwrap_or(0.0);
        let play_deadline = wall_start + Duration::from_secs(3);
        while widget.read_with(cx, |widget, _cx| {
            widget
                .transport_position_secs()
                .is_none_or(|position| position - position_start < 0.9)
        }) && std::time::Instant::now() < play_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        let wall_elapsed = wall_start.elapsed();
        let delta = Duration::from_secs_f64(
            widget
                .read_with(cx, |widget, _cx| widget.transport_position_secs())
                .unwrap_or(0.0)
                - position_start,
        );
        assert!(
            delta >= Duration::from_millis(900),
            "2× playback advances the video clock past 0.9s of media (got {delta:?})"
        );
        assert!(
            wall_elapsed < Duration::from_millis(800),
            "2× playback reaches 0.9s of video media in <0.8s wall (took {wall_elapsed:?}; 1× needs ≥0.9s)"
        );
        assert!(
            delta >= wall_elapsed.mul_f32(1.5),
            "video media time advances at ≥1.5× wall clock (delta {delta:?} over {wall_elapsed:?})"
        );
    }

    /// dcterms:identifier: `clamp_seek_target`
    /// expect: Seek targets clamp to [0, duration] when known and stay
    /// non-negative when unknown.
    /// [P1] Motivating: a stray overshoot must never seek past the media end.
    #[test]
    fn clamp_seek_target_bounds_the_request() {
        let duration = Duration::from_secs(3);
        assert_eq!(clamp_seek_target(1.5, duration), 1.5);
        assert_eq!(clamp_seek_target(99.0, duration), 3.0);
        assert_eq!(clamp_seek_target(-4.0, duration), 0.0);
        assert_eq!(clamp_seek_target(7.0, Duration::ZERO), 7.0);
        assert_eq!(clamp_seek_target(-4.0, Duration::ZERO), 0.0);
    }

    /// dcterms:identifier: `MediaWidget::seek_to_secs`
    /// expect: Non-finite targets (NaN, ±inf) are ignored — no panic, no
    /// state change — on a widget with no media loaded (the eagerly-
    /// constructed players). NaN poisons the clamp and ±inf panics
    /// `Duration::from_secs_f64`; the guard is the public API's defense
    /// (2026-10-26 review).
    /// [P1] Motivating: a public seek API must never take down the
    /// foreground thread on a bad input.
    #[gpui::test]
    async fn seek_to_secs_ignores_non_finite_targets(cx: &mut TestAppContext) {
        let widget = cx.new(|cx| {
            MediaWidget::new(
                MediaRef::new(
                    "file:///nonexistent/non-finite-seek-probe.wav".into(),
                    MediaKind::Audio,
                ),
                cx,
            )
        });
        cx.run_until_parked();
        widget.update(cx, |widget, cx| {
            widget.seek_to_secs(f64::NAN, cx);
            widget.seek_to_secs(f64::INFINITY, cx);
            widget.seek_to_secs(f64::NEG_INFINITY, cx);
            // No panic, and an untouched transport: nothing was loaded.
            assert_eq!(widget.transport_position_secs(), Some(0.0));
            assert!(!widget.is_transport_playing());
        });
    }

    /// dcterms:identifier: `MediaWidget::seek_to_secs`
    /// expect: An absolute seek moves video playback to the requested frame.
    /// [P1] Motivating: click-to-seek must land on the clicked word, not near it.
    #[allow(clippy::await_holding_lock)]
    #[gpui::test]
    async fn seek_to_secs_positions_loaded_video(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = video_fixture();
        let reference = MediaRef::new(
            SharedString::from(path.to_string_lossy().to_string()),
            MediaKind::Video,
        );
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new(reference, cx)));
        cx.update(|cx| widget.update(cx, |widget, cx| widget.load(cx)));
        let load_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.current_frame.is_none())
            && std::time::Instant::now() < load_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert!(
            widget.read_with(cx, |widget, _cx| widget.current_frame.is_some()),
            "video fixture loads"
        );
        cx.update(|cx| widget.update(cx, |widget, cx| widget.seek_to_secs(0.5, cx)));
        let seek_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| {
            widget
                .video_player
                .as_ref()
                .and_then(|player| player.delivery_stats().last_consumed_pts_ms)
                != Some(500)
        }) && std::time::Instant::now() < seek_deadline
        {
            let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
            timer.await;
            cx.run_until_parked();
        }
        assert_eq!(
            widget.read_with(cx, |widget, _cx| {
                widget
                    .video_player
                    .as_ref()
                    .and_then(|player| player.delivery_stats().last_consumed_pts_ms)
            }),
            Some(500),
            "seek_to_secs(0.5) lands on the 500ms frame"
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use gpui::{TestAppContext, px, size};

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/playback-opus.mp4");

    /// Host view: fills the window so the widget under test gets a definite
    /// size to lay out against (as the viewer pane does in production).
    struct WidgetHost {
        widget: Entity<MediaWidget>,
    }

    impl gpui::Render for WidgetHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.widget.clone())
        }
    }

    /// The panel transcript row's host (operator report 2026-11-06): the
    /// sub-surface's bounded player row is a 140px flex row with min_w_0
    /// and overflow_hidden, the widget as its direct child —
    /// media_viewer.rs `render_workspace_header_item` (item 0 of the
    /// workspace list). Mirrors the production row construction exactly:
    /// the widget root resolves the row's height fine; the shipped defect
    /// was the img INSIDE the video area — img.rs injects the frame's
    /// intrinsic aspect ratio into the style whenever none is set, and
    /// that width-derived height overrode the img's height percentage —
    /// so a 16:9 frame laid out ~450px tall inside the ~100px area and
    /// overflow_hidden clipped the row to its top third, with the
    /// transport clipped past the row's bottom.
    struct TranscriptRowHost {
        widget: Entity<MediaWidget>,
    }

    impl gpui::Render for TranscriptRowHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .h(px(140.))
                .flex()
                .min_w_0()
                .overflow_hidden()
                .child(self.widget.clone())
        }
    }

    /// The transcript row pin (2026-11-06 operator report): inside the
    /// panel's 140px player row, the decoded frame AND the transport must
    /// both lay out INSIDE the row. The shipped bug showed the top third
    /// of the image with the transport clipped past the row's bottom —
    /// so the transcript surface carried no visible play controls at all.
    /// Hermetic: a 16:9 frame injected directly, no decoder, no fixture.
    #[gpui::test]
    fn transcript_row_fits_frame_and_transport(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);

        let (_, cx) = cx.add_window_view(|_window, cx| {
            let widget = hermetic_video_widget(Some((1920, 1080)), cx);
            TranscriptRowHost { widget }
        });
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();

        let frame_bounds = cx
            .debug_bounds("media-video-frame")
            .expect("video frame laid out with debug bounds");
        let transport_bounds = cx
            .debug_bounds("media-transport")
            .expect("transport laid out with debug bounds");
        assert!(
            frame_bounds.bottom() <= px(141.),
            "the frame must fit the 140px transcript row, got bottom {:?}",
            frame_bounds.bottom()
        );
        assert!(
            transport_bounds.bottom() <= px(141.),
            "the transport must stay inside the 140px row, got bottom {:?}",
            transport_bounds.bottom()
        );
    }

    /// Layout ground truth: the video area's laid-out bounds must respond to
    /// the window size. This is the property that was FALSE in both shipped
    /// broken versions (scroll-wrapper collapse: constant ~120px; fraction-
    /// width img: constant natural 480px) while every unit test passed —
    /// the assertions here read GPUI's actual laid-out pixel bounds, not the
    /// element tree I wrote.
    #[gpui::test]
    fn video_area_scales_with_window_size(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let body = format!(r#"{{"kind":"video","src":"{FIXTURE}"}}"#);

        let (_, cx) = cx.add_window_view(|window, cx| {
            let widget = crate::create_media_widget(&body, window, cx)
                .expect("video block renders a widget");
            WidgetHost { widget }
        });
        // add_window_view opens maximized — resize to the measurement size
        // before reading bounds.
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();

        let short_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out with debug bounds");

        // Simulate the operator resizing the window taller.
        cx.simulate_resize(size(px(800.), px(900.)));
        cx.run_until_parked();

        let tall_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out with debug bounds after resize");

        // Width fills the window (minus the container's 1px borders).
        assert!(
            short_bounds.size.width > px(700.),
            "video area must fill the window width, got {:?}",
            short_bounds.size.width
        );
        // The discriminating property: height tracks the window height
        // exactly — flex_1 absorbs the entire 300px resize delta.
        assert_eq!(
            tall_bounds.size.height - short_bounds.size.height,
            px(300.),
            "video area height must scale with window height: {:?} at 600px vs {:?} at 900px",
            short_bounds.size.height,
            tall_bounds.size.height
        );
        assert!(
            short_bounds.size.height > px(400.),
            "video area must fill most of a 600px window, got {:?}",
            short_bounds.size.height
        );
    }

    /// A decoded ultra-wide (21:9) frame must not re-inflate the layout:
    /// gpui's `img` injects the frame's intrinsic aspect ratio into the
    /// img style (`img.rs` request_layout), so this probe pins that the
    /// frame element's laid-out bounds still derive from the video area's
    /// bounds — never from the frame's natural 2560px width. Hermetic: the
    /// frame is injected directly (no decoder, no fixture file).
    #[gpui::test]
    fn wide_frame_fits_narrow_host(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let body = r#"{"kind":"video","src":"/hermetic/probe.mp4"}"#;
        let block = MediaBlockBody::parse(body).expect("valid media block");
        let media_ref = block.to_media_ref().expect("video media ref");

        let (_, cx) = cx.add_window_view(|_window, cx| {
            // with_storage initializes the video player + transport WITHOUT
            // loading from disk — no error state — and the synthetic frame
            // below stands in for the decoder's output.
            let widget = cx.new(|cx| {
                let mut widget = MediaWidget::with_storage(
                    media_ref.clone(),
                    Arc::new(PathMediaStorage::default()),
                    cx,
                );
                let buffer =
                    image::ImageBuffer::from_pixel(2560, 1080, image::Rgba([16, 16, 16, 255]));
                let frame = image::Frame::new(buffer);
                widget.current_frame =
                    Some(Arc::new(RenderImage::new(SmallVec::from_elem(frame, 1))));
                widget
            });
            WidgetHost { widget }
        });
        // A narrow host: 320px wide — far narrower than the frame's natural
        // 2560px width.
        cx.simulate_resize(size(px(320.), px(240.)));
        cx.run_until_parked();

        let video_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out with debug bounds");
        let frame_bounds = cx
            .debug_bounds("media-video-frame")
            .expect("video frame laid out with debug bounds");
        assert!(
            video_bounds.right() <= px(320.) + px(1.),
            "video area must fit the 320px host: right {:?}",
            video_bounds.right()
        );
        assert!(
            frame_bounds.size.width <= video_bounds.size.width + px(1.)
                && frame_bounds.right() <= video_bounds.right() + px(1.),
            "the 21:9 frame element must derive its width from the video area \
             (contain-fit), not its natural 2560px width: frame {frame_bounds:?} vs \
             video area {video_bounds:?}"
        );
    }

    /// The content-flow embedding contract host (block_renderer's wrapper):
    /// a scroll container gives its content INDEFINITE height (rows are
    /// content-sized, the viewport scrolls) — the chat's contract, where a
    /// bare auto div would instead be stretched by the window and measure
    /// definite-pane semantics. block_renderer holds the player open with
    /// min_h(240) (hkask_viz_core.rs:383); the widget survives on
    /// content-driven sizing alone (the frame's aspect derivation).
    struct FlowHost {
        widget: Entity<MediaWidget>,
    }

    impl gpui::Render for FlowHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            // The chat's content-flow contract: a scroll container gives its
            // content INDEFINITE height (rows are content-sized, the viewport
            // scrolls) — the first probe draft used a bare `div().w_full()`
            // root, which the window stretched to a definite 600px and
            // measured definite-pane semantics instead. block_renderer's
            // wrapper holds the player open with min_h(240)
            // (hkask_viz_core.rs:383).
            div()
                .id("media-flow-host-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(
                    div()
                        .w_full()
                        .child(div().size_full().min_h(px(240.)).child(self.widget.clone())),
                )
        }
    }

    /// A hermetic video widget: `with_storage` initializes the player +
    /// transport without touching disk; `frame` optionally injects a
    /// synthetic decoded frame (the decoder output's stand-in).
    fn hermetic_video_widget(frame: Option<(u32, u32)>, cx: &mut App) -> Entity<MediaWidget> {
        let body = r#"{"kind":"video","src":"/hermetic/probe.mp4"}"#;
        let block = MediaBlockBody::parse(body).expect("valid media block");
        let media_ref = block.to_media_ref().expect("video media ref");
        cx.new(|cx| {
            let mut widget =
                MediaWidget::with_storage(media_ref, Arc::new(PathMediaStorage::default()), cx);
            if let Some((width, height)) = frame {
                let buffer =
                    image::ImageBuffer::from_pixel(width, height, image::Rgba([16, 16, 16, 255]));
                let frame = image::Frame::new(buffer);
                widget.current_frame =
                    Some(Arc::new(RenderImage::new(SmallVec::from_elem(frame, 1))));
            }
            widget
        })
    }

    /// THE operator-defect pin (2026-11-04): the workspace player lost its
    /// transport — play/stop buttons and the seek slider — because the
    /// video area's 120px min-height floor pushed the transport row past
    /// the pane's clipped bottom (pre-fix measurement at a 56px host:
    /// transport laid out at y=125..159, fully outside the box, while the
    /// same shared widget kept its slider in the chat flow). The floor is
    /// gone; the invariant: the transport stays INSIDE the player box at
    /// every definite host height, at its natural height, and the video
    /// area yields to it rather than pushing it out.
    #[gpui::test]
    fn transport_stays_visible_at_every_host_height(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let (_, cx) = cx.add_window_view(|_window, cx| {
            let widget = hermetic_video_widget(Some((320, 180)), cx);
            WidgetHost { widget }
        });
        for host_height in [px(240.), px(150.), px(56.)] {
            cx.simulate_resize(size(px(320.), host_height));
            cx.run_until_parked();
            let video_area = cx
                .debug_bounds("media-video-area")
                .expect("video area lays out");
            let transport = cx
                .debug_bounds("media-transport")
                .expect("transport lays out");
            let widget_bounds = cx.debug_bounds("media-widget").expect("widget lays out");
            assert!(
                transport.bottom() <= widget_bounds.bottom() + px(1.),
                "the transport (play/stop + seek slider) must stay inside the \
                 player box at host height {host_height:?}: transport {transport:?} \
                 vs widget {widget_bounds:?}"
            );
            assert!(
                transport.size.height >= px(24.),
                "the transport row must render at its natural height, never \
                 crushed ({host_height:?}): {transport:?}"
            );
            assert!(
                transport.top() >= video_area.bottom() - px(1.),
                "the video area yields to the transport — the transport sits \
                 below it, never displaced by it ({host_height:?}): video \
                 {video_area:?} transport {transport:?}"
            );
        }

        // The floor's absence IS the fix: at the shortest host the video
        // area yields to 16px (pre-fix it held 120px and pushed the
        // transport 103px past the box).
        cx.simulate_resize(size(px(320.), px(56.)));
        cx.run_until_parked();
        let video_area = cx
            .debug_bounds("media-video-area")
            .expect("video area lays out at the shortest host");
        assert!(
            video_area.size.height < px(20.),
            "the video area must yield below its former 120px floor at short \
             hosts — it no longer floors: {video_area:?}"
        );
    }

    /// The legitimate-case pin for the floor removal: the content-flow
    /// embedding must keep a DECODED video usable without the removed floor
    /// — gpui's img aspect derivation against the definite flow width holds
    /// the video open (measured 798x449 for a 320x180 frame at an 800px
    /// window: width x 9/16).
    #[gpui::test]
    fn flow_embedding_keeps_decoded_video_open(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let (_, cx) = cx.add_window_view(|_window, cx| {
            let widget = hermetic_video_widget(Some((320, 180)), cx);
            FlowHost { widget }
        });
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();
        let video_area = cx
            .debug_bounds("media-video-area")
            .expect("video area lays out in flow");
        assert!(
            (px(440.)..px(460.)).contains(&video_area.size.height),
            "the decoded frame's aspect derivation must hold the flow video \
             open (798px width x 9/16 for a 320x180 frame): {video_area:?}"
        );
    }

    /// The pre-decode placeholder in flow is the floor removal's only cost:
    /// it shrinks to its text height instead of 120px, but never collapses
    /// the player to zero, and the transport stays inside the
    /// content-sized widget.
    #[gpui::test]
    fn flow_placeholder_does_not_collapse_the_player(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let (_, cx) = cx.add_window_view(|_window, cx| {
            let widget = hermetic_video_widget(None, cx);
            FlowHost { widget }
        });
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();
        let video_area = cx
            .debug_bounds("media-video-area")
            .expect("video area lays out in flow");
        let transport = cx
            .debug_bounds("media-transport")
            .expect("transport lays out in flow");
        let widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget lays out in flow");
        assert!(
            (px(10.)..px(100.)).contains(&video_area.size.height),
            "the placeholder video area is text-sized — never collapsed to \
             zero, never floored back at 120px: {video_area:?}"
        );
        assert!(
            transport.bottom() <= widget_bounds.bottom() + px(1.),
            "the transport must stay inside the content-sized flow player: \
             {transport:?} vs {widget_bounds:?}"
        );
    }

    /// The theme/settings globals the widget's render reads — shared by
    /// this module's layout tests.
    fn init_layout_test_globals(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
    }
}
