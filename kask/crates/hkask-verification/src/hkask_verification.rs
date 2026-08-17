#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Verification ladder for agent ecologies.
//!
//! Implements the four-rung verification ladder (Presence, Truth, Grounding,
//! Binding) from the ABW team's paper *"Verification for Agent Ecologies."*
//! This crate is the single source of truth for verification logic — any
//! MCP server that delegates to agents depends on it for grounding
//! enforcement and the central grounding ledger.
//!
//! ## The central grounding ledger
//!
//! The `VerificationStore` is the system-level capability: every MCP server
//! that delegates to agents calls `enforce_for_agent()` on each delegation,
//! which runs grounding (when a contract exists for the agent_type) and
//! writes a `GroundingRecord` to the central ledger. The ledger is
//! append-only, cross-tool, and cross-server — the curator, regulation
//! system, and gemba walk query it via `grounding_trend()`,
//! `grounding_violations()`, and `grounding_coverage()`.
//!
//! This closes the cybernetic feedback loop: enforcement → ledger →
//! curator → user → action → improved contracts → better enforcement.

pub mod card_contract;
pub mod envelope;
pub mod error;
pub mod grounding;
pub mod ledger;
pub mod rollup_trust;
pub mod schema_validate;
pub mod trend;
pub mod types;

// Re-export the primary API.
pub use error::VerificationError;
pub use grounding::{
    FieldSpec, GroundingContract, GroundingResult, LeakRule, ProvenanceTag, enforce_grounding,
    narrator_agent_contract, research_agent_contract, scan_narrative_for_leaks,
    task_agent_contract,
};
pub use ledger::{CoverageEntry, EnforcementOutcome, VerificationStore};
pub use trend::{GroundingTrendReport, TrendScope};
pub use types::GroundingRecord;
