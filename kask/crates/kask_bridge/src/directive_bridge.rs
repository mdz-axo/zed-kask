//! Bridge between the agent's `CuratorDirectiveTool` and the
//! `CyberneticsLoop`'s directive channel.
//!
//! `BridgeCuratorDirectiveSink` converts the tool-local
//! `CuratorDirectiveRequest` (agent-name strings) into
//! `hkask_types::CuratorDirective` (WebIDs) and sends it via the tokio
//! unbounded channel that `CyberneticsLoop::process_inbox` drains.

use agent::CuratorDirectiveRequest;
use agent::CuratorDirectiveSink;
use hkask_types::WebID;
use hkask_types::curator::{CuratorDirective, EscalationSeverity};
use tokio::sync::mpsc::UnboundedSender;

/// Bridge: `CuratorDirectiveTool` → `CyberneticsLoop` directive channel.
///
/// The sender is an `UnboundedSender` so `send` is synchronous (no
/// backpressure). Dampening is checked asynchronously in
/// `CyberneticsLoop::process_inbox`, not at send time — `send_directive`
/// returns `Ok(true)` when the directive is queued, and the Curator can
/// verify the outcome via `curator_status` on the next tick.
pub struct BridgeCuratorDirectiveSink {
    tx: UnboundedSender<CuratorDirective>,
}

impl BridgeCuratorDirectiveSink {
    pub fn new(tx: UnboundedSender<CuratorDirective>) -> Self {
        Self { tx }
    }
}

fn parse_severity(s: &str) -> EscalationSeverity {
    match s.to_lowercase().as_str() {
        "info" => EscalationSeverity::Info,
        "critical" => EscalationSeverity::Critical,
        _ => EscalationSeverity::Warning,
    }
}

fn convert(req: CuratorDirectiveRequest) -> CuratorDirective {
    match req {
        CuratorDirectiveRequest::CalibrateThreshold {
            domain,
            new_threshold,
        } => CuratorDirective::CalibrateThreshold {
            domain,
            new_threshold,
        },
        CuratorDirectiveRequest::UpdateCapabilities {
            agent,
            additions,
            removals,
        } => CuratorDirective::UpdateCapabilities {
            agent: WebID::for_agent_name(&agent),
            additions,
            removals,
        },
        CuratorDirectiveRequest::OverrideEnergyBudget { agent, new_budget } => {
            CuratorDirective::OverrideEnergyBudget {
                agent: WebID::for_agent_name(&agent),
                new_budget,
            }
        }
        CuratorDirectiveRequest::SeekMoreEvidence {
            context,
            channel,
            confidence,
        } => CuratorDirective::SeekMoreEvidence {
            context,
            channel,
            confidence,
        },
        CuratorDirectiveRequest::ReplenishBudget {
            agent,
            amount,
            priority,
        } => CuratorDirective::ReplenishBudget {
            agent: WebID::for_agent_name(&agent),
            amount,
            priority,
        },
        CuratorDirectiveRequest::ClearOverride { agent } => CuratorDirective::ClearOverride {
            agent: WebID::for_agent_name(&agent),
        },
        CuratorDirectiveRequest::EscalateDomain {
            domain,
            severity,
            evidence,
        } => CuratorDirective::EscalateDomain {
            domain,
            severity: parse_severity(&severity),
            evidence,
        },
    }
}

impl CuratorDirectiveSink for BridgeCuratorDirectiveSink {
    fn send_directive(&self, directive: CuratorDirectiveRequest) -> Result<bool, String> {
        let directive_type = directive.variant_name().to_string();
        let hkask_directive = convert(directive);
        self.tx
            .send(hkask_directive)
            .map_err(|e| format!("regulation loop channel closed: {:?}", e.0))?;
        tracing::info!(
            target: "reg.curator_directive",
            directive_type = %directive_type,
            "Curator directive queued for CyberneticsLoop processing"
        );
        Ok(true)
    }
}
