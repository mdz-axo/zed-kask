#![deny(unsafe_code)]
//! hKask Agents — Agent Pod Lifecycle and A2A Integration
//!
//! This crate provides:
//! - **Agent Pod**: Runtime container for userpods and the curator
//! - **Lifecycle Management**: Active ↔ Sleeping
//! - **Capability Tokens**: OCAP-based access control with attenuation
//! - **A2A Runtime**: Agent registration, A2A messaging, capability verification
//! - **Hexagonal Ports**: memory, consent, escalation, and registry boundaries

pub mod a2a;
pub mod adapters;
pub mod consent;
pub mod curation;
pub mod error;
pub mod pod;
pub mod ports;
pub mod sovereignty;
pub mod types;

pub use a2a::{A2AAgent, A2AError, A2AMessage, A2ARuntime};
pub use consent::{ConsentError, ConsentManager};
pub use curation::context::CuratorContext;
pub use curation::curation_loop::CurationLoop;
pub use curation::{CuratorSync, SemanticIndex};
pub use error::{CoreError, MemoryError};
pub use pod::{ActivePods, PodDeployment, PodFactory, PodID, PodKind, PodRegistry};
pub use sovereignty::{AllowAllConsent, DenyAllConsent, SovereigntyChecker, SovereigntyConsent};
pub use types::voice::VoiceDesign;
