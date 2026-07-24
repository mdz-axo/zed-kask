//! ffmpeg subprocess wrappers for video processing.
//!
//! Detects ffmpeg at startup with graceful degradation.
//! All operations use temp directories for output files.

use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// ffmpeg runner with availability detection.
#[derive(Debug, Clone)]
pub struct FfmpegRunner {
    pub available: bool,
    ffmpeg_path: String,
    temp_dir: PathBuf,
}

impl Drop for FfmpegRunner {
    fn drop(&mut self) {
        // Clean up accumulated temp files on server shutdown
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

impl FfmpegRunner {
    /// Detect ffmpeg on PATH. Returns a runner with `available` set accordingly.
    /// Cleans up leftover temp files from previous crashed sessions.
    pub fn detect() -> Self {
        let ffmpeg_path = "ffmpeg".to_string();
        let available = std::process::Command::new(&ffmpeg_path)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let temp_dir = std::env::temp_dir().join("hkask-media");

        // Clean up leftover temp files from previous crashed sessions
        let _ = std::fs::remove_dir_all(&temp_dir);

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

        let output = self.output_path("mp4");
        let duration = end_sec - start_sec;

        let status = Command::new(&self.ffmpeg_path)
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
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!("ffmpeg clip spawn failed: {}", e))
            })?;

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg clip failed with exit code: {:?}",
                status.code()
            )));
        }

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

        let output = self.output_path("gif");
        let palette = self.output_path("png");

        // Build filter complex for palette generation + GIF conversion
        let filter = format!(
            "fps={},scale={}:-1:flags=lanczos,split[v1][v2];[v1]palettegen[p];[v2][p]paletteuse",
            fps, width
        );

        let status = Command::new(&self.ffmpeg_path)
            .arg("-ss")
            .arg(format!("{:.3}", start_sec))
            .arg("-t")
            .arg(format!("{:.3}", duration_sec))
            .arg("-i")
            .arg(input)
            .arg("-filter_complex")
            .arg(&filter)
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!("ffmpeg GIF spawn failed: {}", e))
            })?;

        // Clean up palette temp file
        let _ = std::fs::remove_file(&palette);

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg GIF conversion failed with exit code: {:?}",
                status.code()
            )));
        }

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

        let output = self.output_path("mp4");

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

        let status = Command::new(&self.ffmpeg_path)
            .arg("-i")
            .arg(input)
            .arg("-vf")
            .arg(&drawtext)
            .arg("-c:a")
            .arg("copy")
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!("ffmpeg caption spawn failed: {}", e))
            })?;

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg caption failed with exit code: {:?}",
                status.code()
            )));
        }

        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, text = %text, output = %output.display(), "Caption added");
        Ok(output)
    }

    /// Capture audio from the default system input device.
    /// Uses ffmpeg to record from the platform-specific default audio source.
    /// Saves to a WAV file in the temp directory (or specified path).
    pub async fn capture_audio(
        &self,
        duration_secs: f32,
        output_path: Option<&str>,
    ) -> Result<PathBuf, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let output = match output_path {
            Some(p) => PathBuf::from(p),
            None => self.output_path("wav"),
        };

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

        let status = Command::new(&self.ffmpeg_path)
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
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!("ffmpeg audio capture spawn failed: {}", e))
            })?;

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg audio capture failed with exit code: {:?}",
                status.code()
            )));
        }

        tracing::info!(target: "hkask.mcp.media.ffmpeg", duration = %duration_secs, output = %output.display(), "Audio captured");
        Ok(output)
    }

    /// Create a video from a sequence of images.
    /// Images are concatenated at the specified frame rate.
    pub async fn images_to_video(
        &self,
        image_paths: &[PathBuf],
        fps: u32,
        output_format: &str,
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

        let ext = match output_format {
            "gif" => "gif",
            "webp" => "webp",
            _ => "mp4",
        };
        let output = self.output_path(ext);

        // Write image list to a temp file for concat demuxer
        let list_path = self.output_path("txt");
        let list_content: String = image_paths
            .iter()
            .map(|p| format!("file '{}'", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&list_path, list_content)
            .map_err(|e| crate::MediaError::Io(format!("Failed to write image list: {}", e)))?;

        let status = Command::new(&self.ffmpeg_path)
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-r")
            .arg(fps.to_string())
            .arg("-i")
            .arg(&list_path)
            .arg("-c:v")
            .arg(if ext == "gif" || ext == "webp" {
                "libwebp"
            } else {
                "libx264"
            })
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!(
                    "ffmpeg images_to_video spawn failed: {}",
                    e
                ))
            })?;

        let _ = std::fs::remove_file(&list_path);

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg images_to_video failed with exit code: {:?}",
                status.code()
            )));
        }

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

        let output = self.output_path("mp4");

        // Write concat list
        let list_path = self.output_path("txt");
        let list_content: String = video_paths
            .iter()
            .map(|p| format!("file '{}'", p.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&list_path, list_content)
            .map_err(|e| crate::MediaError::Io(format!("Failed to write concat list: {}", e)))?;

        let status = Command::new(&self.ffmpeg_path)
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-i")
            .arg(&list_path)
            .arg("-c")
            .arg("copy")
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!("ffmpeg concat spawn failed: {}", e))
            })?;

        let _ = std::fs::remove_file(&list_path);

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg concat failed with exit code: {:?}",
                status.code()
            )));
        }

        tracing::info!(target: "hkask.mcp.media.ffmpeg", clip_count = video_paths.len(), output = %output.display(), "Videos concatenated");
        Ok(output)
    }

    /// Extract keyframes from a video at regular intervals.
    /// Returns paths to extracted frame images for vision LLM analysis.
    pub async fn extract_keyframes(
        &self,
        input: &str,
        interval_sec: f32,
        max_frames: u32,
    ) -> Result<Vec<PathBuf>, crate::MediaError> {
        if !self.available {
            return Err(crate::MediaError::FfmpegUnavailable);
        }
        self.ensure_temp_dir()?;

        let prefix = uuid::Uuid::new_v4().to_string();
        let pattern = self.temp_dir.join(format!("{}_%03d.jpg", prefix));

        let status = Command::new(&self.ffmpeg_path)
            .arg("-i")
            .arg(input)
            .arg("-vf")
            .arg(format!("fps=1/{},scale=640:-1", interval_sec))
            .arg("-vframes")
            .arg(max_frames.to_string())
            .arg(&pattern)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| {
                crate::MediaError::FfmpegFailed(format!(
                    "ffmpeg keyframe extraction spawn failed: {}",
                    e
                ))
            })?;

        if !status.success() {
            return Err(crate::MediaError::FfmpegFailed(format!(
                "ffmpeg keyframe extraction failed with exit code: {:?}",
                status.code()
            )));
        }

        // Collect generated frame files
        let mut frames = Vec::new();
        for i in 1..=max_frames {
            let path = self.temp_dir.join(format!("{}_ {:03}.jpg", prefix, i));
            if path.exists() {
                frames.push(path);
            }
        }

        tracing::info!(target: "hkask.mcp.media.ffmpeg", input = %input, frame_count = frames.len(), "Keyframes extracted");
        Ok(frames)
    }
}
