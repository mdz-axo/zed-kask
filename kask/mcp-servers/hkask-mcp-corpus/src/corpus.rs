//! hKask Corpus Services — discovery and embedding pipeline.
//!
//! Merged from `hkask-services-discover` and `hkask-services-embed`.

mod discover;
mod embed;

pub use discover::{
    DiscoverRequest, DiscoverResult, DiscoveredWork, DiscoveryService, default_corpus_config,
    download_and_cache, generate_corpus_yaml, slugify,
};
pub use embed::{
    ChunkingConfig, CorpusConfig, DimensionCentroid, EmbedPhase, EmbedProgress, EmbedResult,
    EmbedService, EmbeddingConfig, Entity, EntityConfig, FoundationalRule, ProgressFn, TagSet,
    ValidationConfig, Work, ocr_pdf_bytes, strip_html_tags,
};
