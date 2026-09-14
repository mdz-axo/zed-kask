//! Generated-asset persistence — download/decode provider results into
//! `{artifacts_dir}/media-mcp/generated/` (visible under ~/Documents/zk-data/)
//! and index them in the gallery. Generated media are user-facing outputs —
//! they belong in the open artifacts tree, not the hidden internal data dir
//! (which holds only databases/infrastructure).

use crate::error::{MediaError, map_media_error};
use crate::{GalleryState, GalleryStore};
use hkask_mcp_server::server::McpToolError;
use std::sync::Arc;

pub(crate) fn generated_assets_dir() -> std::path::PathBuf {
    let dir = hkask_types::agent_paths::resolve_under_artifacts_dir(
        &hkask_types::agent_paths::mcp_artifacts_subdir("media", "generated"),
    );
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(
            target: "hkask.mcp.media",
            path = %dir.display(),
            %error,
            "Failed to create generated-assets directory — the subsequent write will surface the failure"
        );
    }
    dir
}

/// Persist a single generated asset to the artifacts directory and index it
/// in the gallery — the extraction step of [`persist_and_slim_result`],
/// the composition helper every media tool routes its `media_generate`
/// result through.
///
/// Downloads the asset from a URL or decodes a base64 payload, saves it to
/// `{artifacts_dir}/media-mcp/generated/{uuid}.{ext}`, and registers it in
/// the gallery store (a gallery-less persist still returns
/// the path, with a warning naming the skipped indexing). Returns the local
/// file path on success.
///
/// Takes an immutable activation snapshot so neither downloads nor multiple
/// variants can retarget to a gallery activated while persistence is awaiting.
///
/// `kind` is "image", "video", or "audio" — determines the file extension.
/// `result` is the raw provider response JSON. The recognized shapes:
/// - `data[0].b64_json` (DeepInfra image generation)
/// - `data[0].url` (OpenRouter image generation)
/// - `audio` (TTS — base64 data URI)
/// - `video_url` (DeepInfra video — data URI)
/// - `url` (OpenRouter video — HTTP URL)
pub(crate) async fn persist_generated_asset(
    gallery: Option<&GalleryState>,
    gallery_store: &Arc<GalleryStore>,
    result: &serde_json::Value,
    kind: &str,
) -> Result<std::path::PathBuf, MediaError> {
    let asset_dir = generated_assets_dir();
    let id = uuid::Uuid::new_v4();

    // Extract the asset data from the provider response.
    let (bytes, ext) = match kind {
        "image" => {
            // DeepInfra: data[0].b64_json. The extension comes from the
            // decoded bytes, never a hardcoded label — DeepInfra's FLUX
            // serve returns JPEG, and the old `(bytes, "png")` hardcode
            // saved those files as .png.
            if let Some(b64) = result
                .get("data")
                .and_then(|d| d.get(0))
                .and_then(|d| d.get("b64_json"))
                .and_then(|v| v.as_str())
            {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| MediaError::AssetPersistence(format!("decode b64_json: {e}")))?;
                let ext = image_ext_from_bytes(&bytes);
                (bytes, ext)
            }
            // OpenRouter: data[0].url — download. The bytes are sniffed
            // after download; a URL's suffix is a label and can lie.
            else if let Some(url) = result
                .get("data")
                .and_then(|d| d.get(0))
                .and_then(|d| d.get("url"))
                .and_then(|v| v.as_str())
            {
                let bytes = download_asset(url).await?;
                let ext = image_ext_from_bytes(&bytes);
                (bytes, ext)
            } else {
                return Err(MediaError::AssetPersistence(format!(
                    "unrecognized {kind} provider response shape: no \
                     data[0].b64_json / data[0].url field"
                )));
            }
        }
        "video" => {
            // DeepInfra: video_url (data URI)
            if let Some(url) = result.get("video_url").and_then(|v| v.as_str()) {
                if url.starts_with("data:") {
                    decode_data_uri(url)?
                } else {
                    let bytes = download_asset(url).await?;
                    (bytes, "mp4")
                }
            }
            // OpenRouter: url
            else if let Some(url) = result.get("url").and_then(|v| v.as_str()) {
                let bytes = download_asset(url).await?;
                (bytes, "mp4")
            } else {
                return Err(MediaError::AssetPersistence(format!(
                    "unrecognized {kind} provider response shape: no video_url / url field"
                )));
            }
        }
        "audio" => {
            // TTS: audio field (data URI)
            if let Some(audio) = result.get("audio").and_then(|v| v.as_str()) {
                decode_data_uri(audio)?
            } else {
                return Err(MediaError::AssetPersistence(format!(
                    "unrecognized {kind} provider response shape: no audio field"
                )));
            }
        }
        _ => {
            return Err(MediaError::AssetPersistence(format!(
                "unknown asset kind '{kind}' (expected image, video, or audio)"
            )));
        }
    };

    let filename = format!("{id}.{ext}");
    let path = asset_dir.join(&filename);

    // Write the file.
    if let Err(e) = std::fs::write(&path, &bytes) {
        return Err(MediaError::AssetPersistence(format!(
            "write {}: {e}",
            path.display()
        )));
    }
    tracing::info!(
        target: "hkask.mcp.media",
        path = %path.display(),
        kind,
        "Generated asset persisted"
    );

    // Gallery-less generation still returns its saved path with a warning.
    // A configured gallery's persistence failure propagates with that saved path.
    let media_type = match kind {
        "video" => "video",
        "audio" => "audio",
        _ => "image",
    };
    let Some(gallery_id) = gallery.and_then(|state| state.gallery_id.as_deref()) else {
        tracing::warn!(target: "hkask.mcp.media", "Gallery not initialized — generated {media_type} not indexed");
        return Ok(path);
    };
    let hash = {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    let (width, height) = if media_type == "image" {
        infer_image_dimensions(&bytes)
    } else {
        (0, 0)
    };
    gallery_store
        .add_media(
            gallery_id,
            &path.to_string_lossy(),
            &hash,
            width,
            height,
            ext,
            bytes.len() as u64,
            media_type,
        )
        .map_err(|error| {
            MediaError::AssetPersistence(format!(
                "File saved at {}, but indexing failed: {error}",
                path.display()
            ))
        })?;

    Ok(path)
}

/// A job asset whose final file and gallery row remain rollback-armed until the
/// job controller atomically accepts completion over cancellation.
pub(crate) struct StagedJobPublication {
    assets: Vec<StagedJobAsset>,
    provider_metadata: serde_json::Map<String, serde_json::Value>,
}

struct StagedJobAsset {
    staged_path: std::path::PathBuf,
    final_path: std::path::PathBuf,
    bytes: Vec<u8>,
    ext: String,
    media_type: &'static str,
    gallery_store: Option<Arc<GalleryStore>>,
    gallery_image_id: Option<String>,
    committed: bool,
}

struct StagedPathCleanup {
    path: Option<std::path::PathBuf>,
}

impl StagedPathCleanup {
    fn armed(path: std::path::PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for StagedPathCleanup {
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            return;
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.media.jobs",
                    path = %path.display(),
                    %error,
                    "Failed to clean up partially written staged asset"
                );
            }
        }
    }
}

impl StagedJobAsset {
    fn publish(
        &mut self,
        gallery: Option<&GalleryState>,
        gallery_store: &Arc<GalleryStore>,
    ) -> Result<(), MediaError> {
        std::fs::rename(&self.staged_path, &self.final_path).map_err(|error| {
            MediaError::AssetPersistence(format!("publish {}: {error}", self.final_path.display()))
        })?;

        let Some(gallery_id) = gallery.and_then(|state| state.gallery_id.as_deref()) else {
            tracing::warn!(
                target: "hkask.mcp.media",
                "Gallery not initialized — generated {} not indexed",
                self.media_type
            );
            return Ok(());
        };
        let hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(&self.bytes);
            format!("{:x}", hasher.finalize())
        };
        let (width, height) = if self.media_type == "image" {
            infer_image_dimensions(&self.bytes)
        } else {
            (0, 0)
        };
        let record = gallery_store
            .add_media(
                gallery_id,
                &self.final_path.to_string_lossy(),
                &hash,
                width,
                height,
                &self.ext,
                self.bytes.len() as u64,
                self.media_type,
            )
            .map_err(|error| {
                MediaError::AssetPersistence(format!(
                    "index staged asset {}: {error}",
                    self.final_path.display()
                ))
            })?;
        self.gallery_store = Some(gallery_store.clone());
        self.gallery_image_id = Some(record.id);
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), MediaError> {
        let mut first_error = None;
        if let (Some(store), Some(image_id)) =
            (self.gallery_store.as_ref(), self.gallery_image_id.clone())
        {
            match store.delete_image(&image_id) {
                Ok(()) => self.gallery_image_id = None,
                Err(error) => {
                    first_error = Some(format!("delete gallery row {image_id}: {error}"));
                }
            }
        }
        for path in [&self.final_path, &self.staged_path] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if first_error.is_none() => {
                    first_error = Some(format!("delete {}: {error}", path.display()));
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.media.jobs",
                        path = %path.display(),
                        %error,
                        "Additional staged publication rollback failure"
                    );
                }
            }
        }
        if let Some(error) = first_error {
            return Err(MediaError::AssetPersistence(format!(
                "staged publication rollback failed: {error}"
            )));
        }
        Ok(())
    }
}

impl Drop for StagedJobAsset {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Err(error) = self.rollback() {
            tracing::warn!(
                target: "hkask.mcp.media.jobs",
                path = %self.final_path.display(),
                %error,
                "Failed to roll back uncommitted staged publication"
            );
        }
    }
}

impl StagedJobPublication {
    /// Publish final paths and gallery rows while retaining rollback ownership.
    pub(crate) fn publish_and_slim(
        &mut self,
        gallery: Option<&GalleryState>,
        gallery_store: &Arc<GalleryStore>,
    ) -> Result<serde_json::Value, MediaError> {
        for asset in &mut self.assets {
            if let Err(error) = asset.publish(gallery, gallery_store) {
                let rollback_error = self.rollback().err();
                return match rollback_error {
                    Some(rollback_error) => Err(MediaError::AssetPersistence(format!(
                        "{error}; {rollback_error}"
                    ))),
                    None => Err(error),
                };
            }
        }
        let Some(output_path) = self.assets.first().map(|asset| &asset.final_path) else {
            return Err(MediaError::AssetPersistence(
                "no assets staged — empty provider response".to_string(),
            ));
        };
        let mut slim = self.provider_metadata.clone();
        slim.insert(
            "output".to_string(),
            serde_json::Value::String(output_path.to_string_lossy().into_owned()),
        );
        if self.assets.len() > 1 {
            slim.insert(
                "outputs".to_string(),
                serde_json::Value::Array(
                    self.assets
                        .iter()
                        .map(|asset| {
                            serde_json::Value::String(
                                asset.final_path.to_string_lossy().into_owned(),
                            )
                        })
                        .collect(),
                ),
            );
        }
        Ok(serde_json::Value::Object(slim))
    }

    pub(crate) fn gallery_asset_id(&self) -> Option<&str> {
        self.assets
            .first()
            .and_then(|asset| asset.gallery_image_id.as_deref())
    }

    pub(crate) fn commit(&mut self) {
        for asset in &mut self.assets {
            asset.committed = true;
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), MediaError> {
        let mut first_error = None;
        for asset in &mut self.assets {
            if let Err(error) = asset.rollback() {
                if first_error.is_none() {
                    first_error = Some(error);
                } else {
                    tracing::warn!(
                        target: "hkask.mcp.media.jobs",
                        %error,
                        "Additional staged publication rollback failure"
                    );
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

/// Decode or download provider payloads and write hidden staging files. No
/// final output path or gallery row is visible until `publish_and_slim` runs.
pub(crate) async fn stage_job_publication(
    result: &serde_json::Value,
    kind: &str,
) -> Result<StagedJobPublication, MediaError> {
    let entries = if kind == "image" {
        result
            .get("data")
            .and_then(|data| data.as_array())
            .filter(|entries| entries.len() > 1)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| serde_json::json!({ "data": [entry] }))
                    .collect::<Vec<_>>()
            })
    } else {
        None
    }
    .unwrap_or_else(|| vec![result.clone()]);

    let mut assets = Vec::with_capacity(entries.len());
    for entry in entries {
        assets.push(stage_job_asset(&entry, kind).await?);
    }
    let mut provider_metadata = serde_json::Map::new();
    if let Some(object) = result.as_object() {
        for (field, value) in object {
            if !matches!(field.as_str(), "data" | "video_url" | "url" | "audio") {
                provider_metadata.insert(field.clone(), value.clone());
            }
        }
    }
    Ok(StagedJobPublication {
        assets,
        provider_metadata,
    })
}

async fn stage_job_asset(
    result: &serde_json::Value,
    kind: &str,
) -> Result<StagedJobAsset, MediaError> {
    stage_job_asset_in_dir(result, kind, &generated_assets_dir()).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LocalMediaFormat {
    Mp4,
    Gif,
    Wav,
}

impl LocalMediaFormat {
    pub(crate) fn parse_video_output(value: &str) -> Option<Self> {
        match value {
            "mp4" => Some(Self::Mp4),
            "gif" => Some(Self::Gif),
            _ => None,
        }
    }

    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Gif => "gif",
            Self::Wav => "wav",
        }
    }

    pub(crate) const fn media_type(self) -> &'static str {
        match self {
            Self::Mp4 => "video",
            Self::Gif => "image",
            Self::Wav => "audio",
        }
    }

    pub(crate) const fn block_kind(self) -> &'static str {
        self.media_type()
    }
}

/// Move a completed local processor output under the same rollback-armed
/// publication owner used by background generation jobs. The processor's
/// temporary file is consumed before this returns; no temp-runner lifetime is
/// allowed to own the user-facing output.
pub(crate) fn stage_local_media_publication(
    source_path: &std::path::Path,
    format: LocalMediaFormat,
) -> Result<StagedJobPublication, MediaError> {
    let mut source_cleanup = StagedPathCleanup::armed(source_path.to_path_buf());
    let bytes = std::fs::read(source_path).map_err(|error| {
        MediaError::AssetPersistence(format!(
            "read processor output {}: {error}",
            source_path.display()
        ))
    })?;
    let ext = format.extension().to_string();

    let asset_dir = generated_assets_dir();
    let id = uuid::Uuid::new_v4();
    let final_path = asset_dir.join(format!("{id}.{ext}"));
    let staged_path = asset_dir.join(format!(".{id}.{ext}.staged"));
    let mut staged_path_cleanup = StagedPathCleanup::armed(staged_path.clone());
    write_staged_asset(&staged_path, &bytes).map_err(|error| {
        MediaError::AssetPersistence(format!("stage {}: {error}", staged_path.display()))
    })?;
    std::fs::remove_file(source_path).map_err(|error| {
        MediaError::AssetPersistence(format!(
            "consume processor output {}: {error}",
            source_path.display()
        ))
    })?;
    source_cleanup.disarm();

    let asset = StagedJobAsset {
        staged_path,
        final_path,
        bytes,
        ext,
        media_type: format.media_type(),
        gallery_store: None,
        gallery_image_id: None,
        committed: false,
    };
    staged_path_cleanup.disarm();
    Ok(StagedJobPublication {
        assets: vec![asset],
        provider_metadata: serde_json::Map::new(),
    })
}

fn rollback_local_publication_error(
    publication: &mut StagedJobPublication,
    original: MediaError,
) -> McpToolError {
    let error = match publication.rollback() {
        Ok(()) => original,
        Err(rollback_error) => {
            MediaError::AssetPersistence(format!("{original}; {rollback_error}"))
        }
    };
    map_media_error(error)
}

/// expect: My locally processed media is published under one durable gallery identity.
/// [P1] Motivating: user work survives processor and server teardown.
/// pre: a local processor produced a supported final format and an admission-time gallery was captured.
/// post: file, gallery row, lineage, result id, provenance, and media-block id commit or roll back together.
/// [P1] Constraining: source paths and indices never become parent identities.
pub(crate) fn publish_local_media<T: serde::Serialize + ?Sized>(
    gallery: &GalleryState,
    gallery_store: &Arc<GalleryStore>,
    output: &std::path::Path,
    op: &str,
    status: &str,
    format: LocalMediaFormat,
    effective_params: &T,
) -> Result<serde_json::Value, McpToolError> {
    let mut publication = stage_local_media_publication(output, format).map_err(map_media_error)?;
    let mut effective_value = match serde_json::to_value(effective_params) {
        Ok(value) => value,
        Err(error) => {
            return Err(rollback_local_publication_error(
                &mut publication,
                MediaError::AssetPersistence(format!(
                    "serialize {op} effective parameters: {error}"
                )),
            ));
        }
    };
    let Some(effective_fields) = effective_value.as_object_mut() else {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!(
                "serialize {op} effective parameters: expected an object"
            )),
        ));
    };
    effective_fields.insert(
        "format".to_string(),
        serde_json::Value::String(format.extension().to_string()),
    );

    let mut result = publication
        .publish_and_slim(Some(gallery), gallery_store)
        .map_err(map_media_error)?;
    let Some(gallery_asset_id) = publication.gallery_asset_id().map(str::to_string) else {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!(
                "{op} publication produced no gallery asset identity"
            )),
        ));
    };

    let lineage_params_json = match serde_json::to_string(&effective_value) {
        Ok(json) => json,
        Err(error) => {
            return Err(rollback_local_publication_error(
                &mut publication,
                MediaError::AssetPersistence(format!("serialize {op} lineage: {error}")),
            ));
        }
    };
    if let Err(lineage_error) = gallery_store.record_generation(
        &gallery_asset_id,
        op,
        None,
        None,
        None,
        None,
        Some(&lineage_params_json),
        None,
        None,
    ) {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!("record {op} lineage: {lineage_error}")),
        ));
    }

    let Some(effective_fields) = effective_value.as_object() else {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!(
                "compose {op} result: effective parameters must be an object"
            )),
        ));
    };
    let Some(result_object) = result.as_object_mut() else {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!(
                "compose {op} result: publication result must be an object"
            )),
        ));
    };
    result_object.extend(effective_fields.clone());
    result_object.insert(
        "status".to_string(),
        serde_json::Value::String(status.to_string()),
    );
    result_object.insert("effective_params".to_string(), effective_value.clone());
    result_object.insert(
        "gallery_asset_id".to_string(),
        serde_json::Value::String(gallery_asset_id),
    );
    let result = crate::media_block::enrich_with_omc_and_provenance(
        result,
        op,
        format.block_kind(),
        effective_value,
        None,
    );
    publication.commit();
    Ok(result)
}

async fn stage_job_asset_in_dir(
    result: &serde_json::Value,
    kind: &str,
    asset_dir: &std::path::Path,
) -> Result<StagedJobAsset, MediaError> {
    let (bytes, ext, media_type) = match kind {
        "image" => {
            if let Some(b64) = result
                .get("data")
                .and_then(|data| data.get(0))
                .and_then(|data| data.get("b64_json"))
                .and_then(|value| value.as_str())
            {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|error| {
                        MediaError::AssetPersistence(format!("decode b64_json: {error}"))
                    })?;
                let ext = image_ext_from_bytes(&bytes);
                (bytes, ext, "image")
            } else if let Some(url) = result
                .get("data")
                .and_then(|data| data.get(0))
                .and_then(|data| data.get("url"))
                .and_then(|value| value.as_str())
            {
                let bytes = download_asset(url).await?;
                let ext = image_ext_from_bytes(&bytes);
                (bytes, ext, "image")
            } else {
                return Err(MediaError::AssetPersistence(
                    "unrecognized image provider response shape: no data[0].b64_json / data[0].url field"
                        .to_string(),
                ));
            }
        }
        "video" => {
            let url = result
                .get("video_url")
                .or_else(|| result.get("url"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    MediaError::AssetPersistence(
                        "unrecognized video provider response shape: no video_url / url field"
                            .to_string(),
                    )
                })?;
            let (bytes, ext) = if url.starts_with("data:") {
                decode_data_uri(url)?
            } else {
                (download_asset(url).await?, "mp4")
            };
            (bytes, ext, "video")
        }
        "audio" => {
            let audio = result
                .get("audio")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    MediaError::AssetPersistence(
                        "unrecognized audio provider response shape: no audio field".to_string(),
                    )
                })?;
            let (bytes, ext) = decode_data_uri(audio)?;
            (bytes, ext, "audio")
        }
        _ => {
            return Err(MediaError::AssetPersistence(format!(
                "unknown asset kind '{kind}' (expected image, video, or audio)"
            )));
        }
    };

    let id = uuid::Uuid::new_v4();
    let final_path = asset_dir.join(format!("{id}.{ext}"));
    let staged_path = asset_dir.join(format!(".{id}.{ext}.staged"));
    let mut staged_path_cleanup = StagedPathCleanup::armed(staged_path.clone());
    write_staged_asset(&staged_path, &bytes).map_err(|error| {
        MediaError::AssetPersistence(format!("stage {}: {error}", staged_path.display()))
    })?;
    let asset = StagedJobAsset {
        staged_path,
        final_path,
        bytes,
        ext: ext.to_string(),
        media_type,
        gallery_store: None,
        gallery_image_id: None,
        committed: false,
    };
    staged_path_cleanup.disarm();
    Ok(asset)
}

#[cfg(test)]
const PARTIAL_WRITE_FAILURE_DIR: &str = "inject-partial-staged-write-failure";

fn write_staged_asset(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(test)]
    if path
        .parent()
        .and_then(std::path::Path::file_name)
        .is_some_and(|name| name == std::ffi::OsStr::new(PARTIAL_WRITE_FAILURE_DIR))
    {
        std::fs::write(path, &bytes[..bytes.len().min(1)])?;
        return Err(std::io::Error::other(
            "injected partial staged write failure",
        ));
    }

    std::fs::write(path, bytes)
}

/// Map a `media_generate` op string to the asset kind its result persists
/// as ("image", "video", or "audio"). Ops whose results carry no persisted
/// asset (transcription, audio-chat, structured chat) return `None` — a job
/// submitted for such an op is rejected, since there is nothing to persist.
pub(crate) fn media_op_kind(op: &str) -> Option<&'static str> {
    match op {
        "generate_image" | "image_to_image" | "remove_background" | "upscale" => Some("image"),
        "generate_video" | "image_to_video" => Some("video"),
        "generate_speech" => Some("audio"),
        _ => None,
    }
}

/// Persist a generated-media provider response and compose the slim tool
/// result every media tool returns.
///
/// Provider responses carry megabyte-scale base64 payloads. Returning the
/// raw JSON puts those payloads in the model's context — the 2026-08-31
/// context bomb: two ~65K-token base64 results plus the prompt breached the
/// 262144-token limit on the following turn. This is the single composition
/// site for every `media_generate` caller: the payload is decoded/downloaded
/// exactly once, written under `{artifacts_dir}/media-mcp/generated/`,
/// gallery-indexed, and the tool result becomes the persisted path plus the
/// provider's non-payload metadata. Multi-image responses (`data[]` with
/// several entries) persist every image — `outputs` lists each path,
/// `output` the first.
///
/// `gallery` is the caller's admission-time snapshot (see
/// `MediaServer::capture_gallery`) — never the live state handle, so neither
/// downloads nor multiple variants can retarget to a gallery activated while
/// the operation is in flight. Persist failure returns `Err` — the raw
/// payload is never the fallback.
pub(crate) async fn persist_and_slim_result(
    gallery: Option<&GalleryState>,
    gallery_store: &Arc<GalleryStore>,
    result: &serde_json::Value,
    kind: &str,
) -> Result<serde_json::Value, MediaError> {
    // Multi-image responses persist every entry — the singular persist
    // extracts only data[0], which silently dropped all but the first
    // image of a `num_images > 1` request.
    let multi_image_entries = if kind == "image" {
        result
            .get("data")
            .and_then(|data| data.as_array())
            .filter(|entries| entries.len() > 1)
    } else {
        None
    };
    let paths = match multi_image_entries {
        Some(entries) => {
            let mut paths = Vec::with_capacity(entries.len());
            for entry in entries {
                let single = serde_json::json!({ "data": [entry] });
                paths.push(persist_generated_asset(gallery, gallery_store, &single, kind).await?);
            }
            paths
        }
        None => vec![persist_generated_asset(gallery, gallery_store, result, kind).await?],
    };

    let Some(output_path) = paths.first() else {
        return Err(MediaError::AssetPersistence(
            "no assets persisted — empty provider response".to_string(),
        ));
    };
    let mut slim = serde_json::Map::new();
    slim.insert(
        "output".to_string(),
        serde_json::Value::String(output_path.to_string_lossy().into_owned()),
    );
    if paths.len() > 1 {
        slim.insert(
            "outputs".to_string(),
            serde_json::Value::Array(
                paths
                    .iter()
                    .map(|path| serde_json::Value::String(path.to_string_lossy().into_owned()))
                    .collect(),
            ),
        );
    }

    // Carry the provider's non-payload metadata (model name, usage, seed).
    // The payload fields are the exact fields `persist_generated_asset`
    // consumes — never copy them into the tool result.
    if let Some(object) = result.as_object() {
        for (field, value) in object {
            if !matches!(field.as_str(), "data" | "video_url" | "url" | "audio") {
                slim.entry(field.clone()).or_insert_with(|| value.clone());
            }
        }
    }

    Ok(serde_json::Value::Object(slim))
}

/// Persist a provider payload and compose the complete slim tool result
/// every `media_generate` tool returns: the persisted path plus the
/// provider's non-payload metadata, enriched with the OMC-tagged,
/// provenance-carrying display hint (so the media widget can dispatch the
/// "Explain" affordance and compose back the "I disagree" gesture).
///
/// The single composition path — call this instead of re-assembling
/// `persist_and_slim_result` + `enrich_with_omc_and_provenance` by hand at
/// each call site; a hand-rolled variant is how the base64 payload once
/// leaked into the model's context. `gallery` is the caller's admission-time
/// snapshot (`MediaServer::capture_gallery`).
pub(crate) async fn persist_slim_and_enrich(
    gallery: Option<&GalleryState>,
    gallery_store: &Arc<GalleryStore>,
    result: &serde_json::Value,
    tool: &str,
    kind: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, McpToolError> {
    let slim = crate::persist_and_slim_result(gallery, gallery_store, result, kind)
        .await
        .map_err(map_media_error)?;
    Ok(crate::media_block::enrich_with_omc_and_provenance(
        slim, tool, kind, args, None,
    ))
}

/// Download asset bytes from an HTTP URL.
pub(crate) async fn download_asset(url: &str) -> Result<Vec<u8>, MediaError> {
    // Use a simple reqwest GET — the media server process has network access.
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .and_then(|resp| resp.error_for_status())
        .map_err(|e| MediaError::AssetPersistence(format!("download {url}: {e}")))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| MediaError::AssetPersistence(format!("read bytes from {url}: {e}")))?;
    Ok(bytes.to_vec())
}

/// Decode a `data:{mime};base64,{data}` URI into bytes + extension.
pub(crate) fn decode_data_uri(uri: &str) -> Result<(Vec<u8>, &'static str), MediaError> {
    use base64::Engine;
    let parts: Vec<&str> = uri.splitn(2, ',').collect();
    if parts.len() != 2 {
        return Err(MediaError::AssetPersistence(
            "malformed data URI: no ',' separator".to_string(),
        ));
    }
    let header = parts[0];
    let data = parts[1];
    let ext = if header.contains("image/png") {
        "png"
    } else if header.contains("image/jpeg") || header.contains("image/jpg") {
        "jpg"
    } else if header.contains("image/webp") {
        "webp"
    } else if header.contains("image/gif") {
        "gif"
    } else if header.contains("video/mp4") {
        "mp4"
    } else if header.contains("audio/mp3") || header.contains("audio/mpeg") {
        "mp3"
    } else if header.contains("audio/wav") {
        "wav"
    } else {
        "bin"
    };
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map(|bytes| (bytes, ext))
        .map_err(|e| MediaError::AssetPersistence(format!("decode data URI base64 payload: {e}")))
}

/// Derive the image file extension from the actual bytes (magic-number
/// sniffing via the `image` crate), not from provider labels, URL suffixes,
/// or hardcoded defaults — the bytes are the only authoritative source.
///
/// PNG is the fallback because it is the default output format across
/// image-generation APIs: OpenAI's Images API and OpenRouter's Image API
/// both treat PNG as the canonical case (OpenRouter's docs call out non-PNG
/// outputs — JPEG, WebP, SVG — as per-model exceptions), while JPEG is
/// model-family-specific (BFL FLUX returns it). Every real raster image
/// carries recognizable magic bytes, so the fallback fires only for
/// unrecognizable payloads.
///
/// SVG (Recraft vector models) is text, not covered by magic-number
/// sniffing — checked separately before the binary formats.
pub(crate) fn image_ext_from_bytes(bytes: &[u8]) -> &'static str {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    if head.trim_start().starts_with('<') && head.contains("<svg") {
        return "svg";
    }
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::WebP) => "webp",
        Ok(image::ImageFormat::Gif) => "gif",
        Ok(image::ImageFormat::Bmp) => "bmp",
        Ok(image::ImageFormat::Tiff) => "tiff",
        Ok(image::ImageFormat::Avif) => "avif",
        _ => "png",
    }
}

/// Infer image dimensions from raw bytes using the `image` crate.
pub(crate) fn infer_image_dimensions(bytes: &[u8]) -> (u32, u32) {
    match image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format() {
        Ok(reader) => match reader.into_dimensions() {
            Ok((w, h)) => (w, h),
            Err(_) => (0, 0),
        },
        Err(_) => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::{PARTIAL_WRITE_FAILURE_DIR, image_ext_from_bytes, stage_job_asset_in_dir};

    #[tokio::test]
    async fn partial_staged_write_failure_leaves_no_asset_and_preserves_error() {
        let temp_dir = tempfile::tempdir().expect("create temporary assets directory");
        let asset_dir = temp_dir.path().join(PARTIAL_WRITE_FAILURE_DIR);
        std::fs::create_dir(&asset_dir).expect("create injected-failure assets directory");
        let result = serde_json::json!({
            "data": [{ "b64_json": "cGFydGlhbCBhc3NldA==" }]
        });

        let error = match stage_job_asset_in_dir(&result, "image", &asset_dir).await {
            Ok(_) => panic!("partial staged write unexpectedly succeeded"),
            Err(error) => error,
        };

        let crate::error::MediaError::AssetPersistence(message) = error else {
            panic!("unexpected error variant");
        };
        assert!(
            message.ends_with(": injected partial staged write failure"),
            "original write error was not preserved: {message}"
        );
        assert_eq!(
            std::fs::read_dir(&asset_dir)
                .expect("read injected-failure assets directory")
                .count(),
            0,
            "partial write left a staged or final asset behind"
        );
    }

    #[test]
    fn image_ext_from_bytes_sniffs_magic_numbers() {
        assert_eq!(
            image_ext_from_bytes(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "png"
        );
        assert_eq!(image_ext_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0]), "jpg");
        assert_eq!(image_ext_from_bytes(b"RIFF\x00\x00\x00\x00WEBP"), "webp");
        assert_eq!(image_ext_from_bytes(b"GIF89a"), "gif");
    }

    #[test]
    fn image_ext_from_bytes_detects_svg_text() {
        // Recraft vector models return SVG — text bytes the magic-number
        // sniffer does not cover.
        assert_eq!(
            image_ext_from_bytes(
                b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"
            ),
            "svg"
        );
        assert_eq!(image_ext_from_bytes(b"  <svg width=\"1\"/>"), "svg");
    }

    #[test]
    fn image_ext_from_bytes_defaults_to_png_for_unknown_bytes() {
        // PNG is the default output format across image-generation APIs
        // (OpenAI Images, OpenRouter's Image API); JPEG is model-family
        // specific (BFL FLUX). Real raster images always carry magic bytes,
        // so the default fires only for unrecognizable payloads.
        assert_eq!(image_ext_from_bytes(b"not an image at all"), "png");
        assert_eq!(image_ext_from_bytes(&[]), "png");
    }
}

// ── Combined tool router (P5 Essentialism — modular tool groups) ──────────
