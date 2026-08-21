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
//! - `consolidation`   — `ConsolidationService` (cluster + LLM-synthesize + re-embed)
//! - `prompt_builder`  — `PromptBuilderService` (KNN + concept graph + knowledge graph + QA prompts)

pub(crate) mod assertions;
pub(crate) mod consolidation;
pub(crate) mod convert;
pub(crate) mod prompt_builder;
