//! Dual-axis ontology identity for the Blossom sidecar's tool artifacts (P5.4).

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::PROCEDURE_EXECUTION;

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// Every Blossom sidecar tool performs a read-only inspection.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        "nostr_blossom_status" | "nostr_blossom_list" | "nostr_blossom_log" => {
            Some(PROCEDURE_EXECUTION)
        }
        _ => None,
    }
}

/// The state-axis concept a Blossom sidecar tool's response occupies.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        "nostr_blossom_list" => Some(DATASET),
        "nostr_blossom_status" | "nostr_blossom_log" => Some(REPORT),
        _ => None,
    }
}

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &["nostr_blossom_"],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_discriminate_list_from_status_reports() {
        assert_ne!(
            tool_to_dc("nostr_blossom_list"),
            tool_to_dc("nostr_blossom_status")
        );
    }
}
