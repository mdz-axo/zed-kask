//! Audio playback via `rodio` (already in the zed-kask workspace).
//!
//! Uses the rodio 0.22+ API: `DeviceSinkBuilder` → `MixerDeviceSink`,
//! `MixerDeviceSink::mixer()`, `Player::connect_new`, `Decoder`.

use std::io::Cursor;
use std::time::Duration;

use anyhow::Context as _;
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

/// Audio playback state and controls, backed by `rodio`.
pub struct AudioPlayer {
    inner: Mutex<AudioInner>,
}

struct AudioInner {
    device_sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    duration: Duration,
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

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AudioInner {
                device_sink: None,
                player: None,
                duration: Duration::ZERO,
            }),
        }
    }

    /// Load audio from raw bytes WITHOUT starting playback — the widget's
    /// no-unsolicited-audio policy: audio assets load paused and the
    /// operator presses play. Stops any current playback.
    pub fn load_bytes_paused(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
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

        let source = decode_bytes(bytes)?;

        let duration = source.total_duration().unwrap_or(Duration::ZERO);

        // Pause BEFORE appending so the queued source never sounds: rodio
        // starts a newly appended source immediately, so an append-then-pause
        // would emit a sub-frame blip — exactly the unsolicited audio this
        // method exists to prevent.
        let player = rodio::Player::connect_new(mixer);
        // Keep application gain at rodio's unity default. The operating
        // system's default output device owns volume and mute.
        player.pause();
        player.append(source);

        inner.player = Some(player);
        inner.duration = duration;
        drop(inner);

        Ok(())
    }

    pub fn pause(&self) {
        let inner = self.inner.lock();
        if let Some(player) = &inner.player {
            player.pause();
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
}
