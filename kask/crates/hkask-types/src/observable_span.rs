//! ObservableSpan trait — decouples Regulation observability from the monolithic RegulationSpan enum.
//!
//! `SpanNamespace::from_observable()` bridges domain span enums to the
//! validated namespace construction path. The trait is dyn-compatible —
//! domain crates can use either `&dyn ObservableSpan` or monomorphized
//! generics via `from_observable(&impl ObservableSpan)`.
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
/// identifies the span domain. Call sites choose between three emission paths:
///
/// - `emit()` — log-only, no persistence. For call sites without a sink.
/// - `emit_to(sink, ...)` — produce a RegulationRecord, persist through the sink, AND log.
///   The primary path. Regulation consumers (CurationLoop, AlgedonicManager) react to
///   persisted events.
/// - `to_event(...)` — produce a RegulationRecord without persisting. For call sites that
///   batch events or need custom routing.
pub trait ObservableSpan: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static {
    /// Canonical dot-separated namespace string.
    /// Must match the canonical namespace set byte-for-byte (P8 — Semantic Grounding).
    fn as_str(&self) -> &'static str;

    /// Emit a structured tracing event through the Regulation infrastructure.
    ///
    /// Default implementation emits an info-level event with `target = "reg"`,
    /// `reg_domain` set to `self.as_str()`, and `operation` as provided.
    ///
    /// This is the log-only convenience path. Prefer `emit_to()` when a
    /// `RegulationSink` is available — it persists the event for Regulation consumers.
    fn emit(&self, operation: &str) {
        tracing::info!(
            target: "reg",
            reg_domain = %self.as_str(),
            operation = %operation,
            "REG",
        );
    }

    /// Produce a structured RegulationRecord for this span (without persisting).
    ///
    /// Returns `None` if the span's namespace string is not registered in
    /// the canonical namespace set. Callers should fall back to `emit()`
    /// when `None` is returned.
    fn to_event(
        &self,
        operation: &str,
        observer: &crate::WebID,
        phase: crate::event::CyclePhase,
        observation: serde_json::Value,
    ) -> Option<crate::event::RegulationRecord> {
        let ns = crate::event::SpanNamespace::parse(self.as_str())?;
        let span = crate::event::Span::new(ns, operation);
        Some(crate::event::RegulationRecord::new(
            *observer,
            span,
            phase,
            observation,
            0,
        ))
    }

    /// Emit and persist through a RegulationSink.
    ///
    /// Attempts to produce a RegulationRecord via `to_event()`. If the namespace is
    /// canonical, persists through the sink and logs. On namespace miss or
    /// persistence failure, degrades gracefully to log-only via `emit()`.
    fn emit_to(
        &self,
        sink: &dyn crate::event::RegulationSink,
        operation: &str,
        observer: &crate::WebID,
        phase: crate::event::CyclePhase,
        observation: serde_json::Value,
    ) {
        if let Some(event) = self.to_event(operation, observer, phase, observation)
            && let Err(e) = sink.persist(&event)
        {
            tracing::warn!(
                target: "reg",
                domain = %self.as_str(),
                operation = %operation,
                error = %e,
                "Regulation event persistence failed — continuing with log-only",
            );
        }
        self.emit(operation);
    }
}
