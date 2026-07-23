#![deny(unsafe_code)]
//! hKask Agents — Agent Pod Lifecycle and A2A Integration
//!
//! This crate provides:
//! - **Agent Pod**: Runtime container for userpods and the curator
//! - **Lifecycle Management**: Active ↔ Sleeping
//! - **Capability Tokens**: OCAP-based access control with attenuation
//! - **A2A Runtime**: Agent registration, A2A messaging, capability verification
//! - **Hexagonal Ports**: memory, consent, escalation, and registry boundaries
//!
//! # Example
//!
//! ```rust,ignore
//! use hkask_pods::pod::PodManager;
//! ```

pub mod a2a; // Loop 6 (Cybernetics: A2A is access control)
pub mod adapters;
pub mod consent; // Loop 6
pub mod curation; // Loop 5
pub mod curator_agent; // Loop 5
pub mod error;

pub mod inference_loop; // Loop 1 (domain logic; governance applied externally via GovernedTool in hkask-regulation)
pub mod inference_sensors; // Loop 1 sensors — Sensor implementations for InferenceLoop
pub mod loop_system;
pub mod pod; // Loop 5 (agent pod lifecycle is Curation)
pub mod ports;
pub mod sovereignty; // Loop 6 (sovereignty enforcement)
pub mod types;

// Re-export rich agent domain types from types/ (these are the canonical versions
// that extend the hkask-types foundation types with additional fields).
// NOTE: Agent types (AgentDefinition, Charter, PersonaConstraints, etc.)
// are defined canonically in hkask_types. Import from hkask_types.

pub use a2a::{A2AAgent, A2AError, A2AMessage, A2ARuntime};

pub use consent::{ConsentError, ConsentManager};
pub use curation::context::CuratorContext;
pub use curation::curation_loop::CurationLoop;
pub use curation::{CuratorSync, SemanticIndex};
pub use curator_agent::CuratorAgent;

pub use error::{CoreError, MemoryError};
pub use inference_loop::InferenceLoop;
pub use inference_sensors::{
    CircuitBreakerStateSensor, InferenceAvailableSensor, InferenceGasRemainingSensor,
    InferenceModelAvailableSensor, InferenceSensorState,
};
pub use loop_system::LoopScheduler;
pub use pod::{ActivePods, PodDeployment, PodFactory, PodID, PodKind, PodRegistry};
pub use sovereignty::{AllowAllConsent, DenyAllConsent, SovereigntyChecker, SovereigntyConsent};

// Agent types remain in hkask-types (canonical location for SQL impls).
pub use types::voice::VoiceDesign;
