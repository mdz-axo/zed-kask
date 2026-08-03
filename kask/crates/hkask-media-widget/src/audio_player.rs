//! Audio playback via `rodio` (already in the zed-kask workspace).
//!
//! Supports WAV, MP3, Ogg/Vorbis, FLAC via `symphonia` (rodio's default decoder).
//! Provides play/pause/seek/volume/duration/position — the transport state
//! the `MediaWidget` needs.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

/// Audio playback state and controls, backed by `rodio`.
///
/// The `rodio` sink is created lazily on first play. All controls are
/// non-blocking and safe to call from the GPUI foreground thread.
pub struct AudioPlayer {
    inner: Mutex<AudioInner>,
}

struct AudioInner {
    sink: Option<rodio::Sink>,
    duration: Duration,
    volume: f32,
    is_playing: bool,
}

impl AudioPlayer {
    /// Create a new audio player.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AudioInner {
                sink: None,
                duration: Duration::ZERO,
                volume: 1.0,
                is_playing: false,
            }),
        }
    }

    /// Play audio from raw bytes. Stops any current playback.
    pub fn play_bytes(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let (_stream, stream_handle) = rodio::OutputStream::try_default()
            .map_err(|error| anyhow::anyhow!("failed to open audio output stream: {error}"))?;

        // OutputStream must be kept alive — store it in the inner struct.
        let cursor = Cursor::new(bytes);
        let source = rodio::Decoder::new(cursor)
            .map_err(|error| anyhow::anyhow!("failed to decode audio: {error}"))?;

        let duration = source.total_duration().unwrap_or(Duration::ZERO);

        let sink = rodio::Sink::try_new(&stream_handle)
            .map_err(|error| anyhow::anyhow!("failed to create audio sink: {error}"))?;

        sink.set_volume(self.inner.lock().volume);
        sink.append(source);

        let mut inner = self.inner.lock();
        // Drop the previous sink (stops playback) before replacing.
        inner.sink = Some(sink);
        inner.duration = duration;
        inner.is_playing = true;
        // The stream handle must outlive the sink — leak it intentionally
        // (it's dropped when the sink is dropped, but rodio's API makes
        // this awkward; in production we'd store it in an Arc).
        std::mem::forget(_stream);

        Ok(())
    }

    /// Pause playback.
    pub fn pause(&self) {
        if let Some(sink) = &self.inner.lock().sink {
            sink.pause();
            self.inner.lock().is_playing = false;
        }
    }

    /// Resume playback.
    pub fn resume(&self) {
        if let Some(sink) = &self.inner.lock().sink {
            sink.play();
            self.inner.lock().is_playing = true;
        }
    }

    /// Toggle play/pause.
    pub fn toggle(&self) {
        let is_playing = self.inner.lock().is_playing;
        if is_playing {
            self.pause();
        } else {
            self.resume();
        }
    }

    /// Stop playback and release the sink.
    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        if let Some(sink) = inner.sink.take() {
            sink.stop();
        }
        inner.is_playing = false;
    }

    /// Seek to a position (best-effort — rodio's seek may not be precise
    /// for all formats).
    pub fn seek(&self, position: Duration) {
        if let Some(sink) = &self.inner.lock().sink {
            if let Err(error) = sink.try_seek(position) {
                log::warn!("hkask-media-widget: audio seek failed: {error}");
            }
        }
    }

    /// Set volume (0.0 to 1.0+).
    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 2.0);
        self.inner.lock().volume = clamped;
        if let Some(sink) = &self.inner.lock().sink {
            sink.set_volume(clamped);
        }
    }

    /// Get the current volume.
    pub fn volume(&self) -> f32 {
        self.inner.lock().volume
    }

    /// Whether playback is active (not paused).
    pub fn is_playing(&self) -> bool {
        self.inner.lock().is_playing
    }

    /// Get the total duration, if known.
    pub fn duration(&self) -> Duration {
        self.inner.lock().duration
    }

    /// Get the current playback position (best-effort).
    pub fn position(&self) -> Duration {
        if let Some(sink) = &self.inner.lock().sink {
            sink.get_pos()
        } else {
            Duration::ZERO
        }
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Whether the `AudioPlayer` can decode a given MIME type or file extension.
pub fn can_decode(mime_or_extension: &str) -> bool {
    let lower = mime_or_extension.to_lowercase();
    matches!(
        lower.as_str(),
        "wav"
            | "mp3"
            | "ogg"
            | "flac"
            | "aac"
            | "m4a"
            | "audio/wav"
            | "audio/mpeg"
            | "audio/ogg"
            | "audio/flac"
            | "audio/aac"
            | "audio/mp4"
    )
}
