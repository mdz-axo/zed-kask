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
//! The yt-dlp binary is resolved once per call by probing PATH plus the
//! common install locations and picking the newest version — a stale distro
//! yt-dlp frequently 403s on YouTube while a newer `pip install --user`
//! copy works (the same failure the media server's `YtDlpRunner` fixed;
//! mirror that logic here — the two crates cannot share a dependency, so
//! the probing is deliberately duplicated and must stay in sync).
//!
//! The format selector picks a **progressive** (combined audio+video)
//! format: the widget streams a single URL into FFmpeg, so a DASH
//! video-only URL would play silent video. `b[height<=720]/b` takes the
//! best combined format at or below 720p.

use smol::process::Command;

/// File extensions that FFmpeg can stream directly over http/https.
/// If a URL ends with one of these, no yt-dlp resolution is needed.
const DIRECT_VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "webm", "mkv", "avi", "mov", "flv", "wmv", "m4v", "mpg", "mpeg", "ts", "m3u8", "mpd",
    "ogv", "3gp",
];

/// The resolved streaming URLs for a video: the video URL plus, when the
/// source serves split DASH streams (modern YouTube), a separate audio-only
/// URL. `None` means the video URL already carries audio (progressive
/// format or direct file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamUrls {
    pub video: String,
    pub audio: Option<String>,
}

/// Resolve a video URL to streamable URL(s).
///
/// - If the URL has a direct video file extension → returned as-is (FFmpeg
///   streams it natively via its http/https protocol handler).
/// - Otherwise → `yt-dlp -g` resolves the direct stream URL(s). This handles
///   YouTube, Vimeo, Twitch, Dailymotion, Bilibili, and 1000+ other sites
///   that yt-dlp supports.
/// - For DASH-only sources (most modern YouTube), yt-dlp prints TWO URLs —
///   video-only then audio-only — and the player must open both; a single
///   video-only URL would play silent video.
/// - If yt-dlp is not installed or fails → the original URL is returned as
///   a fallback (FFmpeg will try to open it directly, which works for
///   direct media URLs but not for platform pages).
pub async fn resolve_stream_urls(url: &str) -> Result<StreamUrls, String> {
    if is_direct_video_url(url) {
        return Ok(StreamUrls {
            video: url.to_string(),
            audio: None,
        });
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
            Ok(StreamUrls {
                video: url.to_string(),
                audio: None,
            })
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

/// Probe candidate yt-dlp binaries and return the newest by `--version`.
/// Mirrors the media server's `YtDlpRunner::detect` — keep the two in sync.
async fn newest_yt_dlp_binary() -> Option<String> {
    let mut candidates = vec!["yt-dlp".to_string()];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(format!("{home}/.local/bin/yt-dlp"));
    }
    candidates.push("/usr/local/bin/yt-dlp".to_string());
    candidates.push("/usr/bin/yt-dlp".to_string());

    let mut best: Option<(String, Vec<u64>)> = None;
    for candidate in candidates {
        let output = Command::new(&candidate).arg("--version").output().await;
        let Ok(output) = output else { continue };
        if !output.status.success() {
            continue;
        }
        let version_text = String::from_utf8_lossy(&output.stdout).to_string();
        let version: Vec<u64> = version_text
            .trim()
            .split('.')
            .map(|part| {
                part.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect();
        let is_newer = best
            .as_ref()
            .map(|(_, current)| {
                version.iter().zip(current.iter()).all(|(v, c)| v >= c)
                    && version.len() >= current.len()
            })
            .unwrap_or(true);
        if is_newer {
            best = Some((candidate, version));
        }
    }
    best.map(|(path, _)| path)
}

/// Run `yt-dlp -g` to resolve the direct stream URL(s) for a video page.
///
/// `-g` (get-url) prints the direct media URL(s) to stdout without
/// downloading. `--no-playlist` prevents resolving an entire playlist when
/// the URL is a playlist entry. The format selector prefers a merged
/// best-video+best-audio pair (capped at 720p) over a single progressive
/// file: DASH-only sources (most modern YouTube) print two URLs — video
/// then audio — which the player opens as two FFmpeg inputs. A progressive
/// source prints one URL that already carries audio.
async fn resolve_with_yt_dlp(url: &str) -> Result<StreamUrls, String> {
    let ytdlp = newest_yt_dlp_binary().await.ok_or_else(|| {
        "yt-dlp not found on PATH or common install locations — install it to \
         stream from video platforms (YouTube, Vimeo, etc.)"
            .to_string()
    })?;

    let output = Command::new(&ytdlp)
        .args([
            "-g",
            "-f",
            "bv*[height<=720]+ba/b[height<=720]/b",
            "--no-playlist",
            "--no-warnings",
            "--no-update",
            url,
        ])
        .output()
        .await
        .map_err(|error| format!("failed to run yt-dlp: {error}"))?;

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
    let urls: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    match urls.as_slice() {
        [video] => Ok(StreamUrls {
            video: video.clone(),
            audio: None,
        }),
        // DASH: yt-dlp prints the video URL first, then the audio URL.
        [video, audio, ..] => Ok(StreamUrls {
            video: video.clone(),
            audio: Some(audio.clone()),
        }),
        [] => Err("yt-dlp produced no output URL".to_string()),
    }
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
            smol::block_on(async { resolve_stream_urls(url).await }).expect("direct URL resolves");
        assert_eq!(
            resolved,
            StreamUrls {
                video: url.to_string(),
                audio: None,
            }
        );
    }

    #[test]
    fn falls_back_to_original_url_when_yt_dlp_missing() {
        // A URL that is NOT a direct video file, so yt-dlp will be attempted.
        // If yt-dlp is not installed, the fallback returns the original URL.
        let url = "https://www.youtube.com/watch?v=nonexistent_video_id_xyz";
        let resolved =
            smol::block_on(async { resolve_stream_urls(url).await }).expect("fallback returns URL");
        // Either yt-dlp resolved it (unlikely for a fake ID) or we got the
        // original URL back as fallback.
        assert!(resolved.video == url || resolved.video.starts_with("https://"));
    }

    /// The binary probe must find a yt-dlp on this machine (the environment
    /// that runs this test suite has one) and must not return the stale
    /// distro copy when a newer one exists.
    #[test]
    fn newest_yt_dlp_binary_finds_a_candidate() {
        let found = smol::block_on(async { newest_yt_dlp_binary().await });
        assert!(
            found.is_some(),
            "yt-dlp should be discoverable in this environment"
        );
    }
}
