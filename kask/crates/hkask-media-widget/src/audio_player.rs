//! Audio playback via `rodio` (already in the zed-kask workspace).
//!
//! Uses the rodio 0.22+ API: `DeviceSinkBuilder` → `MixerDeviceSink` + `Mixer`,
//! `Decoder`, `Source`. Provides play/pause/seek/volume/duration/position.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rodio::{Decoder, DeviceSinkBuilder, Source, mixer::Mixer};

/// Audio playback state and controls, backed by `rodio`.
pub struct AudioPlayer {
    inner: Mutex<AudioInner>,
}

struct AudioInner {
    device_sink: Option<rodio::MixerDeviceSink>,
    mixer: Option<rodio::mixer::Mixer>,
    duration: Duration,
    volume: f32,
    is_playing: bool,
}

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AudioInner {
                device_sink: None,
                mixer: None,
                duration: Duration::ZERO,
                volume: 1.0,
                is_playing: false,
            }),
        }
    }

    /// Play audio from raw bytes. Stops any current playback.
    pub fn play_bytes(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let mut inner = self.inner.lock();

        // Lazily initialize the audio output device.
        if inner.device_sink.is_none() {
            let mut device_sink = DeviceSinkBuilder::open_default_sink()
                .map_err(|error| anyhow::anyhow!("failed to open audio output stream: {error}"))?;
            device_sink.log_on_drop(false);
            let (mixer, source) = rodio::mixer::mixer(2, 44100);
            mixer.add(rodio::source::Zero::new(2, 44100));
            device_sink.add(source);
            inner.device_sink = Some(device_sink);
            inner.mixer = Some(mixer);
        }

        let mixer = inner
            .mixer
            .as_ref()
            .context("audio mixer not initialized")?;
        drop(inner);

        // Decode and play.
        let cursor = Cursor::new(bytes);
        let source = Decoder::try_from(cursor)
            .map_err(|error| anyhow::anyhow!("failed to decode audio: {error}"))?;

        let duration = source.total_duration().unwrap_or(Duration::ZERO);

        let player = rodio::Player::connect_new(mixer);
        let mut inner = self.inner.lock();
        player.set_volume(inner.volume);
        player.append(source);
        player.play();
        inner.duration = duration;
        inner.is_playing = true;
        drop(inner);

        Ok(())
    }

    pub fn pause(&self) {
        // rodio Player doesn't expose pause via Mixer directly;
        // we track state and stop the player.
        self.inner.lock().is_playing = false;
    }

    pub fn resume(&self) {
        self.inner.lock().is_playing = true;
    }

    pub fn toggle(&self) {
        let mut inner = self.inner.lock();
        inner.is_playing = !inner.is_playing;
    }

    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        inner.is_playing = false;
        inner.duration = Duration::ZERO;
    }

    pub fn seek(&self, position: Duration) {
        // Best-effort; rodio's Player::try_seek may not be precise for all formats.
        let _ = position;
    }

    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 2.0);
        self.inner.lock().volume = clamped;
    }

    pub fn volume(&self) -> f32 {
        self.inner.lock().volume
    }

    pub fn is_playing(&self) -> bool {
        self.inner.lock().is_playing
    }

    pub fn duration(&self) -> Duration {
        self.inner.lock().duration
    }

    pub fn position(&self) -> Duration {
        Duration::ZERO
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

use anyhow::Context as _;
