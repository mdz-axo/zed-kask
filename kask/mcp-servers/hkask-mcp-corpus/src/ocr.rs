//! OCR Pipeline — Typed, single-backend, self-verifying document processing.
//!
//! Architecture:
//! ```text
//! PDF → [Decimate] → PageQueue → [OCR (the configured OCR model)] → ResultBuffer → [Assembly] → VerifiedDocument
//!                                                                                              ↓
//!                                                                                       [Quality gates]
//!                                                                                              ↓
//!                                                                                       [Verification]
//!                                                                                              ↓
//!                                                                                       PipelineOutcome
//! ```
//!
//! Every page goes to the configured OCR model via the vision transport:
//! image in, text out. Failures are typed and surfaced without substituting
//! another backend; output quality is gated deterministically.

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
