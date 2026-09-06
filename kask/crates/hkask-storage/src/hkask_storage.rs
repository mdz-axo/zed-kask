#![deny(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Storage — SQLite + SQLCipher storage backend
//!
//! Consolidated from hkask-storage + hkask-database + hkask-storage-core.
//! Database driver abstraction and storage core foundation are now modules
//! within this crate. Domain-specific storage modules follow.

pub(crate) mod core;
pub mod database;
pub mod gallery;
pub mod rotation;

pub use core::DatabaseDriverTrait;
pub use core::connection::{
    Database, DatabaseError, LeasedSqliteConnection, SqliteConnectionManager,
};
pub use core::{embedding_dim, open_database, open_or_repair, sanitize_path};
pub use database::{DatabaseDriver, SqliteDriver, WAL_PRAGMA_BATCH, init_wal_pragmas};
pub use hkask_types::time::now_rfc3339;
pub use rotation::{RotationError, rotate_passphrase};

pub(crate) mod embeddings;
pub(crate) mod escalation;
pub(crate) mod hmem;
pub(crate) mod regulation_store;

pub use embeddings::{EmbeddingError, EmbeddingStore, SimilarityResult};
pub use escalation::{EscalationEntry, EscalationError, EscalationQueue, EscalationStatus};
pub use hkask_types::HMemId;
pub use hmem::{HMem, HMemError, HMemStore};
pub use regulation_store::{DecayConfig, RegulationArchive};

pub use gallery::{
    FaceRegistryRecord, GalleryMode, GalleryRecord, GalleryStore, GalleryStoreError, ImageRecord,
    TagRecord,
};
