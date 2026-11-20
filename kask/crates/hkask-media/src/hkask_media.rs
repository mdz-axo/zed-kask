#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hkask-media — pure domain models and algebra for source-media research
//! and curation.
//!
//! Owns the transcript bundle, layer validation, EDL selection algebra,
//! re-transcription edit merging, MovieLabs OMC metadata, media-block
//! enrichment, and the `job_list` wire contract shared by `hkask-mcp-media`
//! and `crates/media_panel`.

pub mod jobs_contract;
pub mod media_block;
pub mod media_limits;
pub mod omc;
#[cfg(test)]
mod test_support;
pub mod transcript;
pub mod transcript_layers;
pub mod transcript_retranscribe;
pub mod transcript_select;
pub mod wire;
pub mod ytdlp;
