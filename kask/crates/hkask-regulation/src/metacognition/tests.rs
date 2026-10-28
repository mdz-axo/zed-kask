use super::*;

/// expect: "No samples stay unknown and accepted advice never becomes causally trusted" [P9]
#[test]
fn acceptance_is_not_causal_trust() {
    let empty = RegulationHealth::default();
    assert_eq!(empty.acceptance_rate(), None);
    let accepted = RegulationHealth {
        total_cycles: 10,
        accepted: 10,
        ..Default::default()
    };
    assert_eq!(accepted.acceptance_rate(), Some(1.0));
    for health in [empty, accepted] {
        assert_eq!(
            MetacognitionLoop::compute_loop_view(&health).outcome_trust,
            crate::loops::OutcomeTrust::Unverified
        );
    }
}

/// expect: "Operator acceptance uses the emitted boolean payload and excludes malformed observations."
#[test]
fn operator_acceptance_reads_the_writer_contract() {
    let spans = [
        StoredSkillSpan {
            skill_id: "tdd".to_string(),
            phase: "operator_feedback".to_string(),
            payload: OperatorFeedbackObservation::new(true, None).into_payload(),
        },
        StoredSkillSpan {
            skill_id: "tdd".to_string(),
            phase: "operator_feedback".to_string(),
            payload: OperatorFeedbackObservation::new(false, None).into_payload(),
        },
        StoredSkillSpan {
            skill_id: "tdd".to_string(),
            phase: "operator_feedback".to_string(),
            payload: serde_json::json!({"note": "missing disposition"}),
        },
    ];

    let observations = operator_acceptance_observations(&spans);
    assert_eq!(observations, vec![true, false]);
    assert_eq!(acceptance_rate(&observations), 0.5);
    assert!(operator_acceptance_observations(&spans[2..]).is_empty());
}

/// expect: "Malformed feedback cannot satisfy the drift loop's minimum-sample gate."
#[tokio::test]
async fn malformed_feedback_does_not_create_a_false_drift_window() {
    let ledger = Arc::new(TokioRwLock::new(RegulationLedger::default()));
    {
        let ledger = ledger.read().await;
        for index in 0..10 {
            let payload = match index {
                0 => OperatorFeedbackObservation::new(true, None).into_payload(),
                5 => OperatorFeedbackObservation::new(false, None).into_payload(),
                _ => serde_json::json!({"note": "missing accepted field"}),
            };
            ledger
                .record_skill_span("tdd", "operator_feedback", payload)
                .await;
        }
    }
    let loop_ = MetacognitionLoop::with_config(
        ledger,
        MetacognitionConfig {
            feedback_drift_min_samples: 10,
            feedback_drift_window: 5,
            ..Default::default()
        },
    );

    loop_.tick().await;

    assert_eq!(
        loop_
            .last_snapshot_blocking()
            .expect("tick stores a snapshot")
            .escalation_count,
        0,
        "two valid observations cannot become a ten-sample drift window"
    );
}

/// The health snapshot carries the native-first drift reading from the
/// ledger's rolling window — the reading the curator status surface
/// renders — and an empty window stays `None`, never a fabricated rate
/// (an unmeasured window read as perfect instrument compliance).
#[tokio::test]
async fn snapshot_carries_the_native_first_drift_reading() {
    let ledger = Arc::new(TokioRwLock::new(RegulationLedger::default()));
    {
        let ledger = ledger.read().await;
        ledger.record_agent_turn(true, true).await; // substantive, native-first
        ledger.record_agent_turn(true, false).await; // substantive, instrumented
        ledger.record_agent_turn(false, true).await; // non-substantive: not recorded
    }
    let loop_ = MetacognitionLoop::with_config(ledger, MetacognitionConfig::default());
    loop_.tick().await;
    let snapshot = loop_
        .last_snapshot_blocking()
        .expect("tick stores a snapshot");
    assert_eq!(snapshot.native_first_rate, Some((0.5, 2)));

    let empty = Arc::new(TokioRwLock::new(RegulationLedger::default()));
    let loop_ = MetacognitionLoop::with_config(empty, MetacognitionConfig::default());
    loop_.tick().await;
    assert_eq!(
        loop_
            .last_snapshot_blocking()
            .expect("tick stores a snapshot")
            .native_first_rate,
        None,
        "an empty window is no data, never a fabricated 0.0"
    );
}
