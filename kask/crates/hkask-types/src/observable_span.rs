//! ObservableSpan trait — decouples Regulation observability from the monolithic RegulationSpan enum.
//!
//! The trait is dyn-compatible — domain crates can use either `&dyn ObservableSpan`
//! or monomorphized generics.
//!
//! # Relationship to SpanNamespace
//!
//! `SpanNamespace` (in `event.rs`) is a validated string wrapper that enforces
//! the canonical namespace set. `ObservableSpan` is the trait that typed span
//! enums implement — it provides the canonical namespace string that feeds into
//! `SpanNamespace` construction.
//!
//! # Design
//!
//! ```text
//! ObservableSpan (trait)
//!   ├── RegulationSpan (canonical Regulation spans — hkask-types)
//!   ├── WalletSpan (future: wallet-specific spans — the wallet crate)
//!   └── ... (per-domain span enums)
//! ```rust,no_run
//!
//! # Example
//!
//! ```rust,ignore
//! use hkask_types::ObservableSpan;
//!
//! #[derive(Debug, Clone)]
//! enum MyDomainSpan { OperationA, OperationB }
//!
//! impl ObservableSpan for MyDomainSpan {
//!     fn as_str(&self) -> &'static str {
//!         match self {
//!             Self::OperationA => "mydomain.operation_a",
//!             Self::OperationB => "mydomain.operation_b",
//!         }
//!     }
//! }
//! ```

/// Trait for typed observability spans that can be emitted through the Regulation
/// infrastructure — both as structured RegulationRecords (persisted + queried) and
/// as tracing log events (for external consumers like OpenTelemetry exporters).
///
/// A canonical dot-separated namespace string (e.g. `"reg.tool.web_search"`)
/// identifies the span domain. Call sites emit through the `emit()` method —
/// log-only, no persistence. For persisted events, construct a `RegulationRecord`
/// directly and persist through a `RegulationSink`.
pub trait ObservableSpan: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static {
    /// Canonical dot-separated namespace string.
    /// Must match the canonical namespace set byte-for-byte (P8 — Semantic Grounding).
    fn as_str(&self) -> &'static str;

    /// Emit a structured tracing event through the Regulation infrastructure.
    ///
    /// Default implementation emits an info-level event with `target = "reg"`,
    /// `reg_domain` set to `self.as_str()`, and `operation` as provided.
    ///
    /// This is the log-only convenience path. For persisted events, construct
    /// a `RegulationRecord` directly and persist through a `RegulationSink`.
    fn emit(&self, operation: &str) {
        tracing::info!(
            target: "reg",
            reg_domain = %self.as_str(),
            operation = %operation,
            "REG",
        );
    }
}
