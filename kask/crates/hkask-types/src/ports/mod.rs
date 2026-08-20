//! Hexagonal port traits — infrastructure abstractions.
//!
//! Moved from the hkask-ports crate during the types consolidation.
//! Port traits enable crates to depend on abstractions rather than
//! concrete implementations.

pub mod cascade_context;
pub mod embedding;
pub mod inference_port;
pub mod inference_types;
pub mod memory_port;
pub mod registry;
pub mod regulation;

pub use cascade_context::{
    CascadeContext, CascadeContextError, CascadeContextProvider, CascadeContextRequest,
};
pub use embedding::EmbeddingGenerationError;
pub use inference_port::{
    EmbedFuture, InferencePort, ModelEntry,
    SkillExecError, SkillExecPort, ToolDispatchPort, WorktreeSpawnPort,
};
pub use inference_types::{
    ChatMessage, ChatToolDefinition, ChatToolFunction, InferenceError, InferenceResult, InferenceStreamChunk,
    InferenceUsage, StructuredToolCall, TokenProb, TokenProbability, compute_confidence,
};
pub use memory_port::{MemoryError, MemoryFuture, MemoryPort, MemorySnippet, TurnRecord};
pub use registry::{
    RegistryEntry, RegistryError, RegistryIndex, Skill, SkillRegistryIndex, SkillZone,
};
pub use regulation::{ConsolidationOutcome, ConsolidationRequest};
