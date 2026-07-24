#![forbid(unsafe_code)]
//! hKask Corpus Services — discovery and embedding pipeline.
//!
//! Merged from `hkask-services-discover` and `hkask-services-embed`.

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]

mod discover;
mod embed;

pub use discover::{
    DiscoverRequest, DiscoverResult, DiscoveredWork, DiscoveryService, default_corpus_config,
    download_and_cache, generate_corpus_yaml, slugify,
};
pub use embed::{
    ChunkingConfig, CorpusConfig, EmbedPhase, EmbedProgress, EmbedResult, EmbedService,
    EmbeddingConfig, Entity, EntityConfig, FoundationalRule, ProgressFn, ValidationConfig, Work,
    ocr_pdf_bytes, strip_html_tags,
};
