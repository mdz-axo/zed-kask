//! Source video inspection, extraction, and traceable presentation derivatives.

pub mod ffmpeg;
pub mod ytdlp;

pub use ffmpeg::FfmpegRunner;
pub use ytdlp::YtDlpRunner;
