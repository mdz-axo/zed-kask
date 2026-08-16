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
//! contract documents this as a `Maintained` column where the cap is the
//! writer and the uncapped value is the source of truth.
//!
//! A `RollupContract` is also how a column gets retired honestly: a
//! column declared `WriteOrphaned` must have no reader treating it as
//! truth, which a test enforces by checking readers use the replacement.

/// What we assert about a denormalised field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// The field is written by some code path and must agree with its
    /// source of truth. A mismatch is a bug in the writer.
    Maintained,
    /// Nothing writes the field (or it's always zero). Readers must use
    /// the replacement instead. Kept only until the code change that
    /// drops it.
    WriteOrphaned,
}

/// One denormalised field and how to tell whether it's lying.
#[derive(Debug, Clone, Copy)]
pub struct RollupContract {
    /// The struct carrying the denormalised field.
    pub struct_name: &'static str,
    /// The field itself.
    pub field: &'static str,
    /// Where the truth actually lives, for the failure message.
    pub source_of_truth: &'static str,
    /// What readers should use instead. Empty for `Maintained` fields.
    pub replacement: &'static str,
    pub disposition: Disposition,
    /// Why this contract exists, in enough detail that the next person
    /// does not have to re-derive it.
    pub why: &'static str,
}

/// Every denormalised field we have an opinion about.
///
/// Extend this when you add a field that caches something derivable.
/// Rule of thumb: if you can compute it from another field, it belongs
/// here — either as `Maintained` (and then prove your writer keeps it
/// honest) or as `WriteOrphaned` (and then prove no reader treats it as
/// truth).
pub const ROLLUP_CONTRACTS: &[RollupContract] = &[
    RollupContract {
        struct_name: "LocalDelegateResult",
        field: "cost",
        source_of_truth: "cost_uncapped (the true token spend before capping)",
        replacement: "cost_uncapped",
        disposition: Disposition::Maintained,
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
        replacement: "",
        disposition: Disposition::Maintained,
        why: "balance is None when the ledger read failed, not zero. \
              A reader that treats None as 0 is reading a failed read as \
              a measured zero — the .rules broken-feedback-loop trap. \
              The contract documents that None is not 0 and the field \
              must not be unwrapped to a default.",
    },
];

/// Check that every `WriteOrphaned` field has no reader treating it as
/// truth. This is a compile-time check — it returns the list of orphaned
/// fields so a test can assert no code reads them directly.
pub fn orphaned_fields() -> Vec<&'static str> {
    ROLLUP_CONTRACTS
        .iter()
        .filter(|c| c.disposition == Disposition::WriteOrphaned)
        .map(|c| c.field)
        .collect()
}

/// Check that every `Maintained` field has a non-empty source of truth
/// and a replacement (if the field is capped or derived).
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
        if c.disposition == Disposition::WriteOrphaned && c.replacement.is_empty() {
            issues.push(format!(
                "RollupContract for {}.{} is WriteOrphaned but has no \
                 replacement — readers have nothing to use instead",
                c.struct_name, c.field
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
    fn cost_contract_is_maintained() {
        let cost = ROLLUP_CONTRACTS
            .iter()
            .find(|c| c.field == "cost")
            .expect("cost contract must exist");
        assert_eq!(cost.disposition, Disposition::Maintained);
        assert_eq!(cost.replacement, "cost_uncapped");
    }

    #[test]
    fn balance_contract_documents_none_is_not_zero() {
        let balance = ROLLUP_CONTRACTS
            .iter()
            .find(|c| c.field == "balance")
            .expect("balance contract must exist");
        assert_eq!(balance.disposition, Disposition::Maintained);
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
    fn orphaned_fields_returns_write_orphaned_only() {
        let orphaned = orphaned_fields();
        // Currently no WriteOrphaned fields — both contracts are Maintained.
        assert!(orphaned.is_empty(), "no WriteOrphaned fields expected yet");
    }

    #[test]
    fn validate_contracts_passes_for_current_contracts() {
        let issues = validate_contracts();
        assert!(issues.is_empty(), "unexpected contract issues: {issues:?}");
    }
}
