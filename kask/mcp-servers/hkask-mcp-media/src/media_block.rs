//! Helper for formatting ```media fenced blocks in tool responses.
//!
//! When a media tool (generate_image, generate_video, etc.) returns a result,
//! it includes a `display_hint` field containing a pre-formatted ```media
//! markdown block. The model can copy this into its reply, and the D18 seam
//! in `crates/markdown` will render it as a `MediaWidget`.

/// Format a ```media fenced block for inclusion in tool responses.
///
/// ```text
/// ```media
/// {"kind":"image","src":"/path/to/image.png"}
/// ```
/// ```
pub fn media_block(kind: &str, src: &str) -> String {
    format!("```media\n{{\"kind\":\"{kind}\",\"src\":\"{src}\"}}\n```")
}

/// Format a ```media block for an image asset.
pub fn image_block(src: &str) -> String {
    media_block("image", src)
}

/// Format a ```media block for a video asset.
pub fn video_block(src: &str) -> String {
    media_block("video", src)
}

/// Format a ```media block for an audio asset.
pub fn audio_block(src: &str) -> String {
    media_block("audio", src)
}

/// Format a ```media block for an SVG asset.
pub fn svg_block(src: &str) -> String {
    media_block("svg", src)
}

/// Extract the first URL from a `media_generate` result's `output_urls`
/// array and format it as an image display hint.
pub fn image_hint_from_result(result: &serde_json::Value) -> Option<String> {
    result
        .get("output_urls")
        .and_then(|urls| urls.as_array())
        .and_then(|urls| urls.first())
        .and_then(|url| url.as_str())
        .map(image_block)
}

/// Extract the first URL from a `media_generate` result's `output_urls`
/// array and format it as a video display hint.
pub fn video_hint_from_result(result: &serde_json::Value) -> Option<String> {
    result
        .get("output_urls")
        .and_then(|urls| urls.as_array())
        .and_then(|urls| urls.first())
        .and_then(|url| url.as_str())
        .map(video_block)
}

/// Extract the first URL from a `media_generate` result's `output_urls`
/// array and format it as an audio display hint. Falls back to the `"audio"`
/// field used by speech generation (DeepInfra TTS returns a single data URI).
pub fn audio_hint_from_result(result: &serde_json::Value) -> Option<String> {
    result
        .get("output_urls")
        .and_then(|urls| urls.as_array())
        .and_then(|urls| urls.first())
        .and_then(|url| url.as_str())
        .map(audio_block)
        .or_else(|| {
            result
                .get("audio")
                .and_then(|audio| audio.as_str())
                .map(audio_block)
        })
}

/// Attach a `display_hint` field to a media tool result if a hint is available.
pub fn enrich_with_display_hint(
    mut result: serde_json::Value,
    hint: Option<String>,
) -> serde_json::Value {
    if let Some(hint) = hint {
        result["display_hint"] = serde_json::Value::String(hint);
    }
    result
}

/// Extract a file path from a JSON object's `"output"` field and format it
/// as a display hint of the given `kind` ("image", "video", "audio").
pub fn hint_from_output_path(result: &serde_json::Value, kind: &str) -> Option<String> {
    result
        .get("output")
        .and_then(|output| output.as_str())
        .map(|path| media_block(kind, path))
}

/// Extract a file path from a JSON object's `"audio_path"` field and format
/// it as an audio display hint.
pub fn audio_hint_from_path(result: &serde_json::Value) -> Option<String> {
    result
        .get("audio_path")
        .and_then(|path| path.as_str())
        .map(audio_block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_block_format() {
        let block = media_block("image", "/path/to/img.png");
        assert_eq!(
            block,
            "```media\n{\"kind\":\"image\",\"src\":\"/path/to/img.png\"}\n```"
        );
    }

    #[test]
    fn test_image_hint_from_result() {
        let result = serde_json::json!({
            "output_urls": ["https://example.com/img.png"]
        });
        let hint = image_hint_from_result(&result).unwrap();
        assert!(hint.contains("\"kind\":\"image\""));
        assert!(hint.contains("https://example.com/img.png"));
    }

    #[test]
    fn test_image_hint_from_result_empty_urls() {
        let result = serde_json::json!({"output_urls": []});
        assert!(image_hint_from_result(&result).is_none());
    }

    #[test]
    fn test_image_hint_from_result_no_urls_field() {
        let result = serde_json::json!({"status": "ok"});
        assert!(image_hint_from_result(&result).is_none());
    }

    #[test]
    fn test_video_hint_from_result() {
        let result = serde_json::json!({
            "output_urls": ["https://example.com/clip.mp4"]
        });
        let hint = video_hint_from_result(&result).unwrap();
        assert!(hint.contains("\"kind\":\"video\""));
    }

    #[test]
    fn test_audio_hint_from_result_output_urls() {
        let result = serde_json::json!({
            "output_urls": ["https://example.com/audio.mp3"]
        });
        let hint = audio_hint_from_result(&result).unwrap();
        assert!(hint.contains("\"kind\":\"audio\""));
    }

    #[test]
    fn test_audio_hint_from_result_audio_field() {
        // DeepInfra TTS returns {"audio": "data:audio/mp3;base64,..."}
        let result = serde_json::json!({
            "audio": "data:audio/mp3;base64,SUQzBAAAAAA",
            "format": "mp3"
        });
        let hint = audio_hint_from_result(&result).unwrap();
        assert!(hint.contains("\"kind\":\"audio\""));
        assert!(hint.contains("data:audio/mp3;base64,SUQzBAAAAAA"));
    }

    #[test]
    fn test_audio_hint_from_result_no_match() {
        let result = serde_json::json!({"status": "ok"});
        assert!(audio_hint_from_result(&result).is_none());
    }

    #[test]
    fn test_enrich_with_display_hint_some() {
        let result = serde_json::json!({"output": "/tmp/clip.mp4"});
        let hint = hint_from_output_path(&result, "video");
        let enriched = enrich_with_display_hint(result, hint);
        assert!(enriched.get("display_hint").is_some());
        let hint_str = enriched["display_hint"].as_str().unwrap();
        assert!(hint_str.contains("\"kind\":\"video\""));
    }

    #[test]
    fn test_enrich_with_display_hint_none() {
        let result = serde_json::json!({"status": "ok"});
        let enriched = enrich_with_display_hint(result, None);
        assert!(enriched.get("display_hint").is_none());
    }

    #[test]
    fn test_hint_from_output_path() {
        let result = serde_json::json!({"output": "/tmp/collage.png"});
        let hint = hint_from_output_path(&result, "image").unwrap();
        assert!(hint.contains("\"kind\":\"image\""));
        assert!(hint.contains("/tmp/collage.png"));
    }

    #[test]
    fn test_hint_from_output_path_missing() {
        let result = serde_json::json!({"status": "done"});
        assert!(hint_from_output_path(&result, "video").is_none());
    }

    #[test]
    fn test_audio_hint_from_path() {
        let result = serde_json::json!({"audio_path": "/tmp/recording.wav"});
        let hint = audio_hint_from_path(&result).unwrap();
        assert!(hint.contains("\"kind\":\"audio\""));
        assert!(hint.contains("/tmp/recording.wav"));
    }

    #[test]
    fn test_audio_hint_from_path_missing() {
        let result = serde_json::json!({"text": "hello"});
        assert!(audio_hint_from_path(&result).is_none());
    }
}
