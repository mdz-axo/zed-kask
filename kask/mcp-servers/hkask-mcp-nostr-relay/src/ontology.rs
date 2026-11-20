//! Dual-axis ontology identity for the relay's tool artifacts (P5.4).
//!
//! Every tool response carries two orthogonal identities: the process axis
//! (PKO: which occurrence the tool call is) and the state axis (DC+BIBO:
//! what kind of thing it returns).
//!
//! The vocabulary lives in the shared `hkask-bridge-ontology` crate; this
//! module holds only the relay's tool-to-concept table. No ontology term is
//! defined here.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis concept a relay tool's call occupies.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Reading the relay's state, the archive, or the admission log runs
        // a whole read and changes nothing.
        "nostr_relay_status" | "nostr_relay_query" | "nostr_relay_log" => Some(PROCEDURE_EXECUTION),
        // A sync is one step that changes the archive.
        "nostr_relay_sync" => Some(STEP_EXECUTION),
        _ => None,
    }
}

/// The state-axis concept a relay tool's response occupies.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Stored events and the outcome of a sync are records.
        "nostr_relay_query" | "nostr_relay_sync" => Some(DATASET),
        // Status and the admission log are compiled readings.
        "nostr_relay_status" | "nostr_relay_log" => Some(REPORT),
        _ => None,
    }
}

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &["nostr_relay_"],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Each axis must tell tools apart. A table with one value per axis
    /// would pass the coverage test and say nothing.
    #[test]
    fn axes_discriminate_reads_from_the_write() {
        assert_ne!(
            tool_to_pko("nostr_relay_query"),
            tool_to_pko("nostr_relay_sync")
        );
        assert_ne!(
            tool_to_dc("nostr_relay_query"),
            tool_to_dc("nostr_relay_status")
        );
    }

    #[test]
    fn annotate_adds_both_axes_and_keeps_the_body() {
        let annotated = annotate("nostr_relay_sync", serde_json::json!({"stored": 3}));
        assert_eq!(annotated["stored"], 3);
        assert_eq!(annotated["ontology"]["pko_procedure"], STEP_EXECUTION);
        assert_eq!(annotated["ontology"]["dc_type"], DATASET);
    }
}
