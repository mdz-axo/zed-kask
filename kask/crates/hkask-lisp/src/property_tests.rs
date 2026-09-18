//! Property layer for the sandboxed evaluator
//! (`kask/docs/reference/testing-protocol.md`). Batch 6 of the propagation
//! plan (`tasks/kask-testing-propagation-plan.md`): the budget-contract pins
//! exist (`infinite_loop_hits_step_budget`,
//! `deep_recursion_hits_depth_budget_before_stack_overflow`); the properties
//! generalize evaluator correctness and totality over generated programs.
//! A shrunk counterexample is a finding to report, never a signal to weaken
//! a property.

use super::*;
use proptest::prelude::*;

proptest! {
    /// Hypothesis: the evaluator computes generated integer additions
    /// exactly — arithmetic over generated operands returns the correct
    /// integer result, not merely a result.
    #[test]
    fn evaluator_sums_generated_additions_exactly(
        a in 0u64..=1_000_000,
        b in 0u64..=1_000_000,
    ) {
        let form = format!("(+ {a} {b})");
        let result =
            eval_sandboxed(&form, &serde_json::json!({})).expect("well-formed addition evaluates");
        prop_assert_eq!(result, serde_json::json!(a + b));
    }

    /// Hypothesis: evaluation is total — arbitrary source text either
    /// evaluates or returns a typed error. The step and depth budgets bound
    /// execution, so no input can hang or crash the host.
    #[test]
    fn evaluation_is_total_over_arbitrary_source(source in ".*") {
        let _ = eval_sandboxed(&source, &serde_json::json!({}));
    }
}
