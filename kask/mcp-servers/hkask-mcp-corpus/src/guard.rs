//! Content safety guard — shared across the corpus pipeline.
//!
//! The output pipeline (secret stripping) is ALWAYS active — secrets must never
//! enter shared memory (P3.1 floor). The input pipeline (prompt injection / role
//! override) protects interactive agent boundaries from untrusted user input.
//! For the docproc corpus curation pipeline, which processes operator-curated
//! literature rather than untrusted user input, the operator may disable input
//! scanning via `HKASK_ENABLE_CONTENT_GUARD=false`. Defaults to enabled.

// Content safety guard — mandatory at every LLM boundary (OWASP LLM01/02/04/06).
pub(crate) static GUARD: std::sync::LazyLock<hkask_guard::ContentGuard> =
    std::sync::LazyLock::new(|| {
        hkask_guard::ContentGuard::mandatory(&hkask_guard::GuardConfig::default())
    });

/// Whether input-guard scanning is active for the docproc corpus pipeline.
///
/// Read once per process from `HKASK_ENABLE_CONTENT_GUARD`. Unset or any value
/// other than `false`/`0`/`off`/`no` leaves it enabled (safe default). The output
/// guard (`scan_output`) is always invoked regardless of this flag — secrets
/// must never enter shared memory.
pub(crate) static INPUT_GUARD_ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    !matches!(
        std::env::var("HKASK_ENABLE_CONTENT_GUARD")
            .ok()
            .map(|v| v.to_ascii_lowercase())
            .as_deref(),
        Some("false" | "0" | "off" | "no")
    )
});
