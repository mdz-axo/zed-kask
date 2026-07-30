//! hKask Adapter — trained adapter metadata & lifecycle types
//!
//! # Architecture
//!
//! ```text
//! AdapterStore      — CRUD for trained LoRA adapters
//! AdapterConfig     — PEFT adapter_config.json parser
//! Expertise         — semantic capability descriptor
//! EndpointLifecycle — state machine (5-phase, cost-tracked)
//! ProviderCost      — cost model per inference provider
//! ```
//!
//! # Design
//!
//! An `Expertise` (a named, provenance-tracked capability descriptor) links a
//! `TrainedLoRAAdapter` (content-addressed, owner-scoped artifact) to an
//! `InferenceEndpointHandle` (a provider-provisioned, lifecycle-governed, cost-tracked resource).
//!
//! OCAP enforcement happens at the `McpRuntime` governance layer
//! (`McpRuntime::with_governance` in `crates/zed/src/main.rs`), not at the
//! adapter layer. The previous `AdapterPort` trait + `AdapterRouter` impl
//! (which advertised OCAP via `_token` parameters but never verified them)
//! was removed — it was dead code with zero production callers. The training
//! tools use `AdapterStore` (CRUD) and `InferencePort` (inference) directly.

pub mod adapter_config;
pub mod adapter_store;
pub mod endpoint_lifecycle;
pub mod expertise;
pub mod provider_cost;

// Re-exports — public API
pub use adapter_config::AdapterConfig;
pub use adapter_store::{AdapterSource, AdapterStore, AdapterStoreError, TrainedLoRAAdapter};
pub use endpoint_lifecycle::{EndpointLifecycle, EndpointPhase, EndpointPhaseError};
pub use expertise::{AdapterLifecycle, Expertise, MdsDomain, TrainingProvenance};
pub use provider_cost::{CostModel, CostModelError, ProviderCapability, ProviderInfo};
