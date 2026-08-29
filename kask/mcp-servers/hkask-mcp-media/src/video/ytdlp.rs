//! yt-dlp subprocess wrapper for video downloading.
//!
//! Mirrors `FfmpegRunner`'s detect-and-run pattern. Detects `yt-dlp` at
//! startup with graceful degradation, preferring the newest installed
//! binary across PATH and the common pip/apt install locations (a stale
//! distro yt-dlp frequently breaks on YouTube format changes while a newer
//! `pip install --user` copy works). When unavailable, the `video_fetch`
//! tool returns a clear `unavailable` error.

use std::process::Stdio;
use tokio::process::Command;

/// Candidate yt-dlp binaries probed at startup, in priority order.
///
/// The bare `yt-dlp` (PATH lookup) is listed first so a PATH-installed
/// binary wins ties against explicit locations; `~/.local/bin` and
/// `/usr/local/bin` outrank `/usr/bin` because pip installs are typically
/// newer than the distro package.
fn candidate_paths() -> Vec<String> {
    let mut candidates = vec!["yt-dlp".to_string()];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(format!("{home}/.local/bin/yt-dlp"));
    }
    candidates.push("/usr/local/bin/yt-dlp".to_string());
    candidates.push("/usr/bin/yt-dlp".to_string());
    candidates
}

/// Parse a `--version` output line ("2026.08.19", "2026.3.17-1~ubuntu")
/// into comparable components. Each dot-segment contributes its leading
/// numeric run; suffixes ("-1~ubuntu", "+git") are ignored so distro
/// packages compare against their upstream version.
fn parse_version(output: &str) -> Vec<u64> {
    output
        .trim()
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// Compare two dotted versions component-wise; longer wins on prefix-equal
/// (2026.8.19.1 > 2026.8.19).
fn version_at_least(newer: &[u64], current: &[u64]) -> bool {
    for (n, c) in newer.iter().zip(current.iter()) {
        if n != c {
            return n > c;
        }
    }
    newer.len() >= current.len()
}

/// yt-dlp runner with availability detection.
#[derive(Debug, Clone)]
pub struct YtDlpRunner {
    pub available: bool,
    ytdlp_path: String,
}

impl YtDlpRunner {
    /// Detect the newest yt-dlp across PATH and common install locations.
    /// Returns a runner with `available` set accordingly.
    pub fn detect() -> Self {
        let mut best: Option<(String, Vec<u64>)> = None;
        for candidate in candidate_paths() {
            // Startup detection only — blocking spawn is acceptable here,
            // mirroring the ffmpeg runner's detect step.
            #[allow(clippy::disallowed_methods)]
            let output = std::process::Command::new(&candidate)
                .arg("--version")
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output();
            let Ok(output) = output else { continue };
            if !output.status.success() {
                continue;
            }
            let version_text = String::from_utf8_lossy(&output.stdout).to_string();
            let version = parse_version(&version_text);
            let is_newer = best
                .as_ref()
                .map(|(_, current)| version_at_least(&version, current))
                .unwrap_or(true);
            if is_newer {
                tracing::info!(
                    target: "hkask.mcp.media.ytdlp",
                    path = %candidate,
                    version = %version_text.trim(),
                    "yt-dlp candidate detected"
                );
                best = Some((candidate, version));
            }
        }

        match best {
            Some((ytdlp_path, _)) => {
                tracing::info!(
                    target: "hkask.mcp.media.ytdlp",
                    path = %ytdlp_path,
                    "yt-dlp selected"
                );
                Self {
                    available: true,
                    ytdlp_path,
                }
            }
            None => {
                tracing::warn!(
                    target: "hkask.mcp.media.ytdlp",
                    "yt-dlp not found — video_fetch will be unavailable. \
                     Install via: pip install yt-dlp  (or apt install yt-dlp on Ubuntu 24.04+)"
                );
                Self {
                    available: false,
                    ytdlp_path: String::new(),
                }
            }
        }
    }

    /// Download a video from a URL to the specified output path.
    ///
    /// The format selector prefers a merged best-video+best-audio pair
    /// (capped at 720p) over a single progressive file — plain `best`
    /// frequently resolves to nothing or a 144p stub on modern YouTube.
    /// `--no-playlist` prevents downloading entire playlists. stderr is
    /// fully drained (`Command::output`) both because yt-dlp writes
    /// continuous progress to it (an undrained pipe fills and wedges the
    /// child) and so failures can surface yt-dlp's actual error text.
    pub async fn fetch(
        &self,
        url: &str,
        output_path: &std::path::Path,
    ) -> Result<(), crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::YtDlpUnavailable);
        }

        let output_template = output_path.to_string_lossy().to_string();

        let output = Command::new(&self.ytdlp_path)
            .arg("--no-update")
            .arg("--no-playlist")
            .arg("-f")
            .arg("bv*[height<=720]+ba/b[height<=720]/b")
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("-o")
            .arg(&output_template)
            .arg(url)
            .output()
            .await
            .map_err(|e| crate::MediaError::Io(format!("yt-dlp fetch failed: {e}")))?;

        if !output.status.success() {
            // Keep the tail of stderr — the head is usually warnings;
            // the ERROR line the operator needs is at the end.
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail: String = stderr
                .chars()
                .rev()
                .take(2000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return Err(crate::MediaError::YtDlpFailed(format!(
                "exit {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_string(), |c| c.to_string()),
                tail.trim()
            )));
        }

        tracing::info!(
            target: "hkask.mcp.media.ytdlp",
            url = %url,
            output = %output_path.display(),
            "Video downloaded"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_components_parse_numerically() {
        assert_eq!(parse_version("2026.08.19\n"), vec![2026, 8, 19]);
        assert_eq!(parse_version("2026.3.17-1~ubuntu"), vec![2026, 3, 17]);
    }

    #[test]
    fn newer_version_outranks_older() {
        assert!(version_at_least(&[2026, 8, 19], &[2026, 3, 17]));
        assert!(!version_at_least(&[2026, 3, 17], &[2026, 8, 19]));
        // Prefix-equal: the longer (more specific) version wins.
        assert!(version_at_least(&[2026, 8, 19, 1], &[2026, 8, 19]));
        assert!(version_at_least(&[2026, 8, 19], &[2026, 8, 19]));
    }
}
