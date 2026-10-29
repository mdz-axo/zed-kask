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
    /// Original selected frame PTS, in seconds on the source stream timeline.
    pub(crate) timestamps_sec: Vec<f64>,
}

/// A scene-detection run's result: the exact select filter applied (the
/// provenance record) and the selected frames' source-PTS seconds in frame
/// order — frame 0 first, then every detected cut boundary.
#[derive(Debug)]
pub(crate) struct SceneDetection {
    pub(crate) filter: String,
    pub(crate) boundaries_sec: Vec<f64>,
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
    /// Whether the input carries a video stream — the stream-requirement
    /// signal the video passes degrade on (an audio-only file probes fine
    /// but has none; width/height/fps stay zero).
    pub has_video_stream: bool,
    /// Whether the input carries an audio stream — the stream-requirement
    /// signal the audio pass degrades on (a video-only file probes fine
    /// but has none).
    pub has_audio_stream: bool,
}

/// Parse only selected-frame showinfo records; no ordinal-based fallback.
fn selected_frame_timestamps(stderr: &str) -> Result<Vec<f64>, crate::MediaError> {
    let mut timestamps = Vec::new();
    for line in stderr
        .lines()
        .filter(|line| line.contains("Parsed_showinfo_") && line.contains("pts_time:"))
    {
        let value = line
            .split_once("pts_time:")
            .and_then(|(_, tail)| tail.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                crate::MediaError::FfmpegFailed(format!("Invalid selected frame PTS: {line}"))
            })?;
        if timestamps.last().is_some_and(|previous| *previous >= value) {
            return Err(crate::MediaError::FfmpegFailed(
                "Non-increasing selected frame PTS".into(),
            ));
        }
        timestamps.push(value);
    }
    Ok(timestamps)
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
    /// The runner's per-instance intermediate directory — its unique
    /// subdirectory under the shared `hkask-media` temp root. The hermetic
    /// observation scope for tests asserting which files an operation left
    /// behind: the shared root is written by every concurrent runner on
    /// the machine (parallel tests, live servers), so observing the root
    /// turns a cleanup assertion into a race against unrelated processes.
    pub fn temp_dir(&self) -> &std::path::Path {
        &self.temp_dir
    }

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
                    if modified < stale_before
                        && let Err(error) = std::fs::remove_dir_all(entry.path())
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        tracing::warn!(
                            target: "hkask.mcp.media.ffmpeg",
                            path = %entry.path().display(),
                            %error,
                            "Failed to remove stale FFmpeg temp directory"
                        );
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

    /// Write the concat list and run FFmpeg's concat demuxer with stream
    /// copy into `output`. List formatting stays at the call site — video
    /// entries escape embedded quotes, audio entries do not.
    async fn concat_via_list(
        &self,
        list_path: &RollbackOwnedOutput,
        list_content: &str,
        output: &RollbackOwnedOutput,
        label: &'static str,
    ) -> Result<(), crate::MediaError> {
        std::fs::write(list_path.path(), list_content)
            .map_err(|e| crate::MediaError::Io(format!("Failed to write concat list: {e}")))?;
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
        Self::run_to_completion(command, label).await
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
            .arg("error")
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
        let audio_stream = streams.and_then(|s| {
            s.iter()
                .find(|st| st.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))
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
            has_video_stream: video_stream.is_some(),
            has_audio_stream: audio_stream.is_some(),
        })
    }

    /// Audio-only container inputs for [`FfmpegRunner::clip`]: these trim by
    /// decoding to 16-bit PCM wav (stream copy cannot land compressed audio in
    /// a wav container — ffmpeg has no wav tag for mp3 frames); every other
    /// input trims by lossless stream copy into mp4.
    fn clip_input_is_audio(input: &str) -> bool {
        matches!(
            std::path::Path::new(input)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "opus" | "wma")
        )
    }

    /// Trim media to specified start/end times. Video inputs stream-copy
    /// losslessly into mp4 (`-c copy`); audio inputs decode to 16-bit PCM wav —
    /// the publication path then infers the right asset kind from the container
    /// (2026-10-27: the Media tab's trim serves both media kinds through this
    /// one path).
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

        let audio_input = Self::clip_input_is_audio(input);
        let output =
            RollbackOwnedOutput::new(self.output_path(if audio_input { "wav" } else { "mp4" }));
        let duration = end_sec - start_sec;

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-ss")
            .arg(format!("{:.3}", start_sec))
            .arg("-to")
            .arg(format!("{:.3}", end_sec))
            .arg("-i")
            .arg(input);
        if audio_input {
            command.arg("-c:a").arg("pcm_s16le");
        } else {
            command
                .arg("-c")
                .arg("copy")
                .arg("-avoid_negative_ts")
                .arg("make_zero");
        }
        command.arg(output.path());
        Self::run_to_completion(command, "clip").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, duration = %duration, audio_input, output = %output.display(), "Media clipped");
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

    /// Decode an authorized span to 16 kHz mono signed-16 PCM. Resampling and
    /// downmixing happen here, before any speaker-embedding feature extraction.
    pub(crate) async fn decode_speech_pcm_16khz_mono(
        &self,
        input: &std::path::Path,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Vec<i16>, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if end_ms <= start_ms {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "PCM span end {end_ms} must be after start {start_ms}"
            )));
        }
        self.ensure_temp_dir()?;
        let output = RollbackOwnedOutput::new(self.output_path("s16le"));
        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-y")
            .arg("-ss")
            .arg(format!("{:.3}", start_ms as f64 / 1_000.0))
            .arg("-to")
            .arg(format!("{:.3}", end_ms as f64 / 1_000.0))
            .arg("-i")
            .arg(input)
            .arg("-vn")
            .arg("-ac")
            .arg("1")
            .arg("-ar")
            .arg("16000")
            .arg("-f")
            .arg("s16le")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg(output.path());
        Self::run_to_completion(command, "speech-pcm-decode").await?;
        let bytes = std::fs::read(output.path())
            .map_err(|error| crate::MediaError::Io(format!("read decoded PCM span: {error}")))?;
        if bytes.len() % 2 != 0 {
            return Err(crate::MediaError::FfmpegFailed(
                "decoded signed-16 PCM has an odd byte count".to_string(),
            ));
        }
        Ok(bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect())
    }

    /// Extract the speech track for STT upload (16 kHz mono, 32 kbps mp3
    /// — the profile the provider consumes internally): the long-input
    /// precondition for transcription, after the observed provider body
    /// cap (OpenRouter 413 at >50 MB, 2026-10-25: a 274 MB video container
    /// was uploaded whole). The extraction preserves the source timeline —
    /// word timings map to the original media unchanged.
    pub async fn extract_speech_audio(
        &self,
        input: &std::path::Path,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let output = RollbackOwnedOutput::new(self.output_path("mp3"));

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-y")
            .arg("-i")
            .arg(input)
            .arg("-vn")
            .arg("-ac")
            .arg("1")
            .arg("-ar")
            .arg("16000")
            .arg("-b:a")
            .arg("32k")
            .arg(output.path());
        Self::run_to_completion(command, "speech-audio-extract").await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input.display(), output = %output.display(), "Speech audio extracted for STT upload");
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
        self.concat_via_list(&list_path, &list_content, &output, "concat")
            .await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", clip_count = video_paths.len(), output = %output.display(), "Videos concatenated");
        Ok(output)
    }

    /// Sample original decoded frames at least `interval_sec` apart.
    /// `showinfo` runs after selection and before encoding: its PTS belongs to
    /// the source frame, not an fps filter's synthetic output timeline.
    pub(crate) async fn extract_keyframes(
        &self,
        input: &str,
        interval_sec: f32,
        max_frames: u32,
    ) -> Result<ExtractedFrames, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if !interval_sec.is_finite() || interval_sec <= 0.0 || max_frames == 0 {
            return Err(crate::MediaError::FfmpegFailed(
                "Invalid frame sampling controls".into(),
            ));
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
            timestamps_sec: Vec::new(),
        };
        let pattern = frames.directory.join("frame_%03d.jpg");

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-copyts")
            .arg("-i")
            .arg(input)
            .arg("-map").arg("0:v:0")
            .arg("-vf")
            .arg(format!("select='isnan(prev_selected_t)+gte(t-prev_selected_t,{interval_sec})',showinfo,scale=640:-1"))
            .arg("-fps_mode").arg("vfr")
            .arg("-frames:v")
            .arg(max_frames.to_string())
            .arg(&pattern);
        let output = command.output().await.map_err(|error| {
            crate::MediaError::FfmpegFailed(format!("frame extraction spawn: {error}"))
        })?;
        if !output.status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "frame extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let timestamps = selected_frame_timestamps(&String::from_utf8_lossy(&output.stderr))?;

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
        if frames.paths.len() > max_frames as usize {
            return Err(crate::MediaError::FfmpegFailed(
                "Frame output exceeded requested maximum".into(),
            ));
        }
        // FFmpeg can filter one extra frame before the encoder's frame limit
        // stops it. Match only encoded files, and refuse missing observations.
        if timestamps.len() < frames.paths.len() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "Missing source PTS: {} images, {} observations",
                frames.paths.len(),
                timestamps.len()
            )));
        }
        frames.timestamps_sec = timestamps.into_iter().take(frames.paths.len()).collect();

        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, frame_count = frames.len(), "Keyframes extracted");
        Ok(frames)
    }

    /// A scene-detection run: the exact select filter used (recorded in the
    /// stored layer's provenance) and the strictly increasing source-PTS
    /// seconds of the frames the filter selected. The
    /// `isnan(prev_selected_t)` term selects frame 0 unconditionally, so the
    /// first boundary is the video's start; every further boundary is a cut
    /// (scene score above the threshold, subject to the minimum shot
    /// length). `showinfo` runs after selection with `-copyts`, so the PTS
    /// belongs to the source timeline — the same parsing contract as
    /// `extract_keyframes`.
    pub(crate) async fn detect_scene_changes(
        &self,
        input: &str,
        threshold: f64,
        min_shot_sec: f64,
    ) -> Result<SceneDetection, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if !threshold.is_finite() || threshold <= 0.0 || threshold > 1.0 {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "Invalid scene threshold {threshold} — must be within (0.0, 1.0]"
            )));
        }
        if !min_shot_sec.is_finite() || min_shot_sec < 0.0 {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "Invalid minimum shot length {min_shot_sec} — must be >= 0.0"
            )));
        }
        let filter = format!(
            "select='isnan(prev_selected_t)+gte(t-prev_selected_t,{min_shot_sec:.3})*gt(scene,{threshold:.3})',showinfo"
        );
        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-copyts")
            .arg("-i")
            .arg(input)
            .arg("-map")
            .arg("0:v:0")
            .arg("-vf")
            .arg(&filter)
            .arg("-f")
            .arg("null")
            .arg("-");
        let output = command.output().await.map_err(|error| {
            crate::MediaError::FfmpegFailed(format!("scene detection spawn: {error}"))
        })?;
        if !output.status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "scene detection failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let boundaries_sec = selected_frame_timestamps(&String::from_utf8_lossy(&output.stderr))?;
        tracing::info!(
            target: "hkask.mcp.media.ffmpeg",
            input = %input,
            boundaries = boundaries_sec.len(),
            "Scene changes detected"
        );
        Ok(SceneDetection {
            filter,
            boundaries_sec,
        })
    }

    /// Sample decoded frames from `start_sec` on the source timeline, at
    /// least `interval_sec` apart, at most `max_frames` — the
    /// `extract_keyframes` contract (observed showinfo source PTS,
    /// `-copyts`) with an input seek bounding the decode. Frames before
    /// `start_sec` (including the seek's landing keyframe) are not selected:
    /// the select gate adds `gte(t, start)`, so the first selected frame is
    /// the first decoded frame at or after the range start — the chunk
    /// sampling the vision passes consume.
    pub(crate) async fn extract_keyframes_in_range(
        &self,
        input: &str,
        start_sec: f64,
        interval_sec: f32,
        max_frames: u32,
    ) -> Result<ExtractedFrames, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        if !start_sec.is_finite() || start_sec < 0.0 {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "Invalid range start {start_sec} — must be >= 0.0"
            )));
        }
        if !interval_sec.is_finite() || interval_sec <= 0.0 || max_frames == 0 {
            return Err(crate::MediaError::FfmpegFailed(
                "Invalid frame sampling controls".into(),
            ));
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
            timestamps_sec: Vec::new(),
        };
        let pattern = frames.directory.join("frame_%03d.jpg");

        let mut command = Command::new(&self.ffmpeg_path);
        command
            .arg("-ss")
            .arg(format!("{start_sec:.3}"))
            .arg("-copyts")
            .arg("-i")
            .arg(input)
            .arg("-map")
            .arg("0:v:0")
            .arg("-vf")
            .arg(format!(
                "select='isnan(prev_selected_t)*gte(t,{start_sec:.3})+gte(t-prev_selected_t,{interval_sec})',showinfo,scale=640:-1"
            ))
            .arg("-fps_mode")
            .arg("vfr")
            .arg("-frames:v")
            .arg(max_frames.to_string())
            .arg(&pattern);
        let output = command.output().await.map_err(|error| {
            crate::MediaError::FfmpegFailed(format!("frame extraction spawn: {error}"))
        })?;
        if !output.status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "frame extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let timestamps = selected_frame_timestamps(&String::from_utf8_lossy(&output.stderr))?;

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
        if frames.paths.len() > max_frames as usize {
            return Err(crate::MediaError::FfmpegFailed(
                "Frame output exceeded requested maximum".into(),
            ));
        }
        if timestamps.len() < frames.paths.len() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "Missing source PTS: {} images, {} observations",
                frames.paths.len(),
                timestamps.len()
            )));
        }
        frames.timestamps_sec = timestamps.into_iter().take(frames.paths.len()).collect();

        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, start_sec, frame_count = frames.len(), "Keyframes extracted in range");
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
            .map(|p| format!("file '{}'\n", p))
            .collect();
        self.concat_via_list(&list_path, &list_content, &output, "audio concat")
            .await?;

        let output = output.commit();
        tracing::info!(target: "hkask.mcp.media.ffmpeg", clip_count = audio_paths.len(), output = %output.display(), "Audio concatenated");
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_pts_are_observed_not_ordinal_times_interval() {
        let stderr = "unrelated pts_time:99\n[Parsed_showinfo_1] n: 0 pts_time:3.7 pos:0\n[Parsed_showinfo_1] n: 1 pts_time:7.25 pos:10";
        assert_eq!(
            selected_frame_timestamps(stderr).expect("valid PTS"),
            vec![3.7, 7.25]
        );
        assert!(selected_frame_timestamps("[Parsed_showinfo_1] pts_time:NOPTS").is_err());
        assert!(selected_frame_timestamps("[Parsed_showinfo_1] pts_time:NaN").is_err());
        assert!(
            selected_frame_timestamps(
                "[Parsed_showinfo_1] pts_time:2\n[Parsed_showinfo_1] pts_time:1"
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extraction_refuses_unobserved_pts_and_cleans_scratch()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let (runner, output_dir) = fake_ffmpeg_runner(
            &root,
            "missing-pts.sh",
            "#!/bin/sh\nfor arg do pattern=$arg; done\noutput=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nprintf frame > \"$output\"\n",
        )?;
        let error = runner
            .extract_keyframes("source.mp4", 2.0, 1)
            .await
            .expect_err("missing PTS must not be invented");
        assert!(error.to_string().contains("Missing source PTS"));
        assert_eq!(std::fs::read_dir(output_dir)?.count(), 0);
        Ok(())
    }

    /// Scene detection records the exact select filter (the provenance
    /// record) and parses the selected frames' showinfo PTS from stderr —
    /// the same observed-PTS contract as frame extraction.
    #[cfg(unix)]
    #[tokio::test]
    async fn scene_detection_records_the_filter_and_parses_selected_pts()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let (runner, _output_dir) = fake_ffmpeg_runner(
            &root,
            "scdet.sh",
            "#!/bin/sh\nprintf '[Parsed_showinfo_1] n: 0 pts_time:0.000\\n[Parsed_showinfo_1] n: 1 pts_time:1.000\\n' >&2\n",
        )?;
        let detection = runner
            .detect_scene_changes("source.mp4", 0.3, 0.5)
            .await
            .expect("scene detection parses the fake output");
        assert_eq!(detection.boundaries_sec, vec![0.0, 1.0]);
        assert!(
            detection.filter.contains("gt(scene,0.300)"),
            "the filter records the threshold: {}",
            detection.filter
        );
        assert!(
            detection.filter.contains("gte(t-prev_selected_t,0.500)"),
            "the filter records the minimum shot length: {}",
            detection.filter
        );
        assert!(
            detection
                .filter
                .starts_with("select='isnan(prev_selected_t)+")
        );
        Ok(())
    }

    /// Invalid detection controls fail before any ffmpeg spawn.
    #[tokio::test]
    async fn scene_detection_rejects_invalid_controls() {
        let runner = FfmpegRunner::detect();
        for (threshold, min_shot) in [(0.0, 0.5), (1.5, 0.5), (-0.1, 0.5), (0.3, -1.0)] {
            let error = runner
                .detect_scene_changes("source.mp4", threshold, min_shot)
                .await
                .expect_err("invalid controls must fail");
            assert!(
                error.to_string().contains("Invalid"),
                "names the invalid control: {error}"
            );
        }
    }

    /// Range extraction gates on the range start (the seek's landing
    /// keyframe before the start is not selected) and keeps the observed
    /// showinfo source PTS — the chunk-sampling contract. The fake ffmpeg
    /// writes its one frame to the command's LAST argument — the output
    /// pattern both extraction commands end with; never the argument
    /// after `-vf`, which is the select filter (capturing that once wrote
    /// a file named after the filter string into the crate root).
    #[cfg(unix)]
    #[tokio::test]
    async fn range_extraction_gates_on_start_and_keeps_source_pts()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let (runner, _output_dir) = fake_ffmpeg_runner(
            &root,
            "range-frames.sh",
            "#!/bin/sh\nfor arg do output=$arg; done\nprintf frame > \"$output\"\nprintf '[Parsed_showinfo_1] n: 0 pts_time:10.000\\n' >&2\n",
        )?;
        let frames = runner
            .extract_keyframes_in_range("source.mp4", 10.0, 1.0, 1)
            .await
            .expect("range extraction parses the fake output");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames.timestamps_sec, vec![10.0]);
        assert_eq!(frames.paths.len(), 1);

        let error = runner
            .extract_keyframes_in_range("source.mp4", -1.0, 1.0, 1)
            .await
            .expect_err("negative start fails before spawn");
        assert!(error.to_string().contains("Invalid range start"));
        Ok(())
    }

    /// Write a fake ffmpeg shell script, mark it executable, and build a
    /// runner over it. Returns the runner and its output directory; the
    /// caller owns the TempDir keep-alive.
    #[cfg(unix)]
    fn fake_ffmpeg_runner(
        root: &tempfile::TempDir,
        script_name: &str,
        script_body: &str,
    ) -> Result<(FfmpegRunner, PathBuf), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let fake_ffmpeg = root.path().join(script_name);
        std::fs::write(&fake_ffmpeg, script_body)?;
        let mut permissions = std::fs::metadata(&fake_ffmpeg)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_ffmpeg, permissions)?;
        let output_dir = root.path().join("outputs");
        let runner = FfmpegRunner::with_binary(
            fake_ffmpeg.to_string_lossy().into_owned(),
            output_dir.clone(),
        );
        Ok((runner, output_dir))
    }

    /// expect: A missing local source keeps ffprobe's filesystem cause. [P1]
    #[tokio::test]
    async fn probe_missing_source_preserves_filesystem_cause() {
        let runner = FfmpegRunner::detect();
        assert!(runner.available, "ffprobe behavior requires ffmpeg on PATH");
        let missing = std::env::temp_dir().join("hkask-missing-video-probe.mp4");
        let error = runner
            .probe(&missing.to_string_lossy())
            .await
            .expect_err("missing source must fail");
        let message = error.to_string();
        assert!(message.contains(&missing.to_string_lossy().to_string()));
        assert!(message.contains("No such file or directory"), "{message}");
    }

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
        let root = tempfile::tempdir()?;
        let (runner, output_dir) = fake_ffmpeg_runner(
            &root,
            "extract-one.sh",
            "#!/bin/sh\nfor arg do pattern=$arg; done\noutput=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nprintf frame > \"$output\"\nprintf '[Parsed_showinfo_1] n: 0 pts: 37 pts_time:3.7\\n' >&2\n",
        )?;

        let frames = runner.extract_keyframes("input.mp4", 2.0, 1).await?;
        assert_eq!(frames.len(), 1);
        assert_eq!(frames.timestamps_sec, vec![3.7]);
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
        let root = tempfile::tempdir()?;
        let (runner, output_dir) = fake_ffmpeg_runner(
            &root,
            "extract-then-fail.sh",
            "#!/bin/sh\nfor arg do pattern=$arg; done\noutput=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nprintf partial > \"$output\"\nprintf 'keyframe sentinel failure' >&2\nexit 23\n",
        )?;

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
        let root = tempfile::tempdir()?;
        let (runner, output_dir) = fake_ffmpeg_runner(
            &root,
            "partial-then-fail.sh",
            "#!/bin/sh\nfor arg do output=$arg; done\nprintf partial > \"$output\"\nprintf 'injected partial-output failure' >&2\nexit 23\n",
        )?;

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
