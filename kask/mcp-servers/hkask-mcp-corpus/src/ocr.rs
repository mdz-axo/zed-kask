//! OCR Pipeline — Typed, single-backend, self-verifying document processing.
//!
//! Architecture:
//! ```text//! PDF → [Decimate] → PageQueue → [OCR (configured vision model)] → ResultBuffer → [Assembly] → VerifiedDocument
//!                                                                                              ↓
//!                                                                                       [Quality gates]
//!                                                                                              ↓
//!                                                                                       [Verification]
//!                                                                                              ↓
//!                                                                                       PipelineOutcome
//! ```
//!
//! The Tesseract backend, its complexity-tier routing, and the fallback
//! ladder were removed (2026-09-10): tier routing silently sent book pages
//! to a garbage-quality engine, and fallbacks silently substituted degraded
//! text. Every page goes to the configured vision model; failures are
//! typed and surfaced; output quality is gated deterministically.

pub(crate) mod config;
pub(crate) mod decimation;
pub(crate) mod document;
pub(crate) mod llm_ocr;
pub(crate) mod pipeline;
pub(crate) mod quality;
pub(crate) mod server;
pub(crate) mod triage;
pub(crate) mod verification;

pub(crate) use config::*;
pub(crate) use document::*;
pub(crate) use server::PipelineExecutor;
