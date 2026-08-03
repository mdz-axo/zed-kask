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
