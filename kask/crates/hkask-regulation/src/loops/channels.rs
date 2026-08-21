//! Domain channel message types — direct typed channels for inter-loop communication.
//!
//! Each pathway gets its own typed `tokio::mpsc` channel. Channel identity replaces
//! both the former `LoopId` and `DispatchTarget` routing of the old Communication Loop.

use crate::algedonic::RuntimeAlert;

// ── Alerts channel: Cybernetics → Curation ──────────────────────────────────

// RuntimeAlert is the canonical type in crate::algedonic.
// Re-imported here so CurationInput::Alert(RuntimeAlert) compiles.

// ── Curation input enum — what CurationLoop reads from its inbox ─────────────

/// Cybernetics sends `Alert` through the `mpsc::Sender<CurationInput>` channel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CurationInput {
    /// Algedonic alert from Cybernetics (variety deficit escalation)
    Alert(RuntimeAlert),
}
