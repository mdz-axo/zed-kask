//! Process-global concurrency limiter wiring.
//!
//! Holds the `ConcurrencyLimiter` in a `OnceLock` so any consumer (skill
//! cascades via `Infra`, corpus OCR, MCP tool calls, future callers) can
//! acquire permits from one shared instance. The limiter is wired once at
//! startup from `KaskSettings::general` (after `settings::init`).
//!
//! Per `.rules`: `OnceLock` hooks must warn on the `Err` branch of `set` —
//! operators can't distinguish "not configured" from "configured but broken"
//! without it. Runtime changes to `max_concurrency` / `concurrency_step` do
//! NOT take effect until restart (the `OnceLock` is set once); this matches
//! the existing `set_manifest_executor` pattern.

use std::sync::{Arc, OnceLock};

use hkask_templates::concurrency::ConcurrencyLimiter;

static GLOBAL_LIMITER: OnceLock<Arc<ConcurrencyLimiter>> = OnceLock::new();

/// Wire the global concurrency limiter from `KaskGeneralSettings`. Called
/// once at startup after settings load. A second call warns and drops the
/// new limiter — the previously-wired limiter remains active.
pub fn set_global_concurrency_limiter(max_concurrency: u32, concurrency_step: u32) {
    let limiter = Arc::new(ConcurrencyLimiter::new(max_concurrency, concurrency_step));
    if GLOBAL_LIMITER.set(limiter).is_err() {
        log::warn!(
            "set_global_concurrency_limiter: hook already set — second wiring attempt \
             dropped. The previously-wired limiter remains active. Restart the app to \
             re-wire from a clean process."
        );
    }
}

/// Access the process-global concurrency limiter. Returns `None` before
/// `set_global_concurrency_limiter` has run (tests, pre-startup). Callers
/// must skip gating when `None`.
pub fn global_concurrency_limiter() -> Option<&'static Arc<ConcurrencyLimiter>> {
    GLOBAL_LIMITER.get()
}
