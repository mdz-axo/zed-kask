//! Dual-axis ontology identity for the client's tool artifacts (P5.4).
//!
//! Every tool response carries two orthogonal identities: the process axis
//! (PKO: which occurrence the tool call is) and the state axis (DC+BIBO:
//! what kind of thing it returns).
//!
//! The vocabulary lives in the shared `hkask-bridge-ontology` crate; this
//! module holds only the client's tool-to-concept table. No ontology term is
//! defined here.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::PROCEDURE_EXECUTION;

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis concept a client tool's call occupies.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Every tool of this server runs a whole read and changes nothing.
        // A tool that publishes would be a step execution, as the relay's
        // sync is; none exists here.
        "nostr_account"
        | "nostr_decode"
        | "nostr_fetch_events"
        | "nostr_thread"
        | "nostr_profile"
        | "nostr_author_relays"
        | "nostr_notifications" => Some(PROCEDURE_EXECUTION),
        _ => None,
    }
}

/// The state-axis concept a client tool's response occupies.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Compiled statements: what is configured, what an identifier
        // names, what an author says about themselves and their relays.
        "nostr_account" | "nostr_decode" | "nostr_profile" | "nostr_author_relays" => Some(REPORT),
        // Events as the relays returned them are records.
        "nostr_fetch_events" | "nostr_thread" | "nostr_notifications" => Some(DATASET),
        _ => None,
    }
}

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &["nostr_"],
    &[],
);
