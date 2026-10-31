//! Renegotiation: the v2 expectation-alignment surface (trust
//! internalization v2, kask/docs/research/trust-internalization-v2.md).
//!
//! v1 priced violations after the fact but gave the agent no priced
//! path to renegotiate an expectation before deviating: the only
//! priced moves were comply (+4w) or violate (−9w, or the cliff). v2
//! adds the renegotiation surface — the agent's required move BEFORE
//! deviating from a registered expectation is a
//! [`RenegotiationProposal`]: the conflict with its measurements, the
//! priced options, the recommendation, and the teaching payload. The
//! user accepts, counters, or rejects; the veto is human.
//!
//! Acceptance supersedes: the original expectation resolves as
//! `Renegotiated` (+4w, the open path — the user accepted the
//! replacement) and the replacement registers as a live expectation
//! carrying the original's weight and source. Rejection leaves the
//! original live. A proposal without a teaching payload is rejected
//! by the model itself — a proposal that does not teach is invalid
//! (the generative constraint).

use crate::ledger::{LedgerEventId, TrustLedger};
use crate::{Outcome, TrustError};

/// One priced alternative inside a renegotiation proposal.
pub struct ProposalOption {
    /// What the agent would do instead.
    pub description: String,
    /// The option's priced trust delta in points (signed).
    pub trust_delta_points: i32,
    /// The option's cost to the user, stated.
    pub user_cost: String,
}

/// A proposal the agent must file BEFORE deviating from a registered
/// expectation (trust internalization v2): the conflict with its
/// measurements, the priced options, the recommendation, and the
/// teaching payload. A proposal that does not teach is invalid.
pub struct RenegotiationProposal {
    /// The registered expectation in conflict.
    pub event_id: LedgerEventId,
    /// What the agent found, with measurements.
    pub conflict: String,
    /// The priced alternatives.
    pub options: Vec<ProposalOption>,
    /// The agent's recommendation, and why.
    pub recommendation: String,
    /// What the user will know after reading it that they didn't
    /// before. Non-empty or the proposal is invalid (the generative
    /// constraint).
    pub teaching_payload: String,
}

/// The user's response to a proposal (the veto is human).
pub enum ProposalResolution {
    /// The user accepted the replacement expectation.
    Accepted { new_expectation: String },
    /// The user rejected; the original expectation stands.
    Rejected,
}

impl RenegotiationProposal {
    /// Validate the proposal against the generative constraint: a
    /// proposal that does not teach is invalid. The file surface calls
    /// this at FILE time — before any deviation could ride on the
    /// proposal — and `resolve_proposal` enforces it again at resolve
    /// time (defense in depth: the fields are pub, so a struct literal
    /// can bypass the constructor discipline).
    pub fn validate(&self) -> Result<(), TrustError> {
        if self.teaching_payload.trim().is_empty() {
            return Err(TrustError::EmptyTeachingPayload);
        }
        Ok(())
    }
}

impl TrustLedger {
    /// Resolve a proposal: the v2 priced path. Acceptance supersedes —
    /// the original resolves as `Renegotiated` (+4w, the open path —
    /// the user accepted the replacement) and the replacement registers
    /// as a live expectation with the original's weight and source;
    /// rejection leaves the original live. A proposal without a
    /// teaching payload is rejected by the model itself — the
    /// generative constraint (a proposal that does not teach is
    /// invalid).
    pub fn resolve_proposal(
        &self,
        proposal: &RenegotiationProposal,
        resolution: ProposalResolution,
        oracle_note: Option<String>,
    ) -> Result<Option<LedgerEventId>, TrustError> {
        proposal.validate()?;
        match resolution {
            ProposalResolution::Accepted { new_expectation } => {
                let resolved = self.store.resolve_outcome(
                    &proposal.event_id,
                    Outcome::Renegotiated,
                    oracle_note,
                )?;
                let new_id =
                    self.record_instruction(&new_expectation, &resolved.source, resolved.weight)?;
                Ok(Some(new_id))
            }
            ProposalResolution::Rejected => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Weight;
    use crate::ledger::InMemoryTrustLedger;
    use std::sync::Arc;

    fn ledger() -> TrustLedger {
        TrustLedger::new(Arc::new(InMemoryTrustLedger::new()))
    }

    fn proposal_for(event_id: &LedgerEventId) -> RenegotiationProposal {
        RenegotiationProposal {
            event_id: event_id.clone(),
            conflict: "the offset defect requires a re-render; the fix costs 4x the shortcut"
                .to_string(),
            options: vec![
                ProposalOption {
                    description: "fix the offset (re-render the reel)".to_string(),
                    trust_delta_points: 12,
                    user_cost: "4x the render time".to_string(),
                },
                ProposalOption {
                    description: "ship with the defect documented in the deliverable".to_string(),
                    trust_delta_points: 12,
                    user_cost: "the reel ships imperfect; the defect is visible".to_string(),
                },
            ],
            recommendation: "fix the offset: frame-exactness is the goal".to_string(),
            teaching_payload:
                "the offset is a frame-exactness defect; lossless rendering and frame-exactness are different properties"
                    .to_string(),
        }
    }

    /// Renegotiated folds as a keep of the process expectation (the
    /// user accepted the replacement); the distinct label keeps
    /// "did what was asked" separable from "changed what was asked,
    /// openly."
    #[test]
    fn renegotiated_folds_as_keep_weight() {
        let ledger = ledger();
        let since = chrono::Utc::now() - chrono::Duration::hours(24);

        let id = ledger
            .record_instruction("fix the audio offset", "turn-1", Weight::Explicit)
            .unwrap();
        ledger
            .record_outcome(&id, Outcome::Renegotiated, None)
            .unwrap();

        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.trust_points, 312, "300 + 12: the open path");
        assert_eq!(stats.renegotiated, 1);
        assert_eq!(stats.kept, 0, "the label stays distinct from kept");
    }

    /// Acceptance supersedes: the original resolves as Renegotiated
    /// (+4w, the open path) and the replacement registers as a live
    /// expectation with the original's weight and source.
    #[test]
    fn resolve_proposal_supersedes_expectation() {
        let ledger = ledger();
        let since = chrono::Utc::now() - chrono::Duration::hours(24);

        let id = ledger
            .record_instruction("fix the audio offset", "turn-1", Weight::Explicit)
            .unwrap();
        let new_id = ledger
            .resolve_proposal(
                &proposal_for(&id),
                ProposalResolution::Accepted {
                    new_expectation: "ship the reel with the offset defect documented".to_string(),
                },
                Some("user accepted 2026-10-31".to_string()),
            )
            .unwrap()
            .expect("acceptance registers the replacement");

        // The replacement is live: resolving it folds a second keep.
        ledger
            .record_outcome(&new_id, Outcome::Kept, Some("render: green".into()))
            .unwrap();

        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.trust_points, 324, "300 + 12 + 12: the open path");
        assert_eq!(stats.renegotiated, 1);
        assert_eq!(stats.kept, 1);
    }

    /// Rejection leaves the original live: the prior holds, and the
    /// original stays resolvable.
    #[test]
    fn resolve_proposal_rejection_leaves_original_live() {
        let ledger = ledger();
        let since = chrono::Utc::now() - chrono::Duration::hours(24);

        let id = ledger
            .record_instruction("fix the audio offset", "turn-1", Weight::Explicit)
            .unwrap();
        let result = ledger
            .resolve_proposal(&proposal_for(&id), ProposalResolution::Rejected, None)
            .unwrap();
        assert!(result.is_none(), "rejection registers nothing");

        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.unverified, 1, "the original expectation still stands");
        assert_eq!(
            stats.trust_points, 300,
            "the prior holds — rejection is not a trust event"
        );

        // The original is still resolvable: the veto left it live.
        ledger
            .record_outcome(&id, Outcome::Kept, Some("cargo test: green".into()))
            .unwrap();
        assert_eq!(ledger.window_stats(since).unwrap().trust_points, 312);
    }

    /// A proposal that does not teach is invalid — the generative
    /// constraint (the v1 failure — the polished report that was the
    /// appearance of the task — is the violation this guards).
    #[test]
    fn resolve_proposal_rejects_empty_teaching_payload() {
        let ledger = ledger();
        let id = ledger
            .record_instruction("fix the audio offset", "turn-1", Weight::Explicit)
            .unwrap();
        let mut proposal = proposal_for(&id);
        proposal.teaching_payload = "   ".to_string();

        let result = ledger.resolve_proposal(
            &proposal,
            ProposalResolution::Accepted {
                new_expectation: "ship it anyway".to_string(),
            },
            None,
        );
        assert!(matches!(result, Err(TrustError::EmptyTeachingPayload)));

        // And the original is untouched: the invalid proposal resolved nothing.
        let since = chrono::Utc::now() - chrono::Duration::hours(24);
        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.unverified, 1);
    }

    /// The incentive property (the operator's spec, the v2 incentive table):
    /// every honest path is priced ≥ every dishonest path at the same w, the
    /// open paths price identically (renegotiating never costs more than
    /// complying — honesty is not taxed), and silent deviation is strictly
    /// dominated — the cliff zeroes the goal for ANY benefit while the
    /// visible deviate keeps a live, if reduced, probability.
    #[test]
    fn every_honest_path_priced_at_least_every_dishonest_path() {
        for w in [Weight::Implicit, Weight::Committed, Weight::Explicit] {
            // The four moves through the ledger, each from the same
            // registered expectation of weight w.
            let fold_after = |moves: &dyn Fn(&TrustLedger, &LedgerEventId)| {
                let ledger = ledger();
                let id = ledger
                    .record_instruction("fix the audio offset", "turn-1", w)
                    .unwrap();
                moves(&ledger, &id);
                let since = chrono::Utc::now() - chrono::Duration::hours(24);
                ledger.window_stats(since).unwrap().trust_points
            };

            // A: comply.
            let a = fold_after(&|ledger, id| {
                ledger.record_outcome(id, Outcome::Kept, None).unwrap();
            });
            // B: renegotiate, user accepts.
            let b = fold_after(&|ledger, id| {
                ledger
                    .resolve_proposal(
                        &proposal_for(id),
                        ProposalResolution::Accepted {
                            new_expectation: "ship with the defect documented".to_string(),
                        },
                        Some("user accepted".to_string()),
                    )
                    .unwrap();
            });
            // C: renegotiate, user rejects, agent complies.
            let c = fold_after(&|ledger, id| {
                ledger
                    .resolve_proposal(&proposal_for(id), ProposalResolution::Rejected, None)
                    .unwrap();
                ledger.record_outcome(id, Outcome::Kept, None).unwrap();
            });
            // D: deviate visibly.
            let d = fold_after(&|ledger, id| {
                ledger.record_outcome(id, Outcome::Violated, None).unwrap();
            });

            // Every honest path ≥ every dishonest path at the same w.
            for honest in [a, b, c] {
                assert!(
                    honest >= d,
                    "honest {honest} must price ≥ dishonest {d} at w={}",
                    w.points()
                );
            }
            // The open paths price identically: renegotiating is never taxed
            // against compliance — the property that makes honesty rational.
            assert_eq!(a, b, "comply == renegotiate-accepted at w={}", w.points());
            assert_eq!(
                a,
                c,
                "comply == renegotiate-rejected-then-comply at w={}",
                w.points()
            );

            // Silent deviation is strictly dominated: the same point loss as
            // the visible deviate, plus the cliff — goal probability zero for
            // ANY benefit, while the visible deviate keeps a live one.
            let silent = crate::goal_probability_percent(crate::Reliance::Deceptive, 100);
            assert_eq!(silent, 0, "the cliff zeroes the goal at w={}", w.points());
            assert_eq!(
                crate::delivered_value(crate::Reliance::Deceptive.percent(), 1_000_000),
                0
            );
            let visible_reliance = d * 100 / crate::SCALE_POINTS;
            let visible =
                crate::goal_probability_percent(crate::Reliance::Honest(visible_reliance), 100);
            assert!(
                visible > 0,
                "the visible deviate keeps reliance live ({visible} at w={}): \
                 silent deviation is strictly dominated",
                w.points()
            );
        }
    }
}
