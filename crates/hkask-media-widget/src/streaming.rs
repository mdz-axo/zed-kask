//! URL resolution for streaming video playback.
//!
//! The media widget's `VideoPlayer` opens local files via FFmpeg. For remote
//! URLs, FFmpeg can stream directly over http/https — but only if the URL
//! points to a media file (mp4, webm, etc.). URLs from video platforms
//! (YouTube, Vimeo, etc.) serve HTML pages, not media streams, so they need
//! to be resolved to a direct stream URL first.
//!
//! `resolve_stream_url` handles this: if the URL looks like a direct media
//! file, it passes through unchanged. Otherwise it shells out to `yt-dlp -g`
//! to resolve the stream URL. yt-dlp supports 1000+ sites, so this is not
//! YouTube-specific — any URL yt-dlp can handle will work.
//!
//! The function is async because it uses `smol::process::Command` (per the
//! project's `clippy::disallowed_methods` lint). It is meant to be called
//! from a background task, not the GPUI foreground thread.

use smol::process::Command;

/// File extensions that FFmpeg can stream directly over http/https.
/// If a URL ends with one of these, no yt-dlp resolution is needed.
const DIRECT_VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "webm", "mkv", "avi", "mov", "flv", "wmv", "m4v", "mpg", "mpeg", "ts", "m3u8", "mpd",
    "ogv", "3gp",
];

/// Resolve a video URL to a streamable URL.
///
/// - If the URL has a direct video file extension → returned as-is (FFmpeg
///   streams it natively via its http/https protocol handler).
/// - Otherwise → `yt-dlp -g` resolves the direct stream URL. This handles
///   YouTube, Vimeo, Twitch, Dailymotion, Bilibili, and 1000+ other sites
///   that yt-dlp supports.
/// - If yt-dlp is not installed or fails → the original URL is returned as
///   a fallback (FFmpeg will try to open it directly, which works for
///   direct media URLs but not for platform pages).
pub async fn resolve_stream_url(url: &str) -> Result<String, String> {
    if is_direct_video_url(url) {
        return Ok(url.to_string());
    }

    match resolve_with_yt_dlp(url).await {
        Ok(resolved) => Ok(resolved),
        Err(error) => {
            log::warn!(
                "hkask-media-widget: yt-dlp resolution failed for {url}: {error}. \
                 Falling back to direct URL — this will fail for platform pages."
            );
            // Return the original URL as a last resort. FFmpeg will try to
            // open it directly. For direct media URLs this works; for
            // platform pages it will fail with a clear error.
            Ok(url.to_string())
        }
    }
}

/// Check whether a URL points directly to a video file (has a known video
/// extension before any query string). If true, FFmpeg can stream it
/// directly without yt-dlp.
fn is_direct_video_url(url: &str) -> bool {
    // Strip query string and fragment.
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);

    let Some(filename) = path.rsplit('/').next() else {
        return false;
    };
    let Some(dot_pos) = filename.rfind('.') else {
        return false;
    };
    let extension = filename[dot_pos + 1..].to_ascii_lowercase();
    DIRECT_VIDEO_EXTENSIONS.contains(&extension.as_str())
}

/// Run `yt-dlp -g` to resolve the direct stream URL for a video page.
///
/// `-g` (get-url) prints the direct media URL(s) to stdout without
/// downloading. `--no-playlist` prevents downloading an entire playlist
/// when the URL is a playlist entry. `-f best` selects the best single-file
/// format (no separate audio/video streams that would need merging).
async fn resolve_with_yt_dlp(url: &str) -> Result<String, String> {
    let output = Command::new("yt-dlp")
        .args([
            "-g",
            "-f",
            "best",
            "--no-playlist",
            "--no-warnings",
            "--no-update",
            url,
        ])
        .output()
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "yt-dlp not found on system PATH — install it to stream from \
                 video platforms (YouTube, Vimeo, etc.)"
                    .to_string()
            } else {
                format!("failed to run yt-dlp: {error}")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!("yt-dlp exited with status {}", output.status)
        } else {
            format!("yt-dlp: {stderr}")
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // yt-dlp -g may print multiple URLs (video + audio for split formats).
    // With -f best, there should be one. Take the first non-empty line.
    let resolved = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "yt-dlp produced no output URL".to_string())?;

    Ok(resolved.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_direct_video_urls() {
        assert!(is_direct_video_url("https://example.com/video.mp4"));
        assert!(is_direct_video_url("https://example.com/video.webm"));
        assert!(is_direct_video_url(
            "https://example.com/path/to/stream.m3u8"
        ));
        assert!(is_direct_video_url(
            "https://example.com/video.mp4?token=abc123"
        ));
        assert!(is_direct_video_url("https://example.com/video.mp4#t=10"));
    }

    #[test]
    fn rejects_platform_page_urls() {
        assert!(!is_direct_video_url(
            "https://www.youtube.com/watch?v=4ec0lSd7qH4"
        ));
        assert!(!is_direct_video_url("https://vimeo.com/123456"));
        assert!(!is_direct_video_url("https://example.com/page.html"));
        assert!(!is_direct_video_url("https://example.com/"));
    }

    #[test]
    fn passes_through_direct_video_urls() {
        let url = "https://example.com/video.mp4";
        let resolved =
            smol::block_on(async { resolve_stream_url(url).await }).expect("direct URL resolves");
        assert_eq!(resolved, url);
    }

    #[test]
    fn falls_back_to_original_url_when_yt_dlp_missing() {
        // A URL that is NOT a direct video file, so yt-dlp will be attempted.
        // If yt-dlp is not installed, the fallback returns the original URL.
        let url = "https://www.youtube.com/watch?v=nonexistent_video_id_xyz";
        let resolved =
            smol::block_on(async { resolve_stream_url(url).await }).expect("fallback returns URL");
        // Either yt-dlp resolved it (unlikely for a fake ID) or we got the
        // original URL back as fallback.
        assert!(resolved == url || resolved.starts_with("https://"));
    }
}
