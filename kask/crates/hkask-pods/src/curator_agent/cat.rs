//! Communication Accommodation Theory (CAT) engagement logic.
//!
//! Pure functions for evaluating whether an agent should respond to
//! a Matrix communication event. The `convergence_bias` IS the decision.

use hkask_regulation::types::loops::CommunicationEvent;

/// Outcome of the engagement evaluation.
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// Agent should respond with this accommodation level (0.0–1.0).
    Speak { convergence_level: f64 },
    /// Agent should remain silent.
    Silent,
}

/// Evaluate whether an agent should engage with a communication event.
///
/// Convergence bias thresholds:
/// - > 0.0: speaks when addressed by name (mentions in body)
/// - ≥ 0.7: speaks to any message (convergent posture)
/// - = 0.0: always silent (divergent posture)
#[must_use]
pub fn evaluate(convergence_bias: f64, agent_name: &str, event: &CommunicationEvent) -> Decision {
    let addressed = body_mentions(event, agent_name);

    if convergence_bias >= 0.7 || (convergence_bias > 0.0 && addressed) {
        Decision::Speak {
            convergence_level: convergence_bias,
        }
    } else {
        Decision::Silent
    }
}

fn body_mentions(event: &CommunicationEvent, agent_name: &str) -> bool {
    event
        .observation
        .get("body")
        .and_then(|v| v.as_str())
        .map(|body| body.contains(agent_name))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(body: &str) -> CommunicationEvent {
        CommunicationEvent {
            span_category: "communication.message".into(),
            span_path: "observed".into(),
            observation: serde_json::json!({
                "room_id": "!test:localhost",
                "sender": "@alice:localhost",
                "body": body,
                "timestamp": 1700000000_i64,
            }),
            observed_at: "2024-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn low_bias_speaks_when_mentioned() {
        assert_eq!(
            evaluate(0.1, "curator", &event("Hey @curator, status?")),
            Decision::Speak {
                convergence_level: 0.1
            }
        );
    }

    #[test]
    fn low_bias_silent_when_not_mentioned() {
        assert_eq!(
            evaluate(0.1, "curator", &event("General discussion")),
            Decision::Silent
        );
    }

    #[test]
    fn moderate_bias_speaks_when_mentioned() {
        assert_eq!(
            evaluate(0.5, "helper", &event("@helper can you check?")),
            Decision::Speak {
                convergence_level: 0.5
            }
        );
    }

    #[test]
    fn moderate_bias_silent_when_not_mentioned() {
        assert_eq!(
            evaluate(0.5, "helper", &event("General discussion")),
            Decision::Silent
        );
    }

    #[test]
    fn high_bias_speaks_without_mention() {
        assert_eq!(
            evaluate(0.9, "helper", &event("General discussion")),
            Decision::Speak {
                convergence_level: 0.9
            }
        );
    }

    #[test]
    fn zero_bias_always_silent() {
        assert_eq!(
            evaluate(0.0, "agent", &event("@agent urgent!")),
            Decision::Silent
        );
    }
}
