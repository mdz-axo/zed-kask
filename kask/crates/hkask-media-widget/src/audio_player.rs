//! Audio playback via `rodio` (already in the zed-kask workspace).
//!
//! Uses the rodio 0.22+ API: `DeviceSinkBuilder` → `MixerDeviceSink`,
//! `MixerDeviceSink::mixer()`, `Player::connect_new`, `Decoder`.

use std::io::Cursor;
use std::time::Duration;

use anyhow::Context as _;
use parking_lot::Mutex;
use rodio::{Decoder, DeviceSinkBuilder, Source};

/// Audio playback state and controls, backed by `rodio`.
pub struct AudioPlayer {
    inner: Mutex<AudioInner>,
}

struct AudioInner {
    device_sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    duration: Duration,
    volume: f32,
    is_playing: bool,
}

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AudioInner {
                device_sink: None,
                player: None,
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
            inner.device_sink = Some(device_sink);
        }

        let device_sink = inner
            .device_sink
            .as_ref()
            .context("audio device not initialized")?;
        let mixer = device_sink.mixer();

        // Decode the audio source.
        let cursor = Cursor::new(bytes);
        let source = Decoder::try_from(cursor)
            .map_err(|error| anyhow::anyhow!("failed to decode audio: {error}"))?;

        let duration = source.total_duration().unwrap_or(Duration::ZERO);

        // Create a new player on the mixer and start playback.
        let player = rodio::Player::connect_new(mixer);
        player.set_volume(inner.volume);
        player.append(source);
        player.play();

        inner.player = Some(player);
        inner.duration = duration;
        inner.is_playing = true;
        drop(inner);

        Ok(())
    }

    pub fn pause(&self) {
        let inner = self.inner.lock();
        if let Some(player) = &inner.player {
            player.pause();
        }
    }

    pub fn resume(&self) {
        let inner = self.inner.lock();
        if let Some(player) = &inner.player {
            player.play();
        }
    }

    pub fn toggle(&self) {
        let inner = self.inner.lock();
        if let Some(player) = &inner.player {
            if player.is_paused() {
                player.play();
            } else {
                player.pause();
            }
        }
    }

    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        if let Some(player) = inner.player.take() {
            player.stop();
        }
        inner.is_playing = false;
        inner.duration = Duration::ZERO;
    }

    pub fn seek(&self, position: Duration) {
        let inner = self.inner.lock();
        if let Some(player) = &inner.player
            && let Err(error) = player.try_seek(position)
        {
            log::warn!("hkask-media-widget: audio seek failed: {error}");
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 2.0);
        let mut inner = self.inner.lock();
        inner.volume = clamped;
        if let Some(player) = &inner.player {
            player.set_volume(clamped);
        }
    }

    pub fn volume(&self) -> f32 {
        self.inner.lock().volume
    }

    pub fn is_playing(&self) -> bool {
        let inner = self.inner.lock();
        inner
            .player
            .as_ref()
            .is_some_and(|player| !player.is_paused() && !player.empty())
    }

    pub fn duration(&self) -> Duration {
        self.inner.lock().duration
    }

    pub fn position(&self) -> Duration {
        let inner = self.inner.lock();
        inner
            .player
            .as_ref()
            .map(|player| player.get_pos())
            .unwrap_or(Duration::ZERO)
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
