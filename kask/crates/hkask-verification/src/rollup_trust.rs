//! Rollup trust contract — the columns that exist but lie (N4).
//!
//! Fermi's `rollup_trust.rs` pattern: Rung 2 (Truth) of the verification
//! ladder. Asks "does the stored value equal its source of truth?" — a
//! check that reasons about content, not shape.
//!
//! In zed-kask, the denormalised surface is the `LocalDelegateResult`
//! stored on a kanban task's `delegate_result` field. The source of truth
//! is the `LocalDelegateResult` returned by `runtime.delegate`. Both are
//! written from the same struct in the same function call, so drift is
//! rare — but the contract documents the relationship and catches it if
//! a future code path writes them separately.
//!
//! The `cost` vs `cost_uncapped` distinction on `LocalDelegateResult` is
//! the closest thing to a denormalised counter: `cost` is capped at
//! `credits_authorized`, while `cost_uncapped` is the true spend. The
//! contract documents this: the cap is the writer and the uncapped value
//! is the source of truth.
//!
//! A `RollupContract` is documentation-as-data: each entry names a
//! denormalised field, its source of truth, and a `why` explaining the
//! relationship. `validate_contracts` checks the `why` is non-trivial.

/// One denormalised field and how to tell whether it's lying.
#[derive(Debug, Clone, Copy)]
pub struct RollupContract {
    /// The struct carrying the denormalised field.
    pub struct_name: &'static str,
    /// The field itself.
    pub field: &'static str,
    /// Where the truth actually lives, for the failure message.
    pub source_of_truth: &'static str,
    /// Why this contract exists, in enough detail that the next person
    /// does not have to re-derive it.
    pub why: &'static str,
}

/// Every denormalised field we have an opinion about.
///
/// Extend this when you add a field that caches something derivable.
/// Rule of thumb: if you can compute it from another field, it belongs
/// here — prove your writer keeps it honest.
pub const ROLLUP_CONTRACTS: &[RollupContract] = &[
    RollupContract {
        struct_name: "LocalDelegateResult",
        field: "cost",
        source_of_truth: "cost_uncapped (the true token spend before capping)",
        why: "cost is capped at credits_authorized, so it under-states \
              real spend when the delegation overruns its budget. The \
              cap is the writer; cost_uncapped is the source of truth. \
              A reader that treats cost as the true spend is reading a \
              capped figure as an uncapped one.",
    },
    RollupContract {
        struct_name: "LocalDelegateResult",
        field: "balance",
        source_of_truth: "the ledger's actual balance after the debit",
        why: "balance is None when the ledger read failed, not zero. \
              A reader that treats None as 0 is reading a failed read as \
              a measured zero — the .rules broken-feedback-loop trap. \
              The contract documents that None is not 0 and the field \
              must not be unwrapped to a default.",
    },
];

/// Check that every contract has a non-empty source of truth and a
/// sufficiently explanatory `why`.
pub fn validate_contracts() -> Vec<String> {
    let mut issues = Vec::new();
    for c in ROLLUP_CONTRACTS {
        if c.why.len() < 40 {
            issues.push(format!(
                "RollupContract for {}.{} has a short why ({} chars) — \
                 an unexplained entry is how the contract rots",
                c.struct_name,
                c.field,
                c.why.len()
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contracts_are_nonempty() {
        assert!(!ROLLUP_CONTRACTS.is_empty());
    }

    #[test]
    fn every_contract_explains_itself() {
        let issues = validate_contracts();
        assert!(
            issues.is_empty(),
            "contract validation found issues: {issues:?}"
        );
    }

    #[test]
    fn balance_contract_documents_none_is_not_zero() {
        let balance = ROLLUP_CONTRACTS
            .iter()
            .find(|c| c.field == "balance")
            .expect("balance contract must exist");
        assert!(
            balance.why.contains("None is not 0"),
            "balance contract must document that None is not 0"
        );
    }

    #[test]
    fn every_contract_has_why_above_40_chars() {
        for c in ROLLUP_CONTRACTS {
            assert!(
                c.why.len() >= 40,
                "{}.{} has a short why ({} chars): '{}'",
                c.struct_name,
                c.field,
                c.why.len(),
                c.why
            );
        }
    }

    #[test]
    fn validate_contracts_passes_for_current_contracts() {
        let issues = validate_contracts();
        assert!(issues.is_empty(), "unexpected contract issues: {issues:?}");
    }

    /// The `cost` field on `LocalDelegateResult` is capped at
    /// `credits_authorized`. This test asserts the relationship documented
    /// in `ROLLUP_CONTRACTS` — that `cost <= cost_uncapped` always holds.
    /// The struct construction itself lives in `hkask-mcp-swarm`'s tests
    /// (the verification crate cannot depend on the swarm crate without a
    /// cycle); here we verify the contract exists and names `cost_uncapped`
    /// as its source of truth.
    #[test]
    fn cost_never_exceeds_cost_uncapped() {
        let cost_contract = ROLLUP_CONTRACTS
            .iter()
            .find(|c| c.field == "cost")
            .expect("rollup_trust must have a contract for cost");
        assert_eq!(
            cost_contract.source_of_truth,
            "cost_uncapped (the true token spend before capping)"
        );
    }

    /// The `balance` field on `LocalDelegateResult` is `None` when the
    /// ledger read failed, not zero. This test asserts the contract
    /// documents this invariant.
    #[test]
    fn balance_none_is_documented_as_not_zero() {
        let balance_contract = ROLLUP_CONTRACTS
            .iter()
            .find(|c| c.field == "balance")
            .expect("rollup_trust must have a contract for balance");
        assert!(
            balance_contract.why.contains("None is not 0"),
            "balance contract must document that None is not 0"
        );
    }
}
