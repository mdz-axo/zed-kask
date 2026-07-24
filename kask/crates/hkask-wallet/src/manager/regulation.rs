//! Regulation event emission — spans, algedonic alerts, chain error signals.

use super::*;

impl WalletManager {
    pub(super) fn default_actor() -> WebID {
        WebID::from_persona_with_namespace(b"wallet-manager", "wallet-surface")
    }

    pub(super) fn emit_span_with_actor(
        &self,
        actor: &WebID,
        span: WalletSpan,
        verb: &str,
        phase: CyclePhase,
        obs: serde_json::Value,
    ) {
        if let Some(ref sink) = self.event_sink {
            let span_obj = Span::new(
                SpanNamespace::from_observable(&span).expect("domain span must be canonical"),
                verb,
            );
            let event = RegulationRecord::new(*actor, span_obj, phase, obs, 0);
            if let Err(e) = sink.persist(&event) {
                tracing::warn!(target: "hkask.wallet", namespace = %span, verb = verb, error = %e, "Failed to persist Regulation span");
            }
        }
    }

    /// Emit a core Regulation span through the wallet's event sink.
    /// Use this for core RegulationSpan variants (Gas, SelfHeal) that are
    /// constructed within the wallet but are not wallet-specific.
    pub(super) fn emit_core_span(
        &self,
        span: hkask_types::regulation::RegulationSpan,
        verb: &str,
        phase: CyclePhase,
        obs: serde_json::Value,
    ) {
        if let Some(ref sink) = self.event_sink {
            let span_obj = Span::new(SpanNamespace::try_from(span).expect("canonical span"), verb);
            let event = RegulationRecord::new(Self::default_actor(), span_obj, phase, obs, 0);
            if let Err(e) = sink.persist(&event) {
                tracing::warn!(target: "hkask.wallet", namespace = %span, verb = verb, error = %e, "Failed to persist Regulation core span");
            }
        }
    }
    pub(super) fn emit_span(
        &self,
        span: WalletSpan,
        verb: &str,
        phase: CyclePhase,
        obs: serde_json::Value,
    ) {
        let actor = Self::default_actor();
        self.emit_span_with_actor(&actor, span, verb, phase, obs);
    }

    pub fn emit_key_alert(&self, key_id: ApiKeyId, exhausted: bool, expired: bool) {
        if expired {
            self.emit_span(
                WalletSpan::KeyExpired,
                "expired",
                CyclePhase::Sense,
                serde_json::json!({
                    "key_id": key_id.to_string(),
                }),
            );
        }
        if exhausted {
            self.emit_span(
                WalletSpan::KeyExhausted,
                "exhausted",
                CyclePhase::Sense,
                serde_json::json!({
                    "key_id": key_id.to_string(),
                }),
            );
        }
    }

    pub fn emit_chain_error_for_actor(
        &self,
        actor: &WebID,
        chain: ChainId,
        operation: &str,
        error_msg: &str,
    ) {
        self.emit_span_with_actor(
            actor,
            WalletSpan::ChainError,
            "error",
            CyclePhase::Sense,
            serde_json::json!({
                "actor": actor.to_string(),
                "chain": chain.to_string(),
                "operation": operation,
                "error": error_msg,
            }),
        );
    }

    pub fn emit_chain_error(&self, chain: ChainId, operation: &str, error_msg: &str) {
        let actor = Self::default_actor();
        self.emit_chain_error_for_actor(&actor, chain, operation, error_msg);
    }
}
