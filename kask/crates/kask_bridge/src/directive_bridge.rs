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
        CuratorDirectiveRequest::EvolveMcpToolSchema {
            server_name,
            tool_name,
            evolution_type,
            field_name,
            new_type,
            rationale,
            evidence,
        } => CuratorDirective::EvolveMcpToolSchema {
            server_name,
            tool_name,
            evolution_type: convert_evolution_type(evolution_type),
            field_name,
            new_type,
            rationale,
            evidence,
        },
    }
}

fn convert_evolution_type(
    ty: agent::SchemaEvolutionType,
) -> hkask_types::curator::SchemaEvolutionType {
    match ty {
        agent::SchemaEvolutionType::AddField => hkask_types::curator::SchemaEvolutionType::AddField,
        agent::SchemaEvolutionType::RemoveField => {
            hkask_types::curator::SchemaEvolutionType::RemoveField
        }
        agent::SchemaEvolutionType::RenameField => {
            hkask_types::curator::SchemaEvolutionType::RenameField
        }
        agent::SchemaEvolutionType::ChangeType => {
            hkask_types::curator::SchemaEvolutionType::ChangeType
        }
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
            target: "reg.curator.directive",
            directive_type = %directive_type,
            "Curator directive queued for CyberneticsLoop processing"
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::CuratorDirectiveRequest;

    #[test]
    fn calibrate_threshold_round_trips() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();
        let sink = BridgeCuratorDirectiveSink::new(tx);
        let accepted = sink
            .send_directive(CuratorDirectiveRequest::CalibrateThreshold {
                domain: "inference".to_string(),
                new_threshold: 42,
            })
            .expect("send should succeed");
        assert!(accepted, "directive should be accepted");
        let directive = rx.try_recv().expect("directive should be in channel");
        match directive {
            CuratorDirective::CalibrateThreshold {
                domain,
                new_threshold,
            } => {
                assert_eq!(domain, "inference");
                assert_eq!(new_threshold, 42);
            }
            other => panic!("expected CalibrateThreshold, got {other:?}"),
        }
    }

    #[test]
    fn update_capabilities_converts_agent_name_to_webid() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();
        let sink = BridgeCuratorDirectiveSink::new(tx);
        sink.send_directive(CuratorDirectiveRequest::UpdateCapabilities {
            agent: "curator".to_string(),
            additions: vec!["regulation:read".to_string()],
            removals: vec![],
        })
        .unwrap();
        let directive = rx.try_recv().unwrap();
        match directive {
            CuratorDirective::UpdateCapabilities {
                agent,
                additions,
                removals,
            } => {
                assert_eq!(agent, WebID::for_agent_name("curator"));
                assert_eq!(additions, vec!["regulation:read"]);
                assert!(removals.is_empty());
            }
            other => panic!("expected UpdateCapabilities, got {other:?}"),
        }
    }

    #[test]
    fn escalate_domain_parses_severity_string() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();
        let sink = BridgeCuratorDirectiveSink::new(tx);
        sink.send_directive(CuratorDirectiveRequest::EscalateDomain {
            domain: "storage".to_string(),
            severity: "critical".to_string(),
            evidence: "db corruption".to_string(),
        })
        .unwrap();
        let directive = rx.try_recv().unwrap();
        match directive {
            CuratorDirective::EscalateDomain {
                domain,
                severity,
                evidence,
            } => {
                assert_eq!(domain, "storage");
                assert_eq!(severity, EscalationSeverity::Critical);
                assert_eq!(evidence, "db corruption");
            }
            other => panic!("expected EscalateDomain, got {other:?}"),
        }
    }

    #[test]
    fn unknown_severity_defaults_to_warning() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();
        let sink = BridgeCuratorDirectiveSink::new(tx);
        sink.send_directive(CuratorDirectiveRequest::EscalateDomain {
            domain: "test".to_string(),
            severity: "xyzzy".to_string(),
            evidence: "test".to_string(),
        })
        .unwrap();
        let directive = rx.try_recv().unwrap();
        match directive {
            CuratorDirective::EscalateDomain { severity, .. } => {
                assert_eq!(severity, EscalationSeverity::Warning);
            }
            other => panic!("expected EscalateDomain, got {other:?}"),
        }
    }

    #[test]
    fn closed_channel_returns_error() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();
        drop(rx);
        let sink = BridgeCuratorDirectiveSink::new(tx);
        let result = sink.send_directive(CuratorDirectiveRequest::ClearOverride {
            agent: "curator".to_string(),
        });
        assert!(result.is_err(), "send to closed channel should fail");
    }

    #[test]
    fn evolve_mcp_tool_schema_round_trips() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();
        let sink = BridgeCuratorDirectiveSink::new(tx);
        sink.send_directive(CuratorDirectiveRequest::EvolveMcpToolSchema {
            server_name: "hkask-mcp-companies".to_string(),
            tool_name: "dcf_valuation".to_string(),
            evolution_type: agent::SchemaEvolutionType::AddField,
            field_name: "wacc_override".to_string(),
            new_type: Some("Option<f64>".to_string()),
            rationale: "superforecasting step 12 needs to pass the forensic-adjusted WACC, but the tool schema doesn't have the field".to_string(),
            evidence: "skill_use_issue:superforecasting step 12 failed with 'missing field wacc_override'".to_string(),
        })
        .unwrap();
        let directive = rx.try_recv().unwrap();
        match directive {
            CuratorDirective::EvolveMcpToolSchema {
                server_name,
                tool_name,
                evolution_type,
                field_name,
                new_type,
                rationale,
                evidence,
            } => {
                assert_eq!(server_name, "hkask-mcp-companies");
                assert_eq!(tool_name, "dcf_valuation");
                assert_eq!(
                    evolution_type,
                    hkask_types::curator::SchemaEvolutionType::AddField
                );
                assert_eq!(field_name, "wacc_override");
                assert_eq!(new_type.as_deref(), Some("Option<f64>"));
                assert!(rationale.contains("forensic-adjusted WACC"));
                assert!(evidence.contains("skill_use_issue"));
            }
            other => panic!("expected EvolveMcpToolSchema, got {other:?}"),
        }
    }
}
