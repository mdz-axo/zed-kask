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

pub mod calibration;
pub mod complexity;
pub mod config;
pub mod decimation;
pub mod document;
pub mod llm_ocr;
pub mod pipeline;
pub mod routing;
pub mod server;
pub mod tesseract;
pub mod triage;
pub mod verification;

pub use config::*;
pub use document::*;
pub use server::PipelineExecutor;
