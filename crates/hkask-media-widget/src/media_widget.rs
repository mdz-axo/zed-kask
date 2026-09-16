//! The `MediaWidget` GPUI view — dispatches on `MediaKind` and renders the
//! appropriate media (image via `img()`, SVG via `img()`, audio via `rodio`
//! with transport controls, video via FFmpeg → `RenderImage` → `img()`).
//!
//! When the parsed block carries an OMC concept tag + provenance (the
//! `media_block::enrich_with_omc_and_provenance` path), the widget also renders
//! two affordances:
//! - **Explain** (F): dispatches the OMC-aware explain tool (`describe_image`
//!   or `gallery_analyze`) via `shared_tool_invoker()`. The OMC concept drives
//!   the tool selection — the first implementation of the "I" pattern
//!   (ontology-bounded affordances).
//! - **I disagree** (C): composes a provenance-scoped revision request and
//!   injects it back into the active conversation via `shared_injector()`
//!   (D21 widget→agent seam). Falls back to a copyable draft when no injector
//!   is active (repo `.rules`).

use futures::AsyncReadExt as _;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, ImageSource, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, RenderImage, SharedString, Styled, StyledImage,
    Subscription, Task, Window, div, img, px,
};
use gpui_util::ResultExt as _;
use hkask_bridge_ontology::omc::explain_tool_for;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request};
use smallvec::SmallVec;
use theme::ActiveTheme;
use ui::prelude::*;

use crate::audio_player::AudioPlayer;
use crate::media_ref::{
    MediaBlockBody, MediaKind, MediaRef, MediaStorage, PathMediaStorage, ResolvedMedia,
};
use crate::transport::{TransportBar, TransportEvent, TransportState};
use crate::video_decoder::{DecodedFrame, VideoPlaybackEvent, WidgetVideoPlayer};
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
    error: Option<SharedString>,
    warning: Option<SharedString>,
    /// Ontology concept tag from the parsed block body (e.g. `omc:CreativeWork`,
    /// `fibo:Corporation`). Drives the "Explain" affordance's tool selection
    /// (the "I" pattern). `None` on older blocks → the widget falls back to
    /// the default explain tool.
    ontology: Option<String>,
    /// Server-authoritative provenance from the parsed block body. Drives the
    /// "Explain" dispatch (re-issues the originating tool's args) and the
    /// "I disagree" compose-back. `BlockProvenance::default()` on older
    /// blocks → the widget renders without dispatch/compose-back affordances.
    provenance: BlockProvenance,
    /// Composed revision request surfaced as a copyable draft when the
    /// conversation injector is absent (no active conversation). Lets the user
    /// still use the "I disagree" body even when it can't be injected. Cleared
    /// when a successful inject fires (repo `.rules`: visible, not a silent
    /// no-op).
    disagree_draft: Option<String>,
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
// foreground thread where `load_bytes_paused` initializes rodio. See SF-1.
async fn fetch_remote_media(
    client: Arc<dyn HttpClient>,
    url: String,
) -> Result<Vec<u8>, SharedString> {
    crate::streaming::validate_network_url(&url)
        .await
        .map_err(SharedString::from)?;
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
    /// for the "Explain" and "I disagree" affordances. Used by
    /// `create_media_widget` so the widget gains the affordances when the
    /// block carries OMC + provenance, and falls back to transport-only
    /// display when it doesn't.
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
            error: None,
            warning: None,
            ontology: None,
            provenance: BlockProvenance::default(),
            disagree_draft: None,
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
    // AudioPlayer was constructed. See SF-1 in tasks/widget-interactivity/plan.md.
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
                        widget.error = Some(SharedString::from(error));
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
            volume: 1.0,
            is_loading: self.audio_loading || self.video_loading,
        };
        if let Some(player) = &self.audio_player {
            state.is_playing = player.is_playing();
            state.position = player.position();
            state.duration = player.duration();
            state.volume = player.volume();
        }
        if let Some(player) = &self.video_player {
            state.is_playing = player.is_playing();
            state.position = player.position();
            state.duration = player.duration();
            state.volume = player.volume();
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
            volume: 1.0,
            is_loading: self.audio_loading || self.video_loading,
        };
        let mut frame_decoded = false;

        if let Some(player) = &self.audio_player {
            transport_state.is_playing = player.is_playing();
            transport_state.position = player.position();
            transport_state.duration = player.duration();
            transport_state.volume = player.volume();
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
            TransportEvent::VolumeChange(volume) => {
                if let Some(player) = &self.audio_player {
                    player.set_volume(*volume);
                }
                if let Some(player) = &mut self.video_player {
                    player.set_volume(*volume);
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
        }
        self.sync_transport_state(cx);
    }

    /// Compose the provenance-scoped "I disagree" body. References the
    /// artifact's OMC concept and provenance (tool + args) so the agent can
    /// correlate the revision request to the exact media result the widget
    /// rendered. Falls back to a generic "the media result" framing when
    /// provenance or OMC is absent (grill-me edge case b).
    fn compose_disagree_body(&self) -> String {
        let tool = self.provenance.tool.as_deref().unwrap_or("the media tool");
        let ontology_label = self.ontology.as_deref().unwrap_or("media result");
        // Pull a short human-readable hint from the provenance args (prompt,
        // text, or video_url) so the body references what the user saw.
        let hint = self
            .provenance
            .args
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                self.provenance
                    .args
                    .get("text")
                    .and_then(serde_json::Value::as_str)
            })
            .or_else(|| {
                self.provenance
                    .args
                    .get("video_url")
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::to_string);
        match hint {
            Some(hint) => format!(
                "Re: the {ontology_label} generated by {tool} ({hint}). I believe this result is incorrect. Please re-check. My concern: "
            ),
            None => format!(
                "Re: the {ontology_label} generated by {tool}. I believe this result is incorrect. Please re-check. My concern: "
            ),
        }
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

    /// The "I disagree" affordance handler (C). Composes the provenance-scoped
    /// revision request and injects it back into the active conversation via
    /// the kask `shared_injector()` (D21 widget→agent seam). When no
    /// conversation is active, surfaces the composed body as a copyable draft
    /// instead of a silent no-op (repo `.rules`). Never auto-sends when the
    /// injector is absent — the production injector only pre-fills the
    /// composer; the user reviews and submits.
    fn on_disagree_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.compose_disagree_body();
        tracing::info!(target: "reg.widget.disagree", "REG");
        if let Some(injector) = hkask_conversation_injector::shared_injector(cx) {
            // The production injector pre-fills the active ThreadView's editor
            // synchronously; it returns `Ok` while that view is alive, or `Err`
            // if the active conversation has been dropped (the global holds a
            // weak ref, so a dead thread is never retained and never leaks
            // across app/test lifetimes). Await in a detached task so the
            // `Err` path surfaces the composed body as a draft (not silently
            // dropped — repo `.rules`), and so `clippy::let_underscore_future`
            // is not triggered.
            let draft = body.clone();
            let task = injector.inject(body, window, cx);
            cx.spawn(async move |this, cx| {
                if let Err(error) = task.await {
                    tracing::warn!(
                        target: "reg.widget.disagree",
                        error = %error,
                        "conversation inject failed; surfacing draft"
                    );
                    this.update(cx, |this, cx| {
                        this.disagree_draft = Some(draft);
                        cx.notify();
                    })
                    .log_err();
                }
            })
            .detach();
            self.disagree_draft = None;
        } else {
            // No active conversation: surface the composed body as a draft so
            // the user can still copy it into chat (visible, not a silent
            // no-op — repo `.rules`).
            self.disagree_draft = Some(body);
        }
        cx.notify();
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

        // A load/decode failure must be visible — storing it in `self.error`
        // without rendering it leaves an empty widget the operator cannot
        // distinguish from a slow load.
        let main_content = if let Some(message) = &self.error {
            div()
                .p_4()
                .flex_1()
                .text_sm()
                .text_color(theme.colors().text_muted)
                .child(SharedString::from(format!("Media error: {message}")))
                .into_any_element()
        } else {
            {
                let src = SharedString::from(self.reference.src());
                match self.reference.kind() {
                    MediaKind::Image | MediaKind::Svg => {
                        let mut container = div().flex_1().min_h(px(100.0));
                        if let Some(image) = self.image_data.clone() {
                            container = container.child(
                                img(ImageSource::from(image))
                                    .size_full()
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
                            .flex_1()
                            .min_h(px(120.0))
                            .bg(theme.colors().editor_background);

                        if let Some(frame) = frame {
                            // size_full + Contain against the flex_1 parent's
                            // definite height: the frame scales to fit the pane
                            // in both dimensions. (A fraction width alone does
                            // NOT trigger gpui's img auto-height derivation —
                            // that only fires for `Definite(Absolute)` widths,
                            // img.rs — so w_full here left the frame at its
                            // natural 480px height, unscaled.)
                            video_area = video_area.child(
                                img(ImageSource::Render(frame))
                                    .size_full()
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
            // In definite-height containers (the viewer pane) size_full
            // fills; in content-flow containers (conversation inline) it
            // collapses and this floor keeps the player usable.
            .min_h(px(240.0))
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
    /// The current playback-clock position in seconds, when a video is
    /// loaded. The viewer's edit toolbar reads this to label its controls.
    #[must_use]
    pub fn playback_position_secs(&self) -> Option<f64> {
        self.video_player
            .as_ref()
            .map(|player| player.position().as_secs_f64())
    }

    /// The asset src this widget plays (path or URL) — the input for
    /// `video_clip` / `video_concat` dispatches from the viewer.
    #[must_use]
    pub fn src(&self) -> &str {
        self.reference.src()
    }

    /// Mark the current playback position as the trim in-point.
    pub fn mark_in(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.playback_position_secs() {
            self.mark_in_secs = Some(position);
            cx.notify();
        }
    }

    /// Mark the current playback position as the trim out-point.
    pub fn mark_out(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.playback_position_secs() {
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
            .playback_position_secs()
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

    /// Render the OMC-driven affordance bar (Explain + I disagree) when the
    /// block carries provenance. Older blocks without provenance render only
    /// the media + transport (no affordances) — the additive contract.
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
            )
            .child(
                div()
                    .id("media-disagree")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.on_disagree_click(window, cx);
                    }))
                    .child(
                        Label::new("I disagree")
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
        // Surface the disagree draft visibly when no injector is active.
        if let Some(draft) = &self.disagree_draft {
            elements.push(
                div()
                    .p_2()
                    .text_sm()
                    .text_color(cx.theme().colors().text)
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_md()
                    .child(SharedString::from(format!(
                        "No active conversation — copy this into chat:\n{draft}"
                    )))
                    .into_any_element(),
            );
        }
        elements
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
mod tests {
    use super::*;
    use crate::media_ref::MediaBlockBody;
    use gpui::{AppContext, TestAppContext, Window};
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

    /// Serializes tests that mutate the process-global `ToolInvoker`
    /// (the `ConversationInjector` is now per-app — it drops with each
    /// `TestAppContext` — but this lock is still shared with the invoker
    /// tests). Without this lock, parallel invoker tests observe each other's
    /// invoker and intermittently fail with "invoker not wired" even when the
    /// test wired a mock.
    static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    /// Records the body of every `inject` call. `Send + Sync` for the
    /// `Arc<dyn ConversationInjector>` global.
    #[derive(Default)]
    struct MockConversationInjector {
        bodies: Mutex<Vec<String>>,
    }

    impl hkask_conversation_injector::ConversationInjector for MockConversationInjector {
        fn inject(
            &self,
            body: String,
            _window: &mut gpui::Window,
            _cx: &mut gpui::App,
        ) -> Task<Result<(), String>> {
            self.bodies
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(body);
            Task::ready(Ok(()))
        }
    }

    /// Trivial root view for `add_window_view` so the test can obtain a `Window`
    /// for `on_disagree_click` without rendering `MediaWidget` (which would
    /// need a theme global this leaf crate's tests don't initialise).
    struct DummyView;
    impl Render for DummyView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn video_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-lifecycle.mp4")
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
            Ok(ResolvedMedia {
                kind: MediaKind::Image,
                path: None,
                bytes: None,
                url: Some(SharedString::from("https://example.invalid/recovered.png")),
            })
        }
    }

    /// expect: Pause and Stop update the visible transport state in the same event turn.
    /// [P1] Motivating: controls never display playback state that the child player has already left.
    /// pre: a video is loaded and playing.
    /// post: pause publishes not-playing immediately; stop publishes not-playing at position zero immediately.
    #[gpui::test]
    async fn pause_and_stop_synchronize_transport_immediately(cx: &mut TestAppContext) {
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
    #[gpui::test]
    async fn activate_restarts_suspended_local_video_load(cx: &mut TestAppContext) {
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

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while widget.read_with(cx, |widget, _cx| widget.current_frame.is_none())
            && std::time::Instant::now() < deadline
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
    #[gpui::test]
    async fn stale_visibility_heartbeat_suspends_playback(cx: &mut TestAppContext) {
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
    #[gpui::test]
    async fn video_loads_first_frame_paused_and_polls_only_while_playing(cx: &mut TestAppContext) {
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
            result: Mutex::new(Err(hkask_tool_invoker::InvokeError::Failed(
                "describe_image unavailable".to_string(),
            ))),
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
    async fn disagree_routes_through_injector(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mock = Arc::new(MockConversationInjector::default());
        cx.update(|cx| {
            hkask_conversation_injector::set_active_injector(cx, Some(mock.clone()));
        });

        let block = block_with_provenance("omc:CreativeWork", "generate_image", "a cat");
        let reference = block.to_media_ref().expect("resolves");
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget =
            cx.update(|_window, cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        let bodies = mock
            .bodies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert_eq!(bodies.len(), 1, "exactly one inject");
        assert!(bodies[0].contains("Re:"), "body references the revision");
        assert!(
            bodies[0].contains("omc:CreativeWork"),
            "body references the ontology concept"
        );
        assert!(
            bodies[0].contains("generate_image"),
            "body references the tool from provenance"
        );
        assert!(
            bodies[0].contains("a cat"),
            "body references the prompt hint from provenance args"
        );

        // A successful inject clears the fallback draft.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        assert!(draft.is_none(), "draft cleared after a successful inject");
    }

    #[gpui::test]
    async fn disagree_surfaces_draft_when_no_injector(cx: &mut TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Per-app global starts empty — no injector is wired by default.

        let block = block_with_provenance("omc:CreativeWork", "generate_image", "a cat");
        let reference = block.to_media_ref().expect("resolves");
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget =
            cx.update(|_window, cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        // No injector: the composed body is surfaced as a copyable draft
        // (visible, not a silent no-op — repo `.rules`), and no panic.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        let draft = draft.expect("draft surfaced when no injector is active");
        assert!(draft.contains("Re:"), "draft carries the revision prefix");
        assert!(draft.contains("omc:CreativeWork"));
    }

    #[gpui::test]
    async fn disagree_body_falls_back_when_provenance_absent(cx: &mut TestAppContext) {
        // grill-me edge case (b): absent provenance → generic "media result"
        // framing. `compose_disagree_body` is pure, so no window is needed.
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let block = block_without_provenance();
        let reference = block.to_media_ref().expect("resolves");
        let widget = cx.update(|cx| cx.new(|cx| MediaWidget::new_with_block(reference, block, cx)));
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("media result"),
            "absent OMC falls back to the generic framing"
        );
        assert!(
            body.contains("the media tool"),
            "absent provenance falls back to the generic tool label"
        );
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
            explain_tool_for(hkask_bridge_ontology::fibo::CORPORATION),
            "describe_image"
        );
        assert_eq!(explain_tool_for(""), "describe_image");
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use gpui::{TestAppContext, px, size};

    const FIXTURE: &str =
        "/home/mdz-axolotl/Documents/zk-data/media-mcp/generated/vonnegut-shape-of-stories.mp4";

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

    /// Layout ground truth: the video area's laid-out bounds must respond to
    /// the window size. This is the property that was FALSE in both shipped
    /// broken versions (scroll-wrapper collapse: constant ~120px; fraction-
    /// width img: constant natural 480px) while every unit test passed —
    /// the assertions here read GPUI's actual laid-out pixel bounds, not the
    /// element tree I wrote.
    #[gpui::test]
    fn video_area_scales_with_window_size(cx: &mut TestAppContext) {
        if !std::path::Path::new(FIXTURE).exists() {
            return;
        }
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
