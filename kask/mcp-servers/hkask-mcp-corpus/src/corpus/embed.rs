//! EmbedService — Style corpus embedding pipeline with metadata layer.
//! # REQ: P3 (Generative Space) — full parameter exposure, no hidden settings.
//! # expect: "The service layer enables generative access to domain capabilities"
//!
//! ## Pipeline phases
//! 1. **Parse config** — YAML with entities, methods, budget, works
//! 2. **Download & chunk** — Gutenberg texts → tagged passages
//! 3. **Tag** — entity matching + method signal extraction
//! 4. **Salience** — weighted graph degree centrality per passage
//! 5. **Budget gate** — sort by salience, top-N by h_mem budget
//! 6. **Embed** — all passages get vectors (via inference providers)
//! 7. **Store h_mems** — budget-selected passages get metadata h_mems
//! 8. **Centroid** — mean vector over prose passages

mod hmems;
mod html;
mod ocr;
mod passage;
mod service;
mod strategies;
mod types;

pub use html::strip_html_tags;
pub use ocr::ocr_pdf_bytes;
pub use service::EmbedService;
pub use strategies::WordCountChunker;
pub use types::{
    CorpusConfig, DimensionCentroid, EmbedPhase, EmbedProgress, EmbedResult,
    EmbeddingConfig, Entity, EntityConfig, FoundationalRule, ProgressFn, TagSet, ValidationConfig,
    Work,
};
