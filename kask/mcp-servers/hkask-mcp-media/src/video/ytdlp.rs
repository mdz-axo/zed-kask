//! yt-dlp subprocess wrapper for video downloading.
//!
//! Mirrors `FfmpegRunner`'s detect-and-run pattern. Detects `yt-dlp` on
//! PATH at startup with graceful degradation. When unavailable, the
//! `video_fetch` tool returns a clear `unavailable` error.

use std::process::Stdio;
use tokio::process::Command;

/// yt-dlp runner with availability detection.
#[derive(Debug, Clone)]
pub struct YtDlpRunner {
    pub available: bool,
    ytdlp_path: String,
}

impl YtDlpRunner {
    /// Detect yt-dlp on PATH. Returns a runner with `available` set accordingly.
    pub fn detect() -> Self {
        let ytdlp_path = "yt-dlp".to_string();
        let available = {
            #[allow(clippy::disallowed_methods)]
            std::process::Command::new(&ytdlp_path)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };

        if available {
            tracing::info!(target: "hkask.mcp.media.ytdlp", "yt-dlp detected");
        } else {
            tracing::warn!(
                target: "hkask.mcp.media.ytdlp",
                "yt-dlp not found — video_fetch will be unavailable. \
                 Install via: pip install yt-dlp  (or apt install yt-dlp on Ubuntu 24.04+)"
            );
        }

        Self {
            available,
            ytdlp_path,
        }
    }

    /// Download a video from a URL to the specified output path.
    /// Uses `-f best` for a single-file best-quality download.
    /// `--no-playlist` prevents downloading entire playlists.
    pub async fn fetch(
        &self,
        url: &str,
        output_path: &std::path::Path,
    ) -> Result<(), crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }

        let output_template = output_path.to_string_lossy().to_string();

        let status = Command::new(&self.ytdlp_path)
            .arg("-f")
            .arg("best")
            .arg("--no-playlist")
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("-o")
            .arg(&output_template)
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| crate::MediaError::Io(format!("yt-dlp fetch failed: {e}")))?;

        if !status.success() {
            return Err(crate::MediaError::Io(format!(
                "yt-dlp exited with status {:?} — the URL may be unsupported, \
                 or yt-dlp may need updating (pip install --upgrade yt-dlp)",
                status.code()
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
