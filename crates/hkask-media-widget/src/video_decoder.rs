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
}

/// Playback state for the transport bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// How far ahead of the playback clock audio is queued. Rodio consumes at
/// real time; a small lead absorbs demux jitter without growing unbounded.
const AUDIO_LEAD: Duration = Duration::from_millis(300);

pub struct VideoPlayer {
    state: PlaybackState,
    /// The master playback clock. While Playing, position is derived from
    /// `playing_since` + `position_at_play` (wall time) — NOT accumulated
    /// tick deltas, which jitter with the event loop and drift against the
    /// audio device's real-time consumption. Audio is queued against this
    /// same clock, so the streams cannot drift apart.
    playing_since: Option<std::time::Instant>,
    position_at_play: Duration,
    duration: Duration,
    volume: f32,
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
            duration: Duration::ZERO,
            volume: 1.0,
            #[cfg(feature = "video")]
            decoder: None,
        }
    }

    /// Open a local video file for playback. Sets up both the video decoder
    /// and, when the file has an audio stream, the audio decode + output
    /// pipeline (as a second FFmpeg input on the same file).
    pub fn open(&mut self, path: &Path) -> anyhow::Result<()> {
        #[cfg(feature = "video")]
        {
            let decoder = VideoDecoderInner::open(path, None)?;
            self.duration = decoder.duration();
            self.decoder = Some(decoder);
            self.state = PlaybackState::Stopped;
            self.playing_since = None;
            self.position_at_play = Duration::ZERO;
            Ok(())
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
    pub fn open_stream(&mut self, video_url: &str, audio_url: Option<&str>) -> anyhow::Result<()> {
        #[cfg(feature = "video")]
        {
            let decoder = VideoDecoderInner::open(
                std::path::Path::new(video_url),
                audio_url.map(std::path::Path::new),
            )?;
            self.duration = decoder.duration();
            self.decoder = Some(decoder);
            self.state = PlaybackState::Stopped;
            self.playing_since = None;
            self.position_at_play = Duration::ZERO;
            Ok(())
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

    /// Open a remote URL for streaming playback. FFmpeg's format input accepts
    /// URL strings — its http/https protocol handlers stream directly. For
    /// platform URLs (YouTube, Vimeo, etc.), the caller should resolve the
    /// direct stream URL(s) via `streaming::resolve_stream_urls` first and
    /// use [`VideoPlayer::open_stream`] so DASH audio is not lost.
    pub fn open_url(&mut self, url: &str) -> anyhow::Result<()> {
        self.open_stream(url, None)
    }

    /// Start playback. Transitions from any state (including Stopped after
    /// `open`) to Playing — video clock and audio output together. The
    /// master clock rebases to now so position advances at real time.
    pub fn play(&mut self) {
        self.playing_since = Some(std::time::Instant::now());
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

    /// Seek to a position. Rebases the master clock and resets both
    /// streams; audio resumes when the player was already Playing because
    /// rodio's `clear()` leaves its player paused.
    pub fn seek(&mut self, position: Duration) {
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
        }
    }

    /// Set volume (0.0 to 1.0+).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 2.0);
        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                decoder.set_audio_volume(self.volume);
            }
        }
    }

    /// Get the current volume.
    #[must_use]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Get the current playback position — the master clock. While Playing
    /// this is wall-time-derived, so it advances at real time regardless of
    /// event-loop jitter.
    #[must_use]
    pub fn position(&self) -> Duration {
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
    /// Called from the GPUI render loop (or a background `cx.spawn` timer)
    /// at ~30fps while playing. The `delta` argument is unused for the
    /// clock (position is wall-time-derived) but kept for signature
    /// stability. Returns the decoded BGRA frame for display via
    /// `img(RenderImage)`.
    pub fn advance_and_decode(&mut self, _delta: Duration) -> anyhow::Result<Option<DecodedFrame>> {
        if self.state != PlaybackState::Playing {
            return Ok(None);
        }

        let position = self.position();
        if self.duration > Duration::ZERO && position >= self.duration {
            self.position_at_play = self.duration;
            self.playing_since = None;
            self.state = PlaybackState::Stopped;
            #[cfg(feature = "video")]
            {
                if let Some(decoder) = &mut self.decoder {
                    decoder.pause_audio();
                }
            }
            return Ok(None);
        }

        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                // Audio first: an audio failure must not kill the video —
                // log it and keep the picture moving.
                decoder.pump_audio_until(position + AUDIO_LEAD);
                return Ok(Some(decoder.decode_frame_at(position)?));
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

// ── FFmpeg-backed implementation ──────────────────────────────────────────

#[cfg(feature = "video")]
mod ffmpeg_impl {
    use super::DecodedFrame;
    use std::path::Path;
    use std::time::Duration;

    use ffmpeg::Rescale;
    use ffmpeg_next as ffmpeg;

    /// The audio output format every stream is resampled to: packed f32,
    /// stereo, 48 kHz — a fixed format keeps the rodio `SamplesBuffer`
    /// construction trivial.
    const AUDIO_RATE: u32 = 48_000;

    /// Samples buffered before one rodio append: 0.5s of stereo f32. See
    /// `AudioPipeline::pending_samples` for why appends are chunked.
    const APPEND_CHUNK_SAMPLES: usize = (AUDIO_RATE as usize) / 2 * 2;

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
        audio: Option<AudioPipeline>,
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
        /// Samples buffered toward the next player append. Appending one
        /// rodio source per resampled frame (~20ms) would queue dozens of
        /// tiny sources, and rodio's `clear()` blocks ~5ms per queued source
        /// — a seek would stall the foreground thread. Buffering to
        /// `APPEND_CHUNK_SAMPLES` keeps the queue at a couple of sources.
        pending_samples: Vec<f32>,
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
                audio,
            })
        }

        #[must_use]
        pub fn has_audio(&self) -> bool {
            self.audio.is_some()
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

        pub fn set_audio_volume(&mut self, volume: f32) {
            if let Some(audio) = &mut self.audio {
                audio.player.set_volume(volume);
            }
        }

        /// Queue audio up to `target` on the playback clock. Audio failures
        /// are logged, not propagated — a broken audio stream must not stop
        /// the video.
        pub fn pump_audio_until(&mut self, target: Duration) {
            if let Some(audio) = &mut self.audio {
                if let Err(error) = audio.pump_until(target) {
                    log::warn!("hkask-media-widget: audio pump failed: {error}");
                }
            }
        }

        /// Decode the video frame closest to `target` time.
        pub fn decode_frame_at(&mut self, target: Duration) -> anyhow::Result<DecodedFrame> {
            let target_ms = target.as_millis() as u64;

            let mut best_frame: Option<DecodedFrame> = None;

            for (stream, packet) in self.input.packets() {
                if stream.index() != self.video_stream_index {
                    continue;
                }

                self.decoder
                    .send_packet(&packet)
                    .map_err(|error| anyhow::anyhow!("decode error: {error}"))?;

                let mut decoded = ffmpeg::util::frame::video::Video::empty();
                while self.decoder.receive_frame(&mut decoded).is_ok() {
                    let pts = decoded.timestamp().or_else(|| decoded.pts()).unwrap_or(0);
                    if pts < 0 {
                        continue;
                    }
                    let frame_ms = pts.rescale(self.time_base, (1, 1000)).max(0) as u64;

                    let mut bgra_frame = ffmpeg::util::frame::video::Video::empty();
                    self.scaler
                        .run(&decoded, &mut bgra_frame)
                        .map_err(|error| anyhow::anyhow!("scale error: {error}"))?;

                    let width = bgra_frame.width();
                    let height = bgra_frame.height();
                    let stride = bgra_frame.stride(0);
                    let mut bgra_bytes = Vec::with_capacity((width * height * 4) as usize);
                    for row in 0..height as usize {
                        let start = row * stride;
                        let end = start + (width as usize * 4);
                        bgra_bytes.extend_from_slice(&bgra_frame.data(0)[start..end]);
                    }

                    if frame_ms >= target_ms {
                        return Ok(DecodedFrame {
                            width,
                            height,
                            bgra: bgra_bytes,
                        });
                    }
                    best_frame = Some(DecodedFrame {
                        width,
                        height,
                        bgra: bgra_bytes,
                    });
                }
            }

            best_frame.ok_or_else(|| anyhow::anyhow!("no video frame decoded"))
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
            // Start paused: audio must not sound until the operator (or
            // autoplay) transitions the player to Playing.
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
                pending_samples: Vec::new(),
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

        fn reset_after_seek(&mut self, target: Duration) {
            let timestamp_us = target.as_micros() as i64;
            if let Err(error) = self.input.seek(timestamp_us, ..) {
                log::warn!("audio seek to {target:?} failed: {error}");
            }
            self.decoder.flush();
            self.player.clear();
            self.queued_until_ms = target.as_millis() as u64;
            self.pending_samples.clear();
            // Rebuild the resampler from the decoder's (unchanged)
            // parameters — swr has no reset, and stale buffered samples
            // from before the seek would play as a glitch.
            if let Ok(resampler) = Self::build_resampler(&self.decoder) {
                self.resampler = resampler;
            }
        }

        /// Demux, decode, resample, and queue audio until the queued samples
        /// cover `target` on the playback clock. The demuxer position
        /// persists across calls, so each pump resumes where the last left
        /// off — the same property the video decode loop relies on.
        fn pump_until(&mut self, target: Duration) -> anyhow::Result<()> {
            let target_ms = target.as_millis() as u64;
            if self.queued_until_ms >= target_ms {
                return Ok(());
            }

            for (stream, packet) in self.input.packets() {
                if stream.index() != self.stream_index {
                    continue;
                }

                self.decoder
                    .send_packet(&packet)
                    .map_err(|error| anyhow::anyhow!("audio decode error: {error}"))?;

                let mut decoded = ffmpeg::frame::Audio::empty();
                while self.decoder.receive_frame(&mut decoded).is_ok() {
                    let pts = decoded.timestamp().or_else(|| decoded.pts()).unwrap_or(0);
                    if pts >= 0 {
                        let frame_ms = pts.rescale(self.time_base, (1, 1000)).max(0) as u64;
                        self.queued_until_ms = self.queued_until_ms.max(frame_ms);
                    }

                    let mut resampled = ffmpeg::frame::Audio::empty();
                    let mut delay = self
                        .resampler
                        .run(&decoded, &mut resampled)
                        .map_err(|error| anyhow::anyhow!("audio resample error: {error}"))?;
                    append_frame(&mut self.pending_samples, &self.player, &resampled);

                    // Drain the resampler's internal frames.
                    let mut guard = 0;
                    while delay.is_some() && guard < 64 {
                        let mut drained = ffmpeg::frame::Audio::empty();
                        delay = self.resampler.flush(&mut drained).map_err(|error| {
                            anyhow::anyhow!("audio resample flush error: {error}")
                        })?;
                        append_frame(&mut self.pending_samples, &self.player, &drained);
                        guard += 1;
                    }
                }

                if self.queued_until_ms >= target_ms {
                    break;
                }
            }
            Ok(())
        }
    }

    /// Buffer one resampled audio frame toward the next player append,
    /// flushing the buffer as a single rodio source once it holds
    /// `APPEND_CHUNK_SAMPLES`. A free function taking the disjoint fields
    /// so the demux loop can hold `&mut input` while appending — an `&mut
    /// self` method would conflict with the iterator's borrow.
    fn append_frame(
        pending_samples: &mut Vec<f32>,
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
        if pending_samples.len() >= APPEND_CHUNK_SAMPLES || player.empty() {
            // Also flush on an empty queue: the chunk threshold alone would
            // strand up to half a second of buffered audio while rodio's
            // queue drains dry (audible gap).
            if pending_samples.is_empty() {
                return;
            }
            let samples = std::mem::take(pending_samples);
            player.append(rodio::buffer::SamplesBuffer::new(
                std::num::NonZero::new(2).expect("nonzero channel count"),
                std::num::NonZero::new(AUDIO_RATE).expect("nonzero sample rate"),
                samples,
            ));
        }
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
        // Pause freezes the clock. Capture AFTER pause() — two position()
        // calls before the freeze differ by microseconds of elapsed time.
        player.pause();
        let paused_at = player.position();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(player.position(), paused_at);
        assert_ne!(player.state(), PlaybackState::Playing);
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
        for _ in 0..30 {
            player
                .advance_and_decode(Duration::from_millis(33))
                .expect("advance");
        }
        assert!(
            player.audio_queue_len() > 0,
            "streamed audio must be queued on the output player"
        );
    }
}
