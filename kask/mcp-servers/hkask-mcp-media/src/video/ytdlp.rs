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

/// The whole-fetch wall bound: a download that exceeds it is stopped, its
/// partial output removed, and the refusal names the bound — `video_fetch`
/// cannot hold a tool call open indefinitely.
pub(crate) const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// The per-stream size bound handed to yt-dlp as `--max-filesize`: each
/// downloaded stream of the ≤720p format pair is refused above 4 GiB, so
/// a runaway download cannot fill the artifacts directory.
pub(crate) const MAX_DOWNLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Candidate yt-dlp binaries probed at startup, in priority order.
///
/// The bare `yt-dlp` (PATH lookup) is listed first so a PATH-installed
/// binary wins ties against explicit locations; `~/.local/bin` and
/// `/usr/local/bin` outrank `/usr/bin` because pip installs are typically
/// newer than the distro package.
fn candidate_paths() -> Vec<String> {
    let mut candidates = vec![hkask_media::ytdlp::PATH_CANDIDATE.to_string()];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(format!(
            "{home}/{}",
            hkask_media::ytdlp::USER_CANDIDATE_SUFFIX
        ));
    }
    candidates.extend(
        hkask_media::ytdlp::SYSTEM_CANDIDATES
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
            let mut cmd = std::process::Command::new(&candidate);
            cmd.env_clear();
            for key in [
                "PATH",
                "HOME",
                "TMPDIR",
                "SSL_CERT_FILE",
                "SSL_CERT_DIR",
                "LANG",
            ] {
                if let Ok(value) = std::env::var(key) {
                    cmd.env(key, value);
                }
            }
            #[allow(clippy::disallowed_methods)]
            let output = cmd
                .arg("--version")
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output();
            let Ok(output) = output else { continue };
            if !output.status.success() {
                continue;
            }
            let version_text = String::from_utf8_lossy(&output.stdout).to_string();
            let Some(version) = hkask_media::ytdlp::parse_version(&version_text) else {
                tracing::warn!(
                    target: "hkask.mcp.media.ytdlp",
                    path = %candidate,
                    "yt-dlp candidate returned an invalid version"
                );
                continue;
            };
            let is_newer = best
                .as_ref()
                .map(|(_, current)| hkask_media::ytdlp::candidate_is_preferred(&version, current))
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

    /// Download a video from a URL to the specified output path, bounded
    /// by [`FETCH_TIMEOUT`] — see [`Self::fetch_with_timeout`].
    pub async fn fetch(
        &self,
        url: &str,
        output_path: &std::path::Path,
    ) -> Result<YtDlpFetchOutcome, crate::MediaError> {
        self.fetch_with_timeout(url, output_path, FETCH_TIMEOUT)
            .await
    }

    /// The argv yt-dlp runs with, extracted so every bound the consumer
    /// experiences is pinned by name: `--no-playlist`, the ≤720p format
    /// pair, and the per-stream size cap.
    fn fetch_args(output_template: &str, url: &str) -> Vec<String> {
        vec![
            "--ignore-config".into(),
            "--no-exec".into(),
            "--no-update".into(),
            "--no-playlist".into(),
            "--proto".into(),
            "https".into(),
            "-f".into(),
            "bv*[height<=720]+ba/b[height<=720]/b".into(),
            "--merge-output-format".into(),
            "mp4".into(),
            format!("--max-filesize={MAX_DOWNLOAD_BYTES}"),
            "-o".into(),
            output_template.into(),
            "--".into(),
            url.into(),
        ]
    }

    /// Download a video from a URL to the specified output path, stopped
    /// at `timeout` with the partial output removed.
    ///
    /// The format selector prefers a merged best-video+best-audio pair
    /// (capped at 720p) over a single progressive file — plain `best`
    /// frequently resolves to nothing or a 144p stub on modern YouTube.
    /// `--no-playlist` prevents downloading entire playlists, and each
    /// downloaded stream is capped at [`MAX_DOWNLOAD_BYTES`] so a runaway
    /// download cannot fill the artifacts directory. The child runs with
    /// `kill_on_drop`, so a fetch stopped at the bound is killed with the
    /// dropped call. stderr is fully drained both because yt-dlp writes
    /// continuous progress to it (an undrained pipe fills and wedges the
    /// child) and so failures can surface yt-dlp's actual error text.
    pub async fn fetch_with_timeout(
        &self,
        url: &str,
        output_path: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Result<YtDlpFetchOutcome, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::YtDlpUnavailable);
        }

        let output_template = output_path.to_string_lossy().to_string();

        let mut command = Command::new(&self.ytdlp_path);
        command.env_clear();
        for key in [
            "PATH",
            "HOME",
            "TMPDIR",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "LANG",
        ] {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }
        command
            .args(Self::fetch_args(&output_template, url))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn().map_err(|error| {
            remove_partial_output(output_path);
            crate::MediaError::Io(format!("yt-dlp fetch failed: {error}"))
        })?;
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(result) => result.map_err(|error| {
                remove_partial_output(output_path);
                crate::MediaError::Io(format!("yt-dlp fetch failed: {error}"))
            })?,
            Err(_stopped) => {
                remove_partial_output(output_path);
                return Err(crate::MediaError::YtDlpFailed(format!(
                    "fetch exceeded its {timeout:?} bound and was stopped; the partial \
                     download was removed — retry, or fetch a shorter source"
                )));
            }
        };

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
            let issue = hkask_media::ytdlp::classify_stderr(tail.trim())
                .unwrap_or(hkask_media::ytdlp::YtDlpIssue::Other);
            return Err(match issue {
                hkask_media::ytdlp::YtDlpIssue::AuthorizationFailure => {
                    crate::MediaError::YtDlpAuthorization(issue.actionable_message().to_string())
                }
                hkask_media::ytdlp::YtDlpIssue::UnavailableVideo => {
                    crate::MediaError::YtDlpVideoUnavailable(issue.actionable_message().to_string())
                }
                hkask_media::ytdlp::YtDlpIssue::ExtractorFailure => {
                    crate::MediaError::YtDlpExtractor(issue.actionable_message().to_string())
                }
                hkask_media::ytdlp::YtDlpIssue::MissingJavascriptRuntime
                | hkask_media::ytdlp::YtDlpIssue::Other => {
                    crate::MediaError::YtDlpFailed(issue.actionable_message().to_string())
                }
            });
        }

        let warning =
            hkask_media::ytdlp::classify_stderr(String::from_utf8_lossy(&output.stderr).as_ref())
                .filter(|issue| *issue != hkask_media::ytdlp::YtDlpIssue::Other)
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
        let current = hkask_media::ytdlp::parse_version("2026.08.19\n").expect("version parses");
        let equal = hkask_media::ytdlp::parse_version("2026.8.19").expect("equal version parses");
        assert!(!hkask_media::ytdlp::candidate_is_preferred(
            &equal, &current
        ));
        assert!(hkask_media::ytdlp::candidate_is_preferred(
            &[2026, 8, 19, 1],
            &current
        ));
    }

    /// Every bound the consumer experiences is on the argv by name.
    #[test]
    fn fetch_args_carry_the_size_bound_and_format_pair() {
        let args = YtDlpRunner::fetch_args("/tmp/out.mp4", "https://example.test/v");
        assert!(
            args.contains(&format!("--max-filesize={MAX_DOWNLOAD_BYTES}")),
            "the per-stream size cap must be handed to yt-dlp: {args:?}"
        );
        assert!(args.contains(&"bv*[height<=720]+ba/b[height<=720]/b".to_string()));
        assert!(args.contains(&"--no-playlist".to_string()));
        assert!(args.contains(&"--ignore-config".to_string()), "{args:?}");
        assert!(args.contains(&"--no-exec".to_string()), "{args:?}");
        let sep_pos = args.iter().position(|a| a == "--").expect("-- terminator");
        let url_pos = args
            .iter()
            .position(|a| a == "https://example.test/v")
            .expect("url");
        assert_eq!(
            sep_pos + 1,
            url_pos,
            "-- must immediately precede url: {args:?}"
        );
    }

    /// A stalled yt-dlp is stopped at the bound, its partial output is
    /// removed, and the child is killed with the dropped call (no orphan
    /// survives the refusal).
    #[tokio::test]
    async fn a_stalled_fetch_is_stopped_at_the_bound_and_the_child_dies() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let pidfile = dir.path().join("fake.pid");
        let script = dir.path().join("fake-yt-dlp");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho $$ > {}\nexec sleep 60\n",
                pidfile.display()
            ),
        )
        .expect("write script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");
        let runner = YtDlpRunner::with_binary(script.to_string_lossy().into_owned());
        let output = dir.path().join("download.mp4");

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            runner.fetch_with_timeout(
                "https://example.test/v",
                &output,
                std::time::Duration::from_millis(300),
            ),
        )
        .await
        .expect("the bound must stop the fetch inside the outer guard");
        let Err(error) = result else {
            panic!("a stalled fetch must refuse, not succeed");
        };
        let message = error.to_string();
        assert!(
            message.contains("exceeded"),
            "the refusal names the bound: {message}"
        );
        assert!(!output.exists(), "the partial download was removed");
        let pid: i32 = std::fs::read_to_string(&pidfile)
            .expect("pid written")
            .trim()
            .parse()
            .expect("pid number");
        // SIGKILL is sent when the call drops, but the entry can linger as
        // an unreaped zombie until the runtime's orphan queue runs — a
        // zombie is dead; only a live process is a leak.
        let mut dead = false;
        for _ in 0..40 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => {
                    dead = true;
                    break;
                }
                Ok(stat) => {
                    if let Some((_, rest)) = stat.rsplit_once(')')
                        && rest.trim_start().starts_with('Z')
                    {
                        dead = true;
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(dead, "the stalled child must be killed with the call");
    }
}
