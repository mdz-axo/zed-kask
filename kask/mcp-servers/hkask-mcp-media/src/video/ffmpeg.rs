//! ffmpeg subprocess wrappers for video processing.
//!
//! Detects ffmpeg at startup with graceful degradation.
//! All operations use temp directories for output files.

use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

struct RollbackOwnedOutput {
    path: PathBuf,
    armed: bool,
}

/// Cleanup-owning keyframe scratch batch. The operation directory and every
/// file FFmpeg creates inside it are removed together on success or failure.
#[derive(Debug)]
pub(crate) struct ExtractedFrames {
    directory: PathBuf,
    paths: Vec<PathBuf>,
}

impl ExtractedFrames {
    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.paths.len()
    }
}

impl<'a> IntoIterator for &'a ExtractedFrames {
    type Item = &'a PathBuf;
    type IntoIter = std::slice::Iter<'a, PathBuf>;

    fn into_iter(self) -> Self::IntoIter {
        self.paths.iter()
    }
}

impl Drop for ExtractedFrames {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                target: "hkask.mcp.media.ffmpeg",
                path = %self.directory.display(),
                %error,
                "Failed to remove keyframe scratch directory"
            ),
        }
    }
}

impl RollbackOwnedOutput {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn commit(mut self) -> PathBuf {
        self.armed = false;
        self.path.clone()
    }
}

impl Drop for RollbackOwnedOutput {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                target: "hkask.mcp.media.ffmpeg",
                path = %self.path.display(),
                %error,
                "Failed to remove rollback-owned FFmpeg output"
            ),
        }
    }
}

/// Video metadata from `ffprobe`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VideoProbeInfo {
    pub duration_secs: f32,
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub fps: f32,
    pub bit_rate: u64,
}

/// ffmpeg runner with availability detection.
#[derive(Debug, Clone)]
pub struct FfmpegRunner {
    pub available: bool,
    ffmpeg_path: String,
    temp_dir: PathBuf,
}

impl Drop for FfmpegRunner {
    fn drop(&mut self) {
        // Clean up accumulated temp files on server shutdown.
        match std::fs::remove_dir_all(&self.temp_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                target: "hkask.mcp.media.ffmpeg",
                path = %self.temp_dir.display(),
                %error,
                "Failed to remove FFmpeg runner temp directory"
            ),
        }
    }
}

impl FfmpegRunner {
    /// Detect ffmpeg on PATH. Returns a runner with `available` set accordingly.
    /// Cleans up leftover temp files from previous crashed sessions.
    pub fn detect() -> Self {
        let ffmpeg_path = "ffmpeg".to_string();
        let available = {
            #[allow(clippy::disallowed_methods)]
            std::process::Command::new(&ffmpeg_path)
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };

        // Per-runner subdirectory — concurrent runners (parallel tests,
        // multiple live servers) must not share output paths: a shared dir
        // made this cleanup and the Drop removal racy against live renders
        // in other runners (a sibling's detect()/Drop could delete the
        // output directory mid-render, failing ffmpeg with a missing
        // parent directory).
        let root = std::env::temp_dir().join("hkask-media");
        let temp_dir = root.join(uuid::Uuid::new_v4().to_string());

        // Clean up leftovers from crashed sessions only — sibling
        // directories not modified in the last day. Never wipe the root:
        // live runners keep their subdirectories there.
        if let Ok(entries) = std::fs::read_dir(&root) {
            let stale_before = std::time::SystemTime::now()
                .checked_sub(std::time::Duration::from_secs(86_400))
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            for entry in entries.flatten() {
                if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                    if modified < stale_before {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
        }

        if available {
            tracing::info!(target: "hkask.mcp.media.ffmpeg", "ffmpeg detected");
        } else {
            tracing::warn!(target: "hkask.mcp.media.ffmpeg", "ffmpeg not found — video tools will be unavailable");
        }

        Self {
            available,
            ffmpeg_path,
            temp_dir,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_binary(ffmpeg_path: String, temp_dir: PathBuf) -> Self {
        Self {
            available: true,
            ffmpeg_path,
            temp_dir,
        }
    }

    /// Ensure the temp directory exists.
    fn ensure_temp_dir(&self) -> Result<(), crate::MediaError> {
        std::fs::create_dir_all(&self.temp_dir)
            .map_err(|e| crate::MediaError::Io(format!("Failed to create temp dir: {}", e)))
    }

    /// Generate a unique output path in the temp directory.
    fn output_path(&self, extension: &str) -> PathBuf {
        let name = uuid::Uuid::new_v4().to_string();
        self.temp_dir.join(format!("{}.{}", name, extension))
    }

    /// Run a configured ffmpeg command to completion, surfacing ffmpeg's
    /// own stderr in the failure. Never `status()` with piped stdio:
    /// tokio's `status()` drops the piped read ends ("ensure we close
    /// any stdio handles"), so the child's first stderr write — ffmpeg's
    /// startup banner — hits a closed pipe and the process dies of
    /// SIGPIPE before doing any work. Observed live as "exit code: None"
    /// with an empty output directory on every video render. `output()`
    /// drains the pipes instead, and its captured stderr becomes the
    /// failure reason.
    async fn run_to_completion(
        mut command: Command,
        what: &'static str,
    ) -> Result<(), crate::MediaError> {
        let output = command.output().await.map_err(|e| {
            crate::MediaError::FfmpegFailed(format!("ffmpeg {what} spawn failed: {e}"))
        })?;
        if !output.status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg {what} failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    /// Probe a video file for metadata (duration, width, height, codec, fps).
    /// Uses `ffprobe` (bundled with ffmpeg) with JSON output.
    pub async fn probe(&self, input: &str) -> Result<VideoProbeInfo, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        // ffprobe is typically at the same path as ffmpeg with "probe" suffix,
        // or in the same directory. Try "ffprobe" on PATH.
        let output = Command::new("ffprobe")
            .arg("-v")
            .arg("quiet")
            .arg("-print_format")
            .arg("json")
            .arg("-show_format")
            .arg("-show_streams")
            .arg(input)
            .output()
            .await
            .map_err(|e| crate::MediaError::Io(format!("ffprobe failed: {e}")))?;
        if !output.status.success() {
            return Err(crate::MediaError::Io(format!(
                "ffprobe exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| crate::MediaError::Io(format!("ffprobe JSON parse: {e}")))?;
        // Extract the first video stream.
        let streams = json.get("streams").and_then(|s| s.as_array());
        let video_stream = streams.and_then(|s| {
            s.iter()
                .find(|st| st.get("codec_type").and_then(|t| t.as_str()) == Some("video"))
        });
        let format = json.get("format");
        let duration = format
            .and_then(|f| f.get("duration"))
            .and_then(|d| d.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let width = video_stream
            .and_then(|s| s.get("width"))
            .and_then(|w| w.as_u64())
            .unwrap_or(0) as u32;
        let height = video_stream
            .and_then(|s| s.get("height"))
            .and_then(|h| h.as_u64())
            .unwrap_or(0) as u32;
        let codec = video_stream
            .and_then(|s| s.get("codec_name"))
            .and_then(|c| c.as_str())
            .unwrap_or("unknown")
            .to_string();
        let fps = video_stream
            .and_then(|s| s.get("r_frame_rate"))
            .and_then(|r| r.as_str())
            .and_then(|s| {
                let parts: Vec<&str> = s.split('/').collect();
                if parts.len() == 2 {
                    let num: f64 = parts[0].parse().ok()?;
                    let den: f64 = parts[1].parse().ok()?;
                    if den != 0.0 { Some(num / den) } else { None }
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let bit_rate = format
            .and_then(|f| f.get("bit_rate"))
            .and_then(|b| b.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        Ok(VideoProbeInfo {
            duration_secs: duration as f32,
            width,
            height,
            codec,
            fps: fps as f32,
            bit_rate,
        })
    }

    /// Trim a video to specified start/end times.
    /// Uses stream copy (-c copy) for fast, lossless trimming.
    pub async fn clip(
        &self,
        input: &str,
        start_sec: f32,
        end_sec: f32,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path("mp4"));
        let duration = end_sec - start_sec;

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-ss")
            .arg(format!("{:.3}", start_sec))
            .arg("-to")
            .arg(format!("{:.3}", end_sec))
            .arg("-i")
            .arg(input)
            .arg("-c")
            .arg("copy")
            .arg("-avoid_negative_ts")
            .arg("make_zero")
            .arg(output.path());
        Self::run_to_completion(command, "clip").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, duration = %duration, output = %output.display(), "Video clipped");
        Ok(output)
    }

    /// Convert a video segment to GIF.
    /// Uses the two-pass palettegen + paletteuse pipeline for quality.
    pub async fn to_gif(
        &self,
        input: &str,
        start_sec: f32,
        duration_sec: f32,
        width: u32,
        fps: u32,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path("gif"));

        // Build filter complex for palette generation + GIF conversion
        let filter = format!(
            "fps={},scale={}:-1:flags=lanczos,split[v1][v2];[v1]palettegen[p];[v2][p]paletteuse",
            fps, width
        );

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-ss")
            .arg(format!("{:.3}", start_sec))
            .arg("-t")
            .arg(format!("{:.3}", duration_sec))
            .arg("-i")
            .arg(input)
            .arg("-filter_complex")
            .arg(&filter)
            .arg(output.path());
        Self::run_to_completion(command, "GIF conversion").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, duration = %duration_sec, width = %width, fps = %fps, output = %output.display(), "GIF created");
        Ok(output)
    }

    /// Add text caption overlay to a video.
    /// Uses the drawtext filter with configurable position and font size.
    pub async fn add_caption(
        &self,
        input: &str,
        text: &str,
        position: &str,
        font_size: u32,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path("mp4"));

        // Map position to drawtext y-coordinate
        let y_pos = match position {
            "top" => "(h-text_h-10)",
            "center" => "(h-text_h)/2",
            _ => "10", // bottom
        };

        // Escape special characters in text for ffmpeg filter
        // % must be escaped first to avoid double-escaping the %% we produce
        let escaped_text = text
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace(':', "\\:")
            .replace('\'', "\\'");

        let drawtext = format!(
            "drawtext=text='{}':fontsize={}:fontcolor=white:box=1:boxcolor=black@0.5:boxborderw=5:x=(w-text_w)/2:y={}",
            escaped_text, font_size, y_pos
        );

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-i")
            .arg(input)
            .arg("-vf")
            .arg(&drawtext)
            .arg("-c:a")
            .arg("copy")
            .arg(output.path());
        Self::run_to_completion(command, "caption").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, text = %text, output = %output.display(), "Caption added");
        Ok(output)
    }

    /// Capture audio from the default system input device.
    /// Uses ffmpeg to record from the platform-specific default audio source.
    /// Saves to a WAV file in the temp directory (or specified path).
    pub async fn capture_audio(&self, duration_secs: f32) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path("wav"));

        // Detect platform-specific audio input device
        let (input_format, input_device) = if cfg!(target_os = "linux") {
            ("alsa", "default")
        } else if cfg!(target_os = "macos") {
            ("avfoundation", ":0")
        } else if cfg!(target_os = "windows") {
            ("dshow", "audio=Microphone")
        } else {
            return Err(crate::MediaError::FfmpegFailed(
                "Unsupported platform for audio capture".to_string(),
            ));
        };

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-f")
            .arg(input_format)
            .arg("-i")
            .arg(input_device)
            .arg("-t")
            .arg(format!("{:.1}", duration_secs))
            .arg("-ac")
            .arg("1") // mono
            .arg("-ar")
            .arg("16000") // 16kHz sample rate (good for Whisper)
            .arg(output.path());
        Self::run_to_completion(command, "audio capture").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", duration = %duration_secs, output = %output.display(), "Audio captured");
        Ok(output)
    }

    /// Create a video from a sequence of images.
    /// Images are concatenated at the specified frame rate.
    pub(crate) async fn images_to_video(
        &self,
        image_paths: &[PathBuf],
        fps: u32,
        format: crate::assets::LocalVideoFormat,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if image_paths.is_empty() {
            return Err(crate::MediaError::FfmpegFailed(
                "No images provided".to_string(),
            ));
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path(format.extension()));

        // Write image list to a temp file for concat demuxer
        let list_path = RollbackOwnedOutput::new(self.output_path("txt"));
        let list_content: String = image_paths
            .iter()
            .map(|p| format!("file '{}'", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(list_path.path(), list_content)
            .map_err(|e| crate::MediaError::Io(format!("Failed to write image list: {}", e)))?;

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-r")
            .arg(fps.to_string())
            .arg("-i")
            .arg(list_path.path());
        match format {
            crate::assets::LocalVideoFormat::Mp4 => {
                command
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-pix_fmt")
                    .arg("yuv420p");
            }
            crate::assets::LocalVideoFormat::Gif => {
                command.arg("-c:v").arg("gif");
            }
        }
        command.arg(output.path());
        Self::run_to_completion(command, "images_to_video").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", image_count = image_paths.len(), fps = %fps, output = %output.display(), "Video created from images");
        Ok(output)
    }

    /// Concatenate multiple video clips into one.
    /// Uses the concat demuxer for fast, lossless joining.
    pub async fn concat(&self, video_paths: &[String]) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if video_paths.len() < 2 {
            return Err(crate::MediaError::FfmpegFailed(
                "At least 2 videos required for concat".to_string(),
            ));
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path("mp4"));

        // Write concat list
        let list_path = RollbackOwnedOutput::new(self.output_path("txt"));
        let list_content: String = video_paths
            .iter()
            .map(|p| format!("file '{}'", p.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(list_path.path(), list_content)
            .map_err(|e| crate::MediaError::Io(format!("Failed to write concat list: {}", e)))?;

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-i")
            .arg(list_path.path())
            .arg("-c")
            .arg("copy")
            .arg(output.path());
        Self::run_to_completion(command, "concat").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", clip_count = video_paths.len(), output = %output.display(), "Videos concatenated");
        Ok(output)
    }

    /// Extract keyframes from a video at regular intervals.
    /// Returns paths to extracted frame images for vision LLM analysis.
    pub(crate) async fn extract_keyframes(
        &self,
        input: &str,
        interval_sec: f32,
        max_frames: u32,
    ) -> Result<ExtractedFrames, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let directory = self.temp_dir.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&directory).map_err(|error| {
            crate::MediaError::Io(format!(
                "Failed to create keyframe scratch directory {}: {error}",
                directory.display()
            ))
        })?;
        let mut frames = ExtractedFrames {
            directory,
            paths: Vec::new(),
        };
        let pattern = frames.directory.join("frame_%03d.jpg");

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-i")
            .arg(input)
            .arg("-vf")
            .arg(format!("fps=1/{interval_sec},scale=640:-1"))
            .arg("-vframes")
            .arg(max_frames.to_string())
            .arg(&pattern);
        Self::run_to_completion(command, "keyframe extraction").await?;

        for entry in std::fs::read_dir(&frames.directory).map_err(|error| {
            crate::MediaError::Io(format!(
                "Failed to read keyframe scratch directory {}: {error}",
                frames.directory.display()
            ))
        })? {
            let path = entry
                .map_err(|error| {
                    crate::MediaError::Io(format!("Failed to read extracted frame entry: {error}"))
                })?
                .path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("jpg") {
                frames.paths.push(path);
            }
        }
        frames.paths.sort();

        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, frame_count = frames.len(), "Keyframes extracted");
        Ok(frames)
    }

    /// Trim an audio file to specified start/end times.
    /// Uses stream copy (-c copy) for fast, lossless trimming.
    pub async fn audio_trim(
        &self,
        input: &str,
        start_sec: f32,
        end_sec: f32,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;
        let output = RollbackOwnedOutput::new(self.output_path("wav"));
        let duration = end_sec - start_sec;
        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-ss")
            .arg(format!("{:.3}", start_sec))
            .arg("-t")
            .arg(format!("{:.3}", duration))
            .arg("-i")
            .arg(input)
            .arg("-c")
            .arg("copy")
            .arg(output.path());
        Self::run_to_completion(command, "audio trim").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, duration = %duration, output = %output.display(), "Audio trimmed");
        Ok(output)
    }

    /// Concatenate multiple audio files into one.
    /// Uses the concat demuxer for fast, lossless joining.
    pub async fn audio_concat(&self, audio_paths: &[String]) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if audio_paths.is_empty() {
            return Err(crate::MediaError::Io(
                "audio_concat requires at least one file".to_string(),
            ));
        }
        self.ensure_temp_dir()?;
        let output = RollbackOwnedOutput::new(self.output_path("wav"));
        if audio_paths.len() == 1 {
            std::fs::copy(&audio_paths[0], output.path())
                .map_err(|e| crate::MediaError::Io(format!("copy single audio: {e}")))?;
            return Ok(output.commit());
        }

        let list_path = RollbackOwnedOutput::new(self.output_path("txt"));
        let list_content: String = audio_paths
            .iter()
            .map(|p| {
                format!(
                    "file '{}'
",
                    p
                )
            })
            .collect();
        std::fs::write(list_path.path(), &list_content)
            .map_err(|e| crate::MediaError::Io(format!("write concat list: {e}")))?;
        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-i")
            .arg(list_path.path())
            .arg("-c")
            .arg("copy")
            .arg(output.path());
        Self::run_to_completion(command, "audio concat").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", clip_count = audio_paths.len(), output = %output.display(), "Audio concatenated");
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The render path's ffmpeg primitives must run real ffmpeg to
    /// completion. Pins the SIGPIPE regression: tokio's `status()`
    /// dropped the piped stderr read end, so the child died on its
    /// banner write before producing any output ("exit code: None",
    /// empty temp dir) — every live video render failed while the
    /// suite stayed green because no test ran real ffmpeg.
    #[tokio::test]
    async fn clip_runs_real_ffmpeg_to_completion() {
        let runner = FfmpegRunner::detect();
        if !runner.available {
            eprintln!("skipping: ffmpeg not installed");
            return;
        }
        runner.ensure_temp_dir().expect("temp dir");
        let source = runner.output_path("mp4");
        let mut generate = Command::new(&runner.ffmpeg_path);
        generate
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("testsrc=duration=1:size=128x96:rate=10")
            .arg("-c:v")
            .arg("mpeg4")
            .arg("-y")
            .arg(&source);
        let generated = generate.output().await.expect("generate test source");
        assert!(
            generated.status.success(),
            "test source generation failed: {}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let clipped = runner
            .clip(source.to_str().expect("utf-8 temp path"), 0.2, 0.8)
            .await
            .expect("clip must run ffmpeg to completion");
        let size = clipped.metadata().map(|m| m.len()).unwrap_or(0);
        assert!(size > 0, "clipped output is empty");
    }

    /// The reel render concatenates stream-copied clips — the second
    /// primitive of the render path, same regression class as clip.
    #[tokio::test]
    async fn concat_runs_real_ffmpeg_to_completion() {
        let runner = FfmpegRunner::detect();
        if !runner.available {
            eprintln!("skipping: ffmpeg not installed");
            return;
        }
        runner.ensure_temp_dir().expect("temp dir");
        let sources: Vec<PathBuf> = (0..2).map(|_| runner.output_path("mp4")).collect();
        for target in &sources {
            let mut generate = Command::new(&runner.ffmpeg_path);
            generate
                .arg("-f")
                .arg("lavfi")
                .arg("-i")
                .arg("testsrc=duration=1:size=128x96:rate=10")
                .arg("-c:v")
                .arg("mpeg4")
                .arg("-y")
                .arg(target);
            let generated = generate.output().await.expect("generate test source");
            assert!(
                generated.status.success(),
                "test source generation failed: {}",
                String::from_utf8_lossy(&generated.stderr)
            );
        }
        let paths: Vec<String> = sources
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        let concatenated = runner
            .concat(&paths)
            .await
            .expect("concat must run ffmpeg to completion");
        let size = concatenated.metadata().map(|m| m.len()).unwrap_or(0);
        assert!(size > 0, "concatenated output is empty");
    }

    /// expect: Keyframe filenames produced from FFmpeg's `%03d` pattern are discovered, and the
    /// entire scratch batch is removed when its owner is dropped.
    #[cfg(unix)]
    #[tokio::test]
    async fn extracted_keyframes_match_pattern_and_are_cleanup_owned()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir()?;
        let fake_ffmpeg = root.path().join("extract-one.sh");
        std::fs::write(
            &fake_ffmpeg,
            "#!/bin/sh\nfor arg do pattern=$arg; done\noutput=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nprintf frame > \"$output\"\n",
        )?;
        let mut permissions = std::fs::metadata(&fake_ffmpeg)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_ffmpeg, permissions)?;
        let output_dir = root.path().join("outputs");
        let runner = FfmpegRunner::with_binary(
            fake_ffmpeg.to_string_lossy().into_owned(),
            output_dir.clone(),
        );

        let frames = runner.extract_keyframes("input.mp4", 2.0, 1).await?;
        assert_eq!(frames.len(), 1);
        assert_eq!(
            (&frames)
                .into_iter()
                .next()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str()),
            Some("frame_001.jpg")
        );
        drop(frames);
        assert_eq!(std::fs::read_dir(&output_dir)?.count(), 0);
        Ok(())
    }

    /// expect: A failed keyframe subprocess removes every scratch frame while preserving the
    /// original FFmpeg cause.
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_keyframe_extraction_removes_partial_scratch_batch()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir()?;
        let fake_ffmpeg = root.path().join("extract-then-fail.sh");
        std::fs::write(
            &fake_ffmpeg,
            "#!/bin/sh\nfor arg do pattern=$arg; done\noutput=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nprintf partial > \"$output\"\nprintf 'keyframe sentinel failure' >&2\nexit 23\n",
        )?;
        let mut permissions = std::fs::metadata(&fake_ffmpeg)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_ffmpeg, permissions)?;
        let output_dir = root.path().join("outputs");
        let runner = FfmpegRunner::with_binary(
            fake_ffmpeg.to_string_lossy().into_owned(),
            output_dir.clone(),
        );

        let error = runner
            .extract_keyframes("input.mp4", 2.0, 1)
            .await
            .expect_err("injected keyframe failure must surface");
        assert!(error.to_string().contains("keyframe sentinel failure"));
        assert_eq!(std::fs::read_dir(&output_dir)?.count(), 0);
        Ok(())
    }

    /// expect: A failed FFmpeg process cannot leave a partial output or concat-list file behind.
    /// pre: the subprocess writes its output path, then exits non-zero.
    /// post: every local-video primitive returns its causal failure and its owned temp directory is empty.
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_ffmpeg_rolls_back_partial_outputs_and_lists()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir()?;
        let fake_ffmpeg = root.path().join("partial-then-fail.sh");
        std::fs::write(
            &fake_ffmpeg,
            "#!/bin/sh\nfor arg do output=$arg; done\nprintf partial > \"$output\"\nprintf 'injected partial-output failure' >&2\nexit 23\n",
        )?;
        let mut permissions = std::fs::metadata(&fake_ffmpeg)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_ffmpeg, permissions)?;
        let output_dir = root.path().join("outputs");
        let runner = FfmpegRunner::with_binary(
            fake_ffmpeg.to_string_lossy().into_owned(),
            output_dir.clone(),
        );

        let assert_rolled_back =
            |error: crate::MediaError| -> Result<(), Box<dyn std::error::Error>> {
                assert!(
                    error
                        .to_string()
                        .contains("injected partial-output failure"),
                    "FFmpeg stderr cause was lost: {error}"
                );
                assert_eq!(
                    std::fs::read_dir(&output_dir)?.count(),
                    0,
                    "failed FFmpeg operation left rollback-owned files"
                );
                Ok(())
            };

        assert_rolled_back(
            runner
                .clip("input.mp4", 0.0, 1.0)
                .await
                .expect_err("clip must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .to_gif("input.mp4", 0.0, 1.0, 320, 10)
                .await
                .expect_err("GIF conversion must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .add_caption("input.mp4", "caption", "bottom", 24)
                .await
                .expect_err("caption must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .images_to_video(
                    &[std::path::PathBuf::from("frame.png")],
                    24,
                    crate::assets::LocalVideoFormat::Mp4,
                )
                .await
                .expect_err("image sequence must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .concat(&["first.mp4".to_string(), "second.mp4".to_string()])
                .await
                .expect_err("concat must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .capture_audio(1.0)
                .await
                .expect_err("audio capture must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .audio_trim("input.wav", 0.0, 1.0)
                .await
                .expect_err("audio trim must surface injected failure"),
        )?;
        assert_rolled_back(
            runner
                .audio_concat(&["first.wav".to_string(), "second.wav".to_string()])
                .await
                .expect_err("audio concat must surface injected failure"),
        )?;
        Ok(())
    }
}
