//! Curation — pure regulatory code (Loop 5)
//!
//! observe → evaluate → compose → regulate
//!
//! The Curation Loop is the ONLY loop that can override Cybernetics.
//! It observes system state and intervenes when Cybernetics
//! can't self-stabilize (e.g., alert cascade).
//!
//! # Curation / Agent Separation (Task 6)
//!
//! This module contains ONLY the pure regulatory code:
//! - `CurationLoop` — sense/compute/act, no persona, no chat, no memory
//! - `CuratorContext` — capability-disciplined runtime references
//! - `CurationConfidenceGate` — deleted (essentialist review: always constructed with
//!   empty ports, never produced a non-Suppress decision).
//!
//! Persona concerns (metacognition, bot orchestration, spec curation,
//! human-facing reporting) moved to `crate::curator_agent`.

pub mod context;
pub mod curation_loop;
pub mod semantic_index;
pub mod semantic_sync;

pub use context::CuratorContext;
pub use curation_loop::CurationLoop;
pub use semantic_index::SemanticIndex;
pub use semantic_sync::CuratorSync;
