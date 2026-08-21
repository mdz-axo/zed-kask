//! hKask Corpus Services — discovery and embedding pipeline.
//!
//! Merged from `hkask-services-discover` and `hkask-services-embed`.

mod discover;
pub(crate) mod embed;
pub(crate) mod fetch;

pub(crate) use discover::CompanySourceManifest;
pub(crate) use embed::{EmbedProgress, EmbedService};
