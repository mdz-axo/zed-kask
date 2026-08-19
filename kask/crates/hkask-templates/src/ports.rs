//! Error types for registry and template execution
//!
//! Defines the error taxonomy (`TemplateError`) for template execution.

use hkask_types::NotFound;

/// Error type for template operations
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("Template not found: {0}")]
    NotFound(NotFound),

    #[error("Render error: {0}")]
    Render(String),
    #[error("Manifest error: {0}")]
    Manifest(String),
    /// A step exceeded its `timeout_seconds`. Carries the step ordinal and
    /// the elapsed seconds so the retry loop in `run_pass` can detect it
    /// without string-matching the `Manifest` message, and so callers can
    /// report which step hung. Typed (not a `Manifest(String)`) because
    /// retry policy branches on it.
    #[error("Step {step_ordinal} timed out after {elapsed_seconds}s")]
    Timeout {
        step_ordinal: u32,
        elapsed_seconds: u64,
    },
    /// Parse failure (JSON parse, empty output, or truncation). Typed so the
    /// retry loop in `run_pass` can detect it without string-matching the
    /// `Manifest` message — same pattern as `Timeout`.
    #[error("Step {step_ordinal}: parse failure: {detail}")]
    ParseFailure { step_ordinal: u32, detail: String },
    #[error("Database error: {0}")]
    Database(#[from] hkask_types::InfrastructureError),
    #[error("Inference error: {0}")]
    Inference(#[from] hkask_types::InferenceError),
    #[error("MCP error: {0}")]
    Mcp(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Path traversal attempt: {0}")]
    PathTraversal(String),
    #[error("Sandbox violation: {0}")]
    SandboxViolation(String),
}

impl From<NotFound> for TemplateError {
    fn from(nf: NotFound) -> Self {
        TemplateError::NotFound(nf)
    }
}

impl TemplateError {
    /// Stable error code for machine-readable consumption (mirrors Nika's
    /// `nika_code()` pattern). Consumers can switch on this without
    /// string-matching the Display output.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "HKASK-SKILL-001",
            Self::Render(_) => "HKASK-SKILL-002",
            Self::Manifest(_) => "HKASK-SKILL-003",
            Self::Timeout { .. } => "HKASK-SKILL-013",
            Self::ParseFailure { .. } => "HKASK-SKILL-014",
            Self::Database(_) => "HKASK-SKILL-004",
            Self::Inference(_) => "HKASK-SKILL-005",
            Self::Mcp(_) => "HKASK-SKILL-006",
            Self::Validation(_) => "HKASK-SKILL-007",
            Self::PathTraversal(_) => "HKASK-SKILL-008",
            Self::SandboxViolation(_) => "HKASK-SKILL-009",
            // HKASK-SKILL-010 was `CapabilityDenied`, removed with the vacuous
            // per-call capability gate. HKASK-SKILL-011 (`SkillLoad`) and
            // HKASK-SKILL-012 (`Frontmatter`) were removed with the dead
            // `skill_loader.rs` module. The codes are retired, not reused, so
            // old logs remain unambiguous.
        }
    }

    /// Whether this error is transient (retryable). Mirrors Nika's
    /// `is_transient()` pattern. Database, inference, MCP, and timeout
    /// errors are transient; validation, not-found, and security errors
    /// are not. Timeouts are transient because a slow model round-trip
    /// may succeed on retry (cold cache warms, transient network latency
    /// clears).
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Database(_) | Self::Inference(_) | Self::Mcp(_) | Self::Timeout { .. }
        )
    }

    /// Whether this error represents a provider-side throttle (429 / 503 /
    /// rate-limit) that should cause the global concurrency limiter to back
    /// off. Distinct from `is_transient`: a 400 validation error is
    /// transient (the model may emit valid JSON on retry) but is NOT a
    /// throttle — backing off the limiter on a deterministic failure would
    /// shrink the pool for unrelated callers. Per `.rules`: error
    /// classification must be per-variant, not blanket.
    ///
    /// The classifier is conservative: it string-matches the inner message
    /// for HTTP status codes and rate-limit signals because `InferenceError`
    /// variants carry a free-form `String`, not a typed HTTP status. A false
    /// positive (treating a non-throttle as a throttle) only causes a
    /// one-step backoff, which `on_success` reverses on the next call —
    /// self-correcting. A false negative (missing a real throttle) leaves
    /// the pool at its current size, which the next 429 will catch.
    #[must_use]
    pub fn is_throttle(&self) -> bool {
        let message = match self {
            Self::Inference(hkask_types::InferenceError::Connection(m))
            | Self::Inference(hkask_types::InferenceError::Model(m))
            | Self::Inference(hkask_types::InferenceError::Generation(m)) => m.as_str(),
            Self::Mcp(err) => &err.to_string(),
            // CircuitOpen is a sustained-breaker state, not a single throttle.
            // Json / NotFound / Render / Manifest / ParseFailure / Validation /
            // Timeout / PathTraversal / SandboxViolation / Database are not
            // throttles.
            _ => return false,
        };
        let lower = message.to_ascii_lowercase();
        lower.contains("429")
            || lower.contains("503")
            || lower.contains("rate limit")
            || lower.contains("rate_limit")
            || lower.contains("too many requests")
            || lower.contains("throttl")
    }
}

pub type Result<T> = std::result::Result<T, TemplateError>;
