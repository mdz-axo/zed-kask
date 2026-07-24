#![forbid(unsafe_code)]
//! hKask inference service layer — `InferenceContext`, `InferenceService`, and the
//! `ModelCache` TTL cache. Extracted from `hkask-services-core` (see
//! `tasks/plan-core-scope-contraction.md`, Task 3.1).
//!
//! `InferenceService` is the service-layer façade over `hkask-inference`'s
//! `InferenceRouter`; `ModelCache` is the process-scoped TTL cache for model lists
//! (poison-recovering — see ADR-043). `ServiceError` is still sourced
//! from `hkask-services-core` (the one remaining foundation dep).

mod inference_svc;
mod model_cache;

pub use inference_svc::{InferenceContext, InferenceService, ModelInfo};
pub use model_cache::ModelCache;
