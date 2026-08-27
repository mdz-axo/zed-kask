//! Service layer for docproc/corpus tools — extracted from god-method tool files.
//!
//! Each service struct holds the shared inference router and exposes an async
//! method containing the orchestration logic that was previously inline in
//! `CorpusServer` tool methods. The tool `#[tool]` methods become thin I/O
//! framing: deserialize params, construct the service, delegate, return.
//!
//! Follows the `ComposeService` pattern (`src/compose.rs`): service struct +
//! request/result types + `#[must_use]` on the service method.
//!
//! - `convert`         — `ConvertService` (document conversion + directory chunking)
//! - `assertions`         — `AssertionsService` (extract RDF h_mems from chunks)
//! - `cluster`         — shared load → normalize → cluster pipeline (dedup + consolidation)
//! - `consolidation`   — `ConsolidationService` (cluster + LLM-synthesize + re-embed)
//! - `prompt_builder`  — `PromptBuilderService` (KNN + concept graph + knowledge graph + QA prompts)
//! - `qa_pipeline`     — shared QA prompt formatting + result envelope construction
//!   (used by `corpus_generate_qa`, `corpus_generate_qa_batch`, and the batch API path)

pub(crate) mod assertions;
pub(crate) mod cluster;
pub(crate) mod consolidation;
pub(crate) mod convert;
pub(crate) mod prompt_builder;
pub(crate) mod qa_pipeline;
