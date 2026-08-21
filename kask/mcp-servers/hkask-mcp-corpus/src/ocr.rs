//! OCR Pipeline — Typed, multi-backend, self-verifying document processing.
//!
//! Architecture:
//! ```text
//! PDF → [Decimate] → PageQueue → [Score → Route → OCR] → ResultBuffer → [Assembly] → VerifiedDocument
//!                                                                             ↓
//!                                                                      [Verification]
//!                                                                             ↓
//!                                                                      PipelineOutcome
//! ```

pub(crate) mod calibration;
pub(crate) mod complexity;
pub(crate) mod config;
pub(crate) mod decimation;
pub(crate) mod document;
pub(crate) mod llm_ocr;
pub(crate) mod pipeline;
pub(crate) mod routing;
pub(crate) mod server;
pub(crate) mod tesseract;
pub(crate) mod triage;
pub(crate) mod verification;

pub(crate) use config::*;
pub(crate) use document::*;
pub(crate) use server::PipelineExecutor;
