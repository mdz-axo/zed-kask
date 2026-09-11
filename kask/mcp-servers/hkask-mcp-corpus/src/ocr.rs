//! OCR Pipeline — Typed, single-backend, self-verifying document processing.
//!
//! Architecture:
//! ```text//! PDF → [Decimate] → PageQueue → [OCR (the configured OCR model)] → ResultBuffer → [Assembly] → VerifiedDocument
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
//! text. Every page goes to the configured OCR model (a dedicated OCR
//! endpoint such as `runpod/kask-ocr` — OLMOCR-2 — invoked via the vision
//! transport: image in, text out); failures are
//! typed and surfaced; output quality is gated deterministically.

pub(crate) mod config;
pub(crate) mod decimation;
pub(crate) mod document;
pub(crate) mod llm_ocr;
pub(crate) mod pipeline;
pub(crate) mod quality;
pub(crate) mod triage;
pub(crate) mod verification;

pub(crate) use config::*;
pub(crate) use document::*;
