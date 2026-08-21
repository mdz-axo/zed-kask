//! DocProc MCP tool groups — split by flow stage for P5 essentialism.
//!
//! Flow: gather → process → output
//! - `gather`   — discover academic works, cache extracted content
//! - `document` — convert, OCR, chunk (process stage: text extraction)
//! - `tagging`  — ontology annotation (process stage: tagging)
//! - `semantic` — embed, extract assertions, generate QA (process + QA output)
//! - `corpus`   — dedup, consolidate, build prompts, ingest QA, training data (QA output)
//! - `persona`  — build persona, compose/rewrite/mashup prose, compare, registry (persona output)
//! - `storage`  — cache, query, clear index, purge QA (management)
pub(crate) mod corpus;
pub(crate) mod document;
pub(crate) mod gather;
pub(crate) mod persona;
pub(crate) mod semantic;
pub(crate) mod storage;
pub(crate) mod tagging;
