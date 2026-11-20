//! Dual-axis ontology identity for `hkask-mcp-wallet` tool artifacts (P5.4).

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// Process-axis concept per wallet tool (design doc §6.1).
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        "wallet_status" | "wallet_balance" | "wallet_transactions" | "wallet_decode_payment" => {
            Some(PROCEDURE_EXECUTION)
        }
        "wallet_reconcile"
        | "wallet_create_invoice"
        | "wallet_receive_ecash"
        | "wallet_pay_invoice"
        | "wallet_send_ecash"
        | "wallet_restore_from_seed" => Some(STEP_EXECUTION),
        _ => None,
    }
}

/// State-axis concept per wallet tool (design doc §6.1).
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        "wallet_status"
        | "wallet_balance"
        | "wallet_decode_payment"
        | "wallet_reconcile"
        | "wallet_restore_from_seed" => Some(REPORT),
        "wallet_transactions"
        | "wallet_create_invoice"
        | "wallet_receive_ecash"
        | "wallet_pay_invoice"
        | "wallet_send_ecash" => Some(DATASET),
        _ => None,
    }
}

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &["wallet_"],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_discriminate_read_reports_from_mutating_steps() {
        assert_ne!(
            tool_to_pko("wallet_status"),
            tool_to_pko("wallet_pay_invoice")
        );
        assert_ne!(
            tool_to_dc("wallet_balance"),
            tool_to_dc("wallet_transactions")
        );
    }
}
