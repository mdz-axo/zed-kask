//! Contract lifecycle Regulation event emitters.
//!
//! Emits Regulation spans for spec contract proposals, acceptances, rejections,
//! and violations. These events flow to the RegulationSink → CurationLoop.

use crate::contract_span::ContractSpan;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use tracing;

/// Emit a Regulation event when a contract is proposed by a userpod (Phase B2–B4).
pub fn emit_contract_proposed(
    sink: &dyn RegulationSink,
    userpod: &str,
    crate_name: &str,
    function: &str,
    contract_id: &str,
) {
    let namespace = SpanNamespace::from_observable(&ContractSpan::ContractProposed)
        .expect("domain span must be canonical");
    let span = Span::new(namespace, "proposed");
    let observation = serde_json::json!({
        "userpod": userpod,
        "crate_name": crate_name,
        "function": function,
        "contract_id": contract_id,
    });
    let event = RegulationRecord::new(WebID::default(), span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.contract", error = %e, "Failed to persist contract_proposed event");
    }
}

/// Emit a Regulation event when a contract proposal is accepted by a human (Phase B3).
pub fn emit_contract_accepted(
    sink: &dyn RegulationSink,
    reviewer: &str,
    _crate_name: &str,
    _function: &str,
    _resource_id: &str,
    contract_id: &str,
) {
    let namespace = SpanNamespace::from_observable(&ContractSpan::ContractAccepted)
        .expect("domain span must be canonical");
    let span = Span::new(namespace, "accepted");
    let observation = serde_json::json!({
        "reviewer": reviewer,
        "contract_id": contract_id,
    });
    let event = RegulationRecord::new(WebID::default(), span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.contract", error = %e, "Failed to persist contract_accepted event");
    }
}

/// Emit a Regulation event when a contract proposal is rejected by a human (Phase B3).
pub fn emit_contract_rejected(
    sink: &dyn RegulationSink,
    reviewer: &str,
    _crate_name: &str,
    _function: &str,
    _resource_id: &str,
    contract_id: &str,
    reason: &str,
) {
    let namespace = SpanNamespace::from_observable(&ContractSpan::ContractRejected)
        .expect("domain span must be canonical");
    let span = Span::new(namespace, "rejected");
    let observation = serde_json::json!({
        "reviewer": reviewer,
        "contract_id": contract_id,
        "reason": reason,
    });
    let event = RegulationRecord::new(WebID::default(), span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.contract", error = %e, "Failed to persist contract_rejected event");
    }
}

/// Emit a Regulation event when a contract violation is detected during testing.
pub fn emit_contract_violated(
    sink: &dyn RegulationSink,
    test_name: &str,
    contract_id: &str,
    failure_reason: &str,
) {
    let namespace = SpanNamespace::from_observable(&ContractSpan::ContractViolated)
        .expect("domain span must be canonical");
    let span = Span::new(namespace, "violated");
    let observation = serde_json::json!({
        "test_name": test_name,
        "contract_id": contract_id,
        "failure_reason": failure_reason,
    });
    let event = RegulationRecord::new(WebID::default(), span, CyclePhase::Act, observation, 0);
    if let Err(e) = sink.persist(&event) {
        tracing::warn!(target: "hkask.contract", error = %e, "Failed to persist contract_violated event");
    }
}
