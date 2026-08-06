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

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, ImageSource, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, RenderImage, SharedString, Styled, StyledImage,
    Subscription, Task, Window, div, img, px,
};
use gpui_util::ResultExt as _;
use hkask_bridge_ontology::omc::explain_tool_for;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use smallvec::SmallVec;
use theme::ActiveTheme;
use ui::prelude::*;

use crate::audio_player::AudioPlayer;
use crate::media_ref::{
    MediaBlockBody, MediaKind, MediaRef, MediaStorage, PathMediaStorage, ResolvedMedia,
};
use crate::transport::{TransportBar, TransportEvent, TransportState};
use crate::video_decoder::VideoPlayer;

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Server that hosts the media tools. Fallback dispatch target when a block
/// carries no dispatchable provenance.
const DEFAULT_SERVER: &str = "hkask-mcp-media";
/// Surfaced when the process-global `ToolInvoker` is not wired. Visible state,
/// not a silent no-op (repo `.rules` startup-failure-signal trap).
const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";

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
    // Single-flight guard for `load_audio_file_async`: storing the latest spawn
    // here drops (cancels) any prior in-flight read, so a stale larger read
    // cannot overwrite a newer smaller one, and dropping the widget cancels the
    // outstanding read (no wasteful I/O after drop). See M3.
    audio_load_task: Option<Task<()>>,
    error: Option<SharedString>,
    /// OMC concept tag from the parsed block body (e.g. `omc:CreativeWork`).
    /// Drives the "Explain" affordance's tool selection (the "I" pattern).
    /// `None` on older blocks → the widget falls back to `describe_image`.
    omc: Option<String>,
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
            omc = block.omc.as_deref().unwrap_or(""),
            "REG",
        );
        let mut widget = Self::with_storage(reference, Arc::new(PathMediaStorage), cx);
        widget.omc = block.omc;
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
            focus_handle,
            audio_player: None,
            video_player: None,
            transport: None,
            current_frame: None,
            playback_task: None,
            last_transport: None,
            audio_loading: false,
            audio_load_task: None,
            error: None,
            omc: None,
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
        self.audio_load_task = Some(cx.spawn(async move |this, cx| {
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
        }));
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

    /// Compose the provenance-scoped "I disagree" body. References the
    /// artifact's OMC concept and provenance (tool + args) so the agent can
    /// correlate the revision request to the exact media result the widget
    /// rendered. Falls back to a generic "the media result" framing when
    /// provenance or OMC is absent (grill-me edge case b).
    fn compose_disagree_body(&self) -> String {
        let tool = self.provenance.tool.as_deref().unwrap_or("the media tool");
        let omc_label = self.omc.as_deref().unwrap_or("media result");
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
                "Re: the {omc_label} generated by {tool} ({hint}). I believe this result is incorrect. Please re-check. My concern: "
            ),
            None => format!(
                "Re: the {omc_label} generated by {tool}. I believe this result is incorrect. Please re-check. My concern: "
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
        let omc_tag = self.omc.as_deref().unwrap_or("");
        let tool = explain_tool_for(omc_tag);
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
                        this.explain_error = Some(error);
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
        if let Some(injector) = hkask_conversation_injector::shared_injector() {
            // The production injector pre-fills the editor synchronously and
            // returns a `Task::ready(Ok(()))`; the returned `Result` is always
            // `Ok`. Await in a detached task so a hypothetical async impl's
            // error path is surfaced (not silently dropped — repo `.rules`),
            // and so `clippy::let_underscore_future` is not triggered.
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
            .children(self.render_affordances(cx))
    }
}

impl MediaWidget {
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

    /// Serializes tests that mutate the process-global `ToolInvoker` and
    /// `ConversationInjector` (separate globals, same racy-global trap — repo
    /// `.rules`). Without this lock, parallel tests observe each other's
    /// invoker/injector and intermittently fail with "invoker not wired" even
    /// when the test wired a mock.
    static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A `MockToolInvoker` whose calls and canned result are configurable.
    struct MockToolInvoker {
        calls: Mutex<Vec<(String, String, serde_json::Value)>>,
        result: Mutex<Result<String, String>>,
    }

    impl hkask_tool_invoker::ToolInvoker for MockToolInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> Task<Result<String, String>> {
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

    /// RAII guard that restores the conversation-injector global to `None`.
    struct ConversationInjectorGuard;
    impl Drop for ConversationInjectorGuard {
        fn drop(&mut self) {
            hkask_conversation_injector::set_active_injector(None);
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

    /// Build a `MediaBlockBody` carrying OMC + dispatchable provenance.
    fn block_with_provenance(omc: &str, tool: &str, prompt: &str) -> MediaBlockBody {
        MediaBlockBody {
            kind: "image".to_string(),
            src: "/tmp/img.png".to_string(),
            omc: Some(omc.to_string()),
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
            omc: None,
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
            result: Mutex::new(Err("describe_image unavailable".to_string())),
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
        let _restore = ConversationInjectorGuard;
        let mock = Arc::new(MockConversationInjector::default());
        hkask_conversation_injector::set_active_injector(Some(mock.clone()));

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
            "body references the OMC concept"
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
        let _restore = ConversationInjectorGuard;
        hkask_conversation_injector::set_active_injector(None);

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
        let _restore = ConversationInjectorGuard;

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
        // The "I" pattern: OMC concept drives the explain tool.
        // Uses the shared `hkask_bridge_ontology::omc::explain_tool_for`.
        assert_eq!(explain_tool_for("omc:Scene"), "gallery_analyze");
        assert_eq!(explain_tool_for("omc:Asset"), "gallery_analyze");
        assert_eq!(explain_tool_for("omc:CreativeWork"), "describe_image");
        assert_eq!(explain_tool_for("omc:Version"), "describe_image");
        assert_eq!(explain_tool_for("omc:MediaSource"), "describe_image");
        assert_eq!(explain_tool_for("omc:Sequence"), "describe_image");
        assert_eq!(explain_tool_for(""), "describe_image");
    }
}
