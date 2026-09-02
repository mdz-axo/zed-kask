//! hKask Adapter — trained adapter metadata & lifecycle types
//!
//! # Architecture
//!
//! ```text
//! AdapterStore — CRUD for trained LoRA adapters
//! Expertise    — semantic capability descriptor
//! ```
//!
//! # Design
//!
//! An `Expertise` (a named, provenance-tracked capability descriptor) links a
//! `TrainedLoRAAdapter` (content-addressed, owner-scoped artifact) to an
//! `InferenceEndpointHandle` (a provider-provisioned, lifecycle-governed, cost-tracked resource).
//!
//! Tool dispatch enforcement happens at the `McpRuntime` governance layer
//! (`McpRuntime::with_governance` in `crates/zed/src/main.rs`), not at the
//! adapter layer. The previous `AdapterPort` trait + `AdapterRouter` impl
//! was removed — it was dead code with zero production callers. The training
//! tools use `AdapterStore` (CRUD) and `InferencePort` (inference) directly.

pub(crate) mod adapter_store;
pub(crate) mod expertise;

// Re-exports — public API
pub(crate) use adapter_store::{
    AdapterSource, AdapterStore, AdapterStoreError, TrainedLoRAAdapter,
};
