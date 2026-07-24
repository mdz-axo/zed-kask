//! Proptest strategies for core hKask types.
//!
//! Free functions returning `BoxedStrategy<T>` for property-based testing.
//! The orphan rule prevents implementing `Arbitrary` for external types,
//! so we provide strategy constructors instead. Use with `proptest!`:
//!
//! ```ignore
//! proptest! {
//!     #[test]
//!     fn my_test(e in strategies::any_regulation_record()) { ... }
//! }
//! ```
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): strategies generate well-formed values
//! - P5 (Essentialism): one strategy function per type, no duplication
//!
//! # Goal Principle Anchoring (v0.30.0)
//! Each strategy function carries an `expect:` annotation (goal principle)
//! and optional `[P{N}] Constraining:` annotations, following the traceability chain:
//! `UserFunctionalExpectation → GoalPrinciple → ConstrainingPrinciple → Pre/Post`

use proptest::sample::select;
use proptest::strategy::{BoxedStrategy, Strategy};

use chrono::Utc;
use hkask_capability::{CapabilitySpec, DelegationAction, DelegationResource};
use hkask_goal::Goal;
use hkask_storage::HMem;
use hkask_types::GoalState;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::id::{GoalID, WebID};
use hkask_types::transcript::TranscriptSegment;
use hkask_types::visibility::Visibility;
use serde_json::Value;

// ── Regulation type strategies ─────────────────────────────────────────────────────────

fn non_empty_string() -> BoxedStrategy<String> {
    proptest::arbitrary::any::<String>()
        .prop_filter("must be non-empty", |s| !s.is_empty())
        .boxed()
}

fn webid_strategy() -> BoxedStrategy<WebID> {
    proptest::arbitrary::any::<[u8; 16]>()
        .prop_map(|bytes| WebID::from_persona(&bytes))
        .boxed()
}

fn span_strategy() -> BoxedStrategy<Span> {
    const NAMESPACES: &[&str] = &[
        "reg.tool",
        "reg.inference",
        "reg.pod",
        "reg.gas",
        "reg.curation",
        "reg.variety",
        "reg.sovereignty",
        "reg.spec",
        "reg.chat",
    ];
    const PATHS: &[&str] = &["invoked", "completed", "error", "sensed", "compared"];

    (select(NAMESPACES), select(PATHS))
        .prop_map(|(ns, path)| Span::new(SpanNamespace::new(ns).unwrap(), path))
        .boxed()
}

fn json_value_strategy() -> BoxedStrategy<Value> {
    non_empty_string().prop_map(Value::String).boxed()
}

// ── Public strategy functions ─────────────────────────────────────────────────

/// Strategy generating valid `RegulationRecord` instances.
///
/// Produces events with random observer WebIDs, canonical Regulation spans,
/// valid phases, string observations, and recursion depth 0–7.
///
/// post: returns `BoxedStrategy<RegulationRecord>` with valid observer, span, phase, observation, depth 0–7
/// expect: "I can generate valid regulation records with correct observer, canonical Regulation spans, and valid phases for property-based testing"
///Constraining: one strategy per type, no duplicate generators
pub fn any_regulation_record() -> BoxedStrategy<RegulationRecord> {
    (
        webid_strategy(),
        span_strategy(),
        select(&[
            CyclePhase::Sense,
            CyclePhase::Compute,
            CyclePhase::Compare,
            CyclePhase::Act,
        ]),
        json_value_strategy(),
        (0u8..7u8),
    )
        .prop_map(|(observer, span, phase, observation, depth)| {
            RegulationRecord::new(observer, span, phase, observation, depth)
        })
        .boxed()
}

/// Strategy generating valid `HMem` instances.
///
/// Produces h_mems with random entity/attribute strings,
/// string JSON values, and random owner WebIDs.
///
/// post: returns `BoxedStrategy<HMem>` with non-empty entity, attribute, value, owner
/// expect: "I can generate valid RDF h_mems with non-empty entities, attributes, and authenticated owners for property-based testing"
///Constraining: every h_mem carries an owner WebID — no anonymous agency
pub fn any_triple() -> BoxedStrategy<HMem> {
    (
        non_empty_string(),
        non_empty_string(),
        json_value_strategy(),
        webid_strategy(),
    )
        .prop_map(|(entity, attribute, value, owner)| HMem::new(&entity, &attribute, value, owner))
        .boxed()
}

/// Strategy generating valid `CapabilitySpec` instances.
///
/// Produces specs with random resource types, actions, and resource IDs.
///
/// post: returns Boxed`Strategy<CapabilitySpec>` with valid resource, action, resource_id
/// expect: "I can generate valid capability specifications with correct resource types and delegation actions for property-based testing"
///Constraining: capabilities encode explicit boundaries — no ambient authority
pub fn any_capability_spec() -> BoxedStrategy<CapabilitySpec> {
    (
        select(&[
            DelegationResource::Tool,
            DelegationResource::Template,
            DelegationResource::Registry,
            DelegationResource::Key,
        ]),
        select(&[
            DelegationAction::Read,
            DelegationAction::Write,
            DelegationAction::Execute,
        ]),
        non_empty_string(),
    )
        .prop_map(|(resource, action, resource_id)| CapabilitySpec {
            resource,
            resource_id,
            action,
        })
        .boxed()
}

/// Strategy generating valid `Goal` instances.
///
/// Produces goals with random text, states, visibility, depth 0–7,
/// and optional display names.
///
/// post: returns Boxed`Strategy<Goal>` with valid webid, text, state, visibility, depth 0–7
/// expect: "I can generate valid goals with correct WebID ownership, state, and visibility classification for property-based testing"
///Constraining: goals carry user-scoped WebIDs — sovereignty boundary respected
pub fn any_goal() -> BoxedStrategy<Goal> {
    (
        webid_strategy(),
        non_empty_string(),
        select(&[
            GoalState::Pending,
            GoalState::Active,
            GoalState::Completed,
            GoalState::Blocked,
            GoalState::Abandoned,
        ]),
        select(&[Visibility::Private, Visibility::Shared, Visibility::Public]),
        (0u8..7u8),
        proptest::option::of(proptest::arbitrary::any::<String>()),
    )
        .prop_map(
            |(webid, text, state, visibility, depth, display_name)| Goal {
                id: GoalID::new(),
                webid,
                text,
                state,
                visibility,
                created_at: Utc::now(),
                completed_at: None,
                parent_goal_id: None,
                depth,
                display_name,
            },
        )
        .boxed()
}

/// Strategy generating valid `TranscriptSegment` instances.
///
/// Produces segments with random text, start times 0–1hr,
/// and durations 100ms–30s.
///
/// post: returns Boxed`Strategy<TranscriptSegment>` with non-empty text, start_ms 0–1hr, duration 100ms–30s
/// expect: "I can generate valid transcript segments with temporal ordering invariants (end > start) for property-based testing"
pub fn any_transcript_segment() -> BoxedStrategy<TranscriptSegment> {
    (non_empty_string(), (0u64..3600000u64), (100u64..30000u64))
        .prop_map(|(text, start_ms, duration_ms)| TranscriptSegment {
            text,
            start_ms,
            end_ms: start_ms + duration_ms,
        })
        .boxed()
}

// ── Regulation proptest strategies ───────────────────────────────────────────────────

/// Strategy generating valid `GasCost` values (1..10000).
///
/// post: returns Boxed`Strategy<GasCost>` with values in 1..10000
/// expect: "I can generate valid energy costs within bounded ranges for gas-budget property-based testing"
pub fn any_energy_cost() -> BoxedStrategy<hkask_regulation::GasCost> {
    (1u64..10000u64).prop_map(hkask_regulation::GasCost).boxed()
}

/// Strategy generating valid `GasBudget` instances with hard limit.
///
/// post: returns Boxed`Strategy<GasBudget>` with cap 100..10000, replenish_rate 1..cap
/// expect: "I can generate valid energy budgets with caps that bound resource consumption for gas-guard property-based testing"
///Constraining: budget caps enforce OCAP boundaries on resource consumption
pub fn any_energy_budget() -> BoxedStrategy<hkask_regulation::GasBudget> {
    (100u64..10000u64)
        .prop_flat_map(|cap| {
            (1u64..cap).prop_map(move |rate| {
                hkask_regulation::GasBudget::new(hkask_regulation::GasCost(cap))
                    .with_replenish_rate(hkask_regulation::GasCost(rate))
            })
        })
        .boxed()
}

// ── Strategy self-tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn regulation_record_strategy_is_valid(e in any_regulation_record()) {
            prop_assert!(!e.id.as_uuid().is_nil());
            prop_assert!(!e.observer_webid.as_uuid().is_nil());
            prop_assert!(e.recursion_depth <= 7);
        }
    }

    proptest! {
        #[test]
        fn triple_strategy_is_valid(t in any_triple()) {
            prop_assert!(!t.id.as_uuid().is_nil());
            prop_assert!(!t.entity.is_empty());
            prop_assert!(!t.attribute.is_empty());
        }
    }

    proptest! {
        #[test]
        fn capability_spec_strategy_is_valid(spec in any_capability_spec()) {
            prop_assert!(!spec.resource_id.is_empty());
        }
    }

    proptest! {
        #[test]
        fn goal_strategy_is_valid(g in any_goal()) {
            prop_assert!(!g.id.as_uuid().is_nil());
            prop_assert!(!g.webid.as_uuid().is_nil());
            prop_assert!(!g.text.is_empty());
            prop_assert!(g.depth <= 7);
        }
    }

    proptest! {
        #[test]
        fn transcript_segment_strategy_is_valid(seg in any_transcript_segment()) {
            prop_assert!(!seg.text.is_empty());
            prop_assert!(seg.end_ms > seg.start_ms);
        }
    }
}
