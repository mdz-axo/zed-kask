//! Video decoding and playback: FFmpeg decode → BGRA frames for GPUI, with
//! synchronized audio via rodio.
//!
//! The video path decodes the best video stream to **BGRA** (GPUI's
//! `RenderImage` upload format — its own asset loader converts RGBA→BGRA at
//! `img.rs`, so frames built directly for `RenderImage` must already be
//! BGRA; feeding RGBA swaps red and blue).
//!
//! The audio path owns a **separate FFmpeg input context**. For local files
//! and progressive stream URLs, that input is the same source as the video
//! (opened twice — FFmpeg handles multiple contexts on one source). For
//! DASH-only sources (most modern YouTube), the resolver hands back a
//! separate audio-only URL and the audio input opens that — a single
//! video-only URL would play silent video. Audio is decoded, resampled to
//! packed f32 stereo 48 kHz via libswresample, and queued on a rodio `Player`
//! as the playback clock advances, keeping the streams aligned.

use std::path::Path;
use std::time::Duration;

/// One decoded video frame in BGRA byte order, row-major, 4 bytes/pixel.
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub presentation_time: Duration,
}

/// Playback state for the transport bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Finished,
}

/// One decoder observation. End-of-stream is a normal terminal outcome, not
/// an error; callers use it to close the playback loop without warning.
enum DecodeOutcome {
    Frame(DecodedFrame),
    Pending,
    EndOfStream,
}

/// How far ahead of the playback clock audio is queued. Rodio consumes at
/// real time; a small lead absorbs demux jitter without growing unbounded.
const AUDIO_LEAD: Duration = Duration::from_millis(300);

pub struct VideoPlayer {
    state: PlaybackState,
    /// The playback clock base: the position at the last play/pause/seek
    /// transition. While Playing WITH audio, position = base + consumed
    /// audio (the audio-master clock — starvation-proof: if the audio queue
    /// drains, the clock freezes with it, so video waits for audio instead
    /// of the two drifting apart permanently). Without audio, position =
    /// base + wall time since `playing_since` (a wall clock drifts against
    /// a busy editor's late tick callbacks, but with no audio there is
    /// nothing to desync against).
    position_at_play: Duration,
    /// Consumed-audio time captured at the last play/seek transition — the
    /// zero point for the audio-master clock.
    #[cfg(feature = "video")]
    audio_consumed_at_play: Option<Duration>,
    playing_since: Option<std::time::Instant>,
    duration: Duration,
    #[cfg(feature = "video")]
    decoder: Option<VideoDecoderInner>,
}

impl VideoPlayer {
    /// Create a new video player.
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            playing_since: None,
            position_at_play: Duration::ZERO,
            #[cfg(feature = "video")]
            audio_consumed_at_play: None,
            duration: Duration::ZERO,
            #[cfg(feature = "video")]
            decoder: None,
        }
    }

    /// Open a local video file and decode its poster frame without playing.
    ///
    /// expect: Opening a video makes its first frame available while playback
    /// remains stopped.
    /// [P1] Motivating: the operator can inspect media before choosing Play.
    /// pre: `path` names a source with a decodable video stream.
    /// post: returns the first frame and leaves `is_playing() == false`.
    /// [P2] Constraining: opening never starts unsolicited audio.
    pub fn open(&mut self, path: &Path) -> anyhow::Result<DecodedFrame> {
        #[cfg(feature = "video")]
        {
            let mut decoder = VideoDecoderInner::open(path, None)?;
            let first_frame = match decoder.decode_frame_at(Duration::ZERO)? {
                DecodeOutcome::Frame(frame) => frame,
                DecodeOutcome::Pending => {
                    return Err(anyhow::anyhow!("video source produced no initial frame"));
                }
                DecodeOutcome::EndOfStream => {
                    return Err(anyhow::anyhow!("video source ended before its first frame"));
                }
            };
            self.duration = decoder.duration();
            self.decoder = Some(decoder);
            self.state = PlaybackState::Stopped;
            self.playing_since = None;
            self.position_at_play = Duration::ZERO;
            Ok(first_frame)
        }
        #[cfg(not(feature = "video"))]
        {
            let _ = path;
            Err(anyhow::anyhow!(
                "video decode is not enabled — rebuild with \
                 --features hkask-media-widget/video (system FFmpeg) or \
                 --features hkask-media-widget/vendored (compiled FFmpeg)"
            ))
        }
    }

    /// Open a remote stream for playback. `video_url` is the direct media
    /// URL (FFmpeg's http/https handlers stream it); `audio_url` carries the
    /// separate audio-only URL for DASH sources — `None` means the video URL
    /// already contains audio (progressive format or direct file).
    pub fn open_stream(
        &mut self,
        video_url: &str,
        audio_url: Option<&str>,
    ) -> anyhow::Result<DecodedFrame> {
        #[cfg(feature = "video")]
        {
            let mut decoder = VideoDecoderInner::open(
                std::path::Path::new(video_url),
                audio_url.map(std::path::Path::new),
            )?;
            let first_frame = match decoder.decode_frame_at(Duration::ZERO)? {
                DecodeOutcome::Frame(frame) => frame,
                DecodeOutcome::Pending => {
                    return Err(anyhow::anyhow!("video stream produced no initial frame"));
                }
                DecodeOutcome::EndOfStream => {
                    return Err(anyhow::anyhow!("video stream ended before its first frame"));
                }
            };
            self.duration = decoder.duration();
            self.decoder = Some(decoder);
            self.state = PlaybackState::Stopped;
            self.playing_since = None;
            self.position_at_play = Duration::ZERO;
            Ok(first_frame)
        }
        #[cfg(not(feature = "video"))]
        {
            let _ = (video_url, audio_url);
            Err(anyhow::anyhow!(
                "video decode is not enabled — rebuild with \
                 --features hkask-media-widget/video (system FFmpeg) or \
                 --features hkask-media-widget/vendored (compiled FFmpeg)"
            ))
        }
    }

    /// Whether the opened source carries an audio pipeline.
    #[must_use]
    pub fn has_audio(&self) -> bool {
        #[cfg(feature = "video")]
        {
            self.decoder
                .as_ref()
                .is_some_and(VideoDecoderInner::has_audio)
        }
        #[cfg(not(feature = "video"))]
        {
            false
        }
    }

    /// Number of audio sources queued on the output player. Test-only:
    /// asserts that pumping actually queues samples, not just that the
    /// pipeline exists.
    #[cfg(test)]
    #[must_use]
    pub fn audio_queue_len(&self) -> usize {
        #[cfg(feature = "video")]
        {
            self.decoder
                .as_ref()
                .and_then(VideoDecoderInner::audio_queue_len)
                .unwrap_or(0)
        }
        #[cfg(not(feature = "video"))]
        {
            0
        }
    }

    /// Start playback. Transitions from any state (including Stopped after
    /// `open`) to Playing — video clock and audio output together. The
    /// clock rebases: audio-master when audio is live, wall time otherwise.
    pub fn play(&mut self) {
        if self.state == PlaybackState::Finished {
            self.position_at_play = Duration::ZERO;
            #[cfg(feature = "video")]
            if let Some(decoder) = &mut self.decoder {
                decoder.reset_after_seek(Duration::ZERO, false);
            }
        }
        self.playing_since = Some(std::time::Instant::now());
        #[cfg(feature = "video")]
        {
            self.audio_consumed_at_play = self
                .decoder
                .as_ref()
                .and_then(VideoDecoderInner::audio_consumed);
        }
        self.state = PlaybackState::Playing;
        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                decoder.resume_audio();
            }
        }
    }

    /// Pause playback. Freezes the master clock and the audio output
    /// together.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.position_at_play = self.position();
            self.playing_since = None;
            self.state = PlaybackState::Paused;
            #[cfg(feature = "video")]
            {
                if let Some(decoder) = &mut self.decoder {
                    decoder.pause_audio();
                }
            }
        }
    }

    /// Stop playback and reset position.
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.playing_since = None;
        self.position_at_play = Duration::ZERO;
        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                decoder.reset_after_seek(Duration::ZERO, false);
            }
        }
    }

    /// Seek to a position. Rebases the clock and resets both streams; the
    /// audio queue is cleared so consumed audio restarts from zero — the
    /// audio-master clock rebases to the seek target. Audio resumes when
    /// the player was already Playing because rodio's `clear()` leaves its
    /// player paused.
    pub fn seek(&mut self, position: Duration) {
        if self.state == PlaybackState::Finished {
            self.state = PlaybackState::Paused;
        }
        self.position_at_play = position;
        if self.playing_since.is_some() {
            self.playing_since = Some(std::time::Instant::now());
        }
        #[cfg(feature = "video")]
        {
            let resume_audio = self.state == PlaybackState::Playing;
            if let Some(decoder) = &mut self.decoder {
                decoder.reset_after_seek(position, resume_audio);
            }
            // The queue was cleared — consumed audio restarts from zero.
            self.audio_consumed_at_play = Some(Duration::ZERO);
        }
    }

    /// Seek and decode the sought frame without changing whether playback is
    /// active. Used by the worker so paused scrubbing has immediate feedback.
    fn seek_and_decode(&mut self, position: Duration) -> anyhow::Result<Option<DecodedFrame>> {
        self.seek(position);
        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                return match decoder.decode_frame_at(position)? {
                    DecodeOutcome::Frame(frame) => Ok(Some(frame)),
                    DecodeOutcome::Pending | DecodeOutcome::EndOfStream => Ok(None),
                };
            }
        }
        Ok(None)
    }

    /// Get the current playback position — the master clock. With live
    /// audio this is consumed-audio-derived (starvation-proof); without
    /// audio it is wall-time-derived.
    #[must_use]
    pub fn position(&self) -> Duration {
        #[cfg(feature = "video")]
        if let Some(consumed_at_play) = self.audio_consumed_at_play
            && let Some(consumed) = self
                .decoder
                .as_ref()
                .and_then(VideoDecoderInner::audio_consumed)
        {
            return self.position_at_play + consumed.saturating_sub(consumed_at_play);
        }
        match self.playing_since {
            Some(since) => self.position_at_play + since.elapsed(),
            None => self.position_at_play,
        }
    }

    /// Get the total duration.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Get the playback state.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Whether playback is active.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    /// Decode the video frame for the current master-clock position and
    /// queue audio ahead of it.
    ///
    /// Called by the dedicated playback worker at ~30fps while playing.
    /// The `delta` argument is unused for the clock because position is
    /// audio-consumption-derived when audio exists and wall-time-derived
    /// otherwise. Returns a decoded BGRA frame for the widget.
    pub fn advance_and_decode(&mut self, _delta: Duration) -> anyhow::Result<Option<DecodedFrame>> {
        if self.state != PlaybackState::Playing {
            return Ok(None);
        }

        let position = self.position();

        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                decoder.pump_audio_until(position + AUDIO_LEAD)?;
                let outcome = if decoder.video_finished() {
                    DecodeOutcome::EndOfStream
                } else {
                    decoder.decode_frame_at(position)?
                };
                if matches!(outcome, DecodeOutcome::EndOfStream) && decoder.presentation_finished()
                {
                    self.position_at_play = if self.duration > Duration::ZERO {
                        self.duration
                    } else {
                        position
                    };
                    self.playing_since = None;
                    self.state = PlaybackState::Finished;
                    decoder.pause_audio();
                    return Ok(None);
                }
                return match outcome {
                    DecodeOutcome::Frame(frame) => Ok(Some(frame)),
                    DecodeOutcome::Pending | DecodeOutcome::EndOfStream => Ok(None),
                };
            }
        }

        Ok(None)
    }
}

impl Default for VideoPlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Non-blocking widget adapter. A dedicated thread owns the FFmpeg/rodio
/// engine; GPUI sends controls and drains already-produced updates.
pub(crate) struct WidgetVideoPlayer {
    commands: std::sync::mpsc::Sender<VideoCommand>,
    events: std::sync::mpsc::Receiver<VideoWorkerEvent>,
    frames: LatestFrameMailbox,
    state: PlaybackState,
    position: Duration,
    duration: Duration,
    pending_error: Option<String>,
    generation: u64,
    last_sequence: u64,
    worker_disconnected_reported: bool,
}

pub(crate) struct WidgetVideoPoll {
    pub frame: Option<DecodedFrame>,
    pub events: Vec<VideoPlaybackEvent>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VideoPlaybackEvent {
    Opened,
    Seeked,
    Completed,
    Failed(String),
}

#[cfg(test)]
impl WidgetVideoPoll {
    fn has_opened(&self) -> bool {
        self.events.contains(&VideoPlaybackEvent::Opened)
    }

    fn has_completed(&self) -> bool {
        self.events.contains(&VideoPlaybackEvent::Completed)
    }

    fn error(&self) -> Option<&str> {
        self.events.iter().find_map(|event| match event {
            VideoPlaybackEvent::Failed(error) => Some(error.as_str()),
            VideoPlaybackEvent::Opened
            | VideoPlaybackEvent::Seeked
            | VideoPlaybackEvent::Completed => None,
        })
    }
}

enum VideoCommand {
    OpenLocal {
        path: std::path::PathBuf,
        generation: u64,
    },
    OpenStream {
        video_url: String,
        audio_url: Option<String>,
        generation: u64,
    },
    Play,
    Pause,
    Stop {
        generation: u64,
    },
    Seek {
        position: Duration,
        generation: u64,
    },
    Shutdown,
}

enum VideoWorkerEvent {
    Opened {
        generation: u64,
        sequence: u64,
        snapshot: VideoSnapshot,
    },
    Seeked {
        generation: u64,
        sequence: u64,
        snapshot: VideoSnapshot,
    },
    Completed {
        generation: u64,
        sequence: u64,
        snapshot: VideoSnapshot,
    },
    Failed {
        generation: u64,
        sequence: u64,
        error: String,
    },
}

#[derive(Clone, Copy)]
struct VideoSnapshot {
    state: PlaybackState,
    position: Duration,
    duration: Duration,
}

impl VideoSnapshot {
    fn from_player(player: &VideoPlayer) -> Self {
        Self {
            state: player.state(),
            position: player.position(),
            duration: player.duration(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoDeliveryStats {
    pub published_frames: u64,
    pub replaced_frames: u64,
    pub consumed_frames: u64,
    pub pending_frames: usize,
    pub max_pending_frames: usize,
    pub last_consumed_pts_ms: Option<u64>,
    pub out_of_order_frames: u64,
}

#[derive(Clone, Default)]
struct LatestFrameMailbox {
    state: std::sync::Arc<parking_lot::Mutex<LatestFrameState>>,
}

#[derive(Default)]
struct LatestFrameState {
    frame: Option<DecodedFrame>,
    snapshot: Option<VideoSnapshot>,
    generation: u64,
    sequence: u64,
    stats: VideoDeliveryStats,
}

struct LatestFrameUpdate {
    frame: Option<DecodedFrame>,
    snapshot: VideoSnapshot,
    generation: u64,
    sequence: u64,
}

impl LatestFrameMailbox {
    /// expect: Publishing frames faster than the foreground consumes them keeps
    /// only the newest full frame while preserving the latest playback state.
    /// [P9] Motivating: delayed GPUI work cannot create unbounded frame memory.
    /// pre: `sequence` is monotonically increasing for one worker.
    /// post: pending frame count is at most one; a newer frame replaces an older one.
    /// [P1] Constraining: replacement never duplicates a frame.
    fn publish(
        &self,
        frame: Option<DecodedFrame>,
        snapshot: VideoSnapshot,
        generation: u64,
        sequence: u64,
    ) {
        let mut state = self.state.lock();
        if generation != state.generation {
            return;
        }
        state.snapshot = Some(snapshot);
        state.sequence = sequence;
        if let Some(frame) = frame {
            state.stats.published_frames = state.stats.published_frames.saturating_add(1);
            if state.frame.replace(frame).is_some() {
                state.stats.replaced_frames = state.stats.replaced_frames.saturating_add(1);
            }
            state.stats.pending_frames = 1;
            state.stats.max_pending_frames = state.stats.max_pending_frames.max(1);
        }
    }

    fn take(&self) -> Option<LatestFrameUpdate> {
        let mut state = self.state.lock();
        let snapshot = state.snapshot.take()?;
        let frame = state.frame.take();
        if frame.is_some() {
            state.stats.pending_frames = 0;
        }
        Some(LatestFrameUpdate {
            frame,
            snapshot,
            generation: state.generation,
            sequence: state.sequence,
        })
    }

    fn record_consumed(&self, frame: &DecodedFrame) {
        let mut state = self.state.lock();
        let pts_ms = frame.presentation_time.as_millis() as u64;
        if state
            .stats
            .last_consumed_pts_ms
            .is_some_and(|last_pts_ms| pts_ms <= last_pts_ms)
        {
            state.stats.out_of_order_frames = state.stats.out_of_order_frames.saturating_add(1);
        }
        state.stats.last_consumed_pts_ms = Some(pts_ms);
        state.stats.consumed_frames = state.stats.consumed_frames.saturating_add(1);
    }

    fn invalidate(&self, generation: u64) {
        let mut state = self.state.lock();
        state.frame = None;
        state.snapshot = None;
        state.generation = generation;
        state.stats.pending_frames = 0;
    }

    #[cfg(any(test, feature = "bench-support"))]
    fn stats(&self) -> VideoDeliveryStats {
        self.state.lock().stats
    }
}

impl WidgetVideoPlayer {
    pub fn new() -> anyhow::Result<Self> {
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let frames = LatestFrameMailbox::default();
        let worker_frames = frames.clone();
        std::thread::Builder::new()
            .name("hkask-video-playback".to_string())
            .spawn(move || run_video_worker(command_rx, event_tx, worker_frames))
            .map_err(|error| anyhow::anyhow!("failed to start video playback worker: {error}"))?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
            frames,
            state: PlaybackState::Stopped,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            pending_error: None,
            generation: 0,
            last_sequence: 0,
            worker_disconnected_reported: false,
        })
    }

    pub fn open(&mut self, path: &Path) {
        let generation = self.begin_generation();
        self.reset_for_open();
        self.send(VideoCommand::OpenLocal {
            path: path.to_path_buf(),
            generation,
        });
    }

    pub fn open_stream(&mut self, video_url: &str, audio_url: Option<&str>) {
        let generation = self.begin_generation();
        self.reset_for_open();
        self.send(VideoCommand::OpenStream {
            video_url: video_url.to_string(),
            audio_url: audio_url.map(str::to_string),
            generation,
        });
    }

    pub fn play(&mut self) {
        self.state = PlaybackState::Playing;
        self.send(VideoCommand::Play);
    }

    pub fn pause(&mut self) {
        self.state = PlaybackState::Paused;
        self.send(VideoCommand::Pause);
    }

    pub fn stop(&mut self) {
        let generation = self.begin_generation();
        self.state = PlaybackState::Stopped;
        self.position = Duration::ZERO;
        self.send(VideoCommand::Stop { generation });
    }

    pub fn seek(&mut self, position: Duration) {
        let generation = self.begin_generation();
        self.position = position;
        if self.state == PlaybackState::Finished {
            self.state = PlaybackState::Paused;
        }
        self.send(VideoCommand::Seek {
            position,
            generation,
        });
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    #[must_use]
    pub fn position(&self) -> Duration {
        self.position
    }

    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn poll(&mut self) -> WidgetVideoPoll {
        let mut poll = WidgetVideoPoll {
            frame: None,
            events: self
                .pending_error
                .take()
                .map(VideoPlaybackEvent::Failed)
                .into_iter()
                .collect(),
        };

        if let Some(update) = self.frames.take()
            && update.generation == self.generation
            && update.sequence >= self.last_sequence
        {
            self.last_sequence = update.sequence;
            self.apply_snapshot(update.snapshot);
            if let Some(frame) = update.frame {
                self.frames.record_consumed(&frame);
                poll.frame = Some(frame);
            }
        }

        loop {
            match self.events.try_recv() {
                Ok(VideoWorkerEvent::Opened {
                    generation,
                    sequence,
                    snapshot,
                }) => {
                    if generation == self.generation {
                        poll.events.push(VideoPlaybackEvent::Opened);
                        if sequence >= self.last_sequence {
                            self.last_sequence = sequence;
                            self.apply_snapshot(snapshot);
                        }
                    }
                }
                Ok(VideoWorkerEvent::Seeked {
                    generation,
                    sequence,
                    snapshot,
                }) => {
                    if generation == self.generation {
                        poll.events.push(VideoPlaybackEvent::Seeked);
                        if sequence >= self.last_sequence {
                            self.last_sequence = sequence;
                            self.apply_snapshot(snapshot);
                        }
                    }
                }
                Ok(VideoWorkerEvent::Completed {
                    generation,
                    sequence,
                    snapshot,
                }) => {
                    if generation == self.generation {
                        poll.events.push(VideoPlaybackEvent::Completed);
                        if sequence >= self.last_sequence {
                            self.last_sequence = sequence;
                            self.apply_snapshot(snapshot);
                        }
                    }
                }
                Ok(VideoWorkerEvent::Failed {
                    generation,
                    sequence,
                    error,
                }) => {
                    if generation == self.generation {
                        poll.events.push(VideoPlaybackEvent::Failed(error));
                        if sequence >= self.last_sequence {
                            self.last_sequence = sequence;
                            self.state = PlaybackState::Stopped;
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if !self.worker_disconnected_reported {
                        self.worker_disconnected_reported = true;
                        self.state = PlaybackState::Stopped;
                        poll.events.push(VideoPlaybackEvent::Failed(
                            "video playback worker stopped unexpectedly".to_string(),
                        ));
                    }
                    break;
                }
            }
        }
        poll
    }

    #[cfg(any(test, feature = "bench-support"))]
    #[must_use]
    pub fn delivery_stats(&self) -> VideoDeliveryStats {
        self.frames.stats()
    }

    #[cfg(feature = "bench-support")]
    #[must_use]
    pub fn playback_state(&self) -> PlaybackState {
        self.state
    }

    fn begin_generation(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.frames.invalidate(self.generation);
        self.generation
    }

    fn reset_for_open(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position = Duration::ZERO;
        self.duration = Duration::ZERO;
        self.pending_error = None;
    }

    fn apply_snapshot(&mut self, snapshot: VideoSnapshot) {
        self.state = snapshot.state;
        self.position = snapshot.position;
        self.duration = snapshot.duration;
    }

    fn send(&mut self, command: VideoCommand) {
        if self.commands.send(command).is_err() {
            self.state = PlaybackState::Stopped;
            self.worker_disconnected_reported = true;
            self.pending_error = Some("video playback worker stopped unexpectedly".to_string());
        }
    }
}

impl Drop for WidgetVideoPlayer {
    fn drop(&mut self) {
        if self.commands.send(VideoCommand::Shutdown).is_err() {
            log::debug!("hkask-media-widget: video worker already stopped");
        }
    }
}

fn run_video_worker(
    commands: std::sync::mpsc::Receiver<VideoCommand>,
    events: std::sync::mpsc::Sender<VideoWorkerEvent>,
    frames: LatestFrameMailbox,
) {
    let mut player = VideoPlayer::new();
    let mut generation = 0_u64;
    let mut sequence = 0_u64;
    loop {
        let command = if player.is_playing() {
            match commands.recv_timeout(Duration::from_millis(33)) {
                Ok(command) => Some(command),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };

        match command {
            Some(VideoCommand::OpenLocal {
                path,
                generation: next_generation,
            }) => {
                generation = next_generation;
                player = VideoPlayer::new();
                match player.open(&path) {
                    Ok(frame) => {
                        sequence = sequence.saturating_add(1);
                        let snapshot = VideoSnapshot::from_player(&player);
                        frames.publish(Some(frame), snapshot, generation, sequence);
                        if events
                            .send(VideoWorkerEvent::Opened {
                                generation,
                                sequence,
                                snapshot,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        sequence = sequence.saturating_add(1);
                        if events
                            .send(VideoWorkerEvent::Failed {
                                generation,
                                sequence,
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Some(VideoCommand::OpenStream {
                video_url,
                audio_url,
                generation: next_generation,
            }) => {
                generation = next_generation;
                player = VideoPlayer::new();
                match player.open_stream(&video_url, audio_url.as_deref()) {
                    Ok(frame) => {
                        sequence = sequence.saturating_add(1);
                        let snapshot = VideoSnapshot::from_player(&player);
                        frames.publish(Some(frame), snapshot, generation, sequence);
                        if events
                            .send(VideoWorkerEvent::Opened {
                                generation,
                                sequence,
                                snapshot,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        sequence = sequence.saturating_add(1);
                        if events
                            .send(VideoWorkerEvent::Failed {
                                generation,
                                sequence,
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Some(VideoCommand::Play) => player.play(),
            Some(VideoCommand::Pause) => player.pause(),
            Some(VideoCommand::Stop {
                generation: next_generation,
            }) => {
                generation = next_generation;
                player.stop();
            }
            Some(VideoCommand::Seek {
                position,
                generation: next_generation,
            }) => {
                generation = next_generation;
                match player.seek_and_decode(position) {
                    Ok(frame) => {
                        sequence = sequence.saturating_add(1);
                        let snapshot = VideoSnapshot::from_player(&player);
                        frames.publish(frame, snapshot, generation, sequence);
                        if events
                            .send(VideoWorkerEvent::Seeked {
                                generation,
                                sequence,
                                snapshot,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        player.pause();
                        sequence = sequence.saturating_add(1);
                        if events
                            .send(VideoWorkerEvent::Failed {
                                generation,
                                sequence,
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Some(VideoCommand::Shutdown) => break,
            None => match player.advance_and_decode(Duration::from_millis(33)) {
                Ok(frame) => {
                    sequence = sequence.saturating_add(1);
                    let snapshot = VideoSnapshot::from_player(&player);
                    if snapshot.state == PlaybackState::Finished {
                        if events
                            .send(VideoWorkerEvent::Completed {
                                generation,
                                sequence,
                                snapshot,
                            })
                            .is_err()
                        {
                            break;
                        }
                    } else {
                        frames.publish(frame, snapshot, generation, sequence);
                    }
                }
                Err(error) => {
                    player.pause();
                    sequence = sequence.saturating_add(1);
                    if events
                        .send(VideoWorkerEvent::Failed {
                            generation,
                            sequence,
                            error: error.to_string(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            },
        }
    }
}

// ── FFmpeg-backed implementation ──────────────────────────────────────────

#[cfg(feature = "video")]
mod ffmpeg_impl {
    use super::{DecodeOutcome, DecodedFrame};
    use std::path::Path;
    use std::time::Duration;

    use ffmpeg::Rescale;
    use ffmpeg_next as ffmpeg;

    /// The audio output format every stream is resampled to: packed f32,
    /// stereo, 48 kHz — a fixed format keeps the rodio `SamplesBuffer`
    /// construction trivial.
    const AUDIO_RATE: u32 = 48_000;

    /// Samples buffered before one rodio append: 100ms of stereo f32. See
    /// `AudioPipeline::pending_samples` for why appends are chunked. The
    /// chunk size must exceed the tick gap (~33ms): the pending buffer only
    /// flushes during pump calls, so a queue chunk smaller than the gap
    /// starves the output between ticks (audible stutter; measurable
    /// starvation in the streaming e2e test).
    const APPEND_CHUNK_SAMPLES: usize = (AUDIO_RATE as usize) / 10 * 2;

    /// FFmpeg-backed video + audio decoder.
    ///
    /// Video: decodes any format FFmpeg supports (MP4, WebM, MKV, AV1, H.264,
    /// HEVC, etc.) to BGRA via `libswscale` (adapted from the proven pattern
    /// in Oxide browser's `video.rs` (Apache-2.0) and `iced_video_player`
    /// (MIT)).
    ///
    /// Audio: a separate FFmpeg input context (the same source for local
    /// files and progressive URLs; the resolver's audio-only URL for DASH
    /// sources), decoded and resampled to packed f32 stereo 48 kHz via
    /// libswresample, queued on a rodio `Player` as the playback clock
    /// advances. The `MixerDeviceSink` is stored to keep the audio device
    /// alive for the decoder's lifetime.
    pub struct VideoDecoderInner {
        input: ffmpeg::format::context::Input,
        video_stream_index: usize,
        decoder: ffmpeg::decoder::Video,
        scaler: ffmpeg::software::scaling::Context,
        time_base: ffmpeg::Rational,
        duration_ms: u64,
        video_input_eof: bool,
        video_decoder_eof: bool,
        video_pts_origin_ms: Option<u64>,
        pending_video_frame: Option<TimedVideoFrame>,
        audio: Option<AudioPipeline>,
    }

    struct TimedVideoFrame {
        pts_ms: u64,
        frame: DecodedFrame,
    }

    /// The audio half of the decoder — its own demuxer, decoder, resampler,
    /// and output device.
    struct AudioPipeline {
        input: ffmpeg::format::context::Input,
        stream_index: usize,
        time_base: ffmpeg::Rational,
        decoder: ffmpeg::decoder::Audio,
        resampler: ffmpeg::software::resampling::Context,
        // Kept alive: dropping the device sink closes the audio output.
        _device_sink: rodio::MixerDeviceSink,
        player: rodio::Player,
        /// Playback-ms covered by the samples queued so far; pumping stops
        /// once this passes the target.
        queued_until_ms: u64,
        /// After a seek, demuxers may resume at an earlier keyframe. Audio
        /// frames before this media timestamp are discarded instead of replayed.
        discard_before_ms: u64,
        /// Samples buffered toward the next player append. Appending one rodio
        /// source per resampled frame (~20ms) would queue dozens of
        /// tiny sources, and rodio's `clear()` blocks ~5ms per queued source
        /// — a seek would stall the playback worker. Buffering to
        /// `APPEND_CHUNK_SAMPLES` keeps the queue at a couple of sources.
        pending_samples: Vec<f32>,
        input_eof: bool,
        decoder_eof: bool,
        /// Media-time duration of each appended chunk, in append order. With
        /// rodio's `len()` (sources still queued) and `get_pos()` (position in
        /// the current source), this yields the consumed-audio time — the
        /// MASTER CLOCK. Wall time cannot serve: tick callbacks run late in a
        /// busy editor, every starvation drains the queue, and consumed audio
        /// falls permanently behind wall time (accumulating desync).
        appended_durations: std::collections::VecDeque<Duration>,
    }

    impl VideoDecoderInner {
        /// Open a video source (local path or direct stream URL) plus an
        /// optional separate audio source. `audio_source` is `Some` for
        /// DASH streams where the video URL carries no audio; `None` opens
        /// audio from the same source as the video.
        pub fn open(video_source: &Path, audio_source: Option<&Path>) -> anyhow::Result<Self> {
            ffmpeg::init()
                .map_err(|error| anyhow::anyhow!("failed to initialize FFmpeg: {error}"))?;

            let input = ffmpeg::format::input(video_source)
                .map_err(|error| anyhow::anyhow!("failed to open video source: {error}"))?;

            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or_else(|| anyhow::anyhow!("no video stream found"))?;
            let video_stream_index = stream.index();
            let time_base = stream.time_base();

            let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|error| anyhow::anyhow!("failed to create codec context: {error}"))?;
            let decoder = context
                .decoder()
                .video()
                .map_err(|error| anyhow::anyhow!("failed to open video decoder: {error}"))?;

            let width = decoder.width();
            let height = decoder.height();

            // BGRA, not RGBA: GPUI's RenderImage upload path expects BGRA (its
            // own asset loader converts RGBA→BGRA before building frames —
            // see the swap in gpui's img.rs). Feeding RGBA swaps red/blue.
            let scaler = ffmpeg::software::scaling::context::Context::get(
                decoder.format(),
                width,
                height,
                ffmpeg::format::Pixel::BGRA,
                width,
                height,
                ffmpeg::software::scaling::flag::Flags::BILINEAR,
            )
            .map_err(|error| anyhow::anyhow!("failed to create scaler: {error}"))?;

            let duration_ms = if input.duration() > 0 {
                input.duration().rescale(
                    ffmpeg::Rational::new(1, ffmpeg::ffi::AV_TIME_BASE),
                    (1, 1000),
                ) as u64
            } else {
                0
            };

            let audio_source_was_explicit = audio_source.is_some();
            let audio_source = audio_source.unwrap_or(video_source);
            // An explicit audio URL that fails to open is an ERROR, not a
            // silent downgrade: DASH sources carry no audio in the video
            // URL, so a failed audio open means the requirement (sound) is
            // unmet. Only an implicit same-source audio lookup may degrade
            // to silent (the source may genuinely have no audio stream).
            let audio = match AudioPipeline::open(audio_source) {
                Ok(audio) => Some(audio),
                Err(error) => {
                    if audio_source_was_explicit {
                        return Err(anyhow::anyhow!(
                            "failed to open audio stream {}: {error} — \
                             refusing to play silent video from a DASH source",
                            audio_source.display()
                        ));
                    }
                    log::warn!(
                        "hkask-media-widget: audio pipeline unavailable for {}: {error} \
                         — playing video without sound",
                        audio_source.display()
                    );
                    None
                }
            };

            Ok(Self {
                input,
                video_stream_index,
                decoder,
                scaler,
                time_base,
                duration_ms,
                video_input_eof: false,
                video_decoder_eof: false,
                video_pts_origin_ms: None,
                pending_video_frame: None,
                audio,
            })
        }

        #[must_use]
        pub fn has_audio(&self) -> bool {
            self.audio.is_some()
        }

        /// Consumed audio time — the master clock while audio is live.
        #[must_use]
        pub fn audio_consumed(&self) -> Option<Duration> {
            self.audio.as_ref().map(AudioPipeline::consumed)
        }

        #[cfg(test)]
        #[must_use]
        pub fn audio_queue_len(&self) -> Option<usize> {
            self.audio.as_ref().map(|audio| audio.player.len())
        }

        pub fn duration(&self) -> Duration {
            Duration::from_millis(self.duration_ms)
        }

        /// Reset both streams after a seek (or stop): demuxer seeks, decoder
        /// flushes, a fresh resampler (libswresample carries buffered state
        /// across seeks otherwise), and drop any queued audio. `resume_audio`
        /// re-starts the audio output when the player was Playing — rodio's
        /// `clear()` leaves its player paused, so skipping the resume would
        /// silence audio after every seek.
        pub fn reset_after_seek(&mut self, target: Duration, resume_audio: bool) {
            let timestamp_us = target.as_micros() as i64;
            if let Err(error) = self.input.seek(timestamp_us, ..) {
                log::warn!("video seek to {target:?} failed: {error}");
            }
            self.decoder.flush();
            self.video_input_eof = false;
            self.video_decoder_eof = false;
            self.pending_video_frame = None;
            if let Some(audio) = &mut self.audio {
                audio.reset_after_seek(target);
                if resume_audio {
                    audio.player.play();
                }
            }
        }

        pub fn pause_audio(&mut self) {
            if let Some(audio) = &mut self.audio {
                audio.player.pause();
            }
        }

        pub fn resume_audio(&mut self) {
            if let Some(audio) = &mut self.audio {
                audio.player.play();
            }
        }

        /// Queue audio up to `target` on the playback clock.
        pub fn pump_audio_until(&mut self, target: Duration) -> anyhow::Result<()> {
            if let Some(audio) = &mut self.audio {
                audio.pump_until(target)?;
            }
            Ok(())
        }

        #[must_use]
        pub fn video_finished(&self) -> bool {
            self.video_decoder_eof
        }

        #[must_use]
        pub fn presentation_finished(&self) -> bool {
            self.video_decoder_eof && self.audio.as_ref().is_none_or(AudioPipeline::is_finished)
        }

        /// Decode the video frame closest to `target` time.
        ///
        /// Decoder output is always drained before another packet is sent.
        /// Packet EOF is flushed through the decoder so delayed frames remain
        /// visible; temporary `EAGAIN` is a pending observation, not failure.
        pub fn decode_frame_at(&mut self, target: Duration) -> anyhow::Result<DecodeOutcome> {
            let target_ms = target.as_millis() as u64;
            let mut best_frame: Option<DecodedFrame> = None;

            if let Some(pending) = self.pending_video_frame.take() {
                if pending.pts_ms > target_ms {
                    self.pending_video_frame = Some(pending);
                    return Ok(DecodeOutcome::Pending);
                }
                best_frame = Some(pending.frame);
            }

            loop {
                let mut decoded = ffmpeg::util::frame::video::Video::empty();
                loop {
                    match self.decoder.receive_frame(&mut decoded) {
                        Ok(()) => {
                            let pts = decoded.timestamp().or_else(|| decoded.pts()).unwrap_or(0);
                            if pts < 0 {
                                continue;
                            }
                            let source_frame_ms =
                                pts.rescale(self.time_base, (1, 1000)).max(0) as u64;
                            let origin_ms =
                                *self.video_pts_origin_ms.get_or_insert(source_frame_ms);
                            let frame_ms = source_frame_ms.saturating_sub(origin_ms);
                            let frame =
                                self.scale_frame(&decoded, Duration::from_millis(frame_ms))?;
                            if frame_ms > target_ms {
                                self.pending_video_frame = Some(TimedVideoFrame {
                                    pts_ms: frame_ms,
                                    frame,
                                });
                                return Ok(match best_frame {
                                    Some(frame) => DecodeOutcome::Frame(frame),
                                    None => DecodeOutcome::Pending,
                                });
                            }
                            best_frame = Some(frame);
                        }
                        Err(ffmpeg::Error::Eof) => {
                            self.video_decoder_eof = true;
                            return Ok(match best_frame {
                                Some(frame) => DecodeOutcome::Frame(frame),
                                None => DecodeOutcome::EndOfStream,
                            });
                        }
                        Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                            break;
                        }
                        Err(error) => {
                            return Err(anyhow::anyhow!("video receive error: {error}"));
                        }
                    }
                }

                if self.video_decoder_eof {
                    return Ok(match best_frame {
                        Some(frame) => DecodeOutcome::Frame(frame),
                        None => DecodeOutcome::EndOfStream,
                    });
                }
                if self.video_input_eof {
                    return Ok(match best_frame {
                        Some(frame) => DecodeOutcome::Frame(frame),
                        None => DecodeOutcome::Pending,
                    });
                }

                let mut packet = ffmpeg::Packet::empty();
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() != self.video_stream_index => continue,
                    Ok(()) => {
                        self.decoder
                            .send_packet(&packet)
                            .map_err(|error| anyhow::anyhow!("video send error: {error}"))?;
                    }
                    Err(ffmpeg::Error::Eof) => {
                        self.video_input_eof = true;
                        self.decoder
                            .send_eof()
                            .map_err(|error| anyhow::anyhow!("video EOF flush error: {error}"))?;
                    }
                    Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                        return Ok(match best_frame {
                            Some(frame) => DecodeOutcome::Frame(frame),
                            None => DecodeOutcome::Pending,
                        });
                    }
                    Err(error) => {
                        return Err(anyhow::anyhow!("video packet read error: {error}"));
                    }
                }
            }
        }

        fn scale_frame(
            &mut self,
            decoded: &ffmpeg::util::frame::video::Video,
            presentation_time: Duration,
        ) -> anyhow::Result<DecodedFrame> {
            let mut bgra_frame = ffmpeg::util::frame::video::Video::empty();
            self.scaler
                .run(decoded, &mut bgra_frame)
                .map_err(|error| anyhow::anyhow!("scale error: {error}"))?;

            let width = bgra_frame.width();
            let height = bgra_frame.height();
            let stride = bgra_frame.stride(0);
            let mut bgra = Vec::with_capacity((width * height * 4) as usize);
            for row in 0..height as usize {
                let start = row * stride;
                let end = start + (width as usize * 4);
                bgra.extend_from_slice(&bgra_frame.data(0)[start..end]);
            }
            Ok(DecodedFrame {
                width,
                height,
                bgra,
                presentation_time,
            })
        }
    }

    impl AudioPipeline {
        /// Open the audio source and set up decode → resample → output.
        /// Errors when the source has no audio stream (the caller decides
        /// whether that is fatal — for the video decoder it is not).
        fn open(source: &Path) -> anyhow::Result<Self> {
            let input = ffmpeg::format::input(source)
                .map_err(|error| anyhow::anyhow!("failed to open audio source: {error}"))?;

            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Audio)
                .ok_or_else(|| anyhow::anyhow!("no audio stream found"))?;
            let stream_index = stream.index();
            let time_base = stream.time_base();

            let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|error| {
                    anyhow::anyhow!("failed to create audio codec context: {error}")
                })?;
            let decoder = context
                .decoder()
                .audio()
                .map_err(|error| anyhow::anyhow!("failed to open audio decoder: {error}"))?;

            let resampler = Self::build_resampler(&decoder)
                .map_err(|error| anyhow::anyhow!("failed to create audio resampler: {error}"))?;

            let mut device_sink = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|error| anyhow::anyhow!("failed to open audio output stream: {error}"))?;
            device_sink.log_on_drop(false);
            let mixer = device_sink.mixer();
            let player = rodio::Player::connect_new(mixer);
            // Start paused: audio must not sound until the operator presses
            // Play.
            player.pause();

            Ok(Self {
                input,
                stream_index,
                time_base,
                decoder,
                resampler,
                _device_sink: device_sink,
                player,
                queued_until_ms: 0,
                discard_before_ms: 0,
                pending_samples: Vec::new(),
                input_eof: false,
                decoder_eof: false,
                appended_durations: std::collections::VecDeque::new(),
            })
        }

        fn build_resampler(
            decoder: &ffmpeg::decoder::Audio,
        ) -> Result<ffmpeg::software::resampling::Context, ffmpeg::Error> {
            ffmpeg::software::resampling::Context::get(
                decoder.format(),
                ffmpeg::util::channel_layout::ChannelLayout::default(
                    decoder.channels().max(1) as i32
                ),
                decoder.rate(),
                ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                ffmpeg::util::channel_layout::ChannelLayout::STEREO,
                AUDIO_RATE,
            )
        }

        /// Consumed audio time: completed chunks (appended minus still
        /// queued) plus the position within the currently-playing chunk.
        /// This is the master playback clock while audio is live.
        fn consumed(&self) -> Duration {
            let queued_sources = self.player.len();
            let completed = self.appended_durations.len().saturating_sub(queued_sources);
            let mut consumed: Duration = self.appended_durations.iter().take(completed).sum();
            if queued_sources > 0 {
                consumed += self.player.get_pos();
            }
            consumed
        }

        fn reset_after_seek(&mut self, target: Duration) {
            let timestamp_us = target.as_micros() as i64;
            if let Err(error) = self.input.seek(timestamp_us, ..) {
                log::warn!("audio seek to {target:?} failed: {error}");
            }
            self.decoder.flush();
            self.player.clear();
            self.queued_until_ms = target.as_millis() as u64;
            self.discard_before_ms = target.as_millis() as u64;
            self.pending_samples.clear();
            self.input_eof = false;
            self.decoder_eof = false;
            self.appended_durations.clear();
            // Rebuild the resampler from the decoder's (unchanged)
            // parameters — swr has no reset, and stale buffered samples
            // from before the seek would play as a glitch.
            if let Ok(resampler) = Self::build_resampler(&self.decoder) {
                self.resampler = resampler;
            }
        }

        /// Demux, decode, resample, and queue audio until the queued samples
        /// cover `target` on the playback clock. EOF flushes both the decoder
        /// and the final partial sample chunk.
        fn pump_until(&mut self, target: Duration) -> anyhow::Result<()> {
            let target_ms = target.as_millis() as u64;
            if self.decoder_eof || self.queued_until_ms >= target_ms {
                return Ok(());
            }

            loop {
                let mut decoded = ffmpeg::frame::Audio::empty();
                loop {
                    match self.decoder.receive_frame(&mut decoded) {
                        Ok(()) => self.queue_decoded_frame(&decoded)?,
                        Err(ffmpeg::Error::Eof) => {
                            self.decoder_eof = true;
                            flush_pending_samples(
                                &mut self.pending_samples,
                                &mut self.appended_durations,
                                &self.player,
                            );
                            return Ok(());
                        }
                        Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                            break;
                        }
                        Err(error) => {
                            return Err(anyhow::anyhow!("audio receive error: {error}"));
                        }
                    }
                }

                if self.queued_until_ms >= target_ms {
                    return Ok(());
                }
                if self.input_eof {
                    return Ok(());
                }

                let mut packet = ffmpeg::Packet::empty();
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() != self.stream_index => continue,
                    Ok(()) => self
                        .decoder
                        .send_packet(&packet)
                        .map_err(|error| anyhow::anyhow!("audio send error: {error}"))?,
                    Err(ffmpeg::Error::Eof) => {
                        self.input_eof = true;
                        self.decoder
                            .send_eof()
                            .map_err(|error| anyhow::anyhow!("audio EOF flush error: {error}"))?;
                    }
                    Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                        return Ok(());
                    }
                    Err(error) => {
                        return Err(anyhow::anyhow!("audio packet read error: {error}"));
                    }
                }
            }
        }

        fn queue_decoded_frame(&mut self, decoded: &ffmpeg::frame::Audio) -> anyhow::Result<()> {
            let pts = decoded.timestamp().or_else(|| decoded.pts()).unwrap_or(0);
            if pts >= 0 {
                let frame_ms = pts.rescale(self.time_base, (1, 1000)).max(0) as u64;
                self.queued_until_ms = self.queued_until_ms.max(frame_ms);
                if frame_ms < self.discard_before_ms {
                    return Ok(());
                }
                self.discard_before_ms = 0;
            }

            let mut resampled = ffmpeg::frame::Audio::empty();
            let mut delay = self
                .resampler
                .run(decoded, &mut resampled)
                .map_err(|error| anyhow::anyhow!("audio resample error: {error}"))?;
            append_frame(
                &mut self.pending_samples,
                &mut self.appended_durations,
                &self.player,
                &resampled,
            );

            let mut guard = 0;
            while delay.is_some() && guard < 64 {
                let mut drained = ffmpeg::frame::Audio::empty();
                delay = self
                    .resampler
                    .flush(&mut drained)
                    .map_err(|error| anyhow::anyhow!("audio resample flush error: {error}"))?;
                append_frame(
                    &mut self.pending_samples,
                    &mut self.appended_durations,
                    &self.player,
                    &drained,
                );
                guard += 1;
            }
            Ok(())
        }

        fn is_finished(&self) -> bool {
            self.decoder_eof && self.pending_samples.is_empty() && self.player.empty()
        }
    }

    /// Buffer one resampled audio frame toward the next player append,
    /// flushing the buffer as a single rodio source once it holds
    /// `APPEND_CHUNK_SAMPLES`. A free function taking the disjoint fields
    /// so the demux loop can hold `&mut input` while appending — an `&mut
    /// self` method would conflict with the iterator's borrow.
    fn append_frame(
        pending_samples: &mut Vec<f32>,
        appended_durations: &mut std::collections::VecDeque<Duration>,
        player: &rodio::Player,
        frame: &ffmpeg::frame::Audio,
    ) {
        if frame.samples() == 0 {
            return;
        }
        // Packed f32: one plane of interleaved stereo samples.
        let bytes = frame.data(0);
        for chunk in bytes.chunks_exact(4) {
            pending_samples.push(f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        if pending_samples.len() >= APPEND_CHUNK_SAMPLES || player.len() == 0 {
            // Also flush when no source is queued: `Player::empty()` remains
            // true until playback starts and would split a fast decode burst
            // into dozens of tiny sources.
            flush_pending_samples(pending_samples, appended_durations, player);
        }
    }

    fn flush_pending_samples(
        pending_samples: &mut Vec<f32>,
        appended_durations: &mut std::collections::VecDeque<Duration>,
        player: &rodio::Player,
    ) {
        if pending_samples.is_empty() {
            return;
        }
        let samples = std::mem::take(pending_samples);
        let duration = Duration::from_secs_f64(samples.len() as f64 / 2.0 / AUDIO_RATE as f64);
        appended_durations.push_back(duration);
        player.append(rodio::buffer::SamplesBuffer::new(
            std::num::NonZero::new(2).expect("nonzero channel count"),
            std::num::NonZero::new(AUDIO_RATE).expect("nonzero sample rate"),
            samples,
        ));
    }
}

#[cfg(feature = "video")]
use ffmpeg_impl::VideoDecoderInner;

// A vendored build that does not enable `video` compiles the stub decoder —
// every `open()` then fails with "video decode is not enabled" while FFmpeg
// is still compiled in (wasted build time, silently broken playback). This
// shipped once: `vendored` listed the ffmpeg deps but not `video`.
#[cfg(all(feature = "vendored", not(feature = "video")))]
compile_error!("the `vendored` feature must enable `video` (see [features] in Cargo.toml)");

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The vonnegut fixture from the media panel session. Skips silently
    /// when absent (other machines) — the assertions it carries are for the
    /// machine that has the file.
    const FIXTURE: &str =
        "/home/mdz-axolotl/Documents/zk-data/media-mcp/generated/vonnegut-shape-of-stories.mp4";

    fn playback_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-lifecycle.mp4")
    }

    fn nonzero_pts_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-nonzero-pts.mp4")
    }

    /// Channel order regression: our decoded frame must match the ffmpeg
    /// CLI's BGRA extraction of the same frame pixel-for-pixel. This pins the
    /// RGBA→BGRA fix — feeding GPUI's RenderImage RGBA swaps red and blue.
    #[test]
    fn decoded_frame_matches_ffmpeg_cli_bgra_ground_truth() {
        let path = std::path::Path::new(FIXTURE);
        if !path.exists() {
            return;
        }
        let reference_path = std::env::temp_dir().join("vonnegut_bgra_ref.bin");
        // Blocking spawn is acceptable in a test (bounded, no GPUI executor
        // on this thread) — same justification as the ytdlp detect probe.
        #[allow(clippy::disallowed_methods)]
        let extraction = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg("0.5")
            .arg("-i")
            .arg(path)
            .arg("-frames:v")
            .arg("1")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("bgra")
            .arg(&reference_path)
            .output()
            .expect("ffmpeg CLI runs");
        assert!(
            extraction.status.success(),
            "ffmpeg CLI extraction failed: {}",
            String::from_utf8_lossy(&extraction.stderr)
        );

        let mut player = VideoPlayer::new();
        player.open(path).expect("open");
        player.play();
        // Position the master clock at the reference extraction point — the
        // clock is wall-time-derived, so `seek` (not accumulated deltas) is
        // how tests and callers land on a specific timestamp.
        player.seek(Duration::from_millis(500));
        let frame = player
            .advance_and_decode(Duration::from_millis(33))
            .expect("advance")
            .expect("frame decoded");
        let reference = std::fs::read(&reference_path).expect("read reference");

        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
        assert_eq!(frame.bgra.len(), reference.len());
        // Compare a spread of pixels, not every byte — encoders may differ by
        // rounding between the CLI and library paths; exact channel ORDER is
        // what this pins.
        for (x, y) in [(320, 200), (100, 400), (320, 240), (50, 50), (600, 460)] {
            let index = (y * frame.width as usize + x) * 4;
            let ours = (
                frame.bgra[index],
                frame.bgra[index + 1],
                frame.bgra[index + 2],
            );
            let reference_pixel = (reference[index], reference[index + 1], reference[index + 2]);
            // Tolerance: same channel order, near-identical values.
            let close = ours.0.abs_diff(reference_pixel.0) <= 2
                && ours.1.abs_diff(reference_pixel.1) <= 2
                && ours.2.abs_diff(reference_pixel.2) <= 2;
            assert!(
                close,
                "pixel ({x},{y}): ours (B,G,R)={ours:?} vs CLI (B,G,R)={reference_pixel:?} — channel order or color regression"
            );
        }
    }

    /// expect: Opening a video shows its first frame without starting playback.
    /// [P1] Motivating: the operator can inspect media before choosing to play it.
    /// pre: the source contains at least one decodable video frame.
    /// post: `open` returns that frame and the player remains non-playing.
    /// [P2] Constraining: opening media never starts unsolicited audio.
    #[test]
    fn opening_video_returns_first_frame_while_paused() {
        let path = playback_fixture();
        let mut player = VideoPlayer::new();
        let frame = player.open(&path).expect("open returns the poster frame");

        assert!(frame.width > 0 && frame.height > 0);
        assert!(!player.is_playing(), "opening a video must not autoplay");
    }

    /// expect: Playback presents each frame only when the media clock reaches
    /// that frame's presentation timestamp.
    /// [P1] Motivating: video motion follows the source timeline instead of
    /// racing ahead while the transport position remains unchanged.
    /// pre: the source has a poster at 0ms and its next frame at 100ms.
    /// post: an immediate playback tick emits no post-poster frame.
    /// [P9] Constraining: decoder throughput cannot advance presentation time.
    #[test]
    fn future_frame_waits_for_its_presentation_timestamp() {
        let path = playback_fixture();
        let mut player = VideoPlayer::new();
        player.open(&path).expect("open returns the poster frame");
        player.play();

        let frame = player
            .advance_and_decode(Duration::ZERO)
            .expect("early playback tick is not a decode failure");

        assert!(
            frame.is_none(),
            "the 100ms frame must remain pending while the media clock is before 100ms"
        );
    }

    /// expect: A valid video whose timestamps begin after zero still opens on
    /// its earliest frame and plays on a timeline relative to that frame.
    /// [P1] Motivating: timestamp pacing works for real media regardless of the
    /// container's absolute timestamp origin.
    /// pre: the source's first three frames have PTS 1000ms, 1100ms, and 1200ms.
    /// post: opening returns the 1000ms source frame as presentation time zero.
    /// [P9] Constraining: normalization preserves relative frame intervals.
    #[test]
    fn nonzero_source_pts_is_normalized_to_the_playback_timeline() {
        let mut player = VideoPlayer::new();
        let frame = player
            .open(&nonzero_pts_fixture())
            .expect("non-zero PTS source opens on its earliest frame");

        assert_eq!(frame.presentation_time, Duration::ZERO);
        assert!(!player.is_playing());
    }

    /// expect: Seeking a paused video immediately presents the frame at the
    /// requested media position without requiring Play.
    /// [P1] Motivating: scrubbing gives visible feedback while media is paused.
    /// pre: the 600ms fixture is opened and remains paused.
    /// post: seeking to 500ms yields the 500ms frame and playback stays paused.
    /// [P9] Constraining: the sought frame belongs to the new seek generation.
    #[test]
    fn paused_seek_decodes_and_delivers_the_sought_frame() {
        let path = playback_fixture();
        let mut player = WidgetVideoPlayer::new().expect("worker starts");
        player.open(&path);
        let open_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < open_deadline {
            if player.poll().has_opened() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        player.seek(Duration::from_millis(500));
        let seek_deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut sought_frame = None;
        while std::time::Instant::now() < seek_deadline {
            if let Some(frame) = player.poll().frame {
                sought_frame = Some(frame);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let frame = sought_frame.expect("paused seek delivers a frame");
        assert_eq!(frame.presentation_time, Duration::from_millis(500));
        assert!(!player.is_playing(), "paused seek does not start playback");
    }

    /// expect: Foreground starvation cannot accumulate more than one full
    /// decoded frame per video player.
    /// [P9] Motivating: busy editor frames cannot turn video decoding into an
    /// unbounded memory or foreground-drain queue.
    /// pre: playback advances while the foreground does not poll for 450ms.
    /// post: pending-frame high-water is one and one poll drains the latest frame.
    /// [P1] Constraining: coalescing may replace stale frames but never duplicate them.
    #[test]
    fn worker_frame_backlog_is_bounded_to_latest_frame() {
        let path = playback_fixture();
        let mut player = WidgetVideoPlayer::new().expect("worker starts");
        player.open(&path);
        let open_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < open_deadline {
            if player.poll().has_opened() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        player.play();
        std::thread::sleep(Duration::from_millis(450));
        let stats = player.delivery_stats();
        assert_eq!(stats.max_pending_frames, 1);
        assert_eq!(stats.pending_frames, 1);
        assert!(
            stats.replaced_frames > 0,
            "later due frames should replace an unconsumed earlier frame"
        );
        assert!(player.poll().frame.is_some(), "latest frame is delivered");
        assert!(
            player.poll().frame.is_none(),
            "the mailbox contains no stale frame backlog"
        );
    }

    /// expect: Frame coalescing never discards playback lifecycle outcomes.
    /// [P1] Motivating: a delayed foreground still observes that media opened
    /// and completed instead of seeing an unexplained final frame.
    /// pre: a complete short video plays while foreground polling is delayed.
    /// post: bounded polling eventually reports both Opened and Completed exactly once.
    /// [P9] Constraining: terminal outcomes close rather than reinforce polling.
    #[test]
    fn lifecycle_events_survive_frame_coalescing() {
        let path = playback_fixture();
        let mut player = WidgetVideoPlayer::new().expect("worker starts");
        player.open(&path);
        player.play();
        std::thread::sleep(Duration::from_millis(450));

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut opened_count = 0;
        let mut completed_count = 0;
        while std::time::Instant::now() < deadline && completed_count == 0 {
            let poll = player.poll();
            opened_count += usize::from(poll.has_opened());
            completed_count += usize::from(poll.has_completed());
            if completed_count == 0 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        assert_eq!(
            opened_count, 1,
            "opened event remains observable exactly once"
        );
        assert_eq!(
            completed_count, 1,
            "completed event remains observable exactly once"
        );
        let final_poll = player.poll();
        assert!(!final_poll.has_opened(), "opened is not repeated");
        assert!(!final_poll.has_completed(), "completed is not repeated");
    }

    /// expect: Delivery statistics count only frames accepted by the foreground,
    /// not stale mailbox entries rejected by generation or sequence checks.
    /// [P9] Motivating: benchmark counts describe visible work rather than queue removal.
    /// pre: a mailbox frame has a sequence older than the player's accepted sequence.
    /// post: polling rejects it and leaves consumed frame count unchanged.
    /// [P1] Constraining: stale media cannot appear successful in performance evidence.
    #[test]
    fn rejected_stale_frame_is_not_counted_as_consumed() {
        let (commands, _command_receiver) = std::sync::mpsc::channel();
        let (_event_sender, events) = std::sync::mpsc::channel();
        let frames = LatestFrameMailbox::default();
        frames.invalidate(1);
        frames.publish(
            Some(DecodedFrame {
                width: 1,
                height: 1,
                bgra: vec![0; 4],
                presentation_time: Duration::ZERO,
            }),
            VideoSnapshot {
                state: PlaybackState::Playing,
                position: Duration::ZERO,
                duration: Duration::from_secs(1),
            },
            1,
            1,
        );
        let mut player = WidgetVideoPlayer {
            commands,
            events,
            frames,
            state: PlaybackState::Playing,
            position: Duration::ZERO,
            duration: Duration::from_secs(1),
            pending_error: None,
            generation: 1,
            last_sequence: 2,
            worker_disconnected_reported: false,
        };

        assert!(player.poll().frame.is_none());
        assert_eq!(player.delivery_stats().consumed_frames, 0);
    }

    /// expect: Polling returns every lifecycle outcome for the current source
    /// generation in producer order.
    /// [P1] Motivating: distinct failures are never overwritten into one vague
    /// terminal result when the foreground is delayed.
    /// pre: two failures are queued before one foreground poll.
    /// post: both failure payloads are returned in FIFO order.
    /// [P9] Constraining: frame coalescing never applies to lifecycle outcomes.
    #[test]
    fn polling_preserves_repeated_lifecycle_events_in_order() {
        let (commands, _command_receiver) = std::sync::mpsc::channel();
        let (event_sender, events) = std::sync::mpsc::channel();
        event_sender
            .send(VideoWorkerEvent::Failed {
                generation: 7,
                sequence: 1,
                error: "first".to_string(),
            })
            .expect("first event queues");
        event_sender
            .send(VideoWorkerEvent::Failed {
                generation: 7,
                sequence: 2,
                error: "second".to_string(),
            })
            .expect("second event queues");
        let mut player = WidgetVideoPlayer {
            commands,
            events,
            frames: LatestFrameMailbox::default(),
            state: PlaybackState::Playing,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            pending_error: None,
            generation: 7,
            last_sequence: 0,
            worker_disconnected_reported: false,
        };

        assert_eq!(
            player.poll().events,
            vec![
                VideoPlaybackEvent::Failed("first".to_string()),
                VideoPlaybackEvent::Failed("second".to_string()),
            ]
        );
    }

    /// expect: Unexpected worker disconnection is a visible fatal outcome
    /// delivered once rather than an endless empty poll.
    /// [P1] Motivating: a broken playback worker explains why video stopped.
    /// pre: the lifecycle sender disconnects while the player remains alive.
    /// post: the first poll reports failure and later polls do not repeat it.
    /// [P9] Constraining: terminal feedback closes the polling loop.
    #[test]
    fn worker_disconnection_surfaces_one_fatal_event() {
        let (commands, _command_receiver) = std::sync::mpsc::channel();
        let (event_sender, events) = std::sync::mpsc::channel();
        drop(event_sender);
        let mut player = WidgetVideoPlayer {
            commands,
            events,
            frames: LatestFrameMailbox::default(),
            state: PlaybackState::Playing,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            pending_error: None,
            generation: 0,
            last_sequence: 0,
            worker_disconnected_reported: false,
        };

        assert_eq!(
            player.poll().error(),
            Some("video playback worker stopped unexpectedly")
        );
        assert!(
            player.poll().error().is_none(),
            "fatal event is delivered once"
        );
        assert!(!player.is_playing(), "disconnection is terminal");
    }

    /// expect: Replacing a source invalidates its unconsumed frame and
    /// lifecycle outcomes before the replacement finishes opening.
    /// [P1] Motivating: a failed replacement never displays or announces the
    /// prior asset as if it belonged to the new request.
    /// pre: valid source A opens without polling, then missing source B replaces it.
    /// post: the first poll reports B's failure with no A frame or Opened outcome.
    /// [P9] Constraining: asynchronous feedback is scoped to its source generation.
    #[test]
    fn replacement_open_invalidates_unconsumed_prior_generation() {
        let path = playback_fixture();
        let mut player = WidgetVideoPlayer::new().expect("worker starts");
        player.open(&path);
        std::thread::sleep(Duration::from_millis(100));
        player.open(std::path::Path::new("/definitely/missing/replacement.mp4"));
        std::thread::sleep(Duration::from_secs(1));

        let poll = player.poll();
        assert!(poll.error().is_some(), "replacement failure is surfaced");
        assert!(poll.frame.is_none(), "prior source frame is invalidated");
        assert!(
            !poll.has_opened(),
            "prior source Opened event is invalidated"
        );
    }

    /// expect: A failed replacement open cannot revive frames from the prior
    /// source, and the worker remains reusable for a later valid open.
    /// [P1] Motivating: media widgets never display or play a stale asset after
    /// reporting that a new asset failed to load.
    /// pre: one valid source is followed by one missing source.
    /// post: the failed source emits no stale frame; reopening succeeds.
    #[test]
    fn worker_discards_prior_media_when_replacement_open_fails() {
        let path = playback_fixture();
        let mut player = WidgetVideoPlayer::new().expect("worker starts");
        player.open(&path);
        let open_deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut initially_opened = false;
        while std::time::Instant::now() < open_deadline {
            if player.poll().has_opened() {
                initially_opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(initially_opened, "initial source opens");

        player.open(std::path::Path::new("/definitely/missing/replacement.mp4"));
        let failure_deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut failed = false;
        while std::time::Instant::now() < failure_deadline {
            if player.poll().error().is_some() {
                failed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(failed, "replacement failure is surfaced");

        player.play();
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            player.poll().frame.is_none(),
            "the prior source cannot emit frames after replacement failure"
        );
        player.stop();

        player.open(&path);
        let reopen_deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut reopened = false;
        while std::time::Instant::now() < reopen_deadline {
            if player.poll().has_opened() {
                reopened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reopened, "worker accepts a valid source after failure");
    }

    /// The fixture carries an opus audio track — opening it must set up the
    /// audio pipeline (a video player without audio is not a video player).
    #[test]
    fn opening_video_with_audio_stream_sets_up_audio_pipeline() {
        let path = std::path::Path::new(FIXTURE);
        if !path.exists() {
            return;
        }
        let mut player = VideoPlayer::new();
        player.open(path).expect("open");
        assert!(
            player.has_audio(),
            "fixture has an audio stream — has_audio must be true"
        );
    }

    /// THE STARVATION TEST — the discriminating property between the
    /// wall-clock design (shipped, desynced in-app) and the audio-master
    /// clock: when the audio queue drains (tick callbacks starved by a busy
    /// editor), the playback clock must FREEZE with the audio, not keep
    /// advancing on wall time. With the wall clock, every starvation event
    /// added permanent desync; with audio-master, video waits for audio.
    #[test]
    fn playback_clock_freezes_when_audio_queue_starves() {
        let path = std::path::Path::new(FIXTURE);
        if !path.exists() {
            return;
        }
        let mut player = VideoPlayer::new();
        player.open(path).expect("open");
        player.play();
        // Pump normally for a moment — audio flowing, clock advancing.
        for _ in 0..10 {
            player
                .advance_and_decode(Duration::from_millis(33))
                .expect("advance");
        }
        // The audio-master clock advances only when the output device consumes
        // real samples; immediate decode calls alone do not advance time.
        std::thread::sleep(Duration::from_millis(50));
        let position_before_starvation = player.position();
        assert!(position_before_starvation > Duration::ZERO);

        // Starve: no pumping for 400ms of wall time (a busy editor's late
        // callbacks). Window 1: rodio consumes the queued LEAD (~300ms) —
        // the clock advances by the lead and no further (the wall-clock
        // design advances the full 400ms+ of wall time).
        std::thread::sleep(Duration::from_millis(400));
        let position_after_lead = player.position();
        let window_one_advance = position_after_lead.saturating_sub(position_before_starvation);
        assert!(
            window_one_advance < Duration::from_millis(350),
            "clock must be audio-bound, not wall-bound: advanced {window_one_advance:?} \
             in 400ms with only the ~300ms lead queued"
        );

        // Window 2: the queue is now drained — consumed audio is frozen, so
        // the clock must freeze with it. (The wall-clock design fails this
        // window by advancing another 400ms.)
        std::thread::sleep(Duration::from_millis(400));
        let position_after_starvation = player.position();
        let window_two_advance = position_after_starvation.saturating_sub(position_after_lead);
        assert!(
            window_two_advance < Duration::from_millis(50),
            "clock must freeze once the audio queue is drained: advanced \
             {window_two_advance:?} with no audio flowing"
        );
    }

    /// Audio must actually flow: after a short playing window, samples are
    /// queued on the output player — not just a pipeline that exists.
    #[test]
    fn audio_pumps_ahead_of_the_playback_clock() {
        let path = std::path::Path::new(FIXTURE);
        if !path.exists() {
            return;
        }
        let mut player = VideoPlayer::new();
        player.open(path).expect("open");
        player.play();
        // The clock is wall-time-derived: seek to 1s, then tick as the
        // widget loop does. Position must hold >= 1s and advance with wall
        // time, and audio must actually be queued on the output player.
        player.seek(Duration::from_secs(1));
        // Seek rebases exactly: position is at the seek target (plus only
        // sub-100ms wall time), never target + pre-seek elapsed.
        assert!(player.position() < Duration::from_millis(1_100));
        for _ in 0..30 {
            player
                .advance_and_decode(Duration::from_millis(33))
                .expect("advance");
        }
        let position_after_ticks = player.position();
        assert!(position_after_ticks >= Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            player.position() > position_after_ticks,
            "the master clock must advance at wall time while playing"
        );
        assert!(
            player.has_audio(),
            "audio pipeline must survive playback ticks"
        );
        assert!(
            player.audio_queue_len() > 0,
            "audio samples must be queued on the output player after seeking into the stream"
        );
        // Pause freezes the clock. Capture AFTER pause() — rodio's position
        // control updates on a 5ms periodic tick, so one final update can
        // land just after the pause; bounded jitter, not clock drift.
        player.pause();
        let paused_at = player.position();
        std::thread::sleep(Duration::from_millis(50));
        let drift_after_pause = player.position().saturating_sub(paused_at);
        assert!(
            drift_after_pause < Duration::from_millis(10),
            "clock must freeze on pause (drifted {drift_after_pause:?} — rodio updates every 5ms)"
        );
        assert_ne!(player.state(), PlaybackState::Playing);
    }

    /// expect: Packet exhaustion finishes playback even when duration metadata
    /// is unavailable, and later ticks remain quiet.
    /// [P1] Motivating: completed media never becomes an unbounded error loop.
    /// pre: a playable source is positioned near its final packets.
    /// post: state becomes Finished without an error from repeated advances.
    /// [P9] Constraining: terminal feedback must reduce, not reinforce, polling.
    #[test]
    fn packet_exhaustion_finishes_unknown_duration_playback_once() {
        let path = playback_fixture();
        let mut player = VideoPlayer::new();
        player.open(&path).expect("open");
        let actual_duration = player.duration();
        player.duration = Duration::ZERO;
        player.seek(actual_duration.saturating_sub(Duration::from_millis(100)));
        player.play();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while player.state() != PlaybackState::Finished && std::time::Instant::now() < deadline {
            player
                .advance_and_decode(Duration::from_millis(10))
                .expect("normal end-of-stream is not a decode failure");
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            player.state(),
            PlaybackState::Finished,
            "position={:?}, queued_audio_sources={}",
            player.position(),
            player.audio_queue_len()
        );
        for _ in 0..3 {
            assert!(
                player
                    .advance_and_decode(Duration::from_millis(10))
                    .expect("finished playback stays quiet")
                    .is_none()
            );
        }

        player.seek(Duration::from_millis(500));
        assert_eq!(
            player.state(),
            PlaybackState::Paused,
            "seeking after EOF clears the terminal state"
        );
        player.play();
        assert!(player.position() >= Duration::from_millis(500));
        assert!(
            player
                .advance_and_decode(Duration::from_millis(10))
                .expect("decode after terminal seek")
                .is_some(),
            "seeking after EOF makes frames decodable again"
        );
    }

    /// End-to-end streaming against the live source the requirement names:
    /// resolve the platform URL to its DASH pair, open both inputs, decode
    /// video, and queue audio. `#[ignore]`d because it needs network and
    /// live YouTube URLs (which expire); run explicitly with
    /// `cargo test -p hkask-media-widget -- --ignored`.
    #[test]
    #[ignore = "requires network + live YouTube stream URLs"]
    fn streams_youtube_video_with_audio_end_to_end() {
        let url = "https://www.youtube.com/watch?v=4ec0lSd7qH4";
        let stream_urls =
            smol::block_on(async { crate::streaming::resolve_stream_urls(url).await })
                .expect("resolve");
        assert!(
            stream_urls.audio.is_some(),
            "DASH source must resolve a separate audio URL — a video-only URL would play silent"
        );

        let mut player = VideoPlayer::new();
        player
            .open_stream(&stream_urls.video, stream_urls.audio.as_deref())
            .expect("open both inputs");
        assert!(player.has_audio(), "audio pipeline must be live");
        player.play();
        let frame = player
            .advance_and_decode(Duration::from_millis(33))
            .expect("advance")
            .expect("first frame decodes from the stream");
        assert!(frame.width > 0 && frame.height > 0);
        // The audio-master clock advances with REAL consumed audio —
        // instant ticks no longer fake time (the wall-delta clock did).
        // Give rodio real time to consume, then pump and assert.
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..3 {
            player
                .advance_and_decode(Duration::from_millis(33))
                .expect("advance");
        }
        // position is consumed-audio-derived under the audio-master clock —
        // advancing position IS the proof that streamed audio flowed to the
        // output. (Queue length is the wrong oracle: a healthy player
        // consumes the queue, so it is routinely empty during playback.)
        assert!(
            player.position() > Duration::from_millis(50),
            "the audio-master clock must advance with consumed streamed audio"
        );
    }
}
