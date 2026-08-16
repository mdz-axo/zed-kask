//! Error types and filesystem reader for registry and template execution
//!
//! Defines the error taxonomy (`TemplateError`, `SkillFinding`,
//! `ManifestResolveError`) and the `FsSkillReader` filesystem wrapper used by
//! `SkillLoader`.

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

    /// Failed to load a skill from disk (typed replacement for `anyhow::anyhow!`
    /// in `skill_loader.rs`). Carries the path so callers can surface it in
    /// findings without re-formatting.
    #[error("skill load error at {path}: {source}")]
    SkillLoad {
        path: String,
        source: std::io::Error,
    },

    /// SKILL.md frontmatter is missing or malformed. `detail` names the
    /// exact repair (mirrors Nika's `SkillDefect` discipline: each variant
    /// names the fix, not just the failure).
    #[error("SKILL.md frontmatter error: {detail}")]
    Frontmatter { detail: String },
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
            Self::Database(_) => "HKASK-SKILL-004",
            Self::Inference(_) => "HKASK-SKILL-005",
            Self::Mcp(_) => "HKASK-SKILL-006",
            Self::Validation(_) => "HKASK-SKILL-007",
            Self::PathTraversal(_) => "HKASK-SKILL-008",
            Self::SandboxViolation(_) => "HKASK-SKILL-009",
            // HKASK-SKILL-010 was `CapabilityDenied`, removed with the vacuous
            // per-call capability gate. The code is retired, not reused, so old
            // logs remain unambiguous.
            Self::SkillLoad { .. } => "HKASK-SKILL-011",
            Self::Frontmatter { .. } => "HKASK-SKILL-012",
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
            // Json / NotFound / Render / Manifest / Validation / Timeout /
            // PathTraversal / SandboxViolation / SkillLoad / Frontmatter /
            // Database are not throttles.
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

/// One skill-system finding — a typed failure surfaced by skill loading or
/// manifest resolution (mirrors Nika's `SkillFinding`: `code` + `detail`,
/// one voice for check and run). The `code` is a stable `&'static str`
/// so consumers can switch on it without string-matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFinding {
    /// The skill or manifest the finding refers to.
    pub skill_id: String,
    /// Stable code (e.g. `"HKASK-SKILL-001"`). Consumers switch on this.
    pub code: &'static str,
    /// Human-readable detail naming the exact repair.
    pub detail: String,
}

impl SkillFinding {
    /// The human-facing row (check rung, run refusal, log line).
    #[must_use]
    pub fn row(&self) -> String {
        format!(
            "[{code}] {skill}: {detail}",
            code = self.code,
            skill = self.skill_id,
            detail = self.detail
        )
    }

    /// The machine-facing JSON object (check --json, structured logs).
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "skill_id": self.skill_id,
            "code": self.code,
            "detail": self.detail,
        })
    }
}

/// Why a manifest reference did not resolve. Replaces the prior
/// `Option<BundleManifest>` return on `resolve_manifest` (which collapsed
/// three distinct failure modes into `None`).
#[derive(Debug, thiserror::Error)]
pub enum ManifestResolveError {
    /// The reference matched no registry entry and no file path.
    #[error("manifest not found: {reference}")]
    NotFound { reference: String },
    /// A file path matched but the manifest failed to load.
    #[error("manifest load failed for {reference}: {source}")]
    LoadFailed {
        reference: String,
        #[source]
        source: super::manifest_loader::ManifestLoadError,
    },
    /// The manifest loaded but is not a `skill` category (e.g. `qa-script`).
    #[error("manifest '{reference}' is not a skill (category={category})")]
    NotASkill { reference: String, category: String },
}

/// Production filesystem reader — thin wrapper over `std::fs::read_to_string`.
#[derive(Debug, Clone, Copy)]
pub struct FsSkillReader;

impl FsSkillReader {
    /// Read a file's contents as UTF-8 text.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or is not valid UTF-8.
    pub fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}
