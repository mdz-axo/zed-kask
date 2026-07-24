//! Video module — short video and GIF creation.
//!
//! Tool families:
//! - Local: video_clip, video_to_gif, video_add_caption, video_remix
//! - AI: image_to_video (routed through inference router)

pub mod ffmpeg;
pub mod generation;

pub use ffmpeg::FfmpegRunner;
