//! Video decode via FFmpeg (behind the `video` or `vendored` feature flag).
//!
//! Architecture: FFmpeg decodes to RGBA via `libswscale`, the RGBA bytes are
//! wrapped as a `gpui::RenderImage` and displayed via `img()`. This is the
//! same pattern used by Servo, Oxide, iced, and egui — the browser-grade
//! standard for video in Rust GUI frameworks.
//!
//! When the `video`/`vendored` feature is NOT enabled, this module provides
//! stub types that compile but return errors at runtime. This keeps dev builds
//! fast (no FFmpeg compile) while allowing release builds to enable video.

use std::path::Path;
use std::time::Duration;

/// A decoded video frame (RGBA bytes + dimensions).
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data, row-major, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Playback state for the video player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Video playback clock + decode state.
pub struct VideoPlayer {
    state: PlaybackState,
    position: Duration,
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
            position: Duration::ZERO,
            duration: Duration::ZERO,
            volume: 1.0,
            #[cfg(feature = "video")]
            decoder: None,
        }
    }

    /// Open a video file for playback.
    pub fn open(&mut self, path: &Path) -> anyhow::Result<()> {
        #[cfg(feature = "video")]
        {
            let decoder = VideoDecoderInner::open(path)?;
            self.duration = decoder.duration();
            self.decoder = Some(decoder);
            self.state = PlaybackState::Stopped;
            self.position = Duration::ZERO;
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

    /// Start playback.
    pub fn play(&mut self) {
        if self.state != PlaybackState::Stopped {
            self.state = PlaybackState::Playing;
        }
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
    }

    /// Stop playback and reset position.
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position = Duration::ZERO;
    }

    /// Seek to a position.
    pub fn seek(&mut self, position: Duration) {
        self.position = position;
        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                decoder.seek(position);
            }
        }
    }

    /// Set volume (0.0 to 1.0+).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 2.0);
    }

    /// Get the current volume.
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Get the current playback position.
    pub fn position(&self) -> Duration {
        self.position
    }

    /// Get the total duration.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Get the playback state.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Whether playback is active.
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    /// Advance the playback clock by `delta` and decode the appropriate frame.
    ///
    /// Called from the GPUI render loop (or a background `cx.spawn` timer)
    /// at ~30fps while playing. Returns the decoded RGBA frame for display
    /// via `img(RenderImage)`.
    pub fn advance_and_decode(&mut self, delta: Duration) -> anyhow::Result<Option<DecodedFrame>> {
        if self.state != PlaybackState::Playing {
            return Ok(None);
        }

        self.position = self.position + delta;
        if self.duration > Duration::ZERO && self.position >= self.duration {
            self.position = self.duration;
            self.state = PlaybackState::Stopped;
        }

        #[cfg(feature = "video")]
        {
            if let Some(decoder) = &mut self.decoder {
                return Ok(Some(decoder.decode_frame_at(self.position)?));
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

    use ffmpeg_next as ffmpeg;

    /// FFmpeg-backed video decoder. Decodes any format FFmpeg supports
    /// (MP4, WebM, MKV, AV1, H.264, HEVC, etc.) to RGBA via `libswscale`.
    ///
    /// Adapted from the proven pattern in Oxide browser's `video.rs`
    /// (Apache-2.0) and `iced_video_player` (MIT).
    pub struct VideoDecoderInner {
        input: ffmpeg::format::context::Input,
        video_stream_index: usize,
        decoder: ffmpeg::decoder::Video,
        scaler: ffmpeg::software::scaling::Context,
        time_base: ffmpeg::Rational,
        duration_ms: u64,
        width: u32,
        height: u32,
    }

    impl VideoDecoderInner {
        pub fn open(path: &Path) -> anyhow::Result<Self> {
            ffmpeg::init()
                .map_err(|error| anyhow::anyhow!("failed to initialize FFmpeg: {error}"))?;

            let input = ffmpeg::format::input(path)
                .map_err(|error| anyhow::anyhow!("failed to open video file: {error}"))?;

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

            let scaler = ffmpeg::software::scaling::context::Context::get(
                decoder.format(),
                width,
                height,
                ffmpeg::format::Pixel::RGBA,
                width,
                height,
                ffmpeg::software::scaling::flag::Flags::BILINEAR,
            )
            .map_err(|error| anyhow::anyhow!("failed to create scaler: {error}"))?;

            let duration_ms = if input.duration() > 0 {
                input.duration().rescale(
                    ffmpeg::Rational::new(1, ffmpeg::ffi::AV_TIME_BASE as i32),
                    (1, 1000),
                ) as u64
            } else {
                0
            };

            Ok(Self {
                input,
                video_stream_index,
                decoder,
                scaler,
                time_base,
                duration_ms,
                width,
                height,
            })
        }

        pub fn duration(&self) -> Duration {
            Duration::from_millis(self.duration_ms)
        }

        pub fn seek(&mut self, target: Duration) {
            let timestamp = target.as_millis() as i64;
            let _ = self.input.seek_to_second(
                ffmpeg::Rational::new(timestamp, 1000),
                ffmpeg::format::SeekTarget::Any,
            );
            self.decoder.flush();
        }

        /// Decode the frame closest to `target` time. Returns RGBA bytes.
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

                    let mut rgba_frame = ffmpeg::util::frame::video::Video::empty();
                    self.scaler
                        .run(&decoded, &mut rgba_frame)
                        .map_err(|error| anyhow::anyhow!("scale error: {error}"))?;

                    let width = rgba_frame.width();
                    let height = rgba_frame.height();
                    let stride = rgba_frame.stride(0);
                    let mut rgba_bytes = Vec::with_capacity((width * height * 4) as usize);
                    for row in 0..height as usize {
                        let start = row * stride;
                        let end = start + (width as usize * 4);
                        rgba_bytes.extend_from_slice(&rgba_frame.data(0)[start..end]);
                    }

                    if frame_ms >= target_ms {
                        return Ok(DecodedFrame {
                            width,
                            height,
                            rgba: rgba_bytes,
                        });
                    }
                    best_frame = Some(DecodedFrame {
                        width,
                        height,
                        rgba: rgba_bytes,
                    });
                }
            }

            best_frame.ok_or_else(|| anyhow::anyhow!("no video frame decoded"))
        }
    }
}

#[cfg(feature = "video")]
use ffmpeg_impl::VideoDecoderInner;
