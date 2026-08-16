//! Re-export of the global concurrency limiter from `hkask-types`.
//!
//! The `ConcurrencyLimiter` and its process-global accessor live in
//! `hkask-types` so both `hkask-templates` (skill cascades) and
//! `hkask-mcp-corpus` (OCR pipeline) can share one limiter instance without
//! either depending on the other. This module re-exports the types so
//! `hkask-templates` callers can `use crate::concurrency::ConcurrencyLimiter`
//! without reaching across crate boundaries.

pub use hkask_types::concurrency::{
    ConcurrencyLimiter, ConcurrencyPermit, global_concurrency_limiter,
    set_global_concurrency_limiter,
};
