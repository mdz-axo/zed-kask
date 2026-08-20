//! Re-export of the global concurrency limiter wiring from `hkask-types`.
//!
//! The `ConcurrencyLimiter`, its process-global `OnceLock`, and the
//! `set_global_concurrency_limiter` / `global_concurrency_limiter` accessors
//! live in `hkask-types` so both the skill system and
//! `hkask-mcp-corpus` (OCR pipeline) can share one limiter instance. This
//! module re-exports them so `kask_bridge` callers (and `main.rs`) can
//! `use kask_bridge::set_global_concurrency_limiter` without a direct
//! `hkask-types` path.

pub use hkask_types::concurrency::{global_concurrency_limiter, set_global_concurrency_limiter};
