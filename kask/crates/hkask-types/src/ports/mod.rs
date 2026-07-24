//! Hexagonal port traits — infrastructure abstractions.
//!
//! Moved from the hkask-ports crate during the types consolidation.
//! Port traits enable crates to depend on abstractions rather than
//! concrete implementations.

pub mod consent_port;
pub mod embedding;
pub mod embedding_port;
pub mod escalation;
pub mod flowdef_validation;
pub mod git_cas;
pub mod inference_port;
pub mod inference_types;
pub mod pipeline_manifest;
pub mod pipeline_runner;
pub mod pipeline_state;
pub mod registry;
pub mod regulation;
pub mod secrets_port;
pub mod wallet_budget_port;

pub use embedding::EmbeddingGenerationError;
pub use flowdef_validation::{
    FlowDefValidationFinding, FlowDefValidationReport, validate_convergence_field,
    validate_step_input_mapping,
};
pub use inference_port::{InferencePort, InferenceStreamChunk};
pub use inference_types::{
    ChatMessage, ChatToolDefinition, ChatToolFunction, InferenceError, InferenceResult,
    InferenceUsage, StructuredToolCall, TokenProb, TokenProbability, compute_confidence,
};
pub use registry::{
    RegistryEntry, RegistryError, RegistryIndex, Skill, SkillRegistryIndex, SkillZone,
};
pub use regulation::{
    BackpressureSignal, CircuitBreakerPort, ConsolidationOutcome, ConsolidationRequest,
    DecayConfig, DepletionSignal, LedgerObserver, LedgerStoragePort, WeightedEvent,
};
pub use secrets_port::{SecretsError, SecretsFuture, SecretsPort};
pub use wallet_budget_port::{WalletBudgetError, WalletBudgetPort};
