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
    candidates
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtDlpFetchOutcome {
    pub warning: Option<String>,
}

/// yt-dlp runner with availability detection.
#[derive(Debug, Clone)]
pub struct YtDlpRunner {
    pub available: bool,
    ytdlp_path: String,
}

impl YtDlpRunner {
    #[cfg(test)]
    pub(crate) fn with_binary(ytdlp_path: String) -> Self {
        Self {
            available: true,
            ytdlp_path,
        }
    }

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
            let Some(version) = hkask_types::ytdlp::parse_version(&version_text) else {
                tracing::warn!(
                    target: "hkask.mcp.media.ytdlp",
                    path = %candidate,
                    "yt-dlp candidate returned an invalid version"
                );
                continue;
            };
            let is_newer = best
                .as_ref()
                .map(|(_, current)| hkask_types::ytdlp::candidate_is_preferred(&version, current))
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
    ) -> Result<YtDlpFetchOutcome, crate::MediaError> {
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
            .map_err(|error| {
                remove_partial_output(output_path);
                crate::MediaError::Io(format!("yt-dlp fetch failed: {error}"))
            })?;

        if !output.status.success() {
            remove_partial_output(output_path);
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
            let issue = hkask_types::ytdlp::classify_stderr(tail.trim())
                .unwrap_or(hkask_types::ytdlp::YtDlpIssue::Other);
            return Err(match issue {
                hkask_types::ytdlp::YtDlpIssue::AuthorizationFailure => {
                    crate::MediaError::YtDlpAuthorization(issue.actionable_message().to_string())
                }
                hkask_types::ytdlp::YtDlpIssue::UnavailableVideo => {
                    crate::MediaError::YtDlpVideoUnavailable(issue.actionable_message().to_string())
                }
                hkask_types::ytdlp::YtDlpIssue::ExtractorFailure => {
                    crate::MediaError::YtDlpExtractor(issue.actionable_message().to_string())
                }
                hkask_types::ytdlp::YtDlpIssue::MissingJavascriptRuntime
                | hkask_types::ytdlp::YtDlpIssue::Other => {
                    crate::MediaError::YtDlpFailed(issue.actionable_message().to_string())
                }
            });
        }

        let warning =
            hkask_types::ytdlp::classify_stderr(String::from_utf8_lossy(&output.stderr).as_ref())
                .filter(|issue| *issue != hkask_types::ytdlp::YtDlpIssue::Other)
                .map(|issue| issue.actionable_message().to_string());
        if let Some(warning) = warning.as_deref() {
            tracing::warn!(
                target: "hkask.mcp.media.ytdlp",
                warning,
                "yt-dlp completed with degraded extraction"
            );
        }
        tracing::info!(
            target: "hkask.mcp.media.ytdlp",
            output = %output_path.display(),
            "Video downloaded"
        );
        Ok(YtDlpFetchOutcome { warning })
    }
}

fn remove_partial_output(output_path: &std::path::Path) {
    match std::fs::remove_file(output_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            target: "hkask.mcp.media.ytdlp",
            path = %output_path.display(),
            %error,
            "Failed to remove partial yt-dlp output"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_uses_shared_version_and_tie_policy() {
        let current = hkask_types::ytdlp::parse_version("2026.08.19\n").expect("version parses");
        let equal = hkask_types::ytdlp::parse_version("2026.8.19").expect("equal version parses");
        assert!(!hkask_types::ytdlp::candidate_is_preferred(
            &equal, &current
        ));
        assert!(hkask_types::ytdlp::candidate_is_preferred(
            &[2026, 8, 19, 1],
            &current
        ));
    }
}
