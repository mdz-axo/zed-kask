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
//! The format selector prefers a best-video + best-audio pair capped at
//! 720p, then falls back to a progressive stream. DASH pairs are returned
//! separately so the playback worker can open both inputs.

use smol::process::Command;
use std::net::ToSocketAddrs as _;

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
    pub warning: Option<String>,
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
/// - If yt-dlp is not installed or fails → the resolution error is returned;
///   platform HTML must not be mislabeled as a direct media stream.
pub async fn resolve_stream_urls(url: &str) -> Result<StreamUrls, String> {
    validate_network_url(url).await?;
    if is_direct_video_url(url) {
        return Ok(StreamUrls {
            video: url.to_string(),
            audio: None,
            warning: None,
        });
    }

    let resolved = resolve_with_yt_dlp(url).await?;
    validate_network_url(&resolved.video).await?;
    if let Some(audio) = resolved.audio.as_deref() {
        validate_network_url(audio).await?;
    }
    Ok(resolved)
}

pub(crate) async fn validate_network_url(url: &str) -> Result<(), String> {
    let parsed = crate::media_ref::validate_remote_url_with_addresses(url, &[])
        .map_err(|error| format!("unsafe media URL: {error}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "unsafe media URL: missing host".to_string())?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "unsafe media URL: unknown port".to_string())?;
    let addresses = smol::unblock(move || {
        (host.as_str(), port)
            .to_socket_addrs()
            .map(|addresses| addresses.map(|address| address.ip()).collect::<Vec<_>>())
    })
    .await
    .map_err(|error| format!("media URL DNS resolution failed: {error}"))?;
    crate::media_ref::validate_remote_url_with_addresses(url, &addresses)
        .map(|_| ())
        .map_err(|error| format!("unsafe media URL: {error}"))
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
    let mut candidates = vec![hkask_types::ytdlp::PATH_CANDIDATE.to_string()];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(format!(
            "{home}/{}",
            hkask_types::ytdlp::USER_CANDIDATE_SUFFIX
        ));
    }
    candidates.extend(
        hkask_types::ytdlp::SYSTEM_CANDIDATES
            .iter()
            .map(|candidate| (*candidate).to_string()),
    );

    let mut best: Option<(String, Vec<u64>)> = None;
    for candidate in candidates {
        let output = Command::new(&candidate).arg("--version").output().await;
        let Ok(output) = output else { continue };
        if !output.status.success() {
            continue;
        }
        let version_text = String::from_utf8_lossy(&output.stdout).to_string();
        let Some(version) = hkask_types::ytdlp::parse_version(&version_text) else {
            continue;
        };
        let is_newer = best
            .as_ref()
            .map(|(_, current)| hkask_types::ytdlp::candidate_is_preferred(&version, current))
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
    resolve_with_yt_dlp_binary(url, &ytdlp).await
}

async fn resolve_with_yt_dlp_binary(url: &str, ytdlp: &str) -> Result<StreamUrls, String> {
    let output = Command::new(ytdlp)
        .args([
            "-g",
            "-f",
            "bv*[height<=720]+ba/b[height<=720]/b",
            "--no-playlist",
            "--no-update",
            url,
        ])
        .output()
        .await
        .map_err(|error| format!("failed to run yt-dlp: {error}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let issue = hkask_types::ytdlp::classify_stderr(&stderr)
            .unwrap_or(hkask_types::ytdlp::YtDlpIssue::Other);
        return Err(issue.actionable_message().to_string());
    }
    let warning = hkask_types::ytdlp::classify_stderr(&stderr)
        .filter(|issue| *issue != hkask_types::ytdlp::YtDlpIssue::Other)
        .map(|issue| issue.actionable_message().to_string());

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
            warning,
        }),
        // DASH: yt-dlp prints the video URL first, then the audio URL.
        [video, audio, ..] => Ok(StreamUrls {
            video: video.clone(),
            audio: Some(audio.clone()),
            warning,
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
        let url = "https://93.184.216.34/video.mp4";
        let resolved =
            smol::block_on(async { resolve_stream_urls(url).await }).expect("direct URL resolves");
        assert_eq!(
            resolved,
            StreamUrls {
                video: url.to_string(),
                audio: None,
                warning: None,
            }
        );
    }

    #[cfg(unix)]
    fn fake_ytdlp(body: &str) -> anyhow::Result<(tempfile::TempDir, String)> {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("yt-dlp");
        std::fs::write(&executable, format!("#!/bin/sh\n{body}\n"))?;
        let mut permissions = std::fs::metadata(&executable)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions)?;
        Ok((directory, executable.to_string_lossy().into_owned()))
    }

    /// expect: A platform-resolution failure remains actionable without leaking stderr.
    /// [P1] Motivating: users see why a stream cannot open, not a decoder error or signed URL.
    #[cfg(unix)]
    #[test]
    fn platform_resolution_failure_is_classified_without_host_mutation() -> anyhow::Result<()> {
        let (_directory, binary) =
            fake_ytdlp("echo 'ERROR: Sign in to confirm your age' >&2; exit 1")?;
        let error = smol::block_on(resolve_with_yt_dlp_binary(
            "https://www.youtube.com/watch?v=example",
            &binary,
        ))
        .expect_err("authorization must fail");
        assert!(error.contains("denied access"));
        assert!(!error.contains("Sign in to confirm"));
        Ok(())
    }

    /// expect: Successful degraded extraction remains visible and playback-capable.
    /// [P1] Motivating: missing runtime support must not disappear on exit status zero.
    #[cfg(unix)]
    #[test]
    fn successful_javascript_degradation_is_returned_as_warning() -> anyhow::Result<()> {
        let (_directory, binary) = fake_ytdlp(
            "echo 'WARNING: No supported JavaScript runtime could be found; some formats may be missing' >&2; echo 'https://93.184.216.34/video.mp4'",
        )?;
        let resolved = smol::block_on(resolve_with_yt_dlp_binary(
            "https://www.youtube.com/watch?v=example",
            &binary,
        ))
        .map_err(anyhow::Error::msg)?;
        assert_eq!(resolved.video, "https://93.184.216.34/video.mp4");
        assert!(
            resolved
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("JavaScript runtime"))
        );
        Ok(())
    }

    #[test]
    fn widget_uses_shared_version_and_tie_policy() {
        let current = hkask_types::ytdlp::parse_version("2026.08.19").expect("version parses");
        assert!(!hkask_types::ytdlp::candidate_is_preferred(
            &current, &current
        ));
        assert!(hkask_types::ytdlp::candidate_is_preferred(
            &[2026, 8, 20],
            &current
        ));
    }
}
