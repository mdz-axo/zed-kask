#![deny(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Storage — SQLite + SQLCipher storage backend
//!
//! Consolidated from hkask-storage + hkask-database + hkask-storage-core.
//! Database driver abstraction and storage core foundation are now modules
//! within this crate. Domain-specific storage modules follow.

pub mod core;
pub mod database;

pub use core::DatabaseDriverTrait;
pub use core::database::{Database, DatabaseError};
pub use core::{
    DEFAULT_EMBEDDING_DIM, check_passphrase, embedding_dim, open_database, open_or_repair,
    sanitize_path,
};
pub use database::{DatabaseDriver, SqliteDriver, WAL_PRAGMA_BATCH, init_wal_pragmas};
pub use hkask_types::time::now_rfc3339;

pub mod embeddings;
pub mod escalation;
pub mod hmem;
pub mod regulation_store;

pub use embeddings::{EmbeddingError, EmbeddingStore, SimilarityResult, StoredEmbedding};
pub use escalation::{
    EscalationBatch, EscalationEntry, EscalationError, EscalationQueue, EscalationStats,
    EscalationStatus,
};
pub use hkask_types::HMemId;
pub use hmem::archive::{ArchiveError, BackupArchive, BackupMeta, MigrationReceipt};
pub use hmem::{HMem, HMemError, HMemStore};
pub use regulation_store::{DecayConfig, RegulationArchive, WeightedEvent};
