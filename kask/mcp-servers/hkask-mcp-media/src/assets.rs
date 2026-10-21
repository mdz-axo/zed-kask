//! Source-derived asset persistence — publish processor outputs into
//! `{artifacts_dir}/media-mcp/generated/` (visible under ~/Documents/zk-data/)
//! and index them in the gallery. Generated media are user-facing outputs —
//! they belong in the open artifacts tree, not the hidden internal data dir
//! (which holds only databases/infrastructure).

use crate::GalleryStore;
use crate::error::{MediaError, map_media_error};
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

/// Scratch intermediates for multi-step local media pipelines: every
/// intermediate file a pipeline stages before its final publication is
/// tracked here, and cleanup removes what it can while surfacing the first
/// failure (paths that failed to delete stay tracked for the Drop-time
/// retry). The label names the pipeline in error and warn messages.
pub(crate) struct ScratchIntermediates {
    paths: Vec<std::path::PathBuf>,
    label: &'static str,
}

impl ScratchIntermediates {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            paths: Vec::new(),
            label,
        }
    }

    pub(crate) fn track(&mut self, path: std::path::PathBuf) {
        self.paths.push(path);
    }

    pub(crate) fn cleanup(&mut self) -> Result<(), MediaError> {
        let mut first_error = None;
        let mut remaining = Vec::new();
        for path in std::mem::take(&mut self.paths) {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(MediaError::Io(format!(
                            "delete {} intermediate {}: {error}",
                            self.label,
                            path.display()
                        )));
                    }
                    remaining.push(path);
                }
            }
        }
        self.paths = remaining;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for ScratchIntermediates {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(
                target: "hkask.mcp.media",
                %error,
                "Failed to clean up {} intermediates",
                self.label
            );
        }
    }
}

/// The gallery a file belongs to: its canonical home folder `home` is
/// registered (idempotent), then the most specific gallery containing the
/// file wins. Files are filed by where they live, never by which gallery
/// happens to be active.
pub(crate) fn owning_gallery_id(
    gallery_store: &GalleryStore,
    file: &std::path::Path,
    home: &std::path::Path,
) -> Result<String, MediaError> {
    let home = gallery_store.open(
        &home.to_string_lossy(),
        crate::gallery::GalleryMode::ReadOnly,
    )?;
    let file = file.canonicalize()?;
    Ok(gallery_store
        .containing(&file)?
        .map_or(home.id, |gallery| gallery.id))
}

/// A processor publication whose final file and gallery row remain
/// rollback-armed until provenance and the tool result are composed.
pub(crate) struct StagedJobPublication {
    assets: Vec<StagedJobAsset>,
}

struct StagedJobAsset {
    staged_path: std::path::PathBuf,
    final_path: std::path::PathBuf,
    asset_id: String,
    task_id: String,
    created_at: String,
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
        gallery_store: &Arc<GalleryStore>,
        op: &str,
        effective_params: &serde_json::Value,
    ) -> Result<(), MediaError> {
        std::fs::rename(&self.staged_path, &self.final_path).map_err(|error| {
            MediaError::AssetPersistence(format!("publish {}: {error}", self.final_path.display()))
        })?;

        let gallery_id =
            owning_gallery_id(gallery_store, &self.final_path, &generated_assets_dir())?;
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
        let params_json = serde_json::to_string(effective_params).map_err(|error| {
            MediaError::AssetPersistence(format!("serialize {op} effective parameters: {error}"))
        })?;
        let graph = crate::omc::creation_graph(&self.asset_id, &self.task_id, &self.created_at);
        let graph_json = serde_json::to_string(&graph).map_err(|error| {
            MediaError::AssetPersistence(format!("serialize {op} OMC creation graph: {error}"))
        })?;
        let string_field = |name: &str| {
            effective_params
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        let publication = crate::gallery::AssetCreationPublication {
            asset_id: self.asset_id.clone(),
            task_id: self.task_id.clone(),
            created_at: self.created_at.clone(),
            observation: crate::gallery::AssetObservation {
                absolute_path: self.final_path.to_string_lossy().into_owned(),
                hash,
                width,
                height,
                format: self.ext.clone(),
                size_bytes: self.bytes.len() as u64,
                media_type: self.media_type.to_string(),
            },
            op: op.to_string(),
            prompt: string_field("prompt"),
            model: string_field("model"),
            provider: string_field("provider"),
            seed: effective_params
                .get("seed")
                .and_then(serde_json::Value::as_i64),
            params: Some(params_json),
            workflow_id: string_field("workflow_id"),
            parent_image_id: string_field("parent_image_id"),
            graph_json,
        };
        let records = gallery_store
            .publish_asset_creation(&gallery_id, &publication)
            .map_err(|error| {
                MediaError::AssetPersistence(format!(
                    "publish staged Asset creation {}: {error}",
                    self.final_path.display()
                ))
            })?;
        self.gallery_store = Some(gallery_store.clone());
        self.gallery_image_id = Some(records.asset.id);
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
        gallery_store: &Arc<GalleryStore>,
        op: &str,
        effective_params: &serde_json::Value,
    ) -> Result<serde_json::Value, MediaError> {
        for asset in &mut self.assets {
            if let Err(error) = asset.publish(gallery_store, op, effective_params) {
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
                "no processor outputs staged".to_string(),
            ));
        };
        let mut slim = serde_json::Map::new();
        slim.insert(
            "output".to_string(),
            serde_json::Value::String(output_path.to_string_lossy().into_owned()),
        );
        if let Some(asset_id) = self.gallery_asset_id() {
            slim.insert(
                "gallery_asset_id".to_string(),
                serde_json::Value::String(asset_id.to_string()),
            );
            slim.insert("gallery_changed".to_string(), serde_json::Value::Bool(true));
        }
        if let Some(task_id) = self.assets.first().map(|asset| asset.task_id.as_str()) {
            slim.insert(
                "omc_task_id".to_string(),
                serde_json::Value::String(task_id.to_string()),
            );
        }
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

    fn created_at(&self) -> Option<&str> {
        self.assets.first().map(|asset| asset.created_at.as_str())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LocalMediaFormat {
    Mp4,
    Gif,
    Wav,
    Png,
    Jpg,
}

impl LocalMediaFormat {
    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Gif => "gif",
            Self::Wav => "wav",
            Self::Png => "png",
            Self::Jpg => "jpg",
        }
    }

    pub(crate) const fn media_type(self) -> &'static str {
        match self {
            Self::Mp4 => "video",
            Self::Gif => "image",
            Self::Wav => "audio",
            Self::Png | Self::Jpg => "image",
        }
    }

    pub(crate) const fn block_kind(self) -> &'static str {
        self.media_type()
    }
}

/// Stage `bytes` into `asset_dir` under a fresh uuid with the rollback-armed
/// staged path — the staging block shared by local processor outputs
/// (`stage_local_media_publication`). The caller owns the returned cleanup and
/// disarms it once the built asset owns the staged path.
fn stage_staged_asset(
    asset_dir: &std::path::Path,
    ext: &str,
    bytes: &[u8],
) -> Result<(std::path::PathBuf, std::path::PathBuf, StagedPathCleanup), MediaError> {
    let id = uuid::Uuid::new_v4();
    let final_path = asset_dir.join(format!("{id}.{ext}"));
    let staged_path = asset_dir.join(format!(".{id}.{ext}.staged"));
    let staged_path_cleanup = StagedPathCleanup::armed(staged_path.clone());
    write_staged_asset(&staged_path, bytes).map_err(|error| {
        MediaError::AssetPersistence(format!("stage {}: {error}", staged_path.display()))
    })?;
    Ok((staged_path, final_path, staged_path_cleanup))
}

/// Move a completed local processor output under the rollback-armed
/// publication owner. The processor's
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
    let (staged_path, final_path, mut staged_path_cleanup) =
        stage_staged_asset(&asset_dir, &ext, &bytes)?;
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
        asset_id: uuid::Uuid::new_v4().to_string(),
        task_id: uuid::Uuid::new_v4().to_string(),
        created_at: hkask_types::time::now_rfc3339(),
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
/// pre: a local processor produced a supported final format.
/// post: file, gallery row, lineage, result id, provenance, and media-block id commit or roll back together.
/// [P1] Constraining: source paths and indices never become parent identities.
pub(crate) fn publish_local_media<T: serde::Serialize + ?Sized>(
    gallery_store: &Arc<GalleryStore>,
    output: &std::path::Path,
    op: &str,
    status: &str,
    format: LocalMediaFormat,
    effective_params: &T,
) -> Result<serde_json::Value, McpToolError> {
    publish_local_media_inner(
        gallery_store,
        output,
        op,
        status,
        format,
        effective_params,
        None,
    )
}

/// Publish a rendered transcript EDL under the canonical local-media owner,
/// recording its typed transcript/layer origin before rollback ownership is
/// released.
pub(crate) fn publish_local_media_with_transcript_render<T: serde::Serialize + ?Sized>(
    gallery_store: &Arc<GalleryStore>,
    output: &std::path::Path,
    op: &str,
    status: &str,
    format: LocalMediaFormat,
    effective_params: &T,
    transcript_id: &str,
    edl_layer_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    publish_local_media_inner(
        gallery_store,
        output,
        op,
        status,
        format,
        effective_params,
        Some((transcript_id, edl_layer_id)),
    )
}

fn publish_local_media_inner<T: serde::Serialize + ?Sized>(
    gallery_store: &Arc<GalleryStore>,
    output: &std::path::Path,
    op: &str,
    status: &str,
    format: LocalMediaFormat,
    effective_params: &T,
    transcript_render: Option<(&str, &str)>,
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
        .publish_and_slim(gallery_store, op, &effective_value)
        .map_err(map_media_error)?;
    let Some(gallery_asset_id) = publication.gallery_asset_id().map(str::to_string) else {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!(
                "{op} publication produced no gallery asset identity"
            )),
        ));
    };

    let Some(created_at) = publication.created_at().map(str::to_string) else {
        return Err(rollback_local_publication_error(
            &mut publication,
            MediaError::AssetPersistence(format!(
                "{op} publication produced no creation timestamp"
            )),
        ));
    };
    if let Some((transcript_id, edl_layer_id)) = transcript_render {
        if let Err(error) = crate::transcript_store::record_render(
            &**gallery_store.driver(),
            &gallery_asset_id,
            transcript_id,
            edl_layer_id,
            &created_at,
        ) {
            return Err(rollback_local_publication_error(
                &mut publication,
                MediaError::AssetPersistence(format!(
                    "record {op} transcript render relationship: {error}"
                )),
            ));
        }
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

/// Maximum size for a downloaded source asset (128 MiB — matches the
/// server's upload ceiling; bounds staging against runaway or malicious URLs).
const MAX_DOWNLOAD_ASSET_BYTES: usize = 128 * 1024 * 1024;
const DOWNLOAD_ASSET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) async fn validate_download_url(
    url: &str,
    allow_loopback_fixture: bool,
) -> Result<(), MediaError> {
    if allow_loopback_fixture
        && let Ok(parsed) = reqwest::Url::parse(url)
        && matches!(parsed.scheme(), "http" | "https")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && matches!(parsed.host_str(), Some("127.0.0.1" | "::1" | "[::1]"))
    {
        return Ok(());
    }
    hkask_mcp_server::server::validate_tool_url_with_dns(url)
        .await
        .map_err(|e| {
            MediaError::AssetPersistence(format!("invalid asset URL {url}: {}", e.message))
        })
}

pub(crate) async fn download_asset_with_limit(
    url: &str,
    max_bytes: usize,
    allow_loopback_fixture: bool,
) -> Result<Vec<u8>, MediaError> {
    validate_download_url(url, allow_loopback_fixture).await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect_policy(reqwest::redirect::Policy::none())
        .timeout(DOWNLOAD_ASSET_TIMEOUT)
        .build()
        .map_err(|e| MediaError::AssetPersistence(format!("build download client: {e}")))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| MediaError::AssetPersistence(format!("download {url}: {e}")))?;
    let status = resp.status();
    if status.is_redirection() {
        return Err(MediaError::AssetPersistence(format!(
            "download {url}: refusing HTTP {status} redirect"
        )));
    }
    if !status.is_success() {
        return Err(MediaError::AssetPersistence(format!(
            "download {url}: HTTP {status}"
        )));
    }
    if let Some(content_length) = resp.content_length()
        && content_length > max_bytes as u64
    {
        return Err(MediaError::AssetPersistence(format!(
            "download {url}: content-length {content_length} exceeds {max_bytes}-byte limit"
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| MediaError::AssetPersistence(format!("read bytes from {url}: {e}")))?
    {
        if chunk.len() > max_bytes.saturating_sub(bytes.len()) {
            return Err(MediaError::AssetPersistence(format!(
                "download {url}: response exceeds {max_bytes}-byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Download asset bytes from a validated HTTP(S) URL with redirect refusal,
/// timeout, and a 128 MiB size cap.
pub(crate) async fn download_asset(url: &str) -> Result<Vec<u8>, MediaError> {
    download_asset_with_limit(url, MAX_DOWNLOAD_ASSET_BYTES, cfg!(test)).await
}

/// Read raster dimensions without fully decoding the image.
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
    use super::{
        PARTIAL_WRITE_FAILURE_DIR, download_asset_with_limit, stage_staged_asset,
        validate_download_url,
    };

    #[tokio::test]
    async fn download_asset_enforces_url_validation_redirect_refusal_and_size_cap() {
        use std::io::{Read, Write};

        // 1. Production URL validation rejects loopback/private IPs and non-http schemes.
        for bad_url in [
            "http://127.0.0.1:8080/asset.png",
            "http://169.254.169.254/latest/meta-data",
            "file:///etc/passwd",
        ] {
            assert!(
                validate_download_url(bad_url, false).await.is_err(),
                "expected {bad_url} to fail production SSRF preflight"
            );
        }

        // 2. Redirects are refused rather than followed.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind redirect fixture");
        let addr = listener.local_addr().expect("redirect addr");
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept redirect request");
            let mut buf = [0u8; 512];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write redirect response");
        });
        let redirect_err = download_asset_with_limit(&format!("http://{addr}/redirect"), 64, true)
            .await
            .expect_err("redirect must be refused");
        peer.join().expect("redirect fixture thread");
        assert!(
            redirect_err.to_string().contains("refusing HTTP 302"),
            "unexpected redirect error: {redirect_err}"
        );

        // 3. Responses exceeding max_bytes fail closed.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind size fixture");
        let addr = listener.local_addr().expect("size addr");
        let peer = std::thread::spawn(move || {
            for body in [&b"0123456789"[..], &b"0123"[..]] {
                let (mut stream, _) = listener.accept().expect("accept size request");
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(header.as_bytes()).expect("write header");
                stream.write_all(body).expect("write body");
            }
        });
        let over_cap = download_asset_with_limit(&format!("http://{addr}/big"), 8, true)
            .await
            .expect_err("oversized payload must fail");
        assert!(
            over_cap.to_string().contains("exceeds 8-byte limit"),
            "unexpected size-cap error: {over_cap}"
        );
        let within_cap = download_asset_with_limit(&format!("http://{addr}/small"), 8, true)
            .await
            .expect("payload within limit succeeds");
        peer.join().expect("size fixture thread");
        assert_eq!(within_cap, b"0123");
    }

    #[test]
    fn partial_staged_write_failure_leaves_no_asset_and_preserves_error() {
        let temp_dir = tempfile::tempdir().expect("create temporary assets directory");
        let asset_dir = temp_dir.path().join(PARTIAL_WRITE_FAILURE_DIR);
        std::fs::create_dir(&asset_dir).expect("create injected-failure assets directory");
        let error = match stage_staged_asset(&asset_dir, "jpg", b"partial processor output") {
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
}

// ── Combined tool router (P5 Essentialism — modular tool groups) ──────────
