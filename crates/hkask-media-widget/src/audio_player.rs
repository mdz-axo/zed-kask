//! Audio playback via `rodio` (already in the zed-kask workspace).
//!
//! Uses the rodio 0.22+ API: `DeviceSinkBuilder` → `MixerDeviceSink`,
//! `MixerDeviceSink::mixer()`, `Player::connect_new`, `Decoder`.

use std::io::Cursor;
use std::time::Duration;

use parking_lot::Mutex;
use rodio::{Decoder, DeviceSinkBuilder, Source};
use symphonia_adapter_libopus::OpusDecoder;
use symphonia_core::audio::{AudioBufferRef, Channels};
use symphonia_core::codecs::{
    CODEC_TYPE_OPUS, CodecDescriptor, CodecParameters, Decoder as SymphoniaDecoder, DecoderOptions,
    FinalizeResult,
};
use symphonia_core::errors::Result as SymphoniaResult;
use symphonia_core::formats::Packet;
use symphonia_core::support_codec;

use crate::transport::{media_elapsed, wall_deadline};

/// Audio playback state and controls, backed by `rodio`.
pub struct AudioPlayer {
    inner: Mutex<AudioInner>,
}

struct AudioInner {
    device_sink: Option<rodio::MixerDeviceSink>,
    /// The decoded source awaiting its first play/seek — loading decodes
    /// and parses duration but defers the output device + player
    /// construction, so a machine with no audio hardware (headless
    /// sessions, CI runners) can still load and read positions (the
    /// eager device open failed the whole load on CI, 2026-11-07).
    pending_source: Option<Decoder<Cursor<Vec<u8>>>>,
    /// A seek requested while no player exists — parked and reported by
    /// `position()` so the transport stays honest without a device, and
    /// applied when the deferred player is constructed.
    pending_seek: Option<Duration>,
    player: Option<rodio::Player>,
    duration: Duration,
    /// Playback rate. rodio's `Speed` wrapper reports positions in wall time
    /// (media / rate) and expects wall-equivalent seek targets, so every
    /// media-time read multiplies by it and the seek target divides by it.
    speed: f32,
}

/// Supplies the mono/stereo channel metadata that Symphonia's MP4 demuxer
/// leaves inside OpusHead; the upstream libopus adapter requires it separately.
struct Mp4OpusDecoder {
    inner: OpusDecoder,
    params: CodecParameters,
}

impl SymphoniaDecoder for Mp4OpusDecoder {
    fn try_new(params: &CodecParameters, options: &DecoderOptions) -> SymphoniaResult<Self> {
        let mut params = params.clone();
        if params.channels.is_none()
            && params.channel_layout.is_none()
            && let Some(extra_data) = params.extra_data.as_ref()
            && extra_data.starts_with(b"OpusHead")
        {
            let channels = match extra_data.get(9) {
                Some(1) => Some(Channels::FRONT_LEFT),
                Some(2) => Some(Channels::FRONT_LEFT | Channels::FRONT_RIGHT),
                _ => None,
            };
            if let Some(channels) = channels {
                params.with_channels(channels);
            }
        }
        let inner = OpusDecoder::try_new(&params, options)?;
        Ok(Self { inner, params })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(CODEC_TYPE_OPUS, "opus", "Opus")]
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> SymphoniaResult<AudioBufferRef<'_>> {
        self.inner.decode(packet)
    }

    fn finalize(&mut self) -> FinalizeResult {
        self.inner.finalize()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.inner.last_decoded()
    }
}

fn decode_bytes(bytes: Vec<u8>) -> anyhow::Result<Decoder<Cursor<Vec<u8>>>> {
    let byte_len = bytes.len() as u64;
    Decoder::builder()
        .with_data(Cursor::new(bytes))
        .with_byte_len(byte_len)
        .with_seekable(true)
        .with_symphonia_decoder::<Mp4OpusDecoder>()
        .build()
        .map_err(|error| anyhow::anyhow!("failed to decode audio: {error}"))
}

impl AudioInner {
    /// Construct the output device + player on first use — the load's
    /// deferral. The pause-BEFORE-append discipline: rodio starts a newly
    /// appended source immediately, so an append-then-pause would emit a
    /// sub-frame blip — exactly the unsolicited audio the paused-load
    /// contract exists to prevent.
    fn ensure_output(&mut self) {
        if self.player.is_some() {
            return;
        }
        let Some(source) = self.pending_source.take() else {
            return;
        };
        // Lazily initialize the audio output device.
        if self.device_sink.is_none() {
            let device_sink = match DeviceSinkBuilder::open_default_sink() {
                Ok(device_sink) => device_sink,
                Err(error) => {
                    // No output device (headless): the load stays usable —
                    // a pre-play seek parks as pending, `position()` still
                    // reports it, and the next play/seek retries the open.
                    log::warn!("hkask-media-widget: audio output unavailable: {error}");
                    self.pending_source = Some(source);
                    return;
                }
            };
            let mut device_sink = device_sink;
            device_sink.log_on_drop(false);
            self.device_sink = Some(device_sink);
        }
        let Some(device_sink) = self.device_sink.as_ref() else {
            self.pending_source = Some(source);
            return;
        };
        let mixer = device_sink.mixer();
        let player = rodio::Player::connect_new(mixer);
        // Keep application gain at rodio's unity default. The operating
        // system's default output device owns volume and mute.
        player.pause();
        player.append(source);
        // A fresh Player starts at rodio's default rate — apply the stored
        // rate so it never describes a player that was already replaced.
        player.set_speed(self.speed);
        // A pre-play seek (headless, or before the first toggle) lands on
        // the constructed player.
        if let Some(target) = self.pending_seek.take() {
            if let Err(error) = player.try_seek(wall_deadline(target, self.speed)) {
                log::warn!("hkask-media-widget: audio seek failed: {error}");
            }
        }
        self.player = Some(player);
    }
}

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AudioInner {
                device_sink: None,
                pending_source: None,
                pending_seek: None,
                player: None,
                duration: Duration::ZERO,
                speed: 1.0,
            }),
        }
    }

    /// Load audio from raw bytes WITHOUT starting playback — the widget's
    /// no-unsolicited-audio policy: audio assets load paused and the
    /// operator presses play. Stops any current playback. The output
    /// device and player are NOT constructed here — the load decodes and
    /// parses duration only, deferring the device to the first play/seek,
    /// so machines with no audio hardware (headless sessions, CI runners)
    /// can still load, read durations, and serve seek positions (the
    /// eager device open failed the whole load on CI's headless runners,
    /// reddening the panel's preview pin three consecutive runs,
    /// 2026-11-07).
    pub fn load_bytes_paused(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let mut inner = self.inner.lock();

        let source = decode_bytes(bytes)?;

        let duration = source.total_duration().unwrap_or(Duration::ZERO);

        // A fresh load replaces the players (pending or live) at rodio's
        // default 1× — the deferral applies the stored rate when the
        // player is constructed.
        inner.player = None;
        inner.pending_source = Some(source);
        inner.pending_seek = None;
        inner.speed = 1.0;
        inner.duration = duration;

        Ok(())
    }

    /// Set the playback rate. rodio applies it live (re-read every ~5 ms), so
    /// a mid-playback change takes effect without rebuilding the source.
    /// The rate is resampling — pitch scales with it.
    pub fn set_speed(&self, rate: f32) {
        let mut inner = self.inner.lock();
        inner.speed = rate;
        if let Some(player) = &inner.player {
            player.set_speed(rate);
        }
    }

    pub fn pause(&self) {
        let inner = self.inner.lock();
        if let Some(player) = &inner.player {
            player.pause();
        }
    }

    pub fn toggle(&self) {
        let mut inner = self.inner.lock();
        // The deferred construction: the first play (or seek) opens the
        // output device and appends the parked source — paused, so the
        // is_paused arm below plays it.
        inner.ensure_output();
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
        inner.pending_source = None;
        inner.pending_seek = None;
        inner.duration = Duration::ZERO;
    }

    pub fn seek(&self, position: Duration) {
        let mut inner = self.inner.lock();
        inner.ensure_output();
        if let Some(player) = &inner.player
            && let Err(error) = player.try_seek(wall_deadline(position, inner.speed))
        {
            log::warn!("hkask-media-widget: audio seek failed: {error}");
        }
        if inner.player.is_none() {
            // No output device (headless) — the seek parks as pending so
            // the position still reports it and the first play lands on
            // the target.
            inner.pending_seek = Some(position);
        }
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
            // get_pos() is wall-equivalent under rodio's Speed wrapper
            // (media / rate) — scale back to media time so the transport
            // position and the transcript click-to-seek clock stay in media
            // seconds at any rate.
            .map(|player| media_elapsed(player.get_pos(), inner.speed))
            // No player yet (headless, or nothing has played): a parked
            // pre-play seek still reports its target — the transport and
            // the transcript's follow-along read media seconds either way.
            .or_else(|| inner.pending_seek)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// dcterms:identifier: `AudioPlayer::load_bytes_paused`
    /// expect: MP3, AAC/MP4, Ogg/Vorbis, FLAC, and Opus/MP4 fixtures each
    /// decode at least one sample through the shared player. [P1]
    #[test]
    fn transcript_audio_regression_formats_decode() {
        for filename in [
            "decode-sine.mp3",
            "decode-sine.m4a",
            "decode-sine.ogg",
            "decode-sine.flac",
            "decode-sine-opus.mp4",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test_data")
                .join(filename);
            let bytes = std::fs::read(&path).expect("read audio decode fixture");
            let mut source =
                decode_bytes(bytes).unwrap_or_else(|error| panic!("{filename}: {error}"));
            assert!(
                source.next().is_some(),
                "{filename}: decoder initialized but produced no samples"
            );
        }
    }

    /// The headless pin (2026-11-07, three consecutive CI reds on the
    /// panel's preview pin): loading audio must never require an audio
    /// output device. The eager device open inside `load_bytes_paused`
    /// failed the whole load on CI's headless runners — the run logs
    /// carry ALSA `cannot find card '0'` microseconds before the panel
    /// pin's "the fixture must load" gate fired — while every device-ful
    /// machine passed. The deferral contract is asserted structurally
    /// (the crate forbids unsafe code, so no in-process env games): after
    /// a load the output device and player are NOT constructed — the
    /// decoded source parks as pending, the duration parses, and a
    /// pre-play seek parks as pending with the position still reporting
    /// it. The deferred player construction at the first play/seek is
    /// exercised by the seek/speed/format suite, and the end-to-end
    /// headless receipt is the panel's preview pin under a broken
    /// ALSA_CONFIG_PATH from the shell (red before this fix, green
    /// after). GLOBAL_TEST_LOCK serializes the audio and video device
    /// families — the eager red run opens the device.
    #[test]
    fn audio_load_defers_the_output_device() {
        let _guard = crate::media_widget::tests::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let player = AudioPlayer::new();
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/playback-seek.wav"),
        )
        .expect("read the audio fixture");
        player.load_bytes_paused(bytes).expect("the load decodes");
        {
            let inner = player.inner.lock();
            assert!(
                inner.device_sink.is_none(),
                "the load never opens the output device"
            );
            assert!(
                inner.player.is_none(),
                "the load never constructs the playback player"
            );
            assert!(
                inner.pending_source.is_some(),
                "the decoded source parks as pending"
            );
        }
        assert!(
            player.duration() > Duration::ZERO,
            "the duration parses without a device"
        );
        // A pre-play seek parks as pending: the position reports the
        // target without a device, and nothing plays.
        player.seek(Duration::from_millis(100));
        assert_eq!(
            player.position(),
            Duration::from_millis(100),
            "a pre-play seek's position is reportable headless"
        );
        assert!(!player.is_playing());
    }
}
