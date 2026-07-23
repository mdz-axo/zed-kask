//! kask_bridge — the sole bidirectional seam between hKask and zed-kask (D8).
//!
//! Implements every port trait (defined in hkask-types / hkask-capability) over
//! a zed-kask facility:
//!
//! | Port              | Over (zed-kask)          |
//! |-------------------|--------------------------|
//! | InferencePort     | LanguageModel (stream)  |
//! | ToolPort          | in-process tool registry|
//! | SecretsPort       | CredentialsProvider      |
//! | CuratorTurnPort   | native-agent turn       |
//! | MemoryPort        | in-process memory handles|
//!
//! Governing invariant (§13.1): hKask crates never depend on zed-kask crates;
//! this crate is the ONLY one that depends on both. Enforced by
//! kask/scripts/check-hkask-no-zed-deps.sh.
//!
//! TODO (T1.4): gpui_tokio wiring + InferencePort-over-LanguageModel adapter.
//! TODO (T2.0): ToolPort-over-tool-registry adapter.
//! TODO (T1.6): SecretsPort-over-CredentialsProvider adapter.
//! TODO (D2):  CuratorTurnPort adapter.
//! TODO (D6):  MemoryPort adapter (EmbeddingPort via pure-Rust cosine similarity
//!             over StorageDriver; HMemStorePort via SQL over StorageDriver).

// Stub — modules will be added as the adapters are implemented.
