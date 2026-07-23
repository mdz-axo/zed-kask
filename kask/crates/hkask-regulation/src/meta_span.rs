//! Meta Regulation spans — the Curator's self-observation channel.
//!
//! `reg.meta.*` records the Curator's OWN decision quality (directives issued,
//! escalation outcomes, circuit-breaker trips, self-calibrations) so a
//! meta-observer can manage the Curator's evolution.
//!
//! # Non-circularity (design constraint)
//!
//! These spans are deliberately NOT in `ALGEDONIC_SPAN_CATEGORIES`
//! (`hkask-storage::regulation_store`). `CurationLoop::sense()` reads via
//! `query_algedonic`, which filters by that category list — so CurationLoop
//! never reads its own self-observation spans back. The Curator observes the
//! *system* (reg.* from other loops) and its *own* in-process counters; it
//! emits `reg.meta.*` for external observability and for a dedicated
//! meta-observer distinct from CurationLoop. This keeps the authority DAG
//! acyclic: Meta -> Curation -> Cybernetics -> domains.

use hkask_types::ObservableSpan;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};

/// Curator self-observation span kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetaSpan {
    /// A CuratorDirective was issued (observation: variant, target agent).
    DirectiveIssued,
    /// An escalation was persisted or dropped (observation: outcome, confidence).
    EscalationOutcome,
    /// The template circuit breaker tripped (observation: skip cycles remaining).
    CircuitBreakerTrip,
    /// The Curator adjusted its own threshold (observation: metric, old, new).
    SelfCalibration,
}

impl MetaSpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaSpan::DirectiveIssued => "reg.meta.directive",
            MetaSpan::EscalationOutcome => "reg.meta.escalation",
            MetaSpan::CircuitBreakerTrip => "reg.meta.circuit_breaker",
            MetaSpan::SelfCalibration => "reg.meta.self_calibration",
        }
    }

    /// The span path (sub-event) for this span kind.
    const fn path(&self) -> &'static str {
        match self {
            MetaSpan::DirectiveIssued => "issued",
            MetaSpan::EscalationOutcome => "outcome",
            MetaSpan::CircuitBreakerTrip => "tripped",
            MetaSpan::SelfCalibration => "adjusted",
        }
    }
}

impl std::fmt::Display for MetaSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for MetaSpan {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.meta.directive" => Ok(MetaSpan::DirectiveIssued),
            "reg.meta.escalation" => Ok(MetaSpan::EscalationOutcome),
            "reg.meta.circuit_breaker" => Ok(MetaSpan::CircuitBreakerTrip),
            "reg.meta.self_calibration" => Ok(MetaSpan::SelfCalibration),
            _ => Err(()),
        }
    }
}

impl ObservableSpan for MetaSpan {
    fn as_str(&self) -> &'static str {
        MetaSpan::as_str(self)
    }
}

// ── Emission helpers ────────────────────────────────────────────────────────

/// Emit a `reg.meta.directive` span recording that the Curator issued a directive.
///
/// Degrades gracefully: on namespace miss or persistence failure, logs a warning
/// and continues (directive issuance is never blocked by observability).
pub fn emit_meta_directive(
    sink: &dyn RegulationSink,
    observer: &WebID,
    variant: &str,
    target: Option<&WebID>,
) {
    let Some(ns) = SpanNamespace::from_observable(&MetaSpan::DirectiveIssued) else {
        tracing::warn!(target: "hkask.meta", "reg.meta.directive namespace not canonical");
        return;
    };
    let span = Span::new(ns, MetaSpan::DirectiveIssued.path());
    let observation = serde_json::json!({
        "directive": variant,
        "target": target.map(|w| w.to_string()).unwrap_or_default(),
    });
    let event = RegulationRecord::new(*observer, span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.meta", error = %e, "Failed to persist reg.meta.directive");
    }
}

/// Emit a `reg.meta.escalation` span recording an escalation persistence outcome.
pub fn emit_meta_escalation(
    sink: &dyn RegulationSink,
    observer: &WebID,
    outcome: &str,
    confidence: f64,
) {
    let Some(ns) = SpanNamespace::from_observable(&MetaSpan::EscalationOutcome) else {
        tracing::warn!(target: "hkask.meta", "reg.meta.escalation namespace not canonical");
        return;
    };
    let span = Span::new(ns, MetaSpan::EscalationOutcome.path());
    let observation = serde_json::json!({ "outcome": outcome, "confidence": confidence });
    let event = RegulationRecord::new(*observer, span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.meta", error = %e, "Failed to persist reg.meta.escalation");
    }
}

/// Emit a `reg.meta.circuit_breaker` span recording a template circuit-breaker trip.
pub fn emit_meta_circuit_breaker(sink: &dyn RegulationSink, observer: &WebID, skip_cycles: u64) {
    let Some(ns) = SpanNamespace::from_observable(&MetaSpan::CircuitBreakerTrip) else {
        tracing::warn!(target: "hkask.meta", "reg.meta.circuit_breaker namespace not canonical");
        return;
    };
    let span = Span::new(ns, MetaSpan::CircuitBreakerTrip.path());
    let observation = serde_json::json!({ "skip_cycles": skip_cycles });
    let event = RegulationRecord::new(*observer, span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.meta", error = %e, "Failed to persist reg.meta.circuit_breaker");
    }
}

/// Data for a `reg.meta.self_calibration` span emission.
///
/// Carries TWO signals:
/// - `signal_before/after` — the PRIMARY causal signal (escalation drop count),
///   lever-controlled by the threshold. This is what the runtime loop judges on.
/// - `eff_before/after` — the SECONDARY signal (regulation_effectiveness), a
///   higher-order system metric from a different loop. Retained for offline GEPA
///   trajectory analysis but NOT used for runtime self-calibration judgments.
pub struct CalibrationSpan<'a> {
    pub metric: &'a str,
    pub old: u64,
    pub new: u64,
    pub signal_before: Option<f64>,
    pub signal_after: Option<f64>,
    pub eff_before: Option<f64>,
    pub eff_after: Option<f64>,
    pub source: &'a str,
}

/// Emit a `reg.meta.self_calibration` span recording a self-applied threshold change.
/// The span records both the primary signal (drop count delta — lever-controlled)
/// and the secondary signal (effectiveness delta — retained for offline GEPA).
pub fn emit_meta_self_calibration(
    sink: &dyn RegulationSink,
    observer: &WebID,
    data: &CalibrationSpan,
) {
    let Some(ns) = SpanNamespace::from_observable(&MetaSpan::SelfCalibration) else {
        tracing::warn!(target: "hkask.meta", "reg.meta.self_calibration namespace not canonical");
        return;
    };
    let span = Span::new(ns, MetaSpan::SelfCalibration.path());
    let signal_delta = match (data.signal_before, data.signal_after) {
        (Some(b), Some(a)) => Some(a - b),
        _ => None,
    };
    let eff_delta = match (data.eff_before, data.eff_after) {
        (Some(b), Some(a)) => Some(a - b),
        _ => None,
    };
    let observation = serde_json::json!({
        "metric": data.metric,
        "old": data.old,
        "new": data.new,
        "signal_before": data.signal_before,
        "signal_after": data.signal_after,
        "signal_delta": signal_delta,
        "eff_before": data.eff_before,
        "eff_after": data.eff_after,
        "eff_delta": eff_delta,
        "source": data.source,
    });
    let event = RegulationRecord::new(*observer, span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.meta", error = %e, "Failed to persist reg.meta.self_calibration");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn meta_span_namespaces_are_canonical() {
        let all = vec![
            MetaSpan::DirectiveIssued,
            MetaSpan::EscalationOutcome,
            MetaSpan::CircuitBreakerTrip,
            MetaSpan::SelfCalibration,
        ];
        for span in all {
            let ns = SpanNamespace::new(span.as_str()).unwrap();
            assert_eq!(
                ns.as_str(),
                span.as_str(),
                "MetaSpan::as_str() must match CANONICAL_NAMESPACES"
            );
        }
    }

    #[test]
    fn meta_span_roundtrip() {
        for span in [
            MetaSpan::DirectiveIssued,
            MetaSpan::EscalationOutcome,
            MetaSpan::CircuitBreakerTrip,
            MetaSpan::SelfCalibration,
        ] {
            let s = span.as_str();
            let back: MetaSpan = s.parse().expect("roundtrip");
            assert_eq!(span, back, "FromStr roundtrip for {s}");
        }
    }
}
