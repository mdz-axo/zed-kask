//! Hexagonal port traits — infrastructure abstractions.
//!
//! Moved from the hkask-ports crate during the types consolidation.
//! Port traits enable crates to depend on abstractions rather than
//! concrete implementations.

pub mod consent_port;
pub mod embedding;
pub mod embedding_port;
pub mod escalation;
pub mod inference_port;
pub mod inference_types;
pub mod memory_port;
pub mod registry;
pub mod regulation;
pub mod wallet_budget_port;

pub use embedding::EmbeddingGenerationError;
pub use inference_port::{InferencePort, InferenceStreamChunk, ModelEntry};
pub use inference_types::{
    ChatMessage, ChatToolDefinition, ChatToolFunction, InferenceError, InferenceResult,
    InferenceUsage, StructuredToolCall, TokenProb, TokenProbability, compute_confidence,
};
pub use memory_port::{MemoryError, MemoryFuture, MemoryPort, MemorySnippet, TurnRecord};
pub use registry::{
    RegistryEntry, RegistryError, RegistryIndex, Skill, SkillRegistryIndex, SkillZone,
};
pub use regulation::{
    BackpressureSignal, CircuitBreakerPort, ConsolidationOutcome, ConsolidationRequest,
    DecayConfig, DepletionSignal, LedgerObserver, LedgerStoragePort, WeightedEvent,
};
pub use wallet_budget_port::{WalletBudgetError, WalletBudgetPort};
