//! hKask Adapter — trained adapter lifecycle & inference composition
//!
//! # Architecture
//!
//! ```text
//! AdapterRouter
//!   ├── AdapterStore      — CRUD for trained LoRA adapters
//!   ├── AdapterConfig     — PEFT adapter_config.json parser
//!   ├── Expertise         — semantic capability descriptor
//!   ├── EndpointLifecycle — state machine (5-phase, cost-tracked)
//!   ├── ProviderCost      — cost model per inference provider
//!   ├── AdapterPort       — trait boundary (6 OCAP-gated methods)
//!   ├── EndpointGuard     — RAII teardown on drop
//!   └── ProviderSelection — user-in-the-loop provider picker (P2 consent)
//! ```rust,no_run
//!
//! # Internal Seam Pattern
//!
//! `AdapterPort` is an **internal seam** — the trait and its single
//! implementation (`AdapterRouter`) live in the same crate because no
//! external consumer exists. Unlike `CircuitBreakerPort` (hkask-ports),
//! which serves `InferenceLoop`, `AdapterPort` has zero external callers.
//! Under ADR-042 (port promotion rule), a port only moves to a shared
//! crate when a second consumer materializes.
//!
//! # Design
//!
//! An `Expertise` (a named, provenance-tracked capability descriptor) links a
//! `TrainedLoRAAdapter` (content-addressed, owner-scoped artifact) to an
//! `InferenceEndpointHandle` (a provider-provisioned, lifecycle-governed, cost-tracked resource).
//! Every operation is OCAP-gated. Every state transition emits a Regulation span.
//! Every endpoint drains on session completion or budget exhaustion.

pub mod adapter_config;
pub mod adapter_port;
pub mod adapter_router;
pub mod adapter_store;
pub mod endpoint_lifecycle;
pub mod expertise;
pub mod provider_cost;

// Re-exports — public API
pub use adapter_config::AdapterConfig;
pub use adapter_port::{
    AdapterError, AdapterPort, InferenceEndpointHandle, ProviderSelection, SingleCandidate,
};
pub use adapter_router::{AdapterRouter, EndpointGuard};
pub use adapter_store::{AdapterSource, AdapterStore, AdapterStoreError, TrainedLoRAAdapter};
pub use endpoint_lifecycle::{EndpointLifecycle, EndpointPhase, EndpointPhaseError};
pub use expertise::{AdapterLifecycle, Expertise, MdsDomain, TrainingProvenance};
pub use provider_cost::{CostModel, CostModelError, ProviderCapability, ProviderInfo};
