//! Durable, model-scoped voice profiles for authorized speaker enrollment.
//!
//! Voiceprints live in a separate SQLCipher database, never in the gallery,
//! transcript exports, corpus handoff, or curator memory. Matching is outside
//! this module's Phase 2 storage and lifecycle contract.

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::{DbRow, DbValue};
use hkask_storage::{Database, DatabaseError, SqliteDriver};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OperatorAuthorization {
    pub transcript_id: String,
    pub start_word: usize,
    pub end_word: usize,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EncoderFingerprint {
    pub model_family: String,
    pub weights_sha256: String,
    pub preprocessing: String,
    pub dimension: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnrollmentAudioEmbedding {
    pub transcript_id: String,
    pub start_word: usize,
    pub end_word: usize,
    pub media_path: String,
    pub gallery_asset_id: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewVoiceEnrollment {
    pub speaker_name: String,
    pub authorization: OperatorAuthorization,
    pub fingerprint: EncoderFingerprint,
    pub segments: Vec<EnrollmentAudioEmbedding>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceEmbedding {
    pub id: String,
    pub transcript_id: String,
    pub start_word: usize,
    pub end_word: usize,
    pub media_path: String,
    pub gallery_asset_id: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(skip_serializing)]
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceProfile {
    pub id: String,
    pub speaker_name: String,
    pub authorization: OperatorAuthorization,
    pub fingerprint: EncoderFingerprint,
    #[serde(skip_serializing)]
    pub centroid: Vec<f32>,
    pub segments: Vec<VoiceEmbedding>,
    pub created_at: String,
    pub last_confirmed_match_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VoiceProfileRemoval {
    pub profiles_removed: usize,
    pub embeddings_removed: usize,
}

#[derive(Debug, Clone)]
enum VoiceProfileUnavailable {
    MissingCredential(String),
    Storage(String),
}

#[derive(Clone, Default)]
pub struct VoiceProfileRuntime {
    store: Option<Arc<VoiceProfileStore>>,
    unavailable: Option<VoiceProfileUnavailable>,
    encoder: Arc<tokio::sync::Mutex<Option<Arc<crate::voice_encoder::OnnxVoiceEncoder>>>>,
}

impl VoiceProfileRuntime {
    pub fn with_store(store: VoiceProfileStore) -> Self {
        Self {
            store: Some(Arc::new(store)),
            unavailable: None,
            encoder: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn open(path: &str, passphrase: &str) -> Self {
        match VoiceProfileStore::open(path, passphrase) {
            Ok(store) => Self::with_store(store),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.media.voice",
                    path,
                    %error,
                    "Voice-profile storage is unavailable"
                );
                Self::unavailable(error.to_string())
            }
        }
    }

    pub fn unavailable(reason: String) -> Self {
        Self::degraded(VoiceProfileUnavailable::Storage(reason))
    }

    pub fn missing_credential(reason: String) -> Self {
        Self::degraded(VoiceProfileUnavailable::MissingCredential(reason))
    }

    fn degraded(reason: VoiceProfileUnavailable) -> Self {
        Self {
            store: None,
            unavailable: Some(reason),
            encoder: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn profile_store(&self) -> Result<Arc<VoiceProfileStore>, VoiceProfileError> {
        if let Some(store) = self.store.clone() {
            return Ok(store);
        }
        Err(match &self.unavailable {
            Some(VoiceProfileUnavailable::MissingCredential(reason)) => {
                VoiceProfileError::MissingCredential(reason.clone())
            }
            Some(VoiceProfileUnavailable::Storage(reason)) => {
                VoiceProfileError::Unavailable(reason.clone())
            }
            None => {
                VoiceProfileError::Unavailable("voice-profile store is not configured".to_string())
            }
        })
    }

    pub async fn encoder(
        &self,
    ) -> Result<Arc<crate::voice_encoder::OnnxVoiceEncoder>, crate::voice_encoder::VoiceEncoderError>
    {
        let mut slot = self.encoder.lock().await;
        if let Some(encoder) = slot.as_ref() {
            return Ok(encoder.clone());
        }
        let encoder = Arc::new(crate::voice_encoder::ensure_campp_model().await?);
        *slot = Some(encoder.clone());
        Ok(encoder)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VoiceProfileError {
    #[error("voice-profile database error: {0}")]
    Connection(#[from] DatabaseError),
    #[error("voice-profile driver error: {0}")]
    Driver(#[from] DbError),
    #[error("voice-profile storage unavailable: {0}")]
    Unavailable(String),
    #[error("voice-profile credential unavailable: {0}")]
    MissingCredential(String),
    #[error("invalid voice enrollment: {0}")]
    InvalidEnrollment(String),
    #[error("voice profile {profile_id} has an incompatible encoder fingerprint")]
    ModelMismatch {
        profile_id: String,
        expected: Box<EncoderFingerprint>,
        actual: Box<EncoderFingerprint>,
    },
    #[error("voice profile {0} not found")]
    ProfileNotFound(String),
}

#[derive(Clone)]
pub struct VoiceProfileStore {
    driver: Arc<dyn DatabaseDriver>,
}

impl VoiceProfileStore {
    pub fn open(path: &str, passphrase: &str) -> Result<Self, VoiceProfileError> {
        let database = Database::open(path, passphrase)?;
        let pool = database.sqlite_pool()?;
        let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_labeled(pool, path));
        let store = Self { driver };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), VoiceProfileError> {
        self.driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS voice_profiles (
                id TEXT PRIMARY KEY,
                speaker_name TEXT NOT NULL,
                model_family TEXT NOT NULL,
                weights_sha256 TEXT NOT NULL,
                preprocessing TEXT NOT NULL,
                dimension INTEGER NOT NULL,
                centroid BLOB NOT NULL,
                authorization_transcript_id TEXT NOT NULL,
                authorization_start_word INTEGER NOT NULL,
                authorization_end_word INTEGER NOT NULL,
                authorization_recorded_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_confirmed_match_at TEXT
            );
            CREATE TABLE IF NOT EXISTS voice_profile_segments (
                id TEXT PRIMARY KEY,
                profile_id TEXT NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
                transcript_id TEXT NOT NULL,
                start_word INTEGER NOT NULL,
                end_word INTEGER NOT NULL,
                media_path TEXT NOT NULL,
                gallery_asset_id TEXT,
                start_ms INTEGER NOT NULL,
                end_ms INTEGER NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_voice_profile_segments_profile
                ON voice_profile_segments(profile_id);",
        )?;
        Ok(())
    }

    pub fn enroll(
        &self,
        enrollment: NewVoiceEnrollment,
    ) -> Result<VoiceProfile, VoiceProfileError> {
        validate_enrollment(&enrollment)?;
        let created_at = hkask_types::time::now_rfc3339();
        let centroid = duration_weighted_centroid(&enrollment.segments)?;
        let profile_id = uuid::Uuid::new_v4().to_string();
        let segments: Vec<VoiceEmbedding> = enrollment
            .segments
            .iter()
            .map(|segment| VoiceEmbedding {
                id: uuid::Uuid::new_v4().to_string(),
                transcript_id: segment.transcript_id.clone(),
                start_word: segment.start_word,
                end_word: segment.end_word,
                media_path: segment.media_path.clone(),
                gallery_asset_id: segment.gallery_asset_id.clone(),
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                embedding: segment.embedding.clone(),
            })
            .collect();
        let profile = VoiceProfile {
            id: profile_id,
            speaker_name: enrollment.speaker_name.clone(),
            authorization: enrollment.authorization.clone(),
            fingerprint: enrollment.fingerprint,
            centroid,
            segments,
            created_at: created_at.clone(),
            last_confirmed_match_at: None,
        };
        self.driver.transaction(&mut |tx| {
            tx.execute(
                "INSERT INTO voice_profiles (
                    id, speaker_name, model_family, weights_sha256, preprocessing,
                    dimension, centroid, authorization_transcript_id,
                    authorization_start_word, authorization_end_word,
                    authorization_recorded_at, created_at, last_confirmed_match_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL)",
                &[
                    DbValue::Text(profile.id.clone()),
                    DbValue::Text(profile.speaker_name.clone()),
                    DbValue::Text(profile.fingerprint.model_family.clone()),
                    DbValue::Text(profile.fingerprint.weights_sha256.clone()),
                    DbValue::Text(profile.fingerprint.preprocessing.clone()),
                    DbValue::Integer(profile.fingerprint.dimension as i64),
                    DbValue::Blob(encode_embedding(&profile.centroid)),
                    DbValue::Text(profile.authorization.transcript_id.clone()),
                    DbValue::Integer(profile.authorization.start_word as i64),
                    DbValue::Integer(profile.authorization.end_word as i64),
                    DbValue::Text(profile.authorization.recorded_at.clone()),
                    DbValue::Text(created_at.clone()),
                ],
            )?;
            for segment in &profile.segments {
                tx.execute(
                    "INSERT INTO voice_profile_segments (
                        id, profile_id, transcript_id, start_word, end_word,
                        media_path, gallery_asset_id, start_ms, end_ms,
                        embedding, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    &[
                        DbValue::Text(segment.id.clone()),
                        DbValue::Text(profile.id.clone()),
                        DbValue::Text(segment.transcript_id.clone()),
                        DbValue::Integer(segment.start_word as i64),
                        DbValue::Integer(segment.end_word as i64),
                        DbValue::Text(segment.media_path.clone()),
                        optional_text(segment.gallery_asset_id.clone()),
                        DbValue::Integer(segment.start_ms as i64),
                        DbValue::Integer(segment.end_ms as i64),
                        DbValue::Blob(encode_embedding(&segment.embedding)),
                        DbValue::Text(created_at.clone()),
                    ],
                )?;
            }
            Ok(())
        })?;
        Ok(profile)
    }

    pub fn list_profiles(&self) -> Result<Vec<VoiceProfile>, VoiceProfileError> {
        let rows = self.driver.query(
            "SELECT id, speaker_name, model_family, weights_sha256, preprocessing,
                    dimension, centroid, authorization_transcript_id,
                    authorization_start_word, authorization_end_word,
                    authorization_recorded_at, created_at, last_confirmed_match_at
             FROM voice_profiles ORDER BY created_at, id",
            &[],
        )?;
        rows.iter()
            .map(|row| {
                let id = row.get_str(0)?.to_string();
                let dimension = row.get_int(5)? as usize;
                let profile = VoiceProfile {
                    id: id.clone(),
                    speaker_name: row.get_str(1)?.to_string(),
                    fingerprint: EncoderFingerprint {
                        model_family: row.get_str(2)?.to_string(),
                        weights_sha256: row.get_str(3)?.to_string(),
                        preprocessing: row.get_str(4)?.to_string(),
                        dimension,
                    },
                    centroid: decode_embedding(row.get(6)?.as_blob()?, dimension)?,
                    authorization: OperatorAuthorization {
                        transcript_id: row.get_str(7)?.to_string(),
                        start_word: row.get_int(8)? as usize,
                        end_word: row.get_int(9)? as usize,
                        recorded_at: row.get_str(10)?.to_string(),
                    },
                    segments: self.load_segments(&id)?,
                    created_at: row.get_str(11)?.to_string(),
                    last_confirmed_match_at: optional_column_text(row, 12)?,
                };
                Ok(profile)
            })
            .collect()
    }

    pub fn record_confirmed_match(
        &self,
        profile_id: &str,
        confirmed_at: &str,
    ) -> Result<(), VoiceProfileError> {
        let changed = self.driver.execute(
            "UPDATE voice_profiles SET last_confirmed_match_at = ?2 WHERE id = ?1",
            &[
                DbValue::Text(profile_id.to_string()),
                DbValue::Text(confirmed_at.to_string()),
            ],
        )?;
        if changed == 0 {
            return Err(VoiceProfileError::ProfileNotFound(profile_id.to_string()));
        }
        Ok(())
    }

    pub fn stale_profile_ids(&self, cutoff: &str) -> Result<Vec<String>, VoiceProfileError> {
        let rows = self.driver.query(
            "SELECT id FROM voice_profiles
             WHERE created_at < ?1
               AND (last_confirmed_match_at IS NULL
                    OR last_confirmed_match_at < ?1)
             ORDER BY created_at, id",
            &[DbValue::Text(cutoff.to_string())],
        )?;
        rows.iter()
            .map(|row| Ok(row.get_str(0)?.to_string()))
            .collect()
    }

    pub fn remove_profile(
        &self,
        profile_id: &str,
    ) -> Result<VoiceProfileRemoval, VoiceProfileError> {
        let mut removal = VoiceProfileRemoval {
            profiles_removed: 0,
            embeddings_removed: 0,
        };
        self.driver.transaction(&mut |tx| {
            let segment_row = tx.query_optional(
                "SELECT COUNT(*) FROM voice_profile_segments WHERE profile_id = ?1",
                &[DbValue::Text(profile_id.to_string())],
            )?;
            if let Some(row) = segment_row {
                removal.embeddings_removed = row.get_int(0)? as usize;
            }
            removal.profiles_removed = tx.execute(
                "DELETE FROM voice_profiles WHERE id = ?1",
                &[DbValue::Text(profile_id.to_string())],
            )?;
            Ok(())
        })?;
        if removal.profiles_removed == 0 {
            return Err(VoiceProfileError::ProfileNotFound(profile_id.to_string()));
        }
        Ok(removal)
    }

    pub fn require_compatible(
        &self,
        profile_id: &str,
        candidate: &EncoderFingerprint,
    ) -> Result<(), VoiceProfileError> {
        let row = self.driver.query_optional(
            "SELECT model_family, weights_sha256, preprocessing, dimension
             FROM voice_profiles WHERE id = ?1",
            &[DbValue::Text(profile_id.to_string())],
        )?;
        let Some(row) = row else {
            return Err(VoiceProfileError::ProfileNotFound(profile_id.to_string()));
        };
        let expected = EncoderFingerprint {
            model_family: row.get_str(0)?.to_string(),
            weights_sha256: row.get_str(1)?.to_string(),
            preprocessing: row.get_str(2)?.to_string(),
            dimension: row.get_int(3)? as usize,
        };
        if &expected != candidate {
            return Err(VoiceProfileError::ModelMismatch {
                profile_id: profile_id.to_string(),
                expected: Box::new(expected),
                actual: Box::new(candidate.clone()),
            });
        }
        Ok(())
    }

    fn load_segments(&self, profile_id: &str) -> Result<Vec<VoiceEmbedding>, VoiceProfileError> {
        let dimension = self.profile_dimension(profile_id)?;
        let rows = self.driver.query(
            "SELECT id, transcript_id, start_word, end_word, media_path,
                    gallery_asset_id, start_ms, end_ms, embedding
             FROM voice_profile_segments WHERE profile_id = ?1
             ORDER BY start_ms, id",
            &[DbValue::Text(profile_id.to_string())],
        )?;
        rows.iter()
            .map(|row| {
                Ok(VoiceEmbedding {
                    id: row.get_str(0)?.to_string(),
                    transcript_id: row.get_str(1)?.to_string(),
                    start_word: row.get_int(2)? as usize,
                    end_word: row.get_int(3)? as usize,
                    media_path: row.get_str(4)?.to_string(),
                    gallery_asset_id: optional_column_text(row, 5)?,
                    start_ms: row.get_int(6)? as u64,
                    end_ms: row.get_int(7)? as u64,
                    embedding: decode_embedding(row.get(8)?.as_blob()?, dimension)?,
                })
            })
            .collect()
    }

    fn profile_dimension(&self, profile_id: &str) -> Result<usize, VoiceProfileError> {
        let row = self.driver.query_optional(
            "SELECT dimension FROM voice_profiles WHERE id = ?1",
            &[DbValue::Text(profile_id.to_string())],
        )?;
        let Some(row) = row else {
            return Err(VoiceProfileError::InvalidEnrollment(format!(
                "voice profile {profile_id} not found"
            )));
        };
        Ok(row.get_int(0)? as usize)
    }
}

fn validate_enrollment(enrollment: &NewVoiceEnrollment) -> Result<(), VoiceProfileError> {
    if enrollment.speaker_name.trim().is_empty() {
        return Err(VoiceProfileError::InvalidEnrollment(
            "speaker name is blank".to_string(),
        ));
    }
    if enrollment.segments.is_empty() {
        return Err(VoiceProfileError::InvalidEnrollment(
            "at least one audio embedding is required".to_string(),
        ));
    }
    if enrollment.fingerprint.dimension == 0 {
        return Err(VoiceProfileError::InvalidEnrollment(
            "embedding dimension must be positive".to_string(),
        ));
    }
    if enrollment.authorization.start_word > enrollment.authorization.end_word {
        return Err(VoiceProfileError::InvalidEnrollment(
            "authorization word range must be ordered and nonempty".to_string(),
        ));
    }
    for segment in &enrollment.segments {
        if segment.transcript_id != enrollment.authorization.transcript_id {
            return Err(VoiceProfileError::InvalidEnrollment(
                "every segment must belong to the authorization transcript".to_string(),
            ));
        }
        if segment.start_word > segment.end_word || segment.end_ms <= segment.start_ms {
            return Err(VoiceProfileError::InvalidEnrollment(
                "word and time ranges must be ordered and nonempty".to_string(),
            ));
        }
        if segment.start_word != enrollment.authorization.start_word
            || segment.end_word != enrollment.authorization.end_word
        {
            return Err(VoiceProfileError::InvalidEnrollment(
                "every segment must equal the exact authorized word range".to_string(),
            ));
        }
        if segment.embedding.len() != enrollment.fingerprint.dimension
            || segment.embedding.iter().any(|value| !value.is_finite())
        {
            return Err(VoiceProfileError::InvalidEnrollment(
                "embedding dimension or finite-value validation failed".to_string(),
            ));
        }
    }
    Ok(())
}

fn duration_weighted_centroid(
    segments: &[EnrollmentAudioEmbedding],
) -> Result<Vec<f32>, VoiceProfileError> {
    let dimension = segments[0].embedding.len();
    let mut sums = vec![0.0_f64; dimension];
    let mut total_duration = 0_u64;
    for segment in segments {
        let duration = segment.end_ms - segment.start_ms;
        total_duration += duration;
        for (index, value) in segment.embedding.iter().enumerate() {
            sums[index] += f64::from(*value) * duration as f64;
        }
    }
    if total_duration == 0 {
        return Err(VoiceProfileError::InvalidEnrollment(
            "enrollment audio has zero duration".to_string(),
        ));
    }
    Ok(sums
        .iter()
        .map(|sum| (*sum / total_duration as f64) as f32)
        .collect())
}

fn encode_embedding(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_embedding(bytes: &[u8], dimension: usize) -> Result<Vec<f32>, VoiceProfileError> {
    if bytes.len() != dimension * std::mem::size_of::<f32>() {
        return Err(VoiceProfileError::InvalidEnrollment(format!(
            "stored embedding has {} bytes; expected {}",
            bytes.len(),
            dimension * std::mem::size_of::<f32>()
        )));
    }
    Ok(bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| {
            let mut bytes = [0_u8; 4];
            bytes.copy_from_slice(chunk);
            f32::from_le_bytes(bytes)
        })
        .collect())
}

fn optional_text(value: Option<String>) -> DbValue {
    value.map(DbValue::Text).unwrap_or(DbValue::Null)
}

fn optional_column_text(row: &DbRow, index: usize) -> Result<Option<String>, DbError> {
    match row.get(index)? {
        DbValue::Null => Ok(None),
        DbValue::Text(value) => Ok(Some(value.clone())),
        other => Err(DbError::Database(format!(
            "expected nullable text, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enrollment() -> NewVoiceEnrollment {
        NewVoiceEnrollment {
            speaker_name: "John F. Kennedy".to_string(),
            authorization: OperatorAuthorization {
                transcript_id: "transcript-jfk".to_string(),
                start_word: 5,
                end_word: 13,
                recorded_at: "2026-10-28T23:00:00Z".to_string(),
            },
            fingerprint: EncoderFingerprint {
                model_family: "wespeaker-voxceleb-campp".to_string(),
                weights_sha256: "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef"
                    .to_string(),
                preprocessing:
                    "16khz-mono-s16le;kaldi-fbank-80;25ms-10ms;dither-0;snip-edges-false"
                        .to_string(),
                dimension: 3,
            },
            segments: vec![EnrollmentAudioEmbedding {
                transcript_id: "transcript-jfk".to_string(),
                start_word: 5,
                end_word: 13,
                media_path: "/recordings/jfk.wav".to_string(),
                gallery_asset_id: Some("asset-jfk".to_string()),
                start_ms: 3_339,
                end_ms: 7_380,
                embedding: vec![0.25, -0.5, 1.0],
            }],
        }
    }

    #[test]
    fn encrypted_reopen_round_trips_voice_profile_and_embeddings() {
        let fixture = tempfile::tempdir().expect("voice DB fixture directory");
        let path = fixture.path().join("voice-profiles.db");
        let path_string = path.to_string_lossy().into_owned();
        let profile_id = {
            let store = VoiceProfileStore::open(&path_string, "test-passphrase")
                .expect("open encrypted voice store");
            store.enroll(enrollment()).expect("persist enrollment").id
        };
        let reopened = VoiceProfileStore::open(&path_string, "test-passphrase")
            .expect("reopen encrypted voice store");
        let profiles = reopened.list_profiles().expect("list profiles");
        assert_eq!(profiles.len(), 1);
        let profile = &profiles[0];
        assert_eq!(profile.id, profile_id);
        assert_eq!(profile.speaker_name, "John F. Kennedy");
        assert_eq!(profile.authorization.start_word, 5);
        assert_eq!(profile.authorization.end_word, 13);
        assert_eq!(profile.segments.len(), 1);
        assert_eq!(profile.segments[0].embedding, vec![0.25, -0.5, 1.0]);
        assert_eq!(profile.centroid, vec![0.25, -0.5, 1.0]);
        assert_eq!(profile.last_confirmed_match_at, None);
    }

    #[test]
    fn stale_candidates_spare_recent_confirmation() {
        let fixture = tempfile::tempdir().expect("voice DB fixture directory");
        let path = fixture.path().join("voice-profiles.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open encrypted voice store");
        let stale = store.enroll(enrollment()).expect("persist stale profile");
        let recent = store.enroll(enrollment()).expect("persist recent profile");
        store
            .record_confirmed_match(&recent.id, "2999-01-01T00:00:00Z")
            .expect("record confirmation");
        let candidates = store
            .stale_profile_ids("2998-01-01T00:00:00Z")
            .expect("list stale profiles");
        assert_eq!(candidates, vec![stale.id]);
        assert!(!candidates.contains(&recent.id));
    }

    #[test]
    fn enrollment_rejects_a_segment_outside_the_exact_authorized_range() {
        let fixture = tempfile::tempdir().expect("voice DB fixture directory");
        let path = fixture.path().join("voice-profiles.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open encrypted voice store");
        let mut enrollment = enrollment();
        enrollment.segments[0].end_word -= 1;
        let error = store
            .enroll(enrollment)
            .expect_err("segment outside the authorized range must be rejected");
        assert!(
            matches!(error, VoiceProfileError::InvalidEnrollment(_)),
            "expected an enrollment invariant error, got {error}"
        );
    }

    #[test]
    fn removal_deletes_the_profile_and_every_embedding() {
        let fixture = tempfile::tempdir().expect("voice DB fixture directory");
        let path = fixture.path().join("voice-profiles.db");
        let path_string = path.to_string_lossy().into_owned();
        let profile_id = {
            let store = VoiceProfileStore::open(&path_string, "test-passphrase")
                .expect("open encrypted voice store");
            let profile = store.enroll(enrollment()).expect("persist enrollment");
            let removed = store.remove_profile(&profile.id).expect("remove profile");
            assert_eq!(removed.profiles_removed, 1);
            assert_eq!(removed.embeddings_removed, 1);
            profile.id
        };
        let reopened = VoiceProfileStore::open(&path_string, "test-passphrase")
            .expect("reopen encrypted voice store");
        assert!(
            reopened
                .list_profiles()
                .expect("list profiles after removal")
                .is_empty(),
            "profile {profile_id} and its embeddings must be gone"
        );
    }

    #[test]
    fn comparability_rejects_a_different_weights_fingerprint() {
        let fixture = tempfile::tempdir().expect("voice DB fixture directory");
        let path = fixture.path().join("voice-profiles.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open encrypted voice store");
        let profile = store.enroll(enrollment()).expect("persist enrollment");
        store
            .require_compatible(&profile.id, &profile.fingerprint)
            .expect("identical fingerprint is comparable");
        let mismatched = EncoderFingerprint {
            weights_sha256: "different-weights".to_string(),
            ..profile.fingerprint.clone()
        };
        let error = store
            .require_compatible(&profile.id, &mismatched)
            .expect_err("different weights must not be compared");
        assert!(
            matches!(error, VoiceProfileError::ModelMismatch { .. }),
            "expected a named model mismatch, got {error}"
        );
    }
}
