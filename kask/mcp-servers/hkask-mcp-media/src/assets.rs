//! Generated-asset persistence — download/decode provider results into
//! `{kask_data_dir}/mcp/media/generated/` and index them in the gallery.

use crate::error::MediaError;
use crate::MediaServer;

pub(crate) fn generated_assets_dir() -> std::path::PathBuf {
    let dir = hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
        "mcp/media/generated",
    ));
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

/// Persist a generated asset to the data directory and add it to the gallery.
///
/// Downloads the asset from a URL or decodes a base64 data URI, saves it to
/// `{data_dir}/mcp/media/generated/{uuid}.{ext}`, and registers it in the
/// gallery store. Returns the local file path on success.
///
/// `kind` is "image", "video", or "audio" — determines the file extension.
/// `result` is the raw provider response JSON. The function tries multiple
/// response shapes:
/// - `data[0].b64_json` (DeepInfra image generation)
/// - `data[0].url` (OpenRouter image generation)
/// - `output_urls[0]` (legacy format)
/// - `audio` (TTS — base64 data URI)
/// - `video_url` (DeepInfra video — data URI)
/// - `url` (OpenRouter video — HTTP URL)
pub(crate) async fn persist_generated_asset(
    server: &MediaServer,
    result: &serde_json::Value,
    kind: &str,
) -> Result<std::path::PathBuf, MediaError> {
    let asset_dir = generated_assets_dir();
    let id = uuid::Uuid::new_v4();

    // Extract the asset data from the provider response.
    let (bytes, ext) = match kind {
        "image" => {
            // DeepInfra: data[0].b64_json
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
                (bytes, "png")
            }
            // OpenRouter: data[0].url — download
            else if let Some(url) = result
                .get("data")
                .and_then(|d| d.get(0))
                .and_then(|d| d.get("url"))
                .and_then(|v| v.as_str())
            {
                let bytes = download_asset(url).await?;
                let ext = infer_image_ext(url);
                (bytes, ext)
            }
            // Legacy: output_urls[0]
            else if let Some(url) = result
                .get("output_urls")
                .and_then(|u| u.as_array())
                .and_then(|u| u.first())
                .and_then(|v| v.as_str())
            {
                if url.starts_with("data:") {
                    decode_data_uri(url)?
                } else {
                    let bytes = download_asset(url).await?;
                    let ext = infer_image_ext(url);
                    (bytes, ext)
                }
            } else {
                return Err(MediaError::AssetPersistence(format!(
                    "unrecognized {kind} provider response shape: no \
                     data[0].b64_json / data[0].url / output_urls[0] field"
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

    // Add to gallery (images only — video/audio are not gallery-indexed).
    if kind == "image" {
        let ga = match server.access_gallery() {
            Ok(ga) => ga,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    error = %e,
                    "Gallery not initialized — generated image not indexed"
                );
                return Ok(path);
            }
        };
        let hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(&bytes);
            format!("{:x}", hasher.finalize())
        };
        let (width, height) = infer_image_dimensions(&bytes);
        if let Err(e) = server.gallery_store.add_image(
            &ga.gallery_id,
            &filename,
            path.to_str().unwrap_or(""),
            &hash,
            width,
            height,
            ext,
            bytes.len() as u64,
        ) {
            tracing::warn!(
                target: "hkask.mcp.media",
                error = %e,
                "Failed to add generated image to gallery"
            );
        }
    }

    Ok(path)
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

/// Infer image file extension from a URL.
pub(crate) fn infer_image_ext(url: &str) -> &'static str {
    let lower = url.to_lowercase();
    if lower.contains(".png") {
        "png"
    } else if lower.contains(".jpg") || lower.contains(".jpeg") {
        "jpg"
    } else if lower.contains(".webp") {
        "webp"
    } else if lower.contains(".gif") {
        "gif"
    } else {
        "png"
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

// ── Combined tool router (P5 Essentialism — modular tool groups) ──────────
