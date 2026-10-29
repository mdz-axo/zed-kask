//! Local speaker-embedding encoder for authorized enrollment spans.
//!
//! The pinned WeSpeaker artifact declares Apache-2.0 on its Hugging Face card,
//! while WeSpeaker's pretrained-model documentation declares CC BY 4.0. Keep
//! that upstream conflict visible; the model is downloaded into the local data
//! directory and is not redistributed by this repository.

use crate::voice_profiles::EncoderFingerprint;
use crate::{FfmpegRunner, MediaError};
use kaldi_native_fbank::online::{FeatureComputer, OnlineFeature};
use kaldi_native_fbank::{FbankComputer, FbankOptions};
use ort::session::Session;
use ort::value::Tensor;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_CAM%2B%2B.onnx";
const MODEL_SHA256: &str = "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef";
const PREPROCESSING: &str =
    "16khz-mono-s16le;kaldi-fbank-80;25ms-10ms;dither-0;snip-edges-false;int16-scale";

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceEmbeddingOutput {
    pub fingerprint: EncoderFingerprint,
    pub embedding: Vec<f32>,
    pub frame_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum VoiceEncoderError {
    #[error("voice encoder media failed: {0}")]
    Media(#[from] MediaError),
    #[error("voice encoder I/O failed: {0}")]
    Io(String),
    #[error("voice encoder download failed: {0}")]
    Download(String),
    #[error("voice encoder model integrity failed: {0}")]
    Integrity(String),
    #[error("voice encoder runtime failed: {0}")]
    Runtime(String),
}

pub struct OnnxVoiceEncoder {
    _environment: ort::environment::Environment,
    session: Arc<Mutex<Session>>,
    fingerprint: EncoderFingerprint,
}

pub fn default_model_path() -> PathBuf {
    hkask_types::agent_paths::resolve_under_data_dir(Path::new(
        "mcp/media/models/wespeaker_en_voxceleb_CAM++.onnx",
    ))
}

pub async fn ensure_campp_model() -> Result<OnnxVoiceEncoder, VoiceEncoderError> {
    let path = default_model_path();
    if !path.exists() {
        let parent = path.parent().ok_or_else(|| {
            VoiceEncoderError::Io(format!("model path {} has no parent", path.display()))
        })?;
        std::fs::create_dir_all(parent)
            .map_err(|error| VoiceEncoderError::Io(error.to_string()))?;
        let bytes = reqwest::get(MODEL_URL)
            .await
            .map_err(|error| VoiceEncoderError::Download(error.to_string()))?
            .error_for_status()
            .map_err(|error| VoiceEncoderError::Download(error.to_string()))?
            .bytes()
            .await
            .map_err(|error| VoiceEncoderError::Download(error.to_string()))?;
        verify_hash(&bytes)?;
        let staged = path.with_extension("onnx.new");
        std::fs::write(&staged, &bytes)
            .map_err(|error| VoiceEncoderError::Io(error.to_string()))?;
        std::fs::rename(&staged, &path)
            .map_err(|error| VoiceEncoderError::Io(error.to_string()))?;
    }
    OnnxVoiceEncoder::open(&path)
}

impl OnnxVoiceEncoder {
    pub fn open(path: &Path) -> Result<Self, VoiceEncoderError> {
        let bytes = std::fs::read(path)
            .map_err(|error| VoiceEncoderError::Io(format!("{}: {error}", path.display())))?;
        verify_hash(&bytes)?;
        let environment = ort::init()
            .build()
            .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
        let session = Session::builder(&environment)
            .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?
            .commit_from_file(path)
            .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
        let metadata = session
            .metadata()
            .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
        require_metadata(&metadata, "sample_rate", "16000")?;
        require_metadata(&metadata, "normalize_samples", "0")?;
        let dimension = metadata
            .custom("output_dim")
            .ok_or_else(|| {
                VoiceEncoderError::Integrity("model has no output_dim metadata".to_string())
            })?
            .parse::<usize>()
            .map_err(|error| VoiceEncoderError::Integrity(error.to_string()))?;
        if dimension == 0 {
            return Err(VoiceEncoderError::Integrity(
                "model output_dim must be positive".to_string(),
            ));
        }
        drop(metadata);
        Ok(Self {
            _environment: environment,
            session: Arc::new(Mutex::new(session)),
            fingerprint: EncoderFingerprint {
                model_family: "wespeaker-voxceleb-campp".to_string(),
                weights_sha256: MODEL_SHA256.to_string(),
                preprocessing: PREPROCESSING.to_string(),
                dimension,
            },
        })
    }

    pub async fn embed_span(
        &self,
        ffmpeg: &FfmpegRunner,
        media_path: &Path,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<VoiceEmbeddingOutput, VoiceEncoderError> {
        let pcm = ffmpeg
            .decode_speech_pcm_16khz_mono(media_path, start_ms, end_ms)
            .await?;
        self.embed_pcm(pcm).await
    }

    async fn embed_pcm(&self, pcm: Vec<i16>) -> Result<VoiceEmbeddingOutput, VoiceEncoderError> {
        if pcm.is_empty() {
            return Err(VoiceEncoderError::Runtime(
                "authorized span decoded to zero PCM samples".to_string(),
            ));
        }
        let session = self.session.clone();
        let fingerprint = self.fingerprint.clone();
        tokio::task::spawn_blocking(move || {
            let samples: Vec<f32> = pcm.iter().map(|sample| f32::from(*sample)).collect();
            let (features, frame_count) = compute_fbank(samples)?;
            let input =
                Tensor::from_array(([1_usize, frame_count, 80], features.into_boxed_slice()))
                    .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
            let mut session = session
                .lock()
                .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
            let outputs = session
                .run(ort::inputs![input])
                .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
            if outputs.len() != 1 {
                return Err(VoiceEncoderError::Runtime(format!(
                    "expected one embedding output, got {}",
                    outputs.len()
                )));
            }
            let (_shape, values) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?;
            if values.len() != fingerprint.dimension
                || values.iter().any(|value| !value.is_finite())
            {
                return Err(VoiceEncoderError::Runtime(
                    "encoder output failed dimension or finite-value validation".to_string(),
                ));
            }
            Ok(VoiceEmbeddingOutput {
                fingerprint,
                embedding: values.to_vec(),
                frame_count,
            })
        })
        .await
        .map_err(|error| VoiceEncoderError::Runtime(error.to_string()))?
    }
}

fn compute_fbank(samples: Vec<f32>) -> Result<(Vec<f32>, usize), VoiceEncoderError> {
    let mut options = FbankOptions::default();
    options.use_energy = false;
    options.frame_opts.dither = 0.0;
    options.frame_opts.snip_edges = false;
    options.mel_opts.num_bins = 80;
    let computer = FbankComputer::new(options).map_err(VoiceEncoderError::Runtime)?;
    let mut online = OnlineFeature::new(FeatureComputer::Fbank(computer));
    online.accept_waveform(16_000.0, &samples);
    online.input_finished();
    let frame_count = online.features.len();
    if frame_count == 0 {
        return Err(VoiceEncoderError::Runtime(
            "fbank produced zero frames".to_string(),
        ));
    }
    let features = online.features.iter().flatten().copied().collect();
    Ok((features, frame_count))
}

fn verify_hash(bytes: &[u8]) -> Result<(), VoiceEncoderError> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != MODEL_SHA256 {
        return Err(VoiceEncoderError::Integrity(format!(
            "CAM++ weights SHA-256 is {actual}; expected {MODEL_SHA256}"
        )));
    }
    Ok(())
}

fn require_metadata(
    metadata: &ort::session::ModelMetadata<'_>,
    key: &str,
    expected: &str,
) -> Result<(), VoiceEncoderError> {
    let actual = metadata
        .custom(key)
        .ok_or_else(|| VoiceEncoderError::Integrity(format!("model has no {key} metadata")))?;
    if actual != expected {
        return Err(VoiceEncoderError::Integrity(format!(
            "model {key} is {actual}; expected {expected}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires the pinned 29 MiB CAM++ model in the media data directory"]
    async fn onnx_encoder_resamples_before_fbank_and_returns_finite_embedding() {
        let fixture = tempfile::tempdir().expect("encoder fixture directory");
        let source = fixture.path().join("8khz-stereo.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2:sample_rate=8000",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&source)
            .status()
            .await
            .expect("generate 8 kHz stereo fixture");
        assert!(status.success());
        let encoder =
            OnnxVoiceEncoder::open(&default_model_path()).expect("load the pinned CAM++ model");
        let embedded = encoder
            .embed_span(&FfmpegRunner::detect(), &source, 0, 1_500)
            .await
            .expect("resample, compute fbank, and embed");
        assert_eq!(embedded.fingerprint.dimension, 512);
        assert_eq!(
            embedded.fingerprint.weights_sha256,
            "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef"
        );
        assert!(embedded.frame_count > 0);
        assert_eq!(embedded.embedding.len(), 512);
        assert!(embedded.embedding.iter().all(|value| value.is_finite()));
    }
}
