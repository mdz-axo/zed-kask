//! DocProc MCP tool groups — split by flow stage for P5 essentialism.
//!
//! Flow: gather → process → output
//! - `gather`   — discover academic works, cache extracted content
//! - `document` — convert, OCR, chunk (process stage: text extraction)
//! - `tagging`  — ontology annotation (process stage: tagging)
//! - `semantic` — embed, extract triples, generate QA (process + QA output)
//! - `corpus`   — dedup, consolidate, build prompts, ingest QA, training data (QA output)
//! - `persona`  — build persona, compose/rewrite/mashup prose, compare, registry (persona output)
//! - `storage`  — cache, query, clear index, purge QA (management)
pub mod corpus;
pub mod document;
pub mod gather;
pub mod persona;
pub mod semantic;
pub mod storage;
pub mod tagging;
