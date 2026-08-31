//! Generated-asset persistence — download/decode provider results into
//! `{artifacts_dir}/media-mcp/generated/` (visible under ~/Documents/zk-data/)
//! and index them in the gallery. Generated media are user-facing outputs —
//! they belong in the open artifacts tree, not the hidden internal data dir
//! (which holds only databases/infrastructure).

use crate::error::{MediaError, map_media_error};
use crate::{GalleryState, GalleryStore};
use hkask_mcp_server::server::McpToolError;
use std::sync::{Arc, Mutex};

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
/// the gallery store (best-effort — a gallery-less persist still returns
/// the path, with a warning naming the skipped indexing). Returns the local
/// file path on success.
///
/// Takes the gallery Arcs rather than `&MediaServer` so the background job
/// task (`job_submit`) — which cannot borrow the server — persists through
/// the same path as the synchronous tools.
///
/// `kind` is "image", "video", or "audio" — determines the file extension.
/// `result` is the raw provider response JSON. The recognized shapes:
/// - `data[0].b64_json` (DeepInfra image generation)
/// - `data[0].url` (OpenRouter image generation)
/// - `audio` (TTS — base64 data URI)
/// - `video_url` (DeepInfra video — data URI)
/// - `url` (OpenRouter video — HTTP URL)
pub(crate) async fn persist_generated_asset(
    gallery_state: &Arc<Mutex<Option<GalleryState>>>,
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

    // Index in the gallery (all media types). Best-effort: without an
    // organized gallery the asset is still persisted and its path returned —
    // the warning names the degradation so it is surfaced, never silent.
    let media_type = match kind {
        "video" => "video",
        "audio" => "audio",
        _ => "image",
    };
    let gallery_id = match gallery_state.lock() {
        Ok(guard) => match guard.as_ref().and_then(|state| state.gallery_id.clone()) {
            Some(gallery_id) => gallery_id,
            None => {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    "Gallery not initialized — generated {media_type} not indexed"
                );
                return Ok(path);
            }
        },
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.media",
                %error,
                "Gallery state lock poisoned — generated {media_type} not indexed"
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
    let (width, height) = if media_type == "image" {
        infer_image_dimensions(&bytes)
    } else {
        (0, 0)
    };
    if let Err(error) = gallery_store.add_media(
        &gallery_id,
        &filename,
        path.to_str().unwrap_or(""),
        &hash,
        width,
        height,
        ext,
        bytes.len() as u64,
        media_type,
    ) {
        tracing::warn!(
            target: "hkask.mcp.media",
            %error,
            "Failed to add generated {media_type} to gallery"
        );
    }

    Ok(path)
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
/// Persist failure returns `Err` — the raw payload is never the fallback.
pub(crate) async fn persist_and_slim_result(
    gallery_state: &Arc<Mutex<Option<GalleryState>>>,
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
                paths.push(
                    persist_generated_asset(gallery_state, gallery_store, &single, kind).await?,
                );
            }
            paths
        }
        None => vec![persist_generated_asset(gallery_state, gallery_store, result, kind).await?],
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
/// leaked into the model's context.
pub(crate) async fn persist_slim_and_enrich(
    gallery_state: &Arc<Mutex<Option<GalleryState>>>,
    gallery_store: &Arc<GalleryStore>,
    result: &serde_json::Value,
    tool: &str,
    kind: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, McpToolError> {
    let slim = persist_and_slim_result(gallery_state, gallery_store, result, kind)
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
    use super::image_ext_from_bytes;

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
